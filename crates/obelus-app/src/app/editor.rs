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
pub(super) mod reading;
pub(super) mod searching;
pub(super) mod tabs;

/// What is being searched for, at which scope, and the answers on their way:
/// the walk, what it has found, and the file each row's colours come from.
#[derive(Debug, Default)]
pub(in crate::app) struct Search {
    /// Which scope the last search opened at, which is the open search's
    /// while one is up.
    ///
    /// Outlives the list, so whatever reads it asks whether the list on
    /// screen is a search as well: the palette opened in a search's place
    /// is not one.
    pub(in crate::app) searching: Option<Scope>,
    /// Which file and version the search's rows were gathered from.
    ///
    /// The file scope's rows are its lines, so they are only right for the
    /// version they were read from: an agent rewriting the file while the
    /// search is open has to change what the list says.
    pub(in crate::app) searched: Option<(PathBuf, i32)>,
    /// What the narrowest scope of the open search is searching, which is
    /// whatever was in front when it opened.
    pub(in crate::app) here: Option<searching::Here>,
    /// And which document that is, for the marks and the scroll a search of
    /// a page leaves on it: taken off that document whatever is in front
    /// by the time the list goes.
    pub(in crate::app) document: Option<DocumentId>,
    /// What the page searched was when its rows were found -- see
    /// `page_stamp`.
    pub(in crate::app) stamp: Option<(u64, usize, u16)>,
    /// What the last search to close was asking, which the next one opens
    /// asking again.
    pub(in crate::app) last_query: String,
    /// Where each scope's last search was left, for the next one opened at
    /// that scope to stand where it stood.
    pub(in crate::app) kept: Vec<searching::Kept>,
    /// Whether the walk of the project has finished, so that what it found
    /// is all of it and worth keeping.
    pub(in crate::app) walked: bool,
    /// What the project's tab shows on its next refresh instead of walking
    /// the project again: the rows a search left, put back.
    pub(in crate::app) reuse: Option<Vec<PickerItem>>,
    /// Where the page under a search of it was scrolled to before the list
    /// showed the reader somewhere else in it, to go back to if they leave
    /// without choosing. A file's reading needs none of this: its view is
    /// the buffer's, which `looked_from` already keeps.
    pub(in crate::app) looked_within: Option<searching::Before>,
    /// What the page was last marked and scrolled for -- which row, and
    /// which of its characters -- so the frame that asks again about the
    /// same match does no work.
    pub(in crate::app) looked_at: Option<(usize, std::ops::Range<usize>)>,
    /// What the query matched on the row the search of the file is on, as
    /// a run of the file: the file is that list's preview, and marks what
    /// the preview of a list that covers it would mark.
    pub(in crate::app) marked: Option<obelus_text::coordinates::Span>,
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
