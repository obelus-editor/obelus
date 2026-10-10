//! The one list-with-a-prompt, instantiated four times.
//!
//! Files, buffers, themes and the command palette differ in what they list and
//! how tall they are, and in nothing else. Later they are joined by symbols,
//! references and commits, which is why this is a component rather than four
//! screens.
//!
//! Whether a query ranks depends on what the rows are, so it is settled per
//! tab. A log is a timeline and reads newest first whatever is typed at it; a
//! list of names is read by the names, and a reader typing `v0.1` wants the tag
//! of that name, not whichever branch containing those letters was pushed most
//! recently. One view holds both, so the flag moves when the tab does.
//!
//! A query is about the rows the list is a list of. A commit's files hang
//! under the commit, and "which commits mention folding" is not a question
//! about filenames -- scoring them too pulls a file out from under a commit
//! that did not match, and empties a commit that did of the files it was
//! opened to show, so opening it looks like it does nothing. Indentation is
//! not the test: an outline's nested symbols *are* what the reader is looking
//! for. The list says which it is (`nests`).
//!
//! A list that is still arriving must sit still. `replace` is for a
//! different list and starts at the top; `relist` is for the same list with
//! more in it and keeps the row under the reader. And the history filters
//! without ranking (`keeps_order`), which is right on its own -- a log is a
//! timeline, and `git log --grep` keeps it -- and which also means arrivals are
//! older commits that land at the bottom, where they move nothing. Measured
//! first: nucleo ties far more often than expected on short subjects, and ties
//! already break by arrival order, so ranking reorders a log less often than it
//! looks. Less often is not never, and a guarantee beats a tendency.
//!
//! Say "still reading" where it does not move the rows. A note above the
//! list that appears and later goes away slides every row twice. The tab row
//! has room that is already there. And the note carries a count, because a
//! file's history can find nothing for a second and a half and still be
//! working: without a number moving, "not found yet" and "not there" look the
//! same.
//!
//! Which tabs a view has must be a cheap question. The search settles its
//! scopes when it opens and the history settles its radii, and both settle them
//! on facts they can have for nothing: is a file open, does the project have a
//! commit. "Does *this file* have a commit" is not such a fact -- every commit
//! has to be asked whether it touched that path, and the walk that asks is
//! bounded -- so gating the tab on it made the tab vanish for files nobody had
//! edited lately, which are exactly the ones whose history a reader goes
//! looking for. An empty list saying "no commit has touched this file" is an
//! answer; a missing tab is a key that does nothing.
//!
//! A row of a list says what is true now, and says it in one answer. The
//! rows are a snapshot -- building them reads files and walks git, which is
//! not work a frame can do -- so what changes under the reader while the
//! list is up is asked again instead of rebuilt: `Picker::remark`. What it
//! carries is `Said`, the mark and whether the key works there together,
//! because a list that could refresh one without the other is a list that
//! draws a lock on a row and lets the reader into it anyway.

pub mod files;
pub mod wrapped;

/// How many rows of an agent's own words a list will carry.
///
/// Five: enough for a sentence about a command and its arguments, and few
/// enough that the list it is about is still the thing on screen.
const MOST_ABOUT: u16 = 5;

use std::{path::PathBuf, sync::Arc};

use nucleo_matcher::{
    Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};
use obelus_buffer::{DocumentId, question::Question};
use obelus_command::Command;

use self::wrapped::{Above, Body, Columns};
use crate::{
    field::Field,
    window::{Move, Window, Wrap},
};

/// What enter does on a row of the worktrees.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorktreeEnter {
    /// Puts this window on the tree: a terminal's one way of going.
    Switch,
    /// Opens another window on the tree.
    Open,
    /// Brings the window the row is forward.
    Bring,
    /// Stays where it is: the row is this window.
    Stay,
}

/// What accepting an item means.
#[derive(Clone, Debug)]
pub enum PickerValue {
    /// Run a command.
    Command(Command),
    /// Do one of the things a language server offered to do here.
    ///
    /// By its place in the list rather than by the action itself: an
    /// action is a lump of the server's own json, and a list of rows is
    /// not where it belongs.
    Action(usize),
    /// Open a file.
    File(PathBuf),
    /// Open a directory of the tree under it, or close it again.
    ///
    /// A directory is not somewhere to go: what it has is what is in it,
    /// and that goes under it in place rather than in a second list with
    /// its own escape. The same as a commit and its files.
    Directory(PathBuf),
    /// Switch to something already open.
    Document(DocumentId),
    /// Switch theme, by the name it answers to.
    ///
    /// The name rather than the colours: a row of a list should not be
    /// carrying thirty-seven of them, and which colours a name stands for is
    /// a question about the settings directories -- which the list knows
    /// nothing about and the application does.
    Theme(String),
    /// Set a setting to one of its choices.
    ///
    /// The settings view's droplist is this picker, opened over it: a list
    /// of its own would be a second list with its own filtering, its own
    /// scrolling and its own idea of what a selected row looks like.
    Setting {
        /// Which setting, by the name it has in the file.
        key: &'static str,
        /// Which of its choices.
        word: String,
    },
    /// Go to a place a language server named.
    ///
    /// The position is in the protocol's own units and is converted when the
    /// file is opened, because converting it needs that file's text and the
    /// file may never be visited.
    Place {
        /// Which file.
        path: PathBuf,
        /// Its line, counted from zero.
        line: u32,
        /// And how far along, in whichever units the server agreed to.
        character: u32,
        /// The line it ends on.
        end_line: u32,
        /// And how far along that one.
        end_character: u32,
    },
    /// Put one of the agent's settings on one of its values.
    AgentValue {
        /// Which setting, by the agent's id for it.
        setting: String,
        /// Which value, by the agent's id for it.
        value: String,
    },
    /// Say what one of an agent's settings is to start a conversation on.
    ///
    /// Not [`PickerValue::AgentValue`], and the difference is the whole of
    /// why there are two: that one changes the conversation the reader is
    /// in and this one changes what the next one opens on. They are set
    /// from different places and one of them outlives the session.
    AgentDefault {
        /// Which agent, by the registry's id for it.
        agent: String,
        /// Which setting, by the agent's id for it.
        setting: String,
        /// Which value, by the agent's id for it -- or nothing, which is
        /// the reader leaving the answer to the agent.
        value: Option<String>,
    },
    /// Open a commit's files under it, or close them again.
    ///
    /// A commit is not a file, so there is nothing for choosing it to open:
    /// what it has is the list of files it changed, and that goes under it
    /// in place rather than in a second list with its own Escape.
    Commit(gix::ObjectId),
    /// Read a file as a commit had it.
    CommitFile {
        /// Which commit.
        id: gix::ObjectId,
        /// Which of the files it changed, relative to the repository.
        path: PathBuf,
    },
    /// Take up one of the conversations the project has had.
    ///
    /// By its place in the list rather than by the conversation, the way
    /// [`PickerValue::Action`] names a server's offer: which conversation a
    /// row stands for is Obelus's own bookkeeping -- a note or a session
    /// id, a claim, where it is already open -- and a list of rows is not
    /// where that belongs.
    Conversation(usize),
    /// Have the agent review one of the repository's pull requests, by its
    /// number -- which is also what the review is claimed and remembered by.
    PullRequest(u64),
    /// Have the agent answer one of the repository's issues, by its number.
    Issue(u64),
    /// One of the repository's worktrees, or an Obelus on one, to go to.
    ///
    /// By its place in the list, as a conversation is: which window has it
    /// open, and how to reach that window, is the application's to know.
    /// What the two enters do on the row is said here rather than worked
    /// out from it, because the foot says it too, and the key and the foot
    /// have to be one answer.
    Worktree {
        /// Where in the list.
        at: usize,
        /// What enter does.
        enter: WorktreeEnter,
        /// Whether `ctrl+enter` puts this window on the row's tree.
        switches: bool,
    },
    /// A piece of the agent's background work: choosing it opens what it
    /// has written.
    Task {
        /// The agent's id for it.
        id: String,
        /// Whether it can be stopped now -- still going, and said by the
        /// agent to be stoppable -- which is whether the list's own key is
        /// lit on this row.
        stoppable: bool,
    },
    /// One of the ways out of a question Obelus stopped to ask.
    Answer(obelus_buffer::question::Answer),
    /// Nothing. A row that is there to say why the list is short.
    Nothing,
}

/// Which files a file list is showing.
///
/// Two, because "which file do I want" and "what have I been working on"
/// are different questions with different answers, and a reader coming back
/// to a project asks the second one first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Listing {
    /// Everything under the working directory.
    All,
    /// Only the files git says have changed since the last commit.
    Changed,
}

impl Listing {
    /// Both, in the order their tabs sit in.
    pub const ALL: [Self; 2] = [Self::All, Self::Changed];

    /// The tab's name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Changed => "Changed",
        }
    }
}

/// One run of a row's characters, and what to draw them in.
///
/// Char offsets into the label, not bytes and not screen columns: the label
/// is what the row draws, and a run is a claim about its characters.
pub type Colouring = (u16, u16, obelus_text::kind::SyntaxKind);

/// What a row's mark is saying, which is what it gets drawn in.
///
/// The fact rather than the colour: a list is built where the fact is known
/// and painted where the theme is, and a `Color` here would be the only one
/// in this module -- every other thing a row says about itself is a
/// `FileStatus` or a `SyntaxKind` that the view looks up.
///
/// Which matters because the marks do not weigh the same. A fold arrow is
/// the same arrow the gutter and the transcript draw, and recedes there;
/// work that is not on disk is the one thing in a list a reader must not
/// miss. Painting every mark alike makes one of those two wrong.
///
/// Two, because that is how many weights there are. The aside covers both
/// marks that recede -- "there is more behind this row" and "you are already
/// here" -- and splitting it in two would be two names for one colour, which
/// is a distinction the screen does not make and nobody could check.
/// Whichever of them needs its own colour can have its own variant then, and
/// the compiler will name every place that has to answer for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marking {
    /// Something the row says about itself on the way past: what it holds,
    /// or that it is where the reader already is.
    Aside,
    /// Work that Obelus has not written.
    Unwritten,
    /// Something is happening in it that nobody is watching.
    Working,
    /// And something in it is waiting on the reader.
    Waiting,
}

/// What [`Picker::remark`] has to say about one row.
///
/// Two answers rather than an `Option<Option<_>>`, which is what this
/// first was and which nobody can read: the outer one meant "not my row"
/// and the inner one meant "no mark", and those are opposite instructions
/// wearing the same word.
#[derive(Clone, Debug)]
pub enum Remark {
    /// Not a row the caller knows anything about: leave it as it is. Every
    /// list but two is made of these.
    Keep,
    /// What the row says about itself now.
    Now(Said),
}

/// What a row says about itself, asked again while the list is up.
///
/// The mark and whether the row can be chosen, together and never apart.
/// They are two halves of one fact -- a conversation another Obelus has
/// open wears a lock *and* refuses the key -- and a list where one could be
/// refreshed without the other is a list that can say a row is somebody
/// else's while still letting the reader into it -- the same shape as the
/// card whose `submit` row stopped saying the keys were on it. Whoever
/// answers answers both.
///
/// It carried the mark alone while the list of open documents was the only
/// list asking, for the mark that turns while an agent works; the list of
/// conversations needs the lock and whether the key works on that row,
/// which are one fact wearing two faces.
///
/// And the words at its end, which are about the row and not matched by
/// the query: the list of open documents names whose a conversation is
/// there, and the agent's own name for itself arrives with its handshake,
/// which can be after the list opened.
#[derive(Clone, Debug)]
pub struct Said {
    /// The mark, or none at all.
    pub marker: Option<(Marking, String)>,
    /// Whether the key works on it here.
    pub enabled: bool,
    /// The words at the row's end, or none.
    pub trailing: Option<String>,
}

/// One row.
#[derive(Clone, Debug)]
pub struct PickerItem {
    /// Shown before the label, in its own two columns.
    ///
    /// Not part of the label, so the query never matches it: nothing a reader
    /// types is a private-use codepoint, and having one in the haystack would
    /// only skew the scores.
    pub icon: Option<char>,
    /// Shown, and matched against the query.
    pub label: String,
    /// Which version of the thing the label names, where it is not the one
    /// on disk: right after the label, in the colour the status row marks
    /// the same fact in, beside the same path.
    ///
    /// It was `trailing` once, at the far end of the row in the gutter's
    /// grey -- and two rows of one path, the file and a commit's version
    /// of it, read as the same file twice, with the one word that told them
    /// apart as far from the name as the row could put it. Its room comes
    /// out of the label's before the label is cut, so a long path cannot
    /// push it off.
    ///
    /// Not matched, like the detail. Drawn on a row of one line only: a list
    /// whose rows wrap is a list of sentences, and this is said of a name.
    pub version: Option<String>,
    /// Shown dimmed after the label. Not matched: a command's description is
    /// there to be read once, not to be searched.
    pub detail: Option<String>,
    /// Whether the label is a sentence rather than a name.
    ///
    /// A row too narrow for a *name* loses its head: the file name is what
    /// is being looked for and the directories above it are already known.
    /// A sentence is the other way round -- "Fold away the block the cursor
    /// is in" cut to "…the block the cursor is in" has lost the half that
    /// tells a reader which commit this is.
    pub prose: bool,
    /// A mark before the icon, for a row that has something to say about
    /// itself: what it is saying, and the glyph that says it.
    ///
    /// Its own field rather than the icon's, because the icon comes and
    /// goes with the reader's font and this does not: "there is more behind
    /// this row" is the only way folding is discovered, and a reader with
    /// no nerd font has to be told it too.
    ///
    /// The glyph stays with the caller for the same reason: an unwritten
    /// buffer wears its nerd-font mark where there is a font for it and a
    /// bullet where there is not, and which of those is on screen is not
    /// something the view is in a position to know.
    pub marker: Option<(Marking, String)>,
    /// Shown dimmed and right-aligned at the end of the row.
    ///
    /// Its width is taken out of the label's before the label is truncated, so
    /// it is the one part of a row that never gets cut.
    pub trailing: Option<String>,
    /// How much the thing this row names has changed: lines added, lines
    /// taken away.
    ///
    /// Its own field rather than words in `trailing`, because it is two facts
    /// and wears two colours -- the same two the margin marks them in beside
    /// the code, and the same two a commit's message carries above a file.
    /// A reader who learnt them in one place has learnt them here.
    ///
    /// Right of everything, where a number is read down a column rather than
    /// hunted for at the ragged end of a name.
    pub changed: Option<(usize, usize)>,
    /// What choosing it does.
    pub value: PickerValue,
    /// How deep the row sits in whatever it is a list of.
    ///
    /// Not part of the label, so the query never matches the indentation and
    /// the score never depends on how deeply nested a symbol is.
    pub depth: u16,
    /// Whether this row opens, and whether it is open.
    ///
    /// `None` for a row that opens nothing -- a file, a setting's value, a
    /// commit with one file in it. `Some(false)` for one the reader can
    /// open, `Some(true)` for one they have.
    ///
    /// Said here rather than read back out of [`Self::marker`], which is
    /// where it used to live: the arrow was a marker like any other, so
    /// "does this row open" could only be answered by comparing that
    /// marker's characters against the arrow's. A pointer asking which rows
    /// it may open would have been the second reader of that comparison,
    /// and the first was already a guess about an incidental property.
    pub opens: Option<bool>,
    /// What git says about the file the row names, if it says anything.
    ///
    /// Colours the row. A list of a project's files is mostly a list of
    /// files nobody has touched, and the few that have been are what a
    /// reader is usually looking for.
    pub status: Option<obelus_git::FileStatus>,
    /// Whether the row can be chosen.
    ///
    /// A row that cannot is drawn dim and the selection walks past it. Shown
    /// rather than left out, because a list that hides what it cannot do
    /// cannot be learned from: a reader who never sees `show-change` does not
    /// find out that Obelus has it. What they see instead is that it is
    /// there and not available *here*.
    pub enabled: bool,
    /// What the label's characters *are*, for a row that is a line of code.
    ///
    /// Char ranges into the label and the kind to draw each in, so a search
    /// result reads like the file it came from. Worked out only for the rows
    /// on screen, and by the application rather than here: it needs the
    /// file's syntax tree, which the picker knows nothing about.
    ///
    /// `None` means nobody has looked yet; `Some` of an empty list means
    /// there was nothing to find, so it is not looked at twice.
    pub colours: Option<Vec<Colouring>>,
    /// What sort of thing the label names, if the row is about one.
    ///
    /// A colour rather than a word: an outline is a list of names, and the
    /// only honest way to highlight a name is by what it names. The matched
    /// characters still win over it -- why a row is in the list beats what
    /// the row is.
    pub kind: Option<obelus_text::kind::SyntaxKind>,
    /// Which tab the row belongs to, if the picker has tabs.
    ///
    /// An index into the picker's own tab names. `None` means every tab,
    /// which is what a picker without tabs gives all of its rows.
    pub tab: Option<usize>,
    /// The heading of the run of rows this one belongs to, in a list whose
    /// rows wrap ([`Picker::wraps`]).
    ///
    /// Drawn where the run starts among the rows that *match*, rather than
    /// being a row of its own: a heading over a run the query has emptied
    /// is a heading over nothing, and one that was a row would have to be
    /// taken out by everything that counts rows.
    pub section: Option<String>,
}

