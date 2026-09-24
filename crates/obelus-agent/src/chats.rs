//! Which of a project's notes have a conversation open, and in whose Obelus.
//!
//! Obelus does not split its window, so several Obelus processes on one
//! project is the normal case -- and a conversation is not a thing two of
//! them may have open at once. The agent keeps what was said and takes one
//! prompt turn at a time; two clients prompting one conversation is two
//! turns in it, which is the thing Obelus's own queue exists to prevent,
//! arriving from outside the process that queue lives in.
//!
//! So a conversation is claimed, and the claim is a lock the operating
//! system holds rather than anything Obelus writes down. A process that is
//! killed, crashes or loses power releases it on the way out without having
//! to be asked, which is the whole reason for choosing a lock over a process
//! number in a file: a number has to be believed, checked against a process
//! that may be somebody else's by now, and given a staleness nobody can pick
//! -- ten minutes is too long for a reader waiting and too short for a
//! conversation left open over lunch.
//!
//! The file itself is what the watcher can see. A lock is invisible to it --
//! nothing is written when one is taken -- so the file is created with the
//! claim and removed with it, and that is the signal another Obelus wakes
//! on. The file's *existence* means nothing on its own: one left behind by a
//! process that died is a file nobody holds, and asking for the lock is what
//! says which it is.

use std::{
    collections::BTreeSet,
    fs::File,
    path::{Path, PathBuf},
};

use obelus_git::todo::NoteId;

/// Where a project's claims are kept, one file per note.
#[must_use]
pub fn directory(root: &Path) -> Option<PathBuf> {
    Some(
        obelus_logging::state_directory()?
            .join("chats")
            .join(obelus_git::project(root)),
    )
}

/// Says this Obelus has the conversation about `note`, unless another has.
///
/// `None` is another Obelus holding it. Also a system with nowhere to keep
/// the claim, and that is deliberately the same answer: a reader whose
/// machine cannot say who has what is a reader for whom Obelus cannot keep
/// this promise, and the honest thing is to decline rather than to let two
/// windows into one conversation while saying nothing.
#[must_use]
pub fn claim(root: &Path, note: &NoteId) -> Option<Claim> {
    let path = directory(root)?.join(note.as_str());
    std::fs::create_dir_all(path.parent()?).ok()?;
    // Opened rather than created exclusively: the file left behind by a
    // process that died is not a claim, and refusing on finding one would
    // hand the reader a conversation they can never open again.
    let file = File::options()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .ok()?;
    if held_by_somebody_else(&file) {
        return None;
    }
    Some(Claim { path, file })
}

/// Every note in this project whose conversation somebody has open.
///
/// Including this Obelus's own: a lock is about the open file and not about
/// the process, so a second look from the same process finds its own claim
/// in the way. Which of them are this one's is a question this cannot answer
/// and the caller already knows -- it is holding them.
#[must_use]
pub fn held(root: &Path) -> BTreeSet<NoteId> {
    let Some(directory) = directory(root) else {
        return BTreeSet::new();
    };
    let Ok(entries) = std::fs::read_dir(&directory) else {
        // No conversation has ever been opened in this project, which is
        // where every project starts.
        return BTreeSet::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let note = NoteId::read(entry.file_name().to_str()?)?;
            let file = File::options().write(true).open(entry.path()).ok()?;
            held_by_somebody_else(&file).then_some(note)
        })
        .collect()
}

/// Whether somebody has this one note's conversation open.
///
/// The same question [`held`] answers for all of them, asked of one: what
/// wants it is the foot of the notes, which says what the key under the
/// reader will do and is drawn every frame. One file opened rather than a
/// directory walked.
#[must_use]
pub fn held_by_anybody(root: &Path, note: &NoteId) -> bool {
    let Some(path) = directory(root).map(|directory| directory.join(note.as_str())) else {
        return false;
    };
    File::options()
        .write(true)
        .open(&path)
        .is_ok_and(|file| held_by_somebody_else(&file))
}

/// A conversation this Obelus has open, which it gives up by being dropped.
///
/// Dropped rather than given up by hand, for the reason the install's claim
/// beside this one is: every way out of a conversation would otherwise have
/// to remember, and the one that forgot would lock the reader out of their
/// own note until they restarted Obelus.
#[derive(Debug)]
pub struct Claim {
    path: PathBuf,
    /// Held open for as long as the claim is: the lock belongs to the open
    /// file and goes when it closes, which is also what makes a killed
    /// Obelus give it up.
    #[expect(
        dead_code,
        reason = "it is the lock itself: what it is for is staying open"
    )]
    file: File,
}

