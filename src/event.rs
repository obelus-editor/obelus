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
///
/// Not `Clone`: an agent's question carries the channel its answer goes
/// back through, and there is one answer. Nothing clones an event anyway --
/// what the producers clone is the sender.
#[derive(Debug)]
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
        /// Whether these are files the tree said it does not keep.
        ///
        /// Their own batches rather than a flag per path: a walk sends one
        /// kind or the other and never a mixture, because it is two walks
        /// -- one that obeys the ignore rules and one that does not.
        ignored: bool,
    },
    /// A batch of matching lines from a search of the tree.
    Matches {
        /// Which search these came from, so answers to a query the reader
        /// has already typed past can be dropped.
        generation: u64,
        /// The matches, in the order the walk found them.
        hits: Vec<crate::search::Hit>,
        /// Whether this is the last batch. With a query typed and nothing
        /// found, "still looking" and "not there" are different facts, and
        /// only the scan knows which one to show.
        done: bool,
    },
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
        commits: Vec<crate::git::history::Commit>,
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
        lines: Vec<Option<crate::git::Blamed>>,
    },
    /// The agent registry, from the disk or from the network.
    ///
    /// Twice per fetch, ordinarily: what was cached from a previous session
    /// arrives first so the page has something to show, and the fetched
    /// list replaces it when it lands.
    Registry {
        /// Every agent it lists that obelus can make sense of.
        agents: Vec<crate::agent::Agent>,
        /// Why nothing was fetched, when nothing was. A page that says
        /// "fetching" for ever is a page that is lying by then.
        failure: Option<String>,
    },
    /// How far an install has got.
    Installing {
        /// Which agent, by the registry's own name for it.
        id: String,
        /// What is known about how far along it is.
        progress: crate::agent::install::Progress,
    },
    /// One agent's mark, from the disk or from the network.
    ///
    /// Its own event per agent rather than a batch: forty small drawings
    /// arriving one at a time is forty cheap frames, and a page whose marks
    /// all appear at once is a page that had none until the slowest one
    /// landed.
    Icon {
        /// Which agent, by the registry's own name for it.
        id: String,
        /// The drawing, still as SVG. What size to draw it at and what
        /// colour to ink it in belong to the view.
        svg: String,
    },
    /// An install finished, one way or the other.
    Installed {
        /// Which agent.
        id: String,
        /// Why it did not work, or `None` because it did.
        failure: Option<String>,
    },
    /// Something from the agent obelus is talking to.
    ///
    /// Typed, unlike the language server's messages: the protocol's own
    /// crate does the reading, so what arrives here is what it means. Some
    /// of it carries a channel to answer through -- an agent asking
    /// permission has stopped and is waiting for a keystroke.
    Acp(crate::acp::Incoming),
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
    /// The pointer, over the screen.
    ///
    /// Where rather than what: which region a click lands in is the
    /// application's to work out, the same as it is for a key. Only the
    /// left button arrives -- the others belong to the terminal, and a
    /// program that took them would be taking away the paste and the menu
    /// every terminal has.
    Pointer {
        /// What the button did.
        kind: Pointer,
        /// The column, counted from the left of the screen.
        x: u16,
        /// And the row, from the top.
        y: u16,
    },
    /// Text the terminal pasted, all at once.
    ///
    /// Because obelus asks for bracketed paste, which wraps what the
    /// terminal's own paste key delivers in a pair of escape sequences.
    /// Without it a pasted function arrives as somebody typing very fast,
    /// newlines and all, and every line of it is indented again by whatever
    /// `Enter` does -- the staircase the mode was invented to stop.
    ///
    /// It is also how text from outside reaches obelus at all: the sequence
    /// obelus copies *with* cannot be read back, so the terminal reading the
    /// clipboard is the way in.
    Paste(String),
    /// Time passed, and something on screen moves with it.
    ///
    /// The only animated thing obelus has is the welcome screen's wordmark,
    /// and the ticker runs only while that is what is on screen. A reader
    /// looking at code gets no ticks at all: there is nothing to animate, and
    /// a redraw a reader did not ask for is a redraw that can only get in the
    /// way.
    Tick,
    /// A tree, counted.
    ///
    /// Boxed because it is much the largest thing an event carries -- two
    /// lists as long as the project is -- and every other variant would be
    /// sized to it.
    Counted(Box<crate::counts::Counted>),
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

/// What the pointer's button did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pointer {
    /// Moved with nothing held down.
    ///
    /// Reported only by terminals that track motion, which is every one
    /// obelus has been run on: it is what a rest is measured from, and
    /// where there is none there is simply no hover on a rest.
    Moved,
    /// Put down.
    Pressed,
    /// Moved with the button held.
    Dragged,
    /// Let go.
    Released,
}

impl Event {
    /// Translates a crossterm event, or `None` for one obelus ignores.
    ///
    /// Mouse and focus events obelus has no use for are dropped rather than
    /// stored: a variant nothing reads is indistinguishable from a broken
    /// feature.
    fn from_terminal(event: TerminalEvent) -> Option<Self> {
        match event {
            TerminalEvent::Key(key) => Some(Self::Key(key)),
            TerminalEvent::Resize(_, _) => Some(Self::Resize),
            // Three rows a notch, which is what everything that scrolls text
            // does. The other mouse events are dropped: nothing reads them,
            // and a variant nothing reads is indistinguishable from a broken
            // feature.
            TerminalEvent::Mouse(mouse) => {
                use crossterm::event::{MouseButton, MouseEventKind};
                let pointer = |kind| {
                    Some(Self::Pointer {
                        kind,
                        x: mouse.column,
                        y: mouse.row,
                    })
                };
                match mouse.kind {
                    MouseEventKind::ScrollDown => Some(Self::Scroll(3)),
                    MouseEventKind::ScrollUp => Some(Self::Scroll(-3)),
                    // The left button only. The others are the terminal's
                    // own -- a paste on middle click, a menu on right --
                    // and taking them would be taking them away.
                    MouseEventKind::Moved => pointer(Pointer::Moved),
                    MouseEventKind::Down(MouseButton::Left) => pointer(Pointer::Pressed),
                    MouseEventKind::Drag(MouseButton::Left) => pointer(Pointer::Dragged),
                    MouseEventKind::Up(MouseButton::Left) => pointer(Pointer::Released),
                    _ => None,
                }
            }
            TerminalEvent::Paste(text) => Some(Self::Paste(text)),
            TerminalEvent::FocusGained | TerminalEvent::FocusLost => None,
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
    /// A paste is the only way text from outside reaches obelus: the
    /// sequence it copies *with* cannot be read back, so what the terminal
    /// delivers is the way in. Dropped, it is not a paste that arrives
    /// wrong -- it is a key that does nothing.
    #[test]
    fn what_the_terminal_pastes_arrives() {
        let pasted =
            super::Event::from_terminal(crossterm::event::Event::Paste("fn main() {}".to_string()));
        assert!(
            matches!(pasted, Some(super::Event::Paste(text)) if text == "fn main() {}"),
            "a paste from the terminal did not arrive"
        );
    }

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
