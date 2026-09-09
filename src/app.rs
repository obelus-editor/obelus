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
    command::{Requires, dispatch},
    component::{
        picker::{Picker, PickerItem, PickerLayout, PickerOutcome, PickerValue, files},
        prompt::{Prompt, PromptKind, PromptOutcome},
    },
    coordinates::{ByteOffset, CharColumn, LineNumber, Span},
    event::{self, Event, Ticker},
    icons,
    jump::{Jump, JumpList},
    keymap::{self, Context, KeyChord, Keymap},
    lsp::{
        self,
        action::{self, Outcome, SymbolAction},
        client::{Client, Reply},
        position,
    },
    markdown,
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
        let Asked::Symbol(action) = question.asked else {
            self.on_outline(reply);
            return;
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

        let items = open
            .into_iter()
            .map(|(index, buffer)| PickerItem {
                icon: Some(icons::for_path(buffer.path())),
                label: relative(buffer.path(), &self.working_directory),
                detail: None,
                trailing: None,
                value: PickerValue::Buffer(BufferId::new(index)),
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
        // question only when there is something to answer it. The same
        // function the symbol menu uses: two rules would disagree, and the
        // disagreement would be a row that does nothing.
        let answerable = self.symbol_actions().unwrap_or_default();
        let serving = self
            .current_buffer()
            .and_then(Buffer::language)
            .is_some_and(|language| self.servers.contains_key(&language));
        let items = crate::command::ALL
            .iter()
            .filter(|spec| match spec.command.requires() {
                Requires::Nothing => true,
                Requires::ARunningServer => serving,
                Requires::AnAnswer => answerable
                    .iter()
                    .any(|action| action.command() == spec.command),
            })
            .map(|spec| PickerItem {
                icon: icons::enabled().then(|| icons::for_command(spec.name)),
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

    fn accept(&mut self, value: PickerValue) {
        self.picker = None;
        match value {
            PickerValue::Command(command) => dispatch::dispatch(self, command),
            PickerValue::File(path) => self.open(&self.working_directory.join(path)),
            PickerValue::Buffer(id) => {
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

        if let Some(index) = self
            .buffers
            .iter()
            .position(|buffer| buffer.as_ref().is_some_and(|open| open.path() == path))
        {
            self.go_to_buffer(BufferId::new(index));
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
                self.editor_area
                    .width
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

        // Before anything is drawn or measured: the theme decides colours
        // only, but the preview is the application wearing it, and a frame
        // drawn half in one theme is a frame nobody should see.
        self.preview_theme();

        let area = self.text_area();
        self.refresh_markdown(editor_area.width);
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
            Event::FilesFound { generation, paths } => {
                // A batch from a walk whose picker is gone, or from one
                // superseded by a later open.
                if generation != self.walk_generation {
                    return;
                }
                if let Some(picker) = self.picker.as_mut() {
                    picker.extend(paths.into_iter().map(|path| PickerItem {
                        icon: Some(icons::for_path(&path)),
                        label: path.display().to_string(),
                        detail: None,
                        trailing: None,
                        value: PickerValue::File(path),
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
            match picker.handle_key(&key, page) {
                PickerOutcome::Consumed => return,
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
            && let Some(pages) = editor_paging(&key)
        {
            let area = self.text_area();
            if let Some(buffer) = self.current_buffer_mut() {
                buffer.page(pages, area);
            }
            return;
        }

        if self.picker.is_none()
            && let Some(motion) = motion_for(&key)
        {
            let area = self.text_area();
            if let Some(buffer) = self.current_buffer_mut() {
                buffer.move_cursor(motion, area);
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
fn editor_paging(key: &KeyEvent) -> Option<isize> {
    if !keymap::modifiers_of(key)?.is_empty() {
        return None;
    }
    match key.code {
        KeyCode::PageDown => Some(1),
        KeyCode::PageUp => Some(-1),
        _ => None,
    }
}

/// The motion a navigation key stands for.
///
/// A modifier obelus has no meaning for disqualifies the key: `ctrl+left` is a
/// word motion it does not have yet, and treating it as a plain left would be
/// a wrong answer rather than a missing one.
fn motion_for(key: &KeyEvent) -> Option<Motion> {
    // Judged the same way the key table judges, so a key means the same thing
    // in both places or nothing in both places.
    let modifiers = keymap::modifiers_of(key)?;

    match (modifiers, key.code) {
        // Not `ctrl+PageUp`/`ctrl+PageDown`: those mean previous and next tab
        // almost everywhere, and the nearest thing obelus has to a tab is a
        // buffer, so they are worth leaving free.
        (KeyModifiers::CONTROL, KeyCode::Home) => Some(Motion::DocumentStart),
        (KeyModifiers::CONTROL, KeyCode::End) => Some(Motion::DocumentEnd),
        (KeyModifiers::NONE, code) => match code {
            KeyCode::Left => Some(Motion::Left),
            KeyCode::Right => Some(Motion::Right),
            KeyCode::Up => Some(Motion::Up),
            KeyCode::Down => Some(Motion::Down),
            KeyCode::Home => Some(Motion::LineStart),
            KeyCode::End => Some(Motion::LineEnd),
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
