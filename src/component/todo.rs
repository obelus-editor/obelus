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

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    component::{composer::Composer, window::Window},
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
    /// Whether the note is done, for the whole of it to be drawn as such.
    pub done: bool,
    /// Whether it opens, and whether it is open. `None` on a body row and on
    /// a note with nothing behind it.
    pub open: Option<bool>,
    /// Where it points, written the way a reader would say it, for the head
    /// row of a note that points anywhere.
    pub at: Option<String>,
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
    /// The reader gave up.
    Cancelled,
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
    /// Which notes are showing their body.
    open: std::collections::HashSet<usize>,
    /// The rows as they stand, rebuilt when anything changes rather than per
    /// frame -- the draw path is the one place that cannot afford to do work
    /// it could have done once.
    rows: Vec<Row>,
    /// Which row is selected and which is on top.
    window: Window,
    /// Whether every key this view answers to is showing.
    keys: bool,
    /// The note being written, where one is.
    ///
    /// The box a message to an agent is written in, because a note is the
    /// same shape: a paragraph, sometimes pasted, with a caret in it.
    writing: Option<(usize, Composer)>,
}

impl TodoView {
    /// Opens the view over what a tree has, with where each note points
    /// worked out.
    #[must_use]
    pub fn new(todo: Todo, where_now: Vec<Option<LineNumber>>) -> Self {
        let mut view = Self {
            todo,
            where_now,
            ..Self::default()
        };
        view.rebuild();
        view
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
        self.todo.notes.push(Note {
            said: String::new(),
            done: false,
            at,
        });
        self.where_now.push(None);
        let index = self.todo.notes.len() - 1;
        self.rebuild();
        if let Some(on) = self.rows.iter().rposition(|row| row.head) {
            self.window.set_focus(on);
        }
        self.writing = Some((index, Composer::default()));
    }

    /// Lays the notes out as rows.
    fn rebuild(&mut self) {
        let mut rows = Vec::new();
        for (index, note) in self.todo.notes.iter().enumerate() {
            // A note being written shows what is in the box, not what is on
            // disk: the reader is looking at their own typing.
            let writing = self
                .writing
                .as_ref()
                .filter(|(at, _)| *at == index)
                .map(|(_, composer)| composer.text());
            let said = writing.clone().unwrap_or_else(|| note.said.clone());
            let open = writing.is_some() || self.open.contains(&index);
            let mut lines = said.lines();
            rows.push(Row {
                note: index,
                said: lines.next().unwrap_or("").to_string(),
                head: true,
                done: note.done,
                // Nothing to fold while it is being written: what is behind
                // the row is on the screen, and a mark saying it could be
                // hidden is a mark for a key that is a character right now.
                open: (writing.is_none() && note.folds()).then_some(open),
                // Where it points *now*, not where it was put: a reader
                // reading the row is about to press enter on it.
                at: note.at.as_ref().map(|at| match self.where_now.get(index) {
                    Some(Some(line)) => format!("{}:{}", at.path.display(), line.get() + 1),
                    // The file still has the note, and the line does not.
                    // Said rather than left off, because "somewhere in this
                    // file" is more than the reader had.
                    _ => format!("{}:gone", at.path.display()),
                }),
            });
            if open {
                for line in lines {
                    rows.push(Row {
                        note: index,
                        said: line.to_string(),
                        head: false,
                        done: note.done,
                        open: None,
                        at: None,
                    });
                }
            }
        }
        self.rows = rows;
        self.window.set_count(self.rows.len());
    }

    /// Keeps what was written, or takes the note away where it says nothing.
    ///
    /// Nothing and nothing but blanks are the same answer. A row a reader
    /// cannot tell from an empty one is not a note, whether it was made that
    /// way or emptied.
    fn finish(&mut self, at: usize, said: &str) {
        if said.trim().is_empty() {
            self.drop_note(at);
        } else if let Some(note) = self.todo.notes.get_mut(at) {
            note.said = said.to_string();
        }
        self.rebuild();
        // The selection follows what it was on, or comes back to the end.
        let on = self
            .rows
            .iter()
            .position(|row| row.note == at && row.head)
            .or_else(|| self.rows.len().checked_sub(1));
        if let Some(on) = on {
            self.window.set_focus(on);
        }
    }

