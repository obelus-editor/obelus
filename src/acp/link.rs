//! The agent, on the other end of the protocol's own crate.
//!
//! `agent-client-protocol` is the protocol's reference implementation: it
//! spawns the agent, frames the messages, numbers the requests, and gives
//! every method of the protocol a type whose field names the compiler
//! checks. What it is built around is `async`, and obelus's main loop is a
//! thread blocked on a channel -- so this is the join between them.
//!
//! One thread runs the connection. It holds the whole of obelus's side of
//! it: the handshake, every conversation opened on it, and a loop over the
//! [`Ask`]s obelus sends it. In the other direction everything becomes an
//! [`Event`] on the loop's own channel, like the keyboard, the file walk and
//! the language servers. Every message in both directions names which
//! conversation it is about, because one agent holds several.
//!
//! The two directions are not symmetrical, and that is the interesting
//! part. What obelus *asks* is fire-and-forget: a prompt is spawned as a
//! task on the connection, so a cancellation typed while the agent is
//! thinking is read rather than queued behind it. What the agent asks --
//! permission, the text of a file -- is a question obelus cannot answer
//! without the reader, so the handler sends the question to the main loop
//! with a [`oneshot`] to answer through, and waits. Waiting is right there:
//! the agent has stopped, and what it is waiting for is a keystroke.

use std::{path::PathBuf, sync::mpsc::Sender};

use agent_client_protocol::{
    AcpAgentConfig, Client, ConnectionTo,
    schema::{
        ProtocolVersion,
        v1::{
            AvailableCommand, BooleanConfigOptionCapabilities, CancelNotification,
            ClientCapabilities, ClientSessionCapabilities, CloseSessionRequest, ContentBlock,
            CreateElicitationRequest, CreateElicitationResponse, DeleteSessionRequest,
            ElicitationAcceptAction, ElicitationAction, ElicitationCapabilities,
            ElicitationContentValue, ElicitationFormCapabilities, ElicitationMode,
            ElicitationPropertySchema, ElicitationSchema, FileSystemCapabilities, Implementation,
            InitializeRequest, LoadSessionRequest, McpCapabilities, McpServer, McpServerHttp,
            McpServerSse, MultiSelectItems,
            NewSessionRequest, PermissionOptionId, PromptRequest, ReadTextFileRequest,
            ReadTextFileResponse, RequestPermissionOutcome, RequestPermissionRequest,
            RequestPermissionResponse, SelectedPermissionOutcome, SessionConfigId,
            SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory,
            SessionConfigOptionValue, SessionConfigOptionsCapabilities, SessionConfigSelectOption,
            SessionConfigSelectOptions, SessionId, SessionModeState, SessionNotification,
            SessionUpdate, SetSessionConfigOptionRequest, SetSessionModeRequest, TextContent,
            ToolCallContent, ToolCallId, ToolCallLocation, ToolCallUpdateFields,
            WriteTextFileRequest, WriteTextFileResponse,
        },
    },
};
use futures::{
    StreamExt as _,
    channel::{mpsc, oneshot},
};

use crate::event::Event;

/// What obelus asks the agent to do.
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
    /// The agent kept every word of it, so obelus keeps none: what it keeps
    /// is the name, because the agent has no idea a note exists.
    Reopen {
        /// The name obelus wrote down last time.
        session: SessionId,
    },
    /// Say this, in that conversation.
    Say {
        /// Which conversation.
        session: SessionId,
        /// What to say.
        words: String,
        /// What obelus has to say about the conversation first, once.
        ///
        /// Its own block rather than stuck to the front of the words: the
        /// protocol takes a prompt as blocks, and one of these is the
        /// reader talking while the other is obelus saying what they are
        /// talking about.
        opening: Option<String>,
    },
    /// Stop what you are doing in that one.
    Interrupt {
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
    /// here, where what obelus calls the two sides of a switch is also
    /// written down.
    #[must_use]
    pub fn of(setting: &Setting, value: &str) -> Self {
        match setting.kind {
            Kind::Switch => Self::Switch(value == ON),
            Kind::Select => Self::Value(value.to_string()),
        }
    }
}

/// How obelus answers something the agent asked.
///
/// One value, once. `None` is a refusal: no option chosen, or no text to
/// hand over -- both of which the protocol has an answer for.
pub type Answer<T> = oneshot::Sender<T>;

