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
//! thread blocked on a channel. [`link`] is the join: the connection runs
//! as a task on the one runtime Obelus waits on, what Obelus wants becomes
//! an [`Ask`] sent to it, and everything the agent says goes into a
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
//!
//! **Obelus numbers its own turns**, because the protocol will not: the
//! number goes out with the prompt, comes back on the answer, and an answer
//! about a turn that is not the one running is dropped where the count is
//! kept (`Session::turn`). It replaced a flag that said only "given up on",
//! which the next prompt cleared -- so the cancelled turn's own answer,
//! which a well-behaved agent sends because the protocol tells it to, was
//! delivered after all and ended the turn that had replaced it. zed numbers
//! them too, and has a test whose name is this paragraph.
//!
//! The queue Obelus keeps in front of a running turn is what makes two turns
//! rare (see `obelus_app::conversation`); the number is what makes the rare
//! one harmless. Both, because the first is Obelus's own discipline and the
//! second is about what arrives.

pub mod link;
mod picture;
pub mod sessions;

use std::path::Path;

pub use agent_client_protocol::schema::v1::SessionId;
use futures::channel::mpsc;
pub use link::{
    Answer, Ask, Call, Category, Change, Choice, Chosen, Cost, Field, How, Incoming, Kind, Login,
    Order, Place, Question, Reply, Setting, Step, Takes, Turn, Update, Usage, Value,
};
use obelus_sink::Sink;

/// Which connection to an agent something came from, by a count kept for
/// the life of the process.
///
/// Nothing the protocol sends says, and it has to be said: a connection
/// that has been stopped goes on talking for a moment -- what it had
/// already read, and last of all that it has gone -- and what it had said
/// before it was stopped is still in the loop's queue when the next one
/// starts. Taken for the next one's, its last word had a process that had
/// just answered taken to have died, and its answer to an old request was
/// matched to the new one's first.
pub type Connection = u64;

/// The last connection handed a number.
static CONNECTIONS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A connection's way into the main loop, which says whose words these are
/// -- see [`Connection`].
#[derive(Clone)]
struct Tagged<S> {
    inner: S,
    from: Connection,
}

impl<S: Sink<crate::Event> + Clone> Sink<crate::Event> for Tagged<S> {
    fn send(&self, event: crate::Event) -> Result<(), obelus_sink::Gone> {
        match event {
            crate::Event::Acp(incoming) => self.inner.send(crate::Event::Heard {
                from: self.from,
                incoming,
            }),
            other => self.inner.send(other),
        }
    }
}

/// Which request for a conversation this is, by the connection's own count.
///
/// Handed out by [`Talk::open`] and [`Talk::reopen`] and carried back on
/// [`Incoming::Started`], so that whoever asked can tell which answer is
/// theirs.
pub type Asking = u64;

