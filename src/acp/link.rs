//! The agent, on the other end of the protocol's own crate.
//!
//! `agent-client-protocol` is the protocol's reference implementation: it
//! spawns the agent, frames the messages, numbers the requests, and gives
//! every method of the protocol a type whose field names the compiler
//! checks. What it is built around is `async`, and obelus's main loop is a
//! thread blocked on a channel -- so this is the join between them.
//!
//! One thread runs the connection. It holds the whole conversation: the
//! handshake, the session, and a loop over the [`Ask`]s obelus sends it. In
//! the other direction everything becomes an [`Event`] on the loop's own
//! channel, like the keyboard, the file walk and the language servers.
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
            AvailableCommand, CancelNotification, ClientCapabilities, ContentBlock,
            FileSystemCapabilities, Implementation, InitializeRequest, NewSessionRequest,
            PermissionOptionId, PromptRequest, ReadTextFileRequest, ReadTextFileResponse,
            RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
            SelectedPermissionOutcome, SessionNotification, SessionUpdate, SetSessionModeRequest,
            TextContent, WriteTextFileRequest,
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
    /// Say this.
    Say(String),
    /// Stop what you are doing.
    Interrupt,
    /// Work this way from now on.
    Mode(String),
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
    /// There is a session, and these are the ways of working it offers.
    Started {
        /// Every mode it has.
        modes: Vec<Mode>,
        /// Which one is on.
        current: Option<String>,
    },
    /// Something to show.
    Update(Update),
    /// The turn ended, for this reason.
    Ended(String),
    /// Something did not work: what obelus was doing, and what it said.
    Failed(&'static str, String),
    /// The agent is asking to be allowed something.
    Permission {
        /// What it wants to do.
        title: String,
        /// What obelus may answer.
        options: Vec<Choice>,
        /// Which option, or nothing for "not answered".
        answer: Answer<Option<String>>,
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
    /// It is using a tool: what it calls the call, and where it has got to.
    Tool {
        /// Its own id for the call, so a later update replaces the row
        /// rather than adding one.
        id: String,
        /// What it calls it.
        title: String,
        /// `pending`, `in_progress`, `completed` or `failed`.
        status: String,
    },
    /// The way of working changed, which the agent can do on its own.
    Mode(String),
    /// The commands it takes, sent once the session is ready and again
    /// whenever they change.
    Orders(Vec<Order>),
}

/// One way of working the agent offers.
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
    /// What it says about whatever is typed after the name.
    pub hint: Option<String>,
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
                Ok(runtime) => runtime.block_on(talk(config, root, told.clone(), taken)),
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

/// The whole conversation, from the handshake to the end of the stream.
///
/// Returns why it ended, or `None` because it ended tidily.
async fn talk(
    config: AcpAgentConfig,
    root: PathBuf,
    events: Sender<Event>,
    mut asks: mpsc::UnboundedReceiver<Ask>,
) -> Option<String> {
    // The transport *is* the agent: connecting spawns the process and
    // frames the messages over its stdin and stdout.
    let agent = agent_client_protocol::AcpAgent::new(config);

    let updates = events.clone();
    let asking = events.clone();
    let reading = events.clone();

    let outcome = Client
        .builder()
        .on_receive_notification(
            async move |notification: SessionNotification, _connection| {
                for update in read_update(notification.update) {
                    let _ = updates.send(Event::Acp(Incoming::Update(update)));
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
                    title: title_of(&request),
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
            async move |_request: WriteTextFileRequest, responder, _connection| {
                // Refused here rather than reported: the answer does not
                // depend on anything the view knows. obelus does not write
                // files, and said so in the handshake.
                tracing::info!("the agent tried to write a file");
                responder.respond_with_error(refusal("obelus is a reader and does not write files"))
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

                let opened = connection
                    .send_request(NewSessionRequest::new(root))
                    .block_task()
                    .await?;
                let session = opened.session_id.clone();
                let (modes, current) = match opened.modes {
                    Some(state) => (
                        state
                            .available_modes
                            .iter()
                            .map(|mode| Mode {
                                id: mode.id.0.to_string(),
                                name: mode.name.clone(),
                            })
                            .collect(),
                        Some(state.current_mode_id.0.to_string()),
                    ),
                    None => (Vec::new(), None),
                };
                let _ = events.send(Event::Acp(Incoming::Started { modes, current }));

                // Whether the turn in flight has been given up on.
                //
                // Set by an interruption and cleared by the next prompt,
                // and read by the answer's callback: an agent that never
                // saw the cancellation answers the prompt anyway, and by
                // then the reader has been told the turn is over.
                let stopped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

                while let Some(ask) = asks.next().await {
                    match ask {
                        // The answer is taken in a callback rather than
                        // awaited, so the loop goes straight back to
                        // reading asks: an interruption typed while the
                        // agent is thinking has to reach it.
                        Ask::Say(words) => {
                            let told = events.clone();
                            let given_up = std::sync::Arc::clone(&stopped);
                            stopped.store(false, std::sync::atomic::Ordering::Relaxed);
                            connection
                                .send_request(PromptRequest::new(
                                    session.clone(),
                                    vec![ContentBlock::Text(TextContent::new(words))],
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
                                        Ok(answer) => Incoming::Ended(
                                            format!("{:?}", answer.stop_reason).to_lowercase(),
                                        ),
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
                        Ask::Interrupt => {
                            stopped.store(true, std::sync::atomic::Ordering::Relaxed);
                            connection
                                .send_notification(CancelNotification::new(session.clone()))?;
                            let _ =
                                events.send(Event::Acp(Incoming::Ended("cancelled".to_string())));
                        }
                        Ask::Mode(mode) => {
                            let told = events.clone();
                            connection
                                .send_request(SetSessionModeRequest::new(
                                    session.clone(),
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

    outcome.err().map(|error| error.to_string())
}

/// What obelus tells an agent about itself.
///
/// It reads files out and does not write them, and it has no terminal to
/// offer. Declaring the truth here is what keeps a well-behaved agent from
/// asking for the rest.
fn handshake() -> InitializeRequest {
    InitializeRequest::new(ProtocolVersion::V1)
        .client_capabilities(
            ClientCapabilities::new()
                .fs(FileSystemCapabilities::new()
                    .read_text_file(true)
                    .write_text_file(false))
                .terminal(false),
        )
        .client_info(Implementation::new("obelus", env!("CARGO_PKG_VERSION")))
}

/// What obelus says when it will not do something.
fn refusal(why: &str) -> agent_client_protocol::Error {
    agent_client_protocol::Error::method_not_found().data(serde_json::json!(why))
}

/// What a permission request is about.
fn title_of(request: &RequestPermissionRequest) -> String {
    request
        .tool_call
        .fields
        .title
        .clone()
        .unwrap_or_else(|| "the agent wants to do something".to_string())
}

/// What a `session/update` means, if it is one obelus shows.
///
/// The protocol has fifteen kinds and this shows five. The rest -- plans,
/// usage, compaction -- are facts about the agent rather than about the
/// conversation, and a conversation with them in it is a log.
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
            id: call.tool_call_id.0.to_string(),
            title: call.title.clone(),
            status: format!("{:?}", call.status).to_lowercase(),
        }],
        SessionUpdate::ToolCallUpdate(call) => vec![Update::Tool {
            id: call.tool_call_id.0.to_string(),
            title: call.fields.title.clone().unwrap_or_default(),
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
        other => {
            tracing::debug!(?other, "an update obelus does not show");
            Vec::new()
        }
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
