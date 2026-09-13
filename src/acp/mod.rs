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
//! thread blocked on a channel. [`link`] is the join: one thread runs a
//! tokio runtime with the connection in it, what obelus wants becomes an
//! [`Ask`] sent to that thread, and everything the agent says becomes an
//! [`Event`] on the loop's own channel like the keyboard and the file
//! walk.
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
pub use link::{
    Answer, Ask, Call, Category, Change, Choice, Chosen, Field, Incoming, Kind, Order, Place,
    Reply, Setting, Takes, Update, Value,
};

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
    /// The commands it says it takes.
    orders: Vec<Order>,
    /// The settings it lets the reader change, as the agent's own list of
    /// config options.
    options: Vec<Setting>,
    /// The mode it offers through the older, dedicated methods, if it does.
    ///
    /// Kept apart from the others so that it can be dropped when the
    /// options turn out to carry the mode themselves: an agent in the
    /// middle of that change offers both, and a reader must see one.
    legacy_mode: Option<Setting>,
    /// Both of those, merged: what everything above this reads.
    settings: Vec<Setting>,
    /// What a mode was before the reader stepped it, while the agent has
    /// not answered.
    ///
    /// `session/set_mode` answers with nothing at all, so what is shown
    /// after that key is obelus's own guess -- and a guess has to be taken
    /// back if the agent refuses, or the row goes on naming a mode the
    /// agent is not in.
    guessed: Option<(String, String)>,
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
            .field("settings", &self.settings.len())
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
            orders: Vec::new(),
            options: Vec::new(),
            legacy_mode: None,
            settings: Vec::new(),
            guessed: None,
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

    /// The way of working, if the agent offers one.
    ///
    /// Which is a setting like the rest of them -- the protocol's own
    /// `category: "mode"` says which -- and is only named apart because one
    /// key steps it and it is drawn first.
    #[must_use]
    pub fn mode(&self) -> Option<&Setting> {
        self.settings
            .iter()
            .find(|setting| setting.category == Category::Mode)
    }

    /// The commands it says it takes.
    #[must_use]
    pub fn orders(&self) -> &[Order] {
        &self.orders
    }

    /// The settings it lets the reader change.
    #[must_use]
    pub fn settings(&self) -> &[Setting] {
        &self.settings
    }

    /// One of them, by the agent's id for it.
    #[must_use]
    pub fn setting(&self, id: &str) -> Option<&Setting> {
        self.settings.iter().find(|setting| setting.id == id)
    }

    /// Puts one of them on one of its values.
    ///
    /// Two doors, and this is the only thing that knows there are two: a
    /// setting the agent offered as a config option goes back as one, and
    /// the mode an agent offers the old way goes back as a mode. When the
    /// old methods leave the protocol, the second branch leaves with them.
    ///
    /// What is shown does not change for a config option: the agent answers
    /// with the whole set of them again, because one setting's value can
    /// change what another offers, so a chosen value appears when the agent
    /// has taken it. `session/set_mode` answers with nothing, so there the
    /// value is put on now and taken back if the agent refuses.
    pub fn set(&mut self, setting: &str, chosen: Chosen) {
        let ask = match (
            self.setting(setting).is_some_and(|known| known.legacy),
            &chosen,
        ) {
            (true, Chosen::Value(mode)) => {
                self.guess(setting, mode);
                Ask::Mode(mode.clone())
            }
            _ => Ask::Set {
                setting: setting.to_string(),
                chosen,
            },
        };
        let _ = self.asks.unbounded_send(ask);
    }

    /// Shows a mode as on before the agent has said so.
    fn guess(&mut self, setting: &str, value: &str) {
        if let Some(mode) = self.legacy_mode.as_mut().filter(|mode| mode.id == setting) {
            self.guessed = Some((setting.to_string(), mode.current.clone()));
            mode.current = value.to_string();
            self.merge();
        }
    }

    /// Takes that guess back.
    fn unguess(&mut self) {
        let Some((setting, was)) = self.guessed.take() else {
            return;
        };
        if let Some(mode) = self.legacy_mode.as_mut().filter(|mode| mode.id == setting) {
            mode.current = was;
            self.merge();
        }
    }

    /// Works out the one list everything above this reads.
    ///
    /// The agent's own order, which is the only order that means anything:
    /// the spec asks a client to place options by it, and an agent puts its
    /// mode where a reader looks for it. Nothing is sorted here -- the mode
    /// is first because the agent says so, not because obelus moved it.
    ///
    /// The one thing obelus has to place is a mode from the older, dedicated
    /// methods: it is not in that array at all, so it goes in front of it,
    /// which is where the old methods drew it.
    ///
    /// And the mode is only in the list once. An agent part-way through the
    /// protocol's change offers it both ways at the same time, so the option
    /// wins and the old one is left out -- decided by what the agent said
    /// the option is *about*, not by obelus recognising a name.
    fn merge(&mut self) {
        let carried = self
            .options
            .iter()
            .any(|option| option.category == Category::Mode);
        let mut settings: Vec<Setting> = Vec::with_capacity(self.options.len() + 1);
        settings.extend(self.legacy_mode.clone().filter(|_| !carried));
        settings.extend(self.options.iter().cloned());
        self.settings = settings;
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
    /// key that steps through them means. It goes out through [`Talk::set`]
    /// like every other change, because it *is* one -- the mode is a
    /// setting, whichever way the agent offers it.
    pub fn step_mode(&mut self) {
        let Some(mode) = self.mode() else {
            return;
        };
        if mode.values.len() < 2 {
            return;
        }
        let at = mode
            .values
            .iter()
            .position(|value| value.id == mode.current)
            .unwrap_or(0);
        let (id, next) = (
            mode.id.clone(),
            mode.values[(at + 1) % mode.values.len()].id.clone(),
        );
        let chosen = self
            .setting(&id)
            .map(|setting| Chosen::of(setting, &next))
            .unwrap_or(Chosen::Value(next));
        self.set(&id, chosen);
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
            Incoming::Started { mode } => {
                self.session = true;
                self.legacy_mode = mode;
                self.merge();
                if let Some(held) = self.held.take() {
                    self.say(&held);
                }
                None
            }
            Incoming::Update(Update::Mode(id)) => {
                // The agent has spoken, so there is no guess left to take
                // back -- whether it moved because the reader asked or on
                // its own.
                self.guessed = None;
                if let Some(mode) = self.legacy_mode.as_mut() {
                    mode.current = id;
                }
                self.merge();
                None
            }
            Incoming::Update(Update::Orders(orders)) => {
                self.orders = orders;
                None
            }
            Incoming::Update(Update::Settings(options)) => {
                self.options = options;
                self.merge();
                None
            }
            Incoming::Ended(reason) => {
                self.thinking = false;
                Some(Incoming::Ended(reason))
            }
            Incoming::Failed(what, why) => {
                self.thinking = false;
                // A mode obelus showed as on that the agent would not take.
                self.unguess();
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