/// A prompt typed before there was a session to send it in, and what
/// Obelus had to say about the conversation ahead of it.
type Held = (Vec<link::Said>, Option<String>);

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
    /// What a prompt to this agent may carry, once it has said.
    ///
    /// `None` until the handshake, which is not the same as "it takes
    /// nothing": a conversation's view opens before the process it starts
    /// has said a word, so a reader can paste a picture into a box
    /// belonging to an agent that has not answered yet. Answering that with
    /// the default would be refusing them on ignorance rather than on
    /// anything the agent said.
    carries: Option<link::Carries>,
    /// The ways it offers to be signed in to, once it has said.
    logins: Vec<link::Login>,
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
    /// The requests for a conversation that have not been answered yet,
    /// oldest first, each with what was said before its answer arrived.
    ///
    /// A queue and not one slot, because opening a conversation asks for
    /// one and a reader can open two faster than node answers: the words
    /// typed into the second went to whichever session arrived first, and
    /// that was the first conversation's. The thread answers these one at
    /// a time in the order they were made, so the answer at the front of
    /// the queue is the one arriving -- which is what makes a count enough
    /// to tell them apart, where the protocol names nothing.
    ///
    /// What was said waits here with whatever Obelus had to say about the
    /// conversation first, because the first thing said is what carries it.
    waiting: std::collections::VecDeque<(Asking, Option<Held>)>,
    /// The last number handed to a request for a conversation.
    askings: Asking,
    /// Which connection this is -- see [`Connection`].
    connection: Connection,
    /// The sessions of `thrown` that were opened to ask what the agent
    /// offers, whose word about that is still wanted after they have gone.
    ///
    /// An agent may say what it can be set to in the answer that opens a
    /// session or in an update a moment after it, and the second arrives
    /// once the session has been let go. Dropped with the rest, that answer
    /// read as "nothing", which is a claim about the agent and false.
    asked: std::collections::HashSet<SessionId>,
    /// The sessions opened only to ask what the agent offers, and let go.
    ///
    /// Kept for the life of the connection, because an agent is free to
    /// write about one after it has been let go -- the list of commands it
    /// takes usually arrives in the breath after the answer that named it
    /// -- and a word about a session nobody holds is a word to drop. Let
    /// through, it made a conversation of its own in `sessions` that
    /// nothing ever removed, and one saying what it offers was taken for a
    /// real one's and answered with the reader's choices, to a session the
    /// agent had already deleted.
    thrown: std::collections::HashSet<SessionId>,
    /// What Obelus wrote down as the name of each conversation it has asked
    /// to take up, until the agent says it has it.
    ///
    /// Because `session/load` replays what was said and is not obliged to
    /// send the title with it: a conversation taken up went by its note, or
    /// by nothing, and the next time it was written down its name was
    /// written down as nothing too. Kept by the session asked for and not by
    /// the request, so a conversation the agent no longer has and opens
    /// afresh in its place is not given the old one's name.
    named: std::collections::HashMap<SessionId, String>,
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
        events: impl Sink<crate::Event> + Clone,
    ) -> Self {
        let connection = CONNECTIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        let events = Tagged {
            inner: events,
            from: connection,
        };
        Self {
            id: id.to_string(),
            asks: link::start(command, arguments, root, events),
            info: None,
            carries: None,
            logins: Vec::new(),
            gone: None,
            sessions: std::collections::HashMap::new(),
            waiting: std::collections::VecDeque::new(),
            askings: 0,
            connection,
            asked: std::collections::HashSet::new(),
            thrown: std::collections::HashSet::new(),
            named: std::collections::HashMap::new(),
            turns: 0,
        }
    }

    /// Which connection this is, for telling its words from another's --
    /// see [`Connection`].
    #[must_use]
    pub const fn connection(&self) -> Connection {
        self.connection
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
    pub const fn carries(&self) -> Option<link::Carries> {
        self.carries
    }

    /// The ways it offers to be signed in to, as it said at the handshake.
    #[must_use]
    pub fn logins(&self) -> &[link::Login] {
        &self.logins
    }

    /// Signs in a way the agent does itself.
    pub fn sign_in(&self, method: &str) {
        let _ = self.asks.unbounded_send(Ask::SignIn {
            method: method.to_string(),
        });
    }

    /// Says the reader has signed in some way of their own, so what was
    /// waiting on it is asked again.
    pub fn signed_in(&self) {
        let _ = self.asks.unbounded_send(Ask::SignedIn);
    }

    /// Says the reader will not sign in.
    pub fn give_up_signing_in(&self) {
        let _ = self.asks.unbounded_send(Ask::GiveUp);
    }

    /// Whether this conversation exists, and so whether a prompt in it goes
    /// anywhere.
    #[must_use]
    pub fn is_started(&self, session: Option<&SessionId>) -> bool {
        session.is_some_and(|session| self.sessions.contains_key(session))
    }

    /// Whether a turn is in flight in this one, or -- before it has a
    /// session -- in the conversation that asked for one as `asking`.
    #[must_use]
    pub fn is_thinking(&self, session: Option<&SessionId>, asking: Option<Asking>) -> bool {
        // A prompt with nowhere to go yet counts. The reader pressed
        // enter and their words are on the page, so something is under
        // way from where they sit -- in the conversation being opened for
        // them, which is the one they typed into and no other.
        //
        // Which is not a corner. An agent replaying a conversation sends
        // every word of it before it answers the request that asked for
        // it, so the page fills and *then* the session arrives: a reader
        // looking at a conversation that is plainly all there types into
        // it, and until this that turn went out with nothing on screen
        // saying anything was happening.
        let held = asking.is_some_and(|asking| {
            self.waiting
                .iter()
                .any(|(waiting, held)| *waiting == asking && held.is_some())
        });
        held || self
            .session(session)
            .is_some_and(|open| open.turn.is_some())
    }

    /// Whether a conversation by this name is open on it.
    #[must_use]
    pub fn holds(&self, session: &SessionId) -> bool {
        self.sessions.contains_key(session)
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

    /// Asks for another conversation on this same process, and says which
    /// request this is.
    ///
    /// One agent holds a project's worth of context, and a second process
    /// to talk about a second note would pay for all of it twice.
    ///
    /// `tools` is where this conversation reaches Obelus's own tools: its
    /// own address, because one of them has to know who is calling.
    pub fn open(&mut self, tools: Option<String>) -> Asking {
        let asking = self.waiting_for_one();
        let _ = self.asks.unbounded_send(Ask::Open { tools });
        asking
    }

    /// Hands out the next number, and queues it for its answer.
    fn waiting_for_one(&mut self) -> Asking {
        self.askings += 1;
        self.waiting.push_back((self.askings, None));
        self.askings
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

    /// Lets one go: the note it was about has gone, or it was opened for a
    /// view the reader left without saying anything.
    ///
    /// Told to the agent rather than only forgotten here: an agent left
    /// holding conversations nobody can reach is the same complaint that
    /// got the language server killed on the way out.
    ///
    /// And forgotten here for good: an agent writes about a session in the
    /// breath after the answer that named it, and one let go a moment after
    /// it opened hears that word after it has gone -- folded back in, it was
    /// a conversation in the mirror for the rest of the connection.
    pub fn let_go(&mut self, session: &SessionId) {
        self.sessions.remove(session);
        self.thrown.insert(session.clone());
        let _ = self.asks.unbounded_send(Ask::Drop {
            session: session.clone(),
        });
    }

    /// Asks for one it had before, by the name Obelus wrote down.
    ///
    /// An agent that will not take it up -- it has forgotten, it never
    /// could -- opens a new one instead and says so, because a reader who
    /// pressed a key has to end up somewhere they can talk.
    ///
    /// Numbered like [`Talk::open`], because it is answered the same way:
    /// with the session it asked for, or -- where the agent will not --
    /// word that it has gone and then a new one, which is still the answer
    /// to this request.
    ///
    /// `title` is what it was called when it was written down, which the
    /// agent's own word about it replaces whenever that arrives.
    pub fn reopen(
        &mut self,
        session: &str,
        title: Option<String>,
        tools: Option<String>,
    ) -> Asking {
        let asking = self.waiting_for_one();
        if let Some(title) = title {
            self.named.insert(SessionId::new(session), title);
        }
        let _ = self.asks.unbounded_send(Ask::Reopen {
            session: SessionId::new(session),
            tools,
        });
        asking
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

    /// What the agent calls this conversation, once it has said -- or, for
    /// one taken up again, what it was called when Obelus wrote it down,
    /// until the agent says otherwise.
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
    /// `asking` is the request the conversation made for its session,
    /// which is what a prompt with no session yet is held against: it goes
    /// out with the answer to that request and no other.
    ///
    /// Says whether it went: a prompt that is being held is a prompt the
    /// view shows as sent, because the reader has finished with it either
    /// way.
    pub fn say(
        &mut self,
        session: Option<&SessionId>,
        asking: Option<Asking>,
        said: Vec<link::Said>,
        opening: Option<&str>,
    ) -> bool {
        let Some(id) = session
            .filter(|id| self.sessions.contains_key(*id))
            .cloned()
        else {
            let waiting = asking.and_then(|asking| {
                self.waiting
                    .iter_mut()
                    .find(|(waiting, _)| *waiting == asking)
            });
            // Written down because the ways out of here look the same on
            // screen: the reader's words are on the page whichever it was.
            // Which one it went is the first thing anybody asks when a
            // conversation goes quiet, and it is the one thing the reader
            // cannot see.
            let Some((_, held)) = waiting else {
                tracing::warn!(
                    asked_in = ?session.map(|id| id.0.to_string()),
                    asking,
                    "a prompt went nowhere: no conversation is open or being opened for it"
                );
                return false;
            };
            tracing::info!(
                asking,
                "a prompt is held: its conversation is still being opened"
            );
            // Held with its opening: the opening belongs to the first thing
            // said in a conversation, and the first thing said is exactly
            // what gets held while the session is still opening.
            *held = Some((said, opening.map(str::to_string)));
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
                said,
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
        self.waiting.clear();
        self.named.clear();
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
        // Before anything else looks at it: a session thrown away is one
        // whose every word is about nothing -- whatever kind of word it is,
        // because the answer to a turn that was running when a note went
        // is as much about it as an update. Except what one opened to ask
        // says it can be set to, which is the answer it was opened for.
        if let Some(session) = about(&incoming)
            && self.thrown.contains(session)
        {
            return match incoming {
                Incoming::Update {
                    session,
                    update: Update::Settings(offers),
                } if self.asked.contains(&session) => Some(Incoming::Offers { session, offers }),
                _ => None,
            };
        }
        match incoming {
            Incoming::Offers { session, offers } => {
                // And what it said before the answer that named it, which
                // is already in here as a conversation of its own -- and is
                // the answer, where the one that named it was empty.
                let early = self.sessions.remove(&session);
                self.thrown.insert(session.clone());
                self.asked.insert(session.clone());
                let offers = match (offers.is_empty(), early) {
                    (true, Some(early)) => early.settings,
                    (_, _) => offers,
                };
                Some(Incoming::Offers { session, offers })
            }
            Incoming::Ready {
                named,
                carries,
                logins,
            } => {
                self.info = named;
                self.carries = Some(carries);
                self.logins = logins;
                None
            }
            // Which request is waiting on it, where it was one: the oldest
            // not yet answered. They are answered in the order they were
            // made, so the one the agent refused is that one, and every
            // request after it waits behind it.
            Incoming::SignIn {
                session: None, why, ..
            } => Some(Incoming::SignIn {
                session: None,
                asking: self.waiting.front().map(|(asking, _)| *asking),
                why,
            }),
            Incoming::Started { session, mode, .. } => {
                // `or_default` rather than an insert, because this is not
                // always the first word about a conversation: an agent is
                // free to write its opening notification in the same breath
                // as the answer that names the session, and the two reach
                // the main loop in whichever order the connection settles
                // them. Whatever named it, the agent says it exists.
                let open = self.sessions.entry(session.clone()).or_default();
                open.legacy_mode = mode;
                // Unless the agent named it during the replay, which is the
                // newer of the two.
                if let Some(title) = self.named.remove(&session) {
                    open.title.get_or_insert(title);
                }
                open.merge();
                // Which request this answers: the oldest, because they are
                // answered in the order they were made.
                let answered = self.waiting.pop_front();
                // And what was typed into that conversation before there
                // was anywhere to send it.
                let asking = answered.as_ref().map(|(asking, _)| *asking);
                if let Some((held, opening)) = answered.and_then(|(_, held)| held) {
                    self.say(Some(&session), None, held, opening.as_deref());
                }
                Some(Incoming::Started {
                    session,
                    mode: None,
                    asking,
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
                // what the settings page lists, and what the reader's
                // standing choices are matched against. Folded away here,
                // both happened only at the moment a session opened, which
                // is before an agent has said what it offers.
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
                // No turn ends here. A turn that fails ends as itself, in
                // `Ended`, because the prompt's answer knows whose it was;
                // what is left to fail is a setting, a mode, a question
                // Obelus could not put -- none of which stopped the agent.
                // This used to stop every conversation's turn on the grounds
                // that it could not tell which one a failure was about, and
                // every one of them went on working with nothing on screen
                // saying so.
                for open in self.sessions.values_mut() {
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
                self.waiting.clear();
                self.named.clear();
                self.gone = Some(why.clone());
                Some(Incoming::Gone(why))
            }
            Incoming::Lost { session, why } => {
                // Refused, so the name it was asked under is nobody's: the
                // one opened in its place arrives under another.
                self.named.remove(&session);
                Some(Incoming::Lost { session, why })
            }
            other => Some(other),
        }
    }
}

/// The session a word from the agent is about, where it names one.
fn about(incoming: &Incoming) -> Option<&SessionId> {
    match incoming {
        Incoming::Started { session, .. }
        | Incoming::Update { session, .. }
        | Incoming::Ended { session, .. }
        | Incoming::Lost { session, .. }
        | Incoming::Asked { session, .. }
        | Incoming::Withdrawn { session }
        | Incoming::Remembered { session, .. }
        | Incoming::SignIn {
            session: Some(session),
            ..
        } => Some(session),
        // Its own session is what it is the answer about, and forgetting
        // that session is the answer's own business.
        Incoming::Offers { .. } => None,
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A handle with no agent behind it: what it is sent goes nowhere,
    /// and what it is told is whatever the test says.
    fn detached() -> Talk {
        let (asks, _) = mpsc::unbounded();
        Talk {
            id: "fake".to_string(),
            asks,
            info: None,
            carries: None,
            logins: Vec::new(),
            gone: None,
            sessions: std::collections::HashMap::new(),
            waiting: std::collections::VecDeque::new(),
            askings: 0,
            connection: 0,
            asked: std::collections::HashSet::new(),
            thrown: std::collections::HashSet::new(),
            named: std::collections::HashMap::new(),
            turns: 0,
        }
    }

    /// A session let go is forgotten, whatever the agent says about it
    /// afterwards.
    ///
    /// An agent writes about a session in the breath after the answer that
    /// named it, and one let go a moment after it opened -- a conversation
    /// the reader left without a word -- hears that word after it has gone.
    /// Folded back in, every one of those was a conversation in the mirror
    /// for the rest of the connection.
    ///
    /// Deliberate break: take the `thrown` out of `let_go`, and the word
    /// that arrives afterwards makes it a conversation that is held.
    #[test]
    fn a_session_let_go_is_forgotten() {
        let mut talk = detached();
        let gone = SessionId::new("s-1");
        talk.on(Incoming::Started {
            session: gone.clone(),
            mode: None,
            asking: None,
        });
        talk.let_go(&gone);
        talk.on(Incoming::Update {
            session: gone.clone(),
            update: Update::Orders(Vec::new()),
        });
        assert!(!talk.holds(&gone), "what it said afterwards is held");
    }

    /// A session let go stays let go whatever kind of word arrives about
    /// it, not only an update.
    ///
    /// Deliberate break: ask only an `Update` whether its session was
    /// thrown away, the way it did. The answer to a turn that was running
    /// when the note went makes the session a conversation that is held.
    #[test]
    fn every_word_about_a_session_let_go_is_dropped() {
        let mut talk = detached();
        let gone = SessionId::new("s-1");
        talk.on(Incoming::Started {
            session: gone.clone(),
            mode: None,
            asking: None,
        });
        talk.let_go(&gone);
        talk.on(Incoming::Ended {
            session: gone.clone(),
            turn: 1,
            why: Ok("end_turn".to_string()),
        });
        assert!(!talk.holds(&gone), "the turn's answer made it held again");
    }

    /// A session opened only to ask is forgotten, whatever the agent says
    /// about it and whenever it says it.
    ///
    /// Both orders, because the agent picks: what it writes in the breath
    /// before the answer that names the session reaches here first as
    /// often as not. Deliberate break: take the `remove` out of the
    /// `Offers` arm and the first half fails -- the commands said early
    /// made a conversation nothing ever let go. Take out the check at the
    /// top of `on` and the second does, and a word about a deleted
    /// session is passed up to be answered.
    ///
    /// Except the one word it was opened for: what it can be set to, said
    /// after the answer that named it, is that answer -- see `asked`. Take
    /// it back out of the check, and the settings said late are dropped.
    #[test]
    fn a_session_opened_to_ask_is_forgotten() {
        let mut talk = detached();
        let thrown = SessionId::new("s-1");
        let early = talk.on(Incoming::Update {
            session: thrown.clone(),
            update: Update::Orders(Vec::new()),
        });
        assert!(early.is_none());
        let answered = talk.on(Incoming::Offers {
            session: thrown.clone(),
            offers: Vec::new(),
        });
        assert!(matches!(answered, Some(Incoming::Offers { .. })));
        assert!(!talk.holds(&thrown), "what it said early is still held");

        let late = talk.on(Incoming::Update {
            session: thrown.clone(),
            update: Update::Orders(Vec::new()),
        });
        assert!(late.is_none(), "a word about it was passed up");
        assert!(!talk.holds(&thrown), "what it said late is held");

        let offered = talk.on(Incoming::Update {
            session: thrown.clone(),
            update: Update::Settings(Vec::new()),
        });
        assert!(
            matches!(offered, Some(Incoming::Offers { .. })),
            "what it said it can be set to, late, was not its answer"
        );
        assert!(!talk.holds(&thrown), "what it offered late is held");
    }

    /// The name an agent gives a conversation while replaying it beats the
    /// one Obelus wrote down.
    ///
    /// The replay arrives before the answer that says the session is taken
    /// up, so the agent's word is already here when the written-down one
    /// would go in -- and it is the newer of the two.
    ///
    /// Deliberate break: have `Started` put the written-down name in with
    /// `open.title = Some(title)` rather than `get_or_insert`, and the
    /// agent's new name is replaced by the old one.
    #[test]
    fn a_name_the_agent_gives_while_replaying_beats_the_one_written_down() {
        let mut talk = detached();
        let session = SessionId::new("s-1");
        talk.reopen("s-1", Some("old".to_string()), None);
        talk.on(Incoming::Update {
            session: session.clone(),
            update: Update::Titled("new".to_string()),
        });
        talk.on(Incoming::Started {
            session: session.clone(),
            mode: None,
            asking: None,
        });
        assert_eq!(talk.title(Some(&session)), Some("new"));
    }
}
