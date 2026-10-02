//! Going to another of the repository's worktrees, in a window of its own.
//!
//! Obelus does not split its window, so a reader with three worktrees of
//! one repository has three Obelus processes on it -- and what the list of
//! open documents could not say is where the other two are. Its second tab
//! is every checkout the repository has, and choosing one goes there: to
//! the window already open on it, or to a new one.
//!
//! **Only where Obelus draws its own window.** Going somewhere is starting
//! an Obelus or bringing one forward, and a window can do both: it can
//! start a program that opens a window, and it can hand that window -- or
//! another Obelus's -- the compositor's permission to come to the front. A
//! terminal can do neither. The window it is in is the terminal's, and
//! which terminal the reader would want a new Obelus started in is not
//! something Obelus can know. So [`Windows`] is what a front end says it
//! can do, and a terminal says nothing: no tab, and `switch-worktree` dim.
//!
//! **A window on a tree is a claim, held the way a conversation's is.** A
//! lock the kernel gives up with the process, and a file beside it for the
//! watcher to wake on -- see `obelus_agent::chats`, whose argument this is
//! word for word. One file per window rather than per tree, because two
//! windows on one tree is as ordinary as two on one project, and the file
//! says which tree and how to reach the window: an address on the loopback
//! and a key that a stranger on the same machine does not have.
//!
//! **A claim appears already held.** It is made under a name nobody reads
//! and renamed once it is locked, so a file nobody holds is a window that
//! died -- which the next look takes away, since every window is a new
//! name and nothing else would (`present`).
//!
//! **A tree that has gone is nowhere to go.** git lists a checkout deleted
//! behind its back until somebody prunes it, and the row says `Missing`
//! and cannot be chosen. A window that was on it has stopped saying so --
//! a tree that goes takes the project and leaves the window, and the
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
    Marking, Picker, PickerItem, PickerLayout, PickerValue, Remark, Said,
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
    /// nothing at the other end, so a row held by another window opens a
    /// new one there instead.
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

/// One row of the worktrees, and the window on it where there is one.
#[derive(Debug)]
struct Listed {
    tree: obelus_git::Worktree,
    door: Option<Door>,
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
    /// What each row of the worktrees tab stands for.
    listed: Vec<Listed>,
}

impl Worktrees {
    /// Forgets which list is showing, for a list that is not this one.
    pub(super) fn not_showing(&mut self) {
        self.tabs.clear();
        self.listed.clear();
    }

    /// Stops saying this window is on a tree, because the tree has gone.
    ///
    /// A window on a tree nobody can open is not somewhere another window
    /// should send the reader: a tree deleted behind git's back is still
    /// listed, and its row is `Missing` and goes nowhere -- and one made
    /// again at the same path is somebody else's tree, which choosing opens
    /// afresh rather than bringing forward a window that has stopped
    /// hearing anything about it.
    pub(super) fn tree_has_gone(&mut self) {
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
        if self.worktrees.windows.is_none() || !self.has_a_project() {
            return;
        }
        if self.worktrees.door.is_none() {
            let Some(events) = self.events.clone() else {
                return;
            };
            match listen(events) {
                Ok(door) => self.worktrees.door = Some(door),
                Err(error) => {
                    // Not a reason to stop: a window nobody can bring
                    // forward is the window every Obelus was until now.
                    tracing::warn!(%error, "no other Obelus can bring this window forward");
                    return;
                }
            }
        }
        let Some(door) = self.worktrees.door.clone() else {
            return;
        };
        let tree = obelus_git::worktree(&self.working_directory)
            .unwrap_or_else(|| self.working_directory.clone());
        self.worktrees.present = None;
        self.worktrees.present = present(&self.working_directory, &tree, &door);
    }