/// How much of the screen the list takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerLayout {
    /// The whole editor region.
    FullArea,
    /// Only as many rows as there are candidates, up to a limit, sitting
    /// directly on the status bar so the code stays visible.
    Compact {
        /// The most rows it may take.
        ///
        /// And exactly what it takes, for a list that asked to keep a steady
        /// height with [`Picker::keeps_height`].
        rows: u16,
    },
}

/// A scoring the picker wants done somewhere that is not the loop.
///
/// The list is the reader's whole project and the scoring is tens of
/// milliseconds of it, which is a keystroke they would watch arrive. So the
/// rule the history already follows applies here too: a list long enough to
/// be worth searching is long enough to be worth threading.
///
/// The picker does not spawn it. What spawns workers is the application --
/// the only part of Obelus that has heard of every one of them -- so this
/// is handed out, done elsewhere, and handed back.
pub struct Scan {
    /// Which scoring this is.
    ///
    /// An answer about an older one is dropped rather than drawn: a reader
    /// who typed three characters while one was out would otherwise watch
    /// the list settle on the first of them.
    pub generation: u64,
    /// What to score against.
    pub query: String,
    /// The rows, shared rather than copied.
    pub items: Arc<Vec<PickerItem>>,
    /// And which of them to ask about, in the order they are in.
    pub candidates: Vec<usize>,
    /// Which listing of the rows these positions are in.
    pub listings: u64,
}

/// What a [`Scan`] found.
#[derive(Debug)]
pub struct Scanned {
    /// Which scoring it was.
    pub generation: u64,
    /// Which listing of the rows it was about, so an answer that outlived
    /// the rows it is about is dropped rather than drawn.
    pub listings: u64,
    /// The rows that matched and what they scored.
    pub matched: Vec<(usize, u32)>,
}

/// Scores a scan, wherever it is being done.
///
/// Its own matcher, because the picker's is on the loop's side of this and
/// a matcher is scratch space rather than state: two of them cannot
/// disagree about anything, they only hold the buffers a scoring needs.
#[must_use]
pub fn scan(asked: &Scan) -> Scanned {
    let pattern = Pattern::parse(&asked.query, CaseMatching::Smart, Normalization::Smart);
    let mut matcher = Matcher::new(nucleo_matcher::Config::DEFAULT);
    let mut haystack = Vec::new();
    let mut matched = Vec::new();
    for &index in &asked.candidates {
        let Some(item) = asked.items.get(index) else {
            continue;
        };
        let text = Utf32Str::new(&item.label, &mut haystack);
        if let Some(score) = pattern.score(text, &mut matcher) {
            matched.push((index, score));
        }
    }
    Scanned {
        generation: asked.generation,
        listings: asked.listings,
        matched,
    }
}

/// Which rows a pass of the matcher has to ask about.
///
/// Every one of these is the same answer worked out from fewer questions:
/// no row is dropped, no order changes, and what comes out is what a pass
/// over everything would have produced. A list that answered less than the
/// whole truth would be a limit on what can be found in it.
#[derive(Clone, Copy, Debug)]
enum Candidates {
    /// All of them, which is what anything but the two below gets.
    Everything,
    /// The items from here on, with what `matched` holds for the ones
    /// before kept: a batch of a walk appends and changes nothing behind
    /// it.
    From(usize),
    /// Whatever matched last time, for a query that only grew.
    Survivors,
}

impl Candidates {
    /// Whether this pass may be done somewhere other than the loop.
    ///
    /// A batch's own rows may not. The answer for them is wanted before
    /// the next batch lands and the list is one row longer, and a scan out
    /// while the walk is still arriving would be answering about a list
    /// that has moved on -- the walk is where the rows a reader is about
    /// to type over come from.
    const fn may_travel(self) -> bool {
        matches!(self, Self::Everything | Self::Survivors)
    }
}

/// How many rows a scoring has to be about before it is worth sending away.
///
/// Not a limit on anything -- every row is scored either way, and what a
/// reader can find is the same. It is where a thread starts being cheaper
/// than the work: a few thousand rows score in well under a millisecond,
/// and a frame of lag bought for that would be the machinery becoming the
/// thing it was added to fix.
const SENT_AWAY: usize = 20_000;

/// What a key did.
#[derive(Debug)]
pub enum PickerOutcome {
    /// Not a key the picker knows; try the key table.
    Ignored,
    /// Handled. Redraw.
    Consumed,
    /// The user asked to open the row they are on, or to close it again.
    ///
    /// Not handled here: what is behind a row is the caller's, and in the
    /// one list that has anything behind a row it is a question for a
    /// language server. All the list knows is that the key was pressed.
    Open,
    /// The user chose something.
    Accepted(PickerValue),
    /// The user chose something, to have it in this window in place of
    /// what is here.
    ///
    /// `ctrl+enter`, on a list that said this window can go to its rows
    /// ([`Picker::switches_in_place`]) and on a row that says it can be
    /// gone to.
    InPlace(PickerValue),
    /// The user gave up.
    Cancelled,
}

/// What a list says about how much of it there is: words, and a key that
/// does something about it where there is one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tally {
    /// What it says.
    pub words: String,
    /// The key, as its cap names it, and what pressing it does.
    pub key: Option<(String, String)>,
}

/// A prompt and a filtered list.
pub struct Picker {
    /// Behind an `Arc` so that scoring them somewhere else costs an atomic
    /// increment rather than a copy of the list. A project's files are a
    /// hundred thousand rows, and handing a worker its own copy of them --
    /// or even of their labels alone -- is dearer than the scoring it was
    /// sent away to do.
    ///
    /// Written through `Arc::make_mut`, which copies only while a scan is
    /// actually out. A batch landing mid-scan pays for one; a reader typing
    /// into a finished list pays for none, which is every keystroke that
    /// matters.
    items: Arc<Vec<PickerItem>>,
    query: Field,
    /// Indices into `items` that match, best first.
    ///
    /// A field rather than a return value so the allocation survives every
    /// keystroke.
    matched: Vec<(usize, u32)>,
    /// Which character positions matched, for the rows on screen.
    ///
    /// One entry per visible row, in the order they are drawn, holding the
    /// row's position in `matched` and its matched character positions. Only
    /// the visible rows: computing positions is dearer than scoring, and a
    /// file list is tens of thousands of rows long.
    ///
    /// Also reused, so the inner allocations survive a keystroke that leaves
    /// something typed.
    indices: Vec<(usize, Vec<u32>)>,
    /// Which matching row is selected, and which is on the top row.
    ///
    /// The same window every list in Obelus has, and the reason it is state
    /// rather than worked out from the selection: it moves only when the
    /// selection would leave it, so walking down the list moves a cursor
    /// through rows that stay still, and the rows only slide once the
    /// cursor is against an edge. Deriving the top from the selection --
    /// keeping it near the middle, say -- means every single step scrolls
    /// the whole list under a cursor that never moves.
    window: Window,
    /// The file this list is the outline of, if that is what it is.
    ///
    /// A language server answers `documentSymbol` a moment after being
    /// asked, by which time the reader may have closed the list or opened a
    /// different one. The tag is what lets the answer find the list it
    /// belongs to -- and lets it be dropped when the list has gone, without
    /// anything having to remember to clear a flag.
    outline: Option<std::path::PathBuf>,
    /// The tabs across the top, if this picker has any.
    ///
    /// Data rather than a kind: the picker itself has no idea what a group
    /// is, and the one caller with groups hands over names and puts an index
    /// on each row. The first tab shows everything, so nothing is ever
    /// unreachable by walking them.
    tabs: Vec<String>,
    /// What the list is about, drawn above its rows.
    about: Option<String>,
    /// Which tab is showing.
    tab: usize,
    /// Whether the tabs are scopes: rows that come from three different
    /// places rather than three groups of one list.
    scopes: bool,
    /// Whether this list is a search, whose rows are refilled as the query
    /// and the tab move.
    searching: bool,
    /// Whether this list is a list of files, whose rows are refilled when
    /// the tab moves.
    listing: bool,
    /// The command that opened this list, where one did.
    ///
    /// Which is the key that, pressed inside it, leaves the reader where they
    /// are rather than giving them the same list again empty -- see
    /// `app/switching`. Declared where the list is made, because a list's
    /// rows do not say: the palette's are commands, and so is the first row
    /// of the conversations.
    opener: Option<Command>,
    /// Whether the list holds the height it asked for rather than shrinking
    /// to the rows that match.
    steady: bool,
    /// How wide the widest label in the showing tab is, for a list that asked
    /// for its details in one column; `None` for one that did not.
    ///
    /// Over the tab's rows rather than the matching ones, for the reason the
    /// mark's column is: a column that moved as the reader typed would slide
    /// every description sideways under them.
    aligned: Option<usize>,
    /// Whether the rows of this list name something worth showing beneath it.
    previews: bool,
    /// Whether any row carries a mark, and so whether every row leaves a
    /// column for one.
    ///
    /// Over all the rows rather than the matching ones: a column that came
    /// and went as the reader typed would slide the whole list sideways
    /// under them, and the mark is there to be glanced at rather than
    /// hunted for.
    marked: bool,
    /// Whether the empty reason is about the world rather than about there
    /// being nothing to list, and so wins over "No match".
    explains: bool,
    /// A question this list is the answer to, shown in front of the prompt.
    question: Option<String>,
    /// What typing into this list does, for the row before anything has
    /// been typed -- see [`Picker::before_typing`].
    invitation: Option<String>,
    /// What to say when there is nothing to list.
    ///
    /// Per picker, because the reason differs: a file list with nothing in
    /// it and a list of what is open with nothing in it are different facts
    /// about the world. An empty region says only that something is broken.
    empty: String,
    /// A row to select as soon as the list contains it.
    ///
    /// The file list arrives in batches from a walking thread, so the row
    /// worth starting on is usually not there yet when the picker opens.
    /// Re-applied on every batch, because an arriving batch renumbers the
    /// rows, and dropped the moment the reader does anything at all: a list
    /// that jumped under a reader who had already started choosing would be
    /// worse than one that never moved.
    prefer: Option<String>,
    layout: PickerLayout,
    /// The query `matched` was worked out for.
    ///
    /// So that a query which grew can be told from one that changed, which
    /// is what says whether the rows that matched last time are the only
    /// rows worth asking about.
    filtered_on: String,
    /// A scoring waiting to be taken away and done.
    ///
    /// Left here rather than started: the picker is a view and knows no
    /// threads. Whoever is driving it takes this, has it done and brings
    /// the answer back.
    wanted: Option<Scan>,
    /// Which scoring the answer being waited for belongs to.
    ///
    /// `None` where nothing is out, which is also what says the list on
    /// screen is the list the query asked for.
    awaiting: Option<u64>,
    /// The rows `matched` holds positions in.
    ///
    /// A position is about a list, not about a picker: `replace` and
    /// `relist` hand over different rows, and a position kept across one of
    /// those is a number about a list that is gone. Which is not a subtle
    /// failure -- it is a panic on the next frame, out of `matches`, and it
    /// is what keeping the old rows on screen across a hand-off did until
    /// this was here to stop it.
    ///
    /// A count rather than the rows themselves. Holding a second `Arc` to
    /// them was the obvious way and is the wrong one: `make_mut` copies
    /// whenever anything else is holding the rows, so a picker that kept
    /// one deep-copied the whole list on every batch of the walk and then
    /// found its own rows unrecognisable -- which is a list that matches
    /// nothing at all.
    ///
    /// Counted up only where the rows are *replaced*. A batch appends, and
    /// appending leaves every position that already existed pointing at
    /// the row it always did.
    matched_for: u64,
    /// How many scorings have been asked for, ever.
    scans: u64,
    /// How many times the rows have been replaced wholesale.
    listings: u64,
    /// Whether the last row a query could be about matched, as the last
    /// pass left it.
    ///
    /// Kept because a pass may start part way along: a batch landing
    /// between a commit and the files under it would otherwise begin with
    /// no parent and drop them.
    parent_matched: bool,
    matcher: Matcher,
    /// Whether a row with a depth belongs to the row above it.
    ///
    /// A commit's files are listed under the commit, and a query about a
    /// history is a question about commits -- which of them mention this.
    /// Scoring the files as well pulls a file out from under a commit that
    /// did not match, leaving a row about a change with nothing on screen
    /// saying which change; and it empties a commit that *did* match of the
    /// files it was opened to show, so opening it looks like it did nothing.
    ///
    /// Not true of every list that indents. An outline's nested symbols are
    /// the things being looked for, not children of the row above them.
    ///
    /// Goes with [`keeps_order`](Self::keeps_order): a child follows its
    /// parent, and a ranking that put one above the other would part them.
    nests: bool,
    /// What is still arriving, drawn beside the tabs.
    ///
    /// A list that is still filling has to say so, and it has to say so
    /// somewhere that does not move its rows: a line above them that
    /// appears and later goes away slides the whole list under the reader
    /// twice. Beside the tabs there is room that is already there.
    filling: Option<String>,
    /// What the list says about how much of it there is, at the far end of
    /// the row it is typed into.
    tally: Option<Tally>,
    /// Whether there is more of the list than has been fetched, so that a
    /// step off either end does not wrap round to the other.
    unfinished: bool,
    /// Whether the list's own order is an answer, so a query filters it
    /// without reordering it.
    ///
    /// A log is a timeline. Typing "fold" into one asks which commits
    /// mention folding, not which subject line a fuzzy matcher liked best,
    /// and a short sentence written by a person gives a matcher very little
    /// to prefer one over another with -- so ranking replaces an order that
    /// means something with one that means almost nothing. `git log --grep`
    /// keeps the timeline; so does every log a reader has seen.
    ///
    /// It also makes a list that is still arriving sit still. Ranked, a
    /// commit that turns up with a better score than the selected row
    /// inserts *above* it, and the row under the reader's eye becomes a
    /// different commit -- repeatedly, for as long as the walk runs. In the
    /// list's own order the arrivals are older commits, which belong at the
    /// bottom, so nothing above the selection ever moves.
    ordered: bool,
    /// Whether this list says at its foot what its own keys do.
    ///
    /// Only a list with keys of its own. Every list answers to the arrows,
    /// to enter and to escape, and a row of the reader's screen spent
    /// saying so is a row spent on what they just did -- so the foot goes
    /// where there is something they could not have guessed.
    footed: bool,
    /// Whether the card listing every key is up.
    keys: bool,
    /// Whether a key that opens a view may take this list's place.
    ///
    /// Every list the reader opened will: it is somewhere they are
    /// choosing, and a key naming another view is the reader choosing that
    /// instead -- the palette held on to `f1` while the files were one row
    /// of it away. What will not is a list waiting on the reader: a
    /// question, whose way out by any other key is an answer nobody gave,
    /// and what went wrong on the way up, which is owed a reading first.
    /// Declared where the list is made, because nothing else about a list
    /// says which of the two it is.
    gives_way: bool,
    /// How this search is looking, where the question arises.
    ///
    /// `None` where it does not: a list that is not a search, and the
    /// symbols tab, whose rows come from a language server that did its own
    /// matching and would not know what to do with a pattern of ours. The
    /// foot greys the keys there rather than dropping them, so it does not
    /// change height as the reader steps between tabs.
    looking: Option<obelus_search::Looking>,
    /// Whether this search is offering names from outside the project,
    /// where that is a question about it.
    ///
    /// `None` on the tabs where it is not one: a walk of this tree and a
    /// search of the open file are inside the project by construction, and
    /// only a language server has an index that reaches past it.
    outside: Option<bool>,
    /// Whether this list is offering the files a project ignores, where that
    /// is a question about it at all.
    ///
    /// `None` where the key means nothing: the changed files come from git
    /// rather than from a walk, and what a project ignores is not part of that
    /// answer either way. Greyed at the foot rather than dropped from it, so
    /// the list does not change height as the reader steps between tabs.
    ///
    /// Set by whoever filled the list, because the walk and the setting
    /// behind it are both theirs -- what is the list's is only that the foot
    /// is drawn from it, and a foot that had to guess would guess wrong on
    /// the first frame after the key.
    ignored: Option<bool>,
    /// And whether it is offering the ones a system keeps out of sight.
    ///
    /// Its own, beside [`Picker::offers_ignored`] and for the reason that
    /// one is its own: they keep two different things out, so a foot that
    /// drew one switch for both would say one of them was set when the
    /// other was.
    hidden: Option<bool>,
    /// Whether the rows of this list open and close.
    ///
    /// Which decides what its two enters mean. A list of places answers
    /// enter by going there; a list whose rows also *hold* something
    /// answers it by opening -- the way a directory in the counted tree
    /// and a commit in a history do -- and going there moves to
    /// `alt+enter`. Both keys are the picker's own, like enter and escape
    /// before them, rather than commands out of the table: nothing else
    /// binds them and there is nothing for a reader to rebind.
    opens: bool,
    /// Whether `ctrl+enter` puts this window where a row is.
    ///
    /// The worktrees in a window: enter is another window, and `ctrl+enter`
    /// is this one going. Said by whoever filled the list, and per tab,
    /// because the other tab of that list is documents, which are only ever
    /// here.
    in_place: bool,
    /// Whether this is a list of the agent's background work, and whether
    /// its rows can be stopped from here -- which is the one key of its own
    /// such a list has.
    tasks: Option<bool>,
    /// Whether the rows of this list are only read.
    ///
    /// A list of things to be told rather than chosen from: what went wrong
    /// on the way up, whose rows go nowhere. Such a list has no row the
    /// reader is on -- a mark behind one would say the keys act on it,
    /// and nothing does -- so it is drawn without one, the keys that move
    /// a selection move the rows instead, and neither enter nor a click
    /// does anything. Nor is it typed at: a filter narrows a list to the
    /// row the reader means to choose, and here there is none to mean, so
    /// a letter is not this list's and nothing goes into the query. Said
    /// where the list is made rather than worked out from its rows going
    /// nowhere, which a row may do in a list that is otherwise chosen
    /// from.
    reads: bool,
    /// Whether the rows of this list wrap, and the mark in front of a
    /// row's detail where they do.
    ///
    /// `Some(None)` for a list that wraps with nothing in front of its
    /// details.
    wraps: Option<Option<char>>,
    /// How many rows a wrapped row's detail may take.
    detail_rows: usize,
    /// Every row's words wrapped, at the width the list was last settled
    /// at.
    ///
    /// Kept because every frame asks the whole list how tall it is -- the
    /// bar says how much is above, and the window which rows fit -- and
    /// wrapping every row of it three times a frame is work a keystroke
    /// would wait on. Asked of rather than trusted: a measurement of other
    /// rows, or at another width, is worked out again row by row.
    measured: Option<Measured>,
    /// How many rows of a wrapping list were on screen when it last
    /// settled, which is how far a page moves it.
    shown: usize,
    /// Scratch for `Utf32Str::new`, which needs somewhere to put a converted
    /// haystack.
    haystack: Vec<char>,
}

