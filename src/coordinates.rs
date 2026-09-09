//! The coordinate spaces obelus works in, each as its own type.
//!
//! Four of them describe the same position in different units, and mixing them
//! up is the single most likely source of bugs in a program like this: a byte
//! offset handed to a `char` API silently points somewhere else the moment a
//! line contains anything outside ASCII. They are distinct types so that the
//! compiler refuses the mix, and every conversion between them lives on
//! [`Text`](crate::text::Text) rather than being open-coded at the use site.

/// A byte offset from the start of the document.
///
/// The unit tree-sitter counts in: node ranges, query ranges and `InputEdit`
/// are all bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ByteOffset(usize);

/// A `char` offset from the start of the document.
///
/// `ropey`'s native unit, and what indexing into the rope expects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CharOffset(usize);

/// A zero-based line number.
///
/// Zero-based throughout; the `+ 1` that users expect to see happens in the
/// gutter and the status line, at the point of display.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LineNumber(usize);

/// A `char` offset from the start of its line.
///
/// The cursor's column, and what the status line reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CharColumn(usize);

/// A terminal display column, counted in cells.
///
/// Distinct from [`CharColumn`] because a wide glyph occupies two cells and a
/// tab occupies however many are left before the next tab stop. Only the
/// renderer and horizontal scrolling think in these.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DisplayColumn(u16);

macro_rules! usize_coordinate {
    ($name:ident) => {
        impl $name {
            /// Wraps a raw value.
            #[must_use]
            pub const fn new(value: usize) -> Self {
                Self(value)
            }

            /// Unwraps to the raw value.
            #[must_use]
            pub const fn get(self) -> usize {
                self.0
            }
        }
    };
}

usize_coordinate!(ByteOffset);
usize_coordinate!(CharOffset);
usize_coordinate!(LineNumber);
usize_coordinate!(CharColumn);

impl DisplayColumn {
    /// Wraps a raw cell count.
    #[must_use]
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    /// Unwraps to the raw cell count.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }

    /// Wraps a cell count that was computed as a `usize`, saturating rather
    /// than wrapping.
    ///
    /// Widths are accumulated in `usize` because a very long line can exceed
    /// `u16`, but a display column beyond the widest possible terminal is only
    /// ever compared against the viewport, so saturating loses nothing.
    #[must_use]
    pub fn saturating_from_usize(value: usize) -> Self {
        Self(u16::try_from(value).unwrap_or(u16::MAX))
    }
}

impl LineNumber {
    /// The line this many lines further down, saturating at the top of `usize`.
    #[must_use]
    pub const fn saturating_add(self, lines: usize) -> Self {
        Self(self.0.saturating_add(lines))
    }

    /// The line this many lines further up, saturating at line zero.
    #[must_use]
    pub const fn saturating_sub(self, lines: usize) -> Self {
        Self(self.0.saturating_sub(lines))
    }
}

impl CharColumn {
    /// The column this many characters to the right, saturating at the top of
    /// `usize`.
    #[must_use]
    pub const fn saturating_add(self, characters: usize) -> Self {
        Self(self.0.saturating_add(characters))
    }

    /// The column this many characters to the left, saturating at column zero.
    #[must_use]
    pub const fn saturating_sub(self, characters: usize) -> Self {
        Self(self.0.saturating_sub(characters))
    }
}
