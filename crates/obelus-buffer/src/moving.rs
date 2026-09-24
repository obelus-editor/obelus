//! Where the cursor is, and which part of the file is on screen.
//!
//! Half of what a buffer knows, and the half with the arithmetic in it: a
//! cursor is a place in the *text* while a screen is a grid of cells, and
//! all of this is about keeping the two agreeing -- while lines wrap or do
//! not, and while the reader moves through them.

use obelus_editing::{end_of, wordish};
use obelus_text::coordinates::ByteOffset;

use super::*;

impl Buffer {
    /// Where the cursor is.
    #[must_use]
    pub const fn cursor(&self) -> Cursor {
        self.editing.cursor()
    }

    /// Forgets the anchor, so nothing is selected.
    ///
    /// Either anchor: a selection made inside an opened hunk is a
    /// selection, and the key that gives up on one has to reach it.
    pub fn clear_selection(&mut self) {
        self.editing.clear_selection();
        if let Some(above) = self.in_block
            && let Some(block) = self.block_above_mut(above)
        {
            block.editing.clear_selection();
        }
    }

    /// Selects the whole file: the anchor at the first character, the
    /// cursor at the last.
    ///
    /// The cursor goes to the end rather than staying where it was, because
    /// the cursor is one end of a selection here -- and the end is where a
    /// reader who has just taken all of it is looking.
    pub fn select_all(&mut self) {
        // Through the ordinary way of arriving somewhere, which clamps into
        // the document. The anchor is taken after both arrivals because
        // arriving lets go of one: the first call puts the cursor on the
        // first character, the second takes it to the last, and the anchor
        // is then put back where the first left it.
        let last = self.editing.text().last_line();
        let end = self.editing.text().line_length(last);
        self.place_cursor(LineNumber::new(0), CharColumn::new(0));
        let anchor = self.editing.cursor();
        self.place_cursor(last, end);
        self.editing.hold_from(anchor);
    }

    /// The selected characters, if the cursor has moved away from its anchor.
    #[must_use]
    pub fn selection(&self) -> Option<Span> {
        self.editing.selection()
    }

    /// Puts the caret where a pointer landed, or drags the selection out
    /// to it.
    ///
    /// `row` and `cell` are counted from the top left of the *text*, which
    /// is what the caller has after taking off everything the view draws
    /// in front of it. The walk from the top of the viewport is the same
    /// one the caret's own screen position is worked out by, run the other
    /// way: any other arithmetic would be a second answer to where a row
    /// is, and the two would disagree over a fold or an opened hunk.
    pub fn place_at_cell(&mut self, row: u16, cell: u16, area: TextArea, extend: bool) {
        let width = area.wrap_width();
        let at = self.step_screen_rows(
            (self.viewport.top, self.viewport.top_row),
            isize::try_from(row).unwrap_or(0),
            area,
        );
        // Whatever is off the left-hand edge is part of the line as much as
        // what is on screen.
        let cell = DisplayColumn::new(
            cell.saturating_add(u16::try_from(self.viewport.left).unwrap_or(u16::MAX)),
        );

        // A row an opened hunk drew, which is text the reader can see and
        // put a caret in -- it is simply not text of the file.
        let Some((line, row)) = self.text_row_at(at, area) else {
            let above = at.0;
            let Some(block) = self.block_above_mut(above) else {
                return;
            };
            let (line, row) = block.place_at_row(at.1, width);
            let column = block.text().column_in_row(line, row, cell, width);
            block.editing.arrive(line, column);
            block.editing.cursor_mut().remembered_cell = cell;
            // Held after arriving, not before: a drag that starts here is
            // holding nothing until it moves, and one already under way
            // keeps the end it started from.
            match extend {
                true => block.editing.hold(),
                false => block.editing.clear_selection(),
            }
            self.in_block = Some(above);
            self.editing.clear_selection();
            return;
        };
        let column = self.editing.text().column_in_row(line, row, cell, width);
        match extend {
            true => self.extend_to(line, column),
            false => {
                self.place_cursor(line, column);
                self.editing.cursor_mut().remembered_cell = cell;
            }
        }
    }

    /// The place in the document a screen cell is over.
    ///
    /// What [`Buffer::place_at_cell`] works out before it moves anything,
    /// for the caller that only wants to know: the pointer resting over a
    /// word asks what the word is, and asking must not move the caret.
    #[must_use]
    pub fn place_of_cell(
        &self,
        row: u16,
        cell: u16,
        area: TextArea,
    ) -> Option<(LineNumber, CharColumn)> {
        let width = area.wrap_width();
        let at = self.step_screen_rows(
            (self.viewport.top, self.viewport.top_row),
            isize::try_from(row).unwrap_or(0),
            area,
        );
        let cell = DisplayColumn::new(
            cell.saturating_add(u16::try_from(self.viewport.left).unwrap_or(u16::MAX)),
        );
        // A row an opened hunk drew is a commit's version of those lines,
        // which is not a place in this document.
        let (line, row) = self.text_row_at(at, area)?;
        Some((line, self.text().column_in_row(line, row, cell, width)))
    }

