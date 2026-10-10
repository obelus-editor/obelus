//! What a project means to come back to.
//!
//! The application's half of [`obelus_todo`]: which project the notes
//! belong to, when they are read and written, and what a key that reaches one
//! of them does. The notes themselves, and the file, are that module's; how
//! they are walked and drawn are [`obelus_component::todo`] and
//! [`obelus_ui::todo`].
//!
//! Read when the view opens rather than held: Obelus is not the only thing
//! that can write the file -- the reader has an editor, and a second Obelus
//! is an ordinary thing to have open -- and a copy kept from startup would
//! be a copy that is wrong by the time anybody looks at it.

use std::path::PathBuf;

use obelus_component::todo::{TodoOutcome, TodoView};
use obelus_text::coordinates::LineNumber;
use obelus_todo::{At, Todo};

use crate::app::*;

impl App {
    /// The notes, while the reader is in them.
    ///
    /// Looked up in the list of what is open rather than held beside it:
    /// the notes are a document, so there is nowhere else for them to be.
    #[must_use]
    pub fn notes(&self) -> Option<&TodoView> {
        self.document(self.current?)?.notes()
    }

    /// And to change them.
    pub fn notes_mut(&mut self) -> Option<&mut TodoView> {
        let id = self.current?;
        self.document_mut(id)?.notes_mut()
    }

    /// Which document is the notes, if one of them is.
    ///
    /// There is one at most, the way there is one file per path: asking for
    /// the notes a second time goes back to them rather than reading the
    /// file again over the reader's place in it.
    pub(in crate::app) fn notes_document(&self) -> Option<DocumentId> {
        (0..self.documents.len())
            .map(DocumentId::new)
            .find(|id| self.document(*id).is_some_and(|it| it.notes().is_some()))
    }

