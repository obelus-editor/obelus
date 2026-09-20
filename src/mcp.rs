//! What obelus lets an agent do.
//!
//! An agent can read a file and ask permission through the protocol it is
//! already speaking. What it cannot do through that protocol is anything
//! about *obelus*: it has no way to say "I think that note is finished" or
//! "here are two more worth writing down". MCP is the door for that, and
//! obelus is the server on the other side of it.
//!
//! None of them asks the reader anything. Two of them change the reader's
//! notes, and the asking before that is the agent's to do -- through
//! `elicitation/create`, the protocol it is already speaking, which obelus
//! answers with the very card it used to raise itself.
//!
//! It was the other way round once: the tools raised that card and waited
//! on it, on the argument that asking *is* the permission. What that bought
//! was a guarantee the reader always got a say. What it cost was three
//! things. A tool call held a request open while a person decided. The card
//! landed in whichever conversation happened to be waiting, because
//! obelus's own asking went through the one door the protocol puts no
//! session on -- so with two conversations open it was a guess. And the
//! agent's client asked permission for the call as well, which put one act
//! to the reader twice.
//!
//! What is left of the guarantee is that permission request, which is the
//! agent's to send and the reader's to answer. A weaker promise honestly
//! kept, against a stronger one bought by making a function wait on a
//! person.
//!
//! Neither of them takes a note away. `done` is how a list keeps what was
//! decided against, so ticking loses nothing and an agent has no need of
//! the one act that cannot be undone.
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

use crate::{event::Event, todo};

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
    /// The notes, in the order they should be written down.
    pub notes: Vec<Offered>,
    /// The note they all go under, by the name `todo_list` gives it.
    ///
    /// Left out, they go at the end of the list at the top level. A
    /// conversation opened on a note is told that note's name in its first
    /// message, and that is usually the one they belong under.
    pub under: Option<String>,
}

/// One note an agent is writing down.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct Offered {
    /// What it says. Short: several small notes are worth more than one
    /// long one, and the first line is what the reader sees in the list.
    pub said: String,
    /// How far under the note before it this one sits, counted from the
    /// top of what is being written down.
    ///
    /// Left out or zero for a note of its own. One more than the note
    /// before it at the most -- a note cannot hang under one that is not
    /// there -- and obelus brings anything deeper up to where it can hang.
    pub depth: Option<u16>,
}

