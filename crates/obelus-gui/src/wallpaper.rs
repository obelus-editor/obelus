//! The picture under the page: reading it, and where it lands.
//!
//! The application says which file and how it is fitted; what that comes
//! to on a window of some size is worked out here, because the window is
//! the one that knows its size and a terminal has no pixels to put it in.
//!
//! **A picture is decoded on a thread of its own and only the newest is
//! drawn.** A photograph takes tens of milliseconds to decode, which is not
//! work for the thread a key is waiting on nor for the one that draws --
//! and a reader walking the list of themes asks for one picture after
//! another faster than they decode. So each is numbered when it is asked
//! for, a picture whose number has been overtaken is not decoded or not
//! sent, and the window keeps the highest number it has been given: two
//! threads finishing in the wrong order cannot put the older picture back.
//!
//! **A picture is never drawn larger than it has to be.** One fitted to the
//! window is made no larger than it takes to cover the screen the window
//! opened on, before it is handed to the card: an eight-thousand-pixel
//! photograph kept whole is a quarter of a gigabyte of somebody's graphics
//! memory for pixels nobody can see, and several Obelus windows open at
//! once is the normal case. One drawn at its own size is left alone,
//! because its own size is what it is drawn at.

use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use obelus_theme::wallpaper::{Anchor, Edge, Fit, Wallpaper};

/// A picture decoded, with what the theme said about laying it.
pub(crate) struct Hung {
    /// The pixels, as the file had them: no colour conversion, for the same
    /// reason a theme's colours go through untouched.
    pub(crate) pixels: image::RgbaImage,
    /// How much of the page covers it, out of a hundred.
    pub(crate) opacity: u8,
    /// How it is fitted to the window.
    pub(crate) fit: Fit,
    /// What stays where the window's edges move.
    pub(crate) anchor: Anchor,
}

impl std::fmt::Debug for Hung {
    // Not the pixels, which are millions of bytes in a log line.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Hung")
            .field("size", &self.pixels.dimensions())
            .field("opacity", &self.opacity)
            .field("fit", &self.fit)
            .field("anchor", &self.anchor)
            .finish()
    }
}

/// The largest side a picture may have on its way to the card.
///
/// What every desktop card has made since long before Obelus, where the
/// floor wgpu promises is a quarter of it: a picture this large has already
/// been made as small as the screen allows, and one still larger than this
/// is one drawn at its own size, which no screen shows whole anyway.
pub(crate) const LARGEST: u32 = 8192;

/// Numbers each picture asked for, so the window can tell the newest.
#[derive(Debug, Default)]
pub(crate) struct Asked(AtomicU64);

