//! Application state, and the loop that drives it.
//!
//! `App` holds everything obelus knows and everything it can be asked to
//! do, so its methods are as many as the things obelus does. They are split
//! across this directory by *what they are about* -- the file being read,
//! the language server, the repository, the search, the settings -- rather
//! than by size, and each file is one `impl App` block. Nothing moved
//! between types to do it: an application's state is one thing, and cutting
//! it into several would mean deciding, for every pair of them, which one
//! owns the answer.
//!
//! The files are named for the *aspect*, not for the module they talk to:
//! `crate::git` is the reading of a repository and `history` here is what
//! obelus does with what it reads. What is left in this file is the state
//! itself, the keys, the frame, and the loop.
pub mod agents;
mod choosing;
mod documents;
mod history;
mod keys;
mod moving;
mod preferences;
mod previewing;
mod searching;
mod semantics;
pub mod talking;

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use documents::Rendered;
use history::Changed;
use keys::{editor_paging, motion_for, view_step};
use previewing::{Preview, preview_paging};
use ratatui::{
    Terminal,
    backend::Backend,
    buffer::Buffer as CellBuffer,
    layout::{Position, Rect},
};
use semantics::Question;

use crate::{
    buffer::{Buffer, BufferId, Cursor, Mode, Motion, TextArea},
    command::{Command, Requires, dispatch},
    component::{
        chat::{ChatOutcome, Room as ChatRoom},
        picker::{
            Colouring, Listing, Picker, PickerItem, PickerLayout, PickerOutcome, PickerValue, files,
        },
        prompt::{Prompt, PromptKind, PromptOutcome},
        settings::{Settings, SettingsOutcome},
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
    /// The agents the registry lists, cached-then-fetched.
    registry: Vec<crate::agent::Agent>,
    /// Whether the registry has been asked for and not yet failed.
    asked_registry: bool,
    /// Why the registry could not be fetched, until it is tried again.
    registry_failure: Option<String>,
    /// The conversation with an agent, whether or not it is on screen.
    ///
    /// Kept rather than opened: the view is a region the reader shows and
    /// hides, and a conversation that started again every time it was
    /// closed would be a conversation nobody could leave for a minute.
    chat: crate::component::chat::Chat,
    /// Whether the conversation is what the editor region is showing.
    showing_chat: bool,
    /// The agent obelus is talking to, once something has needed it.
    talker: Option<crate::acp::Talk>,
    /// The permission request waiting on the reader: the channel its
    /// answer goes back through.
    permission: Option<crate::acp::Answer<Option<String>>>,
    /// Whether the transcript has already said the agent died.
    ///
    /// The check runs once a frame, so without this the news would be in
    /// the transcript once per frame for as long as the view is open.
    said_it_died: bool,
    /// Each agent's own mark, as SVG, by the registry's id for it.
    icons: HashMap<String, String>,
    /// Whether the marks have been asked for.
    asked_icons: bool,
    /// The marks again, as pixels the terminal will take -- or nothing to
    /// take them, on a terminal that cannot show a picture.
    images: crate::ui::image::Images,
    /// The installs running, and how far each has got.
    installing: HashMap<String, crate::agent::install::Progress>,
    /// Why an install did not work, per agent, until it is tried again.
    install_failures: HashMap<String, String>,
    /// What the reader has decided, as read from the file at startup.
    config: crate::config::Config,
    /// Where to write it back, or `None` for an application that was never
    /// told -- which is every test, and is why a test cannot write over the
    /// reader's real settings.
    config_path: Option<PathBuf>,
    /// The settings view, while it is open.
    settings: Option<Settings>,
    /// What a test said git would say, instead of asking it.
    given_statuses: Option<HashMap<PathBuf, git::FileStatus>>,
    /// Which listings the open file list is showing, in tab order.
    ///
    /// The changed listing has a tab only when something has changed, so
    /// which tab is which listing is not fixed.
    listing: Vec<Listing>,
    /// Which scopes the open search is showing, in tab order.
    ///
    /// The tabs are only the scopes that can answer, so which tab is which
    /// scope is not a fixed mapping and has to be remembered.
    searching: Vec<Scope>,
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
            registry: Vec::new(),
            asked_registry: false,
            registry_failure: None,
            chat: crate::component::chat::Chat::new(),
            showing_chat: false,
            talker: None,
            permission: None,
            said_it_died: false,
            icons: HashMap::new(),
            asked_icons: false,
            images: crate::ui::image::Images::none(),
            installing: HashMap::new(),
            install_failures: HashMap::new(),
            config: crate::config::Config::default(),
            config_path: None,
            settings: None,
            given_statuses: None,
            listing: Vec::new(),
            searching: Vec::new(),
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

    /// Gives the application the loop's channel and nothing else.
    ///
    /// Separate from [`App::start`] so that a test can have the parts that
    /// need a channel -- an agent, a file walk -- without a watcher, a
    /// ticker and a language server per open file.
    pub fn events_for_test(&mut self, sender: std::sync::mpsc::Sender<Event>) {
        self.events = Some(sender);
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

    /// The room the text has, once the gutter has taken its columns.
    ///
    /// Public because a test of what the view does when the cursor reaches
    /// the right-hand edge has to know where that edge is, and working it
    /// out again in the test would be the same arithmetic twice.
    pub fn text_area(&self) -> TextArea {
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
            wrap: self.config.wrap,
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
            && self.searching() == Some(Scope::File)
            && self.searched
                != self
                    .current_buffer()
                    .map(|buffer| (buffer.path().to_path_buf(), buffer.version()))
        {
            self.search_this_file();
        }

        self.settle_agents(editor_area);
        self.prepare_icons();
        self.settle_chat(editor_area);

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
            Event::Acp(message) => self.on_acp(message),
            Event::Registry { agents, failure } => self.on_registry(agents, failure),
            Event::Icon { id, svg } => self.on_icon(id, svg),
            Event::Installing { id, progress } => self.on_installing(id, progress),
            Event::Installed { id, failure } => self.on_installed(id, failure),
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
                        enabled: true,
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
            let listing = picker.is_listing();
            let before = (picker.tab(), picker.query().to_string());
            let outcome = picker.handle_key(&key, page);
            let after = (picker.tab(), picker.query().to_string());
            match outcome {
                PickerOutcome::Consumed => {
                    if searching && after != before {
                        self.refresh_search(after.0 != before.0);
                    }
                    if listing && after.0 != before.0 {
                        self.refresh_listing();
                    }
                    return;
                }
                PickerOutcome::Cancelled => {
                    self.picker = None;
                    // A list that was an agent's question has to be
                    // answered even when the reader walks away from it: an
                    // agent whose permission request goes unanswered waits
                    // for ever.
                    if self.is_asking_permission() {
                        self.refuse_permission();
                    }

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

        // The settings take what the picker did not: they are the whole
        // screen while they are open, and every printable character is
        // theirs to filter with. After the picker, because a list opened
        // over them -- a setting's choices -- is what the reader is
        // looking at.
        if self.settings.is_some() {
            // The agents the page would show, worked out before the
            // component is borrowed: it needs them to know what enter
            // means on a card, and it is not the thing that knows them.
            let listed = self.listed_agents();
            let Some(settings) = self.settings.as_mut() else {
                return;
            };
            let room = (self.editor_area.width, self.editor_area.height);
            let outcome = settings.handle_key(&key, &self.config, &listed, room);
            match outcome {
                SettingsOutcome::Consumed => return,
                SettingsOutcome::Cancelled => {
                    self.settings = None;
                    return;
                }
                SettingsOutcome::Changed(key, value) => {
                    self.change_setting(key, &value);
                    return;
                }
                SettingsOutcome::Choose(key, choices, word) => {
                    self.open_choices(key, choices, &word);
                    return;
                }
                SettingsOutcome::Install(id) => {
                    self.install_agent(&id);
                    return;
                }
                SettingsOutcome::Activate(id) => {
                    self.activate_agent(&id);
                    return;
                }
                SettingsOutcome::Deactivate => {
                    self.deactivate_agent();
                    return;
                }
                SettingsOutcome::Ignored => {}
            }
        }

        // The conversation takes what the settings did not: it is the whole
        // editor region while it is showing, and every printable character
        // goes into what is being typed. After the picker, because a list
        // opened over it -- an agent's own question -- is what the reader is
        // answering.
        if self.showing_chat {
            let thinking = self.talking() == talking::Talking::Thinking;
            // The room the two halves have, from the same functions the
            // view lays them out with: a page of scrolling is the page on
            // screen, and the caret moves by the rows the box really has.
            let width = ui::chat::writing_width(self.editor_area);
            let needed = self.chat.writing().rows(width).len();
            let room = ChatRoom {
                transcript: ui::chat::regions(self.editor_area, needed)
                    .transcript
                    .height,
                writing: width,
            };
            let orders = self.agent_orders().to_vec();
            match self.chat.handle_key(&key, thinking, room, &orders) {
                ChatOutcome::Consumed => return,
                ChatOutcome::Cancelled => {
                    self.close_chat();
                    return;
                }
                ChatOutcome::Send(text) => {
                    self.send_to_agent(&text);
                    return;
                }
                ChatOutcome::Interrupt => {
                    self.interrupt_agent();
                    return;
                }
                ChatOutcome::StepMode => {
                    self.step_agent_mode();
                    return;
                }
                ChatOutcome::Ignored => {}
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

/// A path as it should be read: relative to the root when it lies under it.
pub(super) fn relative(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
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

/// Lays out, scrolls and draws one frame.
///
/// Shared with the golden tests so the geometry they assert on is the geometry
/// the loop produces, rather than a second copy of the same three lines.
pub fn render<B>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()>
where
    B: Backend,
    B::Error: std::error::Error + Send + Sync + 'static,
{
    // A frame carrying a picture is written with the caret put out, and
    // every other frame lets `Terminal::draw` place it.
    //
    // `Terminal::draw` writes the whole diff with the caret still visible
    // where the last frame left it, and then -- for a frame that names a
    // position -- *shows* the caret before moving it. Both are moments when
    // a terminal that repaints mid-write would draw a caret somewhere
    // obelus did not put one. Ordinarily nothing repaints mid-write and
    // nobody sees either of them; handing a terminal a sixel makes it draw
    // then and there, which had a caret flashing across the agents page on
    // every step of the selection.
    //
    // Putting the caret out on *every* frame fixed that and cost more than
    // it was worth: a hide and a show per frame is a caret that visibly
    // blinks, and frames arrive as fast as a language server reports
    // progress. So the careful order is used where it is needed, which is
    // the one page that draws pictures.
    let pictures = app.shows_pictures();
    if pictures {
        terminal.hide_cursor()?;
    }
    let mut placed = None;
    terminal.draw(|frame| {
        let area = frame.area();
        let position = app.draw_into(frame.buffer_mut(), area);
        match pictures {
            // Held back until the write is over.
            true => placed = position,
            // `Terminal::draw` shows the cursor and moves it when the frame
            // names a position, and hides it when the frame does not, so
            // saying where it goes is the whole of it.
            false => {
                if let Some(position) = position {
                    frame.set_cursor_position(position);
                }
            }
        }
    })?;
    // Moved first and shown second, which is the order that has no moment
    // in it where the caret is visible in the wrong place.
    if let Some(position) = placed {
        terminal.set_cursor_position(position)?;
        terminal.show_cursor()?;
    }
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
