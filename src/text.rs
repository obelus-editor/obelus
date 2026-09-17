//! The document, and the only place coordinate arithmetic happens.
//!
//! Every conversion between the spaces in [`crate::coordinates`] is a method
//! here. Nothing else in the crate is allowed to derive one coordinate from
//! another by hand — that rule is what keeps a byte offset from reaching a
//! `char` API.

use ropey::{Rope, RopeSlice};
use unicode_linebreak::linebreaks;
use unicode_width::UnicodeWidthChar;

use crate::coordinates::{
    ByteOffset, CharColumn, CharOffset, DisplayColumn, LineNumber, Place, Span, Utf16Column,
};

/// How many cells a tab advances to.
///
/// Fixed rather than configurable: a reader that renders a file differently
/// from the tool that wrote it is worse than one that picks a number, and four
/// is the number the code obelus is written in uses.
pub const TAB_WIDTH: usize = 4;

/// How wide a tab is laid out, while the reader has said something else.
///
/// A global for the reason the glyph switch is one: it is a drawing
/// decision the whole program shares, and threading it through every method
/// here that measures a line -- nine of them, and every caller of each --
/// would put a parameter on the arithmetic rather than on the setting.
/// Written once at startup and once per change of the setting, read while
/// measuring.
static TABS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(TAB_WIDTH);

/// Lays tabs out at `width` from now on.
///
/// Clamped, because a tab of nothing is a character that cannot be stepped
/// over and a file somebody typed `0` into should not produce one.
pub fn lay_tabs_at(width: usize) {
    TABS.store(width.clamp(1, 16), std::sync::atomic::Ordering::Relaxed);
}

/// How wide a tab is being laid out.
#[must_use]
pub fn tab_width() -> usize {
    TABS.load(std::sync::atomic::Ordering::Relaxed)
}

/// What an edit did, in the units everything downstream measures in.
///
/// Three places: where it began, where what it replaced ended, and where
/// what it put there ends. The first two are in the document as it was, the
/// third in the document as it is -- which is why they are taken as the edit
/// happens rather than worked out afterwards from two whole texts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edit {
    /// Where it began. The same in both documents: nothing before it moved.
    pub start: Place,
    /// Where what it took out ended, in the document as it was.
    pub old_end: Place,
    /// Where what it put in ends, in the document as it is.
    pub new_end: Place,
}

impl Edit {
    /// How many lines the document gained, or lost where it is negative.
    ///
    /// Everything below the edit that is remembered by line number -- a fold,
    /// a block, a mark -- moves by this.
    #[must_use]
    pub const fn lines(&self) -> isize {
        self.new_end.row as isize - self.old_end.row as isize
    }
}

/// A cell drawn inside a line that the line does not contain.
///
/// A colour's swatch today, a type a server worked out tomorrow. What they
/// have in common is the only thing this module cares about: a reader sees
/// them and a `char` offset does not, so every column past one of them is
/// drawn one cell further along than the text alone would put it.
///
/// Which is why they live here. This module is the only place a coordinate
/// turns into another one, and a cell that no character owns is exactly
/// that arithmetic -- worked out anywhere else, the cursor, the pointer,
/// the wrapping and the painting would each have their own idea of where
/// the fourth character of a line is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Phantom {
    /// Which line it is drawn in.
    pub line: LineNumber,
    /// The character it is drawn in front of.
    ///
    /// In front of, never on: the caret walks characters, and a cell it
    /// cannot stand on is a cell it never has to know about.
    pub column: CharColumn,
    /// How many cells it takes.
    pub cells: usize,
    /// Which of whatever the caller registered this one draws.
    ///
    /// An index rather than the thing itself: a swatch is a colour and a
    /// hint is a string, and a module about coordinates has no business
    /// knowing either.
    pub which: usize,
}

/// A document's text, plus the coordinate conversions over it.
#[derive(Clone, Debug)]
pub struct Text {
    rope: Rope,
    /// What is drawn in it that it does not contain, by line.
    ///
    /// Empty in almost every document there is: they arrive from a language
    /// server, about the file the reader is looking at, and a preview or a
    /// commit's text is a different document that happens to share a name.
    phantoms: std::collections::HashMap<usize, Vec<Phantom>>,
}

impl Text {
    /// Reads a document from a string.
    #[must_use]
    pub fn from_string(contents: &str) -> Self {
        Self {
            rope: Rope::from_str(contents),
            phantoms: std::collections::HashMap::new(),
        }
    }