impl Asked {
    /// A new number, higher than every one before it.
    pub(crate) fn next(&self) -> u64 {
        self.0.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Whether `number` is still the newest, which is whether its picture
    /// is still worth decoding.
    pub(crate) fn newest(&self, number: u64) -> bool {
        self.0.load(Ordering::Relaxed) == number
    }
}

/// Decodes `wallpaper` off the thread that asked, and hands it to `send`
/// numbered `number` -- unless something newer was asked for meanwhile.
pub(crate) fn decode_apart(
    wallpaper: Wallpaper,
    screen: [u32; 2],
    asked: Arc<Asked>,
    number: u64,
    send: impl FnOnce(Option<Arc<Hung>>) + Send + 'static,
) {
    let started = std::thread::Builder::new()
        .name("obelus wallpaper".to_string())
        .spawn(move || {
            if !asked.newest(number) {
                return;
            }
            let hung = decoded(&wallpaper, screen);
            if asked.newest(number) {
                send(hung.map(Arc::new));
            }
        });
    if let Err(error) = started {
        tracing::warn!(%error, "no thread to read the wallpaper on, so there is none");
    }
}

/// The picture a theme names, made no larger than it has to be.
///
/// Nothing where it will not decode, with a word in the log: the theme's
/// file named a picture that was there when it was read -- that much is
/// marked on the file -- so what is left to go wrong is the picture itself.
fn decoded(wallpaper: &Wallpaper, screen: [u32; 2]) -> Option<Hung> {
    let pixels = match read(&wallpaper.file) {
        Ok(pixels) => pixels,
        Err(error) => {
            tracing::warn!(%error, file = %wallpaper.file.display(), "a wallpaper that would not decode");
            return None;
        }
    };
    let pixels = smaller(pixels, wallpaper.fit, screen);
    tracing::debug!(
        file = %wallpaper.file.display(),
        size = ?pixels.dimensions(),
        "a wallpaper read"
    );
    Some(Hung {
        pixels,
        opacity: wallpaper.opacity,
        fit: wallpaper.fit,
        anchor: wallpaper.anchor,
    })
}

fn read(file: &Path) -> image::ImageResult<image::RgbaImage> {
    Ok(image::ImageReader::open(file)?
        .with_guessed_format()?
        .decode()?
        .into_rgba8())
}

/// The picture at the size it will be drawn at, or smaller.
fn smaller(pixels: image::RgbaImage, fit: Fit, screen: [u32; 2]) -> image::RgbaImage {
    let (width, height) = pixels.dimensions();
    let [wanted_width, wanted_height] = wanted([width, height], fit, screen);
    if (wanted_width, wanted_height) == (width, height) {
        return pixels;
    }
    image::imageops::resize(
        &pixels,
        wanted_width,
        wanted_height,
        image::imageops::FilterType::Triangle,
    )
}

/// How large a picture of `size` needs to be, fitted `fit` on `screen`.
///
/// Its own size where it is drawn at that, and otherwise as large as the
/// fit makes it on the screen -- never larger than it is, because making a
/// picture larger adds pixels and no detail. Kept in proportion even for a
/// stretch, which squashes it the other way on a window of another shape.
/// And never past [`LARGEST`] on either side, whatever the fit.
fn wanted(size: [u32; 2], fit: Fit, screen: [u32; 2]) -> [u32; 2] {
    let [width, height] = size.map(|side| side.max(1) as f64);
    let [across, down] = screen.map(|side| side.max(1) as f64);
    let fitted = match fit {
        Fit::Center | Fit::Tile => 1.0,
        Fit::Cover | Fit::Stretch => (across / width).max(down / height),
        Fit::Contain => (across / width).min(down / height),
    };
    let largest = f64::from(LARGEST);
    let scale = fitted.min(1.0).min(largest / width).min(largest / height);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a scale of at most one on a side that was a u32"
    )]
    [
        ((width * scale).round() as u32).max(1),
        ((height * scale).round() as u32).max(1),
    ]
}

/// Where a picture is drawn, and which part of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Laid {
    /// Left, top, width, height, in the window's pixels.
    pub(crate) rect: [f32; 4],
    /// The part of the picture across that rectangle: left, top, right,
    /// bottom, where one is the picture's far edge. Past one for a tiling,
    /// which the sampler repeats.
    pub(crate) uv: [f32; 4],
}

impl Laid {
    /// The part of this that falls inside `rect`, with the part of the
    /// picture that belongs there -- or nothing, where the picture does not
    /// reach it.
    ///
    /// What paints the page back over one cell: the same picture at the
    /// same place, so the cell is not a seam in it.
    pub(crate) fn piece(&self, rect: [f32; 4]) -> Option<Self> {
        let [x, y, width, height] = self.rect;
        let left = rect[0].max(x);
        let top = rect[1].max(y);
        let right = (rect[0] + rect[2]).min(x + width);
        let bottom = (rect[1] + rect[3]).min(y + height);
        if right <= left || bottom <= top || width <= 0.0 || height <= 0.0 {
            return None;
        }
        let [u0, v0, u1, v1] = self.uv;
        let across = |at: f32| u0 + (at - x) / width * (u1 - u0);
        let down = |at: f32| v0 + (at - y) / height * (v1 - v0);
        Some(Self {
            rect: [left, top, right - left, bottom - top],
            uv: [across(left), down(top), across(right), down(bottom)],
        })
    }
}

