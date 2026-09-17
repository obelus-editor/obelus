//! What a reader means to come back to, as something to walk through.
//!
//! A list of notes, each a row, some of them with somewhere to go. A dialog
//! rather than a page of the settings, for the reason the counts are one: it
//! is a question about the project rather than a switch the reader keeps,
//! and it is left with escape like every other dialog here.
//!
//! What the keys are is said at the foot of the view rather than learned:
//! there are six of them, three do nothing anywhere else in obelus, and a
//! view whose keys can only be found by reading the source is a view nobody
//! uses twice.
//!
//! The notes themselves, and the file they live in, are [`crate::todo`].
//! Nothing here reads or writes that file -- the application does, because
//! it is the one that knows which tree this is.

use std::{ops::Range, path::PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    component::{
        composer::{Composer, Laid},
        window::Window,
    },
    coordinates::LineNumber,
    todo::{Note, Todo},
};

/// One row of the view.
///
/// Flat, the way the counts' rows are: a note's own lines are rows under it
/// when it is open, and what tells them apart is what the row *is* rather
/// than where it sits.
#[derive(Clone, Debug)]
pub struct Row {
    /// Which note it belongs to.
    pub note: usize,
    /// What it says.
    pub said: String,
    /// Whether this is the note's own row, or a line of its body.
    pub head: bool,
    /// Whether this row is where the note points rather than what it says.
    ///
    /// Its own row, under the note. Hung off the end of the first line it
    /// was a place the note's own words had to be laid out around -- and
    /// where half the notes have none, the right-hand edge it lined up
    /// against was not a column anybody could read down.
    pub place: bool,
    /// Whether the note is done, for the whole of it to be drawn as such.
    pub done: bool,
    /// Which of `said`'s characters the reader has hold of, if any.
    ///
    /// Counted from the start of this row rather than of the note: a row is
    /// what gets drawn, and a selection given in the note's own lines would
    /// have to be taken apart again by whoever draws it.
    pub held: Option<Range<usize>>,
}

/// What a key did.
#[derive(Debug)]
pub enum TodoOutcome {
    /// Not a key this view knows; try the key table.
    Ignored,
    /// Handled. Redraw.
    Consumed,
    /// Handled, and the notes changed: write them down.
    Changed,
    /// Go to this place.
    Go(PathBuf, LineNumber),
    /// Talk to an agent about this note.
    Talk(crate::todo::NoteId),
    /// The reader gave up.
    Cancelled,
    /// Put this on the clipboard.
    Copy {
        /// What to copy.
        text: String,
        /// What it was, for the reader to be told what they got.
        what: &'static str,
    },
    /// The same, for text this has just taken out: write the notes down
    /// too, and say it was cut rather than copied.
    Cut {
        /// What to copy.
        text: String,
        /// What it was.
        what: &'static str,
    },
    /// Put whatever is on the clipboard into the note the caret is in.
    ///
    /// Asked for rather than done, because what is on the clipboard is the
    /// application's question: this knows about a text and a caret in it.
    Paste,
}

/// The notes, while they are showing.
#[derive(Debug, Default)]
pub struct TodoView {
    /// What the tree has to come back to.
    todo: Todo,
    /// Where each note points *now*, worked out when the view opens.
    ///
    /// Beside the notes rather than in them: what a note says is what was
    /// written down, and where it points today is an answer about the file
    /// as it is. Keeping them apart is what stops the answer being written
    /// back into the file as though the reader had said it.
    ///
    /// `None` where git says the line has gone.
    where_now: Vec<Option<LineNumber>>,

    /// The rows as they stand, rebuilt when anything changes rather than per
    /// frame -- the draw path is the one place that cannot afford to do work
    /// it could have done once.
    rows: Vec<Row>,
    /// Which row is selected and which is on top.
    window: Window,
    /// Whether every key this view answers to is showing.
    keys: bool,
    /// How wide a note's own text is, and whether it wraps there.
    ///
    /// Kept because the rows depend on it: a note of one long line is one
    /// row or four, and the window, the caret and the drawing have to be
    /// counting the same rows. Set from the frame, which is the only place
    /// that knows.
    laid: (u16, bool),
    /// The note the caret is in, and what is in it.
    ///
    /// Always, where there is a note to be in. There is no editing *mode*
    /// here: the view is a page of notes being written, and a page that had
    /// to be unlocked before it would take a letter is a page where every
    /// change costs two keys it should not.
    ///
    /// The box a message to an agent is written in, because a note is the
    /// same shape: a paragraph, sometimes pasted, with a caret in it.
    writing: Option<(usize, Composer)>,
}

