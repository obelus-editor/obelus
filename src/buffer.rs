//! An open document: its text, where the cursor is, and what part of it is on
//! screen.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};

use crate::{
    coordinates::{CharColumn, DisplayColumn, LineNumber},
    syntax::{
        LanguageId,
        parse::{self, SyntaxState},
    },
    text::Text,
};

/// Which open document, by position in the list.
///
/// An index rather than a generational key because M0 cannot close a buffer.
/// The day it can, this becomes the thing that has to change, and the type
/// makes that one place instead of every `usize` that happened to be a buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BufferId(usize);

impl BufferId {
    /// Wraps a position in the buffer list.
    #[must_use]
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    /// The position in the buffer list.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

/// A direction to move the cursor in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    /// One character left.
    Left,
    /// One character right.
    Right,
    /// One line up.
    Up,
    /// One line down.
    Down,
    /// The first character of the line.
    LineStart,
    /// Past the last character of the line.
    LineEnd,
    /// The start of the document.
    DocumentStart,
    /// The last line of the document.
    DocumentEnd,
}

/// Where the cursor is, and where it would like to be.
#[derive(Clone, Copy, Debug)]
pub struct Cursor {
    /// The line the cursor is on.
    pub line: LineNumber,
    /// The character the cursor is before.
    pub column: CharColumn,
    /// The cell within a visual row that the cursor is aiming for while moving
    /// vertically.
    ///
    /// Without this, moving down through a short row and back up lands in the
    /// wrong place: the column would have been clamped on the way through and
    /// the original never recovered. Within a *row* rather than within a line,
    /// because with wrapping a row is what moving up and down steps over.
    remembered_cell: DisplayColumn,
}

/// Which part of the document is on screen.
///
/// A line and a visual row within it, because with wrapping a line can be
/// taller than the screen: anchoring only to a line would make everything past
/// the first screenful of one unreachable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Viewport {
    /// The first line drawn.
    pub top: LineNumber,
    /// Which visual row of `top` the screen starts on.
    pub top_row: usize,
}

/// The room the text has.
///
/// Both numbers together, because with wrapping neither is useful alone: the
/// width decides where lines break and so how many rows they take, and the
/// height decides how many of those rows fit.
#[derive(Clone, Copy, Debug)]
pub struct TextArea {
    /// Cells across, once the gutter has taken its columns.
    pub width: u16,
    /// Rows down.
    pub height: u16,
}

/// An open document.
#[derive(Debug)]
pub struct Buffer {
    path: PathBuf,
    text: Text,
    syntax: Option<SyntaxState>,
    /// Whether the last attempt to re-read the file failed.
    ///
    /// Set when the file has been deleted, replaced by a directory, or made
    /// unreadable. The buffer keeps showing what it last held — losing the
    /// contents would be worse than showing something out of date — so
    /// without saying so on screen the reader has no way to know.
    stale: bool,
    /// How many times this document has changed, counted from one.
    ///
    /// The protocol needs it on every change, and every request records the
    /// version it was asked against: an answer that arrives after a reload is
    /// about text that is no longer on screen.
    ///
    /// What is sent with it is the whole document, not the range that changed.
    /// The range would have to be in the *old* document's coordinates, which
    /// means a second representation of the edit alongside the byte offsets
    /// tree-sitter needs, converted through whichever units the server
    /// negotiated. That is the shape every coordinate bug in this program has
    /// had, and a source file down a pipe costs nothing.
    version: i32,
    cursor: Cursor,
    /// Whether the viewport has been paged away from the cursor on purpose.
    ///
    /// While it is, nothing drags the screen back: the cursor being off
    /// screen is what the reader asked for. The next cursor move clears it
    /// and brings the screen back, centred.
    detached: bool,
    viewport: Viewport,
}

impl Buffer {
    /// Reads a file from disk.
    pub fn open(path: &Path) -> Result<Self> {
        let contents =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let text = Text::from_string(&contents);
        let syntax =
            LanguageId::for_path(path).and_then(|language| SyntaxState::new(language, &text));
        // Made absolute here, once. A relative path is what a reader types,
        // and it is the wrong thing to keep: the protocol needs an absolute
        // URI, the watcher needs a directory, and deciding whether a file is
        // inside the root is a comparison a relative path fails silently.
        //
        // Absolute rather than canonical: resolving symlinks would report the
        // file under a name the reader did not use.
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());

