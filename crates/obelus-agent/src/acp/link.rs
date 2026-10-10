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
//! with a [`oneshot`] to answer through. That is also why [`Event`] is not
//! `Clone`.
//!
//! **What waits on the reader does not wait in the handler.** The crate
//! hands each message to a handler and does not read the next until the
//! handler returns, so a handler that waited for a keystroke held the whole
//! connection -- and one agent is behind every conversation in the window,
//! so a card up in one held every other one still: no answer, no tool call,
//! no count of what it has used, until the card was answered. What waits
//! for the reader, or for a command that may take minutes, is waited for on
//! a task of its own (`answer_when`), which is what the crate says to do.
//! The question still goes to the main loop from inside the handler, so
//! what the agent said arrives in the order it said it; only the answer
//! goes back later, and it carries the request's own number. So one
//! conversation can have two questions waiting at once now, and the main
//! loop puts them to the reader one at a time. And the agent taking a
//! question back is a message that can be read, which is what takes its
//! card down.
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
//! they return, because what the agent asked for is that they be directed
//! there -- and it says the far end happened with `elicitation/complete`,
//! which is a notification because nothing is owed back.
//!
//! **A sign-in holds what was asked; it does not end the connection.** An
//! agent nobody has signed in to answers `session/new` with `auth_required`,
//! and that answer used to end everything, because an error opening a
//! conversation did. Now the request waits -- and every request for a
//! conversation after it, because they are answered in the order they were
//! made -- while the reader picks one of the ways in the handshake offered,
//! and goes again when they are in. zed's shape: nothing is signed in to
//! until the agent says it needs it, and the agent is not started again
//! afterwards. Most ways in are a program to run (claude-agent-acp offers
//! *only* those, and only to a client that says it can run one), which
//! Obelus runs in a terminal of its own; `authenticate` is for the rest.
//! A reader who will not sign in ends the connection the way it ended
//! before, and the next thing they say starts it again.

use std::path::PathBuf;

use agent_client_protocol::{
    AcpAgentConfig, Agent, Client, ConnectionTo, Handled, JsonRpcResponse, Responder,
    UntypedMessage,
    schema::{
        ProtocolVersion,
        v1::{
            AuthCapabilities, AuthMethod, AuthenticateRequest, BooleanConfigOptionCapabilities,
            CancelNotification, ClientCapabilities, ClientSessionCapabilities, CloseSessionRequest,
            CompleteElicitationNotification, ContentBlock, CreateElicitationRequest,
            CreateElicitationResponse, CreateTerminalRequest, CreateTerminalResponse,
            DeleteSessionRequest, ElicitationAcceptAction, ElicitationAction,
            ElicitationCapabilities, ElicitationContentValue, ElicitationFormCapabilities,
            ElicitationMode, ElicitationScope, ElicitationUrlCapabilities, FileSystemCapabilities,
            ImageContent, Implementation, InitializeRequest, KillTerminalRequest,
            KillTerminalResponse, LoadSessionRequest, McpCapabilities, McpServer, McpServerHttp,
            McpServerSse, NewSessionRequest, PermissionOptionId, PromptRequest,
            ReadTextFileRequest, ReadTextFileResponse, ReleaseTerminalRequest,
            ReleaseTerminalResponse, RequestPermissionOutcome, RequestPermissionRequest,
            RequestPermissionResponse, ResumeSessionRequest, SelectedPermissionOutcome,
            SessionConfigId, SessionConfigOptionValue, SessionConfigOptionsCapabilities, SessionId,
            SessionNotification, SetSessionConfigOptionRequest, SetSessionModeRequest,
            TerminalExitStatus, TerminalId, TerminalOutputRequest, TerminalOutputResponse,
            TextContent, WaitForTerminalExitRequest, WaitForTerminalExitResponse,
            WriteTextFileRequest, WriteTextFileResponse,
        },
    },
};
use futures::{
    StreamExt as _,
    channel::{mpsc, oneshot},
};
use obelus_sink::Sink;

