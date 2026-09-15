//! Noticing that a file changed on disk.
//!
//! An agent rewriting a file while it is open is the normal case, not the
//! exceptional one, so obelus reloads by itself rather than asking. Most of
//! the work here is the edge cases: without them this arrives as "sometimes it
//! does not refresh", which is the hardest kind of bug to be told about.
//!
//! This is a *freshness* mechanism and not a correctness one, and nothing
//! that matters may be hung on it. Events are dropped when the kernel's
//! queue overflows, never arrive at all on NFS and several container
//! mounts, and cannot say anything about what happened before obelus
//! started. Missing one has to mean "the screen is a moment out of date",
//! never "somebody's work was written over": the question of whether a save
//! would go over somebody else's change is asked of disk at the moment of
//! saving, in [`crate::buffer::Buffer::conflicted`].

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::mpsc::{self, RecvTimeoutError, Sender},
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result};
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};

use crate::event::Event;

/// How long to gather changes before reporting them.
///
/// One save arrives as several events — a create, one or more writes, a
/// close, sometimes a rename — and reloading on each would reparse the same
/// file three times and, worse, read it while it is half-written.
const DEBOUNCE: Duration = Duration::from_millis(80);

/// Watches the directories holding open files.
pub struct Watcher {
    inner: RecommendedWatcher,
    /// The directories being watched.
    ///
    /// A set rather than a reference count because nothing can stop watching
    /// yet: M0 has no way to close a buffer. When it has one, this becomes a
    /// count and the last buffer out of a directory takes the watch with it.
    directories: HashMap<PathBuf, usize>,
}

impl std::fmt::Debug for Watcher {
    /// `RecommendedWatcher` has no `Debug`, and the interesting part is which
    /// directories are covered.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Watcher")
            .field("directories", &self.directories)
            .finish_non_exhaustive()
    }
}

impl Watcher {
    /// Starts watching, reporting changes on `sender`.
    pub fn new(sender: Sender<Event>) -> Result<Self> {
        let (raw_sender, raw_receiver) = mpsc::channel::<PathBuf>();

        let inner = RecommendedWatcher::new(
            move |result: notify::Result<notify::Event>| match result {
                Ok(event) => {
                    // Access events say nothing changed, and they arrive
                    // whenever anything reads the file — including obelus.
                    if matches!(event.kind, EventKind::Access(_)) {
                        return;
                    }
                    for path in event.paths {
                        // The receiver is gone, so the debounce thread has
                        // ended and there is nothing left to tell.
                        if raw_sender.send(path).is_err() {
                            return;
                        }
                    }
                }
                Err(error) => tracing::warn!(%error, "watching for file changes"),
            },
            Config::default(),
        )
        .context("starting the file watcher")?;

        spawn_debouncer(raw_receiver, sender);

        Ok(Self {
            inner,
            directories: HashMap::new(),
        })
    }

    /// Watches the directory a file lives in.
    ///
    /// The directory, not the file. Editors and most tools save by writing a
    /// temporary file and renaming it over the original, which replaces the
    /// inode; a watch on the file itself survives exactly one save and then
    /// silently watches something nothing will ever write to again. Watching
    /// the directory and filtering by path is the only arrangement that keeps
    /// working.
    pub fn watch(&mut self, path: &Path) -> Result<()> {
        let Some(directory) = directory_of(path) else {
            return Ok(());
        };
        self.watch_directory(&directory)
    }

    /// Watches a directory itself, for whatever turns up in it.
    ///
    /// [`Watcher::watch`] is about one file and watches the directory
    /// holding it, which is how a rename into place is heard. This is about
    /// the directory: what is wanted from it is every file it has and every
    /// file it is about to have, and the one the reader will ask for next
    /// may not be there yet.
    pub fn watch_directory(&mut self, directory: &Path) -> Result<()> {
        // Counted, not just remembered: two open files in one directory are
        // one watch, and closing the first of them must not take the watch
        // away from the second.
        let count = self.directories.entry(directory.to_path_buf()).or_default();
        *count += 1;
        if *count > 1 {
            return Ok(());
        }
        self.inner
            .watch(directory, RecursiveMode::NonRecursive)
            .with_context(|| format!("watching {}", directory.display()))
    }

    /// Gives up the watch one file needed, if nothing else needs it.
    ///
    /// Failure is ignored: the watch going away is the point, and a watch
    /// that will not go away costs a wakeup for a directory nobody is
    /// reading -- not a wrong answer.
    pub fn unwatch(&mut self, path: &Path) {
        let Some(directory) = directory_of(path) else {
            return;
        };
        self.unwatch_directory(&directory);
    }

    /// The same, for a directory watched as itself.
    pub fn unwatch_directory(&mut self, directory: &Path) {
        let directory = directory.to_path_buf();
        let Some(count) = self.directories.get_mut(&directory) else {
            return;
        };
        *count -= 1;
        if *count > 0 {
            return;
        }
        self.directories.remove(&directory);
        if let Err(error) = self.inner.unwatch(&directory) {
            tracing::debug!(%error, directory = %directory.display(), "not unwatched");
        }
    }
}

/// The directory a path lives in, as a watch needs it.
///
/// One place, because `watch` and `unwatch` disagreeing about what a path's
/// directory is would leak a watch or drop a live one.
fn directory_of(path: &Path) -> Option<PathBuf> {
    let directory = path.parent()?;
    // An empty parent means a bare filename, so the current directory.
    Some(if directory.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        directory.to_path_buf()
    })
}

/// Runs the thread that gathers changes and reports them once.
///
/// The deadline is set by the *first* change in a burst, not refreshed by each
/// one: a file being written continuously would otherwise never be reported at
/// all.
fn spawn_debouncer(raw: mpsc::Receiver<PathBuf>, sender: Sender<Event>) {
    std::thread::Builder::new()
        .name("obelus-watch".to_string())
        .spawn(move || {
            let mut pending: HashMap<PathBuf, ()> = HashMap::new();
            let mut deadline: Option<Instant> = None;

            loop {
                let received = match deadline {
                    None => raw.recv().map_err(|_| RecvTimeoutError::Disconnected),
                    Some(at) => raw.recv_timeout(at.saturating_duration_since(Instant::now())),
                };

                match received {
                    Ok(path) => {
                        pending.insert(path, ());
                        deadline.get_or_insert_with(|| Instant::now() + DEBOUNCE);
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        deadline = None;
                        for (path, ()) in pending.drain() {
                            if sender.send(Event::FileChanged { path }).is_err() {
                                return;
                            }
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        })
        .expect("spawning the watch thread");
}
