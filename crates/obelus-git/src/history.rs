//! What has happened to a file, and to the project it is in.
//!
//! A walk of the commits reachable from `HEAD`, newest first. Filtering by
//! path is done the way the question is asked -- "did this commit change
//! this file?" -- by comparing what the path pointed at in the commit with
//! what it pointed at in its first parent.
//!
//! Where that path was not in the parent at all, the file has either just
//! been written or just been moved here, and the walk asks which: a file
//! that was moved goes on being walked under the name it had before. Git
//! records no rename -- it infers one from what a commit added and removed,
//! which is what `git log --follow` is -- and the same inference has to be
//! made here, because the reader has it in their head already and a history
//! that stopped at the move would be answering about a *path* while looking
//! like a complete answer about the file.
//!
//! The same goes for what a commit opens into: without it a commit that
//! moved a file says the file was deleted and a stranger of the same
//! content arrived, which is two rows to pair up by eye and, where the
//! lines are counted, the whole file counted away and the whole of it
//! counted back. `looking_for_moves` and `looking_harder_for_moves` are the
//! two budgets that pays for, and why they differ.
//!
//! What the remote has not seen is marked, in the colour a new file wears.
//! The few commits a reader has not pushed are the ones still theirs to change,
//! and they are what someone scanning a history is usually looking for -- the
//! same argument the file list makes for colouring what git has not seen.
//! Nothing is marked where the question does not arise: a branch that tracks
//! nothing, or a repository with no remote at all, has every commit equally
//! unpushed, and marking all of them says no more than marking none. It is
//! walked from the tracking branch and stopped as soon as every commit on
//! screen is accounted for, so the ordinary case -- a remote at or near `HEAD`
//! -- costs about what the list itself did; a commit the walk did not reach
//! before its budget ran out is left alone, because telling a reader their work
//! is not on the remote when it is would be the worse lie.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use obelus_sink::Sink;

use crate::{Event, FileStatus};

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
    /// What the file was called in this commit, for a walk of one file.
    ///
    /// Not the name it has now: a walk goes on under the name a file had
    /// before it was moved, so every commit older than the move is about a
    /// path that no longer exists. Opening one means opening *that* path,
    /// and a row that opened the current name would open nothing at all.
    ///
    /// `None` for a walk of the whole project, which is about no path.
    pub at: Option<PathBuf>,
    /// What it was called before, where this is the commit that moved it.
    ///
    /// Set on the one commit rather than on every commit older than it: the
    /// move is a thing that happened once, and saying so on forty rows
    /// would be saying it about forty commits that did not do it.
    pub was: Option<PathBuf>,
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
/// about the whole project. `limit` is how many to collect.
///
/// The walk runs until it has them or the history runs out, and asking
/// about a path costs a tree lookup per commit -- so a rarely-changed file
/// is a walk of the whole project however few rows are wanted. That is what
/// [`spawn_log`] is for. This one answers the cheap questions: what is at
/// the top, does this project have a commit at all.
///
/// Empty for every ordinary way this has no answer: not a repository, no
/// commits yet, a path git has never heard of.
#[must_use]
pub fn of(within: &Path, only: Option<&Path>, limit: usize) -> Vec<Commit> {
    let mut found = Vec::new();
    walk(within, only, |commit| {
        if let Some(commit) = commit {
            found.push(commit);
        }
        found.len() < limit
    });
    found
}

