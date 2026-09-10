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

use std::{path::Path, process::Command};

pub use change::{Changes, Hunk, Marker};

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
