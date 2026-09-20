//! One language server, and the bookkeeping the protocol needs.
//!
//! This layer knows the handshake, the request numbering and the progress
//! notifications. It does not know what any particular request was *for*:
//! that is the caller's, because the answer arrives after the world has moved
//! on and only the caller can say whether it still means anything.

use std::{collections::HashMap, path::Path, process::Stdio};

use anyhow::{Context as _, Result};
use lsp_types::{
    CallHierarchyClientCapabilities, ClientCapabilities, CodeActionCapabilityResolveSupport,
    CodeActionClientCapabilities, CodeActionKind, CodeActionKindLiteralSupport,
    CodeActionLiteralSupport, CompletionClientCapabilities, CompletionItemCapability,
    CompletionItemCapabilityResolveSupport, DidChangeWatchedFilesClientCapabilities,
    DocumentColorClientCapabilities, DocumentFormattingClientCapabilities,
    DocumentHighlightClientCapabilities, DocumentSymbolClientCapabilities,
    ExecuteCommandClientCapabilities, FailureHandlingKind, GeneralClientCapabilities,
    GotoCapability, HoverClientCapabilities, InitializeParams, InitializeResult,
    InlayHintClientCapabilities, MarkupKind, ParameterInformationSettings, PositionEncodingKind,
    PublishDiagnosticsClientCapabilities, ReferenceClientCapabilities, RenameClientCapabilities,
    SemanticTokenType, SemanticTokensClientCapabilities, SemanticTokensClientCapabilitiesRequests,
    SemanticTokensFullOptions, ServerCapabilities, SignatureHelpClientCapabilities,
    SignatureInformationSettings, TextDocumentClientCapabilities,
    TextDocumentSyncClientCapabilities, TokenFormat, Uri, WindowClientCapabilities,
    WorkspaceClientCapabilities, WorkspaceEditClientCapabilities,
    WorkspaceFileOperationsClientCapabilities, WorkspaceFolder, WorkspaceSymbolClientCapabilities,
};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    lsp::{Message, transport},
    sink::Sink,
    syntax::LanguageId,
};

/// The id obelus uses for its `initialize` request.
///
/// Fixed rather than drawn from the counter so recognising the one reply that
/// has to be handled here needs no extra state.
const INITIALIZE_ID: i64 = 0;

/// An answer to something the caller asked for.
#[derive(Debug)]
pub struct Reply {
    /// Which request it answers.
    pub id: i64,
    /// The result, or the error the server reported.
    pub result: Result<Value, String>,
}

/// An edit a server has asked obelus to make across the project.
///
/// A request rather than an answer: the server is waiting to be told
/// whether it happened, which is why the id is kept with it.
#[derive(Debug)]
pub struct AskedEdit {
    /// The request to answer once the edit has been made, or not.
    pub id: Value,
    /// What the server calls it, where it says.
    pub label: Option<String>,
    /// The `WorkspaceEdit` itself.
    pub edit: Value,
}

/// A running language server.
pub struct Client {
    language: LanguageId,
    process: tokio::process::Child,
    /// Whether the process has been found to have stopped.
    exited: bool,
    outgoing: tokio::sync::mpsc::UnboundedSender<String>,
    next_id: i64,
    /// Messages held until the handshake finishes.
    ///
    /// A server may not be sent anything but `initialize` until it has
    /// answered, and obelus opens its first file well before that: the queue
    /// is why the caller does not have to know.
    ///
    /// This is protocol correctness rather than something a reader would
    /// notice. rust-analyzer reads the workspace from disk itself, so it
    /// answers questions about a file whether or not it was told the file is
    /// open — `didOpen` only matters once a buffer and the disk disagree. A
    /// stricter server is entitled to refuse anything sent before it has
    /// answered `initialize`, and this is why obelus never sends it.
    queued: Vec<String>,
    capabilities: Option<ServerCapabilities>,
    encoding: PositionEncodingKind,
    /// Progress tokens in flight, and what each says it is doing.
    working: HashMap<String, String>,
    /// What the server has said is wrong, since the caller last looked.
    ///
    /// Diagnostics arrive unasked, so there is no question waiting for
    /// them and nothing for [`Client::on_message`] to answer with. Kept
    /// here and drained by the caller, which is the only side that knows
    /// whether the file they are about is still open.
    published: Vec<Value>,
    /// The edits the server has asked obelus to make, since the caller
    /// last looked.
    ///
    /// Here for the same reason [`Client::published`] is, and for one
    /// more: this layer knows the protocol and not the documents, so the
    /// only side that can make the edit -- or say why it could not -- is
    /// the caller.
    asked: Vec<AskedEdit>,
    /// How many messages have gone to the writer.
    ///
    /// Alongside [`Client::queued`] because between them they are the only
    /// way to tell a held message from a dropped one: a server answers the
    /// same either way, so nothing that comes back distinguishes flushing the
    /// queue from throwing it away.
    sent: usize,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Client")
            .field("language", &self.language)
            .field("ready", &self.capabilities.is_some())
            .field("encoding", &self.encoding)
            .field("working", &self.working.len())
            .finish_non_exhaustive()
    }
}