    /// Moves the caret without letting go of what is selected.
    ///
    /// What a pointer dragged across the text means, and what a click with
    /// shift held means: the place the reader started from stays put.
    pub fn extend_to(&mut self, line: LineNumber, column: CharColumn) {
        self.editing.hold();
        self.editing.arrive(line, column);
        self.detached = false;
        self.in_block = None;
    }

    /// Selects a whole line, the break at the end of it included.
    ///
    /// With the break, so that what comes out of the clipboard is a line
    /// rather than the middle of one -- the same rule `copy` follows when
    /// nothing is selected.
    pub fn select_line(&mut self, line: LineNumber) {
        let last = self.editing.text().last_line();
        let (end_line, end_column) = match line >= last {
            true => (line, self.editing.text().line_length(line)),
            false => (line.saturating_add(1), CharColumn::new(0)),
        };
        self.select(Span {
            line,
            column: CharColumn::new(0),
            end_line,
            end_column,
        });
    }

    /// Widens what is selected by one step, and says whether it could.
    ///
    /// With nothing selected, the word the caret is in -- which is the
    /// step every reader wants first and the only one they want often.
    /// After that, the smallest thing in the *tree* that holds what is
    /// already selected: the argument, then the call, then the statement,
    /// then the block. A grammar is the only thing that knows where those
    /// begin and end, and Obelus has one for the file already.
    ///
    /// Where there is no grammar, the steps are the line and then the
    /// file. Two coarse steps are worth having: a reader with a text file
    /// open still wants to take a line without reaching for `home` and
    /// `shift+end`.
    pub fn widen_selection(&mut self) -> bool {
        let Some(span) = self.selection() else {
            let Some(word) = self.word_span() else {
                return false;
            };
            self.select(word);
            return true;
        };
        let Some(wider) = self.wider_than(span) else {
            return false;
        };
        self.select(wider);
        true
    }

    /// The word the caret is in, or the one it is at the end of.
    ///
    /// Both, because a caret sits *between* characters: with `word|` a
    /// reader means that word, and so they do with `|word` and `wo|rd`.
    fn word_span(&self) -> Option<Span> {
        let cursor = self.editing.cursor();
        let characters: Vec<char> = self.editing.text().line(cursor.line).chars().collect();
        let at = cursor.column.get().min(characters.len());
        let inside = |at: usize| characters.get(at).copied().is_some_and(wordish);
        let start = match inside(at) || (at > 0 && inside(at - 1)) {
            true => at,
            false => return None,
        };
        let mut from = start;
        while from > 0 && inside(from - 1) {
            from -= 1;
        }
        let mut to = start;
        while inside(to) {
            to += 1;
        }
        (from < to).then_some(Span {
            line: cursor.line,
            column: CharColumn::new(from),
            end_line: cursor.line,
            end_column: CharColumn::new(to),
        })
    }

    /// The smallest span that holds `span` and is bigger than it.
    fn wider_than(&self, span: Span) -> Option<Span> {
        let text = self.editing.text();
        let from = text.byte_of_char(text.char_offset(span.line, span.column));
        let to = text.byte_of_char(text.char_offset(span.end_line, span.end_column));
        if let Some(state) = self.syntax.as_ref() {
            let mut node = state
                .tree()
                .root_node()
                .descendant_for_byte_range(from.get(), to.get())?;
            // Up until something is really bigger: a node whose only child
            // is the selection has the same bytes, and climbing to it would
            // be a key press that changed nothing.
            loop {
                if node.start_byte() < from.get() || node.end_byte() > to.get() {
                    let text = self.editing.text();
                    let (line, column) = text.position(text.char_of_byte(
                        obelus_text::coordinates::ByteOffset::new(node.start_byte()),
                    ));
                    let (end_line, end_column) =
                        text.position(text.char_of_byte(
                            obelus_text::coordinates::ByteOffset::new(node.end_byte()),
                        ));
                    return Some(Span {
                        line,
                        column,
                        end_line,
                        end_column,
                    });
                }
                node = node.parent()?;
            }
        }

        // No grammar: the line, and then everything.
        let whole = self.spanning_all();
        let line = Span {
            line: span.line,
            column: CharColumn::new(0),
            end_line: span.line,
            end_column: self.editing.text().line_length(span.line),
        };
        match span == line || span.line != span.end_line {
            true => (span != whole).then_some(whole),
            false => Some(line),
        }
    }

