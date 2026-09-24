//! One line with a caret in it.
//!
//! The three boxes Obelus asks a short question in -- a list's query, the
//! settings' filter, the prompt on the status bar -- were three `String`s
//! with `push` and `pop`. They had no caret at all: a reader could type at
//! the end and rub out from the end, and nothing else. Fixing the middle of
//! a word meant deleting back to it, and a rename that starts at the old
//! name -- which is most renames -- meant deleting the whole tail to change
//! a letter near the front.
//!
//! What is *in* the line is [`Editing`], which is what the file being read
//! is and what a message to an agent is. That is the whole of the point: a
//! reader who has learnt `ctrl+left` in a file finds it walks a word here,
//! and a key added to one is a key all of them have.
//!
//! What is here is the line's own policy, and it is deliberately the only
//! thing here. A line has no line breaks in it and nothing to indent, and
//! some lines take only some characters -- a line number is digits. None of
//! that belongs to the text; it belongs to the question being asked. So
//! this reaches for the pieces [`Editing`] is built from rather than its
//! all-in-one `handle_key`: what a key *means* is worked out once, in
//! `editing`, and what this line does about each meaning is worked out
//! here. A box with a rule of its own adds an arm, not a copy of the key
//! table.

use crossterm::event::KeyEvent;
use obelus_editing::{Editing, Typing};
use obelus_text::coordinates::{CharColumn, DisplayColumn, LineNumber};

/// Whether a character belongs in a line.
///
/// A function rather than a flag, because the rules do not fall into
/// kinds: a line number is digits, a query is anything that can be typed,
/// and whatever is asked for next will be neither.
pub type Accepts = fn(char) -> bool;

/// Everything a reader can type.
#[must_use]
pub fn anything(_: char) -> bool {
    true
}

/// A line being typed into.
#[derive(Clone, Debug)]
pub struct Field {
    writing: Editing,
    accepts: Accepts,
}

impl Field {
    /// An empty one that takes anything.
    #[must_use]
    pub fn new() -> Self {
        Self::taking(anything)
    }

    /// An empty one that takes only what its rule allows.
    #[must_use]
    pub fn taking(accepts: Accepts) -> Self {
        Self {
            writing: Editing::new(""),
            accepts,
        }
    }

    /// One that starts with something in it, and the caret after it.
    ///
    /// Where a rename starts: the old name is what is being changed, so it
    /// is what the reader should find under the caret rather than have to
    /// type again.
    #[must_use]
    pub fn about(said: &str, accepts: Accepts) -> Self {
        let mut field = Self::taking(accepts);
        field.replace(said);
        field
    }

    /// What has been typed.
    #[must_use]
    pub fn said(&self) -> String {
        self.writing.said()
    }

