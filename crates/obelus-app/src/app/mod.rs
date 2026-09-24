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
//! `obelus_git` is the reading of a repository and `history` here is what
//! obelus does with what it reads. What is left in this file is the state
//! itself, the keys, the frame, and the loop.
pub mod agents;
mod asking;
mod changing;
mod choosing;
mod completing;
mod counting;
pub mod dispatch;
pub mod document;
mod documents;
mod fixing;
mod hierarchy;
mod history;
mod history_view;
mod hovering;
mod noting;
mod opening;
pub use history_view::About;
mod keys;
mod moving;
mod naming;
mod preferences;
mod previewing;
mod renaming;
mod renaming_files;
mod searching;
mod semantics;
pub mod talking;

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use document::Document;
use documents::Rendered;
use history::Changed;
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
        Remark, files,
    },
    prompt::{Prompt, PromptKind, PromptOutcome},
    settings::{Settings, SettingsOutcome},
    todo::TodoView,
};
use obelus_editing::{
    keymap,
    keymap::{Context, KeyChord, Keymap},
    motion_for,
};
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
use ratatui::{
    Terminal,
    backend::Backend,
    buffer::Buffer as CellBuffer,
    layout::{Position, Rect},
};
use semantics::{Asked, Question, named as server_named};

use crate::{
    event,
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
    pub(crate) text: Option<String>,
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

/// Everything obelus is currently showing or remembering.
#[derive(Debug)]
pub struct App {
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
    /// Which file walk the picker is currently expecting batches from.
    ///
    /// Bumped every time a file picker opens, so batches from a walk whose
    /// picker has already closed are recognizable and dropped.
    walk_generation: obelus_runtime::cancel::Latest,
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
        obelus_text::coordinates::LineNumber,
    )>,
    /// Which walk of the history the list is expecting batches from.
    ///
    /// Bumped every time a history starts being read -- a key, a tab, a
    /// different file -- so the batches of the walk before it are
    /// recognizable as stale. Shared with the walking thread, which reads it
    /// to find out that nobody is waiting for it any more: a whole history
    /// is a walk of the whole project, and there is nothing else to stop it
    /// with.
    history_generation: obelus_runtime::cancel::Latest,
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
    troubles: HashMap<PathBuf, Vec<obelus_lsp::trouble::Trouble>>,
    /// What every server has said about every file, as it said it.
    ///
    /// The whole project rather than the files obelus has open, and in the
    /// protocol's own units rather than in any document's: a range is
    /// placed by counting against the text it is in, and most of what a
    /// server talks about after a `cargo check` is text obelus has not
    /// read. Dropping those was dropping the answer to "where is this
    /// project broken", which is a question about the files nobody has
    /// opened yet.
    ///
    /// Every file, including the open ones, so that the list of the whole
    /// project is one list read one way. What is placed in `troubles`
    /// above is the same news, placed, for the things that have to line up
    /// with characters on screen: the underline, the count, the complaint.
    reported: HashMap<PathBuf, Vec<obelus_lsp::trouble::Reported>>,
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
    signature: Option<obelus_lsp::signature::Signature>,
    /// What the server says the place under the caret is, while it is up.
    hover: Option<Hover>,
    /// What the server offered to do here, while a list of it is open.
    ///
    /// Code actions, which are the server's offers to change the file --
    /// not [`App::symbol_actions`], which is the menu of questions about
    /// the name under the caret. Two different things were called actions
    /// here, and this is the half that edits.
    code_actions: Vec<obelus_lsp::actions::Action>,
    /// Every use of the name the pointer is resting on, in this file.
    ///
    /// Marked in the text rather than listed: the answer is "these, here",
    /// and a list would take a region of screen to say what a background
    /// says in place.
    uses: Vec<Span>,

    /// The document being changed, and when it last was.
    ///
    /// The same shape as [`Resting`] and for the same reason: there is a
    /// question worth asking once the reader stops, and none worth asking
    /// while they are still going.
    settling: Option<Settling>,
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
    filling: Option<obelus_lsp::snippet::Filling>,
    /// What each open file's tokens are, as its server last described them.
    ///
    /// Keyed by path rather than by buffer, because a buffer is a slot that
    /// is reused: a closed file's classification would otherwise answer
    /// about whatever is opened into its place. Each carries the document
    /// version it describes and is ignored once the document has moved past
    /// it, so a stale entry is inert rather than wrong.
    tokens: HashMap<PathBuf, obelus_lsp::tokens::Tokens>,
    /// Where the colours are, per file, as a server last said.
    ///
    /// Beside the tokens because it is the same kind of answer: about a
    /// whole file, kept until the file changes, and thrown away rather
    /// than shown stale.
    colours: HashMap<PathBuf, Vec<obelus_lsp::colour::Coloured>>,
    /// What a server would have the reader know, per file, as it last
    /// said.
    hints: HashMap<PathBuf, Vec<obelus_lsp::hint::Hinted>>,
    /// What is drawn in each file that the file does not contain.
    ///
    /// Both answers in one list, because a cell of a line points at one
    /// entry of it and cannot say which of two lists it meant.
    drawn: HashMap<PathBuf, Vec<obelus_ui::Drawn>>,
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
    /// A drag being held against the edge of what it is selecting in.
    ///
    /// The one thing on this screen that moves because of the reader's
    /// hand rather than because something is happening on its own -- and
    /// it has to, because a terminal says nothing at all while a held
    /// pointer is still. Without a tick behind it a reader who dragged to
    /// the edge and waited would wait for ever: the selection they are
    /// making stops where the screen does.
    dragging: Option<Dragging>,
    /// Which build this is, as whatever started obelus was told at compile
    /// time.
    ///
    /// Held rather than worked out, because obelus cannot work it out: the
    /// commit is known to the one crate with a build script, which is the
    /// binary's own, and everything under it takes it as a string like any
    /// other fact about how obelus was started.
    built: &'static str,
    /// What git says about the files in the tree, while a list of them is
    /// open.
    ///
    /// Gathered when a list opens and kept until the next one, because it is
    /// a walk of the whole tree and the rows arrive in batches afterwards.
    statuses: std::collections::HashMap<PathBuf, obelus_git::Standing>,
    /// Where an agent reaches what obelus offers it, if it could listen.
    ///
    /// Taken once and kept: the address is what each agent is told, so a
    /// second one started later reaches the same tools rather than a second
    /// server nobody asked for.
    tools_url: Option<String>,
    /// The agent obelus is talking to, once something has needed it.
    talker: Option<obelus_agent::acp::Talk>,
    /// The commands an agent asked to run, while they run.
    ///
    /// On the loop rather than on the connection's thread, because a
    /// command is a thing on the page: the row that says what is happening
    /// reads its output, and a key stops it.
    runs: obelus_agent::running::Runs,
    /// Who is waiting to be told a command has ended.
    ///
    /// The agent's `terminal/wait_for_exit`, held until the command does.
    /// Answered from the frame check rather than by blocking: the loop
    /// that draws must not wait on a compile.
    waiting_on: Vec<(
        String,
        obelus_agent::acp::Answer<Option<obelus_agent::running::Ended>>,
    )>,
    /// What obelus knows about the agents it could run.
    agents: agents::Agents,
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
    /// What a test said git would say, instead of asking it.
    given_statuses: Option<HashMap<PathBuf, obelus_git::Standing>>,
    /// Which listings the open file list is showing, in tab order.
    ///
    /// The changed listing has a tab only when something has changed, so
    /// which tab is which listing is not fixed.
    listing: Vec<Listing>,
    /// Every path the walk behind the open file list has found, and
    /// whether the tree said to ignore it.
    ///
    /// Put away so the flat listing can be shown again without walking
    /// again: the reader types, the tree is put down and these are picked
    /// up, and clearing the query puts them back down. With the flag,
    /// because it is the walk that knows which files are only there
    /// because the reader asked for them.
    found: Vec<(PathBuf, bool)>,
    /// The row the tree was on when the reader started typing.
    ///
    /// Typing turns the file list from a tree into the flat list of
    /// everything, and clearing the query turns it back. The tree either
    /// side of that is the same tree, so this is what puts the reader back
    /// on the row they were reading rather than on the file they happen to
    /// have open.
    stood_on: Option<PathBuf>,
    /// When the notes were last typed into, so they can be written once the
    /// reader stops.
    ///
    /// Structural changes -- a note added, finished, moved -- are written
    /// the moment they happen and never come through here: they are one
    /// act each, and there is nothing to wait for. Typing is not one act,
    /// and it used to be written when the reader left the page. There is no
    /// leaving a document, so this is the moment instead.
    notes_settling: Option<std::time::Instant>,
    /// A rename of a file, from the question to the act.
    ///
    /// The gap between the two is a round trip: a server that knows the
    /// language knows which other files name this one by where it is, and
    /// obelus asks before renaming it rather than leaving the reader to
    /// find out from the next build.
    renaming: Option<renaming_files::Renaming>,
    /// Which directories of the file tree are open, relative to the root.
    ///
    /// Beside the list rather than in it, the way a history's opened commit
    /// and a tree of calls are: the list is rows, and which of them exist
    /// is worked out from this.
    opened: std::collections::HashSet<PathBuf>,
    /// The commits an open history view is showing, and what is open in it.
    history: history_view::Showing,
    /// The tree of calls an open list of them is showing.
    calls: Option<hierarchy::Calls>,
    /// Which scopes the open search is showing, in tab order.
    ///
    /// The tabs are only the scopes that can answer, so which tab is which
    /// scope is not a fixed mapping and has to be remembered.
    searching: Vec<Scope>,
    /// Which radii the open list of problems is showing, in tab order.
    ///
    /// The same remembering for the same reason: a tab is only there when
    /// it has something to answer with, so its position is not fixed.
    /// Empty when the list showing is not that one.
    troubling: Vec<semantics::Wrong>,
    /// Who last changed each line, per file that has been asked about.
    ///
    /// Kept rather than replaced, because a reader goes back and forth
    /// between two files and a blame is a walk of history: asking again for
    /// one they left a moment ago would spend that walk twice. Bounded by
    /// the files opened in a session, which is tens of them.
    blames: std::collections::HashMap<
        (PathBuf, Option<gix::ObjectId>),
        Vec<Option<obelus_git::Blamed>>,
    >,
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
    looking: obelus_search::Looking,
    /// Which search the answers arriving belong to.
    ///
    /// Bumped on every keystroke that changes what is being asked, so the
    /// batches for the query before it are recognizable as stale. Shared
    /// with the scanning threads, which read it to find out that they are
    /// answering a question nobody is asking any more.
    search_generation: obelus_runtime::cancel::Latest,
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
    /// A directory on the command line is a reader saying which project
    /// rather than which file, and "which file" is what the list answers.
    /// A flag rather than a list opened on the spot, because the rows come
    /// from a walk on the loop's own channel and there is no channel until
    /// [`App::start`].
    list_at_start: bool,
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

impl App {
    /// Starts with the shipped key table and the given documents open.
    #[must_use]
    pub fn new(open: Vec<Buffer>) -> Self {
        let current = (!open.is_empty()).then(|| DocumentId::new(0));
        let documents: Vec<Option<Document>> =
            open.into_iter().map(Document::from).map(Some).collect();
        Self {
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
            clicked: None,
            signature: None,
            hover: None,
            code_actions: Vec::new(),
            uses: Vec::new(),
            resting: None,
            settling: None,
            troubles: HashMap::new(),
            reported: HashMap::new(),
            completion: None,
            filling: None,
            tokens: HashMap::new(),
            colours: HashMap::new(),
            hints: HashMap::new(),
            drawn: HashMap::new(),
            jumps: JumpList::default(),
            preview: None,
            phase: 0,
            ticker: None,
            waking: false,
            dragging: None,
            built: "",
            prompt: None,
            changes: None,
            statuses: std::collections::HashMap::new(),

            tools_url: None,
            talker: None,
            runs: obelus_agent::running::Runs::default(),
            waiting_on: Vec::new(),
            settled: preferences::Settled::default(),
            agents: agents::Agents::default(),
            settings: None,
            counts: None,
            screen_area: Rect::ZERO,
            given_statuses: None,
            listing: Vec::new(),
            found: Vec::new(),
            stood_on: None,
            notes_settling: None,
            renaming: None,
            opened: std::collections::HashSet::new(),
            history: history_view::Showing::default(),
            calls: None,
            searching: Vec::new(),
            troubling: Vec::new(),
            blames: std::collections::HashMap::new(),
            committed: None,
            asking_blame: std::collections::HashSet::new(),
            row_syntax: std::collections::HashMap::new(),
            searched: None,
            looking: obelus_search::Looking::default(),
            outside: false,
            search_generation: obelus_runtime::cancel::Latest::default(),
            history_generation: obelus_runtime::cancel::Latest::default(),
            asked_line: None,
            rendered: None,
            theme_before: None,
            note: None,
            walk_generation: obelus_runtime::cancel::Latest::default(),
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

    /// Puts the application on a project.
    ///
    /// Before the settings are read, always: a project has settings of its
    /// own and a theme beside them, and finding those means knowing which
    /// project first. [`App::load_config`] lays the project's answers over the
    /// reader's at the end, so this only has to have happened by then.
    pub fn work_in(&mut self, root: PathBuf) {
        self.working_directory = root;
    }

    /// Says which build this is, which only the binary knows.
    pub const fn built_at(&mut self, said: &'static str) {
        self.built = said;
    }

    /// Says to open on the file list rather than on a file.
    pub fn list_at_start(&mut self) {
        self.list_at_start = true;
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
        // What obelus offers an agent back. Started with the loop rather
        // than with the first agent, because the address is what an agent is
        // told and telling two of them two addresses would be two servers.
        match obelus_mcp::serve(&self.working_directory, std::sync::Arc::new(sender.clone())) {
            // Said, because the silent half of this is the half nobody can
            // ask about: whether an agent was offered anything, and whether
            // it took it, were both questions obelus had no answer to.
            Ok(url) => {
                tracing::info!(url, "obelus is offering an agent its tools");
                self.tools_url = Some(url);
            }
            Err(error) => {
                // Not a reason to stop: an obelus that cannot listen is an
                // obelus an agent cannot ask anything of, which is what it
                // was until now.
                tracing::warn!(%error, "obelus is offering an agent nothing");
            }
        }
        self.start_watching(sender);
        for index in 0..self.documents.len() {
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
        self.history_generation.now()
    }

    /// Which document is being read, for a test that wants to know whether
    /// a key took the reader somewhere new.
    #[must_use]
    pub const fn current_document_for_test(&self) -> Option<DocumentId> {
        self.current
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

    /// Says where obelus's own tools are, without listening anywhere.
    ///
    /// The loop starts a server and puts its address here; a test wants the
    /// address handed to an agent without a port being opened for it, which
    /// is what an agent is told rather than what it finds at the other end.
    pub fn tools_url_for_test(&mut self, url: &str) {
        self.tools_url = Some(url.to_string());
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
        // is what putting obelus on a project means: at startup the two happen
        // together, and a test that moved one without the other would be
        // testing an application no reader can have.
        self.apply_project();
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
        for buffer in self.documents.iter().flatten().filter_map(Document::file) {
            if let Err(error) = watcher.watch(buffer.path()) {
                tracing::warn!(%error, path = %buffer.path().display(), "not watching");
            }
        }
        // And what git keeps its state in, because obelus is not the only
        // thing in the repository: a commit in another window, or in a
        // shell, changes what has changed in every file on screen. The
        // margin would otherwise go on showing a diff against a commit that
        // is no longer the one the file is against.
        for path in obelus_git::state_of(&self.working_directory) {
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
            for path in [obelus_config::resolved(&path), path] {
                if let Err(error) = watcher.watch(&path) {
                    tracing::warn!(%error, path = %path.display(), "not watching the settings");
                }
            }
        }
        // And the project's own settings, for the same reason twice over:
        // another obelus on this project may be looking at them, and a `git
        // pull` rewrites them under everybody.
        // The file the project *would* have, not the one it has: watching only
        // what was there at startup is the "read once" mistake with a longer
        // fuse, because it looks right until somebody creates the file --
        // the window next door writing the project's first setting, or a
        // pull bringing one.
        let project = obelus_config::project_path_for(&self.working_directory);
        if let Err(error) = watcher.watch(&project) {
            tracing::warn!(%error, path = %project.display(), "not watching the project's settings");
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
    pub const fn signature(&self) -> Option<&obelus_lsp::signature::Signature> {
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

    /// The area a list is drawn in, which is not always the editor region.
    ///
    /// Over a conversation it is everything above a question the agent is
    /// waiting on an answer to, and everything otherwise: the drawing's own
    /// answer, which this has to be the same as or the rows a key moves
    /// through are not the rows on screen.
    ///
    /// An area and not a [`layers::Room`]: a room is how much of the screen
    /// a view declares it takes, and this is the rectangle that comes out of
    /// laying one out.
    fn picker_area(&self) -> Rect {
        obelus_ui::chat::above_a_question(self.drawn_in(), self.card())
    }

    /// The region a view drawn over the file is drawn in.
    ///
    /// Worked out from the screen, the way the drawing works it out, rather
    /// than read from [`Self::editor_area`] -- which is what the *document*
    /// has, and a compact list takes room off it. Asked against that, a
    /// press in the command palette was measured from a rect ten rows above
    /// the one the palette had drawn itself in: its tabs answered nothing,
    /// and its rows answered about the wrong ones.
    fn drawn_in(&self) -> Rect {
        obelus_ui::regions(self.screen_area).editor
    }

    /// Whether anything on screen is moving.
    ///
    /// Several reasons, each said out loud. It was one question with a
    /// `match` on whether the conversation was showing, and that stopped
    /// being true the moment a list of open documents could say an agent is
    /// working in one the reader is not looking at -- a mark that only
    /// turns while you are watching it is a mark that never turns. The
    /// notes say it too, about the conversation a note has.
    ///
    /// Asked every frame from what is true, rather than switched on and off
    /// from the half-dozen places that change any of it, which is how a
    /// ticker outlives its reason.
    fn wants_animating(&self, working: bool) -> bool {
        // Nothing open at all: the welcome screen's sheen.
        self.current.is_none()
            // An agent at work in the conversation being read.
            || working
            // Or in one that is not, while the list that says so is open.
            || (self.selected_document().is_some() && self.anything_working())
            // Or while the notes are, which say the same thing about the
            // conversation a note has: the mark beside a note turns for
            // exactly as long as its agent is at work, and without this
            // it would be woken only by the reader typing -- a mark that
            // moves when you touch it and stands still while the work
            // happens.
            || (self.notes().is_some() && self.anything_working())
            // A row of a tree of calls waiting on a server. The same rule
            // as a conversation's: a mark that turns has to be woken, and
            // a mark that does not turn is a mark saying nothing is
            // happening.
            || self.calls_turning()
            // And a drag held against an edge, which is the one of these
            // that is waiting on the reader's hand rather than on
            // something happening by itself. It is here for the same
            // reason as the rest: without a tick it stops, and a
            // selection that stops at the edge of the screen is a
            // selection of what fits on it.
            || self.dragging.is_some()
    }

    /// Whether an agent is at work in any conversation at all.
    fn anything_working(&self) -> bool {
        let Some(talker) = self.talker.as_ref() else {
            return false;
        };
        self.documents
            .iter()
            .flatten()
            .filter_map(Document::chat)
            .any(|talk| talker.is_thinking(talk.session.as_ref()))
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
        self.documents
            .iter()
            .flatten()
            .filter_map(Document::file)
            .any(Buffer::syntax_is_behind)
    }

    /// Works out what every document that owes it means now.
    ///
    /// Everything open rather than what is on screen: a tree left behind on
    /// a document nobody is looking at would keep the ticker awake for the
    /// rest of the session.
    pub(crate) fn settle_syntax(&mut self) {
        // Nothing else has to be told: the text did not move, only what
        // obelus knows about it, so everything keyed on the version stays
        // keyed on the version it already had.
        for buffer in self
            .documents
            .iter_mut()
            .flatten()
            .filter_map(Document::file_mut)
        {
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
    /// cannot open a second dialog over the first -- `f1` in a
    /// conversation used to put a file list on top of it, which then took
    /// two escapes to leave and gave no way to tell which of the two a key
    /// would reach.
    pub(crate) fn context(&self) -> Context {
        // A list whose rows are open files is the list of open files, and
        // that one has a command of its own.
        if self.selected_document().is_some() {
            return Context::Documents;
        }
        if self.is_showing_dialog() {
            return Context::Dialog;
        }
        if self.conversation().is_some() {
            return Context::Chat;
        }
        Context::Normal
    }

    /// What is on screen over the file, worked out from what is open.
    ///
    /// The one answer. Everything that used to ask "is a dialog showing" or
    /// "which of these is nearest" asks this instead, and the `match` below
    /// is the single place where which field means which layer is written
    /// down. It is exhaustive, so a view added without an answer here is a
    /// view that does not compile.
    #[must_use]
    pub fn layers(&self) -> layers::Layers {
        layers::Layers::showing(|layer| match layer {
            layers::Layer::Counts => self.counts.is_some(),
            layers::Layer::Settings => self.settings.is_some(),
            layers::Layer::Picker => self.picker.is_some(),
            layers::Layer::Prompt => self.prompt.is_some(),
        })
    }

    /// Clears the room a view is about to take.
    ///
    /// The one thing every opener does, in one place. There were six of
    /// them, each with its own idea: two cleared a list and a question, one
    /// cleared a list and the settings, and three cleared nothing at all --
    /// including the two added most recently, which is the shape of the
    /// problem. Nothing made anybody think about it, so nobody did.
    ///
    /// The rule is not "opening covers": that is false for a question on the
    /// status bar, which is about the thing now behind it, and it would
    /// allow two pages at once. The rule is that a view covers what shares
    /// its room, which is [`Room::covers`] and is declared beside the view
    /// rather than here.
    pub(crate) fn make_room(&mut self, room: layers::Room) {
        for layer in self.layers().nearest_first() {
            if room.covers(layer.room()) {
                self.leave(layer);
            }
        }
    }

    /// Takes one layer down, doing whatever leaving it means.
    ///
    /// The same door escape goes through, so a view cannot be left one way
    /// and not the other: the notes are written down however the reader
    /// leaves them, and a list that was an agent's question is answered on
    /// the way out however it goes.
    pub(crate) fn leave(&mut self, layer: Layer) {
        match layer {
            Layer::Picker => {
                self.picker = None;
                // Back to where they were looking from. The other way out
                // of a list is choosing a row, and that goes somewhere on
                // purpose -- see `App::accept`.
                self.look_back();
                self.history = history_view::Showing::default();
                self.troubling.clear();
                self.close_calls();
                // What a server offered to do here, which the rows were
                // indexes into. A row is chosen by its position, so offers
                // outliving their list are offers pointing at nothing.
                self.code_actions.clear();
                // A list that was an agent's question has to be answered
                // even when the reader walks away from it: an agent whose
                // permission request goes unanswered waits for ever.
                if self.is_asking_permission() {
                    self.refuse_permission();
                }
                // And so does a form: a field left unanswered is the whole
                // form declined, because the agent is waiting on all of it.
                if self.is_asking() {
                    self.refuse_asking();
                }
                // A theme previewed but not chosen. Nothing else a picker
                // shows changes the application while it is open, so
                // nothing else has to be put back.
                if let Some((name, before)) = self.theme_before.take() {
                    self.set_theme(&name, before);
                }
            }
            Layer::Settings => self.settings = None,
            Layer::Counts => self.counts = None,
            Layer::Prompt => self.prompt = None,
        }
    }

    /// Whether something is showing that the reader is *in*.
    ///
    /// A list, the settings, or the counts: each takes the keys itself, each
    /// is left with escape, and each is over whatever is being read rather
    /// than being it. Obelus's own commands do not run from inside one, so
    /// the only way to a second one is to leave the first.
    ///
    /// A conversation is not one of these, and stopped being one when it
    /// became a document: it is *what* is being read, not something over it,
    /// which is why obelus's own keys work inside one.
    ///
    /// The question on the status bar is not one of these. It is a row
    /// rather than a screen, what it is asking about is still visible
    /// behind it, and it says its own answer to escape.
    #[must_use]
    pub fn is_showing_dialog(&self) -> bool {
        self.layers()
            .furthest_first()
            .any(|layer| matches!(layer.context(), obelus_editing::keymap::Context::Dialog))
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
    /// what `⠋` against `⠼` means, and it says nothing about obelus.
    pub const fn phase_for_test(&mut self, phase: u32) {
        self.phase = phase;
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
                let before = obelus_ui::editor::text_offset(
                    buffer.text().line_count(),
                    obelus_ui::editor::changed(self.changes()),
                    !buffer.folds().is_empty(),
                );
                let after = obelus_ui::editor::map_width(self.changes());
                self.editor_area
                    .width
                    .saturating_sub(before)
                    .saturating_sub(after)
                    .saturating_sub(obelus_ui::editor::SCROLLBAR_WIDTH)
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
        // What this frame has, before anything is laid out against it.
        // Everything below asks for the room through `editor_area`, so
        // setting it afterwards laid the notes out at the width the *last*
        // frame had -- right on a screen that is not changing, and wrong on
        // every frame that changed it: the view opening, a terminal
        // resized, a region growing as something over it closes. The next
        // redraw put it right, so what a reader saw was their words go and
        // come back.
        self.editor_area = editor_area;
        // The notes are laid out against the room they have: a terminal is
        // resized and a setting is changed while they are open, and the rows
        // they are made of depend on both.
        let laid = self.notes_laid_out();
        if let Some(notes) = self.notes_mut() {
            notes.lay_out(laid.0, laid.1);
        }
        // And the window against the rows that room leaves, which is the
        // other half of the same question: the width says what the rows
        // are and this says how many of them the reader can see. Asked
        // after the laying out, because the rows have to exist before the
        // window can be put over them, and from the region the list is
        // really drawn in -- the foot under it is part of what decides how
        // many rows there are, and it grows and shrinks with what the note
        // under the caret can do.
        let seen = self.notes().map(|notes| {
            obelus_ui::todo::list_region(self.editor_area, &obelus_ui::todo::hints(notes)).height
        });
        if let (Some(seen), Some(notes)) = (seen, self.notes_mut()) {
            notes.settle_window(seen);
        }
        self.check_servers();
        self.check_runs();
        self.show_what_is_wrong();
        // The marks on a list of open documents, which say what an agent is
        // doing in a conversation nobody is watching. Here rather than
        // where the list is built, because that is the whole point: the
        // rows are a snapshot and this is the part of them that is about
        // now.
        self.freshen_the_document_marks();
        // What the conversation says is happening, read off the state
        // rather than remembered: a row that is worked out every frame
        // cannot be left saying something that stopped being true.
        let doing = match self.talking() {
            Talking::Starting => Some("starting\u{2026}"),
            Talking::Thinking => Some("thinking\u{2026}"),
            Talking::Nobody | Talking::Idle | Talking::Ready | Talking::Gone => None,
        };
        self.in_transcript(|chat| chat.doing(doing));
        self.show_what_is_running();
        // A grammar too slow to keep up with typing leaves a tree owing an
        // answer, and the ticker is what comes back for it: the reader
        // stops, the next tick lands, and the colours catch up.
        self.animate(
            self.wants_animating(doing.is_some())
                || self.anything_behind()
                || self.is_resting()
                || self.settling.is_some()
                // And a rename waiting on a server: the clock is what ends
                // that wait, so a clock that is asleep would leave the
                // reader's file where it was for ever.
                || self.renaming_is_waiting()
                // And the notes waiting to be written down.
                || self.notes_settling.is_some(),
        );

        // Which rows the list will draw is what decides which rows need
        // their matched characters worked out, and only the geometry knows
        // how many rows there are. The room is the room it is *drawn* in,
        // which over a conversation is everything above the box.
        let rows = self
            .picker
            .as_ref()
            .map(|picker| obelus_ui::picker::rows_drawn(picker, self.picker_area()));
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
        // And what a server works out about a file the reader has stopped
        // changing, which is noticed the same way.
        self.settle_changes();
        // And what a server works out about a file the reader has stopped
        // changing, which is noticed the same way.
        self.settle_changes();
        // And what the call under the caret takes. It is the third of the
        // panels that belong to a place in the file, and it was the one
        // that never asked whether the file was still what the reader is
        // looking at: the other two go when a view opens over them, and
        // this one stayed, drawn over a screen it is not about.
        if self.layers().any() {
            self.signature = None;
        }
        if let Some(hover) = self.hover.as_mut() {
            // What it is drawn in, so that paging it moves what is on
            // screen rather than a number nothing reads.
            hover.settle(
                obelus_ui::hover::room(editor_area),
                obelus_component::hover::MOST_ROWS,
            );
        }
        // How much room the panel's two halves have, which the keys need
        // as much as the drawing does: a page of documentation is the rows
        // of it that are on screen, and only the geometry knows how many
        // that is.
        if let Some(panel) = obelus_ui::complete::layout(self, editor_area)
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
                    .saturating_sub(obelus_ui::editor::SCROLLBAR_WIDTH),
            );
            completion.settle(panel.list, panel.documentation);
        }

        self.refresh_rendering(
            editor_area
                .width
                .saturating_sub(obelus_ui::editor::SCROLLBAR_WIDTH),
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
            .filter(|buffer| buffer.mode() == obelus_buffer::Mode::Edit)
        {
            buffer.scroll_into_view(area);
        }

        self.refresh_preview(editor_area);
        self.look_at_the_selection();

        let Self {
            documents,
            current,
            highlights,
            ..
        } = self;
        let Some(buffer) = current
            .and_then(|id| documents.get(id.get()))
            .and_then(Option::as_ref)
            .and_then(Document::file)
        else {
            highlights.clear();
            return;
        };
        let Some(state) = buffer.syntax() else {
            highlights.clear();
            return;
        };
        let range = buffer.visible_bytes(area.height);
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
        self.prepare(obelus_ui::editor_room(area, self));
        obelus_ui::draw(cells, area, self);
        obelus_ui::cursor_position(area, self)
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
            Event::Watched(obelus_watch::Changed { path }) => {
                // The settings, by either of their names: the watcher
                // reports whichever path the change arrived on, and a
                // change that came from a repository arrives on the file
                // the link points at rather than on the link.
                let readers = self.settled.path.as_deref().is_some_and(|config| {
                    path == config || path == obelus_config::resolved(config)
                });
                // Or the project's own, which is a change to the settings just
                // as much -- it is the layer over them. Against the file the
                // project *would* have rather than the one it has, so that the
                // file appearing is a change like any other: the ordinary
                // case is a project with no settings yet, and the moment
                // worth hearing about is the one where it gets some.
                let project = obelus_config::project_path_for(&self.working_directory);
                let project = path == project;
                if readers || project {
                    self.reread_config();
                } else if self.is_a_theme(&path) {
                    // The colours the reader is already wearing, read again:
                    // the name in the settings has not moved, and what it
                    // stands for has.
                    self.reread_theme();
                } else if self.is_the_notes_file(&path) {
                    // What the project means to come back to, written by
                    // another obelus, the reader's own editor -- or by this
                    // obelus, which hears its own writes like anybody
                    // else's. Not told apart, because there is nothing to
                    // gain by it: a reread keeps the box the reader is
                    // typing in and puts the caret back by name, so reading
                    // back what obelus itself just wrote changes nothing on
                    // the page.
                    self.reread_notes();
                } else if obelus_git::state_moved(&path) {
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
            Event::Lsp(obelus_lsp::Message { language, message }) => {
                let Some(client) = self.servers.get_mut(&language) else {
                    return;
                };
                // Whether it had finished its handshake before this
                // message, because finishing one is a moment obelus has to
                // act on: every standing question about an open file is
                // refused while a server cannot say what it answers, and
                // opening a file is the moment they are all asked.
                let handshaken = client.capabilities().is_some();
                // And whether it was busy, because stopping is the other
                // moment worth acting on: a server that has not finished
                // reading the project answers what it can, which for the
                // questions below is nothing at all -- measured against
                // rust-analyzer, an empty list a second after the
                // handshake, and nothing asking again for as long as the
                // reader sits still.
                let working = client.working_on().is_some();
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
                // And now it can say what it answers. Without this a file
                // opened before its server was ready is a file nothing is
                // ever asked about: the questions were all refused, and the
                // next thing that asks them is the reader saving.
                let now = self.servers.get(&language);
                let ready = now.is_some_and(|client| client.capabilities().is_some());
                let busy = now.is_some_and(|client| client.working_on().is_some());
                if ready && (!handshaken || (working && !busy)) {
                    self.ask_about_open_files(language);
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
                self.drag_on();
                // The pause the slow grammars are waiting for. A tick that
                // lands mid-word settles the tree that word began in, which
                // is one parse for a burst of typing rather than one per
                // key.
                self.settle_syntax();
                self.settle_notes();
                // And the end of the wait for a server that was asked what
                // a rename changes. A file the reader asked to be called
                // something else is not held up by a subprocess that
                // stopped talking.
                self.rename_without_them();
            }
            Event::Search(obelus_search::Event::Matches {
                generation,
                hits,
                done,
            }) => self.on_matches(generation, hits, done),
            Event::Agent(obelus_agent::Event::Acp(message)) => self.on_acp(message),
            Event::Notes(obelus_mcp::Asked { doing, answer }) => {
                let _ = answer.send(self.change_the_notes(doing));
            }
            Event::Agent(obelus_agent::Event::Registry { agents, failure }) => {
                self.on_registry(agents, failure)
            }
            Event::Agent(obelus_agent::Event::Icon { id, svg }) => self.on_icon(id, svg),
            Event::Agent(obelus_agent::Event::Installing { id, progress }) => {
                self.on_installing(id, progress)
            }
            Event::Agent(obelus_agent::Event::Installed { id, failure }) => {
                self.on_installed(id, failure)
            }
            Event::Git(obelus_git::Event::Blamed { path, at, lines }) => {
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
            Event::Git(obelus_git::Event::Logged {
                generation,
                commits,
                walked,
                done,
            }) => {
                // A batch from a walk whose list is gone, or from one
                // superseded by another tab, another file, another key.
                if self.history_generation.is_current(generation) {
                    self.on_logged(commits, walked, done);
                }
            }
            Event::Search(obelus_search::Event::FilesFound {
                generation,
                paths,
                ignored,
            }) => {
                // A batch from a walk whose picker is gone, or from one
                // superseded by a later open.
                if !self.walk_generation.is_current(generation) {
                    return;
                }
                // Kept as well as shown. A file list is a tree while
                // nothing is typed and these rows the moment something is,
                // and a reader who types, clears and types again would
                // otherwise wait for a fresh walk each time -- which is a
                // walk per first keystroke rather than one per opening.
                self.found
                    .extend(paths.iter().map(|path| (path.clone(), ignored)));
                // Drawn only where the flat listing is what is showing: on
                // the tab these rows are about, with something typed. A
                // batch arriving while the tree is up would mix a walk of
                // the whole project into the branch the reader has open,
                // and one arriving on the changed tab is the other tab's
                // answer.
                if !self.showing_found() {
                    return;
                }
                if let Some(picker) = self.picker.as_mut() {
                    let statuses = &self.statuses;
                    let root = &self.working_directory;
                    picker.extend(paths.into_iter().map(|path| {
                        PickerItem {
                            prose: false,
                            marker: None,
                            icon: Some(obelus_icons::for_path(&path)),
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
                                true => Some(obelus_git::FileStatus::Ignored),
                                false => statuses
                                    .get(&root.join(&path))
                                    .map(|standing| standing.status),
                            },
                            depth: 0,
                            opens: None,
                            kind: None,
                            tab: None,
                        }
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
        // And a drag is over. Mostly it ended with the button coming up,
        // but a pointer that leaves the terminal takes its release with
        // it, and a drag nothing ever ended would go on scrolling under
        // whatever the reader did next.
        self.dragging = None;

        // Except the paging keys, while a preview is on screen: a screenful
        // is what the thing being *read* is moved by, and the list above it
        // is ten rows with its ends a keypress away. With control they page
        // the list, which is the other half of the same swap.
        if self.page_preview(&key) {
            return;
        }
        // The keys a file list and a search have that are not about moving
        // around them. Before the picker, because the picker would not know
        // them: what they change is where the rows come from, which is the
        // application's.
        if self.listing_key(&key) || self.searching_key(&key) {
            return;
        }

        // And then whatever is over the file, nearest the reader first --
        // because escape belongs to whatever is in front, and every other
        // key belongs to whatever owns the thing it moves. One order, the
        // one `layers` declares, read backwards. What a layer does not want
        // falls through it, which is how `ctrl+q` still leaves obelus from
        // inside any of them.
        for layer in self.layers().nearest_first() {
            let taken = match layer {
                Layer::Prompt => self.prompt_key(&key),
                Layer::Picker => self.picker_key(&key),
                Layer::Settings => self.settings_key(&key),
                Layer::Counts => self.counts_key(&key),
            };
            if taken {
                return;
            }
        }

        // The document being read, where that document is a conversation.
        // After the layers, because a list or a page is over it the way it
        // is over a file; before the panels and the file's own keys, which
        // are about a file and there is not one.
        if self.chat_key(&key) {
            return;
        }

        // And where that document is the notes. Beside the conversation
        // rather than in the loop above, because that is what it now is:
        // something the reader goes to, not something over what they were
        // reading.
        if self.notes_key(&key) {
            return;
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

        // The file being read: the motions, the paging and the typing. It
        // goes last of the keys because everything above it is something
        // opened *over* the file, and it refuses outright while any of
        // those is showing.
        if self.editor_key(&key) {
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

        let status = obelus_ui::regions(self.screen_area).status;
        if status.height == 0 || y != status.y || x < status.x || x >= status.right() {
            return false;
        }
        // The agent's settings, which are what this row carries while a
        // conversation is what the screen is showing. Each is a word saying
        // what the session is set to, and each is one key away -- so a
        // press on one goes to it and does what that key does: a switch
        // flips, and one with a list behind it opens the list.
        if kind == Pointer::Pressed
            && !self.layers().any()
            && let Some(view) = obelus_ui::chat::ChatView::new(self)
            && let Some(at) = view.setting_at(status, x, y)
        {
            if let Some(talk) = self.conversation_mut() {
                talk.chat.stand_on_setting(at);
            }
            self.chat_key(&enter());
            return true;
        }
        // Which box is showing, and how far in its text starts. The same
        // order the keys go in by, and the same insets the renderer draws
        // them at.
        let inset = if self.prompt.is_some() {
            self.prompt.as_ref().map(obelus_ui::status::answer_inset)
        } else if self.settings.is_some() {
            Some(obelus_ui::status::typed_inset(None))
        } else {
            self.picker
                .as_ref()
                .map(|picker| obelus_ui::status::typed_inset(picker.question()))
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

    /// What the pointer did to the box a note is written in.
    ///
    /// Where the box is on screen is the view's to say, so it says it --
    /// the same function that puts the caret there, read backwards.
    fn pointer_in_notes(&mut self, kind: crate::event::Pointer, x: u16, y: u16) {
        use crate::event::Pointer;

        let area = self.editor_area;
        // The mark and the box first, which are the two things a row of the
        // list draws that the reader can *do* something to: the box says
        // whether the note is done, and the mark says somebody has talked
        // about it. Both are one key away and both are a picture of that
        // key, so a press on one does what the key does.
        if kind == Pointer::Pressed && self.press_in_a_note(x, y) {
            return;
        }
        let Some(at) = self
            .notes()
            .and_then(|notes| obelus_ui::todo::place_at(area, notes, x, y))
        else {
            return;
        };
        let clicks = match kind {
            Pointer::Pressed => self.clicks_at(x, y),
            _ => 0,
        };
        let Some(notes) = self.notes_mut() else {
            return;
        };
        let width = notes.caret_width();
        let Some(composer) = notes.writing_mut() else {
            return;
        };
        match kind {
            Pointer::Moved | Pointer::Released => {}
            Pointer::Dragged => composer.place_at_cell(at.0, at.1, width, true),
            Pointer::Pressed => {
                composer.place_at_cell(at.0, at.1, width, false);
                match clicks {
                    2 => composer.hold_word(width),
                    3 => composer.hold_line(width),
                    _ => {}
                }
            }
        }
    }

    /// What the pointer did to the box a message is written in.
    fn pointer_in_chat(&mut self, kind: crate::event::Pointer, x: u16, y: u16) {
        use crate::event::Pointer;

        let area = self.editor_area;
        // Worked out before the conversation is borrowed to change: the
        // card is the application's and the box is the conversation's.
        let carded = self.card().is_some();
        // The card first, where one is up: it is what covers the box, and
        // every row of it is a thing the reader answers with. What lands
        // above it is still the transcript, so a question on screen does
        // not stop the reader taking a copy of what led to it.
        if carded && self.press_in_card(kind, x, y) {
            return;
        }
        let Some(at) = self
            .conversation()
            .and_then(|talk| obelus_ui::chat::ChatView::place_at(area, &talk.chat, carded, x, y))
        else {
            self.pointer_in_transcript(kind, x, y);
            return;
        };
        let clicks = match kind {
            Pointer::Pressed => self.clicks_at(x, y),
            _ => 0,
        };
        let width = obelus_ui::chat::writing_width(area);
        let mut held = false;
        self.in_transcript(|chat| {
            let writing = chat.writing_mut();
            match kind {
                Pointer::Moved | Pointer::Released => {}
                Pointer::Dragged => writing.place_at_cell(at.0, at.1, width, true),
                Pointer::Pressed => {
                    // One selection between the two halves, and this is
                    // the other half taking hold.
                    held = true;
                    writing.place_at_cell(at.0, at.1, width, false);
                    match clicks {
                        2 => writing.hold_word(width),
                        3 => writing.hold_line(width),
                        _ => {}
                    }
                }
            }
        });
        if held {
            self.in_transcript(obelus_component::chat::Chat::let_go);
        }
    }

    /// What the pointer did to a view drawn over the file.
    ///
    /// Which for a long time was nothing at all: the wheel reached these --
    /// it is its own event and goes to whichever layer is nearest -- and a
    /// press did not, so a reader could scroll a list of files and not
    /// point at one. The query box was the exception, and a telling one:
    /// it is on the status row, which is asked before this, so the box was
    /// clickable and the list under it was not.
    ///
    /// A press moves the selection and nothing else. What *chooses* a row
    /// stays on the keyboard, because these lists are opened over a file
    /// and drawn where a mis-aimed press would otherwise take the reader
    /// somewhere they did not ask to go. The one exception is a row's own
    /// arrow, which says the row opens: pressing that does what pressing
    /// the arrow means everywhere, and cannot take the reader anywhere,
    /// because the arrow is declared on the rows that open and on no
    /// others.
    fn pointer_in_a_layer(&mut self, kind: crate::event::Pointer, x: u16, y: u16) {
        use crate::event::Pointer;

        if kind != Pointer::Pressed {
            return;
        }
        // Whichever is nearest the reader, which is the one drawn over the
        // others: the same order a key is offered in.
        let Some(layer) = self.layers().nearest_first().next() else {
            return;
        };
        match layer {
            // The status row, which was asked before this.
            obelus_component::layers::Layer::Prompt => {}
            obelus_component::layers::Layer::Picker => self.press_in_picker(x, y),
            obelus_component::layers::Layer::Counts => self.press_in_counts(x, y),
            obelus_component::layers::Layer::Settings => self.press_in_settings(x, y),
        }
    }

    /// Walks to one of a view's tabs, by the shorter way round.
    ///
    /// The tabs wrap, so from where the reader is to where they pressed is
    /// at most half the tabs away -- which for every list obelus has is one
    /// step. Walked rather than jumped because what a tab *costs* is the
    /// application's: a scope asks the search again, a radius walks the
    /// history again, a direction turns the calls round. Going the short
    /// way is what keeps a tab in between from being asked its question on
    /// the way past.
    fn walk_to_tab(
        &mut self,
        now: usize,
        wanted: usize,
        count: usize,
        key: impl Fn(&mut Self, bool),
    ) {
        if count == 0 || wanted == now {
            return;
        }
        let forward = (wanted + count - now) % count;
        let backward = (now + count - wanted) % count;
        let (steps, onwards) = match forward <= backward {
            true => (forward, true),
            false => (backward, false),
        };
        for _ in 0..steps {
            key(self, onwards);
        }
    }

    /// A press in a list of rows to choose from.
    fn press_in_picker(&mut self, x: u16, y: u16) {
        // Where the list drew itself, not the room it was given: a compact
        // one takes as many rows as it needs against the foot of that room,
        // so the two are ten rows apart for a palette on a tall screen.
        let room = self.picker_area();
        let area = self
            .picker
            .as_ref()
            .map_or(room, |picker| obelus_ui::picker::region(picker, room));
        // The tabs, which are above the rows: pressing one is the only
        // thing a tab is for, so there is nothing else a press there could
        // have meant.
        let tab = self.picker.as_ref().and_then(|picker| {
            let row = obelus_ui::picker::tab_row(picker, area)?;
            let at = obelus_ui::tab_at(row, picker.tabs(), picker.tab(), x, y)?;
            Some((picker.tab(), at, picker.tabs().len()))
        });
        if let Some((now, wanted, count)) = tab {
            self.walk_to_tab(now, wanted, count, |app, onwards| {
                app.picker_key(&stepping(onwards));
            });
            return;
        }
        let Some((at, arrow)) = self
            .picker
            .as_ref()
            .and_then(|picker| obelus_ui::picker::row_at(picker, area, x, y))
        else {
            return;
        };
        if let Some(picker) = self.picker.as_mut() {
            picker.select_row(at);
        }
        if arrow {
            // Down the same path the key goes down, rather than a second
            // opener of its own: what enter does to the row under the
            // arrow is what the arrow is a picture of, and two of them
            // would be two answers to keep alike.
            self.picker_key(&enter());
        }
    }

    /// A press in the table of what this project is made of.
    fn press_in_counts(&mut self, x: u16, y: u16) {
        let area = self.drawn_in();
        // The tabs are the table's first row.
        let tab = self.counts.as_ref().and_then(|counts| {
            let names = counts.tabs();
            let at = obelus_ui::tab_at(Rect { height: 1, ..area }, &names, counts.tab(), x, y)?;
            Some((counts.tab(), at, names.len()))
        });
        if let Some((now, wanted, count)) = tab {
            self.walk_to_tab(now, wanted, count, |app, onwards| {
                app.counts_key(&stepping(onwards));
            });
            return;
        }
        let Some((at, mark)) = self
            .counts
            .as_ref()
            .and_then(|counts| obelus_ui::counts::row_at(area, counts, x, y))
        else {
            return;
        };
        if let Some(counts) = self.counts.as_mut() {
            counts.select_row(at);
        }
        if mark {
            self.counts_key(&enter());
        }
    }

    /// What the pointer did to the page of settings.
    ///
    /// Nothing folds there, so there is no arrow. What a press reaches is
    /// the switch: a box with a tick in it or without, which is the one
    /// thing on the page that says by its shape that pressing it changes
    /// it.
    fn press_in_settings(&mut self, x: u16, y: u16) {
        let area = self.drawn_in();
        // The tabs are the page's first row.
        let tab = self.settings.as_ref().and_then(|settings| {
            let names = settings.tabs();
            let at = obelus_ui::tab_at(Rect { height: 1, ..area }, &names, settings.tab(), x, y)?;
            Some((settings.tab(), at, names.len()))
        });
        if let Some((now, wanted, count)) = tab {
            self.walk_to_tab(now, wanted, count, |app, onwards| {
                app.settings_key(&stepping(onwards));
            });
            return;
        }
        let Some((at, switch)) =
            obelus_ui::settings::SettingsView::new(self).and_then(|view| view.row_at(area, x, y))
        else {
            return;
        };
        let offering = self.agent_offering();
        if let Some(settings) = self.settings.as_mut() {
            settings.select_row(at, offering.as_ref());
        }
        if switch {
            self.settings_key(&enter());
        }
    }

    /// A press on one of the candidates a server offered.
    ///
    /// Choosing it outright, because that is what the list is for: it is
    /// up only while the reader is in the middle of typing a word, it
    /// covers the word it is about, and there is nothing in it to browse
    /// past -- a press anywhere else puts it away.
    ///
    /// Answers whether the press was the list's, so that one beside it goes
    /// on to the file.
    fn press_in_completion(&mut self, x: u16, y: u16) -> bool {
        let Some(panel) = obelus_ui::complete::layout(self, self.editor_area) else {
            return false;
        };
        let Some(at) = self
            .completion()
            .and_then(|completion| obelus_ui::complete::row_at(panel, completion, x, y))
        else {
            return false;
        };
        if let Some(completion) = self.completion.as_mut() {
            completion.choose_row(at);
        }
        // Down the key's own path, which is what takes the word and puts
        // the list away.
        self.completion_key(&enter());
        true
    }

    /// A press on one of the two marks a note wears.
    ///
    /// Answers whether it was one of them, so that a press on the words
    /// goes on to put the caret there.
    fn press_in_a_note(&mut self, x: u16, y: u16) -> bool {
        use obelus_ui::todo::Column;

        let area = self.editor_area;
        let Some((row, column)) = self
            .notes()
            .and_then(|notes| obelus_ui::todo::row_at(area, notes, x, y))
        else {
            return false;
        };
        if column == Column::Words {
            return false;
        }
        // On to the note first: both keys ask about the note the caret is
        // in, so the note under the pointer is the note they are about.
        let note = self
            .notes()
            .and_then(|notes| notes.rows().get(row))
            .map(|row| row.note);
        if let (Some(note), Some(notes)) = (note, self.notes_mut()) {
            notes.stand_on(note);
        }
        // Down the keys' own paths, rather than a second way to tick a
        // note off and a second way to open its conversation.
        match column {
            // The arrow, which the key reaches through the command rather
            // than through the notes' own keys -- so this goes the same
            // way, which is the point of going down a key's path at all.
            Column::Folds => {
                self.toggle_fold();
                return true;
            }
            Column::Tick => self.notes_key(&crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(' '),
                crossterm::event::KeyModifiers::ALT,
            )),
            _ => self.notes_key(&crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('a'),
                crossterm::event::KeyModifiers::ALT,
            )),
        };
        true
    }

    /// What the pointer did to a question on a card.
    ///
    /// A card is a question that has taken part of the screen and is
    /// waiting, and every row of it is a thing the reader answers with --
    /// so unlike a list drawn over a file there is nothing here to browse
    /// past, and a press does what the key does on the row it landed on.
    /// The words are the exception: they are a box, and a press in a box
    /// puts the caret where it landed.
    ///
    /// Answers whether the press was the card's, so that one landing above
    /// it falls through to the transcript.
    fn press_in_card(&mut self, kind: crate::event::Pointer, x: u16, y: u16) -> bool {
        use obelus_component::card::On;

        let Some(card) = self.card() else {
            return false;
        };
        let band = obelus_ui::chat::bands_for(self.editor_area, card).writing;
        let Some(on) = obelus_ui::card::row_at(card, band, x, y) else {
            return false;
        };
        if kind != crate::event::Pointer::Pressed {
            // The card's, and nothing for a drag to do in it: said so
            // rather than let through, or a drag begun on the card would
            // take hold of the transcript behind it.
            return true;
        }
        let width = obelus_ui::card::width_of(band);
        let place = obelus_ui::card::place_at(card, band, x, y);

        let Some(card) = self.conversation_mut().and_then(|talk| talk.card.as_mut()) else {
            return true;
        };
        card.stand_on(on);
        if on == On::Words {
            if let Some((row, cell)) = place {
                card.place_in_words(row, cell, width);
            }
            return true;
        }
        // Down the key's own path: what enter does to the row under the
        // pointer is what that row is for, and a second answer about it
        // would be a second answer to keep alike.
        self.card_key(&enter());
        true
    }

    /// What the pointer did to what has been said.
    ///
    /// Which is the half of a conversation with no caret in it: there is
    /// nothing to type there, and the only thing a pointer does is take
    /// hold of some of it.
    ///
    /// One selection between the two halves, so taking hold here lets the
    /// box go. A reader dragging across an answer means that answer, and a
    /// second selection still lit in the box would leave `ctrl+c` with two
    /// things to copy and no way to say which.
    fn pointer_in_transcript(&mut self, kind: crate::event::Pointer, x: u16, y: u16) {
        use crate::event::Pointer;

        let area = self.editor_area;
        let spot = self.conversation().and_then(|talk| {
            obelus_ui::chat::ChatView::place_in_transcript(
                area,
                &talk.chat,
                talk.card.as_ref(),
                x,
                y,
            )
        });
        // Whether it landed on a heading that opens, which is a thing to do
        // to the row rather than to the words in it.
        //
        // Free of the selection, and not by luck: every row that folds is
        // one obelus drew itself -- the heading over a run of tool calls,
        // the one over a piece of thinking, the one over the agent's plan
        // -- and none of them is anybody's words. A press on one already
        // meant nothing but "let go", so opening it costs the reader
        // nothing they had.
        let width = obelus_ui::chat::reading_width(area);
        let folds = self.conversation().and_then(|talk| {
            let at = obelus_ui::chat::ChatView::row_in_transcript(
                area,
                &talk.chat,
                talk.card.as_ref(),
                y,
            )?;
            talk.chat.rows(width).get(at)?.folds
        });
        let Some(talk) = self.conversation_mut() else {
            return;
        };
        match kind {
            Pointer::Moved | Pointer::Released => {}
            Pointer::Pressed if folds.is_some() => {
                if let Some(begins) = folds {
                    talk.chat.fold(begins);
                }
            }
            Pointer::Pressed => {
                talk.chat.writing_mut().let_go();
                match spot {
                    Some(spot) => talk.chat.hold_from(spot),
                    // A press on nothing lets go, the way a press on the
                    // page does everywhere else.
                    None => talk.chat.let_go(),
                }
            }
            Pointer::Dragged => {
                if let Some(spot) = spot {
                    talk.chat.hold_to(spot);
                }
            }
        }
    }

    /// How far past the edge of what a drag is selecting in a row is, if
    /// it is past it at all.
    ///
    /// The band it asks about is the one the reader is dragging in: the
    /// transcript of a conversation, which has the box under it, and
    /// otherwise the whole region a file is read in.
    fn past_the_edge(&self, y: u16) -> Option<i16> {
        let area = self.editor_area;
        let band = match self.chat() {
            Some(chat) => obelus_ui::chat::bands(area, chat, self.card()).transcript,
            None => area,
        };
        let row = i32::from(y);
        let past = match (row < i32::from(band.y), row >= i32::from(band.bottom())) {
            (true, _) => row - i32::from(band.y),
            (_, true) => row - i32::from(band.bottom()) + 1,
            _ => return None,
        };
        i16::try_from(past).ok().filter(|past| *past != 0)
    }

    /// Keeps a drag held against an edge moving.
    ///
    /// Through the same one function a notch of the wheel goes through, so
    /// it reaches whatever the reader is dragging in -- and then the far
    /// end of the selection is put where the pointer is again, against the
    /// edge, because the rows under it have moved.
    fn drag_on(&mut self) {
        let Some(drag) = self.dragging else {
            return;
        };
        let past = i32::from(drag.past);
        // The further past the edge, the faster: a pointer held still has
        // no other way to ask for more, and one row a tick is a minute to
        // cross a morning's conversation.
        let rows = past.signum() * (1 + past.abs().min(4));
        self.scroll(isize::try_from(rows).unwrap_or(0));
        // Against the edge rather than where the pointer really is, which
        // is off the band: what is being asked is "carry on to here", and
        // here is as far as the band goes.
        let area = self.editor_area;
        let band = match self.chat() {
            Some(chat) => obelus_ui::chat::bands(area, chat, self.card()).transcript,
            None => area,
        };
        let y = drag.y.clamp(band.y, band.bottom().saturating_sub(1));
        self.on_pointer(crate::event::Pointer::Dragged, drag.x, y);
        // Which said the drag had come back inside the band, it having
        // been handed a row that is. It has not: the reader is still
        // holding it out there.
        self.dragging = Some(drag);
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

        // Whether the pointer is being held past the edge of what it is
        // selecting in, which nothing else will say again until it moves:
        // a terminal reports a drag when it happens and says nothing at
        // all while a held pointer is still.
        match kind {
            Pointer::Dragged => {
                self.dragging = self.past_the_edge(y).map(|past| Dragging { past, x, y });
            }
            Pointer::Pressed | Pointer::Released => self.dragging = None,
            Pointer::Moved => {}
        }

        // The list of what could be typed next, where one is up: it is
        // drawn over the file at the caret, so it covers the very place a
        // press would otherwise land -- and a press that went through it to
        // the text would move the caret out from under the question the
        // list is answering.
        if kind == Pointer::Pressed && self.press_in_completion(x, y) {
            return;
        }
        // The boxes on the status row first: a question, a list's query, a
        // page's filter. All three are one row, so one piece of arithmetic
        // serves them -- and a reader who can select in a box with the
        // keyboard but not with the pointer has half a selection.
        if self.pointer_on_status(kind, x, y) {
            return;
        }
        // The notes, which are a page with a box on it: the box takes the
        // pointer the way the file does, and the rest of the page takes
        // nothing rather than letting it through to the code behind.
        if self.notes().is_some() {
            self.pointer_in_notes(kind, x, y);
            return;
        }
        // Covering rather than merely open: a question on the status bar
        // leaves every line of the file where the reader can see it, and a
        // line they can see is a line they can point at.
        if self.layers().covering() {
            self.pointer_in_a_layer(kind, x, y);
            return;
        }
        // A conversation is what is being read rather than something over
        // it, so it is asked here, where a file would be. The box is the
        // half of it with a caret in it; the transcript has none.
        if self.conversation().is_some() {
            self.pointer_in_chat(kind, x, y);
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
        // Everything the view draws in front of the text. The numbers are a
        // click at the start of that row rather than nothing: the reader
        // pointed at a line, and pointing left of the words is how a whole
        // line is reached.
        //
        // The two columns beside them say something about the line that the
        // reader can *do*: the fold mark says it has more behind it, and
        // the change margin says what it replaced. Both are one key away
        // and the mark is the picture of the key -- so a click on the mark
        // does what the mark is about, which is what a mark like that means
        // everywhere a reader has met one.
        let lines = buffer.text().line_count();
        let changed = obelus_ui::editor::changed(self.changes());
        let folds = !buffer.folds().is_empty();
        let offset = obelus_ui::editor::text_offset(lines, changed, folds);
        let column = obelus_ui::editor::margin_at(x - area.x, lines, changed, folds);
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
            // A mark in the margin, pressed: the caret goes to that line --
            // both keys ask about the line the caret is on -- and then the
            // key's own work is done. Not a selection as well: the reader
            // asked for one thing.
            Pointer::Pressed
                if matches!(
                    column,
                    obelus_ui::editor::Margin::Folds | obelus_ui::editor::Margin::Changes
                ) =>
            {
                if let Some(buffer) = self.current_buffer_mut() {
                    buffer.place_at_cell(row, 0, text, false);
                }
                match column {
                    obelus_ui::editor::Margin::Folds => self.toggle_fold(),
                    _ => self.toggle_hunk(),
                }
                // The same as every other way out of here: wherever the
                // caret ended up is somewhere the reader is working from.
                if let Some(buffer) = self.current_buffer_mut() {
                    buffer.settle_undo();
                }
                return;
            }
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
    /// When it last changed.
    pub(crate) since: std::time::Instant,
}

/// A path as it should be read: relative to the root when it lies under it.
pub(crate) fn relative(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
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
    /// The project to work in, absolute, where the arguments named one.
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
/// A file names the project it is in and is opened; a directory *is* the
/// project, and the question it leaves -- which file -- is the one the list
/// answers. The first path decides the project, because a reader who names
/// two has said which they meant first.
///
/// Absolute, and by the same rule [`obelus_buffer::Buffer::open`] uses on
/// a file: made absolute rather than canonical, so a project reached through
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
        // is a reader saying which project and asking which file.
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
/// It blocks on the channel, and the terminal is read by a thread that
/// sends into the same channel. Which way round that goes is the whole
/// design, and it is not obvious from here, so: a loop has to block on
/// exactly one thing or spin, and obelus has two sides to wait on -- the
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
/// -- and obelus draws when something happened, which is what lets it be a
/// process that is genuinely asleep when nothing is.
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

/// What the renderer may ask the application.
///
/// Every one of these forwards to the method of the same name: the trait is
/// the list of questions, and the answers stay where they are written. The
/// two cannot drift -- a signature that stopped matching is a compile error
/// here -- and the alternative, moving forty-four methods out of the
/// application's own impl blocks, would have made every one of its several
/// hundred internal calls go through a trait that has to be in scope.
impl Screen for App {
    fn agent_name(&self) -> Option<&str> {
        App::agent_name(self)
    }
    fn agent_settings(&self) -> &[acp::Setting] {
        App::agent_settings(self)
    }
    fn agent_offering(&self) -> Option<obelus_component::settings::Offering> {
        App::agent_offering(self)
    }
    fn agent_usage(&self) -> Option<&acp::Usage> {
        App::agent_usage(self)
    }
    fn blame(&self) -> Option<&[Option<obelus_git::Blamed>]> {
        App::blame(self)
    }
    fn built(&self) -> &str {
        self.built
    }
    fn card(&self) -> Option<&Card> {
        App::card(self)
    }
    fn changes(&self) -> Option<&obelus_git::Changes> {
        App::changes(self)
    }
    fn chat(&self) -> Option<&Chat> {
        App::chat(self)
    }
    fn completion(&self) -> Option<&Completion> {
        App::completion(self)
    }
    fn config(&self) -> &obelus_config::Config {
        App::config(self)
    }
    fn counts(&self) -> Option<&Counts> {
        App::counts(self)
    }
    fn current_buffer(&self) -> Option<&Buffer> {
        App::current_buffer(self)
    }
    fn drawn(&self) -> &[obelus_ui::Drawn] {
        App::drawn(self)
    }
    fn highlights(&self) -> &Highlights {
        App::highlights(self)
    }
    fn hover(&self) -> Option<&Hover> {
        App::hover(self)
    }
    fn images(&self) -> &Images {
        App::images(self)
    }
    fn keymap(&self) -> &Keymap {
        App::keymap(self)
    }
    fn layers(&self) -> layers::Layers {
        App::layers(self)
    }
    fn listed_agents(&self) -> Vec<Listed> {
        App::listed_agents(self)
    }
    fn marked_runs(&self) -> &[obelus_text::coordinates::Span] {
        App::marked_runs(self)
    }
    fn note(&self) -> Option<&str> {
        App::note(self)
    }
    fn talked_about(&self) -> Vec<obelus_component::todo::Talked> {
        App::talked_about(self)
    }
    fn notes(&self) -> Option<&TodoView> {
        App::notes(self)
    }
    fn opened_hunks(&self) -> Vec<LineNumber> {
        App::opened_hunks(self)
    }
    fn phase(&self) -> u32 {
        App::phase(self)
    }
    fn picker(&self) -> Option<&Picker> {
        App::picker(self)
    }
    fn pinned(&self) -> &[&'static str] {
        App::pinned(self)
    }
    fn preview(&self) -> Option<Previewed<'_>> {
        App::preview(self)
    }
    fn prompt(&self) -> Option<&Prompt> {
        App::prompt(self)
    }
    fn readers_named(&self) -> &[&'static str] {
        App::readers_named(self)
    }
    fn reading_nothing(&self) -> bool {
        App::reading_nothing(self)
    }
    fn registry_failure(&self) -> Option<&str> {
        App::registry_failure(self)
    }
    fn rendered_rows(&self) -> Option<usize> {
        App::rendered_rows(self)
    }
    fn rendering(&self) -> Option<&[obelus_row::Row]> {
        App::rendering(self)
    }
    fn server_state(&self) -> Option<(&'static str, obelus_lsp::ServerState)> {
        App::server_state(self)
    }
    fn server_working_on(&self) -> Option<&str> {
        App::server_working_on(self)
    }
    fn settings(&self) -> Option<&Settings> {
        App::settings(self)
    }
    fn signature(&self) -> Option<&obelus_lsp::signature::Signature> {
        App::signature(self)
    }
    fn slash(&self) -> Option<&Picker> {
        App::slash(self)
    }
    fn talking(&self) -> Talking {
        App::talking(self)
    }
    fn text_area(&self) -> TextArea {
        App::text_area(self)
    }
    fn theme(&self) -> &Theme {
        App::theme(self)
    }
    fn project_config(&self) -> Option<&Path> {
        App::project_config(self)
    }
    fn troubles(&self) -> &[obelus_lsp::trouble::Trouble] {
        App::troubles(self)
    }
    fn what_this_conversation_is_about(&self) -> Option<String> {
        App::what_this_conversation_is_about(self)
    }
    fn working_directory(&self) -> &Path {
        App::working_directory(self)
    }
}
