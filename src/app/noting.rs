//! What a tree means to come back to.
//!
//! The application's half of [`crate::todo`]: which tree the notes belong
//! to, when they are read and written, and what a key that reaches one of
//! them does. The notes themselves, and the file, are that module's; how
//! they are walked and drawn are [`crate::component::todo`] and
//! [`crate::ui::todo`].
//!
//! Read when the view opens rather than held: obelus is not the only thing
//! that can write the file -- the reader has an editor, and a second obelus
//! is an ordinary thing to have open -- and a copy kept from startup would
//! be a copy that is wrong by the time anybody looks at it.

use std::path::PathBuf;

use super::*;
use crate::{
    component::todo::{TodoOutcome, TodoView},
    coordinates::LineNumber,
    todo::{At, Todo},
};

impl App {
    /// The notes, while they are showing.
    #[must_use]
    pub const fn notes(&self) -> Option<&TodoView> {
        self.notes.as_ref()
    }

    /// Opens what the tree means to come back to.
    ///
    /// Where each note points is worked out here, once, while the view is
    /// being opened: it is a question for git and for the disk, and the draw
    /// path is the one place that must never ask either.
    pub fn open_todo(&mut self) {
        let todo = Todo::read(&self.working_directory);
        let where_now = todo
            .notes
            .iter()
            .map(|note| {
                note.at
                    .as_ref()
                    .and_then(|at| crate::todo::where_now(&self.working_directory, at))
            })
            .collect();
        self.notes = Some(TodoView::new(todo, where_now));
    }

    /// Writes one down about the line being read.
    ///
    /// Opens the view and starts an empty note in it, carrying where the
    /// reader was. One way to write a note, wherever it is started from: it
    /// is written in the list it will be read in, and a second way -- a line
    /// typed on the status bar -- would be a note made in a shape nobody
    /// ever sees it in.
    pub fn add_todo(&mut self) {
        let at = self.here_now();
        if self.notes.is_none() {
            self.open_todo();
        }
        if let Some(notes) = self.notes.as_mut() {
            notes.write_new(at);
        }
    }

    /// The place a note made now would be about.
    ///
    /// Relative to the tree, because every path obelus writes down is: an
    /// absolute one is about one machine, and the file it names is about the
    /// project.
    ///
    /// Nothing while the notes themselves are showing: there is no line
    /// under a list, and a note made from here is about the project.
    fn here_now(&self) -> Option<At> {
        if self.notes.is_some() {
            return None;
        }
        let buffer = self.current_buffer()?;
        let path = buffer
            .path()
            .strip_prefix(&self.working_directory)
            .unwrap_or(buffer.path())
            .to_path_buf();
        // A buffer with no path behind it -- a commit's message, the welcome
        // screen -- is not a place to come back to.
        if path.as_os_str().is_empty() {
            return None;
        }
        Some(At {
            path,
            line: buffer.cursor().line,
            commit: crate::todo::at_commit(&self.working_directory),
        })
    }

    /// Writes the notes down, and says so if it cannot.
    pub(super) fn save_notes(&mut self, todo: &Todo) {
        if let Err(error) = todo.write(&self.working_directory) {
            tracing::warn!(%error, "the notes were not written");
            self.note = Some("the notes could not be written".to_string());
        }
    }

    /// Whatever a key means to the notes, if they are showing.
    pub(super) fn notes_key(&mut self, key: &KeyEvent) -> bool {
        let Some(notes) = self.notes.as_mut() else {
            return false;
        };
        let hints = crate::ui::todo::hints(notes);
        let list = crate::ui::todo::list_region(self.editor_area, &hints);
        let outcome = notes.handle_key(key, list.height, list.width);
        match outcome {
            TodoOutcome::Ignored => false,
            TodoOutcome::Consumed => true,
            TodoOutcome::Changed => {
                let todo = notes.todo().clone();
                self.save_notes(&todo);
                true
            }
            TodoOutcome::Cancelled => {
                self.notes = None;
                true
            }
            TodoOutcome::Go(path, line) => {
                self.notes = None;
                self.go_to_note(&path, line);
                true
            }
        }
    }

    /// Opens the file a note is about, at the line it is about.
    ///
    /// The same door a search result and a symbol go through, which is what
    /// makes the place a note points at behave like every other place obelus
    /// sends a reader: the file opens, the jump is recorded so `ctrl+o`
    /// comes back, and a file that will not open leaves them where they
    /// were and says so.
    fn go_to_note(&mut self, path: &PathBuf, line: LineNumber) {
        let full = self.working_directory.join(path);
        let line = u32::try_from(line.get()).unwrap_or(u32::MAX);
        self.go_to(&full, line, 0);
    }
}
