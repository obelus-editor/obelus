//! The project as a tree, a level at a time.
//!
//! A list Obelus offers is a list of the reader's own project. A language
//! server answers `workspace/symbol` with everything it has indexed, which for
//! rust-analyzer is every dependency of the project: a search for `new` in a
//! repository of a dozen files comes back with hundreds of rows from the
//! registry, and the one the reader meant is somewhere among them. So
//! `outline::found_in` takes the root and drops everything outside it -- an
//! argument rather than a filter at the call site, because a rule a caller can
//! forget is a rule that comes back. The file list has always worked this way,
//! and it is why a path can be shown relative to the root at all. Going *to* a
//! definition in a dependency is a different thing and still goes there: that
//! is a jump the reader asked for by name, not a list to choose from.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

/// One thing directly inside a directory, as a row of a tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Where it is, relative to the project's root -- which is what the list
    /// shows and what a reader types to reach it.
    pub path: PathBuf,
    /// Whether it is a directory.
    pub directory: bool,
    /// Whether opening it would put anything on screen.
    ///
    /// Which is the whole of what the mark on a row may claim. A directory
    /// with nothing in it, or with nothing in it but files a project was told
    /// to ignore, offers to open and then does not -- and a mark that does
    /// that once is a mark nobody presses again.
    ///
    /// One level deep, and no further. "Is there a file anywhere under
    /// this" is a walk of the whole project per row, and what this says is
    /// exactly what it means: opening it shows at least one row.
    pub holds: bool,
    /// Whether the project said to ignore it.
    ///
    /// Only ever true where the reader asked to see those as well, and
    /// what it is for is saying which they are: a list that offers them
    /// without saying which is a list that lies about the project.
    pub ignored: bool,
}

/// What is directly inside a directory, in the order it is drawn.
///
/// Two levels of walking for one level of rows: the second level is what
/// says which of the directories found have anything in them. One walk
/// rather than a listing per directory found, which on a directory of
/// thirty is thirty system calls for thirty arrows.
///
/// `ignored` offers the files the project has said to ignore as well and
/// `hidden` the ones a system keeps out of sight -- the same two switches
/// the flat listing reads, so what counts as a file worth showing has one
/// answer at both depths and in both shapes of the list.
#[must_use]
pub fn inside(root: &Path, directory: &Path, ignored: bool, hidden: bool) -> Vec<Entry> {
    let mut found = looking(root, directory, true, hidden);
    if !ignored {
        return found;
    }
    // Everything, and then which of it the rules would have kept out: the
    // walk that does not obey them cannot say which those are, and a row
    // offered without saying it is ignored is a row that lies about the
    // tree.
    let offered: HashSet<PathBuf> = found.iter().map(|entry| entry.path.clone()).collect();
    for entry in looking(root, directory, false, hidden) {
        if !offered.contains(&entry.path) {
            found.push(Entry {
                ignored: true,
                ..entry
            });
        }
    }
    found.sort_by(|left, right| {
        right
            .directory
            .cmp(&left.directory)
            .then_with(|| left.path.cmp(&right.path))
    });
    found
}

/// One walk of one level, obeying the rules or not.
///
/// `hidden` is the reader's switch and not this walk's own, so both walks
/// obey it: "ignored" then keeps meaning exactly "the walk that obeys the
/// rules did not offer it" whichever way the other one is set.
fn looking(root: &Path, directory: &Path, obeying: bool, hidden: bool) -> Vec<Entry> {
    let mut walk = crate::walker(directory, obeying, hidden);
    walk.max_depth(Some(2));

    let mut found: Vec<Entry> = Vec::new();
    let mut holding: HashSet<PathBuf> = HashSet::new();
    for entry in walk.build() {
        let entry = match entry {
            Ok(entry) => entry,
            // An unreadable directory is not worth abandoning the rest over.
            Err(error) => {
                tracing::debug!(%error, "skipping an entry");
                continue;
            }
        };
        match entry.depth() {
            // The directory being listed, which is not a row of itself.
            0 => continue,
            1 => found.push(Entry {
                path: entry
                    .path()
                    .strip_prefix(root)
                    .unwrap_or_else(|_| entry.path())
                    .to_path_buf(),
                directory: entry.file_type().is_some_and(|kind| kind.is_dir()),
                holds: false,
                ignored: false,
            }),
            // Only what it says about its parent.
            _ => {
                if let Some(parent) = entry.path().parent() {
                    holding.insert(parent.to_path_buf());
                }
            }
        }
    }

    for entry in &mut found {
        entry.holds = holding.contains(&root.join(&entry.path));
    }
    // Directories first and then by name, which is how a tree is read
    // everywhere: the shape of the thing before the leaves of it.
    found.sort_by(|left, right| {
        right
            .directory
            .cmp(&left.directory)
            .then_with(|| left.path.cmp(&right.path))
    });
    found
}
