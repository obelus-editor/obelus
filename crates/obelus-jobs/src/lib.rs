//! How many build jobs run at once, across every Obelus on this machine.
//!
//! Several Obelus processes is the normal case, and each of them starts
//! things that compile: an agent's shell, a language server's `cargo check`,
//! the reader's own terminal. Every compiler sizes itself to the whole
//! machine and none of them knows about the others, so three of them at once
//! is three machines' worth of jobs on one -- the memory and the disk filled,
//! and all of it finishing later than one after another would have.
//!
//! The answer is not Obelus's own. It is GNU make's jobserver: a pool of
//! tokens, where a job takes one before it starts and puts it back when it
//! ends. Cargo, rustc, the `cc` crate, make from 4.4 and ninja from 1.13
//! already speak it, and find the pool through the environment -- so all
//! Obelus does is make the pool and tell everything it starts where it is.
//! Nothing here runs anybody's build, decides which command is one, or asks
//! an agent to do anything differently.
//!
//! What it costs is that a pool *replaces* a cargo's own `-j` and its
//! `build.jobs`: a program handed one takes the pool's size as the answer,
//! which is the point -- the budget is the machine's, not the invocation's.
//!
//! A pool is a named pipe on unix and a named semaphore on Windows, which
//! are what the protocol is written in on each, and either lasts exactly as
//! long as something holds it open. So every Obelus in a pool holds it, and
//! which of them fills it is decided by a lock: an Obelus that finds nobody
//! holding the members' lock is alone, and makes the pool afresh. That is
//! also what puts back a token a killed compiler took with it -- the next
//! Obelus to start alone starts full. And the last to leave takes the name
//! away, because a program handed the address of a pool nobody keeps would
//! wait on tokens nobody puts back, where a name that is not there makes it
//! fall back on its own `-j`.

use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
    sync::RwLock,
};

/// The lock every change to the pool is made under, briefly.
const SETUP: &str = "setup.lock";

/// The lock every Obelus in the pool holds shared, for as long as it is in.
///
/// Apart from [`SETUP`] because this one is held for the whole of a session
/// and that one for the moment of a change: one file would make every
/// change wait on every member leaving.
const MEMBERS: &str = "members.lock";

/// Where the pool is and how many tokens were put in it.
const RECORD: &str = "pool";

/// What every program Obelus starts is told, while this Obelus is in a pool.
///
/// A global, like the glyph switch: it is one fact the whole program
/// shares, and threading it through would put a parameter on every place a
/// program is started -- an agent, a language server, a terminal, a command
/// an agent asked for -- rather than on the pool.
///
/// One entry per [`Pool`] held, newest last, rather than one answer: a
/// process with two of them -- a test binary is one -- would otherwise have
/// the first to leave take the answer away from the one still in. Each is
/// where its pool is; what that comes to as an environment is worked out
/// when a program is started, because part of it -- whether make can read
/// it -- is asked on a thread and may not be known yet.
static LENT: RwLock<Vec<String>> = RwLock::new(Vec::new());

/// What a program Obelus starts is to be told about the pool, as names and
/// values for its environment.
///
/// Nothing at all outside a pool, rather than anything saying there is
/// none: a reader who set `MAKEFLAGS` themselves, or started Obelus from a
/// make, has a pool of their own, and that is what a program gets.
#[must_use]
pub fn lent() -> Vec<(&'static str, String)> {
    LENT.read()
        .ok()
        .and_then(|lent| lent.last().map(|auth| told(auth)))
        .unwrap_or_default()
}

/// Tells a program about to be started where the pool is.
pub fn lend(command: &mut std::process::Command) -> &mut std::process::Command {
    for (name, value) in lent() {
        command.env(name, value);
    }
    command
}

/// The environment for a pool at this address.
///
/// `CARGO_MAKEFLAGS` always: cargo looks there first, and nothing else
/// does, so it changes nothing but cargo. `MAKEFLAGS` only for a make that
/// can read it, because a make older than 4.4 cannot read a named pipe's
/// address and does not ignore one -- it stops, and its message says
/// nothing about Obelus. Ninja reads `MAKEFLAGS` too, and an old one
/// ignores it.
///
/// Leading with `-j`, which is how make writes it for the makes it starts.
fn told(auth: &str) -> Vec<(&'static str, String)> {
    let flags = format!("-j --jobserver-auth={auth}");
    let mut told = vec![("CARGO_MAKEFLAGS", flags.clone())];
    if MAKE_READS.get().copied().unwrap_or(false) {
        told.push(("MAKEFLAGS", flags));
    }
    told
}