    /// Draws these in it from now on, in place of whatever was there.
    ///
    /// All of them at once, because they arrive that way: a server answers
    /// about a whole document, and a list that were added one at a time
    /// would be a list nothing could take away.
    pub fn show(&mut self, phantoms: &[Phantom]) {
        self.phantoms.clear();
        for phantom in phantoms {
            self.phantoms
                .entry(phantom.line.get())
                .or_default()
                .push(*phantom);
        }
        // In the order they are drawn, so walking a line's glyphs can take
        // them in one pass.
        for line in self.phantoms.values_mut() {
            line.sort_unstable_by_key(|phantom| phantom.column.get());
        }
    }

    /// Where every cell drawn in this text is, as a document-wide offset.
    ///
    /// Offsets rather than lines and columns, because that is what an edit
    /// moves in one number: a line and a column have to be reasoned about
    /// against where the edit began, where it ended and how many lines it
    /// was, and every one of those is a chance to be one out.
    fn held_phantoms(&self) -> Vec<(usize, Phantom)> {
        self.phantoms
            .values()
            .flatten()
            .map(|phantom| {
                (
                    self.char_offset(phantom.line, phantom.column).get(),
                    *phantom,
                )
            })
            .collect()
    }

    /// Puts them back where `moved` says they are now.
    ///
    /// `None` from it drops one: an edit that took away what a cell was
    /// drawn in front of took the cell with it.
    fn put_phantoms_back(
        &mut self,
        held: Vec<(usize, Phantom)>,
        moved: impl Fn(usize) -> Option<usize>,
    ) {
        if held.is_empty() {
            return;
        }
        let put: Vec<Phantom> = held
            .into_iter()
            .filter_map(|(offset, phantom)| {
                let (line, column) = self.position(CharOffset::new(moved(offset)?));
                Some(Phantom {
                    line,
                    column,
                    ..phantom
                })
            })
            .collect();
        self.show(&put);
    }

    /// What is drawn in a line that the line does not contain.
    #[must_use]
    pub fn phantoms(&self, line: LineNumber) -> &[Phantom] {
        self.phantoms
            .get(&line.get())
            .map_or(&[] as &[Phantom], Vec::as_slice)
    }

    /// The underlying rope, for the parser to read through.
    #[must_use]
    pub const fn rope(&self) -> &Rope {
        &self.rope
    }

    /// Puts `what` in at `at`, and says what that did.
    ///
    /// The one way text grows. Everything a document keeps beside its text is
    /// measured in lines or bytes, so the answer has to be in both, and it
    /// has to be taken while each end still exists: the new end is not there
    /// to be measured until the rope has changed.
    pub fn insert(&mut self, at: CharOffset, what: &str) -> Edit {
        let at = CharOffset::new(at.get().min(self.rope.len_chars()));
        // Where the cells drawn in it are, before the places they are named
        // by move. They are the last of the things remembered by line
        // number that an edit has to carry with it -- a fold, a block, a
        // mark, and these.
        let held = self.held_phantoms();
        let start = self.place(self.byte_of_char(at));
        self.rope.insert(at.get(), what);
        self.put_phantoms_back(held, |offset| match offset >= at.get() {
            true => Some(offset + what.chars().count()),
            false => Some(offset),
        });
        Edit {
            start,
            // Nothing was taken out, so the old end is where it began.
            old_end: start,
            new_end: self.place(ByteOffset::new(start.byte.get() + what.len())),
        }
    }

    /// Takes `span` out, and hands back what was there.
    ///
    /// The other way round from [`insert`](Self::insert): the *old* end is
    /// the one that has to be measured first, and afterwards both ends are
    /// where the span began.
    pub fn remove(&mut self, span: Span) -> (String, Edit) {
        let from = self.char_offset(span.line, span.column);
        let to = self.char_offset(span.end_line, span.end_column);
        // A span whose ends arrive the wrong way round is a caller's slip,
        // not a reason to take out the whole document.
        let (from, to) = (from.min(to), from.max(to));
        let start = self.place(self.byte_of_char(from));
        let old_end = self.place(self.byte_of_char(to));
        let removed = self.rope.slice(from.get()..to.get()).to_string();
        // Where the cells drawn in it are, before the places they are named
        // by move.
        let held = self.held_phantoms();
        self.rope.remove(from.get()..to.get());
        let gone = to.get() - from.get();
        self.put_phantoms_back(held, |offset| match offset {
            // Inside what was taken out. There is nothing left for it to
            // be drawn in front of.
            offset if offset >= from.get() && offset < to.get() => None,
            offset if offset >= to.get() => Some(offset - gone),
            offset => Some(offset),
        });
        (
            removed,
            Edit {
                start,
                old_end,
                // What is left begins where the span did, and nothing before
                // it moved, so the place is the same one.
                new_end: start,
            },
        )
    }

