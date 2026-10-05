//! A picture made small enough to send.
//!
//! Nobody else in the chain does it. An agent hands a picture on to its
//! model as it was given -- Claude's adapter puts the bytes straight into
//! the request -- and the model's API refuses one past its limits, so a
//! photograph dragged in from a camera is a turn that fails. What Obelus
//! does is what Claude Code does with a picture before it sends one, and
//! with its numbers: no side over 2000 pixels, and no more than 5 MiB once
//! it is base64.
//!
//! **Almost every picture goes as it came.** A screenshot is inside both
//! limits, and one that is is not decoded at all: its size is read from its
//! header and its bytes are sent untouched. Only past a limit is it decoded
//! -- shrunk to fit if it is too large on a side, then written as a JPEG at
//! falling quality until it fits. Claude Code tries a PNG with fewer colours
//! before the JPEG; `image` cannot make one, and the JPEG is what that ends
//! in anyway for a photograph, which is what is this large.
//!
//! Here, at the moment it is sent, rather than when it is put in the box:
//! the box keeps what the reader gave it, and the work is done once, on a
//! thread of its own.
//!
//! **A picture decoded is turned the way its camera said.** A phone writes
//! a portrait photograph sideways and says so in the EXIF, which a picture
//! written again does not carry -- so one past a limit, which is most
//! photographs, would reach the model on its side.

use std::io::Cursor;

use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, imageops::FilterType};

/// The longest a side may be.
const SIDE: u32 = 2000;

/// The most a picture may be, in bytes: what is 5 MiB once it is base64.
const BYTES: usize = 5 * 1024 * 1024 / 4 * 3;

/// The picture, as it was if it fits and made to fit if it does not.
///
/// What cannot be read is sent as it was: the agent is better placed to say
/// what is wrong with a picture than a guess here is to mend it.
pub(crate) fn fitted(mime: String, bytes: Vec<u8>) -> (String, Vec<u8>) {
    let Some((width, height)) = size_of(&bytes) else {
        return (mime, bytes);
    };
    if bytes.len() <= BYTES && width <= SIDE && height <= SIDE {
        return (mime, bytes);
    }
    let picture = match upright(&bytes) {
        Ok(picture) => picture,
        Err(error) => {
            tracing::warn!(%error, mime, "a picture too large to send could not be read");
            return (mime, bytes);
        }
    };
    let too_wide = width > SIDE || height > SIDE;
    // `resize` keeps the proportions, and fits inside both.
    let picture = match too_wide {
        true => picture.resize(SIDE, SIDE, FilterType::Lanczos3),
        false => picture,
    };
    tracing::info!(
        from = ?(width, height),
        to = ?(picture.width(), picture.height()),
        bytes = bytes.len(),
        "a picture too large to send is being made smaller"
    );
    // Only a picture that was too wide has been changed by now; one that
    // was only too heavy is the same picture, and writing it again as what
    // it was would make it no lighter.
    if too_wide
        && mime == "image/png"
        && let Some(png) = written_as_png(&picture)
        && png.len() <= BYTES
    {
        return ("image/png".to_string(), png);
    }
    for quality in [80, 60, 40, 20] {
        if let Some(jpeg) = written_as_jpeg(&picture, quality)
            && jpeg.len() <= BYTES
        {
            return ("image/jpeg".to_string(), jpeg);
        }
    }
    // What Claude Code ends with: narrower, and the lowest quality.
    let narrower = picture.resize(picture.width().min(1000), u32::MAX, FilterType::Lanczos3);
    match written_as_jpeg(&narrower, 20) {
        Some(jpeg) => ("image/jpeg".to_string(), jpeg),
        None => (mime, bytes),
    }
}

/// The picture decoded, and turned the way its EXIF says it was taken.
fn upright(bytes: &[u8]) -> image::ImageResult<DynamicImage> {
    let mut decoder = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()?
        .into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut picture = DynamicImage::from_decoder(decoder)?;
    picture.apply_orientation(orientation);
    Ok(picture)
}

