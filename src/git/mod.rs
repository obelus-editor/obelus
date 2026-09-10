//! What has changed in a file since the last commit.
//!
//! Only that, so far: the working tree against `HEAD`, for one file at a
//! time, which is what a reader needs in the margin while reading it.
//!
//! The old text comes from `git` itself rather than from a library. Reading
//! git objects properly -- loose and packed, delta chains, alternates -- is
//! the hard part, and `git show HEAD:./file` is the program that already
//! does it; obelus spawns language servers, so spawning one more program is
//! not a new idea here. When the git *views* arrive -- history, blame, tree
//! diffs -- a library earns its weight and can take over behind
//! [`head_text`] without anything above noticing.

pub mod change;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
};

pub use change::{Changes, Hunk, Marker};

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

/// What git says about every file in the repository `root` is in.
///
/// One call rather than one per file: `git status` walks the tree once and
/// respects every ignore rule on the way, which is a great deal of work to
/// repeat for each of ten thousand rows.
///
/// Keyed by absolute path. Git reports paths relative to the repository
/// root, which is not necessarily the directory obelus was started in, and
/// a map keyed by one and read with the other silently matches nothing.
///
/// Empty for anything that is not a repository, which is the same thing it
/// means for a file: nothing to say.
#[must_use]
pub fn statuses(root: &Path) -> HashMap<PathBuf, FileStatus> {
    let Ok(output) = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("status")
        .arg("--porcelain")
        // Untracked files one by one rather than a directory standing for
        // all of them: the picker's rows are files.
        .arg("--untracked-files=all")
        // NUL-separated, because a path may contain anything a path may
        // contain, and the line-based form quotes and escapes those.
        .arg("-z")
        .output()
    else {
        return HashMap::new();
    };
    if !output.status.success() {
        return HashMap::new();
    }

    let Ok(text) = String::from_utf8(output.stdout) else {
        return HashMap::new();
    };
    let Some(top) = toplevel(root) else {
        return HashMap::new();
    };
    let mut statuses = HashMap::new();
    let mut records = text.split('\0');
    while let Some(record) = records.next() {
        // `XY path`, where X is the index and Y the working tree.
        if record.len() < 4 {
            continue;
        }
        let (marks, path) = record.split_at(3);
        let marks = marks.as_bytes();
        // A rename carries its old path in a second record, which is not a
        // row of its own.
        if marks[0] == b'R' || marks[1] == b'R' {
            records.next();
        }
        let status = if marks.starts_with(b"??") || marks[0] == b'A' {
            FileStatus::New
        } else {
            FileStatus::Changed
        };
        statuses.insert(top.join(path), status);
    }
    statuses
}

/// The root of the repository a directory is in.
fn toplevel(directory: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .arg("rev-parse")
        .arg("--show-toplevel")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?;
    Some(PathBuf::from(path.trim_end()))
}

/// The file as the last commit has it, or `None` if that question has no
/// answer.
///
/// No answer covers every way this can decline, and they all mean the same
/// thing to a reader: no markers. Not a repository, a file git has never
/// heard of, a repository with no commits yet, or no `git` on the path.
#[must_use]
pub fn head_text(path: &Path) -> Option<String> {
    let directory = path.parent()?;
    let name = path.file_name()?.to_str()?;

    // `HEAD:./name` from the file's own directory, so obelus does not have
    // to find the repository root and then work out a path relative to it --
    // two chances to disagree with git about which file is meant.
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .arg("show")
        .arg(format!("HEAD:./{name}"))
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    // A file git has but obelus cannot read as text is a file with no line
    // diff to show.
    String::from_utf8(output.stdout).ok()
}