        Ok(Self {
            path,
            text,
            syntax,
            stale: false,
            version: 1,
            cursor: Cursor {
                line: LineNumber::new(0),
                column: CharColumn::new(0),
                remembered_cell: DisplayColumn::new(0),
            },
            detached: false,
            viewport: Viewport {
                top: LineNumber::new(0),
                top_row: 0,
            },
        })
    }

    /// Where the document came from.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The document's text.
    #[must_use]
    pub const fn text(&self) -> &Text {
        &self.text
    }

    /// How many times this document has changed.
    #[must_use]
    pub const fn version(&self) -> i32 {
        self.version
    }

    /// Whether the file could not be re-read the last time obelus tried.
    #[must_use]
    pub const fn is_stale(&self) -> bool {
        self.stale
    }

    /// Which language this is, if obelus knows it.
    ///
    /// Taken from the parse rather than from the path a second time, so the
    /// server and the highlighting can never disagree about what a file is.
    #[must_use]
    pub fn language(&self) -> Option<LanguageId> {
        self.syntax.as_ref().map(SyntaxState::language)
    }

    /// The parse, if this is a language obelus knows.
    #[must_use]
    pub const fn syntax(&self) -> Option<&SyntaxState> {
        self.syntax.as_ref()
    }

    /// Re-reads the file and reparses the part that changed.
    ///
    /// Returns whether anything changed. A watcher fires for `touch`, for a
    /// rename, and for a permission change, none of which alter a byte;
    /// comparing first costs one pass over the rope and saves a reparse and a
    /// cursor clamp.
    ///
    /// The cursor and the viewport are kept: an agent rewriting the file
    /// should not send the reader back to line one.
    pub fn reload(&mut self) -> Result<bool> {
        let contents = match std::fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(error) => {
                self.stale = true;
                return Err(error).with_context(|| format!("re-reading {}", self.path.display()));
            }
        };
        // The file read, so whatever was wrong with it no longer is.
        self.stale = false;
        let new = Text::from_string(&contents);
        if new.rope() == self.text.rope() {
            return Ok(false);
        }

        let edit = parse::edit_between(&self.text, &new);
        self.text = new;
        self.version = self.version.saturating_add(1);

        match (self.syntax.as_mut(), edit) {
            (Some(state), Some(edit)) => state.reparse(&self.text, &edit),
            // Nothing shared to reuse, so start over. Reached only if the
            // trimming above found no common region at all.
            (Some(state), None) => {
                let language = state.language();
                self.syntax = SyntaxState::new(language, &self.text);
            }
            (None, _) => {}
        }

        self.cursor.line = self.text.clamp_line(self.cursor.line);
        self.cursor.column = self.text.clamp_column(self.cursor.line, self.cursor.column);
        // `top_row` is clamped by `scroll_into_view`, which knows the width.
        self.viewport.top = self.text.clamp_line(self.viewport.top);
        Ok(true)
    }

    /// Where the cursor is.
    #[must_use]
    pub const fn cursor(&self) -> Cursor {
        self.cursor
    }

    /// What part of the document is on screen.
    #[must_use]
    pub const fn viewport(&self) -> Viewport {
        self.viewport
    }

    /// Puts the cursor somewhere outright, clamped into the document.
    ///
    /// For arriving rather than for moving: a place a language server named,
    /// or one the history remembered.
    pub fn place_cursor(&mut self, line: LineNumber, column: CharColumn) {
        // Being put somewhere is arriving, and arriving ends a detour: every
        // caller of this follows it by saying where the screen should be.
        self.detached = false;
        self.cursor.line = self.text.clamp_line(line);
        self.cursor.column = self.text.clamp_column(self.cursor.line, column);
        // The remembered cell is recomputed on the next vertical move, which
        // needs the width; leaving it is right, arriving is not a move along
        // a line.
    }

    /// Moves the viewport by whole screenfuls, leaving the cursor alone.
    ///
    /// Paging is *reading*, not moving: a reader looking further down a file
    /// has not chosen a new place to be, and dragging the cursor along would
    /// throw away the place they came from. The viewport stays where it is
    /// put until the cursor moves, and then it comes back to it -- see
    /// [`Buffer::move_cursor`].
    pub fn page(&mut self, pages: isize, area: TextArea) {
        let rows = isize::try_from(area.height.max(1)).unwrap_or(isize::MAX);
        self.scroll_by(pages.saturating_mul(rows), area);
    }

    /// Moves the viewport by rows, leaving the cursor alone.
    ///
    /// What the wheel does. Same rule as [`Buffer::page`], of which it is
    /// the general case: the view moves, the place the reader chose does
    /// not, and the next cursor move brings the screen back to it.
    pub fn scroll_by(&mut self, rows: isize, area: TextArea) {
        self.scroll_rows(rows, area);

        // The last screenful is as far down as it goes. `scroll_rows` stops
        // at the last *line*, which for a reader means a page too far: a
        // screen holding one line of text and ten of nothing, with nothing
        // saying which way is back.
        let width = area.width.max(1);
        let last = self.text.last_line();
        let last_row = self.text.row_count(last, width).saturating_sub(1);
        let back = isize::try_from(usize::from(area.height.max(1)) - 1).unwrap_or(isize::MAX);
        let limit = self.step_rows(last, last_row, -back, width);
        if (self.viewport.top, self.viewport.top_row) > limit {
            self.viewport.top = limit.0;
            self.viewport.top_row = limit.1;
        }
        self.detached = true;
    }

    /// Whether the viewport has been paged away from the cursor.
    #[must_use]
    pub const fn is_detached(&self) -> bool {
        self.detached
    }

    /// Moves the cursor.
    ///
    /// Up and down step one *visual* row, not one line. With wrapping on, a
    /// long line is many rows tall, and stepping over all of them at once is
    /// not what pressing down once looks like it should do.
    ///
    /// After paging, the first move brings the screen back to the cursor and
    /// centres it. Scrolling the least amount instead would drop the reader
    /// at the top or bottom edge of a screen they had left, which is the
    /// worst of both places.
    pub fn move_cursor(&mut self, motion: Motion, area: TextArea) {
        if self.detached {
            self.detached = false;
            self.move_cursor(motion, area);
            self.center_on_cursor(area);
            return;
        }
        let width = area.width.max(1);
        let (row, _) = self
            .text
            .visual_position(self.cursor.line, self.cursor.column, width);

        let rows = match motion {
            Motion::Up => -1,
            Motion::Down => 1,
            Motion::Left => {
                self.cursor.column = self.cursor.column.saturating_sub(1);
                self.remember(width);
                return;
            }
            Motion::Right => {
                self.cursor.column = self
                    .text
                    .clamp_column(self.cursor.line, self.cursor.column.saturating_add(1));
                self.remember(width);
                return;
            }
            Motion::LineStart => {
                self.cursor.column = CharColumn::new(0);
                self.remember(width);
                return;
            }
            // Both land on column zero rather than one keeping the column and
            // the other not. Symmetry is worth more here than either
            // convention: the last line of a file that ends in a newline is
            // empty, so its start and its end are the same place anyway.
            Motion::DocumentStart => {
                self.cursor.line = LineNumber::new(0);
                self.cursor.column = CharColumn::new(0);
                self.remember(width);
                return;
            }
            Motion::DocumentEnd => {
                self.cursor.line = self.text.last_line();
                self.cursor.column = CharColumn::new(0);
                self.remember(width);
                return;
            }
            // Past the last character, where a cursor legitimately sits. The
            // end of the line, not of the visual row: a line is what the key
            // is named after.
            Motion::LineEnd => {
                self.cursor.column = self.text.line_length(self.cursor.line);
                self.remember(width);
                return;
            }
        };

        let (line, row) = self.step_rows(self.cursor.line, row, rows, width);
        self.cursor.line = line;
        // Aim for the remembered cell, then take whatever column covers it on
        // the row arrived at.
        self.cursor.column = self
            .text
            .column_in_row(line, row, self.cursor.remembered_cell, width);
    }

    /// Records the cell the cursor is at, as the column to aim for later.
    fn remember(&mut self, width: u16) {
        let (_, cell) = self
            .text
            .visual_position(self.cursor.line, self.cursor.column, width);
        self.cursor.remembered_cell = cell;
    }

    /// The visual row `rows` away, crossing line boundaries and stopping at
    /// either end of the document.
    fn step_rows(
        &self,
        mut line: LineNumber,
        mut row: usize,
        rows: isize,
        width: u16,
    ) -> (LineNumber, usize) {
        let last = self.text.last_line();
        for _ in 0..rows.unsigned_abs() {
            if rows > 0 {
                if row + 1 < self.text.row_count(line, width) {
                    row += 1;
                } else if line < last {
                    line = line.saturating_add(1);
                    row = 0;
                } else {
                    break;
                }
            } else if row > 0 {
                row -= 1;
            } else if line.get() > 0 {
                line = line.saturating_sub(1);
                row = self.text.row_count(line, width).saturating_sub(1);
            } else {
                break;
            }
        }
        (line, row)
    }

    /// Where the cursor sits on screen, as a row and a cell within the text
    /// area.
    ///
    /// `None` when it is not on screen, which after
    /// [`Buffer::scroll_into_view`] means the text area has no room at all.
    #[must_use]
    pub fn cursor_screen_cell(&self, area: TextArea) -> Option<(u16, u16)> {
        let width = area.width.max(1);
        let (cursor_row, cell) =
            self.text
                .visual_position(self.cursor.line, self.cursor.column, width);
        let cursor = (self.cursor.line, cursor_row);

        let mut at = (self.viewport.top, self.viewport.top_row);
        for row in 0..area.height {
            if at == cursor {
                return Some((row, cell.get()));
            }
            let next = self.step_rows(at.0, at.1, 1, width);
            if next == at {
                return None;
            }
            at = next;
        }
        None
    }

    /// Puts the cursor's row in the middle of the text area.
    ///
    /// For arriving somewhere rather than scrolling to it. Scrolling the
    /// least amount is right for a cursor the reader is moving, but it leaves
    /// a jumped-to definition on the bottom row with all of its context off
    /// the top -- and the context above a definition is the half you came for.
    ///
    /// Near the top of a file the row simply stays where it is: stepping back
    /// stops at the first row, so the screen is not padded with blank rows to
    /// put line one in the middle.
    pub fn center_on_cursor(&mut self, area: TextArea) {
        let width = area.width.max(1);
        let (cursor_row, _) =
            self.text
                .visual_position(self.cursor.line, self.cursor.column, width);
        let above = isize::try_from(area.height / 2).unwrap_or(isize::MAX);
        let (top, top_row) = self.step_rows(self.cursor.line, cursor_row, -above, width);
        self.viewport.top = top;
        self.viewport.top_row = top_row;
    }

    /// Moves the viewport by whole visual rows, leaving the cursor where it
    /// is, and answers how many rows it actually moved.
    ///
    /// The answer is the point. It stops at the ends of the document, so a
    /// caller keeping a scroll offset can store what came back and never
    /// accumulate rows that do not exist: without that, pressing page-up ten
    /// times at the top of a file means pressing page-down ten times before
    /// anything moves.
    pub fn scroll_rows(&mut self, rows: isize, area: TextArea) -> isize {
        let width = area.width.max(1);
        let step = if rows > 0 { 1 } else { -1 };
        let mut at = (self.viewport.top, self.viewport.top_row);
        let mut moved = 0;
        for _ in 0..rows.unsigned_abs() {
            let next = self.step_rows(at.0, at.1, step, width);
            if next == at {
                break;
            }
            at = next;
            moved += step;
        }
        self.viewport.top = at.0;
        self.viewport.top_row = at.1;
        moved
    }

    /// Scrolls the least amount that brings the cursor on screen, unless the
    /// reader has paged away on purpose.
    pub fn scroll_into_view(&mut self, area: TextArea) {
        let width = area.width.max(1);
        let height = usize::from(area.height).max(1);

        // A reload or a resize can leave the anchor past the end of its line.
        self.viewport.top = self.text.clamp_line(self.viewport.top);
        self.viewport.top_row = self.viewport.top_row.min(
            self.text
                .row_count(self.viewport.top, width)
                .saturating_sub(1),
        );

        // Paged away deliberately: the cursor being off screen is the point,
        // and dragging the view back to it every frame would undo the page
        // before it was drawn.
        if self.detached {
            return;
        }

        let (cursor_row, _) =
            self.text
                .visual_position(self.cursor.line, self.cursor.column, width);
        let cursor = (self.cursor.line, cursor_row);
        let mut at = (self.viewport.top, self.viewport.top_row);

        if cursor < at {
            self.viewport.top = self.cursor.line;
            self.viewport.top_row = cursor_row;
            return;
        }

        // Count rows forward from the anchor. Bounded by the height: past
        // that the cursor is below the window wherever exactly it is, and the
        // answer is the same either way.
        for _ in 0..height {
            if at == cursor {
                return;
            }
            let next = self.step_rows(at.0, at.1, 1, width);
            if next == at {
                // The end of the document, so the cursor is on screen.
                return;
            }
            at = next;
        }

        let back = isize::try_from(height - 1).unwrap_or(isize::MAX);
        let (top, top_row) = self.step_rows(cursor.0, cursor.1, -back, width);
        self.viewport.top = top;
        self.viewport.top_row = top_row;
    }
}
