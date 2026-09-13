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

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::{
    component::window::{Move, Window, Wrap},
    counts::{Counted, Tally},
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
}

impl Default for Counts {
    fn default() -> Self {
        Self::new()
    }
}

impl Counts {
    /// Opens the view with nothing in it yet.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            counted: None,
            page: Page::Languages,
            only: None,
            rows: Vec::new(),
            window: Window::new(),
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
                                files: Some(child.files),
                                tally: child.tally,
                                go: None,
                            });
                        }
                    }
                }
                Page::Files => {
                    for file in &counted.files {
                        if self.only.is_some_and(|only| only != file.language) {
                            continue;
                        }
                        rows.push(Row {
                            icon: crate::icons::enabled()
                                .then(|| crate::icons::for_path(&file.path)),
                            name: file.path.display().to_string(),
                            depth: 0,
                            files: None,
                            tally: file.tally,
                            go: Some(Go::File(file.path.clone())),
                        });
                    }
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
            KeyCode::Enter if bare => {
                match self
                    .rows
                    .get(self.window.focus())
                    .and_then(|row| row.go.clone())
                {
                    Some(Go::File(path)) => CountsOutcome::Open(path),
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
        assert_eq!(counts.rows().len(), 3, "not every file came back");
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
        assert_eq!(
            counts.rows().len(),
            2,
            "a language's files were not filtered to it"
        );
        assert!(counts.rows().iter().all(|row| row.name.ends_with(".rs")));
        assert_eq!(
            counts.tabs()[1],
            "rust",
            "the tab did not say what it is showing"
        );

        // And walking to the files tab is the other way back to all of them.
        counts.handle_key(&press(KeyCode::Left), 10);
        counts.handle_key(&press(KeyCode::Right), 10);
        assert_eq!(counts.rows().len(), 3, "walking kept the narrowing");
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
        assert_eq!(counts.rows().len(), 3, "the tab was reached narrowed");

        match counts.handle_key(&press(KeyCode::Enter), 10) {
            CountsOutcome::Open(path) => assert_eq!(path, Path::new("src/main.rs")),
            outcome => panic!("enter on a file answered {outcome:?}"),
        }
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