    /// Opens what the project means to come back to.
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
        // Already open: go back to it. Reading the file again would put the
        // reader at the top of a list they had walked into, and lose
        // whatever they were part way through writing.
        if let Some(id) = self.notes_document() {
            if self.current != Some(id) {
                let from = self.here();
                self.record(from);
            }
            self.go_to_document(id);
            return;
        }
        // What the reader is in, before they are somewhere else: a
        // conversation about a note is how the list knows where to stand.
        let about = self
            .conversation()
            .and_then(|talk| talk.topic.note().cloned());
        let from = self.here();
        let Some(id) = self.put_the_notes_up(about) else {
            return;
        };
        self.record(from);
        self.go_to_document(id);
    }

    /// Opens the notes without going to them, with the caret in `about`
    /// where it names one, and says where they landed.
    pub(in crate::app) fn put_the_notes_up(
        &mut self,
        about: Option<obelus_todo::NoteId>,
    ) -> Option<DocumentId> {
        // Not opened at all where the file will not read: a page that
        // cannot be written is a page that lies, and this one would invite
        // the reader to type into a list that is not their list.
        let todo = self.the_notes_now()?;
        let where_now = self.where_the_notes_point(&todo);
        // How wide a note's text is here, and whether it wraps: the rows
        // depend on both, and the view has to be laid out before anything
        // asks it how many rows it has.
        let laid = self.notes_laid_out();
        let mut view = TodoView::new(todo, where_now, laid);
        // Opened from a conversation: standing on the note it came out of,
        // which is the return leg of that key. A note that has since been
        // taken away simply is not found, and the list opens at the top --
        // which is where a list with nothing to return to puts a reader.
        //
        // Entered rather than only selected: whenever the page lays itself
        // out again or reads its file again, what the selection goes back to
        // is the note the caret is in.
        if let Some(note) = about {
            view.put_caret_in(&note);
        }
        self.documents
            .push(Some(crate::app::document::Document::from(view)));
        // What this page is drawn from, read as it opens. The watches on
        // the three of them are settled from what is open rather than taken
        // here -- see `App::settle_the_watches` -- but a watch says what
        // happens next and not what was already there, and the first frame
        // is too late for either: the key after this one talks about a
        // note, and looks up the conversation written down against it.
        //
        // What the watches buy is freshness and not safety: a change of the
        // other window's is in the file whether this one hears about it or
        // not, and what keeps it there is that every write goes back
        // through the file -- see `App::do_to_the_notes`.
        self.reread_the_sessions();
        self.reread_who_holds_what();
        Some(DocumentId::new(self.documents.len() - 1))
    }

    /// Says the notes have just been typed into, so they are written down
    /// once the reader stops.
    ///
    /// Three hundred milliseconds, the same pause a slow grammar waits out:
    /// what it is for is the same, which is a burst of keys costing one
    /// piece of work rather than one each. Started again by every key, so
    /// what it measures is the reader stopping rather than the reader
    /// starting.
    ///
    /// A clock of its own -- [`crate::event::Pause`] -- and not the
    /// animation's tick, which is where this used to hang. That clock does
    /// not run over a network, and a note that reaches no file until the
    /// reader walks out of it is not something a reader should have to have
    /// a local terminal for.
    fn the_notes_will_settle(&mut self) {
        self.notes.notes_pause =
            self.come_back_in(Self::SETTLES_AFTER, crate::event::Event::NotesSettled);
    }

    /// Writes the notes down once the reader has stopped typing in them.
    ///
    /// The clock arriving *is* the reader having stopped, so there is no
    /// moment to check it against. Nothing else reaches here, and one that
    /// arrives with nothing waiting finds nothing to write.
    pub(in crate::app) fn settle_notes(&mut self) {
        self.notes.notes_pause = None;
        self.write_the_notes();
    }

    /// Writes what the notes hold, if the reader has them open.
    ///
    /// The one place the three moments share: the pause, closing the
    /// document, and leaving Obelus. A note is only ever in the file, so
    /// the pause is what makes the other two rare rather than what makes
    /// them unnecessary.
    pub(in crate::app) fn write_the_notes(&mut self) {
        let Some(changes) = self
            .the_notes_page()
            .map(obelus_component::todo::TodoView::take_changes)
        else {
            return;
        };
        self.do_to_the_notes(changes);
    }

    /// Does what the page has been doing to the file as it is *now*, and
    /// brings the page up to what came out.
    ///
    /// The one way anything the reader does reaches the disk. Not the page's
    /// copy written whole: another Obelus has this project open -- that is what
    /// a second window is for -- and a file written from a copy is that
    /// window's last minute taken back out, with neither reader told. What
    /// goes is the acts, done inside the lock to whatever the file says by
    /// then.
    ///
    /// A change about a note that is no longer there is said out loud. It
    /// means the other window took that note away while this page still
    /// showed it, and the honest thing is neither to let the key look
    /// broken nor to put the note back behind the reader's back.
    pub(in crate::app) fn do_to_the_notes(&mut self, changes: Vec<obelus_todo::Change>) {
        if !self.write_them_down(changes) {
            return;
        }
        // One of them was about a note that has gone. The reread hands the
        // reader back the note they had their hands on -- theirs to put in
        // again, because they are the one here to be surprised -- and what
        // is in it has still not reached the file. So it goes now: they
        // pressed a key that means "write this down", and once is not twice.
        //
        // This cannot go round again. What comes back is a note the file
        // does not have, which is put in rather than changed, and putting
        // one in never fails.
        let again = self
            .the_notes_page()
            .map(obelus_component::todo::TodoView::take_changes)
            .unwrap_or_default();
        self.write_them_down(again);
    }

    /// Does them, and says whether any was about a note that had gone.
    fn write_them_down(&mut self, changes: Vec<obelus_todo::Change>) -> bool {
        self.notes.notes_pause = None;
        if changes.is_empty() {
            return false;
        }
        match obelus_todo::change(&self.working_directory, |todo| {
            changes
                .iter()
                .filter(|change| !todo.apply(change))
                .cloned()
                .collect::<Vec<obelus_todo::Change>>()
        }) {
            Ok((todo, missed)) => {
                if !missed.is_empty() {
                    self.wrong("That note was taken away elsewhere".to_string());
                }
                // The names whose words were in that, so the page knows
                // which of what it is holding is the only copy there is.
                let unwritten: Vec<obelus_todo::NoteId> = missed
                    .iter()
                    .filter_map(obelus_todo::Change::words)
                    .cloned()
                    .collect();
                let where_now = self.where_the_notes_point_again(&todo);
                self.the_notes_are_now(todo, where_now, &unwritten);
                !unwritten.is_empty()
            }
            Err(why) => {
                self.the_notes_will_not(&why);
                // Not lost with the attempt. Nothing was written, so these
                // are still the only account there is of what the reader
                // did, and they go with whatever writes next -- which for a
                // file that will not read is the first key after they have
                // fixed it.
                if let Some(notes) = self.the_notes_page() {
                    notes.put_back(changes);
                }
                false
            }
        }
    }

    /// The page the notes are on, wherever it is.
    ///
    /// By document rather than by what the reader is looking at: the notes
    /// are written down by the clock, by an agent's tool, and by Obelus
    /// leaving, and at none of those moments is the page necessarily the one
    /// on screen.
    fn the_notes_page(&mut self) -> Option<&mut TodoView> {
        self.notes_document()
            .and_then(|at| self.documents.get_mut(at.get()))
            .and_then(Option::as_mut)
            .and_then(Document::notes_mut)
    }

    /// Says why the notes did not take a change, and in which of the two
    /// ways it did not.
    ///
    /// Two sentences because they ask two different things of the reader.
    /// One is theirs to fix and Obelus is holding off until they do -- the
    /// file is there and says something Obelus cannot read, and writing over
    /// it would be trading what they wrote for whatever this session happens
    /// to be holding. The other is a disk, and nothing they type will help.
    fn the_notes_will_not(&mut self, why: &obelus_todo::NotChanged) {
        tracing::warn!(%why, "the notes were not written");
        // Short, because the status row is one row: which file and what went
        // wrong are in the log, where there is room for them.
        self.wrong(match why {
            obelus_todo::NotChanged::Unreadable(_) => {
                "The notes will not read, so none are written".to_string()
            }
            obelus_todo::NotChanged::Unwritable(_) => "The notes could not be written".to_string(),
        });
    }

    /// The file, with any name it was missing written back into it.
    ///
    /// Reading mints a name for a note that has none, and a name minted and
    /// not written is a name minted again on the next open -- nothing could
    /// be keyed to one. Through the same door as everything else, which
    /// reads the file again inside the lock, so what comes back is the file
    /// rather than this Obelus's guess at it.
    fn the_notes_now(&mut self) -> Option<Todo> {
        let todo = match obelus_todo::read(&self.working_directory) {
            obelus_todo::Reading::Nothing => return Some(Todo::default()),
            obelus_todo::Reading::Notes(todo) => {
                if let Some(path) = obelus_todo::path(&self.working_directory) {
                    self.nothing_wrong_with(&path);
                }
                todo
            }
            // Nothing is shown and nothing is written. An empty page is not
            // what this file says -- it is what Obelus can make of a file it
            // cannot read -- and a reader who starts writing notes into it
            // has begun replacing their own list one note at a time.
            obelus_todo::Reading::Unreadable(why, at) => {
                tracing::warn!(why, "the notes will not read");
                self.wrong("The notes will not read".to_string());
                // And on the file itself. It is a file a reader opens --
                // they write notes into it from the page and edit it by
                // hand -- so being told the whole list will not read
                // without being told which line is a reader reading it
                // all.
                if let Some(path) = obelus_todo::path(&self.working_directory) {
                    self.nothing_wrong_with(&path);
                    self.obelus_says(
                        &path,
                        at,
                        obelus_lsp::trouble::Severity::Error,
                        &format!("The notes will not read\n{why}"),
                    );
                }
                return None;
            }
        };
        if !todo.minted {
            return Some(todo);
        }
        match obelus_todo::change(&self.working_directory, |_| ()) {
            Ok((todo, ())) => Some(todo),
            Err(why) => {
                self.the_notes_will_not(&why);
                Some(todo)
            }
        }
    }

    /// Where each note points now, which is a question for git and the disk.
    fn where_the_notes_point(&self, todo: &Todo) -> Vec<Option<LineNumber>> {
        todo.notes
            .iter()
            .map(|note| {
                note.at
                    .as_ref()
                    .and_then(|at| obelus_todo::where_now(&self.working_directory, at))
            })
            .collect()
    }

    /// The same, asked only about the notes nobody has an answer for yet.
    ///
    /// Where a note points is the file as one commit had it against the file
    /// as it is -- a walk of the history, per note. Asked again for every
    /// note every time one is ticked off, that walk would be behind the
    /// space bar. What makes those answers stale is the *file the note
    /// points into* moving, which is the watcher's news and is where they
    /// are all worked out again; a note being ticked off is not.
    fn where_the_notes_point_again(&self, todo: &Todo) -> Vec<Option<LineNumber>> {
        let known = self
            .notes_document()
            .and_then(|id| self.document(id))
            .and_then(Document::notes)
            .map(obelus_component::todo::TodoView::places)
            .unwrap_or_default();
        todo.notes
            .iter()
            .map(|note| match known.iter().find(|(id, _)| *id == note.id) {
                Some((_, line)) => *line,
                None => note
                    .at
                    .as_ref()
                    .and_then(|at| obelus_todo::where_now(&self.working_directory, at)),
            })
            .collect()
    }

    /// Whether a path that changed is the file the notes are kept in.
    #[must_use]
    pub(in crate::app) fn is_the_notes_file(&self, path: &std::path::Path) -> bool {
        obelus_todo::path(&self.working_directory).is_some_and(|notes| path == notes)
    }

    /// Takes the file again, because somebody else wrote it.
    ///
    /// Only while the page is open, which is not the same as its being on
    /// screen. Shut, there is nothing to keep in step and the next open
    /// reads the file anyway.
    ///
    /// Freshness, not safety. What the other window wrote is in the file
    /// whether this one hears about it or not, and it stays there because
    /// every write from here goes back through the file rather than over it
    /// -- [`App::do_to_the_notes`]. This is so that the page in front of the
    /// reader says what the file says before they press anything, rather
    /// than on the keystroke after.
    pub(in crate::app) fn reread_notes(&mut self) {
        // The notes wherever they are open, not only where the reader is
        // standing. An agent writes notes while the reader is talking to
        // it, which is to say while the conversation is the document on
        // screen and the notes are one of the others -- and `notes()`
        // answers about the document on screen, so the page that most has
        // to hear about the write is the one page that could not reach.
        if self.notes_document().is_none() {
            return;
        }
        // The page keeps what it has where the file will not read. What is
        // on it came out of the file and is still the best account of it
        // there is; what must not happen is writing it back, and that is
        // refused where it is done rather than guarded here.
        let Some(todo) = self.the_notes_now() else {
            return;
        };
        // Every one of them worked out again, unlike a write of the notes'
        // own: what arrives here is the file having moved, and the file
        // moving is exactly what changes where a note points.
        let where_now = self.where_the_notes_point(&todo);
        // Nothing of the reader's was lost on the way here: this is the
        // file having moved under a page that has written down everything
        // it was asked to.
        self.the_notes_are_now(todo, where_now, &[]);
    }

    /// Puts the notes that were just written in front of the reader.
    ///
    /// The page wherever it is open, not only where the reader is standing:
    /// an agent writes notes while the reader is talking to it, which is to
    /// say while the conversation is the document on screen.
    ///
    /// And the page only. A conversation hanging under a note that has gone
    /// is not shut here, which it was: the file moving is usually another
    /// window's write, and closing the document that write happened to be
    /// about took the reader out of the conversation they were standing in
    /// -- `close` puts them on the nearest open document, which is the
    /// notes page the conversation was opened from -- and let go of its
    /// session on the way, so there was nothing to go back to. A note
    /// cleared of its words is enough to do it, because a note that says
    /// nothing is not written to the file at all.
    ///
    /// The same rule a file deleted in another window follows: the document
    /// keeps what it has and says what it can -- the key back to the note
    /// goes with it -- and whether to close it is the reader's. What is
    /// swept against the names the file has is the *table* of
    /// conversations, which is `obelus_agent::acp::sessions::change`'s own
    /// business and happens wherever that is written.
    fn the_notes_are_now(
        &mut self,
        todo: Todo,
        where_now: Vec<Option<LineNumber>>,
        unwritten: &[obelus_todo::NoteId],
    ) {
        if let Some(notes) = self
            .notes_document()
            .and_then(|at| self.documents.get_mut(at.get()))
            .and_then(Option::as_mut)
            .and_then(Document::notes_mut)
        {
            notes.reread(todo, where_now, unwritten);
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
        if self.notes().is_none() {
            self.open_todo();
        }
        if let Some(notes) = self.notes_mut() {
            notes.write_new(at);
        }
    }

    /// The place a note made now would be about.
    ///
    /// Relative to the project, because every path Obelus writes down is: an
    /// absolute one is about one machine, and the file it names is about the
    /// project.
    ///
    /// Nothing while the notes themselves are showing: there is no line
    /// under a list, and a note made from here is about the project.
    fn here_now(&self) -> Option<At> {
        if self.notes().is_some() {
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
            commit: obelus_todo::at_commit(&self.working_directory),
        })
    }

    /// Puts a run of text into the note the caret is in.
    ///
    /// The door both pastes come through: the key, and the sequence a
    /// terminal sends when the reader uses its own paste. One change to the
    /// note either way.
    ///
    /// Not written down here, because a paste is typing: it goes into the
    /// box, and what is in the box reaches the note when the reader leaves
    /// it, which is the moment a typed paragraph is written down too. This
    /// did save, and saved the note *without* what had just been pasted
    /// into it -- the box is not the note until `keep` takes it -- so the
    /// file gained a note with nothing in it, and Obelus, which hears about
    /// its own writes while the page is open, read that back over the words
    /// the reader was looking at.
    pub(in crate::app) fn paste_into_notes(&mut self, what: &str) {
        let laid = self.notes_laid_out();
        let Some(notes) = self.notes_mut() else {
            return;
        };
        notes.lay_out(laid.0, laid.1);
        notes.paste(what);
    }

    /// Writes the notes down, and says so if it cannot.
    /// Does what an agent asked to the notes, and says what came of it.
    ///
    /// Read from the file and written back whole, here rather than on the
    /// server's own thread: the loop is the one writer, and two of them
    /// reading and writing a whole file is one losing a change it never
    /// saw. The view is rebuilt from what was written, so a reader with the
    /// page open sees it arrive rather than finding it next time they look.
    ///
    /// Said in words because the words go back to the agent, which has no
    /// use for a code and every use for "there is no note by that name any
    /// more".
    pub(in crate::app) fn change_the_notes(&mut self, doing: obelus_todo::Doing) -> String {
        // A note another Obelus has the conversation of is not this one's
        // to change, and the tool is told rather than quietly obeyed: the
        // agent says why in the transcript, which is where a reader who
        // asked for this is looking. The same refusal the keys make, so an
        // agent cannot do what the reader standing in front of it cannot.
        //
        // Nothing for `Doing::Add`, which hangs a new note under a locked
        // one and changes that one's words, its box and its depth not at
        // all -- the same reason `alt+up` is allowed to carry a locked
        // child past a neighbour.
        let about = match &doing {
            obelus_todo::Doing::Finish(id) => Some(id),
            obelus_todo::Doing::Reword { note, .. } => Some(note),
            obelus_todo::Doing::Add { .. } => None,
        };
        if about.is_some_and(|id| self.the_conversation_is_elsewhere(id)) {
            return "another Obelus has the conversation about that note open, \
                    so it is not this one's to change"
                .to_string();
        }
        let done = obelus_todo::change(&self.working_directory, |todo| match doing {
            obelus_todo::Doing::Add { notes, under } => {
                let written: Vec<(String, u16)> = notes
                    .into_iter()
                    .map(|(said, depth)| (obelus_todo::trimmed(&said), depth))
                    .filter(|(said, _)| !said.trim().is_empty())
                    .collect();
                if written.is_empty() {
                    return "there was nothing there to write down".to_string();
                }
                // Where they go, and how deep the first of them is. Under a
                // note means after the whole of what already hangs under it
                // -- between a note and its children is the one place that
                // adopts what is put in it.
                let (at, beneath) = match under {
                    Some(name) => {
                        let Some(at) = todo.notes.iter().position(|note| note.id == name) else {
                            return "there is no note by that name any more".to_string();
                        };
                        (at + 1 + todo.under(at), todo.notes[at].depth + 1)
                    }
                    None => (todo.notes.len(), 0),
                };
                let how_many = written.len();
                for (offset, (said, depth)) in written.into_iter().enumerate() {
                    // Clamped twice, because what is written has to be what
                    // reading it gives back: one deeper than the note above
                    // at the most, and never past the deepest a note may be.
                    // An agent counts from the top of its own batch and
                    // cannot know what it is landing under.
                    let above = todo
                        .notes
                        .get((at + offset).wrapping_sub(1))
                        .map_or(0, |note| note.depth + 1);
                    let depth = depth
                        .saturating_add(beneath)
                        .min(above)
                        .min(obelus_todo::DEEPEST);
                    todo.notes.insert(
                        at + offset,
                        obelus_todo::Note {
                            id: obelus_todo::NoteId::mint(),
                            said,
                            done: false,
                            at: None,
                            depth,
                        },
                    );
                }
                format!("written down: {how_many}")
            }
            obelus_todo::Doing::Finish(id) => {
                let Some(note) = todo.notes.iter_mut().find(|note| note.id == id) else {
                    return "there is no note by that name any more".to_string();
                };
                note.done = true;
                "ticked off".to_string()
            }
            obelus_todo::Doing::Reword { note: id, said } => {
                let Some(note) = todo.notes.iter_mut().find(|note| note.id == id) else {
                    return "there is no note by that name any more".to_string();
                };
                let said = obelus_todo::trimmed(&said);
                // A note that says nothing is one reading the file drops, so
                // rewording to nothing is taking a note away through another
                // door -- the one act that is the reader's.
                if said.trim().is_empty() {
                    return "a note cannot be made to say nothing".to_string();
                }
                // Its words and nothing else: the rest of what a note is
                // was not what the agent was asked about.
                note.said = said;
                // The agent that asked for this is told the note has been
                // rewritten on its next message, quoting words it wrote
                // itself. Which is right, and not worth suppressing: the
                // tools name no conversation -- the door they come through
                // is the one the protocol puts no session on -- so Obelus
                // cannot tell which agent asked, and what landed is not
                // always what was asked for anyway.
                "reworded".to_string()
            }
        });
        match done {
            Ok((todo, said)) => {
                // And the page, wherever it is open. A reader looking at
                // their notes while an agent writes one should watch it
                // arrive -- and a reader who is talking to the agent
                // instead, with the notes open behind the conversation,
                // must not be left holding a page that says something else.
                let where_now = self.where_the_notes_point_again(&todo);
                self.the_notes_are_now(todo, where_now, &[]);
                said
            }
            Err(why) => {
                tracing::warn!(%why, "the notes were not written");
                // Which of the two, because the agent can say it to the
                // reader and one of them is the reader's to fix. An agent
                // told only that it did not work will try again, and try
                // again against the same unparseable file.
                match why {
                    obelus_todo::NotChanged::Unreadable(_) => {
                        "the notes file will not read, so nothing was written down -- \
                         it is the reader's to fix"
                            .to_string()
                    }
                    obelus_todo::NotChanged::Unwritable(_) => {
                        "the notes could not be written".to_string()
                    }
                }
            }
        }
    }

    /// How wide a note's own text is, and whether it wraps there.
    pub(in crate::app) fn notes_laid_out(&self) -> (u16, bool) {
        let room = obelus_ui::todo::text_width_in(self.editor_area);
        (room, self.config().wrap)
    }

    /// Whatever a key means to the notes, if they are showing.
    pub(in crate::app) fn notes_key(&mut self, key: &KeyEvent) -> bool {
        let laid = self.notes_laid_out();
        // Read before the view is borrowed: a page of the list is measured
        // from the region it is drawn in, and the region belongs to the
        // application rather than to the notes.
        let area = self.editor_area;
        // Before the notes are borrowed to take the key: which notes
        // another Obelus has the conversation of decides what this key may
        // change and which keys the foot offers, and the rows the list gets
        // are what is left under the foot. Said here as well as from the
        // frame, because a key acts before the next frame -- the page is
        // opened by one key and typed into by the next.
        let elsewhere = self.which_notes_are_elsewhere();
        let Some(notes) = self.notes_mut() else {
            return false;
        };
        notes.lay_out(laid.0, laid.1);
        notes.these_are_elsewhere(elsewhere);
        let hints = obelus_ui::todo::hints(notes);
        let list = obelus_ui::todo::list_region(area, &hints);
        let outcome = notes.handle_key(key, list.height);
        // Typing, which is the one change that is not one act: every other
        // way the notes change is written the moment it happens, and this
        // one is written when the reader stops. Asked of whether a note is
        // open to be typed in rather than of the key, because a key that
        // moved the caret inside one is a key that may have been part of
        // typing -- and one write after a pause costs nothing.
        //
        // Or where a key has already changed something and said nothing
        // about it -- walking out of a note writes what was in it down --
        // because a change that reached the page and no disk is one crash
        // from never having happened.
        let waiting = notes.writing().is_some() || notes.waiting();
        match outcome {
            TodoOutcome::Ignored => false,
            TodoOutcome::Consumed => {
                if waiting {
                    self.the_notes_will_settle();
                }
                true
            }
            // Nothing is saved: a copy takes a note away from the page and
            // changes nothing on it.
            TodoOutcome::Copy { text, what } => {
                self.copied(&text, what);
                true
            }
            // A cut did change the page, so it is written down as well.
            TodoOutcome::Cut { text, what } => {
                let changes = notes.take_changes();
                self.cut_away(&text, what);
                self.do_to_the_notes(changes);
                true
            }
            TodoOutcome::Paste => {
                match obelus_clipboard::paste() {
                    Some(what) => self.paste_into_notes(&what),
                    None => self.wrong("Nothing to paste".to_string()),
                }
                true
            }
            TodoOutcome::Changed => {
                let changes = notes.take_changes();
                self.do_to_the_notes(changes);
                true
            }
            // Leaving keeps what was being written, which is why both of
            // these save: there is no moment where the reader said "done
            // with this note", so every way out of the view is one.
            // Talk about one. The notes are written down first, because
            // leaving them *is* finishing them and this leaves them.
            TodoOutcome::Talk(note) => {
                let changes = notes.take_changes();
                self.do_to_the_notes(changes);
                self.talk_about(&note);
                true
            }
            // Escape, which used to close the page. A document is not
            // closed by escape: escape leaves whatever is *over* what is
            // being read, and nothing is over this. What it does here is
            // write what has been typed, which is what leaving used to be
            // the moment for.
            TodoOutcome::Cancelled => {
                let changes = notes.take_changes();
                self.do_to_the_notes(changes);
                true
            }
            TodoOutcome::Go(path, line) => {
                // The notes stay open. They were closed here while they
                // were a page, because a page is what the reader was in
                // and they are going somewhere else -- but a document they
                // came from is a document they come back to, standing on
                // the note that sent them.
                let changes = notes.take_changes();
                self.do_to_the_notes(changes);
                self.go_to_note(&path, line);
                true
            }
        }
    }

    /// Opens the file a note is about, at the line it is about.
    ///
    /// The same door a search result and a symbol go through, which is what
    /// makes the place a note points at behave like every other place Obelus
    /// sends a reader: the file opens, the jump is recorded so `alt+left`
    /// comes back, and a file that will not open leaves them where they
    /// were and says so.
    fn go_to_note(&mut self, path: &PathBuf, line: LineNumber) {
        let full = self.working_directory.join(path);
        let line = u32::try_from(line.get()).unwrap_or(u32::MAX);
        self.go_to(&full, line, 0);
    }
}
