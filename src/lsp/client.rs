//! One language server, and the bookkeeping the protocol needs.
//!
//! This layer knows the handshake, the request numbering and the progress
//! notifications. It does not know what any particular request was *for*:
//! that is the caller's, because the answer arrives after the world has moved
//! on and only the caller can say whether it still means anything.

use std::{
    collections::HashMap,
    io::{BufReader, BufWriter},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Sender},
};

use anyhow::{Context as _, Result};
use lsp_types::{
    ClientCapabilities, DocumentSymbolClientCapabilities, GeneralClientCapabilities,
    InitializeParams, InitializeResult, PositionEncodingKind, ServerCapabilities,
    TextDocumentClientCapabilities, Uri, WindowClientCapabilities, WorkspaceFolder,
};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{event::Event, lsp::transport, syntax::LanguageId};

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

/// A running language server.
pub struct Client {
    language: LanguageId,
    process: Child,
    /// Whether the process has been found to have stopped.
    exited: bool,
    outgoing: Sender<String>,
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
        sender: Sender<Event>,
    ) -> Result<Self> {
        let command = server.command;
        let mut process = Command::new(command)
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
                .unwrap_or("the server reported an error with no message")
                .to_string()),
            None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
        };
        Some(Reply { id, result })
    }

    /// Which process it is, for whoever has to ask the operating system
    /// about it.
    #[must_use]
    pub fn pid(&self) -> Option<u32> {
        Some(self.process.id())
    }

    /// How long a server gets to go on its own before it is killed.
    ///
    /// Politeness with a deadline: rust-analyzer writes its caches out on a
    /// clean shutdown, which is worth a moment, and no server is worth
    /// hanging the exit on.
    const PATIENCE: std::time::Duration = std::time::Duration::from_millis(500);

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
        // thread once it has written what it has.
        let (dead, _) = mpsc::channel();
        self.outgoing = dead;

        let until = std::time::Instant::now() + Self::PATIENCE;
        while std::time::Instant::now() < until {
            match self.process.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(10)),
                Err(_) => break,
            }
        }
        let _ = self.process.kill();
        let _ = self.process.wait();
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
            capabilities: ClientCapabilities {
                general: Some(GeneralClientCapabilities {
                    // Bytes first. A server that agrees makes an LSP position
                    // the same thing as a tree-sitter point, and the whole
                    // UTF-16 path becomes a fallback nothing exercises.
                    position_encodings: Some(vec![
                        PositionEncodingKind::UTF8,
                        PositionEncodingKind::UTF16,
                    ]),
                    ..Default::default()
                }),
                text_document: Some(TextDocumentClientCapabilities {
                    // Nesting, for the outline. Without this the protocol
                    // says a server *may* answer `documentSymbol` with the
                    // flat shape, and rust-analyzer does: every symbol at
                    // the top level, and each one's position the start of
                    // the whole item rather than of its name -- so a mark on
                    // it lands on the line above, on an attribute or a doc
                    // comment. Declared support turns the same request into
                    // a tree of names.
                    document_symbol: Some(DocumentSymbolClientCapabilities {
                        hierarchical_document_symbol_support: Some(true),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                window: Some(WindowClientCapabilities {
                    work_done_progress: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            },
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
/// Built by hand because the protocol's own type will parse one but not make
/// one out of a path. Anything a URI reserves has to be escaped, or a path
/// with a space or a `#` in it names a different file — or no file at all.
pub fn uri_for(path: &Path) -> Result<Uri> {
    use std::str::FromStr as _;

    let mut text = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        match byte {
            b'/' | b'-' | b'_' | b'.' | b'~' => text.push(char::from(byte)),
            _ if byte.is_ascii_alphanumeric() => text.push(char::from(byte)),
            _ => text.push_str(&format!("%{byte:02X}")),
        }
    }
    Uri::from_str(&text).with_context(|| format!("building a uri from {}", path.display()))
}

/// The path a `file://` URI names.
///
/// The inverse of [`uri_for`], and next to it: an escaping and an unescaping
/// that disagree name a different file, and the two are only correct together.
#[must_use]
pub fn path_of(uri: &str) -> Option<std::path::PathBuf> {
    let encoded = uri.strip_prefix("file://")?;
    let bytes = encoded.as_bytes();
    let mut path = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok()?;
            path.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            path.push(bytes[index]);
            index += 1;
        }
    }
    Some(std::path::PathBuf::from(String::from_utf8(path).ok()?))
}

/// Reads messages and puts them on the one channel the loop reads.
fn spawn_reader(language: LanguageId, stdout: std::process::ChildStdout, sender: Sender<Event>) {
    let _ = std::thread::Builder::new()
        .name(format!("obelus-lsp-{}", language.name()))
        .spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match transport::read_message(&mut reader) {
                    Ok(Some(message)) => {
                        if sender.send(Event::Lsp { language, message }).is_err() {
                            return;
                        }
                    }
                    Ok(None) => {
                        tracing::info!(language = language.name(), "the server closed its output");
                        return;
                    }
                    Err(error) => {
                        tracing::warn!(%error, language = language.name(), "reading from the server");
                        return;
                    }
                }
            }
        });
}

/// Writes messages, on its own thread.
///
/// Its own thread because writing blocks: a server busy indexing stops
/// draining its stdin, and a write from the main loop would hold the whole
/// interface until it started again.
fn spawn_writer(command: String, stdin: std::process::ChildStdin) -> Sender<String> {
    let (sender, receiver) = mpsc::channel::<String>();
    let _ = std::thread::Builder::new()
        .name("obelus-lsp-write".to_string())
        .spawn(move || {
            let mut writer = BufWriter::new(stdin);
            while let Ok(body) = receiver.recv() {
                if let Err(error) = transport::write_message(&mut writer, &body) {
                    tracing::warn!(%error, %command, "writing to the server");
                    return;
                }
            }
        });
    sender
}

/// Sends whatever the server writes to its stderr to the log.
fn spawn_logger(command: String, stderr: std::process::ChildStderr) {
    let _ = std::thread::Builder::new()
        .name("obelus-lsp-stderr".to_string())
        .spawn(move || {
            use std::io::BufRead as _;
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
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
    /// `kill_on_drop`, zed with `async-process`'s. obelus spawns with the
    /// standard library, which has neither, so it says it here.
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