/// Walks a whole history on its own thread, sending it in batches.
///
/// Unbounded, because every bound on it was a lie told to save time: a limit
/// on the rows is a limit on which commits can be searched, and a limit on
/// the walk hides the histories of exactly the files nobody has edited
/// lately. A whole walk of a forty-thousand-commit project asking about one
/// path takes about two seconds, which is nothing to wait through when the
/// rows arrive as they are found and everything else stays live.
///
/// `generation` comes back with every batch: a reader moves between tabs and
/// files faster than a history can be walked, and the answers to the
/// question before must be recognizable as stale. `current` is the
/// generation anybody is still waiting for, read as the walk goes, because
/// there is nothing else to stop a thread with -- it has to ask.
///
/// The last batch carries `done`, which is what turns "still reading" into
/// "nothing here": with nothing found, those are different facts.
pub fn spawn_log(
    within: &Path,
    only: Option<&Path>,
    wanted: obelus_runtime::cancel::Wanted,
    sender: impl Sink<Event>,
) {
    let within = within.to_path_buf();
    let only = only.map(Path::to_path_buf);
    obelus_runtime::handle().spawn_blocking(move || {
        let mut batch = Vec::new();
        let mut walked = 0usize;
        let mut sent = Instant::now();
        let mut stopped = false;

        walk(&within, only.as_deref(), |commit| {
            walked += 1;
            if let Some(commit) = commit {
                batch.push(commit);
            }
            // On a clock rather than on a count. A project's walk finds a
            // commit every step and a file's finds one every thousand,
            // and both want the same thing from the reader's side: rows
            // often enough to read, seldom enough not to redraw the
            // screen raw.
            if sent.elapsed() < TICK {
                return true;
            }
            sent = Instant::now();
            if !wanted.still() {
                stopped = true;
                return false;
            }
            let commits = std::mem::take(&mut batch);
            if sender
                .send(Event::Logged {
                    generation: wanted.generation(),
                    commits,
                    walked,
                    done: false,
                })
                .is_err()
            {
                stopped = true;
                return false;
            }
            true
        });

        // Nobody is waiting for the end of a walk they have already
        // moved off: saying it is done would be answering a question
        // that was withdrawn.
        if !stopped {
            let _ = sender.send(Event::Logged {
                generation: wanted.generation(),
                commits: batch,
                walked,
                done: true,
            });
        }
    });
}

/// How often a walk in progress hands over what it has found.
const TICK: Duration = Duration::from_millis(80);

/// What `HEAD` points at, or `None` where there is nothing to point at.
///
/// Which commit a list was read at, so that a repository moving underneath
/// it -- a commit in another window, an amend, a checkout -- can be told
/// from the other things that move git's state. `git add` writes the index
/// on every use and changes no history at all.
#[must_use]
pub fn head_of(within: &Path) -> Option<gix::ObjectId> {
    let repository = super::repository(within)?;
    repository.head_id().ok().map(gix::Id::detach)
}

/// One commit by its id, without a walk.
///
/// A commit that is being shown is one the reader already chose from a list,
/// so the question is not "which commits are there" but "what does this one
/// say" -- and an object store answers that directly. Walking to find it
/// again would cost the whole history for a commit far enough back, which is
/// exactly when a reader most wants to be told what it said.
#[must_use]
pub fn one(within: &Path, id: gix::ObjectId) -> Option<Commit> {
    commit_in(&super::repository(within)?, id)
}

/// One commit out of a repository that is already open.
///
/// The repository is the expensive part of asking -- discovering it and
/// opening it -- so anything asking about more than one commit holds it and
/// calls this.
fn commit_in(repository: &gix::Repository, id: gix::ObjectId) -> Option<Commit> {
    let commit = repository.find_commit(id).ok()?;
    let author = commit.author().ok()?;
    let message = commit.message_raw_sloppy().to_string();
    let (subject, body) = split(&message);
    Some(Commit {
        id,
        subject,
        body,
        who: author.name.to_string(),
        when: author.time().map(|time| time.seconds).unwrap_or_default(),
        // Asked for by its id and about no path: this is a commit somebody
        // named, not a step in a walk of one file.
        at: None,
        was: None,
    })
}

/// Walks the commits reachable from `HEAD`, newest first.
///
/// `each` is handed every commit the walk looks at: `Some` when it answers
/// the question `only` asked, `None` when it was looked at and passed over.
/// Both, rather than only the answers, because the two callers need what the
/// skipped ones cost -- one to stop after enough answers, the other to know
/// how far it has got and whether anybody is still waiting. It returns
/// whether to go on.
fn walk(within: &Path, only: Option<&Path>, mut each: impl FnMut(Option<Commit>) -> bool) {
    let Some(repository) = super::repository(within) else {
        return;
    };
    // A path that cannot be placed in the repository is not the whole
    // project: asking about one file and being handed every commit is the
    // wrong answer told confidently. Nothing says what is true -- obelus has
    // nothing to show about this path.
    let mut relative = match only {
        Some(path) => match within_repository(&repository, path) {
            Some(relative) => Some(relative),
            None => return,
        },
        None => None,
    };
    let Ok(head) = repository.head_id() else {
        return;
    };
    let Ok(commits) = repository.rev_walk([head]).all() else {
        return;
    };

    for info in commits.flatten() {
        let Ok(commit) = repository.find_commit(info.id) else {
            continue;
        };
        let mut moved_from = None;
        if let Some(path) = relative.as_deref() {
            match touches(&repository, &commit, path) {
                Touch::No => {
                    if !each(None) {
                        return;
                    }
                    continue;
                }
                Touch::Yes => {}
                // The commit where the file arrived under this name. It is
                // shown, and everything older is about the name it had
                // before -- which is what a reader asking for the history
                // of a file means by it.
                Touch::Moved(from) => moved_from = Some(from),
            }
        }
        let Ok(author) = commit.author() else {
            continue;
        };
        let message = commit.message_raw_sloppy().to_string();
        let (subject, body) = split(&message);
        let found = Commit {
            id: info.id,
            subject,
            body,
            who: author.name.to_string(),
            when: author.time().map(|time| time.seconds).unwrap_or_default(),
            at: relative.clone(),
            was: moved_from.clone(),
        };
        if !each(Some(found)) {
            return;
        }
        if let Some(from) = moved_from {
            relative = Some(from);
        }
    }
}

