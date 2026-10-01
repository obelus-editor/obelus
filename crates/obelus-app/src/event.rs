//! What the main loop reacts to.
//!
//! Terminal input and background work arrive as the same type through the same
//! channel, so the loop has one handler and a new source — the file watcher
//! and the LSP client, later — is a sender and a variant rather than a second
//! code path.
//!
//! One channel with several producer threads is what makes Obelus need no
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
    sync::mpsc::{self, Receiver, Sender},
    time::Duration,
};

use crossterm::event::{Event as TerminalEvent, KeyEvent};

/// One thing the application has to react to.
///
/// Six of these are the terminal's, and the rest are one worker's each.
/// The workers do not know this type: each names what it produces, and the
/// `From` impls below are where the application -- the only part of Obelus
/// that has heard of all of them -- says they are the same inbox.
///
/// Not `Clone`: an agent's question carries the channel its answer goes
/// back through, and there is one answer. Nothing clones an event anyway --
/// what the producers clone is the sink.
#[derive(Debug)]
pub enum Event {
    /// A key was pressed.
    Key(KeyEvent),
    /// The terminal was resized.
    Resize,
    /// What the faces on this machine are called.
    ///
    /// Only a window can say: a terminal draws with its own font and has
    /// no list to offer. Sent once, when the window has enumerated them,
    /// which is why it arrives as an event rather than being asked for --
    /// the answer is not ready when the reader opens the list.
    Fonts {
        /// Every face, for the reader to choose between.
        here: Vec<String>,
        /// And which of them this machine calls its monospaced one, which
        /// is what a reader who chooses none of them gets.
        otherwise: Option<String>,
    },
    /// The reader asked to close what Obelus is drawn in.
    ///
    /// A terminal never says this: closing one kills the process, and there
    /// is nothing to hear. A window's close button is the reader asking to
    /// leave, which is what the key that leaves means -- so it goes the same
    /// way, unwritten files and their question included, and the window
    /// stays until the application says it is done.
    Closed,
    /// The wheel turned, by this many rows. Negative is up the file.
    ///
    /// A wheel is not an arrow key: it moves the *view*, and the place the
    /// reader had chosen stays where they put it. Which is only knowable
    /// because Obelus asks the terminal to report the mouse -- without that
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
    /// Because Obelus asks for bracketed paste, which wraps what the
    /// terminal's own paste key delivers in a pair of escape sequences.
    /// Without it a pasted function arrives as somebody typing very fast,
    /// newlines and all, and every line of it is indented again by whatever
    /// `Enter` does -- the staircase the mode was invented to stop.
    ///
    /// It is also how text from outside reaches Obelus at all: the sequence
    /// Obelus copies *with* cannot be read back, so the terminal reading the
    /// clipboard is the way in.
    Paste(String),
    /// Time passed, and something on screen moves with it.
    ///
    /// The animation's and nothing else's: a phase and a drag. What moves is
    /// the welcome screen's sheen and the mark that turns while an agent is
    /// working, and the ticker runs only while one of those is on screen
    /// (`App::wants_animating`). A reader looking at code gets no ticks at
    /// all: there is nothing to animate, and a redraw a reader did not ask
    /// for is a redraw that can only get in the way. What is owed at a
    /// moment waits on a [`Pause`] instead.
    Tick,
    /// The pause after typing into the notes ran out.
    NotesSettled,
    /// Long enough since a tree was left behind its text to catch it up.
    ///
    /// A grammar too slow to keep up with typing owes an answer, and this
    /// is what comes back for it.
    SyntaxSettled,
    /// A document the reader has stopped changing is ready to be asked
    /// about.
    ///
    /// What a server works out about a whole file -- its colours, its
    /// hints -- which is asked where the file has stopped moving.
    ChangesSettled,
    /// The caret has stopped moving under the signature panel, so the call
    /// it is in can be asked about again.
    SignatureSettled,
    /// The pointer has rested long enough to be asking.
    PointerRested,
    /// A server asked what a rename changes has had long enough.
    ///
    /// The rename happens without it. A reader who asked for a file to be
    /// called something else is owed the file being called it.
    RenameOverdue,
    /// A walk of the project found something.
    Search(obelus_search::Event),
    /// A walk of the history found something out.
    Git(obelus_git::Event),
    /// Something about an agent, or from one.
    Agent(obelus_agent::Event),
    /// A message from a language server.
    Lsp(obelus_lsp::Message),
    /// An agent used one of the tools Obelus offers it.
    Tools(obelus_mcp::Asked),
    /// A project, counted.
    ///
    /// Boxed because it is much the largest thing an event carries -- two
    /// lists as long as the project is -- and every other variant would be
    /// sized to it. Boxed by the counting rather than here, so the box is
    /// made once and does not travel this whole way by value first.
    Counted(Box<obelus_search::counts::Counted>),
    /// A file on disk changed.
    Watched(obelus_watch::Changed),
    /// A list finished scoring its rows against a query.
    ///
    /// The scoring of a project's whole file list is tens of milliseconds,
    /// which on the loop is a keystroke the reader watches arrive. So it
    /// goes where every other long answer goes: a worker, and back here.
    Scanned(Box<obelus_component::picker::Scanned>),
}

