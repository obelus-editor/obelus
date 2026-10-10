//! What is said between Obelus and an agent, in the words the rest of
//! Obelus reads.
//!
//! The protocol's crate has a type for every message; these are what the
//! application, the conversation and its views hold instead -- [`Ask`] for
//! what Obelus wants, [`Incoming`] for what the agent did -- so that only
//! [`super::link`] and [`super::read`] know the wire's shapes. The one of
//! the protocol's own types that crosses is a session's name.
//!
//! **A command is the agent's namespace; a setting is Obelus's to draw.**
//! Two things in the protocol, and they must not be mistaken for each
//! other. An agent's slash commands ([`Order`]) are names it takes *in a
//! prompt* -- a client offers them and sends the text, and that is all.
//! Session *config options* ([`Setting`]) are the other kind: `session/new`
//! and `session/update` carry the whole set, `session/set_config_option`
//! changes one, the answer is the whole set again because one value can
//! change what another offers, and the client draws them itself (a boolean
//! one only if it advertised `session.configOptions.boolean`). So the
//! conversation's status row is every option with its current value,
//! walked and changed there -- not a command, because the keys that move
//! what is on a screen belong to that screen, the way `shift+tab` always
//! has.
//!
//! Obelus used to take `/model` for itself: the agent's command and
//! Obelus's setting had the same name, and Copilot's own answer to that
//! command is "the model-picker dialog is only available in the interactive
//! CLI", so opening the setting's values instead looked like a kindness. It
//! was a guess about somebody else's namespace -- nothing promises that a
//! command means what an option of the same name means -- and it is gone.
//! `/model` goes to the agent, whose answer is its own business; the same
//! choice is one key away on the row.
//!
//! What Copilot offers, probed at 1.0.83: `mode`, `model`,
//! `reasoning_effort` and `allow_all`. It does not elicit for `/model`
//! either -- with `elicitation.form` advertised it still answers in words.
//! Its mode ids are URLs, and most of its rows describe themselves with
//! their own name, which is why a description that repeats the name is
//! dropped (`said_twice`). What kind each option is declared as is written
//! to the log as it arrives (`setting_of`): how an agent declares one
//! decides how it is drawn and what enter does to it, so that line is
//! where "why is this one drawn like that" is answered.

use std::path::PathBuf;

use agent_client_protocol::schema::v1::SessionId;
use futures::channel::oneshot;

/// Which turn of a conversation something is about.
///
/// Obelus's own count and nothing the agent ever sees. The protocol has no
/// name for a turn -- `session/prompt` answers with a stop reason, and
/// `session/cancel` names a session -- so a client with two of them about
/// one conversation cannot tell two answers apart. This is the name Obelus
/// gives them so that it can.
pub type Turn = u64;

