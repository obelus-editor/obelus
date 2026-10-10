//! What has changed in a file since the last commit, and who changed it.
//!
//! Through `gix` rather than by running `git`: Obelus reads a repository
//! while the reader is reading a file in it, and shelling out means a
//! process per question, output parsed back out of a format meant for
//! people, and a program that has to be installed for the editor to be able
//! to see. Blame in particular is not something to parse: it is a walk of
//! history, and a library that walks it hands back commit ids rather than
//! columns of text.
//!
//! **Nothing here writes.** Committing, pushing, pulling and fetching were
//! investigated and turned down, and the investigation is kept here because
//! it is expensive to redo. gix has no push at all: `gix-transport` knows the
//! name `git-receive-pack` and nothing in `gix` ever asks for it. It *can*
//! commit -- `edit_tree` builds the tree, `repo.commit()` writes the object
//! and moves the ref -- and doing so leaves the index untouched, which is not
//! a cosmetic problem: after a commit made that way `git status` reports the
//! newly committed file as deleted, and the reader's next ordinary `git
//! commit -a` removes it from history. Two lines (`index_from_tree` then
//! `index.write`) fix that, and four more things stay broken: a `pre-commit`
//! hook that exits 1 does not stop it, `commit-msg` never runs,
//! `commit.gpgsign` is ignored, and `.gitattributes` filters are not
//! applied. All measured against a real repository, not read off the
//! documentation.
//!
//! Which is why nobody does it. zed writes -- and has no git library at all:
//! 21 subcommands shelled out, blame and diff included, plus its own
//! `GIT_ASKPASS` script talking back over a socket. helix reads -- and uses
//! gix, with no network feature and no git commands whatsoever. There is no
//! third combination. The choice is not which library; it is whether to
//! write at all, and Obelus does not.
//!
//! If that is ever revisited: shell out for all four verbs, because one of
//! them (push) has no other option and two mechanisms for one act is worse
//! than one. `GIT_TERMINAL_PROMPT=0` fails cleanly without touching the
//! terminal; `GIT_ASKPASS=<program>` is called once per credential with the
//! prompt as `argv[1]` and the answer read from stdout, which is how a TUI
//! asks for a password without losing the screen. Both measured.
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
//! trusted and the program runs: an editor that executes a stranger's code
//! because it was pointed at their checkout is one nobody can open a download
//! in. So `head_text`
//! opens through `git::without_running_anything`, which forces
//! `Trust::Reduced`. `core.autocrlf` and `.gitattributes` both survive that
//! level and the drivers do not -- measured, both ways. It is not the level
//! everything opens at, because reduced trust also stops gix resolving a
//! remote, and that is how the history knows which commits are pushed: the
//! reduction goes where the risk is and nowhere else. `git::statuses` still
//! opens fully, and `core.fsmonitor` is the same kind of hole; nobody has
//! closed it.
//!
//! The other half of the same comparison is the reader's side: a file
//! written back with `\r\n` in a project that stores `\n` differs from the
//! checkout in every line, and `git diff` says it does not. So a `Base`
//! carries the line ending git would rewrite the reader's lines to, and the
//! margin, the preview and the count of a list all compare through it.

pub mod blame;
pub mod change;
pub mod history;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

pub use blame::Blamed;

/// Something a walk of the history found out.
#[derive(Debug)]
pub enum Event {
    /// A batch of commits from a walk of the history.
    ///
    /// The walk is unbounded -- a file's history is every commit that ever
    /// touched it, and finding that out costs a tree lookup per commit of
    /// the whole project -- so the list fills while the reader reads it
    /// rather than making them wait for the end of it.
    Logged {
        /// Which walk these came from, so a history the reader has already
        /// moved off -- another tab, another file, a closed list -- can be
        /// dropped rather than shown under whatever is there now.
        generation: u64,
        /// The commits, newest first, continuing where the last batch left
        /// off.
        commits: Vec<history::Commit>,
        /// How many commits the walk has looked at, which is what says it is
        /// still going and how far it has got. A walk over a file nobody
        /// touched has nothing else to report for seconds at a time.
        walked: usize,
        /// Whether this is the last batch. An empty list that is still
        /// filling and one that is finished are different facts, and only
        /// the walk knows which is true.
        done: bool,
    },
    /// Who last changed each line of a file.
    Blamed {
        /// Which file it is about: a blame is a walk of history, and the
        /// reader may be looking at something else by the time it lands.
        path: std::path::PathBuf,
        /// Which version of it: a commit's, or the one the last commit has.
        /// A file and that file as some commit had it share a path and have
        /// different answers, so the answer has to say which it is.
        at: Option<gix::ObjectId>,
        /// One entry per line of the file as that version has it, from its
        /// first. `None` for a line no commit accounts for.
        lines: Vec<Option<Blamed>>,
    },
}