impl Event {
    /// What kind of thing this is, in a word.
    ///
    /// For the line the loop writes when handling one took long enough to
    /// be worth knowing about. The event has to be named *before* it is
    /// handed over and consumed, so this is paid on every event and not
    /// only on the slow ones -- which is why it is a `&'static str` and
    /// not the `Debug` of the thing: a `format!` per keystroke, for a line
    /// that is almost never written, is work done for nothing.
    ///
    /// Which key it was is not here for the same reason. `KeyCode` is
    /// `Copy`, so the one caller that wants it takes it off the event
    /// itself and pays nothing either.
    #[must_use]
    pub const fn what(&self) -> &'static str {
        match self {
            Self::Key(_) => "Key",
            Self::Resize => "Resize",
            Self::Fonts { .. } => "Fonts",
            Self::Closed => "Closed",
            Self::Scroll(_) => "Scroll",
            Self::Pointer { .. } => "Pointer",
            Self::Paste(_) => "Paste",
            Self::Tick => "Tick",
            Self::NotesSettled => "NotesSettled",
            Self::SyntaxSettled => "SyntaxSettled",
            Self::ChangesSettled => "ChangesSettled",
            Self::SignatureSettled => "SignatureSettled",
            Self::PointerRested => "PointerRested",
            Self::RenameOverdue => "RenameOverdue",
            Self::Search(_) => "Search",
            Self::Git(_) => "Git",
            Self::Agent(_) => "Agent",
            Self::Lsp(_) => "Lsp",
            Self::Tools(_) => "Tools",
            Self::Counted(_) => "Counted",
            Self::Watched(_) => "Watched",
            Self::Scanned(_) => "Scanned",
        }
    }
}

macro_rules! from_worker {
    ($($from:ty => $variant:ident),* $(,)?) => {
        $(impl From<$from> for Event {
            fn from(event: $from) -> Self {
                Self::$variant(event)
            }
        })*
    };
}

impl From<Box<obelus_component::picker::Scanned>> for Event {
    fn from(scanned: Box<obelus_component::picker::Scanned>) -> Self {
        Self::Scanned(scanned)
    }
}

// What joins a worker's own events to the one channel the loop reads. The
// blanket impl in `obelus_sink` turns each of these into a `Sink` the
// worker can be handed, without the worker naming this enum.
from_worker! {
    obelus_search::Event => Search,
    obelus_git::Event => Git,
    obelus_agent::Event => Agent,
    obelus_lsp::Message => Lsp,
    obelus_mcp::Asked => Tools,
    Box<obelus_search::counts::Counted> => Counted,
    obelus_watch::Changed => Watched,
}

/// What the pointer's button did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pointer {
    /// Moved with nothing held down.
    ///
    /// Reported only by terminals that track motion, which is every one
    /// Obelus has been run on: it is what a rest is measured from, and
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
    /// Translates a crossterm event, or `None` for one Obelus ignores.
    ///
    /// Mouse and focus events Obelus has no use for are dropped rather than
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
/// A thread because the loop is already blocked on the channel, and the
/// terminal is the one source that cannot send into it by itself. Which
/// side is blocked on and which gets the thread is argued where the loop
/// is, in [`crate::app::run`].
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

