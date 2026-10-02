//! What Obelus lets an agent do.
//!
//! An agent can read a file and ask permission through the protocol it is
//! already speaking. What it cannot do through that protocol is anything
//! about *obelus*: it has no way to say "I think that note is finished" or
//! "here are two more worth writing down". MCP is the door for that, and
//! Obelus is the server on the other side of it.
//!
//! None of them asks the reader anything, and the asking before any of
//! them is the agent's to do -- through `elicitation/create`, the protocol
//! it is already speaking, which Obelus answers with the very card it used
//! to raise itself.
//!
//! It was the other way round once: the tools raised that card and waited
//! on it, on the argument that asking *is* the permission. What that bought
//! was a guarantee the reader always got a say. What it cost was three
//! things. A tool call held a request open while a person decided. The card
//! landed in whichever conversation happened to be waiting, because
//! Obelus's own asking went through the one door the protocol puts no
//! session on -- so with two conversations open it was a guess. And the
//! agent's client asked permission for the call as well, which put one act
//! to the reader twice.
//!
//! What is left of the guarantee is that permission request, which is the
//! agent's to send and the reader's to answer. A weaker promise honestly
//! kept, against a stronger one bought by making a function wait on a
//! person.
//!
//! Three of them change the reader's notes. The fourth changes nothing at
//! all: `open_file` puts a file on their screen, and what it takes is
//! their attention rather than anything on disk. It is the one here that
//! keeps the strong half of the promise rather than the weak -- it goes
//! through the door every jump in Obelus goes through, so `alt+left`
//! brings them back -- and it is still offered rather than done, because
//! being taken off what you were reading is a thing that happened to you.
//! It is here because the alternative is an agent naming a file and a line
//! and leaving the reader to go and find it.
//!
//! And one is not about the reader at all: `read_workflow` hands over how
//! this project has chosen to have its files changed. It is a tool rather
//! than a paragraph of the opening because most conversations change
//! nothing, and an agent reads it when it is about to -- the way a skill is
//! loaded rather than said. It goes through the loop rather than reading a
//! file here, because which workflow is chosen is the settings laid over
//! one another, and that is answered once, in `obelus-app`.
//!
//! And one takes something off the screen: `close_conversation` closes the
//! conversation it is called from. Closed and not ended: what goes is a
//! document from what is open and nothing else -- the session is not let
//! go and the list of conversations still offers it -- which is why an
//! agent may do it at all once it has asked. Whether taking it up again
//! brings its words back is the agent's, so the description promises
//! nothing about that: an agent that can only resume comes back to an
//! empty page, and one that can do neither to a new conversation. It is
//! the one tool
//! that has to know *which* conversation is calling, and MCP has no word
//! for that: every conversation is told an address of its own, the
//! server's with the conversation's number on the end ([`address`]), and
//! the tool reads the number back off the request it came on.
//!
//! None of them takes a note away. `done` is how a list keeps what was
//! decided against, so ticking loses nothing and an agent has no need of
//! the one act that leaves nothing behind.
//!
//! Rewording is the one that does lose something -- the words the reader
//! wrote, out of a file git has never heard of and a page with no undo.
//! It is here because a note whose subject has turned out to be something
//! else is wrong on the one line the reader reads, and nothing hung under
//! it fixes that line. See `todo::Doing` for the whole of that argument.
//! What it costs is that its description spends most of its words on the
//! asking, and says what the asking is for: the reader agrees to the new
//! words, not to the idea of a change.
//!
//! The dispatch is behind rmcp's macros rather than written out, which is
//! the one place in Obelus where a decision is not on the page beside the
//! code that makes it. It buys the tools' JSON Schemas being generated from
//! these function signatures, so the two cannot drift: written by hand they
//! are two things, and changing one and forgetting the other is a mistake
//! the compiler cannot see. Readability against a bug that really happens.

use std::sync::Arc;

use obelus_git::todo;
use obelus_sink::Sink;
use rmcp::{
    ErrorData, ServerHandler,
    handler::server::{router::tool::ToolRouter, tool::Extension, wrapper::Parameters},
    model::{CallToolResult, ContentBlock, InitializeResult, ServerCapabilities},
    tool, tool_handler, tool_router,
};
use serde::Deserialize;

/// An agent asked Obelus to change the notes.
///
/// Through the loop rather than written from the server's own thread,
/// because the loop is the one writer: the file is read, changed and
/// written whole, and two threads doing that is one of them losing a
/// change it never saw. The answer goes back so the tool can say what
/// happened -- a wait on Obelus itself, over in microseconds, and not
/// the sort a person is at the other end of.
#[derive(Debug)]
pub struct Asked {
    /// What the agent asked for.
    pub wanted: Wanted,
    /// What Obelus did, or why it did not.
    pub answer: futures::channel::oneshot::Sender<String>,
}

