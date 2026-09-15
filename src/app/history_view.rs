//! The list of commits, and what a row of it opens.
//!
//! One view at two radii, the way the search is one question at four: a
//! file's history and a project's differ only in which commits are listed,
//! so they are two tabs of one list and the keys land on the tab they name.
//!
//! A commit in the project's tab is not a file, so it has nothing to open.
//! What it has is the list of files it changed, and that list goes *under*
//! it, in place, the way a run of tool calls opens in the transcript: one
//! list, one selection, one Escape.

use super::*;
use crate::git::history::Commit;

/// Which commits a list is showing, in the order their tabs sit in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum About {
    /// Every commit that changed the file being read.
    File,
    /// Every name in the repository that points at a commit, so the file
    /// being read can be seen as any of them has it.
    Refs,
    /// Every commit in the project.
    Project,
}

impl About {
    /// What `f9` opens: the file being read, over time and over the places
    /// it can be seen from. Two answers of one shape -- a version of this
    /// file -- so the arrow between them stays inside one errand.
    pub const OF_A_FILE: [Self; 2] = [Self::File, Self::Refs];

    /// What `f10` opens. Its own key rather than a third tab, because it is
    /// the one view here that stops being about the file on screen: its
    /// rows are the project's commits, and what hangs under them is
    /// somebody else's files.
    pub const OF_A_PROJECT: [Self; 1] = [Self::Project];

    /// The tab's name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::File => "this file",
            Self::Refs => "the refs",
            Self::Project => "the project",
        }
    }
}

/// The commits a history view is showing, and what has been opened in it.
#[derive(Debug, Default)]
pub(super) struct Showing {
    /// Which radii have tabs, in tab order.
    pub radii: Vec<About>,
    /// The names that point at commits, for the tab that lists them.
    ///
    /// Kept apart from the commits: a ref is a commit with a name on it, and
    /// a list of them is read by the names -- which of a thousand tags is
    /// `v2.1` -- while a log is read by what the commits said.
    pub refs: Vec<crate::git::history::Reference>,
    /// The commits of the tab that is showing.
    pub commits: Vec<Commit>,
    /// Which file the list is about, when it is about one.
    ///
    /// A commit in that tab has nothing to open under it: it is already
    /// about one file, and that file is this. Remembered rather than asked
    /// again when a row is chosen, because by then the reader may be
    /// looking at something else.
    pub of: Option<PathBuf>,
    /// Which commit has been opened, and the files it changed.
    ///
    /// One at a time: a list where three commits are open is a list whose
    /// rows are mostly somebody else's files, and the reader is looking for
    /// one thing.
    pub opened: Option<(gix::ObjectId, Vec<(PathBuf, git::FileStatus)>)>,
    /// Which commit the list was read at.
    ///
    /// A history is an answer about a repository at a moment, and the
    /// repository moves while it is on screen: a commit in another window,
    /// an amend, a checkout. Remembered so that the list can be read again
    /// when it has moved, and *only* when -- git rewrites its index on
    /// every `git add`, and no commit changed.
    pub head: Option<gix::ObjectId>,
    /// A commit to put the reader back on when it turns up.
    ///
    /// A list re-read because `HEAD` moved is the same list with rows added
    /// on top of it, and a reader ten rows down was looking at a commit,
    /// not at a row number. Cleared once it has been honoured.
    pub wanted: Option<gix::ObjectId>,
    /// How many commits the walk filling this list has looked at, and `None`
    /// once it has finished.
    ///
    /// A file's history is found by asking every commit in the project
    /// whether it touched that path, so a list can sit empty for a second
    /// while the answer is still true. "Still reading" and "nothing here"
    /// are different facts and the reader is owed the difference.
    pub reading: Option<usize>,
    /// Which of the commits the remote already has.
    ///
    /// `None` where the question does not arise -- no remote, or a branch
    /// tracking nothing -- and then nothing is marked: every commit is
    /// equally unpushed, and marking all of them says no more than marking
    /// none.
    pub pushed: Option<HashSet<gix::ObjectId>>,
    /// Whether it is still worth asking the remote about a new batch.
    ///
    /// Unpushed commits are the newest ones, so once a batch arrives with
    /// every commit in it already on the remote, so is everything older.
    /// Asking again for each of a whole history's batches would walk the
    /// remote branch once per batch to learn the same thing.
    pub marking: bool,
}