/// A timer sending [`Event::Tick`] until it is dropped.
///
/// The handle is what stops it: dropping it, or calling [`Ticker::stop`],
/// ends it. Held by whatever wanted the animation, so the animation cannot
/// outlive its reason -- a ticker still running behind an open file would
/// redraw the screen twelve times a second for nothing.
///
/// A timer on [`obelus_runtime`] rather than a thread. It was a thread
/// whose whole body was sleep-and-send, with an `AtomicBool` beside it to
/// stop it -- and stopping cost up to a whole tick, because the flag was
/// read *after* the sleep, so a ticker told to stop could still send one
/// last redraw. The comment on that loop said as much and could only
/// narrow the window. An abort has no window.
#[derive(Debug)]
pub struct Ticker {
    beating: tokio::task::JoinHandle<()>,
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
        let beating = obelus_runtime::handle().spawn(async move {
            let mut beat = tokio::time::interval(TICK);
            // The first one is immediate, and a frame drawn the instant the
            // animation starts is the frame that was just drawn.
            beat.tick().await;
            loop {
                beat.tick().await;
                if sender.send(Event::Tick).is_err() {
                    break;
                }
            }
        });
        Some(Self { beating })
    }

    /// Stops ticking.
    pub fn stop(&self) {
        self.beating.abort();
    }
}

impl Drop for Ticker {
    fn drop(&mut self) {
        self.stop();
    }
}

/// A one-shot timer sending one event when a wait has run out.
///
/// What comes back for work that is owed at a moment rather than drawn at a
/// frame rate: the notes' three hundred milliseconds once the reader stops
/// typing, a tree a slow grammar left behind, the five seconds a rename
/// gives a server, the standing questions a server is asked once the reader
/// stops, and the pointer's rest. Every one is started through
/// `App::come_back_in`.
///
/// All five hung on [`Ticker`] or on the frames it kept coming, and that
/// was wrong twice. An animation is a luxury and `Ticker::start` says so by
/// refusing over a network -- so on ssh none of these ever happened: a note
/// typed there reached no file until the reader walked out of it, the
/// colours stopped arriving, a rename waited on a stuck server for the rest
/// of the session, and the pointer could rest for ever and never ask. And a
/// repeating clock asks twelve times a second for an answer that is "not
/// yet" until the one time it is not, waking the screen for each. A pause
/// is a moment and an animation is a frame rate; the two only ever looked
/// alike.
///
/// The event is named by whoever starts one, because the mechanism is
/// shared and the meaning is not: what these are waiting for has nothing
/// in common but the waiting.
///
/// Dropping it, or starting another in its place, ends the one before -- an
/// abort has no window, the same reason [`Ticker`] aborts. Whether it is
/// started again by each key or left alone until it fires belongs to the
/// caller, and they differ: the notes and a document's standing questions
/// measure the reader *stopping* and so start again on every key, while a
/// tree that is behind wants catching up soon whether or not the reader has
/// paused (`App::catch_up_soon`). Where the clock has an owner it lives in
/// it: the rename's is a field on the wait, so finishing takes the wait and
/// drops the clock with it.
///
/// **A guard on a deadline earns its place exactly when something other
/// than that deadline's own clock can reach the work.** Saying the deadline
/// twice is what a repeating clock forces, and three of these said it
/// twice: `rename_without_them` was asked on every tick whether the
/// question was five seconds old, and the notes and the standing questions
/// each kept an `Instant` for a frame to measure. A one-shot arriving *is*
/// the wait having run out, so those checks went, and `Waiting::asked`,
/// `notes_settling` and `Settling::since` with them. The one that stayed is
/// the pointer's -- see `App::settle_hover`.
#[derive(Debug)]
pub struct Pause {
    waiting: tokio::task::JoinHandle<()>,
}

impl Pause {
    /// Starts one, to send `event` in `after`.
    #[must_use]
    pub fn start(sender: Sender<Event>, after: Duration, event: Event) -> Self {
        let waiting = obelus_runtime::handle().spawn(async move {
            tokio::time::sleep(after).await;
            // Nothing is left to send to, which is Obelus leaving. Every one
            // of these has something else that does its work on the way out.
            let _ = sender.send(event);
        });
        Self { waiting }
    }
}

impl Drop for Pause {
    fn drop(&mut self) {
        self.waiting.abort();
    }
}

#[cfg(test)]
mod tests {
    /// A paste is the only way text from outside reaches Obelus: the
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
