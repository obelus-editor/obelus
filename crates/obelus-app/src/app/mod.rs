//! Application state, and the loop that drives it.
//!
//! `App` holds everything Obelus knows and everything it can be asked to
//! do, so its methods are as many as the things Obelus does. They are split
//! across this directory by *what they are about* -- the file being read,
//! the language server, the repository, the search, the settings -- rather
//! than by size, and each file is one `impl App` block. Nothing moved
//! between types to do it: an application's state is one thing, and cutting
//! it into several would mean deciding, for every pair of them, which one
//! owns the answer.
//!
//! The files are named for the *aspect*, not for the module they talk to:
//! `obelus_git` is the reading of a repository and `history` here is what
//! Obelus does with what it reads. What is left in this file is the state
//! itself and the loop; working in a project (`lifecycle`), what a frame is
//! drawn from (`frame`), where an event and a key go (`handling`), the
//! pointer, what moves (`animation`), what is open over the file
//! (`layering`) and what the renderer may ask (`screen`) are files beside
//! it.
mod animation;
mod chat;
pub mod dispatch;
pub mod document;
mod editor;
mod frame;
mod git;
mod handling;
mod hearing;
mod layering;
mod lifecycle;
mod lsp;
mod pointer;
mod project;
mod saying;
mod screen;
mod switching;
mod terminals;

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use anyhow::Result;
pub use chat::{agents, talking};
use chat::{conversations, headless, mirroring, opening, relaying, remote};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use document::Document;
use documents::Rendered;
use editor::{documents, keys, previewing};
pub use git::pulls;
use git::{history, history_view, worktrees};
pub use headless::run_headless;
use history::Changed;
pub use history_view::About;
use lsp::semantics;
use obelus_agent::{Listed, Talking, acp};
use obelus_buffer::{Buffer, Cursor, DocumentId, Mode, Motion, TextArea};
use obelus_command::{Command, Requires};
use obelus_component::{
    card::Card,
    chat::{Chat, ChatOutcome, Room as ChatRoom},
    completion::Completion,
    counts::Counts,
    hover::Hover,
    layers::{self, Layer, Room},
    picker::{
        Colouring, Listing, Marking, Picker, PickerItem, PickerLayout, PickerOutcome, PickerValue,
        Remark,
    },
    prompt::{Prompt, PromptKind, PromptOutcome},
    settings::{Settings, SettingsOutcome},
    todo::TodoView,
};
use obelus_editing::motion_for;
use obelus_keymap::{Context, KeyChord, Keymap};
use obelus_lsp::{
    action,
    action::{Outcome, SymbolAction},
    client::{Client, Reply},
    position,
};
use obelus_reading::Reading;
use obelus_search::Scope;
use obelus_syntax::{LanguageId, brackets, highlight::Highlights, parse::SyntaxState, tags};
use obelus_text::coordinates::{ByteOffset, CharColumn, LineNumber, Span};
use obelus_theme::{Theme, builtin};
use obelus_ui::{Previewed, Screen, image::Images};
use obelus_watch::Watcher;
use previewing::Preview;
use project::{preferences, projects, releases, reopening};
use ratatui::{
    Terminal,
    backend::Backend,
    buffer::Buffer as CellBuffer,
    layout::{Position, Rect},
    style::Color,
};
use semantics::{Asked, Question, named as server_named};
pub use worktrees::{Door, Windows, knock};