    /// The text the reader selected, if any.
    ///
    /// From whichever of the two they selected it in: the file, or the
    /// lines an opened hunk is showing. What a reader can see and put a
    /// caret in is what they can take a copy of.
    #[must_use]
    pub fn selected_text(&self) -> Option<String> {
        if let Some((above, selection)) = self.block_selection()
            && let Some(block) = self.block_above(above)
        {
            return Some(block.text().text_in(selection));
        }
        self.selection()
            .map(|selection| self.editing.text().text_in(selection))
    }

    /// Whether anything is selected, in the file or in an opened block.
    #[must_use]
    pub fn has_selection(&self) -> bool {
        self.selection().is_some() || self.block_selection().is_some()
    }

    /// The byte range the viewport covers.
    ///
    /// Whole lines, so a highlight that starts just off the top edge still
    /// reaches the first visible row.
    pub fn visible_bytes(&self, height: u16) -> std::ops::Range<ByteOffset> {
        let text = self.text();
        let folds = self.folds();
        let top = self.viewport().top;
        // Walked the way the view walks it, past whatever is folded away. A
        // count of `height` *file* lines is the same thing only while nothing
        // is folded: with a run of two hundred lines closed at the top of the
        // screen, the rows below it are lines two hundred further down, and a
        // range that stopped at `top + height` would leave every one of them
        // outside what has been highlighted -- which is not a subtle failure.
        // The code below the fold is simply drawn in the plain foreground.
        //
        // A bound rather than an exact answer: a wrapped line takes more than
        // one row, so this can reach further than the screen does. Covering too
        // much costs a little query time and nothing else; covering too little
        // costs the colours.
        let mut line = folds.first_shown(top);
        let mut rows = 0;
        while rows < usize::from(height) && line.get() < text.line_count() {
            rows += 1;
            line = folds.first_shown(line.saturating_add(1));
        }
        let start = text.line_start_byte(top);
        let end = if line.get() >= text.line_count() {
            text.byte_length()
        } else {
            text.line_start_byte(line)
        };
        start..end
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
        // caller of this follows it by saying where the screen should be --
        // and it ends the other detour too, which is having walked into a
        // hunk's removed lines.
        self.detached = false;
        self.in_block = None;
        self.editing.clear_selection();
        // And it opens whatever was hiding the place. A reader who asked to
        // be taken somewhere has said the destination is worth seeing;
        // leaving them on the folded line above it, with the status row
        // naming a line that is not on screen, answers a different request.
        // Walking there is the other thing, and walks around a fold.
        self.folds.reveal(self.editing.text().clamp_line(line));
        self.editing.arrive(line, column);
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
            self.editing.hold();
        } else {
            self.editing.clear_selection();
        }
        let width = area.wrap_width();
        // Where on the screen the cursor is now, which is what has to hold.
        // Off screen -- after a wheel scroll -- counts as the top row: the
        // reader has no visible place for it to keep.
        let screen_row = self
            .cursor_screen_cell(area)
            .map_or(0, |(row, _)| isize::try_from(row).unwrap_or(0));

        let rows = isize::try_from(area.height.max(1)).unwrap_or(isize::MAX);
        let was = (self.viewport.top, self.viewport.top_row);
        self.move_viewport(pages.saturating_mul(rows), area);

        // Where on the new screen to land. The reader's own row -- unless
        // there was no new screen, because the view was already showing the
        // end they asked to go towards. A cursor keeping its row then does
        // not move at all, which makes it a key that does nothing on the
        // last screenful of every file, leaving the lines past the cursor
        // reachable one at a time and no other way.
        //
        // So where the view cannot go on, the cursor goes to the end
        // instead. Which is what paging a list does: the focus moves a
        // screenful and stops at the last row, rather than the window
        // moving and the focus riding along.
        let screen_row = match (
            (self.viewport.top, self.viewport.top_row) == was,
            pages >= 0,
        ) {
            (false, _) => screen_row,
            (true, true) => rows.saturating_sub(1),
            (true, false) => 0,
        };

