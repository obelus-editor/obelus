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
//! itself, the keys, the frame, and the loop.
pub mod agents;
mod asking;
mod changing;
mod choosing;
mod completing;
mod conversations;
mod counting;
pub mod dispatch;
pub mod document;
mod documents;
mod fixing;
mod hearing;
mod hierarchy;
mod history;
mod history_view;
mod hovering;
mod noting;
mod opening;
pub use history_view::About;
mod keys;
mod mirroring;
mod moving;
mod naming;
mod preferences;
mod previewing;
mod projects;
mod releases;
mod remote;
mod renaming;
mod renaming_files;
mod reopening;
mod saying;
mod searching;
mod semantics;
mod switching;
pub mod talking;
mod worktrees;

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

/// Everything Obelus is currently showing or remembering.
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
    /// Where the pointer was last put down, and how many times in a row.
    ///
    /// A terminal reports button presses and nothing about double clicks,
    /// so the count is Obelus's own: the same cell, pressed again inside
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
    signature: Option<obelus_component::signature::Signature>,
    /// The caret has moved under the panel, and this is the wait for it to
    /// stop -- with the column it is waiting on, so that moving again
    /// starts it again rather than letting a stale one fire.
    signature_pause: Option<(crate::event::Pause, CharColumn)>,
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
    /// A hover on a rest is the one thing in Obelus that happens because a
    /// reader did *nothing*, so the doing-nothing has to be measured: the
    /// same cell, still under the pointer when the next tick lands. The
    /// asking is remembered because a pointer left on a word that has no
    /// answer must ask about it once rather than twelve times a second.
    resting: Option<Resting>,
    /// The cell the pointer was last reported over, wherever it was and
    /// whatever it did there.
    ///
    /// Not `resting`, which is the file's alone and is about how long it
    /// has been still: this is for what is raised under the pointer, which
    /// has to follow it everywhere and at once. Never forgotten, because a
    /// terminal says nothing when the pointer leaves it.
    pointer: Option<(u16, u16)>,
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
    /// A drag being held against the edge of what it is selecting in.
    ///
    /// The one thing on this screen that moves because of the reader's
    /// hand rather than because something is happening on its own -- and
    /// it has to, because a terminal says nothing at all while a held
    /// pointer is still. Without a tick behind it a reader who dragged to
    /// the edge and waited would wait for ever: the selection they are
    /// making stops where the screen does.
    dragging: Option<Dragging>,
    /// The bars the last frame left on the page, which is where a press on
    /// one lands: see `obelus_ui::bars`.
    bars: Vec<obelus_ui::bars::Drawn>,
    /// Which bar the pointer has hold of, and where on its mark.
    ///
    /// Whose rather than the bar itself: the frames go on being drawn while
    /// it is held, and what the next move is measured against is the bar
    /// as the latest of them drew it.
    holding: Option<(obelus_ui::bars::Whose, u16)>,
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
    /// What git says about the files in the tree, while a list of them is
    /// open.
    ///
    /// Gathered when a list opens and kept until the next one, because it is
    /// a walk of the whole tree and the rows arrive in batches afterwards.
    statuses: std::collections::HashMap<PathBuf, obelus_git::Standing>,
    /// Where an agent reaches what Obelus offers it, if it could listen.
    ///
    /// Taken once per project and kept: every conversation is told an
    /// address under this one, so a second agent started later reaches the
    /// same tools rather than a second server nobody asked for. Taken again
    /// only when the project is, because the tools are about one tree.
    tools_url: Option<String>,
    /// The server at that address, which stops listening when this goes.
    listening: Option<obelus_mcp::Listening>,
    /// The agent Obelus is talking to, once something has needed it.
    talker: Option<obelus_agent::acp::Talk>,
    /// Whether `ctrl+enter` arrives as itself rather than as enter.
    ///
    /// A window's keys always do; a terminal's only where it speaks the
    /// kitty keyboard protocol, which `main` asks before the alternate
    /// screen. What it decides is whether the box offers to send now: an
    /// offer of a key that arrives as enter is an offer that queues.
    ctrl_enter_arrives: bool,
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
    /// What will come back for the notes, to write down what was typed.
    ///
    /// Structural changes -- a note added, finished, moved -- are written
    /// the moment they happen and never wait: they are one act each, and
    /// there is nothing to wait for. Typing is not one act, and it used to
    /// be written when the reader left the page. There is no leaving a
    /// document, so a pause is the moment instead.
    notes_pause: Option<crate::event::Pause>,
    /// What will come back for a tree that is behind its text.
    ///
    /// One for all the open documents, because catching up asks every one
    /// of them: which are behind is a question with an answer, and a clock
    /// each would be a clock per file for a question asked once.
    syntax_pause: Option<crate::event::Pause>,
    /// What will come back for a document the reader has stopped changing.
    changes_pause: Option<crate::event::Pause>,
    /// What will come back for a pointer that has stopped moving.
    hover_pause: Option<crate::event::Pause>,
    /// A rename of a file, from the question to the act.
    ///
    /// The gap between the two is a round trip: a server that knows the
    /// language knows which other files name this one by where it is, and
    /// Obelus asks before renaming it rather than leaving the reader to
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
    /// The project's table of conversations, as Obelus last read it.
    ///
    /// `None` until there has been a reason to read it. What the reasons
    /// are is `App::sessions`; what they are *for* is that the notes page
    /// asks which of them has a conversation on every frame it draws, and
    /// parsing that table there cost more than everything else the page
    /// does put together.
    sessions_kept: Option<obelus_agent::acp::sessions::Remembered>,
    /// The project's notes, as Obelus last read them.
    ///
    /// Not the page's copy, which is the reader's and is ahead of the file
    /// while they are typing in it. This one is the file, for the one
    /// thing outside that page which has to know what a note says: the
    /// box of the conversation about it, which offers to ask about the
    /// note again once it has been rewritten.
    notes_kept: Option<obelus_git::todo::Todo>,
    /// Which conversations somebody has open, as Obelus last looked.
    ///
    /// Asked when there is a reason and kept until there is another, like
    /// everything else here. What makes that honest for a *lock* -- which
    /// nothing writes and nothing removes when the process holding it dies
    /// -- is that the kernel closes a dead process's files and a watcher
    /// reports that close. See `obelus_watch` for the one Access event it
    /// lets through, and `obelus_agent::chats` for why Obelus's own looking
    /// is a read.
    ///
    /// And which checkout holds each, where its claim says.
    held_kept: std::collections::BTreeMap<obelus_agent::chats::ChatId, Option<PathBuf>>,
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
    /// The reader's, kept for as long as Obelus is running and not written
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
    /// What is wrong with the line the reader is on, where anything is.
    ///
    /// Worked out every frame from the troubles and the caret rather than
    /// remembered -- the same rule the row that says what is happening
    /// follows, so there is no way for a complaint to be left on a line
    /// that no longer has one.
    complaining: Option<Complaint>,
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
    /// The question "which project", while nobody has answered it.
    ///
    /// `Some` on a start with nothing to go on: no argument, and a
    /// directory git has never heard of -- a desktop launcher, which
    /// begins the process in the home directory. And once more after the
    /// project has gone and the reader has said so, which leaves the window
    /// where such a start began. It is the whole screen until it is
    /// answered, and `None` otherwise: a reader on a project does not go
    /// back to being asked, and the way to another one is a second Obelus,
    /// which is how Obelus is used anyway.
    chooser: Option<obelus_component::chooser::Chooser>,
    /// What could finish the path being named, while one is.
    ///
    /// Here rather than inside the chooser for the reason the agent's own
    /// commands are here: a list is built out of what the application
    /// knows -- a directory it read -- and `obelus-component` draws and
    /// walks lists rather than looking at disks.
    naming_list: Option<Picker>,
    /// What the last directory read found, and which directory that was.
    ///
    /// Kept apart from the list above, because the list *goes* for two
    /// ordinary reasons -- the reader shut it, or what they have typed
    /// since matches none of it -- and both of those have to be
    /// undoable by typing another letter. Held together they were not:
    /// once the list was gone there was nothing left to make it from, so
    /// escape shut it for good and a letter too many could not be rubbed
    /// out. The disk is read when the *directory* moves and never again.
    naming_read: Option<(PathBuf, Vec<obelus_component::picker::PickerItem>)>,
    /// Whether the reader shut the list on what is in the box now.
    ///
    /// Cleared the moment the box moves, which is the rule
    /// `component::completion` follows: escape takes the panel away, and
    /// typing is a new question rather than the same one asked twice.
    naming_shut: bool,
    /// Whether what is in the path box names something that is there.
    ///
    /// Kept rather than asked for: the row is drawn on every frame and
    /// the answer moves only when the box does, so it is worked out on
    /// the key that moved it. What it is for is the ink -- a reader has
    /// to see that enter will refuse before they press it, which is the
    /// rule the palette follows for a command it will not run.
    named_is_there: bool,
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
            clicked: None,
            signature: None,
            signature_pause: None,
            hover: None,
            code_actions: Vec::new(),
            uses: Vec::new(),
            resting: None,
            pointer: None,
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
            bars: Vec::new(),
            holding: None,
            fonts_here: Vec::new(),
            monospace_here: None,
            names: None,
            replacing: false,
            drawing: None,
            prompt: None,
            changes: None,
            statuses: std::collections::HashMap::new(),

            tools_url: None,
            listening: None,
            talker: None,
            ctrl_enter_arrives: true,
            runs: obelus_agent::running::Runs::default(),
            waiting_on: Vec::new(),
            settled: preferences::Settled::default(),
            agents: agents::Agents::default(),
            picture_layout: None,
            settings: None,
            counts: None,
            screen_area: Rect::ZERO,
            given_statuses: None,
            listing: Vec::new(),
            found: Vec::new(),
            stood_on: None,
            notes_pause: None,
            syntax_pause: None,
            changes_pause: None,
            hover_pause: None,
            renaming: None,
            opened: std::collections::HashSet::new(),
            history: history_view::Showing::default(),
            calls: None,
            searching: Vec::new(),
            troubling: Vec::new(),
            conversing: conversations::Conversing::default(),
            watching: [const { conversations::Watched::new() }; conversations::WATCHED],
            sessions_kept: None,
            notes_kept: None,
            held_kept: std::collections::BTreeMap::new(),
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
            taken_from: None,
            complaining: None,
            note: None,
            walk_generation: obelus_runtime::cancel::Latest::default(),
            events: None,
            amiss: Vec::new(),
            releases: releases::Releases::default(),
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
            chooser: None,
            naming_list: None,
            naming_read: None,
            naming_shut: false,
            named_is_there: false,
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
        self.should_quit = true;
    }

    /// The list of names being built, while one is open.
    #[must_use]
    pub fn names(&self) -> Option<&obelus_component::names::Names> {
        self.names.as_ref().map(|(_, names)| names)
    }

    /// Whether what is typed goes over what is under the cursor.
    #[must_use]
    pub const fn replacing(&self) -> bool {
        self.replacing
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
        match self.replacing && self.layers().nearest().is_none() {
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

    /// Puts the application on a project.
    ///
    /// Before the settings are read, always: a project has settings of its
    /// own and a theme beside them, and finding those means knowing which
    /// project first. [`App::load_config`] lays the project's answers over the
    /// reader's at the end, so this only has to have happened by then.
    pub fn work_in(&mut self, root: PathBuf) {
        self.working_directory = root;
        // Now, rather than on the first frame: the row draws the branch
        // and a watch says what happens next rather than what already
        // has.
        self.head = obelus_git::head_of_the_tree(&self.working_directory);
        // The one door every way of settling on a project goes through --
        // an argument, the directory Obelus was started in, a row on the
        // page that asks which -- which is why the remembering is here
        // and at none of the three. `remember` declines anything that is
        // not a worktree, so a process that began in the home directory
        // writes nothing.
        projects::remember(&self.working_directory, jiff::Timestamp::now().as_second());
        let root = self.working_directory.clone();
        self.keep_what_is_open_for(&root);
    }

    /// Which branch the tree Obelus was put on has checked out.
    #[must_use]
    pub fn head(&self) -> Option<&obelus_git::Head> {
        self.head.as_ref()
    }

    /// Whether there is a project to do anything in.
    ///
    /// Not while Obelus is still asking which one, and not once
    /// the one it was has gone. Both are known without doing any work,
    /// which is what a requirement has to be.
    #[must_use]
    pub fn has_a_project(&self) -> bool {
        self.chooser.is_none() && !self.gone
    }

    /// Whether the tree Obelus was put on has gone from disk.
    #[must_use]
    pub const fn tree_has_gone(&self) -> bool {
        self.gone
    }

    /// The tree has gone from under this window.
    ///
    /// Heard from the watcher rather than looked for: the tree going is
    /// its contents going, which are changes like any other, and every
    /// change is asked first whether the tree is still there. Nothing is
    /// polled, which leaves the platforms
    /// whose watcher does not report a watched directory going with the
    /// half that does not depend on it -- what keeps the project's things
    /// from being written into a project that has gone is asked of the disk
    /// at the moment of writing (`obelus_git::project`).
    ///
    /// Said over the whole screen, on top of whatever the reader was in
    /// (`ui::gone`), and answered with one of two keys: enter asks which
    /// project next, and the key that leaves leaves. Everything else that
    /// was over the page goes first, because the page covers the screen
    /// and a page covers what shares its room.
    pub(super) fn the_tree_has_gone(&mut self) {
        tracing::warn!(tree = %self.working_directory.display(), "the tree Obelus is on has gone");
        self.make_room(layers::Room::Screen);
        self.gone = true;
        self.head = None;
        self.worktrees.tree_has_gone();
    }

    /// The page saying the tree has gone, answered -- or not, and then
    /// nothing else hears the key either: what is under the page is about
    /// a project that is not there.
    pub(super) fn the_page_saying_it_has_gone(&mut self, key: &KeyEvent) -> bool {
        if key.code == KeyCode::Enter && key.modifiers == KeyModifiers::NONE {
            self.let_go_of_the_project();
            self.ask_which_project();
        } else if self.keymap.lookup(key, Context::Dialog) == Some(Command::Quit) {
            self.request_quit();
        }
        true
    }

    /// Lets go of everything the project that went was.
    ///
    /// What was open is closed without asking, unsaved work and all -- a
    /// file in a tree that has gone has nowhere to be written, and Obelus
    /// does not make the tree again to write it.
    ///
    /// **A window that starts again, without starting again.** What is
    /// kept is what belongs to the process and not to the project -- the
    /// loop's channel, the reader's settings, what the front end can do --
    /// and everything else is a new [`App`]'s. Kept by name rather than
    /// cleared by name, so that a field nobody thought of here is one that
    /// starts empty, and not one still holding the last project's answer.
    fn let_go_of_the_project(&mut self) {
        // The sessions nothing was said in, as on the way out: an agent
        // keeps what it is not told to let go of.
        self.let_go_of_what_nothing_was_said_in(None);
        // Moved on rather than made again, because a walk still running
        // holds the old count: a new one would start where the old one's
        // answers are numbered, and they would arrive as current.
        self.walk_generation.next();
        self.history_generation.next();
        self.search_generation.next();
        self.worktrees.not_showing();

        let was = std::mem::replace(self, Self::new(Vec::new()));
        self.events = was.events;
        self.drawing = was.drawing;
        self.fonts_here = was.fonts_here;
        self.monospace_here = was.monospace_here;
        self.screen_area = was.screen_area;
        self.editor_area = was.editor_area;
        self.settled = was.settled;
        self.keymap = was.keymap;
        self.theme = was.theme;
        self.theme_name = was.theme_name;
        self.walk_generation = was.walk_generation;
        self.history_generation = was.history_generation;
        self.search_generation = was.search_generation;
        // The door other windows reach this one by is the process's, and
        // listens for as long as it runs.
        self.worktrees = was.worktrees;
        self.agents = was.agents;
        self.releases = was.releases;
        self.looking = was.looking;
        self.outside = was.outside;
        // What Obelus could not make of its own files, which are not the
        // project's: the reader's settings, which nothing reads again here.
        // Obelus's own marks and none of a server's -- the server that said
        // those has gone with the project, and nothing would ever take what
        // it said away.
        let root = was.working_directory;
        self.troubles = was
            .troubles
            .into_iter()
            .filter(|(path, _)| !path.starts_with(&root))
            .filter_map(|(path, troubles)| {
                let ours: Vec<_> = troubles
                    .into_iter()
                    .filter(|trouble| trouble.source.as_deref() == Some(semantics::OBELUS))
                    .collect();
                (!ours.is_empty()).then_some((path, ours))
            })
            .collect();
        self.working_directory = root;
        // A watcher of its own as well, on what is left -- the settings and
        // the theme. Started again rather than kept and given things back
        // one at a time: a watch is a count on the watcher it was taken on,
        // and what this one held for the project and its files is a list
        // nothing here has.
        if was.watcher.is_some()
            && let Some(events) = self.events.clone()
        {
            self.start_watching(events);
        }
        // And the rest of `was` goes at the end of this: the servers, the
        // agent, what it was running and the tools it was offered, all of
        // which stop as they are dropped.
        //
        // The reader's settings without the project's over them, which
        // are in a file that is not there.
        self.apply_project();
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
    /// about what Obelus does on the way up has nothing else to call.
    pub fn start(&mut self, sender: std::sync::mpsc::Sender<Event>) {
        self.events = Some(sender.clone());
        // Before anything else is started: the reader is looking at an empty
        // screen until it arrives.
        self.send_the_reopening();
        self.start_watching(sender);
        // Both of these are about the project, and on a start with
        // nothing to go on there is not one yet: they would be rooted at
        // the directory the process happened to begin in, which from a
        // desktop launcher is the home directory, and nothing would move
        // them when the reader answered. `settle_on` does them then.
        if self.chooser.is_none() {
            self.offer_the_tools();
            self.watch_the_project();
            self.say_where_this_window_is();
        }
        for index in 0..self.documents.len() {
            self.serve(index);
        }
        // Not about the project: whether a newer Obelus is out is the
        // same question wherever this one was started, so it is asked
        // whether or not the reader has said where they work.
        self.ask_about_releases();
        // What went wrong on the way up, over whatever the first screen is,
        // and last of all so that everything that could go wrong has.
        let told = self.tell_what_went_wrong();
        // Last, and here rather than at the command line: the rows come
        // from a walk that sends on this channel, so a list opened before
        // there was one would be a list nothing ever fills. Not over what
        // went wrong, though: a list opened over a list would put the one
        // the reader is owed under the one they asked for, and the files
        // are one key away once it has been read.
        if self.list_at_start && !told {
            self.open_file_picker();
        }
    }

    /// Offers an agent Obelus's own tools, rooted at this project.
    ///
    /// Started with the loop rather than with the first agent, because
    /// the address is what an agent is told and telling two of them two
    /// addresses would be two servers -- but not before there is a
    /// project, because the root is the whole of what the tools are
    /// about.
    pub(super) fn offer_the_tools(&mut self) {
        let Some(sender) = self.events.clone() else {
            return;
        };
        match obelus_mcp::serve(&self.working_directory, std::sync::Arc::new(sender)) {
            // Said, because the silent half of this is the half nobody can
            // ask about: whether an agent was offered anything, and whether
            // it took it, were both questions Obelus had no answer to.
            Ok((url, listening)) => {
                tracing::info!(url, "Obelus is offering an agent its tools");
                self.tools_url = Some(url);
                self.listening = Some(listening);
            }
            Err(error) => {
                // Not a reason to stop: an Obelus that cannot listen is an
                // Obelus an agent cannot ask anything of, which is what it
                // was until now.
                tracing::warn!(%error, "Obelus is offering an agent nothing");
                self.amiss
                    .push("An agent asking Obelus for its tools reaches nothing".to_string());
            }
        }
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
        self.history_generation.now()
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
        self.tools_url.as_deref()
    }

    /// Says where Obelus's own tools are, without listening anywhere.
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
        // is what putting Obelus on a project means: at startup the two happen
        // together, and a test that moved one without the other would be
        // testing an application no reader can have.
        self.apply_project();
    }

    /// Takes the watch on the project's own settings, where there is now
    /// somewhere to take it.
    ///
    /// Asked twice: once while the watches are being set up, and again if
    /// the directory turns up later. Idempotent, because the watcher counts
    /// watches by directory and a second ask for one it already holds is
    /// the count going up -- which is also what makes the project's root
    /// and a file of the reader's that happens to live in it one watch
    /// rather than two.
    fn watch_the_projects_settings(&mut self) {
        let project = obelus_config::project_path_for(&self.working_directory);
        let Some(watcher) = self.watcher.as_mut() else {
            return;
        };
        if let Err(error) = watcher.watch(&project) {
            tracing::debug!(%error, path = %project.display(), "still nothing to watch");
        }
    }

    /// Takes the watches that are about the project, and nothing else.
    ///
    /// Its own piece because the project is not always known when Obelus
    /// starts: a start with nothing to go on asks which one, and these
    /// would otherwise all be taken against the directory the process
    /// happened to begin in -- the home directory, from a desktop
    /// launcher. Taken when the reader answers instead, which is
    /// `App::settle_on`.
    ///
    /// The one that cost most by being wrong is git's: with `HEAD` and
    /// `index` unwatched, `forget_what_git_said` never fires, so the
    /// branch on the status row and the marks in the margin are whatever
    /// they were when the project opened for the rest of the session.
    pub(super) fn watch_the_project(&mut self) {
        let root = self.working_directory.clone();
        let project = obelus_config::project_path_for(&root);
        let Some(watcher) = self.watcher.as_mut() else {
            return;
        };
        // What git keeps its state in, because Obelus is not the only
        // thing in the repository: a commit in another window, or in a
        // shell, changes what has changed in every file on screen. The
        // margin would otherwise go on showing a diff against a commit
        // that is no longer the one the file is against.
        for path in obelus_git::state_of(&root) {
            if let Err(error) = watcher.watch(&path) {
                tracing::warn!(%error, path = %path.display(), "not watching the repository");
            }
        }
        // And the project's own settings, for the same reason twice over:
        // another Obelus on this project may be looking at them, and a `git
        // pull` rewrites them under everybody.
        // The file the project *would* have, not the one it has: watching only
        // what was there at startup is the "read once" mistake with a longer
        // fuse, because it looks right until somebody creates the file --
        // the window next door writing the project's first setting, or a
        // pull bringing one.
        // The project itself, for the directory its settings live in
        // coming or going. Always, not only where it is missing now: a
        // reader who deletes `.obelus` and makes it again is the same
        // question as one who never had it, and a watch taken only in the
        // second case left the first unheard for the rest of the session.
        //
        // Not recursive -- what is wanted is one directory appearing
        // directly in the project, and a recursive watch on a repository is
        // `target` and `.git` reported a thousand times over. Where the
        // reader has a file of the project's root open, this is that same
        // watch counted twice rather than a second one.
        if let Err(error) = watcher.watch_directory(&root) {
            tracing::warn!(
                %error,
                path = %root.display(),
                "not watching the project for settings appearing"
            );
        }
        if let Err(error) = watcher.watch(&project) {
            // A project with no settings of its own has no directory to
            // watch, and that is the ordinary case: a quarter of the starts
            // in this machine's own log said so, every one of them about a
            // project that was working perfectly. A warning on every start
            // is how a log stops being read -- the same argument the list of
            // what went wrong on the way up is built on, and the level
            // `settle_a_watch` already uses for the same failure.
            //
            // A directory that *is* there and will not be watched is a real
            // failure and keeps its warning: settings changed in another
            // window will not arrive, and that is worth a word.
            match project.parent().is_some_and(std::path::Path::is_dir) {
                true => tracing::warn!(
                    %error,
                    path = %project.display(),
                    "not watching the project's settings"
                ),
                // The directory is not there either, which is the ordinary
                // case and not a failure worth a word. But the intent above
                // -- hearing the file *appear* -- is the whole reason this
                // watch is on the file the project would have, and it
                // cannot be served by watching a directory that is not
                // there. So the project itself is watched instead, which is
                // where `.obelus` will turn up.
                //
                // Not recursive: what is wanted is one directory appearing
                // directly in the project, and a recursive watch on a
                // repository is `target` and `.git` reported a thousand
                // times over. It is given up by nothing, because the
                // directory can go again as easily as it came -- and where
                // the reader has a file of the project's root open, this is
                // the same watch counted twice rather than a second one.
                false => tracing::debug!(
                    %error,
                    path = %project.display(),
                    "no settings of the project's own yet"
                ),
            }
        }
    }

    /// Starts watching every open file for changes on disk.
    fn start_watching(&mut self, sender: std::sync::mpsc::Sender<Event>) {
        let mut watcher = match Watcher::new(sender) {
            Ok(watcher) => watcher,
            Err(error) => {
                tracing::warn!(%error, "auto-reload is off");
                // The one watcher failure worth saying: without it nothing
                // Obelus reads is read again for the rest of the session,
                // so a file changed in another window, a commit, and the
                // settings all go unheard.
                self.amiss.push(
                    "Nothing is being watched, so changes made elsewhere will not arrive"
                        .to_string(),
                );
                return;
            }
        };
        for buffer in self.documents.iter().flatten().filter_map(Document::file) {
            if let Err(error) = watcher.watch(buffer.path()) {
                tracing::warn!(%error, path = %buffer.path().display(), "not watching");
            }
        }
        // And the settings, because Obelus is not the only Obelus. Several
        // of them on one project is the ordinary way to work -- the
        // terminal splits the window, Obelus does not -- so a setting
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
        self.watcher = Some(watcher);
        // And wherever the colours come from, which is its own question:
        // a theme is a file Obelus never writes and something else may
        // replace under it.
        self.watch_theme();
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

    /// The area a list is drawn in.
    ///
    /// The whole of the region, a question the agent is waiting on
    /// included: the drawing's own answer, which this has to be the same as
    /// or the rows a key moves through are not the rows on screen.
    ///
    /// An area and not a [`layers::Room`]: a room is how much of the screen
    /// a view declares it takes, and this is the rectangle that comes out of
    /// laying one out.
    fn picker_area(&self) -> Rect {
        self.drawn_in()
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
        // Nothing open and no page taking its place: the welcome screen's
        // sheen. A settings page over the welcome is not a welcome screen,
        // so keeping its clock running would redraw a motionless page.
        //
        // And not where a window is drawing, because there the sheen is
        // the window's own and runs on the window's own clock. What this
        // ticker moves is the ramp written into the cells, which is the
        // sheen a terminal can draw and the one thing a window does not
        // read -- so it would be twelve pages a second pushed at a front
        // end that draws the light itself, on the one screen a reader
        // leaves up while they decide what to open. The cells keep the
        // ramp they were last drawn with, which is that sheen at one
        // moment and as true as any other frame of it.
        //
        // Nor while Obelus is asking which project: that screen is not the
        // welcome screen and has no mark to run a sheen across.
        (self.current.is_none()
            && self.chooser.is_none()
            && !self.layers().filling()
            && !obelus_config::in_a_window())
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
            // And the badge on the status row, for as long as this file's
            // server is reading the project. The same rule again, and the
            // reason the badge turns at all: an empty answer while it
            // reads and an empty answer about a symbol with no definition
            // are the same message, and a mark standing still says the
            // second.
            || self.server_busy()
            // And the chat's mark, while the window it talks to connects.
            || self.remote_turning()
            // And a drag held against an edge, which is the one of these
            // that is waiting on the reader's hand rather than on
            // something happening by itself. It is here for the same
            // reason as the rest: without a tick it stops, and a
            // selection that stops at the edge of the screen is a
            // selection of what fits on it.
            || self.dragging.is_some()
            // And a list being matched somewhere else. The same rule once
            // more: the row that says so turns, and a mark drawn once and
            // never again is a mark saying nothing is happening -- which
            // is the one thing this row exists to contradict.
            || self
                .picker
                .as_ref()
                .is_some_and(obelus_component::picker::Picker::is_matching)
            // And an install, while the page of agents is open: a card
            // whose package manager says nothing until it is done has only
            // its mark to say the install is still going.
            || (self.settings().is_some_and(Settings::on_agents)
                && !self.agents.installing.is_empty())
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
            .any(|talk| talker.is_thinking(talk.session.as_ref(), talk.requested))
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

    /// How long a tree may be behind its text before it is caught up.
    ///
    /// What the animation's tick used to give this by accident, kept at the
    /// same length so that the colours arrive when they always have: long
    /// enough that a burst of keys is one parse, short enough that the
    /// reader is still looking at what they typed.
    const CATCHES_UP_AFTER: std::time::Duration = std::time::Duration::from_millis(80);

    /// Comes back for the trees that owe an answer, unless something
    /// already is.
    ///
    /// Set by the first frame that notices and not put back by the ones
    /// after it -- the same rule the watcher's debouncing follows, and for
    /// the same reason: a deadline that slid would never arrive while the
    /// reader kept typing, which is exactly when a tree is behind.
    ///
    /// Asked from what is true rather than started where a document
    /// changes, which is how the ticker was asked and is what stops a clock
    /// outliving its reason.
    fn catch_up_soon(&mut self) {
        if !self.anything_behind() {
            self.syntax_pause = None;
            return;
        }
        if self.syntax_pause.is_some() {
            return;
        }
        self.syntax_pause = self.come_back_in(Self::CATCHES_UP_AFTER, Event::SyntaxSettled);
    }

    /// Works out what every document that owes it means now.
    ///
    /// Everything open rather than what is on screen: a tree left behind on
    /// a document nobody is looking at would keep the ticker awake for the
    /// rest of the session.
    pub(crate) fn settle_syntax(&mut self) {
        // Nothing else has to be told: the text did not move, only what
        // Obelus knows about it, so everything keyed on the version stays
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

    /// A clock to come back with, where there is a loop to come back to.
    ///
    /// The one place that knows a timer needs the loop's channel. `None`
    /// without one, which is a test driving its own events: whether
    /// something is waiting is decided either way, and what a test drives
    /// by hand is the event the clock would have sent.
    ///
    /// Every wait Obelus keeps goes through here -- the notes, a tree
    /// behind its text, a rename's server, a document's standing questions
    /// and the pointer's rest -- so that the answer to "is there anything
    /// to come back from" is written once.
    fn come_back_in(
        &self,
        after: std::time::Duration,
        event: Event,
    ) -> Option<crate::event::Pause> {
        self.events
            .clone()
            .map(|events| crate::event::Pause::start(events, after, event))
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
    /// takes the keys bound in it and no others, so Obelus's own commands
    /// cannot open a second dialog over the first -- `f1` in a
    /// conversation used to put a file list on top of it, which then took
    /// two escapes to leave and gave no way to tell which of the two a key
    /// would reach.
    pub(crate) fn context(&self) -> Context {
        // Being asked which project is a dialog like any other, and takes
        // what `Context::Dialog` binds: leaving, and the four keys that
        // act on what the reader has hold of -- a box they can select in
        // and not paste into is half a box. Everything else in Obelus is
        // about a project, and `Requires::AProject` is what refuses it.
        if self.chooser.is_some() {
            return Context::Dialog;
        }
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
            layers::Layer::Names => self.names.is_some(),
            layers::Layer::Picker => self.picker.is_some(),
            layers::Layer::Prompt => self.prompt.is_some(),
            layers::Layer::Gone => self.gone,
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
    /// leaves them.
    pub(crate) fn leave(&mut self, layer: Layer) {
        match layer {
            // Not left: answered. Escape gives up on the nearest thing,
            // and here there is nothing nearer to give up on and nothing
            // behind it to give up to -- what is behind it is the project
            // that went.
            Layer::Gone => {}
            Layer::Picker => {
                self.picker = None;
                // Back to where they were looking from. The other way out
                // of a list is choosing a row, and that goes somewhere on
                // purpose -- see `App::accept`.
                self.look_back();
                self.history = history_view::Showing::default();
                self.troubling.clear();
                self.conversing = conversations::Conversing::default();
                self.close_calls();
                // What a server offered to do here, which the rows were
                // indexes into. A row is chosen by its position, so offers
                // outliving their list are offers pointing at nothing.
                self.code_actions.clear();
                // Nothing about the agent's question: that is a card in the
                // conversation, not a list, and a list opened over it and
                // closed again -- or one that took the reader to another
                // conversation -- was never the question. Refusing it here
                // answered "no" for a reader who had only looked at F2.
                // A theme previewed but not chosen, and the file a
                // question about leaving took the reader to. The two
                // things a picker changes about the application while it
                // is open, and so the two that have to be put back.
                self.go_back_from_asking();
                if let Some((name, before)) = self.theme_before.take() {
                    self.set_theme(&name, before);
                }
            }
            Layer::Names => self.names = None,
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
    /// which is why Obelus's own keys work inside one.
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
        self.note = Some(saying::Note::said(said));
    }

    /// Says what would not go.
    ///
    /// Two doors rather than a flag set beside the words: a flag is a
    /// second thing to remember at a hundred and thirty call sites, and
    /// the one that was forgotten would be a refusal drawn as a report.
    /// Which of these a note went through is the whole of how it says
    /// which it is.
    pub(crate) fn wrong(&mut self, said: impl Into<String>) {
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

    /// How far the file's view has travelled altogether, in screen rows.
    #[must_use]
    pub const fn travelled(&self) -> i64 {
        self.travelled
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
        self.note_where_the_view_has_got_to();
        // What the views showing are drawn from, and what Obelus has to be
        // told about it. First, because everything below this reads one of
        // those kept answers -- the notes' marks are worked out a dozen
        // lines down -- and a watch taken at the end of the frame is a view
        // that draws its first frame from whatever was there last time.
        self.settle_the_watches();
        // And what the settings page says about the chat, which can move
        // under it the way the settings can.
        self.settle_the_remote_page();
        // And the connection to that chat, from the same answer: which one
        // is set.
        self.settle_the_connection();
        // And a thread for every conversation there that can be named.
        self.settle_the_threads();
        // And the sessions, from the same question: which conversation is
        // on screen.
        self.settle_the_sessions();
        // And what is open, written down where it has changed, for the
        // same reason: there are a dozen ways a document opens or closes.
        self.write_down_what_is_open();
        // The notes are laid out against the room they have: a terminal is
        // resized and a setting is changed while they are open, and the rows
        // they are made of depend on both.
        let laid = self.notes_laid_out();
        // And which of them another Obelus has the conversation of, which
        // decides what the keys will change and so which of them the foot
        // offers. Beside the room for the same reason: both are the page's
        // answer to something outside it that moves while it is open.
        let elsewhere = self.which_notes_are_elsewhere();
        if let Some(notes) = self.notes_mut() {
            notes.lay_out(laid.0, laid.1);
            notes.these_are_elsewhere(elsewhere);
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
        // And what a row of the list of conversations says about itself,
        // which is the same rule one level along: another Obelus opening
        // or closing one is not this reader's keystroke, and the row has
        // to say so before they press.
        self.freshen_the_conversation_rows();
        // What the conversation says is happening, read off the state
        // rather than remembered: a row that is worked out every frame
        // cannot be left saying something that stopped being true.
        let doing = match self.talking() {
            Talking::Starting => Some("Starting\u{2026}"),
            Talking::Thinking => Some("Thinking\u{2026}"),
            Talking::Nobody | Talking::Idle | Talking::Ready | Talking::Gone => None,
        };
        let can = self.talking() == Talking::Thinking && self.ctrl_enter_arrives;
        self.in_transcript(|chat| {
            chat.doing(doing);
            chat.can_send_now(can);
        });
        self.show_what_is_running();
        // Only the animation, which is what the ticker is for. Everything
        // else that once rode this question waits on a clock of its own:
        // work that is owed is owed on a machine with nothing moving on it.
        self.animate(self.wants_animating(doing.is_some()));
        // Which for a tree behind its text is asked from what is true
        // rather than started where a document changes -- the same way the
        // ticker was asked, and what stops a clock outliving its reason.
        self.catch_up_soon();

        // Which rows the list will draw is what decides which rows need
        // their matched characters worked out, and only the geometry knows
        // how many rows there are. The room is the room it is *drawn* in,
        // which over a conversation is everything above the box.
        let width = self.picker_area().width;
        let rows = self
            .picker
            .as_ref()
            .map(|picker| obelus_ui::picker::rows_drawn(picker, self.picker_area()));
        if let (Some(rows), Some(picker)) = (rows, self.picker.as_mut()) {
            picker.refresh_indices(rows, width);
        }
        // And a scoring the list wants done somewhere that is not here.
        // Taken on the frame rather than where the query changed, for the
        // reason the indices above are: one place asks, so a path that
        // changes a query cannot forget to.
        self.send_the_scan();
        // The same question for the list a setting's names are built in,
        // and the same reason: its window moves when the rows about to be
        // drawn say where it goes.
        let building = self
            .names
            .as_ref()
            .map(|(_, names)| obelus_ui::names::rows_drawn(names, self.picker_area()));
        if let (Some(rows), Some((_, names))) = (building, self.names.as_mut()) {
            names.settle_window(rows);
        }
        // The window over the projects, while Obelus is asking which: the
        // rule every window follows, on the page's own count of the rows it
        // has.
        if self.chooser.is_some() {
            let rows = self.chooser_rows();
            if let Some(chooser) = self.chooser.as_mut() {
                chooser.settle(rows);
            }
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
        // The same thing for the list of what could finish a path: the
        // window follows the selection only once it knows how many rows
        // are on screen, and the matched characters are worked out for
        // the rows about to be drawn. No disk is touched -- the rows are
        // whatever the last directory read found.
        self.settle_the_naming_list();

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
        // And what the call under the caret takes, which is the third of the
        // panels that belong to a place in the file and the last of them to
        // be asked this. It went only when a view opened over it, so a
        // reader who arrowed off the line kept a panel about a call that was
        // no longer under them.
        self.settle_signature();
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

        let painted = obelus_ui::editor_canvas(self.screen_area).height;
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
        // Over the rows that are *painted*, not the rows the reader has.
        // A compact list covers the foot of the document rather than
        // shortening it -- see `obelus_ui::editor_canvas` -- and what is
        // highlighted has to be what is drawn, or the rows under the list
        // come out in the plain foreground and a window shows them that
        // way through the glass.
        let range = buffer.visible_bytes(painted);
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
        self.bars = obelus_ui::draw(cells, area, self);
        // With the frame rather than with the key that changed it: what
        // the caret is doing depends on where it ended up, which is not
        // known until the frame has been laid out.
        if let Some(drawing) = self.drawing.as_ref() {
            drawing.caret_is(self.caret(), self.layers().nearest(), self.takes_text());
        }
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
            Event::Closed => self.request_quit(),
            Event::Summoned(token) => self.summoned(token),
            Event::Remote(event) => self.remote_event(event),
            Event::Reached(number, event) => self.reached_event(number, event),
            Event::Held(number, lock) => self.held_the_remote(number, lock),
            Event::NotLetGo(number) => self.not_let_go(number),
            Event::NotHeld(number) => self.not_held(number),
            Event::Fonts { here, otherwise } => {
                tracing::info!(
                    faces = here.len(),
                    ?otherwise,
                    "the window says what it can draw with"
                );
                self.fonts_here = here;
                self.monospace_here = otherwise;
                // A list already open takes them now: the reader opened it
                // before the window had finished asking, which is the
                // ordinary case on a machine with a thousand fonts.
                let (here, otherwise) = (self.fonts_here.clone(), self.monospace_here.clone());
                if let Some((_, names)) = self.names.as_mut() {
                    names.offered(here, otherwise);
                }
            }
            Event::Watched(obelus_watch::Changed { path }) => {
                // Whether the tree has gone, asked of every change and
                // before anything else is made of it. A tree going is a
                // change to everything in it, in whatever order the kernel
                // and the debouncing leave them -- and the tree's own going
                // is not reliably among them: measured on Linux, an
                // `rm -rf` arrived as the files and directories inside it
                // and nothing for the root. Taken one at a time, the
                // project's settings file going is the reader taking the
                // project's settings away, and git's `HEAD` going is a
                // branch moving. One `stat` per change, which is a change
                // somebody made.
                if self.has_a_project() && obelus_git::is_gone(&self.working_directory) {
                    self.the_tree_has_gone();
                }
                // And from here, nothing that changed is news about the
                // project when there is no project for it to be about.
                let ours = self.has_a_project();
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
                // Or the directory that file lives in, turning up in a
                // project that had none: the watch on the file could not be
                // taken while there was nowhere to take it, so this is
                // where it is taken. Read as well as watched, and in that
                // order -- whoever made the directory may have written the
                // file into it before the watch was attached, and a watch
                // says what happens next rather than what already has.
                let appeared = ours
                    && path.parent() == Some(self.working_directory.as_path())
                    && project.parent() == Some(path.as_path());
                if appeared {
                    self.watch_the_projects_settings();
                }
                // The directory appearing counts as the settings changing,
                // and not only because the file may be in it already: that
                // is the race this is about. Whoever made the directory
                // writes the file into it, and the two arrive together --
                // so by the time the watch is attached the file is there
                // and its own event has been and gone.
                let project = ours && (path == project || appeared);
                if readers || project {
                    self.reread_config();
                } else if self.is_a_theme(&path) {
                    // The colours the reader is already wearing, read again:
                    // the name in the settings has not moved, and what it
                    // stands for has.
                    self.reread_theme();
                } else if ours && self.is_a_window(&path) {
                    // Another window on the repository opened, closed, or
                    // moved to another tree -- which the list of worktrees
                    // draws while it is up.
                    self.reread_the_windows();
                } else if self.is_the_remote_wanted(&path) {
                    // Another window asking for the chat this one has.
                    self.somebody_wants_the_remote();
                } else if ours && self.is_a_claim(&path) {
                    // A conversation taken up or let go in another window
                    // -- including one let go by that window dying, which
                    // is a file closed by a writer and nothing else.
                    self.reread_who_holds_what();
                } else if ours && self.is_the_sessions_file(&path) {
                    // Which of the project's notes has a conversation,
                    // written by another Obelus -- or by this one, which
                    // hears its own writes like anybody else's and has
                    // already kept what it wrote. Reading it again costs
                    // one parse and keeps the two windows in step. And the
                    // list of conversations reads it too, for the name each
                    // goes by.
                    self.reread_the_sessions();
                    self.say_the_new_name();
                } else if ours && self.is_the_notes_file(&path) {
                    // What the project means to come back to, written by
                    // another Obelus, the reader's own editor -- or by this
                    // Obelus, which hears its own writes like anybody
                    // else's. Not told apart, because there is nothing to
                    // gain by it: a reread keeps the box the reader is
                    // typing in and puts the caret back by name, so reading
                    // back what Obelus itself just wrote changes nothing on
                    // the page.
                    self.reread_notes();
                    // And the copy the conversation's box is offered
                    // from, which is wanted whether or not that page is
                    // open: a reader talking about a note has usually
                    // walked away from the list of them.
                    self.reread_the_notes_kept();
                } else if ours && obelus_git::state_moved(&path) {
                    self.forget_what_git_said();
                } else {
                    self.reload_path(&path);
                }
                // And the servers, whatever it was: a file changing on
                // disk is news to them as much as to Obelus -- a branch
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
                // message, because finishing one is a moment Obelus has to
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
                // Everything the protocol needs rather than Obelus — the
                // handshake, progress, the server's own log lines — is dealt
                // with in there.
                let reply = client.on_message(&message);
                // Unasked-for news about a file, which arrives on the same
                // pipe as the answers and belongs to nobody's question.
                let published = client.take_published();
                // And what it says went wrong, which is the one thing it
                // says that belongs on the reader's row rather than in the
                // log: the rest of its talk is progress, and the badge
                // says that by turning.
                let complaints = client.take_complaints();
                // And the edits it wants made, which arrive the same way
                // and are answered by making them.
                let asked = client.take_asked_edits();
                for params in published {
                    self.on_published(language, &params);
                }
                // Named, because the row says nothing else about who is
                // complaining -- and left in the words it arrived in,
                // which are the server's and not Obelus's to rewrite.
                if let Some(said) = complaints.last() {
                    let name = obelus_lsp::command_for(language).unwrap_or(language.name());
                    tracing::warn!(language = language.name(), "{said}");
                    self.wrong(format!("{name}: {said}"));
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
            // A frame of the one thing moving, and nothing else: what is
            // owed at a moment is owed on a machine with nothing animated
            // on it, and each of the three below says when it wants asking.
            Event::Tick => {
                self.phase = self.phase.wrapping_add(1);
                self.drag_on();
            }
            // The notes, once the reader has stopped typing into them.
            Event::NotesSettled => self.settle_notes(),
            Event::SyntaxSettled => {
                // Let go of first: `catch_up_soon` starts another only when
                // there is none, and one held after it has fired is a tree
                // that never gets a second chance.
                self.syntax_pause = None;
                self.settle_syntax();
            }
            // The rename's own clock is inside the wait it belongs to, so
            // there is nothing to let go of here: the wait ending drops it.
            Event::ChangesSettled => self.settle_changes(),
            Event::SignatureSettled => self.ask_signature_again(),
            // The rest asking what is under it. `settle_hover` is still
            // asked every frame, because the rest of what it does is
            // letting go of an answer the pointer has moved off -- that is
            // about where the pointer is now, not about a moment passing.
            Event::PointerRested => self.settle_hover(),
            Event::RenameOverdue => self.rename_without_them(),
            Event::Search(obelus_search::Event::Matches {
                generation,
                hits,
                done,
            }) => self.on_matches(generation, hits, done),
            Event::Agent(obelus_agent::Event::Acp(message)) => self.on_acp(message),
            // Only the running connection's. A word from one that has been
            // stopped -- what it had said before and nobody had read yet,
            // and that it has gone -- is about a process that is not the
            // one running, and read as the running one's it matched an old
            // answer to a new request and had a live agent taken for dead.
            Event::Agent(obelus_agent::Event::Heard { from, incoming }) => {
                match self
                    .talker
                    .as_ref()
                    .map(obelus_agent::acp::Talk::connection)
                {
                    Some(running) if running == from => self.on_acp(incoming),
                    _ => tracing::debug!(
                        from,
                        "a word from a connection that is not the one running"
                    ),
                }
            }
            Event::Tools(obelus_mcp::Asked { wanted, answer }) => {
                let _ = answer.send(match wanted {
                    obelus_mcp::Wanted::Notes(doing) => self.change_the_notes(doing),
                    obelus_mcp::Wanted::Open { path, line } => self.open_for_an_agent(&path, line),
                    obelus_mcp::Wanted::Workflow => self.workflow_for_an_agent(),
                    obelus_mcp::Wanted::Close { conversation } => {
                        self.close_for_an_agent(conversation)
                    }
                });
            }
            Event::Released(tag) => self.on_released(&tag),
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
            Event::Scanned(scanned) => {
                if let Some(picker) = self.picker.as_mut() {
                    picker.scan_arrived(*scanned);
                }
            }
            Event::Reopened(files) => self.take_up_what_was_open(files),
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
                            section: None,
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
        // Whatever Obelus had to say has been read by now, or was not going to
        // be.
        self.quiet();
        // And a drag is over. Mostly it ended with the button coming up,
        // but a pointer that leaves the terminal takes its release with
        // it, and a drag nothing ever ended would go on scrolling under
        // whatever the reader did next.
        self.dragging = None;

        // Whatever is in front, and on down only as far as each thing lets
        // a key through -- which for anything the reader is *in* is not at
        // all (`app/hearing`). What nobody took goes to the key table.
        if self.hand_over(&key) {
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
        //
        // Except for a key that opens a view or a list over the file, from
        // inside a view or from a list the reader opened: that goes to what
        // it opens, in place of this one, rather than being refused because a
        // dialog is showing (`app/switching`). What a key means in
        // a file is what it means here -- the table is asked as though the
        // file were what is showing -- and only those keys are let through,
        // so nothing opens over anything.
        //
        // Not enter, however it is held. Every list and page takes enter
        // itself, which is why it is never bound (`keymap::why_not`), and
        // with a modifier it is still that list's key: `alt+enter` is "go
        // there" in a history, and in the search, which has no use for it,
        // it was the menu about the name under the caret in the file behind
        // -- the search thrown away for a key nobody meant to leave it by.
        //
        // Unless the view showing has bound that key itself, which is a
        // view saying what the key means *here* -- and that beats what it
        // means one level out, the same way `Keymap::lookup` asks a
        // context's own table before the file's and the file's before
        // everywhere. `f4` was the case while it opened a conversation and
        // meant "which one" inside one, and it swapped the conversation for
        // itself; it is the list everywhere now, and this stays for the
        // next view that takes a key of its own.
        if self.gives_way_to_a_view()
            && key.code != KeyCode::Enter
            && self.keymap.bound_here(&key, self.context()).is_none()
            && let Some(command) = self.keymap.lookup(&key, Context::Normal)
            && command.takes_a_view_s_place()
        {
            self.switch_view(command);
            return;
        }
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
        //
        // The list of names before the settings, because it is opened from
        // them and drawn over them; and the page asking which project
        // last, because every one of the others is drawn over it.
        let inset = if self.prompt.is_some() {
            self.prompt.as_ref().map(obelus_ui::status::answer_inset)
        } else if self.names.is_some() || self.settings.is_some() {
            Some(obelus_ui::status::typed_inset(None))
        } else if self.picker.is_some() {
            self.picker
                .as_ref()
                .map(|picker| obelus_ui::status::typed_inset(picker.question()))
        } else {
            self.chooser.as_ref().map(|chooser| {
                obelus_ui::status::typed_inset(Some(obelus_ui::status::choosing_question(
                    chooser.is_naming(),
                )))
            })
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
        // The way back to the end, on the rule over the box: a press on it
        // is the key it names, through the same door the key goes through,
        // so the two cannot come to mean different things -- with the
        // cursor in the transcript, `ctrl+end` takes the cursor along.
        if kind == Pointer::Pressed
            && self
                .conversation()
                .and_then(|talk| obelus_ui::chat::way_back_at(area, &talk.chat, talk.card.as_ref()))
                .is_some_and(|at| at.contains(ratatui::layout::Position { x, y }))
        {
            self.chat_key(&crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::End,
                crossterm::event::KeyModifiers::CONTROL,
            ));
            return;
        }
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
            match kind {
                Pointer::Moved | Pointer::Released => {}
                Pointer::Dragged => chat.writing_mut().place_at_cell(at.0, at.1, width, true),
                Pointer::Pressed => {
                    // One selection between the two halves, and this is
                    // the other half taking hold -- and the keys with it,
                    // or the caret is put where nothing typed would go.
                    held = true;
                    chat.stand_in_the_box();
                    let writing = chat.writing_mut();
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
            // Nothing yet: what a press would have to land on is a row of
            // two sections and a boundary between them, and a press that
            // guessed wrong would add a font the reader did not point at.
            // The keys do all of it, and a list nobody can click is not a
            // list that lies about what it does.
            obelus_component::layers::Layer::Names => {}
            obelus_component::layers::Layer::Counts => self.press_in_counts(x, y),
            obelus_component::layers::Layer::Settings => self.press_in_settings(x, y),
            // Two keys, and nothing to point at.
            obelus_component::layers::Layer::Gone => {}
        }
    }

    /// Walks to one of a view's tabs, by the shorter way round.
    ///
    /// The tabs wrap, so from where the reader is to where they pressed is
    /// at most half the tabs away -- which for every list Obelus has is one
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
        let width = obelus_ui::card::width_of(band);
        // A drag over the words holds what it crosses, the way it does in
        // every other box. Anywhere else on the card there is nothing for
        // one to do: said so rather than let through, or a drag begun on
        // the card would take hold of the transcript behind it.
        if kind == crate::event::Pointer::Dragged && on == On::Words {
            let place = obelus_ui::card::place_at(card, band, x, y);
            if let Some(card) = self.conversation_mut().and_then(|talk| talk.card.as_mut())
                && let Some((row, cell)) = place
            {
                card.place_in_words(row, cell, width, true);
            }
            return true;
        }
        if kind != crate::event::Pointer::Pressed {
            return true;
        }
        let clicks = self.clicks_at(x, y);
        let area = self.editor_area;

        let Some(card) = self.conversation_mut().and_then(|talk| talk.card.as_mut()) else {
            return true;
        };
        card.stand_on(on);
        if on == On::Words {
            // Asked once the reader is standing in the words, because where
            // in them a point is depends on how they scroll under the
            // caret -- and a card says nothing about a caret it has not got.
            let band = obelus_ui::chat::bands_for(area, card).writing;
            let place = obelus_ui::card::place_at(card, band, x, y);
            if let Some((row, cell)) = place {
                card.place_in_words(row, cell, width, false);
                match clicks {
                    2 => card.hold_in_words(false, width),
                    3 => card.hold_in_words(true, width),
                    _ => {}
                }
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
        let width = obelus_ui::chat::reading_width(area);
        // Laid out once, and every question below asked of the one place.
        let found = self.conversation().and_then(|talk| {
            let rows = talk.chat.rows(width);
            let place = obelus_ui::chat::ChatView::place_in_transcript(
                area,
                &talk.chat,
                talk.card.as_ref(),
                &rows,
                x,
                y,
            )?;
            let spot = obelus_ui::chat::ChatView::spot_in_transcript(&rows, place);
            let row = rows.get(place.row);
            // Whether it landed on a heading that opens, which is a thing to
            // do to the row rather than to the words in it.
            //
            // Free of the selection, and not by luck: every row that folds is
            // one Obelus drew itself -- the heading over a run of tool calls,
            // the one over a piece of thinking, the one over the agent's plan
            // -- and none of them is anybody's words. A press on one already
            // meant nothing but "let go", so opening it costs the reader
            // nothing they had.
            let folds = row.and_then(|row| row.folds);
            // A cursor stands on a row, and the band under the last of them
            // is not one; nor while a card is up, which has the keys -- a
            // cursor moved under it would be found there afterwards.
            let cursor = (row.is_some() && talk.card.is_none()).then_some(place);
            Some((spot, folds, cursor))
        });
        let (spot, folds, cursor) = found.unwrap_or((None, None, None));
        // Where the cursor goes, for a press or a drag: the keys follow
        // the pointer, or the arrows after a press walk something the
        // reader had not pointed at.
        //
        // And a drag only carries a cursor the press put here. The box is
        // under the transcript's band, so a drag in the box is one held
        // past its edge, and every tick hands it on as a drag on the
        // transcript's last row: one that moved the cursor took the keys
        // out of the box while the reader was selecting in it.
        let carried = kind == Pointer::Pressed
            || self.chat().is_some_and(|chat| {
                matches!(chat.focus(), obelus_component::chat::Focus::Transcript(_))
            });
        let Some(talk) = self.conversation_mut() else {
            return;
        };
        if matches!(kind, Pointer::Pressed | Pointer::Dragged)
            && carried
            && let Some(cursor) = cursor
        {
            talk.chat.stand_in_transcript(cursor);
        }
        match kind {
            Pointer::Moved => {}
            Pointer::Released => talk.chat.let_go_of_nothing(),
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
        } else if let Some((_, names)) = self.names.as_mut() {
            names.place_in_query(cell, extend);
        } else if let Some(settings) = self.settings.as_mut() {
            settings.place_in_query(cell, extend);
        } else if let Some(picker) = self.picker.as_mut() {
            picker.place_in_query(cell, extend);
        } else if let Some(chooser) = self.chooser.as_mut() {
            chooser.place_in_typing(cell, extend);
        }
    }

    /// Takes hold of a word of it, or of all of it.
    fn hold_on_status(&mut self, all: bool) {
        if let Some(prompt) = self.prompt.as_mut() {
            prompt.hold(all);
        } else if let Some((_, names)) = self.names.as_mut() {
            names.hold_in_query(all);
        } else if let Some(settings) = self.settings.as_mut() {
            settings.hold_in_query(all);
        } else if let Some(picker) = self.picker.as_mut() {
            picker.hold_in_query(all);
        } else if let Some(chooser) = self.chooser.as_mut() {
            chooser.hold_in_typing(all);
        }
    }

    /// Whether a press may take hold of this bar, with what is showing.
    ///
    /// What is showing owns the pointer as it owns the keys: with a list or
    /// a page over the document, a bar of the document's left in sight above
    /// it is still the document's, and the list puts the document back where
    /// its selection is on the next frame -- so the drag would be undone as
    /// it was made.
    fn reaches(&self, whose: obelus_ui::bars::Whose) -> bool {
        use obelus_ui::bars::Whose;

        match whose {
            // And the two lists that are not layers, which are drawn under
            // every layer with the page they belong to: under a short list
            // their bars are in plain sight and the keys are the list's.
            Whose::Document
            | Whose::Conversation
            | Whose::Notes
            | Whose::Projects
            | Whose::Naming
            | Whose::Commands => !self.layers().covering(),
            Whose::Picker
            | Whose::Preview
            | Whose::Settings
            | Whose::Counts
            | Whose::Names
            | Whose::Completion
            | Whose::Documentation
            | Whose::Hover => true,
        }
    }

    /// What the pointer did to a scrollbar, if it did anything to one.
    ///
    /// The bars are the ones the last frame left on the page, nearest the
    /// reader last -- so the last one under the pointer is the one they can
    /// see. A press off the mark brings the mark to it and goes on holding,
    /// which is one gesture wherever on the track it started.
    fn pointer_on_a_bar(&mut self, kind: crate::event::Pointer, x: u16, y: u16) -> bool {
        use crate::event::Pointer;

        match kind {
            Pointer::Moved => false,
            Pointer::Released => self.holding.take().is_some(),
            Pointer::Pressed => {
                // A release can go missing -- let go outside the window --
                // and a press is a fresh start whatever it lands on.
                self.holding = None;
                let Some(bar) = self
                    .bars
                    .iter()
                    .rev()
                    .find(|bar| bar.under(x, y) && self.reaches(bar.whose))
                else {
                    return false;
                };
                let grip = bar.grip(y);
                let (whose, top) = (bar.whose, bar.top_for(y, grip));
                self.holding = Some((whose, grip));
                if let Some(top) = top {
                    self.drag_bar(whose, top);
                }
                true
            }
            Pointer::Dragged => {
                let Some((whose, grip)) = self.holding else {
                    return false;
                };
                // The bar as the latest frame drew it, which may be none:
                // the list it was beside has closed under the pointer.
                let Some(bar) = self.bars.iter().rev().find(|bar| bar.whose == whose) else {
                    self.holding = None;
                    return true;
                };
                if let Some(top) = bar.top_for(y, grip) {
                    self.drag_bar(whose, top);
                }
                true
            }
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

        self.pointer = Some((x, y));
        // A bar first, and whatever it is beside: a press on one is about
        // the bar and nothing under it, and while it is held every move is
        // the bar's -- wherever the pointer has wandered, the way a bar
        // held anywhere else behaves.
        if self.pointer_on_a_bar(kind, x, y) {
            self.dragging = None;
            return;
        }

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
        // Covering rather than merely open: a question on the status bar
        // leaves every line of the file where the reader can see it, and a
        // line they can see is a line they can point at. Before the notes,
        // which are a document like the file: a page over them took the
        // press here, and it ticked off a note nobody could see.
        if self.layers().covering() {
            self.pointer_in_a_layer(kind, x, y);
            return;
        }
        // The notes, which are a page with a box on it: the box takes the
        // pointer the way the file does, and the rest of the page takes
        // nothing rather than letting it through to the code behind.
        if self.notes().is_some() {
            self.pointer_in_notes(kind, x, y);
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
    fn called(&self, key: &str, word: &str) -> Option<std::borrow::Cow<'static, str>> {
        App::called(self, key, word)
    }
    fn agent_usage(&self) -> Option<&acp::Usage> {
        App::agent_usage(self)
    }
    fn blame(&self) -> Option<&[Option<obelus_git::Blamed>]> {
        App::blame(self)
    }
    fn replacing(&self) -> bool {
        App::replacing(self)
    }
    fn names(&self) -> Option<&obelus_component::names::Names> {
        App::names(self)
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
    fn offers(&self, command: Command) -> bool {
        App::offers(self, command)
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
    fn note_is_wrong(&self) -> bool {
        App::note_is_wrong(self)
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
    fn choosing(&self) -> Option<obelus_ui::Choosing> {
        self.what_is_being_chosen()
    }
    fn naming_list(&self) -> Option<&Picker> {
        self.naming_list.as_ref()
    }

    fn newer_release(&self) -> Option<&str> {
        App::newer_release(self)
    }

    fn phase(&self) -> u32 {
        App::phase(self)
    }
    fn pointer(&self) -> Option<(u16, u16)> {
        self.pointer
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
    fn complaint(&self) -> Option<obelus_ui::Complained<'_>> {
        let complaint = self.complaining.as_ref()?;
        Some(obelus_ui::Complained {
            line: complaint.line,
            column: complaint.column,
            said: &complaint.said,
            severity: complaint.severity,
            others: complaint.others,
        })
    }
    fn prompt(&self) -> Option<&Prompt> {
        App::prompt(self)
    }
    fn making_in(&self) -> Option<String> {
        App::making_in(self)
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

    fn remote(&self) -> Option<(&'static str, obelus_remote::State)> {
        App::remote_badge(self)
    }

    fn server_busy(&self) -> bool {
        App::server_busy(self)
    }
    fn settings(&self) -> Option<&Settings> {
        App::settings(self)
    }
    fn signature(&self) -> Option<&obelus_component::signature::Signature> {
        App::signature(self)
    }
    fn slash(&self) -> Option<&Picker> {
        App::slash(self)
    }
    fn talking(&self) -> Talking {
        App::talking(self)
    }
    fn travelled(&self) -> i64 {
        App::travelled(self)
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
    fn is_about_a_note(&self) -> bool {
        App::is_about_a_note(self)
    }
    fn branch_this_conversation_works_on(&self) -> Option<&obelus_git::Head> {
        App::branch_this_conversation_works_on(self)
    }
    fn head(&self) -> Option<&obelus_git::Head> {
        App::head(self)
    }
    fn tree_has_gone(&self) -> bool {
        App::tree_has_gone(self)
    }
    fn working_directory(&self) -> &Path {
        App::working_directory(self)
    }
}