/// What Obelus asks the agent to do.
///
/// Sent from the main loop, read by the thread. Not requests: what comes
/// back comes back as an [`Event`], because by then the reader may be
/// looking at something else entirely.
///
/// [`Event`]: crate::Event
#[derive(Clone, Debug)]
pub enum Ask {
    /// Open another conversation on this connection.
    ///
    /// One process, several sessions: an agent holds a project's worth of
    /// context and starting a second of them to talk about a second note
    /// would pay for all of it twice.
    Open {
        /// Where this conversation reaches Obelus's own tools, if it does.
        ///
        /// Its own address and not the server's: one tool has to know
        /// which conversation is calling it, and MCP has no word for that.
        tools: Option<String>,
    },
    /// Let one go, because the note it was about has gone.
    ///
    /// Told to the agent rather than only forgotten here, because an agent
    /// left holding conversations nobody can reach is the same complaint
    /// that got the language server killed on the way out.
    Drop {
        /// Which conversation.
        session: SessionId,
    },
    /// Take up one the agent already has, from a previous sitting.
    ///
    /// The agent kept every word of it, so Obelus keeps none: what it keeps
    /// is the name, because the agent has no idea a note exists.
    Reopen {
        /// The name Obelus wrote down last time.
        session: SessionId,
        /// Where it reaches Obelus's own tools, as [`Ask::Open`] has it.
        tools: Option<String>,
    },
    /// Say this, in that conversation.
    Say {
        /// Which conversation.
        session: SessionId,
        /// Which turn of it, by Obelus's own count.
        ///
        /// The protocol numbers nothing: a prompt's answer says the turn is
        /// over and names only the session, so with two of them about one
        /// conversation there is no telling which answer belongs to which
        /// question. Obelus counts them itself and puts the number back on
        /// the answer, which is the same trick a language server's version
        /// is.
        turn: Turn,
        /// What to say, in the order the reader put it together.
        ///
        /// A list rather than words and a bag of pictures beside them,
        /// because where a picture sits is part of what the reader said:
        /// "the one below" and "the one above" are about the block after
        /// and the block before, and a prompt that gathered the pictures at
        /// one end would make both of those wrong. So `abc`, a picture,
        /// `def`, a picture is four blocks and stays four blocks.
        said: Vec<Said>,
        /// What Obelus has to say about the conversation first, once.
        ///
        /// Its own block rather than stuck to the front of the words: the
        /// protocol takes a prompt as blocks, and one of these is the
        /// reader talking while the other is Obelus saying what they are
        /// talking about.
        opening: Option<String>,
    },
    /// Open a conversation only to find out what it can be set to, and
    /// let it go again.
    ///
    /// A session of its own and never one the reader is talking in. The
    /// protocol says what an agent can be set to in the answer to
    /// `session/new` and nowhere else -- `initialize` carries the
    /// capabilities and the ways to sign in, and neither of those is this
    /// -- so finding out means opening one. Opening the reader's instead
    /// would mean their conversation existing because a settings page
    /// wanted a list, and going wherever that list went.
    Offers,
    /// Stop what you are doing in that one.
    Interrupt {
        /// Which turn Obelus is giving up on, for the end it writes itself.
        turn: Turn,
        /// Which conversation.
        session: SessionId,
    },
    /// Work this way from now on, in that one.
    Mode {
        /// Which conversation.
        session: SessionId,
        /// Which way.
        mode: String,
    },
    /// Put one of a session's settings on this value.
    Set {
        /// Which conversation.
        session: SessionId,
        /// Which setting, by the agent's id for it.
        setting: String,
        /// What to put it on.
        chosen: Chosen,
    },
    /// Sign in this way, which the agent does itself.
    ///
    /// Only for a way the agent named as its own: a way that is a program
    /// to run is run by Obelus, in a terminal of its own, and the agent is
    /// told nothing until [`Ask::SignedIn`].
    SignIn {
        /// Which way, by the agent's id for it.
        method: String,
    },
    /// The reader has signed in some way the agent was not part of, so what
    /// was waiting on it can be asked again.
    SignedIn,
    /// The reader will not sign in, so what was waiting on it never will be.
    GiveUp,
    /// Stop a piece of background work, in the dialect the agent speaks --
    /// see [`super::tasks`].
    StopTask {
        /// Which conversation started it.
        session: SessionId,
        /// Which piece of work, by the agent's id for it.
        id: String,
    },
}

/// A way the agent offers to be signed in to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Login {
    /// The agent's id for it.
    pub id: String,
    /// What to call it, in the agent's words.
    pub name: String,
    /// What it says about it, where it says anything.
    pub about: Option<String>,
    /// What signing in this way is.
    pub how: How,
}

/// What signing in one way is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum How {
    /// A program to run where the reader can answer it, and its ending well
    /// is the sign-in: `authenticate` is never sent for one of these.
    Run {
        /// The program.
        program: PathBuf,
        /// What it is told.
        arguments: Vec<String>,
        /// What it is given on top of Obelus's own environment.
        env: Vec<(String, String)>,
    },
    /// The agent's own business, asked for with `authenticate`.
    Asked,
}

/// What a setting is being put on.
///
/// Two shapes because the protocol has two: a value chosen from a list, and
/// a switch. Which one a setting takes is the setting's own business, so it
/// is decided where the setting is known rather than here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Chosen {
    /// One of a list, by the agent's id for it.
    Value(String),
    /// A switch, on or off.
    Switch(bool),
}

impl Chosen {
    /// What putting a setting on one of its values means over the wire.
    ///
    /// A switch takes a boolean and a list takes an id, and which one a
    /// setting takes is the setting's own business -- so it is answered
    /// here, where what Obelus calls the two sides of a switch is also
    /// written down.
    #[must_use]
    pub fn of(setting: &Setting, value: &str) -> Self {
        match setting.kind {
            Kind::Switch => Self::Switch(value == ON),
            Kind::Select => Self::Value(value.to_string()),
        }
    }
}

