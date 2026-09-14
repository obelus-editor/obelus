//! What has happened to a file, and to the project it is in.
//!
//! A walk of the commits reachable from `HEAD`, newest first. Filtering by
//! path is done the way the question is asked -- "did this commit change
//! this file?" -- by comparing what the path pointed at in the commit with
//! what it pointed at in its first parent. Renames are not followed: git
//! does not record them, it infers them from what a commit added and
//! removed, and inferring is a different answer to a different question.
//! The list says where it stopped rather than pretending it is the whole
//! of the history.

use std::path::{Path, PathBuf};

use crate::git::FileStatus;

/// One commit, as a row of a list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    /// Its object id, for asking about it again.
    pub id: gix::ObjectId,
    /// The first line of its message, which is what a row shows.
    pub subject: String,
    /// The rest of it, or empty where there is no more.
    pub body: String,
    /// Who wrote it, as they wrote it.
    pub who: String,
    /// When, in seconds since the epoch.
    pub when: i64,
}

impl Commit {
    /// The short form of its id, which is what a reader recognises it by.
    #[must_use]
    pub fn short(&self) -> String {
        self.id.to_string().chars().take(7).collect()
    }
}

/// The most recent commits, newest first.
///
/// `only` narrows the walk to commits that changed one path; `None` asks
/// about the whole project. `limit` is how many to collect -- a history is
/// as long as the project and a reader is looking at one screen of it.
///
/// Empty for every ordinary way this has no answer: not a repository, no
/// commits yet, a path git has never heard of.
#[must_use]
pub fn of(within: &Path, only: Option<&Path>, limit: usize) -> Vec<Commit> {
    let Some(repository) = super::repository(within) else {
        return Vec::new();
    };
    // A path that cannot be placed in the repository is not the whole
    // project: asking about one file and being handed every commit is the
    // wrong answer told confidently. Empty says what is true -- obelus has
    // nothing to show about this path.
    let relative = match only {
        Some(path) => match within_repository(&repository, path) {
            Some(relative) => Some(relative),
            None => return Vec::new(),
        },
        None => None,
    };
    let Ok(head) = repository.head_id() else {
        return Vec::new();
    };
    let Ok(walk) = repository.rev_walk([head]).all() else {
        return Vec::new();
    };

    let mut found = Vec::new();
    // A bound on the walk as well as on the answer: a file touched once, a
    // thousand commits ago, must not cost a thousand tree lookups before
    // the list can be drawn.
    let mut seen = 0;
    for info in walk.flatten() {
        seen += 1;
        if found.len() >= limit || seen > limit.saturating_mul(WALK) {
            break;
        }
        let Ok(commit) = repository.find_commit(info.id) else {
            continue;
        };
        if let Some(relative) = relative.as_deref()
            && !touches(&repository, &commit, relative)
        {
            continue;
        }
        let Ok(author) = commit.author() else {
            continue;
        };
        let message = commit.message_raw_sloppy().to_string();
        let (subject, body) = split(&message);
        found.push(Commit {
            id: info.id,
            subject,
            body,
            who: author.name.to_string(),
            when: author.time().map(|time| time.seconds).unwrap_or_default(),
        });
    }
    found
}

/// Where a path sits in its repository, for a path that may not be there.
///
/// A history is the one question that outlives the file: a reader can ask
/// about something a commit deleted, and there is a good answer. So the
/// resolved form is tried first -- it is the one that handles symlinks --
/// and a plain strip of the working directory answers for the rest.
fn within_repository(repository: &gix::Repository, path: &Path) -> Option<PathBuf> {
    if let Some(relative) = super::in_repository(repository, path) {
        return Some(relative);
    }
    let work_dir = repository.workdir()?.canonicalize().ok()?;
    let path = std::path::absolute(path).ok()?;
    Some(path.strip_prefix(work_dir).ok()?.to_path_buf())
}

/// How many commits may be looked at for each one the list keeps.
///
/// A file that changes rarely is the case this is for: without it, asking
/// for twenty rows about a file nobody has touched since the start would
/// walk the whole project.
const WALK: usize = 50;

/// Whether a commit changed what a path points at.
///
/// Against its first parent only. A merge that took one side's version
/// changed nothing on that side, and a list that showed every merge a file
/// was carried through would be a list of merges.
fn touches(repository: &gix::Repository, commit: &gix::Commit<'_>, path: &Path) -> bool {
    let at = entry_of(repository, commit.id, path);
    let parent = commit
        .parent_ids()
        .next()
        .and_then(|parent| entry_of(repository, parent.detach(), path));
    at != parent
}