/// Where a path sits in its repository, for a path that may not be there.
///
/// A history is the one question that outlives the file: a reader can ask
/// about something a commit deleted, and there is a good answer. So the
/// resolved form is tried first -- it is the one that handles symlinks --
/// and for the rest the deepest part of the path that is still there is
/// resolved and what has gone is put back on the end.
///
/// Resolved, not merely made absolute: the working directory is resolved,
/// and a path that is not is spelled differently wherever a symlink sits
/// above the repository -- which on a mac is every temporary directory,
/// `/var` being `/private/var`. The strip found nothing, and every row of
/// a history older than a move opened nothing.
fn within_repository(repository: &gix::Repository, path: &Path) -> Option<PathBuf> {
    if let Some(relative) = super::in_repository(repository, path) {
        return Some(relative);
    }
    let work_dir = repository.workdir()?.canonicalize().ok()?;
    let path = std::path::absolute(path).ok()?;
    let (there, gone) = path.ancestors().find_map(|ancestor| {
        Some((
            ancestor.canonicalize().ok()?,
            path.strip_prefix(ancestor).ok()?,
        ))
    })?;
    Some(there.join(gone).strip_prefix(work_dir).ok()?.to_path_buf())
}

/// What a commit did to a path.
#[derive(Debug)]
enum Touch {
    /// Left it as its parent had it.
    No,
    /// Changed what it points at.
    Yes,
    /// Is where the file arrived under this name, having been this before.
    Moved(PathBuf),
}

/// What a commit did to a path.
///
/// Against its first parent only. A merge that took one side's version
/// changed nothing on that side, and a list that showed every merge a file
/// was carried through would be a list of merges.
///
/// A path that is in the commit and not in the parent has either just been
/// written or just been moved here, and those are the same thing to every
/// test but one: whether something of the same content went away in the
/// same commit. That is the only place this asks -- the search is not cheap
/// and a file that was merely edited never reaches it.
fn touches(repository: &gix::Repository, commit: &gix::Commit<'_>, path: &Path) -> Touch {
    let at = entry_of(repository, commit.id, path);
    let parent_id = commit.parent_ids().next().map(gix::Id::detach);
    let parent = parent_id.and_then(|parent| entry_of(repository, parent, path));
    match (at.is_some(), parent.is_some()) {
        _ if at == parent => Touch::No,
        (true, false) => {
            match parent_id.and_then(|parent| moved_to(repository, parent, commit, path)) {
                Some(from) => Touch::Moved(from),
                None => Touch::Yes,
            }
        }
        _ => Touch::Yes,
    }
}

/// What `path` was called before this commit, if this commit moved it here.
///
/// One tree diff, run only where a file appears under a name its parent did
/// not have. See `looking_for_moves` for what the search costs and where it
/// gives up: past that, a file that was edited on the way reads as having
/// been written here, which is what obelus said about every move before any
/// of this.
fn moved_to(
    repository: &gix::Repository,
    parent: gix::ObjectId,
    commit: &gix::Commit<'_>,
    path: &Path,
) -> Option<PathBuf> {
    moves_of(repository, parent, commit).get(path).cloned()
}