pub use change::{Changes, Hunk};

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

/// What names the project a path is in, for the things Obelus keeps about it.
///
/// A repository and its worktrees are one project. The notes are about the
/// code and the code is the same code: a reader with three worktrees open
/// has one list of what they mean to come back to, not three. `common_dir`
/// is git's own answer to "which repository is this" -- a linked worktree's
/// is the main checkout's -- so all of them come out with one name.
///
/// This is *the* key, and everything Obelus keeps about a project must use
/// it: the notes and the table saying which conversation is about which note
/// were keyed apart once, and one note with two conversations under it is a
/// reader opening it in one worktree and finding an empty page in the next.
/// The notes moved out of the project's own `.obelus` for this: they were
/// never shared with the next person anyway, since `.obelus` is a directory
/// readers gitignore.
///
/// Canonicalised rather than merely made absolute, which is what settles the
/// same directory reached by two spellings. On Windows that is the whole of
/// it: `canonicalize` goes through `GetFinalPathNameByHandle`, so the
/// spelling the reader typed comes back as the one the disk has. Elsewhere
/// it resolves the symlinks that would otherwise keep two paths to one
/// directory apart. A path that will not canonicalise because it cannot be
/// opened is taken as it came, because a name Obelus cannot work out is
/// worse than one that is merely long.
///
/// **A tree that has gone names no project.** It used to be taken as it
/// came, like the one that cannot be opened, and that answer was a
/// different project: what says a linked worktree belongs to its
/// repository is git, and git cannot be asked about a checkout that is not
/// there -- so the name fell back to the tree's own path, the notes page
/// came up empty, and the next note was written into a second file nobody
/// would ever find again. Nothing is the honest answer, and every caller
/// already has one for it: it is what a machine with nowhere to keep state
/// says.
///
/// Not a path: a file name. Every character a file name cannot safely carry
/// becomes `_`, which is many-to-one and does not matter -- what is wanted
/// is that one project is one name, not that the name can be read back.
#[must_use]
pub fn project(root: &Path) -> Option<String> {
    if is_gone(root) {
        return None;
    }
    let named = main_checkout(root).unwrap_or_else(|| root.to_path_buf());
    let named = named
        .canonicalize()
        .or_else(|_| std::path::absolute(&named))
        .unwrap_or(named);
    Some(file_name_of(&named))
}

/// A path written as a file name: every character a file name cannot safely
/// carry becomes `_`.
///
/// Many-to-one, and not meant to be read back -- what is wanted is that one
/// place is one name.
#[must_use]
pub fn file_name_of(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '.' => character,
            _ => '_',
        })
        .collect()
}

/// Whether a path is certainly not there any more.
///
/// Certainly: an answer the filesystem would not give -- a directory that
/// cannot be read, a disk that did not answer -- is not the same as one
/// that says the path has gone, and only the second is a reason to stop
/// treating it as somewhere.
#[must_use]
pub fn is_gone(path: &Path) -> bool {
    path.try_exists().is_ok_and(|there| !there)
}

/// The working tree a path is in, where it is in one.
///
/// Which is the project a reader opening a file means: `ob src/main.rs`
/// names the tree that file belongs to, not the one directory it happens
/// to sit in -- where the file list would hold four files, the search
/// would never leave them, and the project's own settings would be looked
/// for in a directory nobody puts them in.
///
/// This tree and not [`main_checkout`], which is the other half of the
/// same question and deliberately answers differently: what Obelus
/// *keeps* about a project is shared by every worktree of it, and what it
/// *shows* is the one the reader is actually in. A linked worktree has
/// its own files, and they are the files on the screen.
///
/// Made absolute rather than canonical, which is the rule the command
/// line already follows: a tree reached through a symlink stays under the
/// name the reader typed.
///
/// `None` where git has never heard of the path, and for a bare
/// repository, which has no working tree to read.
#[must_use]
pub fn worktree(path: &Path) -> Option<PathBuf> {
    let repository = repository(path)?;
    let work_dir = repository.workdir()?;
    std::path::absolute(work_dir).ok()
}

