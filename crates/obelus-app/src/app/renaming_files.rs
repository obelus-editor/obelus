//! Renaming a file, and moving one, which are the same act.
//!
//! One question rather than two: a reader who wants `hint.rs` called
//! `hints.rs` and a reader who wants it under `lsp/` are both answering
//! "where should this be instead", and a rename is the answer that happens
//! to keep the directory.
//!
//! Called renaming rather than moving where a reader can see it, because
//! that is the word they reach for: giving a file a new name is a daily
//! act and moving one is an occasional one, and the command palette
//! matches what a row says rather than what it means.
//!
//! What makes it more than `std::fs::rename` is everything that was
//! pointing at the old place: the document the reader has open, the
//! watcher that was told to report changes to it, and whatever a language
//! server has to change in other files so that the project still means
//! what it meant.

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use obelus_lsp::{edits::Wanted, renaming};

use super::{documents, *};

/// How long a rename waits on the servers it asked.
///
/// A server that is indexing answers `null` in milliseconds, so this is
/// not the wait for a busy server -- it is the wait for one that has
/// stopped answering at all. The rename happens either way: a reader who
/// asked for a file to be called something else is owed the file being
/// called it, and an editor that swallowed the request because a
/// subprocess is stuck is worse than one that renames the file and says
/// the references were not updated.
const ANSWERS_WITHIN: Duration = Duration::from_secs(5);

/// A rename, from the moment it is asked about to the moment it happens.
///
/// One field on the application rather than two, because these are the two
/// halves of one act and only one of them is ever true: a question on the
/// status row becomes the answer to it, and a rename that is waiting on a
/// server is not also a question nobody has answered.
#[derive(Debug)]
pub(super) enum Renaming {
    /// The question is on the status row, about this path.
    Asked(PathBuf),
    /// The answer is in, the servers have been asked, and the rename
    /// happens when they answer.
    Waiting(Waiting),
}

/// A rename that has been asked about and not made yet.
#[derive(Debug)]
pub(super) struct Waiting {
    /// Where the file is now.
    pub(super) from: PathBuf,
    /// Where it is going.
    pub(super) to: PathBuf,
    /// The servers that were asked and have not answered.
    ///
    /// More than one is possible -- a directory can hold two languages --
    /// and the move waits for all of them, because applying half the
    /// edits and then the rest a second later is two undo steps for one
    /// act.
    pub(super) waiting: Vec<LanguageId>,
    /// What they have asked for so far.
    pub(super) wanted: Wanted,
    /// Whether one of them said it was not ready.
    ///
    /// A `null` answer rather than an empty edit, which are the two things
    /// a server says when it changes nothing and mean opposite things to a
    /// reader: one is "there was nothing to update" and the other is "ask
    /// me again when I have finished indexing".
    pub(super) unready: bool,
    /// When it was asked, so a server that never answers does not hold the
    /// move forever.
    pub(super) asked: Instant,
}

impl App {
    /// Asks what the file being read should be called instead.
    pub fn rename_file(&mut self) {
        let Some(path) = self
            .current_buffer()
            .map(|buffer| buffer.path().to_path_buf())
        else {
            self.note = Some("No file to rename".to_string());
            return;
        };
        self.ask_what_to_call_it(&path);
    }

    /// The same, about whatever the file list is on.
    pub(super) fn rename_selected(&mut self) -> bool {
        let Some(path) = self.selected_path() else {
            return false;
        };
        self.ask_what_to_call_it(&path);
        true
    }

    /// What the file list is on, if it is on anything with a path.
    fn selected_path(&self) -> Option<PathBuf> {
        let item = self.picker.as_ref()?.selected_item()?;
        Some(self.working_directory.join(documents::path_of_row(item)?))
    }

    /// Puts the question on the status row, with where it is now in it.
    ///
    /// Filled in rather than empty, because the answer is almost always a
    /// small change to it: a word of the name, or the directory in front
    /// of it. Relative to the project, which is how the list writes a path
    /// and how a reader says one.
    fn ask_what_to_call_it(&mut self, path: &Path) {
        let shown = relative(path, &self.working_directory);
        self.renaming = Some(Renaming::Asked(path.to_path_buf()));
        self.ask_on_the_status_row(obelus_component::prompt::Prompt::about(
            obelus_component::prompt::PromptKind::Path,
            shown,
        ));
    }

