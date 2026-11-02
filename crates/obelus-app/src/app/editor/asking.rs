//! The questions Obelus stops to ask, and what answering one does.
//!
//! What a question *is* lives in [`obelus_component::question`], and the list
//! it turns into is [`Picker::asking`]. What is here is which questions get
//! asked, in what words, and what each answer does.
//!
//! They are together in one file on purpose. A question is a sentence the
//! reader has to understand in one pass under a key they pressed by
//! accident, and three of them written next to each other read as three of
//! a kind; three of them written where each is asked would drift apart.

use obelus_component::question::{Answer, Closing, Leaving, Question, Saving, Writing};
use obelus_i18n::editing as said;

use crate::app::*;

impl App {
    /// Puts a question on the status bar and waits for an answer.
    ///
    /// Over whatever list was open, which is then gone: a question is not a
    /// detour from choosing a file, it is what the key the reader pressed
    /// turned into, and a list coming back underneath the answer would be
    /// one they had already left.
    pub(in crate::app) fn stop_to_ask(&mut self, question: Question) {
        self.show_list(Picker::asking(&question));
    }

    /// Asks before closing a document with something unwritten in it.
    ///
    /// Worth asking about at all because a closed buffer takes its undo with
    /// it: there is no other way back to what was in it.
    pub(in crate::app) fn ask_before_closing(&mut self, id: DocumentId) {
        self.stop_to_ask(
            Question::new(said::unsaved_file(self.file_path(id)))
                .way(said::save_and_close(), Answer::Closing(id, Closing::Save))
                .way(
                    said::close_without_saving(),
                    Answer::Closing(id, Closing::Discard),
                ),
        );
    }

    /// Asks before leaving with unwritten changes anywhere.
    pub(in crate::app) fn ask_before_leaving(&mut self, unsaved: usize) {
        // And what else leaving stops, said in the same question: either
        // answer leaves, and leaving takes the programs with it.
        let what = self.what_would_be_lost(unsaved, self.terminals_running(), None);
        self.stop_to_ask(
            Question::new(what)
                .way(said::save_all_and_quit(), Answer::Leaving(Leaving::SaveAll))
                .way(
                    said::quit_without_saving(),
                    Answer::Leaving(Leaving::Discard),
                ),
        );
    }

    /// Asks before going to another worktree with something unwritten here.
    ///
    /// The question leaving asks, in the words of going: this tree is still
    /// there to write what is unwritten into, and the reader says whether it
    /// is.
    pub(in crate::app) fn ask_before_switching(&mut self, unsaved: usize) {
        self.stop_to_ask(
            Question::new(self.what_would_be_lost(unsaved, 0, None))
                .way(
                    said::save_all_and_switch(),
                    Answer::Switching(Leaving::SaveAll),
                )
                .way(
                    said::switch_without_saving(),
                    Answer::Switching(Leaving::Discard),
                )
                .saying(said::changes_will_be_lost()),
        );
    }

    /// Asks before letting go of the project with something unwritten in it.
    ///
    /// The question going to another worktree asks, for the same reason:
    /// the tree is still there to write into.
    pub(in crate::app) fn ask_before_closing_the_project(&mut self, unsaved: usize) {
        // And what else closing stops, as leaving says it: either answer
        // closes, and closing takes the programs with it.
        let what = self.what_would_be_lost(unsaved, self.terminals_running(), None);
        self.stop_to_ask(
            Question::new(what)
                .way(
                    said::save_all_and_close(),
                    Answer::ClosingTheProject(Leaving::SaveAll),
                )
                .way(
                    said::close_without_saving(),
                    Answer::ClosingTheProject(Leaving::Discard),
                )
                .saying(said::changes_will_be_lost()),
        );
    }

    /// Asks before closing everything but one document, with something
    /// unwritten among the rest.
    ///
    /// The question closing the project asks, about everything but `keep`:
    /// what is being read stays, so it is neither counted nor written.
    pub(in crate::app) fn ask_before_closing_the_others(
        &mut self,
        keep: DocumentId,
        unsaved: usize,
    ) {
        let what = self.what_would_be_lost(
            unsaved,
            self.terminals_running_except(Some(keep)),
            Some(keep),
        );
        self.stop_to_ask(
            Question::new(what)
                .way(
                    said::save_all_and_close_others(),
                    Answer::ClosingTheOthers(keep, Leaving::SaveAll),
                )
                .way(
                    said::close_others_without_saving(),
                    Answer::ClosingTheOthers(keep, Leaving::Discard),
                )
                .saying(said::changes_will_be_lost()),
        );
    }

    /// What a question about unwritten work says would be lost: the files,
    /// and the programs that would stop with them -- leaving `except` out,
    /// where that one stays.
    ///
    /// One sentence for every question that asks it, so that four of them
    /// say it the same way.
    fn what_would_be_lost(
        &self,
        unsaved: usize,
        running: usize,
        except: Option<DocumentId>,
    ) -> String {
        // Which one, when there is only one. A count is all a prompt can
        // say about several, but about a single file it would be saying
        // less than it knows -- and the reader is about to decide whether
        // to write it.
        let one = match unsaved {
            1 => self
                .first_unsaved_except(except)
                .map(|id| self.file_path(id)),
            _ => None,
        };
        match (one, running) {
            (Some(path), 0) => said::unsaved_file(path),
            (Some(path), running) => said::unsaved_file_and_terminals(path, running),
            (None, 0) => said::unsaved_files(unsaved),
            (None, running) => said::unsaved_files_and_terminals(unsaved, running),
        }
    }