/// The checkout a project's things are named after, where git knows of one.
///
/// The main worktree, which every linked one shares: `common_dir` is its
/// `.git`, so the checkout is that directory's parent. `None` where git has
/// never heard of this path, and where the repository keeps its `.git`
/// somewhere else entirely -- a bare one, or `--separate-git-dir` -- because
/// then the parent is not a checkout and naming the project after it would
/// be naming it after somebody's directory of git directories.
#[must_use]
pub fn main_checkout(root: &Path) -> Option<PathBuf> {
    let repository = repository(root)?;
    // Resolved rather than taken as it comes: gix hands back what the
    // worktree's `commondir` file says, joined to the git directory and not
    // tidied, so a linked worktree answers
    // `.../.git/worktrees/one/../..` -- whose last component is `..` and
    // whose parent is the wrong directory by two levels. Canonical is also
    // what the name wants on Windows, where it is what turns the spelling
    // the reader typed into the one the disk has.
    let common = repository.common_dir().canonicalize().ok()?;
    if common.file_name()? != ".git" {
        return None;
    }
    // Without the `\\?\` that resolving puts on it on Windows: this is a
    // place a window is put on and a reader's list of projects keeps, and
    // `\\?\E:\work\obelus` is the same place as `E:\work\obelus` written so
    // that nothing comparing the two can tell -- the list had it twice.
    Some(dunce::simplified(common.parent()?).to_path_buf())
}

/// The repository a path is in, if it is in one.
///
/// Discovered from the path rather than from the working directory: the file
/// being read is the thing the question is about, and it can be outside the
/// project Obelus was started in.
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
/// converted the way a checkout would convert it, which is a thing Obelus
/// does to draw an honest margin. An editor that executes a stranger's code
/// because it was pointed at their clone is one nobody can open a download in.
///
/// Its own function rather than the level everything opens at, because
/// reduced trust costs something: gix will not resolve a remote whose url
/// comes from an untrusted config, and that is how the history knows which
/// commits have been pushed. So the reduction is applied where the risk is
/// -- the one place Obelus runs anything -- and nowhere else.
///
/// What survives it is what the conversion actually needs: `core.autocrlf`
/// and `.gitattributes` are both read at this level. Measured, both of
/// them.
///
/// And the reduction is not the whole of it, which is the part that had to
/// be found out. gix takes the level it is given and then *puts it back*
/// to full where `safe.directory` covers the path -- a line plenty of
/// readers have in their own config as `*`, and one GitHub's runner image
/// has. On those machines the reduction bought nothing at all, and Obelus
/// ran the program a stranger's clone named. So the section filter says it
/// again in the one place it decides: see `nothing_the_repository_named`.
fn without_running_anything(path: &Path) -> Option<gix::Repository> {
    let from = if path.is_dir() { path } else { path.parent()? };
    let trust = gix::sec::Trust::Reduced;
    let found = gix::discover::upwards(from).ok()?.0;
    let (git_dir, _) = found.into_repository_and_work_tree_directories();
    let options = gix::open::Options::default()
        // Every config file Obelus would have read anyway. The level is
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
        .filter_config_section(nothing_the_repository_named)
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
/// the project Obelus was started in.
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
/// committing, which Obelus does not do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileStatus {
    /// Tracked, and different from the last commit.
    Changed,
    /// Not in the last commit at all.
    New,
    /// In the last commit, and not on the disk any more.
    ///
    /// A row for something that is not there, which is the point: it is a
    /// change to the tree, git reports it as one, and a list of changes
    /// that left it out was a list that disagreed with `git status`.
    /// Choosing one opens what the last commit had, to read and not to
    /// edit -- the only version of it there is.
    Gone,
    /// Kept out of the tree by `.gitignore` and friends.
    ///
    /// Never from [`statuses`], which does not ask about them -- `git
    /// status` leaves them out and so does Obelus. It comes from the file
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
///
/// **Ask the question you mean.** The gate that wants to know *whether*
/// anything has changed -- asked on every command in the palette -- used to
/// build a map of every changed path and take its length. Measured
/// afterwards, and honestly: this is two and a half times cheaper on a
/// project with something in it and no cheaper at all on a clean one,
/// because the walk has to reach the end to find nothing. The phrasing was
/// wrong; the cost lives in the walk, and saying otherwise would be a win
/// claimed rather than got.
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

/// The mode a staged change records, whichever side of it has one.
fn mode_of(change: &gix::diff::index::Change) -> Option<gix::index::entry::Mode> {
    use gix::diff::index::Change;
    Some(match change {
        Change::Addition { entry_mode, .. }
        | Change::Deletion { entry_mode, .. }
        | Change::Modification { entry_mode, .. }
        | Change::Rewrite { entry_mode, .. } => *entry_mode,
    })
}

/// Whether a mode is the gitlink git records another repository as.
fn gix_mode_is_submodule(mode: gix::index::entry::Mode) -> bool {
    mode.is_submodule()
}

/// How one file stands with git.
///
/// A struct and not just the status, because a move is one change with two
/// names in it and `git status` says both: `R moved.rs -> deep/moved.rs` is
/// one row there and is one row here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Standing {
    /// What has happened to it.
    pub status: FileStatus,
    /// Another repository at this path.
    ///
    /// Git records one as a commit at a path and reports it as changed the
    /// moment that commit moves, so it arrives here looking exactly like a
    /// modified file and is a directory. It is a change to the tree and is
    /// listed as one; it is not something Obelus can open, and the row says
    /// so rather than waiting to be pressed to say it.
    pub submodule: bool,
    /// What it was called before, for a move git has been told about.
    ///
    /// Only a move git itself reports -- which is one that has been staged,
    /// by `git mv` or by adding both halves. A file moved in the working
    /// tree and not staged is a deletion and an untracked file to git, and
    /// Obelus says what git says: pairing those two up would be Obelus's
    /// inference rather than the tree's state, and this list is the tree's
    /// state.
    pub was: Option<PathBuf>,
}