impl App {
    /// Opens the view a key names, on the tab it names.
    pub fn open_history(&mut self, about: About) {
        let radii = self.historic(about);
        let Some(tab) = radii.iter().position(|shown| *shown == about) else {
            self.note = Some(match about {
                About::File | About::Refs => "no file open".to_string(),
                About::Project => "no history here".to_string(),
            });
            return;
        };

        let names: Vec<&str> = radii.iter().map(|radius| radius.label()).collect();
        let mut picker = Picker::new(Vec::new(), PickerLayout::FullArea);
        picker.with_scopes(&names);
        // A commit's files hang under it: the query is about the commits.
        picker.nests();
        picker.go_to_tab(tab);
        self.picker = Some(picker);
        self.history = Showing {
            radii,
            commits: Vec::new(),
            refs: Vec::new(),
            of: None,
            opened: None,
            head: None,
            wanted: None,
            reading: None,
            pushed: None,
            marking: true,
        };
        self.refresh_history();
    }

    /// Which radii can answer, in the order their tabs sit in.
    ///
    /// Settled when the view opens rather than watched while it is open, for
    /// the reason the search settles its own: tabs appearing under the arrow
    /// keys would move the ground while a reader walks it.
    fn historic(&self, about: About) -> Vec<About> {
        let wanted: &[About] = match about {
            About::File | About::Refs => &About::OF_A_FILE,
            About::Project => &About::OF_A_PROJECT,
        };
        wanted
            .iter()
            .copied()
            .filter(|radius| match radius {
                // A file being read is all these tabs need. Whether that
                // file has any commits behind it is a question with a walk
                // in it -- every commit has to be asked whether it touched
                // this path -- and a tab that came and went with the answer
                // would be a tab that disappears for the files nobody has
                // edited lately, which are exactly the ones whose history a
                // reader is curious about. An empty list saying so is an
                // answer; a missing tab is a key that does nothing.
                About::File | About::Refs => self.current_buffer().is_some(),
                // The project's costs no such walk: the first commit the
                // walk reaches is the answer.
                About::Project => self.has_any(),
            })
            .collect()
    }

    /// Whether the project has a history at all, for the key to be offered.
    #[must_use]
    pub(super) fn has_history(&self) -> bool {
        self.has_any()
    }

    /// Whether the project has a single commit to be had.
    ///
    /// The first one the walk reaches is the answer, and no tree is looked
    /// at on the way, so this is a question a key can be offered on. Which
    /// commits touched some *file* is not: that is a tree lookup per commit
    /// of the whole project, and it belongs on the thread that fills the
    /// list rather than in front of the key that opens it.
    fn has_any(&self) -> bool {
        !crate::git::history::of(&self.working_directory, None, 1).is_empty()
    }

    /// Fills the list with the commits of whichever tab is showing.
    pub(super) fn refresh_history(&mut self) {
        let Some(radius) = self
            .picker
            .as_ref()
            .and_then(|picker| self.history.radii.get(picker.tab()).copied())
        else {
            return;
        };
        let only = match radius {
            // The refs tab is about the file too: its rows are places to
            // see *this file* from, so a row opens the same thing a row of
            // its own history does.
            About::File | About::Refs => self
                .current_buffer()
                .map(|buffer| buffer.path().to_path_buf()),
            About::Project => None,
        };
        self.history.commits = Vec::new();
        self.history.refs = Vec::new();
        self.history.of = only.clone();
        // An opened commit belongs to the tab it was opened in.
        self.history.opened = None;
        self.history.pushed = None;
        self.history.marking = true;
        self.history.head = crate::git::history::head_of(&self.working_directory);
        if let Some(picker) = self.picker.as_mut() {
            // A log is read newest first, and a query asks which commits
            // mention something -- not which subject line scored best. A
            // list of names is the other way about: a reader typing `v0.1`
            // wants the tag of that name, not whichever name containing
            // those letters was pushed most recently.
            picker.keeps_order(radius != About::Refs);
        }
        match radius {
            // No walk at all: each name is one commit to decode and no tree
            // is looked at on the way. A project with three thousand of
            // them answers in forty milliseconds, which is a key press.
            About::Refs => {
                // The generation moves anyway, so a walk the other tab
                // started cannot arrive into this list.
                self.next_history_walk();
                self.history.reading = None;
                self.history.refs = crate::git::history::refs_of(&self.working_directory);
            }
            About::File | About::Project => {
                self.history.reading = Some(0);
                self.start_reading_history(only.as_deref());
            }
        }
        self.show_history();
    }

