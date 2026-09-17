//! What obelus lets an agent do, and how it asks first.
//!
//! An agent can read a file and ask permission through the protocol it is
//! already speaking. What it cannot do through that protocol is anything
//! about *obelus*: it has no way to say "I think that note is finished" or
//! "here are two more worth writing down". MCP is the door for that, and
//! obelus is the server on the other side of it.
//!
//! Two of the three tools raise a card in the conversation and wait for the
//! reader to answer it. That is the permission model in its entirety: there
//! is no separate "may I", because asking *is* the asking. A tool that only
//! changes what the reader is looking at needs no card and gets none.
//!
//! The card is the one an agent's own questions are answered on, reached
//! through the same [`crate::acp::Incoming::Ask`] those arrive as -- the
//! conversation's box has nothing to send while an agent is waiting, which
//! is what the card was written for and is exactly the case here.
//!
//! The dispatch is behind rmcp's macros rather than written out, which is
//! the one place in obelus where a decision is not on the page beside the
//! code that makes it. It buys the tools' JSON Schemas being generated from
//! these function signatures, so the two cannot drift: written by hand they
//! are two things, and changing one and forgetting the other is a mistake
//! the compiler cannot see. Readability against a bug that really happens.

use std::sync::{Arc, mpsc::Sender};

use rmcp::{
    ErrorData, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ContentBlock, InitializeResult, ServerCapabilities},
    tool, tool_handler, tool_router,
};
use serde::Deserialize;

use crate::{acp, event::Event, todo};

/// obelus, as an agent can reach it.
#[derive(Clone)]
pub struct Obelus {
    /// The tree the notes belong to.
    root: std::path::PathBuf,
    /// How to reach the main loop, which is the only thing that may draw.
    events: Sender<Event>,
    /// The tools, as `#[tool_router]` built them from the signatures below.
    ///
    /// Read by the macro-generated dispatch rather than by anything here,
    /// which is what the warning about it is: it is the whole of what this
    /// type is *for*, reached through a door obelus does not write.
    #[expect(dead_code, reason = "read by the dispatch `#[tool_handler]` generates")]
    tools: ToolRouter<Self>,
}

/// Which note a tool is about.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct About {
    /// The note's own name, as `todo_list` gave it.
    pub note: String,
}

/// What to write down.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct Proposed {
    /// One line each. The reader is shown all of them and keeps the ones
    /// they want, so several small ones are better than one long one.
    pub notes: Vec<String>,
}

#[tool_router]
impl Obelus {
    /// A server on this tree, answering to this main loop.
    #[must_use]
    pub fn new(root: &std::path::Path, events: Sender<Event>) -> Self {
        Self {
            root: root.to_path_buf(),
            events,
            tools: Self::tool_router(),
        }
    }

    /// What the tree means to come back to.
    ///
    /// Indented the way the reader's own page indents it, because a note may
    /// hang under another: a flat list would have an agent asking to finish
    /// a note without knowing what else was under it, and the tools take a
    /// note at a time.
    #[tool(description = "\
        Every note this project keeps: what it says, whether it is done, and \
        the file and line it is about where it is about one. A note indented \
        under another hangs under it, and finishing or dropping the one \
        above is about the whole of it. Each carries a name -- that is what \
        the other tools take.")]
    fn todo_list(&self) -> Result<CallToolResult, ErrorData> {
        let todo = todo::Todo::read(&self.root);
        let said: Vec<String> = todo
            .notes
            .iter()
            .map(|note| {
                let done = if note.done { "done" } else { "not done" };
                let at = note.at.as_ref().map_or_else(String::new, |at| {
                    format!(" ({}:{})", at.path.display(), at.line.get() + 1)
                });
                let under = " ".repeat(usize::from(
                    note.depth * crate::component::todo::INDENT,
                ));
                format!("{under}{} [{done}]{at} {}", note.id, note.said)
            })
            .collect();
        Ok(CallToolResult::success(vec![ContentBlock::text(
            said.join("\n"),
        )]))
    }