impl TodoView {
    /// Takes the file again, keeping what the reader was doing.
    ///
    /// Somebody else has written it -- a second obelus, or the reader's own
    /// editor -- and what they wrote is now what the file says. Reopening
    /// the view would be the simple answer and would throw away the note
    /// being written, so instead the notes are swapped and the two things
    /// that are the reader's are put back by *name*: which note the caret
    /// is in, and which one they are part-way through typing.
    ///
    /// A note being typed into that somebody else has deleted is kept, at
    /// the end. The reader is looking at it and has their hands on it; the
    /// other writer did not know that, and of the two of them only one is
    /// here to be surprised.
    pub fn reread(&mut self, todo: Todo, where_now: Vec<Option<LineNumber>>) {
        // The box itself is what carries across, not what it would have
        // written: `keep` takes the box with it, and what it writes into is
        // a copy of the notes this is about to throw away. So the note is
        // taken for its name and the box is kept whole -- which also keeps
        // the caret where the reader left it.
        let writing = self
            .writing
            .take()
            .and_then(|(at, composer)| Some((self.todo.notes.get(at)?.clone(), composer)));
        let focused = self.selected_note().map(|note| note.id.clone());

        self.todo = todo;
        self.where_now = where_now;

        let writing = writing.map(|(note, composer)| {
            let at = self.todo.notes.iter().position(|other| other.id == note.id);
            let at = at.unwrap_or_else(|| {
                self.todo.notes.push(note);
                self.where_now.push(None);
                self.todo.notes.len() - 1
            });
            (at, composer)
        });
        self.rebuild();

        // The caret back where it was, by name. A note that has gone leaves
        // the reader at the top, which is where a list with nothing to
        // return to puts them.
        if let Some(id) = focused {
            self.focus(&id);
        }
        self.writing = writing;
        self.follow_caret();
    }

    /// Puts the selection on the note with this name, if it is still there.
    ///
    /// By name rather than by position, which is the whole reason a note has
    /// one: the list is read from the file every time it opens, and a note
    /// inserted above moves every position below it.
    pub fn focus(&mut self, to: &crate::todo::NoteId) {
        let Some(at) = self.todo.notes.iter().position(|note| note.id == *to) else {
            return;
        };
        if let Some(row) = self
            .rows
            .iter()
            .position(|row| row.note == at && !row.place)
        {
            self.window.set_focus(row);
        }
    }

    /// Opens the view over what a tree has, with where each note points
    /// worked out.
    #[must_use]
    pub fn new(todo: Todo, where_now: Vec<Option<LineNumber>>, laid: (u16, bool)) -> Self {
        let mut view = Self {
            todo,
            where_now,
            laid,
            ..Self::default()
        };
        view.rebuild();
        // The caret goes in straight away: there is nothing to unlock, and a
        // page that opened with no caret would be a page that looks like a
        // list until the reader finds out otherwise.
        if !view.todo.notes.is_empty() {
            view.enter_note(0, false);
        }
        view
    }

    /// Says how wide a note's text is and whether it wraps, and lays the
    /// rows out again if that has moved.
    ///
    /// Asked every frame, because a terminal is resized and a setting is
    /// changed while this is open.
    pub fn lay_out(&mut self, room: u16, wrap: bool) {
        if self.laid == (room, wrap) {
            return;
        }
        self.laid = (room, wrap);
        self.rebuild();
        self.follow_caret();
    }

    /// How a note's text is laid out: its width, and whether it wraps.
    #[must_use]
    pub const fn laid(&self) -> (u16, bool) {
        self.laid
    }

    /// The notes, for whoever writes them down.
    #[must_use]
    pub const fn todo(&self) -> &Todo {
        &self.todo
    }

    /// The rows as they stand.
    #[must_use]
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Where the selection is.
    #[must_use]
    pub const fn window(&self) -> &Window {
        &self.window
    }

