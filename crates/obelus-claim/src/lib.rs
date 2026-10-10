//! How one Obelus holds something against the others on the machine, and
//! how it knows a knock is from one of them.
//!
//! Several Obelus processes on one project is the normal case, and a few
//! things may be had by one of them at a time: a conversation, an agent's
//! install, the chat's connection, a window on a tree. Each is a file with a
//! lock on it that the kernel holds -- and gives up with the process, so an
//! Obelus that is killed lets go without being asked. What each of them
//! means is its owner's; how a lock is taken and looked at is here, once,
//! because the two platforms put it two ways and one of them has been got
//! wrong before.
//!
//! And a key: a word nobody else has, which one Obelus writes beside the
//! door it listens at and another says first when it knocks.

use std::fs::File;

/// Whether somebody already has this file locked.
///
/// Asked by trying to take the lock, which is the only way to ask it: what
/// would answer without taking anything is `fcntl`'s `F_GETLK`, and `fcntl`
/// locks are the ones a process drops in their entirety when it closes *any*
/// descriptor on the file -- which this opens twice, once to hold a claim
/// and once to look at one, so Obelus would let go of its own claim by
/// glancing at it.
///
/// The descriptor this is handed must be opened for *reading*, which is
/// what every caller does and what a claim itself deliberately does not.
/// A watcher reports a file being closed by a process that had it open for
/// writing, and that report is the only notice there is that a claim ended
/// because its Obelus died -- see `obelus_watch`. So a claim is held by a
/// writer, on purpose; and Obelus's own looking is reading, on purpose,
/// because a look that announced itself would be Obelus waking itself up to
/// look again.
///
/// A lock taken here is given up again when the file closes, at the end of
/// the caller's expression. The cost is that a claim asked for in exactly
/// that instant is told the conversation is open elsewhere when it is not:
/// microseconds wide, no worse than a message that is wrong until the reader
/// presses the key again, and the alternative was losing a claim for real.
#[cfg(unix)]
pub fn held_by_somebody_else(file: &File) -> bool {
    use std::os::fd::AsRawFd as _;

    // Safety: `flock` takes a descriptor and a flag and reads nothing
    // through a pointer. The descriptor is this file's and outlives the
    // call.
    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) != 0 }
}

/// The same question, in the terms the other platform puts it in -- which
/// are a byte range, and that is not only another spelling.
///
/// A range here denies *reading* as well, which `flock` does not, and the
/// bytes of this file are what say which checkout holds the claim. Locked
/// from zero, as this was, the one thing no other Obelus could read was the
/// holder's own writing. So the range is a single byte past any end this
/// file could have: it names the file as surely as the whole of it does,
/// and it is nowhere near what anybody reads.
///
/// Broken deliberately by locking from zero again: `held` comes back with
/// the claim in it and nothing said about where, and the status row says
/// `in another window` for a worktree it could have named. Only Windows
/// shows it.
#[cfg(windows)]
pub fn held_by_somebody_else(file: &File) -> bool {
    use std::os::windows::io::AsRawHandle as _;

    use windows_sys::Win32::{
        Storage::FileSystem::{LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LockFileEx},
        System::IO::OVERLAPPED,
    };

    // Safety: the handle is this file's and outlives the call, and the
    // overlapped structure is this stack frame's, written here and by the
    // call, and read by nobody after it.
    unsafe {
        let mut overlapped: OVERLAPPED = std::mem::zeroed();
        // The offset, which is where this platform keeps it. Four gigabytes
        // short of the top so that the byte asked for below is still an
        // offset, and further out than any filesystem will go.
        overlapped.Anonymous.Anonymous.OffsetHigh = u32::MAX;
        LockFileEx(
            file.as_raw_handle(),
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            1,
            0,
            &raw mut overlapped,
        ) == 0
    }
}

/// Takes the lock on `file`, waiting for whoever has it to let go.
///
/// For a lock that is handed over rather than refused: the one asking has
/// told the holder it wants it, and waits here for the kernel to wake it
/// when the holder lets go -- or when the holder's process ends, which lets
/// go of everything it held. Nothing is asked again and again: the waiting
/// is the kernel's. On a thread of its own, since it can be a while.
///
/// `false` where the lock could not be taken at all, which is not the same
/// as somebody holding it.
#[cfg(unix)]
pub fn wait_to_hold(file: &File) -> bool {
    use std::os::fd::AsRawFd as _;

    // Safety: as above.
    unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) == 0 }
}

/// The same, on the byte the other platform's lock is taken on -- see
/// [`held_by_somebody_else`].
#[cfg(windows)]
pub fn wait_to_hold(file: &File) -> bool {
    use std::os::windows::io::AsRawHandle as _;

    use windows_sys::Win32::{
        Storage::FileSystem::{LOCKFILE_EXCLUSIVE_LOCK, LockFileEx},
        System::IO::OVERLAPPED,
    };

    // Safety: as above.
    unsafe {
        let mut overlapped: OVERLAPPED = std::mem::zeroed();
        overlapped.Anonymous.Anonymous.OffsetHigh = u32::MAX;
        LockFileEx(
            file.as_raw_handle(),
            LOCKFILE_EXCLUSIVE_LOCK,
            0,
            1,
            0,
            &raw mut overlapped,
        ) != 0
    }
}

/// A key nobody else has, for a door.
///
/// From the hasher std seeds with randomness for every map, twice: enough
/// for a word only this reader's own files say.
#[must_use]
pub fn a_key() -> String {
    use std::hash::{BuildHasher as _, Hasher as _};

    let half = || {
        let mut hasher = std::hash::RandomState::new().build_hasher();
        hasher.write_u32(std::process::id());
        hasher.finish()
    };
    format!("{:016x}{:016x}", half(), half())
}
