//! Talking to an agent, over the Agent Client Protocol.
//!
//! An ACP agent is another program on the other end of a pipe: Obelus starts
//! it, says who it is, opens a session rooted at the project, and then sends
//! prompts and reads what comes back.
//!
//! The protocol itself is `agent-client-protocol`, its own authors' crate.
//! Every method has a type whose field names the compiler checks, which is
//! the whole reason to use it: Obelus had the nine methods it needs written
//! out by hand, checked once against the published schema, and a protocol
//! that grows a field or renames an outcome would have gone on compiling and
//! quietly stopped matching.
//!
//! What that crate is built around is `async`, and Obelus's main loop is a
//! thread blocked on a channel. [`link`] is the join: one thread runs a
//! tokio runtime with the connection in it, what Obelus wants becomes an
//! [`Ask`] sent to that thread, and everything the agent says goes into a
//! [`crate::Event`], which reaches the loop's own channel like the
//! keyboard and the file walk do.
//!
//! What Obelus tells an agent about itself is the shape of the product: it
//! will read a file out -- from a buffer, so an agent sees what the reader
//! sees -- and it will not write one. That was once because Obelus wrote
//! nothing at all, and letting an agent write through it would have been a
//! code editor with no undo. Obelus has an undo now, and the reason has
//! changed rather than gone: a change the reader did not make is a change
//! they cannot see arriving, and the one thing an editor owes them is that
//! what is on screen is what they did to it.
//!
//! An agent that stopped is started again by talking to it. Which is what
//! the view tells the reader to do, and what it did not do: the handle of a
//! conversation that had ended stayed in place, so the check for "is there an
//! agent" found one and said the message into a channel whose far end had gone.
//! The handle stays -- the view reads the state off it, and a screen that
//! forgot the agent had died would have nothing to say about why nothing
//! happens -- so what asks is whether it has *exited*, not whether it is there.
//! Everything it was waiting on goes at the same time: a card the reader can
//! answer into a dead channel is worse than no card.
//!
//! Its last words are a line. The protocol crate's `Display` is its message
//! followed by every field of `data` pretty-printed, which for an agent that
//! exited is four rows of JSON carrying one sentence and the source path of a
//! crate in the cargo registry. The sentence goes in the transcript and the
//! whole of it in the log.

pub mod link;
pub mod sessions;

use std::path::Path;

pub use agent_client_protocol::schema::v1::SessionId;
use futures::channel::mpsc;
pub use link::{
    Answer, Ask, Call, Category, Change, Choice, Chosen, Cost, Field, Incoming, Kind, Order, Place,
    Reply, Setting, Step, Takes, Turn, Update, Usage, Value,
};
use obelus_sink::Sink;

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
    /// What a prompt to this agent may carry, as it said in the handshake.
    carries: link::Carries,
    /// Whether the connection has ended, and why.
    gone: Option<Option<String>>,
    /// The conversations open on it, by the name the agent gave each.
    ///
    /// One process, several conversations. The four fields above are the
    /// *process* -- one name, one pipe, one death -- and everything about
    /// what is being talked about is in here, one of these each. They were
    /// flat beside each other while there was one conversation, which made
    /// the type's own doc untrue: it says "one running agent", and half of
    /// it was about one thing said to it.
    sessions: std::collections::HashMap<SessionId, Session>,
    /// A prompt typed before there was any session to send it in.
    ///
    /// The ordinary case for the first thing said: opening the view starts
    /// the process, and a reader types faster than node starts. On the
    /// connection rather than on a session, because at that moment there is
    /// no session for it to be on.
    ///
    /// With whatever Obelus had to say about the conversation first, because
    /// the first thing said is what carries it and the first thing said is
    /// what gets held.
    held: Option<(String, Option<String>)>,
    /// How many turns have been asked for on this connection.
    ///
    /// The last number handed out, and the next one is one more. On the
    /// connection rather than on a session so that a number means one turn
    /// whichever conversation it turns out to be about -- and because the
    /// conversation an answer belongs to is a thing Obelus reads *off* the
    /// answer, so a count kept per conversation would be a count that has
    /// to be found before it can be used.
    turns: Turn,
}

