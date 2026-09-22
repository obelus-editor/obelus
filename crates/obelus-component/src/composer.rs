//! The box a message or a note is written in.
//!
//! A few lines of text with a caret in them, which is a smaller thing than a
//! buffer -- no file, no syntax, no history -- and a larger thing than a
//! prompt, because a message to an agent is a paragraph and sometimes a
//! pasted stack trace.
//!
//! What is *in* the box is [`obelus_editing::Editing`], which is what the
//! file being read is too. That is the whole of the point: a reader who has
//! learnt `ctrl+left` in a file finds it walks a word here, and every key
//! added to one is a key both have. It was two implementations once, and
//! this box had the poorer half -- no words, no selection, and its own
//! arithmetic over `Vec<String>` beside the file's over a rope.
//!
//! What is left here is the box's own shape: how wide it lays out, which
//! rows it hands to whoever draws it, and where the caret is in them.

use std::ops::Range;

use crossterm::event::KeyEvent;
use obelus_editing::Editing;
use obelus_text::{
    Text,
    coordinates::{CharColumn, DisplayColumn, LineNumber, Span},
};

/// One row of a box, laid out.
///
/// What it says and which of it the reader has hold of, together, because
/// they are worked out together: a row is what the wrapping decided, and a
/// selection given in the note's own lines and columns would have to be
/// taken apart again by whoever draws it.
#[derive(Clone, Debug, Default)]
pub struct Laid {
    /// What the row says.
    pub said: String,
    /// Which of its characters are selected, counted from the start of the
    /// row rather than of the text.
    pub held: Option<Range<usize>>,
}

/// What is being written.
#[derive(Clone, Debug)]
pub struct Composer {
    /// The text and the caret in it.
    writing: Editing,
}

impl Default for Composer {
    fn default() -> Self {
        Self::new()
    }
}

impl Composer {
    /// An empty box.
    #[must_use]
    pub fn new() -> Self {
        Self {
            writing: Editing::new(""),
        }
    }

    /// What has been written, newlines and all.
    #[must_use]
    pub fn text(&self) -> String {
        self.writing.said()
    }

    /// Whether it says nothing, or nothing but blanks.
    #[must_use]
    pub fn is_blank(&self) -> bool {
        self.writing.is_blank()
    }

    /// What has been written, leaving the box empty.
    pub fn take(&mut self) -> String {
        let said = self.text();
        self.writing = Editing::new("");
        said
    }

    /// Puts text in, with the caret at the end of it.
    pub fn replace(&mut self, text: &str) {
        self.writing.replace(text, u16::MAX);
    }

    /// Whatever a key means to what is being written.
    ///
    /// Nothing about folding: a box has no runs closed away, so every line
    /// it holds is a line the caret may stand on.
    pub fn handle_key(&mut self, key: &KeyEvent, room: u16) -> bool {
        // The newline is the box's own rule, not the text's: bare enter is
        // taken by whoever holds the box -- to send a message, to start
        // another note -- so a line break has to arrive some other way.
        // `shift+enter` is what a reader presses and `alt+enter` is what a
        // terminal that cannot tell shift from nothing sends.
        if key.code == crossterm::event::KeyCode::Enter
            && matches!(
                key.modifiers,
                crossterm::event::KeyModifiers::SHIFT | crossterm::event::KeyModifiers::ALT
            )
        {
            self.newline();
            return true;
        }
        self.writing.handle_key(key, &(), room.max(1))
    }

    /// Types one character.
    pub fn insert(&mut self, character: char) {
        self.writing
            .apply(obelus_editing::Typing::Character(character), &(), u16::MAX);
    }

    /// Breaks the line at the caret.
    pub fn newline(&mut self) {
        self.writing
            .apply(obelus_editing::Typing::Newline, &(), u16::MAX);
    }

