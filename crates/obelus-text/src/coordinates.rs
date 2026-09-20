//! The coordinate spaces obelus works in, each as its own type.
//!
//! Four of them describe the same position in different units, and mixing them
//! up is the single most likely source of bugs in a program like this: a byte
//! offset handed to a `char` API silently points somewhere else the moment a
//! line contains anything outside ASCII. They are distinct types so that the
//! compiler refuses the mix, and every conversion between them lives on
//! [`Text`](crate::Text) rather than being open-coded at the use site.

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

/// A UTF-16 code unit offset from the start of its line.
///
/// The unit the language server protocol counts in unless a server agrees to
/// count bytes instead. Distinct from every other column here: a character
/// outside the basic multilingual plane — an emoji — is one `char`, two of
/// these, four bytes and two cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Utf16Column(usize);

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
usize_coordinate!(Utf16Column);

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

/// A place in a document, in every unit that wants one.
///
/// The byte it begins at, and the row and byte-into-the-row a parser wants.
/// All three together rather than one and a conversion, because a conversion
/// needs the text and these are wanted at moments when the text is halfway
/// through changing: an edit's old end does not exist any more once the edit
/// has happened, and its new end did not exist before it.
///
/// `column` is **bytes** into the line, not characters -- tree-sitter's unit,
/// and the one place in this program where a column is not a `char`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place {
    /// How far into the document, in bytes.
    pub byte: ByteOffset,
    /// Which line, counting from zero.
    pub row: usize,
    /// How far into that line, in bytes.
    pub column: usize,
}

/// A run of characters in a document.
///
/// Where the two ends are on the same line, which is nearly always, `line`
/// and `end_line` are equal. Nothing here assumes that: a language server is
/// entitled to name a range that spans lines, and a highlight that assumed
/// otherwise would mark the wrong cells rather than fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    /// The line it starts on.
    pub line: LineNumber,
    /// The character it starts at.
    pub column: CharColumn,
    /// The line it ends on.
    pub end_line: LineNumber,
    /// One past the character it ends at.
    pub end_column: CharColumn,
}

impl Span {
    /// Whether a position is inside it.
    #[must_use]
    pub fn contains(self, line: LineNumber, column: CharColumn) -> bool {
        if line < self.line || line > self.end_line {
            return false;
        }
        if line == self.line && column < self.column {
            return false;
        }
        if line == self.end_line && column >= self.end_column {
            return false;
        }
        true
    }
}

#[cfg(test)]
mod spans {
    use super::*;

    fn span(line: usize, column: usize, end_line: usize, end_column: usize) -> Span {
        Span {
            line: LineNumber::new(line),
            column: CharColumn::new(column),
            end_line: LineNumber::new(end_line),
            end_column: CharColumn::new(end_column),
        }
    }

    #[test]
    fn one_line_covers_its_own_characters_and_no_others() {
        let it = span(4, 8, 4, 12);
        assert!(!it.contains(LineNumber::new(4), CharColumn::new(7)));
        assert!(it.contains(LineNumber::new(4), CharColumn::new(8)));
        assert!(it.contains(LineNumber::new(4), CharColumn::new(11)));
        // The end is one past, the way a range is.
        assert!(!it.contains(LineNumber::new(4), CharColumn::new(12)));
        assert!(!it.contains(LineNumber::new(3), CharColumn::new(9)));
        assert!(!it.contains(LineNumber::new(5), CharColumn::new(9)));
    }

    /// A server is entitled to name a range that spans lines. Assuming it
    /// cannot marks the wrong cells rather than failing.
    #[test]
    fn several_lines_cover_the_ends_and_all_of_the_middle() {
        let it = span(2, 5, 4, 3);
        assert!(!it.contains(LineNumber::new(2), CharColumn::new(4)));
        assert!(it.contains(LineNumber::new(2), CharColumn::new(5)));
        // A whole line in the middle, however long it is.
        assert!(it.contains(LineNumber::new(3), CharColumn::new(0)));
        assert!(it.contains(LineNumber::new(3), CharColumn::new(999)));
        assert!(it.contains(LineNumber::new(4), CharColumn::new(2)));
        assert!(!it.contains(LineNumber::new(4), CharColumn::new(3)));
    }
}
