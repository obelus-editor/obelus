//! A picture under the page.
//!
//! **A picture travels beside a theme's colours and not among them.** It is
//! what a theme may carry and only a window draws: a terminal has no pixels
//! to put one in. So it is not a field of [`Theme`](crate::Theme), whose
//! every field is a colour some view paints with in both front ends -- it
//! goes with the theme as part of what is worn ([`Worn`](crate::Worn)), and
//! the one reader of it is the window.
//!
//! What is here is what the file says. Where the picture lands on a window
//! of a given size is the window's arithmetic, because only the window
//! knows its size.

use std::path::{Path, PathBuf};

/// The picture, and how it is laid under the page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wallpaper {
    /// Where the picture is.
    pub file: PathBuf,
    /// How much of the page's own colour is laid over it, out of a
    /// hundred: a hundred is the page and no picture, nothing is the
    /// picture as it is.
    ///
    /// The page's colour and not the picture's transparency, because what
    /// is read on top of it is code: the number says how quiet the picture
    /// is under the text, which is the one thing a theme's author has to
    /// get right.
    pub opacity: u8,
    /// How it is fitted to the window.
    pub fit: Fit,
    /// Which part of it stays where the window's edges move.
    pub anchor: Anchor,
    /// When the picture was last written, as far as the file system says.
    ///
    /// Here so that a picture replaced under the same name is a different
    /// wallpaper: the window is told only when what is worn has moved, and
    /// a theme whose file and settings are unchanged has not -- while its
    /// picture may have been.
    pub modified: Option<std::time::SystemTime>,
}

/// How much of the page covers a picture that does not say.
///
/// Enough that text is read on the page and not on the picture: a
/// photograph under code at half strength is a photograph with code lost
/// in it.
pub const OPACITY: u8 = 85;

/// How a picture is fitted to a window whose shape is not its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Fit {
    /// As large as it takes to cover the window, and what does not fit
    /// is cut off.
    #[default]
    Cover,
    /// As large as it can be with all of it showing, and the page's colour
    /// round it.
    Contain,
    /// The window's shape, whatever that does to the picture's.
    Stretch,
    /// Its own size, once.
    Center,
    /// Its own size, over and over until the window is covered.
    Tile,
}

impl Fit {
    /// Every fit, by the word a theme file writes it in.
    pub const ALL: [(&'static str, Self); 5] = [
        ("cover", Self::Cover),
        ("contain", Self::Contain),
        ("stretch", Self::Stretch),
        ("center", Self::Center),
        ("tile", Self::Tile),
    ];

    /// The fit a word names, if it names one.
    #[must_use]
    pub fn named(word: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .find(|(name, _)| *name == word)
            .map(|(_, fit)| *fit)
    }
}

/// One side of a picture, or its middle, along one way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Edge {
    /// The left, or the top.
    Start,
    /// Half way.
    #[default]
    Middle,
    /// The right, or the bottom.
    End,
}

/// Which point of a picture is held to the same point of the window: what
/// is cut off, where there is room left over, and where a tiling starts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Anchor {
    /// Left, middle or right.
    pub across: Edge,
    /// Top, middle or bottom.
    pub down: Edge,
}

impl Anchor {
    /// Every anchor, by the word a theme file writes it in.
    pub const ALL: [(&'static str, Self); 9] = [
        ("top-left", Self::at(Edge::Start, Edge::Start)),
        ("top", Self::at(Edge::Middle, Edge::Start)),
        ("top-right", Self::at(Edge::End, Edge::Start)),
        ("left", Self::at(Edge::Start, Edge::Middle)),
        ("center", Self::at(Edge::Middle, Edge::Middle)),
        ("right", Self::at(Edge::End, Edge::Middle)),
        ("bottom-left", Self::at(Edge::Start, Edge::End)),
        ("bottom", Self::at(Edge::Middle, Edge::End)),
        ("bottom-right", Self::at(Edge::End, Edge::End)),
    ];

    const fn at(across: Edge, down: Edge) -> Self {
        Self { across, down }
    }

    /// The anchor a word names, if it names one.
    #[must_use]
    pub fn named(word: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .find(|(name, _)| *name == word)
            .map(|(_, anchor)| *anchor)
    }
}

/// The picture in `file`, decoded, its pixels as the file has them.
///
/// The one reading of a wallpaper: the window that draws it and the check a
/// theme is put through before it is published both come here, so a theme
/// that passes the check has a picture the window can draw. By what the
/// bytes are rather than by the file's name, which a picture saved from a
/// browser does not always say truly.
///
/// # Errors
///
/// When the file cannot be read, or is not a picture this can decode.
pub fn decode(file: &Path) -> image::ImageResult<image::RgbaImage> {
    Ok(image::ImageReader::open(file)?
        .with_guessed_format()?
        .decode()?
        .into_rgba8())
}

#[cfg(test)]
mod tests {
    use super::{Anchor, Edge, Fit};

    /// Every word a theme file may write is read back as what it says,
    /// and nothing else is read at all.
    ///
    /// Deliberate break: `top` and `bottom` swapped in the table, which
    /// fails the third assertion.
    #[test]
    fn the_words_name_what_they_say() {
        assert_eq!(Fit::named("tile"), Some(Fit::Tile));
        assert_eq!(Fit::named("Tile"), None, "a word is written one way");
        assert_eq!(
            Anchor::named("top"),
            Some(Anchor {
                across: Edge::Middle,
                down: Edge::Start
            })
        );
        assert_eq!(Anchor::named("middle"), None);
        assert_eq!(Anchor::default(), Anchor::named("center").expect("center"));
    }
}