/// Every move one commit made, from the name after to the name before.
///
/// The whole map rather than the one entry asked for, because the search
/// works that way: it pairs every addition with every deletion at once, and
/// having paid for that there is no sense in throwing away all but one of
/// the answers.
fn moves_of(
    repository: &gix::Repository,
    parent: gix::ObjectId,
    commit: &gix::Commit<'_>,
) -> Moved {
    let remembered = MOVES.get_or_init(Default::default);
    if let Ok(known) = remembered.lock()
        && let Some(moves) = known.get(&commit.id)
    {
        return std::sync::Arc::clone(moves);
    }

    let mut moves = HashMap::new();
    if let Some(before) = repository
        .find_commit(parent)
        .ok()
        .and_then(|parent| parent.tree().ok())
        && let Ok(after) = commit.tree()
        && let Ok(mut changes) = before.changes()
    {
        changes.options(|options| {
            options.track_rewrites(Some(looking_harder_for_moves()));
        });
        let _ = changes.for_each_to_obtain_tree(&after, |change| {
            if let gix::object::tree::diff::Change::Rewrite {
                location,
                source_location,
                ..
            } = change
            {
                moves.insert(
                    PathBuf::from(location.to_string()),
                    PathBuf::from(source_location.to_string()),
                );
            }
            Ok::<_, std::convert::Infallible>(std::ops::ControlFlow::Continue(()))
        });
    }

    let moves = std::sync::Arc::new(moves);
    if let Ok(mut known) = remembered.lock() {
        known.insert(commit.id, std::sync::Arc::clone(&moves));
    }
    moves
}

/// What a path pointed at in a commit, if anything.
fn entry_of(repository: &gix::Repository, id: gix::ObjectId, path: &Path) -> Option<gix::ObjectId> {
    let commit = repository.find_commit(id).ok()?;
    let mut tree = commit.tree().ok()?;
    let entry = tree.peel_to_entry_by_path(path).ok()??;
    Some(entry.object_id())
}

/// How hard to look for a file that was moved rather than replaced.
///
/// Git does not record a rename; it infers one from what a commit added and
/// removed, and every reader of a history has git's inference in their head
/// already. Without it a commit that moved a file says the file was deleted
/// and a stranger of the same content was added -- two rows a reader has to
/// pair up by eye, and, where the lines are counted, the whole file counted
/// as gone and the whole of it counted as new.
///
/// Half is what `git diff -M50%` uses, which is the answer to compare
/// against.
///
/// The cap is gix's own, and it is a count of *pairings* -- additions times
/// deletions -- rather than of files, whatever the field's documentation
/// says. A thousand of them is about thirty files moved in one commit,
/// which covers moving a module and everything short of reorganising the
/// whole tree. Past it gix does not do a partial pass, it skips the search
/// for anything that was edited on the way; a commit that moved two hundred
/// files reads as two hundred deletions and two hundred additions, the way
/// it did before any of this.
///
/// That cliff is deliberate and the alternative was measured: the search is
/// a blob diff per pairing, so the two-hundred-file commit costs seven
/// seconds, and this runs on the keystroke that opens a commit. Files moved
/// without being edited are matched on content alone and are found whatever
/// the cap says, so the ordinary `git mv` never falls off it.
fn looking_for_moves() -> gix::diff::Rewrites {
    gix::diff::Rewrites {
        percentage: Some(0.5),
        ..Default::default()
    }
}

/// The same search, with the cap off.
///
/// For the walk of one file's history, which is the other place that asks.
/// Two budgets because the two are paid by different people: opening a
/// commit happens under a keystroke and has to be quick or not at all,
/// while a walk already runs in the background and already says how far it
/// has got. Three seconds of a list filling in is a list filling in; three
/// seconds of a key not answering is a broken program.
///
/// A million pairings is git's own `diff.renameLimit` of a thousand files
/// on each side, and the answer is memoised per commit, so the tree's one
/// giant reorganisation is searched once in a session however many files
/// are read through it.
fn looking_harder_for_moves() -> gix::diff::Rewrites {
    gix::diff::Rewrites {
        percentage: Some(0.5),
        limit: 1_000 * 1_000,
        ..Default::default()
    }
}

/// What each commit moved, worked out once.
///
/// Keyed by the commit, because the answer is about two trees git will
/// never change again: it cannot go stale, and nothing has to say when to
/// forget it. What it saves is the search itself -- a blob diff for every
/// addition against every deletion -- which for the commit that moved this
/// tree into crates is three seconds, and which every file read through
/// that commit would otherwise ask for again.
static MOVES: std::sync::OnceLock<std::sync::Mutex<HashMap<gix::ObjectId, Moved>>> =
    std::sync::OnceLock::new();