    /// The note being written, if one is.
    #[must_use]
    pub fn writing(&self) -> Option<&Composer> {
        self.writing.as_ref().map(|(_, composer)| composer)
    }

    /// The same, to change: what a pointer landing in the box moves.
    pub fn writing_mut(&mut self) -> Option<&mut Composer> {
        self.writing.as_mut().map(|(_, composer)| composer)
    }

    /// Puts an empty note at the end and opens it for writing.
    ///
    /// Written where it will live rather than on the status bar: a note is
    /// read in this list, so it should be written in the shape it will be
    /// read in -- and there is nowhere else on this screen it belongs.
    ///
    /// Nothing is written down until it says something. An empty note taken
    /// away again on escape never existed, which is why it is put in here
    /// and only saved when it is kept.
    pub fn write_new(&mut self, at: Option<crate::todo::At>) {
        self.keep();
        self.todo.notes.push(Note {
            id: crate::todo::NoteId::mint(),
            said: String::new(),
            done: false,
            at,
            // At the end and under nothing: a note made about a line was
            // made somewhere else, and there is no note it was made beneath.
            depth: 0,
        });
        self.where_now.push(None);
        self.enter_note(self.todo.notes.len() - 1, true);
    }

    /// Lays the notes out as rows: every line of every one of them.
    ///
    /// Nothing is folded away. A note is what it says, and a list that shows
    /// a third of each note is a list a reader has to open one row at a time
    /// to read -- which is what they opened the list to avoid.
    fn rebuild(&mut self) {
        let mut rows = Vec::new();
        // Wrapped where the reader asked for wrapping, and one row per line
        // where they did not -- the same answer the file behind this view
        // gives, because it is the same question, and the same width the
        // caret is measured against, because the two have to be counting
        // the same rows.
        let room = self.caret_width().max(1);
        for (index, note) in self.todo.notes.iter().enumerate() {
            // A note the caret is in shows what is in the box, not what is
            // on disk: the reader is looking at their own typing, and it is
            // the box that knows what of it they have hold of.
            let laid = match self.writing.as_ref().filter(|(at, _)| *at == index) {
                Some((_, composer)) => composer.laid(room),
                None => crate::component::composer::wrapped(&note.said, room),
            };
            let mut lines = laid.into_iter();
            let first: Laid = lines.next().unwrap_or_default();
            rows.push(Row {
                note: index,
                said: first.said,
                head: true,
                place: false,
                done: note.done,
                held: first.held,
            });
            for line in lines {
                rows.push(Row {
                    note: index,
                    said: line.said,
                    head: false,
                    place: false,
                    done: note.done,
                    held: line.held,
                });
            }
            // And where it points, under what it says: a row of its own
            // rather than the end of the first line, so what a note says is
            // laid out the same whether it points anywhere or not.
            if let Some(at) = note.at.as_ref() {
                rows.push(Row {
                    note: index,
                    said: match self.where_now.get(index) {
                        Some(Some(line)) => format!("{}:{}", at.path.display(), line.get() + 1),
                        // The file still has the note, and the line does not.
                        _ => format!("{}:gone", at.path.display()),
                    },
                    head: false,
                    place: true,
                    done: note.done,
                    // Never: it is a fact about the note rather than a word
                    // of it, and it is not the reader's to take a copy of
                    // by selecting it.
                    held: None,
                });
            }
        }
        self.rows = rows;
        self.window.set_count(self.rows.len());
    }

    /// Puts the caret in a note, keeping whatever the last one said.
    ///
    /// The commit happens here rather than on a key, because leaving a note
    /// *is* finishing it: there is no other moment, and asking the reader to
    /// mark one would be the mode again under another name.
    fn enter_note(&mut self, to: usize, end: bool) {
        self.keep();
        let Some(note) = self.todo.notes.get(to) else {
            self.writing = None;
            return;
        };
        let mut composer = Composer::new();
        composer.replace(&note.said);
        let room = self.caret_width();
        if !end {
            composer.home(room);
            while composer.up(room) {}
        }
        self.writing = Some((to, composer));
        self.rebuild();
        // Never the place: it is a fact about the note rather than a line
        // of it, and a caret standing on it would be a caret in text the
        // reader cannot change.
        let row = match end {
            true => self
                .rows
                .iter()
                .rposition(|row| row.note == to && !row.place),
            false => self
                .rows
                .iter()
                .position(|row| row.note == to && !row.place),
        };
        if let Some(row) = row {
            self.window.set_focus(row);
        }
    }