    /// Where a byte is, in all three units at once.
    ///
    /// Taken together rather than one at a time, because the caller wanting
    /// them is in the middle of changing the text and the other two answers
    /// would be about a different document by the time it asked.
    #[must_use]
    pub fn place(&self, byte: ByteOffset) -> Place {
        Place {
            byte,
            row: self.line_of_byte(byte).get(),
            column: self.byte_column(byte),
        }
    }

    /// How many lines the document has.
    ///
    /// A document ending in a newline has a final empty line, which is
    /// `ropey`'s convention and also what an editor shows.
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    /// The last valid line number.
    #[must_use]
    pub fn last_line(&self) -> LineNumber {
        LineNumber::new(self.line_count().saturating_sub(1))
    }

    /// Clamps a line number to one that exists in this document.
    #[must_use]
    pub fn clamp_line(&self, line: LineNumber) -> LineNumber {
        line.min(self.last_line())
    }

    /// A line's content, without its line ending.
    ///
    /// Out-of-range lines clamp to the last line rather than panicking:
    /// callers hold line numbers across reloads, and a document can shrink
    /// under them.
    #[must_use]
    pub fn line(&self, line: LineNumber) -> RopeSlice<'_> {
        let slice = self.rope.line(self.clamp_line(line).get());
        let mut end = slice.len_chars();
        // A line ending is at most "\r\n", and only ever at the end.
        if end > 0 && slice.char(end - 1) == '\n' {
            end -= 1;
        }
        if end > 0 && slice.char(end - 1) == '\r' {
            end -= 1;
        }
        slice.slice(..end)
    }

    /// How many characters a line holds, not counting its line ending.
    #[must_use]
    pub fn line_length(&self, line: LineNumber) -> CharColumn {
        CharColumn::new(self.line(line).len_chars())
    }

    /// Clamps a column to one that exists on `line`.
    ///
    /// The column may equal the line's length: that is the position after the
    /// last character, where a cursor legitimately sits.
    #[must_use]
    pub fn clamp_column(&self, line: LineNumber, column: CharColumn) -> CharColumn {
        column.min(self.line_length(line))
    }

    /// The document-wide `char` offset of a position.
    #[must_use]
    pub fn char_offset(&self, line: LineNumber, column: CharColumn) -> CharOffset {
        let line = self.clamp_line(line);
        let column = self.clamp_column(line, column);
        CharOffset::new(self.rope.line_to_char(line.get()) + column.get())
    }

    /// Splits a document-wide `char` offset back into a line and a column.
    #[must_use]
    pub fn position(&self, offset: CharOffset) -> (LineNumber, CharColumn) {
        let offset = offset.get().min(self.rope.len_chars());
        let line = self.rope.char_to_line(offset);
        let column = offset - self.rope.line_to_char(line);
        (LineNumber::new(line), CharColumn::new(column))
    }

    /// The characters in `span`, including any line endings between its ends.
    ///
    /// A selection is expressed in line and character columns, while Rope
    /// slices use document-wide character offsets. This is the one conversion
    /// between them, so copying a selection cannot accidentally index bytes.
    #[must_use]
    pub fn text_in(&self, span: Span) -> String {
        let start = self.char_offset(span.line, span.column);
        let end = self.char_offset(span.end_line, span.end_column);
        self.rope.slice(start.get()..end.get()).to_string()
    }

    /// The byte offset of a `char` offset.
    #[must_use]
    pub fn byte_of_char(&self, offset: CharOffset) -> ByteOffset {
        ByteOffset::new(
            self.rope
                .char_to_byte(offset.get().min(self.rope.len_chars())),
        )
    }

    /// The `char` offset of a byte offset.
    ///
    /// # Panics
    ///
    /// Panics if `offset` is not a character boundary. Every byte offset in
    /// obelus comes from tree-sitter, which only ever reports boundaries.
    #[must_use]
    pub fn char_of_byte(&self, offset: ByteOffset) -> CharOffset {
        CharOffset::new(
            self.rope
                .byte_to_char(offset.get().min(self.rope.len_bytes())),
        )
    }

    /// The byte offset at which a line starts.
    #[must_use]
    pub fn line_start_byte(&self, line: LineNumber) -> ByteOffset {
        ByteOffset::new(self.rope.line_to_byte(self.clamp_line(line).get()))
    }

    /// How many UTF-16 code units into its line a column is.
    ///
    /// What the language server protocol counts in when a server will not
    /// agree to bytes. A character outside the basic multilingual plane costs
    /// two of these and one of everything else, which is the case that a
    /// conversion written as `column as u32` gets wrong on every position
    /// after it on the line.
    #[must_use]
    pub fn utf16_column(&self, line: LineNumber, column: CharColumn) -> Utf16Column {
        let column = self.clamp_column(line, column);
        Utf16Column::new(
            self.line(line)
                .chars()
                .take(column.get())
                .map(char::len_utf16)
                .sum(),
        )
    }

    /// The column at a UTF-16 offset into a line.
    ///
    /// An offset landing on the second half of a surrogate pair resolves to
    /// the character it belongs to, the way a display column landing on the
    /// second cell of a wide glyph does.
    #[must_use]
    pub fn column_at_utf16(&self, line: LineNumber, target: Utf16Column) -> CharColumn {
        let target = target.get();
        let mut units = 0usize;
        for (index, character) in self.line(line).chars().enumerate() {
            let next = units + character.len_utf16();
            if target < next {
                return CharColumn::new(index);
            }
            units = next;
        }
        CharColumn::new(self.line(line).len_chars())
    }

    /// The line a byte offset falls on.
    #[must_use]
    pub fn line_of_byte(&self, byte: ByteOffset) -> LineNumber {
        LineNumber::new(
            self.rope
                .byte_to_line(byte.get().min(self.rope.len_bytes())),
        )
    }

    /// How many bytes into its line a byte offset is.
    ///
    /// Bytes, not characters: this is the number tree-sitter's `Point.column`
    /// wants, and giving it a character count puts every edit on a non-ASCII
    /// line in the wrong place.
    #[must_use]
    pub fn byte_column(&self, byte: ByteOffset) -> usize {
        let byte = byte.get().min(self.rope.len_bytes());
        byte - self.rope.line_to_byte(self.rope.byte_to_line(byte))
    }

    /// How many bytes this document has.
    #[must_use]
    pub fn byte_length(&self) -> ByteOffset {
        ByteOffset::new(self.rope.len_bytes())
    }

    /// How many leading bytes two documents share, rounded down to a character
    /// boundary.
    #[must_use]
    pub fn common_prefix(&self, other: &Self) -> ByteOffset {
        let mut shared = 0usize;
        for (left, right) in self.rope.bytes().zip(other.rope.bytes()) {
            if left != right {
                break;
            }
            shared += 1;
        }
        ByteOffset::new(self.floor_boundary(shared))
    }

    /// How many trailing bytes two documents share, without overlapping
    /// `prefix` and rounded to a character boundary.
    #[must_use]
    pub fn common_suffix(&self, other: &Self, prefix: ByteOffset) -> usize {
        let limit = self
            .rope
            .len_bytes()
            .min(other.rope.len_bytes())
            .saturating_sub(prefix.get());
        let mut shared = 0usize;
        for (left, right) in self
            .rope
            .bytes_at(self.rope.len_bytes())
            .reversed()
            .zip(other.rope.bytes_at(other.rope.len_bytes()).reversed())
        {
            if shared >= limit || left != right {
                break;
            }
            shared += 1;
        }
        // Round the resulting position up to a boundary, which can only
        // shorten the suffix.
        let position = self.rope.len_bytes() - shared;
        self.rope.len_bytes() - self.ceil_boundary(position)
    }

    /// The character boundary at or before `byte`.
    fn floor_boundary(&self, byte: usize) -> usize {
        let byte = byte.min(self.rope.len_bytes());
        self.rope.char_to_byte(self.rope.byte_to_char(byte))
    }

    /// The character boundary at or after `byte`.
    fn ceil_boundary(&self, byte: usize) -> usize {
        let byte = byte.min(self.rope.len_bytes());
        let floor = self.floor_boundary(byte);
        if floor == byte {
            return byte;
        }
        self.rope.char_to_byte(self.rope.byte_to_char(byte) + 1)
    }

    /// The display column a position renders at.
    #[must_use]
    pub fn display_column(&self, line: LineNumber, column: CharColumn) -> DisplayColumn {
        // Over the glyphs rather than the characters, because a line can
        // be drawn with cells in it that it does not contain and every
        // column past one of those is a cell further along.
        let mut width = 0usize;
        for glyph in self.glyphs(line) {
            if glyph.phantom.is_none() && glyph.column.get() >= column.get() {
                break;
            }
            width += glyph.cells;
        }
        DisplayColumn::saturating_from_usize(width)
    }

    /// How many cells a whole line renders as.
    #[must_use]
    pub fn line_display_width(&self, line: LineNumber) -> DisplayColumn {
        self.display_column(line, self.line_length(line))
    }

    /// The column whose glyph covers `target`.
    ///
    /// A display column landing on the second cell of a wide glyph resolves to
    /// that glyph, not to the one after it.
    #[must_use]
    pub fn column_at_display(&self, line: LineNumber, target: DisplayColumn) -> CharColumn {
        let target = usize::from(target.get());
        for glyph in self.glyphs(line) {
            if target < glyph.first_cell + glyph.cells {
                // A cell nothing in the file owns answers with the
                // character it was drawn in front of, which is where a
                // reader pointing at it means to be.
                return glyph.column;
            }
        }
        CharColumn::new(self.line(line).len_chars())
    }
}

