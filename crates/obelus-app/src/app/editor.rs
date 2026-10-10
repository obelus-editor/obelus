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