/// What one commit moved, from the name after to the name before.
///
/// Shared rather than copied out: the map for the commit that moved this
/// tree into crates has two hundred entries in it, and every file read
/// through that commit wants the same one.
type Moved = std::sync::Arc<HashMap<PathBuf, PathBuf>>;

/// The files a commit changed, against its first parent.
///
/// What a row of the project's history opens into: a commit is not a file,
/// so the thing under it is the list of files it touched. Against the first
/// parent only, for the reason the walk uses it -- a merge that took one
/// side's version changed nothing on that side.
#[must_use]
pub fn files_in(within: &Path, id: gix::ObjectId) -> Vec<Touched> {
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

    let mut changed: Vec<Touched> = Vec::new();
    match parent {
        Some(parent) => {
            if let Ok(mut changes) = parent.changes() {
                changes.options(|options| {
                    options.track_rewrites(Some(looking_for_moves()));
                });
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
                changed.push(Touched {
                    path: PathBuf::from(entry.inner.filename.to_string()),
                    status: FileStatus::New,
                    was: None,
                });
            }
        }
    }
    changed.sort_by(|left, right| left.path.cmp(&right.path));
    changed
}

/// One file a commit touched.
///
/// A struct and not a pair, because what a reader needs about a row is
/// three things now: where the file is, what happened to it, and -- where
/// the commit moved it -- what it used to be called.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Touched {
    /// Where the file is, as the commit left it.
    pub path: PathBuf,
    /// What happened to it.
    pub status: FileStatus,
    /// What it was called before, where this commit is the one that moved
    /// it. `None` for a file that stayed where it was.
    pub was: Option<PathBuf>,
}

/// One change from a tree diff, or `None` for a change to a directory.
///
/// A directory turns up in the walk because the walk goes through it: a
/// commit that changes `src/keymap.rs` changes `src` as well, and a list
/// with both in it is a list with a row nobody can open.
///
/// `New` for a file the commit added and `Changed` for everything else,
/// deletions included: those two are what the file list colours by, and a
/// file a commit removed is a change to it as far as a reader scanning the
/// list is concerned.
fn file_of(change: &gix::object::tree::diff::Change<'_, '_, '_>) -> Option<Touched> {
    use gix::object::tree::diff::Change;
    let (location, mode, status, was) = match change {
        Change::Addition {
            location,
            entry_mode,
            ..
        } => (location, entry_mode, FileStatus::New, None),
        Change::Deletion {
            location,
            entry_mode,
            ..
        }
        | Change::Modification {
            location,
            entry_mode,
            ..
        } => (location, entry_mode, FileStatus::Changed, None),
        // Moved, and mostly itself on the way. `Changed` rather than `New`
        // because that is what it is: the content came through, and the
        // name it came from is what the row says besides.
        Change::Rewrite {
            location,
            entry_mode,
            source_location,
            ..
        } => (
            location,
            entry_mode,
            FileStatus::Changed,
            Some(PathBuf::from(source_location.to_string())),
        ),
    };
    (!mode.is_tree()).then(|| Touched {
        path: PathBuf::from(location.to_string()),
        status,
        was,
    })
}

/// How many lines a commit added and took away, over everything it touched.
///
/// One walk of the tree diff, with the blobs taken from the walk itself. The
/// obvious way -- ask `files_in` what changed, then `text_before` and
/// `text_at` for each -- reopens the repository and walks the tree twice per
/// file, which for a commit touching a dozen of them is two dozen of the
/// most expensive thing here.
///
/// A commit with no parent counts as all additions: there was nothing there
/// before it, and that is what it did.
///
/// What is not text is not counted. A line is the unit, and a PNG has none;
/// counting its bytes as lines would put a number beside a commit that means
/// nothing.
#[must_use]
pub fn counted_in(within: &Path, id: gix::ObjectId) -> Option<(usize, usize)> {
    let repository = super::repository(within)?;
    let commit = repository.find_commit(id).ok()?;
    let tree = commit.tree().ok()?;
    let before = commit
        .parent_ids()
        .next()
        .and_then(|parent| repository.find_commit(parent.detach()).ok())
        .and_then(|parent| parent.tree().ok())
        .unwrap_or_else(|| repository.empty_tree());

    let mut added = 0usize;
    let mut removed = 0usize;
    let mut changes = before.changes().ok()?;
    changes.options(|options| {
        options.track_rewrites(Some(looking_for_moves()));
    });
    let _ = changes.for_each_to_obtain_tree(&tree, |change| {
        if let Some((was, now)) = texts_of(change) {
            let (up, down) = super::change::counted(&super::change::drawn(&was, &now));
            added += up;
            removed += down;
        }
        Ok::<_, std::convert::Infallible>(std::ops::ControlFlow::<()>::Continue(()))
    });
    Some((added, removed))
}