use crate::{
    event::{Event, Ticker},
    jump::{Jump, JumpList},
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
pub(crate) struct Committed {
    /// Which file, and which commit it is being compared with.
    pub(crate) of: (PathBuf, Option<gix::ObjectId>),
    /// What that commit had in it.
    pub(crate) text: Option<obelus_git::Base>,
}

/// A drag held against the edge of what it is selecting in.
///
/// What is kept is where the pointer is and how far past the edge, not
/// which end of what: the scrolling goes through the same one function a
/// notch of the wheel does, so it reaches whatever the reader is dragging
/// in without this having to know which that is.
#[derive(Clone, Copy, Debug)]
struct Dragging {
    /// How far past the edge, in rows. Below is positive and above is
    /// negative, and the further past, the faster -- which is the only
    /// speed a pointer held still can ask for.
    past: i16,
    /// Where the pointer is, so the far end of the selection can be put
    /// there again once the rows underneath it have moved.
    x: u16,
    /// The same.
    y: u16,
}

/// Everything Obelus is currently showing or remembering.
#[derive(Debug)]
pub struct App {
    /// The notes, as last read, and the wait for a burst of writes to settle.
    notes: project::Notes,
    /// Which conversations other windows have had and hold, as last read.
    kept: chat::Kept,
    /// The agent this window talks to, and what is running on its behalf.
    agent: chat::Agent,
    /// What the pointer is doing and what the last frame left for it to land
    /// on.
    pointing: pointer::Pointing,
    /// What git has said about the files being read, kept until it moves.
    git: git::Said,
    /// The terminals Obelus has started, and the one it signs an agent in on.
    terminal: terminals::Terminals,
    /// The page that asks which project, while it is up.
    which_project: project::Asking,
    /// The project's files, as the lists of them have walked them.
    files: editor::Files,
    /// What is being searched for, and where.
    search: editor::Search,
    /// What the language server has said about the file being read, and what
    /// is waiting on it.
    lsp: lsp::State,
    keymap: Keymap,
    /// Everything opened this session, with a hole where one has been
    /// closed.
    ///
    /// Holes rather than removal, because [`DocumentId`] is an index and the
    /// jump list, the pending questions and `current` all hold one. Removing
    /// an element would leave every id above it pointing at a *different*
    /// file, which is the kind of wrong that shows up as the wrong file
    /// opening a week later. A closed slot makes a stale id dead instead:
    /// whoever holds it gets nothing and does nothing.
    documents: Vec<Option<Document>>,
    current: Option<DocumentId>,
    /// Where the reader was looking before a list started showing them
    /// somewhere else, and which document they were looking at it in.
    ///
    /// A list that sits on the status bar leaves the file on screen above
    /// it, so walking its rows scrolls that file to each place. That is a
    /// look and not a move -- the caret has not gone anywhere -- so leaving
    /// the list without choosing has to put the view back *exactly*, and
    /// this is the only thing that knows where exactly was.
    looked_from: Option<(DocumentId, obelus_buffer::Viewport)>,
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
    /// Replaced wholesale, because that is what a server sends -- and
    /// carried across the reader's edits until it does, because each one
    /// is about a piece of text and the text moves under it.
    troubles: HashMap<PathBuf, Vec<obelus_lsp::trouble::Trouble>>,
    /// The line the reader is typing on, until they leave it.
    ///
    /// What every server has said about every file, as it said it.
    ///
    /// The whole project rather than the files Obelus has open, and in the
    /// protocol's own units rather than in any document's: a range is
    /// placed by counting against the text it is in, and most of what a
    /// server talks about after a `cargo check` is text Obelus has not
    /// read. Dropping those was dropping the answer to "where is this
    /// project broken", which is a question about the files nobody has
    /// opened yet.
    ///
    /// Every file, including the open ones, so that the list of the whole
    /// project is one list read one way. What is placed in `troubles`
    /// above is the same news, placed, for the things that have to line up
    /// with characters on screen: the underline, the count, the complaint.
    reported: HashMap<PathBuf, Vec<obelus_lsp::trouble::Reported>>,

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
    /// Where the reader was before quitting took them to the file it is
    /// asking about.
    ///
    /// Put back wherever the question is cancelled, by either of the two
    /// routes out of one -- a key pressed by accident may not leave the
    /// reader somewhere they did not ask to be.
    taken_from: Option<DocumentId>,
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
    /// What this machine's faces are called, as whatever is drawing
    /// Obelus reported them.
    ///
    /// Empty in a terminal, where the font is the terminal's own and there
    /// is nothing to offer. What it is for is the list a setting's names
    /// are built in, which offers what is here and takes what is typed.
    fonts_here: Vec<String>,
    /// And which of them it calls monospaced, which is what a reader who
    /// has chosen none of them is drawn in.
    monospace_here: Option<String>,
    /// A setting's list of names, while the reader is building one.
    ///
    /// Which setting it is goes with it: the list knows names and nothing
    /// about what they are for, so what writes the answer down has to be
    /// told where it goes.
    names: Option<(&'static str, obelus_component::names::Names)>,
    /// Whether what is typed goes over what is under the cursor.
    ///
    /// Not a setting and not a preference: it is a mode, which is to say
    /// something the reader turns on for the next minute and off again, and
    /// what says they are in it is the shape of the caret and a word on the
    /// status row.
    replacing: bool,
    /// What is drawing Obelus, where it has anything to be told.
    ///
    /// `None` in a terminal, which is told nothing: see [`App::drawn_by`].
    drawing: Option<std::sync::Arc<dyn Drawing>>,
    /// Whether `ctrl+enter` arrives as itself rather than as enter.
    ///
    /// A window's keys always do; a terminal's only where it speaks the
    /// kitty keyboard protocol, which `main` asks before the alternate
    /// screen. What it decides is whether the box offers to send now: an
    /// offer of a key that arrives as enter is an offer that queues.
    ctrl_enter_arrives: bool,
    /// This Obelus's place in the machine's pool of build jobs, while the
    /// settings ask for one.
    jobs: Option<obelus_jobs::Pool>,
    /// What Obelus knows about the agents it could run.
    agents: agents::Agents,
    /// Where the agents page's marks were on the frame just drawn.
    ///
    /// Kept so that the next frame can be asked whether they have moved,
    /// which is the whole of whether it has to be written with the caret
    /// put out. `None` when the page is not showing: what comes back to it
    /// is a page that has to be written whatever it holds.
    picture_layout: Option<agents::PictureLayout>,
    /// The settings as they stand, and where each part came from.
    settled: preferences::Settled,
    /// The settings view, while it is open.
    settings: Option<Settings>,
    /// The line counts, while they are showing.
    counts: Option<Counts>,
    /// The whole screen, as of the last frame.
    ///
    /// Kept beside `editor_area` because one view is not in it: the counts
    /// take the screen whole -- no status row and no rule above one -- and
    /// the keys that move about their list have to be told the height that
    /// is actually drawn.
    screen_area: Rect,
    /// What will come back for a tree that is behind its text.
    ///
    /// One for all the open documents, because catching up asks every one
    /// of them: which are behind is a question with an answer, and a clock
    /// each would be a clock per file for a question asked once.
    syntax_pause: Option<crate::event::Pause>,
    /// What will come back for a document the reader has stopped changing.
    changes_pause: Option<crate::event::Pause>,
    /// The commits an open history view is showing, and what is open in it.
    history: history_view::Showing,
    /// Which agents the open list of conversations is showing, and what
    /// each of its rows stands for.
    ///
    /// The same remembering as the two above, for the same reason: a tab is
    /// only there when that agent has said something here, so which tab is
    /// which agent is not a fixed mapping. Empty when the list showing is
    /// not that one.
    conversing: conversations::Conversing,
    /// What Obelus is watching for the views drawn from it, by the places
    /// in `conversations`: the claims, the table of conversations, the
    /// notes, and the other windows on the repository.
    ///
    /// Which of them are wanted is worked out from what is open, every
    /// frame, by `App::settle_the_watches` -- rather than taken at each
    /// door a view opens by and given up at each door it closes by, which
    /// is how one of these came to be watched twice.
    watching: [conversations::Watched; conversations::WATCHED],
    /// Something to tell the reader, until the next key.
    ///
    /// Half of what a language server does is answer with nothing, and
    /// nothing is invisible: without somewhere to say "no definition found" or
    /// "still indexing", pressing the key looks like the key not working.
    ///
    /// Written through [`App::say`] and [`App::wrong`] and nowhere else,
    /// which is what keeps every note saying which kind it is.
    note: Option<saying::Note>,
    /// What went wrong on the way up that no file can be marked with.
    ///
    /// A watcher that would not start, an agent offered no tools. Most of
    /// what Obelus finds on the way up is a mark on a line of a file the
    /// reader wrote; these are what is left over, and they have nowhere to
    /// go but the list put up over the first screen of a start.
    ///
    /// Not every failure on the way up belongs here. Watching a file that
    /// is not there yet fails, and that is the ordinary state of a project
    /// with no settings of its own -- a list that said so would be a list
    /// that says something on every start.
    amiss: Vec<String>,
    /// Whether a newer Obelus is out, and whether this session asked.
    releases: releases::Releases,
    /// The repository's open pull requests, as `gh` last listed them.
    pulls: pulls::Pulls,
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
    /// Which document the view was on last frame, and where in it.
    ///
    /// Only for working out how far it has travelled since: nothing reads
    /// it, and nothing else may.
    viewport_was: Option<(obelus_buffer::DocumentId, (LineNumber, usize))>,
    /// How many screen rows the file's view has travelled altogether.
    ///
    /// A number to be *compared* rather than read -- see
    /// `note_where_the_view_has_got_to`.
    travelled: i64,
    working_directory: PathBuf,
    /// What this window knows about its tree's record of what was open.
    reopening: reopening::Reopening,
    /// Which branch the tree Obelus was put on has checked out.
    ///
    /// Kept rather than asked for, and read at the three moments anything
    /// about the world is read here: when Obelus is told which directory it
    /// is working in, when the watcher says `HEAD` moved, and when Obelus
    /// is the one who changed it -- which is never, git being read-only.
    /// Asking per frame would be `gix::discover` walking up the tree on
    /// every keystroke, which is a file read wearing a costume.
    ///
    /// `None` outside a repository, and the status row then says nothing
    /// and spends no column: a row that said something there would be
    /// saying it about a question that does not arise.
    head: Option<obelus_git::Head>,
    /// Whether the tree Obelus was put on has gone from disk.
    ///
    /// For good: a tree made again at the same path is somebody else's
    /// tree, and every watch Obelus had in this one went with it. Nothing
    /// about the project is asked or written from here on, and what was
    /// open goes once the reader has read so. See [`App::the_tree_has_gone`].
    gone: bool,
    /// What this window knows about the others on the repository, and the
    /// list of worktrees while it is showing.
    worktrees: worktrees::Worktrees,
    /// What this window knows about the chat it can be reached from.
    remote: remote::Remote,
    /// Which of its conversations is which thread in that chat.
    mirror: mirroring::Mirror,
    /// The other windows heard in that chat through this one, or the one
    /// this one is heard through.
    relaying: relaying::Relaying,
    /// Whether to open on the file list.
    ///
    /// A directory on the command line is a reader saying which project
    /// rather than which file, and "which file" is what the list answers.
    /// A flag rather than a list opened on the spot, because the rows come
    /// from a walk on the loop's own channel and there is no channel until
    /// [`App::start`].
    list_at_start: bool,
    /// Whether to connect to the chat once started, as `connect-remote`
    /// would. A flag for the same reason as the one above: connecting
    /// waits on the loop's channel.
    remote_at_start: bool,
    /// Whether nobody is at the screen: `ob --headless` (`headless`).
    headless: bool,
    /// Why a headless Obelus gave up, where it did.
    stopped_because: Option<String>,
    should_quit: bool,
}

/// One of what is open, where it is a file, from the list alone.
///
/// The same answer [`App::file`] gives, reached without borrowing the whole
/// application. A caller that holds the buffer while it asks a language
/// server something needs those two borrows apart, and a method on `App`
/// cannot give it that -- so the handful of places that do reach the list
/// through this rather than writing the four lines out again.
pub(crate) fn file_in(documents: &[Option<Document>], id: DocumentId) -> Option<&Buffer> {
    documents.get(id.get())?.as_ref()?.file()
}

/// The same, to change.
pub(crate) fn file_in_mut(
    documents: &mut [Option<Document>],
    id: DocumentId,
) -> Option<&mut Buffer> {
    documents.get_mut(id.get())?.as_mut()?.file_mut()
}

/// What a front end is told about a theme -- see `App::say_the_colours` --
/// as one value, so that whether it has changed is one comparison of the
/// very things that would be said.
const fn colours_said(theme: &Theme) -> (Color, Color, Color) {
    (
        theme.background,
        theme.selection_background,
        theme.selected_row_background,
    )
}

impl App {
    /// Starts with the shipped key table and the given documents open.
    #[must_use]
    pub fn new(open: Vec<Buffer>) -> Self {
        let current = (!open.is_empty()).then(|| DocumentId::new(0));
        let documents: Vec<Option<Document>> =
            open.into_iter().map(Document::from).map(Some).collect();
        Self {
            notes: project::Notes::default(),
            kept: chat::Kept::default(),
            agent: chat::Agent::default(),
            pointing: pointer::Pointing::default(),
            git: git::Said::default(),
            terminal: terminals::Terminals::default(),
            which_project: project::Asking::default(),
            files: editor::Files::default(),
            search: editor::Search::default(),
            lsp: lsp::State::default(),
            viewport_was: None,
            travelled: 0,
            looked_from: None,
            keymap: Keymap::new(),
            documents,
            current,
            theme: builtin::DARK,
            theme_name: obelus_config::DEFAULT_THEME.to_string(),
            picker: None,
            servers: HashMap::new(),
            stopped: HashSet::new(),
            asked: HashMap::new(),
            troubles: HashMap::new(),
            reported: HashMap::new(),
            jumps: JumpList::default(),
            preview: None,
            phase: 0,
            ticker: None,
            waking: false,
            fonts_here: Vec::new(),
            monospace_here: None,
            names: None,
            replacing: false,
            drawing: None,
            prompt: None,

            ctrl_enter_arrives: true,
            jobs: None,
            settled: preferences::Settled::default(),
            agents: agents::Agents::default(),
            picture_layout: None,
            settings: None,
            counts: None,
            screen_area: Rect::ZERO,
            syntax_pause: None,
            changes_pause: None,
            history: history_view::Showing::default(),
            conversing: conversations::Conversing::default(),
            watching: [const { conversations::Watched::new() }; conversations::WATCHED],
            rendered: None,
            theme_before: None,
            taken_from: None,
            note: None,
            events: None,
            amiss: Vec::new(),
            releases: releases::Releases::default(),
            pulls: pulls::Pulls::default(),
            watcher: None,
            theme_watched: Vec::new(),
            highlights: Highlights::default(),
            editor_area: Rect::ZERO,
            // Read once. Nothing later asks the operating system again, so
            // every path Obelus shows is relative to the same root for the
            // whole session even if something else changes the process's
            // directory.
            // Nobody is being asked until `startup` says so: everything
            // that builds an `App` without going through it -- every test
            // -- has a directory already and is not a reader standing in
            // front of a launcher.
            working_directory: std::env::current_dir().unwrap_or_default(),
            reopening: reopening::Reopening::default(),
            // Not read here. Which branch the tree is on is a fact about
            // the directory Obelus was *told* to work in, so it is read
            // where it is told -- `App::work_in`, which both the startup
            // and `working_directory_for_test` go through. The same
            // argument `apply_project` makes from there: a test that moved
            // one without the other would be testing an application no
            // reader can have.
            head: None,
            gone: false,
            worktrees: worktrees::Worktrees::default(),
            remote: remote::Remote::default(),
            mirror: mirroring::Mirror::default(),
            relaying: relaying::Relaying::default(),
            list_at_start: false,
            remote_at_start: false,
            headless: false,
            stopped_because: None,
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
        // Nothing is asked once the project has gone: what is unwritten
        // has nowhere to be written, and neither have the notes.
        if self.gone {
            self.should_quit = true;
            return;
        }
        // The notes first, and without asking. A buffer that is unwritten
        // is a decision the reader has to make -- their change, or the file
        // on disk -- and there is no such decision here: a note lives
        // nowhere else, and typing that has not had its pause yet is
        // typing they did and would expect to find.
        self.write_the_notes();
        // Not while something is unwritten: the reader is asked, because
        // "press it again" is an answer that has to be guessed at, and the
        // two things they might have meant -- write them, or let them go --
        // are not the same key twice.
        let unsaved = self
            .documents
            .iter()
            .flatten()
            .filter_map(Document::file)
            .filter(|buffer| buffer.is_dirty())
            .count();
        if unsaved > 0 {
            // Nothing over the question. It is a list itself, so a list
            // already showing is replaced by it -- but the settings, the
            // counts and a list being built each have a layer of their
            // own, and the question would open over those: two things on
            // screen and two escapes to leave, which is the thing a
            // context is for preventing. Left the way escape leaves them,
            // so each puts back whatever it changed.
            for layer in self.layers().nearest_first() {
                self.leave(layer);
            }
            // And the file it is about under it, where there is one of
            // them. The prompt can name a path, but a reader deciding
            // whether to write something should be looking at it -- and
            // the name is what they are least likely to need, because they
            // know what they were typing.
            //
            // Only for one. A count is all a question can say about
            // several, and there is nowhere to go that is all of them.
            if unsaved == 1
                && let Some(id) = self.first_unsaved()
                && self.current != Some(id)
            {
                self.taken_from = self.current;
                self.go_to_document(id);
            }
            self.ask_before_leaving(unsaved);
            return;
        }
        // And a program still running in a terminal, which leaving stops:
        // asked for the reason closing that terminal is. Over nothing, for
        // the reason the question above is.
        let running = self.terminals_running();
        if running > 0 {
            for layer in self.layers().nearest_first() {
                self.leave(layer);
            }
            self.ask_before_stopping_them(
                running,
                "leave",
                obelus_component::question::Answer::Leaving(
                    obelus_component::question::Leaving::Discard,
                ),
            );
            return;
        }
        self.should_quit = true;
    }

    /// The list of names being built, while one is open.
    #[must_use]
    pub fn names(&self) -> Option<&obelus_component::names::Names> {
        self.names.as_ref().map(|(_, names)| names)
    }

    /// Whether what is typed goes over what is under the cursor.
    ///
    /// Not while the reader's keys change no file: the key that turns the
    /// mode off is one of those refused, so a mode that was on would sit on
    /// the status row and in the caret's shape with nothing typed to obey it.
    #[must_use]
    pub const fn replacing(&self) -> bool {
        self.replacing && !self.settled.config.read_only
    }

    /// Turns typing over on, or off again.
    pub fn toggle_replacing(&mut self) {
        self.replacing = !self.replacing;
    }

    /// What shape the caret is, which is what it says about the next thing
    /// typed.
    ///
    /// A bar sits between two characters and says the next one goes there;
    /// a block sits on one and says the next one takes its place. So the
    /// shape follows the mode -- and only in a document, because a box the
    /// reader is typing into does not have the mode: a query or a message
    /// is one short line, and typing over it means nothing.
    #[must_use]
    pub fn caret(&self) -> Caret {
        match self.replacing() && self.layers().nearest().is_none() {
            true => Caret::Block,
            false => Caret::Bar,
        }
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
    ///
    /// And tells whatever is drawing, because a window paints outside the
    /// cells in the theme's colours too. Here rather than where the settings
    /// are applied, because this is the one door every theme goes through
    /// and the settings are only one of them: a theme being previewed, the
    /// one put back when the reader escapes, and a theme file somebody
    /// edited all came in by it without a word, and the window went on
    /// painting its margin and its title bar in the old one.
    ///
    /// Only when they have moved. A theme being previewed is worn again on
    /// every frame the list is open, and a front end told something wakes
    /// to draw it -- so telling it every time is a frame that asks for the
    /// next one, for as long as the reader looks at the list.
    pub fn set_theme(&mut self, name: &str, theme: Theme) {
        let moved = colours_said(&theme) != colours_said(&self.theme);
        self.theme_name = name.to_string();
        self.theme = theme;
        if moved {
            self.say_the_colours();
        }
    }

    /// The two things about the theme a front end paints for itself: the
    /// ground outside the cells, and the colours a hold is drawn in.
    pub(crate) fn say_the_colours(&self) {
        if let Some(drawing) = self.drawing.as_ref() {
            let (ground, held, row) = colours_said(&self.theme);
            drawing.drawn_on(ground);
            drawing.holding(held, row);
        }
    }

    /// The highlight kinds for what is on screen.
    #[must_use]
    pub const fn highlights(&self) -> &Highlights {
        &self.highlights
    }

    /// Where Obelus was started, and the root every path is shown relative to.
    #[must_use]
    pub fn working_directory(&self) -> &Path {
        &self.working_directory
    }

    /// Says to open on the file list rather than on a file.
    pub fn list_at_start(&mut self) {
        self.list_at_start = true;
    }

    /// Says to connect to the chat once started.
    pub fn remote_at_start(&mut self) {
        self.remote_at_start = true;
    }

    /// One of what is open, by its id.
    ///
    /// Four doors rather than forty-odd repetitions of
    /// `get(id.get()).and_then(Option::as_ref)`: a stale id and a closed
    /// slot are the same answer here, and the answer is `None`.
    #[must_use]
    pub fn document(&self, id: DocumentId) -> Option<&Document> {
        self.documents.get(id.get())?.as_ref()
    }

    /// The same, to change.
    pub fn document_mut(&mut self, id: DocumentId) -> Option<&mut Document> {
        self.documents.get_mut(id.get())?.as_mut()
    }

    /// Does something to the transcript of the conversation being read.
    ///
    /// A closure because the alternative is a guard at two dozen call sites
    /// that all say the same thing: a note about a conversation nobody is
    /// in goes nowhere, and every one of them was written when there was
    /// one conversation and it was always there.
    pub(crate) fn in_transcript(&mut self, what: impl FnOnce(&mut obelus_component::chat::Chat)) {
        if let Some(talk) = self.conversation_mut() {
            what(&mut talk.chat);
        }
    }

    /// Whether there is anything being read at all.
    ///
    /// Which is not "no file": a conversation is something to read, and it
    /// is what the reader is on while they are in one. The welcome screen
    /// asks this, and asked about a file until a conversation could be the
    /// answer -- so it drew itself over one.
    #[must_use]
    pub fn reading_nothing(&self) -> bool {
        self.current.is_none_or(|id| self.document(id).is_none())
    }

    /// The conversation being read, where that is what is being read.
    ///
    /// There is no flag for this any more. A conversation is showing when it
    /// is the document the reader is on, which is one fact in one place --
    /// and a `showing_chat` beside a conversation that exists was two.
    #[must_use]
    pub(crate) fn conversation(&self) -> Option<&crate::conversation::Conversation> {
        self.document(self.current?)?.chat()
    }

    /// The same, to change.
    pub(crate) fn conversation_mut(&mut self) -> Option<&mut crate::conversation::Conversation> {
        self.document_mut(self.current?)?.chat_mut()
    }

    /// One of what is open, where it is a file.
    ///
    /// Which almost everything wants: the motions, the syntax, the language
    /// server and the margin are all about a file, and a document that is
    /// not one answers `None` rather than being handed to them.
    #[must_use]
    pub fn file(&self, id: DocumentId) -> Option<&Buffer> {
        self.document(id)?.file()
    }

    /// The same, to change.
    pub fn file_mut(&mut self, id: DocumentId) -> Option<&mut Buffer> {
        self.document_mut(id)?.file_mut()
    }

    /// The document being read, if any is open.
    #[must_use]
    pub fn current_buffer(&self) -> Option<&Buffer> {
        self.file(self.current?)
    }

    /// The same, to change.
    pub fn current_buffer_mut(&mut self) -> Option<&mut Buffer> {
        self.file_mut(self.current?)
    }

    /// Gives the application the loop's channel and nothing else.
    ///
    /// Separate from [`App::start`] so that a test can have the parts that
    /// need a channel -- an agent, a file walk -- without a watcher, a
    /// ticker and a language server per open file.
    pub fn events_for_test(&mut self, sender: std::sync::mpsc::Sender<Event>) {
        self.events = Some(sender);
        // What was open is read on a thread as soon as there is a channel to
        // answer on, as `start` does it.
        self.send_the_reopening();
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
        for index in 0..self.documents.len() {
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
        self.git.history_generation.now()
    }

    /// Which document is being read, for a test that wants to know whether
    /// a key took the reader somewhere new.
    #[must_use]
    pub const fn current_document_for_test(&self) -> Option<DocumentId> {
        self.current
    }

    /// Goes back to a document the test was reading, by what
    /// [`App::current_document_for_test`] said it was.
    pub fn go_to_document_for_test(&mut self, id: DocumentId) {
        self.go_to_document(id);
    }

    /// How many *files* are open, for a test that wants to know whether a
    /// key that had nowhere to go left one behind anyway.
    ///
    /// Files rather than documents: what these tests ask about is a file
    /// that was opened or not opened, and a conversation among them would be
    /// a number that moves for a reason they are not about.
    #[must_use]
    pub fn file_count_for_test(&self) -> usize {
        self.documents
            .iter()
            .flatten()
            .filter_map(Document::file)
            .count()
    }

    /// The address an agent is told, where Obelus is offering one.
    ///
    /// `None` until there is a project: the tools are about one, so the
    /// server is not started before the reader has said which.
    #[must_use]
    pub fn tools_url(&self) -> Option<&str> {
        self.agent.tools_url.as_deref()
    }

    /// Says where Obelus's own tools are, without listening anywhere.
    ///
    /// The loop starts a server and puts its address here; a test wants the
    /// address handed to an agent without a port being opened for it, which
    /// is what an agent is told rather than what it finds at the other end.
    pub fn tools_url_for_test(&mut self, url: &str) {
        self.agent.tools_url = Some(url.to_string());
    }

    /// Puts the application on a project of the test's choosing.
    ///
    /// The working directory is read from the process once, at startup, and
    /// a test inherits whatever directory the test runner was started in --
    /// which is the checkout's own path, and differs between a clone, a
    /// worktree and somebody else's machine. Anything that *shows* that path
    /// is then a golden grid that passes where it was written and nowhere
    /// else, so a test that renders one says which project it is on.
    pub fn working_directory_for_test(&mut self, root: PathBuf) {
        self.work_in(root);
        // And whatever that project has to say about the settings, which
        // is what putting Obelus on a project means: at startup the two happen
        // together, and a test that moved one without the other would be
        // testing an application no reader can have.
        self.apply_project();
    }

    /// The open picker, for the renderer.
    #[must_use]
    pub const fn picker(&self) -> Option<&Picker> {
        self.picker.as_ref()
    }

    /// What went wrong on the way up, in one list.
    ///
    /// Both halves: what Obelus could not make of a file it reads for its
    /// own sake, and what has no file to be about. The first is already a
    /// mark on that file -- but a mark on a file nobody has opened is a
    /// mark nobody sees, and the first screen of a start is the one place a
    /// reader will be standing when it matters.
    ///
    /// Obelus's own only. What a server says about the code is the code's
    /// business and is not something that went wrong starting up.
    ///
    /// As the rows of a list, because that is what they are drawn as, and
    /// rows that go nowhere: a row is read here, not gone to. Where it is
    /// is said at the end of it all the same, which is how a reader finds
    /// the line to fix -- and the marks on that file, and `show-problems`,
    /// are where it is gone to.
    #[must_use]
    pub fn what_went_wrong(&self) -> Vec<PickerItem> {
        let mut rows = Vec::new();
        let mut paths: Vec<&PathBuf> = self.troubles.keys().collect();
        paths.sort();
        for path in paths {
            for trouble in self.troubles.get(path).into_iter().flatten() {
                if trouble.source.as_deref() != Some(semantics::OBELUS) {
                    continue;
                }
                // The file by its name and the line counted the way a
                // reader counts: the directory is the reader's own settings
                // or the project's, and either way the name says which.
                let name = path.file_name().map_or_else(
                    || path.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                );
                rows.push(semantics::problem_row(
                    // The first line: what Obelus says about a file that
                    // will not read carries the parser's own words under
                    // its sentence, and a row is one line.
                    trouble.message.lines().next().unwrap_or_default(),
                    trouble.severity,
                    Some(format!("{name}:{}", trouble.span.line.get() + 1)),
                    PickerValue::Nothing,
                ));
            }
        }
        // A warning, each of them: something did not start or would not
        // read, and Obelus went on without it -- which is what a warning
        // is, where an error is what stops the thing it is about.
        rows.extend(self.amiss.iter().map(|said| {
            semantics::problem_row(
                said,
                obelus_lsp::trouble::Severity::Warning,
                None,
                PickerValue::Nothing,
            )
        }));
        rows
    }

    /// Puts what went wrong on the way up in front of the reader, where
    /// anything did, and answers whether it did.
    ///
    /// A list over the first screen -- the welcome screen, or the page
    /// asking which project -- and not a block drawn on it: a block on a
    /// screen was a second thing there with keys of its own, and a list is
    /// the one thing on screen until it is let go, which is what the
    /// reader is owed before the way in. Once a start, from `start`: what
    /// went wrong on the way up is news once, and the marks on the file
    /// and `show-problems` are where it is kept after that.
    ///
    /// Not where the first screen is a file, which was asked for by name
    /// and is what the reader came to read.
    pub(super) fn tell_what_went_wrong(&mut self) -> bool {
        // Into the log where nobody is at the screen, and nothing put up to
        // wait for a key that will not come.
        if self.headless {
            for row in self.what_went_wrong() {
                match row.detail {
                    Some(at) => tracing::warn!("{at}: {}", row.label),
                    None => tracing::warn!("{}", row.label),
                }
            }
            return false;
        }
        if !self.reading_nothing() {
            return false;
        }
        let rows = self.what_went_wrong();
        if rows.is_empty() {
            return false;
        }
        let mut picker = Picker::new(rows, PickerLayout::Compact { rows: COMPACT_ROWS });
        // Read, not chosen from: its rows go nowhere, so no row is the
        // reader's to be on and nothing is typed to narrow them.
        picker.only_read();
        picker.will_not_give_way();
        picker.ask("What went wrong starting up");
        self.show_list(picker);
        true
    }

    /// What went wrong on the way up with no file to mark.
    ///
    /// The front end's own belong here too: a terminal that would not turn
    /// on bracketed paste is a terminal Obelus cannot be pasted into, and
    /// `ob` is the only thing that knows.
    pub fn amiss(&mut self, said: &str) {
        self.amiss.push(said.to_string());
    }

    /// And everything that has gone in it.
    #[must_use]
    pub fn went_wrong(&self) -> &[String] {
        &self.amiss
    }

    /// What the server says is wrong with the file being read.
    ///
    /// In the order they are in the file, which is put right as they
    /// arrive: a server reports what it found in the order it found it,
    /// and that is not top to bottom.
    #[must_use]
    pub fn troubles(&self) -> &[obelus_lsp::trouble::Trouble] {
        self.current_buffer()
            .and_then(|buffer| self.troubles.get(buffer.path()))
            .map_or(&[], Vec::as_slice)
    }

    /// The ones that are a problem in their own right, which is everything
    /// the server said except the notes it hung on the others.
    ///
    /// A compiler answers with one diagnostic and several sub-diagnostics:
    /// rustc's `this function takes 2 arguments but 1 was supplied` comes
    /// with `function defined here`, its `cannot find function step_99`
    /// with `a function with a similar name exists`. rust-analyzer sends
    /// each of those as a diagnostic of its own, at the place it points
    /// at and one severity down -- so `function defined here` arrives as a
    /// hint eighty lines away from the error it belongs to.
    ///
    /// Read alone they say nothing: "function defined here" is not a
    /// question anybody asked, and the error it is an answer to is in the
    /// list anyway. So they are left out of the two places a reader works
    /// through what is wrong -- the list and the keys that walk it -- and
    /// left in everywhere the reader is asking about a particular place:
    /// the underline under the word, the complaint under the caret's line,
    /// the count on the status row. A mark the reader can see and cannot
    /// ask about would be worse than a row they can skip.
    ///
    /// The cost, and it is a real one: a server that uses hints for
    /// something that does stand alone loses it from the list.
    pub fn problems(&self) -> impl Iterator<Item = &obelus_lsp::trouble::Trouble> {
        self.troubles()
            .iter()
            .filter(|trouble| trouble.severity != obelus_lsp::trouble::Severity::Hint)
    }

    /// What the call the cursor is inside takes, while it is showing.
    ///
    /// Not while the completion panel is up: the two would be drawn in the
    /// same place, and what could be typed next is the nearer question.
    #[must_use]
    pub const fn signature(&self) -> Option<&obelus_component::signature::Signature> {
        match self.lsp.completion.is_some() {
            true => None,
            false => self.lsp.signature.as_ref(),
        }
    }

    /// What the server says the place under the caret is, while it is up.
    ///
    /// Not while either of the other two panels is: all three want the
    /// cells beside the cursor, and of the three this is the question
    /// asked longest ago.
    #[must_use]
    pub const fn hover(&self) -> Option<&Hover> {
        match self.lsp.completion.is_some() || self.lsp.signature.is_some() {
            true => None,
            false => self.lsp.hover.as_ref(),
        }
    }

    /// What could be typed next, while a server's answer is on screen.
    #[must_use]
    pub const fn completion(&self) -> Option<&Completion> {
        self.lsp.completion.as_ref()
    }

    /// How far along the welcome screen's colours have travelled, in ticks.
    ///
    /// Zero until something ticks, so a screen drawn without a running
    /// ticker -- a test, a remote session -- is the same screen every time.
    #[must_use]
    pub const fn phase(&self) -> u32 {
        self.phase
    }

    /// Puts the animation back where it starts, for a test.
    ///
    /// A spinner is a cell that changes on a clock nobody in a test is
    /// driving on purpose: the ticker runs on a thread, so how many of its
    /// ticks have arrived by the time a screen is drawn depends on how
    /// quickly the machine got there. A golden screen holding one is a
    /// golden screen that passes on the machine it was made on -- which is
    /// what `⠋` against `⠼` means, and it says nothing about Obelus.
    pub const fn phase_for_test(&mut self, phase: u32) {
        self.phase = phase;
    }

    /// What Obelus has to say, until the next key.
    #[must_use]
    pub fn note(&self) -> Option<&str> {
        self.note.as_ref().map(saying::Note::words)
    }

    /// And whether it is about something that would not go.
    #[must_use]
    pub fn note_is_wrong(&self) -> bool {
        self.note.as_ref().is_some_and(saying::Note::is_wrong)
    }

    /// Says what happened, or what is happening.
    pub(crate) fn say(&mut self, said: impl Into<String>) {
        let said = said.into();
        // Where nobody is at the screen the log is the status row.
        if self.headless {
            tracing::info!("{said}");
        }
        self.note = Some(saying::Note::said(said));
    }

    /// Says something on the status row, for a test about how a note is
    /// drawn rather than about what put it there.
    pub fn say_for_test(&mut self, said: &str) {
        self.say(said);
    }

    /// Says what would not go.
    ///
    /// Two doors rather than a flag set beside the words: a flag is a
    /// second thing to remember at a hundred and thirty call sites, and
    /// the one that was forgotten would be a refusal drawn as a report.
    /// Which of these a note went through is the whole of how it says
    /// which it is.
    pub(crate) fn wrong(&mut self, said: impl Into<String>) {
        let said = said.into();
        if self.headless {
            tracing::warn!("{said}");
        }
        self.note = Some(saying::Note::wrong(said));
    }

    /// Takes back whatever was said.
    pub(crate) fn quiet(&mut self) {
        self.note = None;
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

    /// Adds up how far the file's view has travelled, in screen rows.
    ///
    /// Asked here because here is once a frame, which is what makes the
    /// total honest: a frame drawn again without the view moving adds
    /// nothing, so whoever is watching the number can take a change in it
    /// for a scroll. Twelve places move the viewport and none of them has
    /// to know about this.
    ///
    /// Screen rows and not lines, because a fold between two tops is no
    /// rows at all and a wrapped line is several -- see
    /// `Buffer::screen_rows_between`, which is also what bounds the walk.
    fn note_where_the_view_has_got_to(&mut self) {
        let area = self.text_area();
        let Some(current) = self.current else {
            self.viewport_was = None;
            return;
        };
        let Some(buffer) = self.current_buffer() else {
            self.viewport_was = None;
            return;
        };
        let now = (buffer.viewport().top, buffer.viewport().top_row);
        let moved = match self.viewport_was {
            // Another document is not this one having scrolled.
            Some((was, at)) if was == current && at != now => {
                buffer.screen_rows_between(at, now, area, usize::from(area.height))
            }
            _ => None,
        };
        self.viewport_was = Some((current, now));
        if let Some(rows) = moved {
            self.travelled = self.travelled.saturating_add(rows as i64);
        }
    }
}

/// The key that walks to the next tab, or to the one before it.
fn stepping(onwards: bool) -> crossterm::event::KeyEvent {
    match onwards {
        true => crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Tab,
            crossterm::event::KeyModifiers::NONE,
        ),
        false => crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::BackTab,
            crossterm::event::KeyModifiers::SHIFT,
        ),
    }
}

/// A bare `enter`, for a press that means what that key means.
///
/// A press that opens a row goes down the key's own path rather than having
/// an opener of its own: the mark under the pointer is a picture of the
/// key, and two answers about one row are two answers to keep alike.
fn enter() -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    )
}

/// The pointer, standing still.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Resting {
    /// Which cell of the screen.
    pub(crate) x: u16,
    pub(crate) y: u16,
    /// When it arrived there.
    pub(crate) since: std::time::Instant,
    /// Whether this rest has asked what is under it.
    pub(crate) asked: bool,
}

/// A document that has been changed, waiting to be asked about.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Settling {
    /// Which document.
    pub(crate) buffer: DocumentId,
}