/// Whether the make on this machine reads a pool by its name, once that
/// has been asked.
static MAKE_READS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

/// Asks whether the make on this machine reads a pool by its name, on a
/// thread, once.
///
/// Not where a pool is joined, which is the first thing a starting Obelus
/// does, in front of its first screen and with every other Obelus's
/// changes waiting on `SETUP`: finding a program and starting it is
/// milliseconds here and tens of them on Windows. Until the answer is in, a
/// program is told only `CARGO_MAKEFLAGS` -- a language server started on
/// the way up, which is cargo's anyway.
///
/// Asked of the make Obelus finds: the one a shell finds may be another,
/// which is the cost of asking at all.
fn ask_about_make() {
    static ASKED: std::sync::Once = std::sync::Once::new();
    ASKED.call_once(|| {
        obelus_runtime::handle().spawn_blocking(|| {
            MAKE_READS.get_or_init(|| {
                let Some(make) = obelus_program::found("make") else {
                    return false;
                };
                if known_too_old(&make, cfg!(target_os = "macos")) {
                    return false;
                }
                let said = obelus_program::without_a_window(&mut std::process::Command::new(make))
                    .arg("--version")
                    .output();
                let reads = said.is_ok_and(|said| {
                    make_version(&String::from_utf8_lossy(&said.stdout))
                        .is_some_and(|at| at >= (4, 4))
                });
                tracing::info!(reads, "whether make reads a pool by its name");
                reads
            });
        });
    });
}

/// Whether a make is one whose answer is known without running it.
///
/// Apple's: GNU make became GPLv3 at 3.82 and Apple ships no GPLv3, so the
/// make in `/usr/bin` on a Mac has been 3.81 for as long as there has been
/// one -- and without the command line tools it is a stub, and running it
/// puts up a dialog offering to install them, at every start. A make of the
/// reader's own, a Homebrew one first on `PATH`, is somewhere else and is
/// asked like any other.
fn known_too_old(make: &Path, on_a_mac: bool) -> bool {
    on_a_mac && make == Path::new("/usr/bin/make")
}

/// The version GNU make says it is, from what `make --version` printed.
fn make_version(said: &str) -> Option<(u32, u32)> {
    let version = said.lines().next()?.strip_prefix("GNU Make ")?;
    let mut parts = version.split('.');
    let major = parts.next()?.trim().parse().ok()?;
    let minor = parts.next().map_or(Some(0), |minor| {
        minor
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .ok()
    })?;
    Some((major, minor))
}

/// This Obelus's place in the machine's pool, which it gives up by being
/// dropped.
#[derive(Debug)]
pub struct Pool {
    /// Where the pool's locks and record are.
    directory: PathBuf,
    /// Held shared for as long as this is: see [`MEMBERS`].
    members: File,
    /// The pool itself, held open so that it lasts.
    channel: channel::Channel,
    /// Where the pool is, as the protocol writes it.
    auth: String,
}