    /// Reads an open history again if the repository has moved under it.
    ///
    /// A list on screen is an answer about a repository at a moment, and the
    /// moment passes: the reader commits in another window, amends, checks
    /// something out. Leaving the list as it was would be showing them a
    /// history that no longer exists, with their own newest commit missing
    /// from the top of it.
    ///
    /// Only when `HEAD` has actually moved. Everything that writes git's
    /// state arrives here, and `git add` writes the index on every use
    /// without changing a single commit -- re-reading on that would throw
    /// away a walk in progress for nothing.
    pub(super) fn reread_history(&mut self) {
        if self.picker.is_none() || self.history.radii.is_empty() {
            return;
        }
        let head = crate::git::history::head_of(&self.working_directory);
        if head == self.history.head {
            return;
        }
        // Where the reader is, so the same commit is under them afterwards.
        let wanted = self
            .picker
            .as_ref()
            .and_then(Picker::selected_item)
            .and_then(|item| match item.value {
                PickerValue::Commit(id) => Some(id),
                PickerValue::CommitFile { id, .. } => Some(id),
                _ => None,
            });
        // And which commit was opened, which the re-read has no reason to
        // close: it is the same commit, and its files are the same files.
        // One that the move took away shows as nothing, because a row is
        // only drawn under the commit it belongs to.
        let opened = self.history.opened.take();
        self.refresh_history();
        self.history.opened = opened;
        self.history.wanted = wanted;
        self.show_history();
    }

    /// Starts a walk of the history the list is waiting for.
    ///
    /// Bumping the generation first is what tells the walk before it --
    /// another tab, another file -- that nobody is waiting for it any more.
    fn start_reading_history(&mut self, only: Option<&Path>) {
        let generation = self.next_history_walk();
        let Some(sender) = self.events.clone() else {
            // No loop to answer into: the tests that drive the app by hand
            // read the history themselves.
            return;
        };
        crate::git::history::spawn_log(
            &self.working_directory,
            only,
            generation,
            &self.history_generation,
            sender,
        );
    }

    /// Says that whatever a walk is answering, nobody is waiting for it.
    ///
    /// Every way of filling this list bumps the generation, including the
    /// ways that need no walk: a list of refs left the previous tab's walk
    /// running, and its batches would arrive into a list they are not about.
    fn next_history_walk(&mut self) -> u64 {
        self.history_generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1
    }

    /// Puts a batch of commits into the list waiting for them.
    pub(super) fn on_logged(&mut self, commits: Vec<Commit>, walked: usize, done: bool) {
        // The generation says the walk is the one wanted; this says there is
        // still a list for it to fill.
        if self.picker.is_none() || self.history.radii.is_empty() {
            return;
        }
        self.history.commits.extend(commits);
        self.history.reading = (!done).then_some(walked);
        if self.history.marking {
            let asked: Vec<gix::ObjectId> = self
                .history
                .commits
                .iter()
                .map(|commit| commit.id)
                .collect();
            let found = crate::git::history::pushed(&self.working_directory, &asked);
            // Once every commit in hand is on the remote, so is every older
            // one, and there is nothing left for the question to tell apart.
            self.history.marking = found
                .as_ref()
                .is_some_and(|found| found.len() != asked.len());
            self.history.pushed = found;
        }
        self.show_history();
    }

