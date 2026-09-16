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
mod asking;
mod changing;
mod choosing;
mod completing;
mod counting;
mod documents;
mod fixing;
mod history;
mod history_view;
mod hovering;
mod noting;
pub use history_view::About;
mod keys;
mod moving;
mod naming;
mod preferences;
mod previewing;
mod renaming;
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
use keys::{editor_paging, view_step};
use previewing::Preview;
use ratatui::{
    Terminal,
    backend::Backend,
    buffer::Buffer as CellBuffer,
    layout::{Position, Rect},
};
use semantics::{Asked, Question, named as server_named};

use crate::{
    buffer::{Buffer, BufferId, Cursor, Mode, Motion, TextArea},
    command::{Command, Requires, dispatch},
    component::{
        chat::{ChatOutcome, Room as ChatRoom},
        completion::Completion,
        counts::Counts,
        hover::Hover,
        picker::{
            Colouring, Listing, Marking, Picker, PickerItem, PickerLayout, PickerOutcome,
            PickerValue, files,
        },
        prompt::{Prompt, PromptKind, PromptOutcome},
        settings::{Settings, SettingsOutcome},
    },
    coordinates::{ByteOffset, CharColumn, LineNumber, Span},
    editing::motion_for,
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
    reading::{self, Reading},
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

/// How long a frame has to take before the log says so.
///
/// A frame is microseconds of work; anything a reader could notice is a
/// frame that waited on something, and that is what there is to find out
/// about.
const SLOW_FRAME: std::time::Duration = std::time::Duration::from_millis(50);

/// How many already-ready events to fold into one frame.
///
/// A held-down arrow key or a scroll burst delivers faster than a terminal can
/// usefully be redrawn; draining what is ready before drawing turns a burst
/// into one frame. The cap keeps a pathological producer from starving the
/// draw entirely.
const EVENT_DRAIN_LIMIT: usize = 256;

/// What a file looked like when it was committed, and which file and
/// commit that is the text of.
///
/// The text is `None` for a file with nothing committed, which has to be
/// remembered as much as a text does: otherwise every keystroke goes and
/// finds out again that there is nothing to find.
#[derive(Debug)]
pub(super) struct Committed {
    /// Which file, and which commit it is being compared with.
    pub(super) of: (PathBuf, Option<gix::ObjectId>),
    /// What that commit had in it.
    pub(super) text: Option<String>,
}

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
    /// The colours in force, and the name they answer to.
    ///
    /// Owned rather than borrowed from the built-in ones, because a theme
    /// can come from a file now. The name is beside it rather than in it:
    /// a theme is a set of colours and a name is not one of them.
    theme: Theme,
    theme_name: String,
    /// The open picker, if one is open.
    ///
    /// One field for all four, because they differ only in what they list.
    picker: Option<Picker>,
    /// Which file walk the picker is currently expecting batches from.
    ///
    /// Bumped every time a file picker opens, so batches from a walk whose
    /// picker has already closed are recognizable and dropped.
    walk_generation: u64,
    /// A line whose commit was asked for before anything knew who wrote it.
    ///
    /// The walk that names lines is only started for a reader who wants
    /// names in the margin, so for everyone else the key that opens the
    /// commit behind a line is the thing that starts it -- and an answer
    /// that arrives after the key has already been let go is an answer to a
    /// question nobody is still holding. Held here, and carried out when
    /// the walk lands, so the key works on the first press for everybody.
    asked_line: Option<(
        PathBuf,
        Option<gix::ObjectId>,
        crate::coordinates::LineNumber,
    )>,
    /// Which walk of the history the list is expecting batches from.
    ///
    /// Bumped every time a history starts being read -- a key, a tab, a
    /// different file -- so the batches of the walk before it are
    /// recognizable as stale. Shared with the walking thread, which reads it
    /// to find out that nobody is waiting for it any more: a whole history
    /// is a walk of the whole project, and there is nothing else to stop it
    /// with.
    history_generation: std::sync::Arc<std::sync::atomic::AtomicU64>,
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
    /// What each open file's server says is wrong with it.
    ///
    /// Keyed by path rather than by buffer, for the reason the tokens are:
    /// a buffer is a slot that is reused, and a closed file's troubles
    /// would otherwise be shown against whatever is opened into its place.
    /// Replaced wholesale, because that is what a server sends.
    troubles: HashMap<PathBuf, Vec<crate::lsp::trouble::Trouble>>,
    /// Where the pointer was last put down, and how many times in a row.
    ///
    /// A terminal reports button presses and nothing about double clicks,
    /// so the count is obelus's own: the same cell, pressed again inside
    /// the time below, is the second press of one gesture.
    clicked: Option<(u16, u16, std::time::Instant, u8)>,
    /// What could be typed next, while a server's answer is on screen.
    ///
    /// Beside the cursor rather than in a region of its own, and its own
    /// field rather than a picker, because the reader is typing into the
    /// document the whole time it is up: it takes six keys and the rest go
    /// where they were going.
    completion: Option<Completion>,
    /// What the call the cursor is inside takes, while it is showing.
    signature: Option<crate::lsp::signature::Signature>,
    /// What the server says the place under the caret is, while it is up.
    hover: Option<Hover>,
    /// What the server offered to do here, while a list of it is open.
    actions: Vec<crate::lsp::actions::Action>,
    /// Every use of the name the pointer is resting on, in this file.
    ///
    /// Marked in the text rather than listed: the answer is "these, here",
    /// and a list would take a region of screen to say what a background
    /// says in place.
    uses: Vec<Span>,

    /// Where the pointer is resting, since when, and whether that rest
    /// has already asked its question.
    ///
    /// A hover on a rest is the one thing in obelus that happens because a
    /// reader did *nothing*, so the doing-nothing has to be measured: the
    /// same cell, still under the pointer when the next tick lands. The
    /// asking is remembered because a pointer left on a word that has no
    /// answer must ask about it once rather than twelve times a second.
    resting: Option<Resting>,
    /// The holes left by a snippet, while the reader is filling them in.
    ///
    /// Character offsets into the document, moved by every edit. A snippet
    /// is over once the reader has tabbed past the last of them, which is
    /// what gives `tab` back to indenting.
    filling: Option<crate::lsp::snippet::Filling>,
    /// What each open file's tokens are, as its server last described them.
    ///
    /// Keyed by path rather than by buffer, because a buffer is a slot that
    /// is reused: a closed file's classification would otherwise answer
    /// about whatever is opened into its place. Each carries the document
    /// version it describes and is ignored once the document has moved past
    /// it, so a stale entry is inert rather than wrong.
    tokens: HashMap<PathBuf, crate::lsp::tokens::Tokens>,
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
    /// The current file laid out as whatever reading it has, if it is being
    /// shown that way.
    ///
    /// Kept here rather than in the buffer for the same reason the
    /// highlights are: it is a function of the text, the width and nothing
    /// else, and re-deriving it when either changes is simpler than keeping
    /// a buffer's copy of it right.
    rendered: Option<Rendered>,
    /// The theme to go back to if the theme picker is cancelled.
    ///
    /// Set while that picker is open, because moving through it *applies*
    /// each theme: a list of colour names is not a choice between colour
    /// schemes, and the only honest preview of a theme is the screen wearing
    /// it. Cancelling has to undo that.
    theme_before: Option<(String, Theme)>,
    /// The thread sending ticks, while anything wants them.
    ///
    /// Held so that dropping it stops the animation. There is nothing to
    /// animate once a file is open, and nothing over a network at all.
    ticker: Option<Ticker>,
    /// Whether the last frame asked to be woken again.
    ///
    /// Beside the ticker rather than read off it: the ticker needs the
    /// loop's channel, and the decision is the thing worth seeing -- an
    /// application with no loop behind it still makes it.
    waking: bool,
    /// What git says about the files in the tree, while a list of them is
    /// open.
    ///
    /// Gathered when a list opens and kept until the next one, because it is
    /// a walk of the whole tree and the rows arrive in batches afterwards.
    statuses: std::collections::HashMap<PathBuf, git::FileStatus>,
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
    /// The agent's own commands, while one is being typed in the box.
    ///
    /// Its own list rather than [`App::picker`], because it does not take
    /// the keys: it follows what is being typed and the box keeps them.
    slash: Option<Picker>,
    /// The form the agent asked the reader to fill in, while one is open.
    asking: Option<talking::Asking>,
    /// The card whatever the agent asked is answered on.
    ///
    /// One field for both kinds of question it can ask -- a form's field
    /// and a request for permission -- because on screen they are the same
    /// thing: what it wants to know, what the answers are, and room to say
    /// one in your own words where it will take those.
    card: Option<crate::component::card::Card>,
    /// The permission request waiting on the reader: the channel its
    /// answer goes back through.
    permission: Option<crate::acp::Answer<Option<String>>>,
    /// What obelus knows about the agents it could run.
    agents: agents::Agents,
    /// The settings as they stand, and where each part came from.
    settled: preferences::Settled,
    /// The settings view, while it is open.
    settings: Option<Settings>,
    /// The line counts, while they are showing.
    counts: Option<Counts>,
    /// What the tree means to come back to, while it is showing.
    notes: Option<crate::component::todo::TodoView>,
    /// The whole screen, as of the last frame.
    ///
    /// Kept beside `editor_area` because one view is not in it: the counts
    /// take the screen whole -- no status row and no rule above one -- and
    /// the keys that move about their list have to be told the height that
    /// is actually drawn.
    screen_area: Rect,
    /// What a test said git would say, instead of asking it.
    given_statuses: Option<HashMap<PathBuf, git::FileStatus>>,
    /// Which listings the open file list is showing, in tab order.
    ///
    /// The changed listing has a tab only when something has changed, so
    /// which tab is which listing is not fixed.
    listing: Vec<Listing>,
    /// The commits an open history view is showing, and what is open in it.
    history: history_view::Showing,
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
    blames: std::collections::HashMap<(PathBuf, Option<gix::ObjectId>), Vec<Option<git::Blamed>>>,
    /// The committed text the margin's diff is against.
    ///
    /// One file's, because one file's diff is drawn: switching to another
    /// reads that one's. Kept because reading it is opening the repository,
    /// finding the commit, walking its tree and unpacking the blob -- and
    /// what it answers changes only when the repository moves, where the
    /// document it is compared with changes on every keystroke.
    ///
    /// `None` inside the answer is a file with nothing committed, which has
    /// to be remembered too: otherwise every keystroke goes and finds out
    /// again that there is nothing to find.
    committed: Option<Committed>,
    /// Which files have been asked about and have not answered yet, so a
    /// frame does not start a second walk of the same history.
    asking_blame: std::collections::HashSet<(PathBuf, Option<gix::ObjectId>)>,
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
    /// Whether the symbols search offers names from outside the project.
    ///
    /// Off, because a server that has indexed a project has indexed what it
    /// was built on too: a search for `new` answered from the whole index is
    /// the registry's answer with the reader's own names somewhere in it.
    /// The switch is for the times they meant the dependency.
    outside: bool,
    /// How the search is looking: the three switches at its foot.
    ///
    /// The reader's, kept for as long as obelus is running and not written
    /// to their settings: a pattern answers *this* question, and one turned
    /// on to find one thing should not still be on next week.
    looking: search::Looking,
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
    /// inotify limit — and `reload-file` still works.
    watcher: Option<Watcher>,
    /// The directories being watched for a change to the theme.
    ///
    /// Kept because they are given up and taken again on every re-read:
    /// where a theme really lives moves when the reader chooses another one,
    /// and the directory itself is replaced when a desktop swaps a theme.
    theme_watched: Vec<PathBuf>,
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
    /// Whether to open on the file list.
    ///
    /// A directory on the command line is a reader saying which tree
    /// rather than which file, and "which file" is what the list answers.
    /// A flag rather than a list opened on the spot, because the rows come
    /// from a walk on the loop's own channel and there is no channel until
    /// [`App::start`].
    list_at_start: bool,
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
            theme: builtin::DARK,
            theme_name: builtin::DEFAULT.to_string(),
            picker: None,
            servers: HashMap::new(),
            stopped: HashSet::new(),
            asked: HashMap::new(),
            clicked: None,
            signature: None,
            hover: None,
            actions: Vec::new(),
            uses: Vec::new(),
            resting: None,
            troubles: HashMap::new(),
            completion: None,
            filling: None,
            tokens: HashMap::new(),
            jumps: JumpList::default(),
            preview: None,
            phase: 0,
            ticker: None,
            waking: false,
            prompt: None,
            changes: None,
            statuses: std::collections::HashMap::new(),
            chat: crate::component::chat::Chat::new(),
            showing_chat: false,
            talker: None,
            slash: None,
            asking: None,
            card: None,
            permission: None,
            settled: preferences::Settled::default(),
            agents: agents::Agents::default(),
            settings: None,
            counts: None,
            notes: None,
            screen_area: Rect::ZERO,
            given_statuses: None,
            listing: Vec::new(),
            history: history_view::Showing::default(),
            searching: Vec::new(),
            blames: std::collections::HashMap::new(),
            committed: None,
            asking_blame: std::collections::HashSet::new(),
            row_syntax: std::collections::HashMap::new(),
            searched: None,
            looking: search::Looking::default(),
            outside: false,
            search_generation: std::sync::Arc::default(),
            history_generation: std::sync::Arc::default(),
            asked_line: None,
            rendered: None,
            theme_before: None,
            note: None,
            walk_generation: 0,
            events: None,
            watcher: None,
            theme_watched: Vec::new(),
            highlights: Highlights::default(),
            editor_area: Rect::ZERO,
            // Read once. Nothing later asks the operating system again, so
            // every path obelus shows is relative to the same root for the
            // whole session even if something else changes the process's
            // directory.
            working_directory: std::env::current_dir().unwrap_or_default(),
            list_at_start: false,
            should_quit: false,
        }
    }

    /// Whether the loop should stop.
    #[must_use]
    pub const fn should_quit(&self) -> bool {
        self.should_quit
    }

    /// Asks the loop to stop after this iteration.
    pub fn request_quit(&mut self) {
        // Not while something is unwritten: the reader is asked, because
        // "press it again" is an answer that has to be guessed at, and the
        // two things they might have meant -- write them, or let them go --
        // are not the same key twice.
        let unsaved = self
            .buffers
            .iter()
            .flatten()
            .filter(|buffer| buffer.is_dirty())
            .count();
        if unsaved > 0 {
            self.ask_before_leaving(unsaved);
            return;
        }
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
    pub const fn theme(&self) -> &Theme {
        &self.theme
    }

    /// And the name they answer to, which is what the settings hold.
    #[must_use]
    pub fn theme_name(&self) -> &str {
        &self.theme_name
    }

    /// Switches theme.
    ///
    /// Takes effect on the next frame and costs nothing else: what is cached
    /// per byte is which kind of thing it is, not what colour, so there is no
    /// reparse and nothing to invalidate.
    pub fn set_theme(&mut self, name: &str, theme: Theme) {
        self.theme_name = name.to_string();
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

    /// Puts the application on a tree.
    ///
    /// Before the settings are read, always: a tree has settings of its
    /// own and a theme beside them, and finding those means knowing which
    /// tree first. [`App::load_config`] lays the tree's answers over the
    /// reader's at the end, so this only has to have happened by then.
    pub fn work_in(&mut self, root: PathBuf) {
        self.working_directory = root;
    }

    /// Says to open on the file list rather than on a file.
    pub fn list_at_start(&mut self) {
        self.list_at_start = true;
    }

    /// The document being read, if any is open.
    #[must_use]
    pub fn current_buffer(&self) -> Option<&Buffer> {
        self.buffers.get(self.current?.get())?.as_ref()
    }

    /// The same, to change.
    pub fn current_buffer_mut(&mut self) -> Option<&mut Buffer> {
        self.buffers.get_mut(self.current?.get())?.as_mut()
    }

    /// Starts everything that needs the loop's channel.
    ///
    /// Best effort throughout: a watcher that will not start, or a language
    /// server that is not installed, is logged and then done without.
    /// Refusing to run because a convenience is missing would trade it for a
    /// missing program.
    ///
    /// Public because it is the whole of what starting means, and a test
    /// about what obelus does on the way up has nothing else to call.
    pub fn start(&mut self, sender: std::sync::mpsc::Sender<Event>) {
        self.events = Some(sender.clone());
        self.start_watching(sender);
        for index in 0..self.buffers.len() {
            self.serve(index);
        }
        // Last, and here rather than at the command line: the rows come
        // from a walk that sends on this channel, so a list opened before
        // there was one would be a list nothing ever fills.
        if self.list_at_start {
            self.open_file_picker();
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

    /// The region the editor was last drawn in.
    ///
    /// For a test that has to ask where something on screen is, which is
    /// the one question a test about the pointer cannot avoid.
    #[must_use]
    pub const fn editor_area_for_test(&self) -> Rect {
        self.editor_area
    }

    /// Starts a language server for the open file, as the loop does.
    ///
    /// [`App::events_for_test`] deliberately leaves this out -- most tests
    /// want a channel and no subprocesses -- so a test that is about
    /// talking to a real server asks for it by name.
    pub fn serve_for_test(&mut self) {
        for index in 0..self.buffers.len() {
            self.serve(index);
        }
    }

    /// Which walk of the history the list is waiting for.
    ///
    /// So that a test can hand the list a batch by hand and have it taken
    /// for the answer it is waiting for. Batches arrive on a clock, and a
    /// test that waited for one would be a test that sometimes did not.
    #[must_use]
    pub fn history_walk_for_test(&self) -> u64 {
        self.history_generation
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// How many buffers are open, for a test that wants to know whether a
    /// key that had nowhere to go left one behind anyway.
    #[must_use]
    pub fn buffer_count_for_test(&self) -> usize {
        self.buffers.iter().flatten().count()
    }

    /// Puts the application on a tree of the test's choosing.
    ///
    /// The working directory is read from the process once, at startup, and
    /// a test inherits whatever directory the test runner was started in --
    /// which is the checkout's own path, and differs between a clone, a
    /// worktree and somebody else's machine. Anything that *shows* that path
    /// is then a golden grid that passes where it was written and nowhere
    /// else, so a test that renders one says which tree it is on.
    pub fn working_directory_for_test(&mut self, root: PathBuf) {
        self.work_in(root);
        // And whatever that tree has to say about the settings, which is
        // what putting obelus on a tree means: at startup the two happen
        // together, and a test that moved one without the other would be
        // testing an application no reader can have.
        self.apply_tree();
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
        // And what git keeps its state in, because obelus is not the only
        // thing in the repository: a commit in another window, or in a
        // shell, changes what has changed in every file on screen. The
        // margin would otherwise go on showing a diff against a commit that
        // is no longer the one the file is against.
        for path in crate::git::state_of(&self.working_directory) {
            if let Err(error) = watcher.watch(&path) {
                tracing::warn!(%error, path = %path.display(), "not watching the repository");
            }
        }
        // And the settings, because obelus is not the only obelus. Several
        // of them on one project is the ordinary way to work -- the
        // terminal splits the window, obelus does not -- so a setting
        // changed in one of them is a setting changed for all of them, and
        // a file read once at startup would leave every other window
        // holding what the reader has already moved on from.
        if let Some(path) = self.settled.path.clone() {
            // And whatever it really names, which for a reader who keeps
            // their settings in a dotfiles repository is a file in there:
            // what a `git pull` rewrites is that one, and a watch on the
            // link's own directory would never hear about it. Both, because
            // the link itself can be replaced too -- by the thing that made
            // it -- and that is a change to these settings as well.
            for path in [crate::config::resolved(&path), path] {
                if let Err(error) = watcher.watch(&path) {
                    tracing::warn!(%error, path = %path.display(), "not watching the settings");
                }
            }
        }
        // And the tree's own settings, for the same reason twice over:
        // another obelus on this project may be looking at them, and a `git
        // pull` rewrites them under everybody.
        // The file the tree *would* have, not the one it has: watching only
        // what was there at startup is the "read once" mistake with a longer
        // fuse, because it looks right until somebody creates the file --
        // the window next door writing the project's first setting, or a
        // pull bringing one.
        let tree = crate::config::tree_path_for(&self.working_directory);
        if let Err(error) = watcher.watch(&tree) {
            tracing::warn!(%error, path = %tree.display(), "not watching the tree's settings");
        }
        self.watcher = Some(watcher);
        // And wherever the colours come from, which is its own question:
        // a theme is a file obelus never writes and something else may
        // replace under it.
        self.watch_theme();
    }

    /// The open picker, for the renderer.
    #[must_use]
    pub const fn picker(&self) -> Option<&Picker> {
        self.picker.as_ref()
    }

    /// What the server says is wrong with the file being read.
    ///
    /// In the order the server sent them, which is the order they are in
    /// the file for every server obelus talks to.
    #[must_use]
    pub fn troubles(&self) -> &[crate::lsp::trouble::Trouble] {
        self.current_buffer()
            .and_then(|buffer| self.troubles.get(buffer.path()))
            .map_or(&[], Vec::as_slice)
    }

    /// What the call the cursor is inside takes, while it is showing.
    ///
    /// Not while the completion panel is up: the two would be drawn in the
    /// same place, and what could be typed next is the nearer question.
    #[must_use]
    pub const fn signature(&self) -> Option<&crate::lsp::signature::Signature> {
        match self.completion.is_some() {
            true => None,
            false => self.signature.as_ref(),
        }
    }

    /// What the server says the place under the caret is, while it is up.
    ///
    /// Not while either of the other two panels is: all three want the
    /// cells beside the cursor, and of the three this is the question
    /// asked longest ago.
    #[must_use]
    pub const fn hover(&self) -> Option<&Hover> {
        match self.completion.is_some() || self.signature.is_some() {
            true => None,
            false => self.hover.as_ref(),
        }
    }

    /// What could be typed next, while a server's answer is on screen.
    #[must_use]
    pub const fn completion(&self) -> Option<&Completion> {
        self.completion.as_ref()
    }

    /// The room a list is drawn in, which is not always the editor region.
    ///
    /// Over a conversation it is everything above the box: the list is a
    /// list of what is being typed there, and it may not cover it.
    fn picker_room(&self) -> Rect {
        match self.chat() {
            Some(chat) => ui::chat::above_writing(self.editor_area, chat),
            None => self.editor_area,
        }
    }

    /// Whether anything on screen is moving.
    ///
    /// One question, because one screen animates at a time: the welcome
    /// screen's sheen while there is nothing open, and an agent at work
    /// while the conversation is showing. Asked every frame from what is
    /// true, rather than switched on and off from the half-dozen places
    /// that change either, which is how a ticker outlives its reason.
    const fn wants_animating(&self, working: bool) -> bool {
        match self.showing_chat {
            true => working,
            false => self.current.is_none(),
        }
    }

    /// Whether the last frame asked to be woken again.
    ///
    /// Which is the difference between a tree that catches up on its own
    /// and one that waits for the reader to press something else.
    #[must_use]
    pub const fn is_waking(&self) -> bool {
        self.waking
    }

    /// Whether any open document's tree is older than its text.
    fn anything_behind(&self) -> bool {
        self.buffers.iter().flatten().any(Buffer::syntax_is_behind)
    }

    /// Works out what every document that owes it means now.
    ///
    /// Everything open rather than what is on screen: a tree left behind on
    /// a document nobody is looking at would keep the ticker awake for the
    /// rest of the session.
    pub(super) fn settle_syntax(&mut self) {
        // Nothing else has to be told: the text did not move, only what
        // obelus knows about it, so everything keyed on the version stays
        // keyed on the version it already had.
        for buffer in self.buffers.iter_mut().flatten() {
            buffer.settle_syntax();
        }
    }

    /// Starts or stops the ticker, and does nothing where it is already
    /// what it should be.
    ///
    /// A thread waking twelve times a second to redraw a screen with
    /// nothing moving on it is the one cost an animation must not have.
    fn animate(&mut self, wanted: bool) {
        self.waking = wanted;
        match (wanted, self.ticker.is_some()) {
            (true, false) => self.ticker = self.events.clone().and_then(Ticker::start),
            (false, true) => self.ticker = None,
            _ => {}
        }
    }

    /// Which set of key bindings a key is looked up in.
    ///
    /// What the reader is in, rather than what they are doing: a dialog
    /// takes the keys bound in it and no others, so obelus's own commands
    /// cannot open a second dialog over the first -- `ctrl+o` in a
    /// conversation used to put a file list on top of it, which then took
    /// two escapes to leave and gave no way to tell which of the two a key
    /// would reach.
    pub(super) fn context(&self) -> Context {
        // A list whose rows are open files is the list of open files, and
        // that one has a command of its own.
        if self.selected_buffer().is_some() {
            return Context::Buffers;
        }
        if self.is_showing_dialog() {
            return Context::Dialog;
        }
        Context::Normal
    }

    /// Whether something is showing that the reader is *in*.
    ///
    /// A list, the settings, or a conversation with an agent: each takes
    /// the keys itself, each is left with escape, and none of them is a
    /// file being read. Obelus's own commands do not run from inside one,
    /// so the only way to a second one is to leave the first.
    ///
    /// The question on the status bar is not one of these. It is a row
    /// rather than a screen, what it is asking about is still visible
    /// behind it, and it says its own answer to escape.
    #[must_use]
    pub const fn is_showing_dialog(&self) -> bool {
        self.picker.is_some()
            || self.settings.is_some()
            || self.counts.is_some()
            || self.notes.is_some()
            || self.showing_chat
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
                // The margin on the left and the change map on the right
                // both appear only for a file in a repository, and they
                // appear together: they are the same answer at two scales.
                // The fold column is its own condition, and it is asked
                // through `text_offset` so that this and the view cannot
                // disagree about what comes before the text -- a width one
                // cell wider than the view draws wraps a line here and not
                // there, and the caret then sits a row below the character
                // it is on.
                let before = ui::editor::text_offset(
                    buffer.text().line_count(),
                    self.changes.is_some(),
                    !buffer.folds().is_empty(),
                );
                let after = if self.changes.is_some() {
                    ui::editor::CHANGE_MAP_WIDTH
                } else {
                    0
                };
                self.editor_area
                    .width
                    .saturating_sub(before)
                    .saturating_sub(after)
                    .saturating_sub(ui::editor::SCROLLBAR_WIDTH)
            }
            None => self.editor_area.width,
        };
        TextArea {
            width,
            height: self.editor_area.height,
            wrap: self.settled.config.wrap,
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
        // The notes are laid out against the room they have: a terminal is
        // resized and a setting is changed while they are open, and the rows
        // they are made of depend on both.
        let laid = self.notes_laid_out();
        if let Some(notes) = self.notes.as_mut() {
            notes.lay_out(laid.0, laid.1);
        }
        self.editor_area = editor_area;
        self.check_servers();
        // What the conversation says is happening, read off the state
        // rather than remembered: a row that is worked out every frame
        // cannot be left saying something that stopped being true.
        let doing = match self.talking() {
            talking::Talking::Starting => Some("starting\u{2026}"),
            talking::Talking::Thinking => Some("thinking\u{2026}"),
            talking::Talking::Nobody
            | talking::Talking::Idle
            | talking::Talking::Ready
            | talking::Talking::Gone => None,
        };
        self.chat.doing(doing);
        // A grammar too slow to keep up with typing leaves a tree owing an
        // answer, and the ticker is what comes back for it: the reader
        // stops, the next tick lands, and the colours catch up.
        self.animate(
            self.wants_animating(doing.is_some()) || self.anything_behind() || self.is_resting(),
        );

        // Which rows the list will draw is what decides which rows need
        // their matched characters worked out, and only the geometry knows
        // how many rows there are. The room is the room it is *drawn* in,
        // which over a conversation is everything above the box.
        let rows = self
            .picker
            .as_ref()
            .map(|picker| ui::picker::rows_drawn(picker, self.picker_room()));
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
        self.refresh_slash();

        // Less the scrollbar's column, which the drawing keeps for itself:
        // a reading laid out for the whole width would have its last cell
        // clipped, and a box drawn round a block of code would lose the
        // side that closes it.
        // The panel is checked against the document rather than told about
        // every way the document can move: a cursor that has left the word
        // is a panel about somewhere else.
        self.settle_completion();
        // And the answer about a place, which the pointer resting is what
        // asks for: this is where the resting is noticed.
        self.settle_hover();
        if let Some(hover) = self.hover.as_mut() {
            // What it is drawn in, so that paging it moves what is on
            // screen rather than a number nothing reads.
            hover.settle(
                ui::hover::room(editor_area),
                crate::component::hover::MOST_ROWS,
            );
        }
        // How much room the panel's two halves have, which the keys need
        // as much as the drawing does: a page of documentation is the rows
        // of it that are on screen, and only the geometry knows how many
        // that is.
        if let Some(panel) = ui::complete::layout(self, editor_area)
            && let Some(completion) = self.completion.as_mut()
        {
            // The width inside the box, less the column the reading keeps
            // for its scrollbar: laid out for cells it does not get, the
            // last of every row would be clipped.
            completion.settle_documentation(
                panel
                    .area
                    .width
                    .saturating_sub(2)
                    .saturating_sub(ui::editor::SCROLLBAR_WIDTH),
            );
            completion.settle(panel.list, panel.documentation);
        }

        self.refresh_rendering(
            editor_area
                .width
                .saturating_sub(crate::ui::editor::SCROLLBAR_WIDTH),
        );
        self.refresh_changes();
        self.refresh_blame();
        // After the changes, because the room the text has includes the
        // rows an opened hunk draws: the arithmetic counts them, so nothing
        // here has to make up for them.
        let area = self.text_area();
        // Only where the viewport is a place in the *text*. While a reading
        // is showing, the viewport's top is a row of that reading -- and a
        // reading has more rows than the file has lines, because it wraps
        // -- so the text's own arithmetic would clamp the top to the line
        // count and put the last rows out of reach. The reading's own
        // scrolling is what keeps it in bounds there.
        if let Some(buffer) = self
            .current_buffer_mut()
            .filter(|buffer| buffer.mode() == crate::buffer::Mode::Edit)
        {
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
        self.screen_area = area;
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
            Event::FileChanged { path } => {
                // The settings, by either of their names: the watcher
                // reports whichever path the change arrived on, and a
                // change that came from a repository arrives on the file
                // the link points at rather than on the link.
                let readers = self.settled.path.as_deref().is_some_and(|config| {
                    path == config || path == crate::config::resolved(config)
                });
                // Or the tree's own, which is a change to the settings just
                // as much -- it is the layer over them. Against the file the
                // tree *would* have rather than the one it has, so that the
                // file appearing is a change like any other: the ordinary
                // case is a project with no settings yet, and the moment
                // worth hearing about is the one where it gets some.
                let tree = path == crate::config::tree_path_for(&self.working_directory);
                if readers || tree {
                    self.reread_config();
                } else if self.is_a_theme(&path) {
                    // The colours the reader is already wearing, read again:
                    // the name in the settings has not moved, and what it
                    // stands for has.
                    self.reread_theme();
                } else if crate::git::state_moved(&path) {
                    self.forget_what_git_said();
                } else {
                    self.reload_path(&path);
                }
                // And the servers, whatever it was: a file changing on
                // disk is news to them as much as to obelus -- a branch
                // checked out, a build script's output, an editor
                // somewhere else. Some of them watch for themselves and
                // will have heard already; the protocol's own answer is
                // that the client says so, and a server that relies on it
                // is otherwise answering about a file nobody has.
                self.told_servers_about(&path);
            }
            Event::Lsp { language, message } => {
                let Some(client) = self.servers.get_mut(&language) else {
                    return;
                };
                // Everything the protocol needs rather than obelus — the
                // handshake, progress, the server's own log lines — is dealt
                // with in there.
                let reply = client.on_message(&message);
                // Unasked-for news about a file, which arrives on the same
                // pipe as the answers and belongs to nobody's question.
                let published = client.take_published();
                // And the edits it wants made, which arrive the same way
                // and are answered by making them.
                let asked = client.take_asked_edits();
                for params in published {
                    self.on_published(language, &params);
                }
                for edit in &asked {
                    self.on_asked_edit(language, edit);
                }
                if let Some(reply) = reply {
                    self.on_reply(language, reply);
                }
            }
            Event::Counted(counted) => self.on_counted(*counted),
            Event::Scroll(rows) => self.scroll(rows),
            Event::Pointer { kind, x, y } => self.on_pointer(kind, x, y),
            // One change for the whole of it, so undoing a paste is one
            // step rather than however many lines it happened to be.
            Event::Paste(text) => self.paste_text(&text),
            Event::Tick => {
                self.phase = self.phase.wrapping_add(1);
                // The pause the slow grammars are waiting for. A tick that
                // lands mid-word settles the tree that word began in, which
                // is one parse for a burst of typing rather than one per
                // key.
                self.settle_syntax();
            }
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
            Event::Blamed { path, at, lines } => {
                // Kept whether or not the reader is still looking at that
                // file: they walked away from it while a walk of its history
                // was running, and they will walk back.
                self.asking_blame.remove(&(path.clone(), at));
                self.blames.insert((path.clone(), at), lines);
                // And if this is the answer somebody pressed a key for,
                // that key finishes now rather than needing pressing again.
                if let Some((asked, version, line)) = self.asked_line.take()
                    && (asked, version) == (path, at)
                {
                    self.open_line_commit_at(line);
                }
            }
            Event::Logged {
                generation,
                commits,
                walked,
                done,
            } => {
                // A batch from a walk whose list is gone, or from one
                // superseded by another tab, another file, another key.
                if generation
                    == self
                        .history_generation
                        .load(std::sync::atomic::Ordering::Relaxed)
                {
                    self.on_logged(commits, walked, done);
                }
            }
            Event::FilesFound {
                generation,
                paths,
                ignored,
            } => {
                // A batch from a walk whose picker is gone, or from one
                // superseded by a later open.
                if generation != self.walk_generation {
                    return;
                }
                if let Some(picker) = self.picker.as_mut() {
                    let statuses = &self.statuses;
                    let root = &self.working_directory;
                    picker.extend(paths.into_iter().map(|path| PickerItem {
                        prose: false,
                        marker: None,
                        icon: Some(icons::for_path(&path)),
                        label: path.display().to_string(),
                        detail: None,
                        trailing: None,
                        changed: None,
                        value: PickerValue::File(path.clone()),
                        enabled: true,
                        colours: None,
                        // Git says nothing about a file it was told to
                        // ignore -- `git status` leaves them out -- so the
                        // walk that went looking is what says it.
                        status: match ignored {
                            true => Some(crate::git::FileStatus::Ignored),
                            false => statuses.get(&root.join(&path)).copied(),
                        },
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
        // Whatever obelus had to say has been read by now, or was not going to
        // be.
        self.note = None;

        // The picker gets first refusal, because the keys it wants are the
        // ones that move the thing it owns. What it does not want falls
        // through, which is how `ctrl+q` still works with one open.
        // A page is the rows actually on screen, which is the region the
        // list is *drawn* in -- not the region it was offered. A full-area
        // list gives half of that to a preview, and a page of the whole
        // editor would walk the selection twice as far as the reader can
        // see.
        let page = self.picker.as_ref().map_or(1, |picker| {
            ui::picker::rows_drawn(picker, self.picker_room())
        });
        // Except the paging keys, while a preview is on screen: a screenful
        // is what the thing being *read* is moved by, and the list above it
        // is ten rows with its ends a keypress away. With control they page
        // the list, which is the other half of the same swap.
        if self.page_preview(&key) {
            return;
        }
        // The card an agent's question is answered on, which is nearer
        // than anything else on screen: it covers the box a message would
        // be written in, because while the agent is waiting on an answer
        // there is no message to send.
        if self.card_key(&key) {
            return;
        }

        // The keys a file list and a search have that are not about moving
        // around them. Before the picker, because the picker would not know
        // them: what they change is where the rows come from, which is the
        // application's.
        if self.listing_key(&key) || self.searching_key(&key) {
            return;
        }

        if let Some(picker) = self.picker.as_mut() {
            // What a search is asking, before and after the key. The picker
            // owns the query and the tab and knows nothing about where rows
            // come from, so the application watches those two for movement
            // rather than the picker reporting it.
            let searching = picker.is_searching();
            let listing = picker.is_listing();
            // A history has tabs too, and walking onto one is what asks its
            // question: the commits of a file and of a project are two
            // answers, not two views of one.
            let historic = !self.history.radii.is_empty();
            let before = (picker.tab(), picker.query().to_string());
            let outcome = picker.handle_key(&key, page);
            let after = (picker.tab(), picker.query().to_string());
            match outcome {
                PickerOutcome::Consumed => {
                    if searching && after != before {
                        self.refresh_search();
                    }
                    if listing && after.0 != before.0 {
                        self.refresh_listing();
                    }
                    if historic && after.0 != before.0 {
                        self.refresh_history();
                    }
                    return;
                }
                PickerOutcome::Cancelled => {
                    self.picker = None;
                    self.history = history_view::Showing::default();
                    // A list that was an agent's question has to be
                    // answered even when the reader walks away from it: an
                    // agent whose permission request goes unanswered waits
                    // for ever.
                    if self.is_asking_permission() {
                        self.refuse_permission();
                    }
                    // And so does a form: a field left unanswered is the
                    // whole form declined, because the agent is waiting on
                    // all of it.
                    if self.is_asking() {
                        self.refuse_asking();
                    }

                    // A theme previewed but not chosen. Nothing else a picker
                    // shows changes the application while it is open, so
                    // nothing else has to be put back.
                    if let Some((name, before)) = self.theme_before.take() {
                        self.set_theme(&name, before);
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
            // Cloned for the same reason: the page needs the table to say
            // which key each command is on, and it is the application that
            // owns it.
            let keymap = self.keymap.clone();
            let Some(settings) = self.settings.as_mut() else {
                return;
            };
            let room = (self.editor_area.width, self.editor_area.height);
            let outcome = settings.handle_key(&key, &self.settled.config, &keymap, &listed, room);
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
                SettingsOutcome::Unset(key) => {
                    self.unset_setting(key);
                    return;
                }
                SettingsOutcome::Bind(command, chord) => {
                    self.rebind(command, chord);
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

        // The counts take what the settings did not. They are a dialog with
        // nothing to type into, so what they take is the keys that walk a
        // list and the two that leave it -- and everything else falls
        // through to the table, where nothing is bound in a dialog.
        // Not while a prompt is open over it: the prompt is a thing the
        // reader is *in*, and a list that went on taking enter and space
        // underneath it would swallow the answer and tick something.
        if self.prompt.is_none() && self.notes.is_some() && self.notes_key(&key) {
            return;
        }

        if self.counts.is_some() && self.counts_key(&key) {
            return;
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
            let room = ChatRoom {
                transcript: ui::chat::bands(self.editor_area, &self.chat, self.card.as_ref())
                    .transcript
                    .height,
                reading: ui::chat::reading_width(self.editor_area),
                writing: width,
            };
            // The list of the agent's own commands, when one is showing:
            // it follows what is being typed in the box, so it takes the
            // keys that move about a list and leaves the rest to the box.
            if self.slash_key(&key) {
                return;
            }
            // What the agent lets the reader change, which is what the row
            // under the box is showing -- so the keys that walk it need it
            // as much as the view does. Cloned because the box is about to
            // be borrowed to take the key.
            let settings = self.agent_settings().to_vec();
            match self.chat.handle_key(&key, thinking, room, &settings) {
                ChatOutcome::Consumed => return,
                ChatOutcome::Cancelled => {
                    // Escape gives up on the nearest thing first, and a
                    // question the agent is waiting on is nearer than the
                    // conversation it was asked in.
                    if self.is_asking() {
                        self.refuse_asking();
                        return;
                    }
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
                // Where a row of the transcript says the agent was. The
                // conversation stays as it was behind it: a reader who
                // followed the agent into a file is still in the
                // conversation about that file, and escape brings it back.
                ChatOutcome::GoTo(place) => {
                    // The protocol counts a file's lines from one and the
                    // rest of obelus counts them from zero, which is what
                    // `go_to` takes: a language server's numbering, because
                    // that is who it was written for.
                    let line = place.line.unwrap_or(1).saturating_sub(1);
                    self.go_to(&place.path, line, 0);
                    // And out of the way, because going somewhere means
                    // seeing it: the conversation is the whole region while
                    // it is showing. It is hidden rather than ended, so the
                    // key that opens it brings back every word of it.
                    //
                    // Only if there is something to see, though: a file an
                    // agent named can have gone away, and hiding the
                    // conversation to show a file that never opened would
                    // take away the only thing on screen.
                    if self
                        .current_buffer()
                        .is_some_and(|buffer| buffer.path() == place.path)
                    {
                        self.close_chat();
                    }
                    return;
                }
                ChatOutcome::Choose(id) => {
                    self.open_agent_setting(&id);
                    return;
                }
                ChatOutcome::Toggle(id) => {
                    self.flip_agent_setting(&id);
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

        // What the server said about a place. Before the panels below it
        // because escape belongs to whatever is nearest, and it takes no
        // other key from them: what it does not want, it closes itself for
        // and lets through.
        if self.hover_key(&key) {
            return;
        }
        // What could be typed next, while a server's answer is beside the
        // cursor. Before the motions and the typing, because the arrows
        // walk the list and `enter` takes what is selected -- and after
        // everything above, because a list or a dialog open over the file
        // is what the reader is looking at instead.
        if self.completion_key(&key) {
            return;
        }
        // And the holes a snippet left, which take `tab` while there are
        // any left to fill in.
        if self.snippet_key(&key) {
            return;
        }

        // A rendering scrolls by rows. Its rows are not the file's lines, so
        // the cursor has nowhere to be in it and the motions have nothing to
        // move: what the keys do here is move the window.
        if self.picker.is_none()
            && let Some(rows) = self.rendered_rows()
            && let Some(step) = view_step(&key, self.editor_area.height)
        {
            let height = self.editor_area.height;
            if let Some(buffer) = self.current_buffer_mut() {
                buffer.scroll_rendering(step, rows, height);
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

        // What a key puts into the document. After the motions, which have
        // the arrows and the ends of a line, and before the table, which has
        // the chords: a bare character is neither of those, and `Backspace`,
        // `Delete`, `Enter` and `Tab` cannot be in the table at all --
        // `why_not` refuses them, because every list and box takes them
        // itself.
        if self.picker.is_none()
            && self.settings.is_none()
            && !self.showing_chat
            && let Some(typing) = crate::editing::typing_for(&key)
        {
            self.typed(typing);
            // A letter is a reason to ask what could follow it; everything
            // else is a reason to stop offering.
            self.after_typing(typing);
            return;
        }

        // A key whose command cannot do its job here does nothing at all.
        // The palette draws such a row dim and refuses to run it; a key is
        // the same row reached another way, and one judgement -- `offers`
        // -- has to answer for both, or a command is off in one place and
        // live in the other. Silence is the answer because the reader has
        // the palette to find out why: `f3` on a tree with nothing changed
        // used to open a list of nothing and say so on the status row,
        // which is a sentence nobody asked for.
        if let Some(command) = self.keymap.lookup(&key, self.context())
            && self.offers(command)
        {
            dispatch::dispatch(self, command);
        }
    }
}

impl App {
    /// What the pointer did to a box on the status row, and whether it was
    /// one of those.
    ///
    /// The nearest thing on screen, so it is asked first -- and it answers
    /// for every click on that row, including one that lands past the end
    /// of what was typed: the row is the box, and a click on it is a click
    /// in the box.
    fn pointer_on_status(&mut self, kind: crate::event::Pointer, x: u16, y: u16) -> bool {
        use crate::event::Pointer;

        let status = ui::regions(self.screen_area).status;
        if status.height == 0 || y != status.y || x < status.x || x >= status.right() {
            return false;
        }
        // Which box is showing, and how far in its text starts. The same
        // order the keys go in by, and the same insets the renderer draws
        // them at.
        let inset = if self.prompt.is_some() {
            self.prompt.as_ref().map(ui::status::answer_inset)
        } else if self.settings.is_some() {
            Some(ui::status::typed_inset(None))
        } else {
            self.picker
                .as_ref()
                .map(|picker| ui::status::typed_inset(picker.question()))
        };
        let Some(inset) = inset else {
            return false;
        };
        let cell = (x - status.x).saturating_sub(inset);
        let clicks = match kind {
            Pointer::Pressed => self.clicks_at(x, y),
            _ => 0,
        };
        match kind {
            // Nothing to do, but the row is still the box's: a move over it
            // must not reach the file underneath.
            Pointer::Moved | Pointer::Released => {}
            Pointer::Dragged => self.place_on_status(cell, true),
            Pointer::Pressed => {
                self.place_on_status(cell, false);
                // Twice is the word and three times is the whole of it,
                // which is what a line has instead of a line.
                match clicks {
                    2 => self.hold_on_status(false),
                    3 => self.hold_on_status(true),
                    _ => {}
                }
            }
        }
        true
    }

    /// Puts the caret of whichever box is on the status row.
    fn place_on_status(&mut self, cell: u16, extend: bool) {
        if let Some(prompt) = self.prompt.as_mut() {
            prompt.place_at_cell(cell, extend);
        } else if let Some(settings) = self.settings.as_mut() {
            settings.place_in_query(cell, extend);
        } else if let Some(picker) = self.picker.as_mut() {
            picker.place_in_query(cell, extend);
        }
    }

    /// Takes hold of a word of it, or of all of it.
    fn hold_on_status(&mut self, all: bool) {
        if let Some(prompt) = self.prompt.as_mut() {
            prompt.hold(all);
        } else if let Some(settings) = self.settings.as_mut() {
            settings.hold_in_query(all);
        } else if let Some(picker) = self.picker.as_mut() {
            picker.hold_in_query(all);
        }
    }

    /// What the pointer did to the file being read.
    ///
    /// Only over the text, and only with nothing else open: a list, the
    /// settings or the conversation is what the screen is showing while it
    /// is up, and a click landing on the code behind one would move a caret
    /// nobody can see.
    fn on_pointer(&mut self, kind: crate::event::Pointer, x: u16, y: u16) {
        use crate::event::Pointer;

        // The boxes on the status row first: a question, a list's query, a
        // page's filter. All three are one row, so one piece of arithmetic
        // serves them -- and a reader who can select in a box with the
        // keyboard but not with the pointer has half a selection.
        if self.pointer_on_status(kind, x, y) {
            return;
        }
        if self.picker.is_some()
            || self.settings.is_some()
            || self.counts.is_some()
            || self.showing_chat
        {
            return;
        }
        let Some(buffer) = self.current_buffer() else {
            return;
        };
        // A reading has no places in it for a caret: its rows are not the
        // file's lines.
        if buffer.mode() != Mode::Edit {
            return;
        }
        let area = self.editor_area;
        if x < area.x || x >= area.right() || y < area.y || y >= area.bottom() {
            return;
        }
        // Everything the view draws in front of the text. A click to the
        // left of it -- on the gutter, a fold mark, the change margin --
        // is a click at the start of that row rather than nothing: the
        // reader pointed at a line.
        let offset = ui::editor::text_offset(
            buffer.text().line_count(),
            self.changes().is_some(),
            !buffer.folds().is_empty(),
        );
        let row = y - area.y;
        let cell = (x - area.x).saturating_sub(offset);
        let text = self.text_area();

        // Where it is, whatever it is doing: the rest that asks a question
        // is measured from the last place it was seen.
        self.pointer_rested(x, y);
        let count = match kind {
            Pointer::Pressed => self.clicks_at(x, y),
            _ => 0,
        };
        match kind {
            // Nothing but where it is, which was noted above.
            Pointer::Moved => return,
            // Dragging is what a reader does to select, so the place they
            // put the button down stays put.
            Pointer::Dragged => {
                if let Some(buffer) = self.current_buffer_mut() {
                    buffer.place_at_cell(row, cell, text, true);
                }
            }
            Pointer::Released => return,
            Pointer::Pressed => {
                if let Some(buffer) = self.current_buffer_mut() {
                    buffer.place_at_cell(row, cell, text, false);
                }
                match count {
                    // Twice is the word, three times is the line: what
                    // every editor with a pointer has taught.
                    2 => self.widen_selection(),
                    3 => {
                        let line = self.current_buffer().map(|buffer| buffer.cursor().line);
                        if let Some(line) = line
                            && let Some(buffer) = self.current_buffer_mut()
                        {
                            buffer.select_line(line);
                        }
                    }
                    _ => {}
                }
            }
        }
        // Wherever the caret ended up is somewhere the reader is now
        // working from, so the next edit is a step of its own to undo.
        if let Some(buffer) = self.current_buffer_mut() {
            buffer.settle_undo();
        }
    }

    /// How many times in a row the pointer has been put down here.
    ///
    /// The same cell and inside the gap below, or the count starts again.
    /// Three is as far as it goes: a fourth press is a first press, which
    /// is what selecting a line and then clicking in it has to be.
    fn clicks_at(&mut self, x: u16, y: u16) -> u8 {
        /// Long enough for a deliberate double click and short enough that
        /// two separate clicks are not taken for one. The figure every
        /// desktop uses.
        const GAP: std::time::Duration = std::time::Duration::from_millis(400);

        let now = std::time::Instant::now();
        let count = match self.clicked {
            Some((was_x, was_y, when, count))
                if (was_x, was_y) == (x, y) && now.duration_since(when) < GAP && count < 3 =>
            {
                count + 1
            }
            _ => 1,
        };
        self.clicked = Some((x, y, now, count));
        count
    }
}

/// The pointer, standing still.
#[derive(Clone, Copy, Debug)]
pub(super) struct Resting {
    /// Which cell of the screen.
    pub(super) x: u16,
    pub(super) y: u16,
    /// When it arrived there.
    pub(super) since: std::time::Instant,
    /// Whether this rest has asked what is under it.
    pub(super) asked: bool,
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
    let folds = buffer.folds();
    let top = buffer.viewport().top;
    // Walked the way the view walks it, past whatever is folded away. A
    // count of `height` *file* lines is the same thing only while nothing
    // is folded: with a run of two hundred lines closed at the top of the
    // screen, the rows below it are lines two hundred further down, and a
    // range that stopped at `top + height` would leave every one of them
    // outside what has been highlighted -- which is not a subtle failure.
    // The code below the fold is simply drawn in the plain foreground.
    //
    // A bound rather than an exact answer: a wrapped line takes more than
    // one row, so this can reach further than the screen does. Covering too
    // much costs a little query time and nothing else; covering too little
    // costs the colours.
    let mut line = folds.first_shown(top);
    let mut rows = 0;
    while rows < usize::from(height) && line.get() < text.line_count() {
        rows += 1;
        line = folds.first_shown(line.saturating_add(1));
    }
    let start = text.line_start_byte(top);
    let end = if line.get() >= text.line_count() {
        text.byte_length()
    } else {
        text.line_start_byte(line)
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

/// What the command line asked obelus to open.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Opening {
    /// The tree to work in, absolute, where the arguments named one.
    ///
    /// `None` for no arguments at all, which leaves the directory obelus
    /// was started in -- the shell's answer to the same question.
    pub root: Option<PathBuf>,
    /// The files to open, in the order they were given.
    pub files: Vec<PathBuf>,
    /// Whether to open on the file list.
    pub list: bool,
}

/// What a set of command-line paths means.
///
/// A file names the tree it is in and is opened; a directory *is* the
/// tree, and the question it leaves -- which file -- is the one the list
/// answers. The first path decides the tree, because a reader who names
/// two has said which they meant first.
///
/// Absolute, and by the same rule [`crate::buffer::Buffer::open`] uses on
/// a file: made absolute rather than canonical, so a tree reached through
/// a symlink is still shown under the name the reader typed. A relative
/// root would fail quietly -- every path obelus shows is worked out by
/// stripping this off an absolute one, and git is asked about it from a
/// process whose own directory nothing here controls.
#[must_use]
pub fn opening(paths: &[PathBuf]) -> Opening {
    let Some(first) = paths.first() else {
        return Opening::default();
    };
    // Anything that is not a directory is a file to open, including one
    // that is not there: what to say about a path that cannot be read is
    // `Buffer::open`'s to say, and it says it better than this could.
    let files: Vec<PathBuf> = paths
        .iter()
        .filter(|path| !path.is_dir())
        .cloned()
        .collect();
    let root = match first.is_dir() {
        true => first.clone(),
        // The directory the file is in. Absolute first, because the parent
        // of a bare `main.rs` is nothing at all.
        false => absolute(first)
            .parent()
            .map_or_else(|| absolute(first), Path::to_path_buf),
    };
    Opening {
        root: Some(absolute(&root)),
        // Nothing to open means the list is the whole answer: `ob src`
        // is a reader saying which tree and asking which file.
        list: files.is_empty(),
        files,
    }
}

/// A path from the command line, made absolute against where obelus was
/// started.
fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
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
        // Timed, because a frame blocking the loop is the one performance
        // risk this design took knowingly: `draw` writes to stdout, and
        // over ssh or in tmux that write can wait on the far end. A line
        // here is the evidence for taking the escape hatch -- a thread of
        // its own for the terminal -- rather than a hunch about it.
        let started = std::time::Instant::now();
        render(terminal, app)?;
        let took = started.elapsed();
        if took >= SLOW_FRAME {
            tracing::debug!(?took, "a slow frame");
        }

        let Ok(event) = events.recv() else {
            // Every sender is gone, so no further event can arrive. Which
            // is not how obelus is meant to end -- the reader asks -- so it
            // says so: the keyboard's thread has died.
            tracing::warn!("nothing is left to send events, so there is nothing to wait for");
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
