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
    ATTACHED, Text,
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

/// A picture the reader put in the box.
///
/// The bytes as the clipboard gave them. The box knows nothing about what
/// is in one and nothing about the protocol that will carry it -- it holds
/// them so that they stay in step with the marks that stand for them, which
/// is the one thing that cannot be worked out again afterwards.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attached {
    /// What shape it is in, as the clipboard named it.
    pub mime: String,
    /// The picture itself.
    pub bytes: Vec<u8>,
}

/// One piece of what is in the box, in the order it is in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Part {
    /// A run of what was typed.
    Words(String),
    /// A picture between two of those runs.
    Picture(Attached),
}

/// What is being written.
#[derive(Clone, Debug)]
pub struct Composer {
    /// The text and the caret in it.
    writing: Editing,
    /// The pictures put in, in the order their marks appear.
    ///
    /// Kept beside the text rather than in it, because a picture is not
    /// text and the box is a box of text. What is in the text is one
    /// [`ATTACHED`] per picture, which is what makes the two lists the same
    /// length and the same order -- and what makes the caret step over a
    /// picture in one press and the backspace take the whole of it, both of
    /// them without knowing pictures exist.
    attached: Vec<Attached>,
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
            attached: Vec::new(),
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

    /// Puts a picture in where the caret is.
    ///
    /// One [`ATTACHED`] goes into the text and the picture goes into the
    /// list at the place the marks put it -- which is how many marks are
    /// already behind the caret, not how many pictures there are: a reader
    /// who goes back and pastes one between two others has put it between
    /// them, and the order is what the agent will be told.
    pub fn attach(&mut self, picture: Attached, width: u16) {
        let at = self.marks_before(self.caret_offset());
        self.attached.insert(at, picture);
        self.writing.write_in(&ATTACHED.to_string(), width.max(1));
    }

    /// What is in the box, in order, with the runs between the pictures.
    ///
    /// Empty runs are left out: a picture on its own, or two in a row, has
    /// nothing between them to say, and a block of no words is a block an
    /// agent has to be given a reason for.
    #[must_use]
    pub fn parts(&self) -> Vec<Part> {
        let said = self.writing.said();
        let mut pictures = self.attached.iter().cloned();
        let mut parts = Vec::new();
        let mut words = String::new();
        for character in said.chars() {
            if character == ATTACHED {
                if !words.is_empty() {
                    parts.push(Part::Words(std::mem::take(&mut words)));
                }
                // A mark with no picture behind it cannot happen -- the two
                // lists are kept in step -- and if it ever did, saying
                // nothing is better than saying an empty picture.
                if let Some(picture) = pictures.next() {
                    parts.push(Part::Picture(picture));
                }
                continue;
            }
            words.push(character);
        }
        if !words.is_empty() {
            parts.push(Part::Words(words));
        }
        parts
    }

    /// Takes everything out, in order, leaving the box empty.
    ///
    /// The pictures go with the words because they are the same message:
    /// a box emptied of its text while it still held pictures would send
    /// them with the next thing typed.
    pub fn take_parts(&mut self) -> Vec<Part> {
        let parts = self.parts();
        self.attached.clear();
        self.replace("");
        parts
    }

    /// What a run of parts says, with each picture written out the way the
    /// box draws it.
    ///
    /// For the page rather than for the agent: a message that has been sent
    /// is a record, and the record says what the reader saw themselves
    /// writing.
    #[must_use]
    pub fn spelling(parts: &[Part]) -> String {
        let mut seen = 0;
        let mut out = String::new();
        for part in parts {
            match part {
                Part::Words(words) => out.push_str(words),
                Part::Picture(_) => {
                    seen += 1;
                    out.push_str(&label(seen));
                }
            }
        }
        out
    }

    /// Whether anything in here is a picture.
    #[must_use]
    pub fn has_pictures(&self) -> bool {
        !self.attached.is_empty()
    }

    /// Where the caret is, as characters from the start of the whole text.
    fn caret_offset(&self) -> usize {
        let cursor = self.writing.cursor();
        self.writing
            .text()
            .char_offset(cursor.line, cursor.column)
            .get()
    }

    /// How many marks are in the first `characters` of the text.
    fn marks_before(&self, characters: usize) -> usize {
        self.writing
            .said()
            .chars()
            .take(characters)
            .filter(|character| *character == ATTACHED)
            .count()
    }

    /// Takes out what is behind the caret.
    pub fn backspace(&mut self) {
        let was = self.marks_before(usize::MAX);
        // Which picture the mark behind the caret is, before it is gone.
        let doomed = (self.caret_offset() > 0
            && self.writing.said().chars().nth(self.caret_offset() - 1) == Some(ATTACHED))
        .then(|| self.marks_before(self.caret_offset() - 1));
        self.writing
            .apply(obelus_editing::Typing::Backward, &(), u16::MAX);
        self.forget(was, doomed);
    }