/// One thing from the agent worth acting on.
#[derive(Debug)]
pub enum Incoming {
    /// The handshake finished. What it calls itself, if it said.
    Ready(Option<String>),
    /// There is a session to talk in.
    Started {
        /// Which one, which is what everything said in it names.
        session: SessionId,
        /// The mode it offers through the dedicated methods, if it offers
        /// one that way -- read into a setting like any other, and dropped
        /// if the settings turn out to carry the mode themselves.
        mode: Option<Setting>,
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
        /// obelus reads and obelus threw it away, which was free while
        /// there was one conversation and is the whole of the routing now.
        session: SessionId,
        /// What to show.
        update: Update,
    },
    /// A turn ended, for this reason.
    Ended {
        /// Whose turn.
        session: SessionId,
        /// And why it stopped.
        why: String,
    },
    /// Something did not work: what obelus was doing, and what it said.
    Failed(&'static str, String),
    /// A conversation obelus asked to pick up again is not there any more.
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
        call: Call,
        /// And in its own words: which command, which file -- what the
        /// reader is actually being asked about.
        reason: Option<String>,
        /// What obelus may answer.
        options: Vec<Choice>,
        /// Which option, or nothing for "not answered".
        answer: Answer<Option<String>>,
    },
    /// The agent is asking the reader for something.
    ///
    /// With no conversation on it, because the protocol does not put one
    /// there: `session/request_permission` names the session and an
    /// elicitation does not. Whoever routes this has to work it out, and
    /// the only honest answer is "whichever one is waiting on the agent".
    Ask {
        /// What it says it needs, in its own words.
        message: String,
        /// What it wants, in the order obelus will put them.
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
        /// The text, or nothing for "obelus will not read that".
        answer: Answer<Option<String>>,
    },
    /// The conversation is over: the agent exited, or the protocol did.
    Gone(Option<String>),
}

/// One thing the agent said, in the form the view shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Update {
    /// A piece of the answer.
    Said(String),
    /// A piece of its thinking, which agents send separately so that it can
    /// be shown as what it is.
    Thought(String),
    /// It is using a tool, and this is where it has got to.
    Tool {
        /// The call itself.
        call: Call,
        /// `pending`, `in_progress`, `completed` or `failed`. Empty on an
        /// update that did not say, which means it has not changed.
        status: String,
    },
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
    /// and again after every change -- by obelus or by the agent itself.
    Settings(Vec<Setting>),
}

/// One thing about the session the agent lets the reader change.
///
/// The model, how hard it thinks, whether it asks before doing things: the
/// agent names them, obelus lists them. Which are on offer is the agent's,
/// and so is what each of them means -- obelus only shows the names and
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
    /// methods, which obelus reads into a setting like any other. The
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
    /// On or off, which obelus offers as two values of its own making.
    Switch,
}

/// What a setting is about, in the protocol's own words.
///
/// `category` exists for exactly this -- the spec's own list of what a
/// client may do with it is "keyboard shortcuts, icons, placement" -- and
/// it says a client must work without it. So obelus uses it for a glyph,
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
    /// one: the name of a category obelus has never heard of tells it no
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
        self.values
            .iter()
            .find(|value| value.id == self.current)
            .map(|value| value.name.as_str())
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
/// A field of a form, in the shape obelus can put it: what to call it, what
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
    /// something they can only read about: obelus opens files for a living,
    /// and this is the agent saying which.
    pub places: Vec<Place>,
    /// The change it is making, when it said.
    pub change: Option<Change>,
}