/// What an agent asked Obelus to do.
///
/// Four kinds, and the difference is worth the enum: three of the tools
/// write the reader's notes, one of them puts a file on their screen, one
/// asks what the settings say, and one closes a conversation. The notes are
/// a file Obelus is the only writer of; the file is the reader's own
/// attention; the workflow is a question only the loop can answer; and
/// what is open is the loop's.
#[derive(Debug)]
pub enum Wanted {
    /// A change to the notes.
    Notes(obelus_git::todo::Doing),
    /// A file, in front of the reader.
    Open {
        /// Where it is: against the project, or a path of its own.
        path: String,
        /// Which line to land on, counted the way a reader counts them.
        ///
        /// `None` leaves the file where it was last read, which for one
        /// being opened for the first time is its first line.
        line: Option<u32>,
    },
    /// How this project has chosen to have its files changed.
    Workflow,
    /// The conversation that asked, closed.
    Close {
        /// Which one, by the number its address carries.
        ///
        /// `None` where the address carried none, which is an agent that
        /// was told the tools by some other way than a conversation: there
        /// is nothing it could be asking to close.
        conversation: Option<usize>,
    },
}

/// Obelus, as an agent can reach it.
#[derive(Clone)]
pub struct Obelus {
    /// The project the notes belong to.
    root: std::path::PathBuf,
    /// How to reach the main loop, which is the only thing that may draw.
    ///
    /// Behind a pointer rather than as a type parameter: this struct has to
    /// be `Clone`, and the two impls that make it a server are written by
    /// `#[tool_router]` and `#[tool_handler]` on a plain `impl Obelus`. A
    /// parameter here would have to appear in a macro expansion Obelus does
    /// not write.
    events: Arc<dyn Sink<Asked>>,
    /// The tools, as `#[tool_router]` built them from the signatures below.
    ///
    /// Read by the macro-generated dispatch rather than by anything here,
    /// which is what the warning about it is: it is the whole of what this
    /// type is *for*, reached through a door Obelus does not write.
    #[expect(dead_code, reason = "read by the dispatch `#[tool_handler]` generates")]
    tools: ToolRouter<Self>,
}

/// Which note a tool is about.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct About {
    /// The note's own name, as `todo_list` gave it.
    pub note: String,
}

/// A file to put in front of the reader.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct Shown {
    /// Where it is, against the project this conversation is about. A
    /// path of its own is taken as it is.
    pub path: String,
    /// Which line to land on, counted from one the way `todo_list`
    /// prints them and the way the reader's own screen numbers them.
    pub line: Option<u32>,
}

/// A note, and what it should say instead.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct Reworded {
    /// The note's own name, as `todo_list` gave it.
    pub note: String,
    /// The whole of what it says from now on.
    ///
    /// What it says now is replaced, not added to. A note may be a
    /// paragraph, and its first line is the one the list shows -- so a
    /// rewording that leaves that line alone has left the note looking
    /// exactly as wrong as it did.
    pub said: String,
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
    /// there -- and Obelus brings anything deeper up to where it can hang.
    pub depth: Option<u16>,
}