impl From<FileStatus> for Standing {
    /// A file in that state, where it was, and a file rather than another
    /// repository -- which is the ordinary case and the one a test setting
    /// up a tree means.
    fn from(status: FileStatus) -> Self {
        Self {
            status,
            submodule: false,
            was: None,
        }
    }
}

impl Standing {
    /// Changed, and still where it was.
    #[must_use]
    fn changed() -> Self {
        Self {
            status: FileStatus::Changed,
            submodule: false,
            was: None,
        }
    }

    /// Not in the last commit at all.
    #[must_use]
    fn new() -> Self {
        Self {
            status: FileStatus::New,
            submodule: false,
            was: None,
        }
    }

    /// In the last commit, and not on the disk any more.
    #[must_use]
    fn gone() -> Self {
        Self {
            status: FileStatus::Gone,
            submodule: false,
            was: None,
        }
    }
}

/// What `HEAD` points at, for asking what a file used to say.
#[must_use]
pub fn head_commit(root: &Path) -> Option<gix::ObjectId> {
    Some(repository(root)?.head_id().ok()?.detach())
}

/// What `HEAD` is, where a reader is being told which branch they are on.
///
/// Three answers and not two, which is why this is an enum inside an
/// option: there being no repository and there being one with no branch
/// checked out are different things to say, and a reader told nothing in
/// both cases cannot tell them apart. `head_ref` answers `None` for a
/// detached `HEAD`, so folding that into "no repository" is exactly the
/// mistake available here.
///
/// The commit is not one of the answers. A short id already means
/// something else where this is drawn -- that the buffer is some commit's
/// version of its file -- and two short ids on one row meaning two things
/// is a row a reader has to guess at. The history is one key away.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Head {
    /// The branch it is on, by the name the reader wrote.
    Branch(String),
    /// A commit, rather than a branch.
    Detached,
}

/// Which branch the tree at this path has checked out.
///
/// The tree's and not the project's: a linked worktree has a `HEAD` of its
/// own while sharing `common_dir` with the repository it came from, so
/// asking by [`project`] would answer the same branch for all of them and
/// be wrong for all but one. What is keyed by the project is what the
/// worktrees are meant to share -- the notes, and which conversation is
/// about which -- and the branch is not that. The test for which key a
/// question wants is whether two worktrees should agree about the answer:
/// the notes yes, the branch no.
///
/// Through `head_ref`, which is the same question [`history::refs_of`]
/// asks to mark which of the names it lists is the current one: one place
/// that knows how to ask what `HEAD` is.
#[must_use]
pub fn head_of_the_tree(within: &Path) -> Option<Head> {
    Some(head_of(&repository(within)?))
}

/// What `HEAD` is in a repository already opened.
fn head_of(repository: &gix::Repository) -> Head {
    match repository.head_ref() {
        Ok(Some(head)) => Head::Branch(head.name().shorten().to_string()),
        Ok(None) | Err(_) => Head::Detached,
    }
}

/// One of the checkouts a repository has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Worktree {
    /// Where it is, as git wrote it down.
    pub path: PathBuf,
    /// What it has checked out.
    ///
    /// Read from the tree's own `HEAD`, which lives in the repository and
    /// not in the checkout, so a tree whose directory has gone still says.
    pub head: Head,
    /// Whether its directory is there.
    ///
    /// Asked here rather than left to whoever draws it, because git goes
    /// on listing a checkout that was deleted without being told --
    /// `rm -rf` rather than `git worktree remove` -- until somebody prunes
    /// it, and a row for it is a row for somewhere there is nothing.
    pub there: bool,
}

/// Every checkout of the repository a path is in: the main one first, then
/// the linked ones in git's order, which is by their names.
///
/// Empty where git has never heard of the path. A bare repository has no
/// main checkout, so its list is the linked ones alone.
#[must_use]
pub fn worktrees(within: &Path) -> Vec<Worktree> {
    let Some(repository) = repository(within) else {
        return Vec::new();
    };
    let main = main_checkout(within).and_then(|path| {
        let opened = repository.main_repo().ok()?;
        Some(Worktree {
            head: head_of(&opened),
            there: path.is_dir(),
            path,
        })
    });
    let linked = repository.worktrees().unwrap_or_default();
    main.into_iter()
        .chain(linked.into_iter().filter_map(|proxy| {
            let path = proxy.base().ok()?;
            // Opened without its checkout, because the checkout is what
            // may have gone and the `HEAD` that says what it had is not in
            // it.
            let opened = proxy.into_repo_with_possibly_inaccessible_worktree().ok()?;
            Some(Worktree {
                head: head_of(&opened),
                there: path.is_dir(),
                path,
            })
        }))
        .collect()
}

