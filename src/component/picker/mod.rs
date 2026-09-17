//! The one list-with-a-prompt, instantiated four times.
//!
//! Files, buffers, themes and the command palette differ in what they list and
//! how tall they are, and in nothing else. Later they are joined by symbols,
//! references and commits, which is why this is a component rather than four
//! screens.

pub mod files;

/// How many rows of an agent's own words a list will carry.
///
/// Five: enough for a sentence about a command and its arguments, and few
/// enough that the list it is about is still the thing on screen.
const MOST_ABOUT: u16 = 5;

use std::path::PathBuf;

use nucleo_matcher::{
    Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};

use crate::{
    buffer::DocumentId,
    command::Command,
    component::{
        field::Field,
        window::{Move, Window, Wrap},
    },
    question::Question,
};

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
    /// Switch to something already open.
    Document(DocumentId),
    /// Switch theme.
    /// A theme, by the name it answers to.
    ///
    /// The name rather than the colours: a row of a list should not be
    /// carrying thirty-nine of them, and which colours a name stands for is
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
    /// One of the ways out of a question obelus stopped to ask.
    Answer(crate::question::Answer),
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
            Self::All => "all",
            Self::Changed => "changed",
        }
    }
}

/// One run of a row's characters, and what to draw them in.
///
/// Char offsets into the label, not bytes and not screen columns: the label
/// is what the row draws, and a run is a claim about its characters.
pub type Colouring = (u16, u16, crate::theme::SyntaxKind);

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
    /// Work that obelus has not written.
    Unwritten,
    /// Something is happening in it that nobody is watching.
    Working,
    /// And something in it is waiting on the reader.
    Waiting,
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
    /// What git says about the file the row names, if it says anything.
    ///
    /// Colours the row. A list of a project's files is mostly a list of
    /// files nobody has touched, and the few that have been are what a
    /// reader is usually looking for.
    pub status: Option<crate::git::FileStatus>,
    /// Whether the row can be chosen.
    ///
    /// A row that cannot is drawn dim and the selection walks past it. Shown
    /// rather than left out, because a list that hides what it cannot do
    /// cannot be learned from: a reader who never sees `show-change` does not
    /// find out that obelus has it. What they see instead is that it is
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
    pub kind: Option<crate::theme::SyntaxKind>,
    /// Which tab the row belongs to, if the picker has tabs.
    ///
    /// An index into the picker's own tab names. `None` means every tab,
    /// which is what a picker without tabs gives all of its rows.
    pub tab: Option<usize>,
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

/// What a key did.
#[derive(Debug)]
pub enum PickerOutcome {
    /// Not a key the picker knows; try the key table.
    Ignored,
    /// Handled. Redraw.
    Consumed,
    /// The user chose something.
    Accepted(PickerValue),
    /// The user gave up.
    Cancelled,
}