/// A path as it should be read: relative to the root when it lies under it.
pub(crate) fn relative(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// What is wrong with the line the reader is on.
///
/// The worst of what a server says about that one line, and how many
/// others it said -- which is what the box floated under it says. Kept
/// rather than drawn straight from the troubles because two things settle
/// it and neither is the drawing: which line the reader is looking at, and
/// which of that line's troubles a list of them has walked to.
#[derive(Debug)]
struct Complaint {
    /// The line it is about.
    line: LineNumber,
    /// And the character of it the trouble starts at.
    column: CharColumn,
    /// The worst one's own words, in the server's spelling.
    said: String,
    /// How bad it is, which is the colour they are drawn in.
    severity: obelus_lsp::trouble::Severity,
    /// How many others are on that line.
    others: usize,
}

/// What shape the caret is drawn in.
///
/// Two, because there are two things the next character can do: go between
/// what is there, or go over it. A terminal has four of these and the
/// reader has chosen one of them for their terminal; a window draws its own
/// and so has to be told which.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Caret {
    /// Between two characters: what is typed goes in.
    Bar,
    /// On one: what is typed takes its place.
    Block,
}

/// What the thing drawing Obelus can be told.
///
/// One method, and the shape is deliberate: this is not a way for the
/// application to drive a front end, it is the list of settings that mean
/// something only to whatever is drawing. A terminal implements none of it
/// because there is none of it a terminal owns -- its font and its size
/// belong to the terminal and to the reader who configured it.
///
/// `Send + Sync`, because in a window the application runs on a thread of
/// its own and what it is talking to is the window's loop.
pub trait Drawing: std::fmt::Debug + Send + Sync {
    /// The text should be this many points from now on.
    ///
    /// Said once when the front end says who it is, and again after every
    /// change to the settings -- including one made in another Obelus,
    /// which the watcher notices. Called from the application's own thread,
    /// so what it must not do is wait: putting it down a channel is what
    /// the window does.
    fn text_size(&self, points: usize);