    /// Puts the commits, and whatever is open under one of them, into the
    /// list.
    fn show_history(&mut self) {
        let showing = self
            .picker
            .as_ref()
            .and_then(|picker| self.history.radii.get(picker.tab()).copied());
        let expands = showing == Some(About::Project);
        let now = std::time::SystemTime::now();
        let pushed = self.history.pushed.clone();
        let mut items: Vec<PickerItem> = Vec::new();
        for reference in &self.history.refs {
            use crate::git::history::RefKind;
            items.push(PickerItem {
                // A name, and names lose their head rather than their tail:
                // `origin/kb/some-long-branch` is told from its fellows at
                // the end, not the beginning.
                prose: false,
                icon: icons::enabled().then_some(match reference.kind {
                    RefKind::Branch => icons::ui::BRANCH,
                    RefKind::Remote => icons::ui::REMOTE,
                    RefKind::Tag => icons::ui::TAG,
                }),
                // Where the reader is, which is the one row in a list of
                // places that they do not need to go to.
                marker: reference
                    .head
                    .then(|| (Marking::Aside, "\u{2022}".to_string())),
                label: reference.name.clone(),
                // What it points at, after the name: a name says which
                // place, and a subject says what is there.
                detail: Some(reference.at.subject.clone()),
                trailing: Some(format!(
                    "{} \u{b7} {}",
                    crate::git::how_long_ago(reference.at.when, now),
                    reference.at.short()
                )),
                value: PickerValue::Commit(reference.at.id),
                depth: 0,
                status: None,
                enabled: true,
                colours: None,
                kind: None,
                tab: None,
            });
        }
        for commit in &self.history.commits {
            let open = self
                .history
                .opened
                .as_ref()
                .is_some_and(|(id, _)| *id == commit.id);
            items.push(PickerItem {
                // A subject is a sentence: cut it at the end, where the
                // words it can spare are.
                prose: true,
                icon: icons::enabled().then_some(icons::ui::COMMIT),
                // The mark, when this tab has anything to open: a reader
                // cannot press a key on a row that never said it had
                // something behind it.
                marker: expands.then(|| (Marking::Aside, crate::ui::opens(open).to_string())),
                label: commit.subject.clone(),
                detail: None,
                // Both on the right, where the width is taken out of the
                // subject's before it is truncated: a subject is long and a
                // row is narrow, so the one thing that must survive the cut
                // is how to find this commit again.
                trailing: Some(format!(
                    "{} \u{b7} {}",
                    crate::git::how_long_ago(commit.when, now),
                    commit.short()
                )),
                value: PickerValue::Commit(commit.id),
                depth: 0,
                // Not on the remote yet, which is the same colour a file
                // git has not seen wears, and for the same reason: it is
                // the one still to be dealt with, and the few of them are
                // what a reader scanning the list is looking for.
                status: pushed
                    .as_ref()
                    .filter(|pushed| !pushed.contains(&commit.id))
                    .map(|_| git::FileStatus::New),
                enabled: true,
                colours: None,
                kind: None,
                tab: None,
            });
            if let Some((_, files)) = self.history.opened.as_ref().filter(|_| open) {
                for (path, status) in files {
                    items.push(PickerItem {
                        // A path, and paths lose their head.
                        prose: false,
                        icon: icons::enabled().then(|| icons::for_path(path)),
                        marker: None,
                        label: path.display().to_string(),
                        detail: None,
                        trailing: None,
                        value: PickerValue::CommitFile {
                            id: commit.id,
                            path: path.clone(),
                        },
                        depth: 1,
                        status: Some(*status),
                        enabled: true,
                        colours: None,
                        kind: None,
                        tab: None,
                    });
                }
            }
        }
        let empty = match (self.history.reading.is_some(), self.history.of.as_deref()) {
            _ if showing == Some(About::Refs) => "nothing points at a commit here",
            // Still looking. A file's history is every commit that ever
            // touched it, and nothing found yet is not nothing to find.
            (true, _) => "reading the history\u{2026}",
            // Said in the reader's terms: they pressed a key about *this*
            // file, and the answer is about this file.
            (false, Some(_)) => "no commit has touched this file",
            (false, None) => "nothing in the history here",
        };
        // How far the walk has got. A file's history can find nothing for a
        // second and a half and still be working, and a count that moves is
        // the only thing that tells that apart from a list that is finished
        // and empty.
        let filling = self
            .history
            .reading
            .map(|walked| format!("{walked} commits read\u{2026}"));
        let wanted = self.history.wanted;
        let mut found = false;
        if let Some(picker) = self.picker.as_mut() {
            picker.relist(items);
            picker.when_empty(empty);
            picker.filling(filling);
            // Once the commit the reader was on turns up, they are put back
            // on it. Before that the list is a list they have not seen yet,
            // and there is nothing to put them back on.
            let row = wanted.and_then(|id| {
                picker
                    .matches()
                    .position(|item| matches!(item.value, PickerValue::Commit(at) if at == id))
            });
            if let Some(row) = row {
                picker.select_row(row);
                found = true;
            }
        }
        if found {
            self.history.wanted = None;
        }
    }

