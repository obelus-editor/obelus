//! The one list-with-a-prompt, instantiated four times.
//!
//! Files, buffers, themes and the command palette differ in what they list and
//! how tall they are, and in nothing else. Later they are joined by symbols,
//! references and commits, which is why this is a component rather than four
//! screens.

pub mod files;

use std::path::PathBuf;

use nucleo_matcher::{
    Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};

use crate::{
    buffer::BufferId,
    command::Command,
    component::window::{Move, Window, Wrap},
    theme::Theme,
};

/// What accepting an item means.
#[derive(Clone, Debug)]
pub enum PickerValue {
    /// Run a command.
    Command(Command),
    /// Open a file.
    File(PathBuf),
    /// Switch to an open buffer.
    Buffer(BufferId),
    /// Switch theme.
    Theme(&'static Theme),
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
    /// Answer an agent's permission request with this option.
    ///
    /// The agent's own id for it, which is what the answer names -- not the
    /// words on the row, which are the agent's and can be anything.
    Permission(String),
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
    /// Shown dimmed and right-aligned at the end of the row.
    ///
    /// Its width is taken out of the label's before the label is truncated, so
    /// it is the one part of a row that never gets cut.
    pub trailing: Option<String>,
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
    /// cannot be learned from: a reader who never sees `git.hunk` does not
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
    query: String,
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
    /// Scratch for `Utf32Str::new`, which needs somewhere to put a converted
    /// haystack.
    haystack: Vec<char>,
}

impl std::fmt::Debug for Picker {
    /// `Matcher` has no `Debug` and is large.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Picker")
            .field("query", &self.query)
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
            query: String::new(),
            matched: Vec::new(),
            indices: Vec::new(),
            window: Window::new(),
            outline: Option::None,
            tabs: Vec::new(),
            tab: 0,
            scopes: false,
            searching: false,
            listing: false,
            explains: false,
            question: None,
            empty: "nothing to choose from".to_string(),
            prefer: None,
            layout,
            matcher: Matcher::new(nucleo_matcher::Config::DEFAULT),
            haystack: Vec::new(),
        };
        picker.refilter();
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
        self.query = query.to_string();
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
    pub fn visible_rows(&self, available: u16) -> u16 {
        match self.layout {
            PickerLayout::FullArea => available,
            // A list with tabs keeps its full height whatever the tab holds:
            // walking the tabs would otherwise resize the block under the
            // reader, and the rows would move as they read them. Without
            // tabs the list is as tall as it has rows -- at least one, which
            // is where the reason for having none goes.
            PickerLayout::Compact { rows } if !self.tabs.is_empty() => {
                rows.saturating_add(self.tab_rows()).min(available)
            }
            PickerLayout::Compact { rows } => u16::try_from(self.match_count())
                .unwrap_or(u16::MAX)
                .max(1)
                .min(rows)
                .min(available),
        }
    }

    /// What has been typed.
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
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
        let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
        let mut indices = Vec::new();
        let haystack = Utf32Str::new(&self.items[index].label, &mut self.haystack);
        pattern.indices(haystack, &mut self.matcher, &mut indices);
        indices.sort_unstable();
        indices.dedup();
        indices
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

        let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
        let first = self.first_visible(height);
        for row in first..first.saturating_add(usize::from(height)) {
            let Some((index, _)) = self.matched.get(row) else {
                break;
            };
            let mut indices = spare.pop().unwrap_or_default();
            let haystack = Utf32Str::new(&self.items[*index].label, &mut self.haystack);
            pattern.indices(haystack, &mut self.matcher, &mut indices);
            indices.sort_unstable();
            indices.dedup();
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
            // Only where there are tabs to walk. Elsewhere they fall
            // through, which is what a picker with nothing to switch should
            // do with an arrow that means nothing to it.
            KeyCode::Right if bare && !self.tabs.is_empty() => {
                self.step_tab(true);
                PickerOutcome::Consumed
            }
            KeyCode::Left if bare && !self.tabs.is_empty() => {
                self.step_tab(false);
                PickerOutcome::Consumed
            }
            KeyCode::Esc if bare => PickerOutcome::Cancelled,
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
            code if bare && let Some(movement) = Move::of(code) => {
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
            KeyCode::Backspace if bare => {
                self.query.pop();
                self.refilter();
                PickerOutcome::Consumed
            }
            // Only a bare or shifted character is text. `ctrl+q` has to reach
            // the key table, or there would be no way out of a picker other
            // than Escape.
            KeyCode::Char(character) if (modifiers - KeyModifiers::SHIFT).is_empty() => {
                self.query.push(character);
                self.refilter();
                PickerOutcome::Consumed
            }
            _ => PickerOutcome::Ignored,
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

        // The first tab is every row; any other one is its own. Scope tabs
        // do not filter at all -- every row in the list belongs to the scope
        // that fetched it.
        let tab = self.tab;
        let scopes = self.scopes;
        let showing = move |item: &PickerItem| match (scopes, tab, item.tab) {
            (true, _, _) | (_, 0, _) | (_, _, None) => true,
            (_, tab, Some(of)) => tab == of,
        };

        if self.query.is_empty() {
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
            let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
            for (index, item) in self.items.iter().enumerate() {
                if !showing(item) {
                    continue;
                }
                let haystack = Utf32Str::new(&item.label, &mut self.haystack);
                if let Some(score) = pattern.score(haystack, &mut self.matcher) {
                    self.matched.push((index, score));
                }
            }
            // Best first, and ties by the original order so the list does not
            // reshuffle as more items arrive.
            self.matched
                .sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));
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
