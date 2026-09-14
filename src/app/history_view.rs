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
pub enum Radius {
    /// The commits that changed the file being read.
    File,
    /// Every commit in the project.
    Project,
}

impl Radius {
    /// Both, in tab order.
    pub const ALL: [Self; 2] = [Self::File, Self::Project];

    /// The tab's name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::File => "this file",
            Self::Project => "the project",
        }
    }
}

/// The commits a history view is showing, and what has been opened in it.
#[derive(Debug, Default)]
pub(super) struct Showing {
    /// Which radii have tabs, in tab order.
    pub radii: Vec<Radius>,
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
}

/// How many commits a list asks for.
///
/// A screenful and a great deal more, so that scrolling lands somewhere
/// rather than running out -- and a bound, because a history is as long as
/// the project and a reader is looking at one list.
const LISTED: usize = 200;

impl App {
    /// Opens the history at the radius a key names.
    pub fn open_history(&mut self, radius: Radius) {
        let radii = self.historic();
        let Some(tab) = radii.iter().position(|shown| *shown == radius) else {
            self.note = Some(match (radius, self.current_buffer().is_some()) {
                (Radius::File, true) => "nothing in this file's history".to_string(),
                (Radius::File, false) => "no file open".to_string(),
                (Radius::Project, _) => "no history here".to_string(),
            });
            return;
        };

        let names: Vec<&str> = radii.iter().map(|radius| radius.label()).collect();
        let mut picker = Picker::new(Vec::new(), PickerLayout::FullArea);
        picker.with_scopes(&names);
        picker.go_to_tab(tab);
        self.picker = Some(picker);
        self.history = Showing {
            radii,
            commits: Vec::new(),
            of: None,
            opened: None,
        };
        self.refresh_history();
    }

    /// Which radii can answer, in the order their tabs sit in.
    ///
    /// Settled when the view opens rather than watched while it is open, for
    /// the reason the search settles its own: tabs appearing under the arrow
    /// keys would move the ground while a reader walks it.
    fn historic(&self) -> Vec<Radius> {
        Radius::ALL
            .into_iter()
            .filter(|radius| match radius {
                // A file, and one git has heard of. A file with no commits
                // behind it has an empty tab, which is an answer; a file
                // outside the repository has no tab at all.
                Radius::File => self
                    .current_buffer()
                    .is_some_and(|buffer| self.has_any(Some(buffer.path()))),
                Radius::Project => self.has_any(None),
            })
            .collect()
    }

    /// Whether the project has a history at all, for the key to be offered.
    #[must_use]
    pub(super) fn has_history(&self) -> bool {
        self.has_any(None)
    }

    /// Whether there is a single commit to be had at a radius.
    ///
    /// Asked with a limit of one, because that is the question. Asking for
    /// the whole list and looking at its length costs a walk that finds two
    /// hundred commits and a tree lookup for each of them -- ten times over
    /// on this repository, and the answer was needed before the first row
    /// could be drawn.
    fn has_any(&self, only: Option<&Path>) -> bool {
        !crate::git::history::of(&self.working_directory, only, 1).is_empty()
    }

    /// The commits at a radius, newest first.
    fn history_of(&self, only: Option<&Path>) -> Vec<Commit> {
        crate::git::history::of(&self.working_directory, only, LISTED)
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
            Radius::File => self
                .current_buffer()
                .map(|buffer| buffer.path().to_path_buf()),
            Radius::Project => None,
        };
        self.history.commits = self.history_of(only.as_deref());
        self.history.of = only.clone();
        // An opened commit belongs to the tab it was opened in.
        self.history.opened = None;
        self.show_history();
    }

    /// Puts the commits, and whatever is open under one of them, into the
    /// list.
    fn show_history(&mut self) {
        let expands = self
            .picker
            .as_ref()
            .and_then(|picker| self.history.radii.get(picker.tab()).copied())
            == Some(Radius::Project);
        let now = std::time::SystemTime::now();
        // Which of them the remote already has. `None` where the question
        // does not arise -- no remote, or a branch tracking nothing -- and
        // then nothing is marked: every commit is equally unpushed, and
        // marking all of them says no more than marking none.
        let asked: Vec<gix::ObjectId> = self.history.commits.iter().map(|c| c.id).collect();
        let pushed = crate::git::history::pushed(&self.working_directory, &asked);
        let mut items: Vec<PickerItem> = Vec::new();
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
                marker: expands.then(|| crate::ui::opens(open).to_string()),
                label: commit.subject.clone(),
                detail: None,
                // Both on the right, where the width is taken out of the
                // subject's before it is truncated: a subject is long and a
                // row is narrow, so the one thing that must survive the cut
                // is how to find this commit again.
                trailing: Some(format!(
                    "{} \u{b7} {}",
                    crate::git::blame::how_long_ago(commit.when, now),
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
        if let Some(picker) = self.picker.as_mut() {
            picker.replace(items);
            picker.when_empty("nothing in the history here");
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
            == Some(Radius::Project);
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
    pub(super) fn open_at_commit(&mut self, id: gix::ObjectId, path: &Path) {
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
        self.buffers.push(Some(buffer));
        let index = self.buffers.len() - 1;
        self.go_to_buffer(BufferId::new(index));
    }
}

impl App {
    /// What a commit said about itself, as the rows of a block.
    ///
    /// The first row names it -- the id, who wrote it, how long ago -- and
    /// the rest is the message. A reader opening a file as a commit had it
    /// is asking why it says what it says, and that is the answer.
    pub(super) fn said_at(&self, id: gix::ObjectId) -> Option<Vec<String>> {
        let commit = crate::git::history::of(&self.working_directory, None, 1)
            .into_iter()
            .find(|commit| commit.id == id)
            .or_else(|| {
                crate::git::history::of(&self.working_directory, None, LISTED)
                    .into_iter()
                    .find(|commit| commit.id == id)
            })?;
        let now = std::time::SystemTime::now();
        let mut said = vec![
            format!(
                "{}   {}   {}",
                commit.short(),
                commit.who,
                crate::git::blame::how_long_ago(commit.when, now)
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