/// A change to a file, as the agent describes it.
///
/// The file as it is and as it would be, which is what the protocol sends
/// rather than a patch: obelus diffs the two with the engine it diffs
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
    events: Sender<Event>,
) -> mpsc::UnboundedSender<Ask> {
    let (asks, taken) = mpsc::unbounded();
    let config = AcpAgentConfig::new(command).args(arguments.iter().cloned());
    let root = root.to_path_buf();
    let told = events.clone();
    let outcome = std::thread::Builder::new()
        .name("obelus-acp".to_string())
        .spawn(move || {
            // A runtime of its own, on this thread: the conversation is one
            // connection with a handful of tasks in it, so a current-thread
            // runtime is the whole of what it needs -- a work-stealing pool
            // for one agent would be threads nobody asked for. Everything
            // obelus does outside this thread is still a thread blocked on
            // a channel.
            //
            // The channels stay `futures`': that is what the protocol's own
            // crate speaks, and a channel is runtime-agnostic anyway. What
            // tokio is here for is driving them.
            let reason = match tokio::runtime::Builder::new_current_thread().build() {
                Ok(runtime) => runtime.block_on(talk(config, root, tools, told.clone(), taken)),
                Err(error) => Some(error.to_string()),
            };
            let _ = told.send(Event::Acp(Incoming::Gone(reason)));
        });
    if let Err(error) = outcome {
        tracing::warn!(%error, "no thread for the agent");
        let _ = events.send(Event::Acp(Incoming::Gone(Some(error.to_string()))));
    }
    asks
}

/// Opens one conversation on a connection that is already up.
///
/// Both the first and every one after it: the first is opened without being
/// asked for, because opening the view *is* the request, and the rest come
/// from `Ask::Open`. One path for both, so that what a new conversation
/// arrives with cannot depend on which it is.
/// Which way obelus can hand an agent its tools, if any.
///
/// The agent says in the handshake which transports it can connect to, and
/// the protocol is strict about it: `Http` and `Sse` are "only available
/// when the agent capabilities indicate" so. Offering one it did not ask
/// for is a server it is entitled to ignore without saying anything, which
/// is the shape of an obelus that looks like it works and quietly offers
/// nothing.
///
/// `Stdio` every agent must take, but the agent is the one that spawns
/// the server there -- obelus's is already running inside obelus, so it
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
                "this agent takes neither http nor sse, so obelus offers it no tools"
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
    events: &Sender<Event>,
    stopped: &mut std::collections::HashMap<
        SessionId,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
    >,
) -> Result<SessionId, agent_client_protocol::Error> {
    // What obelus itself offers the agent: a handful of tools about this
    // reader's notes, which the protocol has no way to express because it is
    // about talking to an agent rather than about being talked to.
    let mut asking = NewSessionRequest::new(root.to_path_buf());
    if let Some(offered) = tools {
        asking = asking.mcp_servers(vec![offered.clone()]);
    }
    let opened = connection.send_request(asking).block_task().await?;
    let session = opened.session_id.clone();
    stopped.entry(session.clone()).or_default();
    // The old mode methods, read into a setting at the edge -- and kept
    // only until the settings say they carry the mode themselves, which is
    // what replaces them.
    let mode = opened.modes.as_ref().map(mode_setting);
    let _ = events.send(Event::Acp(Incoming::Started {
        session: session.clone(),
        mode,
    }));
    if let Some(options) = opened.config_options.as_ref() {
        let settings = options.iter().filter_map(setting_of).collect();
        let _ = events.send(Event::Acp(Incoming::Update {
            session: session.clone(),
            update: Update::Settings(settings),
        }));
    }
    Ok(session)
}