    /// Asks before writing over a file that moved while it was being edited.
    ///
    /// The one question with no safe answer: each way out keeps one of the
    /// two versions and loses the other, and only the reader knows which one
    /// matters. Which is why it says whose is whose rather than "yes" and
    /// "no".
    pub(in crate::app) fn ask_before_saving(&mut self, id: DocumentId) {
        self.stop_to_ask(
            Question::new(said::changed_on_disk(self.file_path(id)))
                .way(said::overwrite(), Answer::Saving(id, Saving::Mine))
                .saying(said::overwrite_about())
                .way(said::use_the_disk(), Answer::Saving(id, Saving::Theirs))
                // Not lost for good, which is the whole difference between
                // this way out and the one above it.
                .saying(said::use_the_disk_about()),
        );
    }

    /// Asks before saving a file somebody else took away.
    ///
    /// Its own question because two of the answers to a file that merely
    /// changed are not available here: there is nothing on disk to take
    /// instead, and nothing there to write over.
    pub(in crate::app) fn ask_before_writing_back(&mut self, id: DocumentId) {
        self.stop_to_ask(
            Question::new(said::deleted(self.file_path(id)))
                .way(said::write_it_back(), Answer::Writing(id, Writing::Back))
                .way(
                    said::close_without_saving(),
                    Answer::Writing(id, Writing::LetGo),
                )
                .saying(said::changes_will_be_lost()),
        );
    }

    /// Does what a question was answered with.
    ///
    /// One arm per answer, and every arm is reachable: the point of each
    /// question having its own answers is that this match cannot grow arms
    /// for pairings that never happen.
    pub(in crate::app) fn answered(&mut self, answer: Answer) {
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
            Answer::Switching(Leaving::SaveAll) => {
                if self.save_everything()
                    && let Some(tree) = self.going()
                {
                    self.move_to_tree(&tree);
                }
            }
            Answer::Switching(Leaving::Discard) => {
                if let Some(tree) = self.going() {
                    self.move_to_tree(&tree);
                }
            }
            Answer::ClosingTheProject(Leaving::SaveAll) => {
                if self.save_everything() {
                    self.leave_the_project();
                }
            }
            Answer::ClosingTheProject(Leaving::Discard) => self.leave_the_project(),
            Answer::ClosingTheOthers(keep, Leaving::SaveAll) => {
                if self.save_everything_except(Some(keep)) {
                    self.close_all_but(keep);
                }
            }
            Answer::ClosingTheOthers(keep, Leaving::Discard) => self.close_all_but(keep),
            Answer::Saving(id, Saving::Mine) => self.save_now(id.get()),
            Answer::Saving(id, Saving::Theirs) => self.take_what_is_on_disk(id.get()),
            Answer::Writing(id, Writing::Back) => self.save_now(id.get()),
            Answer::Writing(id, Writing::LetGo) => self.close(id),
            // Including escape, which is the same answer by a shorter
            // route -- and the two routes are why this is a call rather
            // than the putting back itself: escape goes through `leave`
            // and a chosen row does not, so one of them would otherwise
            // leave the reader on a file they never asked to see.
            Answer::Cancel => self.go_back_from_asking(),
        }
    }

    /// Puts the reader back where a question about leaving took them from.
    ///
    /// Taken rather than read, so whichever way out of the question comes
    /// first is the one that spends it: the other ways all end in Obelus
    /// leaving, and a value left lying about would move the reader on the
    /// next list they escaped out of.
    pub(in crate::app) fn go_back_from_asking(&mut self) {
        if let Some(id) = self.taken_from.take() {
            self.go_to_document(id);
        }
    }

    /// The first document with something unwritten in it.
    ///
    /// Which is the only one, where the caller has counted one.
    pub(in crate::app) fn first_unsaved(&self) -> Option<DocumentId> {
        self.first_unsaved_except(None)
    }

    /// The first document but `except` with something unwritten in it.
    fn first_unsaved_except(&self, except: Option<DocumentId>) -> Option<DocumentId> {
        (0..self.documents.len())
            .map(DocumentId::new)
            .filter(|id| Some(*id) != except)
            .find(|id| self.file(*id).is_some_and(Buffer::is_dirty))
    }

    /// Which document a question is about, in the words the prompt says it
    /// in.
    ///
    /// The path relative to the working directory, which is how every other
    /// list in Obelus names a file -- and not the bare file name, because a
    /// reader may have four files called `mod.rs` open and only one of them
    /// is the one about to be closed. It goes in the prompt rather than in
    /// the block above the ways out: a block that said the path while the
    /// prompt said the name would be one fact drawn twice, taking two rows
    /// and a rule to do it.
    fn file_path(&self, id: DocumentId) -> String {
        self.file(id).map_or_else(String::new, |buffer| {
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
        if self.save_everything() {
            self.should_quit = true;
        }
    }

    /// Writes every unwritten document, and says whether they all went.
    fn save_everything(&mut self) -> bool {
        self.save_everything_except(None)
    }

    /// Writes every unwritten document but `except`, and says whether they
    /// all went.
    ///
    /// The one left out stays open, so whether it is written is still the
    /// reader's to say.
    fn save_everything_except(&mut self, except: Option<DocumentId>) -> bool {
        for index in 0..self.documents.len() {
            let unwritten = Some(DocumentId::new(index)) != except
                && self
                    .file(DocumentId::new(index))
                    .is_some_and(|buffer| buffer.is_dirty() && buffer.content().is_file());
            if unwritten && !self.write_now(index) {
                return false;
            }
        }
        true
    }
}
