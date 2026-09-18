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
//!
//! The diff base is the blob as a checkout would write it, and reading it must
//! not run anything. git stores `\n` and a project with `eol=crlf` checks the
//! file out as `\r\n`, so comparing the stored blob against the reader's buffer
//! marked every line of every file as changed. `git::head_text` runs the blob
//! through gix's worktree conversion, which fixes that -- and also executes any
//! `filter.*` driver the repository's own config names. `gix::discover` derives
//! trust from who owns `.git`, so a clone the reader happens to own is fully
//! trusted and the program runs: a code reader that executes a stranger's code
//! because it was pointed at their checkout is not a reader. So `head_text`
//! opens through `git::without_running_anything`, which forces
//! `Trust::Reduced`. `core.autocrlf` and `.gitattributes` both survive that
//! level and the drivers do not -- measured, both ways. It is not the level
//! everything opens at, because reduced trust also stops gix resolving a
//! remote, and that is how the history knows which commits are pushed: the
//! reduction goes where the risk is and nowhere else. `git::statuses` still
//! opens fully, and `core.fsmonitor` is the same kind of hole; nobody has
//! closed it.

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

/// The same, opened so that the repository cannot ask for a program to run.
///
/// Discovery derives trust from who owns `.git`, so a checkout the reader
/// happens to own is fully trusted -- and a fully trusted repository's own
/// config may name a program: `filter.*` drivers are run while a blob is
/// converted the way a checkout would convert it, which is a thing obelus
/// does to draw an honest margin. A code reader that executes a stranger's
/// code because it was pointed at their clone is not a reader.
///
/// Its own function rather than the level everything opens at, because
/// reduced trust costs something: gix will not resolve a remote whose url
/// comes from an untrusted config, and that is how the history knows which
/// commits have been pushed. So the reduction is applied where the risk is
/// -- the one place obelus runs anything -- and nowhere else.
///
/// What survives it is what the conversion actually needs: `core.autocrlf`
/// and `.gitattributes` are both read at this level. Measured, both of
/// them.
fn without_running_anything(path: &Path) -> Option<gix::Repository> {
    let from = if path.is_dir() { path } else { path.parent()? };
    let trust = gix::sec::Trust::Reduced;
    let found = gix::discover::upwards(from).ok()?.0;
    let (git_dir, _) = found.into_repository_and_work_tree_directories();
    let options = gix::open::Options::default()
        // Every config file obelus would have read anyway. The level is
        // about what a repository may *do*, and its own default turns these
        // off as well.
        .permissions(gix::open::Permissions {
            config: gix::open::permissions::Config {
                system: true,
                git: true,
                user: true,
                env: true,
                includes: true,
                // A lookup that costs a subprocess, and only tells gix where
                // a Windows git installation put its bundled config.
                git_binary: cfg!(windows),
            },
            ..<gix::open::Permissions as gix::sec::trust::DefaultForLevel>::default_for_level(trust)
        })
        // What discovery handed back is the `.git` directory itself, and
        // opening expects a worktree unless told otherwise.
        .open_path_as_is(true)
        .with(trust);
    gix::open_opts(git_dir, options).ok()
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
/// Only the states a reader cares about while choosing a file to read: one
/// they have changed, one that is not committed at all, and one the tree
/// has said it does not keep. Staged or not is a distinction for
/// committing, which obelus does not do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileStatus {
    /// Tracked, and different from the last commit.
    Changed,
    /// Not in the last commit at all.
    New,
    /// Kept out of the tree by `.gitignore` and friends.
    ///
    /// Never from [`statuses`], which does not ask about them -- `git
    /// status` leaves them out and so does obelus. It comes from the file
    /// walk, which is the only thing that goes looking for them, and only
    /// when the reader has asked to be offered them.
    Ignored,
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
    // Every untracked file, rather than the directory holding them.
    //
    // git's own default is to collapse a directory nothing in it is
    // tracked into one line -- `dir/` -- which is the right answer for a
    // terminal reporting to a person and the wrong one here: what this
    // feeds is a list of files to *open*, and a reader who picks the
    // folder gets nothing, because a folder is not a file. A new module is
    // exactly the case it goes wrong on: the one directory whose contents
    // a reader most wants listed.
    let platform = platform.untracked_files(gix::status::UntrackedFiles::Files);
    let Ok(iterator) = platform.into_iter(None) else {
        return statuses;
    };

    for item in iterator.filter_map(Result::ok) {
        use gix::status::{Item, index_worktree};
        let (path, status) = match item {
            // Tracked and different from the index.
            Item::IndexWorktree(index_worktree::Item::Modification {
                entry, rela_path, ..
            }) => {
                // Except another repository. git records a submodule as a
                // commit at a path, and reports the path as changed the
                // moment that commit moves -- so `vendor` arrives here
                // looking exactly like a file, and it is a directory. The
                // walk one arm down has refused those since it was
                // written; this is the same rule, for the entries git
                // tracks rather than the ones it has never seen.
                //
                // Asked of git's own mode rather than of the disk: a
                // submodule whose directory is missing is still a
                // submodule, and `is_dir` would call it a file.
                if entry.mode.is_submodule() {
                    continue;
                }
                (rela_path, FileStatus::Changed)
            }
            // Found by the directory walk, which is how a file git has never
            // seen arrives.
            Item::IndexWorktree(index_worktree::Item::DirectoryContents { entry, .. }) => {
                // Only what can be opened. A walk that emits every file
                // still reports a directory of its own for an empty one,
                // and a repository nested in the tree arrives as one
                // entry -- neither is a file, and a row naming one is a
                // row that does nothing.
                if !matches!(entry.disk_kind, Some(gix::dir::entry::Kind::File)) {
                    continue;
                }
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
                // The same, for a submodule whose new commit has been
                // staged.
                if change.entry_mode().is_submodule() {
                    continue;
                }
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
/// One repository and one head tree, against [`head_text`], which opens the
/// repository and resolves the head tree for every file it is asked about.
/// That is most of the cost, and a list of changed files asks about all of
/// them at once.
///
/// The tree itself is copied per path, which is not a saving thrown away:
/// walking a tree to a path *moves* it -- gix leaves it on the subtree it
/// descended into -- so one tree asked twice answers the second question
/// from wherever the first left it. Every file below the first one then
/// looked like a file the commit does not have, which reads as a file
/// where every line was just added. The copy is the root tree's own bytes
/// and nothing else; what was expensive is still done once.
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
    let Ok(head) = repository.head_tree() else {
        return counts;
    };
    for path in paths {
        let Some(relative) = in_repository(&repository, path) else {
            continue;
        };
        let mut tree = head.clone();
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
    let repository = without_running_anything(path)?;
    let relative = in_repository(&repository, path)?;
    let mut tree = repository.head_tree().ok()?;
    let entry = tree.peel_to_entry_by_path(&relative).ok()??;
    let object = entry.object().ok()?;
    Some(as_checked_out(&repository, &object.data, &relative))
}

/// A stored blob as it would be on disk.
///
/// The one place this matters is here, and it is why: everywhere else that
/// obelus reads an old version of a file, it compares it against *another*
/// stored version, and two blobs converted the same way or not at all give
/// the same answer. The margin compares a stored version against the
/// reader's own buffer, which came off the disk -- so a project whose
/// `.gitattributes` says `eol=crlf` had every line of every file marked as
/// changed, because git stores `\n` and the file on disk holds `\r\n`.
///
/// The bytes unchanged where there is nothing to convert or the conversion
/// fails, which is what the diff had before and is never worse than it.
fn as_checked_out(repository: &gix::Repository, data: &[u8], relative: &Path) -> String {
    let text = || String::from_utf8_lossy(data).into_owned();
    // The name the attributes are matched against, which is a repository
    // path with forward slashes whatever this platform writes.
    let name: gix::bstr::BString =
        gix::path::to_unix_separators_on_windows(gix::path::into_bstr(relative)).into_owned();
    let Ok((mut pipeline, _index)) = repository.filter_pipeline(None) else {
        return text();
    };
    let Ok(converted) = pipeline.convert_to_worktree(data, name.as_ref(), Default::default())
    else {
        return text();
    };
    let mut out = Vec::with_capacity(data.len());
    match std::io::Read::read_to_end(&mut { converted }, &mut out) {
        Ok(_) => String::from_utf8_lossy(&out).into_owned(),
        Err(_) => text(),
    }
}
