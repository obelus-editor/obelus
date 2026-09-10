//! Where the cursor is, and which part of the file is on screen.
//!
//! Half of what a buffer knows, and the half with the arithmetic in it: a
//! cursor is a place in the *text* while a screen is a grid of cells, and
//! all of this is about keeping the two agreeing -- while lines wrap or do
//! not, and while the reader moves through them.

use super::*;

impl Buffer {
    /// Where the cursor is.
    #[must_use]
    pub const fn cursor(&self) -> Cursor {
        self.cursor
    }

    /// Forgets the anchor, so nothing is selected.
    pub const fn clear_selection(&mut self) {
        self.selection_anchor = None;
    }

    /// The selected characters, if the cursor has moved away from its anchor.
    #[must_use]
    pub fn selection(&self) -> Option<Span> {
        let anchor = self.selection_anchor?;
        let (start, end) = if (anchor.line, anchor.column) <= (self.cursor.line, self.cursor.column)
        {
            (anchor, self.cursor)
        } else {
            (self.cursor, anchor)
        };
        ((start.line, start.column) != (end.line, end.column)).then_some(Span {
            line: start.line,
            column: start.column,
            end_line: end.line,
            end_column: end.column,
        })
    }

    /// The text the reader selected, if any.
    #[must_use]
    pub fn selected_text(&self) -> Option<String> {
        self.selection()
            .map(|selection| self.text.text_in(selection))
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
        self.selection_anchor = None;
        self.cursor.line = self.text.clamp_line(line);
        self.cursor.column = self.text.clamp_column(self.cursor.line, column);
        // The remembered cell is recomputed on the next vertical move, which
        // needs the width; leaving it is right, arriving is not a move along
        // a line.
    }

    /// Moves the viewport by whole screenfuls, and the cursor with it.
    ///
    /// The cursor keeps its *place on the screen*: whichever row of the
    /// window it was on, it is on after the page too. Reading on is still
    /// reading -- the reader has not chosen a new place -- but a cursor left
    /// behind means the next arrow key throws the page away, and a cursor
    /// dropped at the top of the new screen loses where on the page you
    /// were.
    ///
    /// Unlike the wheel, which is a glance: that leaves the cursor where it
    /// is and the screen comes back on the next move. See
    /// [`Buffer::scroll_by`].
    pub fn page(&mut self, pages: isize, area: TextArea) {
        self.page_with_selection(pages, area, false);
    }

    /// Moves the viewport by whole screenfuls and extends the selection.
    pub fn extend_selection_by_page(&mut self, pages: isize, area: TextArea) {
        self.page_with_selection(pages, area, true);
    }

    fn page_with_selection(&mut self, pages: isize, area: TextArea, extend_selection: bool) {
        if extend_selection {
            self.selection_anchor.get_or_insert(self.cursor);
        } else {
            self.selection_anchor = None;
        }
        let width = area.wrap_width();
        // Where on the screen the cursor is now, which is what has to hold.
        // Off screen -- after a wheel scroll -- counts as the top row: the
        // reader has no visible place for it to keep.
        let screen_row = self
            .cursor_screen_cell(area)
            .map_or(0, |(row, _)| isize::try_from(row).unwrap_or(0));

        let rows = isize::try_from(area.height.max(1)).unwrap_or(isize::MAX);
        self.move_viewport(pages.saturating_mul(rows), area);

        let (line, row) =
            self.step_rows(self.viewport.top, self.viewport.top_row, screen_row, width);
        self.cursor.line = line;
        // The remembered cell, like a vertical move: paging is one.
        self.cursor.column = self
            .text
            .column_in_row(line, row, self.cursor.remembered_cell, width);
        // On screen again, wherever the wheel had left it.
        self.detached = false;
    }

    /// Moves the window over a rendering, which has rows and nothing else.
    ///
    /// The viewport's top line is read as a row index while a mode other
    /// than [`Mode::Edit`] is on. Reusing it rather than adding a second
    /// offset keeps one answer to "where is this buffer scrolled to", which
    /// is what the status bar and the scrollbar both ask.
    pub fn scroll_rendering(&mut self, rows: isize, total: usize) {
        let last = total.saturating_sub(1);
        let top = self.viewport.top.get();
        let moved = if rows >= 0 {
            top.saturating_add(rows.unsigned_abs())
        } else {
            top.saturating_sub(rows.unsigned_abs())
        };
        self.viewport.top = LineNumber::new(moved.min(last));
        self.viewport.top_row = 0;
    }

    /// Moves the viewport by rows, leaving the cursor where it is.
    ///
    /// What the wheel does, and it is a glance rather than a move: the place
    /// the reader chose stays chosen, the cursor may go off screen, and the
    /// next cursor move brings the screen back to it. Paging is the other
    /// thing -- see [`Buffer::page`], which takes the cursor along.
    pub fn scroll_by(&mut self, rows: isize, area: TextArea) {
        self.move_viewport(rows, area);
        self.detached = true;
    }