/// How Obelus answers something the agent asked.
///
/// One value, once. `None` is a refusal: no option chosen, or no text to
/// hand over -- both of which the protocol has an answer for.
pub type Answer<T> = oneshot::Sender<T>;

/// What a prompt may carry, as the agent said in the handshake.
///
/// The protocol asks a client to send only what was advertised, and the
/// reason is the reader's: an agent handed a picture it cannot read
/// answers about the words around it and says nothing about the picture,
/// which looks like the picture arriving and being ignored. So this is
/// asked before the paste is offered, not after it fails.
///
/// Audio is not here. Obelus has no way to put any in a prompt, and a
/// field nothing reads is indistinguishable from a broken feature.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Carries {
    /// Whether a picture may go in one.
    pub image: bool,
    /// Whether a file's contents may be carried in one, rather than named
    /// and left for the agent to read.
    pub embedded: bool,
}

/// One piece of what the reader put together, in the order they put it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Said {
    /// A run of what they typed.
    Words(String),
    /// A picture they put in between two of those runs.
    Picture(Picture),
}

/// A picture on its way to an agent.
///
/// The bytes as they came off the clipboard, not base64: the encoding is
/// the wire's and belongs where the block is built, which is the one place
/// that knows the protocol wants a string. Carried this far as bytes so
/// that nothing in between has to know either.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    /// What shape it is in, as the clipboard named it.
    pub mime: String,
    /// The picture itself.
    pub bytes: Vec<u8>,
}

/// What [`Incoming::Failed`] says it was doing when asking what the agent
/// offers did not work.
///
/// Named, because the application has to tell this failure from the rest:
/// the settings page is waiting on it, and says it is asking for as long
/// as nothing says otherwise.
pub const ASKING_WHAT_IT_OFFERS: &str = "Asking what it can be set to";