/// What a change had on either side of it, where both sides are text.
fn texts_of(change: gix::object::tree::diff::Change<'_, '_, '_>) -> Option<(String, String)> {
    use gix::object::tree::diff::Change;
    let text = |id: gix::Id<'_>| {
        id.object()
            .ok()
            .and_then(|object| String::from_utf8(object.data.clone()).ok())
    };
    let (was, now, mode) = match change {
        Change::Addition { id, entry_mode, .. } => (None, Some(id), entry_mode),
        Change::Deletion { id, entry_mode, .. } => (Some(id), None, entry_mode),
        Change::Modification {
            previous_id,
            id,
            entry_mode,
            ..
        } => (Some(previous_id), Some(id), entry_mode),
        Change::Rewrite {
            source_id,
            id,
            entry_mode,
            ..
        } => (Some(source_id), Some(id), entry_mode),
    };
    if mode.is_tree() {
        return None;
    }
    // Either side may be missing -- a file arriving has no before, one going
    // has no after -- but a side that is there and is not text means this is
    // not a thing with lines, and it is left out altogether.
    let side = |id: Option<gix::Id<'_>>| match id {
        None => Some(String::new()),
        Some(id) => text(id),
    };
    Some((side(was)?, side(now)?))
}

/// A file as a commit had it, or `None` where that question has no answer.
///
/// Not a repository, a commit that is not there, a path the commit does not
/// have, or content that is not text -- all of which mean the same thing to
/// a reader: there is nothing here to open.
#[must_use]
pub fn text_at(within: &Path, id: gix::ObjectId, path: &Path) -> Option<String> {
    let repository = super::repository(within)?;
    // Placed the way the walk places a path, which is the way that works for
    // a name the working tree no longer has: a file a commit deleted, or one
    // a later commit moved, and a history is the question that outlives both.
    let relative = within_repository(&repository, path)?;
    let commit = repository.find_commit(id).ok()?;
    let mut tree = commit.tree().ok()?;
    let entry = tree.peel_to_entry_by_path(relative).ok()??;
    let object = entry.object().ok()?;
    String::from_utf8(object.data.clone()).ok()
}

/// Which of these commits the remote already has.
///
/// `None` where the question does not arise: no remote, or a branch that
/// tracks nothing. Every commit is then equally unpushed, and marking all
/// of them says no more than marking none.
///
/// Walked from the tracking branch rather than compared commit by commit,
/// and stopped as soon as every commit asked about is accounted for --
/// which in the ordinary case, where the remote is at or near `HEAD`, is
/// after about as many commits as were asked about. A commit the walk did
/// not reach before its budget ran out is left alone rather than marked:
/// telling a reader their work is not on the remote when it is would be a
/// worse lie than saying nothing.
#[must_use]
pub fn pushed(within: &Path, asked: &[gix::ObjectId]) -> Option<HashSet<gix::ObjectId>> {
    let repository = super::repository(within)?;
    let head = repository.head_ref().ok()??;
    let name = head.name().to_owned();
    let tracking = repository
        .branch_remote_tracking_ref_name(name.as_ref(), gix::remote::Direction::Fetch)?
        .ok()?;
    let mut reference = repository.find_reference(tracking.as_ref()).ok()?;
    let id = reference.peel_to_id().ok()?;
    let walk = repository.rev_walk([id]).all().ok()?;

    let wanted: HashSet<gix::ObjectId> = asked.iter().copied().collect();
    let mut found = HashSet::new();
    let mut seen = 0;
    for info in walk.flatten() {
        seen += 1;
        if wanted.contains(&info.id) {
            found.insert(info.id);
            if found.len() == wanted.len() {
                break;
            }
        }
        if seen > asked.len().saturating_mul(BEHIND) {
            break;
        }
    }
    Some(found)
}

/// How far behind the remote may be before obelus stops looking.
///
/// A branch that is a few commits ahead is the ordinary case and stops at
/// once. One that is thousands behind is a reader who has not fetched in
/// months, and walking all of it to colour twenty rows is not worth a key
/// press.
const BEHIND: usize = 4;