impl Client {
    /// Starts a server and sends the handshake.
    ///
    /// Returns as soon as the process is spawned. The server is not usable
    /// until its `initialize` reply arrives, which is what the queue is for;
    /// rust-analyzer answers in milliseconds and then spends seconds indexing,
    /// which is a different thing and visible through [`Client::working_on`].
    pub fn start(
        language: LanguageId,
        server: crate::lsp::Server,
        root: &Path,
        sender: impl Sink<Message> + Clone,
    ) -> Result<Self> {
        let command = server.command;
        // Inside the runtime, because a child's pipes register with it.
        let _inside = crate::runtime::handle().enter();
        let mut process = tokio::process::Command::new(command)
            .args(server.arguments)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("starting {command}"))?;

        let stdout = process.stdout.take().context("the server has no stdout")?;
        let stdin = process.stdin.take().context("the server has no stdin")?;
        // Piped rather than discarded: a server that will not start says why
        // here and nowhere else.
        if let Some(stderr) = process.stderr.take() {
            spawn_logger(command.to_string(), stderr);
        }

        spawn_reader(language, stdout, sender);
        let outgoing = spawn_writer(command.to_string(), stdin);

        let mut client = Self {
            language,
            process,
            exited: false,
            outgoing,
            next_id: INITIALIZE_ID + 1,
            queued: Vec::new(),
            capabilities: None,
            // Until the server says otherwise. The protocol's default, and
            // the expensive one.
            encoding: PositionEncodingKind::UTF16,
            working: HashMap::new(),
            published: Vec::new(),
            asked: Vec::new(),
            sent: 0,
        };
        client.send_initialize(root)?;
        Ok(client)
    }

    /// The language this server was started for.
    #[must_use]
    pub const fn language(&self) -> LanguageId {
        self.language
    }

    /// Whether the handshake has finished.
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        self.capabilities.is_some()
    }

    /// Asks the operating system whether the process is still running, and
    /// remembers the answer.
    ///
    /// Needs `&mut` because reaping does, which is why the answer is kept:
    /// the status bar reads it from a place that has only `&self`. A server
    /// that has died is not otherwise noticed -- its reader thread simply
    /// stops, and every question after that goes unanswered with no reply to
    /// say so.
    pub fn check_alive(&mut self) -> bool {
        if self.exited {
            return false;
        }
        if matches!(self.process.try_wait(), Ok(Some(_))) {
            tracing::warn!(language = self.language.name(), "the server has exited");
            self.exited = true;
        }
        !self.exited
    }

    /// What [`Client::check_alive`] last found out.
    #[must_use]
    pub const fn state(&self) -> crate::lsp::ServerState {
        if self.exited {
            crate::lsp::ServerState::Gone
        } else if self.capabilities.is_some() {
            crate::lsp::ServerState::Ready
        } else {
            crate::lsp::ServerState::Starting
        }
    }

    /// What the server said it can do, once it has said so.
    #[must_use]
    pub const fn capabilities(&self) -> Option<&ServerCapabilities> {
        self.capabilities.as_ref()
    }

    /// Which units positions are counted in.
    #[must_use]
    pub const fn encoding(&self) -> &PositionEncodingKind {
        &self.encoding
    }

    /// How many messages are being held until the handshake finishes.
    ///
    /// Exposed because the queue has no other observable effect: what comes
    /// back from a server is the same either way, so the only way to know it
    /// works is to look at what has not gone out yet.
    #[must_use]
    pub fn queued(&self) -> usize {
        self.queued.len()
    }

    /// How many messages have gone out.
    #[must_use]
    pub const fn sent(&self) -> usize {
        self.sent
    }

    /// What the server is busy with, if it is busy.
    ///
    /// The reason this is exposed: a server still indexing answers a query
    /// with nothing, which is the same answer it gives for a symbol that has
    /// no definition. Only this tells the two apart.
    #[must_use]
    pub fn working_on(&self) -> Option<&str> {
        self.working.values().next().map(String::as_str)
    }

    /// Everything the server has said is wrong since this was last asked.
    pub fn take_published(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.published)
    }

    /// Every edit the server has asked for since this was last asked.
    ///
    /// Each one is a question still open: whoever takes it owes the
    /// server an [`answer_request`](Client::answer_request) saying what
    /// became of it.
    pub fn take_asked_edits(&mut self) -> Vec<AskedEdit> {
        std::mem::take(&mut self.asked)
    }

    /// Sends an answer to something the server asked.
    ///
    /// For the requests this layer cannot answer by itself, which is the
    /// ones that are about the documents rather than about the protocol.
    pub fn answer_request(&mut self, answer: &Value) {
        let _ = self.send(answer);
    }

    /// Sends a request and returns the id its answer will carry.
    pub fn request<P>(&mut self, method: &str, params: &P) -> Result<i64>
    where
        P: Serialize,
    {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))?;
        Ok(id)
    }

    /// Sends a notification, which has no answer.
    pub fn notify<P>(&mut self, method: &str, params: &P) -> Result<()>
    where
        P: Serialize,
    {
        self.send(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
    }

    /// Takes one message from the server.
    ///
    /// Returns the answers the caller asked for. Everything the protocol
    /// needs rather than the caller — the handshake, progress, the server's
    /// own log lines — is dealt with here and reported as `None`.
    pub fn on_message(&mut self, message: &Value) -> Option<Reply> {
        if let Some(method) = message.get("method").and_then(Value::as_str) {
            // A message with a method *and* an id is the server asking
            // obelus something, and the protocol says every request gets
            // an answer. Left unanswered -- which is what this did -- a
            // server that waits for one waits for ever, and the ones that
            // do not wait have still been told nothing: a server asking
            // for its configuration and hearing nothing uses its defaults
            // and never says so.
            if let Some(id) = message.get("id") {
                // Except the one request that is answered by doing
                // something rather than by saying something. Kept for the
                // caller, who has the documents; the answer goes back when
                // they have said what happened.
                if method == "workspace/applyEdit" {
                    let params = &message["params"];
                    self.asked.push(AskedEdit {
                        id: id.clone(),
                        label: params
                            .get("label")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        edit: params.get("edit").cloned().unwrap_or(Value::Null),
                    });
                    return None;
                }
                self.answer(id, method, &message["params"]);
                return None;
            }
            self.on_notification(method, &message["params"]);
            return None;
        }

        let id = message.get("id").and_then(Value::as_i64)?;
        if id == INITIALIZE_ID {
            self.on_initialized(message);
            return None;
        }

        let result = match message.get("error") {
            Some(error) => Err(error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("The server reported an error with no message")
                .to_string()),
            None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
        };
        Some(Reply { id, result })
    }

    /// Which process it is, for whoever has to ask the operating system
    /// about it.
    #[must_use]
    pub fn pid(&self) -> Option<u32> {
        self.process.id()
    }

    /// How long a server gets to go on its own before it is killed.
    ///
    /// Politeness with a deadline: rust-analyzer writes its caches out on a
    /// clean shutdown, which is worth a moment, and no server is worth
    /// hanging the exit on.
    const PATIENCE: std::time::Duration = std::time::Duration::from_millis(500);

    /// Says obelus has stopped waiting for an answer.
    ///
    /// A notification, so there is nothing to wait for and nothing to go
    /// wrong: a server that has already answered ignores it, and one that
    /// is still working on it stops. What it saves is real -- a reader
    /// typing a word asks for a completion per letter, and without this
    /// every one of them is computed in full before being thrown away.
    pub fn cancel(&mut self, request: i64) {
        let _ = self.notify("$/cancelRequest", &json!({ "id": request }));
    }

    /// What obelus would send back for a message from a server.
    ///
    /// The answering itself, without a server to send it to: what is
    /// interesting is the shape of the answer, and a live server cannot be
    /// made to ask an awkward question on demand.
    #[must_use]
    pub fn answered_for_test(message: &Value) -> Option<Value> {
        let method = message.get("method")?.as_str()?;
        let id = message.get("id")?;
        Some(answered(id, method, &message["params"]))
    }

    /// Answers a request the server made of obelus.
    ///
    /// The answers are the smallest legal ones. Saying nothing useful is
    /// allowed and saying nothing at all is not: `null` for a setting
    /// obelus does not have is exactly what the protocol asks a client to
    /// send for a scope it cannot answer for, and a method obelus does not
    /// know gets the error the protocol has for that -- which is an answer
    /// a server can act on, where silence is a server waiting.
    fn answer(&mut self, id: &Value, method: &str, params: &Value) {
        let _ = self.send(&answered(id, method, params));
    }

    /// Asks the server to stop, and stops waiting for it if it will not.
    ///
    /// The request is sent and not waited on. A server with a lot to say on
    /// the way down would answer it late -- gopls flushes a thousand log
    /// lines first -- and what obelus needs is not the answer but that the
    /// `exit` after it has been written.
    pub fn shutdown(&mut self) {
        let _ = self.request("shutdown", &Value::Null);
        let _ = self.notify("exit", &Value::Null);
        // Dropping the sender closes the writer's channel, which ends that
        // task once it has written what it has.
        let (dead, _) = tokio::sync::mpsc::unbounded_channel();
        self.outgoing = dead;

        let until = std::time::Instant::now() + Self::PATIENCE;
        while std::time::Instant::now() < until {
            match self.process.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(10)),
                Err(_) => break,
            }
        }
        let _ = self.process.start_kill();
    }

    fn send(&mut self, message: &Value) -> Result<()> {
        let body = serde_json::to_string(message).context("serialising a message")?;
        // Anything but the handshake waits for the handshake.
        if self.capabilities.is_none()
            && message.get("id").and_then(Value::as_i64) != Some(INITIALIZE_ID)
        {
            self.queued.push(body);
            return Ok(());
        }
        self.sent += 1;
        self.outgoing
            .send(body)
            .context("the writer thread has ended")
    }

    fn send_initialize(&mut self, root: &Path) -> Result<()> {
        let uri = uri_for(root)?;
        let params = InitializeParams {
            process_id: Some(std::process::id()),
            capabilities: client_capabilities(),
            workspace_folders: Some(vec![WorkspaceFolder {
                uri: uri.clone(),
                name: root
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("root")
                    .to_string(),
            }]),
            ..Default::default()
        };
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": INITIALIZE_ID,
            "method": "initialize",
            "params": params,
        }))
    }

    fn on_initialized(&mut self, message: &Value) {
        let Some(result) = message.get("result") else {
            tracing::error!(language = self.language.name(), "the handshake failed");
            return;
        };
        match serde_json::from_value::<InitializeResult>(result.clone()) {
            Ok(result) => {
                self.encoding = result
                    .capabilities
                    .position_encoding
                    .clone()
                    .unwrap_or(PositionEncodingKind::UTF16);
                tracing::info!(
                    language = self.language.name(),
                    encoding = ?self.encoding,
                    "the server is ready"
                );
                self.capabilities = Some(result.capabilities);
            }
            Err(error) => {
                tracing::error!(%error, "the handshake reply made no sense");
                // Ready enough to talk to. Refusing to would leave the server
                // running and unusable.
                self.capabilities = Some(ServerCapabilities::default());
            }
        }

        let _ = self.notify("initialized", &json!({}));
        for body in std::mem::take(&mut self.queued) {
            self.sent += 1;
            let _ = self.outgoing.send(body);
        }
    }

    fn on_notification(&mut self, method: &str, params: &Value) {
        match method {
            "$/progress" => self.on_progress(params),
            // Unasked for, and the whole truth about one file: the last
            // set a server sends replaces whatever it said before.
            "textDocument/publishDiagnostics" => self.published.push(params.clone()),
            "window/logMessage" | "window/showMessage" => {
                if let Some(text) = params.get("message").and_then(Value::as_str) {
                    tracing::debug!(language = self.language.name(), "{text}");
                }
            }
            other => tracing::trace!(language = self.language.name(), "ignoring {other}"),
        }
    }

    fn on_progress(&mut self, params: &Value) {
        let Some(token) = params.get("token").map(ToString::to_string) else {
            return;
        };
        let value = &params["value"];
        match value.get("kind").and_then(Value::as_str) {
            Some("begin") => {
                let title = value
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("working")
                    .to_string();
                self.working.insert(token, title);
            }
            Some("report") => {
                if let Some(message) = value.get("message").and_then(Value::as_str)
                    && let Some(title) = self.working.get_mut(&token)
                {
                    *title = message.to_string();
                }
            }
            Some("end") => {
                self.working.remove(&token);
            }
            _ => {}
        }
    }
}

