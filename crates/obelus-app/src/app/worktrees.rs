//! Going to another of the repository's worktrees.
//!
//! Obelus does not split its window, so a reader with three worktrees of
//! one repository has three Obelus processes on it -- and what the list of
//! open documents could not say is where the other two are. Its second tab
//! is every checkout the repository has, and which of them an Obelus is on.
//!
//! **In a window, enter is another window and `ctrl+enter` is this one
//! going.** Going to another window is starting an Obelus or bringing one
//! forward, and a window can do both: it can start a program that opens a
//! window, and it can hand that window -- or another Obelus's -- the
//! compositor's permission to come to the front. So a tree's row opens a
//! new window on it, this window's own tree among them, and each Obelus on
//! a tree is a row under it that brings that one forward. Putting this
//! window on the tree instead is the second enter, and the foot says both,
//! because a list whose enter does not take the row here is news.
//!
//! **In a terminal, enter is this one going, because it is all a terminal
//! can do.** The window it is in is the terminal's, and which terminal the
//! reader would want a new Obelus started in is not something Obelus can
//! know. So [`Windows`] is what a front end says it can do, and a terminal
//! says nothing: its list has the one enter, which is a list's ordinary
//! one, and no foot. The windows on each tree are listed under it all the
//! same, dim, because where everybody is does not depend on what this one
//! is drawn on.
//!
//! Going is the same either way: what the project was is let go of the way
//! a tree that went lets go of it (`App::let_go_of_the_project`), with what
//! was unwritten asked about first -- the tree is still there to write it
//! in -- and what was open written down, so going back opens it again.
//!
//! **A window on a tree is a claim, held the way a conversation's is.** A
//! lock the kernel gives up with the process, and a file beside it for the
//! watcher to wake on -- see `obelus_agent::chats`, whose argument this is
//! word for word. One file per window rather than per tree, because two
//! windows on one tree is as ordinary as two on one project, and the file
//! says which tree and, where it can be, how to reach the window: an
//! address on the loopback and a key that a stranger on the same machine
//! does not have. A terminal's Obelus claims its tree too, with no door:
//! which tree has somebody on it is true whatever they are drawn on, and
//! only the bringing forward is a window's.
//!
//! **A claim appears already held.** It is made under a name nobody reads
//! and renamed once it is locked, so a file nobody holds is a window that
//! died -- which the next look takes away, since every window is a new
//! name and nothing else would (`present`).
//!
//! **A tree that has gone is nowhere to go.** git lists a checkout deleted
//! behind its back until somebody prunes it, and the row says `Missing`
//! and cannot be chosen. A window that was on it has stopped saying so --
//! a tree that goes takes the project and everything open in it, and the
//! window is left on nothing (`App::the_tree_has_gone`). One removed
//! through git is not listed at all.
//!
//! **What happens on the far side is drawn, not said.** A window that
//! comes forward is the answer, and so is a new one appearing; a
//! compositor that only marks the window as wanting attention has been
//! told by the reader that this is what it does, and a note on the status
//! row would be Obelus disagreeing with their settings.

use std::{
    fs::File,
    io::{Read as _, Write as _},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};

use obelus_component::picker::{
    Marking, Picker, PickerItem, PickerLayout, PickerValue, WorktreeEnter,
};

use super::*;
use crate::event::Event;

/// What a front end can do about other windows.
///
/// Said by a window and by nothing else: a terminal does not own the
/// window it is drawn in, and has no answer to any of these.
///
/// `Send + Sync`, for the reason [`super::Drawing`] is: the application
/// runs on a thread of its own and what it is talking to is the window's
/// loop. None of these may wait for the thing it asks for.
pub trait Windows: std::fmt::Debug + Send + Sync {
    /// Starts another Obelus, in a window of its own, on this tree.
    fn open(&self, tree: &Path);

    /// Asks the window behind this door to come forward.
    ///
    /// The front end's, because what it takes is the compositor's
    /// permission and only the window the reader is in can ask for that:
    /// it is asked for against their last keypress. [`knock`] is how the
    /// permission goes across once there is one.
    fn bring(&self, door: &Door);

    /// Comes forward, for another Obelus whose reader asked to be taken
    /// here.
    ///
    /// With whatever that window was given to come forward with, which a
    /// front end that needs none -- or did not get one -- does without.
    fn come_forward(&self, token: Option<String>);

    /// Whether another window can be brought forward from here at all.
    ///
    /// A compositor without the protocol for it is a key that would do
    /// nothing at the other end, so the rows of the other windows are drawn
    /// and cannot be chosen.
    fn can_bring(&self) -> bool;
}

/// Where another Obelus can be reached, to ask its window to come forward.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Door {
    /// On the loopback, at a port the machine handed out.
    pub address: SocketAddr,
    /// What a knock has to say first.
    ///
    /// The address is open to everything on the machine, and the file this
    /// is written in is the reader's own: whoever can read it is somebody
    /// the reader's window may come forward for.
    pub key: String,
}

/// The tabs of the list of open documents, in the order they sit in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tab {
    /// What is open in this window.
    Documents,
    /// Every checkout of the repository.
    Worktrees,
}

impl Tab {
    /// The word on the tab.
    const fn label(self) -> &'static str {
        match self {
            Self::Documents => "Documents",
            Self::Worktrees => "Worktrees",
        }
    }
}