impl Pool {
    /// Joins the pool kept in `directory`, making it if nobody else is in
    /// one, and makes it `jobs` jobs at once.
    ///
    /// # Errors
    ///
    /// Where the directory, its locks or the pool cannot be made or opened.
    /// What to do then is the caller's: Obelus goes on without a pool, and
    /// every build sizes itself the way it did before there was one.
    pub fn join(directory: &Path, jobs: usize) -> io::Result<Self> {
        ask_about_make();
        std::fs::create_dir_all(directory)?;
        let setup = lock_file(&directory.join(SETUP))?;
        setup.lock()?;
        let members = lock_file(&directory.join(MEMBERS))?;
        // Asked by trying to take it outright, which only works where
        // nobody holds it at all: the one question a lock can answer about
        // who else is there. Under `SETUP`, so nobody can arrive between
        // the answer and acting on it.
        let alone = members.try_lock().is_ok();
        let (auth, channel) = match alone {
            true => {
                members.unlock()?;
                channel::sweep(directory);
                let (auth, channel) = channel::make(directory)?;
                write_record(
                    directory,
                    &Record {
                        auth: auth.clone(),
                        tokens: 0,
                    },
                )?;
                (auth, channel)
            }
            false => {
                let record = read_record(directory).ok_or_else(|| {
                    io::Error::other("a pool somebody is in says nothing about where it is")
                })?;
                let channel = channel::open(&record.auth)?;
                (record.auth, channel)
            }
        };
        members.lock_shared()?;
        // Settled before there is a `Pool` to drop: one that failed here
        // would be dropped while `setup` is still held, and leaving waits
        // for `SETUP` through a file of its own -- which this process
        // already has locked, so it would wait for ever.
        settle(directory, &channel, &auth, jobs)?;
        let pool = Self {
            directory: directory.to_path_buf(),
            members,
            channel,
            auth,
        };
        if let Ok(mut lent) = LENT.write() {
            lent.push(pool.auth.clone());
        }
        tracing::info!(
            auth = pool.auth,
            alone,
            jobs,
            "in the machine's pool of build jobs"
        );
        Ok(pool)
    }

    /// Makes the pool `jobs` jobs at once.
    ///
    /// Every Obelus is told when the settings change, and every one of them
    /// asks for this. The record is what keeps that to one change: the
    /// first to ask makes it and writes the new size down, and the rest
    /// find nothing left to do.
    ///
    /// # Errors
    ///
    /// Where the lock or the pool cannot be reached.
    pub fn resize(&self, jobs: usize) -> io::Result<()> {
        let setup = lock_file(&self.directory.join(SETUP))?;
        setup.lock()?;
        settle(&self.directory, &self.channel, &self.auth, jobs)
    }

    /// Gives up this Obelus's place in the pool, and the pool with it if
    /// nobody else is in.
    ///
    /// Everything leaving does is done with [`SETUP`] held, the members'
    /// lock let go of included: the last one out takes it outright to find
    /// out that it is the last, and held past `SETUP` it would say to the
    /// next one in that somebody is still here -- in a pool whose record
    /// has just been taken away.
    fn leave(&self) {
        if let Ok(mut lent) = LENT.write()
            && let Some(at) = lent.iter().rposition(|auth| *auth == self.auth)
        {
            lent.remove(at);
        }
        let Ok(setup) = lock_file(&self.directory.join(SETUP)) else {
            return;
        };
        if setup.lock().is_err() {
            return;
        }
        let _ = self.members.unlock();
        // The last one out takes the name with it -- see the module's note
        // on why a name nobody keeps is worse than none.
        if self.members.try_lock().is_ok() {
            channel::forget(&self.auth);
            let _ = std::fs::remove_file(self.directory.join(RECORD));
            let _ = self.members.unlock();
            tracing::info!("the last out of the machine's pool of build jobs");
        }
    }
}

/// Puts tokens into a pool or takes them out until it is `jobs` jobs at
/// once. Asked with [`SETUP`] held.
///
/// One fewer token than jobs, because every program in a pool has one of
/// its own that it never puts in: a cargo in an empty pool still builds,
/// one job at a time.
fn settle(directory: &Path, channel: &channel::Channel, auth: &str, jobs: usize) -> io::Result<()> {
    let wanted = jobs.saturating_sub(1);
    let had = read_record(directory).map_or(0, |record| record.tokens);
    match wanted.cmp(&had) {
        std::cmp::Ordering::Equal => return Ok(()),
        std::cmp::Ordering::Greater => channel.give(wanted - had)?,
        std::cmp::Ordering::Less => channel.take(had - wanted, auth)?,
    }
    write_record(
        directory,
        &Record {
            auth: auth.to_string(),
            tokens: wanted,
        },
    )
}

impl Pool {
    /// Lets go the way an Obelus that was killed does: the members' lock
    /// goes with the process, and nothing is tidied.
    #[cfg(test)]
    fn die(self) {
        // And the process with it, which takes what it told its programs.
        if let Ok(mut lent) = LENT.write() {
            lent.pop();
        }
        let this = std::mem::ManuallyDrop::new(self);
        // Safety: read once, out of a value that is never dropped, so the
        // file is closed exactly once -- here.
        drop(unsafe { std::ptr::read(&raw const this.members) });
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.leave();
    }
}