/// One thing from the agent worth acting on.
#[derive(Debug)]
pub enum Incoming {
    /// The handshake finished: what it calls itself, if it said, and what
    /// a prompt to it may carry.
    Ready {
        /// What it calls itself.
        named: Option<String>,
        /// What may go in a prompt.
        carries: Carries,
        /// The ways it offers to be signed in to, which are only asked for
        /// once the agent has said that it needs it.
        logins: Vec<Login>,
    },
    /// The agent will not go on until the reader has signed in.
    ///
    /// Said from a conversation being opened, or taken up again, or from a
    /// turn. What was being opened waits for the sign-in rather than ending
    /// the connection, which is what an error opening one does; a turn has
    /// already ended, and is the reader's to say again.
    SignIn {
        /// The conversation whose turn it was, where it was a turn.
        session: Option<SessionId>,
        /// Which request for a conversation is waiting on it, where one is
        /// -- counted at the other end, like [`Incoming::Started`]'s.
        asking: Option<super::Asking>,
        /// How many requests for a conversation wait on the sign-in ahead
        /// of this one: none for the one the agent refused, and one more
        /// for every one held behind it, so each conversation waiting is
        /// told and not only the first.
        behind: usize,
        /// What the agent said about it, where it said more than that it
        /// needs it.
        why: Option<String>,
    },
    /// The agent has signed the reader in.
    SignedIn,
    /// There is a session to talk in.
    Started {
        /// Which one, which is what everything said in it names.
        session: SessionId,
        /// The mode it offers through the dedicated methods, if it offers
        /// one that way -- read into a setting like any other, and dropped
        /// if the settings turn out to carry the mode themselves.
        mode: Option<Setting>,
        /// Which request for a conversation this answers, by the count
        /// `Talk::open` and `Talk::reopen` hand out.
        ///
        /// Nothing from here: this thread answers the requests in the
        /// order they were made, one at a time, so which one this is can be
        /// counted off at the other end -- see `Talk::on`.
        asking: Option<super::Asking>,
    },
    /// The agent wants to write a file.
    Write {
        /// Which file, as the agent named it.
        path: std::path::PathBuf,
        /// What it should say afterwards. The whole of it: the protocol has
        /// no way to say "this part", so an agent that changed one line
        /// sends the file back with that line changed.
        text: String,
        /// Whether it was allowed to.
        answer: Answer<bool>,
    },
    /// Something to show, in one of the conversations.
    Update {
        /// Which one it belongs to. The protocol puts it on every message
        /// Obelus reads and Obelus threw it away, which was free while
        /// there was one conversation and is the whole of the routing now.
        session: SessionId,
        /// What to show.
        update: Update,
    },
    /// A turn ended, for this reason.
    Ended {
        /// Whose turn.
        session: SessionId,
        /// And which one of that conversation's, by Obelus's count.
        ///
        /// Because the answer itself does not say. An answer to a turn that
        /// is no longer the one running is an answer about something nobody
        /// is waiting for, and it used to end the turn that had replaced it.
        turn: Turn,
        /// And why it stopped: the agent's stop reason as the wire spells
        /// it, or what went wrong where the turn never got an answer.
        ///
        /// A failure is the turn's own end and not a [`Self::Failed`],
        /// which names no conversation: read as one, it stopped every
        /// conversation's turn while the agent went on working in the
        /// others, and nothing on screen said so.
        why: Result<String, String>,
    },
    /// What the agent says it can be set to, asked on a session of its own
    /// and with nothing else in it.
    ///
    /// About the *agent*, not a conversation -- which is what a page about
    /// what conversations should start on is about. The session is named
    /// all the same, and only so that it can be forgotten: the agent is
    /// free to write about it before and after the answer that names it,
    /// and what it writes is about a conversation nobody holds. See
    /// `Talk::on`.
    Offers {
        /// The session that was opened to ask, and let go.
        session: SessionId,
        /// What it offers.
        offers: Vec<Setting>,
    },
    /// Something did not work: what Obelus was doing, and what it said.
    Failed(&'static str, String),
    /// A conversation Obelus asked to pick up again is not there any more.
    ///
    /// Its own message rather than a [`Self::Failed`], because it names
    /// which conversation: a failure that says only *what* went wrong lands
    /// in whichever one the reader happens to be looking at, and the one it
    /// is about is left waiting for a session that is never coming.
    ///
    /// A fresh one is opened straight after, so what this asks for is that
    /// the conversation go back to how a conversation with no session yet
    /// looks -- which is what the session about to arrive is expecting to
    /// find.
    Lost {
        /// The one that could not be picked up.
        session: SessionId,
        /// What the agent said about it.
        why: String,
    },
    /// The agent is asking the reader something, and waits for the
    /// answer.
    Asked {
        /// Which conversation it is asking in.
        session: SessionId,
        /// What it is asking.
        question: Question,
    },
    /// It took back something it had asked the reader, before they
    /// answered.
    ///
    /// Which one is not said, because it does not need to be: the answer
    /// channel of a question taken back has nobody at the other end now,
    /// and that is the mark it carries.
    Withdrawn {
        /// Which conversation it was asked in.
        session: SessionId,
    },
    /// A conversation taken up again by an agent that keeps its context and
    /// cannot send back what was said.
    ///
    /// After [`Incoming::Started`], and only where it is true. The page is
    /// empty and the agent is not, which somebody has to say: a reader
    /// looking at an empty page explains the whole thing again to an agent
    /// that already knows it.
    Remembered {
        /// Which conversation.
        session: SessionId,
    },
    /// A question the agent has stopped needing an answer to, because it
    /// watched the far end and saw it happen.
    Finished {
        /// Which question, by the name it was asked under.
        id: String,
    },
    /// The agent wants a file's text.
    Read {
        /// Which file.
        path: PathBuf,
        /// The first line it wants, counted from one.
        line: Option<u32>,
        /// How many lines.
        limit: Option<u32>,
        /// The text, or nothing for "Obelus will not read that".
        answer: Answer<Option<String>>,
    },
    /// It wants a command run.
    ///
    /// Not a question for the reader. The agent asks before it does
    /// anything it thinks is worth asking about -- that is what
    /// `session/request_permission` is -- and a client that asked again
    /// would be a second question about one thing. What Obelus owes is
    /// that the command is on the page in the words it was run in, and
    /// that a key stops it: what cannot be undone has to be visible while
    /// it happens.
    Run {
        /// The command line, as the agent wrote it.
        command: String,
        /// Its arguments, where the agent kept them apart.
        args: Vec<String>,
        /// What to set in its environment, beside Obelus's own.
        env: Vec<(String, String)>,
        /// Where to run it, or the tree when the agent did not say.
        cwd: Option<PathBuf>,
        /// How much of its output to keep.
        limit: Option<usize>,
        /// What Obelus calls it, or nothing where it would not start.
        answer: Answer<Option<String>>,
    },
    /// What a command has written so far.
    Wrote {
        /// Which command.
        id: String,
        /// Its output, whether it was cut, and how it ended if it has.
        answer: Answer<Option<(String, bool, Option<crate::running::Ended>)>>,
    },
    /// Wait for a command to end.
    ///
    /// Answered when it does, which may be minutes -- on a task of its own,
    /// so that the agent's other conversations are not held for them.
    Waited {
        /// Which command.
        id: String,
        /// How it ended, or nothing for a command Obelus has no record of.
        answer: Answer<Option<crate::running::Ended>>,
    },
    /// Stop one.
    Stop {
        /// Which command.
        id: String,
        /// Said once it has stopped, so the output asked for next is the
        /// output of something that is no longer writing.
        answer: Answer<()>,
    },
    /// Let go of one: stopped if it is still going, and forgotten.
    Forget {
        /// Which command.
        id: String,
        /// Said once it is gone.
        answer: Answer<()>,
    },
    /// What this connection hears of background work, which is a fact about
    /// the agent and not about any one conversation.
    ///
    /// Said once the handshake has settled which dialect the agent speaks,
    /// and again when that changes: a dialect given up on because it could
    /// not be read, an agent that turned out not to stop what it said could
    /// be stopped.
    Tasks {
        /// Whether Obelus hears of background work at all.
        heard: bool,
        /// Whether it can ask for any to be stopped.
        stoppable: bool,
    },
    /// Asking for a piece of background work to stop came to nothing.
    ///
    /// Not a failure to show: the agent says it could not, or never said
    /// anything Obelus could read, and what became of the work is whatever
    /// its next update says. What this ends is Obelus saying it is stopping.
    NotStopped {
        /// Which conversation started it.
        session: SessionId,
        /// Which piece of work.
        id: String,
    },
    /// The conversation is over: the agent exited, or the protocol did.
    Gone(Option<String>),
}

/// What the agent can ask the reader: one card's worth.
///
/// Apart from [`Incoming`] because it is kept: a question asked while another
/// is up waits behind it, and what waits is only ever one of these.
#[derive(Debug)]
pub enum Question {
    /// To be allowed something.
    Permission {
        /// The call it is asking about, which is the same call the
        /// transcript already has a row for -- or is about to.
        /// Boxed, like the one on [`Update::Tool`] and for the same
        /// reason: this enum travels inside an event, where every other
        /// variant is a key or a path.
        call: Box<Call>,
        /// And in its own words: which command, which file -- what the
        /// reader is actually being asked about.
        reason: Option<String>,
        /// What Obelus may answer.
        options: Vec<Choice>,
        /// Which option, or nothing for "not answered".
        answer: Answer<Option<String>>,
    },
    /// That the reader go and do something on the web: sign in
    /// somewhere, authorise something.
    ///
    /// Answered by going, not by finishing: the agent watches for the far
    /// end itself and says when it is done. See [`Incoming::Finished`].
    Open {
        /// What it says this is for, in its own words.
        message: String,
        /// Where to send them. Checked before it gets here: `http` or
        /// `https`, and a host.
        url: String,
        /// The agent's name for this question, which is how it later says
        /// the question is answered.
        id: String,
        /// `true` once the reader has been sent there, `false` if they will
        /// not go. Dropped without an answer means the question went away,
        /// which the agent hears as a cancellation.
        answer: Answer<bool>,
    },
    /// For something: a form, filled in a field at a time.
    Ask {
        /// What it says it needs, in its own words.
        message: String,
        /// What it wants, in the order Obelus will put them.
        fields: Vec<Field>,
        /// Every field's answer, or nothing for "not answered".
        answer: Answer<Option<Vec<(String, Reply)>>>,
    },
}

/// One thing the agent has to say, in the form the view shows it.
///
/// Not all of it is speech: what it is working through, how full it is,
/// what it lets the reader change. What they have in common is that they
/// arrive on the same notification and are about one conversation.
///
/// Not `Eq`: what a turn has cost is a number of money, which the protocol
/// carries as a float, and two of those are never exactly the same thing.
#[derive(Clone, Debug, PartialEq)]
pub enum Update {
    /// A piece of the answer.
    Said(String),
    /// A piece of its thinking, which agents send separately so that it can
    /// be shown as what it is.
    Thought(String),
    /// A piece of what the *reader* said, as the agent has it.
    ///
    /// Which sounds like news Obelus already has, and in a live turn it is
    /// -- it put those words there itself. The turn this is for is the one
    /// nobody was here for: `session/load` replays a conversation to a
    /// client that may be a fresh process, and the reader's own half comes
    /// back only this way. Without it a conversation taken up again is a
    /// run of answers with no questions above them.
    Heard(String),
    /// It is using a tool, and this is where it has got to.
    Tool {
        /// The call itself.
        ///
        /// Boxed: a call carries six fields of its own and this enum
        /// travels on the one channel the loop reads, where every other
        /// variant is a key or a path -- one fat arm makes every event on
        /// that channel this size.
        call: Box<Call>,
        /// `pending`, `in_progress`, `completed` or `failed`. Empty on an
        /// update that did not say, which means it has not changed.
        status: String,
    },
    /// What it means to do about this turn, and how far along it is.
    ///
    /// The whole list every time: the protocol says the agent sends all of
    /// the entries with their current status and the client replaces what
    /// it had, so there is nothing here to merge and nothing to keep in
    /// step.
    Plan(Vec<Step>),
    /// The way of working changed, which the agent can do on its own.
    Mode(String),
    /// The commands it takes, sent once the session is ready and again
    /// whenever they change.
    Orders(Vec<Order>),
    /// What the agent calls this conversation.
    ///
    /// The agent's own name for it, which it usually sends once the first
    /// thing has been said: the note a conversation is about says what the
    /// reader meant to do, and this says what the conversation turned into.
    Titled(String),
    /// The settings it lets the reader change, sent when the session opens
    /// and again after every change -- by Obelus or by the agent itself.
    Settings(Vec<Setting>),
    /// How much of what the agent can hold this conversation is using, and
    /// what it has cost. Sent several times a turn.
    Used(Usage),
    /// Something about a piece of work the agent goes on with after the
    /// call that started it returned -- see [`super::tasks`].
    Task(super::tasks::News),
}

/// How full the agent's memory of this conversation is, and what it has
/// cost so far.
///
/// The agent sends this as it works, so it is a fact about the session
/// rather than a thing said: it never goes in the transcript.
#[derive(Clone, Debug, PartialEq)]
pub struct Usage {
    /// What the conversation is taking up now, in the agent's own tokens.
    pub used: u64,
    /// How much it can take before the agent starts forgetting.
    pub room: u64,
    /// What it has cost so far, where the agent says -- not every one does.
    pub cost: Option<Cost>,
}

/// What a session has cost, as the agent counts it.
#[derive(Clone, Debug, PartialEq)]
pub struct Cost {
    /// The amount, in whatever the currency is.
    pub amount: f64,
    /// Which currency, as an ISO 4217 code. Passed through rather than
    /// turned into a sign: Obelus does not know every currency's, and one
    /// it guessed wrong would be a number about the wrong money.
    pub currency: String,
}

/// One thing about the session the agent lets the reader change.
///
/// The model, how hard it thinks, whether it asks before doing things: the
/// agent names them, Obelus lists them. Which are on offer is the agent's,
/// and so is what each of them means -- Obelus only shows the names and
/// sends back the id of what was chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setting {
    /// The agent's id for it, which is what a change names.
    pub id: String,
    /// What to call it on screen.
    pub name: String,
    /// One line about it, if it said.
    pub about: Option<String>,
    /// Every value it can take, in the agent's own order.
    pub values: Vec<Value>,
    /// Which value is on, by its id.
    pub current: String,
    /// Which of the two shapes it is, which decides both how it is drawn
    /// and what happens when a reader presses enter on it.
    pub kind: Kind,
    /// What sort of thing it is about, as the agent itself says.
    pub category: Category,
    /// Set with `session/set_mode` rather than `session/set_config_option`.
    ///
    /// True only of the mode an agent offers through the older, dedicated
    /// methods, which Obelus reads into a setting like any other. The
    /// protocol is dropping those methods; this is the one field that
    /// remembers which door a setting goes back out of, and when they are
    /// gone it is the only thing to delete.
    pub legacy: bool,
}