/// One checkout of the repository, and every Obelus on it.
#[derive(Debug)]
struct Listed {
    tree: obelus_git::Worktree,
    seen: Vec<Seen>,
}

/// An Obelus on a tree, as its claim says -- a terminal's among them.
#[derive(Clone, Debug)]
struct Seen {
    /// The claim itself, which is how this window knows its own.
    claim: PathBuf,
    tree: PathBuf,
    /// Where it can be reached, where it is a window that can be.
    door: Option<Door>,
    /// What it is reading, the way the list of open documents names it.
    reading: String,
}

/// What a row of the worktrees stands for: a tree, or one Obelus on it.
#[derive(Clone, Copy, Debug)]
enum Row {
    Tree(usize),
    Window(usize, usize),
}

/// What this window knows about the others, and what its list shows.
#[derive(Debug, Default)]
pub(super) struct Worktrees {
    /// What the front end can do, where it can do anything.
    windows: Option<Arc<dyn Windows>>,
    /// Where this window can be reached, once it is listening.
    ///
    /// Kept for the life of the process: a reader who settles on a project
    /// from the welcome screen is the same window, and is reached the same
    /// way.
    door: Option<Door>,
    /// This window's claim on the tree it is on.
    present: Option<Present>,
    /// Which tabs the list of open documents has, while it is showing.
    ///
    /// Empty when the list showing is not that one, which is what
    /// [`App::show_list`] makes it.
    pub(super) tabs: Vec<Tab>,
    /// Every checkout, with the Obelus on each.
    listed: Vec<Listed>,
    /// What each row of the worktrees tab stands for.
    rows: Vec<Row>,
    /// The tree the reader chose to go to, while they are asked about what
    /// is unwritten here.
    going: Option<PathBuf>,
    /// The notes, as read to name the conversation in front, and which
    /// document they were read for.
    notes: Option<obelus_todo::Todo>,
    named_for: Option<DocumentId>,
    /// Whether there is another worktree, where a test says so.
    given: Option<bool>,
}

impl Worktrees {
    /// Forgets which list is showing, for a list that is not this one.
    pub(super) fn not_showing(&mut self) {
        self.tabs.clear();
        self.listed.clear();
        self.rows.clear();
    }

    /// The tree a row is on.
    fn tree_of(&self, at: usize) -> Option<&Listed> {
        match self.rows.get(at)? {
            Row::Tree(tree) | Row::Window(tree, _) => self.listed.get(*tree),
        }
    }

    /// Where a row is, in terms that outlast the list being built again:
    /// the tree, and the claim of the Obelus where the row is one.
    fn place_of(&self, at: usize) -> Option<(PathBuf, Option<PathBuf>)> {
        let row = *self.rows.get(at)?;
        let listed = self.tree_of(at)?;
        let claim = match row {
            Row::Tree(_) => None,
            Row::Window(_, seen) => Some(listed.seen.get(seen)?.claim.clone()),
        };
        Some((listed.tree.path.clone(), claim))
    }

    /// The row at a place, where it is still in the list.
    fn row_at(&self, place: &(PathBuf, Option<PathBuf>)) -> Option<usize> {
        (0..self.rows.len()).find(|at| self.place_of(*at).as_ref() == Some(place))
    }

    /// This window's own claim, where it could make one.
    fn own(&self) -> Option<&Path> {
        self.present.as_ref().map(|present| present.path.as_path())
    }

    /// Stops saying this window is on a tree, because the tree has gone or
    /// the reader closed the project.
    ///
    /// A window on a tree nobody can open is not somewhere another window
    /// should send the reader: a tree deleted behind git's back is still
    /// listed, and its row is `Missing` and goes nowhere -- and one made
    /// again at the same path is somebody else's tree, which choosing opens
    /// afresh rather than bringing forward a window that has stopped
    /// hearing anything about it. Nor is one asking which project: brought
    /// forward from the tree's row, it is not on the tree.
    pub(super) fn left_the_tree(&mut self) {
        self.present = None;
    }
}

impl App {
    /// Says what the window Obelus is drawn in can do about other windows.
    ///
    /// Before [`App::start`], which is where this window starts listening
    /// and says which tree it is on.
    pub fn windowed_by(&mut self, windows: Arc<dyn Windows>) {
        self.worktrees.windows = Some(windows);
        // Where the loop's channel is already here -- a test, which is told
        // after it has one -- this is the moment it can be said.
        self.say_where_this_window_is();
    }

    /// Says where this window is, for the others' lists.
    ///
    /// Once there is a project: before that, the tree is the directory the
    /// process happened to start in, which is nowhere a reader works. Done
    /// again by everything that settles on one, which drops the claim on
    /// the old tree as it takes the new.
    pub(super) fn say_where_this_window_is(&mut self) {
        if !self.has_a_project() {
            return;
        }
        if self.worktrees.windows.is_some() && self.worktrees.door.is_none() {
            let Some(events) = self.events.clone() else {
                return;
            };
            match listen(events) {
                Ok(door) => self.worktrees.door = Some(door),
                // Not a reason to stop: a window nobody can bring forward
                // still says which tree it is on, as a terminal's does.
                Err(error) => {
                    tracing::warn!(%error, "no other Obelus can bring this window forward");
                }
            }
        }
        let tree = self.this_tree();
        self.worktrees.present = None;
        let reading = self.what_this_window_is_reading();
        self.worktrees.present = present(
            &self.working_directory,
            &tree,
            self.worktrees.door.as_ref(),
            &reading,
        );
    }