    /// The reader is asked what to do about a note the agent thinks is done.
    #[tool(description = "\
        Say that a note's goal looks met. The reader is asked what to do \
        about it and this returns what they chose, which may be nothing. \
        Call it when the work a note describes is finished, not to check in.")]
    async fn todo_finish(
        &self,
        Parameters(About { note }): Parameters<About>,
    ) -> Result<CallToolResult, ErrorData> {
        let Some(id) = todo::NoteId::read(&note) else {
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                "that is not a note's name; `todo_list` gives them",
            )]));
        };
        let todo = todo::Todo::read(&self.root);
        let Some(about) = todo.notes.iter().find(|note| note.id == id) else {
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                "there is no note by that name any more",
            )]));
        };

        let chosen = ask(
            self.events.clone(),
            format!("about \"{}\"", about.title()),
            vec![acp::Field {
                name: "what".to_string(),
                title: "what should happen to it".to_string(),
                about: None,
                takes: acp::Takes::One(vec![
                    value("done", "mark it done"),
                    value("drop", "take it away"),
                    value("leave", "leave it as it is"),
                ]),
                required: true,
            }],
        )
        .await;

        // Nothing chosen is the reader walking away from the question, which
        // is an answer: they have not said no, they have said nothing, and
        // the note stays exactly as it was.
        let Some(chosen) = chosen else {
            return Ok(CallToolResult::success(vec![ContentBlock::text(
                "the reader did not answer; the note is unchanged",
            )]));
        };
        Ok(CallToolResult::success(vec![ContentBlock::text(format!(
            "the reader chose: {chosen}"
        ))]))
    }

    /// Follow-up notes, of which the reader keeps the ones they want.
    #[tool(description = "\
        Offer notes to add to this project. The reader is shown all of them \
        and keeps the ones they want; this returns which were kept. Offer \
        what somebody would want to come back to, not a summary of what you \
        just did.")]
    async fn todo_add(
        &self,
        Parameters(Proposed { notes }): Parameters<Proposed>,
    ) -> Result<CallToolResult, ErrorData> {
        if notes.is_empty() {
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                "there is nothing there to offer",
            )]));
        }
        let values: Vec<acp::Value> = notes
            .iter()
            .enumerate()
            .map(|(at, said)| value(&at.to_string(), said))
            .collect();

        let kept = ask(
            self.events.clone(),
            "notes to add".to_string(),
            vec![acp::Field {
                name: "keep".to_string(),
                title: "which of these to write down".to_string(),
                about: None,
                takes: acp::Takes::Some {
                    values,
                    // None of them, which is a real answer: an agent
                    // that offered four bad ones should be told so
                    // rather than made to have one of them written down.
                    least: Some(0),
                    most: None,
                    chosen: Vec::new(),
                },
                required: false,
            }],
        )
        .await;

        let Some(kept) = kept else {
            return Ok(CallToolResult::success(vec![ContentBlock::text(
                "the reader did not answer; nothing was written down",
            )]));
        };
        Ok(CallToolResult::success(vec![ContentBlock::text(format!(
            "the reader kept: {kept}"
        ))]))
    }
}

/// Puts a question to the reader and waits for it.
///
/// Through the door an agent's own questions come through, which is the
/// whole of why this layer is thin: the card, the keys that walk it, the
/// answer that goes back and the refusal when the reader walks away are all
/// already there, and were written for exactly this shape of thing.
///
/// Blocking is right. The conversation genuinely cannot go on -- the agent
/// is waiting on an answer -- and the row in the list of open documents says
/// so, so a reader who is somewhere else can see which conversation wants
/// them. It costs no thread: the wait is an `await`.
///
/// Taking the channel by value rather than off `&self`, because the future
/// outlives the call: a future holding a borrow of the server could not be
/// the `'static` one the router takes.
async fn ask(events: Sender<Event>, message: String, fields: Vec<acp::Field>) -> Option<String> {
    let (answer, answered) = futures::channel::oneshot::channel();
    let question = acp::Incoming::Ask {
        message,
        fields,
        answer,
    };
    // A main loop that has gone is obelus shutting down, and a tool call
    // answered with "nobody is there" is better than one that never returns.
    events.send(Event::Acp(question)).ok()?;
    let given = answered.await.ok()??;
    let said: Vec<String> = given
        .into_iter()
        .map(|(_, reply)| format!("{reply:?}"))
        .collect();
    Some(said.join(", "))
}

/// One of the ways out of a question.
fn value(id: &str, name: &str) -> acp::Value {
    acp::Value {
        id: id.to_string(),
        name: name.to_string(),
        about: None,
    }
}

#[tool_handler]
impl ServerHandler for Obelus {
    fn get_info(&self) -> InitializeResult {
        let mut info = InitializeResult::default();
        info.instructions = Some(
            "obelus, the reader this conversation is happening inside. Two of \
             these tools put a question to the reader and wait for an answer; \
             neither changes anything until they have answered."
                .to_string(),
        );
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info
    }
}

/// Starts the server and says where an agent should reach it.
///
/// On the loopback and on whatever port the machine hands out: the agent is
/// told the address, so there is nothing to agree in advance and nothing to
/// collide with a second obelus.
///
/// # Errors
///
/// Where the socket cannot be taken, which is a machine with no loopback --
/// obelus goes on without the tools and says so.
pub fn serve(root: &std::path::Path, events: Sender<Event>) -> std::io::Result<String> {
    use rmcp::transport::streamable_http_server::{
        StreamableHttpService, session::local::LocalSessionManager,
    };

    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    listener.set_nonblocking(true)?;

    let root = root.to_path_buf();
    let service = StreamableHttpService::new(
        move || Ok(Obelus::new(&root, events.clone())),
        Arc::new(LocalSessionManager::default()),
        rmcp::transport::streamable_http_server::StreamableHttpServerConfig::default(),
    );
    let router = axum::Router::new().route_service("/mcp", service);

    // Its own runtime and its own thread. The agent's connection has one of
    // each already and sharing would tie two lifetimes together for no
    // reason: this one lives as long as obelus, and that one lives as long
    // as the agent does.
    std::thread::Builder::new()
        .name("obelus-mcp".to_string())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    tracing::warn!(%error, "no runtime for the tools obelus offers");
                    return;
                }
            };
            runtime.block_on(async move {
                let listener = match tokio::net::TcpListener::from_std(listener) {
                    Ok(listener) => listener,
                    Err(error) => {
                        tracing::warn!(%error, "the tools obelus offers are not listening");
                        return;
                    }
                };
                if let Err(error) = axum::serve(listener, router).await {
                    tracing::warn!(%error, "the tools obelus offers stopped");
                }
            });
        })?;

    Ok(format!("http://{address}/mcp"))
}