    /// Takes a note away, and the marks that pointed past it with it.
    fn drop_note(&mut self, at: usize) {
        if at >= self.todo.notes.len() {
            return;
        }
        self.todo.notes.remove(at);
        self.where_now.remove(at);
        self.open = self
            .open
            .iter()
            .filter_map(|open| match (*open).cmp(&at) {
                std::cmp::Ordering::Less => Some(*open),
                std::cmp::Ordering::Equal => None,
                std::cmp::Ordering::Greater => Some(open - 1),
            })
            .collect();
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

    /// Whether the note under the selection has anything behind it.
    #[must_use]
    pub fn can_fold(&self) -> bool {
        self.selected_note().is_some_and(Note::folds)
    }

    /// Whether the list of every key is showing.
    #[must_use]
    pub const fn showing_keys(&self) -> bool {
        self.keys
    }

    /// Which row is being written in, for the view to draw the box there.
    #[must_use]
    pub fn writing_at(&self) -> Option<usize> {
        let (note, _) = self.writing.as_ref()?;
        self.rows
            .iter()
            .position(|row| row.note == *note && row.head)
    }

    /// Whatever a key means here.
    pub fn handle_key(&mut self, key: &KeyEvent, page: u16, room: u16) -> TodoOutcome {
        if self.writing.is_some() {
            return self.write_key(key, room);
        }
        let bare = key.modifiers.is_empty();
        let alt = key.modifiers == KeyModifiers::ALT;
        match key.code {
            // The card first: a key that opens a thing closes that thing.
            KeyCode::Esc if bare && self.keys => {
                self.keys = false;
                TodoOutcome::Consumed
            }
            KeyCode::Esc if bare => TodoOutcome::Cancelled,
            KeyCode::Up if bare => {
                self.window.step(-1, crate::component::window::Wrap::Yes);
                TodoOutcome::Consumed
            }
            KeyCode::Down if bare => {
                self.window.step(1, crate::component::window::Wrap::Yes);
                TodoOutcome::Consumed
            }
            KeyCode::PageUp if bare => {
                self.window.step(
                    -isize::try_from(page.max(1)).unwrap_or(1),
                    crate::component::window::Wrap::No,
                );
                TodoOutcome::Consumed
            }
            KeyCode::PageDown if bare => {
                self.window.step(
                    isize::try_from(page.max(1)).unwrap_or(1),
                    crate::component::window::Wrap::No,
                );
                TodoOutcome::Consumed
            }
            // Where it points, which is what enter means in every list here.
            // A note about the project has nowhere to go, and nothing is the
            // honest answer -- the row says so by having no place on it.
            KeyCode::Enter if bare => match self.selected().and_then(|at| {
                let note = self.todo.notes.get(at)?;
                let place = note.at.as_ref()?;
                let line = (*self.where_now.get(at)?)?;
                Some((place.path.clone(), line))
            }) {
                Some((path, line)) => TodoOutcome::Go(path, line),
                None => TodoOutcome::Consumed,
            },
            KeyCode::Char(' ') if bare => match self.selected() {
                Some(at) => {
                    if let Some(note) = self.todo.notes.get_mut(at) {
                        note.done = !note.done;
                    }
                    self.rebuild();
                    TodoOutcome::Changed
                }
                None => TodoOutcome::Consumed,
            },
            KeyCode::Delete if bare => match self.selected() {
                Some(at) => {
                    self.drop_note(at);
                    self.rebuild();
                    TodoOutcome::Changed
                }
                None => TodoOutcome::Consumed,
            },
            KeyCode::Char('f') if alt => match self.selected() {
                Some(at) if self.todo.notes.get(at).is_some_and(Note::folds) => {
                    if !self.open.remove(&at) {
                        self.open.insert(at);
                    }
                    let on = self.rows.iter().position(|row| row.note == at && row.head);
                    self.rebuild();
                    if let Some(on) = on {
                        self.window.set_focus(on);
                    }
                    TodoOutcome::Consumed
                }
                _ => TodoOutcome::Ignored,
            },
            // The same key that writes one down while reading. A note made
            // from here has no line under it, which the application knows
            // and this does not have to.
            KeyCode::Char('n') if alt => {
                self.write_new(None);
                TodoOutcome::Consumed
            }
            // Where a note sits is the reader's to decide, so nothing else
            // reorders the list: ticking one leaves it where it is.
            KeyCode::Up | KeyCode::Down if alt => {
                let Some(at) = self.selected() else {
                    return TodoOutcome::Ignored;
                };
                let to = match key.code {
                    KeyCode::Up if at > 0 => at - 1,
                    KeyCode::Down if at + 1 < self.todo.notes.len() => at + 1,
                    _ => return TodoOutcome::Consumed,
                };
                self.todo.notes.swap(at, to);
                self.where_now.swap(at, to);
                self.open = self
                    .open
                    .iter()
                    .map(|open| match *open {
                        it if it == at => to,
                        it if it == to => at,
                        it => it,
                    })
                    .collect();
                self.rebuild();
                if let Some(on) = self.rows.iter().position(|row| row.note == to && row.head) {
                    self.window.set_focus(on);
                }
                TodoOutcome::Changed
            }
            KeyCode::F(1) if bare => {
                self.keys = !self.keys;
                TodoOutcome::Consumed
            }
            KeyCode::Char('e') if alt => match self.selected() {
                Some(at) => {
                    let mut composer = Composer::default();
                    if let Some(note) = self.todo.notes.get(at) {
                        composer.replace(&note.said);
                    }
                    self.writing = Some((at, composer));
                    TodoOutcome::Consumed
                }
                None => TodoOutcome::Ignored,
            },
            _ => TodoOutcome::Ignored,
        }
    }

    /// Whatever a key means while a note is being written.
    ///
    /// Escape gives up on the writing and not on the view, which is the rule
    /// every other thing here follows: a key that opens a thing closes that
    /// thing.
    fn write_key(&mut self, key: &KeyEvent, room: u16) -> TodoOutcome {
        let outcome = self.write_key_in(key, room);
        // What the box holds is what the rows show, so they follow it.
        if matches!(outcome, TodoOutcome::Consumed) {
            self.rebuild();
        }
        outcome
    }

    fn write_key_in(&mut self, key: &KeyEvent, room: u16) -> TodoOutcome {
        let Some((_, composer)) = self.writing.as_mut() else {
            return TodoOutcome::Ignored;
        };
        let bare = key.modifiers.is_empty();
        match key.code {
            // Given up on. A note that was new is gone with it -- it never
            // said anything, so there was never a note -- and one that was
            // being written over keeps what it said.
            KeyCode::Esc if bare => {
                let Some((at, _)) = self.writing.take() else {
                    return TodoOutcome::Consumed;
                };
                let empty = self
                    .todo
                    .notes
                    .get(at)
                    .is_some_and(|note| note.said.trim().is_empty());
                if empty {
                    self.drop_note(at);
                }
                self.rebuild();
                match empty {
                    true => TodoOutcome::Changed,
                    false => TodoOutcome::Consumed,
                }
            }
            // Finished, because enter is what finishes a thing here -- and
            // `alt+enter` is the newline, the way the message box does it.
            KeyCode::Enter if bare => {
                let (at, composer) = self.writing.take().unwrap_or_else(|| unreachable!());
                self.finish(at, &composer.text());
                TodoOutcome::Changed
            }
            // Everything else is the box's, and the box knows which keys
            // those are: it is the same box a message to an agent is
            // written in, and which keys a box answers to is one rule.
            _ => match composer.handle_key(key, room) {
                true => TodoOutcome::Consumed,
                false => TodoOutcome::Ignored,
            },
        }
    }
}