    /// Says again what this window is reading, where that has moved since
    /// it last said.
    ///
    /// Asked once a frame, from what is on screen, for the reason what is
    /// open is written down that way: there are a dozen ways the document
    /// in front of the reader changes, and a rule kept at each is a rule
    /// the next one forgets. The question is a comparison of two names; the
    /// write happens only when the answer moved.
    pub(super) fn say_what_this_window_is_reading(&mut self) {
        if self.worktrees.present.is_none() {
            return;
        }
        let reading = self.what_this_window_is_reading();
        if let Some(present) = self.worktrees.present.as_mut()
            && present.reading != reading
        {
            present.say(reading);
        }
    }

    /// What this window is reading, in the words the list of open documents
    /// names it by.
    ///
    /// A conversation's name may be the note it is about, which is a file
    /// read, and this is asked every frame. So the notes are read once for
    /// the document in front and kept until another is: only where the name
    /// needs them -- a conversation about a note the agent has not named --
    /// and let go of when the reader turns to something else, which is the
    /// moment a name is looked at again.
    fn what_this_window_is_reading(&mut self) -> String {
        if self.worktrees.named_for != self.current {
            self.worktrees.named_for = self.current;
            self.worktrees.notes = None;
        }
        let needs_the_notes = self.conversation().is_some_and(|talk| {
            matches!(talk.topic, crate::conversation::Topic::Note(_)) && !self.has_a_title(talk)
        });
        if needs_the_notes && self.worktrees.notes.is_none() {
            self.worktrees.notes = Some(
                obelus_todo::read(&self.working_directory)
                    .notes()
                    .unwrap_or_default(),
            );
        }
        let none = obelus_todo::Todo::default();
        let notes = self.worktrees.notes.as_ref().unwrap_or(&none);
        let said = match self.current.and_then(|id| self.document(id)) {
            Some(Document::File(buffer)) => relative(buffer.path(), &self.working_directory),
            // Said as the list of open documents says it, by the same
            // function, so the two cannot drift apart.
            Some(Document::Chat(talk)) => self
                .conversation_name(talk, notes)
                .unwrap_or_else(|| "A conversation".to_string()),
            Some(Document::Notes(_)) => "Todo".to_string(),
            Some(Document::Terminal(terminal)) => {
                terminal.title().unwrap_or(terminal.said()).to_string()
            }
            None => String::new(),
        };
        // One line of a file that is read a line at a time.
        said.replace(['\n', '\r'], " ")
    }

    /// Whether `switch-worktree` has anywhere to go.
    pub(super) fn another_worktree(&self) -> bool {
        self.has_a_project()
            && self
                .worktrees
                .given
                .unwrap_or_else(|| obelus_git::has_another_worktree(&self.working_directory))
    }

    /// Says whether there is another worktree to go to, for a test.
    ///
    /// The tests run in a checkout whose worktrees are not theirs to depend
    /// on, for the reason `statuses_for_test` gives about its dirtiness:
    /// the list of what is open grows a tab where there are several, and
    /// the checkout anyone runs them in while working has several.
    pub fn worktrees_for_test(&mut self, another: bool) {
        self.worktrees.given = Some(another);
    }

    /// Opens the list of open documents, on one of its tabs.
    ///
    /// The worktrees are a tab of it rather than a list of their own,
    /// because the question is the same one at a wider radius: where else
    /// is there to be. And only where there is a second worktree to go to
    /// and a window to go there in -- a row of tabs with one tab on it says
    /// there is somewhere else when there is not.
    pub(super) fn open_switching(&mut self, tab: Tab) {
        let tabs = match self.another_worktree() {
            true => vec![Tab::Documents, Tab::Worktrees],
            false => vec![Tab::Documents],
        };
        let mut picker = Picker::new(Vec::new(), PickerLayout::FullArea);
        if tabs.len() > 1 {
            let names: Vec<&str> = tabs.iter().map(|tab| tab.label()).collect();
            picker.with_scopes(&names);
            picker.go_to_tab(tabs.iter().position(|shown| *shown == tab).unwrap_or(0));
        }
        self.show_list(picker);
        self.worktrees.tabs = tabs;
        self.refresh_switching();
    }