/// A prompt and a filtered list.
pub struct Picker {
    items: Vec<PickerItem>,
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
    /// The same window every list in obelus has, and the reason it is state
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
    /// Whether the list holds the height it asked for rather than shrinking
    /// to the rows that match.
    steady: bool,
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
    /// being nothing to list, and so wins over "no match".
    explains: bool,
    /// A question this list is the answer to, shown in front of the prompt.
    question: Option<String>,
    /// What to say when there is nothing to list.
    ///
    /// Per picker, because the reason differs: an empty file list and an
    /// empty buffer list are different facts about the world. An empty
    /// region says only that something is broken.
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
    /// How this search is looking, where the question arises.
    ///
    /// `None` where it does not: a list that is not a search, and the
    /// symbols tab, whose rows come from a language server that did its own
    /// matching and would not know what to do with a pattern of ours. The
    /// foot greys the keys there rather than dropping them, so it does not
    /// change height as the reader steps between tabs.
    looking: Option<crate::search::Looking>,
    /// Whether this search is offering names from outside the project,
    /// where that is a question about it.
    ///
    /// `None` on the tabs where it is not one: a walk of this tree and a
    /// search of the open file are inside the project by construction, and
    /// only a language server has an index that reaches past it.
    outside: Option<bool>,
    /// Whether this list is offering the files a tree ignores, where that
    /// is a question about it at all.
    ///
    /// `None` where the key means nothing: the changed files come from git
    /// rather than from a walk, and what a tree ignores is not part of that
    /// answer either way. Greyed at the foot rather than dropped from it, so
    /// the list does not change height as the reader steps between tabs.
    ///
    /// Set by whoever filled the list, because the walk and the setting
    /// behind it are both theirs -- what is the list's is only that the foot
    /// is drawn from it, and a foot that had to guess would guess wrong on
    /// the first frame after the key.
    ignored: Option<bool>,
    /// Scratch for `Utf32Str::new`, which needs somewhere to put a converted
    /// haystack.
    haystack: Vec<char>,
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
        let mut picker = Self {
            items,
            query: Field::new(),
            matched: Vec::new(),
            indices: Vec::new(),
            window: Window::new(),
            outline: Option::None,
            tabs: Vec::new(),
            about: None,
            tab: 0,
            scopes: false,
            searching: false,
            listing: false,
            steady: false,
            previews: false,
            explains: false,
            marked: false,
            question: None,
            empty: "nothing to choose from".to_string(),
            prefer: None,
            nests: false,
            filling: None,
            ordered: false,
            footed: false,
            keys: false,
            looking: None,
            outside: None,
            ignored: None,
            layout,
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
            detail: about,
            trailing: None,
            changed: None,
            value: PickerValue::Answer(answer),
            enabled: true,
            colours: None,
            status: None,
            depth: 0,
            kind: None,
            tab: None,
        };
        let items: Vec<_> = question
            .ways()
            .iter()
            .map(|way| row(way.label.clone(), way.detail.clone(), way.answer))
            .chain(std::iter::once(row(
                "cancel".to_string(),
                None,
                crate::question::Answer::Cancel,
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
        picker
    }

    /// Gives the picker a row of tabs.
    ///
    /// `names` are the groups; the tab drawn first is "all", which the picker
    /// adds itself, so every row is reachable by walking them and so is a row
    /// belonging to no group.
    pub fn with_tabs(&mut self, names: &[&str]) {
        self.tabs = std::iter::once("all".to_string())
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

    /// Says the list is still being filled, and what to show while it is.
    ///
    /// `None` once it is not.
    pub fn filling(&mut self, note: Option<String>) {
        self.filling = note;
    }

    /// What the list is still waiting for, if it is waiting.
    #[must_use]
    pub fn is_filling(&self) -> Option<&str> {
        self.filling.as_deref()
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

    /// Whether it does.
    #[must_use]
    pub const fn shows_previews(&self) -> bool {
        self.previews
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
    pub const fn looking_how(&mut self, how: Option<crate::search::Looking>) {
        self.looking = how;
    }

    /// And what it was told.
    #[must_use]
    pub const fn looks_how(&self) -> Option<crate::search::Looking> {
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

    /// Says whether this list is offering the files a tree ignores, or that
    /// the question does not arise here.
    pub const fn offering_ignored(&mut self, offering: Option<bool>) {
        self.ignored = offering;
    }

    /// And what it was told.
    #[must_use]
    pub const fn offers_ignored(&self) -> Option<bool> {
        self.ignored
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
    pub fn about(&mut self, about: &str) {
        self.about = Some(about.to_string());
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
        let rows = u16::try_from(crate::text::wrapped(about, inside).len()).unwrap_or(MOST_ABOUT);
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
        self.items = items;
        self.window.set_focus(0);
        self.refilter();
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
        self.items = items;
        self.refilter();
        self.select_row(selected);
    }

    /// Moves the selection by rows, stopping at the ends.
    ///
    /// For the wheel, which is not the arrow keys: rolling past the end of a
    /// list and reappearing at the top is a jump nobody asked for, and a
    /// wheel is rolled without looking.
    pub fn move_selection_by(&mut self, rows: isize) {
        self.move_selection(rows, Wrap::No);
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
    /// For a search: with a query in the prompt and no rows, "no match" is
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
            "no match"
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
        self.items.get_mut(index)
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
        self.query.put(said);
        self.refilter();
    }

    /// Puts the query's caret where a cell of its row is.
    ///
    /// `cell` is counted from the first character of the query: what is
    /// drawn in front of it belongs to whoever draws it.
    pub fn place_in_query(&mut self, cell: u16, extend: bool) {
        self.query.place_at_cell(cell, extend);
    }

    /// Takes hold of the word under the caret, or of the whole query.
    pub fn hold_in_query(&mut self, all: bool) {
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
            let Some(run) = crate::search::Needle::new(said, how).found_in(label) else {
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
    /// point.
    pub fn refresh_indices(&mut self, height: u16) {
        // The window first: which rows are about to be drawn is the question
        // the matched characters are worked out for, and the height is only
        // known here.
        self.window.settle(height);

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

    /// Adds more items to a list that is still being gathered.
    ///
    /// The file walk arrives in batches, and the user is already typing while
    /// it does.
    pub fn extend(&mut self, items: impl IntoIterator<Item = PickerItem>) {
        self.items.extend(items);
        self.refilter();
    }

    /// Handles a key.
    ///
    /// The picker takes everything it recognizes, including the printable
    /// characters that go into the prompt, which is why these are not commands:
    /// a command per character would be the logical end of that road.
    /// `page` is how many rows are on screen, which only the layout knows.
    pub fn handle_key(&mut self, key: &crossterm::event::KeyEvent, page: u16) -> PickerOutcome {
        use crossterm::event::{KeyCode, KeyModifiers};

        let page = isize::try_from(page.max(1)).unwrap_or(isize::MAX);
        // A key carrying a modifier this branch does not name falls through,
        // the same rule the key table and the editor's motions follow.
        // Without it `ctrl+pageup` pages the list, which is a different thing
        // from what it should do.
        let Some(modifiers) = crate::keymap::modifiers_of(key) else {
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
            // the one completion obelus accepts with a key accepts with
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
            KeyCode::Esc if bare => PickerOutcome::Cancelled,
            KeyCode::F(1) if bare && self.footed => {
                self.keys = !self.keys;
                PickerOutcome::Consumed
            }
            KeyCode::Enter if bare => self
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
            // every other text obelus holds. The duplicate was what made
            // them free to give away.
            code if (bare || (control && paging))
                && !(bare && matches!(code, KeyCode::Home | KeyCode::End))
                && let Some(movement) = Move::of(code) =>
            {
                match movement {
                    Move::Up => self.move_selection(-1, Wrap::Yes),
                    Move::Down => self.move_selection(1, Wrap::Yes),
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
            // Everything the list did not want goes to the query, which
            // is a line with a caret in it and takes the keys a line takes:
            // the arrows, the words, what is held, what is typed. A key it
            // has no use for it refuses, and that is how `ctrl+q` still
            // reaches the key table and leaves obelus from in here.
            _ => match self.query.handle_key(key) {
                true => {
                    self.refilter();
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

    /// Selects a row outright.
    /// Puts the selection on a row, for a caller that has just replaced the
    /// list under it.
    ///
    /// Opening a commit's files puts rows below the row the key was pressed
    /// on; a selection that jumped to the top would leave the reader
    /// somewhere they did not ask to be.
    pub fn select_row(&mut self, row: usize) {
        self.select(row);
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

    fn refilter(&mut self) {
        self.matched.clear();
        // One more pass over a list this function already walks whole, and
        // the only place that knows about every row rather than the visible
        // ones.
        self.marked = self.items.iter().any(|item| item.marker.is_some());

        // The first tab is every row; any other one is its own. Scope tabs
        // do not filter at all -- every row in the list belongs to the scope
        // that fetched it.
        let tab = self.tab;
        let scopes = self.scopes;
        let showing = move |item: &PickerItem| match (scopes, tab, item.tab) {
            (true, _, _) | (_, 0, _) | (_, _, None) => true,
            (_, tab, Some(of)) => tab == of,
        };

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
            // An empty query keeps the given order, which is the order the
            // caller thought worth showing: recent buffers, the command table.
            self.matched.extend(
                self.items
                    .iter()
                    .enumerate()
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
            let mut parent = false;
            for (index, item) in self.items.iter().enumerate() {
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
            // Best first, and ties by the original order so the list does not
            // reshuffle as more items arrive. A list whose order is itself an
            // answer keeps it: the scoring above has already said which rows
            // match, which is all such a list wants from a query.
            if !self.ordered {
                self.matched
                    .sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));
            }
        }

        self.window.set_count(self.matched.len());
        // A query can narrow the list to rows that cannot be chosen, or move
        // one under the selection: whatever else happens, the selection is
        // on a row a reader can press Enter on if there is one.
        if let Some(choosable) = self.choosable(self.window.focus(), true) {
            self.window.set_focus(choosable);
        }

        // Last, because it is the strongest claim about which row to start
        // on: it beats both the order the items came in and where the
        // selection happened to be before the list changed under it.
        if let Some(label) = self.prefer.as_deref()
            && let Some(row) = self
                .matched
                .iter()
                .position(|(index, _)| self.items[*index].label == label)
            && self.can_choose(row)
        {
            self.window.set_focus(row);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// One row, for the tests above.
    fn named(label: &str) -> PickerItem {
        PickerItem {
            prose: false,
            marker: None,
            icon: None,
            label: label.to_string(),
            detail: None,
            trailing: None,
            changed: None,
            value: PickerValue::Nothing,
            enabled: true,
            colours: None,
            status: None,
            depth: 0,
            kind: None,
            tab: None,
        }
    }

    /// A list can say what it is about, and what it says takes room from
    /// its rows rather than from the screen around it.
    ///
    /// Nothing in obelus sets this today -- an agent's question moved to a
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
                detail: None,
                trailing: None,
                changed: None,
                value: PickerValue::Nothing,
                enabled: true,
                colours: None,
                status: None,
                depth: 0,
                kind: None,
                tab: None,
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