/// Where a picture of `size` pixels lands in a window of `window` pixels.
///
/// `scale` is how many of the window's pixels a point is, which is what a
/// picture drawn at its own size is measured in: a picture is a picture of
/// something, and the same one on a denser screen is the same size to the
/// eye rather than half of it. It was decoded at its own size and is still
/// at it -- see `wanted` -- so its pixels here are its points.
///
/// Off the window's edges where it is larger than the window, which is
/// what is cut off: the window draws only what is in it.
pub(crate) fn laid(size: [u32; 2], fit: Fit, anchor: Anchor, window: [f32; 2], scale: f32) -> Laid {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a picture is thousands of pixels"
    )]
    let [width, height] = size.map(|side| side.max(1) as f32);
    let [across, down] = window;
    let whole = [0.0, 0.0, 1.0, 1.0];
    // How large it is drawn, in the window's pixels.
    let drawn = match fit {
        Fit::Stretch => {
            return Laid {
                rect: [0.0, 0.0, across, down],
                uv: whole,
            };
        }
        Fit::Cover => {
            let fitted = (across / width).max(down / height);
            [width * fitted, height * fitted]
        }
        Fit::Contain => {
            let fitted = (across / width).min(down / height);
            [width * fitted, height * fitted]
        }
        Fit::Center | Fit::Tile => [width * scale, height * scale],
    };
    // Where its anchor puts it: the same point of the picture as of the
    // window, which is the left or top edge, the middle, or the far edge.
    let left = (across - drawn[0]) * part(anchor.across);
    let top = (down - drawn[1]) * part(anchor.down);
    if fit != Fit::Tile {
        return Laid {
            rect: [left, top, drawn[0], drawn[1]],
            uv: whole,
        };
    }
    // A tiling is the whole window, with the picture placed as it would be
    // once and repeated out from there: so one copy sits exactly where its
    // anchor says, and the window growing moves no copy that was in it.
    Laid {
        rect: [0.0, 0.0, across, down],
        uv: [
            -left / drawn[0],
            -top / drawn[1],
            (across - left) / drawn[0],
            (down - top) / drawn[1],
        ],
    }
}