/// Opens a file that is only ever locked, never read.
fn lock_file(path: &Path) -> io::Result<File> {
    File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
}

/// What the pool's record says.
#[derive(Debug, PartialEq, Eq)]
struct Record {
    /// Where the pool is, as the protocol writes it.
    auth: String,
    /// How many tokens are meant to be going round.
    tokens: usize,
}

fn read_record(directory: &Path) -> Option<Record> {
    let text = std::fs::read_to_string(directory.join(RECORD)).ok()?;
    let mut lines = text.lines();
    let auth = lines.next()?.to_string();
    let tokens = lines.next()?.parse().ok()?;
    Some(Record { auth, tokens })
}

fn write_record(directory: &Path, record: &Record) -> io::Result<()> {
    std::fs::write(
        directory.join(RECORD),
        format!("{}\n{}\n", record.auth, record.tokens),
    )
}

/// Somewhere no other pool was ever made, for a name of its own.
///
/// A pool made afresh gets a new name rather than the old one's, because
/// a compiler still running from before can be holding the old one open,
/// and a name it shares with the new pool would be its tokens counted
/// twice.
fn fresh_name() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    format!("obelus-jobs-{}-{now}", std::process::id())
}

#[cfg(unix)]
mod channel {
    //! The pool as a named pipe, which is a byte per token.

    use std::{
        ffi::CString,
        fs::File,
        io::{self, Read as _, Write as _},
        os::unix::{ffi::OsStrExt as _, fs::OpenOptionsExt as _},
        path::Path,
    };

    /// The pipe, held open for reading and writing.
    ///
    /// Both, because a pipe opened one way waits for somebody to open it
    /// the other -- and because what keeps the bytes in a pipe is somebody
    /// having it open, which is the whole of why this is held.
    #[derive(Debug)]
    pub(crate) struct Channel {
        held: File,
    }

    fn path_of(auth: &str) -> io::Result<&Path> {
        auth.strip_prefix("fifo:")
            .map(Path::new)
            .ok_or_else(|| io::Error::other(format!("not a named pipe: {auth}")))
    }

    fn opened(path: &Path, waiting: bool) -> io::Result<File> {
        let mut options = File::options();
        options.read(true).write(true);
        if !waiting {
            options.custom_flags(libc::O_NONBLOCK);
        }
        options.open(path)
    }

    pub(crate) fn make(directory: &Path) -> io::Result<(String, Channel)> {
        let path = directory.join(super::fresh_name());
        // Refused rather than made: the flags a pool is handed in are split
        // on spaces by everything that reads them, so a path with one in it
        // is a pool no program could find -- and one that half-found it
        // would be told it is somewhere else.
        if path
            .as_os_str()
            .as_bytes()
            .iter()
            .any(u8::is_ascii_whitespace)
        {
            return Err(io::Error::other(format!(
                "a pool cannot be kept at {}, which has a space in it",
                path.display()
            )));
        }
        let named = CString::new(path.as_os_str().as_bytes()).map_err(io::Error::other)?;
        // Safety: `mkfifo` reads the name it is given, which is this
        // frame's and ends in the nul `CString` put there.
        if unsafe { libc::mkfifo(named.as_ptr(), 0o600) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let auth = format!(
            "fifo:{}",
            path.to_str()
                .ok_or_else(|| io::Error::other("a pool's path the environment cannot carry"))?
        );
        let channel = open(&auth)?;
        Ok((auth, channel))
    }

    pub(crate) fn open(auth: &str) -> io::Result<Channel> {
        Ok(Channel {
            held: opened(path_of(auth)?, true)?,
        })
    }

    /// Takes away every pipe a pool was ever kept in here, for one made
    /// afresh by somebody who is alone.
    ///
    /// Every one, not only the one the record names: an Obelus that died
    /// between making a pipe and writing it down left one the record never
    /// named. A compiler still holding an old one open keeps it, because
    /// what goes is the name and not the pipe.
    pub(crate) fn sweep(directory: &Path) {
        let Ok(entries) = std::fs::read_dir(directory) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            if entry.file_name().as_bytes().starts_with(b"obelus-jobs-")
                && let Err(error) = std::fs::remove_file(entry.path())
            {
                tracing::warn!(%error, path = %entry.path().display(), "a pool's name outlived it");
            }
        }
    }