/// Which of the two shapes a [`Setting`] is.
///
/// The protocol has exactly these two, and the difference is not
/// decoration: a list of choices is a list to open, and a switch has
/// nothing to open because there is nowhere else for it to go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// One value out of several.
    Select,
    /// On or off, which Obelus offers as two values of its own making.
    Switch,
}

/// What a setting is about, in the protocol's own words.
///
/// `category` exists for exactly this -- the spec's own list of what a
/// client may do with it is "keyboard shortcuts, icons, placement" -- and
/// it says a client must work without it. So Obelus uses it for a glyph,
/// for which setting `shift+tab` steps, and for nothing that would be
/// wrong if an agent said nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    /// The way of working: what the dedicated mode methods used to carry.
    Mode,
    /// Which model answers.
    Model,
    /// Something about the model.
    ModelConfig,
    /// How hard it thinks.
    ThoughtLevel,
    /// Something else, or nothing said. Every unknown category is this
    /// one: the name of a category Obelus has never heard of tells it no
    /// more than silence does.
    Other,
}

/// One value a [`Setting`] can be put on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Value {
    /// The agent's id for it, which is what a change names.
    pub id: String,
    /// What to call it on screen.
    pub name: String,
    /// One line about it, if it said.
    pub about: Option<String>,
}