    /// The viewport arithmetic both of them share.
    fn move_viewport(&mut self, rows: isize, area: TextArea) {
        self.scroll_rows(rows, area);

        // The last screenful is as far down as it goes. `scroll_rows` stops
        // at the last *line*, which for a reader means a page too far: a
        // screen holding one line of text and ten of nothing, with nothing
        // saying which way is back.
        let width = area.wrap_width();
        let last = self.text.last_line();
        let last_row = self.text.row_count(last, width).saturating_sub(1);
        let back = isize::try_from(usize::from(area.height.max(1)) - 1).unwrap_or(isize::MAX);
        let limit = self.step_rows(last, last_row, -back, width);
        if (self.viewport.top, self.viewport.top_row) > limit {
            self.viewport.top = limit.0;
            self.viewport.top_row = limit.1;
        }
    }

    /// Whether the viewport has been scrolled away from the cursor.
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
    /// After a wheel scroll, the first move brings the screen back to the
    /// cursor and centres it. Scrolling the least amount instead would drop
    /// the reader at the top or bottom edge of a screen they had left, which
    /// is the worst of both places.
    pub fn move_cursor(&mut self, motion: Motion, area: TextArea) {
        self.move_cursor_with_selection(motion, area, false);
    }

    /// Moves the cursor and extends the selection from its original position.
    pub fn extend_selection(&mut self, motion: Motion, area: TextArea) {
        self.move_cursor_with_selection(motion, area, true);
    }

    fn move_cursor_with_selection(
        &mut self,
        motion: Motion,
        area: TextArea,
        extend_selection: bool,
    ) {
        if extend_selection {
            self.selection_anchor.get_or_insert(self.cursor);
        } else {
            self.selection_anchor = None;
        }
        if self.detached {
            self.detached = false;
            self.move_cursor_with_selection(motion, area, extend_selection);
            self.center_on_cursor(area);
            return;
        }
        let width = area.wrap_width();
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
        let width = area.wrap_width();
        let (cursor_row, cell) =
            self.text
                .visual_position(self.cursor.line, self.cursor.column, width);
        let cursor = (self.cursor.line, cursor_row);

        // The cell the cursor is in, counted from the left edge of what is
        // on screen rather than from the start of the line. With wrapping
        // off a long line is scrolled sideways, and the cursor's own cell
        // is a cell of the *line*: on a line scrolled by forty cells the
        // caret was drawn forty cells to the right of the character it is
        // on, or -- past the edge -- not drawn at all.
        let Ok(left) = u16::try_from(self.viewport.left) else {
            return None;
        };
        let cell = cell.get().checked_sub(left)?;
        if cell >= area.width {
            return None;
        }

        let mut at = (self.viewport.top, self.viewport.top_row);
        for row in 0..area.height {
            if at == cursor {
                return Some((row, cell));
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
        let width = area.wrap_width();
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
        let width = area.wrap_width();
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

    /// Follows the cursor along a line that is not wrapped.
    ///
    /// The least that puts it back on screen, the same rule the vertical
    /// window follows: one cell at an edge rather than a leap that loses the
    /// reader's place. Zero while lines wrap, because then there is nothing
    /// off to the side.
    fn scroll_sideways(&mut self, area: TextArea) {
        if area.wrap {
            self.viewport.left = 0;
            return;
        }
        let width = usize::from(area.width).max(1);
        let (_, cell) =
            self.text
                .visual_position(self.cursor.line, self.cursor.column, area.wrap_width());
        let cell = usize::from(cell.get());
        if cell < self.viewport.left {
            self.viewport.left = cell;
        } else if cell >= self.viewport.left + width {
            self.viewport.left = cell + 1 - width;
        }
    }

    /// Scrolls the least amount that brings the cursor on screen, unless the
    /// reader has paged away on purpose.
    pub fn scroll_into_view(&mut self, area: TextArea) {
        let width = area.wrap_width();
        let height = usize::from(area.height).max(1);
        // Sideways first and unconditionally: it is about the cursor's
        // column, which the vertical window has no opinion about.
        self.scroll_sideways(area);

        // A reload or a resize can leave the anchor past the end of its line.
        self.viewport.top = self.text.clamp_line(self.viewport.top);
        self.viewport.top_row = self.viewport.top_row.min(
            self.text
                .row_count(self.viewport.top, width)
                .saturating_sub(1),
        );

        // Scrolled away deliberately: the cursor being off screen is the
        // point, and dragging the view back to it every frame would undo the
        // scroll before it was drawn.
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