#[tool_router]
impl Obelus {
    /// A server on this tree, answering to this main loop.
    #[must_use]
    pub fn new(root: &std::path::Path, events: Sender<Event>) -> Self {
        // One line per connection to the tools, which is the thing that
        // could not be found out before: obelus offering them and an agent
        // taking them up looked exactly alike from outside, and both looked
        // like nothing at all.
        //
        // Per connection and not per agent, which is what it says: the
        // transport builds one of these for each, and one agent opens
        // several over a turn. Counting them is not the point -- the point
        // is that there were any.
        tracing::info!(root = %root.display(), "something has connected to the tools");
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
    ///
    /// Said to be read-only, which it is: it reads a file of the reader's
    /// own and changes nothing. Agents ask their reader before running a
    /// tool and many of them stop asking for the ones that say this, which
    /// spares a question about a tool whose whole act is to look something
    /// up. A hint and not a promise -- the protocol says so, and says a
    /// client should not trust one from a server it does not know -- but
    /// obelus is the one making the claim about itself here, and it is
    /// true.
    ///
    /// The other two say nothing of the sort, because it would not be true:
    /// they write the reader's notes. What spares the question there is not
    /// something obelus can say about a tool -- see the module's own note on
    /// asking being the asking.
    #[tool(
        annotations(read_only_hint = true),
        description = "\
        Every note this project keeps: what it says, whether it is done, and \
        where it points. A line beginning with a name starts a note and the \
        lines under it are the rest of what that one says. A note indented \
        under another hangs under it, and finishing the one above is about \
        the whole of it. The name is what the other tools take."
    )]
    fn todo_list(&self) -> Result<CallToolResult, ErrorData> {
        tracing::info!("an agent asked for the notes");
        let todo = todo::Todo::read(&self.root);
        let said: Vec<String> = todo
            .notes
            .iter()
            .map(|note| {
                let done = if note.done { "done" } else { "not done" };
                let at = note.at.as_ref().map_or_else(String::new, |at| {
                    format!(" ({}:{})", at.path.display(), at.line.get() + 1)
                });
                let under = " ".repeat(usize::from(note.depth * crate::todo::INDENT));
                // The first line beside the name and the rest under it. A
                // note is allowed to be a paragraph, and printing the whole
                // of one where a line was expected put newlines in the
                // middle of a row: the list said it was one note per line
                // and was not, so a three-line note read as three notes
                // with two of them nameless.
                let mut said = format!(
                    "{under}{} [{done}]{at} {}",
                    note.id,
                    note.said.lines().next().unwrap_or_default()
                );
                for line in note.said.lines().skip(1) {
                    said.push_str(&format!("\n{under}  {line}"));
                }
                said
            })
            .collect();
        Ok(CallToolResult::success(vec![ContentBlock::text(
            said.join("\n"),
        )]))
    }

    /// Ticks a note off.
    #[tool(description = "\
        Tick a note off, once its work is done. Ask the reader first -- \
        this writes their file and does not ask for you. It only ticks: a \
        note that is done is kept, because a list of what was decided \
        against is worth as much as a list of what was never got to, and \
        taking one away is the reader's own.")]
    async fn todo_finish(
        &self,
        Parameters(About { note }): Parameters<About>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::info!(note, "an agent is ticking a note off");
        let Some(id) = todo::NoteId::read(&note) else {
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                "that is not a note's name; `todo_list` gives them",
            )]));
        };
        Ok(said(self.told(todo::Doing::Finish(id)).await))
    }

    /// Writes notes down.
    #[tool(description = "\
        Write notes down in this project. Ask the reader first -- this \
        writes their file and does not ask for you. Write down what \
        somebody would want to come back to, not a summary of what you just \
        did. From any conversation: what is worth writing down usually \
        turns up while doing something else. Give `under` a note's name to \
        hang them beneath it, and a note a `depth` to hang it beneath the \
        one before it.")]
    async fn todo_add(
        &self,
        Parameters(Proposed { notes, under }): Parameters<Proposed>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::info!(offered = notes.len(), "an agent is writing notes down");
        if notes.is_empty() {
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                "there is nothing there to write down",
            )]));
        }
        let beneath = match under.as_deref() {
            Some(name) => match todo::NoteId::read(name) {
                Some(id) => Some(id),
                None => {
                    return Ok(CallToolResult::error(vec![ContentBlock::text(
                        "that is not a note's name; `todo_list` gives them",
                    )]));
                }
            },
            None => None,
        };
        let notes = notes
            .into_iter()
            .map(|note| (note.said, note.depth.unwrap_or(0)))
            .collect();
        Ok(said(
            self.told(todo::Doing::Add {
                notes,
                under: beneath,
            })
            .await,
        ))
    }

    /// Hands one of those to the main loop and waits for it to be done.
    ///
    /// A wait on obelus itself, which is over in the time a file takes to
    /// write. Not the sort a person is at the other end of: an agent that
    /// wants the reader asked asks them, through the protocol it is already
    /// speaking.
    async fn told(&self, doing: todo::Doing) -> Option<String> {
        let (answer, answered) = futures::channel::oneshot::channel();
        self.events.send(Event::Notes { doing, answer }).ok()?;
        answered.await.ok()
    }
}

/// What obelus did, as the agent hears it.
///
/// A loop that has gone is obelus shutting down, and a tool answered with
/// "nobody is there" is better than one that never returns.
fn said(what: Option<String>) -> CallToolResult {
    match what {
        Some(said) => CallToolResult::success(vec![ContentBlock::text(said)]),
        None => CallToolResult::error(vec![ContentBlock::text("obelus is not there")]),
    }
}

#[tool_handler]
impl ServerHandler for Obelus {
    fn get_info(&self) -> InitializeResult {
        let mut info = InitializeResult::default();
        info.instructions = Some(
            "obelus, the reader this conversation is happening inside. It \
             keeps this project's notes.\n\n\
             `todo_finish` ticks a note off; `todo_add` writes notes down. \
             Both change the reader's file and neither asks for them, so ask \
             before calling either -- about finished work, not progress, and \
             notes worth returning to, not summaries.\n\n\
             Work that belongs to a note goes under it: `todo_add` takes \
             `under`, a note's name, and puts them beneath it. A note's own \
             `depth` puts it beneath the note before it.\n\n\
             From any conversation. One about a note says so in its first \
             message; otherwise `todo_list`."
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

    // A task on the one runtime, which is what it was already: a thread
    // whose whole job was to own a runtime of its own, because there was
    // none to put this on.
    crate::runtime::handle().spawn(async move {
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

    Ok(format!("http://{address}/mcp"))
}