/// The `file://` URI for a path.
///
/// `url` rather than an escaping written out here, and the protocol's own type
/// parsed from what it produced: `Uri` will read a URI and not make one, and
/// the two halves of making one are exactly the halves that were wrong.
///
/// A URI is not a path with a scheme in front of it. Its parts are separated
/// by `/` where Windows writes `\`, and its path begins with one where a
/// Windows path begins at a drive letter -- so `C:\src\main.rs`, escaped
/// character by character, arrived as `file://C%3A%5Csrc%5Cmain.rs`: one long
/// word in the place a URI keeps the *host*. Every server obelus spoke to on
/// that platform was told about a file on a machine that does not exist.
///
/// Made absolute first, because `Url::from_file_path` will not take anything
/// else and is right not to: `ob src/main.rs` opens a buffer called what the
/// reader typed, and `file://src/main.rs` reads `src` as a host the same way.
/// `std::path::absolute` rather than `canonicalize`, which would touch the
/// disk and hand back the file a link points at -- a different path from the
/// one every other part of obelus knows this buffer by.
pub fn uri_for(path: &Path) -> Result<Uri> {
    use std::str::FromStr as _;

    let whole =
        std::path::absolute(path).with_context(|| format!("making {} absolute", path.display()))?;
    let url = url::Url::from_file_path(&whole)
        .map_err(|()| anyhow::anyhow!("no uri names {}", whole.display()))?;
    Uri::from_str(url.as_str()).with_context(|| format!("building a uri from {}", path.display()))
}