    /// Up or down one row, answering `false` at the ends -- which is what
    /// lets a caller with rows above and below step out of the box.
    ///
    /// With something held, that is the second press. The first lets go of
    /// it and answers `true`, because letting go is what the reader asked
    /// the key for and is a thing that happened: a press that both dropped
    /// the selection and left the box did two things at once, and the one
    /// the reader could see was the wrong one.
    pub fn up(&mut self, width: u16) -> bool {
        self.moved(obelus_editing::Motion::Up, width)
    }

    /// The same, downwards.
    pub fn down(&mut self, width: u16) -> bool {
        self.moved(obelus_editing::Motion::Down, width)
    }

    /// One step, answering for what it did rather than only for whether the
    /// caret moved. The same answer
    /// [`Editing::handle_key`](obelus_editing::Editing::handle_key)
    /// gives, because it is the same question.
    fn moved(&mut self, motion: obelus_editing::Motion, width: u16) -> bool {
        let held = self.writing.has_selection();
        self.writing.move_to(motion, &(), width.max(1)) || held
    }

    /// Takes out what is behind the caret.
    pub fn backspace(&mut self) {
        self.writing
            .apply(obelus_editing::Typing::Backward, &(), u16::MAX);
    }

    /// And what is in front of it.
    pub fn delete(&mut self) {
        self.writing
            .apply(obelus_editing::Typing::Forward, &(), u16::MAX);
    }

    /// One character left, or right.
    pub fn left(&mut self) {
        self.writing
            .move_to(obelus_editing::Motion::Left, &(), u16::MAX);
    }

    /// The same, rightwards.
    pub fn right(&mut self) {
        self.writing
            .move_to(obelus_editing::Motion::Right, &(), u16::MAX);
    }

    /// To the start of the row the caret is on, or the end of it.
    pub fn home(&mut self, width: u16) {
        self.writing
            .move_to(obelus_editing::Motion::LineStart, &(), width.max(1));
    }

    /// The same, to the end.
    pub fn end(&mut self, width: u16) {
        self.writing
            .move_to(obelus_editing::Motion::LineEnd, &(), width.max(1));
    }

    /// To the start of the row, holding what it passes over.
    ///
    /// Which is all shift ever means on a motion. A box is where a reader
    /// reaches for it first -- to take back the line they have just
    /// written -- and it was the one place in obelus where the pair did
    /// something else entirely.
    pub fn hold_home(&mut self, width: u16) {
        self.writing
            .extend_to(obelus_editing::Motion::LineStart, &(), width.max(1));
    }

    /// The same, to the end.
    pub fn hold_end(&mut self, width: u16) {
        self.writing
            .extend_to(obelus_editing::Motion::LineEnd, &(), width.max(1));
    }

    /// The rows it takes at a width, wrapped the way a file's lines are,
    /// with whatever the reader has hold of marked on each.
    #[must_use]
    pub fn laid(&self, width: u16) -> Vec<Laid> {
        laid_out(self.writing.text(), width, self.writing.selection())
    }

    /// The same, for a caller with nothing to say about a selection.
    #[must_use]
    pub fn rows(&self, width: u16) -> Vec<String> {
        self.laid(width).into_iter().map(|row| row.said).collect()
    }

    /// Lets go of whatever is held, leaving the caret where it is.
    ///
    /// For the one selection rule: a reader takes hold of the transcript
    /// or of the box, never both, so whichever is taking hold says the
    /// other has let go.
    pub const fn let_go(&mut self) {
        self.writing.clear_selection();
    }

    /// What the reader has hold of, if they have hold of anything.
    #[must_use]
    pub fn selected(&self) -> Option<String> {
        self.writing.selected()
    }

    /// Takes hold of the whole of it.
    pub fn select_all(&mut self, width: u16) {
        self.writing.select_all(width.max(1));
    }

    /// Puts a run of text in, over whatever is held.
    pub fn write_in(&mut self, what: &str, width: u16) {
        self.writing.write_in(what, width.max(1));
    }

