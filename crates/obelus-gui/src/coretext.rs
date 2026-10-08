//! The glyphs of the faces only CoreText can draw, drawn by CoreText.
//!
//! Every glyph is drawn by swash, which reads outlines from `glyf`, `CFF `
//! and `CFF2` and pictures from the tables beside them. `PingFang SC` has
//! none of those: its outlines are in `hvgl`, a table of Apple's that only
//! CoreText reads -- and it is the face macOS draws Chinese in, the one a
//! reader on a Mac sees everywhere else. So a face swash cannot draw and
//! CoreText can is kept, and its glyphs are drawn here instead (see
//! [`crate::faces`]).
//!
//! Only the drawing. What the face is called, what is in its character map
//! and how far each glyph advances are ordinary tables, and they are read
//! the way every other face's are, which is why a glyph number cosmic-text
//! shaped is the number CoreText is asked for: it is the same file.
//!
//! In one shade, the way swash draws a letter: the coverage, with the colour
//! coming from the cell. Without font smoothing, which is CoreText thickening
//! the strokes of light text on a dark page -- a letter from here beside one
//! from swash would be the heavier of the two.

use cosmic_text::{CacheKey, SwashImage};

/// Whether CoreText draws the face by this PostScript name.
///
/// Asked by making the font and seeing whether it is that face: CoreText
/// answers a name it does not know with some other face rather than with
/// nothing.
#[cfg(target_os = "macos")]
pub(crate) fn draws(name: &str) -> bool {
    use objc2_core_foundation::CFString;
    use objc2_core_text::CTFont;

    // SAFETY: a size of nothing is the face's own size, and no matrix is the
    // identity.
    let font = unsafe { CTFont::with_name(&CFString::from_str(name), 0.0, std::ptr::null()) };
    // SAFETY: a font CoreText made always has a PostScript name.
    unsafe { font.post_script_name() }.to_string() == name
}

/// Nowhere else is there a CoreText to draw with.
#[cfg(not(target_os = "macos"))]
pub(crate) const fn draws(_: &str) -> bool {
    false
}

/// One glyph of the face by this PostScript name, at the size and the
/// offset the key asks for -- or nothing, where it has no pixels, which is
/// a space.
#[cfg(target_os = "macos")]
pub(crate) fn draw(name: &str, key: CacheKey) -> Option<SwashImage> {
    use std::ptr::NonNull;

    use cosmic_text::{Placement, SwashContent};
    use objc2_core_foundation::{CFString, CGFloat, CGPoint, CGRect};
    use objc2_core_graphics::{
        CGBitmapContextCreate, CGColorSpace, CGContext, CGGlyph, CGImageAlphaInfo,
    };
    use objc2_core_text::{CTFont, CTFontOrientation};

    let size = CGFloat::from(f32::from_bits(key.font_size_bits));
    // SAFETY: as in `draws`.
    let font = unsafe { CTFont::with_name(&CFString::from_str(name), size, std::ptr::null()) };
    let glyph: CGGlyph = key.glyph_id;
    let mut bounds = CGRect::default();
    // SAFETY: one glyph, and room for its one rectangle.
    unsafe {
        font.bounding_rects_for_glyphs(
            CTFontOrientation::Default,
            NonNull::from(&glyph),
            &raw mut bounds,
            1,
        );
    };
    if bounds.size.width <= 0.0 || bounds.size.height <= 0.0 {
        return None;
    }
    // Where the pen is inside its pixel, which is the part of the position
    // the grid could not put it at.
    let offset = CGFloat::from(key.x_bin.as_float());
    // A pixel of room each way, for what antialiasing spills past the
    // outline's own box.
    let left = (bounds.origin.x + offset).floor() - 1.0;
    let right = (bounds.origin.x + bounds.size.width + offset).ceil() + 1.0;
    let bottom = bounds.origin.y.floor() - 1.0;
    let top = (bounds.origin.y + bounds.size.height).ceil() + 1.0;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a glyph is a few dozen whole pixels across"
    )]
    let (width, height) = ((right - left) as usize, (top - bottom) as usize);
    let mut data = vec![0_u8; width * height];
    let gray = CGColorSpace::new_device_gray();
    // SAFETY: `data` is a row of `width` bytes for each of `height` rows, and
    // outlives the context, which is dropped before it is read.
    let context = unsafe {
        CGBitmapContextCreate(
            data.as_mut_ptr().cast(),
            width,
            height,
            8,
            width,
            gray.as_deref(),
            CGImageAlphaInfo::None.0,
        )
    }?;
    CGContext::set_gray_fill_color(Some(&context), 1.0, 1.0);
    CGContext::set_should_antialias(Some(&context), true);
    CGContext::set_allows_font_smoothing(Some(&context), false);
    CGContext::set_should_smooth_fonts(Some(&context), false);
    // CoreGraphics counts up from the bottom of the bitmap; its first row in
    // memory is the top, which is the row swash puts first too.
    let at = CGPoint {
        x: offset - left,
        y: -bottom,
    };
    // SAFETY: one glyph at one position, into a context that is alive.
    unsafe { font.draw_glyphs(NonNull::from(&glyph), NonNull::from(&at), 1, &context) };
    drop(context);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a glyph is a few dozen whole pixels from the pen"
    )]
    let placement = Placement {
        left: left as i32,
        top: top as i32,
        width: u32::try_from(width).ok()?,
        height: u32::try_from(height).ok()?,
    };
    let mut image = SwashImage::new();
    image.content = SwashContent::Mask;
    image.placement = placement;
    image.data = data;
    Some(image)
}

/// Nor anything to draw.
#[cfg(not(target_os = "macos"))]
pub(crate) const fn draw(_: &str, _: CacheKey) -> Option<SwashImage> {
    None
}