/// The path a `file://` URI names.
///
/// The inverse of [`uri_for`], and next to it: an escaping and an unescaping
/// that disagree name a different file, and the two are only correct together.
/// Which is the other reason both are `url`'s now -- one crate holding both
/// halves cannot disagree with itself.
///
/// `None` for a URI that names no file on this machine: another scheme, or a
/// host, which is somebody else's disk.
///
/// What comes back is a path this platform would have written itself --
/// `C:\src\main.rs` rather than `/C:/src/main.rs`. These are compared against
/// paths a walk of the tree found and drawn on the rows beside them, and a
/// second spelling of one file is a file obelus opens twice.
#[must_use]
pub fn path_of(uri: &str) -> Option<std::path::PathBuf> {
    url::Url::parse(uri).ok()?.to_file_path().ok()
}

/// Reads messages and puts them on the one channel the loop reads.
///
/// Two halves on purpose. The framing is only waiting on a pipe, so it is a
/// task and costs no thread. The parsing is work -- semantic tokens for a
/// two-thousand-line file is 473 KiB of JSON and about twenty milliseconds
/// -- and doing it on the runtime would stall the agent's connection, the
/// clock behind an animation and every other server behind it, which is
/// exactly what separate threads never did.
fn spawn_reader(
    language: LanguageId,
    stdout: tokio::process::ChildStdout,
    sender: impl Sink<Message>,
) {
    crate::runtime::handle().spawn(async move {
        let mut reader = tokio::io::BufReader::new(stdout);
        loop {
            let body = match transport::read_body(&mut reader).await {
                Ok(Some(body)) => body,
                Ok(None) => {
                    tracing::info!(language = language.name(), "the server closed its output");
                    return;
                }
                Err(error) => {
                    tracing::warn!(%error, language = language.name(), "reading from the server");
                    return;
                }
            };
            let parsed = crate::runtime::handle()
                .spawn_blocking(move || serde_json::from_slice::<serde_json::Value>(&body))
                .await;
            let message = match parsed {
                Ok(Ok(message)) => message,
                Ok(Err(error)) => {
                    tracing::warn!(%error, language = language.name(), "a message obelus could not read");
                    continue;
                }
                Err(error) => {
                    tracing::warn!(%error, language = language.name(), "parsing gave up");
                    return;
                }
            };
            if sender.send(Message { language, message }).is_err() {
                return;
            }
        }
    });
}