    pub(crate) fn forget(auth: &str) {
        if let Ok(path) = path_of(auth)
            && let Err(error) = std::fs::remove_file(path)
            && error.kind() != io::ErrorKind::NotFound
        {
            tracing::warn!(%error, auth, "a pool's name outlived it");
        }
    }

    impl Channel {
        pub(crate) fn give(&self, tokens: usize) -> io::Result<()> {
            (&self.held).write_all(&vec![b'+'; tokens])
        }

        /// Takes tokens out of the pool: what is in it now at once, and
        /// what is out with a build as each comes back.
        ///
        /// Waited for rather than owed, because the tokens a build holds
        /// are not in the pipe to take. The waiting is a read the kernel
        /// wakes, on the runtime the waiting is done on; and a size put
        /// back up meanwhile is not undone by it, because what is taken
        /// and what is put in are counted against one total.
        pub(crate) fn take(&self, tokens: usize, auth: &str) -> io::Result<()> {
            let path = path_of(auth)?;
            let mut now = vec![0; tokens];
            let taken = match opened(path, false)?.read(&mut now) {
                Ok(taken) => taken,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => 0,
                Err(error) => return Err(error),
            };
            let owed = tokens - taken;
            if owed > 0 {
                let mut later = opened(path, true)?;
                obelus_runtime::handle().spawn_blocking(move || {
                    let mut back = vec![0; owed];
                    if let Err(error) = later.read_exact(&mut back) {
                        tracing::warn!(%error, "the pool's tokens did not all come back");
                    }
                });
            }
            Ok(())
        }
    }
}

#[cfg(windows)]
mod channel {
    //! The pool as a named semaphore, whose count is the tokens.

