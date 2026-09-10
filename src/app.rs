//! Application state, and the loop that drives it.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Terminal,
    backend::Backend,
    buffer::Buffer as CellBuffer,
    layout::{Position, Rect},
};

use crate::{
    buffer::{Buffer, BufferId, Cursor, Mode, Motion, TextArea},
    command::{Command, Requires, dispatch},
    component::{
        picker::{Colouring, Picker, PickerItem, PickerLayout, PickerOutcome, PickerValue, files},
        prompt::{Prompt, PromptKind, PromptOutcome},
    },
    coordinates::{ByteOffset, CharColumn, LineNumber, Span},
    event::{self, Event, Ticker},
    git, icons,
    jump::{Jump, JumpList},
    keymap::{self, Context, KeyChord, Keymap},
    lsp::{
        self,
        action::{self, Outcome, SymbolAction},
        client::{Client, Reply},
        position,
    },
    markdown,
    search::{self, Scope},
    syntax::{LanguageId, brackets, highlight::Highlights, parse::SyntaxState, tags},
    theme::{Theme, builtin},
    ui,
    watch::Watcher,
};

/// How many rows a compact picker may take.
///
/// It sits on the status bar and grows upwards only as far as it has to, so
/// the code stays visible. Both pickers over a fixed, short list work that
/// way: they are used while looking at something, and covering the something
/// would be the wrong trade.
const COMPACT_ROWS: u16 = 10;

/// How many already-ready events to fold into one frame.
///
/// A held-down arrow key or a scroll burst delivers faster than a terminal can
/// usefully be redrawn; draining what is ready before drawing turns a burst
/// into one frame. The cap keeps a pathological producer from starving the
/// draw entirely.
const EVENT_DRAIN_LIMIT: usize = 256;

/// Everything obelus is currently showing or remembering.
#[derive(Debug)]
pub struct App {
    keymap: Keymap,
    /// Every file opened this session, with a hole where one has been closed.
    ///
    /// Holes rather than removal, because [`BufferId`] is an index and the
    /// jump list, the pending questions and `current` all hold one. Removing
    /// an element would leave every id above it pointing at a *different*
    /// file, which is the kind of wrong that shows up as the wrong file
    /// opening a week later. A closed slot makes a stale id dead instead:
    /// whoever holds it gets nothing and does nothing.
    buffers: Vec<Option<Buffer>>,
    current: Option<BufferId>,
    theme: &'static Theme,
    /// The open picker, if one is open.
    ///
    /// One field for all four, because they differ only in what they list.
    picker: Option<Picker>,
    /// Which file walk the picker is currently expecting batches from.
    ///
    /// Bumped every time a file picker opens, so batches from a walk whose
    /// picker has already closed are recognizable and dropped.
    walk_generation: u64,
    /// Sender for the background walk, once the loop has started.
    events: Option<std::sync::mpsc::Sender<Event>>,
    /// One language server per language, started when a file of that language
    /// is first opened.
    servers: HashMap<LanguageId, Client>,
    /// Languages whose server has been stopped on purpose.
    ///
    /// Without this, opening the next file of that language starts it again,
    /// and a reader who stopped rust-analyzer because it was eating the
    /// machine would find it back a moment later with nothing saying why.
    stopped: HashSet<LanguageId>,
    /// What each question that is still out was about.
    ///
    /// Kept here rather than in the client because the answer arrives after
    /// the world has moved on, and only this side can say whether it still
    /// means anything.
    ///
    /// Keyed by the server as well as the request id, because ids are each
    /// server's own and start again from zero. Two servers running at once,
    /// or one restarted, otherwise hand out the same id twice, and the second
    /// answer would be matched to the first question.
    asked: HashMap<(LanguageId, i64), Question>,
    /// Where the reader has been.
    jumps: JumpList,
    /// The file the picker's selection names, opened so it can be shown.
    ///
    /// Keyed by path: moving through a list reads each file once as it is
    /// passed, and moving back to one that is still selected reads nothing.
    preview: Option<Preview>,
    /// How far along the welcome screen's colours have travelled.
    ///
    /// One number, advanced by a tick. The wordmark is the only thing that
    /// reads it, and it reads it as an offset into a repeating ramp, so it
    /// can grow forever and wrap on its own.
    phase: u32,
    /// A question on the status bar, while one is being asked.
    ///
    /// Not a picker: a prompt has nothing to list, and going through a
    /// picker to ask for a line number puts a region of screen over the code
    /// to hold one row that says "type a line number".
    prompt: Option<Prompt>,
    /// What has changed in the current file since the last commit.
    ///
    /// Kept here rather than in the buffer for the same reason the
    /// highlights are: it is a function of the file's text and nothing else,
    /// and re-deriving it when the text changes is simpler than keeping a
    /// buffer's copy of it right. Re-derived means asking git again, so it
    /// happens when a file is opened or reloaded -- which is exactly when
    /// what changed can have changed -- and not per frame.
    changes: Option<Changed>,
    /// The hunk the reader has opened in place, if any.
    ///
    /// The line it is anchored to. Held here rather than in the buffer
    /// because it is about a *view* of the file, like the markdown
    /// rendering, and closing it must not need the file.
    opened: Option<LineNumber>,
    /// The current file laid out as markdown, if it is being shown that way.
    ///
    /// Kept here rather than in the buffer for the same reason the
    /// highlights are: it is a function of the text, the width and nothing
    /// else, and re-deriving it when either changes is simpler than keeping
    /// a buffer's copy of it right.
    markdown: Option<Rendered>,
    /// The theme to go back to if the theme picker is cancelled.
    ///
    /// Set while that picker is open, because moving through it *applies*
    /// each theme: a list of colour names is not a choice between colour
    /// schemes, and the only honest preview of a theme is the screen wearing
    /// it. Cancelling has to undo that.
    theme_before: Option<&'static Theme>,
    /// The thread sending ticks, while anything wants them.
    ///
    /// Held so that dropping it stops the animation. There is nothing to
    /// animate once a file is open, and nothing over a network at all.
    ticker: Option<Ticker>,
    /// What git says about the files in the tree, while a list of them is
    /// open.
    ///
    /// Gathered when a list opens and kept until the next one, because it is
    /// a walk of the whole tree and the rows arrive in batches afterwards.
    statuses: std::collections::HashMap<PathBuf, git::FileStatus>,
    /// Who last changed each line, per file that has been asked about.
    ///
    /// Kept rather than replaced, because a reader goes back and forth
    /// between two files and a blame is a walk of history: asking again for
    /// one they left a moment ago would spend that walk twice. Bounded by
    /// the files opened in a session, which is tens of them.
    blames: std::collections::HashMap<PathBuf, Vec<Option<git::Blamed>>>,
    /// Which files have been asked about and have not answered yet, so a
    /// frame does not start a second walk of the same history.
    asking_blame: std::collections::HashSet<PathBuf>,
    /// Whether to show it at all.
    showing_blame: bool,
    /// Files parsed only to colour a search's rows.
    ///
    /// A search of a project answers with lines from files that are not
    /// open, and a line reads like code only if something has parsed the
    /// file it is a line of. Filled for the rows on screen and dropped when
    /// the list closes: this is a cache for one list's lifetime, not a
    /// second set of buffers.
    row_syntax: std::collections::HashMap<PathBuf, Buffer>,
    /// Which file and version the search's rows were gathered from.
    ///
    /// The file scope's rows are its lines, so they are only right for the
    /// version they were read from: an agent rewriting the file while the
    /// search is open has to change what the list says.
    searched: Option<(PathBuf, i32)>,
    /// Which search the answers arriving belong to.
    ///
    /// Bumped on every keystroke that changes what is being asked, so the
    /// batches for the query before it are recognizable as stale. Shared
    /// with the scanning threads, which read it to find out that they are
    /// answering a question nobody is asking any more.
    search_generation: std::sync::Arc<std::sync::atomic::AtomicU64>,
    /// Something to tell the reader, until the next key.
    ///
    /// Half of what a language server does is answer with nothing, and
    /// nothing is invisible: without somewhere to say "no definition found" or
    /// "still indexing", pressing the key looks like the key not working.
    note: Option<String>,
    /// The file watcher, once started.
    ///
    /// Held because dropping it stops the watch. `None` means auto-reload is
    /// unavailable — the watcher failed to start, most likely against an
    /// inotify limit — and `file.reload` still works.
    watcher: Option<Watcher>,
    /// The highlight kinds for what is on screen.
    ///
    /// Recomputed every frame from the tree and kept here so the allocation is
    /// reused. Nothing invalidates it, because it is never stale: a reparse
    /// changes the tree and the next frame reads the new one.
    highlights: Highlights,
    /// Where the text was drawn last frame.
    ///
    /// Paging and horizontal scrolling need the size of the text area, which
    /// only the layout knows. Keeping last frame's is right: on the first
    /// frame there is nothing to page through yet, and afterwards it is the
    /// geometry the user is looking at.
    editor_area: Rect,
    working_directory: PathBuf,
    should_quit: bool,
}

impl App {
    /// Starts with the shipped key table and the given documents open.
    #[must_use]
    pub fn new(buffers: Vec<Buffer>) -> Self {
        let current = (!buffers.is_empty()).then(|| BufferId::new(0));
        let buffers: Vec<Option<Buffer>> = buffers.into_iter().map(Some).collect();
        Self {
            keymap: Keymap::new(),
            buffers,
            current,
            theme: &builtin::DARK,
            picker: None,
            servers: HashMap::new(),
            stopped: HashSet::new(),
            asked: HashMap::new(),
            jumps: JumpList::default(),
            preview: None,
            phase: 0,
            ticker: None,
            prompt: None,
            changes: None,
            opened: None,
            statuses: std::collections::HashMap::new(),
            blames: std::collections::HashMap::new(),
            asking_blame: std::collections::HashSet::new(),
            showing_blame: true,
            row_syntax: std::collections::HashMap::new(),
            searched: None,
            search_generation: std::sync::Arc::default(),
            markdown: None,
            theme_before: None,
            note: None,
            walk_generation: 0,
            events: None,
            watcher: None,
            highlights: Highlights::default(),
            editor_area: Rect::ZERO,
            // Read once. Nothing later asks the operating system again, so
            // every path obelus shows is relative to the same root for the
            // whole session even if something else changes the process's
            // directory.
            working_directory: std::env::current_dir().unwrap_or_default(),
            should_quit: false,
        }
    }

    /// Whether the loop should stop.
    #[must_use]
    pub const fn should_quit(&self) -> bool {
        self.should_quit
    }

    /// Asks the loop to stop after this iteration.
    pub const fn request_quit(&mut self) {
        self.should_quit = true;
    }

    /// The bindings currently in force.
    #[must_use]
    pub const fn keymap(&self) -> &Keymap {
        &self.keymap
    }

    /// Replaces the key table.
    ///
    /// The point of the table being data. A configuration file becomes a
    /// caller of this rather than a change to how lookup works.
    pub fn set_keymap(&mut self, keymap: Keymap) {
        self.keymap = keymap;
    }