    /// Writes what is in the box back into its note, and drops the note if
    /// it says nothing.
    ///
    /// Returns whether anything changed, so a caller can tell a move that
    /// wrote something from one that did not.
    fn keep(&mut self) -> bool {
        let Some((at, composer)) = self.writing.take() else {
            return false;
        };
        // The same shape it would come back in from the file: a note that
        // changed when it was read again would be a note whose rows moved
        // under a reader who had not touched it.
        let said = crate::todo::trimmed(&composer.text());
        if said.trim().is_empty() {
            self.drop_note(at);
            return true;
        }
        match self.todo.notes.get_mut(at) {
            Some(note) if note.said != said => {
                note.said = said;
                true
            }
            _ => false,
        }
    }

    /// Puts a run of text into the note the caret is in, over whatever is
    /// held.
    ///
    /// A note of its own where there is none to be in: a reader who pastes
    /// into an empty page meant to start one.
    pub fn paste(&mut self, what: &str) {
        if self.writing.is_none() {
            self.write_new(None);
        }
        let room = self.caret_width();
        if let Some((_, composer)) = self.writing.as_mut() {
            composer.write_in(what, room);
        }
        self.rebuild();
        self.follow_caret();
    }

    /// Takes the note the caret is in away, and puts the caret on whatever
    /// takes its place. Says whether there was one to take.
    ///
    /// Two keys do this -- `alt+backspace`, which throws it away, and
    /// `ctrl+x`, which takes a copy on the way out -- and a note dropped
    /// half-way by one of them would leave the list a row it cannot fill.
    fn take_note_away(&mut self) -> bool {
        let Some(at) = self.selected() else {
            return false;
        };
        self.writing = None;
        self.drop_note(at);
        self.rebuild();
        if !self.todo.notes.is_empty() {
            self.enter_note(at.min(self.todo.notes.len() - 1), false);
        }
        true
    }

    /// Takes a note away, and the marks that pointed past it with it.
    fn drop_note(&mut self, at: usize) {
        if at >= self.todo.notes.len() {
            return;
        }
        self.todo.notes.remove(at);
        self.where_now.remove(at);
    }

    /// Which note the selection is on, whichever of its rows that is.
    fn selected(&self) -> Option<usize> {
        self.rows.get(self.window.focus()).map(|row| row.note)
    }

    /// The note the selection is on, for whoever is saying what the keys do.
    #[must_use]
    pub fn selected_note(&self) -> Option<&Note> {
        self.todo.notes.get(self.selected()?)
    }

    /// Whether there is anywhere for enter to go from here.
    #[must_use]
    pub fn can_go(&self) -> bool {
        self.selected()
            .is_some_and(|at| self.where_now.get(at).copied().flatten().is_some())
    }

    /// Whether the list of every key is showing.
    #[must_use]
    pub const fn showing_keys(&self) -> bool {
        self.keys
    }

    /// Which row the note being written starts on, for the caret.
    #[must_use]
    pub fn writing_at(&self) -> Option<usize> {
        let (note, _) = self.writing.as_ref()?;
        self.rows
            .iter()
            .position(|row| row.note == *note && !row.place)
    }