/// One conversation, as the main loop needs to see it.
///
/// The protocol's own state lives on the thread; this is what a view asks
/// about on a frame and cannot wait for an answer to.
#[derive(Debug, Default)]
pub struct Session {
    /// Which turn is in flight in this one, by Obelus's own count.
    ///
    /// A number and not a flag, because the answer that ends a turn does
    /// not say which turn it is about -- the protocol has no name for one.
    /// A flag was put down by whichever answer came home first, so an
    /// answer to a turn that had been cancelled, or overtaken, ended the
    /// turn that had replaced it: the conversation went to resting with an
    /// agent still working in it, and nothing on screen said so.
    turn: Option<Turn>,
    /// The commands it says it takes.
    orders: Vec<Order>,
    /// How full the agent's memory of this conversation is, once it has
    /// said. Kept rather than shown as it arrives: several of these land in
    /// one turn, and the row that shows it is drawn every frame anyway.
    usage: Option<Usage>,
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
    /// What the agent calls this conversation, once it has said.
    ///
    /// Kept rather than only shown, because it is the name a list of open
    /// conversations goes by: the note one is about says what the reader
    /// meant to do, and this says what it turned into.
    title: Option<String>,
    /// What a mode was before the reader stepped it, while the agent has
    /// not answered.
    ///
    /// `session/set_mode` answers with nothing at all, so what is shown
    /// after that key is Obelus's own guess -- and a guess has to be taken
    /// back if the agent refuses, or the row goes on naming a mode the
    /// agent is not in.
    guessed: Option<(String, String)>,
}

impl Session {
    /// The way of working, if the agent offers one.
    ///
    /// Which is a setting like the rest of them -- the protocol's own
    /// `category: "mode"` says which -- and is only named apart because one
    /// key steps it and it is drawn first.
    fn mode(&self) -> Option<&Setting> {
        self.settings
            .iter()
            .find(|setting| setting.category == Category::Mode)
    }