    /// Fills the list again for the tab the reader is on.
    ///
    /// On the row they are standing in: this document, or this window. A list
    /// that started somewhere arbitrary would make them find where they are
    /// before they could leave it.
    pub(super) fn refresh_switching(&mut self) {
        let Some(tab) = self
            .picker
            .as_ref()
            .and_then(|picker| self.worktrees.tabs.get(picker.tab()).copied())
        else {
            return;
        };
        let (items, empty, typing) = match tab {
            Tab::Documents => (
                self.document_rows(),
                // Reachable with nothing open at all, which is how Obelus
                // starts -- and was not, for as long as the command asked
                // for a file.
                "Nothing is open",
                "Filter open documents",
            ),
            Tab::Worktrees => (self.worktree_rows(), "No worktrees", "Filter worktrees"),
        };
        let here = self.current;
        let this_window = self.row_of_this_window();
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        picker.replace(items);
        // Per tab, because the two are lists of different things: an open
        // document is somewhere to look, and a tree is a whole checkout and
        // no one file of it. Previewing the file being read under the
        // worktrees said nothing about any row, and halved the list to say
        // it.
        match tab {
            Tab::Documents => picker.previews(),
            Tab::Worktrees => picker.stops_previewing(),
        }
        picker.switches_in_place(tab == Tab::Worktrees && self.worktrees.windows.is_some());
        picker.when_empty(empty);
        picker.before_typing(typing);
        // By the row itself rather than by its label, which is what
        // `prefer` is keyed on: two conversations nobody has named yet are
        // both called the same thing, and a list keyed on what a row *says*
        // would open on the first of them. The file list has to prefer by
        // label because its rows arrive in batches and the one worth
        // starting on is usually not there yet; every row of this list is
        // here already, so it can be pointed at outright.
        let row = picker.matches().position(|item| match (tab, &item.value) {
            (Tab::Documents, PickerValue::Document(id)) => Some(*id) == here,
            (Tab::Worktrees, PickerValue::Worktree { at, .. }) => Some(*at) == this_window,
            _ => false,
        });
        if let Some(row) = row {
            picker.select_row(row);
        }
    }

    /// Fills the list again where it is up, on the row the reader was on
    /// rather than on this document: what moved is not where they are.
    pub(super) fn relist_switching(&mut self) {
        let Some(picker) = self.picker.as_ref() else {
            return;
        };
        if self.worktrees.tabs.get(picker.tab()).is_none() {
            return;
        }
        let on = picker.selected_item().and_then(|item| match item.value {
            PickerValue::Document(id) => Some(id),
            _ => None,
        });
        self.refresh_switching();
        let (Some(on), Some(picker)) = (on, self.picker.as_mut()) else {
            return;
        };
        let row = picker
            .matches()
            .position(|item| matches!(item.value, PickerValue::Document(id) if id == on));
        if let Some(row) = row {
            picker.select_row(row);
        }
    }

    /// Whether the list showing is the worktrees, which is what the watch on
    /// the other windows is wanted for.
    pub(super) fn showing_worktrees(&self) -> bool {
        self.picker
            .as_ref()
            .is_some_and(|picker| self.worktrees.tabs.get(picker.tab()) == Some(&Tab::Worktrees))
    }

    /// Which tab of the list on screen a command names, where the list is
    /// this one.
    pub(super) fn switching_tab_for(&self, command: Command) -> Option<usize> {
        let wanted = match command {
            Command::DocumentList => Tab::Documents,
            Command::WorktreeList => Tab::Worktrees,
            _ => return None,
        };
        self.worktrees.tabs.iter().position(|tab| *tab == wanted)
    }

    /// The tree this window is on.
    fn this_tree(&self) -> PathBuf {
        obelus_git::worktree(&self.working_directory)
            .unwrap_or_else(|| self.working_directory.clone())
    }

    /// The row this window is: its own under its tree, and the tree's where
    /// it could not claim one.
    fn row_of_this_window(&self) -> Option<usize> {
        let tree = self.this_tree();
        let own = self.worktrees.own();
        let rows = &self.worktrees.rows;
        let listed = &self.worktrees.listed;
        let mine = rows.iter().position(|row| match row {
            Row::Window(at, seen) => listed
                .get(*at)
                .and_then(|listed| listed.seen.get(*seen))
                .is_some_and(|seen| Some(seen.claim.as_path()) == own),
            Row::Tree(_) => false,
        });
        mine.or_else(|| {
            rows.iter().position(|row| match row {
                Row::Tree(at) => listed
                    .get(*at)
                    .is_some_and(|listed| same_tree(&listed.tree.path, &tree)),
                Row::Window(..) => false,
            })
        })
    }