#[tool_router]
impl Obelus {
    /// A server on this project, answering to this main loop.
    #[must_use]
    pub fn new(root: &std::path::Path, events: Arc<dyn Sink<Asked>>) -> Self {
        // One line per connection to the tools, which is the thing that
        // could not be found out before: Obelus offering them and an agent
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

    /// What the project means to come back to.
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
    /// Obelus is the one making the claim about itself here, and it is
    /// true.
    ///
    /// The other two say nothing of the sort, because it would not be true:
    /// they write the reader's notes. What spares the question there is not
    /// something Obelus can say about a tool -- see the module's own note on
    /// asking being the asking.
    #[tool(
        annotations(read_only_hint = true),
        description = "\
        Every note this project keeps: what it says, whether it is done, and \
        where it points. A line beginning with a name starts a note and the \
        lines under it are the rest of what that one says. A note indented \
        under another hangs under it, and finishing the one above is about \
        the whole of it. The name is what the other tools take, and only \
        them: nothing on the reader's screen shows it."
    )]
    fn todo_list(&self) -> Result<CallToolResult, ErrorData> {
        tracing::info!("an agent asked for the notes");
        // "There are none" is not the answer to "it will not read". An
        // agent told the list is empty writes down what is already in it,
        // and tells the reader their project has nothing to come back to.
        let Some(todo) = todo::read(&self.root).notes() else {
            return Ok(CallToolResult::success(vec![ContentBlock::text(
                "the notes file will not read, so this is not the list".to_string(),
            )]));
        };
        let said: Vec<String> = todo
            .notes
            .iter()
            .map(|note| {
                let done = if note.done { "done" } else { "not done" };
                let at = note.at.as_ref().map_or_else(String::new, |at| {
                    format!(" ({}:{})", at.path.display(), at.line.get() + 1)
                });
                let under = " ".repeat(usize::from(note.depth * obelus_git::todo::INDENT));
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

    /// Puts a file on the reader's screen.
    ///
    /// The one tool that changes nothing at all. What it takes is the
    /// reader's attention, which is why the description asks first the
    /// same way the others do -- but the promise behind it is the strong
    /// one rather than the weak: this goes through the same door every
    /// jump in Obelus goes through, so `alt+left` brings them back, and
    /// what an agent does the reader can see and take back.
    ///
    /// Not `read_only_hint`, which is about the *world*: a tool that
    /// moves the reader off what they were reading has done something,
    /// and a hint that spares the question would spare it for the one
    /// act here that is about them rather than about a file.
    #[tool(description = "\
        Put a file on the reader's screen, at a line if you name one. \
        Offer first and let them say yes -- this takes them off whatever \
        they were reading, and `elicitation/create` is how to ask. Use it \
        when you are talking about a place in the code: naming a file and \
        a line asks them to go and find it, and this is the going. A file \
        they already have open is the one they are taken to rather than a \
        second copy of it, and `alt+left` brings them back to where they \
        were. `path` is against the project; `line` counts from one, the \
        way `todo_list` prints the line a note points at.")]
    async fn open_file(
        &self,
        Parameters(Shown { path, line }): Parameters<Shown>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::info!(path, line, "an agent is opening a file for the reader");
        if path.trim().is_empty() {
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                "there is no path there to open",
            )]));
        }
        Ok(said(self.told(Wanted::Open { path, line }).await))
    }

    /// Hands over the project's workflow.
    ///
    /// Read-only, which it is: what it reads is the settings, and what it
    /// answers changes nothing. Through the loop all the same, because
    /// which workflow is chosen is the reader's settings with the
    /// project's laid over them, and that is worked out in one place.
    #[tool(
        annotations(read_only_hint = true),
        description = "\
        How this project has chosen to have its files changed: where to \
        make a change, what to ask the reader once it is made, and how it \
        reaches the main branch. Read it before your first change and again \
        when the reader says the work is done, and follow it. A project \
        that has chosen none says so."
    )]
    async fn read_workflow(&self) -> Result<CallToolResult, ErrorData> {
        tracing::info!("an agent asked for the workflow");
        Ok(said(self.told(Wanted::Workflow).await))
    }

    /// Closes the conversation it is called from.
    ///
    /// Which one is read off the address the request came to, because
    /// nothing in the call says: see the module's note on why every
    /// conversation has an address of its own.
    #[tool(description = "\
        Close this conversation, once the work it was opened for is done. \
        Ask the reader first, with `elicitation/create` -- this takes it off \
        their screen and does not ask for you. It closes when this turn \
        ends, so make it the last thing you do and say whatever is left to \
        say before it; and it stays open if the reader has started writing \
        in it. It stays in the list of conversations, and is taken up again \
        from there as fully as you can take a conversation up again -- so \
        do not promise the reader it comes back as it was.")]
    async fn close_conversation(
        &self,
        Extension(parts): Extension<axum::http::request::Parts>,
    ) -> Result<CallToolResult, ErrorData> {
        let conversation = conversation_in(parts.uri.path());
        tracing::info!(conversation, "an agent is closing its conversation");
        Ok(said(self.told(Wanted::Close { conversation }).await))
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
        Ok(said(
            self.told(Wanted::Notes(todo::Doing::Finish(id))).await,
        ))
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
            self.told(Wanted::Notes(todo::Doing::Add {
                notes,
                under: beneath,
            }))
            .await,
        ))
    }

    /// Makes a note say something else.
    ///
    /// The description asks for more than the other two do, because this is
    /// the one act with nothing behind it: the notes are not in git and the
    /// page has no undo, so the words it replaces are gone. Showing the
    /// reader those words is therefore part of the asking rather than a
    /// nicety -- "may I reword it" is a question nobody can answer.
    #[tool(description = "\
        Rewrite what a note says, keeping the note. Ask the reader first, \
        and show them both what it says now and what it would say instead \
        -- this writes their file, does not ask for you, and replaces words \
        they wrote that nothing else keeps. For a note whose subject has \
        turned out to be something other than what it was written for. \
        Where the work has merely grown, `todo_add` with `under` hangs the \
        new part beneath it and keeps what they wrote, which is the usual \
        answer. `said` replaces the whole note, first line and all.")]
    async fn todo_reword(
        &self,
        // `said` renamed on the way in: the answer goes back through a
        // function of that name, and a binding here would shadow it.
        Parameters(Reworded { note, said: words }): Parameters<Reworded>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::info!(note, "an agent is rewording a note");
        let Some(id) = todo::NoteId::read(&note) else {
            return Ok(CallToolResult::error(vec![ContentBlock::text(
                "that is not a note's name; `todo_list` gives them",
            )]));
        };
        Ok(said(
            self.told(Wanted::Notes(todo::Doing::Reword {
                note: id,
                said: words,
            }))
            .await,
        ))
    }

    /// Hands one of those to the main loop and waits for it to be done.
    ///
    /// A wait on Obelus itself, which is over in the time a file takes to
    /// write. Not the sort a person is at the other end of: an agent that
    /// wants the reader asked asks them, through the protocol it is already
    /// speaking.
    async fn told(&self, wanted: Wanted) -> Option<String> {
        let (answer, answered) = futures::channel::oneshot::channel();
        self.events.send(Asked { wanted, answer }).ok()?;
        answered.await.ok()
    }
}

