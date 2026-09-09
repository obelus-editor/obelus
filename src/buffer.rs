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
    /// One screenful up.
    PageUp,
    /// One screenful down.
    PageDown,
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
    cursor: Cursor,
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
        Ok(Self {
            path: path.to_path_buf(),
            text,
            syntax,
            stale: false,
            cursor: Cursor {
                line: LineNumber::new(0),
                column: CharColumn::new(0),
                remembered_cell: DisplayColumn::new(0),
            },
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

    /// Whether the file could not be re-read the last time obelus tried.
    #[must_use]
    pub const fn is_stale(&self) -> bool {
        self.stale
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

    /// Moves the cursor.
    ///
    /// Up and down step one *visual* row, not one line. With wrapping on, a
    /// long line is many rows tall, and stepping over all of them at once is
    /// not what pressing down once looks like it should do.
    pub fn move_cursor(&mut self, motion: Motion, area: TextArea) {
        let width = area.width.max(1);
        let page = isize::try_from(area.height.max(1)).unwrap_or(isize::MAX);
        let (row, _) = self
            .text
            .visual_position(self.cursor.line, self.cursor.column, width);

        let rows = match motion {
            Motion::Up => -1,
            Motion::Down => 1,
            Motion::PageUp => -page,
            Motion::PageDown => page,
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

    /// Scrolls the least amount that brings the cursor on screen.
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