/// What a path pointed at in a commit, if anything.
fn entry_of(repository: &gix::Repository, id: gix::ObjectId, path: &Path) -> Option<gix::ObjectId> {
    let commit = repository.find_commit(id).ok()?;
    let mut tree = commit.tree().ok()?;
    let entry = tree.peel_to_entry_by_path(path).ok()??;
    Some(entry.object_id())
}

/// The files a commit changed, against its first parent.
///
/// What a row of the project's history opens into: a commit is not a file,
/// so the thing under it is the list of files it touched. Against the first
/// parent only, for the reason the walk uses it -- a merge that took one
/// side's version changed nothing on that side.
#[must_use]
pub fn files_in(within: &Path, id: gix::ObjectId) -> Vec<(PathBuf, FileStatus)> {
    let Some(repository) = super::repository(within) else {
        return Vec::new();
    };
    let Ok(commit) = repository.find_commit(id) else {
        return Vec::new();
    };
    let Ok(tree) = commit.tree() else {
        return Vec::new();
    };
    let parent = commit
        .parent_ids()
        .next()
        .and_then(|parent| repository.find_commit(parent.detach()).ok())
        .and_then(|parent| parent.tree().ok());

    let mut changed: Vec<(PathBuf, FileStatus)> = Vec::new();
    match parent {
        Some(parent) => {
            if let Ok(mut changes) = parent.changes() {
                let _ = changes.for_each_to_obtain_tree(&tree, |change| {
                    if let Some(file) = file_of(&change) {
                        changed.push(file);
                    }
                    // Continue is what walks *into* a changed directory, so
                    // the list is of files however deep they are.
                    Ok::<_, std::convert::Infallible>(std::ops::ControlFlow::Continue(()))
                });
            }
        }
        // The first commit of a project, where everything in it is new.
        None => {
            for entry in tree.iter().flatten() {
                changed.push((
                    PathBuf::from(entry.inner.filename.to_string()),
                    FileStatus::New,
                ));
            }
        }
    }
    changed.sort_by(|left, right| left.0.cmp(&right.0));
    changed
}

/// One change from a tree diff, as a path and what happened to it, or
/// `None` for a change to a directory.
///
/// A directory turns up in the walk because the walk goes through it: a
/// commit that changes `src/keymap.rs` changes `src` as well, and a list
/// with both in it is a list with a row nobody can open.
///
/// `New` for a file the commit added and `Changed` for everything else,
/// deletions included: those two are what the file list colours by, and a
/// file a commit removed is a change to it as far as a reader scanning the
/// list is concerned.
fn file_of(change: &gix::object::tree::diff::Change<'_, '_, '_>) -> Option<(PathBuf, FileStatus)> {
    use gix::object::tree::diff::Change;
    let (location, mode, status) = match change {
        Change::Addition {
            location,
            entry_mode,
            ..
        } => (location, entry_mode, FileStatus::New),
        Change::Deletion {
            location,
            entry_mode,
            ..
        }
        | Change::Modification {
            location,
            entry_mode,
            ..
        }
        | Change::Rewrite {
            location,
            entry_mode,
            ..
        } => (location, entry_mode, FileStatus::Changed),
    };
    (!mode.is_tree()).then(|| (PathBuf::from(location.to_string()), status))
}

/// A file as a commit had it, or `None` where that question has no answer.
///
/// Not a repository, a commit that is not there, a path the commit does not
/// have, or content that is not text -- all of which mean the same thing to
/// a reader: there is nothing here to open.
#[must_use]
pub fn text_at(within: &Path, id: gix::ObjectId, path: &Path) -> Option<String> {
    let repository = super::repository(within)?;
    let relative = super::in_repository(&repository, path)?;
    let commit = repository.find_commit(id).ok()?;
    let mut tree = commit.tree().ok()?;
    let entry = tree.peel_to_entry_by_path(relative).ok()??;
    let object = entry.object().ok()?;
    String::from_utf8(object.data.clone()).ok()
}

/// The first line of a message, and the rest of it.
///
/// The subject is what a row shows and the body is what the reader opens to
/// read, so they are told apart once, here, rather than by every caller.
fn split(message: &str) -> (String, String) {
    let mut lines = message.lines();
    let subject = lines.next().unwrap_or_default().trim().to_string();
    let body = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    (subject, body)
}