/// Every row of a wrapping list laid out, and what it was laid out for.
#[derive(Debug)]
struct Measured {
    /// Which listing it was, so that a list refilled is measured again.
    listings: u64,
    /// How many rows there were, so that a batch appended is too.
    count: usize,
    /// How wide the list was.
    width: u16,
    /// Where the words went across it.
    columns: Columns,
    /// Each row's words, by the row's place in the whole list.
    bodies: Vec<Body>,
}

impl std::fmt::Debug for Picker {
    /// `Matcher` has no `Debug` and is large.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Picker")
            .field("query", &self.query.said())
            .field("items", &self.items.len())
            .field("matched", &self.matched.len())
            .field("selected", &self.window.focus())
            .field("layout", &self.layout)
            .finish_non_exhaustive()
    }
}

impl Picker {
    /// Opens a picker over `items`.
    #[must_use]
    pub fn new(items: Vec<PickerItem>, layout: PickerLayout) -> Self {
        let items = Arc::new(items);
        let mut picker = Self {
            items: Arc::clone(&items),
            query: Field::new(),
            matched: Vec::new(),
            indices: Vec::new(),
            filtered_on: String::new(),
            wanted: None,
            awaiting: None,
            matched_for: 0,
            listings: 0,
            scans: 0,
            parent_matched: false,
            window: Window::new(),
            outline: Option::None,
            tabs: Vec::new(),
            about: None,
            tab: 0,
            scopes: false,
            searching: false,
            listing: false,
            opener: None,
            steady: false,
            aligned: None,
            previews: false,
            explains: false,
            marked: false,
            question: None,
            invitation: None,
            empty: "Nothing to choose from".to_string(),
            prefer: None,
            nests: false,
            opens: false,
            in_place: false,
            tasks: None,
            reads: false,
            filling: None,
            tally: None,
            unfinished: false,
            ordered: false,
            footed: false,
            keys: false,
            gives_way: true,
            looking: None,
            outside: None,
            ignored: None,
            hidden: None,
            layout,
            wraps: None,
            detail_rows: wrapped::MOST_DETAIL_ROWS,
            measured: None,
            shown: 0,
            matcher: Matcher::new(nucleo_matcher::Config::DEFAULT),
            haystack: Vec::new(),
        };
        picker.refilter();
        picker
    }

    /// The list a question is.
    ///
    /// One row per way out, in the order the question offers them, and a
    /// last row for cancelling -- which every question has, which means the
    /// same thing in all of them, and which escape does as well. As tall as
    /// it has rows: a question small enough to answer is small enough to
    /// show whole, and one that scrolled would be hiding one of its answers.
    ///
    /// The rows are built here rather than by the caller because a question
    /// has no business knowing what a row is made of. What every question
    /// gets for free is exactly this function.
    #[must_use]
    pub fn asking(question: &Question) -> Self {
        let row = |label: String, about: Option<String>, answer| PickerItem {
            // A sentence rather than a name: "close without saving" cut to
            // "\u{2026}without saving" has lost the half that says what it
            // does, which is the other way round from a file path.
            prose: true,
            marker: None,
            // No icon. A question's rows are not things of a kind the way
            // files and commands are, and a glyph on each would be three
            // decorations standing in for three different meanings.
            icon: None,
            label,
            version: None,
            detail: about,
            trailing: None,
            changed: None,
            opens: None,
            value: PickerValue::Answer(answer),
            enabled: true,
            colours: None,
            status: None,
            depth: 0,
            kind: None,
            tab: None,
            section: None,
        };
        let items: Vec<_> = question
            .ways()
            .iter()
            .map(|way| row(way.label.clone(), way.detail.clone(), way.answer))
            .chain(std::iter::once(row(
                "cancel".to_string(),
                None,
                obelus_buffer::question::Answer::Cancel,
            )))
            .collect();
        let rows = u16::try_from(items.len()).unwrap_or(u16::MAX);
        let mut picker = Self::new(items, PickerLayout::Compact { rows });
        // Above the ways out rather than in front of the prompt, which is
        // where a list that is being searched says what it is searching.
        // A question is read before its answers, not after them, and the
        // prompt sits *under* the rows: a reader who found the question
        // there had already read the three things they could do about it.
        picker.about(question.prompt());
        picker.will_not_give_way();
        picker
    }

    /// Gives the picker a row of tabs.
    ///
    /// `names` are the groups; the tab drawn first is "all", which the picker
    /// adds itself, so every row is reachable by walking them and so is a row
    /// belonging to no group.
    pub fn with_tabs(&mut self, names: &[&str]) {
        self.tabs = std::iter::once("All".to_string())
            .chain(names.iter().map(|name| (*name).to_string()))
            .collect();
        self.refilter();
    }

    /// Gives the picker a row of tabs that are *scopes* rather than groups.
    ///
    /// No "all" tab, and the rows are not filtered by which tab is showing:
    /// the caller swaps the rows when the tab moves, because each scope's
    /// rows come from somewhere else -- the file in memory, a walk of the
    /// tree, a language server. A synthetic "all" would promise a list that
    /// nothing can produce.
    pub fn with_scopes(&mut self, names: &[&str]) {
        self.tabs = names.iter().map(|name| (*name).to_string()).collect();
        self.scopes = true;
        self.refilter();
    }

    /// Shows a particular tab, for a key that opens the list at one.
    pub fn go_to_tab(&mut self, tab: usize) {
        if tab < self.tabs.len() {
            self.tab = tab;
            self.window.set_focus(0);
            self.refilter();
        }
    }

    /// Says this list is a search, whose rows the application refills as the
    /// query and the tab move.
    pub const fn searches(&mut self) {
        self.searching = true;
    }

    /// Whether this list is a search.
    #[must_use]
    pub const fn is_searching(&self) -> bool {
        self.searching
    }

    /// Says a row with a depth belongs to the row above it, so a query asks
    /// about the parents and a child is shown when its parent is.
    pub const fn nests(&mut self) {
        self.nests = true;
    }

    /// Says the rows of this list open and close.
    pub const fn opens_rows(&mut self) {
        self.opens = true;
        self.footed = true;
    }

    /// Whether they do.
    #[must_use]
    pub const fn rows_open(&self) -> bool {
        self.opens
    }

    /// Says the list is still being filled, and what to show while it is.
    ///
    /// `None` once it is not.
    pub fn filling(&mut self, note: Option<String>) {
        self.filling = note;
    }

    /// Says how much of it there is, or what is being done about the rest,
    /// at the far end of the row it is typed into -- `None` for nothing.
    pub fn tally(&mut self, tally: Option<Tally>) {
        self.tally = tally;
    }

    /// What it says about how much of it there is.
    #[must_use]
    pub const fn how_much(&self) -> Option<&Tally> {
        self.tally.as_ref()
    }

    /// Says whether there is more of the list than it has: a list fetched a
    /// page at a time has no other end to wrap round to until its last page
    /// is in, and a step past the last row it has would put the reader at
    /// the top of a list that goes on below them.
    pub const fn unfinished(&mut self, unfinished: bool) {
        self.unfinished = unfinished;
    }

    /// What the list is still waiting for, if it is waiting.
    ///
    /// Two things can be keeping it, and they are one thing to a reader:
    /// the rows are still arriving, or the rows are still being matched.
    /// Either way what is on screen is not the answer yet, and the place
    /// that says so is the same place -- a second note for the second
    /// reason would be two answers to "is this list still moving".
    ///
    /// The scoring's own words, because the caller has none for it: it
    /// does not know a scoring was sent away and should not have to. What
    /// it is waiting *for* is the difference, and the difference is the
    /// whole of what the note is for.
    #[must_use]
    pub fn is_filling(&self) -> Option<&str> {
        match self.awaiting {
            Some(_) => Some("Matching\u{2026}"),
            None => self.filling.as_deref(),
        }
    }

    /// Says whether this list's own order is an answer, so that a query
    /// filters the rows without reordering them.
    ///
    /// Not settled once for a list whose tabs hold different kinds of
    /// thing: a log is a timeline and reads newest first whatever is typed
    /// at it, while a list of names is read by the names, and a reader
    /// typing one wants the nearest name rather than the newest.
    pub fn keeps_order(&mut self, keeps: bool) {
        if self.ordered != keeps {
            self.ordered = keeps;
            self.refilter();
        }
    }

    /// Holds the list at the height it asked for, however few rows match.
    ///
    /// For a list a reader walks as much as they type at. The palette is
    /// read down -- most of what it offers is what the reader came to find
    /// out -- and a block that resized on every keystroke would move the row
    /// under their eye between one letter and the next.
    ///
    /// Off by default, and asked for rather than worked out. It used to be
    /// inferred from the list having tabs, which was one list's preference
    /// wearing another list's property: nothing about a tab says anything
    /// about height, and the next list to grow tabs would have inherited a
    /// decision nobody made for it.
    ///
    /// The rows it does not fill are blank, and a compact list is drawn over
    /// code that is still being read, so each of them is a row of that code
    /// covered by nothing. That is the price, and it is why this is off
    /// unless a list says the steadiness is worth more.
    pub const fn keeps_height(&mut self) {
        self.steady = true;
    }