    /// Whatever a key means here.
    ///
    /// The letters go in the note the caret is in, always: this page is a
    /// page of notes being written. What acts on a note *as a note* -- tick
    /// it, go where it points, move it, take it away -- is under `alt`,
    /// which is the question alt asks everywhere in obelus: about the thing
    /// the cursor is on.
    pub fn handle_key(&mut self, key: &KeyEvent, page: u16) -> TodoOutcome {
        // The width the box is asked about is this view's own answer, not
        // the caller's: the rows were laid out at it, the caret is measured
        // against it, and a third number arriving through the door is a
        // third answer for them to disagree over.
        let room = self.caret_width();
        let bare = key.modifiers.is_empty();
        let alt = key.modifiers == KeyModifiers::ALT;
        let control = key.modifiers == KeyModifiers::CONTROL;
        match key.code {
            // The card first: a key that opens a thing closes that thing.
            KeyCode::Esc if bare && self.keys => {
                self.keys = false;
                TodoOutcome::Consumed
            }
            KeyCode::Esc if bare => {
                self.keep();
                self.rebuild();
                TodoOutcome::Cancelled
            }
            KeyCode::F(1) if bare => {
                self.keys = !self.keys;
                TodoOutcome::Consumed
            }

            // Another note, which is what enter means in a page being
            // written. Where it points is `alt+enter`: this is not a list of
            // rows to choose from, it is the text of them.
            KeyCode::Enter if bare => {
                let after = self.selected().map_or(0, |at| at + 1);
                self.keep();
                // The note may have gone with the keep, if it said nothing.
                let after = after.min(self.todo.notes.len());
                self.todo.notes.insert(
                    after,
                    Note {
                        id: crate::todo::NoteId::mint(),
                        said: String::new(),
                        done: false,
                        at: None,
                        depth: 0,
                    },
                );
                self.where_now.insert(after, None);
                self.enter_note(after, true);
                TodoOutcome::Changed
            }

            // Up and down walk the note's own lines first, and step to the
            // next note when there are none left -- which is what the box
            // answering `false` at its ends is for.
            KeyCode::Up | KeyCode::Down if bare => {
                let down = key.code == KeyCode::Down;
                let inside = self
                    .writing
                    .as_mut()
                    .is_some_and(|(_, composer)| match down {
                        true => composer.down(room),
                        false => composer.up(room),
                    });
                if inside {
                    self.follow_caret();
                    return TodoOutcome::Consumed;
                }
                let Some(at) = self.selected() else {
                    return TodoOutcome::Consumed;
                };
                let to = match down {
                    true if at + 1 < self.todo.notes.len() => at + 1,
                    false if at > 0 => at - 1,
                    _ => return TodoOutcome::Consumed,
                };
                self.enter_note(to, !down);
                TodoOutcome::Consumed
            }
            KeyCode::PageUp | KeyCode::PageDown if bare => {
                let by = isize::try_from(page.max(1)).unwrap_or(1);
                let by = match key.code {
                    KeyCode::PageUp => -by,
                    _ => by,
                };
                let landed = self.window.step(by, crate::component::window::Wrap::No);
                if let Some(to) = self.rows.get(landed).map(|row| row.note) {
                    self.enter_note(to, false);
                }
                TodoOutcome::Consumed
            }

            // Done, or not. On `alt` because space is a space here.
            KeyCode::Char(' ') if alt => match self.selected() {
                Some(at) => {
                    if let Some(note) = self.todo.notes.get_mut(at) {
                        note.done = !note.done;
                    }
                    self.rebuild();
                    TodoOutcome::Changed
                }
                None => TodoOutcome::Consumed,
            },
            // Where it points. A note about the project has nowhere to go,
            // and nothing is the honest answer.
            KeyCode::Enter if alt => match self.selected().and_then(|at| {
                let place = self.todo.notes.get(at)?.at.as_ref()?;
                Some((place.path.clone(), (*self.where_now.get(at)?)?))
            }) {
                Some((path, line)) => {
                    self.keep();
                    TodoOutcome::Go(path, line)
                }
                None => TodoOutcome::Consumed,
            },
            // The whole note, because backspace on its own is a character.
            // Talk about this one. `alt+a` because the agent's other keys
            // are on `a`, and because it reads as a thing done *to* the row
            // the caret is in -- which is what every other `alt` key on this
            // page is.
            KeyCode::Char('a') if alt => match self.selected_note() {
                Some(note) => {
                    let id = note.id.clone();
                    self.keep();
                    TodoOutcome::Talk(id)
                }
                None => TodoOutcome::Consumed,
            },
            KeyCode::Backspace | KeyCode::Delete if alt => match self.take_note_away() {
                true => TodoOutcome::Changed,
                false => TodoOutcome::Consumed,
            },
            // Where a note sits is the reader's to decide, so nothing else
            // reorders the list: ticking one leaves it where it is.
            KeyCode::Up | KeyCode::Down if alt => {
                self.keep();
                let Some(at) = self.selected() else {
                    return TodoOutcome::Consumed;
                };
                let to = match key.code {
                    KeyCode::Up if at > 0 => at - 1,
                    KeyCode::Down if at + 1 < self.todo.notes.len() => at + 1,
                    _ => {
                        self.enter_note(at, false);
                        return TodoOutcome::Consumed;
                    }
                };
                self.todo.notes.swap(at, to);
                self.where_now.swap(at, to);
                self.enter_note(to, false);
                TodoOutcome::Changed
            }

            // The four keys a reader arrives already holding. They have to
            // be answered here because a dialog is bound to nothing in the
            // key table -- and a box a reader can select in but not copy out
            // of is a box with half a selection.
            //
            // The whole note where nothing is held, the way the file takes
            // the whole line: copying nothing is not something a key can
            // usefully do, and the note is what a line is here.
            KeyCode::Char('c') if control => {
                let Some((_, composer)) = self.writing.as_ref() else {
                    return TodoOutcome::Consumed;
                };
                match composer.selected() {
                    Some(text) => TodoOutcome::Copy {
                        text,
                        what: "selection",
                    },
                    None => TodoOutcome::Copy {
                        text: composer.text(),
                        what: "note",
                    },
                }
            }
            KeyCode::Char('x') if control => {
                let room = self.caret_width();
                // What the note said before the cut, for the arm below: a
                // cut that took nothing has to know what the whole of it
                // was, and after the fact is too late to ask.
                let taken = self.writing.as_mut().map(|(_, composer)| {
                    let whole = composer.text();
                    (composer.cut(room), whole)
                });
                let Some((held, whole)) = taken else {
                    return TodoOutcome::Consumed;
                };
                if let Some(text) = held {
                    self.rebuild();
                    self.follow_caret();
                    return TodoOutcome::Cut {
                        text,
                        what: "selection",
                    };
                }
                // Nothing held, so the whole note goes. Which is how a note
                // is moved somewhere else, and is `alt+backspace` with a
                // copy taken on the way out.
                match self.take_note_away() {
                    true => TodoOutcome::Cut {
                        text: whole,
                        what: "note",
                    },
                    false => TodoOutcome::Consumed,
                }
            }
            KeyCode::Char('v') if control => TodoOutcome::Paste,
            KeyCode::Char('a') if control => {
                let room = self.caret_width();
                let Some((_, composer)) = self.writing.as_mut() else {
                    return TodoOutcome::Consumed;
                };
                composer.select_all(room);
                self.rebuild();
                self.follow_caret();
                TodoOutcome::Consumed
            }

            // Everything else is the box's, and the box knows which keys
            // those are: it is the same box a message to an agent is written
            // in, and which keys a box answers to is one rule.
            _ => {
                let took = self
                    .writing
                    .as_mut()
                    .is_some_and(|(_, composer)| composer.handle_key(key, room));
                if !took {
                    return TodoOutcome::Ignored;
                }
                self.rebuild();
                self.follow_caret();
                TodoOutcome::Consumed
            }
        }
    }

    /// The width the caret is measured against: the row's, where the text
    /// wraps there, and no limit where it does not.
    ///
    /// The same for every note, because where a note points is a row of its
    /// own: what a note says is laid out the same whether it points anywhere
    /// or not.
    #[must_use]
    pub const fn caret_width(&self) -> u16 {
        match self.laid.1 {
            true => self.laid.0,
            false => u16::MAX,
        }
    }

    /// Keeps the selection on the row the caret is really in.
    ///
    /// The window is what scrolls, and it follows the caret rather than the
    /// note: a note of ten lines is ten rows, and a reader typing on the
    /// last of them should not have the view sitting on the first.
    fn follow_caret(&mut self) {
        let Some((at, composer)) = self.writing.as_ref() else {
            return;
        };
        let (line, _) = composer.caret(self.caret_width());
        let Some(first) = self
            .rows
            .iter()
            .position(|row| row.note == *at && !row.place)
        else {
            return;
        };
        self.window
            .set_focus((first + line).min(self.rows.len().saturating_sub(1)));
    }
}
