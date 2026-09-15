//! What has changed in a file since the last commit, and who changed it.
//!
//! Through `gix` rather than by running `git`: obelus reads a repository
//! while the reader is reading a file in it, and shelling out means a
//! process per question, output parsed back out of a format meant for
//! people, and a program that has to be installed for the editor to be able
//! to see. Blame in particular is not something to parse: it is a walk of
//! history, and a library that walks it hands back commit ids rather than
//! columns of text.
//!
//! Nothing here fails loudly. Every answer is an `Option` or an empty
//! collection, because every one of them is missing for ordinary reasons: a
//! file outside a repository, a repository with no commits yet, a file git
//! has never seen.

pub mod blame;
pub mod change;
pub mod history;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

pub use blame::Blamed;
pub use change::{Changes, Hunk, Marker};

/// Where a file sits inside its repository, which is how git addresses it.
///
/// Both sides are resolved before they are compared. A repository
/// discovered from a relative path reports a relative working directory, and
/// stripping an absolute path with it finds nothing -- which looks exactly
/// like a file outside the repository it is plainly in. Started as
/// `ob src/main.rs`, that was every file.
fn in_repository(repository: &gix::Repository, path: &Path) -> Option<PathBuf> {
    let work_dir = repository.workdir()?;
    let work_dir = work_dir.canonicalize().ok()?;
    let file = path.canonicalize().ok()?;
    Some(file.strip_prefix(work_dir).ok()?.to_path_buf())
}

/// The repository a path is in, if it is in one.
///
/// Discovered from the path rather than from the working directory: the file
/// being read is the thing the question is about, and it can be outside the
/// tree obelus was started in.
fn repository(path: &Path) -> Option<gix::Repository> {
    let from = if path.is_dir() { path } else { path.parent()? };
    // Ceiling directories are left alone deliberately: a reader who opens a
    // file three levels above the working directory still wants to know
    // what git says about it.
    gix::discover(from).ok()
}

/// The files a repository changes when its *state* changes, for whoever is
/// watching.
///
/// `HEAD` moves on a commit, a checkout or a rebase; `index` on a stage or
/// an unstage. Between them they cover every way the answer to "what has
/// changed in this file" can change without the file itself being touched
/// -- which is to say, every way another process can move the ground under
/// a reader. Nothing else in `.git` is worth watching: the object files
/// churn constantly and say nothing a margin cares about.
#[must_use]
pub fn state_of(path: &Path) -> Vec<PathBuf> {
    let Some(repository) = repository(path) else {
        return Vec::new();
    };
    let directory = repository.path();
    ["HEAD", "index"]
        .iter()
        .map(|name| directory.join(name))
        .collect()
}

/// Whether a path is one of the files that say a repository has moved.
///
/// Asked rather than remembered: it is asked only when one of those two
/// names arrives, which is rare, and a list kept up to date would have to
/// be kept up to date -- through a checkout that replaces the directory,
/// through a reader opening a file in another repository entirely.
///
/// The repository is discovered from the path itself for the same reason
/// everything else here is: the file the question is about can be outside
/// the tree obelus was started in.
#[must_use]
pub fn state_moved(path: &Path) -> bool {
    if !matches!(
        path.file_name().and_then(std::ffi::OsStr::to_str),
        Some("HEAD" | "index")
    ) {
        return false;
    }
    let Some(directory) = path.parent() else {
        return false;
    };
    repository(path).is_some_and(|repository| repository.path() == directory)
}

/// What git says about a file in the working tree.
///
/// Only the two states a reader cares about while choosing a file to read:
/// one they have changed, and one that is not committed at all. Staged or
/// not is a distinction for committing, which obelus does not do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileStatus {
    /// Tracked, and different from the last commit.
    Changed,
    /// Not in the last commit at all.
    New,
}

/// How long ago something happened, in the fewest words that are true.
///
/// One unit, always the largest that gives a number of at least one: "3
/// days" rather than "3 days 4 hours", because this sits at the end of a
/// line of code and its job is to be readable at a glance rather than
/// precise. Rounded down, the way people say it.
#[must_use]
pub fn how_long_ago(when: i64, now: std::time::SystemTime) -> String {
    let now = now
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64);
    let seconds = now.saturating_sub(when);
    // A commit from the future is a clock that disagrees, not a fact about
    // the file. "Just now" is the least wrong thing to say about it.
    if seconds < 60 {
        return "just now".to_string();
    }
    for (unit, name) in [
        (60 * 60 * 24 * 365, "year"),
        (60 * 60 * 24 * 30, "month"),
        (60 * 60 * 24 * 7, "week"),
        (60 * 60 * 24, "day"),
        (60 * 60, "hour"),
        (60, "minute"),
    ] {
        let count = seconds / unit;
        if count >= 1 {
            let plural = if count == 1 { "" } else { "s" };
            return format!("{count} {name}{plural} ago");
        }
    }
    "just now".to_string()
}