    /// One row per checkout, and under it a row for each Obelus on it.
    ///
    /// A row for each because in a window each is somewhere enter goes,
    /// and a tree's own row is not: it is a new window. One alone on a tree
    /// is a row as well, or the only way to it would be gone. And a reader
    /// who keeps two windows on one tree keeps them because they are
    /// reading two things -- so each says what it is reading, which is what
    /// tells them apart.
    ///
    /// Drawn the same where one cannot be gone to, and dim: a terminal's,
    /// which has no door, and every other one where this Obelus cannot
    /// bring a window forward -- a terminal, or a compositor without the
    /// protocol for it. That somebody is there, and reading what, is true
    /// whatever this one is drawn on, and a tree's mark alone could not say
    /// it for the tree this one is on, whose mark is that the reader is.
    fn worktree_rows(&mut self) -> Vec<PickerItem> {
        let seen = windows_on(&self.working_directory);
        let tree = self.this_tree();
        self.worktrees.listed = obelus_git::worktrees(&self.working_directory)
            .into_iter()
            .map(|tree| {
                let seen = seen
                    .iter()
                    .filter(|seen| same_tree(&seen.tree, &tree.path))
                    .cloned()
                    .collect();
                Listed { tree, seen }
            })
            .collect();
        // Named from the directory the main checkout sits in, which is the
        // one rule that names every row the same way: `git worktree add
        // ../name` leaves a tree beside it and so called by its name, and
        // `.worktree/name` -- where the feature-branch workflow puts one --
        // leaves it inside, and called by the way down to it from there.
        // Naming the second by its whole path while the first had a word
        // made the main checkout a name and the others addresses. Anything
        // under that directory is called by the way down to it, and a tree
        // outside it is said in full, because nothing shorter is true.
        // Resolved, and every row resolved before it is compared, because a
        // path as git writes it and the same path resolved can be spelled
        // two ways -- on Windows always, where a resolved path is a `\\?\`
        // one -- and a row compared in the other spelling was called by its
        // whole path.
        let beside = self
            .worktrees
            .listed
            .first()
            .and_then(|first| named_from(&first.tree.path));
        let own = self.worktrees.own().map(Path::to_path_buf);
        let windowed = self.worktrees.windows.is_some();
        let brings = self
            .worktrees
            .windows
            .as_ref()
            .is_some_and(|windows| windows.can_bring());
        let mut rows = Vec::new();
        let mut items = Vec::new();
        for (at, listed) in self.worktrees.listed.iter().enumerate() {
            let path = &listed.tree.path;
            let label = match beside.as_deref().and_then(|beside| {
                resolved_as_far_as_it_goes(path)
                    .strip_prefix(beside)
                    .ok()
                    .map(Path::to_path_buf)
            }) {
                Some(under) if !under.as_os_str().is_empty() => under.display().to_string(),
                _ => obelus_ui::with_home_as_tilde(path),
            };
            let here = same_tree(path, &tree);
            let mine_under = listed
                .seen
                .iter()
                .any(|seen| Some(&seen.claim) == own.as_ref());
            rows.push(Row::Tree(at));
            items.push(worktree_row(Shown {
                at: rows.len() - 1,
                depth: 0,
                icon: obelus_icons::ui::TREE,
                label,
                detail: Some(match &listed.tree.head {
                    obelus_git::Head::Branch(name) => name.clone(),
                    obelus_git::Head::Detached => "Detached".to_string(),
                }),
                // The row that is this window is marked as where the reader
                // is: the tree's where this one is not a row under it, which
                // is a window that could not claim its tree.
                marker: match (here, mine_under) {
                    (true, false) => Some(super::conversations::here()),
                    _ => (here || !listed.seen.is_empty()).then(somebody),
                },
                trailing: (!listed.tree.there).then(|| "Missing".to_string()),
                // In a terminal this one is where the reader is, which is
                // the list closing.
                enabled: here || listed.tree.there,
                enter: match windowed {
                    true => WorktreeEnter::Open,
                    false => WorktreeEnter::Switch,
                },
                switches: windowed && !here && listed.tree.there,
            }));
            for (which, seen) in listed.seen.iter().enumerate() {
                let mine = Some(&seen.claim) == own.as_ref();
                rows.push(Row::Window(at, which));
                items.push(worktree_row(Shown {
                    at: rows.len() - 1,
                    depth: 1,
                    // Which can be brought forward, and which is a terminal
                    // and cannot be.
                    icon: match seen.door {
                        Some(_) => obelus_icons::ui::WINDOW,
                        None => obelus_icons::ui::IN_A_TERMINAL,
                    },
                    label: match seen.reading.is_empty() {
                        true => "Nothing open".to_string(),
                        false => seen.reading.clone(),
                    },
                    detail: None,
                    marker: mine.then(super::conversations::here),
                    trailing: None,
                    enabled: mine || (brings && seen.door.is_some()),
                    enter: match mine {
                        true => WorktreeEnter::Stay,
                        false => WorktreeEnter::Bring,
                    },
                    switches: false,
                }));
            }
        }
        self.worktrees.rows = rows;
        items
    }

    /// Builds the worktrees again while the list is up, with the reader on
    /// the row they were on: another Obelus opened, closed, or turned to
    /// something else as they were looking.
    ///
    /// Built again rather than marked again, because what moved may be
    /// rows: a second Obelus on a tree is a row under it.
    pub(super) fn reread_the_windows(&mut self) {
        if !self.showing_worktrees() {
            return;
        }
        let on = self
            .picker
            .as_ref()
            .and_then(Picker::selected_item)
            .and_then(|item| match item.value {
                PickerValue::Worktree { at, .. } => self.worktrees.place_of(at),
                _ => None,
            });
        let items = self.worktree_rows();
        // The tree, where the Obelus the reader was on has gone from it.
        let at = on.and_then(|on| {
            self.worktrees
                .row_at(&on)
                .or_else(|| self.worktrees.row_at(&(on.0, None)))
        });
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        picker.replace(items);
        let row = at.and_then(|at| {
            picker.matches().position(
                |item| matches!(item.value, PickerValue::Worktree { at: row, .. } if row == at),
            )
        });
        if let Some(row) = row {
            picker.select_row(row);
        }
    }

    /// Whether a path that changed is one of this project's windows.
    pub(super) fn is_a_window(&self, path: &Path) -> bool {
        directory(&self.working_directory)
            .is_some_and(|directory| path.parent() == Some(directory.as_path()))
    }