    /// Starts every row's detail in one column, after the widest label in
    /// the tab that is showing.
    ///
    /// For a list whose labels are names and whose details say what each
    /// does: the palette's descriptions are read down as a column, and a
    /// column that starts wherever its name happened to end is one the eye
    /// has to find again on every row. Per tab, so a tab of short names is
    /// not pushed across the row by a long name it does not hold.
    pub fn aligns_details(&mut self) {
        self.aligned = Some(0);
        self.refilter();
    }

    /// Where a row's detail starts, counted in columns from where its label
    /// does, for a list that asked for its details in one column.
    #[must_use]
    pub const fn detail_column(&self) -> Option<usize> {
        self.aligned
    }

    /// Says the rows of this list are sentences, read whole: see
    /// [`wrapped`].
    ///
    /// `detail_mark` is what stands in front of a row's detail, saying what
    /// the detail is -- one mark for the list, because a detail is the same
    /// kind of thing on every row of it.
    ///
    /// And the list is as tall as all of it would be, up to what it asked
    /// for, rather than as tall as what matches: rows this tall resizing the
    /// block on every letter typed would move the row under the reader's eye
    /// by several rows at a time.
    pub const fn wraps(&mut self, detail_mark: Option<char>) {
        self.wraps = Some(detail_mark);
    }

    /// Says a wrapped row's detail is shown whole, however many rows it
    /// takes.
    ///
    /// The cap is for somebody else's text -- what an agent called a
    /// conversation, what it says about one of its settings -- which may be
    /// as long as it likes. A list whose details are Obelus's own words has
    /// nobody to guard against, and cutting them would end a sentence
    /// Obelus wrote to explain the choice half-way through it. On a narrow
    /// screen that is a taller row, and the list scrolls.
    pub const fn details_whole(&mut self) {
        self.detail_rows = usize::MAX;
    }

    /// Whether the rows of this list wrap.
    #[must_use]
    pub const fn is_wrapping(&self) -> bool {
        self.wraps.is_some()
    }

    /// What stands in front of a row's detail, in a list whose rows wrap.
    #[must_use]
    pub const fn detail_mark(&self) -> Option<char> {
        match self.wraps {
            Some(mark) => mark,
            None => None,
        }
    }

    /// Where a wrapped row's words go, in a list `width` wide.
    #[must_use]
    pub fn columns(&self, width: u16) -> Columns {
        match self.measured_for(width) {
            Some(measured) => measured.columns,
            None => Columns::of(width, wrapped::trailing_width(self.items.iter())),
        }
    }