/// How large a picture is, from its header alone.
fn size_of(bytes: &[u8]) -> Option<(u32, u32)> {
    ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
}

fn written_as_png(picture: &DynamicImage) -> Option<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    picture.write_to(&mut out, ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// As a JPEG, which has no transparency: what was transparent is dropped
/// rather than laid on a colour, as Claude Code's does.
fn written_as_jpeg(picture: &DynamicImage, quality: u8) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .encode_image(&DynamicImage::ImageRgb8(picture.to_rgb8()))
        .ok()?;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32, noisy: bool) -> Vec<u8> {
        // A cheap generator, so a noisy picture is one PNG cannot shrink.
        let mut seed: u32 = 1;
        let picture = image::RgbImage::from_fn(width, height, |x, y| {
            if !noisy {
                return image::Rgb([(x % 256) as u8, (y % 256) as u8, 128]);
            }
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let [a, b, c, _] = seed.to_le_bytes();
            image::Rgb([a, b, c])
        });
        written_as_png(&DynamicImage::ImageRgb8(picture)).expect("a png")
    }

    /// A picture inside both limits is sent byte for byte as it came.
    ///
    /// Deliberate break: dropping the early return writes every picture
    /// out again, and the screenshot comes back as a JPEG.
    #[test]
    fn a_picture_that_fits_goes_as_it_came() {
        let bytes = png(300, 200, false);
        let (mime, sent) = fitted("image/png".to_string(), bytes.clone());
        assert_eq!(mime, "image/png");
        assert_eq!(sent, bytes, "a picture that fitted was written again");
    }

    /// One too wide is made narrow enough, in proportion, and stays a PNG
    /// where a PNG of it fits.
    ///
    /// Deliberate break: not resizing leaves it 2400 wide.
    #[test]
    fn a_picture_too_wide_is_made_to_fit() {
        let (mime, sent) = fitted("image/png".to_string(), png(2400, 600, false));
        assert_eq!(mime, "image/png");
        assert_eq!(size_of(&sent), Some((2000, 500)));
    }

    /// A photograph its camera said is on its side is sent upright.
    ///
    /// Deliberate break: decoding without asking the orientation sends it
    /// 2000 wide and 500 tall, the way the sensor wrote it.
    #[test]
    fn a_photograph_on_its_side_is_sent_upright() {
        use image::ImageEncoder as _;
        // EXIF as a TIFF, big-endian: one entry, orientation (0x0112), a
        // short, six -- turned a quarter clockwise to be seen upright.
        let exif = b"MM\x00\x2a\x00\x00\x00\x08\x00\x01\x01\x12\x00\x03\x00\x00\x00\x01\x00\x06\x00\x00\x00\x00\x00\x00".to_vec();
        let sideways = image::RgbImage::from_pixel(2400, 600, image::Rgb([200, 100, 50]));
        let mut bytes = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 90);
        encoder.set_exif_metadata(exif).expect("exif in a jpeg");
        encoder
            .write_image(&sideways, 2400, 600, image::ExtendedColorType::Rgb8)
            .expect("a jpeg");

        let (_, sent) = fitted("image/jpeg".to_string(), bytes);
        assert_eq!(size_of(&sent), Some((500, 2000)));
    }

    /// One too heavy is made light enough, and is a JPEG.
    ///
    /// Deliberate break: skipping the qualities sends the narrow last
    /// resort, which is 1000 wide rather than the 1200 it was.
    #[test]
    fn a_picture_too_heavy_is_made_lighter() {
        let bytes = png(1200, 1200, true);
        assert!(
            bytes.len() > BYTES,
            "the picture is not heavy enough to test"
        );
        let (mime, sent) = fitted("image/png".to_string(), bytes);
        assert_eq!(mime, "image/jpeg");
        assert!(sent.len() <= BYTES, "still {} bytes", sent.len());
        assert_eq!(size_of(&sent), Some((1200, 1200)));
    }
}