use super::read::{
    call_of, fields_of, mode_setting, offers_in, read_update, reason_of, said_as, setting_of,
    somewhere_to_go,
};
pub use super::said::{
    ASKING_WHAT_IT_OFFERS, Answer, Ask, BACKGROUNDED, Call, Carries, Category, Change, Choice,
    Chosen, Cost, Field, How, Incoming, Kind, Login, MODE, Order, Picture, Place, Question, Reply,
    Said, Setting, Step, Takes, Turn, Update, Usage, Value,
};
use crate::Event;

/// One piece of what the reader said, as the protocol wants it.
///
/// Base64 here and nowhere earlier: this is the one place that knows the
/// wire takes a string, and a picture that crossed three layers already
/// encoded would be a megabyte of text being copied about for nothing. A
/// picture too large to send is made smaller here for the same reason --
/// which is work, so this is called on a thread of its own.
fn block_of(said: Said) -> ContentBlock {
    use base64::Engine as _;
    match said {
        Said::Words(words) => ContentBlock::Text(TextContent::new(words)),
        Said::Picture(picture) => {
            let (mime, bytes) = super::picture::fitted(picture.mime, picture.bytes);
            ContentBlock::Image(ImageContent::new(
                base64::engine::general_purpose::STANDARD.encode(&bytes),
                mime,
            ))
        }
    }
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

/// Starts an agent on a thread of its own, and says how to talk to it.
///
/// Best effort, like every other producer: a thread that will not start, or
/// an agent that will not run, becomes a `Gone` with the reason in it
/// rather than a failure to open the view.
///
/// And how to stop it, which is dropping what comes back beside the asks.
/// Closing the asks ends a connection that is listening for them, and one
/// still waiting on the agent -- for its handshake, which an agent that
/// hangs on the way up never gives -- is not listening. So the whole of the
/// connection is raced against the stop, and losing drops it, which is
/// what kills the process.
pub fn start(
    command: &std::path::Path,
    arguments: &[String],
    environment: &[(String, String)],
    root: &std::path::Path,
    events: impl Sink<Event> + Clone,
) -> (
    mpsc::UnboundedSender<Ask>,
    futures::channel::oneshot::Sender<()>,
) {
    let (asks, taken) = mpsc::unbounded();
    let (stop, stopped) = futures::channel::oneshot::channel::<()>();
    // Not always the file that was installed: what npm writes on Windows is
    // a `.cmd`, which is started by being handed to the command processor
    // rather than by being run. [`obelus_program::as_started_here`] is the
    // one place that knows the difference.
    //
    // Kept as it was named as well, for a way in that is this command with
    // more on the end: the terminal it is run in asks the same question of
    // it, and asking twice would wrap a shim in two command processors.
    let started = (command.to_path_buf(), arguments.to_vec());
    let (program, arguments) = obelus_program::as_started_here(command, arguments);
    // And where the machine's pool of build jobs is, so that everything
    // the agent runs in its own shell builds inside it -- the agent starts
    // its commands, and this is the one moment Obelus has a say in them.
    //
    // And what the reader said it is to be started with, after the pool so
    // that a reader who names one of those has the last word on it.
    let config = AcpAgentConfig::new(&program)
        .args(arguments.iter().cloned())
        .envs(obelus_jobs::lent())
        .envs(environment.iter().cloned());
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
        let talking = std::pin::pin!(talk(config, started, root, told.clone(), taken));
        let reason = match futures::future::select(talking, stopped).await {
            futures::future::Either::Left((reason, _)) => reason,
            futures::future::Either::Right(_) => None,
        };
        let _ = told.send(Event::Acp(Incoming::Gone(reason)));
    });
    (asks, stop)
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
    (command, arguments): (PathBuf, Vec<String>),
    root: PathBuf,
    events: impl Sink<Event> + Clone,
    mut asks: mpsc::UnboundedReceiver<Ask>,
) -> Option<String> {
    // The transport *is* the agent: connecting spawns the process and
    // frames the messages over its stdin and stdout.
    // `AcpAgent::with_debug` hands over every line in both directions,
    // which is how the traffic in this file was read while it was written.
    let agent = agent_client_protocol::AcpAgent::new(config);

    let updates = events.clone();
    // Which dialect of background work the agent speaks, settled by the
    // handshake and read by every update after it. Shared, because the
    // handler is built before the handshake has happened and the asks are
    // read after it.
    let speaking = std::sync::Arc::new(std::sync::Mutex::new(Speaking::default()));
    let hearing = speaking.clone();
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
        // Untyped, and read here. Typed, an update of a kind the protocol's
        // crate does not know -- a dialect's, or one a later protocol adds
        // -- fails to parse before Obelus sees it, and the crate drops it
        // with a warning: the connection lives, but a dialect's updates
        // could never be read at all. Everything that is not an update goes
        // on to the handlers after this one.
        .on_receive_notification(
            async move |notification: UntypedMessage, connection| {
                if notification.method != UPDATE {
                    return Ok(Handled::No {
                        message: (notification, connection),
                        retry: false,
                    });
                }
                hear_update(notification.params, &hearing, &updates);
                Ok(Handled::Yes)
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
            async move |request: RequestPermissionRequest, responder, connection| {
                said_about(&request);
                // The reader's to answer, so the question goes to the main
                // loop and the answer is waited for off the connection's
                // loop. An answer that never comes -- the view closed,
                // Obelus quit -- is the protocol's "cancelled", which is
                // what an agent needs to hear to stop waiting.
                let session = request.session_id.clone();
                let (answer, answered) = oneshot::channel();
                let question = Question::Permission {
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
                let question = Incoming::Asked {
                    session: request.session_id.clone(),
                    question,
                };
                if asking.send(Event::Acp(question)).is_err() {
                    return responder.respond(RequestPermissionResponse::new(
                        RequestPermissionOutcome::Cancelled,
                    ));
                }
                let withdrawn = asking.clone();
                answer_when(
                    &connection,
                    responder,
                    answered,
                    move || {
                        let _ = withdrawn.send(Event::Acp(Incoming::Withdrawn { session }));
                    },
                    |answered| {
                        let outcome = match answered {
                            Ok(Some(option)) => RequestPermissionOutcome::Selected(
                                SelectedPermissionOutcome::new(PermissionOptionId::new(option)),
                            ),
                            Ok(None) | Err(_) => RequestPermissionOutcome::Cancelled,
                        };
                        Ok(RequestPermissionResponse::new(outcome))
                    },
                )
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
            async move |request: CreateElicitationRequest, responder, connection| {
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
                    let question = Question::Open {
                        message: request.message.clone(),
                        url,
                        id: mode.elicitation_id.0.to_string(),
                        answer,
                    };
                    let question = Incoming::Asked {
                        session: session.clone(),
                        question,
                    };
                    if elicited.send(Event::Acp(question)).is_err() {
                        return responder
                            .respond(CreateElicitationResponse::new(ElicitationAction::Cancel));
                    }
                    // Accepted the moment the reader is sent there, not
                    // when they come back: what the agent asked for is that
                    // they be directed to the URL, and it watches the far
                    // end itself. Waiting for the return would hold a turn
                    // open across a sign-in nobody can time.
                    let withdrawn = elicited.clone();
                    return answer_when(
                        &connection,
                        responder,
                        answered,
                        move || {
                            let _ = withdrawn.send(Event::Acp(Incoming::Withdrawn { session }));
                        },
                        |answered| {
                            let action = match answered {
                                Ok(true) => {
                                    ElicitationAction::Accept(ElicitationAcceptAction::new())
                                }
                                Ok(false) => ElicitationAction::Decline,
                                // The question going away without an answer,
                                // which is what the card being taken down
                                // means.
                                Err(_) => ElicitationAction::Cancel,
                            };
                            Ok(CreateElicitationResponse::new(action))
                        },
                    );
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
                let question = Question::Ask {
                    message: request.message.clone(),
                    fields,
                    answer,
                };
                let question = Incoming::Asked {
                    session: session.clone(),
                    question,
                };
                if elicited.send(Event::Acp(question)).is_err() {
                    return responder
                        .respond(CreateElicitationResponse::new(ElicitationAction::Cancel));
                }
                // Nothing back is a refusal; the view going away with the
                // question still up is a cancellation. Either way the agent
                // hears something, because one that hears nothing waits for
                // ever.
                let withdrawn = elicited.clone();
                answer_when(
                    &connection,
                    responder,
                    answered,
                    move || {
                        let _ = withdrawn.send(Event::Acp(Incoming::Withdrawn { session }));
                    },
                    |answered| {
                        let action = match answered {
                            Ok(Some(given)) => ElicitationAction::Accept(
                                ElicitationAcceptAction::new().content(content_of(given)),
                            ),
                            Ok(None) => ElicitationAction::Decline,
                            Err(_) => ElicitationAction::Cancel,
                        };
                        Ok(CreateElicitationResponse::new(action))
                    },
                )
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
            async move |request: WaitForTerminalExitRequest, responder, connection| {
                // Answered when the command ends, which may be minutes --
                // off the connection's loop, because every other
                // conversation on it would wait those minutes too.
                let (answer, answered) = oneshot::channel();
                let question = Incoming::Waited {
                    id: request.terminal_id.0.to_string(),
                    answer,
                };
                if waiting.send(Event::Acp(question)).is_err() {
                    return responder.respond_with_error(refusal("Obelus is not listening"));
                }
                // Taken back, it is the command's end that nobody is
                // waiting for, and the command is still the reader's to see.
                answer_when(
                    &connection,
                    responder,
                    answered,
                    || {},
                    |answered| match answered {
                        Ok(Some(ended)) => Ok(WaitForTerminalExitResponse::new(exit_status(ended))),
                        Ok(None) | Err(_) => Err(refusal("Obelus is not running that")),
                    },
                )
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
                let logins = logins_of(&ready.auth_methods, &command, &arguments);
                // Written down for the same reason: "why does it offer no
                // way to sign in" is answered by what it said here.
                tracing::info!(?logins, "the ways in it offers");
                let _ = events.send(Event::Acp(Incoming::Ready {
                    named,
                    carries,
                    logins,
                }));
                // Which dialect of background work it answered in, if any.
                // From what it said here and nothing else: not its name, not
                // its version.
                let dialect = super::tasks::Dialect::chosen(ready.meta.as_ref());
                tracing::info!(?dialect, "what the agent says of background work");
                lock(&speaking).dialect = dialect;
                let heard = dialect != super::tasks::Dialect::None;
                let _ = events.send(Event::Acp(Incoming::Tasks {
                    heard,
                    stoppable: heard,
                }));

                // Which way the tools can be handed over, decided once from
                // what the agent said it takes rather than guessed afresh
                // per session: the answer cannot change while the agent
                // runs, and asking twice would be two answers to keep alike.
                // Where they are handed over to is each conversation's own,
                // and comes with the ask.
                let can = ready.agent_capabilities.mcp_capabilities.clone();
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
                // The requests for a conversation that are waiting for the
                // reader to sign in, oldest first, and the ones being asked
                // again now that they have. Every request for one waits
                // behind the first that is held, whatever it would have been
                // answered: they are answered in the order they were made,
                // which is all the other end has to tell them apart by.
                let mut held: std::collections::VecDeque<Ask> = std::collections::VecDeque::new();
                let mut retried: std::collections::VecDeque<Ask> = std::collections::VecDeque::new();
                loop {
                    let ask = match retried.pop_front() {
                        Some(ask) => ask,
                        None => match asks.next().await {
                            Some(ask) => ask,
                            None => break,
                        },
                    };
                    match ask {
                        ask @ (Ask::Open { .. } | Ask::Reopen { .. }) if !held.is_empty() => {
                            held.push_back(ask);
                            // Said for this one too, or the conversation it
                            // is for sits opening with nothing on it saying
                            // why.
                            let _ = events.send(Event::Acp(Incoming::SignIn {
                                session: None,
                                asking: None,
                                behind: held.len() - 1,
                                why: None,
                            }));
                        }
                        Ask::Open { tools } => {
                            let offered = offering(tools.as_deref(), &can);
                            match open_session(&connection, &root, offered.as_ref(), &events).await
                            {
                                Ok(_) => {}
                                Err(error) if wants_signing_in(&error) => {
                                    held.push_back(Ask::Open { tools });
                                    let _ = events.send(Event::Acp(Incoming::SignIn {
                                        session: None,
                                        asking: None,
                                        behind: 0,
                                        why: why_signing_in(&error),
                                    }));
                                }
                                Err(error) => return Err(error),
                            }
                        }
                        // A way in the agent does itself. Whatever was
                        // waiting is asked again once it says it has, and
                        // an answer that it has not is the same question
                        // put again, with what it said.
                        Ask::SignIn { method } => {
                            match connection
                                .send_request(AuthenticateRequest::new(method))
                                .block_task()
                                .await
                            {
                                Ok(_) => {
                                    retried.extend(held.drain(..));
                                    let _ = events.send(Event::Acp(Incoming::SignedIn));
                                }
                                Err(error) => {
                                    let _ = events.send(Event::Acp(Incoming::SignIn {
                                        session: None,
                                        asking: None,
                                        behind: 0,
                                        why: Some(ended_because(&error)),
                                    }));
                                }
                            }
                        }
                        Ask::SignedIn => retried.extend(held.drain(..)),
                        // Nothing that was waiting will ever be answered,
                        // and a request that is never answered is a
                        // conversation that says it is opening for ever. So
                        // the connection ends the way it did before anybody
                        // could sign in, with what the agent said, and the
                        // next thing the reader says starts it again.
                        Ask::GiveUp => {
                            if !held.is_empty() {
                                return Err(agent_client_protocol::Error::auth_required());
                            }
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
                        Ask::Reopen { session, tools } => {
                            let offered = offering(tools.as_deref(), &can);
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
                            if let Some(Err(error)) = &taken
                                && wants_signing_in(error)
                            {
                                let why = why_signing_in(error);
                                held.push_back(Ask::Reopen { session, tools });
                                let _ = events.send(Event::Acp(Incoming::SignIn {
                                    session: None,
                                    asking: None,
                                    behind: 0,
                                    why,
                                }));
                                continue;
                            }
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
                                    // And the new one may want a sign-in
                                    // as much as a first one does: what is
                                    // held then is the opening, which is all
                                    // that is left of this request.
                                    match open_session(
                                        &connection,
                                        &root,
                                        offered.as_ref(),
                                        &events,
                                    )
                                    .await
                                    {
                                        Ok(_) => {}
                                        Err(error) if wants_signing_in(&error) => {
                                            held.push_back(Ask::Open { tools });
                                            let _ = events.send(Event::Acp(Incoming::SignIn {
                                                session: None,
                                                asking: None,
                                                behind: 0,
                                                why: why_signing_in(&error),
                                            }));
                                        }
                                        Err(error) => return Err(error),
                                    }
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
                            // On a thread of the runtime's blocking pool, and
                            // waited for here: a picture too large to send is
                            // decoded and written again, which is work rather
                            // than waiting -- and waiting for it keeps the
                            // order, so an interruption asked meanwhile goes
                            // after the prompt it interrupts.
                            let blocks = tokio::task::spawn_blocking(move || {
                                said.into_iter().map(block_of).collect::<Vec<_>>()
                            })
                            .await
                            .map_err(|error| {
                                agent_client_protocol::Error::internal_error()
                                    .data(serde_json::json!(error.to_string()))
                            })?;
                            connection
                                .send_request(PromptRequest::new(
                                    session,
                                    opening
                                        .into_iter()
                                        .map(|words| ContentBlock::Text(TextContent::new(words)))
                                        .chain(blocks)
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
                                    // A turn that needed a sign-in has ended
                                    // all the same, and says so; the card
                                    // that asks for one comes after it.
                                    let signing_in = match &asked {
                                        Err(error) if wants_signing_in(error) => {
                                            Some(why_signing_in(error))
                                        }
                                        _ => None,
                                    };
                                    let _ = told.send(Event::Acp(Incoming::Ended {
                                        session: whose.clone(),
                                        turn,
                                        why: asked
                                            .map(|answer| said_as(&answer.stop_reason))
                                            .map_err(|error| error.to_string()),
                                    }));
                                    if let Some(why) = signing_in {
                                        let _ = told.send(Event::Acp(Incoming::SignIn {
                                            session: Some(whose),
                                            asking: None,
                                            behind: 0,
                                            why,
                                        }));
                                    }
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
                        Ask::StopTask { session, id } => {
                            let dialect = lock(&speaking).dialect;
                            let told = events.clone();
                            let still = speaking.clone();
                            let Some((method, params)) = dialect.stop(&session.0, &id) else {
                                let _ = told.send(Event::Acp(Incoming::NotStopped { session, id }));
                                continue;
                            };
                            connection
                                .send_request(UntypedMessage::new(method, params)?)
                                .on_receiving_result(move |answer| {
                                    // Stopped is said by the update that
                                    // follows; what is said here is only
                                    // what did not happen.
                                    let refused = match answer {
                                        // Anything but a plain yes: an
                                        // answer Obelus cannot read says
                                        // nothing was stopped as surely as
                                        // a no does, and a row left saying
                                        // it is stopping would say so for
                                        // ever.
                                        Ok(answer) => dialect.stopped(&answer) != Some(true),
                                        // The agent has no such method after
                                        // all: nothing on this connection
                                        // can be stopped, and the key that
                                        // asks goes.
                                        Err(error)
                                            if error.code
                                                == agent_client_protocol::ErrorCode::MethodNotFound =>
                                        {
                                            tracing::info!(
                                                "the agent cannot stop background work after all"
                                            );
                                            // Only while the dialect is still
                                            // spoken. Given up on while this
                                            // was in flight, saying it is
                                            // heard would bring back every
                                            // list the giving up took away.
                                            //
                                            // Said with the lock held, as the
                                            // giving up is: whichever takes it
                                            // first is also first on the loop's
                                            // channel, so the two cannot cross
                                            // whatever order the connection
                                            // runs them in.
                                            let speaking = lock(&still);
                                            if speaking.dialect != super::tasks::Dialect::None {
                                                let _ = told.send(Event::Acp(Incoming::Tasks {
                                                    heard: true,
                                                    stoppable: false,
                                                }));
                                            }
                                            drop(speaking);
                                            true
                                        }
                                        Err(error) => {
                                            let _ = told.send(Event::Acp(Incoming::Failed(
                                                "Stopping a background task",
                                                error.to_string(),
                                            )));
                                            true
                                        }
                                    };
                                    if refused {
                                        let _ = told
                                            .send(Event::Acp(Incoming::NotStopped { session, id }));
                                    }
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
                )
                // A sign-in that is a program to run, which Obelus runs in a
                // terminal of its own. Said twice, because the protocol said
                // it in `_meta` before it said it here and an agent written
                // then still asks there: Copilot's one way in is a
                // `_meta` program, and claude-agent-acp offers *no* way in
                // to a client that says neither.
                .auth(AuthCapabilities::new().terminal(true))
                .meta(client_meta()),
        )
        .client_info(Implementation::new("obelus", env!("CARGO_PKG_VERSION")))
}

/// What goes in the client capabilities' `_meta`: the older word for a
/// sign-in that is a program, and every dialect of background work Obelus
/// reads.
fn client_meta() -> agent_client_protocol::schema::v1::Meta {
    let mut meta =
        serde_json::Map::from_iter([(TERMINAL_AUTH.to_string(), serde_json::Value::Bool(true))]);
    super::tasks::Dialect::declare(&mut meta);
    meta
}

/// The method every update about a conversation arrives on.
const UPDATE: &str = "session/update";

/// What the connection has settled about background work: which dialect the
/// agent speaks, and how much of it has been unreadable.
#[derive(Debug, Default)]
struct Speaking {
    /// The dialect, once the handshake has said.
    dialect: super::tasks::Dialect,
    /// How many of its updates could not be read -- see
    /// [`super::tasks::UNREADABLE`].
    unreadable: u32,
}

/// The connection's word on background work, whatever became of the last
/// thread that held it.
///
/// A panic while holding it leaves a value that is still whole -- two plain
/// fields, set in one statement each -- so it is taken rather than refused.
fn lock(speaking: &std::sync::Mutex<Speaking>) -> std::sync::MutexGuard<'_, Speaking> {
    speaking
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Reads one `session/update` and sends on what it means.
///
/// The dialect of background work first, because its updates are kinds the
/// protocol does not have; then the protocol's own. An update neither can
/// read is left unread, which is what the protocol asks of a client.
fn hear_update(
    params: serde_json::Value,
    speaking: &std::sync::Mutex<Speaking>,
    events: &impl Sink<Event>,
) {
    let dialect = lock(speaking).dialect;
    let session = params
        .get("sessionId")
        .and_then(serde_json::Value::as_str)
        .map(|id| SessionId::new(id.to_string()));
    if let Some(update) = params.get("update")
        && let Some(read) = dialect.read(update)
    {
        // One of the dialect's that names no conversation is as unreadable
        // as one with no id: it is about work nobody can be shown.
        let read = match session {
            Some(session) => read.map(|news| (session, news)),
            None => Err("no sessionId".to_string()),
        };
        match read {
            Ok((session, news)) => {
                let _ = events.send(Event::Acp(Incoming::Update {
                    session,
                    update: Update::Task(news),
                }));
            }
            Err(why) => {
                let mut speaking = lock(speaking);
                speaking.unreadable += 1;
                tracing::warn!(
                    why,
                    unreadable = speaking.unreadable,
                    "an update about background work Obelus could not read"
                );
                // Given up on once it is plainly not the dialect Obelus was
                // written against any more -- and said, so that every list
                // built from it goes rather than going on saying work runs.
                if speaking.unreadable >= super::tasks::UNREADABLE
                    && speaking.dialect != super::tasks::Dialect::None
                {
                    speaking.dialect = super::tasks::Dialect::None;
                    tracing::warn!("background work is no longer heard on this connection");
                    let _ = events.send(Event::Acp(Incoming::Tasks {
                        heard: false,
                        stoppable: false,
                    }));
                }
            }
        }
        return;
    }
    match serde_json::from_value::<SessionNotification>(params) {
        Ok(notification) => {
            // Which conversation it is about, which the protocol has said
            // all along.
            let session = notification.session_id;
            for update in read_update(notification.update, dialect) {
                let _ = events.send(Event::Acp(Incoming::Update {
                    session: session.clone(),
                    update,
                }));
            }
        }
        Err(error) => tracing::debug!(%error, "an update Obelus cannot read, left unread"),
    }
}

/// Where an agent written before the protocol had a word for it says a
/// sign-in is a program: in a capability's `_meta`, and in a way in's.
const TERMINAL_AUTH: &str = "terminal-auth";

/// The ways in an agent offers, as Obelus can take them.
///
/// A program the agent names is run as it named it. The newer kind names
/// only what to add to the agent's own command line -- the agent knows how
/// it was started better than it knows how to say so -- so it is the
/// command Obelus started it with and those after it.
fn logins_of(
    methods: &[AuthMethod],
    command: &std::path::Path,
    arguments: &[String],
) -> Vec<Login> {
    methods
        .iter()
        .map(|method| {
            let how = match method {
                AuthMethod::Terminal(terminal) => How::Run {
                    program: command.to_path_buf(),
                    arguments: arguments.iter().chain(&terminal.args).cloned().collect(),
                    env: terminal
                        .env
                        .iter()
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect(),
                },
                _ => method
                    .meta()
                    .and_then(|meta| meta.get(TERMINAL_AUTH))
                    .and_then(run_of)
                    .unwrap_or(How::Asked),
            };
            Login {
                id: method.id().to_string(),
                name: method.name().to_string(),
                about: method
                    .description()
                    .map(str::trim)
                    .filter(|about| !about.is_empty())
                    .map(str::to_string),
                how,
            }
        })
        .collect()
}

/// A program a way in names in its `_meta`: `{command, args?, env?}`.
fn run_of(said: &serde_json::Value) -> Option<How> {
    let program = said.get("command")?.as_str()?;
    let arguments = said
        .get("args")
        .and_then(serde_json::Value::as_array)
        .map(|all| {
            all.iter()
                .filter_map(|it| it.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let env = said
        .get("env")
        .and_then(serde_json::Value::as_object)
        .map(|all| {
            all.iter()
                .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default();
    Some(How::Run {
        program: PathBuf::from(program),
        arguments,
        env,
    })
}

/// Whether the agent's answer is that the reader has to sign in first.
fn wants_signing_in(error: &agent_client_protocol::Error) -> bool {
    error.code == agent_client_protocol::ErrorCode::AuthRequired
}

/// What the agent said about needing a sign-in, where it said more than the
/// protocol's own words for it.
fn why_signing_in(error: &agent_client_protocol::Error) -> Option<String> {
    let said = error.message.trim();
    (!said.is_empty() && said != agent_client_protocol::ErrorCode::AuthRequired.to_string())
        .then(|| said.to_string())
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

/// Answers a request when what it waits on arrives, on a task of its own.
///
/// Not in the handler: the connection reads nothing while a handler runs,
/// and one agent is behind every conversation in the window -- so a card
/// waited for in there held every other conversation still until it was
/// answered. And off the loop, the agent taking the request back is a
/// message that can be read: it drops what was waiting, and `withdrawn`
/// says so.
fn answer_when<T, R>(
    connection: &ConnectionTo<Agent>,
    responder: Responder<R>,
    answered: oneshot::Receiver<T>,
    withdrawn: impl FnOnce() + Send + 'static,
    answer: impl FnOnce(Result<T, oneshot::Canceled>) -> Result<R, agent_client_protocol::Error>
    + Send
    + 'static,
) -> Result<(), agent_client_protocol::Error>
where
    T: Send + 'static,
    R: JsonRpcResponse + Send + 'static,
{
    let cancellation = responder.cancellation();
    connection.spawn(async move {
        let sent = match cancellation
            .run_until_cancelled(async { Ok(answered.await) })
            .await
        {
            Ok(got) => responder.respond_with_result(answer(got)),
            Err(error) => {
                withdrawn();
                responder.respond_with_error(error)
            }
        };
        // Swallowed rather than returned: an error from a task ends the
        // whole connection, and an answer that could not be sent is one
        // to a connection that has already ended.
        if let Err(error) = sent {
            tracing::debug!(?error, "an answer went nowhere");
        }
        Ok(())
    })
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

#[cfg(test)]
mod tests {
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