    /// One row's words, wrapped for a list `width` wide.
    #[must_use]
    pub fn body(&self, index: usize, width: u16) -> std::borrow::Cow<'_, Body> {
        if let Some(body) = self
            .measured_for(width)
            .and_then(|measured| measured.bodies.get(index))
        {
            return std::borrow::Cow::Borrowed(body);
        }
        let columns = self.columns(width);
        std::borrow::Cow::Owned(
            self.items
                .get(index)
                .map(|item| Body::of(item, columns, self.detail_rows))
                .unwrap_or_default(),
        )
    }

    /// What goes above the row at a place among the rows that match.
    #[must_use]
    pub fn above(&self, at: usize) -> Above {
        let item = |at: usize| self.matched.get(at).map(|(index, _)| &self.items[*index]);
        item(at).map_or(
            Above {
                headed: false,
                first: at == 0,
            },
            |this| Above::of(this, at.checked_sub(1).and_then(item)),
        )
    }

    /// How tall each row that matches is, in a list `width` wide: what goes
    /// above it and its words.
    #[must_use]
    pub fn heights(&self, width: u16) -> Vec<u16> {
        self.matched
            .iter()
            .enumerate()
            .map(|(at, (index, _))| {
                self.above(at)
                    .rows()
                    .saturating_add(self.body(*index, width).rows())
            })
            .collect()
    }

    /// The row at a place among the rows that match, laid out for a list
    /// `width` wide: the row, what goes above it, and its words.
    #[must_use]
    pub fn wrapped_row(
        &self,
        at: usize,
        width: u16,
    ) -> Option<(&PickerItem, Above, std::borrow::Cow<'_, Body>)> {
        let (index, _) = self.matched.get(at)?;
        Some((
            &self.items[*index],
            self.above(at),
            self.body(*index, width),
        ))
    }

    /// The measurement, where it is of these rows at this width.
    fn measured_for(&self, width: u16) -> Option<&Measured> {
        self.measured.as_ref().filter(|measured| {
            measured.listings == self.listings
                && measured.count == self.items.len()
                && measured.width == width
        })
    }

    /// Lays every row out at `width`, unless that has been done.
    fn measure(&mut self, width: u16) {
        if self.measured_for(width).is_some() {
            return;
        }
        let columns = Columns::of(width, wrapped::trailing_width(self.items.iter()));
        self.measured = Some(Measured {
            listings: self.listings,
            count: self.items.len(),
            width,
            columns,
            bodies: self
                .items
                .iter()
                .map(|item| Body::of(item, columns, self.detail_rows))
                .collect(),
        });
    }

    /// How tall the whole list is, wrapped, counted no further than `most`.
    ///
    /// Every row rather than the ones matching, so that it does not change
    /// as the query does -- see [`Picker::wraps`]. Stops at `most` because
    /// that is all the answer is used for, and the rows past it are rows
    /// nobody has to wrap to say so.
    fn wrapped_height(&self, width: u16, most: u16) -> u16 {
        let mut taken = 0u16;
        let mut before: Option<&PickerItem> = None;
        for (index, item) in self.items.iter().enumerate() {
            taken = taken
                .saturating_add(Above::of(item, before).rows())
                .saturating_add(self.body(index, width).rows());
            if taken >= most {
                return most;
            }
            before = Some(item);
        }
        taken
    }

    /// Says the rows of this list name something worth showing beneath it.
    ///
    /// A file, a place in one, a commit: whatever the reader would be looking
    /// at if they chose the row. A list of commands, themes or settings names
    /// none of those, and the room a preview would take is better spent on
    /// the code the list is drawn over.
    ///
    /// Asked for rather than worked out. It used to be read off the layout --
    /// a full-area list previewed and a compact one did not -- which held
    /// only because the lists that name files are the same seven lists that
    /// take the whole region. Two sets that happen to coincide, and nothing
    /// making them: the next full-area list of something unpreviewable would
    /// have set aside half the screen to show nothing in.
    ///
    /// Said by the list rather than worked out per row, so the pane does not
    /// come and go under a reader walking a list whose rows differ.
    pub const fn previews(&mut self) {
        self.previews = true;
    }

    /// Says it shows none, for a list whose tab has moved to rows that name
    /// nowhere to look -- a list of open documents previews them, and its
    /// worktrees tab is whole checkouts.
    pub const fn stops_previewing(&mut self) {
        self.previews = false;
    }

    /// Says whether `ctrl+enter` puts this window where a row is, which
    /// makes the two enters two different things, and so whether its foot
    /// says what each of them does.
    pub const fn switches_in_place(&mut self, in_place: bool) {
        self.in_place = in_place;
        self.footed = in_place;
    }

    /// Whether it does.
    #[must_use]
    pub const fn goes_in_place(&self) -> bool {
        self.in_place
    }

    /// What enter does on the row the reader is on, where it is a worktree
    /// that can be chosen.
    ///
    /// Nothing on a dim one, which the selection rests on where a query
    /// has left nothing else: enter refuses it, so the foot must too.
    #[must_use]
    pub fn worktree_enter(&self) -> Option<WorktreeEnter> {
        match self
            .selected_item()
            .filter(|item| item.enabled)
            .map(|item| &item.value)
        {
            Some(PickerValue::Worktree { enter, .. }) => Some(*enter),
            _ => None,
        }
    }

    /// Whether `ctrl+enter` does anything on the row the reader is on: the
    /// list says this window can go to its rows, and the row says it is
    /// somewhere to go.
    #[must_use]
    pub fn switches_here(&self) -> bool {
        self.in_place
            && matches!(
                self.selected_item()
                    .filter(|item| item.enabled)
                    .map(|item| &item.value),
                Some(PickerValue::Worktree { switches: true, .. })
            )
    }

    /// Says this is a list of the agent's background work, and whether a
    /// row of it can be stopped -- which its foot says.
    pub const fn lists_tasks(&mut self, stoppable: bool) {
        self.tasks = Some(stoppable);
        self.footed = true;
    }

    /// Whether it is one, and whether its rows can be stopped.
    #[must_use]
    pub const fn listing_tasks(&self) -> Option<bool> {
        self.tasks
    }

    /// Whether the row the reader is on can be stopped from here: the list
    /// says the agent stops work at all, and the row says this work can be.
    #[must_use]
    pub fn stops_this_one(&self) -> bool {
        self.tasks == Some(true)
            && matches!(
                self.selected_item().map(|item| &item.value),
                Some(PickerValue::Task {
                    stoppable: true,
                    ..
                })
            )
    }

    /// Whether it does.
    #[must_use]
    pub const fn shows_previews(&self) -> bool {
        self.previews
    }

    /// Says which command opened this list: see `opener`.
    pub const fn opened_by(&mut self, command: Command) {
        self.opener = Some(command);
    }

    /// The command that opened it, where one did.
    #[must_use]
    pub const fn opener(&self) -> Option<Command> {
        self.opener
    }

    /// Says this list is a list of files, whose rows the application
    /// refills when the tab moves.
    pub const fn lists_files(&mut self) {
        self.listing = true;
    }

    /// Whether this list is a list of files.
    #[must_use]
    pub const fn is_listing(&self) -> bool {
        self.listing
    }

    /// Says this list has keys of its own worth a foot.
    pub const fn says_its_keys(&mut self) {
        self.footed = true;
    }

    /// Whether it does.
    #[must_use]
    pub const fn says_keys(&self) -> bool {
        self.footed
    }

    /// Whether the card listing every key is showing.
    #[must_use]
    pub const fn showing_keys(&self) -> bool {
        self.keys
    }

    /// Says how this search is looking, or that the question does not arise.
    pub const fn looking_how(&mut self, how: Option<obelus_search::Looking>) {
        self.looking = how;
    }

    /// And what it was told.
    #[must_use]
    pub const fn looks_how(&self) -> Option<obelus_search::Looking> {
        self.looking
    }

    /// Says whether this search reaches past the project, or that the
    /// question does not arise.
    pub const fn reaching_outside(&mut self, outside: Option<bool>) {
        self.outside = outside;
    }

    /// And what it was told.
    #[must_use]
    pub const fn reaches_outside(&self) -> Option<bool> {
        self.outside
    }

    /// Says whether this list is offering the files a project ignores, or that
    /// the question does not arise here.
    pub const fn offering_ignored(&mut self, offering: Option<bool>) {
        self.ignored = offering;
    }

    /// And what it was told.
    #[must_use]
    pub const fn offers_ignored(&self) -> Option<bool> {
        self.ignored
    }

    /// Says whether this list is offering the files a system keeps out of
    /// sight, or that the question does not arise here.
    pub const fn offering_hidden(&mut self, offering: Option<bool>) {
        self.hidden = offering;
    }

    /// And what it was told.
    #[must_use]
    pub const fn offers_hidden(&self) -> Option<bool> {
        self.hidden
    }

    /// The tab names, empty for a picker without tabs.
    #[must_use]
    pub fn tabs(&self) -> &[String] {
        &self.tabs
    }

    /// Which tab is showing.
    #[must_use]
    pub const fn tab(&self) -> usize {
        self.tab
    }

    /// How many rows the tabs take: the names, a rule, or none at all.
    #[must_use]
    pub const fn tab_rows(&self) -> u16 {
        if self.tabs.is_empty() { 0 } else { 2 }
    }

    /// Says what the list is about, above its rows.
    ///
    /// For a list that is an answer to something the reader did not start:
    /// an agent asking to run a command is a question, and three options
    /// with no account of what they answer is a question with the words
    /// missing. The prompt row can hold a few of those words; this holds
    /// the ones that do not fit on a row.
    ///
    /// Nothing at all for nothing to say, so that a list which says
    /// something about one of its tabs and nothing about the next does not
    /// keep a blank row and a rule where the words were.
    pub fn about(&mut self, about: &str) {
        self.about = (!about.is_empty()).then(|| about.to_string());
    }

    /// What the list is about, if it says.
    #[must_use]
    pub fn what_about(&self) -> Option<&str> {
        self.about.as_deref()
    }

    /// How many rows that takes at a width: the words, and a rule under
    /// them.
    ///
    /// Capped, because it is somebody else's prose: an agent explaining
    /// itself at length must not push the list it belongs to off the
    /// screen. What is left of it is on the row that was cut.
    #[must_use]
    pub fn about_rows(&self, width: u16) -> u16 {
        let Some(about) = self.about.as_deref() else {
            return 0;
        };
        // Wrapped at the width the drawing wraps at, which is two columns
        // in from the edge. Counted at the full width instead, a sentence
        // that needs one more row than the count says loses its tail --
        // and loses it silently, which is worse than not saying it.
        let inside = width.saturating_sub(2);
        let rows = u16::try_from(obelus_text::wrapped(about, inside).len()).unwrap_or(MOST_ABOUT);
        rows.clamp(1, MOST_ABOUT).saturating_add(1)
    }

    /// Moves to the next tab, or the previous one, wrapping.
    fn step_tab(&mut self, forward: bool) {
        if self.tabs.is_empty() {
            return;
        }
        let last = self.tabs.len() - 1;
        self.tab = match (forward, self.tab) {
            (true, at) if at == last => 0,
            (true, at) => at + 1,
            (false, 0) => last,
            (false, at) => at - 1,
        };
        // A different list, so the old selection means nothing.
        self.window.set_focus(0);
        self.refilter();
    }

    /// Says this list is the outline of a file.
    pub fn is_outline_of(&mut self, path: std::path::PathBuf) {
        self.outline = Some(path);
    }

    /// Which file it is the outline of, if it is one.
    #[must_use]
    pub fn outline_of(&self) -> Option<&std::path::Path> {
        self.outline.as_deref()
    }

    /// Replaces every row, keeping the query and the tag.
    ///
    /// For an answer that arrives after the list is on screen. The selection
    /// goes back to the top: the rows are not the rows that were there, so
    /// where the selection was means nothing.
    pub fn replace(&mut self, items: Vec<PickerItem>) {
        self.items = Arc::new(items);
        self.listings += 1;
        self.window.set_focus(0);
        self.refilter();
    }

    /// Replaces every row with the same rows as they are now, standing on
    /// the one the reader was on.
    ///
    /// Not [`Picker::replace`], which is for an answer that is a different
    /// list: these are the same things, said again because one of them
    /// moved on, and a selection sent back to the top under a reader who
    /// was about to stop the third one stops the first.
    ///
    /// `same` says whether two rows' values are one thing: a value is not
    /// always comparable, and the caller knows what its rows stand for.
    pub fn renew(
        &mut self,
        items: Vec<PickerItem>,
        same: impl Fn(&PickerValue, &PickerValue) -> bool,
    ) {
        let standing = self.selected_item().map(|item| item.value.clone());
        self.items = Arc::new(items);
        self.listings += 1;
        self.refilter();
        let at = standing.and_then(|standing| {
            self.matched.iter().position(|(index, _)| {
                self.items
                    .get(*index)
                    .is_some_and(|item| same(&item.value, &standing))
            })
        });
        if let Some(at) = at {
            self.window.set_focus(at);
        }
    }

    /// Puts fresh marks on the rows the caller claims, without rebuilding
    /// them.
    ///
    /// For the one thing in a list that changes while the reader is looking
    /// at it and is nobody's keystroke: what an agent is doing in a
    /// conversation. The rows themselves are a snapshot on purpose --
    /// building them walks the tree for git's opinion and reads the notes
    /// off disk, which is not work a frame can do -- and the mark went with
    /// them, so a conversation that started working while the list was up
    /// never said so and one that finished went on turning. The frame of
    /// the turning mark comes from the ticker, which is what made the
    /// second so convincing.
    ///
    /// Never the words. A row's label is what the query matched, and the
    /// matched characters are offsets into it: changing the words here
    /// would leave a list highlighting cells that are no longer the ones
    /// that matched. What a row *says about itself* is another matter, and
    /// [`Said`] is the whole of it.
    pub fn remark(&mut self, mut mark: impl FnMut(&PickerValue) -> Remark) {
        for item in Arc::make_mut(&mut self.items) {
            if let Remark::Now(now) = mark(&item.value) {
                item.marker = now.marker;
                item.enabled = now.enabled;
                item.trailing = now.trailing;
            }
        }
        // The column is kept for whichever rows have one, and whether any
        // row has one is what decides whether every row leaves room. Worked
        // out again here for the same reason the marks are: a mark that
        // appeared where the list had none would otherwise be drawn in a
        // column nothing left room for.
        self.marked = self
            .items
            .iter()
            .any(|item| item.marker.is_some() || item.opens.is_some());
    }

    /// Puts new rows in the list without moving the reader off theirs.
    ///
    /// [`replace`](Self::replace) is for a different list, and a different
    /// list starts at the top. This is for the same list with more in it --
    /// a history still arriving, a commit opened to show its files -- where
    /// the row under the reader is still the row they chose, and yanking
    /// them back to the top every time a batch lands would make a filling
    /// list impossible to read.
    pub fn relist(&mut self, items: Vec<PickerItem>) {
        let selected = self.window.focus();
        self.items = Arc::new(items);
        self.listings += 1;
        self.refilter();
        // Only where the batch moved it: choosing the same row again is a
        // choice, and would put a window the reader dragged back on it.
        if self.window.focus() != selected {
            self.select_row(selected);
        }
    }

    /// Moves the selection by rows, stopping at the ends.
    ///
    /// For the wheel, which is not the arrow keys: rolling past the end of a
    /// list and reappearing at the top is a jump nobody asked for, and a
    /// wheel is rolled without looking.
    pub fn move_selection_by(&mut self, rows: isize) {
        // The wheel over a list that is only read moves the rows, since
        // there is no selection for a notch to step. A page of one, which
        // lets the top go as far as the last row: the next frame's settle
        // is what holds it to a page from the end.
        if self.reads {
            let movement = match rows < 0 {
                true => Move::Up,
                false => Move::Down,
            };
            for _ in 0..rows.unsigned_abs() {
                self.scroll_reading(movement, 1);
            }
            return;
        }
        self.move_selection(rows, Wrap::No);
    }

    /// Makes this a list that is waiting on the reader: see `gives_way`.
    pub const fn will_not_give_way(&mut self) {
        self.gives_way = false;
    }

    /// Whether a key that opens a view may take its place.
    #[must_use]
    pub const fn gives_way(&self) -> bool {
        self.gives_way
    }

    /// Makes this a list that is only read: see `reads`.
    pub fn only_read(&mut self) {
        self.reads = true;
    }

    /// Whether it is one.
    #[must_use]
    pub const fn is_only_read(&self) -> bool {
        self.reads
    }

    /// Puts a query back, for a list that has been rebuilt under a reader
    /// who had already typed one.
    pub fn set_query(&mut self, query: &str) {
        self.query.replace(query);
        self.refilter();
    }

    /// Puts a question in front of the prompt.
    ///
    /// For a list that is an answer to something rather than a way of
    /// finding something: an agent asking to run a command is a question,
    /// and a bare list of three options is that question with the words
    /// missing.
    pub fn ask(&mut self, question: &str) {
        self.question = Some(question.to_string());
    }

    /// Whether any row carries a mark, and so whether every row leaves a
    /// column for one.
    #[must_use]
    pub const fn marked(&self) -> bool {
        self.marked
    }

    /// The question this list is answering, if it is answering one.
    #[must_use]
    pub fn question(&self) -> Option<&str> {
        self.question.as_deref()
    }

    /// Says what typing into this list does, for the row before anything
    /// has been.
    ///
    /// Every list in Obelus is typed into and none of them said so: the
    /// row is a prompt glyph and nothing else, so a reader who has not
    /// been told has no way to find out but to try. Which is the same
    /// argument [`Picker::when_empty`] makes about a list with nothing in
    /// it -- what the reader cannot see, the row has to say.
    ///
    /// Its own words per list rather than one sentence for all of them,
    /// because the verb differs: a file list narrows what it is already
    /// showing and a search goes and looks. One sentence would be wrong
    /// on half of them.
    ///
    /// Nothing where a list is a *question* -- an agent asking to be
    /// allowed something -- because the question is already the words in
    /// front of the prompt, and two sets of words in one row is neither.
    pub fn before_typing(&mut self, what: &str) {
        self.invitation = Some(what.to_string());
    }

    /// What this list says typing into it does, while nothing has been.
    #[must_use]
    pub fn invitation(&self) -> Option<&str> {
        match self.query.said().is_empty() && self.question.is_none() {
            true => self.invitation.as_deref(),
            false => None,
        }
    }

    /// Sets what the list says when it is empty.
    ///
    /// Written for the case where there is nothing *to* list, which is a fact
    /// about the world; a query that matches nothing is a fact about the
    /// query, and the view says that itself.
    pub fn when_empty(&mut self, reason: &str) {
        self.empty = reason.to_string();
        self.explains = false;
    }

    /// Sets what the list says when it is empty, whether or not something
    /// has been typed.
    ///
    /// For a search: with a query in the prompt and no rows, "No match" is
    /// only true once something has looked. While nothing has been asked
    /// yet, while a walk is still running, or when there is no server to
    /// ask, the fact about the world is the true answer and the query is
    /// beside the point.
    pub fn while_empty(&mut self, reason: &str) {
        self.empty = reason.to_string();
        self.explains = true;
    }

    /// What to show instead of rows, if anything.
    ///
    /// `None` while there are rows to draw.
    #[must_use]
    pub fn nothing_to_show(&self) -> Option<&str> {
        if self.match_count() > 0 {
            return None;
        }
        Some(if self.query.is_empty() || self.explains {
            &self.empty
        } else {
            "No match"
        })
    }

    /// Which rows are on screen, as indexes into the whole list.
    ///
    /// For work only the application can do and only for what is visible:
    /// a search of a project can hold two thousand rows, and the ten being
    /// looked at are the ten worth spending anything on.
    #[must_use]
    pub fn visible(&self, height: u16) -> Vec<usize> {
        let first = self.first_visible(height);
        self.matched
            .iter()
            .skip(first)
            .take(usize::from(height))
            .map(|(index, _)| *index)
            .collect()
    }

    /// Whether the row on a matched position can be chosen.
    fn can_choose(&self, row: usize) -> bool {
        self.matched
            .get(row)
            .and_then(|(index, _)| self.items.get(*index))
            .is_some_and(|item| item.enabled)
    }

    /// The nearest row that can be chosen, looking `forward` first.
    ///
    /// Both ways, because the rows that cannot be chosen come in runs: a
    /// reader stepping down into a run of them should come out the bottom of
    /// it, and one that reaches the end of the list should not be left
    /// pointing at nothing.
    fn choosable(&self, from: usize, forward: bool) -> Option<usize> {
        let rows = self.matched.len();
        if rows == 0 {
            return None;
        }
        let from = from.min(rows - 1);
        let ahead = if forward {
            (from..rows).chain(0..from).collect::<Vec<_>>()
        } else {
            (0..=from).rev().chain((from..rows).rev()).collect()
        };
        ahead.into_iter().find(|row| self.can_choose(*row))
    }

    /// One row, by its index in the whole list.
    #[must_use]
    pub fn rows_at(&self, index: usize) -> Option<&PickerItem> {
        self.items.get(index)
    }

    /// One row, to fill in what only the application can work out.
    pub fn row_mut(&mut self, index: usize) -> Option<&mut PickerItem> {
        // Whatever is filled in may be words, and the rows were wrapped
        // from the words.
        self.measured = None;
        Arc::make_mut(&mut self.items).get_mut(index)
    }

    /// Asks for a row to be selected once the list holds one with this label.
    ///
    /// For the file picker, which opens on the file being read: a list of
    /// every file in a project, opened at the top, starts by pointing at
    /// something arbitrary.
    pub fn prefer(&mut self, label: String) {
        self.prefer = Some(label);
        self.refilter();
    }

    /// How much of the screen it takes.
    #[must_use]
    pub const fn layout(&self) -> PickerLayout {
        self.layout
    }

    /// How many rows the list occupies, given the room available.
    ///
    /// One implementation, called by the renderer to place the list and by the
    /// key handler to size a page. Two would drift, and the symptom would be
    /// a page that moves by not quite a screenful.
    #[must_use]
    pub fn visible_rows(&self, available: u16, width: u16) -> u16 {
        let above = self.tab_rows().saturating_add(self.about_rows(width));
        match self.layout {
            PickerLayout::FullArea => available,
            PickerLayout::Compact { rows } if self.wraps.is_some() => self
                .wrapped_height(width, rows)
                .clamp(1, rows)
                .saturating_add(above)
                .min(available),
            // All of what it asked for, for a list that said it wants to stay
            // the height it started at.
            PickerLayout::Compact { rows } if self.steady => {
                rows.saturating_add(above).min(available)
            }
            // Otherwise as tall as it has rows, up to what it asked for. A
            // compact list is drawn over code the reader is still reading, so
            // a row it takes and does not use is a row of that code covered
            // by nothing.
            //
            // At least one, which is where the reason for having none goes.
            PickerLayout::Compact { rows } => u16::try_from(self.match_count())
                .unwrap_or(u16::MAX)
                .max(1)
                .min(rows)
                .saturating_add(above)
                .min(available),
        }
    }

    /// What has been typed.
    #[must_use]
    pub fn query(&self) -> String {
        self.query.said()
    }

    /// Where the caret is in it, and which of it is held.
    ///
    /// For whoever draws the row: the caret is a column worked out from
    /// what comes before it, and a run held is a run marked. Both are the
    /// query's own, because the query is a text with a caret in it.
    #[must_use]
    pub fn query_caret(&self) -> usize {
        self.query.caret().get()
    }

    /// Which characters of it the reader has hold of.
    #[must_use]
    pub fn query_held(&self) -> Option<std::ops::Range<usize>> {
        self.query.held()
    }

    /// Puts a run of text into the query, which is what a paste is.
    pub fn put_in_query(&mut self, said: &str) {
        // A paste is typing all at once, and a list that is only read is
        // not typed at.
        if self.reads {
            return;
        }
        self.query.put(said);
        self.refilter_typed();
    }

    /// Puts the query's caret where a cell of its row is.
    ///
    /// `cell` is counted from the first character of the query: what is
    /// drawn in front of it belongs to whoever draws it.
    pub fn place_in_query(&mut self, cell: u16, extend: bool) {
        if self.reads {
            return;
        }
        self.query.place_at_cell(cell, extend);
    }

    /// Takes hold of the word under the caret, or of the whole query.
    pub fn hold_in_query(&mut self, all: bool) {
        if self.reads {
            return;
        }
        match all {
            true => self.query.hold_all(),
            false => self.query.hold_word(),
        }
    }

    /// What a copy takes from the query: what is held, or all of it.
    #[must_use]
    pub fn copy_query(&self) -> (String, &'static str) {
        self.query.copied()
    }

    /// The same, and takes it out.
    pub fn cut_query(&mut self) -> (String, &'static str) {
        let taken = self.query.cut();
        self.refilter();
        taken
    }

    /// The matching rows, best first.
    pub fn matches(&self) -> impl Iterator<Item = &PickerItem> {
        self.matched.iter().map(|(index, _)| &self.items[*index])
    }

    /// Which of the list's own rows the selection is on.
    ///
    /// The list's numbering rather than the query's: a caller who keeps
    /// something beside the list keeps it per row, and which rows match is
    /// not a thing that alignment survives.
    #[must_use]
    pub fn selected_row(&self) -> Option<usize> {
        self.matched
            .get(self.window.focus())
            .map(|(index, _)| *index)
    }

    /// Puts the selection on one of the list's own rows.
    ///
    /// The inverse of [`selected_row`](Self::selected_row), and the one a
    /// caller who keeps something beside the list needs: what it knows is
    /// which row it wants, and where that row sits among the ones matching
    /// is the list's to work out. Does nothing where the query is hiding
    /// it, which is the honest answer -- the reader cannot be moved to a
    /// row that is not on screen.
    pub fn select_item(&mut self, index: usize) {
        if let Some(at) = self.matched.iter().position(|(row, _)| *row == index) {
            self.select(at);
        }
    }

    /// The selected row, if there is one.
    #[must_use]
    pub fn selected_item(&self) -> Option<&PickerItem> {
        let (index, _) = self.matched.get(self.window.focus())?;
        self.items.get(*index)
    }

    /// How many rows there are, before the query narrows them.
    ///
    /// Distinct from [`Picker::match_count`]: a search fills its rows when
    /// there is a question to answer and clears them when there is not, and
    /// "has this been filled" is not the same as "does the query match".
    #[must_use]
    pub fn row_count(&self) -> usize {
        self.items.len()
    }

    /// How many rows match.
    #[must_use]
    pub fn match_count(&self) -> usize {
        self.matched.len()
    }

    /// Which matching row is selected.
    #[must_use]
    pub const fn selected(&self) -> usize {
        self.window.focus()
    }

    /// The window itself, for the view: what is on screen, and whether
    /// there is more of it than there is screen.
    #[must_use]
    pub const fn window(&self) -> &Window {
        &self.window
    }

    /// Puts this row at the top, for a bar the reader has hold of.
    pub fn drag_to(&mut self, top: usize) {
        self.window.drag_to(top);
    }

    /// Where the query matched in one row, worked out now.
    ///
    /// [`Picker::indices_at`] answers the same question from what the last
    /// frame worked out, which is right for drawing -- the rows on screen
    /// are the rows being drawn -- and wrong for anything that happens on a
    /// key: a key can arrive before the first frame, and then the answer
    /// would be "nothing matched".
    pub fn matched_columns(&mut self, row: usize) -> Vec<u32> {
        if self.query.is_empty() {
            return Vec::new();
        }
        let Some((index, _)) = self.matched.get(row).copied() else {
            return Vec::new();
        };
        let mut indices = Vec::new();
        let said = self.query.said();
        self.marks_in(index, &said, &mut indices);
        indices
    }

    /// Where the query is in one row's label, as character positions.
    ///
    /// Two rules, because a picker and a search are two things. A picker is
    /// choosing among names it is holding and matches the way a reader types
    /// a name they half remember, loosely. A search is asking where a string
    /// is, and `ac` is not in `abc` -- so it marks the run it found and
    /// nothing else.
    ///
    /// `said` is the query, passed in rather than read: this runs once per
    /// row on screen, and the query lives in a rope that would be walked
    /// into a fresh string for every one of them.
    fn marks_in(&mut self, index: usize, said: &str, indices: &mut Vec<u32>) {
        indices.clear();
        let label = &self.items[index].label;
        if self.searching {
            let how = self.looking.unwrap_or_default();
            let Some(run) = obelus_search::Needle::new(said, how).found_in(label) else {
                return;
            };
            indices.extend(run.map(|at| u32::try_from(at).unwrap_or(u32::MAX)));
            return;
        }
        let pattern = Pattern::parse(said, CaseMatching::Smart, Normalization::Smart);
        let haystack = Utf32Str::new(label, &mut self.haystack);
        pattern.indices(haystack, &mut self.matcher, indices);
        indices.sort_unstable();
        indices.dedup();
    }

    /// The character positions of one visible row that the query matched.
    ///
    /// Empty for a row outside the window [`Picker::refresh_indices`] was last
    /// given, which is the window the renderer is about to draw.
    #[must_use]
    pub fn indices_at(&self, row: usize) -> &[u32] {
        self.indices
            .iter()
            .find(|(at, _)| *at == row)
            .map_or(&[], |(_, indices)| indices.as_slice())
    }

    /// Which match is on the top row of a window `height` rows tall.
    ///
    /// A pure reader: the window is put right by the window's own settling,
    /// which runs once a frame before anything is drawn, and having two
    /// places clamp it would leave neither of them responsible.
    #[must_use]
    pub const fn first_visible(&self, _height: u16) -> usize {
        self.window.top()
    }

    /// Works out which characters matched, for the rows about to be drawn.
    ///
    /// Called once a frame with the height the list will have, rather than
    /// from every path that changes the query or the selection: the window
    /// depends on the geometry, and the geometry is only settled at that
    /// point. `width` is the list's, which says how tall a row that wraps
    /// is.
    pub fn refresh_indices(&mut self, height: u16, width: u16) {
        // The window first: which rows are about to be drawn is the question
        // the matched characters are worked out for, and the height is only
        // known here.
        if self.wraps.is_some() {
            self.measure(width);
            let heights = self.heights(width);
            self.window.settle_by_height(&heights, height);
            self.shown = self.window.visible_by_height(&heights, height).len();
        } else {
            self.window.settle(height);
        }

        // Reuse the allocations: `indices` holds one vector per row, and the
        // rows are the same rows on the next keystroke.
        let mut spare: Vec<Vec<u32>> = self
            .indices
            .drain(..)
            .map(|(_, mut indices)| {
                indices.clear();
                indices
            })
            .collect();
        if self.query.is_empty() {
            return;
        }

        let first = self.first_visible(height);
        let said = self.query.said();
        for row in first..first.saturating_add(usize::from(height)) {
            let Some((index, _)) = self.matched.get(row).copied() else {
                break;
            };
            let mut indices = spare.pop().unwrap_or_default();
            self.marks_in(index, &said, &mut indices);
            self.indices.push((row, indices));
        }
    }

    /// The scoring this list wants done somewhere else, if it wants one.
    ///
    /// Taken rather than read: there is one of them and whoever takes it
    /// owes an answer.
    pub fn wanted_scan(&mut self) -> Option<Scan> {
        self.wanted.take()
    }

    /// Whether a scoring is out, so the rows on screen are the rows the
    /// query before this one asked for.
    ///
    /// What a view says about it is the same thing it says about a list
    /// still arriving: something is happening, and the rows have not
    /// changed yet.
    #[must_use]
    pub const fn is_matching(&self) -> bool {
        self.awaiting.is_some()
    }

    /// An answer to a scoring this list asked for.
    ///
    /// One that is not the one being waited for is dropped: a reader who
    /// typed again while it was out has asked a newer question, and drawing
    /// the older answer would settle the list on a query they have already
    /// moved past.
    pub fn scan_arrived(&mut self, scanned: Scanned) {
        if self.awaiting != Some(scanned.generation) {
            return;
        }
        // And about the rows it was asked about. A batch that landed while
        // it was out copied them out from under it, so the positions it
        // came back with are positions in a list this picker no longer
        // holds -- which is a panic on the next frame rather than a wrong
        // row. Asked again instead, against the rows there are now.
        if scanned.listings != self.listings {
            self.awaiting = None;
            self.refilter();
            return;
        }
        self.awaiting = None;
        self.matched = scanned.matched;
        if !self.ordered {
            self.matched
                .sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));
        }
        self.settled();
    }

    /// Adds more items to a list that is still being gathered.
    ///
    /// The file walk arrives in batches, and the user is already typing while
    /// it does.
    pub fn extend(&mut self, items: impl IntoIterator<Item = PickerItem>) {
        let was = self.items.len();
        Arc::make_mut(&mut self.items).extend(items);
        // Only what the batch brought. The rows before it are rows the last
        // pass already answered for, and a batch appends rather than
        // changing any of them.
        self.filter(Candidates::From(was));
    }

    /// Handles a key.
    ///
    /// The picker takes everything it recognizes, including the printable
    /// characters that go into the prompt, which is why these are not commands:
    /// a command per character would be the logical end of that road.
    /// `page` is how many rows are on screen, which only the layout knows.
    pub fn handle_key(&mut self, key: &crossterm::event::KeyEvent, page: u16) -> PickerOutcome {
        use crossterm::event::{KeyCode, KeyModifiers};

        // A page of rows that wrap is the rows that were on screen, which is
        // not how many screen rows there are: moving by that would step
        // several screenfuls at once.
        let page = match self.wraps {
            Some(_) => self.shown,
            None => usize::from(page),
        };
        let page = isize::try_from(page.max(1)).unwrap_or(isize::MAX);
        // A key carrying a modifier this branch does not name falls through,
        // the same rule the key table and the editor's motions follow.
        // Without it `ctrl+pageup` pages the list, which is a different thing
        // from what it should do.
        let Some(modifiers) = obelus_keymap::modifiers_of(key) else {
            return PickerOutcome::Ignored;
        };
        let control = modifiers == KeyModifiers::CONTROL;
        let bare = modifiers.is_empty();
        // The paging keys are the two the list does not have to itself:
        // bare, they belong to whatever the list is showing underneath --
        // a preview is read a screenful at a time -- and the application
        // takes them for it before the list is asked. So they page the
        // list with control held, and bare only where nothing took them.
        let paging = matches!(key.code, KeyCode::PageUp | KeyCode::PageDown);

        let outcome = match key.code {
            // A list that is only read is moved, not chosen in: the keys
            // that walk a selection elsewhere move the rows under a window
            // with no row marked in it, and enter has nothing to act on.
            // Swallowed rather than refused, so it does not reach whatever
            // is under the list.
            KeyCode::Enter if self.reads => PickerOutcome::Consumed,
            // Bare home and end among them: here there is no query for
            // a caret to move along.
            code if self.reads
                && (bare || control)
                && let Some(movement) = Move::of(code) =>
            {
                self.scroll_reading(movement, page);
                PickerOutcome::Consumed
            }
            // The same keys the editor uses to reach the ends of a document,
            // doing the same thing to a list. That they duplicate Home and
            // End here is worth it: a key should not mean one thing in one
            // view and nothing in the next.
            KeyCode::Home if control => {
                self.select(0);
                PickerOutcome::Consumed
            }
            KeyCode::End if control => {
                self.select(self.matched.len().saturating_sub(1));
                PickerOutcome::Consumed
            }
            // `tab` walks the tabs, which is the key's own name and the
            // only thing it can mean in a list: nothing here indents, and
            // the one completion Obelus accepts with a key accepts with
            // enter. The arrows used to do this and cannot any more -- the
            // query is a text with a caret in it, and left and right are
            // where a caret goes.
            //
            // Only where there are tabs to walk. Elsewhere they fall
            // through, which is what a picker with nothing to switch should
            // do with a key that means nothing to it.
            KeyCode::Tab if bare && !self.tabs.is_empty() => {
                self.step_tab(true);
                PickerOutcome::Consumed
            }
            KeyCode::BackTab if !self.tabs.is_empty() => {
                self.step_tab(false);
                PickerOutcome::Consumed
            }
            // The card first: a key that opens a thing closes that thing,
            // and escape reaches the nearest thing on screen.
            KeyCode::Esc if bare && self.keys => {
                self.keys = false;
                PickerOutcome::Consumed
            }
            // Then what is held in the query, which is nearer than the
            // list it is part of.
            KeyCode::Esc if bare && self.query.let_go() => PickerOutcome::Consumed,
            KeyCode::Esc if bare => PickerOutcome::Cancelled,
            _ if self.footed && obelus_keymap::is_keys_card(key) => {
                self.keys = !self.keys;
                PickerOutcome::Consumed
            }
            // Enter opens the row where the rows open, and `alt+enter`
            // goes there -- the pair a list of places that hold places
            // needs, since one key cannot mean both.
            KeyCode::Enter if bare && self.opens => PickerOutcome::Open,
            KeyCode::Enter if self.in_place && control => self
                .matched
                .get(self.window.focus())
                .map(|(index, _)| &self.items[*index])
                .filter(|item| item.enabled && self.switches_here())
                .map_or(PickerOutcome::Consumed, |item| {
                    PickerOutcome::InPlace(item.value.clone())
                }),
            KeyCode::Enter if bare || (self.opens && modifiers == KeyModifiers::ALT) => self
                .matched
                .get(self.window.focus())
                .map(|(index, _)| &self.items[*index])
                .filter(|item| item.enabled)
                .map_or(PickerOutcome::Consumed, |item| {
                    PickerOutcome::Accepted(item.value.clone())
                }),
            // Every key that moves about a list, from the table every list
            // reads. What is this list's own is what a step means here: a
            // row that cannot be chosen is stepped over rather than landed
            // on, so the moving goes through `move_selection` rather than
            // straight to the window.
            //
            // Bare home and end are not among them any more. They used to
            // reach the first and last row, duplicating `ctrl+home` and
            // `ctrl+end` on purpose -- but the query is a line with a caret
            // in it now, and bare home and end are where a caret goes in
            // every other text Obelus holds. The duplicate was what made
            // them free to give away.
            code if (bare || (control && paging))
                && !(bare && matches!(code, KeyCode::Home | KeyCode::End))
                && let Some(movement) = Move::of(code) =>
            {
                let wrap = match self.unfinished {
                    true => Wrap::No,
                    false => Wrap::Yes,
                };
                match movement {
                    Move::Up => self.move_selection(-1, wrap),
                    Move::Down => self.move_selection(1, wrap),
                    // Clamped rather than wrapped, unlike a single step:
                    // paging is how you get to the end of a long list, and
                    // a page that wraps past it overshoots the thing you
                    // were reaching for.
                    Move::PageUp => self.move_selection(-page, Wrap::No),
                    Move::PageDown => self.move_selection(page, Wrap::No),
                    Move::First => self.select(0),
                    Move::Last => self.select(self.matched.len().saturating_sub(1)),
                }
                PickerOutcome::Consumed
            }
            // Not a list that is only read, which has no query: what it did
            // not want is somebody else's, and `ctrl+q` still leaves.
            _ if self.reads => PickerOutcome::Ignored,
            // Everything the list did not want goes to the query, which
            // is a line with a caret in it and takes the keys a line takes:
            // the arrows, the words, what is held, what is typed. A key it
            // has no use for it refuses, and that is how `ctrl+q` still
            // reaches the key table and leaves Obelus from in here.
            _ => match self.query.handle_key(key) {
                true => {
                    self.refilter_typed();
                    PickerOutcome::Consumed
                }
                false => PickerOutcome::Ignored,
            },
        };

        // The reader has taken over. One place rather than a line in each arm
        // above: an arm that forgot it would leave the list jumping to a file
        // that arrived after the reader started choosing.
        if !matches!(outcome, PickerOutcome::Ignored) {
            self.prefer = None;
        }
        outcome
    }

    /// Puts the selection on a row, for a caller that knows which one it
    /// wants.
    ///
    /// Opening a commit's files puts rows below the row the key was pressed
    /// on; a selection that jumped to the top would leave the reader
    /// somewhere they did not ask to be.
    pub fn select_row(&mut self, row: usize) {
        // A list that is only read has no row to be on.
        if self.reads {
            return;
        }
        self.select(row);
    }

    /// Moves the rows of a list that is only read.
    ///
    /// The window's top and its focus kept together, so that settling it
    /// on the next frame -- which brings the focus back on screen -- has
    /// nothing to bring back: the focus is where the window starts.
    fn scroll_reading(&mut self, movement: Move, page: isize) {
        let rows = isize::try_from(self.matched.len()).unwrap_or(isize::MAX);
        let last = (rows - page).max(0);
        let top = isize::try_from(self.window.top()).unwrap_or(0);
        let to = match movement {
            Move::Up => top - 1,
            Move::Down => top + 1,
            Move::PageUp => top - page,
            Move::PageDown => top + page,
            Move::First => 0,
            Move::Last => last,
        }
        .clamp(0, last);
        self.window.scroll(to - top);
        self.window.set_focus(usize::try_from(to).unwrap_or(0));
    }

    fn select(&mut self, row: usize) {
        let row = row.min(self.matched.len().saturating_sub(1));
        // Onwards from where it was asked for, so `ctrl+home` lands on the
        // first row that can be chosen rather than on the first row.
        self.window
            .set_focus(self.choosable(row, true).unwrap_or(row));
    }

    /// Moves the selection, skipping what cannot be chosen.
    ///
    /// The moving is the window's, which is what makes a list here walk the
    /// way a page of settings and a list of cards walk. What is this
    /// list's own is the skipping: a picker has rows that are there to be
    /// read rather than pressed.
    fn move_selection(&mut self, by: isize, wrap: Wrap) {
        let landed = self.window.step(by, wrap);
        // Carried on in the direction of travel: stepping down into a run of
        // rows that cannot be chosen comes out of the bottom of it, which is
        // where the reader was going.
        if let Some(choosable) = self.choosable(landed, by >= 0) {
            self.window.set_focus(choosable);
        }
    }

    /// Works the matched list out again, asking about every item.
    ///
    /// The one to call where anything but the query has moved: a list
    /// relisted, a tab walked to, a preference put on it. What `matched`
    /// held before says nothing about a list whose items are not the items
    /// it was built from.
    fn refilter(&mut self) {
        self.filter(Candidates::Everything);
    }

    /// The same, after the reader has typed into the query.
    ///
    /// A query that only grew can only narrow: everything matching `rr`
    /// matched `r`, because a fuzzy match is a subsequence and a longer
    /// pattern is a stricter one. So the rows to ask about are the rows
    /// that answered last time, which on a list of a hundred thousand
    /// files is the difference between every keystroke costing the whole
    /// tree and only the first one doing.
    ///
    /// Not a limit and not a guess -- the answer is the same list in the
    /// same order, and nothing is hidden. A limit on a list is a limit on
    /// what can be found in it, which is a trade this project has turned
    /// down once already.
    ///
    /// Two lists cannot take it. A search's rows are answers rather than
    /// candidates and never go through the matcher at all; and a nested
    /// list's children are kept by whether their parent matched, which is
    /// a fact about the order the items are in and not about any row on
    /// its own.
    fn refilter_typed(&mut self) {
        // What was typed is a different list, and the reader is looking at
        // the box rather than at where they dragged the window to.
        self.window.back_to_the_focus();
        let said = self.query.said();
        let narrows = !said.is_empty()
            && !self.filtered_on.is_empty()
            && !self.searching
            && !self.nests
            && said.starts_with(&self.filtered_on);
        match narrows {
            true => self.filter(Candidates::Survivors),
            false => self.filter(Candidates::Everything),
        }
    }

    /// Works out which rows match, over the candidates it is given.
    fn filter(&mut self, candidates: Candidates) {
        // The rows before a batch are rows this already answered for, and a
        // batch cannot change what they said: the walk appends. Re-asking
        // about them is what made a list of a hundred thousand files cost
        // the whole list once per batch of five hundred -- the same answer,
        // worked out two hundred times over.
        let from = match candidates {
            Candidates::From(first) => first,
            _ => 0,
        };
        // Nothing is cleared yet. Whether this pass does the scoring here
        // or hands it out is not known until the candidates are counted,
        // and a list emptied before that question is answered is a list
        // that says there is nothing while it thinks.

        // A marker or an arrow: the column is kept for whichever of them a
        // row has, because both are drawn in it and the names of a list
        // have to line up whether the row beside them opens or is merely
        // marked.
        //
        // A batch can only add rows, so a list that had one still has one
        // and only the new rows are worth asking about. A narrowing pass
        // adds none at all.
        self.marked = match candidates {
            Candidates::Survivors => self.marked,
            Candidates::From(_) => {
                self.marked
                    || self.items[from..]
                        .iter()
                        .any(|item| item.marker.is_some() || item.opens.is_some())
            }
            Candidates::Everything => self
                .items
                .iter()
                .any(|item| item.marker.is_some() || item.opens.is_some()),
        };

        // The first tab is every row; any other one is its own. Scope tabs
        // do not filter at all -- every row in the list belongs to the scope
        // that fetched it.
        let tab = self.tab;
        let scopes = self.scopes;
        let showing = move |item: &PickerItem| match (scopes, tab, item.tab) {
            (true, _, _) | (_, 0, _) | (_, _, None) => true,
            (_, tab, Some(of)) => tab == of,
        };
        // A narrowing pass is the same tab with the same rows in it.
        if self.aligned.is_some() && !matches!(candidates, Candidates::Survivors) {
            self.aligned = self
                .items
                .iter()
                .filter(|item| showing(item))
                .map(|item| obelus_text::text_width(&item.label))
                .max()
                .or(Some(0));
        }

        // A search's rows are answers, not candidates. Whatever produced
        // them -- the walk of the tree, the language server, the search of
        // the open file -- was given the query and has already said which
        // lines have it in them. Asking again here is a second matcher over
        // the first, and a second matcher can only disagree: it did, and
        // what it disagreed about it threw away.
        //
        // So a search is the empty query's case. The order is the producer's
        // too, which for a walk is the order of the tree and for a file is
        // the order of its lines -- both of which mean something, where a
        // match score here would not.
        if self.query.is_empty() || self.searching {
            if !matches!(candidates, Candidates::From(_)) {
                self.matched.clear();
            }
            // An empty query keeps the given order, which is the order the
            // caller thought worth showing: recent buffers, the command table.
            self.matched.extend(
                self.items
                    .iter()
                    .enumerate()
                    .skip(from)
                    .filter(|(_, item)| showing(item))
                    .map(|(index, _)| (index, 0)),
            );
        } else {
            let pattern = Pattern::parse(
                &self.query.said(),
                CaseMatching::Smart,
                Normalization::Smart,
            );
            // Whether the last row a query could be about matched, for the
            // rows that hang under it.
            //
            // A pass that starts part way along carries the flag the pass
            // before it ended on, or a batch landing between a commit and
            // its files would drop the files.
            let mut parent = self.parent_matched;
            let scan: Vec<usize> = match candidates {
                Candidates::Survivors => {
                    let mut kept: Vec<usize> =
                        self.matched.iter().map(|(index, _)| *index).collect();
                    // In the order the items are in, which is not the order
                    // they are kept in: a ranked list holds them by score.
                    // Worth nothing that was measured -- a hundred and
                    // twenty-six thousand rows cost the same either way --
                    // and kept because the rows come out of here in the
                    // order they go in everywhere else, which is one less
                    // thing to be surprised by.
                    kept.sort_unstable();
                    kept
                }
                _ => (from..self.items.len()).collect(),
            };
            // Big, and flat, and the loop is the wrong place for it. A
            // nested list is not sent away because what keeps a child is
            // whether its parent matched, which is a fact about the order
            // the rows are in rather than about any row on its own -- and
            // a list with tabs filters as it goes, which is the same kind
            // of fact. Both of those are small.
            //
            // The number is not a limit on anything. It is where handing
            // the work to another thread stops costing more than doing it:
            // a scan of a few thousand rows is under a millisecond, and a
            // frame of lag for that would be the machinery making the
            // thing it was added to fix.
            if candidates.may_travel() && scan.len() >= SENT_AWAY && !self.nests && !self.scopes {
                self.scans += 1;
                self.awaiting = Some(self.scans);
                self.wanted = Some(Scan {
                    generation: self.scans,
                    query: self.query.said(),
                    items: Arc::clone(&self.items),
                    candidates: scan,
                    listings: self.listings,
                });
                // What is on screen stays on screen, which is why nothing
                // above cleared it: the rows the reader is looking at are
                // the rows enter acts on, and a list that emptied itself
                // while it thought would be Obelus saying there is nothing,
                // about a question it has not answered yet.
                //
                // Only where they are rows of *this* list. `replace` and
                // `relist` hand over different ones, and positions kept
                // across that are numbers about a list that is gone.
                if self.matched_for != self.listings {
                    self.matched.clear();
                    self.settled();
                }
                self.filtered_on = self.query.said();
                return;
            }
            // Doing it here after all, so what was on screen goes now.
            if !matches!(candidates, Candidates::From(_)) {
                self.matched.clear();
            }
            for index in scan {
                let item = &self.items[index];
                if !showing(item) {
                    continue;
                }
                if self.nests && item.depth > 0 {
                    if parent {
                        self.matched.push((index, 0));
                    }
                    continue;
                }
                let haystack = Utf32Str::new(&item.label, &mut self.haystack);
                let score = pattern.score(haystack, &mut self.matcher);
                parent = score.is_some();
                if let Some(score) = score {
                    self.matched.push((index, score));
                }
            }
            self.parent_matched = parent;
            // Best first, and ties by the original order so the list does not
            // reshuffle as more items arrive. A list whose order is itself an
            // answer keeps it: the scoring above has already said which rows
            // match, which is all such a list wants from a query.
            if !self.ordered {
                self.matched
                    .sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));
            }
        }

        // What this pass answered for, so the next one can tell a query that
        // grew from a query that changed.
        self.filtered_on = self.query.said();
        self.settled();
    }

    /// What every pass ends with, wherever the scoring was done.
    ///
    /// One piece rather than two, because a list that came back from a
    /// worker and a list worked out here are the same list: two endings
    /// would be somewhere for the selection to be put back differently.
    fn settled(&mut self) {
        self.matched_for = self.listings;
        self.window.set_count(self.matched.len());
        // A query can narrow the list to rows that cannot be chosen, or move
        // one under the selection: whatever else happens, the selection is
        // on a row a reader can press Enter on if there is one.
        //
        // Only where that moves it: rows arrive here from a walk still
        // under way, and a window the reader has dragged would otherwise
        // be put back on the selection by every batch.
        if let Some(choosable) = self.choosable(self.window.focus(), true)
            && choosable != self.window.focus()
        {
            self.window.set_focus(choosable);
        }

        // Last, because it is the strongest claim about which row to start
        if let Some(label) = self.prefer.as_deref()
            && let Some(row) = self
                .matched
                .iter()
                .position(|(index, _)| self.items[*index].label == label)
            && self.can_choose(row)
            && row != self.window.focus()
        {
            self.window.set_focus(row);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A batch landing in a list the reader has dragged leaves it where
    /// they dragged it.
    ///
    /// Deliberate breaks, each failing this on its own: choose the row
    /// again in `relist` whether or not the batch moved it; and drop the
    /// "only where that moves it" from `settled`. Either one puts the
    /// window back on the selection with every batch of a history still
    /// arriving, so the drag lasts until the next one.
    #[test]
    fn a_batch_arriving_leaves_a_dragged_list_where_it_was() {
        let rows = |count: usize| (0..count).map(|n| named(&n.to_string())).collect();
        let mut picker = Picker::new(rows(30), PickerLayout::Compact { rows: 10 });
        picker.refresh_indices(10, 40);
        picker.drag_to(15);
        picker.refresh_indices(10, 40);
        assert_eq!(picker.window().top(), 15, "the drag did not move the list");

        picker.relist(rows(40));
        picker.refresh_indices(10, 40);
        assert_eq!(picker.window().top(), 15, "the batch undid the drag");
        assert_eq!(picker.selected(), 0, "the batch chose something");
    }

    /// A list that is only read is moved, not chosen in.
    ///
    /// The keys that walk a selection move the rows, enter does nothing, a
    /// click puts the reader on no row, and the wheel moves the rows too --
    /// what went wrong on the way up is such a list, and its rows go
    /// nowhere.
    ///
    /// Deliberate break: taking the `self.reads` arms out of `handle_key`
    /// (the first two assertions: the arrow moves a selection inside the
    /// window, which does not move, and enter accepts the row); taking the
    /// early return out of `select_row` (the third); and out of
    /// `move_selection_by` (the fourth).
    #[test]
    fn a_list_that_is_only_read_moves_its_rows_and_chooses_nothing() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let rows = (0..30).map(|n| named(&n.to_string())).collect();
        let mut picker = Picker::new(rows, PickerLayout::Compact { rows: 10 });
        picker.only_read();
        let bare = |code| KeyEvent::new(code, KeyModifiers::NONE);

        picker.handle_key(&bare(KeyCode::Down), 10);
        picker.refresh_indices(10, 40);
        assert_eq!(picker.window().top(), 1, "the arrow did not move the rows");

        assert!(
            matches!(
                picker.handle_key(&bare(KeyCode::Enter), 10),
                PickerOutcome::Consumed
            ),
            "enter chose a row of a list that is only read"
        );

        picker.select_row(7);
        assert_eq!(picker.selected(), 1, "a click put the reader on a row");

        picker.move_selection_by(3);
        picker.refresh_indices(10, 40);
        assert_eq!(picker.window().top(), 4, "the wheel did not move the rows");
    }

    /// And it is not typed at: a letter is not its own, and a paste is
    /// typing all at once.
    ///
    /// Deliberate break: taking the `_ if self.reads` arm out of
    /// `handle_key` (the first two assertions: the letter is taken and the
    /// rows narrow to it), and the early return out of `put_in_query` (the
    /// third).
    #[test]
    fn a_list_that_is_only_read_is_not_typed_at() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let rows = (0..30).map(|n| named(&n.to_string())).collect();
        let mut picker = Picker::new(rows, PickerLayout::Compact { rows: 10 });
        picker.only_read();

        assert!(
            matches!(
                picker.handle_key(&KeyEvent::new(KeyCode::Char('7'), KeyModifiers::NONE), 10),
                PickerOutcome::Ignored
            ),
            "a letter was taken by a list that is only read"
        );
        assert_eq!(picker.match_count(), 30, "a letter narrowed the rows");
        picker.put_in_query("7");
        assert_eq!(picker.match_count(), 30, "a paste narrowed the rows");
    }

    /// A compact list is as tall as it has rows, up to what it asked for.
    /// It is drawn over code that is still being read, so a row it takes
    /// and does not use is a row of that code covered by nothing.
    ///
    /// Tabs do not change that. Height used to be inferred from having
    /// them, which was one list's preference wearing another list's
    /// property -- so a list with tabs that has not asked to stay still
    /// closes up like any other.
    #[test]
    fn a_compact_list_shrinks_to_what_matches_tabs_or_not() {
        let plain = |count: usize| {
            let rows = (0..count).map(|n| named(&n.to_string())).collect();
            Picker::new(rows, PickerLayout::Compact { rows: 10 })
        };

        assert_eq!(
            plain(3).visible_rows(20, 40),
            3,
            "three rows took not three"
        );
        assert_eq!(plain(30).visible_rows(20, 40), 10, "it went past its most");
        assert_eq!(
            plain(0).visible_rows(20, 40),
            1,
            "nothing to say needs a row to say it in"
        );

        let mut tabbed = plain(3);
        tabbed.with_tabs(&["one", "two"]);
        assert_eq!(
            tabbed.visible_rows(20, 40),
            3 + tabbed.tab_rows(),
            "a list with tabs kept room it had nothing to put in"
        );
    }

    /// Unless it says otherwise. A list read down as much as it is typed at
    /// would move the row under the reader's eye between one letter and the
    /// next.
    #[test]
    fn a_list_that_asked_to_keep_its_height_keeps_it() {
        let rows = (0..3).map(|n| named(&n.to_string())).collect();
        let mut picker = Picker::new(rows, PickerLayout::Compact { rows: 10 });
        picker.keeps_height();
        assert_eq!(
            picker.visible_rows(20, 40),
            10,
            "it closed up on a list that asked not to"
        );

        // Including when nothing matches at all, which is the moment the
        // steadiness is for: the block does not blink out from under a
        // reader who typed one letter too many.
        picker.set_query("nothing here matches this");
        assert_eq!(picker.match_count(), 0, "the query matched something");
        assert_eq!(
            picker.visible_rows(20, 40),
            10,
            "it moved when the list emptied"
        );
    }

    /// A list that asked for its details in one column puts it after the
    /// widest label of the tab showing, and keeps it there as it is typed
    /// at.
    ///
    /// Broken twice: measuring every row rather than the tab's left the
    /// second tab's column at the first tab's widest, and measuring the rows
    /// that match moved the column when a query left only the short one.
    #[test]
    fn details_line_up_after_the_widest_label_in_the_tab() {
        let rows = vec![
            PickerItem {
                tab: Some(1),
                ..named("open-changed-file")
            },
            PickerItem {
                tab: Some(1),
                ..named("save")
            },
            PickerItem {
                tab: Some(2),
                ..named("copy")
            },
            PickerItem {
                tab: Some(2),
                ..named("paste-it")
            },
        ];
        let mut picker = Picker::new(rows, PickerLayout::Compact { rows: 10 });
        picker.with_tabs(&["All", "Files", "Edit"]);
        assert_eq!(
            picker.detail_column(),
            None,
            "a list that did not ask got one"
        );

        picker.aligns_details();
        assert_eq!(picker.detail_column(), Some(17), "not the widest of all");
        picker.go_to_tab(2);
        assert_eq!(
            picker.detail_column(),
            Some(8),
            "not the widest of this tab"
        );

        picker.go_to_tab(1);
        picker.set_query("save");
        assert_eq!(picker.match_count(), 1, "the query matched something else");
        assert_eq!(
            picker.detail_column(),
            Some(17),
            "it moved as it was typed at"
        );
    }

    /// A list whose rows wrap, of runs headed by `section`.
    fn wrapping(rows: &[(&str, &str)]) -> Picker {
        let items = rows
            .iter()
            .map(|(label, section)| PickerItem {
                section: Some((*section).to_string()),
                prose: true,
                ..named(label)
            })
            .collect();
        let mut picker = Picker::new(items, PickerLayout::Compact { rows: 24 });
        picker.wraps(None);
        picker.keeps_order(true);
        picker
    }

    /// A list whose rows wrap is as tall as all of it, not as what the
    /// query leaves: rows this tall resizing the block on every letter
    /// would move the row under the reader's eye by several rows at once.
    ///
    /// Broken by counting the rows that match in `wrapped_height` rather
    /// than every row: the list went from nine rows to one on a query.
    #[test]
    fn a_list_whose_rows_wrap_does_not_resize_as_it_is_typed_at() {
        let mut picker = wrapping(&[
            ("the margin lies about a file nobody has touched", "Today"),
            ("count the lines", "Today"),
            ("an older one", "Earlier"),
        ]);
        // Twenty columns: the first row is three rows of words, then a
        // blank and one, then a blank, a heading, a blank and one -- and a
        // heading and its blank over the first.
        let tall = picker.visible_rows(40, 20);
        assert_eq!(
            tall,
            2 + 3 + 1 + 1 + 3 + 1,
            "the list is not as tall as its rows"
        );
        picker.set_query("count");
        assert_eq!(picker.match_count(), 1, "the query matched something else");
        assert_eq!(picker.visible_rows(40, 20), tall, "it resized on a query");
    }

    /// A heading goes over the first row of its run among the rows that
    /// *match*, so a run the query has emptied takes its heading with it
    /// and a run it has cut into keeps one.
    ///
    /// Broken by asking `Above::of` about the row before in the whole list
    /// rather than among the rows matching: the query left "count the
    /// lines" at the top of its run with no heading over it.
    #[test]
    fn a_heading_goes_with_its_run() {
        let mut picker = wrapping(&[
            ("the margin lies", "Today"),
            ("count the lines", "Today"),
            ("an older one", "Earlier"),
        ]);
        let headings = |picker: &Picker| -> Vec<String> {
            (0..picker.match_count())
                .filter(|at| picker.above(*at).headed)
                .filter_map(|at| picker.wrapped_row(at, 40)?.0.section.clone())
                .collect()
        };
        assert_eq!(headings(&picker), ["Today", "Earlier"]);

        picker.set_query("older");
        assert_eq!(
            headings(&picker),
            ["Earlier"],
            "an empty run kept its heading"
        );

        picker.set_query("count");
        assert_eq!(
            headings(&picker),
            ["Today"],
            "a run cut into lost its heading"
        );
    }

    /// A page of rows that wrap is as many rows as were on screen, not as
    /// many screen rows: those are ten times fewer rows than a page of
    /// screen rows would step.
    ///
    /// Broken by paging by `page` whatever the list is: the selection went
    /// to the eleventh row, when four had been on screen.
    #[test]
    fn a_page_of_rows_that_wrap_is_the_rows_that_were_on_screen() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let names: Vec<String> = (0..20).map(|n| format!("row {n}")).collect();
        let rows: Vec<(&str, &str)> = names.iter().map(|name| (name.as_str(), "Today")).collect();
        let mut picker = wrapping(&rows);
        // A heading and its blank, then a row, and a blank and a row after
        // that: 2 + 1 + 2 + 2 + 2 is nine of ten rows, which is four rows.
        picker.refresh_indices(10, 40);
        picker.handle_key(&KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE), 10);
        assert_eq!(
            picker.selected(),
            4,
            "a page did not move by the rows shown"
        );
    }

    /// One row, for the tests above.
    pub(super) fn named(label: &str) -> PickerItem {
        PickerItem {
            prose: false,
            marker: None,
            icon: None,
            label: label.to_string(),
            version: None,
            detail: None,
            trailing: None,
            changed: None,
            value: PickerValue::Nothing,
            enabled: true,
            colours: None,
            status: None,
            depth: 0,
            opens: None,
            kind: None,
            tab: None,
            section: None,
        }
    }

    /// A list can say what it is about, and what it says takes room from
    /// its rows rather than from the screen around it.
    ///
    /// Nothing in Obelus sets this today -- an agent's question moved to a
    /// card of its own, which is where prose above answers belongs when the
    /// answers are the whole point. It is kept because the next list that
    /// is an answer to something the reader did not start will want it, and
    /// a feature nobody exercises is a feature that has quietly stopped
    /// working by then.
    #[test]
    fn what_a_list_is_about_takes_room_from_its_rows() {
        let rows = ["one", "two", "three"]
            .into_iter()
            .map(|name| PickerItem {
                prose: false,
                marker: None,
                icon: None,
                label: name.to_string(),
                version: None,
                detail: None,
                trailing: None,
                changed: None,
                value: PickerValue::Nothing,
                enabled: true,
                colours: None,
                status: None,
                depth: 0,
                opens: None,
                kind: None,
                tab: None,
                section: None,
            })
            .collect();
        let mut picker = Picker::new(rows, PickerLayout::Compact { rows: 10 });
        let width = 20;
        let plain = picker.visible_rows(12, width);
        assert_eq!(picker.about_rows(width), 0, "a list with nothing to say");

        picker.about("a sentence long enough to want two rows of a narrow list");
        assert_eq!(
            picker.what_about(),
            Some("a sentence long enough to want two rows of a narrow list")
        );
        // The words, wrapped, and a rule under them: what makes the rows
        // under it read as answers rather than as more of the sentence.
        let about = picker.about_rows(width);
        assert!(about > 2, "the prose was not wrapped: {about}");
        assert_eq!(
            picker.visible_rows(12, width),
            plain + about,
            "the prose did not take its room from the list"
        );
    }
}