    /// Puts this window on the tree a row names.
    ///
    /// Asking first where something is unwritten, the way leaving does:
    /// the tree being left is still there, so what is unwritten has
    /// somewhere to go, and the reader says whether it goes. The notes are
    /// written without asking, as on the way out.
    pub(super) fn go_to_worktree(&mut self, at: usize) {
        let Some(listed) = self.worktrees.tree_of(at) else {
            return;
        };
        let path = listed.tree.path.clone();
        // Asked of the disk again, for the reason the window is: a tree
        // that went since the row was drawn is nowhere to be put.
        if same_tree(&path, &self.this_tree()) || obelus_git::is_gone(&path) {
            return;
        }
        self.write_the_notes();
        let unsaved = self
            .documents
            .iter()
            .flatten()
            .filter_map(Document::file)
            .filter(|buffer| buffer.is_dirty())
            .count();
        if unsaved > 0 {
            self.worktrees.going = Some(path);
            self.ask_before_switching(unsaved);
            return;
        }
        self.move_to_tree(&path);
    }

    /// The tree the reader was asked about going to, taken.
    pub(super) fn going(&mut self) -> Option<PathBuf> {
        self.worktrees.going.take()
    }

    /// Lets go of this project and settles on `tree` in its place.
    ///
    /// What was open is written down first, so that coming back opens it
    /// again: going is leaving, as far as this tree is concerned.
    pub(super) fn move_to_tree(&mut self, tree: &Path) {
        tracing::info!(tree = %tree.display(), "putting this window on another worktree");
        // Asked before the letting go, which takes the chat with it.
        let reached = self.has_the_remote();
        self.write_down_what_is_open_on_leaving();
        self.let_go_of_the_project();
        self.settle(tree.to_path_buf(), &[]);
        // The chat is reached through this window, and the reader who put it
        // here did not say "for this tree": a reader away from the machine
        // who goes to another tree from the chat would otherwise have cut
        // themselves off. Connected again rather than carried over, the way
        // a start that was told to connects -- what the chat's threads were
        // about was the tree that has gone.
        if reached {
            self.connect_remote();
        }
    }

    /// Goes to a row in a window of its own: the window the row is, or a
    /// new one on the tree the row is.
    ///
    /// A tree's row is always a new window, this window's own tree among
    /// them: the windows already on a tree are rows of their own under it,
    /// so a tree's row is never the way to one of them. Which window a
    /// window's row is is asked again here rather than read off the row,
    /// because the row was drawn a moment ago and the window may have
    /// closed since -- and bringing forward a window that has gone is a key
    /// that does nothing. A window that closed is a tree to open a window
    /// on.
    pub(super) fn go_elsewhere(&mut self, at: usize) {
        let (Some((path, claim)), Some(windows)) =
            (self.worktrees.place_of(at), self.worktrees.windows.clone())
        else {
            return;
        };
        let door = claim.and_then(|claim| {
            windows_on(&self.working_directory)
                .into_iter()
                .find(|seen| seen.claim == claim)
                .and_then(|seen| seen.door)
        });
        match door {
            Some(door) if windows.can_bring() => {
                tracing::info!(tree = %path.display(), "bringing the window on a worktree forward");
                windows.bring(&door);
            }
            // Asked of the disk again, for the reason the window is: a tree
            // that went since the row was drawn is no tree to start an
            // Obelus on.
            _ if !obelus_git::is_gone(&path) => {
                tracing::info!(tree = %path.display(), "opening a window on a worktree");
                windows.open(&path);
            }
            _ => {}
        }
    }

    /// Another Obelus's reader asked to be brought here.
    pub(super) fn summoned(&self, token: Option<String>) {
        if let Some(windows) = self.worktrees.windows.as_ref() {
            windows.come_forward(token);
        }
    }
}

/// One row of the worktrees, before it is a row of a list.
struct Shown {
    at: usize,
    depth: u16,
    icon: char,
    label: String,
    detail: Option<String>,
    /// That an Obelus is on it, for a tree, or that it is this window.
    marker: Option<(Marking, String)>,
    trailing: Option<String>,
    enabled: bool,
    enter: WorktreeEnter,
    /// Whether `ctrl+enter` puts this window on the row's tree.
    switches: bool,
}

/// The mark on a tree an Obelus is on.
///
/// Not the bullet without a nerd font, which is the mark on the row that is
/// this window: two rows marked alike would say the reader is on both.
fn somebody() -> (Marking, String) {
    (
        Marking::Aside,
        match obelus_icons::enabled() {
            true => obelus_icons::ui::WINDOW.to_string(),
            false => "\u{25e6}".to_string(),
        },
    )
}

/// A row of the worktrees, as the list draws it.
fn worktree_row(shown: Shown) -> PickerItem {
    PickerItem {
        icon: obelus_icons::enabled().then_some(shown.icon),
        label: shown.label,
        version: None,
        detail: shown.detail,
        prose: false,
        marker: shown.marker,
        trailing: shown.trailing,
        changed: None,
        value: PickerValue::Worktree {
            at: shown.at,
            enter: shown.enter,
            switches: shown.switches,
        },
        depth: shown.depth,
        opens: None,
        status: None,
        enabled: shown.enabled,
        colours: None,
        kind: None,
        tab: None,
        section: None,
    }
}