    /// The colours currently in force.
    #[must_use]
    pub const fn theme(&self) -> &'static Theme {
        self.theme
    }

    /// Switches theme.
    ///
    /// Takes effect on the next frame and costs nothing else: what is cached
    /// per byte is which kind of thing it is, not what colour, so there is no
    /// reparse and nothing to invalidate.
    pub const fn set_theme(&mut self, theme: &'static Theme) {
        self.theme = theme;
    }

    /// The highlight kinds for what is on screen.
    #[must_use]
    pub const fn highlights(&self) -> &Highlights {
        &self.highlights
    }

    /// Where obelus was started, and the root every path is shown relative to.
    #[must_use]
    pub fn working_directory(&self) -> &Path {
        &self.working_directory
    }

    /// The document being read, if any is open.
    #[must_use]
    pub fn current_buffer(&self) -> Option<&Buffer> {
        self.buffers.get(self.current?.get())?.as_ref()
    }

    fn current_buffer_mut(&mut self) -> Option<&mut Buffer> {
        self.buffers.get_mut(self.current?.get())?.as_mut()
    }

    /// Starts everything that needs the loop's channel.
    ///
    /// Best effort throughout: a watcher that will not start, or a language
    /// server that is not installed, is logged and then done without.
    /// Refusing to run because a convenience is missing would trade it for a
    /// missing program.
    fn start(&mut self, sender: std::sync::mpsc::Sender<Event>) {
        self.events = Some(sender.clone());
        // Only if the welcome screen is what will be on screen. Starting a
        // ticker for a reader who opened a file from the command line would
        // be twelve redraws a second behind a screen with nothing moving on
        // it.
        if self.buffers.is_empty() {
            self.ticker = Ticker::start(sender.clone());
        }
        self.start_watching(sender);
        for index in 0..self.buffers.len() {
            self.serve(index);
        }
    }

    /// Starts watching every open file for changes on disk.
    fn start_watching(&mut self, sender: std::sync::mpsc::Sender<Event>) {
        let mut watcher = match Watcher::new(sender) {
            Ok(watcher) => watcher,
            Err(error) => {
                tracing::warn!(%error, "auto-reload is off");
                return;
            }
        };
        for buffer in self.buffers.iter().flatten() {
            if let Err(error) = watcher.watch(buffer.path()) {
                tracing::warn!(%error, path = %buffer.path().display(), "not watching");
            }
        }
        self.watcher = Some(watcher);
    }

    /// The open picker, for the renderer.
    #[must_use]
    pub const fn picker(&self) -> Option<&Picker> {
        self.picker.as_ref()
    }

    /// The file the picker's selection names, if it has been read, and the
    /// part of it the selection is about.
    #[must_use]
    pub fn preview(&self) -> Option<(&Buffer, &Highlights, Option<Span>)> {
        let preview = self.preview.as_ref()?;
        Some((&preview.buffer, &preview.highlights, preview.marked))
    }

    /// How far along the welcome screen's colours have travelled, in ticks.
    ///
    /// Zero until something ticks, so a screen drawn without a running
    /// ticker -- a test, a remote session -- is the same screen every time.
    #[must_use]
    pub const fn phase(&self) -> u32 {
        self.phase
    }

    /// What obelus has to say, until the next key.
    #[must_use]
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    /// What a language server is busy with, if one is.
    ///
    /// Shown so that an empty answer during indexing can be told from an
    /// empty answer about a symbol that has no definition. They are the same
    /// message on the wire.
    #[must_use]
    pub fn server_working_on(&self) -> Option<&str> {
        self.servers.values().find_map(Client::working_on)
    }

    /// The server for the file being read, and what it is doing.
    ///
    /// Only the current file's: a status bar listing every server obelus has
    /// started would be a table, and the question a reader has is whether
    /// *this* file's questions can be answered.
    #[must_use]
    pub fn server_state(&self) -> Option<(&'static str, lsp::ServerState)> {
        let language = self.current_buffer()?.language()?;
        let client = self.servers.get(&language)?;
        Some((lsp::command_for(language)?, client.state()))
    }

    /// Asks each server whether it is still running.
    ///
    /// Once a frame, from [`App::prepare`]: reaping needs `&mut`, and a dead
    /// server is otherwise silent -- the reader thread stops and the
    /// questions simply stop being answered.
    fn check_servers(&mut self) {
        for client in self.servers.values_mut() {
            client.check_alive();
        }
    }

    /// Starts a server for a buffer's language, if there is one to start and
    /// it is not already running.
    ///
    /// Only for files under the root. A server is rooted at the working
    /// directory, and asking it about a file outside its own tree gets
    /// answers about a project it cannot see.
    fn serve(&mut self, index: usize) {
        let Some(buffer) = self.buffers.get(index).and_then(Option::as_ref) else {
            return;
        };
        let Some(language) = buffer.language() else {
            return;
        };
        if !buffer.path().starts_with(&self.working_directory) {
            tracing::debug!(path = %buffer.path().display(), "outside the root, so no server");
            return;
        }
        if self.stopped.contains(&language) {
            tracing::debug!(
                language = language.name(),
                "stopped on purpose, so no server"
            );
            return;
        }

        if !self.servers.contains_key(&language) {
            let Some(server) = lsp::server_for(language) else {
                return;
            };
            let command = server.command;
            if !lsp::on_path(command) {
                tracing::info!(%command, "not on PATH, so no server for {}", language.name());
                return;
            }
            let Some(sender) = self.events.clone() else {
                return;
            };
            match Client::start(language, server, &self.working_directory, sender) {
                Ok(client) => {
                    tracing::info!(%command, "started");
                    self.servers.insert(language, client);
                }
                Err(error) => {
                    tracing::warn!(%error, %command, "could not start");
                    return;
                }
            }
        }

        self.open_document(index);
    }

    /// Tells the server about a document.
    fn open_document(&mut self, index: usize) {
        let Some(buffer) = self.buffers.get(index).and_then(Option::as_ref) else {
            return;
        };
        let Some(language) = buffer.language() else {
            return;
        };
        let Ok(uri) = lsp::client::uri_for(buffer.path()) else {
            return;
        };
        let text = buffer.text().rope().to_string();
        let version = buffer.version();
        let name = language.name();

        if let Some(client) = self.servers.get_mut(&language) {
            let _ = client.notify(
                "textDocument/didOpen",
                &serde_json::json!({
                    "textDocument": {
                        "uri": uri,
                        "languageId": name,
                        "version": version,
                        "text": text,
                    }
                }),
            );
        }
    }

    /// Tells the server a document changed.
    fn change_document(&mut self, index: usize) {
        let Some(buffer) = self.buffers.get(index).and_then(Option::as_ref) else {
            return;
        };
        let Some(language) = buffer.language() else {
            return;
        };
        let Ok(uri) = lsp::client::uri_for(buffer.path()) else {
            return;
        };
        let text = buffer.text().rope().to_string();
        let version = buffer.version();

        if let Some(client) = self.servers.get_mut(&language) {
            let _ = client.notify(
                "textDocument/didChange",
                &serde_json::json!({
                    "textDocument": { "uri": uri, "version": version },
                    "contentChanges": [{ "text": text }],
                }),
            );
        }
    }

    /// Opens a picker over rows the caller built.
    ///
    /// The only way to reach a list of places without a language server
    /// answering first, which is what a test of the preview needs.
    pub fn open_picker_for_test(&mut self, items: Vec<PickerItem>, layout: PickerLayout) {
        self.picker = Some(Picker::new(items, layout));
    }

    /// Offers what a language server can say about the symbol under the
    /// cursor.
    ///
    /// One key for every question rather than a key each: the questions all
    /// take the same argument and differ only in what comes back, and a menu
    /// can say which ones this server actually answers.
    pub fn open_symbol_menu(&mut self) {
        let actions = match self.symbol_actions() {
            Ok(actions) => actions,
            // No menu at all. A list with one row explaining itself is still
            // a list: it covers the code, it has to be dismissed, and it
            // offers nothing. The reason belongs on the status bar, which is
            // where every other passing word about state goes.
            Err(why) => {
                self.note = Some(why);
                return;
            }
        };

        // Rows over commands, like the palette, so both offer the same things
        // and both show whatever key the table has for them.
        let items: Vec<PickerItem> = actions
            .into_iter()
            .map(|action| {
                let command = action.command();
                PickerItem {
                    icon: icons::enabled().then(|| icons::for_command(command.spec().name)),
                    label: command.spec().name.to_string(),
                    detail: Some(command.spec().title.to_string()),
                    trailing: self.keymap.chord_for(command).map(KeyChord::label),
                    value: PickerValue::Command(command),
                    colours: None,
                    status: None,
                    depth: 0,
                    kind: None,
                    tab: None,
                }
            })
            .collect();
        self.picker = Some(Picker::new(
            items,
            PickerLayout::Compact { rows: COMPACT_ROWS },
        ));
    }

    /// What the server for the current buffer will answer, or why nothing.
    fn symbol_actions(&self) -> Result<Vec<SymbolAction>, String> {
        let buffer = self
            .current_buffer()
            .ok_or_else(|| "no file open".to_string())?;
        let language = buffer
            .language()
            .ok_or_else(|| "obelus does not know this language".to_string())?;

        // On a name, before anything about servers. Every question in the
        // menu is about the thing under the cursor, and on a bracket or a
        // blank line there is no thing: the answer would be nothing, four
        // different ways. Asked first because it is the reason a reader can
        // act on -- move the cursor -- where the others are about the
        // machine.
        let cursor = buffer.cursor();
        let at = buffer
            .text()
            .byte_of_char(buffer.text().char_offset(cursor.line, cursor.column));
        if !buffer
            .syntax()
            .is_some_and(|state| state.is_name_at(buffer.text(), at))
        {
            return Err("no symbol here".to_string());
        }

        let Some(client) = self.servers.get(&language) else {
            return Err(match lsp::command_for(language) {
                Some(command) if !lsp::on_path(command) => format!("{command} is not installed"),
                Some(_) => format!("no server running for {}", language.name()),
                None => format!("no language server for {}", language.name()),
            });
        };
        let Some(capabilities) = client.capabilities() else {
            return Err("the language server is still starting".to_string());
        };
        let actions: Vec<SymbolAction> = action::ALL
            .iter()
            .copied()
            .filter(|action| action.supported(capabilities))
            .collect();
        if actions.is_empty() {
            return Err("the language server answers none of these".to_string());
        }
        Ok(actions)
    }

    /// Asks whichever question a command names.
    pub fn ask_about_symbol(&mut self, command: crate::command::Command) {
        let Some(action) = SymbolAction::for_command(command) else {
            return;
        };
        self.ask(action);
    }

    /// Asks one of those questions.
    fn ask(&mut self, action: SymbolAction) {
        let Some(id) = self.current else { return };
        let Some(buffer) = self.buffers.get(id.get()).and_then(Option::as_ref) else {
            return;
        };
        let Some(language) = buffer.language() else {
            return;
        };
        let Ok(uri) = lsp::client::uri_for(buffer.path()) else {
            return;
        };
        let cursor = buffer.cursor();
        let version = buffer.version();

        let Some(client) = self.servers.get_mut(&language) else {
            return;
        };
        let at = position::to_lsp(buffer.text(), cursor.line, cursor.column, client.encoding());
        let mut params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": at,
        });
        if action == SymbolAction::References {
            // Without this the definition itself is left out, which reads as
            // one reference missing rather than as a deliberate omission.
            params["context"] = serde_json::json!({ "includeDeclaration": true });
        }

        match client.request(action.method(), &params) {
            Ok(request) => {
                self.asked.insert(
                    (language, request),
                    Question {
                        asked: Asked::Symbol(action),
                        buffer: id,
                        version,
                    },
                );
                self.note = Some(format!("{}\u{2026}", action.title()));
            }
            Err(error) => {
                tracing::warn!(%error, "could not ask");
                self.note = Some("the language server is not listening".to_string());
            }
        }
    }

    /// Takes an answer, if it still means anything.
    ///
    /// What it means is worked out by [`action::outcome_of`], which is a
    /// function of the reply and two numbers: this is only what to do about
    /// each answer.
    fn on_reply(&mut self, language: LanguageId, reply: Reply) {
        let Some(question) = self.asked.remove(&(language, reply.id)) else {
            tracing::debug!(id = reply.id, "an answer with nothing waiting for it");
            return;
        };

        let now = self
            .buffers
            .get(question.buffer.get())
            .and_then(Option::as_ref)
            .map(Buffer::version);
        let action = match question.asked {
            Asked::Symbol(action) => action,
            Asked::Outline => {
                self.on_outline(reply);
                return;
            }
            Asked::Workspace => {
                self.on_workspace_symbols(reply);
                return;
            }
        };
        let indexing = self.server_working_on().is_some();
        match action::outcome_of(reply.result, question.version, now, indexing) {
            Outcome::Stale => {
                tracing::debug!(
                    asked_against = question.version,
                    now = ?now,
                    "dropping an answer about a version that has been replaced"
                );
                self.note = Some("the file changed while asking".to_string());
            }
            Outcome::Failed(message) => self.note = Some(message),
            Outcome::NotYet => self.note = Some("still indexing".to_string()),
            Outcome::Nothing => {
                self.note = Some(format!("nothing for {}", action.title()));
            }
            Outcome::Places(mut places) if places.len() == 1 => {
                let place = places.remove(0);
                self.note = None;
                self.go_to(&place.path, place.line, place.character);
            }
            Outcome::Places(places) => {
                let items = places
                    .into_iter()
                    .map(|place| PickerItem {
                        icon: Some(icons::for_path(&place.path)),
                        label: format!(
                            "{}:{}:{}",
                            relative(&place.path, &self.working_directory),
                            place.line.saturating_add(1),
                            place.character.saturating_add(1)
                        ),
                        detail: None,
                        trailing: None,
                        value: PickerValue::Place {
                            path: place.path,
                            line: place.line,
                            character: place.character,
                            end_line: place.end_line,
                            end_character: place.end_character,
                        },
                        colours: None,
                        status: None,
                        depth: 0,
                        kind: None,
                        tab: None,
                    })
                    .collect();
                self.note = None;
                self.picker = Some(Picker::new(items, PickerLayout::FullArea));
            }
        }
    }

    /// Goes to a place a server named, recording where the reader was.
    ///
    /// The position is converted here rather than when the answer arrived,
    /// because converting it needs the target file's text and that file may
    /// never have been opened.
    fn go_to(&mut self, path: &Path, line: u32, character: u32) {
        let from = self.here();
        self.open(path);

        let Some(id) = self.current else { return };
        // Before the buffer is borrowed: the area depends on which file is
        // current, which the open above has just settled.
        let area = self.text_area();
        let Some(buffer) = self.buffers.get_mut(id.get()).and_then(Option::as_mut) else {
            return;
        };
        let encoding = buffer
            .language()
            .and_then(|language| self.servers.get(&language))
            .map_or(lsp_types::PositionEncodingKind::UTF16, |client| {
                client.encoding().clone()
            });
        let at = lsp_types::Position { line, character };
        let (line, column) = position::from_lsp(buffer.text(), at, &encoding);
        buffer.place_cursor(line, column);
        buffer.center_on_cursor(area);

        if let Some(from) = from {
            self.jumps.push(from);
        }
    }

    /// Records a place in the history, if there was one.
    ///
    /// One line, but it is the line every leap has to remember, and the
    /// three callers each compute `from` before moving.
    fn record(&mut self, from: Option<Jump>) {
        if let Some(from) = from {
            self.jumps.push(from);
        }
    }

    /// Where the cursor is, for the history.
    fn here(&self) -> Option<Jump> {
        let id = self.current?;
        let cursor = self.buffers.get(id.get())?.as_ref()?.cursor();
        Some(Jump {
            buffer: id,
            line: cursor.line,
            column: cursor.column,
        })
    }

    /// Returns to where the last jump started.
    pub fn go_back(&mut self) {
        let Some(here) = self.here() else { return };
        match self.jumps.back(here) {
            Some(there) => self.go(there),
            None => self.note = Some("nowhere further back".to_string()),
        }
    }

    /// Undoes a jump back.
    pub fn go_forward(&mut self) {
        match self.jumps.forward() {
            Some(there) => self.go(there),
            None => self.note = Some("nowhere further forward".to_string()),
        }
    }

    fn go(&mut self, to: Jump) {
        if to.buffer.get() >= self.buffers.len() {
            return;
        }
        self.go_to_buffer(to.buffer);
        let area = self.text_area();
        if let Some(buffer) = self
            .buffers
            .get_mut(to.buffer.get())
            .and_then(Option::as_mut)
        {
            buffer.place_cursor(to.line, to.column);
            // Arriving, like the jump that led here: the line the reader left
            // deserves its context as much as the definition did.
            buffer.center_on_cursor(area);
        }
    }

    /// Which context key lookup happens in.
    const fn context(&self) -> Context {
        Context::Normal
    }

    /// Offers every file under the working directory.
    pub fn open_file_picker(&mut self) {
        self.walk_generation += 1;
        // Asked once, here, rather than per row: `git status` walks the tree
        // and applies every ignore rule on the way, and a list of ten
        // thousand files would ask ten thousand times.
        self.statuses = git::statuses(&self.working_directory);
        let mut picker = Picker::new(Vec::new(), PickerLayout::FullArea);
        // Shown for the moment before the first batch arrives as well as for
        // a tree with nothing in it, which is why it is about the search
        // rather than about the result.
        picker.when_empty("no files under this directory");
        // Open on the file being read. The walk decides where in the list it
        // is, and it may be in the last batch, so the picker holds on to the
        // name and selects the row when it turns up. The window puts the
        // selection near its middle, so this also decides what is around it.
        if let Some(buffer) = self.current_buffer() {
            picker.prefer(relative(buffer.path(), &self.working_directory));
        }
        self.picker = Some(picker);
        if let Some(sender) = self.events.clone() {
            files::spawn_walk(&self.working_directory, self.walk_generation, sender);
        }
    }

    /// Offers the files already open.
    pub fn open_buffer_picker(&mut self) {
        let mut open: Vec<(usize, &Buffer)> = self
            .buffers
            .iter()
            .enumerate()
            // Closed slots are holes, not rows.
            .filter_map(|(index, buffer)| buffer.as_ref().map(|buffer| (index, buffer)))
            .collect();
        // Most visited first. A list in the order files were opened puts the
        // one opened by accident an hour ago above the one being read all
        // afternoon; ties keep the order they were opened in, which is the
        // only other thing obelus knows about them.
        open.sort_by_key(|(index, buffer)| (std::cmp::Reverse(buffer.activations()), *index));

        self.statuses = git::statuses(&self.working_directory);
        let statuses = &self.statuses;
        let items = open
            .into_iter()
            .map(|(index, buffer)| PickerItem {
                icon: Some(icons::for_path(buffer.path())),
                label: relative(buffer.path(), &self.working_directory),
                detail: None,
                trailing: None,
                value: PickerValue::Buffer(BufferId::new(index)),
                colours: None,
                status: statuses.get(buffer.path()).copied(),
                depth: 0,
                kind: None,
                tab: None,
            })
            .collect();
        let mut picker = Picker::new(items, PickerLayout::FullArea);
        // Reachable with nothing open at all, which is how obelus starts.
        picker.when_empty("no file is open");
        self.picker = Some(picker);
    }

    /// Offers the built-in themes.
    pub fn open_theme_picker(&mut self) {
        self.theme_before = Some(self.theme);
        let items = builtin::ALL
            .iter()
            .map(|theme| PickerItem {
                // The same glyph on every row, which is the honest one: what
                // distinguishes two themes is the colours, and the row's own
                // name is what says which.
                icon: icons::enabled().then_some(icons::ui::THEME),
                label: theme.name.to_string(),
                detail: None,
                trailing: None,
                value: PickerValue::Theme(theme),
                colours: None,
                status: None,
                depth: 0,
                kind: None,
                tab: None,
            })
            .collect();
        let mut picker = Picker::new(items, PickerLayout::Compact { rows: COMPACT_ROWS });
        picker.when_empty("no theme is built in");
        // Open on the one that is on, so the list starts by saying which
        // theme this is rather than making the reader work it out.
        picker.prefer(self.theme.name.to_string());
        self.picker = Some(picker);
    }

    /// Offers every command by name.
    pub fn open_command_palette(&mut self) {
        // What a server would answer right now, so the palette offers a
        // question only when there is something to answer it.
        let items = crate::command::ALL
            .iter()
            .filter(|spec| self.offers(spec.command))
            .map(|spec| PickerItem {
                icon: icons::enabled().then(|| icons::for_command(spec.name)),
                colours: None,
                status: None,
                depth: 0,
                kind: None,
                label: spec.name.to_string(),
                // The tab it lives under. One past its position in the list
                // of groups, because the picker's own first tab is "all".
                tab: crate::command::Group::ALL
                    .iter()
                    .position(|group| *group == spec.command.group())
                    .map(|at| at + 1),
                detail: Some(spec.title.to_string()),
                // The key it is bound to, if it is bound to one. A command
                // with nothing here is one the palette is the only way to
                // reach, which is worth being able to see.
                trailing: self.keymap.chord_for(spec.command).map(KeyChord::label),
                value: PickerValue::Command(spec.command),
            })
            .collect();
        let mut picker = Picker::new(items, PickerLayout::Compact { rows: COMPACT_ROWS });
        // The palette leaves out what cannot run, so it can come up empty --
        // and an empty palette with no explanation reads as a broken key.
        picker.when_empty("no command can run here");
        // Tabs over one long list. Fourteen commands is already more than a
        // compact list shows at once, and the groups are what a reader is
        // choosing between when they do not already know the name.
        let names: Vec<&str> = crate::command::Group::ALL
            .iter()
            .map(|group| group.name())
            .collect();
        picker.with_tabs(&names);
        self.picker = Some(picker);
    }

    /// Stops the server for the current file and starts it again.
    ///
    /// The way out of a server that has died, or wedged, or was installed
    /// after obelus started: those are the three states where every question
    /// gets the same silence, and none of them is worth restarting the whole
    /// program over. With none running it simply starts one, which is why it
    /// is offered whether or not there is one.
    pub fn restart_server(&mut self) {
        let Some(language) = self.current_buffer().and_then(Buffer::language) else {
            self.note = Some("no file to restart a server for".to_string());
            return;
        };

        self.stop(language);
        // Deliberately stopped and now deliberately started: the restart is
        // the way back from a stop, so it lifts one.
        self.stopped.remove(&language);

        // Announce every open file of that language to the new server, not
        // just the current one: the others are still open, and a server that
        // has not been told about a file answers nothing about it.
        let indices: Vec<usize> = (0..self.buffers.len())
            .filter(|index| {
                self.buffers
                    .get(*index)
                    .and_then(Option::as_ref)
                    .and_then(Buffer::language)
                    .is_some_and(|of| of == language)
            })
            .collect();
        for index in indices {
            self.serve(index);
        }

        self.note = Some(match lsp::command_for(language) {
            Some(command) if self.servers.contains_key(&language) => format!("restarted {command}"),
            Some(command) if !lsp::on_path(command) => format!("{command} is not installed"),
            Some(command) => format!("{command} would not start"),
            None => format!("no language server for {}", language.name()),
        });
    }

    /// Stops the server for the current file and leaves it stopped.
    ///
    /// For a server that is costing more than it is answering. It stays
    /// stopped until `lsp.restart`, because otherwise opening the next
    /// file of that language would start it again.
    pub fn stop_server(&mut self) {
        let Some(language) = self.current_buffer().and_then(Buffer::language) else {
            self.note = Some("no file to stop a server for".to_string());
            return;
        };
        let was_running = self.stop(language);
        self.stopped.insert(language);
        self.note = Some(match (lsp::command_for(language), was_running) {
            (Some(command), true) => format!("stopped {command}"),
            (Some(command), false) => format!("{command} was not running"),
            (None, _) => format!("no language server for {}", language.name()),
        });
    }

    /// Stops one server, and says whether there was one to stop.
    ///
    /// Shared by stopping and restarting, which differ only in what they do
    /// afterwards.
    fn stop(&mut self, language: LanguageId) -> bool {
        let running = match self.servers.remove(&language) {
            // Politely, then not: `shutdown` waits briefly for the process to
            // go and kills it if it does not. A server left running would
            // hold the same files open and answer nothing.
            Some(mut client) => {
                client.shutdown();
                true
            }
            None => false,
        };
        // Every question still out was asked of the process that has just
        // gone. Its answers are never coming, and the next server's ids start
        // again from zero.
        self.asked.retain(|(asked, _), _| *asked != language);
        running
    }

    /// Stops showing the current file.
    ///
    /// The slot stays: [`BufferId`] is an index, and the jump list holds
    /// them. What goes is the file, the language server's copy of it, and
    /// the watch on it -- and the reader is left on whichever file is
    /// nearest, or on the welcome screen if that was the last one.
    pub fn close_current(&mut self) {
        // Whichever file the screen is about. With the buffer list open that
        // is the row under the selection, not the file behind it: the list is
        // what the reader is pointing at, and one key that means "close this"
        // everywhere beats a second key that only works in one place.
        if let Some(id) = self.selected_buffer() {
            self.close(id);
            // Rebuilt rather than patched, keeping whatever was typed: a
            // patched list would have to agree with the buffers about which
            // slots are holes.
            let query = self
                .picker
                .as_ref()
                .map(|picker| picker.query().to_string())
                .unwrap_or_default();
            self.open_buffer_picker();
            if let Some(picker) = self.picker.as_mut() {
                picker.set_query(&query);
            }
            return;
        }

        let Some(id) = self.current else {
            self.note = Some("no file to close".to_string());
            return;
        };
        self.close(id);
    }

    /// Moves to a buffer, counting the visit.
    ///
    /// One place, because the count is what orders the buffer list and a
    /// path that set `current` without counting would quietly leave a file
    /// out of that order.
    fn go_to_buffer(&mut self, id: BufferId) {
        if let Some(buffer) = self.buffers.get_mut(id.get()).and_then(Option::as_mut) {
            buffer.activate();
            self.current = Some(id);
        }
    }

    /// The buffer the open picker's selection names, if that is what it is.
    fn selected_buffer(&self) -> Option<BufferId> {
        match self.picker.as_ref()?.selected_item()?.value {
            PickerValue::Buffer(id) => Some(id),
            _ => None,
        }
    }

    /// Stops showing one file, whichever the reader is on.
    fn close(&mut self, id: BufferId) {
        let Some(buffer) = self.buffers.get_mut(id.get()).and_then(Option::take) else {
            return;
        };

        // Tell the server before dropping it: the message needs the path, and
        // a server left believing a file is open answers questions about a
        // version that no longer exists anywhere.
        if let Some(language) = buffer.language()
            && let Some(client) = self.servers.get_mut(&language)
            && let Ok(uri) = lsp::client::uri_for(buffer.path())
        {
            let _ = client.notify(
                "textDocument/didClose",
                &serde_json::json!({ "textDocument": { "uri": uri } }),
            );
        }
        if let Some(watcher) = self.watcher.as_mut() {
            watcher.unwatch(buffer.path());
        }
        self.note = Some(format!(
            "closed {}",
            relative(buffer.path(), &self.working_directory)
        ));
        drop(buffer);

        // Whichever file is nearest, before the closed one for preference:
        // closing the last of several usually means going back to the one
        // before it.
        if self.current == Some(id) {
            self.current = self.nearest_open(id.get());
        }
        // Nothing left, so the welcome screen is what is on screen -- and it
        // is the only thing that animates.
        if self.current.is_none() {
            self.ticker = self.events.clone().and_then(Ticker::start);
        }
    }

    /// The open buffer nearest to a slot, looking back first.
    fn nearest_open(&self, from: usize) -> Option<BufferId> {
        (0..from)
            .rev()
            .chain(from + 1..self.buffers.len())
            .find(|index| self.buffers.get(*index).is_some_and(Option::is_some))
            .map(BufferId::new)
    }

    /// Everything the current file defines, to jump into.
    ///
    /// A full-area picker, so it gets the preview the file picker has: the
    /// list on top, the symbol in its own code below, marked. Filtering is
    /// the prompt, moving is the arrows, and choosing is a jump -- all of
    /// which the picker already does. What is new here is only where the
    /// rows come from.
    ///
    /// From the syntax tree. A language server knows more, and asking it is
    /// the next step; the tree is what makes the outline work on a file with
    /// no server, before indexing has finished, and outside the project
    /// root.
    pub fn open_outline(&mut self) {
        let Some(buffer) = self.current_buffer() else {
            self.note = Some("no file to outline".to_string());
            return;
        };
        let path = buffer.path().to_path_buf();
        let Some(language) = buffer.syntax().map(SyntaxState::language) else {
            self.note = Some("obelus does not know this language".to_string());
            return;
        };

        // A server, if one is running for this language, gets asked. What it
        // knows is not what a tags query knows: the nesting is real, a
        // method is a method rather than a function that happens to sit
        // inside something, and a name it reports is a name the rest of the
        // semantic layer will agree about.
        //
        // The cost is that the answer arrives afterwards, so the list opens
        // saying it is waiting. That is the price of the better answer, and
        // it is a few milliseconds once the project is indexed.
        if self.servers.contains_key(&language) && self.ask_outline(&path, language) {
            let mut picker = Picker::new(Vec::new(), PickerLayout::FullArea);
            picker.when_empty("asking the language server\u{2026}");
            picker.is_outline_of(path);
            self.picker = Some(picker);
            return;
        }

        self.outline_from_tree();
    }

    /// The outline the syntax tree gives.
    ///
    /// The floor: no server, or one that will not answer. Also what a
    /// server's empty answer falls back to, which is why it is its own
    /// method rather than the tail of the one above.
    fn outline_from_tree(&mut self) {
        let Some(buffer) = self.current_buffer() else {
            return;
        };
        let path = buffer.path().to_path_buf();
        let Some(state) = buffer.syntax() else {
            return;
        };
        let language = state.language();
        let symbols = tags::outline(state, buffer.text());
        let text = buffer.text();
        let encoding = self.encoding_for(language);

        let items: Vec<PickerItem> = symbols
            .iter()
            .map(|symbol| {
                let at = position::to_lsp(text, symbol.line, symbol.column, &encoding);
                let end = position::to_lsp(text, symbol.line, symbol.end_column, &encoding);
                let (line, character) = (at.line, at.character);
                let end_character = end.character;
                PickerItem {
                    icon: icons::enabled().then(|| icons::for_kind(symbol.kind)),
                    colours: None,
                    status: None,
                    depth: u16::try_from(symbol.depth).unwrap_or(u16::MAX),
                    kind: Some(symbol.kind),
                    label: symbol.name.clone(),
                    detail: None,
                    trailing: Some(format!("{}", symbol.line.get() + 1)),
                    value: PickerValue::Place {
                        path: path.clone(),
                        line,
                        character,
                        end_line: line,
                        end_character,
                    },
                    tab: None,
                }
            })
            .collect();

        let mut picker = Picker::new(items, PickerLayout::FullArea);
        picker.when_empty(if tags::has_tags(language) {
            "this file defines nothing"
        } else {
            // Not the same fact, and the difference is the reader's next
            // move: one means look elsewhere, the other means do not bother
            // pressing this key for this language.
            "no outline for this language"
        });
        // On the symbol the cursor is in, or the nearest one above it, which
        // is the answer to "where am I" that an outline is usually opened to
        // ask.
        if let Some(here) = nearest_symbol(&symbols, self.current_buffer().map(Buffer::cursor)) {
            picker.prefer(here);
        }
        picker.is_outline_of(path);
        self.picker = Some(picker);
    }

    /// Asks a server what a file defines, and says whether the question got
    /// out.
    fn ask_outline(&mut self, path: &Path, language: LanguageId) -> bool {
        let Ok(uri) = lsp::client::uri_for(path) else {
            return false;
        };
        let Some(id) = self.current else { return false };
        let Some(version) = self
            .buffers
            .get(id.get())
            .and_then(Option::as_ref)
            .map(Buffer::version)
        else {
            return false;
        };
        let Some(client) = self.servers.get_mut(&language) else {
            return false;
        };
        let params = serde_json::json!({ "textDocument": { "uri": uri } });
        match client.request("textDocument/documentSymbol", &params) {
            Ok(request) => {
                self.asked.insert(
                    (language, request),
                    Question {
                        asked: Asked::Outline,
                        buffer: id,
                        version,
                    },
                );
                true
            }
            Err(error) => {
                tracing::warn!(%error, "could not ask for an outline");
                false
            }
        }
    }

    /// Puts a server's answer into the list that is waiting for it.
    ///
    /// Nothing to put it in means the reader has closed the list or opened a
    /// different one, and an answer nobody is looking at is dropped. An
    /// empty answer falls back to the syntax tree: a server that will not
    /// answer this question yet should not cost the reader the outline.
    fn on_outline(&mut self, reply: Reply) {
        let Some(path) = self
            .picker
            .as_ref()
            .and_then(Picker::outline_of)
            .map(Path::to_path_buf)
        else {
            tracing::debug!("an outline with nothing waiting for it");
            return;
        };

        let symbols = lsp::outline::symbols_in(reply.result);
        if symbols.is_empty() {
            tracing::debug!("the server has no outline, so the tree's it is");
            self.outline_from_tree();
            return;
        }

        let items: Vec<PickerItem> = symbols
            .iter()
            .map(|symbol| PickerItem {
                icon: icons::enabled().then(|| icons::for_kind(symbol.kind)),
                colours: None,
                status: None,
                depth: u16::try_from(symbol.depth).unwrap_or(u16::MAX),
                kind: Some(symbol.kind),
                label: symbol.name.clone(),
                detail: None,
                trailing: Some(format!("{}", symbol.line.saturating_add(1))),
                value: PickerValue::Place {
                    path: path.clone(),
                    line: symbol.line,
                    character: symbol.character,
                    end_line: symbol.line,
                    end_character: symbol.end_character,
                },
                tab: None,
            })
            .collect();

        let here = self
            .current_buffer()
            .map(Buffer::cursor)
            .and_then(|cursor| {
                symbols
                    .iter()
                    .rfind(|symbol| symbol.line as usize <= cursor.line.get())
            })
            .map(|symbol| symbol.name.clone());
        if let Some(picker) = self.picker.as_mut() {
            picker.replace(items);
            picker.when_empty("this file defines nothing");
            if let Some(here) = here {
                picker.prefer(here);
            }
        }
    }

    /// Which units a server counts positions in, or UTF-16 if none will say.
    fn encoding_for(&self, language: LanguageId) -> lsp_types::PositionEncodingKind {
        self.servers
            .get(&language)
            .map_or(lsp_types::PositionEncodingKind::UTF16, |client| {
                client.encoding().clone()
            })
    }

    /// Shows the current file as rendered markdown, or stops.
    ///
    /// By extension, case-insensitively, and nothing else: the mode is a
    /// *reading* of the bytes, and a reading that does not fit them produces
    /// a screen of nonsense. A file that is not markdown gets a note, which
    /// is the honest answer to a key that cannot do anything here.
    pub fn toggle_markdown(&mut self) {
        let Some(buffer) = self.current_buffer_mut() else {
            self.note = Some("no file to render".to_string());
            return;
        };
        if buffer.mode() == Mode::Markdown {
            buffer.set_mode(Mode::Edit);
            self.markdown = None;
            return;
        }
        if !is_markdown(buffer.path()) {
            self.note = Some("not a markdown file".to_string());
            return;
        }
        buffer.set_mode(Mode::Markdown);
        // The first row, because a rendering is a different document from
        // the file: the cursor's line is not one of its rows, and the
        // viewport's top is read as a row while this mode is on.
        buffer.scroll_by(
            isize::MIN / 2,
            TextArea {
                width: 1,
                height: 1,
            },
        );
    }

    /// What has changed in the current file, if obelus can tell.
    #[must_use]
    pub fn changes(&self) -> Option<&git::Changes> {
        self.changes.as_ref().map(|changed| &changed.changes)
    }

    /// The hunk the reader has opened in place, if any.
    #[must_use]
    pub const fn opened_hunk(&self) -> Option<LineNumber> {
        self.opened
    }

    /// Opens what changed at the cursor, in place, or closes it again.
    ///
    /// In place rather than in a panel: what a changed line means is what it
    /// replaced, and the two belong next to each other. The removed lines
    /// push the file's lines down while they are open, which is what makes
    /// it obvious they are not part of the file.
    pub fn toggle_hunk(&mut self) {
        let Some(line) = self.current_buffer().map(|buffer| buffer.cursor().line) else {
            self.note = Some("no file open".to_string());
            return;
        };
        let Some(hunk) = self.changes().and_then(|changes| changes.hunk_at(line)) else {
            self.note = Some("nothing changed here".to_string());
            return;
        };
        // Every hunk opens, including one that replaced nothing: opening it
        // is what puts the change type behind the lines, and "which lines
        // exactly are new here" is a question the margin's one column cannot
        // answer. An added hunk simply has nothing to show above itself.
        let anchor = hunk.line;
        self.opened = if self.opened == Some(anchor) {
            None
        } else {
            Some(anchor)
        };
    }

    /// Moves the cursor to the change above it.
    pub fn go_to_previous_change(&mut self) {
        self.go_to_change(false);
    }

    /// Moves the cursor to the change below it.
    pub fn go_to_next_change(&mut self) {
        self.go_to_change(true);
    }

    /// Moves the cursor to the first line of the next change one way or the
    /// other.
    ///
    /// No wrapping. A reader who steps past the last change and lands back
    /// at the top has lost their place to a keystroke that looked like it
    /// did nothing; the command is not offered when there is nothing that
    /// way, which is the honest version of the same information.
    fn go_to_change(&mut self, forward: bool) {
        let Some(line) = self.current_buffer().map(|buffer| buffer.cursor().line) else {
            self.note = Some("no file open".to_string());
            return;
        };
        let target = self.changes().and_then(|changes| {
            if forward {
                changes.hunk_after(line)
            } else {
                changes.hunk_before(line)
            }
            .map(|hunk| hunk.line)
        });
        let Some(target) = target else {
            self.note = Some(if forward {
                "no change below here".to_string()
            } else {
                "no change above here".to_string()
            });
            return;
        };

        // A jump, so `go.back` comes back: stepping to a change is a leap
        // across the file, the same as typing a line number.
        let from = self.here();
        let area = self.text_area();
        if let Some(buffer) = self.current_buffer_mut() {
            buffer.place_cursor(target, CharColumn::new(0));
            // Centred only when the change was somewhere else entirely, the
            // same as arriving at a bracket: a hunk already on screen is a
            // short hop, and moving the view for it throws away the
            // reader's place.
            if buffer.cursor_screen_cell(area).is_none() {
                buffer.center_on_cursor(area);
            }
        }
        if let Some(from) = from {
            self.jumps.push(from);
        }
    }

    /// Who last changed each line of the file being read, if the answer has
    /// arrived and the reader wants to see it.
    ///
    /// One entry per line of the *committed* file: the caller maps a line of
    /// the working tree onto it, because the two are not the same file once
    /// the reader has changed anything.
    #[must_use]
    pub fn blame(&self) -> Option<&[Option<git::Blamed>]> {
        if !self.showing_blame {
            return None;
        }
        let path = self.current_buffer()?.path();
        self.blames.get(path).map(Vec::as_slice)
    }

    /// Shows or stops showing who changed each line.
    pub fn toggle_blame(&mut self) {
        self.showing_blame = !self.showing_blame;
        if self.showing_blame {
            self.refresh_blame();
        } else {
            self.note = Some("not showing who changed each line".to_string());
        }
    }

    /// Starts a walk of history for the file being read, once per file.
    fn refresh_blame(&mut self) {
        if !self.showing_blame {
            return;
        }
        let Some(path) = self
            .current_buffer()
            .map(|buffer| buffer.path().to_path_buf())
        else {
            return;
        };
        if self.blames.contains_key(&path) || !self.asking_blame.insert(path.clone()) {
            return;
        }
        if let Some(sender) = self.events.clone() {
            git::blame::spawn_blame(&path, sender);
        }
    }

    /// Asks git what has changed, if it has not already been asked about
    /// this version of this file.
    fn refresh_changes(&mut self) {
        let Some(buffer) = self.current_buffer() else {
            self.changes = None;
            return;
        };
        let at = (buffer.path().to_path_buf(), buffer.version());
        if self
            .changes
            .as_ref()
            .is_some_and(|changed| changed.at == at)
        {
            return;
        }

        // No committed text is every way this can have no answer -- not a
        // repository, a file git has never heard of, no commits yet -- and
        // they all mean the same thing in the margin: nothing to say.
        self.changes = git::head_text(buffer.path()).map(|committed| Changed {
            changes: git::Changes::between(&committed, &buffer.text().rope().to_string()),
            at,
        });
        // A hunk that was open belonged to the diff that has just been
        // replaced. Leaving it open would show removed lines that are no
        // longer removed anywhere.
        self.opened = None;
    }

    /// The rendering on screen, if the current file is being shown as one.
    #[must_use]
    pub fn markdown(&self) -> Option<&[markdown::Row]> {
        self.markdown
            .as_ref()
            .map(|rendered| rendered.rows.as_slice())
    }

    /// Lays the current file out as markdown, if it needs laying out.
    ///
    /// Once per change of text, width or file, not once per frame: the
    /// layout is the expensive part, and a reader scrolling a README would
    /// otherwise pay for it on every row moved.
    fn refresh_markdown(&mut self, width: u16) {
        let Some(buffer) = self.current_buffer() else {
            self.markdown = None;
            return;
        };
        if buffer.mode() != Mode::Markdown {
            self.markdown = None;
            return;
        }

        let at = (buffer.path().to_path_buf(), buffer.version(), width);
        if self
            .markdown
            .as_ref()
            .is_some_and(|rendered| rendered.at == at)
        {
            return;
        }
        let rows = markdown::render(&buffer.text().rope().to_string(), width);
        self.markdown = Some(Rendered { at, rows });
    }

    /// Goes to the bracket that matches the one under the cursor.
    ///
    /// Over the whole file rather than what is on screen: the partner being
    /// off screen is the case worth having a key for. That costs one pass of
    /// highlighting over the file, because the scan has to know which
    /// brackets are inside strings and comments -- `"("` is not an unclosed
    /// bracket -- and highlights are otherwise only computed for what is
    /// visible. One pass on a keystroke, not per frame.
    ///
    /// Not recorded in the jump list. It is a motion within one expression,
    /// and a history filled with bracket hops is a history you cannot use to
    /// get back to where you were reading.
    pub fn go_to_bracket(&mut self) {
        let Some(buffer) = self.current_buffer() else {
            self.note = Some("no file open".to_string());
            return;
        };
        let Some(state) = buffer.syntax() else {
            self.note = Some("obelus does not know this language".to_string());
            return;
        };
        let text = buffer.text();
        let cursor = buffer.cursor();
        let at = text.byte_of_char(text.char_offset(cursor.line, cursor.column));

        let whole = ByteOffset::new(0)..text.byte_length();
        let mut highlights = Highlights::default();
        highlights.refresh(state, text, whole.clone());
        let Some((open, close)) = brackets::pair_at(text, &highlights, at, whole) else {
            self.note = Some("no bracket here".to_string());
            return;
        };

        let partner = if open == at { close } else { open };
        let (line, column) = text.position(text.char_of_byte(partner));
        let area = self.text_area();
        if let Some(buffer) = self.current_buffer_mut() {
            buffer.place_cursor(line, column);
            // Centred only if it was somewhere else entirely. A partner on
            // screen is a short hop, and moving the whole view for it would
            // throw away the reader's place to show them something they were
            // already looking at.
            if buffer.cursor_screen_cell(area).is_none() {
                buffer.center_on_cursor(area);
            }
        }
    }

    /// Opens the search, at one of its three scopes.
    ///
    /// One view with three tabs rather than three views: the question is
    /// "where is this", and only its radius changes. The query survives a
    /// walk between the tabs, which is the whole point -- a reader who does
    /// not find it in this file looks in the project without retyping it.
    pub fn open_search(&mut self, scope: Scope) {
        let names: Vec<&str> = Scope::ALL.iter().map(|scope| scope.label()).collect();
        let mut picker = Picker::new(Vec::new(), PickerLayout::FullArea);
        picker.with_scopes(&names);
        picker.searches();
        picker.go_to_tab(scope.tab());
        self.picker = Some(picker);
        self.refresh_search(true);
    }

    /// How many files the search has parsed to colour its rows.
    ///
    /// Bounded by the rows that have been on screen, which is the rule worth
    /// stating: a project search can hold two thousand rows, and parsing
    /// every one of their files would be a search that takes as long as
    /// reading the project.
    #[must_use]
    pub fn files_parsed_for_rows(&self) -> usize {
        self.row_syntax.len()
    }

    /// Which search the rows arriving belong to.
    ///
    /// A reader types faster than a tree can be walked, so every batch
    /// carries the generation it was asked under and anything older is
    /// dropped. Public because the scan is started from outside the loop in
    /// tests, which have to say which search they are answering.
    #[must_use]
    pub fn search_generation(&self) -> u64 {
        self.search_generation
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Says that what is being asked has changed, and returns the generation
    /// the answers must now carry. Every earlier scan learns from this that
    /// it can stop.
    fn ask_again(&mut self) -> u64 {
        self.search_generation
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1
    }

    /// Works out what the characters of the rows on screen *are*.
    ///
    /// A search result is a line of code, and it reads like one only if it
    /// is coloured like one. Only the rows on screen, and only once each:
    /// a project search can hold two thousand of them, and the query for
    /// one line's worth of a tree is small but not free.
    fn colour_visible_rows(&mut self, height: u16) {
        if self.picker.is_none() {
            // Nothing is listing files any more, so nothing needs the trees
            // that were parsed to colour them.
            self.row_syntax.clear();
            return;
        }
        let Some(picker) = self.picker.as_ref() else {
            return;
        };
        if !picker.is_searching() {
            return;
        }
        let wanted: Vec<(usize, PathBuf, u32)> = picker
            .visible(height)
            .into_iter()
            .filter_map(|index| {
                let item = picker.rows_at(index)?;
                if item.colours.is_some() {
                    return None;
                }
                match &item.value {
                    PickerValue::Place { path, line, .. } => Some((index, path.clone(), *line)),
                    _ => None,
                }
            })
            .collect();

        for (index, path, line) in wanted {
            let colours = self.colours_of(&path, line);
            if let Some(picker) = self.picker.as_mut()
                && let Some(item) = picker.row_mut(index)
            {
                item.colours = Some(colours);
            }
        }
    }

    /// The kinds of the characters of one line of one file.
    ///
    /// Relative to the row's label, which starts at the line's first
    /// character that is not a space: the label is the line trimmed, and
    /// both ends of that trim are the same rule wherever a row is built.
    ///
    /// An empty list for a file obelus cannot parse or cannot read, which is
    /// an answer rather than a miss: the row is not asked about again.
    fn colours_of(&mut self, path: &Path, line: u32) -> Vec<Colouring> {
        // The file being read is already parsed, and its tree is the one
        // that matches what the reader is looking at.
        let open = self
            .buffers
            .iter()
            .flatten()
            .find(|buffer| buffer.path() == path);
        let buffer = match open {
            Some(buffer) => buffer,
            None => {
                if !self.row_syntax.contains_key(path) {
                    // Only as many as the rows that have been on screen, and
                    // dropped with the list. A file that will not open is
                    // not retried, because the row remembers the answer.
                    match Buffer::open(path) {
                        Ok(buffer) => {
                            self.row_syntax.insert(path.to_path_buf(), buffer);
                        }
                        Err(error) => {
                            tracing::debug!(%error, "no colours for a row");
                            return Vec::new();
                        }
                    }
                }
                match self.row_syntax.get(path) {
                    Some(buffer) => buffer,
                    None => return Vec::new(),
                }
            }
        };

        let Some(state) = buffer.syntax() else {
            return Vec::new();
        };
        let text = buffer.text();
        let line = LineNumber::new(line as usize);
        if line.get() >= text.line_count() {
            return Vec::new();
        }
        let start = text.line_start_byte(line);
        let end = text.line_start_byte(line.saturating_add(1));
        let mut highlights = Highlights::default();
        highlights.refresh(state, text, start..end);

        // The label starts at the first character that is not a space, and
        // its characters are counted from there.
        let contents = text.line(line).to_string();
        let indent = contents.chars().take_while(|c| c.is_whitespace()).count();
        let mut runs: Vec<Colouring> = Vec::new();
        let mut byte = start.get() + contents.char_indices().nth(indent).map_or(0, |(at, _)| at);
        for (column, character) in contents.chars().skip(indent).enumerate() {
            let kind = highlights.kind_at(ByteOffset::new(byte));
            byte += character.len_utf8();
            let Ok(column) = u16::try_from(column) else {
                break;
            };
            match (kind, runs.last_mut()) {
                (Some(kind), Some(last)) if last.2 == kind && last.1 == column => last.1 += 1,
                (Some(kind), _) => runs.push((column, column + 1, kind)),
                (None, _) => {}
            }
        }
        runs
    }

    /// Fills the search with the rows of whichever scope is showing.
    ///
    /// `moved` says the tab changed, as against only the query: the file's
    /// rows are every line of it and are filtered by the picker itself, so
    /// they are gathered once per visit rather than once per keystroke.
    fn refresh_search(&mut self, moved: bool) {
        let Some(picker) = self.picker.as_ref() else {
            return;
        };
        let scope = Scope::of_tab(picker.tab());
        match scope {
            Scope::File => {
                let empty = picker.query().is_empty();
                // The rows are the lines of one version of one file, so
                // that is what says whether they are still the right rows:
                // a file an agent rewrites while the search is open must not
                // go on being listed as it was.
                let stale = self.searched
                    != self
                        .current_buffer()
                        .map(|buffer| (buffer.path().to_path_buf(), buffer.version()));
                let filled = picker.row_count() > 0;
                if empty {
                    // Every line of the file is not an answer to no
                    // question -- the reader is looking at the file already,
                    // and a list of it says nothing they cannot see. With no
                    // file at all the reason is that, which is a fact about
                    // the world rather than an invitation to type.
                    let reason = if self.current_buffer().is_some() {
                        "type to search this file"
                    } else {
                        "no file open"
                    };
                    self.searched = None;
                    if let Some(picker) = self.picker.as_mut() {
                        picker.replace(Vec::new());
                        picker.while_empty(reason);
                    }
                } else if moved || !filled || stale {
                    // Gathered on the way in from an empty query rather than
                    // per keystroke: the rows are the file's lines, which do
                    // not depend on what has been typed. The picker narrows
                    // them from there, and re-gathering per keystroke would
                    // also throw away the row the reader had moved to.
                    self.search_this_file();
                }
            }
            Scope::Project => self.search_the_project(),
            Scope::Symbols => self.search_the_symbols(),
        }
    }

    /// Every line of the file being read, for the picker to narrow.
    ///
    /// The rows are the lines rather than the matches, because the picker is
    /// already a matcher: it scores the query against every row, highlights
    /// what it matched and keeps the best first. A search that filtered the
    /// lines itself would be a second, worse matcher beside it.
    fn search_this_file(&mut self) {
        let Some(buffer) = self.current_buffer() else {
            if let Some(picker) = self.picker.as_mut() {
                picker.replace(Vec::new());
                picker.when_empty("no file open");
            }
            return;
        };
        let path = buffer.path().to_path_buf();
        let version = buffer.version();
        let text = buffer.text();
        // The file's own encoding if a server is attached to it, because the
        // row's position is handed back through the same door a server's
        // answer goes through.
        let encoding = buffer
            .syntax()
            .map(SyntaxState::language)
            .map_or(lsp_types::PositionEncodingKind::UTF16, |language| {
                self.encoding_for(language)
            });

        let items: Vec<PickerItem> = (0..text.line_count())
            .map(|number| {
                let line = LineNumber::new(number);
                let at = position::to_lsp(text, line, CharColumn::new(0), &encoding);
                let end = position::to_lsp(text, line, text.line_length(line), &encoding);
                PickerItem {
                    icon: None,
                    colours: None,
                    status: None,
                    depth: 0,
                    kind: None,
                    // Trimmed at the front: the indentation is the same on
                    // every row of a block, so matching it finds nothing and
                    // showing it spends the width where the answer is.
                    label: text
                        .line(line)
                        .to_string()
                        .trim_end()
                        .trim_start()
                        .to_string(),
                    detail: None,
                    trailing: Some(format!("{}", number + 1)),
                    value: PickerValue::Place {
                        path: path.clone(),
                        line: at.line,
                        character: at.character,
                        end_line: end.line,
                        end_character: end.character,
                    },
                    tab: None,
                }
            })
            .collect();

        self.searched = Some((path, version));
        if let Some(picker) = self.picker.as_mut() {
            picker.replace(items);
            picker.when_empty("this file is empty");
        }
    }

    /// Starts a walk of the tree looking for the query.
    ///
    /// A thread per query, and the answers carry the generation they were
    /// asked under: a reader types faster than a tree can be walked, so the
    /// rows for "sc" must not land in a list that is now asking about
    /// "scope".
    fn search_the_project(&mut self) {
        let query = self
            .picker
            .as_ref()
            .map(|picker| picker.query().to_string());
        let Some(query) = query else { return };

        // Only the empty query is not a search: it matches every line of
        // every file, which is the tree rather than an answer. One letter is
        // a real question -- and the cheapest one there is, because it fills
        // the row limit in the first few files and stops.
        let generation = self.ask_again();
        if query.is_empty() {
            if let Some(picker) = self.picker.as_mut() {
                picker.replace(Vec::new());
                picker.while_empty("type to search every file");
            }
            return;
        }

        if let Some(picker) = self.picker.as_mut() {
            picker.replace(Vec::new());
            picker.while_empty("searching\u{2026}");
        }
        if let Some(sender) = self.events.clone() {
            search::spawn_scan(
                &self.working_directory,
                &query,
                generation,
                &self.search_generation,
                sender,
            );
        }
    }

    /// Puts a batch of matching lines into the list waiting for them.
    fn on_matches(&mut self, generation: u64, hits: Vec<search::Hit>, done: bool) {
        if generation != self.search_generation() {
            tracing::debug!(
                generation,
                "dropping matches for a query already typed past"
            );
            return;
        }
        let root = self.working_directory.clone();
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        if !picker.is_searching() || Scope::of_tab(picker.tab()) != Scope::Project {
            return;
        }
        picker.extend(hits.into_iter().map(|hit| PickerItem {
            icon: None,
            colours: None,
            status: None,
            depth: 0,
            kind: None,
            label: hit.text,
            detail: None,
            trailing: Some(format!("{}:{}", hit.path.display(), hit.line + 1)),
            // The line, not the column: the file is not open, so its text --
            // which is what a column in the protocol's units is counted
            // against -- is not here to count with. The row highlights what
            // matched, which is where the reader is looking anyway.
            value: PickerValue::Place {
                path: root.join(&hit.path),
                line: u32::try_from(hit.line).unwrap_or(u32::MAX),
                character: 0,
                end_line: u32::try_from(hit.line).unwrap_or(u32::MAX),
                end_character: 0,
            },
            tab: None,
        }));
        if done {
            // Now it is true that there is no match, and the row count says
            // it about the *project* rather than about the list: the picker's
            // own "no match" is about a query against rows it was given, and
            // here the rows never existed.
            picker.while_empty("no match in the project");
        }
    }

    /// Asks the language server for the names it knows across the project.
    fn search_the_symbols(&mut self) {
        let query = self
            .picker
            .as_ref()
            .map(|picker| picker.query().to_string());
        let Some(query) = query else { return };

        let language = self.current_buffer().and_then(Buffer::language);
        let Some(language) = language.filter(|language| self.servers.contains_key(language)) else {
            if let Some(picker) = self.picker.as_mut() {
                picker.replace(Vec::new());
                // The server is per language, and the language comes from
                // the file being read: with nothing open there is nobody to
                // ask, which is a different thing from an empty answer.
                picker.while_empty("no language server to ask");
            }
            return;
        };
        // An empty query asks a server for every name it knows, which is
        // its whole index; the protocol allows it and no server means it.
        if query.is_empty() {
            if let Some(picker) = self.picker.as_mut() {
                picker.replace(Vec::new());
                picker.while_empty("type to search the project's symbols");
            }
            return;
        }

        let asked = self.ask_workspace_symbols(language, &query);
        if let Some(picker) = self.picker.as_mut() {
            picker.replace(Vec::new());
            picker.while_empty(if asked {
                "asking the language server\u{2026}"
            } else {
                "the language server would not answer"
            });
        }
    }

    /// Sends `workspace/symbol`, and says whether the question got out.
    fn ask_workspace_symbols(&mut self, language: LanguageId, query: &str) -> bool {
        let Some(id) = self.current else { return false };
        let version = self
            .buffers
            .get(id.get())
            .and_then(Option::as_ref)
            .map(Buffer::version)
            .unwrap_or_default();
        let Some(client) = self.servers.get_mut(&language) else {
            return false;
        };
        let params = serde_json::json!({ "query": query });
        match client.request("workspace/symbol", &params) {
            Ok(request) => {
                self.asked.insert(
                    (language, request),
                    Question {
                        asked: Asked::Workspace,
                        buffer: id,
                        version,
                    },
                );
                true
            }
            Err(error) => {
                tracing::warn!(%error, "could not ask for the project's symbols");
                false
            }
        }
    }

    /// Puts the server's list of names into the search.
    ///
    /// Dropped if the reader has moved off the tab or closed the list: an
    /// answer nobody is looking at is not worth a redraw, and putting rows
    /// from one scope into another is worse than dropping them.
    fn on_workspace_symbols(&mut self, reply: Reply) {
        let symbols = lsp::outline::found_in(reply.result);
        let root = self.working_directory.clone();
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        if !picker.is_searching() || Scope::of_tab(picker.tab()) != Scope::Symbols {
            tracing::debug!("symbols with nothing waiting for them");
            return;
        }
        let items: Vec<PickerItem> = symbols
            .iter()
            .map(|symbol| PickerItem {
                icon: icons::enabled().then(|| icons::for_kind(symbol.kind)),
                colours: None,
                status: None,
                depth: 0,
                kind: Some(symbol.kind),
                label: symbol.name.clone(),
                detail: None,
                // Relative to the working directory, the way the file
                // list shows paths: a server answers with absolute paths,
                // and a row that spends thirty columns on a prefix every
                // row shares is thirty columns not spent on the name.
                trailing: Some(format!(
                    "{}:{}",
                    symbol
                        .path
                        .strip_prefix(&root)
                        .unwrap_or(&symbol.path)
                        .display(),
                    symbol.line.saturating_add(1)
                )),
                value: PickerValue::Place {
                    path: symbol.path.clone(),
                    line: symbol.line,
                    character: symbol.character,
                    end_line: symbol.line,
                    end_character: symbol.end_character,
                },
                tab: None,
            })
            .collect();
        picker.replace(items);
        picker.while_empty("the server knows no such name");
    }

    /// Asks for a line number.
    ///
    /// A prompt with no rows: there is nothing to list, and a list of every
    /// line in the file would be the file. The picker takes the query as the
    /// answer, which is the shape searching a file will want too.
    pub fn open_line_prompt(&mut self) {
        if self.current_buffer().is_none() {
            self.note = Some("no file to go into".to_string());
            return;
        }
        self.prompt = Some(Prompt::new(PromptKind::Line));
    }

    /// Stops selecting, leaving the cursor where it is.
    ///
    /// What escape does at the file itself. Silent when there is nothing
    /// selected: escape meaning "never mind" is not worth a note when there
    /// was nothing to mind.
    pub fn clear_selection(&mut self) {
        if let Some(buffer) = self.current_buffer_mut() {
            buffer.clear_selection();
        }
    }

    /// Copies the selected text to the system clipboard.
    pub fn copy_selection(&mut self) {
        let Some(text) = self.current_buffer().and_then(Buffer::selected_text) else {
            self.note = Some("nothing selected".to_string());
            return;
        };

        // Handed to the terminal, which owns it from here: that is what
        // makes the copy outlive obelus and what makes it work over ssh. A
        // terminal that does not implement the sequence copies nothing and
        // cannot say so, so the note reports what obelus did rather than
        // what the terminal did with it.
        match crate::clipboard::copy(&text) {
            Ok(()) => self.note = Some("copied selection".to_string()),
            Err(error) => {
                tracing::warn!(%error, "copying the selection failed");
                self.note = Some("could not copy selection".to_string());
            }
        }
    }

    /// The question being asked, if one is.
    #[must_use]
    pub const fn prompt(&self) -> Option<&Prompt> {
        self.prompt.as_ref()
    }

    /// Opens the log, as a file like any other.
    ///
    /// A reader is what obelus is, so the log needs no viewer of its own: it
    /// becomes a buffer, the watcher on its directory reloads it as it grows,
    /// and the cursor stays where it was put. What is in it that no screen
    /// shows is a server's own words -- its stderr, its handshake, and the
    /// requests obelus sent it.
    pub fn open_log(&mut self) {
        match crate::logging::current_file() {
            Some(path) => self.open(&path),
            // Logging is allowed to fail without stopping obelus starting, so
            // there may genuinely be no file.
            None => self.note = Some("no log file".to_string()),
        }
    }

    /// Acts on an answered prompt.
    ///
    /// Where "what it means" lives: the prompt knows what was typed and what
    /// kind of question it was, and nothing more. A line past the end
    /// of the file is clamped rather than refused -- `9999` in a short file
    /// means the end of it -- and a line number that is not a number is a
    /// note, because it is the reader's slip and not the file's.
    fn answer(&mut self, kind: PromptKind, text: &str) {
        match kind {
            PromptKind::Line => {
                let Ok(line) = text.trim().parse::<usize>() else {
                    self.note = Some(format!("{text:?} is not a line number"));
                    return;
                };
                let from = self.here();
                let area = self.text_area();
                if let Some(buffer) = self.current_buffer_mut() {
                    // One-based on the way in, because that is what every
                    // other tool prints and what the status bar shows.
                    buffer
                        .place_cursor(LineNumber::new(line.saturating_sub(1)), CharColumn::new(0));
                    buffer.center_on_cursor(area);
                }
                // A jump, so `go.back` comes back: typing a line number is
                // exactly the kind of leap the history is for.
                if let Some(from) = from {
                    self.jumps.push(from);
                }
            }
        }
    }

    /// Whether a command can do its job right now.
    ///
    /// One exhaustive match over the conditions rather than a test per
    /// command: what each command needs is declared beside it in
    /// [`crate::command::Command::requires`], and this is the one place that
    /// turns a condition into a yes or no from the application's own state.
    /// A row that silently fails is worse than a row that is not there.
    #[must_use]
    pub fn offers(&self, command: Command) -> bool {
        let buffer = self.current_buffer();
        match command.requires() {
            Requires::Nothing => true,
            Requires::AFileOpen => buffer.is_some(),
            Requires::AKnownLanguage => buffer.and_then(Buffer::language).is_some(),
            Requires::AMarkdownFile => buffer.is_some_and(|buffer| {
                is_markdown(buffer.path()) || buffer.mode() == Mode::Markdown
            }),
            // The character under the cursor, not the scan the command does.
            // The scan needs the whole file highlighted to know a bracket in
            // a string from one in code, and deciding whether to *list* a
            // row is not worth a pass over the file; a bracket inside a
            // string is then offered and answers "no bracket here", which is
            // a reason rather than a silence.
            Requires::ABracket => buffer.is_some_and(|buffer| {
                let cursor = buffer.cursor();
                let text = buffer.text();
                let at = text.byte_of_char(text.char_offset(cursor.line, cursor.column));
                text.rope()
                    .byte_slice(at.get()..)
                    .chars()
                    .next()
                    .is_some_and(|character| matches!(character, '(' | ')' | '[' | ']' | '{' | '}'))
            }),
            Requires::ASelection => buffer.and_then(Buffer::selection).is_some(),
            // Something that changed *and* has something to show: a run of
            // added lines changed nothing that is not already on screen.
            Requires::AHunk => buffer.is_some_and(|buffer| {
                self.changes()
                    .and_then(|changes| changes.hunk_at(buffer.cursor().line))
                    .is_some()
            }),
            Requires::AHunkBefore => buffer.is_some_and(|buffer| {
                self.changes()
                    .and_then(|changes| changes.hunk_before(buffer.cursor().line))
                    .is_some()
            }),
            Requires::AHunkAfter => buffer.is_some_and(|buffer| {
                self.changes()
                    .and_then(|changes| changes.hunk_after(buffer.cursor().line))
                    .is_some()
            }),
            Requires::SomewhereBack => self.jumps.can_go_back(),
            Requires::SomewhereForward => self.jumps.can_go_forward(),
            Requires::ARunningServer => buffer
                .and_then(Buffer::language)
                .is_some_and(|language| self.servers.contains_key(&language)),
            // Per command: a server may answer one of these questions and
            // not another, and the menu is built from the same list.
            Requires::AnAnswer => self
                .symbol_actions()
                .unwrap_or_default()
                .iter()
                .any(|action| action.command() == command),
        }
    }

    fn accept(&mut self, value: PickerValue) {
        self.picker = None;
        match value {
            PickerValue::Command(command) => dispatch::dispatch(self, command),
            PickerValue::File(path) => self.open(&self.working_directory.join(path)),
            PickerValue::Buffer(id) => {
                if self.current != Some(id) {
                    let from = self.here();
                    self.record(from);
                }
                if id.get() < self.buffers.len() {
                    self.go_to_buffer(id);
                }
            }
            PickerValue::Theme(theme) => {
                // Chosen, so there is nothing to go back to.
                self.theme_before = None;
                self.set_theme(theme);
            }
            PickerValue::Place {
                path,
                line,
                character,
                ..
            } => self.go_to(&path, line, character),
            PickerValue::Nothing => {}
        }
    }

    /// Opens a file, or switches to it if it is already open.
    fn open(&mut self, path: &Path) {
        // Whatever happens next, the welcome screen is over: even a file that
        // fails to open leaves a reader looking at something other than a
        // shimmering logo, and nothing else on screen moves.
        self.ticker = None;

        // Where the reader was, before they are somewhere else. Opening a
        // file is a leap, and the history is for leaps: without this, a
        // session of opening files leaves nothing to go back *to*, and
        // `go.back` answers "nowhere further back" to a reader who has been
        // three files deep. `push` drops a repeat of the same place, so
        // re-opening the file already being read records nothing.
        let from = self.here();

        if let Some(index) = self
            .buffers
            .iter()
            .position(|buffer| buffer.as_ref().is_some_and(|open| open.path() == path))
        {
            let id = BufferId::new(index);
            if self.current != Some(id) {
                self.record(from);
            }
            self.go_to_buffer(id);
            return;
        }
        match Buffer::open(path) {
            Ok(buffer) => {
                if let Some(watcher) = self.watcher.as_mut()
                    && let Err(error) = watcher.watch(buffer.path())
                {
                    tracing::warn!(%error, path = %buffer.path().display(), "not watching");
                }
                self.buffers.push(Some(buffer));
                let index = self.buffers.len() - 1;
                // Only once the file is known to be readable: a path that
                // turns out to be a directory leaves the reader where they
                // were, and a history entry for a leap that did not happen
                // is a place `go.back` would take them for no reason.
                self.record(from);
                self.go_to_buffer(BufferId::new(index));
                self.serve(index);
            }
            // A path from the walk can have gone away, or be a file this user
            // cannot read. Neither is a reason to stop.
            Err(error) => tracing::warn!(%error, "could not open"),
        }
    }

    /// The room the text has, once the gutter has taken its columns.
    pub(crate) fn text_area(&self) -> TextArea {
        let width = match self.current_buffer() {
            Some(buffer) => {
                let gutter = ui::editor::gutter_width(buffer.text().line_count());
                // The margin on the left and the change map on the right
                // both appear only for a file in a repository, and they
                // appear together: they are the same answer at two scales.
                let margins = if self.changes.is_some() {
                    ui::editor::MARGIN_WIDTH + ui::editor::CHANGE_MAP_WIDTH
                } else {
                    0
                };
                self.editor_area
                    .width
                    .saturating_sub(margins)
                    .saturating_sub(gutter)
                    .saturating_sub(ui::editor::SCROLLBAR_WIDTH)
            }
            None => self.editor_area.width,
        };
        TextArea {
            width,
            height: self.editor_area.height,
        }
    }

    /// Records the geometry, scrolls the cursor on screen, and highlights what
    /// that leaves visible.
    ///
    /// In this order: the highlight range depends on where the viewport ended
    /// up, so scrolling has to have happened.
    ///
    /// Private: it has to run before the frame is drawn, and the only thing
    /// that knows that is [`App::draw_into`], which is the one caller.
    fn prepare(&mut self, editor_area: Rect) {
        self.editor_area = editor_area;
        self.check_servers();

        // Which rows the list will draw is what decides which rows need
        // their matched characters worked out, and only the geometry knows
        // how many rows there are.
        let rows = ui::picker::PickerView::new(self).map(|view| view.region(editor_area).height);
        if let (Some(rows), Some(picker)) = (rows, self.picker.as_mut()) {
            picker.refresh_indices(rows);
        }
        // Unconditionally, because with no list open the geometry is `None`
        // and the trees parsed for the last one are what has to be let go.
        self.colour_visible_rows(rows.unwrap_or(0));

        // Before anything is drawn or measured: the theme decides colours
        // only, but the preview is the application wearing it, and a frame
        // drawn half in one theme is a frame nobody should see.
        self.preview_theme();

        // A file the search is listing can be rewritten under it -- by an
        // agent, which is the ordinary case here -- and the rows are the
        // lines of one version of it. Checked per frame rather than per
        // keystroke because nothing the reader does is what changed it.
        if self
            .picker
            .as_ref()
            .is_some_and(|picker| picker.is_searching() && picker.row_count() > 0)
            && Scope::of_tab(self.picker.as_ref().map_or(0, Picker::tab)) == Scope::File
            && self.searched
                != self
                    .current_buffer()
                    .map(|buffer| (buffer.path().to_path_buf(), buffer.version()))
        {
            self.search_this_file();
        }

        let area = self.text_area();
        self.refresh_markdown(editor_area.width);
        self.refresh_changes();
        self.refresh_blame();
        if let Some(buffer) = self.current_buffer_mut() {
            buffer.scroll_into_view(area);
        }

        self.refresh_preview(editor_area);

        let Self {
            buffers,
            current,
            highlights,
            ..
        } = self;
        let Some(buffer) = current
            .and_then(|id| buffers.get(id.get()))
            .and_then(Option::as_ref)
        else {
            highlights.clear();
            return;
        };
        let Some(state) = buffer.syntax() else {
            highlights.clear();
            return;
        };
        let range = visible_bytes(buffer, area.height);
        highlights.refresh(state, buffer.text(), range);
    }

    /// Wears whatever theme the picker's selection names.
    ///
    /// The preview *is* the application: there is no way to show what a theme
    /// looks like other than by using it, and every view already reads its
    /// colours from one place. Cancelling puts the old one back.
    fn preview_theme(&mut self) {
        let selected = self
            .picker
            .as_ref()
            .and_then(Picker::selected_item)
            .map(|item| item.value.clone());
        if let Some(PickerValue::Theme(theme)) = selected {
            self.theme = theme;
        }
    }

    /// Reads whatever the picker's selection names, and points it at the line
    /// the selection is about.
    fn refresh_preview(&mut self, editor_area: Rect) {
        let Some(area) = ui::picker::preview_region(self.picker.as_ref(), editor_area) else {
            self.preview = None;
            return;
        };
        let Some((path, marked)) = self.preview_target() else {
            self.preview = None;
            return;
        };

        if self.preview.as_ref().map(|preview| preview.path.as_path()) != Some(path.as_path()) {
            self.preview = match Buffer::open(&path) {
                Ok(buffer) => Some(Preview {
                    path: path.clone(),
                    buffer,
                    highlights: Highlights::default(),
                    marked: None,
                    target: (marked.line, marked.character),
                    scrolled: 0,
                }),
                // A file that has gone, or one this reader cannot read. No
                // preview rather than a message: the list is the subject here.
                Err(error) => {
                    tracing::debug!(%error, "no preview");
                    None
                }
            };
        }

        // Whichever encoding the server for this language agreed to. Nothing
        // named the place if there is no server, and then the mark is the top
        // of the file, where the units do not matter.
        let encoding = self
            .preview
            .as_ref()
            .and_then(Preview::language)
            .and_then(|language| self.servers.get(&language))
            .map_or(lsp_types::PositionEncodingKind::UTF16, |client| {
                client.encoding().clone()
            });

        let Some(preview) = self.preview.as_mut() else {
            return;
        };
        // A different row is a different subject, so whatever the reader had
        // scrolled to is about the row they have left.
        if preview.target != (marked.line, marked.character) {
            preview.target = (marked.line, marked.character);
            preview.scrolled = 0;
        }

        // The line in question in the middle, the same as arriving at it by
        // jumping, and then wherever the reader has scrolled to. A preview is
        // read for the context around a line, so putting the line at the top
        // spends half the room on the half that was not asked for.
        let target = LineNumber::new(marked.line as usize);
        let text = TextArea {
            width: area
                .width
                .saturating_sub(ui::editor::gutter_width(preview.buffer.text().line_count()))
                .saturating_sub(ui::editor::SCROLLBAR_WIDTH),
            height: area.height,
        };
        preview.buffer.place_cursor(target, CharColumn::new(0));
        preview.buffer.center_on_cursor(text);
        // Stored back, so rows the file does not have are not banked against
        // the next press the other way.
        preview.scrolled = preview.buffer.scroll_rows(preview.scrolled, text);
        preview.marked = marked.resolve(preview.buffer.text(), &encoding);

        let range = visible_bytes(&preview.buffer, area.height);
        if let Some(state) = preview.buffer.syntax() {
            preview
                .highlights
                .refresh(state, preview.buffer.text(), range);
        } else {
            preview.highlights.clear();
        }
    }

    /// What the wheel turns.
    ///
    /// Whatever the reader is looking at: the list when one is open -- a list
    /// under a wheel scrolls, and with the mouse reported the wheel no longer
    /// arrives as arrow keys, so a picker that ignored it would have lost
    /// something -- and otherwise the file, by rows, with the cursor left
    /// where it was put.
    fn scroll(&mut self, rows: isize) {
        if let Some(picker) = self.picker.as_mut() {
            // One row a notch in a list. Three is right for text, where a
            // notch is a gesture at a paragraph; a list is chosen through one
            // row at a time.
            picker.move_selection_by(rows.signum());
            return;
        }
        if let Some(rows_in_view) = self.markdown().map(<[_]>::len)
            && let Some(buffer) = self.current_buffer_mut()
        {
            buffer.scroll_rendering(rows, rows_in_view);
            return;
        }
        let area = self.text_area();
        if let Some(buffer) = self.current_buffer_mut() {
            buffer.scroll_by(rows, area);
        }
    }

    /// Scrolls the preview, without moving the selection.
    ///
    /// Not a command: it is navigation, and navigation belongs to whatever
    /// holds the position it moves. What holds this one is the preview.
    fn scroll_preview(&mut self, pages: isize) {
        let Some(area) = ui::picker::preview_region(self.picker.as_ref(), self.editor_area) else {
            return;
        };
        let Some(preview) = self.preview.as_mut() else {
            return;
        };
        let rows = isize::try_from(area.height.max(1)).unwrap_or(1);
        // Not clamped here: what the file can actually give is known when it
        // is drawn, and `refresh_preview` stores that back.
        preview.scrolled += pages * rows;
    }

    /// The file, and the part of it, the picker's selection is about.
    fn preview_target(&self) -> Option<(PathBuf, Marked)> {
        let item = self.picker.as_ref()?.selected_item()?;
        match &item.value {
            // A file has no symbol in it to mark, so the preview starts at
            // the top with nothing highlighted.
            PickerValue::File(path) => Some((self.working_directory.join(path), Marked::top())),
            PickerValue::Buffer(id) => self
                .buffers
                .get(id.get())
                .and_then(Option::as_ref)
                .map(|buffer| (buffer.path().to_path_buf(), Marked::top())),
            PickerValue::Place {
                path,
                line,
                character,
                end_line,
                end_character,
            } => Some((
                path.clone(),
                Marked {
                    line: *line,
                    character: *character,
                    end_line: *end_line,
                    end_character: *end_character,
                },
            )),
            PickerValue::Command(_) | PickerValue::Theme(_) | PickerValue::Nothing => None,
        }
    }

    /// Re-reads whichever open buffers came from `path`.
    ///
    /// The watch is on a directory, so most of what arrives here is about
    /// files obelus does not have open.
    fn reload_path(&mut self, path: &Path) {
        for index in 0..self.buffers.len() {
            let Some(buffer) = self.buffers[index].as_mut() else {
                continue;
            };
            if buffer.path() == path && reload(buffer) {
                self.change_document(index);
            }
        }
    }

    /// Re-reads the current file and reparses what changed.
    pub fn reload_current(&mut self) {
        let Some(index) = self.current.map(BufferId::get) else {
            return;
        };
        if let Some(buffer) = self.buffers.get_mut(index).and_then(Option::as_mut)
            && reload(buffer)
        {
            self.change_document(index);
        }
    }

    /// Lays the screen out, scrolls the cursor into view, draws, and says
    /// where the terminal should put its cursor.
    ///
    /// The one path to a rendered frame, shared by the loop and the golden
    /// tests. Laying out here means it happens inside `Terminal::draw`, which
    /// is allowed: it is arithmetic over sizes, not work.
    pub fn draw_into(&mut self, cells: &mut CellBuffer, area: Rect) -> Option<Position> {
        self.prepare(ui::regions(area).editor);
        ui::draw(cells, area, self);
        ui::cursor_position(area, self)
    }

    /// Reacts to one event.
    ///
    /// The order keys are offered in is fixed here rather than encoded in the
    /// key table, because it is about which component owns the state a key
    /// moves, not about which key it is. Navigation belongs to whatever holds
    /// the position it moves; commands are the named actions left over.
    pub fn handle(&mut self, event: Event) {
        match event {
            Event::Key(key) => self.handle_key(key),
            // Redrawing is unconditional after every event, so a resize needs
            // no handling of its own beyond waking the loop.
            Event::Resize => {}
            Event::FileChanged { path } => self.reload_path(&path),
            Event::Lsp { language, message } => {
                let Some(client) = self.servers.get_mut(&language) else {
                    return;
                };
                // Everything the protocol needs rather than obelus — the
                // handshake, progress, the server's own log lines — is dealt
                // with in there.
                if let Some(reply) = client.on_message(&message) {
                    self.on_reply(language, reply);
                }
            }
            Event::Scroll(rows) => self.scroll(rows),
            Event::Tick => self.phase = self.phase.wrapping_add(1),
            Event::Matches {
                generation,
                hits,
                done,
            } => self.on_matches(generation, hits, done),
            Event::Blamed { path, lines } => {
                // Kept whether or not the reader is still looking at that
                // file: they walked away from it while a walk of its history
                // was running, and they will walk back.
                self.asking_blame.remove(&path);
                self.blames.insert(path, lines);
            }
            Event::FilesFound { generation, paths } => {
                // A batch from a walk whose picker is gone, or from one
                // superseded by a later open.
                if generation != self.walk_generation {
                    return;
                }
                if let Some(picker) = self.picker.as_mut() {
                    let statuses = &self.statuses;
                    let root = &self.working_directory;
                    picker.extend(paths.into_iter().map(|path| PickerItem {
                        icon: Some(icons::for_path(&path)),
                        label: path.display().to_string(),
                        detail: None,
                        trailing: None,
                        value: PickerValue::File(path.clone()),
                        colours: None,
                        status: statuses.get(&root.join(&path)).copied(),
                        depth: 0,
                        kind: None,
                        tab: None,
                    }));
                }
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        // Discards releases once, here, so nothing further down has to
        // remember to.
        if KeyChord::from_event(&key).is_none() {
            return;
        }
        let editor_height = self.editor_area.height;
        // Whatever obelus had to say has been read by now, or was not going to
        // be.
        self.note = None;

        // The picker gets first refusal, because the keys it wants are the
        // ones that move the thing it owns. What it does not want falls
        // through, which is how `ctrl+q` still works with one open.
        if let Some(picker) = self.picker.as_mut() {
            // A page is the rows actually on screen, which is why the layout
            // and the key handler share one function for it.
            let page = picker.visible_rows(editor_height);
            // What a search is asking, before and after the key. The picker
            // owns the query and the tab and knows nothing about where rows
            // come from, so the application watches those two for movement
            // rather than the picker reporting it.
            let searching = picker.is_searching();
            let before = (picker.tab(), picker.query().to_string());
            let outcome = picker.handle_key(&key, page);
            let after = (picker.tab(), picker.query().to_string());
            match outcome {
                PickerOutcome::Consumed => {
                    if searching && after != before {
                        self.refresh_search(after.0 != before.0);
                    }
                    return;
                }
                PickerOutcome::Cancelled => {
                    self.picker = None;
                    // A theme previewed but not chosen. Nothing else a picker
                    // shows changes the application while it is open, so
                    // nothing else has to be put back.
                    if let Some(before) = self.theme_before.take() {
                        self.theme = before;
                    }
                    return;
                }
                PickerOutcome::Accepted(value) => {
                    self.accept(value);
                    return;
                }
                PickerOutcome::Ignored => {}
            }
        }

        // A question on the status bar takes keys before anything else: it
        // is what the reader is looking at, and it is one row rather than a
        // region, so nothing under it is competing for them.
        if let Some(prompt) = self.prompt.as_mut() {
            let kind = prompt.kind();
            match prompt.handle_key(&key) {
                PromptOutcome::Consumed => return,
                PromptOutcome::Cancelled => {
                    self.prompt = None;
                    return;
                }
                PromptOutcome::Accepted(text) => {
                    self.prompt = None;
                    self.answer(kind, &text);
                    return;
                }
                // `ctrl+q` and the rest still reach the key table.
                PromptOutcome::Ignored => {}
            }
        }

        // A key the picker did not want, while one is open: the only thing
        // left that a key can move is the preview.
        if self.picker.is_some()
            && let Some(pages) = preview_paging(&key)
        {
            self.scroll_preview(pages);
            return;
        }

        // A rendering scrolls by rows. Its rows are not the file's lines, so
        // the cursor has nowhere to be in it and the motions have nothing to
        // move: what the keys do here is move the window.
        if self.picker.is_none()
            && let Some(rows) = self.markdown().map(<[_]>::len)
            && let Some(step) = view_step(&key, self.editor_area.height)
        {
            if let Some(buffer) = self.current_buffer_mut() {
                buffer.scroll_rendering(step, rows);
            }
            return;
        }

        // Paging the file being read, which is scrolling and not a motion:
        // the cursor stays where the reader left it.
        if self.picker.is_none()
            && let Some((pages, extend_selection)) = editor_paging(&key)
        {
            let area = self.text_area();
            if let Some(buffer) = self.current_buffer_mut() {
                if extend_selection {
                    buffer.extend_selection_by_page(pages, area);
                } else {
                    buffer.page(pages, area);
                }
            }
            return;
        }

        if self.picker.is_none()
            && let Some((motion, extend_selection)) = motion_for(&key)
        {
            let area = self.text_area();
            if let Some(buffer) = self.current_buffer_mut() {
                if extend_selection {
                    buffer.extend_selection(motion, area);
                } else {
                    buffer.move_cursor(motion, area);
                }
            }
            return;
        }

        if let Some(command) = self.keymap.lookup(&key, self.context()) {
            dispatch::dispatch(self, command);
        }
    }
}

/// A file read so that the picker's selection can be shown.
#[derive(Debug)]
struct Preview {
    path: PathBuf,
    buffer: Buffer,
    highlights: Highlights,
    /// The part of it the selection is about, once converted.
    marked: Option<Span>,
    /// Which part of the file the selection is about, as it arrived.
    ///
    /// Kept so that moving to a different row can be told from redrawing the
    /// same one, which is when the scrolling below is forgotten.
    target: (u32, u32),
    /// How many rows the reader has scrolled the preview by.
    ///
    /// Held here rather than in the buffer's viewport because the viewport is
    /// set from the target on every frame: without this, a scroll would be
    /// undone before it was drawn.
    scrolled: isize,
}

impl Preview {
    fn language(&self) -> Option<LanguageId> {
        self.buffer.language()
    }
}

/// The part of a file a selection is about, in the protocol's units.
#[derive(Clone, Copy, Debug)]
struct Marked {
    line: u32,
    character: u32,
    end_line: u32,
    end_character: u32,
}

impl Marked {
    /// The top of a file, with nothing to mark.
    const fn top() -> Self {
        Self {
            line: 0,
            character: 0,
            end_line: 0,
            end_character: 0,
        }
    }

    /// The same span in obelus's own coordinates, or nothing when it is empty.
    fn resolve(
        self,
        text: &crate::text::Text,
        encoding: &lsp_types::PositionEncodingKind,
    ) -> Option<Span> {
        let at = |line, character| {
            position::from_lsp(text, lsp_types::Position { line, character }, encoding)
        };
        let (line, column) = at(self.line, self.character);
        let (end_line, end_column) = at(self.end_line, self.end_character);
        // An empty span marks nothing: a file preview has no symbol in it.
        if (line, column) == (end_line, end_column) {
            return None;
        }
        Some(Span {
            line,
            column,
            end_line,
            end_column,
        })
    }
}

/// What one question that is still out was about.
#[derive(Debug)]
struct Question {
    asked: Asked,
    buffer: BufferId,
    /// The document version it was asked against.
    version: i32,
}

/// What a question was about.
///
/// Two kinds of answer come back over the same channel and are told apart by
/// the id they arrive under, so what was asked has to be remembered here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Asked {
    /// One of the questions about the symbol under the cursor.
    Symbol(SymbolAction),
    /// Everything the file defines.
    Outline,
    /// The names the server knows across the project.
    Workspace,
}

/// A file's changes, and which version of which file they are about.
#[derive(Debug)]
struct Changed {
    changes: git::Changes,
    at: (PathBuf, i32),
}

/// A markdown rendering, and what it was made from.
///
/// The three things it depends on, so that a change to any of them is
/// noticed: which file, which version of it, and how wide the screen was.
#[derive(Debug)]
struct Rendered {
    at: (PathBuf, i32, u16),
    rows: Vec<markdown::Row>,
}

/// Whether a path names a markdown file.
///
/// By extension and case-insensitively -- `README.MD` is one -- and by
/// nothing else. Sniffing the contents would be guessing, and markdown's
/// whole trick is that it looks like the text it came from.
fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}

