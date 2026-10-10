//! The file being read: opening, moving about, typing, finding, and the
//! lists that choose what to read next.

use super::*;

pub(super) mod asking;
pub(super) mod choosing;
pub(super) mod counting;
pub(super) mod documents;
pub(super) mod keys;
pub(super) mod moving;
pub(super) mod previewing;
pub(super) mod searching;

/// What is being searched for, at which scope, and the answers on their way:
/// the walk, what it has found, and the file each row's colours come from.
#[derive(Debug, Default)]
pub(in crate::app) struct Search {
    /// Which scopes the open search is showing, in tab order.
    ///
    /// The tabs are only the scopes that can answer, so which tab is which
    /// scope is not a fixed mapping and has to be remembered.
    pub(in crate::app) searching: Vec<Scope>,
    /// Which file and version the search's rows were gathered from.
    ///
    /// The file scope's rows are its lines, so they are only right for the
    /// version they were read from: an agent rewriting the file while the
    /// search is open has to change what the list says.
    pub(in crate::app) searched: Option<(PathBuf, i32)>,
    /// Whether the symbols search offers names from outside the project.
    ///
    /// Off, because a server that has indexed a project has indexed what it
    /// was built on too: a search for `new` answered from the whole index is
    /// the registry's answer with the reader's own names somewhere in it.
    /// The switch is for the times they meant the dependency.
    pub(in crate::app) outside: bool,
    /// How the search is looking: the three switches at its foot.
    ///
    /// The reader's, kept for as long as Obelus is running and not written
    /// to their settings: a pattern answers *this* question, and one turned
    /// on to find one thing should not still be on next week.
    pub(in crate::app) looking: obelus_search::Looking,
    /// Which search the answers arriving belong to.
    ///
    /// Bumped on every keystroke that changes what is being asked, so the
    /// batches for the query before it are recognizable as stale. Shared
    /// with the scanning threads, which read it to find out that they are
    /// answering a question nobody is asking any more.
    pub(in crate::app) search_generation: obelus_runtime::cancel::Latest,
    /// Files parsed only to colour a search's rows.
    ///
    /// A search of a project answers with lines from files that are not
    /// open, and a line reads like code only if something has parsed the
    /// file it is a line of. Filled for the rows on screen and dropped when
    /// the list closes: this is a cache for one list's lifetime, not a
    /// second set of buffers.
    pub(in crate::app) row_syntax: std::collections::HashMap<PathBuf, Buffer>,
}

/// The project's files as the lists have walked them: the walk under way, what
/// it has found, which directories of the tree are open, and what git says of
/// each.
#[derive(Debug, Default)]
pub(in crate::app) struct Files {
    /// Which file walk the picker is currently expecting batches from.
    ///
    /// Bumped every time a file picker opens, so batches from a walk whose
    /// picker has already closed are recognizable and dropped.
    pub(in crate::app) walk_generation: obelus_runtime::cancel::Latest,
    /// Which listings the open file list is showing, in tab order.
    ///
    /// The changed listing has a tab only when something has changed, so
    /// which tab is which listing is not fixed.
    pub(in crate::app) listing: Vec<Listing>,
    /// Every path the walk behind the open file list has found, and
    /// whether the tree said to ignore it.
    ///
    /// Put away so the flat listing can be shown again without walking
    /// again: the reader types, the tree is put down and these are picked
    /// up, and clearing the query puts them back down. With the flag,
    /// because it is the walk that knows which files are only there
    /// because the reader asked for them.
    pub(in crate::app) found: Vec<(PathBuf, bool)>,
    /// The row the tree was on when the reader started typing.
    ///
    /// Typing turns the file list from a tree into the flat list of
    /// everything, and clearing the query turns it back. The tree either
    /// side of that is the same tree, so this is what puts the reader back
    /// on the row they were reading rather than on the file they happen to
    /// have open.
    pub(in crate::app) stood_on: Option<PathBuf>,
    /// Which directories of the file tree are open, relative to the root.
    ///
    /// Beside the list rather than in it, the way a history's opened commit
    /// and a tree of calls are: the list is rows, and which of them exist
    /// is worked out from this.
    pub(in crate::app) opened: std::collections::HashSet<PathBuf>,
    /// What git says about the files in the tree, while a list of them is
    /// open.
    ///
    /// Gathered when a list opens and kept until the next one, because it is
    /// a walk of the whole tree and the rows arrive in batches afterwards.
    pub(in crate::app) statuses: std::collections::HashMap<PathBuf, obelus_git::Standing>,
    /// What a test said git would say, instead of asking it.
    pub(in crate::app) given_statuses: Option<HashMap<PathBuf, obelus_git::Standing>>,
}