    /// The caret is this shape, and belongs to this.
    ///
    /// Said on every frame rather than when it changes, because what it
    /// depends on -- the mode, and whether the caret is in a box -- is
    /// worked out while the frame is laid out and not kept anywhere else.
    /// The front end compares it with what it is already drawing.
    ///
    /// Whose it is, because a caret that changes hands has not *moved*.
    /// A list opening over the page puts the caret in its own box, which
    /// is a different caret at a different place, and a front end that
    /// took the two for one would walk it down the screen -- from where
    /// the reader was reading to where the list came up. `None` is the
    /// document's own.
    ///
    /// And whether a character typed now goes into any text, which is not
    /// whether there is a caret -- see [`App::takes_text`]. A window turns
    /// its input method off where it does not, so that spelling a word
    /// does not swallow keys that are not going to be a word. With the
    /// caret rather than on its own because it is the same kind of fact,
    /// about where the keys are going, and known at the same moment.
    fn caret_is(&self, caret: Caret, whose: Option<Layer>, typing: bool);

    /// Things arrive where they are going, or are simply there.
    ///
    /// A setting, said the same way and at the same moments as the size:
    /// it means nothing to a terminal, whose unit is a whole cell and
    /// which cannot draw a step of anything, so it is the front end's to
    /// act on and the application does not read it back.
    fn animates(&self, on: bool);