/// Writes messages, on a task of its own.
///
/// Its own task because writing blocks: a server busy indexing stops
/// draining its stdin, and a write from the main loop would hold the whole
/// interface until it started again. An unbounded channel in front of it,
/// so the loop's side of a send never waits.
fn spawn_writer(
    command: String,
    stdin: tokio::process::ChildStdin,
) -> tokio::sync::mpsc::UnboundedSender<String> {
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<String>();
    crate::runtime::handle().spawn(async move {
        use tokio::io::AsyncWriteExt as _;

        let mut writer = tokio::io::BufWriter::new(stdin);
        while let Some(body) = receiver.recv().await {
            if let Err(error) = writer.write_all(transport::framed(&body).as_bytes()).await {
                tracing::warn!(%error, %command, "writing to the server");
                return;
            }
            if let Err(error) = writer.flush().await {
                tracing::warn!(%error, %command, "writing to the server");
                return;
            }
        }
    });
    sender
}

/// Sends whatever the server writes to its stderr to the log.
fn spawn_logger(command: String, stderr: tokio::process::ChildStderr) {
    crate::runtime::handle().spawn(async move {
        use tokio::io::AsyncBufReadExt as _;

        let mut lines = tokio::io::BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            tracing::debug!(%command, "{line}");
        }
    });
}

impl Drop for Client {
    /// Ends the process when the client goes.
    ///
    /// `Child` does not do this itself: dropping one leaves the process
    /// running, deliberately, because a child outliving its parent is the
    /// usual thing to want -- it is what every daemon ever started from a
    /// shell depends on. It is not the thing to want here, and every other
    /// editor says so in its own words: helix spawns with tokio's
    /// `kill_on_drop`, zed with `async-process`'s. obelus now spawns with
    /// tokio too and could ask for the same flag; it says it here instead,
    /// because a server is not killed on the way out -- it is told to shut
    /// down, and that is a conversation rather than a signal.
    ///
    /// Here rather than only where a server is stopped on purpose, because
    /// "on purpose" was never the leak: it is every other way a client goes
    /// -- the application ending, a test finishing, one server replacing
    /// another -- and a leaked rust-analyzer is a quarter of a gigabyte
    /// holding an index of a project nobody is reading.
    fn drop(&mut self) {
        // Already gone, and `shutdown` would only be talking to a pipe with
        // nobody on the other end.
        if matches!(self.process.try_wait(), Ok(Some(_))) {
            return;
        }
        self.shutdown();
    }
}