impl Setting {
    /// What the value that is on is called, for the row that says so.
    #[must_use]
    pub fn current_name(&self) -> Option<&str> {
        self.name_of(&self.current)
    }

    /// What one of its values is called, by the agent's id for it -- or
    /// nothing, where it does not offer that value.
    #[must_use]
    pub fn name_of(&self, value: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|offered| offered.id == value)
            .map(|offered| offered.name.as_str())
    }
}

/// One command the agent offers, of the kind typed with a slash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Order {
    /// Its name, without the slash.
    pub name: String,
    /// One line about what it does.
    pub description: String,
    /// What it says about whatever is typed after the name.
    pub hint: Option<String>,
}

/// One thing an agent asked the reader for.
///
/// A field of a form, in the shape Obelus can put it: what to call it, what
/// sort of answer it takes, and the name the answer goes back under.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    /// Its name in the schema, which is what keys the answer.
    pub name: String,
    /// What to call it on screen.
    pub title: String,
    /// One line about it, if the agent said.
    pub about: Option<String>,
    /// What sort of answer it takes.
    pub takes: Takes,
    /// Whether the agent said it has to be answered.
    ///
    /// What it buys is a question the reader can walk past: a form with an
    /// optional "anything else?" on it is a form where saying nothing is
    /// an answer, and a client that insists on one is a client the reader
    /// has to escape out of -- which gives up the whole form.
    pub required: bool,
}