    use std::{io, path::Path};

    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_OBJECT_0},
        System::Threading::{
            CreateSemaphoreW, INFINITE, OpenSemaphoreW, ReleaseSemaphore, SEMAPHORE_MODIFY_STATE,
            SYNCHRONIZATION_SYNCHRONIZE, WaitForSingleObject,
        },
    };

    /// A handle on the semaphore, which lasts as long as one does.
    #[derive(Debug)]
    pub(crate) struct Channel {
        pub(crate) held: HANDLE,
    }

    // Safety: a handle is the kernel's, and any thread may use it.
    unsafe impl Send for Channel {}
    // Safety: as above; the calls on it are the kernel's to serialise.
    unsafe impl Sync for Channel {}

    impl Drop for Channel {
        fn drop(&mut self) {
            // Safety: the handle is this one's, and nothing uses it after.
            unsafe { CloseHandle(self.held) };
        }
    }

    fn wide(name: &str) -> Vec<u16> {
        name.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub(crate) fn make(_directory: &Path) -> io::Result<(String, Channel)> {
        let name = super::fresh_name();
        let named = wide(&name);
        // Safety: the name is this frame's and ends in a nul; no attributes.
        let held = unsafe { CreateSemaphoreW(std::ptr::null(), 0, i32::MAX, named.as_ptr()) };
        if held.is_null() {
            return Err(io::Error::last_os_error());
        }
        // A name somebody already has is their semaphore and their count,
        // handed back as though it were new. Never, with a name this fresh;
        // and a pool counted twice if it were.
        //
        // Safety: asked straight after the call it is about.
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            // Safety: the handle is the one just opened, and nothing else has
            // it.
            unsafe { CloseHandle(held) };
            return Err(io::Error::other(format!(
                "a pool called {name} already exists"
            )));
        }
        Ok((name, Channel { held }))
    }

    pub(crate) fn open(auth: &str) -> io::Result<Channel> {
        let named = wide(auth);
        // Safety: as above.
        let held = unsafe {
            OpenSemaphoreW(
                SYNCHRONIZATION_SYNCHRONIZE | SEMAPHORE_MODIFY_STATE,
                0,
                named.as_ptr(),
            )
        };
        if held.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Channel { held })
    }

    /// Nothing to take away: a semaphore goes with its last handle.
    pub(crate) fn forget(_auth: &str) {}

    /// Nothing to take away here either, for the same reason.
    pub(crate) fn sweep(_directory: &Path) {}

    impl Channel {
        pub(crate) fn give(&self, tokens: usize) -> io::Result<()> {
            let tokens = i32::try_from(tokens).map_err(io::Error::other)?;
            // Safety: the handle is this one's; the previous count is not
            // asked for.
            match unsafe { ReleaseSemaphore(self.held, tokens, std::ptr::null_mut()) } {
                0 => Err(io::Error::last_os_error()),
                _ => Ok(()),
            }
        }

        /// The same as the other platform's: what is there now at once,
        /// and the rest as builds give it back.
        pub(crate) fn take(&self, tokens: usize, auth: &str) -> io::Result<()> {
            let mut taken = 0;
            // Safety: the handle is this one's.
            while taken < tokens && unsafe { WaitForSingleObject(self.held, 0) } == WAIT_OBJECT_0 {
                taken += 1;
            }
            let owed = tokens - taken;
            if owed > 0 {
                let later = open(auth)?;
                obelus_runtime::handle().spawn_blocking(move || {
                    // The whole of it, not the field a closure would take
                    // on its own: the handle is only `Send` inside it.
                    let later = later;
                    for _ in 0..owed {
                        // Safety: the handle is `later`'s, which this owns.
                        unsafe { WaitForSingleObject(later.held, INFINITE) };
                    }
                });
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Pool, read_record};

    /// One at a time, because what a program is told is one global and
    /// every pool sets it.
    fn turn() -> std::sync::MutexGuard<'static, ()> {
        static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
        TURN.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn scratch(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("obelus-jobs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        directory
    }

    /// What `make --version` prints is read for its version.
    ///
    /// Deliberate break: require a minor, and a make that says only `4`
    /// is read as no make at all.
    #[test]
    fn a_make_says_its_version() {
        assert_eq!(
            super::make_version("GNU Make 4.4.1\nBuilt for x86_64"),
            Some((4, 4))
        );
        assert_eq!(super::make_version("GNU Make 3.81\n"), Some((3, 81)));
        assert_eq!(super::make_version("GNU Make 4\n"), Some((4, 0)));
        assert_eq!(super::make_version("bmake 20240711\n"), None);
    }

    /// Whether make can read a pool is asked once a pool is joined, and
    /// answered -- whatever the answer is on this machine, which is not
    /// this test's to know.
    ///
    /// Deliberate break: take `ask_about_make` out of `join`, and nothing
    /// ever asks, so no program is ever told `MAKEFLAGS`.
    #[test]
    fn whether_make_can_read_a_pool_is_asked() {
        let _turn = turn();
        let directory = scratch("make");
        let pool = Pool::join(&directory, 2).expect("in");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while super::MAKE_READS.get().is_none() {
            assert!(
                std::time::Instant::now() < deadline,
                "nobody asked whether make can read a pool"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        drop(pool);
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A pipe nobody's record names -- left by an Obelus that died between
    /// making it and writing it down -- goes when the next one in is alone.
    ///
    /// Deliberate break: take only the pipe the record names, as `join`
    /// did, and the stray one stays for ever.
    #[cfg(unix)]
    #[test]
    fn a_pipe_nobody_wrote_down_is_taken_away() {
        let _turn = turn();
        let directory = scratch("stray");
        std::fs::create_dir_all(&directory).expect("the directory");
        let stray = directory.join("obelus-jobs-1-1");
        std::fs::write(&stray, "").expect("a stray");
        let pool = Pool::join(&directory, 2).expect("in");
        assert!(
            !stray.exists(),
            "the stray pipe outlived a pool made afresh"
        );
        drop(pool);
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A pool is not kept where its path would have a space in it, which
    /// every program that reads the flags would split in two.
    ///
    /// Deliberate break: take the check out of `make`, and the join
    /// succeeds with an address nobody can read.
    #[cfg(unix)]
    #[test]
    fn a_pool_is_not_kept_where_a_space_would_split_it() {
        let _turn = turn();
        let directory = scratch("with space");
        assert!(Pool::join(&directory, 2).is_err());
        assert!(super::lent().is_empty(), "{:?}", super::lent());
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// Apple's own make is known to be too old without being run, and any
    /// other is asked.
    ///
    /// Deliberate break: answer `false` in `known_too_old` whatever it is
    /// given, and the first assertion fails -- which on a Mac without the
    /// command line tools is a dialog at every start.
    #[test]
    fn apples_make_is_not_asked() {
        let apple = std::path::Path::new("/usr/bin/make");
        assert!(super::known_too_old(apple, true));
        assert!(!super::known_too_old(apple, false), "only on a Mac");
        assert!(!super::known_too_old(
            std::path::Path::new("/opt/homebrew/opt/make/libexec/gnubin/make"),
            true
        ));
    }

    /// The first in fills the pool; the second finds it full and adds
    /// nothing; and the last out takes it away.
    ///
    /// Deliberate breaks: have a join that is not alone count the pool as
    /// empty, and the second in doubles what the first put in; or never
    /// take the name away, and the last assertion fails.
    #[test]
    fn the_first_in_fills_the_pool_and_the_last_out_empties_it() {
        let _turn = turn();
        let directory = scratch("fills");
        let first = Pool::join(&directory, 4).expect("the first in");
        let record = read_record(&directory).expect("a record");
        assert_eq!(record.tokens, 3);
        assert_eq!(tokens_in(&record.auth), 3);

        let second = Pool::join(&directory, 4).expect("the second in");
        assert_eq!(second.auth, first.auth, "one pool, not two");
        assert_eq!(tokens_in(&record.auth), 3);

        drop(first);
        assert!(
            read_record(&directory).is_some(),
            "the pool went with somebody still in it"
        );
        drop(second);
        assert!(
            read_record(&directory).is_none(),
            "the pool outlived everybody in it"
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A pool resized by one member is the size everybody asked for, however
    /// many of them asked.
    ///
    /// Deliberate break: have `settle` add `wanted` rather than the
    /// difference, and the pool grows by every member's request.
    #[test]
    fn a_pool_is_resized_once_however_many_ask() {
        let _turn = turn();
        let directory = scratch("resized");
        let first = Pool::join(&directory, 4).expect("the first in");
        let second = Pool::join(&directory, 4).expect("the second in");
        first.resize(8).expect("grown");
        second.resize(8).expect("grown again");
        assert_eq!(tokens_in(&first.auth), 7);
        first.resize(2).expect("shrunk");
        second.resize(2).expect("shrunk again");
        assert_eq!(tokens_in(&first.auth), 1);
        drop((first, second));
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A pool nobody is in any more is made afresh, full, under a new name.
    ///
    /// Which is what puts back a token a killed build took: the members'
    /// lock went with whoever held it, and the next in is alone.
    ///
    /// Deliberate break: have `join` decide it is alone by whether the
    /// record exists, and a record left by a crash is joined as it was.
    #[test]
    fn a_pool_left_behind_is_made_again() {
        let directory = scratch("left");
        let _turn = turn();
        let first = Pool::join(&directory, 3).expect("the first in");
        let old = first.auth.clone();
        first.die();

        let again = Pool::join(&directory, 3).expect("in again");
        assert_ne!(again.auth, old);
        assert_eq!(tokens_in(&again.auth), 2);
        drop(again);
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A program started while Obelus is in a pool is told where it is, and
    /// one started after it has left is told nothing.
    ///
    /// Deliberate breaks: leave `LENT` alone in `drop`, and the last
    /// assertion fails; or empty it in `drop` whoever else holds a pool,
    /// and the one still in tells nobody.
    #[test]
    fn what_a_program_is_told_follows_the_pool() {
        let _turn = turn();
        let directory = scratch("told");
        let pool = Pool::join(&directory, 2).expect("in");
        let told = super::lent();
        let flags = told
            .iter()
            .find(|(name, _)| *name == "CARGO_MAKEFLAGS")
            .map(|(_, value)| value.clone())
            .expect("cargo is told");
        assert!(
            flags.ends_with(&format!("--jobserver-auth={}", pool.auth)),
            "{flags}"
        );
        let second = Pool::join(&directory, 2).expect("in twice");
        drop(pool);
        assert!(
            !super::lent().is_empty(),
            "the first out took the pool away from the second"
        );
        drop(second);
        assert!(super::lent().is_empty(), "{:?}", super::lent());
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A join that fails part-way says so, rather than waiting for ever on
    /// a lock its own process holds -- and takes nothing away from the
    /// Obelus already in.
    ///
    /// Made to fail by a record nobody may write: the second in finds the
    /// pool too small, puts tokens in and cannot write the new size down.
    /// Which is the disk being full, the case the pool exists for.
    ///
    /// Deliberate break: build the `Pool` before settling it in `join`, and
    /// this waits out its ten seconds.
    #[test]
    fn a_join_that_fails_says_so() {
        let _turn = turn();
        let directory = scratch("fails");
        let first = Pool::join(&directory, 1).expect("the first in");
        let record = directory.join(super::RECORD);
        let mut readonly = std::fs::metadata(&record)
            .expect("the record")
            .permissions();
        readonly.set_readonly(true);
        std::fs::set_permissions(&record, readonly).expect("read-only");
        // Somebody the permissions do not stop -- root, on a machine that
        // runs its tests as root -- writes it anyway, and there is no
        // failure to see.
        if std::fs::OpenOptions::new()
            .write(true)
            .open(&record)
            .is_ok()
        {
            drop(first);
            let _ = std::fs::remove_dir_all(&directory);
            return;
        }

        let (said, heard) = std::sync::mpsc::channel();
        let joining = directory.clone();
        std::thread::spawn(move || {
            let _ = said.send(Pool::join(&joining, 4).is_err());
        });
        let Ok(failed) = heard.recv_timeout(std::time::Duration::from_secs(10)) else {
            // The joiner waits for ever with `SETUP` held, and the first in
            // would wait behind it to leave: not dropped, so that this says
            // what went wrong rather than hanging too.
            std::mem::forget(first);
            panic!("a join that failed waited for ever");
        };
        assert!(failed, "a join whose record could not be written succeeded");
        assert!(
            !super::lent().is_empty(),
            "a join that failed took away what the first in tells its programs"
        );

        let mut writable = std::fs::metadata(&record)
            .expect("the record")
            .permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        writable.set_readonly(false);
        std::fs::set_permissions(&record, writable).expect("writable");
        drop(first);
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// The last one out lets go of everything before anybody else can come
    /// in: a lock still held past `SETUP` tells the next in that somebody
    /// is still here, in a pool whose record has gone.
    ///
    /// Deliberate break: take out the `unlock` after the record is removed
    /// in `leave`, and the members' lock is still held when it returns.
    #[test]
    fn the_last_out_lets_go_before_the_next_comes_in() {
        let _turn = turn();
        let directory = scratch("lets-go");
        let pool = Pool::join(&directory, 2).expect("in");
        pool.leave();
        let members = super::lock_file(&directory.join(super::MEMBERS)).expect("the lock");
        assert!(
            members.try_lock().is_ok(),
            "the last out still holds the members' lock"
        );
        drop(members);
        // Left already: what is left of it holds nothing.
        std::mem::forget(pool);
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// How many tokens are in a pool, counted by taking them all and
    /// putting them back.
    #[cfg(unix)]
    fn tokens_in(auth: &str) -> usize {
        use std::{
            io::{Read as _, Write as _},
            os::unix::fs::OpenOptionsExt as _,
        };
        let path = auth.strip_prefix("fifo:").expect("a named pipe");
        let mut pipe = std::fs::File::options()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)
            .expect("the pipe");
        let mut all = vec![0; 4096];
        let taken = pipe.read(&mut all).unwrap_or(0);
        pipe.write_all(&all[..taken]).expect("put back");
        taken
    }

    #[cfg(windows)]
    fn tokens_in(auth: &str) -> usize {
        use windows_sys::Win32::{
            Foundation::WAIT_OBJECT_0,
            System::Threading::{ReleaseSemaphore, WaitForSingleObject},
        };
        let channel = super::channel::open(auth).expect("the semaphore");
        let mut taken = 0;
        // Safety: the handle is `channel`'s, which outlives these calls.
        unsafe {
            while WaitForSingleObject(channel.held, 0) == WAIT_OBJECT_0 {
                taken += 1;
            }
            if taken > 0 {
                ReleaseSemaphore(channel.held, taken, std::ptr::null_mut());
            }
        }
        usize::try_from(taken).expect("a count")
    }
}
