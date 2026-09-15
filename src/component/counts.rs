//! The line counts, as something to walk through.
//!
//! Two pages over one walk: the languages a tree is written in, and the files
//! it is written across. A dialog rather than a page of the settings, because
//! it is a question a reader asks about the project and not a switch they
//! keep -- and one left with escape, like every other dialog here.
//!
//! Every row is somewhere to go. The tree's own row shows every file, a
//! language row shows that language's, a file row opens the file, and the
//! rows that are none of those -- a language written *inside* another --
//! cannot be landed on at all, which is the rule a list already follows for
//! a row that cannot be chosen. A selection that can sit on a row where
//! enter does nothing is a selection that has stopped promising anything.
//!
//! The tree's row is what keeps the two pages joined up: without it,
//! choosing a language would be a one-way door onto a fraction of the files,
//! since the tab beside it is the page a reader just left.

use std::{
    collections::{BTreeMap, HashSet},
    ffi::OsString,
    path::{Path, PathBuf},
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    component::window::{Move, Window, Wrap},
    counts::{Counted, File, Tally},
};

/// Which page is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    /// What the tree is written in.
    Languages,
    /// What it is written across.
    Files,
}

/// What enter does on a row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Go {
    /// Show every file, whatever language it is in.
    Everything,
    /// Show only this language's files.
    Language(&'static str),
    /// Open this file.
    File(PathBuf),
    /// Show what this directory holds, or stop showing it.
    ///
    /// Somewhere to go like the others: a directory is a row a reader presses
    /// a key on and something happens, which is the rule this pagefollows
    /// for what may hold the selection.
    Fold(PathBuf),
}

/// One row of either page.
///
/// Flat, and the same shape on both pages, because the drawing is the same
/// on both: a name, a bar, and the numbers in their columns. What differs is
/// what the name names and what enter does with it.
#[derive(Clone, Debug)]
pub struct Row {
    /// The glyph in front of the name, where the font has one.
    pub icon: Option<char>,
    /// The name: a language, or a path relative to the tree.
    pub name: String,
    /// How far the row is indented, in levels.
    pub depth: u16,
    /// Whether it is showing what it holds, for a row that holds anything.
    ///
    /// `None` for a row that is not a directory. Drawn in a column of its
    /// own in front of the glyph, which every row on the page leaves room
    /// for once any row has one -- so a file's name and a directory's start
    /// in the same place. It is also the half that survives a reader with no
    /// Nerd Font, which is the half that has to: folding is only ever
    /// discovered by seeing the mark.
    pub open: Option<bool>,
    /// How many files the row counts, where that is a question about it.
    /// A file row has no answer to it, which is not the same as one.
    pub files: Option<usize>,
    /// Its lines.
    pub tally: Tally,
    /// Where enter goes, or `None` for a row that is only a fact.
    pub go: Option<Go>,
}

/// What a key did.
#[derive(Debug)]
pub enum CountsOutcome {
    /// Not a key this view knows; try the key table.
    Ignored,
    /// Handled. Redraw.
    Consumed,
    /// Open this file.
    Open(PathBuf),
    /// The reader gave up.
    Cancelled,
}

/// The counts, while they are showing.
#[derive(Debug)]
pub struct Counts {
    /// What the walk found, or `None` while it is still walking.
    counted: Option<Counted>,
    /// Which page.
    page: Page,
    /// Whether every key this view answers to is showing.
    keys: bool,
    /// The one language the file page is showing, if it was reached by
    /// choosing that language rather than by asking for every file.
    only: Option<&'static str>,
    /// The rows of the page as it stands, rebuilt whenever that changes
    /// rather than per frame: a hundred rows with a `String` in each is an
    /// allocation per row, and the draw path is the one place that cannot
    /// afford to do work it could have done once.
    rows: Vec<Row>,
    /// Which row is selected and which is on top -- the window every list
    /// here has, for the reason every list here has it.
    window: Window,
    /// The directories showing what they hold, by their path in the tree.
    ///
    /// What is open rather than what is closed, so the page opens at its top
    /// level with nothing walked into: a set that started full would have to
    /// be built from a walk before the first frame, and a project is mostly
    /// directories a reader is not asking about.
    ///
    /// By path rather than by row, because the rows are rebuilt whenever
    /// anything changes and a row's number is not a name for anything.
    opened: HashSet<PathBuf>,
}