    /// Puts the caret where a cell of a row is, and holds from where it
    /// was if this is a drag.
    ///
    /// `row` and `cell` are counted from the box's own first row and first
    /// column: where the box sits on screen is the renderer's, and the box
    /// knows nothing about it. A row past the last one is the end of the
    /// text and a cell past the end of a row is the end of that row --
    /// pointing outside means the nearest place inside, which is what
    /// pointing at a file means.
    ///
    /// Held before the caret moves, which is what makes a drag a
    /// selection: `hold` takes the caret where it *is*, so holding after
    /// would anchor to the point just reached and every drag would hold
    /// nothing.
    pub fn place_at_cell(&mut self, row: u16, cell: u16, width: u16, extend: bool) {
        let width = width.max(1);
        let text = self.writing.text();
        // Below the text there is no row to point at, so the nearest place
        // is where the text stops -- not the start of the last line, which
        // is what carrying the column down there would give.
        let (line, column) = match line_of_row(text, width, usize::from(row)) {
            Some((line, within)) => (
                line,
                text.column_in_row(line, within, DisplayColumn::new(cell), width),
            ),
            None => {
                let last = text.last_line();
                (last, text.line_length(last))
            }
        };
        match extend {
            true => self.writing.hold(),
            false => self.writing.clear_selection(),
        }
        self.writing.arrive(line, column);
    }

    /// Takes hold of the word the caret is in, which is what a second
    /// click means.
    pub fn hold_word(&mut self, width: u16) {
        let width = width.max(1);
        self.writing
            .move_to(obelus_editing::Motion::WordLeft, &(), width);
        self.writing
            .extend_to(obelus_editing::Motion::WordRight, &(), width);
    }

    /// Takes hold of the line the caret is on, which is what a third means.
    pub fn hold_line(&mut self, width: u16) {
        let width = width.max(1);
        self.writing
            .move_to(obelus_editing::Motion::LineStart, &(), width);
        self.writing
            .extend_to(obelus_editing::Motion::LineEnd, &(), width);
    }

    /// Takes out what is held, and says what it was.
    pub fn cut(&mut self, width: u16) -> Option<String> {
        self.writing.cut(width.max(1))
    }

    /// Which row of the box the caret is on, and how many cells into it.
    #[must_use]
    pub fn caret(&self, width: u16) -> (usize, DisplayColumn) {
        let width = width.max(1);
        let cursor = self.writing.cursor();
        let text = self.writing.text();
        let (row, cell) = text.visual_position(cursor.line, cursor.column, width);
        let before: usize = (0..cursor.line.get())
            .map(|index| text.row_count(obelus_text::coordinates::LineNumber::new(index), width))
            .sum();
        (before + row, cell)
    }
}

/// `text` in rows of `width`, the way a box lays out what is in it.
///
/// A free function because two things want it and only one of them is a box:
/// a page of notes has to lay out the ones nobody is typing in, and laying
/// them out a second way would be a second wrapping to keep in step with
/// this one.
#[must_use]
pub fn wrapped(text: &str, width: u16) -> Vec<Laid> {
    laid_out(&Text::from_string(text), width, None)
}

/// The rows of a text at a width, with `held` broken up across them.
fn laid_out(text: &Text, width: u16, held: Option<Span>) -> Vec<Laid> {
    let width = width.max(1);
    let mut rows = Vec::new();
    for index in 0..text.line_count() {
        let line = LineNumber::new(index);
        let characters: Vec<char> = text.line(line).chars().collect();
        for row in text.wrap_rows(line, width) {
            let words: String = characters
                .iter()
                .take(row.end.get())
                .skip(row.first.get())
                .collect();
            let said = words.trim_end_matches('\n').to_string();
            rows.push(Laid {
                held: held.and_then(|span| held_in(span, line, row.first, said.chars().count())),
                said,
            });
        }
    }
    rows
}