    /// Renames what the question was about to what the answer says.
    ///
    /// Or asks first. Everything that can refuse it is checked here -- a
    /// file already there, a directory that cannot be made -- so that a
    /// server is only ever asked about a rename that is going to happen.
    pub(super) fn rename_file_to(&mut self, answer: &Path) {
        let Some(Renaming::Asked(from)) = self.renaming.take() else {
            return;
        };
        let to = match answer.is_absolute() {
            true => answer.to_path_buf(),
            false => self.working_directory.join(answer),
        };
        if from == to {
            return;
        }
        // Somewhere with something already in it. Refused rather than
        // asked about: writing over a file is the one thing here that
        // cannot be put back, and a reader who meant it can say so by
        // taking the other file away first.
        if to.exists() {
            self.note = Some(format!(
                "{} is already there",
                relative(&to, &self.working_directory)
            ));
            return;
        }
        // Somewhere that does not exist yet, which is what taking a path
        // rather than a name is for: a reader moving a file into a new
        // directory should not have to leave to make it.
        if let Some(parent) = to.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            self.note = Some(format!("Could not make {}: {error}", parent.display()));
            return;
        }
        let waiting = self.ask_what_the_rename_changes(&from, &to);
        if waiting.is_empty() {
            self.make_the_rename(&from, &to, &Wanted::default(), None);
            return;
        }
        self.note = Some(format!(
            "asking what renaming {} changes\u{2026}",
            relative(&from, &self.working_directory)
        ));
        self.renaming = Some(Renaming::Waiting(Waiting {
            from,
            to,
            waiting,
            wanted: Wanted::default(),
            unready: false,
            asked: Instant::now(),
        }));
    }

    /// Asks every server that registered an interest in this path.
    ///
    /// Every one rather than the one for the file's own language: what a
    /// server wants to hear about is a thing it says for itself, in globs,
    /// and a directory being renamed has no language of its own to look
    /// up.
    fn ask_what_the_rename_changes(&mut self, from: &Path, to: &Path) -> Vec<LanguageId> {
        let Some(params) = renaming::params(from, to) else {
            return Vec::new();
        };
        let mut asked = Vec::new();
        for language in self.servers_wanting(from, from.is_dir(), renaming::asked_before) {
            let Some(client) = self.servers.get_mut(&language) else {
                continue;
            };
            match client.request("workspace/willRenameFiles", &params) {
                Ok(request) => {
                    self.remember(
                        language,
                        request,
                        Question {
                            asked: Asked::WillRename,
                            // Whatever document the reader is on. The
                            // question is about a path rather than a
                            // document -- the file may not be open at all
                            // -- and this is only what the table keys one
                            // question of a kind against another by.
                            buffer: self.current.unwrap_or(DocumentId::new(0)),
                            version: 0,
                        },
                    );
                    asked.push(language);
                }
                Err(error) => tracing::warn!(%error, "could not ask about a rename"),
            }
        }
        asked
    }

    /// Tells the servers that wanted telling.
    ///
    /// The ones that registered `didRename`, which is a different list
    /// from the ones that were asked: a server can want only the
    /// notification, and one that answered the question has already been
    /// told by being asked.
    fn told_about_the_rename(&mut self, from: &Path, to: &Path) {
        let Some(params) = renaming::params(from, to) else {
            return;
        };
        for language in self.servers_wanting(from, to.is_dir(), renaming::told_after) {
            if let Some(client) = self.servers.get_mut(&language)
                && let Err(error) = client.notify("workspace/didRenameFiles", &params)
            {
                tracing::debug!(%error, "could not say a file was renamed");
            }
        }
    }

    /// The servers whose own filters cover this path.
    ///
    /// Collected before any of them is written to, which is the only
    /// reason this is a list rather than a loop: asking a server borrows
    /// the table the filters were read from.
    fn servers_wanting(
        &self,
        path: &Path,
        directory: bool,
        wants: fn(&lsp_types::ServerCapabilities, &Path, bool) -> bool,
    ) -> Vec<LanguageId> {
        self.servers
            .iter()
            .filter(|(_, client)| {
                client
                    .capabilities()
                    .is_some_and(|capabilities| wants(capabilities, path, directory))
            })
            .map(|(language, _)| *language)
            .collect()
    }

    /// Takes one server's answer about the rename that is waiting.
    pub(super) fn on_will_rename(&mut self, language: LanguageId, reply: Reply) {
        let Some(Renaming::Waiting(waiting)) = self.renaming.as_mut() else {
            return;
        };
        waiting.waiting.retain(|waited| *waited != language);
        match reply.result {
            // A server still indexing answers `null`, which is not the
            // same as answering that nothing changes.
            Ok(serde_json::Value::Null) => waiting.unready = true,
            Ok(result) => {
                let wanted = obelus_lsp::edits::wanted_in(&result);
                waiting.wanted.changes.extend(wanted.changes);
                waiting.wanted.refused.extend(wanted.refused);
            }
            Err(why) => tracing::warn!(why, "a server refused to say what a rename changes"),
        }
        if waiting.waiting.is_empty() {
            self.finish_the_rename(None);
        }
    }

    /// Makes the rename a server has stopped answering about.
    ///
    /// Called from the clock, which is awake for exactly as long as this
    /// is waiting. A rename that is waiting is the reader's file not being
    /// called what they asked for, so the wait has an end.
    pub(super) fn rename_without_them(&mut self) {
        let overdue = matches!(
            self.renaming.as_ref(),
            Some(Renaming::Waiting(waiting)) if waiting.asked.elapsed() > ANSWERS_WITHIN
        );
        if overdue {
            self.finish_the_rename(Some("The language server did not answer"));
        }
    }

    /// Whether a rename is waiting on a server, and so the clock is needed.
    pub(super) fn renaming_is_waiting(&self) -> bool {
        matches!(self.renaming, Some(Renaming::Waiting(_)))
    }

    /// Does what the answers said, and then the rename itself.
    fn finish_the_rename(&mut self, instead: Option<&'static str>) {
        let Some(Renaming::Waiting(waiting)) = self.renaming.take() else {
            return;
        };
        let aside = match instead {
            Some(said) => Some(said),
            // Said only when a server told Obelus it was not ready. A
            // server that had nothing to change says so with an empty
            // edit, and a reader who renamed a file with no references to
            // it should not be told anything went wrong.
            None if waiting.unready => Some("The language server was not ready to update anything"),
            None => None,
        };
        self.make_the_rename(&waiting.from, &waiting.to, &waiting.wanted, aside);
    }

    /// The rename itself: the edits, the rename on disk, and everything
    /// pointing at where it was.
    ///
    /// The edits first, so that a server which asked for a change in the
    /// file being renamed gets it in the buffer that then follows the
    /// file. They land in documents and not on disk -- [`App::apply_wanted`]
    /// opens whatever it has to -- so the reader can undo what the server
    /// asked for, which is the half of this that can be taken back. The
    /// rename cannot, and is not pretended to be.
    fn make_the_rename(&mut self, from: &Path, to: &Path, wanted: &Wanted, aside: Option<&str>) {
        let changed = match wanted.is_empty() {
            true => None,
            false => Some(self.apply_wanted(wanted)),
        };
        if let Err(error) = std::fs::rename(from, to) {
            self.note = Some(format!("Could not rename it: {error}"));
            return;
        }
        self.followed(from, to);
        self.told_about_the_rename(from, to);
        let mut said = format!("renamed to {}", relative(to, &self.working_directory));
        if let Some(changed) = changed {
            said.push_str(&format!("; {changed}"));
        }
        if let Some(aside) = aside {
            said.push_str(&format!("; {aside}"));
        }
        self.note = Some(said);
    }

    /// Takes everything that was pointing at the old place to the new one.
    fn followed(&mut self, from: &Path, to: &Path) {
        // Every document at or under it: moving a directory moves each of
        // the files open from inside it, and the reader has them open by
        // what they hold rather than by where they were.
        let moved: Vec<(DocumentId, PathBuf, PathBuf)> = (0..self.documents.len())
            .map(DocumentId::new)
            .filter_map(|id| {
                let was = self.file(id)?.path().to_path_buf();
                let under = was.strip_prefix(from).ok()?;
                let now = to.join(under);
                Some((id, was, now))
            })
            .collect();
        for (id, was, path) in moved {
            // The old path, which is the one being watched. Unwatching the
            // new one would leave the watcher reporting a file that is not
            // there any more and saying nothing about the one that is.
            if let Some(watcher) = self.watcher.as_mut() {
                watcher.unwatch(&was);
            }
            if let Some(buffer) = file_in_mut(&mut self.documents, id) {
                buffer.moved_to(path.clone());
            }
            if let Some(watcher) = self.watcher.as_mut()
                && let Err(error) = watcher.watch(&path)
            {
                tracing::debug!(%error, "watching where it went");
            }
        }
        // The list is a walk of what is on disk, and what is on disk has
        // changed.
        if self.picker.is_some() && !self.listing.is_empty() {
            self.start_walk();
            self.refresh_listing();
        }
    }
}
