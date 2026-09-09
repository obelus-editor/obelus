//! Application state, and the loop that drives it.

use std::{
    collections::HashMap,
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
    buffer::{Buffer, BufferId, Motion, TextArea},
    command::dispatch,
    coordinates::{ByteOffset, CharColumn, LineNumber, Span},
    event::{self, Event},
    jump::{Jump, JumpList},
    keymap::{self, Context, KeyChord, Keymap},
    lsp::{
        self,
        action::{self, Outcome, SymbolAction},
        client::{Client, Reply},
        position,
    },
    picker::{Picker, PickerItem, PickerLayout, PickerOutcome, PickerValue, files, icons},
    syntax::{LanguageId, highlight::Highlights},
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
    buffers: Vec<Buffer>,
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
    /// What each question that is still out was about.
    ///
    /// Kept here rather than in the client because the answer arrives after
    /// the world has moved on, and only this side can say whether it still
    /// means anything.
    asked: HashMap<i64, Question>,
    /// Where the reader has been.
    jumps: JumpList,
    /// The file the picker's selection names, opened so it can be shown.
    ///
    /// Keyed by path: moving through a list reads each file once as it is
    /// passed, and moving back to one that is still selected reads nothing.
    preview: Option<Preview>,
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
        Self {
            keymap: Keymap::new(),
            buffers,
            current,
            theme: &builtin::DARK,
            picker: None,
            servers: HashMap::new(),
            asked: HashMap::new(),
            jumps: JumpList::default(),
            preview: None,
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
        self.buffers.get(self.current?.get())
    }

    fn current_buffer_mut(&mut self) -> Option<&mut Buffer> {
        self.buffers.get_mut(self.current?.get())
    }

    /// Starts everything that needs the loop's channel.
    ///
    /// Best effort throughout: a watcher that will not start, or a language
    /// server that is not installed, is logged and then done without.
    /// Refusing to run because a convenience is missing would trade it for a
    /// missing program.
    fn start(&mut self, sender: std::sync::mpsc::Sender<Event>) {
        self.events = Some(sender.clone());
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
        for buffer in &self.buffers {
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

    /// Starts a server for a buffer's language, if there is one to start and
    /// it is not already running.
    ///
    /// Only for files under the root. A server is rooted at the working
    /// directory, and asking it about a file outside its own tree gets
    /// answers about a project it cannot see.
    fn serve(&mut self, index: usize) {
        let Some(buffer) = self.buffers.get(index) else {
            return;
        };
        let Some(language) = buffer.language() else {
            return;
        };
        if !buffer.path().starts_with(&self.working_directory) {
            tracing::debug!(path = %buffer.path().display(), "outside the root, so no server");
            return;
        }

        if !self.servers.contains_key(&language) {
            let Some(command) = lsp::command_for(language) else {
                return;
            };
            if !lsp::on_path(command) {
                tracing::info!(%command, "not on PATH, so no server for {}", language.name());
                return;
            }
            let Some(sender) = self.events.clone() else {
                return;
            };
            match Client::start(language, command, &self.working_directory, sender) {
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
        let Some(buffer) = self.buffers.get(index) else {
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
        let Some(buffer) = self.buffers.get(index) else {
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
        let items = match self.symbol_actions() {
            // Rows over commands, like the palette, so both offer the same
            // things and both show whatever key the table has for them.
            Ok(actions) => actions
                .into_iter()
                .map(|action| {
                    let command = action.command();
                    PickerItem {
                        icon: None,
                        label: command.spec().name.to_string(),
                        detail: Some(command.spec().title.to_string()),
                        trailing: self.keymap.chord_for(command).map(KeyChord::label),
                        value: PickerValue::Command(command),
                    }
                })
                .collect(),
            // A row saying why there is nothing to offer. An empty list would
            // say only that.
            Err(why) => vec![PickerItem {
                icon: None,
                label: why,
                detail: None,
                trailing: None,
                value: PickerValue::Nothing,
            }],
        };
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
        let Some(buffer) = self.buffers.get(id.get()) else {
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
                    request,
                    Question {
                        action,
                        buffer: id,
                        version,
                        language,
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
        let Some(question) = self.asked.remove(&reply.id) else {
            tracing::debug!(id = reply.id, "an answer with nothing waiting for it");
            return;
        };
        if question.language != language {
            return;
        }

        let now = self.buffers.get(question.buffer.get()).map(Buffer::version);
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
                self.note = Some(format!("nothing for {}", question.action.title()));
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
                        icon: Some(crate::picker::icons::for_path(&place.path)),
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
        let Some(buffer) = self.buffers.get_mut(id.get()) else {
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

        if let Some(from) = from {
            self.jumps.push(from);
        }
    }

    /// Where the cursor is, for the history.
    fn here(&self) -> Option<Jump> {
        let id = self.current?;
        let cursor = self.buffers.get(id.get())?.cursor();
        Some(Jump {
            buffer: id,
            line: cursor.line,
            column: cursor.column,
        })
    }

    /// Returns to where the last jump started.
    pub fn jump_back(&mut self) {
        let Some(here) = self.here() else { return };
        match self.jumps.back(here) {
            Some(there) => self.go(there),
            None => self.note = Some("nowhere further back".to_string()),
        }
    }

    /// Undoes a jump back.
    pub fn jump_forward(&mut self) {
        match self.jumps.forward() {
            Some(there) => self.go(there),
            None => self.note = Some("nowhere further forward".to_string()),
        }
    }

    fn go(&mut self, to: Jump) {
        if to.buffer.get() >= self.buffers.len() {
            return;
        }
        self.current = Some(to.buffer);
        if let Some(buffer) = self.buffers.get_mut(to.buffer.get()) {
            buffer.place_cursor(to.line, to.column);
        }
    }

    /// Which context key lookup happens in.
    const fn context(&self) -> Context {
        Context::Normal
    }

    /// Offers every file under the working directory.
    pub fn open_file_picker(&mut self) {
        self.walk_generation += 1;
        self.picker = Some(Picker::new(Vec::new(), PickerLayout::FullArea));
        if let Some(sender) = self.events.clone() {
            files::spawn_walk(&self.working_directory, self.walk_generation, sender);
        }
    }

    /// Offers the files already open.
    pub fn open_buffer_picker(&mut self) {
        let items = self
            .buffers
            .iter()
            .enumerate()
            .map(|(index, buffer)| PickerItem {
                icon: Some(icons::for_path(buffer.path())),
                label: relative(buffer.path(), &self.working_directory),
                detail: None,
                trailing: None,
                value: PickerValue::Buffer(BufferId::new(index)),
            })
            .collect();
        self.picker = Some(Picker::new(items, PickerLayout::FullArea));
    }

    /// Offers the built-in themes.
    pub fn open_theme_picker(&mut self) {
        let items = builtin::ALL
            .iter()
            .map(|theme| PickerItem {
                // Themes and commands are not files, and a glyph for each
                // would be decoration rather than information.
                icon: None,
                label: theme.name.to_string(),
                detail: None,
                trailing: None,
                value: PickerValue::Theme(theme),
            })
            .collect();
        self.picker = Some(Picker::new(
            items,
            PickerLayout::Compact { rows: COMPACT_ROWS },
        ));
    }

    /// Offers every command by name.
    pub fn open_command_palette(&mut self) {
        let items = crate::command::ALL
            .iter()
            .map(|spec| PickerItem {
                icon: None,
                label: spec.name.to_string(),
                detail: Some(spec.title.to_string()),
                // The key it is bound to, if it is bound to one. A command
                // with nothing here is one the palette is the only way to
                // reach, which is worth being able to see.
                trailing: self.keymap.chord_for(spec.command).map(KeyChord::label),
                value: PickerValue::Command(spec.command),
            })
            .collect();
        self.picker = Some(Picker::new(
            items,
            PickerLayout::Compact { rows: COMPACT_ROWS },
        ));
    }

    fn accept(&mut self, value: PickerValue) {
        self.picker = None;
        match value {
            PickerValue::Command(command) => dispatch::dispatch(self, command),
            PickerValue::File(path) => self.open(&self.working_directory.join(path)),
            PickerValue::Buffer(id) => {
                if id.get() < self.buffers.len() {
                    self.current = Some(id);
                }
            }
            PickerValue::Theme(theme) => self.set_theme(theme),
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
        if let Some(index) = self.buffers.iter().position(|buffer| buffer.path() == path) {
            self.current = Some(BufferId::new(index));
            return;
        }
        match Buffer::open(path) {
            Ok(buffer) => {
                if let Some(watcher) = self.watcher.as_mut()
                    && let Err(error) = watcher.watch(buffer.path())
                {
                    tracing::warn!(%error, path = %buffer.path().display(), "not watching");
                }
                self.buffers.push(buffer);
                let index = self.buffers.len() - 1;
                self.current = Some(BufferId::new(index));
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
                self.editor_area.width.saturating_sub(gutter)
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
        let area = self.text_area();
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
        let Some(buffer) = current.and_then(|id| buffers.get(id.get())) else {
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

        // Two lines above the line in question, so there is something to read
        // it in the context of, and then wherever the reader has scrolled to.
        let target = LineNumber::new(marked.line as usize);
        let start = target.saturating_sub(2);
        let top = if preview.scrolled >= 0 {
            start.saturating_add(preview.scrolled.unsigned_abs())
        } else {
            start.saturating_sub(preview.scrolled.unsigned_abs())
        };
        preview.buffer.place_viewport(top);
        preview.buffer.place_cursor(target, CharColumn::new(0));
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
        // Clamped so the top of the file is as far up as it goes; the far end
        // is clamped by the viewport itself, which cannot pass the last line.
        let anchor = isize::try_from(preview.target.0.saturating_sub(2)).unwrap_or(isize::MAX);
        preview.scrolled = (preview.scrolled + pages * rows).max(-anchor);
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
            if self.buffers[index].path() == path && reload(&mut self.buffers[index]) {
                self.change_document(index);
            }
        }
    }

    /// Re-reads the current file and reparses what changed.
    pub fn reload_current(&mut self) {
        let Some(index) = self.current.map(BufferId::get) else {
            return;
        };
        if let Some(buffer) = self.buffers.get_mut(index)
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
                    return;
                }
                PickerOutcome::Accepted(value) => {
                    self.accept(value);
                    return;
                }
                PickerOutcome::Ignored => {}
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
    action: SymbolAction,
    buffer: BufferId,
    /// The document version it was asked against.
    version: i32,
    language: LanguageId,
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
fn visible_bytes(buffer: &Buffer, height: u16) -> std::ops::Range<ByteOffset> {
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
            KeyCode::PageUp => Some(Motion::PageUp),
            KeyCode::PageDown => Some(Motion::PageDown),
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
