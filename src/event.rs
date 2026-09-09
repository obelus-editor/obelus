//! What the main loop reacts to.
//!
//! Terminal input and background work arrive as the same type through the same
//! channel, so the loop has one handler and a new source — the file watcher
//! and the LSP client, later — is a sender and a variant rather than a second
//! code path.
//!
//! One channel with several producer threads is what makes obelus need no
//! async runtime: the main loop blocks on `recv`, which is exactly what a
//! reader with nothing else to do should be doing, and every source — the
//! keyboard, the file walker, the watcher, later a language server's output —
//! is a thread holding a `Sender`.
//!
//! Input is read by a thread blocking in `crossterm::event::read` rather than
//! through crossterm's `EventStream`, which cannot be drained: dropping a
//! `Next` future that has already been polled loses the wakeup and the stream
//! never yields again. Draining is the whole point — a held-down arrow key has
//! to cost one frame, not one frame per repeat — and `try_recv` on a channel
//! does it without touching a future at all.

use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
};

use crossterm::event::{Event as TerminalEvent, KeyEvent};

/// One thing the application has to react to.
#[derive(Clone, Debug)]
pub enum Event {
    /// A key was pressed.
    Key(KeyEvent),
    /// The terminal was resized.
    Resize,
    /// A batch of paths from the file walk.
    FilesFound {
        /// Which walk these came from, so batches from a walk whose picker has
        /// already closed can be dropped.
        generation: u64,
        /// The paths, relative to the walk's root.
        paths: Vec<PathBuf>,
    },
    /// A file on disk changed.
    ///
    /// Reported for everything in a watched directory, since the watch is on
    /// the directory. Whoever handles it decides whether the path is one of
    /// its own.
    FileChanged {
        /// Which file.
        path: PathBuf,
    },
}

impl Event {
    /// Translates a crossterm event, or `None` for one obelus ignores.
    ///
    /// Mouse, focus and paste events are dropped rather than stored: nothing
    /// reads them yet, and a variant nothing reads is indistinguishable from a
    /// broken feature.
    #[must_use]
    pub fn from_terminal(event: TerminalEvent) -> Option<Self> {
        match event {
            TerminalEvent::Key(key) => Some(Self::Key(key)),
            TerminalEvent::Resize(_, _) => Some(Self::Resize),
            TerminalEvent::FocusGained
            | TerminalEvent::FocusLost
            | TerminalEvent::Mouse(_)
            | TerminalEvent::Paste(_) => None,
        }
    }
}

/// The channel every event source sends on.
#[must_use]
pub fn channel() -> (Sender<Event>, Receiver<Event>) {
    mpsc::channel()
}

/// Starts the thread that reads the terminal.
///
/// The thread outlives the loop, still blocked in `read`. Nothing waits for
/// it: there is no way to interrupt that call, and the process is on its way
/// out by then.
pub fn spawn_terminal_reader(sender: Sender<Event>) {
    std::thread::Builder::new()
        .name("obelus-input".to_string())
        .spawn(move || {
            loop {
                match crossterm::event::read() {
                    Ok(event) => {
                        if let Some(event) = Event::from_terminal(event) {
                            // The receiver is gone, which means the loop has
                            // ended.
                            if sender.send(event).is_err() {
                                break;
                            }
                        }
                    }
                    Err(error) => {
                        tracing::error!(%error, "reading terminal input");
                        break;
                    }
                }
            }
        })
        .expect("spawning the input thread");
}
