//! Talking to an agent, over the Agent Client Protocol.
//!
//! An ACP agent is another program on the other end of a pipe: obelus starts
//! it, says who it is, opens a session rooted at the project, and then sends
//! prompts and reads what comes back.
//!
//! The protocol itself is `agent-client-protocol`, its own authors' crate.
//! Every method has a type whose field names the compiler checks, which is
//! the whole reason to use it: obelus had the nine methods it needs written
//! out by hand, checked once against the published schema, and a protocol
//! that grows a field or renames an outcome would have gone on compiling and
//! quietly stopped matching.
//!
//! What that crate is built around is `async`, and obelus's main loop is a
//! thread blocked on a channel. [`link`] is the join: one thread runs the
//! connection, what obelus wants becomes an [`Ask`] sent to it, and
//! everything the agent says becomes an [`Event`] on the loop's own channel
//! like the keyboard and the file walk.
//!
//! [`Event`]: crate::event::Event
//!
//! What obelus tells an agent about itself is the shape of the product: it
//! will read a file out -- from a buffer, so an agent sees what the reader
//! sees -- and it will not write one. A code reader that let an agent write
//! through it would be a code editor with no undo.

pub mod link;

use std::path::Path;

use futures::channel::mpsc;
pub use link::{Answer, Ask, Choice, Incoming, Mode, Order, Update};

/// One running agent: how to ask it things, and what it has said about
/// itself.
///
/// The state here is a mirror. The protocol's own state lives on the
/// thread; what a view needs -- which mode is on, whether it is thinking,
/// what it is called -- arrives as events and is kept here, because a view
/// asks questions on a frame and cannot wait for an answer.
pub struct Talk {
    /// The registry's id for it, which is what the settings stored.
    id: String,
    /// How to ask it things.
    asks: mpsc::UnboundedSender<Ask>,
    /// What it calls itself, once it has said.
    info: Option<String>,
    /// Whether there is a session to talk in.
    session: bool,
    /// Whether a turn is in flight.
    thinking: bool,
    /// Whether the conversation has ended, and why.
    gone: Option<Option<String>>,
    /// The ways of working it offers.
    modes: Vec<Mode>,
    /// Which one is on, by its id.
    mode: Option<String>,
    /// The commands it says it takes.
    orders: Vec<Order>,
    /// A prompt typed before there was a session to send it in.
    ///
    /// The ordinary case for the first thing said: opening the view starts
    /// the process, and a reader types faster than node starts.
    held: Option<String>,
}

impl std::fmt::Debug for Talk {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Talk")
            .field("id", &self.id)
            .field("session", &self.session)
            .field("thinking", &self.thinking)
            .field("mode", &self.mode)
            .finish_non_exhaustive()
    }
}

impl Talk {
    /// Starts an agent and returns the handle to it.
    #[must_use]
    pub fn start(
        id: &str,
        command: &Path,
        arguments: &[String],
        root: &Path,
        events: std::sync::mpsc::Sender<crate::event::Event>,
    ) -> Self {
        Self {
            id: id.to_string(),
            asks: link::start(command, arguments, root, events),
            info: None,
            session: false,
            thinking: false,
            gone: None,
            modes: Vec::new(),
            mode: None,
            orders: Vec::new(),
            held: None,
        }
    }

    /// Which agent this is, by the registry's id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// What the agent calls itself, once it has said.
    #[must_use]
    pub fn info(&self) -> Option<&str> {
        self.info.as_deref()
    }

    /// Whether there is a session, and so whether a prompt goes anywhere.
    #[must_use]
    pub const fn is_started(&self) -> bool {
        self.session
    }

    /// Whether a turn is in flight.
    #[must_use]
    pub const fn is_thinking(&self) -> bool {
        self.thinking
    }

    /// Whether the conversation has ended.
    #[must_use]
    pub const fn has_exited(&self) -> bool {
        self.gone.is_some()
    }