        let landed =
            self.step_screen_rows((self.viewport.top, self.viewport.top_row), screen_row, area);
        // The row it landed on is one the view drew and the text does not
        // have -- inside a hunk the reader has opened. There is nowhere for
        // the cursor to go that would keep its place on the screen, so it
        // keeps its place in the *file* instead and the page is a scroll:
        // the way through a deletion taller than the screen, and the same
        // thing the wheel does. The next cursor move brings the screen back
        // to it, as it does after any scroll.
        // The row it landed on is one the view drew and the text does not
        // have -- inside a hunk the reader has opened. The caret goes there
        // instead: those rows are where a page through a long deletion
        // lands, and the cursor stays on the line the block belongs to.
        let aim = self.editing.cursor().remembered_cell;
        let Some((line, row)) = self.text_row_at(landed, area) else {
            // Into the block, on the row it landed on and at the cell it
            // was aiming for -- a page keeps the reader's place on the
            // screen, and their column with it.
            if let Some(block) = self.block_above_mut(landed.0) {
                let (line, row) = block.place_at_row(landed.1, width);
                let column = block.text().column_in_row(line, row, aim, width);
                block.editing.clear_selection();
                block.editing.arrive(line, column);
                block.editing.cursor_mut().remembered_cell = aim;
                self.in_block = Some(landed.0);
            }
            // Whatever was being selected was being selected in the file,
            // and the caret has just left it: a selection with one end in
            // the file and the other in lines the file does not have is
            // not a thing either half could be asked about.
            self.editing.clear_selection();
            self.detached = false;
            return;
        };
        self.in_block = None;
        // The remembered cell, like a vertical move: paging is one.
        let column = self.editing.text().column_in_row(line, row, aim, width);
        self.editing.arrive(line, column);
        // On screen again, wherever the wheel had left it.
        self.detached = false;
    }

    /// Moves the window over a rendering, which has rows and nothing else.
    ///
    /// The viewport's top line is read as a row index while a mode other
    /// than [`Mode::Edit`] is on. Reusing it rather than adding a second
    /// offset keeps one answer to "where is this buffer scrolled to", which
    /// is what the status bar and the scrollbar both ask.
    ///
    /// It stops with the last row on screen rather than at the top of it. A
    /// reading has no cursor, so there is nothing to be at the end *of*
    /// except the rows -- and blank rows under the last one are a screen
    /// saying there is more to come when there is not. The text does it the
    /// other way because there the cursor is the thing at the end.
    pub fn scroll_rendering(&mut self, rows: isize, total: usize, height: u16) {
        let last = total.saturating_sub(usize::from(height).max(1));
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

        let limit = self.last_top(area);
        if (self.viewport.top, self.viewport.top_row) > limit {
            self.viewport.top = limit.0;
            self.viewport.top_row = limit.1;
        }
    }

    /// As far down as the viewport goes: the top of the last screenful.
    ///
    /// Scrolling stops at the last *line*, which for a reader is a page too
    /// far: a screen holding one line of text and ten of nothing, with
    /// nothing saying which way is back.
    fn last_top(&self, area: TextArea) -> (LineNumber, usize) {
        let last = self.editing.text().last_line();
        let last_row = self.screen_rows_of(last, area).saturating_sub(1);
        let back = isize::try_from(usize::from(area.height.max(1)) - 1).unwrap_or(isize::MAX);
        self.step_screen_rows((last, last_row), -back, area)
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
        // A left or right arrow on a selection puts the caret at that end
        // of it and lets it go. The reader has a piece of the file in hand
        // and is saying which end of it they mean; stepping one character
        // from wherever the caret happens to be would move away from the
        // end they pointed at half the time, because which end the caret
        // is on depends on which way they selected.
        if !extend_selection
            && self.in_block.is_none()
            && let Some(span) = self.selection()
            && let Some((line, column)) = end_of(span, motion)
        {
            self.place_cursor(line, column);
            self.scroll_into_view(area);
            return;
        }
        // The anchor belongs to whichever place the caret is in. Arming the
        // file's while the reader is selecting inside a block would leave a
        // selection nobody made: from the line the cursor is parked on to
        // wherever it steps out.
        if self.in_block.is_none() {
            if extend_selection {
                self.editing.hold();
            } else {
                self.editing.clear_selection();
            }
        }
        if self.detached {
            self.detached = false;
            self.move_cursor_with_selection(motion, area, extend_selection);
            self.center_on_cursor(area);
            return;
        }
        // Inside an opened hunk, where the caret has its own place to move
        // about: those lines are text, and a reader looking at what a file
        // used to say wants to walk it and take a copy of it.
        if self.in_block.is_some() {
            self.move_in_block(motion, area, extend_selection);
            return;
        }
        // And the step that walks into one. The block sits between the line
        // it was drawn above and the line before that, so it is entered
        // from either side -- going up off the top row of the line below
        // it, or down off the last row of the line above.
        if let Some((above, entering)) = self.entering_block(motion, area) {
            if let Some(block) = self.block_above_mut(above) {
                *block.editing.cursor_mut() = entering;
                block.editing.clear_selection();
            }
            self.in_block = Some(above);
            self.editing.clear_selection();
            return;
        }
        self.editing.step(motion, &self.folds, area.wrap_width());
    }

    /// Which line of an opened block a motion walks into, if it walks into
    /// one.
    ///
    /// Up from the first row of the line the block was drawn above enters
    /// at its last line; down from the last row of the line before it
    /// enters at its first. Every other motion, and every other place,
    /// leaves the block alone.
    fn entering_block(&self, motion: Motion, area: TextArea) -> Option<(LineNumber, Cursor)> {
        // The one hanging above the cursor's own line, going up, and the
        // one hanging above the line below it, going down -- a block sits
        // between two lines and is entered from either side.
        let cursor = self.editing.cursor();
        let below = self.next_shown(cursor.line, true, area);
        let block = match motion {
            Motion::Up => self.block_above(cursor.line),
            Motion::Down => below.and_then(|line| self.block_above(line)),
            _ => None,
        }
        .filter(|block| !block.is_empty())
        // Not into a complaint. The others are rows the reader *opened*,
        // and walking into what you opened is the point of opening it;
        // this one arrived on its own because the caret came to rest on
        // the line above, and stepping through it would charge a keystroke
        // for every line a server has something to say about -- on the one
        // motion the whole thing is built around, which is moving to the
        // line to read it.
        .filter(|block| block.kind != Held::Wrong)?;
        let width = area.wrap_width();
        let (row, _) = self
            .editing
            .text()
            .visual_position(cursor.line, cursor.column, width);
        let entered = match motion {
            Motion::Up if cursor.line == block.above && row == 0 => {
                LineNumber::new(block.text().line_count().saturating_sub(1))
            }
            Motion::Down
                if block.above.get() > 0
                    && cursor.line.get() + 1 == block.above.get()
                    && row + 1 == self.editing.text().row_count(cursor.line, width) =>
            {
                LineNumber::new(0)
            }
            _ => return None,
        };
        // Arriving on a row aims for the cell the reader was in, the same
        // as arriving on any other row: a column of zero would drag the
        // caret to the left edge on the way in and leave it there on the
        // way out. Which row of the line, for a line that wraps: the last
        // going up, the first coming down.
        let row = match motion {
            Motion::Up => block.text().row_count(entered, width).saturating_sub(1),
            _ => 0,
        };
        Some((
            block.above,
            Cursor {
                line: entered,
                column: block
                    .text()
                    .column_in_row(entered, row, cursor.remembered_cell, width),
                remembered_cell: cursor.remembered_cell,
            },
        ))
    }

    /// Moves the caret about inside an opened block.
    ///
    /// The same motions over the block's own text: it is a text, so
    /// wrapping, tabs, wide glyphs and the cell to aim for are all the
    /// ones the file gets, from the same code. What is the block's own is
    /// the two edges -- a move that cannot go further up or down leaves
    /// it, for the line above or the line it was drawn above.
    fn move_in_block(&mut self, motion: Motion, area: TextArea, extend_selection: bool) {
        let width = area.wrap_width();
        let Some(above) = self.in_block else {
            return;
        };
        let Some(block) = self.block_above_mut(above) else {
            return;
        };
        if extend_selection {
            block.editing.hold();
        } else {
            block.editing.clear_selection();
        }
        // Nothing hides a line of a block: it is a few lines the file used
        // to have, not a file, and it has no runs to fold.
        let moved = block.editing.step(motion, &(), width);
        let aim = block.editing.cursor().remembered_cell;
        if moved || !matches!(motion, Motion::Up | Motion::Down) {
            return;
        }

        // It could not move, so it is at one end of the block and on its
        // way out.
        match motion {
            // A block at the top of the file has nothing above it, so the
            // caret stays on its first row. Leaving here would put it on
            // the line the block was drawn above -- *below* where it was
            // -- and the next press would walk back in.
            Motion::Up if above.get() == 0 => {}
            Motion::Up => {
                // The first line above that is on screen, not simply the
                // line before: a run folded away above the hunk would
                // otherwise take the caret with it, onto a line the status
                // bar names and nobody can see.
                let Some(line) = self.next_shown(above, false, area) else {
                    return;
                };
                self.in_block = None;
                let last = self.editing.text().row_count(line, width).saturating_sub(1);
                let column = self.editing.text().column_in_row(line, last, aim, width);
                self.editing.arrive(line, column);
                self.editing.cursor_mut().remembered_cell = aim;
            }
            // Out of the bottom, onto the line the block was drawn above.
            // Said rather than assumed: the cursor is only already there
            // for a reader who walked in from below, and one who walked in
            // from above would be put back where they started -- which,
            // pressed again, walks into the block again and never gets
            // past it.
            _ => {
                self.in_block = None;
                let column = self.editing.text().column_in_row(above, 0, aim, width);
                self.editing.arrive(above, column);
                self.editing.cursor_mut().remembered_cell = aim;
            }
        }
    }

    /// How many rows of the screen a line takes: the rows the view draws
    /// above it, then its own.
    ///
    /// The other count is [`obelus_text::Text::row_count`], which is the
    /// rows the *text* has. Two counts because there are two questions: a
    /// cursor moves through the text, and a viewport is a window on the
    /// screen -- and one function answering both is what let an opened hunk
    /// be drawn where the viewport could not reach it.
    fn screen_rows_of(&self, line: LineNumber, area: TextArea) -> usize {
        // A folded-away line is a line with no rows. Everything that counts
        // rows -- the caret, the paging, the scrolling, the view -- counts
        // with this one function, so folding is that one term going to zero
        // rather than a second set of arithmetic beside the first.
        if self.folds.hides(line) {
            return 0;
        }
        self.rows_above(line, area.wrap_width())
            + self.editing.text().row_count(line, area.wrap_width())
            + self.rows_below(line, area.wrap_width())
    }

    /// The next line with rows of its own, in either direction.
    ///
    /// `None` at the end of what is visible, which is not always the end of
    /// the file: a folded run reaching the last line leaves its own first
    /// line as the last one there is.
    fn next_shown(&self, line: LineNumber, down: bool, area: TextArea) -> Option<LineNumber> {
        let last = self.editing.text().last_line();
        let mut at = line;
        loop {
            if down {
                if at >= last {
                    return None;
                }
                at = at.saturating_add(1);
            } else {
                if at.get() == 0 {
                    return None;
                }
                at = at.saturating_sub(1);
            }
            if self.screen_rows_of(at, area) > 0 {
                return Some(at);
            }
        }
    }

    /// How many rows the view draws above a line, which is a hunk the
    /// reader has opened there and nothing else.
    fn rows_above(&self, line: LineNumber, width: u16) -> usize {
        self.block_above(line).map_or(0, |block| block.rows(width))
    }

    /// How many rows the view draws *below* a line, which is only ever the
    /// block hanging past the end of the file.
    ///
    /// A block belongs to the line it is drawn above, and the row after the
    /// last line belongs to no line at all -- so a hunk that deleted the
    /// end of a file, or a complaint about its last line, hung off a line
    /// nothing counts and nothing drew. Counted here, on the last line,
    /// rather than by teaching every walk about a line past the end:
    /// everything that counts rows counts with `screen_rows_of`, and one
    /// more term in it is how folding is done too.
    fn rows_below(&self, line: LineNumber, width: u16) -> usize {
        if line != self.editing.text().last_line() {
            return 0;
        }
        self.rows_under(line, width)
    }

    /// How many rows hang directly under a line, wherever they are counted.
    ///
    /// The same rows as [`Buffer::rows_above`] of the line below, said from
    /// the other side. Which line *counts* them depends on whether there is
    /// a line below to count them; which line they belong *to* does not,
    /// and this is the question the viewport asks.
    fn rows_under(&self, line: LineNumber, width: u16) -> usize {
        self.block_above(LineNumber::new(line.get() + 1))
            .map_or(0, |block| block.rows(width))
    }

    /// The row of the screen `rows` away, crossing line boundaries and
    /// stopping at either end of the document.
    ///
    /// A position here is a line and a row *within its screen rows*, so a
    /// row below the inserted ones is the line's own first row. Everything
    /// that moves the viewport counts with this.
    fn step_screen_rows(
        &self,
        at: (LineNumber, usize),
        rows: isize,
        area: TextArea,
    ) -> (LineNumber, usize) {
        let (mut line, mut row) = at;
        for _ in 0..rows.unsigned_abs() {
            if rows > 0 {
                if row + 1 < self.screen_rows_of(line, area) {
                    row += 1;
                } else if let Some(next) = self.next_shown(line, true, area) {
                    line = next;
                    row = 0;
                } else {
                    break;
                }
            } else if row > 0 {
                row -= 1;
            } else if let Some(above) = self.next_shown(line, false, area) {
                line = above;
                row = self.screen_rows_of(line, area).saturating_sub(1);
            } else {
                break;
            }
        }
        (line, row)
    }

    /// Where the cursor is, as a row of the screen.
    ///
    /// The rows drawn above its line are between the top of that line and
    /// the cursor, so they count -- which is the whole of what the caret
    /// and the scrolling had wrong.
    fn cursor_screen_row(&self, area: TextArea) -> (LineNumber, usize) {
        // In the block, the caret is on one of the rows the view drew, and
        // those are the first rows of the line they were drawn above.
        let width = area.wrap_width();
        if let Some(above) = self.in_block
            && let Some(block) = self.block_above(above)
        {
            let cursor = block.editing.cursor();
            let (row, _) = block
                .text()
                .visual_position(cursor.line, cursor.column, width);
            return (block.above, block.rows_before(cursor.line, width) + row);
        }
        let cursor = self.editing.cursor();
        let (row, _) = self
            .editing
            .text()
            .visual_position(cursor.line, cursor.column, width);
        (cursor.line, self.rows_above(cursor.line, width) + row)
    }

    /// The place in the *text* a row of the screen is on, or `None` for a
    /// row the view inserted.
    ///
    /// `None` rather than the nearest text row, because the two answers are
    /// what a caller has to tell apart: a cursor cannot be put on a row the
    /// file does not have, and the first row of the block is not the first
    /// row of the line.
    fn text_row_at(&self, at: (LineNumber, usize), area: TextArea) -> Option<(LineNumber, usize)> {
        let above = self.rows_above(at.0, area.wrap_width());
        (at.1 >= above).then(|| (at.0, at.1 - above))
    }

    /// How far along its row the caret is, in cells.
    ///
    /// From whichever text it is in: the file's, or an opened block's --
    /// which wraps at the same width and counts cells the same way, being
    /// a text like any other.
    fn caret_cell(&self, area: TextArea) -> DisplayColumn {
        let width = area.wrap_width();
        match self.in_block.and_then(|above| self.block_above(above)) {
            Some(block) => {
                let cursor = block.editing.cursor();
                block
                    .text()
                    .visual_position(cursor.line, cursor.column, width)
                    .1
            }
            None => {
                let cursor = self.editing.cursor();
                self.editing
                    .text()
                    .visual_position(cursor.line, cursor.column, width)
                    .1
            }
        }
    }

    /// Where a place in the text is on screen, as a row and a cell of the
    /// text area.
    ///
    /// The other direction of [`Buffer::place_of_cell`], and the same
    /// question [`Buffer::cursor_screen_cell`] answers about the caret --
    /// which is its own function because the caret can be somewhere a
    /// place in the text cannot: inside the lines an opened hunk is
    /// showing, which belong to a commit rather than to this document.
    ///
    /// `None` for a place that is not on screen, which is the honest
    /// answer for anything hung off it: there is nowhere to hang it.
    #[must_use]
    pub fn cell_of_place(
        &self,
        line: LineNumber,
        column: CharColumn,
        area: TextArea,
    ) -> Option<(u16, u16)> {
        let width = area.wrap_width();
        let (row_in_line, cell) = self.text().visual_position(line, column, width);
        let left = u16::try_from(self.viewport.left).ok()?;
        let cell = cell.get().checked_sub(left)?;
        if cell >= area.width {
            return None;
        }
        let wanted = (line, self.rows_above(line, width) + row_in_line);
        let mut at = (self.viewport.top, self.viewport.top_row);
        for row in 0..area.height {
            if at == wanted {
                return Some((row, cell));
            }
            let next = self.step_screen_rows(at, 1, area);
            if next == at {
                return None;
            }
            at = next;
        }
        None
    }

    /// Where the cursor sits on screen, as a row and a cell within the text
    /// area.
    ///
    /// `None` when it is not on screen, which after
    /// [`Buffer::scroll_into_view`] means the text area has no room at all.
    #[must_use]
    pub fn cursor_screen_cell(&self, area: TextArea) -> Option<(u16, u16)> {
        let cell = self.caret_cell(area);
        let cursor = self.cursor_screen_row(area);

        // The cell the cursor is in, counted from the left edge of what is
        // on screen rather than from the start of the line. With wrapping
        // off a long line is scrolled sideways, and the cursor's own cell
        // is a cell of the *line*: on a line scrolled by forty cells the
        // caret was drawn forty cells to the right of the character it is
        // on, or -- past the edge -- not drawn at all.
        //
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
            let next = self.step_screen_rows(at, 1, area);
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
        let above = isize::try_from(area.height / 2).unwrap_or(isize::MAX);
        let (top, top_row) = self.step_screen_rows(self.cursor_screen_row(area), -above, area);
        self.viewport.top = top;
        self.viewport.top_row = top_row;
    }

    /// Which row of the screen a line's first row is drawn on, if it is
    /// drawn at all.
    ///
    /// Asked about a line rather than about the caret: a list showing the
    /// reader somewhere must not move the view for somewhere they can
    /// already see, and where it is showing them is not where the caret is.
    ///
    /// The area is the room that is actually *visible*, which for a list
    /// drawn over the foot of the editor is short of what the editor
    /// draws. A line under the list is a line the reader cannot see, and
    /// answering otherwise would leave them looking at nothing.
    ///
    /// The row rather than a yes: whether somewhere counts as shown
    /// depends on how near the edge it is, and only the caller knows how
    /// much room around it the reader needs.
    #[must_use]
    pub fn screen_row_of(&self, line: LineNumber, area: TextArea) -> Option<u16> {
        if line < self.viewport.top {
            return None;
        }
        let mut at = (self.viewport.top, self.viewport.top_row);
        for row in 0..area.height {
            if at.0 == line {
                return Some(row);
            }
            let next = self.step_screen_rows(at, 1, area);
            if next == at {
                return None;
            }
            at = next;
        }
        None
    }

    /// Puts a line in the middle of the text area, leaving the caret where
    /// it is.
    ///
    /// For showing the reader somewhere while they decide whether to go
    /// there. A list that sits on the status bar leaves the file on screen
    /// above it, so what it shows its selection in is the file itself --
    /// and the caret has not moved, because nobody has chosen anything yet.
    ///
    /// Detached for that reason: the view would otherwise be dragged back
    /// to the caret on the very next frame. It is the same thing a wheel
    /// scroll says -- the reader is looking somewhere their caret is not --
    /// said the same way.
    pub fn look_at(&mut self, line: LineNumber, area: TextArea) {
        let line = self.editing.text().clamp_line(line);
        let above = isize::try_from(area.height / 2).unwrap_or(isize::MAX);
        let (top, top_row) = self.step_screen_rows((line, 0), -above, area);
        self.viewport.top = top;
        self.viewport.top_row = top_row;
        self.detached = true;
    }

    /// Puts the view back exactly where a look took it from.
    ///
    /// Exactly, rather than by letting the caret pull it back: the caret is
    /// wherever it was, which may be anywhere on the screen the reader
    /// left, and scrolling the least amount to reach it would land them at
    /// an edge of a screen they had not moved from.
    pub const fn look_back(&mut self, viewport: Viewport) {
        self.viewport = viewport;
        self.detached = false;
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
        let step = if rows > 0 { 1 } else { -1 };
        let mut at = (self.viewport.top, self.viewport.top_row);
        let mut moved = 0;
        for _ in 0..rows.unsigned_abs() {
            let next = self.step_screen_rows(at, step, area);
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
        // Whichever caret is on screen: a reader walking a long line of an
        // opened block is as far along it as a reader walking a long line
        // of the file, and the window has to follow the one they can see.
        let cell = usize::from(self.caret_cell(area).get());
        if cell < self.viewport.left {
            self.viewport.left = cell;
        } else if cell >= self.viewport.left + width {
            self.viewport.left = cell + 1 - width;
        }
    }

    /// Scrolls the least amount that brings the cursor on screen, unless the
    /// reader has paged away on purpose.
    pub fn scroll_into_view(&mut self, area: TextArea) {
        let height = usize::from(area.height).max(1);
        // Sideways first and unconditionally: it is about the cursor's
        // column, which the vertical window has no opinion about.
        self.scroll_sideways(area);

        // A reload or a resize can leave the anchor past the end of its line.
        self.viewport.top = self.editing.text().clamp_line(self.viewport.top);
        self.viewport.top_row = self.viewport.top_row.min(
            self.screen_rows_of(self.viewport.top, area)
                .saturating_sub(1),
        );

        // Scrolled away deliberately: the cursor being off screen is the
        // point, and dragging the view back to it every frame would undo the
        // scroll before it was drawn.
        if self.detached {
            return;
        }

        let cursor = self.cursor_screen_row(area);
        // What hangs directly under the caret's line comes with it. A
        // complaint is opened *because* the caret arrived on that line, so
        // one below the bottom edge is an answer to a question the reader
        // just asked and cannot see -- walking onto a wrong line would
        // look like nothing happened. Capped below the height, because
        // keeping a tall block on screen must not push the line it is
        // about off the top: the line is the thing being read.
        let tail = self
            .rows_under(self.cursor().line, area.wrap_width())
            .min(height - 1);
        let wanted = self.step_screen_rows(cursor, isize::try_from(tail).unwrap_or(0), area);
        let mut at = (self.viewport.top, self.viewport.top_row);

        if cursor < at {
            self.viewport.top = cursor.0;
            self.viewport.top_row = cursor.1;
            return;
        }

        // Count rows forward from the anchor. Bounded by the height: past
        // that the cursor is below the window wherever exactly it is, and the
        // answer is the same either way.
        for _ in 0..height {
            if at == wanted {
                return;
            }
            let next = self.step_screen_rows(at, 1, area);
            if next == at {
                // The end of the document, so the cursor is on screen.
                return;
            }
            at = next;
        }

        let back = isize::try_from(height - 1).unwrap_or(isize::MAX);
        let (top, top_row) = self.step_screen_rows(wanted, -back, area);
        self.viewport.top = top;
        self.viewport.top_row = top_row;
    }
}