/// How many cells to indent the rows after a line's first.
///
/// Capped: an indentation taking more than half the room would leave a
/// wrapped line in deeply nested code almost no width to wrap into, and many
/// short rows are harder to read than one unindented one.
fn continuation_indent(glyphs: &[Glyph], width: u16) -> u16 {
    let cells: usize = glyphs
        .iter()
        .take_while(|glyph| glyph.character.is_whitespace())
        .map(|glyph| glyph.cells)
        .sum();
    let indent = u16::try_from(cells).unwrap_or(u16::MAX);
    if indent.saturating_mul(2) >= width {
        0
    } else {
        indent
    }
}

/// How many cells `character` occupies when the cursor is already `width`
/// cells into the line.
///
/// The position matters only for a tab, which advances to the next tab stop
/// rather than by a fixed amount. Characters `unicode-width` has no opinion
/// about — control characters, most combining marks — occupy none, which is
/// also how a terminal treats them.
fn char_width(character: char, width: usize) -> usize {
    if character == '\t' {
        let tabs = tab_width();
        tabs - (width % tabs)
    } else {
        character.width().unwrap_or(0)
    }
}

/// One visual row of a wrapped line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WrapRow {
    /// The first character on the row.
    pub first: CharColumn,
    /// One past the last character on the row.
    pub end: CharColumn,
    /// Cells of blank drawn before the text on this row.
    ///
    /// Zero on a line's first row, which already contains its own
    /// indentation. On the rows after it, the line's indentation again, so a
    /// wrapped statement still reads as being inside its block.
    pub indent: u16,
}