/// What sort of answer a [`Field`] takes.
#[derive(Clone, Debug, PartialEq)]
pub enum Takes {
    /// One of these.
    One(Vec<Value>),
    /// Any of these, with how many of them the agent will take.
    Some {
        /// What there is to choose from.
        values: Vec<Value>,
        /// The fewest it will take, if it said.
        least: Option<u64>,
        /// And the most.
        most: Option<u64>,
        /// Which of them start chosen, by the agent's ids for them.
        chosen: Vec<String>,
    },
    /// On or off, starting here.
    Switch(bool),
    /// Words, starting with these if the agent suggested any.
    Words(Option<String>),
    /// A number, within these if it said.
    Number {
        /// Whether it has to be whole.
        whole: bool,
        /// The smallest it may be.
        least: Option<f64>,
        /// And the largest.
        most: Option<f64>,
    },
}

/// One thing an agent did, or is asking to do.
///
/// One type for both, because on screen they are one row: the agent sends a
/// call, asks permission for it, and updates it when it is done, all under
/// the same id.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Call {
    /// Its own id for the call, so a later update replaces the row rather
    /// than adding one.
    pub id: String,
    /// What it calls it. Empty on an update that did not say, which means
    /// it has not changed -- and the same for the two fields under it.
    pub title: String,
    /// What sort of thing it is doing: `read`, `edit`, `search`, `execute`
    /// and the rest of the protocol's own list.
    pub kind: String,
    /// The files it named, with the line where it said one.
    ///
    /// What makes a tool call something a reader can go to rather than
    /// something they can only read about: Obelus opens files for a living,
    /// and this is the agent saying which.
    pub places: Vec<Place>,
    /// The change it is making, when it said.
    pub change: Option<Change>,
    /// The command Obelus is running for it, where it is running one.
    ///
    /// The agent embeds one it asked for with `terminal/create`, so the
    /// call's row is where the command shows: what it is running, what it
    /// has printed, and whether it is still going. Obelus holds the
    /// process, so the row is filled from what Obelus has rather than from
    /// anything the agent sends.
    pub ran: Option<String>,
    /// What it said in words, which is not always nothing.
    ///
    /// A call may carry text as well as a diff -- the plan an agent asks
    /// leave to act on is a call of this shape, and so is anything whose
    /// result is prose rather than a file. Obelus kept the diff and threw
    /// the words away, so the row that was actually asking the reader
    /// something had nothing on it.
    ///
    /// Empty on an update that carried none, which means "the same as
    /// before" like every other field here.
    pub said: Vec<String>,
    /// Whether the work it started goes on after it, as the agent's dialect
    /// for background work says -- see [`super::tasks`].
    ///
    /// A call that returned while what it started keeps running says
    /// `completed` like any other, and a row that believed it would say the
    /// server is done. False where nothing said so, which is every agent
    /// that does not speak of background work at all.
    pub backgrounded: bool,
}