    /// The text is drawn in these faces, tried in this order.
    ///
    /// A setting, like the size, and said the same way and at the same
    /// moments. A name this machine does not have is the front end's to
    /// step over: a settings file is read on more than one machine.
    fn use_fonts(&self, names: &[String]);

    /// What the page is drawn on, for the margin round the grid.
    ///
    /// A window's size is the compositor's and a cell's is the font's, so
    /// the one is not a whole number of the other and a strip is left over
    /// on each side. The cells are all one size and none of them is
    /// stretched to cover it, so something has to say what colour it is --
    /// and that is the theme's, which the cells never have to say because
    /// a terminal has no such strip: there the grid *is* the cells.
    ///
    /// A setting, so it is said the way the size is and at the same two
    /// moments: a theme changes under a reader when another Obelus writes
    /// the settings.
    fn drawn_on(&self, ground: Color);

    /// Which two colours mean the reader has hold of something: a run of
    /// characters, and the row their keys are on.
    ///
    /// A terminal's whole answer to a hold is the colour itself, and there
    /// is nothing else a cell can be. A window can draw the run as a shape
    /// -- see `paint::holdings` -- and to find the run at all it has to
    /// know which colour it is looking for.
    ///
    /// Not a shape, although it ends in one. A shape is a claim about a
    /// region of the frame being drawn, and this is a fact about the
    /// *theme*: it changes when the settings do and not when the screen
    /// does, which is why it is said the way the page's ground is and at
    /// the same two moments. Where the hold is, the cells already say --
    /// which is why this is two colours and no rectangle, and why no view
    /// has to remember to say anything. A list added next month is drawn
    /// like every other list because it paints the row the same colour,
    /// which it must do anyway for the terminal.
    fn holding(&self, held: Color, row: Color);
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
    // Obelus did not put one. Ordinarily nothing repaints mid-write and
    // nobody sees either of them; handing a terminal a sixel makes it draw
    // then and there, which had a caret flashing across the agents page on
    // every step of the selection.
    //
    // Putting the caret out on *every* frame fixed that and cost more than
    // it was worth: a hide and a show per frame is a caret that visibly
    // blinks, and frames arrive as fast as a language server reports
    // progress. So the careful order is used on the frames that need it,
    // which are the ones that actually hand the terminal a picture -- see
    // `App::writes_a_picture`.
    //
    // Measured from the terminal rather than from the last frame's area,
    // because the first frame has no last one and a resize is exactly when
    // the marks move.
    let size = terminal.size()?;
    let screen = Rect::new(0, 0, size.width, size.height);
    let pictures = app.writes_a_picture(obelus_ui::editor_room(screen, &*app));
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

/// What the command line asked Obelus to open.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Opening {
    /// The project to work in, absolute, where the arguments named one.
    ///
    /// `None` for no arguments at all, which leaves the directory Obelus
    /// was started in -- the shell's answer to the same question.
    pub root: Option<PathBuf>,
    /// The files to open, in the order they were given.
    pub files: Vec<PathBuf>,
    /// Whether to open on the file list.
    pub list: bool,
}