/// One rendered glyph: what it is, and the cells it covers.
///
/// The renderer needs the cell span of every character on a line, and working
/// it out itself would mean accumulating widths outside this module. Yielding
/// spans keeps that arithmetic here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glyph {
    /// The character.
    ///
    /// A blank for a phantom, which is a cell with nothing written in it:
    /// what is drawn there is whatever registered it, and all this knows is
    /// that the cell is spoken for.
    pub character: char,
    /// Which character of the line it is.
    ///
    /// Carried rather than counted by whoever walks them: the walk yields
    /// cells that are not characters, so counting the steps stopped being
    /// the same as counting the columns.
    pub column: CharColumn,
    /// Which of the caller's own list this draws, where it is a phantom.
    pub phantom: Option<usize>,
    /// Where it starts in the document, for looking up its highlight.
    ///
    /// The byte of the character a phantom sits in front of, so what is
    /// drawn there is coloured the way that character is.
    pub first_byte: ByteOffset,
    /// The first cell it occupies, counted from the start of the line.
    pub first_cell: usize,
    /// How many cells it occupies. Zero for a character a terminal does not
    /// advance over.
    pub cells: usize,
}

impl Text {
    /// Where a line breaks when wrapped to `width` cells.
    ///
    /// Break opportunities come from `unicode-linebreak`, which implements the
    /// Unicode line breaking algorithm. Breaking only at whitespace would be
    /// simpler and would fail completely on Chinese, Japanese and Thai, which
    /// have no spaces to break at: the algorithm knows a break is allowed
    /// between two Han characters and not between a word and the comma after
    /// it.
    ///
    /// A run with no break opportunity in it at all — `foo(bar,baz)`, a
    /// base64 blob, a long path — is broken at the margin instead. There is
    /// nothing better to do with it, and leaving it to overflow would be
    /// worse.
    ///
    /// A wide glyph is never split: with only one cell left before the margin
    /// it moves to the next row, and that cell stays blank. An empty line is
    /// one row, because a cursor can sit on it.
    ///
    /// Allocates. This is called for the visible lines once per keystroke, and
    /// a keystroke already costs a tree-sitter query and a repaint; the
    /// scratch buffers that would avoid it would have to be threaded through
    /// the cursor, the scroller and the renderer to save nothing measurable.
    #[must_use]
    pub fn wrap_rows(&self, line: LineNumber, width: u16) -> Vec<WrapRow> {
        let width = width.max(1);
        // Nothing wraps at this width, and the answer is the whole line --
        // which is what `wrap_width` says when the reader has wrapping off.
        // Worth its own arm: this is asked twice for every row of every
        // frame, and the general answer walks the line measuring glyphs to
        // find the break it will not need.
        if width == u16::MAX {
            return vec![WrapRow {
                first: CharColumn::new(0),
                end: self.line_length(line),
                indent: 0,
            }];
        }
        let glyphs: Vec<Glyph> = self.glyphs(line).collect();
        if glyphs.is_empty() {
            // The one row an empty line gets.
            return vec![WrapRow {
                first: CharColumn::new(0),
                end: CharColumn::new(0),
                indent: 0,
            }];
        }

        let breaks = self.break_columns(line);
        let indent = continuation_indent(&glyphs, width);

        let mut rows: Vec<WrapRow> = Vec::new();
        let mut first = 0usize;
        while first < glyphs.len() {
            let pad = if rows.is_empty() { 0 } else { indent };
            let room = usize::from(width - pad);

            // How far the cells reach before the margin.
            let mut fits = first;
            let mut cells = 0usize;
            while fits < glyphs.len() {
                let taken = glyphs[fits].cells.max(1);
                // A glyph wider than the whole row still has to go somewhere,
                // or an empty row would be emitted forever.
                if cells + taken > room && fits > first {
                    break;
                }
                cells += taken;
                fits += 1;
            }

            let end = if fits >= glyphs.len() {
                glyphs.len()
            } else {
                // Back off to the last place a break is allowed. None in
                // range means an unbreakable run, and the margin is the only
                // answer left.
                let candidate = breaks.partition_point(|column| *column <= fits);
                breaks
                    .get(candidate.wrapping_sub(1))
                    .copied()
                    .filter(|column| *column > first)
                    .unwrap_or(fits)
            };

            rows.push(WrapRow {
                first: CharColumn::new(first),
                end: CharColumn::new(end),
                indent: pad,
            });
            first = end;
        }
        rows
    }