/// What the row of a call whose work goes on says, in place of the
/// protocol's `completed`.
///
/// Obelus's own word and not the wire's: the protocol has no state for a
/// call that has returned while what it started has not, and `completed`
/// is the one word that would be false.
pub const BACKGROUNDED: &str = "backgrounded";

/// One thing an agent means to do about the turn it is working on.
///
/// Not a note: a note is what the reader means to come back to next week,
/// and this is the agent's own list for the next two minutes. They look
/// alike and are not the same thing, which is why one is written to the
/// tree's file and the other is gone when the turn ends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    /// What it says it will do.
    pub said: String,
    /// `pending`, `in_progress` or `completed` -- the same words a tool
    /// call's state comes in, so the glyph that says how far along one is
    /// says it for the other.
    pub state: String,
    /// How important the agent thinks it is.
    ///
    /// Read and not drawn, which is a decision rather than an oversight:
    /// Obelus's own notes have no priority because the order is the
    /// reader's, and three shades of urgency on a list the reader cannot
    /// reorder is colour spent on something they cannot act on.
    pub priority: String,
}

/// A change to a file, as the agent describes it.
///
/// The file as it is and as it would be, which is what the protocol sends
/// rather than a patch: Obelus diffs the two with the engine it diffs
/// everything else with, so a change that has not happened is read the way
/// every change that has is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// Which file.
    pub path: PathBuf,
    /// What is in it now, or nothing where the file is new.
    pub before: Option<String>,
    /// What would be in it.
    pub after: String,
}

/// Somewhere in the project an agent said it was working.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    /// The file.
    pub path: PathBuf,
    /// The line it named, counted from one, if it named one.
    pub line: Option<u32>,
}

/// One answer to one [`Field`].
#[derive(Clone, Debug, PartialEq)]
pub enum Reply {
    /// One of a list, by the agent's id for it.
    Value(String),
    /// Several of a list, by the agent's ids for them.
    Values(Vec<String>),
    /// A switch.
    Switch(bool),
    /// Words.
    Words(String),
    /// A number.
    Number(f64),
    /// A whole number, which the protocol keeps apart from the other kind.
    Whole(i64),
}

/// One answer a permission request offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    /// The agent's id for it, which is what an answer names.
    pub id: String,
    /// What to call it on screen.
    pub name: String,
    /// `allow_once`, `allow_always`, `reject_once` or `reject_always`.
    pub kind: String,
}

/// What Obelus calls the setting it makes out of the old mode methods.
///
/// Only Obelus's own name for it -- the agent never sees it, because a
/// change to this one goes out as `session/set_mode` and names a mode
/// rather than a setting.
pub const MODE: &str = "mode";

/// What Obelus calls the two sides of a switch.
pub(crate) const ON: &str = "on";
/// The other one.
pub(crate) const OFF: &str = "off";
