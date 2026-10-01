//! The agent, on the other end of the protocol's own crate.
//!
//! `agent-client-protocol` is the protocol's reference implementation: it
//! spawns the agent, frames the messages, numbers the requests, and gives
//! every method of the protocol a type whose field names the compiler
//! checks. What it is built around is `async`, and Obelus's main loop is a
//! thread blocked on a channel -- so this is the join between them.
//!
//! One task runs the connection. It holds the whole of Obelus's side of
//! it: the handshake, every conversation opened on it, and a loop over the
//! [`Ask`]s Obelus sends it. In the other direction everything becomes an
//! [`Event`] on the loop's own channel, like the keyboard, the file walk and
//! the language servers. Every message in both directions names which
//! conversation it is about, because one agent holds several.
//!
//! On the one runtime Obelus waits on rather than a runtime of its own. It
//! had one of its own once, built on a thread whose whole job was to own
//! it, because the loop had none to offer; a connection is one task with a
//! handful in it either way, and what it wanted was somewhere to put them.
//! The channels stay `futures`' rather than tokio's -- that is what the
//! protocol's crate speaks, and a channel is runtime-agnostic anyway; tokio
//! is here to drive them and for nothing else.
//!
//! The two directions are not symmetrical, and that is the interesting
//! part. What Obelus *asks* is fire-and-forget: a prompt is spawned as a
//! task on the connection, so a cancellation typed while the agent is
//! thinking is read rather than queued behind it. What the agent asks --
//! permission, the text of a file -- is a question Obelus cannot answer
//! without the reader, so the handler sends the question to the main loop
//! with a [`oneshot`] to answer through, and waits. Waiting is right there:
//! the agent has stopped, and what it is waiting for is a keystroke. It is
//! also why [`Event`] is not `Clone`.
//!
//! What waiting costs is the whole connection, not that one request. The
//! crate hands each message to a handler and does not read the next until
//! the handler returns, so while a card is up nothing else from that agent
//! is read -- no answer, no tool call, no count of what it has used. That
//! is right for a question it has stopped on, and it is why a question
//! that only *sends* the reader somewhere is answered the moment they are
//! sent rather than when they come back: the second would hold the agent
//! shut for as long as a sign-in takes. It also means one agent cannot
//! have two questions up at once, whatever the protocol allows.
//!
//! One thing the crate does not promise: that a notification sent after a
//! request leaves after it. A cancellation typed in the same instant as a
//! prompt can reach the agent first. So an interruption does both halves --
//! it tells the agent *and* ends the turn on Obelus's side -- and a late
//! answer to a turn the reader stopped is dropped rather than shown.
//!
//! An agent that wants to ask something uses `elicitation/create`. That is
//! the one way it can put UI on a client's screen, and it is gated on a
//! capability: a mode not named in the handshake and an agent either falls
//! back or gives up. Obelus names both.
//!
//! A *form* is a flat set of primitives: one of a list, several of a list, a
//! switch, words, a number. The whole form goes back as one answer, keyed by
//! the agent's own names; escape declines it, and the view going away cancels
//! it, because an agent that hears nothing waits for ever. A property type
//! Obelus has never heard of is declined with the reason in the transcript
//! rather than half-filled in.
//!
//! A *url* is somewhere the reader has to go: to sign in, to authorise
//! something. Obelus is not a browser, which was once the reason not to
//! declare this at all -- but it does not have to be one. It shows the
//! address whole and hands it to whatever the machine opens links with,
//! which is the same thing it does with a file it cannot display. Only
//! `http` and `https`, and with a host: what happens to one of these is
//! that the machine runs whatever is registered for the scheme, and the
//! string came from the agent. Answered when the reader is sent, not when
//! they return -- see the paragraph above on what waiting costs -- and the
//! agent says the far end happened with `elicitation/complete`, which is a
//! notification because nothing is owed back.

use std::path::PathBuf;

use agent_client_protocol::{
    AcpAgentConfig, Client, ConnectionTo,
    schema::{
        ProtocolVersion,
        v1::{
            AvailableCommand, BooleanConfigOptionCapabilities, CancelNotification,
            ClientCapabilities, ClientSessionCapabilities, CloseSessionRequest,
            CompleteElicitationNotification, ContentBlock, CreateElicitationRequest,
            CreateElicitationResponse, CreateTerminalRequest, CreateTerminalResponse,
            DeleteSessionRequest, ElicitationAcceptAction, ElicitationAction,
            ElicitationCapabilities, ElicitationContentValue, ElicitationFormCapabilities,
            ElicitationMode, ElicitationPropertySchema, ElicitationSchema, ElicitationScope,
            ElicitationUrlCapabilities, FileSystemCapabilities, ImageContent, Implementation,
            InitializeRequest, KillTerminalRequest, KillTerminalResponse, LoadSessionRequest,
            McpCapabilities, McpServer, McpServerHttp, McpServerSse, MultiSelectItems,
            NewSessionRequest, NewSessionResponse, PermissionOptionId, PromptRequest,
            ReadTextFileRequest, ReadTextFileResponse, ReleaseTerminalRequest,
            ReleaseTerminalResponse, RequestPermissionOutcome, RequestPermissionRequest,
            RequestPermissionResponse, ResumeSessionRequest, SelectedPermissionOutcome,
            SessionConfigId, SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory,
            SessionConfigOptionValue, SessionConfigOptionsCapabilities, SessionConfigSelectOption,
            SessionConfigSelectOptions, SessionId, SessionModeState, SessionNotification,
            SessionUpdate, SetSessionConfigOptionRequest, SetSessionModeRequest,
            TerminalExitStatus, TerminalId, TerminalOutputRequest, TerminalOutputResponse,
            TextContent, ToolCallContent, ToolCallId, ToolCallLocation, ToolCallUpdateFields,
            WaitForTerminalExitRequest, WaitForTerminalExitResponse, WriteTextFileRequest,
            WriteTextFileResponse,
        },
    },
};
use futures::{
    StreamExt as _,
    channel::{mpsc, oneshot},
};
use obelus_sink::Sink;

