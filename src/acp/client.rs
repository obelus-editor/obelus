//! One running agent, and the bookkeeping the protocol needs.
//!
//! This layer knows the handshake, the request numbering, and which of the
//! agent's own requests obelus answers. It does not know what a conversation
//! looks like: what arrives is turned into [`Incoming`] and handed on, and
//! the view decides what it means.

use std::{
    collections::HashMap,
    io::{BufReader, BufWriter},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Sender},
};

use anyhow::{Context as _, Result};
use serde_json::{Value, json};

use crate::{acp::transport, event::Event};

/// The id obelus uses for `initialize`.
///
/// Fixed rather than drawn from the counter, so the one reply that has to be
/// recognised here needs no bookkeeping of its own.
const INITIALIZE_ID: i64 = 0;

/// What obelus asked for, so an answer can be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ask {
    /// The handshake.
    Initialize,
    /// A session to talk in.
    NewSession,
    /// A turn of the conversation.
    Prompt,
    /// A change of the way of working.
    SetMode,
}

impl Ask {
    /// What to call it when it fails.
    const fn what(self) -> &'static str {
        match self {
            Self::Initialize => "starting the agent",
            Self::NewSession => "opening a session",
            Self::Prompt => "the agent",
            Self::SetMode => "changing the mode",
        }
    }
}

/// Something from the agent worth acting on.
#[derive(Clone, Debug)]
pub enum Incoming {
    /// Nothing the caller has to do. Dealt with here, or ignored on purpose.
    Nothing,
    /// The handshake finished and a session is being opened.
    Ready,
    /// There is a session, so prompts can be sent.
    Started,
    /// Something to show.
    Update(Update),
    /// The turn ended, for this reason.
    Ended(String),
    /// Something obelus asked for did not work: what it was, and what the
    /// agent said.
    Failed(&'static str, String),
    /// The agent is asking to be allowed to do something.
    Permission(Permission),
    /// The agent wants a file's text. Answered from a buffer if obelus has
    /// one, because what a reader is looking at is not always what is on
    /// disk.
    Read {
        /// The request to answer.
        id: Value,
        /// Which file.
        path: PathBuf,
        /// The first line it wants, counted from one.
        line: Option<u32>,
        /// How many lines.
        limit: Option<u32>,
    },
}

/// One thing the agent said, in the form the view shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Update {
    /// A piece of the answer.
    Said(String),
    /// A piece of the agent's thinking, which agents send separately so it
    /// can be shown as what it is.
    Thought(String),
    /// The way of working changed, to this one's id. Which the agent can do
    /// on its own -- finishing a plan and starting to write code is a mode
    /// change nobody pressed a key for.
    Mode(String),
    /// The commands the agent takes. Sent once the session is ready,
    /// usually, and again whenever they change.
    Orders(Vec<Order>),
    /// The agent is using a tool: what it calls the call, and where it has
    /// got to.
    Tool {
        /// The agent's own id for the call, so a later update replaces the
        /// row rather than adding one.
        id: String,
        /// What it calls it.
        title: String,
        /// `pending`, `in_progress`, `completed` or `failed`.
        status: String,
    },
}

/// One way of working the agent offers.
///
/// Agents call these modes: "ask", "plan", "code", whatever they have.
/// What they mean is the agent's business; what obelus does is show which
/// one is on and let the reader change it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mode {
    /// The agent's id for it, which is what a change names.
    pub id: String,
    /// What to call it on screen.
    pub name: String,
}

/// One command the agent offers, of the kind typed with a slash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Order {
    /// Its name, without the slash.
    pub name: String,
    /// One line about what it does.
    pub description: String,
    /// What it says about whatever is typed after the name, if it takes
    /// anything.
    pub hint: Option<String>,
}

