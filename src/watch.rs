//! Noticing that a file changed on disk.
//!
//! An agent rewriting a file while it is open is the normal case, not the
//! exceptional one, so obelus reloads by itself rather than asking. Most of
//! the work here is the edge cases: without them this arrives as "sometimes it
//! does not refresh", which is the hardest kind of bug to be told about.

use std::{
    collections::{HashMap, HashSet},
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
    directories: HashSet<PathBuf>,
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
            directories: HashSet::new(),
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
        let Some(directory) = path.parent() else {
            return Ok(());
        };
        // An empty parent means a bare filename, so the current directory.
        let directory = if directory.as_os_str().is_empty() {
            Path::new(".")
        } else {
            directory
        };

        if !self.directories.insert(directory.to_path_buf()) {
            return Ok(());
        }
        self.inner
            .watch(directory, RecursiveMode::NonRecursive)
            .with_context(|| format!("watching {}", directory.display()))
    }
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