/// Whether the repository a path is in has a checkout besides that one.
///
/// [`worktrees`] counted rather than built: building opens every linked
/// tree to read what it has checked out, and this is asked for a row of the
/// palette, where what is wanted is whether there is a second one to go to.
#[must_use]
pub fn has_another_worktree(within: &Path) -> bool {
    let Some(repository) = repository(within) else {
        return false;
    };
    let linked = repository.worktrees().map_or(0, |linked| linked.len());
    linked + usize::from(main_checkout(within).is_some()) > 1
}

/// Everything git says has changed in a tree, by path.
pub fn statuses(root: &Path) -> HashMap<PathBuf, Standing> {
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
        let (path, standing) = match item {
            // Tracked, and not as the index has it. What kind of not is
            // git's own answer rather than a guess from the disk: a file
            // that is gone says so, and a submodule -- which git records as
            // a commit at a path and reports as changed the moment that
            // commit moves -- says that too, and is a directory Obelus has
            // no notion of opening.
            Item::IndexWorktree(index_worktree::Item::Modification {
                rela_path, status, ..
            }) => {
                use gix::status::plumbing::index_as_worktree::{Change, EntryStatus};
                match status {
                    EntryStatus::Change(Change::Removed) => (rela_path, Standing::gone()),
                    // Another repository at a path. Listed, because it is
                    // a change to this tree and git reports it as one; the
                    // row says what it is and cannot be pressed, which is
                    // the word said where it is read rather than behind a
                    // key that answers "no".
                    EntryStatus::Change(Change::SubmoduleModification(_)) => (
                        rela_path,
                        Standing {
                            submodule: true,
                            ..Standing::changed()
                        },
                    ),
                    _ => (rela_path, Standing::changed()),
                }
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
                (entry.rela_path, Standing::new())
            }
            // A move git found between the index and the working tree.
            Item::IndexWorktree(index_worktree::Item::Rewrite {
                dirwalk_entry,
                source,
                ..
            }) => (
                dirwalk_entry.rela_path,
                Standing {
                    status: FileStatus::Changed,
                    submodule: false,
                    was: gix::path::try_from_bstr(source.rela_path())
                        .ok()
                        .map(|path| path.into_owned()),
                },
            ),
            // Staged: the index differs from `HEAD`.
            Item::TreeIndex(change) => {
                let path = change.location().to_owned();
                // A submodule arrives here too, once the commit it records
                // has been staged, and it is the same directory it was
                // before. Told by the mode git keeps for one -- a gitlink,
                // which is neither a file nor a tree -- because that is
                // what it is rather than what the disk happens to hold.
                let submodule = mode_of(&change).is_some_and(gix_mode_is_submodule);
                if submodule {
                    let Ok(path) = gix::path::try_from_bstring(path) else {
                        continue;
                    };
                    statuses
                        .entry(work_dir.join(path))
                        .or_insert_with(|| Standing {
                            submodule: true,
                            ..Standing::changed()
                        });
                    continue;
                }
                let standing = match &change {
                    // Not in the last commit at all.
                    gix::diff::index::Change::Addition { .. } => Standing::new(),
                    gix::diff::index::Change::Deletion { .. } => Standing::gone(),
                    gix::diff::index::Change::Modification { .. } => Standing::changed(),
                    // The one change that carries two names, and the reason
                    // this is a struct: `git status` writes it
                    // `R old -> new`, and so does the list.
                    gix::diff::index::Change::Rewrite {
                        source_location, ..
                    } => Standing {
                        status: FileStatus::Changed,
                        submodule: false,
                        was: gix::path::try_from_bstr(source_location.as_ref())
                            .ok()
                            .map(|path| path.into_owned()),
                    },
                };
                (path, standing)
            }
        };
        let Ok(path) = gix::path::try_from_bstring(path) else {
            continue;
        };
        fold(&mut statuses, work_dir.join(path), standing);
    }
    statuses
}