    /// And what is in front of it.
    pub fn delete(&mut self) {
        let was = self.marks_before(usize::MAX);
        let doomed = (self.writing.said().chars().nth(self.caret_offset()) == Some(ATTACHED))
            .then(|| self.marks_before(self.caret_offset()));
        self.writing
            .apply(obelus_editing::Typing::Forward, &(), u16::MAX);
        self.forget(was, doomed);
    }

    /// Drops the picture an edit just took the mark of.
    ///
    /// `doomed` is which one it was, worked out before the edit while the
    /// mark was still there. Where the edit took out more than that one --
    /// a selection with marks inside it -- the count is what says so, and
    /// the tail goes: a selection is a run, so what it removed is a run of
    /// this list too.
    fn forget(&mut self, was: usize, doomed: Option<usize>) {
        let now = self
            .writing
            .said()
            .chars()
            .filter(|c| *c == ATTACHED)
            .count();
        if now == was {
            return;
        }
        if let Some(which) = doomed
            && was - now == 1
            && which < self.attached.len()
        {
            self.attached.remove(which);
            return;
        }
        // More than one went, which a selection typed over does. Which of
        // them is not a count -- but a selection is one run, so what it
        // took out is one run of this list, and the marks that are left say
        // how many. Trimmed from the end, which is right for the common
        // shape of clearing the box and wrong only for a reader who
        // selected across the middle and kept typing; the alternative is
        // carrying an identity for each mark through a box that is
        // deliberately not a buffer.
        self.attached.truncate(now);
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
    /// written -- and it was the one place in Obelus where the pair did
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
    ///
    /// Marks are taken out of it first. What arrives here is text from
    /// somewhere else -- a paste, an agent's words put back in the box --
    /// and one of these in it would be a mark with no picture behind it:
    /// the box would draw `[Image 1]` over nothing and the prompt would be
    /// split where the reader never put anything.
    pub fn write_in(&mut self, what: &str, width: u16) {
        let what = &if what.contains(ATTACHED) {
            what.replace(ATTACHED, "")
        } else {
            what.to_string()
        };
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
    // Which picture the next mark stands for, counted across the whole box
    // rather than the row: a reader who wrapped a line did not renumber
    // what they put in it.
    let mut seen = 0;
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
            let held = held.and_then(|span| held_in(span, line, row.first, said.chars().count()));
            let (said, held) = spelt_out(&said, &mut seen, held);
            rows.push(Laid { held, said });
        }
    }
    rows
}

/// What one mark is drawn as.
///
/// The number, then the mark of a picture and the blank column a Nerd Font
/// glyph is always given -- or two spaces where there is no font for it,
/// which is the same room spent on nothing. Same width either way, because
/// [`obelus_text::ATTACHED_WIDTH`] is one number and the wrapping has
/// already been told it.
fn label(which: usize) -> String {
    let numbered = match which {
        which @ 1..=9 => format!("[Image {which}]"),
        _ => "[Image +]".to_string(),
    };
    match obelus_icons::enabled() {
        true => format!("{numbered}{} ", obelus_icons::ui::PICTURE),
        false => format!("{numbered}  "),
    }
}