/// A question the agent is waiting on an answer to.
#[derive(Clone, Debug)]
pub struct Permission {
    /// The request to answer.
    pub id: Value,
    /// What it wants to do.
    pub title: String,
    /// What obelus may answer.
    pub options: Vec<Choice>,
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

/// A running agent.
pub struct Client {
    /// The registry's id for it, which is what the settings stored.
    id: String,
    process: Child,
    /// Whether the process has been found to have stopped.
    exited: bool,
    outgoing: Sender<String>,
    next_id: i64,
    /// What each request obelus sent was for.
    pending: HashMap<i64, Ask>,
    /// The session to talk in, once there is one.
    session: Option<String>,
    /// Whether the handshake has finished.
    ready: bool,
    /// What the agent called itself, if it said.
    info: Option<String>,
    /// The prompt in flight, if there is one.
    turn: Option<i64>,
    /// The ways of working the agent offers, and which one is on.
    ///
    /// From the session it opened, and then from the agent's own updates: a
    /// mode can change because the reader asked for it or because the agent
    /// moved on to something else, and both arrive the same way.
    modes: Vec<Mode>,
    /// Which mode is on, by its id.
    mode: Option<String>,
    /// The commands the agent says it takes.
    ///
    /// Empty until it says. Most agents send this once, just after the
    /// session opens; some never do, and then a slash is just a character.
    orders: Vec<Order>,
    /// A prompt typed before there was a session to send it in.
    ///
    /// The reader can open the agent view and start typing while the process
    /// is still starting, which is the ordinary case: a node agent takes a
    /// second. Holding it here means they do not have to press enter twice.
    held: Option<String>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Client")
            .field("id", &self.id)
            .field("ready", &self.ready)
            .field("session", &self.session)
            .field("turn", &self.turn)
            .finish_non_exhaustive()
    }
}

impl Client {
    /// Starts an agent and sends the handshake.
    ///
    /// Returns as soon as the process is spawned. Nothing can be said to it
    /// until a session exists, which is two round trips away; [`Client::say`]
    /// holds a prompt typed before then.
    pub fn start(
        id: &str,
        command: &Path,
        arguments: &[String],
        root: &Path,
        sender: Sender<Event>,
    ) -> Result<Self> {
        let mut process = Command::new(command)
            .args(arguments)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("starting {}", command.display()))?;

        let stdout = process.stdout.take().context("the agent has no stdout")?;
        let stdin = process.stdin.take().context("the agent has no stdin")?;
        // Piped rather than discarded: an agent that will not start says why
        // here and nowhere else.
        if let Some(stderr) = process.stderr.take() {
            spawn_logger(id.to_string(), stderr);
        }

        spawn_reader(stdout, sender);
        let outgoing = spawn_writer(id.to_string(), stdin);