    /// The file a commit's row names, when the list is about one file.
    ///
    /// A row in the file's own tab is a commit that changed *this* file, so
    /// choosing it opens the file as that commit had it. There is nothing
    /// to open under such a row -- the list of files it changed would be a
    /// list with the tab's own name in it -- so this is what Enter means
    /// there.
    pub(super) fn commit_opens(&self) -> Option<PathBuf> {
        self.history.of.clone()
    }

    /// Opens a commit's files under it, or closes them again.
    ///
    /// Says whether it did, so that a row with nothing under it can be
    /// chosen instead of toggled.
    pub(super) fn expand_commit(&mut self, id: gix::ObjectId) -> bool {
        let expands = self
            .picker
            .as_ref()
            .and_then(|picker| self.history.radii.get(picker.tab()).copied())
            == Some(About::Project);
        if !expands {
            return false;
        }
        let row = self.picker.as_ref().map(Picker::selected);
        if self
            .history
            .opened
            .as_ref()
            .is_some_and(|(open, _)| *open == id)
        {
            self.history.opened = None;
        } else {
            let files = crate::git::history::files_in(&self.working_directory, id);
            self.history.opened = Some((id, files));
        }
        self.show_history();
        // Back onto the row the key was pressed on: the rows below it have
        // moved, and a selection that jumped to the top would leave the
        // reader somewhere they did not ask to be.
        if let (Some(picker), Some(row)) = (self.picker.as_mut(), row) {
            picker.select_row(row);
        }
        true
    }
}

impl App {
    /// Opens a file as a commit had it.
    ///
    /// A buffer of its own rather than the file on disk: what the reader
    /// asked for is what that commit said, and the file in the working tree
    /// is a different document that happens to share a name.
    /// `at` is the line to land on, for a reader who asked about one line
    /// rather than about the commit. `None` lands in the message, which is
    /// what a reader who chose a commit from a list asked to read.
    pub(super) fn open_at_commit(
        &mut self,
        id: gix::ObjectId,
        path: &Path,
        at: Option<LineNumber>,
    ) {
        let full = self.working_directory.join(path);
        let Some(text) = crate::git::history::text_at(&self.working_directory, id, &full) else {
            self.note = Some("nothing to read there".to_string());
            return;
        };
        let from = self.here();
        self.record(from);
        let mut buffer = Buffer::at_commit(&full, id, &text);
        // The message above the first line, so the reader lands on *why*
        // and pages down to what. It is not a line of the file, and a
        // block is exactly the shape obelus has for that -- rows on screen
        // the file does not have, with no line numbers, that the caret can
        // walk into and copy from.
        if let Some(said) = self.said_at(id) {
            buffer.open_held(LineNumber::new(0), &said, crate::buffer::Held::Message);
            buffer.enter_block(LineNumber::new(0));
        }
        // A reader who asked about a line is put on that line, where it was
        // then -- which is not where it is now, because everything added
        // above it since has pushed it down. The message stays above the
        // first line, a page away, for when they want it.
        if let Some(line) = at {
            buffer.place_cursor(line, crate::coordinates::CharColumn::new(0));
        }
        self.buffers.push(Some(buffer));
        let index = self.buffers.len() - 1;
        self.go_to_buffer(BufferId::new(index));
    }
}

