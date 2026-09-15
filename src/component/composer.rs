//! The box a message to an agent is written in.
//!
//! A few lines of text with a caret in them, which is a smaller thing than a
//! buffer -- no file, no syntax, no history, no undo -- and a larger thing
//! than a prompt, because a message to an agent is a paragraph and sometimes
//! a pasted stack trace.
//!
//! The lines are kept as lines and the layout is borrowed from
//! [`crate::text::Text`]: wrapping, display columns and the arithmetic that
//! keeps a caret in the right cell when a line holds wide characters are all
//! there already, and a second implementation of them is a second set of
//! off-by-ones. What is here is the editing, which `Text` does not do.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    coordinates::{CharColumn, DisplayColumn, LineNumber},
    text::Text,
};

/// What is being written.
#[derive(Clone, Debug)]
pub struct Composer {
    /// The lines, never empty: an empty box is one empty line, because a
    /// caret has to be somewhere.
    lines: Vec<String>,
    /// Which line the caret is on.
    line: usize,
    /// How far along it, in characters.
    column: usize,
}

impl Default for Composer {
    /// Not derived: the derived one hands back no lines at all, and every
    /// method here reads `lines[line]` on the promise that there is always
    /// one. An empty box is one empty line, because a caret has to be
    /// somewhere.
    fn default() -> Self {
        Self::new()
    }
}