/// Puts one of git's answers about a file in with whatever it has already
/// said about it.
///
/// The first answer wins: a file is reported twice when it was staged and
/// then changed again, and the first says the more surprising thing, which
/// is what a reader scanning the list is looking for.
///
/// Every part of it but the name it had, which is merged in whichever
/// answer carries it. Only one of them does -- a rename is a change
/// between two of the three states git compares, and the other answer is
/// about the third -- and which arrives first is not Obelus's to decide:
/// gix reports the index against the working tree and the head against the
/// index as one stream, and a file that was moved and then edited came
/// back a move or an ordinary change depending on which finished first. A
/// move that reads as a move on some runs and not others is worse than one
/// that never did, because nothing about the tree says which the reader is
/// looking at.
fn fold(statuses: &mut HashMap<PathBuf, Standing>, path: PathBuf, standing: Standing) {
    let was = standing.was.clone();
    let standing = statuses.entry(path).or_insert(standing);
    if standing.was.is_none() {
        standing.was = was;
    }
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
///
/// Counted against a [`Base`], the margin's own, so the list and the margin
/// agree about which line endings are changes.
#[must_use]
pub fn counted_against_head(paths: &[PathBuf]) -> HashMap<PathBuf, (usize, usize)> {
    let mut counts = HashMap::new();
    let Some(first) = paths.first() else {
        return counts;
    };
    // Converting a blob the way a checkout would is a place a repository's
    // `filter.*` would run, so the repository is opened the way the margin's
    // is.
    let Some(repository) = without_running_anything(first) else {
        return counts;
    };
    let Ok(head) = repository.head_tree() else {
        return counts;
    };
    let mut checkout = Checkout::of(&repository);
    for path in paths {
        let Some(relative) = in_repository(&repository, path) else {
            continue;
        };
        let mut tree = head.clone();
        let mut base = match tree.peel_to_entry_by_path(&relative) {
            Ok(Some(entry)) => match entry.object() {
                // Not text, so it has no lines to count.
                Ok(object) if std::str::from_utf8(&object.data).is_err() => continue,
                Ok(object) => checkout.base(&object.data, &relative),
                Err(_) => continue,
            },
            // The commit does not have it: everything in it arrived.
            Ok(None) => checkout.base(b"", &relative),
            Err(_) => continue,
        };
        // A file that is gone reads as empty, which makes its lines removed.
        let now = std::fs::read_to_string(path).unwrap_or_default();
        counts.insert(
            path.clone(),
            change::counted(&change::drawn(&base.text.clone(), &base.as_compared(&now))),
        );
    }
    counts
}

/// The file as the last commit has it, or `None` if that question has no
/// answer -- [`head`]'s text.
#[must_use]
pub fn head_text(path: &Path) -> Option<String> {
    head(path).map(|base| base.text)
}

/// What the file is compared with to say what has changed in it since the
/// last commit, or `None` if that question has no answer.
///
/// No answer covers every way this can decline, and they all mean the same
/// thing to a reader: no markers. Not a repository, a file git has never
/// heard of, a repository with no commits yet, or a file whose committed
/// content is not text.
#[must_use]
pub fn head(path: &Path) -> Option<Base> {
    let repository = without_running_anything(path)?;
    let relative = in_repository(&repository, path)?;
    let mut tree = repository.head_tree().ok()?;
    let entry = tree.peel_to_entry_by_path(&relative).ok()??;
    let object = entry.object().ok()?;
    Some(Checkout::of(&repository).base(&object.data, &relative))
}

/// What a text is compared with to say what has changed in it.
///
/// A stored version as a checkout would write it, and the way git would
/// carry the reader's text there: in as it would be committed and back out
/// as it would be checked out, which is what `git diff` compares by. A file
/// the attributes say is text, written back with `\r\n` by something that
/// picks the platform's line ending -- Python's text mode and PowerShell's
/// `Set-Content` both do, on Windows -- differs from its commit in every line
/// and in nothing git would commit. Compared as it was, a two-hundred-line
/// change was listed as four thousand lines taken away and four thousand put
/// back, and the margin marked every line.
///
/// Through git's own conversion rather than a rule about `\r` of Obelus's,
/// because which files git rewrites is git's question: where nothing says a
/// file is text, where the index already holds `\r\n`, or where `text=auto`
/// finds a lone `\r` and calls the file binary, git keeps the `\r` and
/// `git diff` calls those lines changed -- and a margin that did not would be
/// the one disagreeing.
pub struct Base {
    text: String,
    /// Whether the stored version, as checked out, has a `\r` anywhere.
    returns: bool,
    rewrite: Option<Rewrite>,
}

impl std::fmt::Debug for Base {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Base")
            .field("text", &self.text.len())
            .field("rewrite", &self.rewrite.is_some())
            .finish()
    }
}

impl Base {
    /// A stored version compared with exactly as it is: the text beside it
    /// is another stored version, and two blobs need no conversion to agree.
    #[must_use]
    pub const fn as_stored(text: String) -> Self {
        Self {
            // Nothing to carry it through, so nothing to skip.
            returns: false,
            text,
            rewrite: None,
        }
    }