/// The whole connection, from the handshake to the end of the stream.
///
/// Returns why it ended, or `None` because it ended tidily.
async fn talk(
    config: AcpAgentConfig,
    root: PathBuf,
    tools: Option<String>,
    events: Sender<Event>,
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
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, _connection| {
                // The reader's to answer, so the question goes to the main
                // loop and this waits for the keystroke. An answer that
                // never comes -- the view closed, obelus quit -- is the
                // protocol's "cancelled", which is what an agent needs to
                // hear to stop waiting.
                let (answer, answered) = oneshot::channel();
                let question = Incoming::Permission {
                    session: request.session_id.clone(),
                    call: call_of(&request.tool_call.tool_call_id, &request.tool_call.fields),
                    reason: reason_of(&request),
                    options: request
                        .options
                        .iter()
                        .map(|option| Choice {
                            id: option.option_id.0.to_string(),
                            name: option.name.clone(),
                            kind: format!("{:?}", option.kind).to_lowercase(),
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
                // From a buffer if obelus has one, which is the main loop's
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
                    return responder.respond_with_error(refusal("obelus is not listening"));
                }
                match answered.await {
                    Ok(Some(text)) => responder.respond(ReadTextFileResponse::new(text)),
                    Ok(None) | Err(_) => {
                        responder.respond_with_error(refusal("obelus will not read that"))
                    }
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: CreateElicitationRequest, responder, _connection| {
                // The agent is asking the reader something. What it may ask
                // for is a flat form of primitives, and obelus puts that
                // the way it puts every other choice: a list where the
                // answer is one of a few, the box where it is words. One
                // field at a time, because a terminal reader has one thing
                // on screen and one caret in it.
                let asked = match &request.mode {
                    ElicitationMode::Form(form) => fields_of(&form.requested_schema),
                    // A mode obelus never offered to show -- a URL to open,
                    // which a reader is not in a browser to follow.
                    // Declined rather than errored: the agent asked a fair
                    // question of a client that cannot put it, and it has
                    // to be able to carry on.
                    other => Err(format!("{other:?}")),
                };
                let fields = match asked {
                    Ok(fields) => fields,
                    Err(why) => {
                        let _ = elicited.send(Event::Acp(Incoming::Failed(
                            "a question obelus cannot put",
                            why,
                        )));
                        return responder
                            .respond(CreateElicitationResponse::new(ElicitationAction::Decline));
                    }
                };
                let (answer, answered) = oneshot::channel();
                let question = Incoming::Ask {
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
            async move |request: WriteTextFileRequest, responder, _connection| {
                // Through the main loop, the same as a read -- and for a
                // stronger reason. A file obelus has open is a document the
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
                    return responder.respond_with_error(refusal("obelus is not listening"));
                }
                match answered.await {
                    Ok(true) => responder.respond(WriteTextFileResponse::new()),
                    Ok(false) | Err(_) => {
                        responder.respond_with_error(refusal("obelus will not write that"))
                    }
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(
            agent,
            |connection: ConnectionTo<agent_client_protocol::Agent>| async move {
                let ready = connection.send_request(handshake()).block_task().await?;
                let named = ready.agent_info.map(|info| match info.version.is_empty() {
                    true => info.name.clone(),
                    false => format!("{} {}", info.name, info.version),
                });
                let _ = events.send(Event::Acp(Incoming::Ready(named)));

                // Which way the tools can be handed over, decided once from
                // what the agent said it takes rather than guessed afresh
                // per session: the answer cannot change while the agent
                // runs, and asking twice would be two answers to keep alike.
                let offered = offering(
                    tools.as_deref(),
                    &ready.agent_capabilities.mcp_capabilities,
                );

                // Whether each conversation's turn in flight has been
                // given up on.
                //
                // One flag per conversation and not one for the connection.
                // It is set by an interruption, cleared by the next prompt,
                // and read by the answer's callback -- an agent that never
                // saw the cancellation answers the prompt anyway, and by
                // then the reader has been told that turn is over. Shared
                // between two conversations, stopping one would throw away
                // the answer arriving in the other, with nothing to say so
                // and no way to notice but to be talking in two at once.
                let mut stopped: std::collections::HashMap<
                    SessionId,
                    std::sync::Arc<std::sync::atomic::AtomicBool>,
                > = std::collections::HashMap::new();

                // The first one, opened without being asked for: the reader
                // opened the view, which is a request to talk.
                open_session(&connection, &root, offered.as_ref(), &events, &mut stopped).await?;

                while let Some(ask) = asks.next().await {
                    match ask {
                        Ask::Open => {
                            open_session(
                                &connection,
                                &root,
                                offered.as_ref(),
                                &events,
                                &mut stopped,
                            )
                            .await?;
                        }
                        // Gone, because the note it was about is. Two ways
                        // to say it and they mean different things: `delete`
                        // is "forget this", `close` is "I am done talking in
                        // it". The first is what a deleted note means, so it
                        // is tried first and the second is the fallback for
                        // an agent that only offers that.
                        Ask::Drop { session } => {
                            stopped.remove(&session);
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
                            let taken = connection
                                .send_request(LoadSessionRequest::new(
                                    session.clone(),
                                    root.clone(),
                                ))
                                .block_task()
                                .await;
                            match taken {
                                Ok(loaded) => {
                                    stopped.entry(session.clone()).or_default();
                                    let mode = loaded.modes.as_ref().map(mode_setting);
                                    let _ = events.send(Event::Acp(Incoming::Started {
                                        session: session.clone(),
                                        mode,
                                    }));
                                    if let Some(options) = loaded.config_options.as_ref() {
                                        let settings =
                                            options.iter().filter_map(setting_of).collect();
                                        let _ = events.send(Event::Acp(Incoming::Update {
                                            session,
                                            update: Update::Settings(settings),
                                        }));
                                    }
                                }
                                Err(error) => {
                                    let _ = events.send(Event::Acp(Incoming::Lost {
                                        session: session.clone(),
                                        why: error.to_string(),
                                    }));
                                    open_session(
                                        &connection,
                                        &root,
                                        offered.as_ref(),
                                        &events,
                                        &mut stopped,
                                    )
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
                            words,
                            opening,
                        } => {
                            let told = events.clone();
                            let flag = stopped.entry(session.clone()).or_default();
                            flag.store(false, std::sync::atomic::Ordering::Relaxed);
                            let given_up = std::sync::Arc::clone(flag);
                            let whose = session.clone();
                            connection
                                .send_request(PromptRequest::new(
                                    session,
                                    opening
                                        .into_iter()
                                        .chain(std::iter::once(words))
                                        .map(|said| ContentBlock::Text(TextContent::new(said)))
                                        .collect(),
                                ))
                                .on_receiving_result(move |asked| {
                                    // A turn the reader stopped is over,
                                    // and whatever the agent says about it
                                    // now is about something nobody is
                                    // waiting for.
                                    if given_up.load(std::sync::atomic::Ordering::Relaxed) {
                                        return std::future::ready(Ok(()));
                                    }
                                    let _ = told.send(Event::Acp(match asked {
                                        Ok(answer) => Incoming::Ended {
                                            session: whose,
                                            why: format!("{:?}", answer.stop_reason).to_lowercase(),
                                        },
                                        Err(error) => {
                                            Incoming::Failed("the agent", error.to_string())
                                        }
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
                        Ask::Interrupt { session } => {
                            stopped
                                .entry(session.clone())
                                .or_default()
                                .store(true, std::sync::atomic::Ordering::Relaxed);
                            connection
                                .send_notification(CancelNotification::new(session.clone()))?;
                            let _ = events.send(Event::Acp(Incoming::Ended {
                                session,
                                why: "cancelled".to_string(),
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
                                            "changing a setting",
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
                                            "changing the mode",
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

/// What obelus tells an agent about itself.
///
/// It reads files out and writes them back inside the tree it was opened
/// on, and it has no terminal to offer. Declaring the truth here is what
/// keeps a well-behaved agent from asking for the rest.
fn handshake() -> InitializeRequest {
    InitializeRequest::new(ProtocolVersion::V1)
        .client_capabilities(
            ClientCapabilities::new()
                .fs(FileSystemCapabilities::new()
                    .read_text_file(true)
                    .write_text_file(true))
                .terminal(false)
                .elicitation(
                    ElicitationCapabilities::new().form(ElicitationFormCapabilities::new()),
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

/// What obelus says when it will not do something.
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
/// which obelus would have to guess the meaning of. The typed fields are
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

/// What a `session/update` means, if it is one obelus shows.
///
/// The protocol has a dozen and a half kinds and this reads eight. The rest
/// -- plans, usage, compaction -- are facts about the agent rather than
/// about the conversation, and a conversation with them in it is a log.
fn read_update(update: SessionUpdate) -> Vec<Update> {
    match update {
        SessionUpdate::AgentMessageChunk(chunk) => words(&chunk.content)
            .map(Update::Said)
            .into_iter()
            .collect(),
        SessionUpdate::AgentThoughtChunk(chunk) => words(&chunk.content)
            .map(Update::Thought)
            .into_iter()
            .collect(),
        SessionUpdate::ToolCall(call) => vec![Update::Tool {
            call: Call {
                id: call.tool_call_id.0.to_string(),
                title: call.title.clone(),
                kind: format!("{:?}", call.kind).to_lowercase(),
                places: call.locations.iter().map(place_of).collect(),
                change: change_of(&call.content),
            },
            status: format!("{:?}", call.status).to_lowercase(),
        }],
        // A later update carries only what changed, so what it leaves out
        // arrives here as nothing and is read as "the same as before".
        SessionUpdate::ToolCallUpdate(call) => vec![Update::Tool {
            call: call_of(&call.tool_call_id, &call.fields),
            status: call
                .fields
                .status
                .map(|status| format!("{status:?}").to_lowercase())
                .unwrap_or_default(),
        }],
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
        SessionUpdate::SessionInfoUpdate(info) => match info.title {
            agent_client_protocol::schema::MaybeUndefined::Value(title) => {
                vec![Update::Titled(title)]
            }
            agent_client_protocol::schema::MaybeUndefined::Undefined
            | agent_client_protocol::schema::MaybeUndefined::Null => Vec::new(),
        },
        other => {
            tracing::debug!(?other, "an update obelus does not show");
            Vec::new()
        }
    }
}

/// The fields of a form, in the order obelus will put them.
///
/// Or why it cannot put this one. A form obelus half-fills in is worse than
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
    // the time obelus sees it, and asking by the alphabet put "Other" in
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

/// One setting, as the view offers it -- if it is one obelus can show.
///
/// Nothing but a kind it has never heard of is dropped: a setting whose
/// values obelus cannot list is a row that would do nothing when chosen,
/// and the agent's own dialog for it is not obelus's to open.
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
            tracing::debug!(?other, "a setting obelus cannot show");
            return None;
        }
    };
    // What each one is, once, where it arrives. How an agent declares a
    // setting decides how obelus draws it and what pressing enter on it
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

/// What the agent said a setting is about, as one of the few obelus can do
/// something with.
///
/// A category obelus has never heard of is [`Category::Other`], which is
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
            tracing::debug!(name, "a category obelus has never heard of");
            Category::Other
        }
        None | Some(_) => Category::Other,
    }
}

/// The mode an agent offers through the dedicated methods, as a setting
/// like any other.
///
/// The protocol is dropping those methods: "Dedicated session mode methods
/// will be removed in a future version of the protocol", and the option
/// with `category: "mode"` is what replaces them -- so an agent in the
/// middle of that change offers both, to be understood by clients on either
/// side of it. obelus reads the old shape into the new one here, at the
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

/// What obelus calls the setting it makes out of the old mode methods.
///
/// Only obelus's own name for it -- the agent never sees it, because a
/// change to this one goes out as `session/set_mode` and names a mode
/// rather than a setting.
pub const MODE: &str = "mode";

/// What obelus calls the two sides of a switch.
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
            tracing::debug!(?other, "values obelus cannot list");
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
/// that says the same thing twice reads as a mistake in obelus.
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
        kind: fields
            .kind
            .map(|kind| format!("{kind:?}").to_lowercase())
            .unwrap_or_default(),
        places: fields
            .locations
            .clone()
            .map(|places| places.iter().map(place_of).collect())
            .unwrap_or_default(),
        change: fields
            .content
            .clone()
            .and_then(|content| change_of(&content)),
    }
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
            // A kind of input obelus has not heard of. The name is still
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
    use super::*;

    /// The transport is the agent's to choose, and obelus asks.
    ///
    /// The protocol is strict about it -- `Http` and `Sse` are "only
    /// available when the Agent capabilities indicate" so -- and an agent
    /// that is handed one it did not ask for is entitled to ignore it
    /// without saying anything. obelus hard-coded `Http` and got away with
    /// it because the two agents installed today both take it.
    ///
    /// Broken deliberately by going back to that: the third case stops
    /// being `None` and obelus offers an agent a server it cannot reach.
    #[test]
    fn the_tools_go_by_whichever_way_the_agent_says_it_takes() {
        let url = Some("http://127.0.0.1:1/mcp");
        let takes = |http, sse| McpCapabilities::new().http(http).sse(sse);

        assert!(matches!(
            offering(url, &takes(true, true)),
            Some(McpServer::Http(_))
        ));
        // Http wins where both are offered: one request and one answer,
        // against a stream obelus would have to hold open.
        assert!(matches!(
            offering(url, &takes(true, false)),
            Some(McpServer::Http(_))
        ));
        assert!(matches!(
            offering(url, &takes(false, true)),
            Some(McpServer::Sse(_))
        ));
        // Neither: obelus has a server running and no way to hand it over,
        // and says so rather than offering one that will be dropped.
        assert!(offering(url, &takes(false, false)).is_none());

        // And nothing to offer is nothing to offer, whatever it takes.
        assert!(offering(None, &takes(true, true)).is_none());
    }
}