/// Which line a laid-out row belongs to, and which of that line's rows it
/// is.
///
/// The inverse of [`laid_out`], walking the same wrapping in the same
/// order: a row on screen is a row of some line, and which one is a
/// question only the wrapping can answer. `None` for a row past the end,
/// which a click below the last line is.
fn line_of_row(text: &Text, width: u16, row: usize) -> Option<(LineNumber, usize)> {
    let width = width.max(1);
    let mut seen = 0;
    for index in 0..text.line_count() {
        let line = LineNumber::new(index);
        let rows = text.wrap_rows(line, width).len();
        if seen + rows > row {
            return Some((line, row - seen));
        }
        seen += rows;
    }
    None
}

/// Which of a row's characters a span covers, if it covers any.
///
/// Asked of each character rather than worked out as an intersection,
/// because that is the answer the file's own rows get: a second arithmetic
/// for the same question is a second answer for the two to disagree over,
/// and a selection that stopped one character short in a note and not in a
/// file is exactly the kind of thing nobody would find.
fn held_in(span: Span, line: LineNumber, first: CharColumn, shown: usize) -> Option<Range<usize>> {
    let mut held = (0..shown).filter(|at| span.contains(line, CharColumn::new(first.get() + at)));
    let from = held.next()?;
    Some(from..held.next_back().map_or(from + 1, |last| last + 1))
}

#[cfg(test)]
mod pointer_tests {
    use super::Composer;

    /// A click lands where it points, and a drag from one place to another
    /// holds what is between them -- across a wrap, because a box wraps and
    /// the row on screen is not the line in the text.
    #[test]
    fn a_drag_holds_what_it_crossed() {
        let mut composer = Composer::new();
        composer.replace("hello world");
        // Six cells wide, so "hello " and "world" are two rows of one line.
        let rows = composer.rows(6);
        assert_eq!(rows.len(), 2, "the box did not wrap: {rows:?}");

        composer.place_at_cell(0, 0, 6, false);
        composer.place_at_cell(1, 5, 6, true);
        assert_eq!(
            composer.selected().as_deref(),
            Some("hello world"),
            "the drag did not reach across the wrap"
        );

        // And into the middle of the second row rather than its end: the
        // end of a wrapped line is also where a row past the text lands,
        // so a drag that stops there cannot tell the wrapping from the
        // fallback.
        composer.place_at_cell(0, 0, 6, false);
        composer.place_at_cell(1, 2, 6, true);
        assert_eq!(
            composer.selected().as_deref(),
            Some("hello wo"),
            "the second row was not read as the second row of the line"
        );
    }

    /// Pointing below the text is pointing at the end of it, and pointing
    /// past the end of a row is the end of that row.
    #[test]
    fn pointing_outside_lands_at_the_nearest_place() {
        let mut composer = Composer::new();
        composer.replace("one\ntwo");
        composer.place_at_cell(9, 0, 40, false);
        composer.place_at_cell(0, 0, 40, true);
        assert_eq!(
            composer.selected().as_deref(),
            Some("one\ntwo"),
            "a click below the last line did not land at the end"
        );

        composer.place_at_cell(0, 99, 40, false);
        composer.place_at_cell(0, 0, 40, true);
        assert_eq!(
            composer.selected().as_deref(),
            Some("one"),
            "a click past the end of a row did not land at its end"
        );
    }

    /// Twice is the word and three times is the line, the way it is in the
    /// file.
    #[test]
    fn two_clicks_hold_a_word_and_three_hold_the_line() {
        let mut composer = Composer::new();
        composer.replace("hello world");
        composer.place_at_cell(0, 8, 40, false);
        composer.hold_word(40);
        assert_eq!(composer.selected().as_deref(), Some("world"));

        composer.hold_line(40);
        assert_eq!(composer.selected().as_deref(), Some("hello world"));
    }
}