    /// The stored version, as a checkout would write it.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// What has changed between this and `now`.
    #[must_use]
    pub fn changes(&mut self, now: &str) -> Changes {
        let now = self.as_compared(now);
        Changes::between(&self.text, &now)
    }

    /// `now` as it would be after a round trip through git, or as it is
    /// where there is no trip to take.
    ///
    /// Only ever with as many lines as `now` has, because what the margin
    /// marks is drawn against the buffer's line numbers: a conversion that
    /// moved them would put every mark on the wrong line, which is worse than
    /// the `\r`s it was asked about.
    fn as_compared<'a>(&mut self, now: &'a str) -> std::borrow::Cow<'a, str> {
        use std::borrow::Cow;
        // The trip is the margin's diff again in cost, on every keystroke,
        // and almost every file has no `\r` on either side -- where the
        // trip cannot change a line ending. A checkout that writes `\r\n`
        // put them in the stored side too, so a `\r` it would add shows up
        // there; and with none in the text there is none to take out.
        if !self.returns && !now.contains('\r') {
            return Cow::Borrowed(now);
        }
        let Some(rewrite) = self.rewrite.as_mut() else {
            return Cow::Borrowed(now);
        };
        match rewrite.round_trip(now.as_bytes()).map(String::from_utf8) {
            Some(Ok(trip)) if trip != now && lines(&trip) == lines(now) => Cow::Owned(trip),
            _ => Cow::Borrowed(now),
        }
    }
}

fn lines(text: &str) -> usize {
    text.bytes().filter(|&byte| byte == b'\n').count()
}

/// git's conversions for one file, kept so that a text can be carried in and
/// back out on every keystroke without opening the repository again.
///
/// Without three things the pipeline a checkout uses would do. No driver,
/// because this runs as the reader types, and a `filter.*` program is a
/// process per keystroke -- the stored side has been through it once, on
/// the way to [`Base::text`], and a file a driver owns is compared as it
/// was. And no round-trip check of either kind: `core.safecrlf` makes gix
/// *fail* a conversion `git add` would only warn about, and a failed
/// conversion here is a comparison of raw bytes, which is every line marked.
struct Rewrite {
    pipeline: gix::filter::plumbing::Pipeline,
    attributes: gix::worktree::Stack,
    objects: gix::OdbHandle,
    relative: PathBuf,
    /// What the index holds for the file: `text=auto` leaves a file's `\r`s
    /// alone where the index already has them, so the answer depends on it.
    indexed: Option<Vec<u8>>,
}

impl Rewrite {
    fn round_trip(&mut self, now: &[u8]) -> Option<Vec<u8>> {
        let Self {
            pipeline,
            attributes,
            objects,
            relative,
            indexed,
        } = self;
        let name = unix_name(relative);
        let stored = {
            let at = attributes
                .at_entry(AsRef::<gix::bstr::BStr>::as_ref(&name), None, &*objects)
                .ok()?;
            read_all(
                pipeline
                    .convert_to_git(
                        now,
                        relative,
                        &mut |_, out| {
                            at.matching_attributes(out);
                        },
                        &mut |buf| {
                            Ok(indexed.as_ref().map(|indexed| {
                                buf.clear();
                                buf.extend_from_slice(indexed);
                            }))
                        },
                    )
                    .ok()?,
            )?
        };
        let at = attributes
            .at_entry(AsRef::<gix::bstr::BStr>::as_ref(&name), None, &*objects)
            .ok()?;
        read_all(
            pipeline
                .convert_to_worktree(
                    &stored,
                    name.as_ref(),
                    &mut |_, out| {
                        at.matching_attributes(out);
                    },
                    Default::default(),
                )
                .ok()?,
        )
    }
}

/// A repository's own way of converting a file, built once for every file
/// it is asked about.
struct Checkout<'repo> {
    repository: &'repo gix::Repository,
    pipeline: Option<(
        gix::filter::Pipeline<'repo>,
        gix::worktree::IndexPersistedOrInMemory,
    )>,
}

impl<'repo> Checkout<'repo> {
    fn of(repository: &'repo gix::Repository) -> Self {
        Self {
            repository,
            pipeline: repository.filter_pipeline(None).ok(),
        }
    }

    fn base(&mut self, data: &[u8], relative: &Path) -> Base {
        let text = self.written(data, relative);
        Base {
            returns: text.contains('\r'),
            text,
            rewrite: self.rewrite(relative),
        }
    }