/// A row with its marks written out, and whatever is held moved to match.
///
/// The row that reaches the drawing is the row the reader sees, so the mark
/// becomes its words here rather than three layers further on: everything
/// that measures a row then measures what is on screen, which is the same
/// nine columns [`obelus_text::ATTACHED_WIDTH`] already told the wrapping
/// about.
///
/// Past nine pictures the number no longer fits the room, and the room is
/// what the wrapping was told. A `+` rather than a wider mark, because the
/// alternative is a row whose columns and characters disagree -- and the
/// order is still the order, which is what the number was for.
fn spelt_out(
    said: &str,
    seen: &mut usize,
    held: Option<Range<usize>>,
) -> (String, Option<Range<usize>>) {
    if !said.contains(ATTACHED) {
        return (said.to_string(), held);
    }
    let mut out = String::with_capacity(said.len());
    let (mut start, mut end) = (
        held.clone().map_or(0, |range| range.start),
        held.as_ref().map_or(0, |range| range.end),
    );
    for (index, character) in said.chars().enumerate() {
        if character != ATTACHED {
            out.push(character);
            continue;
        }
        *seen += 1;
        out.push_str(&label(*seen));
        // The mark was one character and is now nine, so anything held at
        // or after it moves by eight.
        let grown = obelus_text::ATTACHED_WIDTH - 1;
        debug_assert_eq!(label(*seen).chars().count(), obelus_text::ATTACHED_WIDTH);
        if held.is_some() {
            if index < start {
                start += grown;
            }
            if index < end {
                end += grown;
            }
        }
    }
    (out, held.map(|_| start..end))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(name: &str) -> Attached {
        Attached {
            mime: "image/png".to_string(),
            bytes: name.as_bytes().to_vec(),
        }
    }

    /// A picture is one character in the text, wherever the reader put it.
    ///
    /// The three things a reader asked for are one thing: the mark is a
    /// single character, so the caret steps over the whole of it, backspace
    /// takes the whole of it, and what goes to the agent splits on it into
    /// the blocks the reader built. `abc`, a picture, `def`, a picture is
    /// four blocks and has to stay four -- "the one below" is about the
    /// block after, and a prompt that gathered the pictures at one end
    /// would make it point at nothing.
    ///
    /// Broken deliberately by gathering: returning the words as one `Part`
    /// with the pictures after them gives two blocks here rather than four.
    #[test]
    fn what_is_in_the_box_keeps_the_order_the_reader_built_it_in() {
        let mut box_ = Composer::new();
        box_.write_in("abc", 80);
        box_.attach(picture("one"), 80);
        box_.write_in("def", 80);
        box_.attach(picture("two"), 80);

        assert_eq!(
            box_.parts(),
            vec![
                Part::Words("abc".to_string()),
                Part::Picture(picture("one")),
                Part::Words("def".to_string()),
                Part::Picture(picture("two")),
            ]
        );
        // And the text really is one character per picture.
        assert_eq!(box_.text().chars().count(), 8, "{:?}", box_.text());
    }

    /// Backspace over a picture takes *that* picture, not the last one.
    ///
    /// The marks and the list have to stay in the same order, or a picture
    /// is sent in another's place: the reader asks about "this one" and the
    /// agent is shown a different one.
    ///
    /// The mark taken out is the *first*, which is the half that has to be
    /// asserted. Taking the last one out corrects itself and says nothing
    /// about the bookkeeping -- `parts` walks the marks and takes a picture
    /// per mark, so a picture with no mark left is never reached. That
    /// version of this test passed with `forget` removed from `backspace`
    /// altogether, which is how this one came to be written.
    ///
    /// Broken deliberately by leaving `forget` out of `backspace`: the
    /// first mark goes, the first picture stays, and the mark that is left
    /// -- the reader's second picture -- is paired with the first, so the
    /// agent is shown the picture that was deleted.
    #[test]
    fn taking_a_mark_out_takes_its_own_picture_with_it() {
        let mut box_ = Composer::new();
        box_.attach(picture("one"), 80);
        box_.write_in("between", 80);
        box_.attach(picture("two"), 80);
        assert_eq!(box_.parts().len(), 3);

        // Back to just after the first mark: one for the mark, seven for
        // the word between them.
        for _ in 0..8 {
            box_.left();
        }
        box_.backspace();

        assert_eq!(
            box_.parts(),
            vec![
                Part::Words("between".to_string()),
                Part::Picture(picture("two")),
            ],
            "the mark that is left was paired with the picture that went"
        );
    }

    /// A mark is written out where it is drawn, and it takes the room the
    /// wrapping was told it takes.
    ///
    /// Nine columns, because that is what `char_width` answers -- a row
    /// whose characters and columns disagree is a caret that lands
    /// somewhere else.
    ///
    /// Broken deliberately by returning the row without `spelt_out`: the
    /// row is then one character where the wrapping counted nine.
    #[test]
    fn a_mark_is_drawn_as_the_words_it_stands_for() {
        let mut box_ = Composer::new();
        box_.write_in("look at ", 80);
        box_.attach(picture("one"), 80);
        box_.write_in(" and ", 80);
        box_.attach(picture("two"), 80);

        let rows = box_.rows(80);
        // Written out rather than compared against a literal: what a mark
        // is drawn as depends on the glyph switch, and what it must *not*
        // depend on is how wide it is.
        assert_eq!(rows, vec![format!("look at {} and {}", label(1), label(2))]);
        assert!(rows[0].contains("[Image 1]"), "{rows:?}");
        assert_eq!(
            label(1).chars().count(),
            obelus_text::ATTACHED_WIDTH,
            "a mark is drawn a different width from the one the wrapping was told"
        );
        assert_eq!(
            obelus_text::text_width(&box_.text()),
            rows[0].chars().count(),
            "the room the wrapping was told is not the room the row takes"
        );
    }

    /// Text put in from somewhere else cannot bring a mark with it.
    ///
    /// Broken deliberately by dropping the strip in `write_in`: the box
    /// then draws `[Image 1]` over a picture nobody attached.
    #[test]
    fn a_mark_pasted_in_as_text_is_not_a_picture() {
        let mut box_ = Composer::new();
        box_.write_in(&format!("before{ATTACHED}after"), 80);
        assert_eq!(box_.text(), "beforeafter");
        assert!(!box_.has_pictures());
        assert_eq!(box_.parts(), vec![Part::Words("beforeafter".to_string())]);
    }
}