#[cfg(test)]
mod travelling {
    use super::{tests::named, *};

    /// A list of `rows`, every one of which matches `row`.
    fn rows(count: usize) -> Vec<PickerItem> {
        (0..count).map(|n| named(&format!("row-{n}"))).collect()
    }

    /// A scoring that goes away and comes back matches what it should.
    ///
    /// The happy path, which nothing held until a picker that handed every
    /// scoring out and accepted none of the answers shipped: it matched
    /// nothing at all, and every test here passed, because each of them was
    /// about an answer being *refused*.
    ///
    /// Deliberate break: refuse the answer in `scan_arrived` whatever it
    /// says. The list keeps whatever was on screen when the query changed
    /// and never narrows again.
    #[test]
    fn a_scoring_that_comes_back_is_the_list() {
        let mut picker = Picker::new(rows(SENT_AWAY * 2), PickerLayout::FullArea);
        picker.set_query("row-7");
        let asked = picker.wanted_scan().expect("a scoring was asked for");
        assert!(picker.is_matching());

        picker.scan_arrived(scan(&asked));
        assert!(!picker.is_matching(), "it is still waiting");
        let found = picker.matches().count();
        assert!(found > 0, "a query that matches rows matched none");
        assert!(found < SENT_AWAY * 2, "it narrowed nothing: {found}");
        assert!(
            picker.matches().all(|item| item.label.contains('7')),
            "a row that does not hold the query"
        );
    }