impl Composer {
    /// An empty box.
    #[must_use]
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
            line: 0,
            column: 0,
        }
    }

    /// What has been written, newlines and all.
    #[must_use]
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// Whether there is anything worth sending.
    #[must_use]
    pub fn is_blank(&self) -> bool {
        self.lines.iter().all(|line| line.trim().is_empty())
    }

    /// Takes what was written, leaving the box empty.
    pub fn take(&mut self) -> String {
        let text = self.text();
        *self = Self::new();
        text
    }

    /// Replaces everything, with the caret at the end.
    ///
    /// For completing a command: what the reader typed is replaced by the
    /// whole of it, and they carry on typing after it.
    pub fn replace(&mut self, text: &str) {
        self.lines = text.split('\n').map(str::to_string).collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.line = self.lines.len() - 1;
        self.column = self.lines[self.line].chars().count();
    }

    /// Whatever a key means to the text, or `false` for one that means
    /// nothing to it.
    ///
    /// The keys of a box of text, in one place: there are two boxes now --
    /// a message to an agent and a note -- and the next will be the third.
    /// Which keys a box answers to is a rule rather than a preference, and a
    /// rule written twice is a rule that will be true in one place.
    ///
    /// `enter` and `esc` are *not* here. Finishing and giving up are the
    /// caller's, and the callers answer them differently: one sends a
    /// message, one keeps a note, and a box that decided either would be
    /// deciding something it knows nothing about.
    ///
    /// `shift+enter` is the newline, and `alt+enter` with it: the first is
    /// what a reader presses, and the second is what a terminal that cannot
    /// tell shift from nothing sends.
    ///
    /// Up and down answer `false` at the ends, so a caller with rows above
    /// and below the box can step out of it rather than have the key
    /// swallowed.
    pub fn handle_key(&mut self, key: &KeyEvent, room: u16) -> bool {
        let bare = key.modifiers.is_empty();
        let shift = key.modifiers == KeyModifiers::SHIFT;
        match key.code {
            KeyCode::Enter if shift || key.modifiers == KeyModifiers::ALT => {
                self.newline();
                true
            }
            KeyCode::Up if bare => self.up(room),
            KeyCode::Down if bare => self.down(room),
            KeyCode::Left if bare => {
                self.left();
                true
            }
            KeyCode::Right if bare => {
                self.right();
                true
            }
            KeyCode::Home if bare => {
                self.home(room);
                true
            }
            KeyCode::End if bare => {
                self.end(room);
                true
            }
            KeyCode::Backspace if bare => {
                self.backspace();
                true
            }
            KeyCode::Delete if bare => {
                self.delete();
                true
            }
            KeyCode::Char(character) if bare || shift => {
                self.insert(character);
                true
            }
            _ => false,
        }
    }

    /// Types one character.
    pub fn insert(&mut self, character: char) {
        let at = self.byte_of(self.line, self.column);
        self.lines[self.line].insert(at, character);
        self.column += 1;
    }

    /// Breaks the line at the caret.
    pub fn newline(&mut self) {
        let at = self.byte_of(self.line, self.column);
        let rest = self.lines[self.line].split_off(at);
        self.lines.insert(self.line + 1, rest);
        self.line += 1;
        self.column = 0;
    }

    /// Rubs out the character before the caret, or joins two lines.
    pub fn backspace(&mut self) {
        if self.column > 0 {
            let at = self.byte_of(self.line, self.column - 1);
            self.lines[self.line].remove(at);
            self.column -= 1;
            return;
        }
        if self.line == 0 {
            return;
        }
        let line = self.lines.remove(self.line);
        self.line -= 1;
        self.column = self.lines[self.line].chars().count();
        self.lines[self.line].push_str(&line);
    }

    /// Rubs out the character at the caret, or joins two lines.
    pub fn delete(&mut self) {
        let length = self.lines[self.line].chars().count();
        if self.column < length {
            let at = self.byte_of(self.line, self.column);
            self.lines[self.line].remove(at);
            return;
        }
        if self.line + 1 >= self.lines.len() {
            return;
        }
        let next = self.lines.remove(self.line + 1);
        self.lines[self.line].push_str(&next);
    }

    /// Moves the caret one character left, over a line ending if it is at
    /// the start of a line.
    pub fn left(&mut self) {
        if self.column > 0 {
            self.column -= 1;
        } else if self.line > 0 {
            self.line -= 1;
            self.column = self.lines[self.line].chars().count();
        }
    }

    /// And one right.
    pub fn right(&mut self) {
        if self.column < self.lines[self.line].chars().count() {
            self.column += 1;
        } else if self.line + 1 < self.lines.len() {
            self.line += 1;
            self.column = 0;
        }
    }

    /// To the start of the row the caret is on.
    pub fn home(&mut self, width: u16) {
        let (row, _) = self.caret(width);
        self.caret_onto(row, DisplayColumn::new(0), width);
    }

    /// And to the end of it.
    pub fn end(&mut self, width: u16) {
        let (row, _) = self.caret(width);
        self.caret_onto(row, DisplayColumn::new(u16::MAX), width);
    }

    /// Up one row of the box, keeping the column where it can.
    ///
    /// Rows of the box, not lines of the text: a line that wrapped is
    /// several rows on screen, and a caret that skipped over them would be
    /// a caret that goes where the reader cannot see why.
    ///
    /// Says whether it moved: a caret already on the first row means the
    /// key was meant for whatever else uses it.
    pub fn up(&mut self, width: u16) -> bool {
        let (row, cell) = self.caret(width);
        if row == 0 {
            return false;
        }
        self.caret_onto(row - 1, cell, width);
        true
    }

    /// And down one.
    pub fn down(&mut self, width: u16) -> bool {
        let (row, cell) = self.caret(width);
        if row + 1 >= self.rows(width).len() {
            return false;
        }
        self.caret_onto(row + 1, cell, width);
        true
    }

    /// The box as it is drawn: every row of every line, wrapped.
    #[must_use]
    pub fn rows(&self, width: u16) -> Vec<String> {
        let text = self.layout();
        let mut rows = Vec::new();
        for (index, _) in self.lines.iter().enumerate() {
            let line = LineNumber::new(index);
            let characters: Vec<char> = text.line(line).chars().collect();
            for row in text.wrap_rows(line, width.max(1)) {
                let words: String = characters
                    .iter()
                    .take(row.end.get())
                    .skip(row.first.get())
                    .collect();
                rows.push(words.trim_end_matches('\n').to_string());
            }
        }
        rows
    }

    /// Which row of the box the caret is on, and how many cells into it.
    #[must_use]
    pub fn caret(&self, width: u16) -> (usize, DisplayColumn) {
        let text = self.layout();
        let line = LineNumber::new(self.line);
        let (row, cell) = text.visual_position(line, CharColumn::new(self.column), width.max(1));
        (self.rows_before(&text, self.line, width) + row, cell)
    }

    /// Puts the caret on a row of the box, as near the cell as that row
    /// reaches.
    fn caret_onto(&mut self, row: usize, cell: DisplayColumn, width: u16) {
        let text = self.layout();
        let width = width.max(1);
        let mut before = 0;
        for index in 0..self.lines.len() {
            let line = LineNumber::new(index);
            let count = text.row_count(line, width);
            if row < before + count {
                self.line = index;
                self.column = text.column_in_row(line, row - before, cell, width).get();
                return;
            }
            before += count;
        }
    }

    /// How many rows the lines before this one take.
    fn rows_before(&self, text: &Text, line: usize, width: u16) -> usize {
        (0..line)
            .map(|index| text.row_count(LineNumber::new(index), width.max(1)))
            .sum()
    }

    /// The text, as something that can lay itself out.
    ///
    /// Built rather than kept. A message is a few hundred characters and
    /// this happens on a keystroke; keeping a rope in step with the lines
    /// would be two representations of the same string, which is the
    /// arrangement that eventually disagrees with itself.
    fn layout(&self) -> Text {
        Text::from_string(&self.text())
    }

    /// Where a character column is, in bytes.
    fn byte_of(&self, line: usize, column: usize) -> usize {
        self.lines[line]
            .char_indices()
            .nth(column)
            .map_or(self.lines[line].len(), |(at, _)| at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Types a string, one character at a time, as a reader would.
    fn typed(text: &str) -> Composer {
        let mut composer = Composer::new();
        for character in text.chars() {
            match character {
                '\n' => composer.newline(),
                character => composer.insert(character),
            }
        }
        composer
    }

    #[test]
    fn typing_and_rubbing_out_leave_the_caret_where_it_belongs() {
        let mut composer = typed("hello");
        assert_eq!(composer.text(), "hello");
        assert_eq!(composer.caret(20), (0, DisplayColumn::new(5)));

        composer.left();
        composer.left();
        composer.insert('!');
        assert_eq!(composer.text(), "hel!lo");
        composer.backspace();
        assert_eq!(composer.text(), "hello");
        composer.delete();
        assert_eq!(composer.text(), "helo");
        // And neither runs off the end.
        for _ in 0..10 {
            composer.backspace();
            composer.delete();
        }
        assert_eq!(composer.text(), "");
        assert!(composer.is_blank());
    }

    #[test]
    fn a_line_break_splits_the_line_and_joins_back() {
        let mut composer = typed("onethree");
        for _ in 0..5 {
            composer.left();
        }
        composer.newline();
        assert_eq!(composer.text(), "one\nthree");
        assert_eq!(composer.caret(20), (1, DisplayColumn::new(0)));

        // Rubbing out at the start of a line takes the break with it, and
        // the caret lands where the join happened.
        composer.backspace();
        assert_eq!(composer.text(), "onethree");
        assert_eq!(composer.caret(20), (0, DisplayColumn::new(3)));
    }

    /// The caret moves by rows of the box, not by lines of the text: a line
    /// that wrapped is several rows on screen.
    #[test]
    fn the_caret_walks_the_rows_a_wrapped_line_takes() {
        let composer = typed("one two three four five");
        // Ten cells wide, so the words break across rows.
        let rows = composer.rows(10);
        assert!(rows.len() > 2, "nothing wrapped: {rows:?}");

        let mut walking = composer.clone();
        // From the end, up through every row and back down again.
        let (last, _) = walking.caret(10);
        assert_eq!(last + 1, rows.len());
        for expected in (0..last).rev() {
            assert!(walking.up(10));
            assert_eq!(walking.caret(10).0, expected);
        }
        assert!(!walking.up(10), "it went above the first row");
        for expected in 1..=last {
            assert!(walking.down(10));
            assert_eq!(walking.caret(10).0, expected);
        }
        assert!(!walking.down(10), "it went below the last row");
    }

    /// Which is the whole reason the layout is borrowed: a caret after a
    /// wide character is two cells along, not one.
    #[test]
    fn a_wide_character_moves_the_caret_two_cells() {
        let composer = typed("\u{4f60}\u{597d}a");
        assert_eq!(composer.caret(20), (0, DisplayColumn::new(5)));
    }

    #[test]
    fn home_and_end_are_the_rows_own() {
        let mut composer = typed("one\ntwo");
        composer.home(20);
        assert_eq!(composer.caret(20), (1, DisplayColumn::new(0)));
        composer.end(20);
        assert_eq!(composer.caret(20), (1, DisplayColumn::new(3)));
        assert_eq!(composer.text(), "one\ntwo");
    }

    #[test]
    fn what_is_taken_leaves_an_empty_box() {
        let mut composer = typed("send me");
        assert_eq!(composer.take(), "send me");
        assert_eq!(composer.text(), "");
        assert_eq!(composer.caret(20), (0, DisplayColumn::new(0)));
        assert_eq!(composer.rows(20), [""]);
    }

    #[test]
    fn a_completed_command_leaves_the_caret_after_it() {
        let mut composer = typed("/comp");
        composer.replace("/compact ");
        assert_eq!(composer.text(), "/compact ");
        assert_eq!(composer.caret(20), (0, DisplayColumn::new(9)));
    }
}