        let mut client = Self {
            id: id.to_string(),
            process,
            exited: false,
            outgoing,
            next_id: INITIALIZE_ID + 1,
            pending: HashMap::new(),
            session: None,
            ready: false,
            info: None,
            turn: None,
            held: None,
            modes: Vec::new(),
            mode: None,
            orders: Vec::new(),
        };
        client.send_initialize()?;
        Ok(client)
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
        self.session.is_some()
    }

    /// Whether a turn is in flight.
    #[must_use]
    pub const fn is_thinking(&self) -> bool {
        self.turn.is_some()
    }

    /// The ways of working the agent offers.
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

    /// The commands the agent says it takes.
    #[must_use]
    pub fn orders(&self) -> &[Order] {
        &self.orders
    }

    /// Changes the way of working, by walking to the next one.
    ///
    /// Walked rather than chosen from a list: there are two or three of
    /// these and they are a cycle in the agent's own order, which is what a
    /// key that steps through them means. The agent answers with nothing,
    /// so what is shown changes here -- and its own update, if it sends
    /// one, says the same thing again.
    pub fn step_mode(&mut self) -> Result<()> {
        let Some(session) = self.session.clone() else {
            return Ok(());
        };
        if self.modes.len() < 2 {
            return Ok(());
        }
        let at = self
            .mode
            .as_deref()
            .and_then(|id| self.modes.iter().position(|mode| mode.id == id))
            .unwrap_or(0);
        let next = self.modes[(at + 1) % self.modes.len()].id.clone();
        self.mode = Some(next.clone());
        self.request(
            Ask::SetMode,
            "session/set_mode",
            &json!({ "sessionId": session, "modeId": next }),
        )
        .map(|_| ())
    }

    /// Asks the operating system whether the process is still running, and
    /// remembers the answer.
    ///
    /// An agent that has died is not otherwise noticed: its reader thread
    /// stops, and every prompt after that goes unanswered with nothing to
    /// say so.
    pub fn check_alive(&mut self) -> bool {
        if self.exited {
            return false;
        }
        if matches!(self.process.try_wait(), Ok(Some(_))) {
            tracing::warn!(id = self.id, "the agent has exited");
            self.exited = true;
        }
        !self.exited
    }

    /// Whether the process has been found to have stopped.
    #[must_use]
    pub const fn has_exited(&self) -> bool {
        self.exited
    }

    /// Sends a prompt, or holds it until there is a session to send it in.
    ///
    /// Says whether it went: a prompt that is being held is a prompt the
    /// view shows as sent, because the reader has finished with it either
    /// way.
    pub fn say(&mut self, text: &str) -> Result<bool> {
        let Some(session) = self.session.clone() else {
            self.held = Some(text.to_string());
            return Ok(false);
        };
        let id = self.request(
            Ask::Prompt,
            "session/prompt",
            &json!({
                "sessionId": session,
                "prompt": [{ "type": "text", "text": text }],
            }),
        )?;
        self.turn = Some(id);
        Ok(true)
    }

    /// Asks the agent to stop what it is doing.
    ///
    /// A notification, so there is no answer to wait for -- the turn ends
    /// with the agent's own `cancelled` stop reason, which is why the turn
    /// is left in flight here.
    pub fn interrupt(&mut self) -> Result<()> {
        let Some(session) = self.session.clone() else {
            return Ok(());
        };
        if self.turn.is_none() {
            return Ok(());
        }
        self.notify("session/cancel", &json!({ "sessionId": session }))
    }

    /// Answers one of the agent's requests.
    pub fn answer(&mut self, id: &Value, result: Value) -> Result<()> {
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "result": result }))
    }

    /// Refuses one of the agent's requests, with a reason.
    pub fn refuse(&mut self, id: &Value, code: i64, message: &str) -> Result<()> {
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": code, "message": message },
        }))
    }

    /// Takes one message from the agent.
    ///
    /// Everything the protocol needs rather than the view -- the handshake,
    /// opening the session, methods obelus does not implement -- is dealt
    /// with here and reported as [`Incoming::Nothing`].
    pub fn on_message(&mut self, message: &Value) -> Incoming {
        if let Some(method) = message.get("method").and_then(Value::as_str) {
            return self.on_call(method, message);
        }
        let Some(id) = message.get("id").and_then(Value::as_i64) else {
            tracing::debug!(?message, "a message that is neither a call nor an answer");
            return Incoming::Nothing;
        };
        let ask = self.pending.remove(&id);

        if let Some(error) = message.get("error") {
            let why = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("no reason given")
                .to_string();
            if ask == Some(Ask::Prompt) {
                self.turn = None;
            }
            return Incoming::Failed(ask.map_or("the agent", Ask::what), why);
        }
        let result = message.get("result").cloned().unwrap_or(Value::Null);

        match ask {
            Some(Ask::Initialize) => self.on_initialized(&result),
            Some(Ask::NewSession) => self.on_session(&result),
            // Nothing comes back but an empty object, and what it confirms
            // is already on screen: the mode changed when the reader asked
            // for it, because an agent that refuses says so with an error.
            Some(Ask::SetMode) => Incoming::Nothing,
            Some(Ask::Prompt) => {
                self.turn = None;
                Incoming::Ended(
                    result
                        .get("stopReason")
                        .and_then(Value::as_str)
                        .unwrap_or("end_turn")
                        .to_string(),
                )
            }
            None => {
                tracing::debug!(id, "an answer to something obelus did not ask");
                Incoming::Nothing
            }
        }
    }

    /// Stops the agent, politely and then not.
    pub fn shutdown(&mut self) {
        // Dropping the sender ends the writer thread once it has written
        // what it has.
        let (dead, _) = mpsc::channel();
        self.outgoing = dead;
        for _ in 0..8 {
            match self.process.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(25)),
                Err(_) => break,
            }
        }
        let _ = self.process.kill();
        let _ = self.process.wait();
    }

    /// The handshake's answer: what the agent is, and a session to talk in.
    fn on_initialized(&mut self, result: &Value) -> Incoming {
        self.ready = true;
        let version = result.get("protocolVersion").and_then(Value::as_u64);
        self.info = result
            .get("agentInfo")
            .and_then(|info| {
                let name = info.get("name").and_then(Value::as_str)?;
                let version = info.get("version").and_then(Value::as_str);
                Some(match version {
                    Some(version) => format!("{name} {version}"),
                    None => name.to_string(),
                })
            })
            .or_else(|| self.info.clone());
        // An agent answering with a version obelus does not speak is an
        // agent to stop rather than guess at: every message after this
        // would be a different protocol's.
        if version.is_some_and(|version| version != u64::from(super::VERSION)) {
            let why = format!(
                "it speaks protocol version {}, and obelus speaks {}",
                version.unwrap_or_default(),
                super::VERSION
            );
            return Incoming::Failed("starting the agent", why);
        }
        if let Err(error) = self.open_session() {
            return Incoming::Failed("opening a session", error.to_string());
        }
        Incoming::Ready
    }

    /// The session's answer, and whatever was typed while it was opening.
    fn on_session(&mut self, result: &Value) -> Incoming {
        let Some(session) = result.get("sessionId").and_then(Value::as_str) else {
            return Incoming::Failed("opening a session", "it named no session".to_string());
        };
        self.session = Some(session.to_string());
        if let Some(state) = result.get("modes") {
            self.modes = modes_in(state);
            self.mode = state
                .get("currentModeId")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        if let Some(held) = self.held.take()
            && let Err(error) = self.say(&held)
        {
            return Incoming::Failed("the agent", error.to_string());
        }
        Incoming::Started
    }

    /// Opens the session, rooted where obelus was started.
    ///
    /// The current directory rather than a git root: it is what the file
    /// list, the search and the language server all use, so an agent that
    /// sees something else is an agent talking about a different project.
    fn open_session(&mut self) -> Result<()> {
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        self.request(
            Ask::NewSession,
            "session/new",
            &json!({ "cwd": root, "mcpServers": [] }),
        )?;
        Ok(())
    }

    /// Something the agent called obelus about.
    fn on_call(&mut self, method: &str, message: &Value) -> Incoming {
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let id = message.get("id").cloned();

        match (method, id) {
            // A notification, so no answer.
            ("session/update", _) => match update_in(&params) {
                // Two of the kinds are about the agent rather than about
                // the conversation, so they are kept here and the view
                // reads them off the client like everything else it knows.
                Some(Update::Mode(id)) => {
                    self.mode = Some(id);
                    Incoming::Nothing
                }
                Some(Update::Orders(orders)) => {
                    self.orders = orders;
                    Incoming::Nothing
                }
                Some(update) => Incoming::Update(update),
                None => Incoming::Nothing,
            },
            ("session/request_permission", Some(id)) => match permission_in(id, &params) {
                Some(permission) => Incoming::Permission(permission),
                None => Incoming::Nothing,
            },
            ("fs/read_text_file", Some(id)) => match params.get("path").and_then(Value::as_str) {
                Some(path) => Incoming::Read {
                    id,
                    path: PathBuf::from(path),
                    line: params
                        .get("line")
                        .and_then(Value::as_u64)
                        .and_then(|line| u32::try_from(line).ok()),
                    limit: params
                        .get("limit")
                        .and_then(Value::as_u64)
                        .and_then(|limit| u32::try_from(limit).ok()),
                },
                None => {
                    let _ = self.refuse(&id, INVALID_PARAMS, "no path");
                    Incoming::Nothing
                }
            },
            // Refused here rather than reported, because the answer does not
            // depend on anything the view knows: obelus does not write
            // files, and said so in the handshake. An agent asking anyway
            // gets the protocol's own "no such method".
            ("fs/write_text_file", Some(id)) => {
                tracing::info!(id = self.id, "the agent tried to write a file");
                let _ = self.refuse(
                    &id,
                    NO_SUCH_METHOD,
                    "obelus is a reader and does not write files",
                );
                Incoming::Nothing
            }
            // Everything else obelus did not declare: terminals, MCP,
            // elicitation. A well-behaved agent never asks; one that does
            // gets an answer rather than silence, which would hang its turn.
            (_, Some(id)) => {
                tracing::debug!(method, "a request obelus does not implement");
                let _ = self.refuse(&id, NO_SUCH_METHOD, "obelus does not implement this");
                Incoming::Nothing
            }
            (_, None) => {
                tracing::debug!(method, "a notification obelus ignores");
                Incoming::Nothing
            }
        }
    }

    /// Sends a request and returns the id its answer will carry.
    fn request(&mut self, ask: Ask, method: &str, params: &Value) -> Result<i64> {
        let id = if ask == Ask::Initialize {
            INITIALIZE_ID
        } else {
            let id = self.next_id;
            self.next_id += 1;
            id
        };
        self.pending.insert(id, ask);
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))?;
        Ok(id)
    }

    /// Sends a notification, which has no answer.
    fn notify(&mut self, method: &str, params: &Value) -> Result<()> {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
    }

    fn send(&mut self, message: &Value) -> Result<()> {
        let body = serde_json::to_string(message).context("serialising a message")?;
        self.outgoing
            .send(body)
            .context("the writer thread has ended")
    }

    /// What obelus tells an agent about itself.
    ///
    /// It reads files out and does not write them, and it has no terminal to
    /// offer. Declaring the truth here is what keeps a well-behaved agent
    /// from asking for the rest.
    fn send_initialize(&mut self) -> Result<()> {
        self.request(
            Ask::Initialize,
            "initialize",
            &json!({
                "protocolVersion": super::VERSION,
                "clientCapabilities": {
                    "fs": { "readTextFile": true, "writeTextFile": false },
                    "terminal": false,
                },
                "clientInfo": {
                    "name": "obelus",
                    "version": env!("CARGO_PKG_VERSION"),
                },
            }),
        )?;
        Ok(())
    }
}