/// The answer obelus sends for one of the server's own requests.
///
/// The answers are the smallest legal ones. Saying nothing useful is
/// allowed and saying nothing at all is not: `null` for a setting obelus
/// does not have is exactly what the protocol asks a client to send for a
/// scope it cannot answer for.
///
/// A free function so that a test can read the answer without a server to
/// send it to: the shape of the answer is the whole of what is
/// interesting, and a live server cannot be made to ask an awkward
/// question on demand.
fn answered(id: &Value, method: &str, params: &Value) -> Value {
    let result = match method {
        // One entry per item asked about, all of them nothing: obelus
        // keeps no per-server settings, and a shorter array than the
        // question is a malformed answer.
        "workspace/configuration" => {
            let items = params
                .get("items")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            Some(Value::Array(vec![Value::Null; items]))
        }
        // Acknowledged and nothing more. obelus declares no dynamic
        // registration, so a server should not be asking; one that asks
        // anyway is told yes rather than left hanging.
        "client/registerCapability" | "client/unregisterCapability" => Some(Value::Null),
        // A progress token the server wants to use, which obelus reads
        // from the notifications it already handles.
        "window/workDoneProgress/create" => Some(Value::Null),
        // A message with buttons on it. obelus has nowhere to put the
        // buttons, and `null` is the protocol's word for "the reader
        // pressed none of them".
        "window/showMessageRequest" => Some(Value::Null),
        // `workspace/applyEdit` is not here: it is answered by making the
        // edit, so [`Client::on_message`] keeps it for the side that has
        // the documents and the answer goes back through [`edit_answer`].
        other => {
            tracing::debug!(method = other, "a request obelus has no answer for");
            None
        }
    };
    match result {
        Some(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        // -32601 is the protocol's "method not found", which is what this
        // is: an answer a server can act on, where silence is a server
        // waiting.
        None => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": format!("obelus does not answer {method}") },
        }),
    }
}