use crate::Event;

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
#[derive(Clone, Debug)]
pub enum Ask {
    /// Open another conversation on this connection.
    ///
    /// One process, several sessions: an agent holds a project's worth of
    /// context and starting a second of them to talk about a second note
    /// would pay for all of it twice.
    Open,
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

/// One piece of what the reader said, as the protocol wants it.
///
/// Base64 here and nowhere earlier: this is the one place that knows the
/// wire takes a string, and a picture that crossed three layers already
/// encoded would be a megabyte of text being copied about for nothing.
fn block_of(said: Said) -> ContentBlock {
    use base64::Engine as _;
    match said {
        Said::Words(words) => ContentBlock::Text(TextContent::new(words)),
        Said::Picture(picture) => ContentBlock::Image(ImageContent::new(
            base64::engine::general_purpose::STANDARD.encode(&picture.bytes),
            picture.mime,
        )),
    }
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
    },
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
    /// The agent is asking to be allowed something.
    Permission {
        /// Which conversation it is asking in.
        session: SessionId,
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
    /// It wants the reader to go and do something on the web: sign in
    /// somewhere, authorise something.
    ///
    /// Answered by going, not by finishing: the agent watches for the far
    /// end itself and says when it is done. See [`Incoming::Finished`].
    Open {
        /// Which conversation it is asking in.
        session: SessionId,
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
    /// The agent is asking the reader for something.
    Ask {
        /// Which conversation it is asking in.
        session: SessionId,
        /// What it says it needs, in its own words.
        message: String,
        /// What it wants, in the order Obelus will put them.
        fields: Vec<Field>,
        /// Every field's answer, or nothing for "not answered".
        answer: Answer<Option<Vec<(String, Reply)>>>,
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
    /// Answered when it does, which may be minutes. The dispatch loop is
    /// held for the whole of it -- see this module's own account of what
    /// waiting costs -- and that is right here: the agent asked to wait.
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
    /// The conversation is over: the agent exited, or the protocol did.
    Gone(Option<String>),
}

/// How a conversation from a previous sitting is taken up again.
///
/// Decided from what the agent said at the handshake rather than by trying
/// the fullest way and reading the error: an agent that cannot replay says
/// so, and sending it `session/load` to find out costs a round trip, an
/// error in the log, and -- the part that matters -- a conversation put
/// back as lost when the agent still had every word of its context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Again {
    /// `session/load`: the agent sends the whole conversation back, and the
    /// reader sees what they said three days ago.
    Replayed,
    /// `session/resume`: the agent picks the conversation up with its
    /// context intact and sends none of it. The page starts empty and the
    /// agent still knows what was decided -- which has to be said, or a
    /// reader looking at an empty page starts explaining it all again.
    Remembered,
    /// Neither. A conversation from before cannot be had again, so a new
    /// one is opened and the reader is told.
    Not,
}

/// Which of the three this agent offers, most to least.
fn taking_up(agent: &agent_client_protocol::schema::v1::AgentCapabilities) -> Again {
    if agent.load_session {
        return Again::Replayed;
    }
    if agent.session_capabilities.resume.is_some() {
        return Again::Remembered;
    }
    Again::Not
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
}

/// The protocol's own word for one of its enums.
///
/// Through serde, which is the only thing that knows: these are
/// `snake_case` on the wire and `CamelCase` in Rust, and Obelus used to
/// bridge them with `{:?}` lowercased. That gives `inprogress` for
/// `InProgress` and `switchmode` for `SwitchMode` -- so every arm in
/// Obelus written against the protocol's spelling was an arm nothing could
/// reach: the glyph that says a call is running, the rule that keeps its
/// lines open while it runs, the picture on a plan being approved.
/// Somebody had already met it and papered over it by matching both
/// spellings of one word.
///
/// Derived rather than written out, so a variant Obelus has never seen
/// still comes out as whatever the wire calls it.
fn said_as(value: &impl serde::Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

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

/// Starts an agent on a thread of its own, and says how to talk to it.
///
/// Best effort, like every other producer: a thread that will not start, or
/// an agent that will not run, becomes a `Gone` with the reason in it
/// rather than a failure to open the view.
pub fn start(
    command: &std::path::Path,
    arguments: &[String],
    root: &std::path::Path,
    tools: Option<String>,
    events: impl Sink<Event> + Clone,
) -> mpsc::UnboundedSender<Ask> {
    let (asks, taken) = mpsc::unbounded();
    // Not always the file that was installed: what npm writes on Windows is
    // a `.cmd`, which is started by being handed to the command processor
    // rather than by being run. [`obelus_program::as_started_here`] is the
    // one place that knows the difference.
    let (program, arguments) = obelus_program::as_started_here(command, arguments);
    let config = AcpAgentConfig::new(&program).args(arguments.iter().cloned());
    let root = root.to_path_buf();
    let told = events.clone();
    // A task on the one runtime, which is what it was already: a thread
    // that built a runtime on itself, because the loop had none to offer.
    // The conversation is one connection with a handful of tasks in it
    // either way.
    //
    // The channels stay `futures`': that is what the protocol's own crate
    // speaks, and a channel is runtime-agnostic anyway.
    obelus_runtime::handle().spawn(async move {
        let reason = talk(config, root, tools, told.clone(), taken).await;
        let _ = told.send(Event::Acp(Incoming::Gone(reason)));
    });
    asks
}

/// Opens one conversation on a connection that is already up.
///
/// Both the first and every one after it: the first is opened without being
/// asked for, because opening the view *is* the request, and the rest come
/// from `Ask::Open`. One path for both, so that what a new conversation
/// arrives with cannot depend on which it is.
/// How much of a permission request is written down.
///
/// Enough to tell which call it is about and nothing like enough to be a
/// copy of the work: a request to write a file carries the whole of what
/// the file would say, and a log that kept those would be a log of the
/// reader's source with their notes buried in it.
const SAID_ABOUT: usize = 1200;

/// Writes down what an agent asked permission for.
///
/// The protocol carries more about a call than Obelus keeps -- the tool's
/// own arguments, and whatever the agent puts in `_meta` -- and what Obelus
/// keeps is what it can draw. This is the rest of it, which is the only
/// place to find out how an agent names a call: Obelus's own tools raise a
/// card of their own and being asked about them first is being asked twice,
/// but telling one of those apart from an agent's own tool means knowing
/// what a request about one looks like.
fn said_about(request: &RequestPermissionRequest) {
    let said = serde_json::to_string(&request.tool_call)
        .unwrap_or_else(|error| format!("unreadable: {error}"));
    // And what it offered, which the call itself does not say.
    //
    // The answers on the card are the agent's alone. The protocol has four
    // kinds -- allowed once, allowed always, refused once, refused always
    // -- and Obelus shows every one it is given, so a card with two
    // answers on it is an agent that sent two. Which made "why am I not
    // offered `always`" a question about the agent that nothing here could
    // answer: this line said what was asked and not what was on offer.
    let offered = serde_json::to_string(&request.options)
        .unwrap_or_else(|error| format!("unreadable: {error}"));
    tracing::info!(
        session = %request.session_id.0,
        asked = cut_to(&said, SAID_ABOUT),
        whole = said.len(),
        offered = %offered,
        "an agent is asking permission"
    );
}

/// The first `most` characters of it.
///
/// Characters rather than bytes, because a cut between the two halves of
/// one is not a string at all -- and what an agent puts in a tool call is
/// whatever the reader's files and the reader's language have in them.
fn cut_to(said: &str, most: usize) -> &str {
    match said.char_indices().nth(most) {
        Some((at, _)) => &said[..at],
        None => said,
    }
}

/// Which way Obelus can hand an agent its tools, if any.
///
/// The agent says in the handshake which transports it can connect to, and
/// the protocol is strict about it: `Http` and `Sse` are "only available
/// when the agent capabilities indicate" so. Offering one it did not ask
/// for is a server it is entitled to ignore without saying anything, which
/// is the shape of an Obelus that looks like it works and quietly offers
/// nothing.
///
/// `Stdio` every agent must take, but the agent is the one that spawns
/// the server there -- Obelus's is already running inside Obelus, so it
/// would have to be a second program that connects back to this one. That
/// is a real option and not this one.
fn offering(url: Option<&str>, can: &McpCapabilities) -> Option<McpServer> {
    let url = url?;
    let server = match (can.http, can.sse) {
        (true, _) => McpServer::Http(McpServerHttp::new("obelus", url)),
        (false, true) => McpServer::Sse(McpServerSse::new("obelus", url)),
        (false, false) => {
            tracing::warn!(
                url,
                "this agent takes neither http nor sse, so Obelus offers it no tools"
            );
            return None;
        }
    };
    tracing::info!(url, http = can.http, sse = can.sse, "offering the tools");
    Some(server)
}

async fn open_session(
    connection: &ConnectionTo<agent_client_protocol::Agent>,
    root: &std::path::Path,
    tools: Option<&McpServer>,
    events: &(impl Sink<Event> + Clone),
) -> Result<SessionId, agent_client_protocol::Error> {
    // What Obelus itself offers the agent: a handful of tools about this
    // reader's notes, which the protocol has no way to express because it is
    // about talking to an agent rather than about being talked to.
    let mut asking = NewSessionRequest::new(root.to_path_buf());
    if let Some(offered) = tools {
        asking = asking.mcp_servers(vec![offered.clone()]);
    }
    // Said from the callback rather than after an await, because the
    // callback is the one place the crate keeps in order: nothing the agent
    // sends after this answer is read until it has run. Awaited, the answer
    // could reach the main loop after a question the agent asked in the same
    // breath -- and a question naming a conversation nothing has been told
    // of yet is a question about nothing, dropped.
    let (told, opened) = oneshot::channel();
    let events = events.clone();
    connection
        .send_request(asking)
        .on_receiving_result(move |answer| {
            if let Ok(opened) = &answer {
                let session = opened.session_id.clone();
                // The old mode methods, read into a setting at the edge --
                // and kept only until the settings say they carry the mode
                // themselves, which is what replaces them.
                let mode = opened.modes.as_ref().map(mode_setting);
                let _ = events.send(Event::Acp(Incoming::Started {
                    session: session.clone(),
                    mode,
                    asking: None,
                }));
                if let Some(options) = opened.config_options.as_ref() {
                    let settings = options.iter().filter_map(setting_of).collect();
                    let _ = events.send(Event::Acp(Incoming::Update {
                        session,
                        update: Update::Settings(settings),
                    }));
                }
            }
            let _ = told.send(answer.map(|opened| opened.session_id));
            std::future::ready(Ok(()))
        })?;
    opened.await.map_err(|_| {
        agent_client_protocol::util::internal_error("the answer to session/new never came")
    })?
}

/// The whole connection, from the handshake to the end of the stream.
///
/// Returns why it ended, or `None` because it ended tidily.
async fn talk(
    config: AcpAgentConfig,
    root: PathBuf,
    tools: Option<String>,
    events: impl Sink<Event> + Clone,
    mut asks: mpsc::UnboundedReceiver<Ask>,
) -> Option<String> {
    // The transport *is* the agent: connecting spawns the process and
    // frames the messages over its stdin and stdout.
    // `AcpAgent::with_debug` hands over every line in both directions,
    // which is how the traffic in this file was read while it was written.
    let agent = agent_client_protocol::AcpAgent::new(config);

    let updates = events.clone();
    let asking = events.clone();
    let elicited = events.clone();
    let completed = events.clone();
    let running = events.clone();
    let reading_output = events.clone();
    let waiting = events.clone();
    let stopping = events.clone();
    let forgetting = events.clone();
    let reading = events.clone();
    let writing = events.clone();

    let outcome = Client
        .builder()
        .on_receive_notification(
            async move |notification: SessionNotification, _connection| {
                // Which conversation it is about, which the protocol has
                // said all along.
                let session = notification.session_id;
                for update in read_update(notification.update) {
                    let _ = updates.send(Event::Acp(Incoming::Update {
                        session: session.clone(),
                        update,
                    }));
                }
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_notification(
            async move |notification: CompleteElicitationNotification, _connection| {
                // The agent watched the far end of a question it sent the
                // reader away to answer, and saw it happen. Nothing is
                // owed back -- this is it saying the waiting is over.
                let _ = completed.send(Event::Acp(Incoming::Finished {
                    id: notification.elicitation_id.0.to_string(),
                }));
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, _connection| {
                said_about(&request);
                // The reader's to answer, so the question goes to the main
                // loop and this waits for the keystroke. An answer that
                // never comes -- the view closed, Obelus quit -- is the
                // protocol's "cancelled", which is what an agent needs to
                // hear to stop waiting.
                let (answer, answered) = oneshot::channel();
                let question = Incoming::Permission {
                    session: request.session_id.clone(),
                    call: Box::new(call_of(
                        &request.tool_call.tool_call_id,
                        &request.tool_call.fields,
                    )),
                    reason: reason_of(&request),
                    options: request
                        .options
                        .iter()
                        .map(|option| Choice {
                            id: option.option_id.0.to_string(),
                            name: option.name.clone(),
                            kind: said_as(&option.kind),
                        })
                        .collect(),
                    answer,
                };
                if asking.send(Event::Acp(question)).is_err() {
                    return responder.respond(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Cancelled,
                    ));
                }
                let outcome = match answered.await {
                    Ok(Some(option)) => RequestPermissionOutcome::Selected(
                        SelectedPermissionOutcome::new(PermissionOptionId::new(option)),
                    ),
                    Ok(None) | Err(_) => RequestPermissionOutcome::Cancelled,
                };
                responder.respond(RequestPermissionResponse::new(outcome))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: ReadTextFileRequest, responder, _connection| {
                // From a buffer if Obelus has one, which is the main loop's
                // to know: what the reader is looking at is not always what
                // is on disk, and an agent inside a reader should be
                // looking at the same thing.
                let (answer, answered) = oneshot::channel();
                let question = Incoming::Read {
                    path: request.path.clone(),
                    line: request.line,
                    limit: request.limit,
                    answer,
                };
                if reading.send(Event::Acp(question)).is_err() {
                    return responder.respond_with_error(refusal("Obelus is not listening"));
                }
                match answered.await {
                    Ok(Some(text)) => responder.respond(ReadTextFileResponse::new(text)),
                    Ok(None) | Err(_) => {
                        responder.respond_with_error(refusal("Obelus will not read that"))
                    }
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: CreateElicitationRequest, responder, _connection| {
                // The agent is asking the reader something. What it may ask
                // for is a flat form of primitives, and Obelus puts that
                // the way it puts every other choice: a list where the
                // answer is one of a few, the box where it is words. One
                // field at a time, because a terminal reader has one thing
                // on screen and one caret in it.
                // A URL takes a different road: nothing is filled in, the
                // reader is sent somewhere, and the agent hears that they
                // went rather than what they said.
                //
                // Asked in a conversation, which is where it waits for the
                // reader: a question is about the turn that raised it, not
                // about whatever is on screen when it arrives. The other
                // scope is a question about a request Obelus made outside
                // any conversation -- opening one, before it exists -- and
                // no agent Obelus has met sends it, so it is declined rather
                // than put in front of a conversation it was not asked in.
                let ElicitationScope::Session(scope) = request.scope() else {
                    tracing::warn!(
                        message = %request.message,
                        "a question about no conversation, declined"
                    );
                    return responder
                        .respond(CreateElicitationResponse::new(ElicitationAction::Decline));
                };
                let session = scope.session_id.clone();
                if let ElicitationMode::Url(mode) = &request.mode {
                    let Some(url) = somewhere_to_go(&mode.url) else {
                        // An error rather than a decline, because this is
                        // not the reader refusing: the agent sent something
                        // Obelus will not hand to the machine's own
                        // launcher, and it should hear which of those it
                        // was. `file:` and the schemes an editor or a
                        // chat program registers can start a program, and
                        // this URL came from the agent.
                        return responder.respond_with_error(
                            agent_client_protocol::Error::invalid_params()
                                .data("an elicitation URL must be http or https, with a host"),
                        );
                    };
                    let (answer, answered) = oneshot::channel();
                    let question = Incoming::Open {
                        session,
                        message: request.message.clone(),
                        url,
                        id: mode.elicitation_id.0.to_string(),
                        answer,
                    };
                    if elicited.send(Event::Acp(question)).is_err() {
                        return responder
                            .respond(CreateElicitationResponse::new(ElicitationAction::Cancel));
                    }
                    // Accepted the moment the reader is sent there, not
                    // when they come back: what the agent asked for is that
                    // they be directed to the URL, and it watches the far
                    // end itself. Waiting here would hold a turn open
                    // across a sign-in nobody can time.
                    let action = match answered.await {
                        Ok(true) => ElicitationAction::Accept(ElicitationAcceptAction::new()),
                        Ok(false) => ElicitationAction::Decline,
                        // The question going away without an answer, which
                        // is what the card being taken down means.
                        Err(_) => ElicitationAction::Cancel,
                    };
                    return responder.respond(CreateElicitationResponse::new(action));
                }
                let asked = match &request.mode {
                    ElicitationMode::Form(form) => fields_of(&form.requested_schema),
                    // A mode Obelus never offered to show. Declined rather
                    // than errored: the agent asked a fair question of a
                    // client that cannot put it, and it has to be able to
                    // carry on.
                    other => Err(format!("{other:?}")),
                };
                let fields = match asked {
                    Ok(fields) => fields,
                    Err(why) => {
                        let _ = elicited.send(Event::Acp(Incoming::Failed(
                            "a question Obelus cannot put",
                            why,
                        )));
                        return responder
                            .respond(CreateElicitationResponse::new(ElicitationAction::Decline));
                    }
                };
                let (answer, answered) = oneshot::channel();
                let question = Incoming::Ask {
                    session,
                    message: request.message.clone(),
                    fields,
                    answer,
                };
                if elicited.send(Event::Acp(question)).is_err() {
                    return responder
                        .respond(CreateElicitationResponse::new(ElicitationAction::Cancel));
                }
                // Nothing back is a refusal; the view going away with the
                // question still up is a cancellation. Either way the agent
                // hears something, because one that hears nothing waits for
                // ever.
                let action = match answered.await {
                    Ok(Some(given)) => ElicitationAction::Accept(
                        ElicitationAcceptAction::new().content(content_of(given)),
                    ),
                    Ok(None) => ElicitationAction::Decline,
                    Err(_) => ElicitationAction::Cancel,
                };
                responder.respond(CreateElicitationResponse::new(action))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: CreateTerminalRequest, responder, _connection| {
                let (answer, answered) = oneshot::channel();
                let question = Incoming::Run {
                    command: request.command.clone(),
                    args: request.args.clone(),
                    env: request
                        .env
                        .iter()
                        .map(|set| (set.name.clone(), set.value.clone()))
                        .collect(),
                    cwd: request.cwd.clone(),
                    limit: request
                        .output_byte_limit
                        .and_then(|it| usize::try_from(it).ok()),
                    answer,
                };
                if running.send(Event::Acp(question)).is_err() {
                    return responder.respond_with_error(refusal("Obelus is not listening"));
                }
                match answered.await {
                    Ok(Some(id)) => {
                        responder.respond(CreateTerminalResponse::new(TerminalId::new(id)))
                    }
                    Ok(None) | Err(_) => {
                        responder.respond_with_error(refusal("Obelus could not run that"))
                    }
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: TerminalOutputRequest, responder, _connection| {
                let (answer, answered) = oneshot::channel();
                let question = Incoming::Wrote {
                    id: request.terminal_id.0.to_string(),
                    answer,
                };
                if reading_output.send(Event::Acp(question)).is_err() {
                    return responder.respond_with_error(refusal("Obelus is not listening"));
                }
                match answered.await {
                    Ok(Some((output, truncated, ended))) => responder.respond(
                        TerminalOutputResponse::new(output, truncated)
                            .exit_status(ended.map(exit_status)),
                    ),
                    Ok(None) | Err(_) => {
                        responder.respond_with_error(refusal("Obelus is not running that"))
                    }
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: WaitForTerminalExitRequest, responder, _connection| {
                // Answered when the command ends, which may be minutes.
                // The whole connection waits with it -- what this module
                // says about waiting -- and that is what the agent asked
                // for by calling this rather than reading the output.
                let (answer, answered) = oneshot::channel();
                let question = Incoming::Waited {
                    id: request.terminal_id.0.to_string(),
                    answer,
                };
                if waiting.send(Event::Acp(question)).is_err() {
                    return responder.respond_with_error(refusal("Obelus is not listening"));
                }
                match answered.await {
                    Ok(Some(ended)) => {
                        responder.respond(WaitForTerminalExitResponse::new(exit_status(ended)))
                    }
                    Ok(None) | Err(_) => {
                        responder.respond_with_error(refusal("Obelus is not running that"))
                    }
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: KillTerminalRequest, responder, _connection| {
                let (answer, answered) = oneshot::channel();
                let question = Incoming::Stop {
                    id: request.terminal_id.0.to_string(),
                    answer,
                };
                if stopping.send(Event::Acp(question)).is_err() {
                    return responder.respond_with_error(refusal("Obelus is not listening"));
                }
                // Waited for, so that the output asked for next is the
                // output of something no longer writing.
                let _ = answered.await;
                responder.respond(KillTerminalResponse::new())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: ReleaseTerminalRequest, responder, _connection| {
                let (answer, answered) = oneshot::channel();
                let question = Incoming::Forget {
                    id: request.terminal_id.0.to_string(),
                    answer,
                };
                if forgetting.send(Event::Acp(question)).is_err() {
                    return responder.respond_with_error(refusal("Obelus is not listening"));
                }
                let _ = answered.await;
                responder.respond(ReleaseTerminalResponse::new())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: WriteTextFileRequest, responder, _connection| {
                // Through the main loop, the same as a read -- and for a
                // stronger reason. A file Obelus has open is a document the
                // reader can undo, and an agent writing straight to disk
                // under one would leave two versions with no way back to
                // either.
                let (answer, answered) = oneshot::channel();
                let question = Incoming::Write {
                    path: request.path.clone(),
                    text: request.content.clone(),
                    answer,
                };
                if writing.send(Event::Acp(question)).is_err() {
                    return responder.respond_with_error(refusal("Obelus is not listening"));
                }
                match answered.await {
                    Ok(true) => responder.respond(WriteTextFileResponse::new()),
                    Ok(false) | Err(_) => {
                        responder.respond_with_error(refusal("Obelus will not write that"))
                    }
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(
            agent,
            |connection: ConnectionTo<agent_client_protocol::Agent>| async move {
                let ready = connection.send_request(handshake()).block_task().await?;
                // The title, which the protocol says is the one for people:
                // the name is for programs, and claude-agent-acp's is its npm
                // package. No version beside it -- the header says who the
                // reader is talking to, and which build is the log's to say.
                let named = ready.agent_info.map(|info| {
                    tracing::info!(name = %info.name, version = %info.version, "the agent says who it is");
                    info.title
                        .filter(|title| !title.is_empty())
                        .unwrap_or(info.name)
                });
                let prompts = &ready.agent_capabilities.prompt_capabilities;
                let carries = Carries {
                    image: prompts.image,
                    embedded: prompts.embedded_context,
                };
                // Written down as it arrives, because "why did my picture
                // do nothing" is answered by what the agent said it takes
                // and by nothing else.
                tracing::info!(?carries, "what a prompt to this agent may carry");
                let _ = events.send(Event::Acp(Incoming::Ready { named, carries }));

                // Which way the tools can be handed over, decided once from
                // what the agent said it takes rather than guessed afresh
                // per session: the answer cannot change while the agent
                // runs, and asking twice would be two answers to keep alike.
                let offered =
                    offering(tools.as_deref(), &ready.agent_capabilities.mcp_capabilities);
                // And how a conversation from a previous sitting is taken
                // up, decided once for the same reason.
                let again = taking_up(&ready.agent_capabilities);

                // Nothing is opened here. One was, on the grounds that the
                // reader opening the view is a request to talk -- which
                // was true while opening the view was the only way to get
                // a conversation. A view can open on a note that already
                // names one now, and then the session minted on the way up
                // is a conversation nobody asked for: empty, so the agent
                // never keeps it, and yet a name Obelus could write down
                // against the note in place of the one the reader had been
                // talking in. Whoever opens a conversation says what it
                // wants, and waits the one round trip that costs.
                while let Some(ask) = asks.next().await {
                    match ask {
                        Ask::Open => {
                            open_session(&connection, &root, offered.as_ref(), &events).await?;
                        }
                        // A conversation opened to read one thing off it
                        // and closed again. Nothing is said in it and
                        // nothing above this hears that it existed: what
                        // comes back is a list of what the agent offers,
                        // which is a fact about the agent.
                        Ask::Offers => {
                            // Without Obelus's own tools: they are what a
                            // conversation is given so that the agent can
                            // reach the reader's notes, and nothing is
                            // going to be said in this one.
                            let asking = NewSessionRequest::new(root.to_path_buf());
                            match connection.send_request(asking).block_task().await {
                                Ok(opened) => {
                                    let session = opened.session_id.clone();
                                    let _ = events.send(Event::Acp(Incoming::Offers {
                                        session: session.clone(),
                                        offers: offers_in(&opened),
                                    }));
                                    // Let go the way a conversation about
                                    // a deleted note is, and by the same
                                    // two names: an agent left holding a
                                    // session nobody can reach is the
                                    // complaint that got the language
                                    // server killed on the way out.
                                    let forgotten = connection
                                        .send_request(DeleteSessionRequest::new(session.clone()))
                                        .block_task()
                                        .await;
                                    if forgotten.is_err() {
                                        let _ = connection
                                            .send_request(CloseSessionRequest::new(session))
                                            .block_task()
                                            .await;
                                    }
                                }
                                Err(error) => {
                                    let _ = events.send(Event::Acp(Incoming::Failed(
                                        ASKING_WHAT_IT_OFFERS,
                                        error.to_string(),
                                    )));
                                }
                            }
                        }
                        // Gone, because the note it was about is. Two ways
                        // to say it and they mean different things: `delete`
                        // is "forget this", `close` is "I am done talking in
                        // it". The first is what a deleted note means, so it
                        // is tried first and the second is the fallback for
                        // an agent that only offers that.
                        Ask::Drop { session } => {
                            let forgotten = connection
                                .send_request(DeleteSessionRequest::new(session.clone()))
                                .block_task()
                                .await;
                            if forgotten.is_err() {
                                let _ = connection
                                    .send_request(CloseSessionRequest::new(session))
                                    .block_task()
                                    .await;
                            }
                        }
                        // What the reader had before, taken up again. The
                        // agent replays it, so nothing here has to hold a
                        // transcript between sittings.
                        //
                        // Every way this can fail ends in a conversation the
                        // reader can talk in: an agent that will not load
                        // one -- it has forgotten it, it never could -- gets
                        // asked for a new one instead, and the reader is
                        // told what happened rather than left looking at an
                        // empty screen that used to have something in it.
                        Ask::Reopen { session } => {
                            // The fullest way this agent offers, and only
                            // that one: the two answers carry the same two
                            // things, and which was asked is the difference
                            // between a page with the conversation on it
                            // and a page with none.
                            //
                            // And Obelus's own tools go with it, the same
                            // as they go with a conversation being opened
                            // for the first time. Where Obelus offers
                            // them is a port the machine handed out when
                            // this process started, so it is a different
                            // one every run -- and a conversation outlives
                            // the run it was started in, which is the
                            // whole reason this ask exists. Left unsaid,
                            // the agent went on using the address it was
                            // given the first time, which died with the
                            // process that gave it: the notes worked all
                            // morning and then stopped, and from the
                            // agent's side the tools had simply gone.
                            let mut loading =
                                LoadSessionRequest::new(session.clone(), root.clone());
                            let mut resuming =
                                ResumeSessionRequest::new(session.clone(), root.clone());
                            if let Some(offered) = offered.as_ref() {
                                loading = loading.mcp_servers(vec![offered.clone()]);
                                resuming = resuming.mcp_servers(vec![offered.clone()]);
                            }
                            let taken = match again {
                                Again::Replayed => Some(
                                    connection
                                        .send_request(loading)
                                        .block_task()
                                        .await
                                        .map(|it| (it.modes, it.config_options)),
                                ),
                                Again::Remembered => Some(
                                    connection
                                        .send_request(resuming)
                                        .block_task()
                                        .await
                                        .map(|it| (it.modes, it.config_options)),
                                ),
                                // Not asked at all. The agent said at the
                                // handshake that it cannot, and a request
                                // sent to be told that again is a round
                                // trip spent learning nothing.
                                Again::Not => None,
                            };
                            match taken {
                                Some(Ok((modes, options))) => {
                                    let mode = modes.as_ref().map(mode_setting);
                                    let _ = events.send(Event::Acp(Incoming::Started {
                                        session: session.clone(),
                                        mode,
                                        asking: None,
                                    }));
                                    // And, where the words did not come
                                    // with it, that they did not.
                                    if again == Again::Remembered {
                                        let _ = events.send(Event::Acp(Incoming::Remembered {
                                            session: session.clone(),
                                        }));
                                    }
                                    if let Some(options) = options.as_ref() {
                                        let settings =
                                            options.iter().filter_map(setting_of).collect();
                                        let _ = events.send(Event::Acp(Incoming::Update {
                                            session,
                                            update: Update::Settings(settings),
                                        }));
                                    }
                                }
                                answer => {
                                    let why = match answer {
                                        Some(Err(error)) => error.to_string(),
                                        _ => "this agent cannot take a conversation up again"
                                            .to_string(),
                                    };
                                    let _ = events.send(Event::Acp(Incoming::Lost {
                                        session: session.clone(),
                                        why,
                                    }));
                                    open_session(&connection, &root, offered.as_ref(), &events)
                                        .await?;
                                }
                            }
                        }
                        // The answer is taken in a callback rather than
                        // awaited, so the loop goes straight back to
                        // reading asks: an interruption typed while the
                        // agent is thinking has to reach it.
                        Ask::Say {
                            session,
                            turn,
                            said,
                            opening,
                        } => {
                            let told = events.clone();
                            let whose = session.clone();
                            connection
                                .send_request(PromptRequest::new(
                                    session,
                                    opening
                                        .into_iter()
                                        .map(|words| ContentBlock::Text(TextContent::new(words)))
                                        .chain(said.into_iter().map(block_of))
                                        .collect(),
                                ))
                                .on_receiving_result(move |asked| {
                                    // Sent whatever became of the turn, and
                                    // whatever has happened here since. An
                                    // answer about a turn nobody is waiting
                                    // for any more is thrown away by the
                                    // handle, which is the side that counts
                                    // them -- this one keeps nothing that
                                    // could go stale.
                                    let _ = told.send(Event::Acp(Incoming::Ended {
                                        session: whose,
                                        turn,
                                        why: asked
                                            .map(|answer| said_as(&answer.stop_reason))
                                            .map_err(|error| error.to_string()),
                                    }));
                                    std::future::ready(Ok(()))
                                })?;
                        }
                        // The agent is told, and the turn is over here
                        // whatever it does about that. Both halves are
                        // needed: an agent that gets the notification stops
                        // and says so, and one that does not -- the
                        // outgoing side is free to let a notification leave
                        // before a request queued ahead of it, so a
                        // cancellation can reach an agent before the prompt
                        // it is about -- goes on working on a turn nobody
                        // is waiting for.
                        Ask::Interrupt { session, turn } => {
                            connection
                                .send_notification(CancelNotification::new(session.clone()))?;
                            let _ = events.send(Event::Acp(Incoming::Ended {
                                session,
                                turn,
                                why: Ok("cancelled".to_string()),
                            }));
                        }
                        Ask::Set {
                            session,
                            setting,
                            chosen,
                        } => {
                            let told = events.clone();
                            let value = match chosen {
                                Chosen::Value(id) => SessionConfigOptionValue::value_id(id),
                                Chosen::Switch(on) => SessionConfigOptionValue::boolean(on),
                            };
                            let whose = session.clone();
                            connection
                                .send_request(SetSessionConfigOptionRequest::new(
                                    session,
                                    SessionConfigId::new(setting),
                                    value,
                                ))
                                .on_receiving_result(move |asked| {
                                    // The answer is the whole set of them
                                    // again: one setting's value can
                                    // change what another one offers -- a
                                    // model with no thinking levels, say --
                                    // so what comes back replaces what is
                                    // shown rather than patching it.
                                    let _ = told.send(Event::Acp(match asked {
                                        Ok(answer) => Incoming::Update {
                                            session: whose,
                                            update: Update::Settings(
                                                answer
                                                    .config_options
                                                    .iter()
                                                    .filter_map(setting_of)
                                                    .collect(),
                                            ),
                                        },
                                        Err(error) => Incoming::Failed(
                                            "Changing a setting",
                                            error.to_string(),
                                        ),
                                    }));
                                    std::future::ready(Ok(()))
                                })?;
                        }
                        Ask::Mode { session, mode } => {
                            let told = events.clone();
                            connection
                                .send_request(SetSessionModeRequest::new(
                                    session,
                                    agent_client_protocol::schema::v1::SessionModeId::new(mode),
                                ))
                                .on_receiving_result(move |asked| {
                                    if let Err(error) = asked {
                                        let _ = told.send(Event::Acp(Incoming::Failed(
                                            "Changing the mode",
                                            error.to_string(),
                                        )));
                                    }
                                    std::future::ready(Ok(()))
                                })?;
                        }
                    }
                }
                Ok(())
            },
        )
        .await;

    outcome.err().map(|error| {
        // The whole of it in the log, where the `spawned_at` of a protocol
        // crate is worth having, and a sentence in the reason -- which is
        // what the transcript says.
        tracing::warn!(error = %error, "the conversation ended");
        ended_because(&error)
    })
}

/// Why the conversation ended, in a line.
///
/// The protocol's own `Display` is its message and then every field of
/// `data` pretty-printed, which for an agent that exited is four rows of
/// JSON carrying one sentence and the source path of a crate in the cargo
/// registry. The sentence is the part a reader is owed.
fn ended_because(error: &agent_client_protocol::schema::v1::Error) -> String {
    error
        .data
        .as_ref()
        .and_then(|data| data.get("data"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| match error.message.is_empty() {
            true => error.to_string(),
            false => error.message.clone(),
        })
}

/// What Obelus tells an agent about itself.
///
/// It reads files out and writes them back inside the tree it was opened
/// on, runs the commands it is given, and puts both of the questions an
/// agent can ask. Declaring the truth here is what keeps a well-behaved
/// agent from asking for the rest.
fn handshake() -> InitializeRequest {
    InitializeRequest::new(ProtocolVersion::V1)
        .client_capabilities(
            ClientCapabilities::new()
                .fs(FileSystemCapabilities::new()
                    .read_text_file(true)
                    .write_text_file(true))
                // Commands, which Obelus runs and shows rather than
                // asking about: see [`Incoming::Run`]. There is no
                // terminal behind this and none is needed -- the five
                // `terminal/*` methods want a process, not a screen.
                .terminal(true)
                // Both kinds of question: a form, which goes on a card,
                // and a URL, which Obelus hands to whatever the reader
                // opens links with. Saying only `form` left an agent that
                // needed the reader to sign in somewhere with no way to
                // say so.
                .elicitation(
                    ElicitationCapabilities::new()
                        .form(ElicitationFormCapabilities::new())
                        .url(ElicitationUrlCapabilities::new()),
                )
                // A switch is two rows of a list here, which is what the
                // capability is about: an agent may only offer boolean
                // settings to a client that says it can show them.
                .session(
                    ClientSessionCapabilities::default().config_options(
                        SessionConfigOptionsCapabilities::new()
                            .boolean(BooleanConfigOptionCapabilities::new()),
                    ),
                ),
        )
        .client_info(Implementation::new("obelus", env!("CARGO_PKG_VERSION")))
}

/// How a command ended, as the protocol says it.
fn exit_status(ended: crate::running::Ended) -> TerminalExitStatus {
    TerminalExitStatus::new()
        .exit_code(ended.code)
        .signal(ended.signal.map(str::to_string))
}

/// What Obelus says when it will not do something.
fn refusal(why: &str) -> agent_client_protocol::Error {
    agent_client_protocol::Error::method_not_found().data(serde_json::json!(why))
}

/// What the agent is actually about to do, for the reader deciding whether
/// to let it.
///
/// The title is a line -- "run a command", "edit a file" -- and a line is
/// not enough to answer a question about permission: *which* command, on
/// *which* file. The protocol carries that as the tool call's content, so
/// this is the words of it, and the files it names when it has no words.
///
/// Not `raw_input`: that is the agent's own arguments in its own shape,
/// which Obelus would have to guess the meaning of. The typed fields are
/// what an agent fills in to be shown.
fn reason_of(request: &RequestPermissionRequest) -> Option<String> {
    let fields = &request.tool_call.fields;
    let said: Vec<String> = fields
        .content
        .iter()
        .flatten()
        .filter_map(|content| match content {
            ToolCallContent::Content(block) => words(&block.content),
            // A diff and a terminal are shown by the conversation itself
            // once the work is allowed; what the question needs is the
            // file it is about, which the locations carry.
            _ => None,
        })
        .collect();
    if !said.is_empty() {
        return Some(said.join("\n"));
    }
    let places: Vec<String> = fields
        .locations
        .iter()
        .flatten()
        .map(|place| place.path.display().to_string())
        .collect();
    (!places.is_empty()).then(|| places.join("\n"))
}

/// What a `session/update` means, if it is one Obelus shows.
///
/// The protocol has a dozen and a half kinds and this reads nine. The rest
/// -- usage, compaction -- are facts about the agent rather than about the
/// conversation, and a conversation with them in it is a log.
///
/// A plan was in that list once and does not belong in it: what an agent
/// means to do about what the reader just asked is the most conversation-
/// shaped thing the protocol carries. The objection was right about where
/// it goes, though -- a finished list of seven completed steps *is* a log,
/// so it is never written into the transcript. It is what is happening
/// now, and it is drawn where that is drawn.
fn read_update(update: SessionUpdate) -> Vec<Update> {
    match update {
        SessionUpdate::AgentMessageChunk(chunk) => words(&chunk.content)
            .map(Update::Said)
            .into_iter()
            .collect(),
        SessionUpdate::UserMessageChunk(chunk) => words(&chunk.content)
            .map(Update::Heard)
            .into_iter()
            .collect(),
        SessionUpdate::AgentThoughtChunk(chunk) => words(&chunk.content)
            .map(Update::Thought)
            .into_iter()
            .collect(),
        SessionUpdate::ToolCall(call) => vec![Update::Tool {
            call: Box::new(Call {
                id: call.tool_call_id.0.to_string(),
                title: call.title.clone(),
                kind: said_as(&call.kind),
                places: call.locations.iter().map(place_of).collect(),
                change: change_of(&call.content),
                ran: ran_in(&call.content),
                said: words_of(&call.content),
            }),
            status: said_as(&call.status),
        }],
        // A later update carries only what changed, so what it leaves out
        // arrives here as nothing and is read as "the same as before".
        SessionUpdate::ToolCallUpdate(call) => vec![Update::Tool {
            call: Box::new(call_of(&call.tool_call_id, &call.fields)),
            status: call
                .fields
                .status
                .map(|status| said_as(&status))
                .unwrap_or_default(),
        }],
        SessionUpdate::Plan(plan) => vec![Update::Plan(
            plan.entries
                .iter()
                .map(|entry| Step {
                    said: entry.content.clone(),
                    state: said_as(&entry.status),
                    priority: said_as(&entry.priority),
                })
                .collect(),
        )],
        SessionUpdate::CurrentModeUpdate(mode) => {
            vec![Update::Mode(mode.current_mode_id.0.to_string())]
        }
        SessionUpdate::AvailableCommandsUpdate(update) => vec![Update::Orders(
            update.available_commands.iter().map(order_of).collect(),
        )],
        SessionUpdate::ConfigOptionUpdate(update) => vec![Update::Settings(
            update
                .config_options
                .iter()
                .filter_map(setting_of)
                .collect(),
        )],
        // What the agent calls this conversation. A patch rather than a
        // value: absent means unchanged, null means cleared, and only a
        // string is a new name -- so the two that are not a string are
        // nothing to do, not a name of nothing.
        // How full it is, and what it has cost. Several of these arrive in
        // one turn -- the numbers only go up within a turn -- so this is
        // kept and shown rather than said.
        SessionUpdate::UsageUpdate(used) => vec![Update::Used(Usage {
            used: used.used,
            room: used.size,
            cost: used.cost.map(|cost| Cost {
                amount: cost.amount,
                currency: cost.currency,
            }),
        })],
        SessionUpdate::SessionInfoUpdate(info) => match info.title {
            agent_client_protocol::schema::MaybeUndefined::Value(title) => {
                vec![Update::Titled(title)]
            }
            agent_client_protocol::schema::MaybeUndefined::Undefined
            | agent_client_protocol::schema::MaybeUndefined::Null => Vec::new(),
        },
        other => {
            tracing::debug!(?other, "an update Obelus does not show");
            Vec::new()
        }
    }
}

/// A URL Obelus is willing to hand to the machine, or nothing.
///
/// `http` and `https` only, and it must name a host. Everything else is
/// refused, because what happens next is that Obelus asks the machine to
/// open this with whatever is registered for it: `file:` reaches the disk,
/// and an editor or a chat program registering a scheme of its own turns a
/// link into a way to start a program. The string came from the agent.
///
/// Parsed by hand rather than with a URL crate. What is being asked is
/// which scheme it is and whether anything follows -- not what the host
/// normalises to -- and a crate that answers the second brings a Unicode
/// database to do it.
fn somewhere_to_go(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    // A host is whatever comes before the path, and it has to be
    // something: `https:///whatever` names no machine.
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() {
        return None;
    }
    // And nothing a shell or a launcher would read as more than one
    // argument. Every one of these is legal in a URL only when it is
    // written `%20`, `%0a` and so on, so refusing them refuses nothing a
    // well-formed URL needed.
    if url.chars().any(char::is_whitespace) || url.contains('\0') {
        return None;
    }
    Some(url.to_string())
}

/// The fields of a form, in the order Obelus will put them.
///
/// Or why it cannot put this one. A form Obelus half-fills in is worse than
/// one it declines: the agent gets an answer to a question it did not ask.
///
/// The order is the schema's map order, which is alphabetical by name --
/// the wire has an object and objects have no order, so there is nothing
/// else to go on.
fn fields_of(schema: &ElicitationSchema) -> Result<Vec<Field>, String> {
    let required: Vec<&str> = schema
        .required
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(String::as_str)
        .collect();
    let mut fields = Vec::new();
    for (name, property) in &schema.properties {
        let (title, about, takes) = match property {
            ElicitationPropertySchema::String(text) => (
                text.title.clone(),
                text.description.clone(),
                match (text.one_of.as_ref(), text.enum_values.as_ref()) {
                    // Named values: the agent gave each one a title, and
                    // that is what the row says.
                    (Some(named), _) => Takes::One(
                        named
                            .iter()
                            .map(|option| Value {
                                id: option.value.clone(),
                                name: option.title.clone(),
                                about: said_twice(option.description.as_deref(), &option.title),
                            })
                            .collect(),
                    ),
                    // Bare values, which are their own names.
                    (None, Some(values)) => Takes::One(
                        values
                            .iter()
                            .map(|value| Value {
                                id: value.clone(),
                                name: value.clone(),
                                about: None,
                            })
                            .collect(),
                    ),
                    (None, None) => Takes::Words(text.default.clone()),
                },
            ),
            ElicitationPropertySchema::Boolean(switch) => (
                switch.title.clone(),
                switch.description.clone(),
                Takes::Switch(switch.default.unwrap_or(false)),
            ),
            ElicitationPropertySchema::Number(number) => (
                number.title.clone(),
                number.description.clone(),
                Takes::Number {
                    whole: false,
                    least: number.minimum,
                    most: number.maximum,
                },
            ),
            ElicitationPropertySchema::Integer(number) => (
                number.title.clone(),
                number.description.clone(),
                Takes::Number {
                    whole: true,
                    #[expect(
                        clippy::cast_precision_loss,
                        reason = "a bound a reader is expected to type by hand"
                    )]
                    least: number.minimum.map(|least| least as f64),
                    #[expect(
                        clippy::cast_precision_loss,
                        reason = "a bound a reader is expected to type by hand"
                    )]
                    most: number.maximum.map(|most| most as f64),
                },
            ),
            // Several of a list. The items come either as bare strings or
            // as titled options, which is the same pair the single-select
            // kind comes in and is read the same way.
            ElicitationPropertySchema::Array(several) => (
                several.title.clone(),
                several.description.clone(),
                Takes::Some {
                    values: match &several.items {
                        MultiSelectItems::Titled(items) => items
                            .options
                            .iter()
                            .map(|option| Value {
                                id: option.value.clone(),
                                name: option.title.clone(),
                                about: said_twice(option.description.as_deref(), &option.title),
                            })
                            .collect(),
                        MultiSelectItems::String(items) => items
                            .values
                            .iter()
                            .map(|value| Value {
                                id: value.clone(),
                                name: value.clone(),
                                about: None,
                            })
                            .collect(),
                        other => return Err(format!("{name} is a list of {other:?}")),
                    },
                    least: several.min_items,
                    most: several.max_items,
                    chosen: several.default.clone().unwrap_or_default(),
                },
            ),
            other => return Err(format!("{name} is a {other:?}")),
        };
        fields.push(Field {
            name: name.clone(),
            title: title.unwrap_or_else(|| name.clone()),
            about,
            takes,
            required: required.contains(&name.as_str()),
        });
    }
    // The ones that have to be answered first, in the order the agent
    // listed them; the rest after, in the only order left.
    //
    // The schema's properties arrive as a sorted map -- JSON objects have
    // no order to keep -- so the order the agent wrote them in is gone by
    // the time Obelus sees it, and asking by the alphabet put "Other" in
    // front of the question it was an alternative to. What is left is what
    // the agent said had to be answered, which is the question itself.
    fields.sort_by_key(
        |field| match required.iter().position(|name| *name == field.name) {
            Some(at) => (0, at),
            None => (1, 0),
        },
    );
    Ok(fields)
}

/// The answers, as the protocol takes them.
fn content_of(
    given: Vec<(String, Reply)>,
) -> std::collections::BTreeMap<String, ElicitationContentValue> {
    given
        .into_iter()
        .map(|(name, reply)| {
            let value = match reply {
                Reply::Value(value) | Reply::Words(value) => ElicitationContentValue::String(value),
                Reply::Values(values) => ElicitationContentValue::StringArray(values),
                Reply::Switch(on) => ElicitationContentValue::Boolean(on),
                Reply::Number(number) => ElicitationContentValue::Number(number),
                Reply::Whole(number) => ElicitationContentValue::Integer(number),
            };
            (name, value)
        })
        .collect()
}

/// One setting, as the view offers it -- if it is one Obelus can show.
///
/// Nothing but a kind it has never heard of is dropped: a setting whose
/// values Obelus cannot list is a row that would do nothing when chosen,
/// and the agent's own dialog for it is not Obelus's to open.
fn setting_of(option: &SessionConfigOption) -> Option<Setting> {
    let (values, current, kind) = match &option.kind {
        SessionConfigKind::Select(select) => (
            values_of(&select.options),
            select.current_value.0.to_string(),
            Kind::Select,
        ),
        SessionConfigKind::Boolean(boolean) => (
            vec![
                Value {
                    id: ON.to_string(),
                    name: ON.to_string(),
                    about: None,
                },
                Value {
                    id: OFF.to_string(),
                    name: OFF.to_string(),
                    about: None,
                },
            ],
            match boolean.current_value {
                true => ON.to_string(),
                false => OFF.to_string(),
            },
            Kind::Switch,
        ),
        other => {
            tracing::debug!(?other, "a setting Obelus cannot show");
            return None;
        }
    };
    // What each one is, once, where it arrives. How an agent declares a
    // setting decides how Obelus draws it and what pressing enter on it
    // does -- a switch is flipped and a list is opened -- so "why is this
    // one drawn like that" is a question about this line, and it was
    // unanswerable without it.
    let setting = Setting {
        id: option.id.0.to_string(),
        name: option.name.clone(),
        about: said_twice(option.description.as_deref(), &option.name),
        values,
        current,
        kind,
        category: category_of(option.category.as_ref()),
        legacy: false,
    };
    tracing::debug!(
        id = setting.id,
        name = setting.name,
        kind = ?setting.kind,
        category = ?setting.category,
        values = setting.values.len(),
        current = setting.current,
        "a setting the agent offers"
    );
    Some(setting)
}

/// What the agent said a setting is about, as one of the few Obelus can do
/// something with.
///
/// A category Obelus has never heard of is [`Category::Other`], which is
/// also what nothing said means: the spec reserves the unprefixed names for
/// itself and tells clients to handle the rest gracefully, and the graceful
/// thing is to show the setting and claim nothing about it.
fn category_of(category: Option<&SessionConfigOptionCategory>) -> Category {
    match category {
        Some(SessionConfigOptionCategory::Mode) => Category::Mode,
        Some(SessionConfigOptionCategory::Model) => Category::Model,
        Some(SessionConfigOptionCategory::ModelConfig) => Category::ModelConfig,
        Some(SessionConfigOptionCategory::ThoughtLevel) => Category::ThoughtLevel,
        Some(SessionConfigOptionCategory::Other(name)) => {
            tracing::debug!(name, "a category Obelus has never heard of");
            Category::Other
        }
        None | Some(_) => Category::Other,
    }
}

/// Everything a new conversation says it can be set to, as one list.
///
/// The same two halves a session keeps apart and merges -- the mode from
/// the older dedicated methods, and the config options -- put together by
/// the same rule: the options win where they carry a mode themselves, and
/// the old one goes in front of them where they do not. Written here as
/// well as there because this list has no session behind it to do it.
fn offers_in(opened: &NewSessionResponse) -> Vec<Setting> {
    let options: Vec<Setting> = opened
        .config_options
        .as_ref()
        .map(|options| options.iter().filter_map(setting_of).collect())
        .unwrap_or_default();
    let carried = options
        .iter()
        .any(|option| option.category == Category::Mode);
    let mut offers = Vec::with_capacity(options.len() + 1);
    offers.extend(opened.modes.as_ref().map(mode_setting).filter(|_| !carried));
    offers.extend(options);
    offers
}

/// The mode an agent offers through the dedicated methods, as a setting
/// like any other.
///
/// The protocol is dropping those methods: "Dedicated session mode methods
/// will be removed in a future version of the protocol", and the option
/// with `category: "mode"` is what replaces them -- so an agent in the
/// middle of that change offers both, to be understood by clients on either
/// side of it. Obelus reads the old shape into the new one here, at the
/// edge, so that everything above this has one kind of thing to draw, walk
/// and set. What is left of the old way is [`Setting::legacy`] and the one
/// branch that reads it.
fn mode_setting(state: &SessionModeState) -> Setting {
    Setting {
        id: MODE.to_string(),
        name: "Mode".to_string(),
        about: None,
        values: state
            .available_modes
            .iter()
            .map(|mode| Value {
                id: mode.id.0.to_string(),
                name: mode.name.clone(),
                about: said_twice(mode.description.as_deref(), &mode.name),
            })
            .collect(),
        current: state.current_mode_id.0.to_string(),
        kind: Kind::Select,
        category: Category::Mode,
        legacy: true,
    }
}

/// What Obelus calls the setting it makes out of the old mode methods.
///
/// Only Obelus's own name for it -- the agent never sees it, because a
/// change to this one goes out as `session/set_mode` and names a mode
/// rather than a setting.
pub const MODE: &str = "mode";

/// What Obelus calls the two sides of a switch.
const ON: &str = "on";
/// The other one.
const OFF: &str = "off";

/// The values of a selector, as one list.
fn values_of(options: &SessionConfigSelectOptions) -> Vec<Value> {
    match options {
        SessionConfigSelectOptions::Ungrouped(values) => values.iter().map(value_of).collect(),
        // Flattened, with each group's name kept on its rows. A list of
        // rows that can be chosen and headers that cannot would be a list
        // where the arrows sometimes land on nothing.
        SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|group| {
                group.options.iter().map(|option| {
                    let value = value_of(option);
                    Value {
                        about: Some(match value.about {
                            Some(about) => format!("{} \u{b7} {about}", group.name),
                            None => group.name.clone(),
                        }),
                        ..value
                    }
                })
            })
            .collect(),
        other => {
            tracing::debug!(?other, "values Obelus cannot list");
            Vec::new()
        }
    }
}

/// One value of a selector.
fn value_of(option: &SessionConfigSelectOption) -> Value {
    Value {
        id: option.value.0.to_string(),
        name: option.name.clone(),
        about: said_twice(option.description.as_deref(), &option.name),
    }
}

/// A description, unless it is the name again.
///
/// Agents fill both in for every row whether they have anything to add or
/// not -- Copilot's model list describes "GPT-5.4" as "GPT-5.4" -- and a row
/// that says the same thing twice reads as a mistake in Obelus.
fn said_twice(about: Option<&str>, name: &str) -> Option<String> {
    about
        .filter(|about| about.trim() != name.trim())
        .map(str::to_string)
}

/// A call, from the fields an update carries.
fn call_of(id: &ToolCallId, fields: &ToolCallUpdateFields) -> Call {
    Call {
        id: id.0.to_string(),
        title: fields.title.clone().unwrap_or_default(),
        kind: fields.kind.map(|kind| said_as(&kind)).unwrap_or_default(),
        places: fields
            .locations
            .clone()
            .map(|places| places.iter().map(place_of).collect())
            .unwrap_or_default(),
        change: fields
            .content
            .clone()
            .and_then(|content| change_of(&content)),
        ran: fields.content.clone().and_then(|content| ran_in(&content)),
        said: fields.content.as_deref().map(words_of).unwrap_or_default(),
    }
}

/// The words a call carries, in the order it gave them.
///
/// Its own list rather than one string: a call says things at different
/// moments -- the plan first and what became of it after -- and joining
/// them at the edge would leave whoever draws them unable to tell one from
/// the next.
fn words_of(content: &[ToolCallContent]) -> Vec<String> {
    content
        .iter()
        .filter_map(|content| match content {
            ToolCallContent::Content(block) => words(&block.content),
            // A diff and a terminal are not words: they have their own
            // shapes and their own rows, and reading them as prose would
            // draw a file twice in two different ways.
            _ => None,
        })
        .collect()
}

/// The command a call is running, if it is running one.
fn ran_in(content: &[ToolCallContent]) -> Option<String> {
    content.iter().find_map(|content| match content {
        ToolCallContent::Terminal(terminal) => Some(terminal.terminal_id.0.to_string()),
        _ => None,
    })
}

/// The change a call carries, if it carries one.
fn change_of(content: &[ToolCallContent]) -> Option<Change> {
    content.iter().find_map(|content| match content {
        ToolCallContent::Diff(diff) => Some(Change {
            path: diff.path.clone(),
            before: diff.old_text.clone(),
            after: diff.new_text.clone(),
        }),
        _ => None,
    })
}

/// Where a tool call said it was working.
fn place_of(location: &ToolCallLocation) -> Place {
    Place {
        path: location.path.clone(),
        line: location.line,
    }
}

/// One command, as the view offers it.
fn order_of(order: &AvailableCommand) -> Order {
    Order {
        name: order.name.clone(),
        description: order.description.clone(),
        hint: order.input.as_ref().and_then(|input| match input {
            agent_client_protocol::schema::v1::AvailableCommandInput::Unstructured(hint) => {
                Some(hint.hint.clone())
            }
            // A kind of input Obelus has not heard of. The name is still
            // the command; what it takes after it is between the reader and
            // the agent.
            _ => None,
        }),
    }
}

/// The words in a content block.
///
/// A block is text, an image, audio, or a link to something in the
/// workspace. Only the first is words; the others are named rather than
/// dropped, because a turn that silently loses a block reads as an agent
/// that said nothing.
fn words(content: &ContentBlock) -> Option<String> {
    match content {
        ContentBlock::Text(text) => Some(text.text.clone()),
        ContentBlock::Image(_) => Some("(an image)".to_string()),
        ContentBlock::Audio(_) => Some("(audio)".to_string()),
        ContentBlock::ResourceLink(link) => Some(format!("({})", link.uri)),
        ContentBlock::Resource(_) => Some("(a resource)".to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    /// Every shape Obelus refuses to hand to the machine's own launcher.
    ///
    /// The end-to-end test drives one of these through a real agent; this
    /// is the rest of them, because a predicate with five arms wants five
    /// cases and not five conversations.
    ///
    /// Broken deliberately by returning the string whatever it says.
    #[test]
    fn only_a_web_address_is_somewhere_to_go() {
        for said in [
            // A scheme some program on this machine has registered, which
            // is the shape that turns a link into a way to start it.
            "vscode://file/etc/passwd",
            "ms-msdt:/id",
            // The disk, by either spelling.
            "file:///etc/passwd",
            "file://localhost/etc/passwd",
            // No machine named.
            "https:///nowhere",
            "http://?query",
            // Not a URL at all.
            "console.example.com",
            "javascript:alert(1)",
            // Whitespace, which is only ever `%20` in a well-formed URL and
            // is how a string becomes two arguments.
            "https://example.com/ --a-flag",
            "https://example.com/\nhttps://elsewhere.com",
        ] {
            assert_eq!(
                super::somewhere_to_go(said),
                None,
                "Obelus would have opened {said:?}"
            );
        }

        for said in [
            "https://console.example.com/oauth/authorize?code=1&state=2",
            "http://localhost:8080/callback",
            // The scheme as the agent happens to spell it.
            "HTTPS://example.com/",
        ] {
            assert_eq!(
                super::somewhere_to_go(said).as_deref(),
                Some(said),
                "Obelus would not have opened {said:?}"
            );
        }
    }

    use super::*;

    /// What is written down about a call is cut by characters.
    ///
    /// Broken deliberately by cutting by bytes: a tool call naming a file
    /// with anything but ASCII in it landed the cut inside a character, and
    /// writing the log panicked in the middle of answering the agent.
    #[test]
    fn what_is_said_about_a_call_is_cut_where_a_character_ends() {
        assert_eq!(cut_to("abcdef", 3), "abc");
        // Shorter than the cut is the whole of it, not a panic.
        assert_eq!(cut_to("ab", 8), "ab");
        assert_eq!(cut_to("", 8), "");
        // Three bytes each, so every one of these cuts would be inside a
        // character if it were counting bytes.
        assert_eq!(cut_to("笔记本", 2), "笔记");
        assert_eq!(cut_to("笔记本", 3), "笔记本");
        assert_eq!(cut_to("笔记本", 9), "笔记本");
    }

    /// The transport is the agent's to choose, and Obelus asks.
    ///
    /// The protocol is strict about it -- `Http` and `Sse` are "only
    /// available when the Agent capabilities indicate" so -- and an agent
    /// that is handed one it did not ask for is entitled to ignore it
    /// without saying anything. Obelus hard-coded `Http` and got away with
    /// it because the two agents installed today both take it.
    ///
    /// Broken deliberately by going back to that: the third case stops
    /// being `None` and Obelus offers an agent a server it cannot reach.
    #[test]
    fn the_tools_go_by_whichever_way_the_agent_says_it_takes() {
        let url = Some("http://127.0.0.1:1/mcp");
        let takes = |http, sse| McpCapabilities::new().http(http).sse(sse);

        assert!(matches!(
            offering(url, &takes(true, true)),
            Some(McpServer::Http(_))
        ));
        // Http wins where both are offered: one request and one answer,
        // against a stream Obelus would have to hold open.
        assert!(matches!(
            offering(url, &takes(true, false)),
            Some(McpServer::Http(_))
        ));
        assert!(matches!(
            offering(url, &takes(false, true)),
            Some(McpServer::Sse(_))
        ));
        // Neither: Obelus has a server running and no way to hand it over,
        // and says so rather than offering one that will be dropped.
        assert!(offering(url, &takes(false, false)).is_none());

        // And nothing to offer is nothing to offer, whatever it takes.
        assert!(offering(None, &takes(true, true)).is_none());
    }
}