/// A directory while the rows are being built: what is under it, and what it
/// all adds up to.
///
/// The counts arrive as a flat list of paths, which is the shape tokei
/// answers in and the shape the page showed until it grew a tree. Building
/// this each time the rows are is cheap beside the walk that produced them,
/// and it means there is one copy of the truth rather than a tree kept in
/// step with a list.
#[derive(Debug, Default)]
struct Node {
    /// What it holds, by name. Sorted by name here and by size on the way
    /// out: a `BTreeMap` is how two files of the same name in two places
    /// stay apart, not how the reader will see them.
    directories: BTreeMap<OsString, Node>,
    /// The files directly in it, as name, glyph and lines.
    files: Vec<(OsString, PathBuf, Tally)>,
    /// Everything under it, however deep.
    tally: Tally,
    /// And how many files that is.
    count: usize,
}

impl Node {
    /// The tree these files make.
    fn of<'a>(files: impl Iterator<Item = &'a File>) -> Self {
        let mut root = Self::default();
        for file in files {
            root.add(file);
        }
        root
    }

    /// Puts one file in, adding its lines to every directory above it.
    fn add(&mut self, file: &File) {
        let mut here = &mut *self;
        here.tally.add(file.tally);
        here.count += 1;
        let mut walked = PathBuf::new();
        let components: Vec<_> = file.path.components().collect();
        let Some((name, directories)) = components.split_last() else {
            return;
        };
        for directory in directories {
            walked.push(directory);
            here = here
                .directories
                .entry(directory.as_os_str().to_os_string())
                .or_default();
            here.tally.add(file.tally);
            here.count += 1;
        }
        here.files.push((
            name.as_os_str().to_os_string(),
            file.path.clone(),
            file.tally,
        ));
    }

    /// Lays the tree out as rows, deepest-first where a directory is open.
    ///
    /// Siblings biggest first, directories and files together. The page asks
    /// how much code is where, not what kind of thing each row is, and
    /// putting the directories above the files would be sorting by kind
    /// before sorting by the thing being asked about.
    fn rows_into(&self, rows: &mut Vec<Row>, at: &Path, depth: u16, opened: &HashSet<PathBuf>) {
        enum Child<'a> {
            Directory(&'a OsString, &'a Node),
            File(&'a OsString, &'a PathBuf, Tally),
        }
        let mut children: Vec<(usize, Child<'_>)> = Vec::new();
        for (name, node) in &self.directories {
            children.push((node.tally.lines(), Child::Directory(name, node)));
        }
        for (name, path, tally) in &self.files {
            children.push((tally.lines(), Child::File(name, path, *tally)));
        }
        // By size, and by name where two are the same size, so a list of
        // empty files is in an order a reader can predict rather than in
        // whatever order the walk happened to find them.
        children.sort_by(|left, right| {
            right
                .0
                .cmp(&left.0)
                .then_with(|| match (&left.1, &right.1) {
                    (
                        Child::Directory(left, _) | Child::File(left, _, _),
                        Child::Directory(right, _) | Child::File(right, _, _),
                    ) => left.cmp(right),
                })
        });

        for (_, child) in children {
            match child {
                Child::Directory(name, node) => {
                    let path = at.join(name);
                    let open = opened.contains(&path);
                    rows.push(Row {
                        icon: crate::icons::enabled().then_some(crate::icons::ui::DIRECTORY),
                        name: name.to_string_lossy().into_owned(),
                        depth,
                        open: Some(open),
                        files: Some(node.count),
                        tally: node.tally,
                        go: Some(Go::Fold(path.clone())),
                    });
                    if open {
                        node.rows_into(rows, &path, depth + 1, opened);
                    }
                }
                Child::File(name, path, tally) => rows.push(Row {
                    icon: crate::icons::enabled().then(|| crate::icons::for_path(path)),
                    name: name.to_string_lossy().into_owned(),
                    depth,
                    open: None,
                    // A file is one file, which is not an answer to "how
                    // many are in here" so much as a restatement of the row.
                    files: None,
                    tally,
                    go: Some(Go::File(path.clone())),
                }),
            }
        }
    }
}

impl Default for Counts {
    fn default() -> Self {
        Self::new()
    }
}