/// The name of the symbol the cursor is in, or the last one before it.
///
/// "Where am I in this file" is what an outline is usually opened to ask, and
/// a list that opens at the top answers "at the beginning", which is almost
/// never true.
fn nearest_symbol(symbols: &[tags::Symbol], cursor: Option<Cursor>) -> Option<String> {
    let cursor = cursor?;
    symbols
        .iter()
        .rfind(|symbol| symbol.line <= cursor.line)
        .map(|symbol| symbol.name.clone())
}

/// A path as it should be read: relative to the root when it lies under it.
fn relative(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// Re-reads one buffer, reporting rather than propagating a failure.
///
/// A file that has been deleted or replaced by a directory leaves the buffer
/// showing what it last held. Losing the contents would be worse than showing
/// something a moment out of date.
/// Re-reads one buffer, and says whether the text changed.
fn reload(buffer: &mut Buffer) -> bool {
    match buffer.reload() {
        Ok(true) => {
            tracing::debug!(path = %buffer.path().display(), "reloaded");
            true
        }
        Ok(false) => {
            tracing::trace!(path = %buffer.path().display(), "no change");
            false
        }
        Err(error) => {
            tracing::warn!(%error, path = %buffer.path().display(), "reload failed");
            false
        }
    }
}

/// The byte range the viewport covers.
///
/// Whole lines, so a highlight that starts just off the top edge still reaches
/// the first visible row.
pub(crate) fn visible_bytes(buffer: &Buffer, height: u16) -> std::ops::Range<ByteOffset> {
    let text = buffer.text();
    let top = buffer.viewport().top;
    let bottom = top.saturating_add(usize::from(height));
    let start = text.line_start_byte(top);
    let end = if bottom.get() >= text.line_count() {
        text.byte_length()
    } else {
        text.line_start_byte(bottom)
    };
    start..end
}

/// How many screenfuls a key scrolls the preview by.
///
/// The plain keys page the list, so these are the same keys with control
/// held. Reading a candidate and choosing between candidates are different
/// jobs, and a list of references is read by doing both at once.
fn preview_paging(key: &KeyEvent) -> Option<isize> {
    if keymap::modifiers_of(key)? != KeyModifiers::CONTROL {
        return None;
    }
    match key.code {
        KeyCode::PageDown => Some(1),
        KeyCode::PageUp => Some(-1),
        _ => None,
    }
}

/// How far a key moves a rendered view, in rows.
///
/// The arrows, the paging keys and the ends of the document, over a document
/// whose rows are all there is: no columns, no cursor, nothing to remember.
fn view_step(key: &KeyEvent, height: u16) -> Option<isize> {
    let modifiers = keymap::modifiers_of(key)?;
    let page = isize::from(height.max(1) as i16).max(1);
    match (modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Down) => Some(1),
        (KeyModifiers::NONE, KeyCode::Up) => Some(-1),
        (KeyModifiers::NONE, KeyCode::PageDown) => Some(page),
        (KeyModifiers::NONE, KeyCode::PageUp) => Some(-page),
        (KeyModifiers::CONTROL, KeyCode::End) => Some(isize::MAX),
        (KeyModifiers::CONTROL, KeyCode::Home) => Some(isize::MIN),
        _ => None,
    }
}