/// What obelus sends back for an edit a server asked it to make.
///
/// The protocol asks for a plain yes or no, and a reason when it is no.
/// Saying yes to an edit that did not happen is the failure worth
/// avoiding: a server told its refactoring landed goes on to the next
/// step of it.
#[must_use]
pub fn edit_answer(id: &Value, applied: bool, why: &str) -> Value {
    let mut result = json!({ "applied": applied });
    if !applied {
        result["failureReason"] = Value::String(why.to_string());
    }
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// What obelus tells a server it can do.
///
/// Every line of this is load-bearing and the way it fails is silence: a
/// server does not complain about a capability that is missing, it
/// quietly answers less. The one that cost the most was `code_action` --
/// undeclared, rust-analyzer answers `null` to every
/// `textDocument/codeAction`, whatever the file and wherever the range,
/// so the key that asks what can be done here had nothing to show
/// anywhere.
///
/// That was one instance of a general mistake, which is why this is now
/// written from the other end: every entry here is something obelus's own
/// code reads, and the doc comment says which code. A capability declared
/// and not used invites answers nobody looks at; one used and not
/// declared is a feature that is written, tested, and never reached --
/// and which server it goes missing on differs by server, so no one
/// server finds them all.
#[must_use]
pub fn client_capabilities() -> ClientCapabilities {
    let markup = || Some(vec![MarkupKind::Markdown, MarkupKind::PlainText]);
    ClientCapabilities {
        general: Some(GeneralClientCapabilities {
            // Bytes first. A server that agrees makes an LSP position the
            // same thing as a tree-sitter point, and the whole UTF-16 path
            // becomes a fallback nothing exercises.
            position_encodings: Some(vec![
                PositionEncodingKind::UTF8,
                PositionEncodingKind::UTF16,
            ]),
            ..Default::default()
        }),
        text_document: Some(TextDocumentClientCapabilities {
            // `didOpen`, `didChange`, `didSave`, `didClose`. Not the two
            // `willSave` messages: obelus has nothing to say before a
            // write, and `willSaveWaitUntil` is a server being allowed to
            // hold up the save.
            synchronization: Some(TextDocumentSyncClientCapabilities {
                did_save: Some(true),
                ..Default::default()
            }),
            // Read by `lsp::complete`. Snippets because obelus has an
            // engine for them (`lsp::snippet`), and a server that has not
            // been told may not send one; label details because the offer
            // list draws them; insert-and-replace because a completion in
            // the middle of a word replaces the word.
            completion: Some(CompletionClientCapabilities {
                completion_item: Some(CompletionItemCapability {
                    snippet_support: Some(true),
                    label_details_support: Some(true),
                    insert_replace_support: Some(true),
                    documentation_format: markup(),
                    // What `completionItem/resolve` is asked for: the
                    // three things an offer may arrive without.
                    resolve_support: Some(CompletionItemCapabilityResolveSupport {
                        properties: vec![
                            "documentation".to_string(),
                            "detail".to_string(),
                            "additionalTextEdits".to_string(),
                        ],
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            // Rendered as markdown, by the same renderer a README gets.
            hover: Some(HoverClientCapabilities {
                content_format: markup(),
                ..Default::default()
            }),
            // The panel marks the parameter the cursor is in, which it
            // can only do from offsets: given the parameter as a piece of
            // text instead, obelus has to find it in the label, and a
            // name that appears twice is found in the wrong place.
            signature_help: Some(SignatureHelpClientCapabilities {
                signature_information: Some(SignatureInformationSettings {
                    documentation_format: markup(),
                    parameter_information: Some(ParameterInformationSettings {
                        label_offset_support: Some(true),
                    }),
                    active_parameter_support: Some(true),
                }),
                ..Default::default()
            }),
            // Nesting, for the outline. Without this the protocol says a
            // server *may* answer `documentSymbol` with the flat shape,
            // and rust-analyzer does: every symbol at the top level, and
            // each one's position the start of the whole item rather than
            // of its name -- so a mark on it lands on the line above, on
            // an attribute or a doc comment.
            document_symbol: Some(DocumentSymbolClientCapabilities {
                hierarchical_document_symbol_support: Some(true),
                ..Default::default()
            }),
            // The three jumps, and `references` beside them. Link support
            // because `lsp::action` reads a `LocationLink`'s `targetUri`,
            // and because the link is the shape that carries the name's
            // own range rather than the whole definition's.
            definition: Some(GotoCapability {
                link_support: Some(true),
                ..Default::default()
            }),
            type_definition: Some(GotoCapability {
                link_support: Some(true),
                ..Default::default()
            }),
            implementation: Some(GotoCapability {
                link_support: Some(true),
                ..Default::default()
            }),
            references: Some(ReferenceClientCapabilities::default()),
            document_highlight: Some(DocumentHighlightClientCapabilities::default()),
            formatting: Some(DocumentFormattingClientCapabilities::default()),
            rename: Some(RenameClientCapabilities::default()),
            // The whole file at once, in the packed form, with the
            // server's own legend: `lsp::tokens` reads the names out of
            // it rather than assuming the standard set, so what is
            // declared here is the floor and anything past it still
            // works. Not the range request: obelus asks for a file once
            // and keeps the answer.
            semantic_tokens: Some(SemanticTokensClientCapabilities {
                requests: SemanticTokensClientCapabilitiesRequests {
                    full: Some(SemanticTokensFullOptions::Bool(true)),
                    range: Some(false),
                },
                token_types: standard_token_types(),
                token_modifiers: Vec::new(),
                formats: vec![TokenFormat::RELATIVE],
                ..Default::default()
            }),
            // Where the colours are written down. Declared for the reason
            // everything here is: a server that has not been told asks
            // itself whether to bother, and the one that answers this is
            // the one a reader opens a stylesheet with.
            color_provider: Some(DocumentColorClientCapabilities::default()),
            // Who calls this, and what it calls. Two requests behind one
            // capability, which is the protocol's own arrangement: the
            // item a server prepares is what both of them are asked with.
            call_hierarchy: Some(CallHierarchyClientCapabilities::default()),
            // What a server would have the reader know that the file does
            // not say. Undeclared, a server is entitled to answer nothing,
            // and a reader would be reading the file rather than the one
            // the compiler has.
            inlay_hint: Some(InlayHintClientCapabilities::default()),
            // The diagnostic obelus keeps is the one the server sent,
            // whole, because it goes back in a code action's context and
            // the server matches it by every field -- `data` included.
            publish_diagnostics: Some(PublishDiagnosticsClientCapabilities {
                data_support: Some(true),
                ..Default::default()
            }),
            // Without this a server is entitled to answer
            // `textDocument/codeAction` with commands only, and
            // rust-analyzer does something stronger: it answers `null` to
            // every such request, whatever the file and wherever the
            // range.
            code_action: Some(CodeActionClientCapabilities {
                code_action_literal_support: Some(CodeActionLiteralSupport {
                    code_action_kind: CodeActionKindLiteralSupport {
                        value_set: [
                            CodeActionKind::EMPTY,
                            CodeActionKind::QUICKFIX,
                            CodeActionKind::REFACTOR,
                            CodeActionKind::REFACTOR_EXTRACT,
                            CodeActionKind::REFACTOR_INLINE,
                            CodeActionKind::REFACTOR_REWRITE,
                            CodeActionKind::SOURCE,
                            CodeActionKind::SOURCE_ORGANIZE_IMPORTS,
                            CodeActionKind::SOURCE_FIX_ALL,
                        ]
                        .iter()
                        .map(|kind| kind.as_str().to_string())
                        .collect(),
                    },
                }),
                // Both read: the first is what sorts the list, and the
                // second is what an offer that arrived without its edit
                // is recognised by.
                is_preferred_support: Some(true),
                data_support: Some(true),
                resolve_support: Some(CodeActionCapabilityResolveSupport {
                    properties: vec!["edit".to_string(), "command".to_string()],
                }),
                // The offers a server knows cannot be taken here, each
                // with a reason. Worth asking for: without it a server
                // leaves them out, and a reader never learns that
                // "extract into a function" exists because the one time
                // they would have wanted it their selection crossed a
                // `?`. The reason is the row's own detail, and the list
                // steps over the row.
                disabled_support: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        }),
        workspace: Some(WorkspaceClientCapabilities {
            // obelus makes the edits a server asks for, which a server
            // that has not been told this will never ask for: the whole
            // path from `workspace/executeCommand` to a refactoring
            // landing in the files goes through one request, and this is
            // what permits it.
            apply_edit: Some(true),
            workspace_edit: Some(WorkspaceEditClientCapabilities {
                document_changes: Some(true),
                // Empty on purpose, and it is the same policy the code
                // enforces: obelus will not create, move or delete a file
                // because a server suggested it. Said here, a server has
                // the chance not to ask.
                resource_operations: Some(Vec::new()),
                failure_handling: Some(FailureHandlingKind::Abort),
                ..Default::default()
            }),
            // Each of these is a message obelus sends.
            symbol: Some(WorkspaceSymbolClientCapabilities::default()),
            execute_command: Some(ExecuteCommandClientCapabilities::default()),
            did_change_watched_files: Some(DidChangeWatchedFilesClientCapabilities::default()),
            // A file that moves takes its meaning with it, and the server
            // is the only thing that knows which other files said that
            // meaning out loud. Asked for both: `willRename` is the
            // question whose answer is an edit, and `didRename` is how a
            // server that registered only the second still learns the
            // file is gone from where it was.
            //
            // Not `willCreate` or `willDelete`: obelus does neither.
            file_operations: Some(WorkspaceFileOperationsClientCapabilities {
                will_rename: Some(true),
                did_rename: Some(true),
                ..Default::default()
            }),
            // Not `configuration`: obelus keeps no per-server settings,
            // so a server that asked would be asked to wait for an answer
            // of nulls. It is answered when it comes anyway, because the
            // protocol says every request is.
            ..Default::default()
        }),
        window: Some(WindowClientCapabilities {
            work_done_progress: Some(true),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// The token types the protocol itself names.
///
/// A floor rather than a list: `lsp::tokens` reads the server's own
/// legend by name and treats anything it does not know as a name, which
/// is how `lifetime` and `builtinType` and a dozen kinds of punctuation
/// work without being here.
fn standard_token_types() -> Vec<SemanticTokenType> {
    vec![
        SemanticTokenType::NAMESPACE,
        SemanticTokenType::TYPE,
        SemanticTokenType::CLASS,
        SemanticTokenType::ENUM,
        SemanticTokenType::INTERFACE,
        SemanticTokenType::STRUCT,
        SemanticTokenType::TYPE_PARAMETER,
        SemanticTokenType::PARAMETER,
        SemanticTokenType::VARIABLE,
        SemanticTokenType::PROPERTY,
        SemanticTokenType::ENUM_MEMBER,
        SemanticTokenType::EVENT,
        SemanticTokenType::FUNCTION,
        SemanticTokenType::METHOD,
        SemanticTokenType::MACRO,
        SemanticTokenType::KEYWORD,
        SemanticTokenType::MODIFIER,
        SemanticTokenType::COMMENT,
        SemanticTokenType::STRING,
        SemanticTokenType::NUMBER,
        SemanticTokenType::REGEXP,
        SemanticTokenType::OPERATOR,
        SemanticTokenType::DECORATOR,
    ]
}

#[cfg(test)]
mod tests {
    use super::{path_of, uri_for};

    /// A path spelled the way this platform spells one survives being said
    /// to a server and read back.
    ///
    /// Built from the directory obelus is running in rather than written
    /// out, because a written-out path is a unix path and a unix path is
    /// exactly the case that was never broken: on Windows every file obelus
    /// named arrived as one escaped word where the host goes, and every
    /// server answered about a file it could not find.
    ///
    /// Broken deliberately, once for each half. Leaving `\` between the
    /// parts escapes it to `%5C`, and the URI stops saying `/src/lsp/` --
    /// the parts of a path a server has to be able to see. Leaving the
    /// leading `/` off puts the drive in the authority, and the URI stops
    /// starting `file:///`. Either one alone still round-trips, which is
    /// why neither is checked by the round trip.
    #[test]
    fn a_path_of_this_platform_survives_being_a_uri() {
        let here = std::env::current_dir()
            .expect("a working directory")
            .join("src")
            .join("lsp")
            .join("client.rs");

        let uri = uri_for(&here).expect("a uri");
        assert!(
            uri.as_str().starts_with("file:///"),
            "the path went where a URI keeps the host: {uri:?}"
        );
        assert!(
            uri.as_str().ends_with("/src/lsp/client.rs"),
            "the parts are not separated the way a URI separates them: {uri:?}"
        );
        assert_eq!(
            path_of(uri.as_str()).as_deref(),
            Some(here.as_path()),
            "the path that came back is not the one that went out"
        );
    }
}