/// What a set of command-line paths means.
///
/// A file names the tree it is in and is opened: the repository, which is
/// what a reader means by the project, and the directory the file sits in
/// only where git has never heard of it. A directory *is* the project --
/// naming one is a reader saying where to work, so it is taken at its word
/// and not widened to the repository over it -- and the question it leaves,
/// which file, is the one the list answers. The first path decides the
/// project, because a reader who names two has said which they meant
/// first.
///
/// Absolute, and by the same rule [`obelus_buffer::Buffer::open`] uses on
/// a file: made absolute rather than canonical, so a project reached through
/// a symlink is still shown under the name the reader typed. A relative
/// root would fail quietly -- every path Obelus shows is worked out by
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
        // The tree the file is in, which is what a reader naming a file
        // means by the project: `ob src/main.rs` from anywhere opens the
        // repository that file belongs to. The directory it sits in is
        // the answer only where git has never heard of it -- a file in
        // `/tmp`, a scratch note, something downloaded. Absolute first,
        // because the parent of a bare `main.rs` is nothing at all.
        false => obelus_git::worktree(&absolute(first)).unwrap_or_else(|| {
            absolute(first)
                .parent()
                .map_or_else(|| absolute(first), Path::to_path_buf)
        }),
    };
    Opening {
        root: Some(absolute(&root)),
        // Nothing to open means the list is the whole answer: `ob src`
        // is a reader saying which project and asking which file.
        list: files.is_empty(),
        files,
    }
}