/// How many screenfuls a bare paging key moves the file by.
///
/// Separate from the motions because paging is not one: what moves is the
/// window on the file, not the place in it.
fn editor_paging(key: &KeyEvent) -> Option<(isize, bool)> {
    match (keymap::modifiers_of(key)?, key.code) {
        (KeyModifiers::NONE, KeyCode::PageDown) => Some((1, false)),
        (KeyModifiers::NONE, KeyCode::PageUp) => Some((-1, false)),
        (KeyModifiers::SHIFT, KeyCode::PageDown) => Some((1, true)),
        (KeyModifiers::SHIFT, KeyCode::PageUp) => Some((-1, true)),
        _ => None,
    }
}

/// The motion a navigation key stands for.
///
/// A modifier obelus has no meaning for disqualifies the key: `ctrl+left` is a
/// word motion it does not have yet, and treating it as a plain left would be
/// a wrong answer rather than a missing one.
fn motion_for(key: &KeyEvent) -> Option<(Motion, bool)> {
    // Judged the same way the key table judges, so a key means the same thing
    // in both places or nothing in both places.
    let modifiers = keymap::modifiers_of(key)?;

    match (modifiers, key.code) {
        // Not `ctrl+PageUp`/`ctrl+PageDown`: those mean previous and next tab
        // almost everywhere, and the nearest thing obelus has to a tab is a
        // buffer, so they are worth leaving free.
        (KeyModifiers::CONTROL, KeyCode::Home) => Some((Motion::DocumentStart, false)),
        (KeyModifiers::CONTROL, KeyCode::End) => Some((Motion::DocumentEnd, false)),
        // With shift as well, the same two motions extend the selection.
        // Without these the ends of the file are the one place a selection
        // cannot reach, and the rule that a modifier obelus has no meaning
        // for disqualifies the key made them do nothing at all.
        (m, KeyCode::Home) if m == KeyModifiers::CONTROL | KeyModifiers::SHIFT => {
            Some((Motion::DocumentStart, true))
        }
        (m, KeyCode::End) if m == KeyModifiers::CONTROL | KeyModifiers::SHIFT => {
            Some((Motion::DocumentEnd, true))
        }
        (KeyModifiers::SHIFT, code) => match code {
            KeyCode::Left => Some((Motion::Left, true)),
            KeyCode::Right => Some((Motion::Right, true)),
            KeyCode::Up => Some((Motion::Up, true)),
            KeyCode::Down => Some((Motion::Down, true)),
            KeyCode::Home => Some((Motion::LineStart, true)),
            KeyCode::End => Some((Motion::LineEnd, true)),
            _ => None,
        },
        (KeyModifiers::NONE, code) => match code {
            KeyCode::Left => Some((Motion::Left, false)),
            KeyCode::Right => Some((Motion::Right, false)),
            KeyCode::Up => Some((Motion::Up, false)),
            KeyCode::Down => Some((Motion::Down, false)),
            KeyCode::Home => Some((Motion::LineStart, false)),
            KeyCode::End => Some((Motion::LineEnd, false)),
            _ => None,
        },
        _ => None,
    }
}