    /// A list being matched says so where a list being filled says so.
    ///
    /// The reader typed and the rows did not move. Without this there is
    /// nothing on screen that says why, which is the one thing a list that
    /// has stopped answering must not leave them to guess.
    ///
    /// Deliberate break: answer `self.filling` whatever `awaiting` says.
    /// The note goes back to being about a walk alone, and a list waiting
    /// on a scoring looks like a list that has decided nothing matches.
    #[test]
    fn a_list_being_matched_says_so() {
        let mut picker = Picker::new(rows(SENT_AWAY * 2), PickerLayout::FullArea);
        assert_eq!(picker.is_filling(), None, "nothing is happening yet");

        picker.set_query("row-7");
        assert_eq!(
            picker.is_filling(),
            Some("Matching\u{2026}"),
            "a list that sent its scoring away says nothing about it"
        );

        let asked = picker.wanted_scan().expect("a scoring was asked for");
        picker.scan_arrived(scan(&asked));
        assert_eq!(picker.is_filling(), None, "it is still saying so");
    }

    /// And a batch landing while one is out does not throw it away.
    ///
    /// Appending leaves every position that already existed pointing at the
    /// row it always did, so the answer is still about rows this list
    /// holds. Refusing it there is what a picker does when it identifies
    /// the rows by the `Arc` they are in: the walk copies them out from
    /// under the scoring on the very next batch, every answer looks stale,
    /// and nothing is ever matched.
    ///
    /// Deliberate break: count `listings` up in `extend` as well.
    #[test]
    fn a_batch_while_a_scoring_is_out_does_not_refuse_it() {
        let mut picker = Picker::new(rows(SENT_AWAY * 2), PickerLayout::FullArea);
        picker.set_query("row-7");
        let asked = picker.wanted_scan().expect("a scoring was asked for");

        picker.extend(vec![named("row-70000")]);
        picker.scan_arrived(scan(&asked));

        assert!(!picker.is_matching(), "the answer was thrown away");
        assert!(picker.matches().count() > 0, "it matched nothing");
    }