/// How far along a side an edge is, from nothing to the whole of it.
const fn part(edge: Edge) -> f32 {
    match edge {
        Edge::Start => 0.0,
        Edge::Middle => 0.5,
        Edge::End => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use obelus_theme::wallpaper::{Anchor, Edge, Fit};

    use super::{LARGEST, laid, wanted};

    const MIDDLE: Anchor = Anchor {
        across: Edge::Middle,
        down: Edge::Middle,
    };

    /// A cover fills the window and cuts off the overflow evenly, a
    /// contain fits inside it with the page round it, a stretch is the
    /// window -- and the anchor decides which side is cut or where the
    /// room is left.
    ///
    /// Deliberate breaks: `max` for `min` in the cover (it then leaves the
    /// page showing above and below), and the anchor's part read from
    /// `down` for `across` (the left-anchored cover is then centred).
    #[test]
    fn a_fit_lays_a_picture_where_it_says() {
        // A 200x100 picture in a 100x100 window.
        let cover = laid([200, 100], Fit::Cover, MIDDLE, [100.0, 100.0], 1.0);
        assert_eq!(cover.rect, [-50.0, 0.0, 200.0, 100.0], "covered");
        let contain = laid([200, 100], Fit::Contain, MIDDLE, [100.0, 100.0], 1.0);
        assert_eq!(contain.rect, [0.0, 25.0, 100.0, 50.0], "contained");
        let stretch = laid([200, 100], Fit::Stretch, MIDDLE, [100.0, 100.0], 1.0);
        assert_eq!(stretch.rect, [0.0, 0.0, 100.0, 100.0], "stretched");

        let left = Anchor {
            across: Edge::Start,
            down: Edge::Middle,
        };
        let held = laid([200, 100], Fit::Cover, left, [100.0, 100.0], 1.0);
        assert_eq!(held.rect, [0.0, 0.0, 200.0, 100.0], "the left was cut");
    }

    /// A picture drawn at its own size is measured in points, so it is the
    /// same size to the eye on a denser screen.
    ///
    /// Deliberate break: leaving `scale` out of `Fit::Center`, which draws
    /// it at 40 pixels on a screen of two to a point.
    #[test]
    fn a_picture_at_its_own_size_is_measured_in_points() {
        let centre = laid([40, 20], Fit::Center, MIDDLE, [200.0, 100.0], 2.0);
        assert_eq!(centre.rect, [60.0, 30.0, 80.0, 40.0]);
    }

    /// A tiling covers the window, with one copy exactly where its anchor
    /// puts it -- so the window growing to the right moves no copy that
    /// was already on it, when the anchor is the left.
    ///
    /// Deliberate break: the tiling's `uv` starting at nothing whatever
    /// the anchor, which puts no copy in the middle of a centred one.
    #[test]
    fn a_tiling_repeats_out_from_where_its_anchor_puts_one() {
        let centred = laid([10, 10], Fit::Tile, MIDDLE, [100.0, 40.0], 1.0);
        assert_eq!(centred.rect, [0.0, 0.0, 100.0, 40.0]);
        // One copy from 45 to 55 across and 15 to 25 down: the window's
        // left edge is four and a half copies before it.
        assert_eq!(centred.uv, [-4.5, -1.5, 5.5, 2.5]);

        let corner = Anchor {
            across: Edge::Start,
            down: Edge::Start,
        };
        let narrow = laid([10, 10], Fit::Tile, corner, [100.0, 40.0], 1.0);
        let wide = laid([10, 10], Fit::Tile, corner, [300.0, 40.0], 1.0);
        assert_eq!(narrow.uv[0], wide.uv[0], "growing moved the copies");
        assert_eq!(wide.uv[2], 30.0);
    }

    /// A cell painted back is the picture's own piece at that place, and
    /// nothing where the picture does not reach.
    ///
    /// Deliberate break: the piece's `uv` taken from the rectangle asked
    /// for rather than the part of it the picture covers, which reads the
    /// picture from outside its edge.
    #[test]
    fn a_piece_is_the_picture_where_it_is() {
        let laid = super::Laid {
            rect: [10.0, 0.0, 100.0, 50.0],
            uv: [0.0, 0.0, 1.0, 1.0],
        };
        let piece = laid.piece([0.0, 25.0, 20.0, 10.0]).expect("a piece");
        assert_eq!(piece.rect, [10.0, 25.0, 10.0, 10.0]);
        assert_eq!(piece.uv, [0.0, 0.5, 0.1, 0.7]);
        assert!(laid.piece([0.0, 60.0, 20.0, 10.0]).is_none());
    }

    /// A picture fitted to the window is made no larger than the screen
    /// needs, and none is made larger than it is.
    ///
    /// Deliberate breaks: dropping `.min(1.0)` (a small picture is then
    /// blown up to the screen), and treating a tile like a cover (it is
    /// then shrunk, and drawn smaller than its own size).
    #[test]
    fn a_picture_is_kept_no_larger_than_it_is_drawn() {
        let screen = [1920, 1080];
        assert_eq!(wanted([7680, 4320], Fit::Cover, screen), [1920, 1080]);
        // Covering a wide screen with a tall picture takes its full width.
        assert_eq!(wanted([2000, 4000], Fit::Cover, screen), [1920, 3840]);
        assert_eq!(wanted([800, 600], Fit::Cover, screen), [800, 600]);
        assert_eq!(wanted([3000, 3000], Fit::Tile, screen), [3000, 3000]);
        // And never past what a card is sure to take.
        let huge = wanted([20000, 100], Fit::Tile, screen);
        assert_eq!(huge[0], LARGEST);
    }
}