/// What Obelus did, as the agent hears it.
///
/// A loop that has gone is Obelus shutting down, and a tool answered with
/// "nobody is there" is better than one that never returns.
fn said(what: Option<String>) -> CallToolResult {
    match what {
        Some(said) => CallToolResult::success(vec![ContentBlock::text(said)]),
        None => CallToolResult::error(vec![ContentBlock::text("Obelus is not there")]),
    }
}

#[tool_handler]
impl ServerHandler for Obelus {
    fn get_info(&self) -> InitializeResult {
        let mut info = InitializeResult::default();
        // Nothing about `open_file` or `close_conversation`. When to offer
        // either is said in the opening every conversation begins with, and
        // how in the tool's own description, so a paragraph here was the
        // same thing told a third time -- and a story told twice drifts.
        info.instructions = Some(
            "Obelus, the reader this conversation is happening inside. It \
             keeps this project's notes.\n\n\
             `todo_finish` ticks a note off; `todo_add` writes notes down; \
             `todo_reword` makes one say something else. All three change \
             the reader's file and none of them asks for them, so ask before \
             calling any -- about finished work, not progress, and notes \
             worth returning to, not summaries. Rewording replaces what they \
             wrote and nothing keeps it, so that one is asked with the words \
             themselves, both what it says and what it would say.\n\n\
             A note's name is a handle for these tools and for nothing \
             else. The reader has never seen one: their notes are drawn as \
             the words they wrote, and no name appears anywhere on their \
             screen. So say which note you mean in its own words -- naming \
             one at them asks them to look up something they have no way to \
             look up.\n\n\
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
/// collide with a second Obelus.
///
/// # Errors
///
/// Where the socket cannot be taken, which is a machine with no loopback --
/// Obelus goes on without the tools and says so.
///
/// What comes back is where the server is and not where to reach it:
/// a conversation is offered the tools at its own [`address`] under it.
pub fn serve(root: &std::path::Path, events: Arc<dyn Sink<Asked>>) -> std::io::Result<String> {
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
    let router = axum::Router::new().route_service("/mcp/{conversation}", service);

    // A task on the one runtime, which is what it was already: a thread
    // whose whole job was to own a runtime of its own, because there was
    // none to put this on.
    obelus_runtime::handle().spawn(async move {
        let listener = match tokio::net::TcpListener::from_std(listener) {
            Ok(listener) => listener,
            Err(error) => {
                tracing::warn!(%error, "the tools Obelus offers are not listening");
                return;
            }
        };
        if let Err(error) = axum::serve(listener, router).await {
            tracing::warn!(%error, "the tools Obelus offers stopped");
        }
    });

    Ok(format!("http://{address}/mcp"))
}

/// Where one conversation reaches the tools: the server's address, with
/// the conversation's number on the end.
///
/// Written here beside [`conversation_in`], which reads it back, so that
/// the two cannot come to disagree about the shape.
#[must_use]
pub fn address(server: &str, conversation: usize) -> String {
    format!("{server}/{conversation}")
}

/// Which conversation an address names, read off the path a request came
/// to.
fn conversation_in(path: &str) -> Option<usize> {
    path.strip_prefix("/mcp/")?.parse().ok()
}

#[cfg(test)]
mod tests {
    /// What `address` writes, `conversation_in` reads.
    ///
    /// Deliberate break: have `address` put the number in a query rather
    /// than the path, and the number read back is `None`.
    #[test]
    fn the_number_an_address_carries_is_the_number_read_off_it() {
        let written = super::address("http://127.0.0.1:4000/mcp", 7);
        let path = written
            .strip_prefix("http://127.0.0.1:4000")
            .expect("the server's own address");
        assert_eq!(super::conversation_in(path), Some(7));
        assert_eq!(super::conversation_in("/mcp"), None);
    }
}