/// Whether git says anything in the tree has changed.
///
/// A yes or a no, and it stops at the first answer.
#[must_use]
pub fn anything_changed(root: &Path) -> bool {
    let Some(repository) = repository(root) else {
        return false;
    };
    let Ok(platform) = repository.status(gix::progress::Discard) else {
        return false;
    };
    let Ok(iterator) = platform.into_iter(None) else {
        return false;
    };
    // The first one is the whole answer, and stopping there is the
    // difference between a walk of the tree and a glance at it. Asking
    // `statuses` and looking at its length builds a map of every changed
    // path to find out whether there is one, which is the same trade the
    // history makes when it asks for a single commit to find out whether
    // there is a history at all.
    iterator.filter_map(Result::ok).next().is_some()
}

/// Everything git says has changed in a tree, by path.
pub fn statuses(root: &Path) -> HashMap<PathBuf, FileStatus> {
    let mut statuses = HashMap::new();
    let Some(repository) = repository(root) else {
        return statuses;
    };
    let Some(work_dir) = repository.workdir().map(Path::to_path_buf) else {
        return statuses;
    };
    let Ok(platform) = repository.status(gix::progress::Discard) else {
        return statuses;
    };
    let Ok(iterator) = platform.into_iter(None) else {
        return statuses;
    };

    for item in iterator.filter_map(Result::ok) {
        use gix::status::{Item, index_worktree};
        let (path, status) = match item {
            // Tracked and different from the index.
            Item::IndexWorktree(index_worktree::Item::Modification { rela_path, .. }) => {
                (rela_path, FileStatus::Changed)
            }
            // Found by the directory walk, which is how a file git has never
            // seen arrives.
            Item::IndexWorktree(index_worktree::Item::DirectoryContents { entry, .. }) => {
                (entry.rela_path, FileStatus::New)
            }
            // A rename is a deletion and an addition to git; to a reader
            // looking for something to read, the file that is *there* is a
            // file they have changed.
            Item::IndexWorktree(index_worktree::Item::Rewrite { dirwalk_entry, .. }) => {
                (dirwalk_entry.rela_path, FileStatus::Changed)
            }
            // Staged: the index differs from `HEAD`. An addition is a file
            // that is not in the last commit at all, which is what `New`
            // means; everything else is a change to a file that is.
            Item::TreeIndex(change) => {
                let new = matches!(change, gix::diff::index::Change::Addition { .. });
                let path = change.location().to_owned();
                (
                    path,
                    if new {
                        FileStatus::New
                    } else {
                        FileStatus::Changed
                    },
                )
            }
        };
        let Ok(path) = gix::path::try_from_bstring(path) else {
            continue;
        };
        // The first answer wins: a file can be reported twice -- staged and
        // then modified again -- and "new" is the more surprising of the two
        // to lose.
        statuses.entry(work_dir.join(path)).or_insert(status);
    }
    statuses
}

/// How much each of these files has changed since the last commit.
///
/// One repository, one head tree, and a peel per path -- against
/// [`head_text`], which opens the repository and resolves the head tree for
/// every file it is asked about. That is most of the cost, and a list of
/// changed files asks about all of them at once.
///
/// A path with no answer is left out rather than counted as nothing: a file
/// git has never heard of, one whose committed content is not text, one that
/// cannot be read off disk now. Nothing beside the name is the honest mark
/// for a file this cannot speak about; `+0 \u{2212}0` would be a claim.
///
/// A file that is gone from disk counts as all removed, which is what
/// deleting it did.
#[must_use]
pub fn counted_against_head(paths: &[PathBuf]) -> HashMap<PathBuf, (usize, usize)> {
    let mut counts = HashMap::new();
    let Some(first) = paths.first() else {
        return counts;
    };
    let Some(repository) = repository(first) else {
        return counts;
    };
    let Ok(mut tree) = repository.head_tree() else {
        return counts;
    };
    for path in paths {
        let Some(relative) = in_repository(&repository, path) else {
            continue;
        };
        let committed = match tree.peel_to_entry_by_path(&relative) {
            Ok(Some(entry)) => match entry.object() {
                Ok(object) => match String::from_utf8(object.data.clone()) {
                    Ok(text) => text,
                    // Not text, so it has no lines to count.
                    Err(_) => continue,
                },
                Err(_) => continue,
            },
            // The commit does not have it: everything in it arrived.
            Ok(None) => String::new(),
            Err(_) => continue,
        };
        // A file that is gone reads as empty, which makes its lines removed.
        let now = std::fs::read_to_string(path).unwrap_or_default();
        counts.insert(
            path.clone(),
            change::counted(&change::drawn(&committed, &now)),
        );
    }
    counts
}

/// The file as the last commit has it, or `None` if that question has no
/// answer.
///
/// No answer covers every way this can decline, and they all mean the same
/// thing to a reader: no markers. Not a repository, a file git has never
/// heard of, a repository with no commits yet, or a file whose committed
/// content is not text.
#[must_use]
pub fn head_text(path: &Path) -> Option<String> {
    let repository = repository(path)?;
    let relative = in_repository(&repository, path)?;
    let mut tree = repository.head_tree().ok()?;
    let entry = tree.peel_to_entry_by_path(relative).ok()??;
    let object = entry.object().ok()?;
    String::from_utf8(object.data.clone()).ok()
}