    /// A stored blob as it would be on disk.
    ///
    /// The one place this matters is here, and it is why: everywhere else
    /// that Obelus reads an old version of a file, it compares it against
    /// *another* stored version, and two blobs converted the same way or not
    /// at all give the same answer. The margin compares a stored version
    /// against the reader's own buffer, which came off the disk -- so a
    /// project whose `.gitattributes` says `eol=crlf` had every line of
    /// every file marked as changed, because git stores `\n` and the file on
    /// disk holds `\r\n`.
    ///
    /// The bytes unchanged where there is nothing to convert or the
    /// conversion fails, which is what the diff had before and is never
    /// worse than it.
    fn written(&mut self, data: &[u8], relative: &Path) -> String {
        let text = || String::from_utf8_lossy(data).into_owned();
        let Some((pipeline, _)) = self.pipeline.as_mut() else {
            return text();
        };
        let Ok(converted) =
            pipeline.convert_to_worktree(data, unix_name(relative).as_ref(), Default::default())
        else {
            return text();
        };
        read_all(converted).map_or_else(text, |out| String::from_utf8_lossy(&out).into_owned())
    }

    fn rewrite(&self, relative: &Path) -> Option<Rewrite> {
        let (_, index) = self.pipeline.as_ref()?;
        let mut options = gix::filter::Pipeline::options(self.repository).ok()?;
        options.drivers.clear();
        options.crlf_roundtrip_check = gix::filter::plumbing::pipeline::CrlfRoundTripCheck::Skip;
        options.encodings_with_roundtrip_check.clear();
        let attributes = self
            .repository
            .attributes_only(
                index,
                gix::worktree::stack::state::attributes::Source::WorktreeThenIdMapping,
            )
            .ok()?
            .detach();
        let indexed = index
            .entry_by_path(unix_name(relative).as_ref())
            .and_then(|entry| self.repository.find_object(entry.id).ok())
            .map(|object| object.detach().data);
        Some(Rewrite {
            pipeline: gix::filter::plumbing::Pipeline::new(
                self.repository.command_context().ok()?,
                options,
            ),
            attributes,
            objects: self.repository.objects.clone(),
            relative: relative.to_path_buf(),
            indexed,
        })
    }
}

/// The name the attributes are matched against, which is a repository path
/// with forward slashes whatever this platform writes.
fn unix_name(relative: &Path) -> gix::bstr::BString {
    gix::path::to_unix_separators_on_windows(gix::path::into_bstr(relative)).into_owned()
}

fn read_all(mut from: impl std::io::Read) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    from.read_to_end(&mut out).ok()?;
    Some(out)
}

/// Whether a section of the configuration may be read from.
///
/// Anything but the repository's own file. gix's own answer is "a section
/// whose file is fully trusted, or which did not come from the repository"
/// -- and the first half of that is the half that moves: `safe.directory`
/// puts a reduced level back to full, so the repository's own config
/// becomes trusted and `filter.*` becomes a program Obelus runs. This
/// answer does not move, because it does not ask about trust at all.
///
/// `filter.*` is the only place a repository names a program that Obelus
/// would run, and `extract_drivers` asks this before it takes one. What
/// this does not touch is `core.autocrlf` and `.gitattributes`: the
/// conversion reads the resolved file directly, without the filter, which
/// is why the margin is still honest about a project storing `\n` and
/// checking out `\r\n`.
///
/// The reader's own global config is not the repository's, so a
/// `filter.*` they wrote themselves still runs. That is the line: what
/// Obelus refuses is a program named by the thing it was pointed at.
fn nothing_the_repository_named(meta: &gix::config::file::Metadata) -> bool {
    meta.source.kind() != gix::config::source::Kind::Repository
}

#[cfg(test)]
mod folding {
    use super::{FileStatus, Standing, fold};

    /// A file git reports twice keeps the name it had, whichever answer
    /// carried it.
    ///
    /// gix reports the index against the working tree and the head against
    /// the index as one stream, and a file moved and then edited comes back
    /// in both halves: the move carries the old name, the edit does not.
    /// The order they arrive in is gix's and changes with the load on the
    /// machine, so a rule that took the first answer whole made a move show
    /// as a move on some runs and as an ordinary change on others -- which
    /// is what it did, about one workspace run in three.
    ///
    /// Broken deliberately by dropping the merge and keeping the first
    /// answer whole: the second order below then loses the name.
    #[test]
    fn a_file_reported_twice_keeps_the_name_it_had() {
        let moved = || Standing {
            status: FileStatus::Changed,
            submodule: false,
            was: Some(std::path::PathBuf::from("file.rs")),
        };
        let edited = || Standing::changed();
        for (first, second) in [(moved(), edited()), (edited(), moved())] {
            let mut statuses = std::collections::HashMap::new();
            let path = std::path::PathBuf::from("deep/moved.rs");
            fold(&mut statuses, path.clone(), first);
            fold(&mut statuses, path.clone(), second);
            assert_eq!(
                statuses.get(&path).and_then(|it| it.was.clone()),
                Some(std::path::PathBuf::from("file.rs")),
                "the name it had went with the order the answers arrived in"
            );
        }
    }
}
