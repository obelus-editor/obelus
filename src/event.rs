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
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::Duration,
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
    /// A message from a language server.
    Lsp {
        /// Which server it came from.
        language: crate::syntax::LanguageId,
        /// The message, still as JSON: what it means depends on what was
        /// asked for, and that is not the transport's business.
        message: serde_json::Value,
    },
    /// The wheel turned, by this many rows. Negative is up the file.
    ///
    /// A wheel is not an arrow key: it moves the *view*, and the place the
    /// reader had chosen stays where they put it. Which is only knowable
    /// because obelus asks the terminal to report the mouse -- without that
    /// the wheel arrives as arrow keys.
    Scroll(isize),
    /// Time passed, and something on screen moves with it.
    ///
    /// The only animated thing obelus has is the welcome screen's wordmark,
    /// and the ticker runs only while that is what is on screen. A reader
    /// looking at code gets no ticks at all: there is nothing to animate, and
    /// a redraw a reader did not ask for is a redraw that can only get in the
    /// way.
    Tick,
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
    fn from_terminal(event: TerminalEvent) -> Option<Self> {
        match event {
            TerminalEvent::Key(key) => Some(Self::Key(key)),
            TerminalEvent::Resize(_, _) => Some(Self::Resize),
            // Three rows a notch, which is what everything that scrolls text
            // does. The other mouse events are dropped: nothing reads them,
            // and a variant nothing reads is indistinguishable from a broken
            // feature.
            TerminalEvent::Mouse(mouse) => match mouse.kind {
                crossterm::event::MouseEventKind::ScrollDown => Some(Self::Scroll(3)),
                crossterm::event::MouseEventKind::ScrollUp => Some(Self::Scroll(-3)),
                _ => None,
            },
            TerminalEvent::FocusGained | TerminalEvent::FocusLost | TerminalEvent::Paste(_) => None,
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

/// How often the animated wordmark moves.
///
/// Twelve frames a second. A colour ramp sliding across five rows of block
/// elements needs no more than that, and every frame is a whole redraw.
const TICK: Duration = Duration::from_millis(80);

/// Whether this session is being read over a network.
///
/// Only a heuristic, and only the one that is worth having: the variables
/// below are set by sshd for the session it starts, so their presence means
/// the terminal is at the other end of a link. It says nothing about how fast
/// that link is, but the animation is worth exactly nothing over any of them
/// -- every frame is a screenful of escape sequences sent so that a logo can
/// shimmer -- so the heuristic being crude costs nothing either.
///
/// What it misses: a local terminal that got here some other way (mosh
/// without ssh, a serial console, a container's exec). Those keep the
/// animation, which is a wasted redraw and not a broken screen.
#[must_use]
pub fn remote() -> bool {
    remote_by(|name| std::env::var_os(name))
}

/// The same, asking `lookup` instead of the environment.
///
/// Split out so the rule can be tested: setting an environment variable is
/// `unsafe` in this edition and a data race against every other thread in the
/// test binary, which is a high price for four lines of logic.
fn remote_by(mut lookup: impl FnMut(&str) -> Option<std::ffi::OsString>) -> bool {
    ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY"]
        .iter()
        // Set but empty counts as unset: a shell that exports a variable
        // without a value has said nothing.
        .any(|name| lookup(name).is_some_and(|value| !value.is_empty()))
}

/// A thread sending [`Event::Tick`] until it is told to stop.
///
/// The handle is what stops it: dropping it, or calling
/// [`Ticker::stop`], lets the thread notice on its next wake and exit. Held
/// by whatever wanted the animation, so the animation cannot outlive its
/// reason -- a ticker still running behind an open file would redraw the
/// screen twelve times a second for nothing.
#[derive(Debug)]
pub struct Ticker {
    wanted: Arc<AtomicBool>,
}

impl Ticker {
    /// Starts ticking, unless this session is remote.
    ///
    /// `None` over a network, so the caller has nothing to hold and nothing
    /// to stop: an animation is a luxury, and a luxury paid for in round
    /// trips is not one.
    #[must_use]
    pub fn start(sender: Sender<Event>) -> Option<Self> {
        if remote() {
            tracing::info!("a remote session, so no animation");
            return None;
        }
        let wanted = Arc::new(AtomicBool::new(true));
        let mine = Arc::clone(&wanted);
        std::thread::Builder::new()
            .name("obelus-tick".to_string())
            .spawn(move || {
                while mine.load(Ordering::Relaxed) {
                    std::thread::sleep(TICK);
                    // Checked again after the sleep: the reason for ticking
                    // can have gone while this thread was asleep, and one
                    // tick too many is one redraw too many.
                    if !mine.load(Ordering::Relaxed) || sender.send(Event::Tick).is_err() {
                        break;
                    }
                }
            })
            .expect("spawning the tick thread");
        Some(Self { wanted })
    }

    /// Stops ticking.
    pub fn stop(&self) {
        self.wanted.store(false, Ordering::Relaxed);
    }
}

impl Drop for Ticker {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;

    /// What decides whether the animation runs at all.
    #[test]
    fn an_ssh_session_is_remote_and_an_empty_variable_says_nothing() {
        assert!(!remote_by(|_| None), "a local session read as remote");

        for name in ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY"] {
            assert!(
                remote_by(|asked| (asked == name).then(|| OsString::from("10.0.0.1 1 10.0.0.2 22"))),
                "{name} did not count"
            );
            // Exported with no value, which several shells do.
            assert!(
                !remote_by(|asked| (asked == name).then(OsString::new)),
                "{name} counted while empty"
            );
        }

        // A variable that is not one of the three says nothing either.
        assert!(!remote_by(
            |asked| (asked == "TERM").then(|| OsString::from("foot"))
        ));
    }
}