    /// Whether `switch-worktree` has anywhere to go.
    ///
    /// The window first, because it is free and a terminal stops there.
    pub(super) fn another_worktree(&self) -> bool {
        self.worktrees.windows.is_some()
            && self.has_a_project()
            && obelus_git::has_another_worktree(&self.working_directory)
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
    /// On the row they are standing in: this document, or this tree. A list
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
        let tree = self.this_tree();
        let listed = &self.worktrees.listed;
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
            (Tab::Worktrees, PickerValue::Worktree(at)) => listed
                .get(*at)
                .is_some_and(|listed| same_tree(&listed.tree.path, &tree)),
            _ => false,
        });
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

    /// One row per checkout, and which of them a window has.
    fn worktree_rows(&mut self) -> Vec<PickerItem> {
        let windows = windows_on(&self.working_directory);
        let tree = self.this_tree();
        self.worktrees.listed = obelus_git::worktrees(&self.working_directory)
            .into_iter()
            .map(|tree| {
                let door = door_of(&windows, &tree.path);
                Listed { tree, door }
            })
            .collect();
        // Named from the directory the main checkout sits in, which is the
        // one rule that names every row the same way: `git worktree add
        // ../name` leaves a tree beside it and so called by its name, and
        // `.worktree/name` -- where the feature-branch workflow puts one --
        // leaves it inside, and called by the way down to it from there.
        // Naming the second by its whole path while the first had a word
        // made the main checkout a name and the others addresses. A tree
        // anywhere else is said in full, because nothing shorter is true.
        // Resolved, because git hands the main checkout back resolved and
        // the linked ones as they were added -- which on a mac, whose
        // temporary directory is a link, are two spellings of one place.
        let beside = self
            .worktrees
            .listed
            .first()
            .and_then(|first| first.tree.path.parent().map(resolved));
        self.worktrees
            .listed
            .iter()
            .enumerate()
            .map(|(at, listed)| {
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
                let Said { marker, enabled } = said(listed, here);
                PickerItem {
                    icon: obelus_icons::enabled().then_some(obelus_icons::ui::TREE),
                    label,
                    detail: Some(match &listed.tree.head {
                        obelus_git::Head::Branch(name) => name.clone(),
                        obelus_git::Head::Detached => "Detached".to_string(),
                    }),
                    prose: false,
                    marker,
                    trailing: match (here, listed.tree.there) {
                        (true, _) => Some("This window".to_string()),
                        (false, false) => Some("Missing".to_string()),
                        (false, true) => None,
                    },
                    changed: None,
                    value: PickerValue::Worktree(at),
                    depth: 0,
                    opens: None,
                    status: None,
                    enabled,
                    colours: None,
                    kind: None,
                    tab: None,
                    section: None,
                }
            })
            .collect()
    }

    /// Looks again at which windows are on which tree, while the list is
    /// up: another window opened or closed as the reader was looking.
    pub(super) fn reread_the_windows(&mut self) {
        if !self.showing_worktrees() {
            return;
        }
        let windows = windows_on(&self.working_directory);
        let tree = self.this_tree();
        for listed in &mut self.worktrees.listed {
            listed.door = door_of(&windows, &listed.tree.path);
        }
        let listed = &self.worktrees.listed;
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        picker.remark(|value| match value {
            PickerValue::Worktree(at) => listed.get(*at).map_or(Remark::Keep, |listed| {
                Remark::Now(said(listed, same_tree(&listed.tree.path, &tree)))
            }),
            _ => Remark::Keep,
        });
    }

    /// Whether a path that changed is one of this project's windows.
    pub(super) fn is_a_window(&self, path: &Path) -> bool {
        directory(&self.working_directory)
            .is_some_and(|directory| path.parent() == Some(directory.as_path()))
    }

    /// Goes to the tree a row names: to the window on it, or to a new one.
    ///
    /// Which window is asked again here rather than read off the row,
    /// because the row was drawn a moment ago and a window may have closed
    /// since -- and bringing forward a window that has gone is a key that
    /// does nothing. A window that closed is a tree to open a window on.
    pub(super) fn go_to_worktree(&mut self, at: usize) {
        let (Some(listed), Some(windows)) = (
            self.worktrees.listed.get(at),
            self.worktrees.windows.clone(),
        ) else {
            return;
        };
        let path = listed.tree.path.clone();
        if same_tree(&path, &self.this_tree()) {
            return;
        }
        let door = door_of(&windows_on(&self.working_directory), &path);
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

/// What a row of the worktrees says about itself: whether a window has it,
/// and whether there is anywhere for the key to go.
///
/// One answer for the rows as they are built and as they are asked again,
/// so that asking again cannot disagree with the first.
fn said(listed: &Listed, here: bool) -> Said {
    Said {
        marker: (listed.door.is_some() || here).then(|| {
            (
                Marking::Aside,
                match obelus_icons::enabled() {
                    true => obelus_icons::ui::WINDOW.to_string(),
                    false => "\u{2022}".to_string(),
                },
            )
        }),
        // This one is where the reader is, which is the list closing.
        enabled: here || listed.tree.there,
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

/// The door of a window on this tree, where one is open.
fn door_of(windows: &[(PathBuf, Door)], tree: &Path) -> Option<Door> {
    windows
        .iter()
        .find(|(on, _)| same_tree(on, tree))
        .map(|(_, door)| door.clone())
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
    /// file, which is what makes a killed Obelus give it up.
    #[expect(
        dead_code,
        reason = "it is the lock itself: what it is for is staying open"
    )]
    file: File,
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

/// Says this window is on `tree` and can be reached at `door`.
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
fn present(root: &Path, tree: &Path, door: &Door) -> Option<Present> {
    static COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let directory = directory(root)?;
    std::fs::create_dir_all(&directory).ok()?;
    let count = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let word = door.key.get(..8).unwrap_or_default();
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
    if obelus_agent::chats::held_by_somebody_else(&file) {
        return None;
    }
    let tree = tree.canonicalize().unwrap_or_else(|_| tree.to_path_buf());
    let said = format!("{}\n{}\n{}\n", tree.display(), door.address, door.key);
    if let Err(error) = (&file).write_all(said.as_bytes()) {
        tracing::warn!(%error, path = %making.display(), "a window does not say where it is");
    }
    if let Err(error) = std::fs::rename(&making, &path) {
        tracing::warn!(%error, path = %path.display(), "a window could not say where it is");
        let _ = std::fs::remove_file(&making);
        return None;
    }
    Some(Present { path, file })
}

/// Every window on this project that is open, with the tree it is on.
///
/// This one's own among them: a lock is about the open file and not about
/// the process, so a look from here finds this window's claim held too.
/// Which is the truth about its tree.
fn windows_on(root: &Path) -> Vec<(PathBuf, Door)> {
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
            // `obelus_agent::chats::held_by_somebody_else`.
            let mut file = File::options().read(true).open(entry.path()).ok()?;
            let mut said = String::new();
            file.read_to_string(&mut said).ok()?;
            if !obelus_agent::chats::held_by_somebody_else(&file) {
                // Left by a window that died, and nobody's: a claim only
                // ever appears held, so nobody is about to take it either.
                // Taken away, which wakes the others once and is the last
                // anybody hears of it.
                let _ = std::fs::remove_file(entry.path());
                return None;
            }
            let mut lines = said.lines();
            let tree = PathBuf::from(lines.next()?);
            let address = lines.next()?.parse().ok()?;
            let key = lines.next()?.to_string();
            Some((tree, Door { address, key }))
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
    let key = a_key();
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

/// A key nobody else has, for the door.
///
/// From the hasher std seeds with randomness for every map, which is
/// enough for a word only this reader's files say.
fn a_key() -> String {
    use std::hash::{BuildHasher as _, Hasher as _};

    let half = || {
        let mut hasher = std::hash::RandomState::new().build_hasher();
        hasher.write_u32(std::process::id());
        hasher.finish()
    };
    format!("{:016x}{:016x}", half(), half())
}