/// A path from the command line, made absolute against where Obelus was
/// started.
fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Runs until the application asks to quit or input ends.
///
/// It blocks on the channel, and input is read by a thread that sends into
/// the same channel. Which way round that goes is the whole design, and it
/// is not obvious from here, so: a loop has to block on
/// exactly one thing or spin, and Obelus has two sides to wait on -- the
/// terminal, and everything else. Everything else is fourteen of the
/// nineteen [`Event`](crate::event::Event) variants, and every one of them
/// arrives with the reader's hands still: a walk finding files, a language
/// server answering, an agent saying the next word, the watcher, the clock.
/// The terminal is one source.
///
/// So the loop parks on the side with the fourteen. Parking on the terminal
/// instead and draining the channel with `try_recv` reads like the same
/// thing and is not: `try_recv` returns "nothing yet" at once and the loop
/// goes back to waiting for a *key*, so the list stays empty until you
/// type, the agent's answer appears a keystroke late, and a file that
/// changed on disk is not re-read until you press something. Parking on
/// neither means spinning, or drawing on a clock at sixty frames a second
/// -- and Obelus draws when something happened, which is what lets it be a
/// process that is genuinely asleep when nothing is.
///
/// `terminal.draw` is synchronous and blocks the loop on a write to stdout.
/// Into a local tty that is tens of microseconds; over ssh or inside tmux,
/// stdout is a pipe and a slow reader really does stall the write. Accepted
/// for now — but nothing slow may go inside the draw closure, which is the
/// mistake that actually happens.
///
/// The receiving end is passed in, because the other end belongs to
/// whatever is drawing Obelus: the terminal reads keys on a thread of its
/// own, and a window gets them from the event loop it is obliged to run on
/// its process's first thread. Both hold a `Sender`, and the loop here
/// cannot tell which it is waiting on -- which is the whole point. So the
/// front end makes the channel, starts its own input, and hands the
/// application the sender with [`App::start`] before this is called.
pub fn run<B>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    events: std::sync::mpsc::Receiver<Event>,
) -> Result<()>
where
    B: Backend,
    // 0.30 made the backend's error an associated type; `anyhow` needs it to
    // be a real, sendable error before `?` will take it.
    B::Error: std::error::Error + Send + Sync + 'static,
{
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

        // And the other half of the same measurement. The loop is blocked by
        // whatever `handle` does for exactly as long as it does it, and what
        // it does is most of Obelus -- a query refiltered, a preview read and
        // parsed, a batch of a walk folded in. Timing only the drawing meant
        // a keystroke that took a tenth of a second left nothing in the log
        // at all, and "the frames are fine" was being read as "nothing is
        // slow".
        //
        // The event is named because the answer is useless without it: what
        // there is to find out is *which* keystroke, or which arrival, and a
        // duration on its own says only that something was slow.
        let Ok(event) = events.recv() else {
            // Every sender is gone, so no further event can arrive. Which
            // is not how Obelus is meant to end -- the reader asks -- so it
            // says so: whatever was reading input has died.
            tracing::warn!("nothing is left to send events, so there is nothing to wait for");
            break;
        };
        handle(app, event);

        // Fold in whatever else is already queued.
        for _ in 0..EVENT_DRAIN_LIMIT {
            let Ok(ready) = events.try_recv() else {
                break;
            };
            handle(app, ready);
        }
    }
    // And on the way out, every session opened here that nothing was said
    // in. Sent and not waited for: the process ends a moment after, and a
    // reader who asked to leave is not kept waiting on an agent. What
    // arrives in that moment is let go; what does not stays with the
    // agent, which is where it would have been anyway.
    app.let_go_of_what_nothing_was_said_in(None);
    // And what was open, with the carets where they are now: the frames
    // wrote it down as it changed, and the way out is the last chance to
    // say where in each file the reader was.
    app.write_down_what_is_open_on_leaving();

    Ok(())
}

/// Hands the application one event, and says so where it was slow.
///
/// The threshold is the frame's, because what is being measured is the same
/// thing: how long the loop was unable to answer the reader. Which half it
/// was is the whole of what the line is for.
fn handle(app: &mut App, event: Event) {
    let what = event.what();
    // Which key, where it is one. The whole reason for this line is finding
    // out that typing is slow, and one saying `Key` about every letter of a
    // query would answer with the question again. `KeyCode` is `Copy`, so
    // taking it costs nothing on the events that are not slow.
    let key = match &event {
        Event::Key(pressed) => Some(pressed.code),
        _ => None,
    };
    let started = std::time::Instant::now();
    app.handle(event);
    let took = started.elapsed();
    if took >= SLOW_FRAME {
        tracing::debug!(?took, event = what, ?key, "a slow event");
    }
}
