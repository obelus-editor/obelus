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
    todo::{At, Note, Todo},
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

    /// Asks for something to come back to, about the line being read.
    ///
    /// On the status bar, because a note is made *while* reading and a view
    /// that covered the code would be asking the reader to remember what
    /// they were looking at. The same prompt a line number is typed on.
    pub fn open_todo_prompt(&mut self) {
        self.prompt = Some(Prompt::new(PromptKind::Todo));
    }

    /// Writes down what was typed on the prompt.
    ///
    /// With the place when there is a file under the prompt, and without one
    /// when the notes themselves are what is showing: a note made from the
    /// list is about the project, and there is no line to put on it.
    pub(super) fn note_down(&mut self, said: &str) {
        if said.trim().is_empty() {
            return;
        }
        let at = self.notes.is_none().then(|| self.here_now()).flatten();
        let note = Note {
            said: said.to_string(),
            done: false,
            at,
        };
        match self.notes.as_mut() {
            Some(notes) => notes.add(note),
            None => {
                let mut todo = Todo::read(&self.working_directory);
                todo.notes.push(note);
                self.save_notes(&todo);
                self.note = Some("written down".to_string());
                return;
            }
        }
        let todo = self.notes.as_ref().map(|notes| notes.todo().clone());
        if let Some(todo) = todo {
            self.save_notes(&todo);
        }
    }

    /// The place a note made now would be about.
    ///
    /// Relative to the tree, because every path obelus writes down is: an
    /// absolute one is about one machine, and the file it names is about the
    /// project.
    fn here_now(&self) -> Option<At> {
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
        let hints = crate::ui::todo::hints();
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
            TodoOutcome::Asking => {
                self.open_todo_prompt();
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
