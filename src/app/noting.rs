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
    ///
    /// It reads the file, so it must not be called on a view that is already
    /// open: making room writes the old one down first, but the reader's
    /// place in the list would still be lost for nothing. `add_todo` checks,
    /// which is why.
    pub fn open_todo(&mut self) {
        self.make_room(Room::Region);
        let todo = Todo::read(&self.working_directory);
        // Names given to notes that had none go back to the file now, not
        // the next time something happens to write it: a name minted and
        // not written is a name minted again on the next open, and nothing
        // could be keyed to one.
        if todo.minted {
            self.save_notes(&todo);
        }
        let where_now = self.where_the_notes_point(&todo);
        // How wide a note's text is here, and whether it wraps: the rows
        // depend on both, and the view has to be laid out before anything
        // asks it how many rows it has.
        let laid = self.notes_laid_out();
        self.notes = Some(TodoView::new(todo, where_now, laid));
        // Heard about for as long as the page is showing, which is the
        // window that matters: obelus writes the whole file from what it
        // holds, so a change made while the reader has the list open is a
        // change the next save would put back the way it was.
        if let Some(watcher) = self.watcher.as_mut()
            && let Err(error) = watcher.watch(&crate::todo::path(&self.working_directory))
        {
            tracing::debug!(%error, "not watching what the tree means to come back to");
        }
    }

    /// Where each note points now, which is a question for git and the disk.
    fn where_the_notes_point(&self, todo: &Todo) -> Vec<Option<LineNumber>> {
        todo.notes
            .iter()
            .map(|note| {
                note.at
                    .as_ref()
                    .and_then(|at| crate::todo::where_now(&self.working_directory, at))
            })
            .collect()
    }

    /// Whether a path that changed is the file the notes are kept in.
    #[must_use]
    pub(super) fn is_the_notes_file(&self, path: &std::path::Path) -> bool {
        path == crate::todo::path(&self.working_directory)
    }

    /// Takes the file again, because somebody else wrote it.
    ///
    /// Only while the page is showing: with it shut there is nothing to
    /// keep in step, and the next open reads the file anyway.
    pub(super) fn reread_notes(&mut self) {
        if self.notes.is_none() {
            return;
        }
        let todo = Todo::read(&self.working_directory);
        if todo.minted {
            self.save_notes(&todo);
        }
        let where_now = self.where_the_notes_point(&todo);
        if let Some(notes) = self.notes.as_mut() {
            notes.reread(todo, where_now);
        }
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

    /// Puts a run of text into the note the caret is in.
    ///
    /// The door both pastes come through: the key, and the sequence a
    /// terminal sends when the reader uses its own paste. One change to the
    /// note either way, and the notes are written down after it.
    pub(super) fn paste_into_notes(&mut self, what: &str) {
        let laid = self.notes_laid_out();
        let Some(notes) = self.notes.as_mut() else {
            return;
        };
        notes.lay_out(laid.0, laid.1);
        notes.paste(what);
        let todo = notes.todo().clone();
        self.save_notes(&todo);
    }

    /// Writes the notes down, and says so if it cannot.
    pub(super) fn save_notes(&mut self, todo: &Todo) {
        if let Err(error) = todo.write(&self.working_directory) {
            tracing::warn!(%error, "the notes were not written");
            self.note = Some("the notes could not be written".to_string());
        }
    }

    /// How wide a note's own text is, and whether it wraps there.
    pub(super) fn notes_laid_out(&self) -> (u16, bool) {
        let room = crate::ui::todo::text_width_in(self.editor_area);
        (room, self.config().wrap)
    }

    /// Whatever a key means to the notes, if they are showing.
    pub(super) fn notes_key(&mut self, key: &KeyEvent) -> bool {
        let laid = self.notes_laid_out();
        let Some(notes) = self.notes.as_mut() else {
            return false;
        };
        notes.lay_out(laid.0, laid.1);
        let hints = crate::ui::todo::hints(notes);
        let list = crate::ui::todo::list_region(self.editor_area, &hints);
        let outcome = notes.handle_key(key, list.height);
        match outcome {
            TodoOutcome::Ignored => false,
            TodoOutcome::Consumed => true,
            // Nothing is saved: a copy takes a note away from the page and
            // changes nothing on it.
            TodoOutcome::Copy { text, what } => {
                self.copied(&text, what);
                true
            }
            // A cut did change the page, so it is written down as well.
            TodoOutcome::Cut { text, what } => {
                let todo = notes.todo().clone();
                self.cut_away(&text, what);
                self.save_notes(&todo);
                true
            }
            TodoOutcome::Paste => {
                match crate::clipboard::paste() {
                    Some(what) => self.paste_into_notes(&what),
                    None => self.note = Some("nothing to paste".to_string()),
                }
                true
            }
            TodoOutcome::Changed => {
                let todo = notes.todo().clone();
                self.save_notes(&todo);
                true
            }
            // Leaving keeps what was being written, which is why both of
            // these save: there is no moment where the reader said "done
            // with this note", so every way out of the view is one.
            TodoOutcome::Cancelled => {
                self.leave(Layer::Notes);
                true
            }
            TodoOutcome::Go(path, line) => {
                let todo = notes.todo().clone();
                self.save_notes(&todo);
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
