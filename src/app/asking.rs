//! The questions obelus stops to ask, and what answering one does.
//!
//! What a question *is* lives in [`crate::question`], and the list it turns
//! into is [`Picker::asking`]. What is here is which questions get asked,
//! in what words, and what each answer does.
//!
//! They are together in one file on purpose. A question is a sentence the
//! reader has to understand in one pass under a key they pressed by
//! accident, and three of them written next to each other read as three of
//! a kind; three of them written where each is asked would drift apart.

use super::*;
use crate::question::{Answer, Closing, Leaving, Question, Saving, Writing};

impl App {
    /// Puts a question on the status bar and waits for an answer.
    ///
    /// Over whatever list was open, which is then gone: a question is not a
    /// detour from choosing a file, it is what the key the reader pressed
    /// turned into, and a list coming back underneath the answer would be
    /// one they had already left.
    pub(super) fn stop_to_ask(&mut self, question: Question) {
        self.show_list(Picker::asking(&question));
    }

    /// Asks before closing a document with something unwritten in it.
    ///
    /// Worth asking about at all because a closed buffer takes its undo with
    /// it: there is no other way back to what was in it.
    pub(super) fn ask_before_closing(&mut self, id: DocumentId) {
        self.stop_to_ask(
            Question::new(format!("{} is unsaved", self.buffer_path(id)))
                .way("save and close it", Answer::Closing(id, Closing::Save))
                .way(
                    "close it without saving",
                    Answer::Closing(id, Closing::Discard),
                ),
        );
    }

    /// Asks before leaving with unwritten changes anywhere.
    pub(super) fn ask_before_leaving(&mut self, unsaved: usize) {
        let what = match unsaved {
            // Which one, when there is only one. A count is all a prompt
            // can say about several, but about a single file it would be
            // saying less than it knows -- and the reader is about to
            // decide whether to write it.
            1 => match self.first_unsaved() {
                Some(id) => format!("{} is unsaved", self.buffer_path(id)),
                None => "1 file is unsaved".to_string(),
            },
            many => format!("{many} files are unsaved"),
        };
        self.stop_to_ask(
            Question::new(what)
                .way(
                    "save everything and leave",
                    Answer::Leaving(Leaving::SaveAll),
                )
                .way("leave without saving", Answer::Leaving(Leaving::Discard)),
        );
    }

    /// Asks before writing over a file that moved while it was being edited.
    ///
    /// The one question with no safe answer: each way out keeps one of the
    /// two versions and loses the other, and only the reader knows which one
    /// matters. Which is why it says whose is whose rather than "yes" and
    /// "no".
    pub(super) fn ask_before_saving(&mut self, id: DocumentId) {
        self.stop_to_ask(
            Question::new(format!("{} changed on disk", self.buffer_path(id)))
                .way("save mine over it", Answer::Saving(id, Saving::Mine))
                .saying("loses what was written there")
                .way("take what is on disk", Answer::Saving(id, Saving::Theirs))
                // Not lost for good, which is the whole difference between
                // this way out and the one above it.
                .saying("undo brings yours back"),
        );
    }

    /// Asks before saving a file somebody else took away.
    ///
    /// Its own question because two of the answers to a file that merely
    /// changed are not available here: there is nothing on disk to take
    /// instead, and nothing there to write over.
    pub(super) fn ask_before_writing_back(&mut self, id: DocumentId) {
        self.stop_to_ask(
            Question::new(format!("{} was deleted", self.buffer_path(id)))
                .way("write it back", Answer::Writing(id, Writing::Back))
                .way(
                    "close it and let it go",
                    Answer::Writing(id, Writing::LetGo),
                )
                .saying("loses your changes"),
        );
    }

    /// Does what a question was answered with.
    ///
    /// One arm per answer, and every arm is reachable: the point of each
    /// question having its own answers is that this match cannot grow arms
    /// for pairings that never happen.
    pub(super) fn answered(&mut self, answer: Answer) {
        match answer {
            // Written without laying it out first, even where the reader
            // asked for formatting on save: laying out is a round trip to a
            // server, and this one has a close waiting on the other side of
            // it. A reader closing a file is leaving it, not polishing it.
            Answer::Closing(id, Closing::Save) => {
                if self.write_now(id.get()) {
                    self.close(id);
                }
            }
            Answer::Closing(id, Closing::Discard) => self.close(id),
            Answer::Leaving(Leaving::SaveAll) => self.save_everything_and_leave(),
            Answer::Leaving(Leaving::Discard) => self.should_quit = true,
            Answer::Saving(id, Saving::Mine) => self.save_now(id.get()),
            Answer::Saving(id, Saving::Theirs) => self.take_what_is_on_disk(id.get()),
            Answer::Writing(id, Writing::Back) => self.save_now(id.get()),
            Answer::Writing(id, Writing::LetGo) => self.close(id),
            // Including escape, which is the same answer by a shorter route.
            Answer::Cancel => {}
        }
    }

    /// The first document with something unwritten in it.
    ///
    /// Which is the only one, where the caller has counted one.
    fn first_unsaved(&self) -> Option<DocumentId> {
        (0..self.documents.len()).map(DocumentId::new).find(|id| {
            self.documents[id.get()]
                .as_ref()
                .is_some_and(Buffer::is_dirty)
        })
    }

    /// Which document a question is about, in the words the prompt says it
    /// in.
    ///
    /// The path relative to the working directory, which is how every other
    /// list in obelus names a file -- and not the bare file name, because a
    /// reader may have four files called `mod.rs` open and only one of them
    /// is the one about to be closed. It goes in the prompt rather than in
    /// the block above the ways out: a block that said the path while the
    /// prompt said the name would be one fact drawn twice, taking two rows
    /// and a rule to do it.
    fn buffer_path(&self, id: DocumentId) -> String {
        self.documents
            .get(id.get())
            .and_then(Option::as_ref)
            .map_or_else(String::new, |buffer| {
                relative(buffer.path(), &self.working_directory)
            })
    }

    /// Writes every unwritten document, and leaves if they all went.
    ///
    /// A save that failed is the whole reason for asking: leaving anyway
    /// would throw away exactly what the reader just said to keep. So the
    /// first failure stops it, and says which file, because "saving failed"
    /// with four files open names nothing the reader can act on.
    fn save_everything_and_leave(&mut self) {
        for index in 0..self.documents.len() {
            let unwritten = self.documents[index]
                .as_ref()
                .is_some_and(|buffer| buffer.is_dirty() && buffer.content().is_file());
            if unwritten && !self.write_now(index) {
                return;
            }
        }
        self.should_quit = true;
    }
}