/// Whether two paths name the same tree.
///
/// Resolved where they can be, because git writes down the path a tree was
/// added by and a window its own, and one may be through a link the other
/// is not. A tree that has gone resolves to nothing, and is taken as it
/// was written.
fn same_tree(one: &Path, other: &Path) -> bool {
    one == other || resolved(one) == resolved(other)
}

/// A path with its links followed, or as it was written where it has gone.
fn resolved(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// The directory rows are named from: the one the main checkout sits in.
///
/// Not the root of the disk. A main checkout at `/repo` sits in `/`, and
/// every path there is under it -- so a tree at `/tmp/x` would have been
/// called `tmp/x`, a path with its first character taken off.
fn named_from(main: &Path) -> Option<PathBuf> {
    main.parent()
        .filter(|parent| parent.parent().is_some())
        .map(resolved)
}

/// A path with its links followed as far as it is still there, and the
/// rest of it as it was written.
///
/// For a row's name, which is worked out against the main checkout
/// resolved. A tree deleted behind git's back resolves to nothing, and
/// taken as it was written it is a different spelling of the place
/// wherever resolving changes the spelling -- on Windows always, where a
/// resolved path is a `\\?\` one -- so a tree called `spare` the moment
/// before was called by its whole path the moment it went. Held by
/// `a_tree_that_has_gone_is_missing_and_goes_nowhere`, on Windows: git
/// resolves a link when a tree is added, so nothing on Linux spells the
/// place two ways.
fn resolved_as_far_as_it_goes(path: &Path) -> PathBuf {
    path.canonicalize()
        .unwrap_or_else(|_| match (path.parent(), path.file_name()) {
            (Some(parent), Some(name)) => resolved_as_far_as_it_goes(parent).join(name),
            _ => path.to_path_buf(),
        })
}

/// Where a project's windows say where they are, one file each.
pub(super) fn directory(root: &Path) -> Option<PathBuf> {
    Some(
        obelus_logging::state_directory()?
            .join("windows")
            .join(obelus_git::project(root)?),
    )
}

/// A window's claim on the tree it is on, given up by being dropped.
#[derive(Debug)]
struct Present {
    path: PathBuf,
    /// Held open for as long as the claim is: the lock belongs to the open
    /// file, which is what makes a killed Obelus give it up. And written
    /// through, because the lock is the file's and a new file is no claim.
    file: File,
    /// The lines that do not change: the tree, and how to reach this window.
    head: String,
    /// What it last said it was reading.
    reading: String,
}

impl Present {
    /// Says what this window is reading now, over what it said before.
    ///
    /// In place, in one write from the start: what another Obelus reads is
    /// the old words or the new, the head the same in both. Cut to length
    /// after, where the new is shorter, so that for a moment the old tail
    /// may follow it -- a line after the last, which nobody reads.
    fn say(&mut self, reading: String) {
        use std::io::Seek as _;

        let said = format!("{}{reading}\n", self.head);
        let written = (&self.file)
            .seek(std::io::SeekFrom::Start(0))
            .and_then(|_| (&self.file).write_all(said.as_bytes()))
            .and_then(|()| self.file.set_len(said.len() as u64));
        if let Err(error) = written {
            tracing::warn!(%error, path = %self.path.display(), "a window does not say what it is reading");
        }
        self.reading = reading;
    }
}

impl Drop for Present {
    fn drop(&mut self) {
        // The name goes and then the lock, which is the order another
        // Obelus wants: it wakes on the file going, and by the time it has
        // looked the lock has gone too.
        if let Err(error) = std::fs::remove_file(&self.path) {
            tracing::debug!(%error, path = %self.path.display(), "a window's claim outlived it");
        }
    }
}

/// Says this window is on `tree`, and can be reached at `door` where it can.
///
/// Named by the window, not by the tree: two windows on one tree are two
/// claims. The process for whoever reads the directory, a word of the key
/// so that a number the system hands out again after a restart is not the
/// same name, and a count because two applications in one process -- which
/// is what a test is -- are two windows as well.
///
/// **A claim appears already held.** It is made under a name nobody reads,
/// locked and written there, and only then renamed to its own -- the lock
/// belongs to the open file and goes with it. So a file under its own name
/// that nobody holds is a window that died without tidying up, and whoever
/// finds one may take it away ([`windows_on`]). A name nobody reuses would
/// otherwise be a file per window that was ever killed, kept for ever.
fn present(root: &Path, tree: &Path, door: Option<&Door>, reading: &str) -> Option<Present> {
    static COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let directory = directory(root)?;
    std::fs::create_dir_all(&directory).ok()?;
    let count = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let key = door.map_or_else(obelus_claim::a_key, |door| door.key.clone());
    let word = key.get(..8).unwrap_or_default();
    let name = format!("{}-{word}-{count}", std::process::id());
    let (making, path) = (directory.join(format!(".{name}")), directory.join(&name));
    // For writing, for the reason a conversation's claim is: a process
    // that dies closes it, and a file closed by a writer is the one notice
    // a watcher gives of that.
    let file = File::options()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&making)
        .ok()?;
    if obelus_claim::held_by_somebody_else(&file) {
        return None;
    }
    let tree = tree.canonicalize().unwrap_or_else(|_| tree.to_path_buf());
    // The address and the key left blank for a window with no door: an
    // Obelus before this one read those or nothing, so it takes a
    // terminal's for nobody's. What it is reading after them, which an
    // Obelus before this one never asked for.
    let head = match door {
        Some(door) => format!("{}\n{}\n{}\n", tree.display(), door.address, door.key),
        None => format!("{}\n\n\n", tree.display()),
    };
    let said = format!("{head}{reading}\n");
    if let Err(error) = (&file).write_all(said.as_bytes()) {
        tracing::warn!(%error, path = %making.display(), "a window does not say where it is");
    }
    if let Err(error) = std::fs::rename(&making, &path) {
        tracing::warn!(%error, path = %path.display(), "a window could not say where it is");
        let _ = std::fs::remove_file(&making);
        return None;
    }
    Some(Present {
        path,
        file,
        head,
        reading: reading.to_string(),
    })
}