/// A file as the commit *before* one had it.
///
/// What a version of a file is compared against, so its margin says what
/// that commit changed rather than how it differs from today. `None` where
/// there is no such answer -- the first commit of a project, a file that
/// commit added -- which is the same answer as an empty file: everything in
/// it is new.
#[must_use]
pub fn text_before(within: &Path, id: gix::ObjectId, path: &Path) -> Option<String> {
    let repository = super::repository(within)?;
    let commit = repository.find_commit(id).ok()?;
    let parent = commit.parent_ids().next()?;
    text_at(within, parent.detach(), path)
}

/// What kind of name points at a commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RefKind {
    /// A branch in this repository.
    Branch,
    /// A branch as a remote last had it.
    Remote,
    /// A tag.
    Tag,
}

/// A name that points at a commit.
#[derive(Clone, Debug)]
pub struct Reference {
    /// As a reader knows it: `master`, `origin/master`, `v1.0`.
    pub name: String,
    /// Which kind of name it is.
    pub kind: RefKind,
    /// The commit it points at, fully peeled -- an annotated tag names a tag
    /// object, and what a reader wants to see is the commit under it.
    pub at: Commit,
    /// Whether this is the one `HEAD` is on.
    pub head: bool,
}

/// Every name in the repository that points at a commit.
///
/// Branches first, then remote branches, then tags, and newest first within
/// each -- a reader has a handful of branches and may have a thousand tags,
/// and the handful is what they came for. No walk: each name is one commit
/// to decode.
///
/// `only` drops the names whose commit has no such file. Choosing one of
/// these opens the file as that name has it, so a name that has not got it
/// is a row nobody can open -- and on a tree that has just been
/// reorganised that is most of them. `None` asks for every name, which is
/// the question a list about no file is asking.
#[must_use]
pub fn refs_of(within: &Path, only: Option<&Path>) -> Vec<Reference> {
    let Some(repository) = super::repository(within) else {
        return Vec::new();
    };
    let on = repository
        .head_ref()
        .ok()
        .flatten()
        .map(|head| head.name().as_bstr().to_string());
    let Ok(platform) = repository.references() else {
        return Vec::new();
    };
    let Ok(all) = platform.all() else {
        return Vec::new();
    };
    // Placed once, outside the loop: it is the same answer for every name,
    // and it is the half of the question that touches the working tree.
    let relative = match only {
        Some(path) => match within_repository(&repository, path) {
            Some(relative) => Some(relative),
            None => return Vec::new(),
        },
        None => None,
    };

    let mut found = Vec::new();
    for reference in all.flatten() {
        let full = reference.name().as_bstr().to_string();
        let Some((kind, name)) = named(&full) else {
            continue;
        };
        let Ok(id) = reference.clone().into_fully_peeled_id() else {
            continue;
        };
        // Read straight from the repository already open. Asking for each
        // commit by path would discover and open the repository once per
        // name, which on a project with a thousand tags is the whole cost
        // of the list.
        if let Some(relative) = relative.as_deref()
            && entry_of(&repository, id.detach(), relative).is_none()
        {
            continue;
        }
        let Some(at) = commit_in(&repository, id.detach()) else {
            continue;
        };
        found.push(Reference {
            name,
            kind,
            at,
            head: on.as_deref() == Some(full.as_str()),
        });
    }
    // The kind first, because that is the order of how much a reader is
    // likely to want them; then newest first, which is the order every
    // other list of commits here is in.
    found.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then(right.at.when.cmp(&left.at.when))
            .then(left.name.cmp(&right.name))
    });
    found
}

/// The kind of a full ref name, and the short form a reader knows it by.
///
/// `None` for everything that is not a name for a commit -- `HEAD` itself,
/// notes, stash, whatever else a tool has left in there -- because a list
/// of places to read this file from is not a list of git's bookkeeping.
fn named(full: &str) -> Option<(RefKind, String)> {
    for (prefix, kind) in [
        ("refs/heads/", RefKind::Branch),
        ("refs/remotes/", RefKind::Remote),
        ("refs/tags/", RefKind::Tag),
    ] {
        if let Some(name) = full.strip_prefix(prefix) {
            // A remote's own `HEAD` is a pointer at one of its branches,
            // not a place of its own.
            if kind == RefKind::Remote && name.ends_with("/HEAD") {
                return None;
            }
            return Some((kind, name.to_string()));
        }
    }
    None
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