impl Counts {
    /// Opens the view with nothing in it yet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            counted: None,
            page: Page::Languages,
            keys: false,
            only: None,
            rows: Vec::new(),
            window: Window::new(),
            opened: HashSet::new(),
        }
    }

    /// Takes the answer the walk came back with.
    pub fn show(&mut self, counted: Counted) {
        self.counted = Some(counted);
        self.rebuild();
    }

    /// What the walk found, for the view to draw the totals from.
    #[must_use]
    pub const fn counted(&self) -> Option<&Counted> {
        self.counted.as_ref()
    }

    /// Whether the walk has answered yet.
    ///
    /// What the view says instead of rows: "counting" and "nothing here" are
    /// different facts, and only this knows which one is true.
    #[must_use]
    pub const fn is_counting(&self) -> bool {
        self.counted.is_none()
    }

    /// The rows of the page that is showing.
    #[must_use]
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Which page is showing.
    #[must_use]
    pub const fn page(&self) -> Page {
        self.page
    }

    /// The words on the tabs.
    ///
    /// The second says the language while the page is showing only that
    /// language's files: a tab reading "files" over a fraction of them would
    /// be the one thing on screen that is not true.
    #[must_use]
    pub fn tabs(&self) -> Vec<String> {
        vec![
            "languages".to_string(),
            match self.only {
                Some(language) => language.to_lowercase(),
                None => "files".to_string(),
            },
        ]
    }

    /// Which tab is lit.
    #[must_use]
    pub fn tab(&self) -> usize {
        usize::from(self.page == Page::Files)
    }

    /// The window over the rows.
    #[must_use]
    pub const fn window(&self) -> &Window {
        &self.window
    }

    /// Rebuilds the rows for the page as it stands.
    ///
    /// Into a local list and then into place, which is not ceremony: the
    /// rows are built *from* what the walk found, and that is a field of
    /// this same struct.
    fn rebuild(&mut self) {
        let mut rows = Vec::new();
        if let Some(counted) = self.counted.as_ref() {
            match self.page {
                Page::Languages => {
                    // The whole tree, first and choosable. It is what all of
                    // this adds up to, and it is the way to every file:
                    // without it, choosing a language would be a one-way
                    // door onto a fraction of them.
                    rows.push(Row {
                        // A folder: the row is the tree itself. It needs a
                        // glyph of its own rather than a blank because the
                        // languages hang from this row, and the line down
                        // to them is drawn in this column -- above a blank
                        // it would hang from nothing.
                        icon: crate::icons::enabled().then_some(crate::icons::ui::TREE),
                        // Capitalised like the language names under it:
                        // it sits in that column and is read down it.
                        name: "All".to_string(),
                        depth: 0,
                        open: None,
                        files: Some(counted.files.len()),
                        tally: counted.total,
                        go: Some(Go::Everything),
                    });
                    for language in &counted.languages {
                        rows.push(Row {
                            icon: crate::icons::enabled().then(|| {
                                crate::icons::for_extension(language.extension.unwrap_or(""))
                            }),
                            name: language.name.to_string(),
                            depth: 1,
                            open: None,
                            files: Some(language.files),
                            tally: language.tally,
                            go: Some(Go::Language(language.name)),
                        });
                        for child in &language.children {
                            rows.push(Row {
                                // No glyph: the row is a part of the one
                                // above it, and a second glyph down the
                                // column would read as another language of
                                // its own.
                                icon: None,
                                name: child.name.to_string(),
                                depth: 2,
                                open: None,
                                files: Some(child.files),
                                tally: child.tally,
                                go: None,
                            });
                        }
                    }
                }
                Page::Files => {
                    // A tree rather than a list of whole paths: a project is
                    // written in directories, and the question this page
                    // asks -- where is the code -- is mostly a question
                    // about them.
                    let tree = Node::of(
                        counted
                            .files
                            .iter()
                            .filter(|file| self.only.is_none_or(|only| only == file.language)),
                    );
                    tree.rows_into(&mut rows, &PathBuf::new(), 0, &self.opened);
                }
            }
        }

        self.rows = rows;
        self.window.set_count(self.rows.len());
        // The first row that can be chosen, which on the languages page is
        // the first row: a view that opened with the selection on a row
        // enter does nothing to would be teaching the wrong thing about it.
        self.window.set_focus(self.choosable(0, true).unwrap_or(0));
    }

    /// Shows what a directory holds, or stops showing it.
    ///
    /// The selection stays on the directory rather than on whichever row
    /// happens to land under it: it is the row the key was pressed on, and
    /// folding one closed while the cursor was inside has nowhere else to
    /// put it -- there is nowhere inside to stand.
    fn fold(&mut self, path: &Path) {
        if !self.opened.remove(path) {
            self.opened.insert(path.to_path_buf());
        }
        let on = self
            .rows
            .get(self.window.focus())
            .map(|row| row.name.clone());
        self.rebuild();
        if let Some(name) = on
            && let Some(at) = self
                .rows
                .iter()
                .position(|row| row.go == Some(Go::Fold(path.to_path_buf())) || row.name == name)
        {
            self.window.set_focus(at);
        }
    }

    /// Whether any row on the page folds, and so whether every row leaves a
    /// column in front of its glyph for the mark.
    ///
    /// Over all the rows rather than the visible ones: a column that came
    /// and went as the list scrolled would move every name on the screen.
    #[must_use]
    pub fn folds(&self) -> bool {
        self.rows.iter().any(|row| row.open.is_some())
    }

    /// Whether the list of every key is showing.
    #[must_use]
    pub const fn showing_keys(&self) -> bool {
        self.keys
    }

    /// Whether the row at `at` is one enter does something to.
    fn can_choose(&self, at: usize) -> bool {
        self.rows.get(at).is_some_and(|row| row.go.is_some())
    }

    /// The nearest row that can be chosen, looking `forward` first.
    ///
    /// Both ways round, for the reason a picker looks both ways: the rows
    /// that cannot be chosen come in runs, and a reader stepping into one
    /// has to come out the other side of it rather than stop in the middle.
    fn choosable(&self, from: usize, forward: bool) -> Option<usize> {
        let rows = self.rows.len();
        if rows == 0 {
            return None;
        }
        let from = from.min(rows - 1);
        let order: Vec<usize> = if forward {
            (from..rows).chain(0..from).collect()
        } else {
            (0..=from).rev().chain((from..rows).rev()).collect()
        };
        order.into_iter().find(|at| self.can_choose(*at))
    }

    /// Moves the selection by `by` rows, stepping over what cannot be chosen.
    fn move_selection(&mut self, by: isize, height: u16, wrap: Wrap) {
        let landed = self.window.step(by, wrap);
        if let Some(at) = self.choosable(landed, by >= 0) {
            self.window.set_focus(at);
        }
        self.window.settle(height);
    }

    /// Walks to the other page.
    ///
    /// Round the ends, like every other row of tabs here: a walk that stops
    /// at either end is a key that does nothing half the time it is pressed.
    /// With two tabs that makes both directions the same move, so this takes
    /// no direction -- the day there is a third page it will need one back.
    fn step_page(&mut self) {
        self.page = match self.page {
            Page::Languages => Page::Files,
            Page::Files => Page::Languages,
        };
        // A tab reached by the arrow keys is the page it is named after, so
        // walking to `files` means every file. Which language was chosen is
        // given up with it, and the way to choose again -- or to ask for all
        // of them without walking at all -- is the `All` row on the page
        // beside this one.
        self.only = None;
        self.window = Window::new();
        self.rebuild();
    }

    /// Handles a key. `height` is how many rows of list are on screen.
    pub fn handle_key(&mut self, key: &KeyEvent, height: u16) -> CountsOutcome {
        let bare = key.modifiers == KeyModifiers::NONE;
        let control = key.modifiers == KeyModifiers::CONTROL;
        let paging = matches!(key.code, KeyCode::PageUp | KeyCode::PageDown);

        match key.code {
            // Escape gives up on the nearest thing, which while the file
            // page is showing one language is that language: the reader
            // chose it a keystroke ago, and taking the whole view away is
            // not what going back from it means.
            // The card first: a key that opens a thing closes that thing.
            KeyCode::Esc if bare && self.keys => {
                self.keys = false;
                CountsOutcome::Consumed
            }
            KeyCode::F(1) if bare => {
                self.keys = !self.keys;
                CountsOutcome::Consumed
            }
            KeyCode::Esc if bare && self.only.is_some() => {
                self.page = Page::Languages;
                self.only = None;
                self.window = Window::new();
                self.rebuild();
                CountsOutcome::Consumed
            }
            KeyCode::Esc if bare => CountsOutcome::Cancelled,
            KeyCode::Right if bare => {
                self.step_page();
                CountsOutcome::Consumed
            }
            KeyCode::Left if bare => {
                self.step_page();
                CountsOutcome::Consumed
            }
            // The mark on the row says this: the same arrow the gutter, the
            // transcript and a commit's files turn, and the same key that
            // turns them. Taken here rather than left to the command table
            // because the counts are a dialog, and obelus's own commands do
            // not run from inside one.
            KeyCode::Char('f') if key.modifiers == KeyModifiers::ALT => {
                match self
                    .rows
                    .get(self.window.focus())
                    .and_then(|row| row.go.clone())
                {
                    Some(Go::Fold(path)) => {
                        self.fold(&path);
                        CountsOutcome::Consumed
                    }
                    _ => CountsOutcome::Ignored,
                }
            }
            KeyCode::Enter if bare => {
                match self
                    .rows
                    .get(self.window.focus())
                    .and_then(|row| row.go.clone())
                {
                    Some(Go::File(path)) => CountsOutcome::Open(path),
                    Some(Go::Fold(path)) => {
                        self.fold(&path);
                        CountsOutcome::Consumed
                    }
                    Some(go @ (Go::Everything | Go::Language(_))) => {
                        self.only = match go {
                            Go::Language(name) => Some(name),
                            _ => None,
                        };
                        self.page = Page::Files;
                        self.window = Window::new();
                        self.rebuild();
                        CountsOutcome::Consumed
                    }
                    None => CountsOutcome::Consumed,
                }
            }
            // The keys that move about a list, from the table every list
            // here reads. What is this view's own is only what a step means
            // in it, which is why the moving goes through `move_selection`.
            code if bare || (control && paging) => match Move::of(code) {
                Some(Move::Up) => {
                    self.move_selection(-1, height, Wrap::Yes);
                    CountsOutcome::Consumed
                }
                Some(Move::Down) => {
                    self.move_selection(1, height, Wrap::Yes);
                    CountsOutcome::Consumed
                }
                // Clamped rather than wrapped, like every other list: paging
                // is how a long one is crossed, and a page that wrapped past
                // the end would overshoot what it was reaching for.
                Some(Move::PageUp) => {
                    self.window.page(-1, height);
                    self.settle_to_choosable(false, height);
                    CountsOutcome::Consumed
                }
                Some(Move::PageDown) => {
                    self.window.page(1, height);
                    self.settle_to_choosable(true, height);
                    CountsOutcome::Consumed
                }
                Some(Move::First) => {
                    self.window.home();
                    self.settle_to_choosable(true, height);
                    CountsOutcome::Consumed
                }
                Some(Move::Last) => {
                    self.window.end();
                    self.settle_to_choosable(false, height);
                    CountsOutcome::Consumed
                }
                None => CountsOutcome::Ignored,
            },
            _ => CountsOutcome::Ignored,
        }
    }

    /// Puts the selection on a row that can be chosen after a jump.
    fn settle_to_choosable(&mut self, forward: bool, height: u16) {
        if let Some(at) = self.choosable(self.window.focus(), forward) {
            self.window.set_focus(at);
        }
        self.window.settle(height);
    }

    /// What a wheel's notch does here.
    ///
    /// Steps the selection, which is the exception a list is: a list's view
    /// *is* its selection, and there is nothing else in it to scroll. One
    /// row a notch, not three -- three is right for text, where a notch is a
    /// gesture at a paragraph.
    pub fn scroll(&mut self, rows: isize, height: u16) {
        self.move_selection(rows.signum(), height, Wrap::No);
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::counts::{Child, File, Language};

    fn tally(code: usize) -> Tally {
        Tally {
            code,
            comments: 1,
            blanks: 1,
        }
    }

    /// A tree with one language that has something embedded in it, and one
    /// that does not.
    fn counted() -> Counted {
        Counted {
            languages: vec![
                Language {
                    name: "Rust",
                    files: 2,
                    extension: Some("rs"),
                    tally: tally(100),
                    children: vec![Child {
                        name: "Markdown",
                        files: 2,
                        tally: tally(10),
                    }],
                },
                Language {
                    name: "TOML",
                    files: 1,
                    extension: Some("toml"),
                    tally: tally(5),
                    children: Vec::new(),
                },
            ],
            files: vec![
                File {
                    path: PathBuf::from("src/main.rs"),
                    language: "Rust",
                    tally: tally(80),
                },
                File {
                    path: PathBuf::from("src/lib.rs"),
                    language: "Rust",
                    tally: tally(20),
                },
                File {
                    path: PathBuf::from("Cargo.toml"),
                    language: "TOML",
                    tally: tally(5),
                },
            ],
            total: tally(105),
        }
    }

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// The selection never stops on a row enter does nothing to.
    ///
    /// Broken deliberately by having `move_selection` set the focus to where
    /// the window landed: the selection then sat on the embedded Markdown
    /// row, and the assertion that it walks to TOML failed.
    #[test]
    fn the_selection_steps_over_a_language_written_inside_another() {
        let mut counts = Counts::new();
        counts.show(counted());

        // The whole tree, then a row per language and per child.
        assert_eq!(counts.rows().len(), 4);
        assert_eq!(counts.rows()[0].name, "All");
        assert_eq!(counts.rows()[2].depth, 2, "the child is not indented");
        assert!(counts.rows()[2].go.is_none(), "the child can be chosen");

        counts.handle_key(&press(KeyCode::Down), 10);
        assert_eq!(counts.rows()[counts.window().focus()].name, "Rust");
        // Down from Rust is the next *language*, not the child between them.
        counts.handle_key(&press(KeyCode::Down), 10);
        assert_eq!(counts.rows()[counts.window().focus()].name, "TOML");
        counts.handle_key(&press(KeyCode::Up), 10);
        assert_eq!(counts.rows()[counts.window().focus()].name, "Rust");
    }

    /// The tree's own row is what all of it adds up to, and the way to every
    /// file.
    ///
    /// Broken deliberately by leaving the `all` row out of the languages
    /// page: choosing a language became a one-way door onto a fraction of
    /// the files, and the first assertion failed.
    #[test]
    fn the_first_row_is_the_whole_tree_and_opens_every_file() {
        let mut counts = Counts::new();
        counts.show(counted());

        let all = &counts.rows()[0];
        assert_eq!(all.files, Some(3), "the tree's own row miscounts its files");
        assert_eq!(all.tally.code, 105, "it is not the total");

        counts.handle_key(&press(KeyCode::Enter), 10);
        assert_eq!(counts.page(), Page::Files);
        // The top of the tree: `src`, holding two, and the lone `Cargo.toml`.
        assert_eq!(counts.rows().len(), 2, "not the top of the tree");
        assert_eq!(counts.rows()[0].name, "src");
        assert_eq!(counts.rows()[0].files, Some(2), "the directory miscounts");
        assert_eq!(counts.tabs()[1], "files", "the tab claims a language");
    }

    /// Choosing a language shows its files, and the tab says which.
    ///
    /// Broken deliberately by leaving `only` unset when a language is
    /// chosen: every file came back on the page and the tab went on reading
    /// "files", so both assertions failed.
    #[test]
    fn choosing_a_language_shows_its_files_and_the_tab_says_which() {
        let mut counts = Counts::new();
        counts.show(counted());

        // Past the tree's own row, onto Rust.
        counts.handle_key(&press(KeyCode::Down), 10);
        counts.handle_key(&press(KeyCode::Enter), 10);
        assert_eq!(counts.page(), Page::Files);
        // Only Rust, so `Cargo.toml` is gone and `src` holds what is left.
        assert_eq!(
            counts.rows().len(),
            1,
            "a language's files were not filtered to it"
        );
        assert_eq!(counts.rows()[0].name, "src");
        assert_eq!(counts.rows()[0].files, Some(2));
        assert_eq!(
            counts.tabs()[1],
            "rust",
            "the tab did not say what it is showing"
        );

        // And walking to the files tab is the other way back to all of them.
        counts.handle_key(&press(KeyCode::Left), 10);
        counts.handle_key(&press(KeyCode::Right), 10);
        // `Cargo.toml` is back beside `src`, so the narrowing is gone.
        assert_eq!(counts.rows().len(), 2, "walking kept the narrowing");
        assert_eq!(counts.tabs()[1], "files");

        // Escape gives up on the language first, not on the whole view.
        counts.handle_key(&press(KeyCode::Left), 10);
        counts.handle_key(&press(KeyCode::Down), 10);
        counts.handle_key(&press(KeyCode::Enter), 10);
        assert!(matches!(
            counts.handle_key(&press(KeyCode::Esc), 10),
            CountsOutcome::Consumed
        ));
        assert_eq!(counts.page(), Page::Languages);
        // And then on the view.
        assert!(matches!(
            counts.handle_key(&press(KeyCode::Esc), 10),
            CountsOutcome::Cancelled
        ));
    }

    /// The tabs go round the ends.
    ///
    /// Broken deliberately by clamping instead of wrapping, which is what
    /// this did: `right` on the last tab and `left` on the first were keys
    /// that did nothing, and both assertions failed.
    #[test]
    fn walking_off_either_end_of_the_tabs_comes_back_at_the_other() {
        let mut counts = Counts::new();
        counts.show(counted());

        counts.handle_key(&press(KeyCode::Right), 10);
        assert_eq!(counts.page(), Page::Files);
        // Off the right-hand end.
        counts.handle_key(&press(KeyCode::Right), 10);
        assert_eq!(counts.page(), Page::Languages);
        // And off the left-hand one.
        counts.handle_key(&press(KeyCode::Left), 10);
        assert_eq!(counts.page(), Page::Files);
    }

    /// Enter on a file is how a count becomes somewhere to go.
    ///
    /// Broken deliberately by making the file rows' `go` `None`: enter
    /// answered `Consumed` and the file never opened.
    #[test]
    fn enter_on_a_file_opens_it() {
        let mut counts = Counts::new();
        counts.show(counted());

        // Walk to the file page rather than choosing a language, so this is
        // the whole list.
        counts.handle_key(&press(KeyCode::Right), 10);
        assert_eq!(counts.page(), Page::Files);
        assert_eq!(counts.rows().len(), 2, "the tab was reached narrowed");

        // Enter on the directory opens it rather than a file, which is the
        // other half of this: the selection stays where the key was pressed.
        match counts.handle_key(&press(KeyCode::Enter), 10) {
            CountsOutcome::Consumed => {}
            outcome => panic!("enter on a directory answered {outcome:?}"),
        }
        assert_eq!(counts.rows()[counts.window().focus()].name, "src");
        assert_eq!(counts.rows().len(), 4, "the directory did not open");

        counts.handle_key(&press(KeyCode::Down), 10);
        match counts.handle_key(&press(KeyCode::Enter), 10) {
            CountsOutcome::Open(path) => assert_eq!(path, Path::new("src/main.rs")),
            outcome => panic!("enter on a file answered {outcome:?}"),
        }
    }

    /// A directory folds, and the same key folds it back.
    ///
    /// Broken deliberately by having `fold` always insert: the second press
    /// did nothing and the row count never came back down.
    #[test]
    fn a_directory_opens_and_closes_on_the_same_key() {
        let mut counts = Counts::new();
        counts.show(counted());
        counts.handle_key(&press(KeyCode::Right), 10);

        let shut = counts.rows().len();
        assert_eq!(counts.rows()[0].open, Some(false), "it opened opened");
        assert!(counts.rows()[1].open.is_none(), "a file says it folds");

        counts.handle_key(&press(KeyCode::Enter), 10);
        assert_eq!(counts.rows()[0].open, Some(true));
        assert_eq!(counts.rows().len(), shut + 2, "not its two files");
        assert_eq!(counts.rows()[1].depth, 1, "its files are not indented");

        // `alt+f` is the same act, and the mark on the row says so.
        counts.handle_key(&KeyEvent::new(KeyCode::Char('f'), KeyModifiers::ALT), 10);
        assert_eq!(counts.rows()[0].open, Some(false));
        assert_eq!(counts.rows().len(), shut, "it would not fold back");
    }

    /// Siblings are ordered by what the page is about, biggest first, with
    /// directories and files in one ordering rather than two.
    #[test]
    fn a_directory_sits_among_the_files_by_size() {
        let mut counts = Counts::new();
        counts.show(counted());
        counts.handle_key(&press(KeyCode::Right), 10);

        // `src` holds a hundred lines and `Cargo.toml` five.
        let names: Vec<&str> = counts.rows().iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["src", "Cargo.toml"]);

        counts.handle_key(&press(KeyCode::Enter), 10);
        let names: Vec<&str> = counts.rows().iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["src", "main.rs", "lib.rs", "Cargo.toml"]);
    }

    /// A key the view does not know is left for the key table.
    ///
    /// Broken deliberately by answering `Consumed` to everything: a key that
    /// means nothing here would have been swallowed, and nothing bound in a
    /// dialog would ever fire.
    #[test]
    fn a_key_this_view_does_not_use_is_passed_on() {
        let mut counts = Counts::new();
        counts.show(counted());
        assert!(matches!(
            counts.handle_key(&press(KeyCode::Char('x')), 10),
            CountsOutcome::Ignored
        ));
    }
}