/// Every Obelus on this project that is open, with the tree it is on.
///
/// This one's own among them: a lock is about the open file and not about
/// the process, so a look from here finds this window's claim held too.
/// Which is the truth about its tree.
fn windows_on(root: &Path) -> Vec<Seen> {
    let Some(directory) = directory(root) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&directory) else {
        return Vec::new();
    };
    entries
        .flatten()
        // One still being made, which is nobody's to read yet.
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|entry| {
            // For reading, so that looking cannot wake anybody: see
            // `obelus_claim::held_by_somebody_else`.
            let mut file = File::options().read(true).open(entry.path()).ok()?;
            let mut said = String::new();
            file.read_to_string(&mut said).ok()?;
            if !obelus_claim::held_by_somebody_else(&file) {
                // Left by a window that died, and nobody's: a claim only
                // ever appears held, so nobody is about to take it either.
                // Taken away, which wakes the others once and is the last
                // anybody hears of it.
                let _ = std::fs::remove_file(entry.path());
                return None;
            }
            let mut lines = said.lines();
            let tree = PathBuf::from(lines.next()?);
            let (address, key) = (lines.next(), lines.next());
            let door = address
                .and_then(|address| address.parse().ok())
                .zip(key.filter(|key| !key.is_empty()))
                .map(|(address, key)| Door {
                    address,
                    key: key.to_string(),
                });
            let reading = lines.next().unwrap_or_default().to_string();
            Some(Seen {
                claim: entry.path(),
                tree,
                door,
                reading,
            })
        })
        .collect()
}

/// Starts listening for another Obelus asking this window to come forward.
///
/// A task on the one runtime, like the tools an agent is offered: it waits
/// and does no work. Each knock is one line -- the key, then whatever the
/// window was given to come forward with -- and the loop hears it as
/// [`Event::Summoned`].
fn listen(events: std::sync::mpsc::Sender<Event>) -> std::io::Result<Door> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    listener.set_nonblocking(true)?;
    let key = obelus_claim::a_key();
    let expected = key.clone();
    obelus_runtime::handle().spawn(async move {
        use tokio::io::AsyncBufReadExt as _;

        let listener = match tokio::net::TcpListener::from_std(listener) {
            Ok(listener) => listener,
            Err(error) => {
                tracing::warn!(%error, "this window is not listening for another Obelus");
                return;
            }
        };
        while let Ok((stream, _)) = listener.accept().await {
            let mut line = String::new();
            // A line and no more: a knock is a key and a token, and a
            // stranger who sends a megabyte is not knocking.
            let mut reading =
                tokio::io::BufReader::new(tokio::io::AsyncReadExt::take(stream, 4096));
            if reading.read_line(&mut line).await.is_err() {
                continue;
            }
            let mut words = line.trim_end().splitn(2, ' ');
            if words.next() != Some(expected.as_str()) {
                tracing::warn!("a knock on this window without its key");
                continue;
            }
            let token = words.next().filter(|token| !token.is_empty());
            if events
                .send(Event::Summoned(token.map(str::to_string)))
                .is_err()
            {
                return;
            }
        }
    });
    Ok(Door { address, key })
}

/// Asks the window behind `door` to come forward, with what this one was
/// given to let it.
///
/// For the front end, once it has what the compositor gave it: the other
/// half of [`listen`], in the one place that knows what a knock says.
pub fn knock(door: &Door, token: Option<&str>) {
    let said = format!("{} {}\n", door.key, token.unwrap_or_default());
    let address = door.address;
    obelus_runtime::handle().spawn(async move {
        use tokio::io::AsyncWriteExt as _;

        let knocked = async {
            let mut stream = tokio::net::TcpStream::connect(address).await?;
            stream.write_all(said.as_bytes()).await?;
            stream.shutdown().await
        };
        if let Err(error) = knocked.await {
            tracing::warn!(%error, %address, "the other window could not be reached");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A main checkout at the root of a disk names nothing from where it
    /// sits, and one anywhere else names from its parent.
    ///
    /// Deliberate break: drop the filter in `named_from`, and `/repo`
    /// names every row from `/`.
    #[test]
    fn rows_are_not_named_from_the_root() {
        let root = std::env::temp_dir()
            .ancestors()
            .last()
            .expect("a path has a root")
            .to_path_buf();
        assert_eq!(named_from(&root.join("repo")), None);
        let deeper = root.join("nowhere-obelus-made").join("repo");
        assert_eq!(named_from(&deeper), Some(root.join("nowhere-obelus-made")));
    }
}