    /// The columns a line may start at, ascending.
    fn break_columns(&self, line: LineNumber) -> Vec<usize> {
        let contents: String = self.line(line).chars().collect();
        // The algorithm reports byte offsets; everything else here counts
        // characters.
        let mut byte_to_column = vec![0usize; contents.len() + 1];
        for (column, (byte, _)) in contents.char_indices().enumerate() {
            byte_to_column[byte] = column;
        }
        byte_to_column[contents.len()] = contents.chars().count();

        linebreaks(&contents)
            .filter_map(|(byte, _)| byte_to_column.get(byte).copied())
            .collect()
    }

    /// How many visual rows a line takes.
    #[must_use]
    pub fn row_count(&self, line: LineNumber, width: u16) -> usize {
        self.wrap_rows(line, width).len()
    }

    /// Which visual row a position is on, and how many cells into it.
    #[must_use]
    pub fn visual_position(
        &self,
        line: LineNumber,
        column: CharColumn,
        width: u16,
    ) -> (usize, DisplayColumn) {
        let column = self.clamp_column(line, column);
        let rows = self.wrap_rows(line, width);
        for (index, row) in rows.iter().enumerate() {
            // `end` is one past the row's last character. A cursor sitting
            // there belongs to the next row, except on the last one, where it
            // is the end of the line and there is nowhere else for it to be.
            let last = index + 1 == rows.len();
            if column < row.end || last {
                let start = self.display_column(line, row.first).get();
                let here = self.display_column(line, column).get();
                let inside = here.saturating_sub(start);
                return (index, DisplayColumn::new(row.indent.saturating_add(inside)));
            }
        }
        (0, DisplayColumn::new(0))
    }