impl Drop for Claim {
    fn drop(&mut self) {
        // The name goes here and the lock goes a moment later, when the
        // file this holds is closed on the way out of this. That order is
        // the useful one: another Obelus wakes on the file going, and by
        // the time it has looked the lock is gone too.
        if let Err(error) = std::fs::remove_file(&self.path) {
            tracing::warn!(%error, path = %self.path.display(), "a claim outlived its conversation");
        }
    }
}

/// Whether somebody already has this file locked.
///
/// Asked by trying to take the lock, which is the only way to ask it: what
/// would answer without taking anything is `fcntl`'s `F_GETLK`, and `fcntl`
/// locks are the ones a process drops in their entirety when it closes *any*
/// descriptor on the file -- which this opens twice, once to hold a claim
/// and once to look at one, so Obelus would let go of its own claim by
/// glancing at it.
///
/// A lock taken here is given up again when the file closes, at the end of
/// the caller's expression. The cost is that a claim asked for in exactly
/// that instant is told the conversation is open elsewhere when it is not:
/// microseconds wide, no worse than a message that is wrong until the reader
/// presses the key again, and the alternative was losing a claim for real.
#[cfg(unix)]
fn held_by_somebody_else(file: &File) -> bool {
    use std::os::fd::AsRawFd as _;

    // Safety: `flock` takes a descriptor and a flag and reads nothing
    // through a pointer. The descriptor is this file's and outlives the
    // call.
    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) != 0 }
}

/// The same question, in the terms the other platform puts it in.
#[cfg(windows)]
fn held_by_somebody_else(file: &File) -> bool {
    use std::os::windows::io::AsRawHandle as _;

    use windows_sys::Win32::{
        Storage::FileSystem::{LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LockFileEx},
        System::IO::OVERLAPPED,
    };

    // Safety: the handle is this file's and outlives the call, and the
    // overlapped structure is this stack frame's, written by the call and
    // read by nobody.
    unsafe {
        let mut overlapped: OVERLAPPED = std::mem::zeroed();
        LockFileEx(
            file.as_raw_handle(),
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            // The whole file, which has nothing in it: what is being locked
            // is the name, and a range is how this platform spells one.
            u32::MAX,
            u32::MAX,
            &raw mut overlapped,
        ) == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Somewhere of this run's own, and a project name to go with it.
    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("obelus-chats-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("the directory");
        obelus_logging::state_directory_for_test(
            std::env::temp_dir().join(format!("obelus-chats-state-{}", std::process::id())),
        );
        root
    }

    /// Two Obelus cannot have one note's conversation open at once.
    ///
    /// The agent takes one prompt turn at a time and Obelus queues what the
    /// reader says into a running one -- a queue that lives in one process,
    /// so a second process prompting the same conversation walks straight
    /// past it. This is the half of that promise the other process can see.
    ///
    /// Two claims from one process rather than two processes, and it is the
    /// same question: a `flock` belongs to the open file and not to the
    /// process, so a second `open` of the same path is as much somebody else
    /// as another Obelus is. That is also what makes [`held`] work.
    ///
    /// Broken deliberately by having `claim` answer `Some` without asking
    /// `held_by_somebody_else`: the second one comes back held as well.
    #[test]
    fn one_note_has_one_conversation_open() {
        let root = scratch("one-at-a-time");
        let note = NoteId::read("0123456A").expect("a name");

        let first = claim(&root, &note).expect("nobody had it");
        assert!(
            claim(&root, &note).is_none(),
            "a second Obelus was let into the conversation"
        );
        assert!(held(&root).contains(&note), "the claim does not show");

        // And giving it up gives it up: a reader who closes a conversation
        // in one window can open it in the next.
        drop(first);
        assert!(
            held(&root).is_empty(),
            "the claim outlived the conversation"
        );
        assert!(claim(&root, &note).is_some(), "nobody can have it now");
    }

    /// A claim left behind by an Obelus that died is not a claim.
    ///
    /// Which is the whole reason for a lock rather than a process number
    /// written down: nothing sweeps this directory, nothing has to decide
    /// how old is too old, and a machine that lost power comes back with
    /// every conversation openable.
    ///
    /// Broken deliberately by having `held` answer for the file existing
    /// rather than for the lock: the leftover reads as somebody's.
    #[test]
    fn a_file_nobody_holds_is_not_a_claim() {
        let root = scratch("left-behind");
        let note = NoteId::read("0123456B").expect("a name");
        let path = directory(&root).expect("somewhere").join(note.as_str());
        std::fs::create_dir_all(path.parent().expect("a directory")).expect("the directory");
        std::fs::write(&path, "").expect("what the dead Obelus left");

        assert!(held(&root).is_empty(), "a leftover file reads as a claim");
        assert!(
            claim(&root, &note).is_some(),
            "the conversation cannot be opened again"
        );
    }
}