/// JSON-RPC's code for a method the other end does not have.
const NO_SUCH_METHOD: i64 = -32601;

/// JSON-RPC's code for a call that is missing something.
const INVALID_PARAMS: i64 = -32602;

/// What a `session/update` notification means, if it is one obelus shows.
///
/// The protocol has fifteen kinds and this shows three. The rest -- plans,
/// modes, usage, available commands -- are facts about the agent rather than
/// the conversation, and a conversation with them mixed in is a log.
fn update_in(params: &Value) -> Option<Update> {
    let update = params.get("update")?;
    let kind = update.get("sessionUpdate").and_then(Value::as_str)?;
    match kind {
        "agent_message_chunk" => Some(Update::Said(text_in(update.get("content")?)?)),
        "agent_thought_chunk" => Some(Update::Thought(text_in(update.get("content")?)?)),
        "current_mode_update" => Some(Update::Mode(
            update.get("currentModeId")?.as_str()?.to_string(),
        )),
        "available_commands_update" => Some(Update::Orders(orders_in(update))),
        "tool_call" | "tool_call_update" => Some(Update::Tool {
            id: update
                .get("toolCallId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            title: update
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            status: update
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("pending")
                .to_string(),
        }),
        _ => {
            tracing::debug!(kind, "an update obelus does not show");
            None
        }
    }
}

/// The words in a content block.
///
/// A block is text, an image, audio, or a link to something in the
/// workspace. Only the first is words; the others are named rather than
/// dropped, because a turn that silently loses a block reads as an agent
/// that said nothing.
fn text_in(content: &Value) -> Option<String> {
    match content.get("type").and_then(Value::as_str) {
        Some("text") => Some(content.get("text")?.as_str()?.to_string()),
        Some("image") => Some("(an image)".to_string()),
        Some("audio") => Some("(audio)".to_string()),
        Some("resource_link") => content
            .get("uri")
            .and_then(Value::as_str)
            .map(|uri| format!("({uri})")),
        Some("resource") => Some("(a resource)".to_string()),
        _ => None,
    }
}

/// The modes in a session's mode state.
fn modes_in(state: &Value) -> Vec<Mode> {
    state
        .get("availableModes")
        .and_then(Value::as_array)
        .map(|modes| {
            modes
                .iter()
                .filter_map(|mode| {
                    let id = mode.get("id")?.as_str()?.to_string();
                    let name = mode
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or(&id)
                        .to_string();
                    Some(Mode { id, name })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The commands in an update that lists them.
fn orders_in(update: &Value) -> Vec<Order> {
    update
        .get("availableCommands")
        .and_then(Value::as_array)
        .map(|orders| {
            orders
                .iter()
                .filter_map(|order| {
                    Some(Order {
                        name: order.get("name")?.as_str()?.to_string(),
                        description: order
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        // "All text that was typed after the command name
                        // is provided as input", says the protocol, and the
                        // hint is what the agent calls that text.
                        hint: order
                            .get("input")
                            .and_then(|input| input.get("hint"))
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A permission request, as the picker will show it.
fn permission_in(id: Value, params: &Value) -> Option<Permission> {
    let options: Vec<Choice> = params
        .get("options")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(|option| {
            Some(Choice {
                id: option.get("optionId")?.as_str()?.to_string(),
                name: option
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("(no name)")
                    .to_string(),
                kind: option
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            })
        })
        .collect();
    if options.is_empty() {
        tracing::warn!("a permission request with no options");
        return None;
    }
    let title = params
        .get("toolCall")
        .and_then(|call| call.get("title"))
        .and_then(Value::as_str)
        .unwrap_or("the agent wants to do something")
        .to_string();
    Some(Permission { id, title, options })
}

/// Reads messages, on its own thread.
fn spawn_reader(stdout: std::process::ChildStdout, sender: Sender<Event>) {
    let _ = std::thread::Builder::new()
        .name("obelus-acp".to_string())
        .spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match transport::read_message(&mut reader) {
                    Ok(Some(message)) => {
                        if sender.send(Event::Acp(message)).is_err() {
                            return;
                        }
                    }
                    Ok(None) => {
                        tracing::info!("the agent closed its output");
                        return;
                    }
                    Err(error) => {
                        tracing::warn!(%error, "reading from the agent");
                        return;
                    }
                }
            }
        });
}

/// Writes messages, on its own thread.
fn spawn_writer(id: String, stdin: std::process::ChildStdin) -> Sender<String> {
    let (sender, receiver) = mpsc::channel::<String>();
    let _ = std::thread::Builder::new()
        .name("obelus-acp-write".to_string())
        .spawn(move || {
            let mut writer = BufWriter::new(stdin);
            while let Ok(body) = receiver.recv() {
                if let Err(error) = transport::write_message(&mut writer, &body) {
                    tracing::warn!(%error, %id, "writing to the agent");
                    return;
                }
            }
        });
    sender
}

/// Puts the agent's own complaints in obelus's log.
fn spawn_logger(id: String, stderr: std::process::ChildStderr) {
    let _ = std::thread::Builder::new()
        .name("obelus-acp-log".to_string())
        .spawn(move || {
            use std::io::BufRead as _;
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                tracing::debug!(%id, "{line}");
            }
        });
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::Receiver;

    use serde_json::json;

    use super::*;

    /// A client whose messages can be read, and whose process does nothing.
    ///
    /// The process is real because [`Client`] holds one, and it is `true`
    /// because nothing here needs it to answer: what the agent says arrives
    /// through [`Client::on_message`], which is the seam this tests. The
    /// outgoing channel is swapped for one the test holds, which is why
    /// these are in this file -- the field is private, and exposing a way to
    /// redirect the wire would be an API for nobody.
    fn harness() -> (Client, Receiver<String>) {
        let (events, _) = mpsc::channel();
        let mut client = Client::start("fake", Path::new("true"), &[], Path::new("."), events)
            .expect("starting `true`");
        let (sender, wire) = mpsc::channel();
        client.outgoing = sender;
        (client, wire)
    }

    /// Everything the client has said since it was last asked.
    fn said(wire: &Receiver<String>) -> Vec<Value> {
        wire.try_iter()
            .map(|body| serde_json::from_str(&body).expect("what obelus wrote is json"))
            .collect()
    }

    /// The handshake, which is the whole of what obelus promises an agent.
    ///
    /// Asserted on the bytes because it is a promise: an agent reads this
    /// and decides what it may ask for, and `writeTextFile: true` here would
    /// be a code reader inviting an agent to write through it.
    #[test]
    fn the_handshake_says_what_obelus_will_and_will_not_do() {
        let (events, _) = mpsc::channel();
        // Its own client, because the harness swaps the wire out after the
        // handshake has already gone.
        let (sender, wire) = mpsc::channel();
        let mut client = Client::start("fake", Path::new("true"), &[], Path::new("."), events)
            .expect("starting `true`");
        client.outgoing = sender;
        client.send_initialize().expect("sending the handshake");

        let messages = said(&wire);
        assert_eq!(messages.len(), 1);
        let handshake = &messages[0];
        assert_eq!(handshake["method"], "initialize");
        assert_eq!(handshake["id"], INITIALIZE_ID);
        assert_eq!(
            handshake["params"]["protocolVersion"],
            super::super::VERSION
        );
        let capabilities = &handshake["params"]["clientCapabilities"];
        assert_eq!(capabilities["fs"]["readTextFile"], true);
        assert_eq!(
            capabilities["fs"]["writeTextFile"], false,
            "obelus offered to write files"
        );
        assert_eq!(capabilities["terminal"], false);
        assert_eq!(handshake["params"]["clientInfo"]["name"], "obelus");
    }

    /// The handshake's answer opens a session, and the session lets a prompt
    /// be sent.
    #[test]
    fn the_answer_opens_a_session_and_then_a_prompt_goes_out() {
        let (mut client, wire) = harness();

        let ready = client.on_message(&json!({
            "id": 0, "result": { "protocolVersion": 1, "agentInfo": { "name": "Fake", "version": "9" } }
        }));
        assert!(matches!(ready, Incoming::Ready), "got {ready:?}");
        assert_eq!(client.info(), Some("Fake 9"));
        let opening = said(&wire);
        assert_eq!(opening.len(), 1);
        assert_eq!(opening[0]["method"], "session/new");
        assert!(opening[0]["params"]["cwd"].is_string());
        let session_id = opening[0]["id"].as_i64().expect("an id");

        assert!(!client.is_started());
        let started =
            client.on_message(&json!({ "id": session_id, "result": { "sessionId": "s-1" } }));
        assert!(matches!(started, Incoming::Started), "got {started:?}");
        assert!(client.is_started());

        assert!(client.say("what is this file").expect("sending"));
        let prompts = said(&wire);
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0]["method"], "session/prompt");
        assert_eq!(prompts[0]["params"]["sessionId"], "s-1");
        assert_eq!(prompts[0]["params"]["prompt"][0]["type"], "text");
        assert_eq!(
            prompts[0]["params"]["prompt"][0]["text"],
            "what is this file"
        );
        assert!(client.is_thinking());

        // And the turn ends with the reason the agent gave.
        let turn = prompts[0]["id"].as_i64().expect("an id");
        let ended =
            client.on_message(&json!({ "id": turn, "result": { "stopReason": "end_turn" } }));
        assert!(matches!(ended, Incoming::Ended(reason) if reason == "end_turn"));
        assert!(!client.is_thinking());
    }

    /// A prompt typed while the agent is still starting is held, not lost.
    ///
    /// Which is the ordinary case for the first thing said: opening the view
    /// starts the process, and a reader types faster than node starts.
    #[test]
    fn a_prompt_typed_before_the_session_waits_for_it() {
        let (mut client, wire) = harness();
        assert!(!client.say("early").expect("holding"), "it claimed to send");
        assert!(said(&wire).is_empty(), "it sent something with no session");

        client.on_message(&json!({ "id": 0, "result": { "protocolVersion": 1 } }));
        let session_id = said(&wire)[0]["id"].as_i64().expect("an id");
        client.on_message(&json!({ "id": session_id, "result": { "sessionId": "s-2" } }));

        let sent = said(&wire);
        assert_eq!(sent.len(), 1, "the held prompt did not go: {sent:?}");
        assert_eq!(sent[0]["method"], "session/prompt");
        assert_eq!(sent[0]["params"]["prompt"][0]["text"], "early");
    }

    /// An agent speaking a protocol obelus does not speak is stopped rather
    /// than guessed at: every message after the handshake would be a
    /// different protocol's.
    #[test]
    fn a_version_obelus_does_not_speak_is_a_failure() {
        let (mut client, wire) = harness();
        let outcome = client.on_message(&json!({ "id": 0, "result": { "protocolVersion": 2 } }));
        assert!(
            matches!(&outcome, Incoming::Failed(what, why) if *what == "starting the agent" && why.contains("version 2")),
            "got {outcome:?}"
        );
        assert!(said(&wire).is_empty(), "it opened a session anyway");
    }

    /// The three kinds of update obelus shows, and the ones it does not.
    #[test]
    fn what_the_agent_says_comes_back_as_updates() {
        let (mut client, _wire) = harness();
        let mut update = |update: Value| {
            client_update(&mut client, json!({ "sessionId": "s", "update": update }))
        };

        assert_eq!(
            update(
                json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "hello" } })
            ),
            Some(Update::Said("hello".to_string()))
        );
        assert_eq!(
            update(
                json!({ "sessionUpdate": "agent_thought_chunk", "content": { "type": "text", "text": "hmm" } })
            ),
            Some(Update::Thought("hmm".to_string()))
        );
        assert_eq!(
            update(json!({
                "sessionUpdate": "tool_call", "toolCallId": "t1",
                "title": "Read src/main.rs", "status": "in_progress",
            })),
            Some(Update::Tool {
                id: "t1".to_string(),
                title: "Read src/main.rs".to_string(),
                status: "in_progress".to_string(),
            })
        );
        // A block that is not words is named rather than dropped: a turn
        // that quietly loses one reads as an agent that said nothing.
        assert_eq!(
            update(
                json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "image", "data": "..." } })
            ),
            Some(Update::Said("(an image)".to_string()))
        );
        // And the kinds obelus does not show are not the conversation.
        assert_eq!(
            update(json!({ "sessionUpdate": "plan", "entries": [] })),
            None
        );
        assert_eq!(
            update(json!({ "sessionUpdate": "current_mode_update", "currentModeId": "ask" })),
            None
        );
    }

    /// A permission request, with the options the reader will be shown.
    #[test]
    fn a_permission_request_comes_back_with_its_options() {
        let (mut client, _wire) = harness();
        let outcome = client.on_message(&json!({
            "id": 41,
            "method": "session/request_permission",
            "params": {
                "sessionId": "s",
                "toolCall": { "toolCallId": "t2", "title": "Run cargo test" },
                "options": [
                    { "optionId": "yes", "name": "Allow", "kind": "allow_once" },
                    { "optionId": "no", "name": "Reject", "kind": "reject_once" },
                ],
            },
        }));
        let Incoming::Permission(permission) = outcome else {
            panic!("got {outcome:?}");
        };
        assert_eq!(permission.id, json!(41));
        assert_eq!(permission.title, "Run cargo test");
        assert_eq!(
            permission.options,
            vec![
                Choice {
                    id: "yes".to_string(),
                    name: "Allow".to_string(),
                    kind: "allow_once".to_string()
                },
                Choice {
                    id: "no".to_string(),
                    name: "Reject".to_string(),
                    kind: "reject_once".to_string()
                },
            ]
        );
    }

    /// Writing is refused here rather than reported, and refused with the
    /// protocol's own "no such method": obelus said in the handshake that it
    /// does not write, so an agent asking is asking for something that is
    /// not there.
    #[test]
    fn a_write_is_refused_and_the_agent_is_told() {
        let (mut client, wire) = harness();
        let outcome = client.on_message(&json!({
            "id": 7,
            "method": "fs/write_text_file",
            "params": { "sessionId": "s", "path": "/tmp/x", "content": "no" },
        }));
        assert!(matches!(outcome, Incoming::Nothing), "got {outcome:?}");
        let replies = said(&wire);
        assert_eq!(replies.len(), 1, "the agent was left waiting");
        assert_eq!(replies[0]["id"], 7);
        assert_eq!(replies[0]["error"]["code"], NO_SUCH_METHOD);
    }

    /// Anything else obelus never declared is answered rather than ignored.
    ///
    /// A request with no answer is a turn that never ends: the agent waits
    /// for a terminal it will not get, and the conversation stops with no
    /// sign of why.
    #[test]
    fn a_method_obelus_does_not_have_is_still_answered() {
        let (mut client, wire) = harness();
        for method in ["terminal/create", "elicitation/create", "mcp/connect"] {
            let outcome = client.on_message(&json!({ "id": 3, "method": method, "params": {} }));
            assert!(matches!(outcome, Incoming::Nothing), "got {outcome:?}");
            let replies = said(&wire);
            assert_eq!(replies.len(), 1, "{method} was left unanswered");
            assert_eq!(replies[0]["error"]["code"], NO_SUCH_METHOD);
        }
        // A notification is not answered, because nothing is waiting on it.
        client.on_message(&json!({ "method": "some/notification", "params": {} }));
        assert!(said(&wire).is_empty());
    }

    /// An error names what obelus was doing, because the reader sees it in
    /// the transcript with nothing else around it.
    #[test]
    fn an_error_says_which_of_obeluss_requests_failed() {
        let (mut client, wire) = harness();
        client.on_message(&json!({ "id": 0, "result": { "protocolVersion": 1 } }));
        let session_id = said(&wire)[0]["id"].as_i64().expect("an id");
        client.on_message(&json!({ "id": session_id, "result": { "sessionId": "s" } }));
        client.say("hello").expect("sending");
        let turn = said(&wire)[0]["id"].as_i64().expect("an id");

        let outcome = client.on_message(&json!({
            "id": turn, "error": { "code": -32000, "message": "out of credit" }
        }));
        assert!(
            matches!(&outcome, Incoming::Failed(what, why) if *what == "the agent" && why == "out of credit"),
            "got {outcome:?}"
        );
        // And the turn is over, so the view stops saying it is thinking.
        assert!(!client.is_thinking());
    }

    /// Cancelling is only sent while there is something to cancel.
    #[test]
    fn nothing_is_cancelled_when_nothing_is_running() {
        let (mut client, wire) = harness();
        client.on_message(&json!({ "id": 0, "result": { "protocolVersion": 1 } }));
        let session_id = said(&wire)[0]["id"].as_i64().expect("an id");
        client.on_message(&json!({ "id": session_id, "result": { "sessionId": "s" } }));

        client.interrupt().expect("interrupting nothing");
        assert!(
            said(&wire).is_empty(),
            "it cancelled a turn that was not there"
        );

        client.say("think about it").expect("sending");
        let _ = said(&wire);
        client.interrupt().expect("interrupting");
        let sent = said(&wire);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["method"], "session/cancel");
        assert_eq!(sent[0]["params"]["sessionId"], "s");
        // A notification, so no id: the turn ends with the agent's own
        // `cancelled` stop reason rather than with an answer to this.
        assert!(sent[0].get("id").is_none());
    }

    /// Feeds one `session/update` in and says what it turned into.
    fn client_update(client: &mut Client, params: Value) -> Option<Update> {
        match client.on_message(&json!({ "method": "session/update", "params": params })) {
            Incoming::Update(update) => Some(update),
            Incoming::Nothing => None,
            other => panic!("got {other:?}"),
        }
    }
}