/// Lays out, scrolls and draws one frame.
///
/// Shared with the golden tests so the geometry they assert on is the geometry
/// the loop produces, rather than a second copy of the same three lines.
pub fn render<B>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()>
where
    B: Backend,
    B::Error: std::error::Error + Send + Sync + 'static,
{
    terminal.draw(|frame| {
        let area = frame.area();
        // `Terminal::draw` shows the cursor and moves it when the frame names
        // a position, and hides it when the frame does not, so saying where it
        // goes is the whole of it.
        if let Some(position) = app.draw_into(frame.buffer_mut(), area) {
            frame.set_cursor_position(position);
        }
    })?;
    Ok(())
}

/// Runs until the application asks to quit or input ends.
///
/// `terminal.draw` is synchronous and blocks the loop on a write to stdout.
/// Into a local tty that is tens of microseconds; over ssh or inside tmux,
/// stdout is a pipe and a slow reader really does stall the write. Accepted
/// for now — but nothing slow may go inside the draw closure, which is the
/// mistake that actually happens.
pub fn run<B>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()>
where
    B: Backend,
    // 0.30 made the backend's error an associated type; `anyhow` needs it to
    // be a real, sendable error before `?` will take it.
    B::Error: std::error::Error + Send + Sync + 'static,
{
    let (sender, events) = event::channel();
    event::spawn_terminal_reader(sender.clone());
    app.start(sender);

    while !app.should_quit() {
        render(terminal, app)?;

        let Ok(event) = events.recv() else {
            // Every sender is gone, so no further event can arrive.
            break;
        };
        app.handle(event);

        // Fold in whatever else is already queued.
        for _ in 0..EVENT_DRAIN_LIMIT {
            let Ok(ready) = events.try_recv() else {
                break;
            };
            app.handle(ready);
        }
    }

    Ok(())
}