    /// A position is about a list, and the list can be swapped while a
    /// scoring of it is still out.
    ///
    /// Both lists are big enough to be scored somewhere else and the second
    /// is the shorter, which is the whole of the setup: the rows kept on
    /// screen are positions in the list that has gone, and the highest of
    /// them is past the end of the one that replaced it.
    ///
    /// Deliberate break: drop the `Arc::ptr_eq` guard in `filter`. The next
    /// thing to read the list -- `matches`, on the very next frame -- walks
    /// off the end of it and panics, which is what it did.
    #[test]
    fn rows_are_not_kept_across_a_list_that_was_swapped() {
        let long = SENT_AWAY * 2;
        let short = SENT_AWAY + 1;
        let mut picker = Picker::new(rows(long), PickerLayout::FullArea);
        picker.set_query("row");
        assert!(picker.is_matching(), "a list this big was scored here");

        picker.replace(rows(short));
        assert!(picker.is_matching(), "and so was the one that replaced it");
        // Every row it offers has to be a row it holds.
        assert!(
            picker.matches().count() <= short,
            "more rows than the list has"
        );
    }

    /// And an answer about rows the picker no longer holds is not drawn.
    ///
    /// Deliberate break: drop the `Arc::ptr_eq` in `scan_arrived`. The
    /// answer is about the rows it was handed, the list has been relisted
    /// shorter since, and its positions are about neither of them.
    #[test]
    fn an_answer_about_rows_that_moved_is_not_drawn() {
        let mut picker = Picker::new(rows(SENT_AWAY * 2), PickerLayout::FullArea);
        picker.set_query("row");
        let asked = picker.wanted_scan().expect("a scoring was asked for");

        picker.relist(rows(1));
        picker.scan_arrived(scan(&asked));

        assert!(picker.matches().count() <= 1, "more rows than the list has");
    }
}

/// What a list of a whole project costs, which is the only size that ever
/// showed any of this up.
///
/// A hundred and twenty-six thousand rows, which is what `ignored_files`
/// makes of this repository once `target` has been built in a few times.
/// Every number in the comments around `filter` was taken here, and taking
/// them needs no walk, no application and no disk: the rows are made up,
/// and what is being measured is the matching.
///
/// `#[ignore]`d because it is a stopwatch and asserts almost nothing. Run
/// it deliberately:
///
/// ```text
/// cargo test --release -p obelus-component --lib scale -- --ignored --nocapture
/// ```
///
/// Release, because the answer in a debug build is the answer about
/// nucleo at `opt-level = 0` rather than about anything written here.
#[cfg(test)]
mod scale {
    use super::{tests::named, *};

    fn paths(count: usize) -> Vec<PickerItem> {
        (0..count)
            .map(|n| {
                named(&format!(
                    "target/debug/deps/obelus_app-{n:06}/build/some-crate-{n}/out/render.rs"
                ))
            })
            .collect()
    }

    #[test]
    #[ignore = "a stopwatch"]
    fn how_long_a_hundred_thousand_rows_take() {
        let count = 126_000;
        let built = std::time::Instant::now();
        let items = paths(count);
        println!("building {count} rows: {:?}", built.elapsed());

        // The walk's own shape: batches of five hundred into a list.
        let batched = std::time::Instant::now();
        let mut picker = Picker::new(Vec::new(), PickerLayout::FullArea);
        for batch in items.chunks(512) {
            picker.extend(batch.to_vec());
        }
        println!("the walk, in batches of 512: {:?}", batched.elapsed());
        assert_eq!(picker.match_count(), count);

        // And then four of the same letter, which is what was reported.
        for press in 1..=4 {
            let typed = std::time::Instant::now();
            picker.handle_key(
                &crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Char('r'),
                    crossterm::event::KeyModifiers::NONE,
                ),
                40,
            );
            println!(
                "'r' x{press}: {:?}, {} rows left",
                typed.elapsed(),
                picker.match_count()
            );
        }

        // The control. `set_query` takes the full-scan path, so this is the
        // same four-character pattern over the same hundred and twenty-six
        // thousand rows with no narrowing at all: whatever it costs is the
        // pattern's, not the narrowing's.
        let whole = std::time::Instant::now();
        picker.set_query("rrrr");
        println!(
            "a full scan for `rrrr`: {:?}, {} rows",
            whole.elapsed(),
            picker.match_count()
        );

        // And one character, scanned whole, for the other end of it.
        let one = std::time::Instant::now();
        picker.set_query("r");
        println!("a full scan for `r`: {:?}", one.elapsed());
    }
}