    /// Whether nothing has been.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.writing.is_blank()
    }

    /// Puts something else in, with the caret after it.
    pub fn replace(&mut self, said: &str) {
        self.writing.replace(said, ROOM);
        self.writing
            .move_to(obelus_editing::Motion::DocumentEnd, &(), ROOM);
    }

    /// Where the caret is, counted in characters from the start.
    ///
    /// What the renderer needs, and the reason it is characters: the row is
    /// drawn from the text, and how wide that is on screen is the
    /// renderer's own arithmetic over what comes before the caret.
    #[must_use]
    pub fn caret(&self) -> CharColumn {
        self.writing.cursor().column
    }

    /// Which characters are held, if any are.
    #[must_use]
    pub fn held(&self) -> Option<std::ops::Range<usize>> {
        let span = self.writing.selection()?;
        Some(span.column.get()..span.end_column.get())
    }

    /// What is held, as text.
    #[must_use]
    pub fn selected(&self) -> Option<String> {
        self.writing.selected()
    }

    /// Puts a run of text in where the caret is, one character at a time so
    /// the line's own rule applies to every one of them.
    ///
    /// What a paste arrives as. A newline in it is not a line break here --
    /// there is one line -- so it becomes the blank that a reader pasting a
    /// wrapped path or a line of a log meant by it.
    pub fn put(&mut self, said: &str) {
        for character in said.chars() {
            let character = match character {
                '\n' | '\r' | '\t' => ' ',
                other => other,
            };
            if (self.accepts)(character) {
                self.writing.apply(Typing::Character(character), &(), ROOM);
            }
        }
    }

    /// Whatever a key means to the line, and whether it meant anything.
    ///
    /// Only what a line can answer. Enter, tab and the arrows that walk a
    /// list are the owner's -- it asks this last, after it has taken its
    /// own -- and this refuses them again rather than relying on that: a
    /// line that put a line break in because somebody reordered two `if`s
    /// would be a line nobody could see the end of.
    pub fn handle_key(&mut self, key: &KeyEvent) -> bool {
        if let Some((motion, extend)) = obelus_editing::motion_for(key) {
            // Up and down have nowhere to go in one line, and saying so is
            // what lets the owner give them to its list.
            if matches!(
                motion,
                obelus_editing::Motion::Up | obelus_editing::Motion::Down
            ) {
                return false;
            }
            match extend {
                true => self.writing.extend_to(motion, &(), ROOM),
                false => self.writing.move_to(motion, &(), ROOM),
            };
            // Taken whether or not the caret moved. A left arrow at the
            // start is a key the line *saw*, and one that fell through
            // because it had nowhere to go would be a key doing one thing
            // in the middle of a line and something else at its start.
            return true;
        }
        let Some(typing) = obelus_editing::typing_for(key) else {
            return false;
        };
        match typing {
            // A line has no line breaks in it and nothing to indent.
            Typing::Newline | Typing::Tab | Typing::Outdent => false,
            Typing::Character(character) => {
                // Refused characters are still keys the line *saw*: letting
                // one fall through to the key table would run a command
                // from inside a question.
                if (self.accepts)(character) {
                    self.writing.apply(typing, &(), ROOM);
                }
                true
            }
            taking => {
                self.writing.apply(taking, &(), ROOM);
                true
            }
        }
    }

    /// Puts the caret where a cell of the row is, and says whether it
    /// landed anywhere.
    ///
    /// `cell` is counted from the first character of the line rather than
    /// from the edge of the screen: what is drawn in front of the line --
    /// a prompt's label, the mark a list puts before its query -- is the
    /// renderer's, and the line knows nothing about it.
    ///
    /// `extend` is a drag: the place the button went down stays put and
    /// this end moves, which is what makes a selection out of two points.
    pub fn place_at_cell(&mut self, cell: u16, extend: bool) {
        let line = LineNumber::new(0);
        let column = self
            .writing
            .text()
            .column_in_row(line, 0, DisplayColumn::new(cell), ROOM);
        // Held before arriving, which is what makes the anchor the place
        // the button went down: `hold` takes the caret where it *is*, so
        // holding after the move would anchor the selection to the point
        // it had just reached and every drag would hold nothing. The
        // keyboard path does the same thing in the same order.
        match extend {
            true => self.writing.hold(),
            false => self.writing.clear_selection(),
        }
        self.writing.arrive(line, column);
    }

    /// Takes hold of the word the caret is in.
    ///
    /// What a second click means, the way it does in the file.
    pub fn hold_word(&mut self) {
        self.writing
            .move_to(obelus_editing::Motion::WordLeft, &(), ROOM);
        self.writing
            .extend_to(obelus_editing::Motion::WordRight, &(), ROOM);
    }

    /// Takes hold of all of it, which is what a third click means: a line
    /// is what a line has instead of a line.
    pub fn hold_all(&mut self) {
        self.writing.select_all(ROOM);
    }

    /// What a copy takes from it, and what to call it.
    ///
    /// What is held, or the whole line where nothing is -- the rule the
    /// file follows with its line and the notes follow with their note.
    /// Copying nothing is not something a key can usefully do, and holding
    /// the whole of a one-line answer first is a step worth sparing.
    #[must_use]
    pub fn copied(&self) -> (String, &'static str) {
        match self.selected() {
            Some(held) => (held, "selection"),
            None => (self.said(), "line"),
        }
    }

    /// The same, and takes it out.
    pub fn cut(&mut self) -> (String, &'static str) {
        match self.writing.cut(ROOM) {
            Some(held) => (held, "selection"),
            None => (self.take(), "line"),
        }
    }

    /// Takes everything out and hands it over.
    fn take(&mut self) -> String {
        let said = self.said();
        self.replace("");
        said
    }
}

impl Default for Field {
    fn default() -> Self {
        Self::new()
    }
}

/// How wide the line is told it is.
///
/// Nothing here wraps: a line is one row however long it gets, and which
/// part of it is on screen is the renderer's question rather than the
/// text's. The width only reaches [`Editing`] so that a motion measured in
/// rows has something to measure against, and there are no rows here.
const ROOM: u16 = u16::MAX;
