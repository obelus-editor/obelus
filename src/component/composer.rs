//! The box a message or a note is written in.
//!
//! A few lines of text with a caret in them, which is a smaller thing than a
//! buffer -- no file, no syntax, no history -- and a larger thing than a
//! prompt, because a message to an agent is a paragraph and sometimes a
//! pasted stack trace.
//!
//! What is *in* the box is [`crate::editing::Editing`], which is what the
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

use crate::{
    coordinates::{CharColumn, DisplayColumn, LineNumber, Span},
    editing::Editing,
    text::Text,
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
            .apply(crate::editing::Typing::Character(character), &(), u16::MAX);
    }

    /// Breaks the line at the caret.
    pub fn newline(&mut self) {
        self.writing
            .apply(crate::editing::Typing::Newline, &(), u16::MAX);
    }

    /// Up or down one row, answering `false` at the ends -- which is what
    /// lets a caller with rows above and below step out of the box.
    pub fn up(&mut self, width: u16) -> bool {
        self.writing
            .move_to(crate::editing::Motion::Up, &(), width.max(1))
    }

    /// The same, downwards.
    pub fn down(&mut self, width: u16) -> bool {
        self.writing
            .move_to(crate::editing::Motion::Down, &(), width.max(1))
    }

    /// Takes out what is behind the caret.
    pub fn backspace(&mut self) {
        self.writing
            .apply(crate::editing::Typing::Backward, &(), u16::MAX);
    }

    /// And what is in front of it.
    pub fn delete(&mut self) {
        self.writing
            .apply(crate::editing::Typing::Forward, &(), u16::MAX);
    }

    /// One character left, or right.
    pub fn left(&mut self) {
        self.writing
            .move_to(crate::editing::Motion::Left, &(), u16::MAX);
    }

    /// The same, rightwards.
    pub fn right(&mut self) {
        self.writing
            .move_to(crate::editing::Motion::Right, &(), u16::MAX);
    }

    /// To the start of the row the caret is on, or the end of it.
    pub fn home(&mut self, width: u16) {
        self.writing
            .move_to(crate::editing::Motion::LineStart, &(), width.max(1));
    }

    /// The same, to the end.
    pub fn end(&mut self, width: u16) {
        self.writing
            .move_to(crate::editing::Motion::LineEnd, &(), width.max(1));
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
            .map(|index| text.row_count(crate::coordinates::LineNumber::new(index), width))
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