    /// The column at a cell offset into one visual row of a line.
    ///
    /// The cell is counted from the left edge of the text area, so it includes
    /// the row's continuation indent.
    #[must_use]
    pub fn column_in_row(
        &self,
        line: LineNumber,
        row_index: usize,
        cell: DisplayColumn,
        width: u16,
    ) -> CharColumn {
        let rows = self.wrap_rows(line, width);
        let Some(row) = rows.get(row_index) else {
            return self.line_length(line);
        };
        let start = self.display_column(line, row.first).get();
        let inside = cell.get().saturating_sub(row.indent);
        let target = DisplayColumn::new(start.saturating_add(inside));

        // A row that is not the last stops one character short of its `end`:
        // that character starts the next row, and a cursor there belongs to
        // that row.
        let ceiling = if row_index + 1 == rows.len() {
            row.end
        } else {
            CharColumn::new(row.end.get().saturating_sub(1))
        };
        self.column_at_display(line, target)
            .min(ceiling.max(row.first))
            .max(row.first)
    }

    /// Every glyph on a line, with the cells it covers.
    pub fn glyphs(&self, line: LineNumber) -> impl Iterator<Item = Glyph> + '_ {
        let phantoms = self.phantoms(line);
        // The line's characters, once. `Chars` outlives the slice it came
        // from, which is what lets this be one walk rather than a lookup
        // per character -- and a lookup per character is the line's length
        // squared, forty rows a frame.
        let mut characters = self.line(line).chars();
        let mut cell = 0usize;
        let mut byte = self.line_start_byte(line).get();
        let mut next = 0usize;
        let mut column = 0usize;
        let mut held: Option<char> = None;
        std::iter::from_fn(move || {
            let character = match held.take() {
                Some(character) => Some(character),
                None => characters.next(),
            };
            // A phantom comes out in front of the character it sits on, and
            // wears that character's byte so whatever colours the line
            // colours it too. The character waits a turn.
            //
            // Past the last character as well, where there is no character
            // to wait: servers put them there, and a hint about what a
            // chain returns or what a closing brace closes goes after the
            // last thing on its line.
            if let Some(phantom) = phantoms.get(next).filter(|it| it.column.get() == column) {
                next += 1;
                held = character;
                let glyph = Glyph {
                    character: ' ',
                    column: CharColumn::new(column),
                    phantom: Some(phantom.which),
                    first_byte: ByteOffset::new(byte),
                    first_cell: cell,
                    cells: phantom.cells,
                };
                cell += phantom.cells;
                return Some(glyph);
            }
            let character = character?;
            let cells = char_width(character, cell);
            let glyph = Glyph {
                character,
                column: CharColumn::new(column),
                phantom: None,
                first_byte: ByteOffset::new(byte),
                first_cell: cell,
                cells,
            };
            cell += cells;
            byte += character.len_utf8();
            column += 1;
            Some(glyph)
        })
    }
}

/// Breaks prose into the rows it takes at this width.
///
/// The same word-breaking the editor wraps with, over something that is not
/// a document: an agent's answer, a card's description. Newlines in it are
/// its own, so each is a row of its own -- prose with a blank line in it has
/// a blank row, because that is what the writer meant by it.
///
/// The rows come back as owned strings because what asks for them is a view
/// drawing them once, not a document keeping them.
#[must_use]
pub fn wrapped(prose: &str, width: u16) -> Vec<String> {
    let width = width.max(1);
    let mut rows = Vec::new();
    for paragraph in prose.split('\n') {
        if paragraph.is_empty() {
            rows.push(String::new());
            continue;
        }
        let text = Text::from_string(paragraph);
        let line = LineNumber::new(0);
        let characters: Vec<char> = text.line(line).chars().collect();
        for row in text.wrap_rows(line, width) {
            let words: String = characters
                .iter()
                .take(row.end.get())
                .skip(row.first.get())
                .collect();
            rows.push(words.trim_end().to_string());
        }
    }
    rows
}
