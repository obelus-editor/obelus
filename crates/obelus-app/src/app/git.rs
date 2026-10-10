//! What git and GitHub say about the project: the margin, the history,
//! the trees, and the pull requests and issues.

use super::*;

pub(super) mod history;
pub(super) mod history_view;
pub mod pulls;
pub(super) mod worktrees;

/// What git has said about the files being read, kept until `HEAD` or the index
/// moves: what changed in each, the commit a version came from, who wrote each
/// line and which of those walks are under way, and the walk of the history.
#[derive(Debug, Default)]
pub(in crate::app) struct Said {
    /// What has changed in the current file since the last commit.
    ///
    /// Kept here rather than in the buffer for the same reason the
    /// highlights are: it is a function of the file's text and nothing else,
    /// and re-deriving it when the text changes is simpler than keeping a
    /// buffer's copy of it right. Re-derived means asking git again, so it
    /// happens when a file is opened or reloaded -- which is exactly when
    /// what changed can have changed -- and not per frame.
    pub(in crate::app) changes: Option<Changed>,
    /// The committed text the margin's diff is against.
    ///
    /// One file's, because one file's diff is drawn: switching to another
    /// reads that one's. Kept because reading it is opening the repository,
    /// finding the commit, walking its tree and unpacking the blob -- and
    /// what it answers changes only when the repository moves, where the
    /// document it is compared with changes on every keystroke.
    ///
    /// `None` inside the answer is a file with nothing committed, which has
    /// to be remembered too: otherwise every keystroke goes and finds out
    /// again that there is nothing to find.
    pub(in crate::app) committed: Option<Committed>,
    /// Who last changed each line, per file that has been asked about.
    ///
    /// Kept rather than replaced, because a reader goes back and forth
    /// between two files and a blame is a walk of history: asking again for
    /// one they left a moment ago would spend that walk twice. Bounded by
    /// the files opened in a session, which is tens of them.
    pub(in crate::app) blames: std::collections::HashMap<
        (PathBuf, Option<gix::ObjectId>),
        Vec<Option<obelus_git::Blamed>>,
    >,
    /// Which files have been asked about and have not answered yet, so a
    /// frame does not start a second walk of the same history.
    pub(in crate::app) asking_blame: std::collections::HashSet<(PathBuf, Option<gix::ObjectId>)>,
    /// Which walk of the history the list is expecting batches from.
    ///
    /// Bumped every time a history starts being read -- a key, a tab, a
    /// different file -- so the batches of the walk before it are
    /// recognizable as stale. Shared with the walking thread, which reads it
    /// to find out that nobody is waiting for it any more: a whole history
    /// is a walk of the whole project, and there is nothing else to stop it
    /// with.
    pub(in crate::app) history_generation: obelus_runtime::cancel::Latest,
}