impl App {
    /// Opens the commit that wrote the line under the cursor.
    ///
    /// The direct answer to "why is this line here", which is the question a
    /// reader asks most often and the one obelus could not answer: the
    /// margin said who and when, and there was no way from a line to the
    /// commit behind it. A list would be ceremony -- one line has one
    /// commit.
    ///
    /// Pressing it again in what it opens walks back another step -- the
    /// version it opened has a blame of its own -- until the line reaches
    /// the commit that wrote it, which is where it stops. Going past that
    /// is a different question ("what was here before this commit touched
    /// it"), and the line it would land on is one this commit removed: it
    /// has no number in the file on screen.
    pub fn open_line_commit(&mut self) {
        let Some(line) = self.current_buffer().map(|buffer| buffer.cursor().line) else {
            self.note = Some("no file open".to_string());
            return;
        };
        self.open_line_commit_at(line);
    }

    /// The same, for one line rather than for wherever the cursor is.
    ///
    /// Which is what the answer to a question asked before the walk had
    /// run is about: the line the reader pressed on, not whichever line
    /// they are on by the time it lands.
    pub(super) fn open_line_commit_at(&mut self, line: LineNumber) {
        let Some(buffer) = self.current_buffer() else {
            self.note = Some("no file open".to_string());
            return;
        };
        let (version, path) = (buffer.content().at(), buffer.path().to_path_buf());
        let Some(blamed) = self.blamed_at(line) else {
            // Two different nothings, and the reader is owed the
            // difference: a walk still running is worth waiting for, and a
            // line no commit accounts for is not.
            let walked = self.blamed_lines().is_some();
            // And if nobody has asked yet -- which is every file, for a
            // reader who keeps the margin's names off -- this is the asking.
            // Saying "still reading" while nothing was being read was the
            // one answer that was not true. The question is held so that the
            // answer finishes it: a key that has to be pressed twice for
            // half the readers is a key that works for half the readers.
            if !walked {
                self.asked_line = Some((path.clone(), version, line));
                self.ask_blame();
            }
            self.note = Some(match walked {
                true => "no commit has this line".to_string(),
                false => "still reading who wrote this\u{2026}".to_string(),
            });
            return;
        };
        let (id, at) = (blamed.id, LineNumber::new(blamed.line as usize));
        // Already there. Opening a second buffer on the same version of the
        // same file, with the caret where it already is, is a key that
        // looks broken and leaves a buffer behind every time it is pressed.
        if self
            .current_buffer()
            .and_then(|buffer| buffer.content().at())
            == Some(id)
        {
            self.note = Some("this commit wrote this line".to_string());
            return;
        }
        // Whatever was being said about waiting for this is answered.
        self.note = None;
        self.open_at_commit(id, &path, Some(at));
    }

    /// What a commit said about itself, as the rows of a block.
    ///
    /// The first row names it -- the id, who wrote it, how long ago -- and
    /// the rest is the message. A reader opening a file as a commit had it
    /// is asking why it says what it says, and that is the answer.
    pub(super) fn said_at(&self, id: gix::ObjectId) -> Option<Vec<String>> {
        let commit = crate::git::history::one(&self.working_directory, id)?;
        let now = std::time::SystemTime::now();
        let mut said = vec![
            format!(
                "{}   {}   {}",
                commit.short(),
                commit.who,
                crate::git::how_long_ago(commit.when, now)
            ),
            String::new(),
            commit.subject.clone(),
        ];
        if !commit.body.is_empty() {
            said.push(String::new());
            said.extend(commit.body.lines().map(str::to_string));
        }
        Some(said)
    }
}