    /// One of them, by the agent's id for it.
    fn setting(&self, id: &str) -> Option<&Setting> {
        self.settings.iter().find(|setting| setting.id == id)
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
    /// is first because the agent says so, not because Obelus moved it.
    ///
    /// The one thing Obelus has to place is a mode from the older, dedicated
    /// methods: it is not in that array at all, so it goes in front of it,
    /// which is where the old methods drew it.
    ///
    /// And the mode is only in the list once. An agent part-way through the
    /// protocol's change offers it both ways at the same time, so the option
    /// wins and the old one is left out -- decided by what the agent said
    /// the option is *about*, not by Obelus recognising a name.
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
}

impl std::fmt::Debug for Talk {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Talk")
            .field("id", &self.id)
            .field("sessions", &self.sessions.len())
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
        tools: Option<String>,
        events: impl Sink<crate::Event> + Clone,
    ) -> Self {
        Self {
            id: id.to_string(),
            asks: link::start(command, arguments, root, tools, events),
            info: None,
            carries: link::Carries::default(),
            gone: None,
            sessions: std::collections::HashMap::new(),
            held: None,
            turns: 0,
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

    /// What a prompt to this agent may carry.
    ///
    /// Everything false until the handshake has finished, which is the
    /// honest answer while nobody has said: a picture offered before the
    /// agent has spoken is a picture Obelus guessed it would take.
    #[must_use]
    pub const fn carries(&self) -> link::Carries {
        self.carries
    }

    /// Whether this conversation exists, and so whether a prompt in it goes
    /// anywhere.
    #[must_use]
    pub fn is_started(&self, session: Option<&SessionId>) -> bool {
        session.is_some_and(|session| self.sessions.contains_key(session))
    }

    /// Whether a turn is in flight in this one.
    #[must_use]
    pub fn is_thinking(&self, session: Option<&SessionId>) -> bool {
        // A prompt with nowhere to go yet counts. The reader pressed
        // enter and their words are on the page, so something is under
        // way from where they sit -- and the conversation it is waiting
        // for is the one being opened, of which there is only ever one.
        //
        // Which is not a corner. An agent replaying a conversation sends
        // every word of it before it answers the request that asked for
        // it, so the page fills and *then* the session arrives: a reader
        // looking at a conversation that is plainly all there types into
        // it, and until this that turn went out with nothing on screen
        // saying anything was happening.
        self.held.is_some()
            || self
                .session(session)
                .is_some_and(|open| open.turn.is_some())
    }

    /// One conversation, by the name the agent gave it.
    #[must_use]
    fn session(&self, session: Option<&SessionId>) -> Option<&Session> {
        self.sessions.get(session?)
    }

    /// The same, to change.
    fn session_mut(&mut self, session: Option<&SessionId>) -> Option<&mut Session> {
        self.sessions.get_mut(session?)
    }

    /// Asks for another conversation on this same process.
    ///
    /// One agent holds a project's worth of context, and a second process
    /// to talk about a second note would pay for all of it twice.
    pub fn open(&mut self) {
        let _ = self.asks.unbounded_send(Ask::Open);
    }

    /// Asks what it can be set to, on a conversation of its own.
    ///
    /// Opened to read one list off it and let go again, so nothing here
    /// keeps a session for it: what comes back is
    /// [`Incoming::Offers`], which names no conversation because by then
    /// there is none. The reader's own conversations are not used for
    /// this -- one of them existing because a settings page wanted a list
    /// would be a conversation that goes wherever that list goes.
    pub fn offers(&mut self) {
        let _ = self.asks.unbounded_send(Ask::Offers);
    }

    /// Lets one go, because the note it was about has gone.
    ///
    /// Told to the agent rather than only forgotten here: an agent left
    /// holding conversations nobody can reach is the same complaint that
    /// got the language server killed on the way out.
    pub fn let_go(&mut self, session: &SessionId) {
        self.sessions.remove(session);
        let _ = self.asks.unbounded_send(Ask::Drop {
            session: session.clone(),
        });
    }

    /// Asks for one it had before, by the name Obelus wrote down.
    ///
    /// An agent that will not take it up -- it has forgotten, it never
    /// could -- opens a new one instead and says so, because a reader who
    /// pressed a key has to end up somewhere they can talk.
    pub fn reopen(&mut self, session: &str) {
        let _ = self.asks.unbounded_send(Ask::Reopen {
            session: SessionId::new(session),
        });
    }

    /// Whether the agent's process has ended.
    ///
    /// The process, not a conversation: one agent holds as many
    /// conversations as the reader has opened, and they end when it does.
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
    pub fn mode(&self, session: Option<&SessionId>) -> Option<&Setting> {
        self.session(session)?.mode()
    }

    /// What the agent calls this conversation, once it has said.
    #[must_use]
    pub fn title(&self, session: Option<&SessionId>) -> Option<&str> {
        self.session(session)?.title.as_deref()
    }

    /// The commands it says it takes.
    #[must_use]
    pub fn orders(&self, session: Option<&SessionId>) -> &[Order] {
        self.session(session).map_or(&[], |open| &open.orders)
    }

    /// How full it is, and what it has cost, where the agent has said.
    #[must_use]
    pub fn usage(&self, session: Option<&SessionId>) -> Option<&Usage> {
        self.session(session)?.usage.as_ref()
    }

    /// The settings it lets the reader change.
    #[must_use]
    pub fn settings(&self, session: Option<&SessionId>) -> &[Setting] {
        self.session(session).map_or(&[], |open| &open.settings)
    }

    /// One of them, by the agent's id for it.
    #[must_use]
    pub fn setting(&self, session: Option<&SessionId>, id: &str) -> Option<&Setting> {
        self.session(session)?.setting(id)
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
    pub fn set(&mut self, session: Option<&SessionId>, setting: &str, chosen: Chosen) {
        let Some(id) = session.cloned() else {
            return;
        };
        let legacy = self
            .setting(session, setting)
            .is_some_and(|known| known.legacy);
        let ask = match (legacy, &chosen) {
            (true, Chosen::Value(mode)) => {
                let mode = mode.clone();
                if let Some(open) = self.session_mut(session) {
                    open.guess(setting, &mode);
                }
                Ask::Mode { session: id, mode }
            }
            _ => Ask::Set {
                session: id,
                setting: setting.to_string(),
                chosen,
            },
        };
        let _ = self.asks.unbounded_send(ask);
    }

    /// Sends a prompt, or holds it until there is a session to send it in.
    ///
    /// Says whether it went: a prompt that is being held is a prompt the
    /// view shows as sent, because the reader has finished with it either
    /// way.
    pub fn say(&mut self, session: Option<&SessionId>, words: &str, opening: Option<&str>) -> bool {
        let Some(id) = session
            .filter(|id| self.sessions.contains_key(*id))
            .cloned()
        else {
            // Written down because the three ways out of here look the
            // same on screen: the reader's words are on the page whichever
            // it was. Which one it went is the first thing anybody asks
            // when a conversation goes quiet, and it is the one thing the
            // reader cannot see.
            tracing::info!(
                asked_in = ?session.map(|id| id.0.to_string()),
                open = self.sessions.len(),
                "a prompt is held: there is no conversation open to send it in"
            );
            // Held with its opening: the opening belongs to the first thing
            // said in a conversation, and the first thing said is exactly
            // what gets held while the session is still opening.
            self.held = Some((words.to_string(), opening.map(str::to_string)));
            return false;
        };
        let named = id.0.to_string();
        // Numbered before it goes, and the number comes back on the answer.
        // Counted on the connection rather than per conversation so that
        // one of these is never two turns, whichever conversation an answer
        // turns out to be about.
        self.turns += 1;
        let turn = self.turns;
        let sent = self
            .asks
            .unbounded_send(Ask::Say {
                session: id,
                turn,
                words: words.to_string(),
                opening: opening.map(str::to_string),
            })
            .is_ok();
        match sent {
            true => tracing::info!(session = %named, turn, "a prompt is on its way"),
            false => tracing::warn!(
                session = %named,
                turn,
                "a prompt went nowhere: the connection to the agent has ended"
            ),
        }
        if let Some(open) = self.session_mut(session) {
            open.turn = sent.then_some(turn);
        }
        sent
    }

    /// Asks the agent to stop what it is doing.
    ///
    /// The turn is left in flight: it ends with the agent's own `cancelled`
    /// stop reason, which is the agent saying it has stopped rather than
    /// Obelus assuming it.
    pub fn interrupt(&mut self, session: Option<&SessionId>) {
        let Some(id) = session.cloned() else {
            return;
        };
        // Named, so that the end Obelus writes for it is about the turn the
        // reader stopped and not about whatever is running by the time it
        // is read. A turn that is not running has nothing to stop.
        let Some(turn) = self.session(session).and_then(|open| open.turn) else {
            return;
        };
        let _ = self
            .asks
            .unbounded_send(Ask::Interrupt { session: id, turn });
    }

    /// Moves to the next way of working.
    ///
    /// Walked rather than chosen from a list: there are two or three of
    /// these and they are a cycle in the agent's own order, which is what a
    /// key that steps through them means. It goes out through [`Talk::set`]
    /// like every other change, because it *is* one -- the mode is a
    /// setting, whichever way the agent offers it.
    pub fn step_mode(&mut self, session: Option<&SessionId>) {
        let Some(mode) = self.mode(session) else {
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
            .setting(session, &id)
            .map(|setting| Chosen::of(setting, &next))
            .unwrap_or(Chosen::Value(next));
        self.set(session, &id, chosen);
    }

    /// Stops talking, which ends every conversation and the process with it.
    ///
    /// Dropping the asks is the whole of it: the thread's loop over them
    /// ends, the connection closes, and the agent -- reading a pipe that
    /// has gone -- exits. Which is how a language server is stopped too.
    pub fn shutdown(&mut self) {
        self.asks.close_channel();
        self.sessions.clear();
    }

    /// Whether the agent is still there, for the frame that checks.
    ///
    /// It ends by saying so -- the thread sends `Gone` -- so there is
    /// nothing to ask an operating system here. It is a reader of what has
    /// already arrived, which is why the view can call it.
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
            Incoming::Ready { named, carries } => {
                self.info = named;
                self.carries = carries;
                None
            }
            Incoming::Started { session, mode } => {
                // `or_default` rather than an insert, because this is not
                // always the first word about a conversation: an agent is
                // free to write its opening notification in the same breath
                // as the answer that names the session, and the two reach
                // the main loop in whichever order the connection settles
                // them. Whatever named it, the agent says it exists.
                let open = self.sessions.entry(session.clone()).or_default();
                open.legacy_mode = mode;
                open.merge();
                // A prompt typed before there was anywhere to send it.
                // It goes to whichever conversation opened first, which is
                // the one the reader was looking at when they typed it --
                // there was no other.
                if let Some((held, opening)) = self.held.take() {
                    self.say(Some(&session), &held, opening.as_deref());
                }
                Some(Incoming::Started {
                    session,
                    mode: None,
                })
            }
            Incoming::Update {
                session,
                update: Update::Mode(id),
            } => {
                let open = self.sessions.entry(session).or_default();
                // The agent has spoken, so there is no guess left to take
                // back -- whether it moved because the reader asked or on
                // its own.
                open.guessed = None;
                if let Some(mode) = open.legacy_mode.as_mut() {
                    mode.current = id;
                }
                open.merge();
                None
            }
            Incoming::Update {
                session,
                update: Update::Titled(title),
            } => {
                self.sessions.entry(session.clone()).or_default().title = Some(title.clone());
                // Kept *and* passed up, which none of the other folded
                // updates are: it is the name a conversation goes by in the
                // list of open documents, and a name has to survive Obelus
                // being shut. Whoever writes that down is above this.
                Some(Incoming::Update {
                    session,
                    update: Update::Titled(title),
                })
            }
            Incoming::Update {
                session,
                update: Update::Orders(orders),
            } => {
                self.sessions.entry(session).or_default().orders = orders;
                None
            }
            Incoming::Update {
                session,
                update: Update::Used(usage),
            } => {
                self.sessions.entry(session).or_default().usage = Some(usage);
                None
            }
            Incoming::Update {
                session,
                update: Update::Settings(options),
            } => {
                let open = self.sessions.entry(session.clone()).or_default();
                open.options = options.clone();
                open.merge();
                // Kept *and* passed up, like the title and unlike the rest
                // of the folded updates: what an agent offers to be set is
                // something Obelus acts on outside this mirror -- it is
                // written down beside the install so the settings page has
                // it before there is a conversation, and it is what the
                // reader's standing choices are matched against. Folded
                // away here, both happened only at the moment a session
                // opened, which is before an agent has said what it
                // offers.
                Some(Incoming::Update {
                    session,
                    update: Update::Settings(options),
                })
            }
            Incoming::Ended { session, turn, why } => {
                let open = self.sessions.entry(session.clone()).or_default();
                // An answer about a turn that is not the one running is an
                // answer nobody is waiting for: a prompt the reader
                // cancelled, answered by an agent that never saw the
                // cancellation, or one overtaken while it was in flight.
                // Passed up, it ends the turn that replaced it -- which is
                // the conversation going to rest with an agent still
                // working in it.
                if open.turn != Some(turn) {
                    tracing::debug!(
                        session = %session.0,
                        turn,
                        running = ?open.turn,
                        "an answer about a turn nobody is waiting for"
                    );
                    return None;
                }
                open.turn = None;
                Some(Incoming::Ended { session, turn, why })
            }
            Incoming::Failed(what, why) => {
                // Which conversation it was is not on the message, so every
                // one of them stops thinking -- one that is not would
                // otherwise spin for ever.
                //
                // Nothing puts that back. It read "a turn that is still
                // running says so again on its next update", which was
                // never true: `say` is the only place this is ever set, and
                // no update touches it. A turn that outlives a `Failed`
                // about something else is a turn Obelus has stopped saying
                // is running, and the honest reason to accept that is that
                // it cannot tell which turn the failure was about.
                for open in self.sessions.values_mut() {
                    open.turn = None;
                    // A mode Obelus showed as on that the agent would not
                    // take.
                    open.unguess();
                }
                Some(Incoming::Failed(what, why))
            }
            Incoming::Gone(why) => {
                // The process is what died, so every conversation on it is
                // over. Clearing each one's `thinking` is the half that
                // matters: a conversation left thinking spins a marker for
                // an agent that is not there.
                self.sessions.clear();
                self.gone = Some(why.clone());
                Some(Incoming::Gone(why))
            }
            other => Some(other),
        }
    }
}