    /// The ways of working it offers.
    #[must_use]
    pub fn modes(&self) -> &[Mode] {
        &self.modes
    }

    /// Which one is on.
    #[must_use]
    pub fn mode(&self) -> Option<&Mode> {
        let id = self.mode.as_deref()?;
        self.modes.iter().find(|mode| mode.id == id)
    }

    /// The commands it says it takes.
    #[must_use]
    pub fn orders(&self) -> &[Order] {
        &self.orders
    }

    /// Sends a prompt, or holds it until there is a session to send it in.
    ///
    /// Says whether it went: a prompt that is being held is a prompt the
    /// view shows as sent, because the reader has finished with it either
    /// way.
    pub fn say(&mut self, words: &str) -> bool {
        if !self.session {
            self.held = Some(words.to_string());
            return false;
        }
        self.thinking = self
            .asks
            .unbounded_send(Ask::Say(words.to_string()))
            .is_ok();
        self.thinking
    }

    /// Asks the agent to stop what it is doing.
    ///
    /// The turn is left in flight: it ends with the agent's own `cancelled`
    /// stop reason, which is the agent saying it has stopped rather than
    /// obelus assuming it.
    pub fn interrupt(&mut self) {
        if self.thinking {
            let _ = self.asks.unbounded_send(Ask::Interrupt);
        }
    }

    /// Moves to the next way of working.
    ///
    /// Walked rather than chosen from a list: there are two or three of
    /// these and they are a cycle in the agent's own order, which is what a
    /// key that steps through them means. What is shown changes here, and
    /// the agent's own update -- if it sends one -- says the same thing
    /// again.
    pub fn step_mode(&mut self) {
        if self.modes.len() < 2 {
            return;
        }
        let at = self
            .mode
            .as_deref()
            .and_then(|id| self.modes.iter().position(|mode| mode.id == id))
            .unwrap_or(0);
        let next = self.modes[(at + 1) % self.modes.len()].id.clone();
        self.mode = Some(next.clone());
        let _ = self.asks.unbounded_send(Ask::Mode(next));
    }

    /// Stops talking, which ends the conversation and the process with it.
    ///
    /// Dropping the asks is the whole of it: the thread's loop over them
    /// ends, the connection closes, and the agent -- reading a pipe that
    /// has gone -- exits. Which is how a language server is stopped too.
    pub fn shutdown(&mut self) {
        self.asks.close_channel();
        self.session = false;
    }

    /// Whether the conversation is still going, for the frame that checks.
    ///
    /// A conversation ends by saying so -- the thread sends `Gone` -- so
    /// there is nothing to ask an operating system here. It is a reader of
    /// what has already arrived, which is why the view can call it.
    pub const fn is_alive(&self) -> bool {
        self.gone.is_none()
    }

    /// Folds what arrived into what is known, and hands back whatever the
    /// view still has to do something about.
    ///
    /// The same shape the language server's client has: what the protocol
    /// needs is dealt with here, and what a reader needs to see goes on.
    pub fn on(&mut self, incoming: Incoming) -> Option<Incoming> {
        match incoming {
            Incoming::Ready(named) => {
                self.info = named;
                None
            }
            Incoming::Started { modes, current } => {
                self.session = true;
                self.modes = modes;
                self.mode = current;
                if let Some(held) = self.held.take() {
                    self.say(&held);
                }
                None
            }
            Incoming::Update(Update::Mode(id)) => {
                self.mode = Some(id);
                None
            }
            Incoming::Update(Update::Orders(orders)) => {
                self.orders = orders;
                None
            }
            Incoming::Ended(reason) => {
                self.thinking = false;
                Some(Incoming::Ended(reason))
            }
            Incoming::Failed(what, why) => {
                self.thinking = false;
                Some(Incoming::Failed(what, why))
            }
            Incoming::Gone(why) => {
                self.thinking = false;
                self.session = false;
                self.gone = Some(why.clone());
                Some(Incoming::Gone(why))
            }
            other => Some(other),
        }
    }
}
