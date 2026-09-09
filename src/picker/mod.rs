//! The one list-with-a-prompt, instantiated four times.
//!
//! Files, buffers, themes and the command palette differ in what they list and
//! how tall they are, and in nothing else. Later they are joined by symbols,
//! references and commits, which is why this is a component rather than four
//! screens.

pub mod files;
pub mod icons;

use std::path::PathBuf;

use nucleo_matcher::{
    Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};

use crate::{buffer::BufferId, command::Command, theme::Theme};

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
    /// Nothing. A row that is there to say why the list is short.
    Nothing,
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
    /// Shown dimmed and right-aligned at the end of the row.
    ///
    /// Its width is taken out of the label's before the label is truncated, so
    /// it is the one part of a row that never gets cut.
    pub trailing: Option<String>,
    /// What choosing it does.
    pub value: PickerValue,
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

/// Whether a move off the end comes back round.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wrap {
    /// Off the end and round to the other.
    Yes,
    /// Stop at the end.
    No,
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
    /// Which character positions of the selected row matched.
    ///
    /// Also reused. Only ever holds the selected row's, because that is the
    /// only row whose matched characters get their own colour.
    indices: Vec<u32>,
    selected: usize,
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
            .field("selected", &self.selected)
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
            selected: 0,
            layout,
            matcher: Matcher::new(nucleo_matcher::Config::DEFAULT),
            haystack: Vec::new(),
        };
        picker.refilter();
        picker
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
            PickerLayout::Compact { rows } => u16::try_from(self.match_count())
                .unwrap_or(u16::MAX)
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
        let (index, _) = self.matched.get(self.selected)?;
        self.items.get(*index)
    }

    /// How many rows match.
    #[must_use]
    pub fn match_count(&self) -> usize {
        self.matched.len()
    }

    /// Which matching row is selected.
    #[must_use]
    pub const fn selected(&self) -> usize {
        self.selected
    }

    /// The character positions of the selected row that the query matched.
    #[must_use]
    pub fn selected_indices(&self) -> &[u32] {
        &self.indices
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

        match key.code {
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
            KeyCode::Esc if bare => PickerOutcome::Cancelled,
            KeyCode::Enter if bare => self
                .matched
                .get(self.selected)
                .map_or(PickerOutcome::Consumed, |(index, _)| {
                    PickerOutcome::Accepted(self.items[*index].value.clone())
                }),
            KeyCode::Down if bare => {
                self.move_selection(1, Wrap::Yes);
                PickerOutcome::Consumed
            }
            KeyCode::Up if bare => {
                self.move_selection(-1, Wrap::Yes);
                PickerOutcome::Consumed
            }
            // Clamped rather than wrapped, unlike a single step. Paging is how
            // you get to the end of a long list, and a page that wraps past it
            // back to the top overshoots the thing you were reaching for.
            KeyCode::PageDown if bare => {
                self.move_selection(page, Wrap::No);
                PickerOutcome::Consumed
            }
            KeyCode::PageUp if bare => {
                self.move_selection(-page, Wrap::No);
                PickerOutcome::Consumed
            }
            KeyCode::Home if bare => {
                self.select(0);
                PickerOutcome::Consumed
            }
            KeyCode::End if bare => {
                self.select(self.matched.len().saturating_sub(1));
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
        }
    }

    /// Selects a row outright.
    fn select(&mut self, row: usize) {
        self.selected = row.min(self.matched.len().saturating_sub(1));
        self.recompute_indices();
    }

    fn move_selection(&mut self, by: isize, wrap: Wrap) {
        if self.matched.is_empty() {
            self.selected = 0;
            return;
        }
        let last = self.matched.len() - 1;
        self.selected = match wrap {
            // Wrapping, because the other end of a list is faster to reach
            // than to scroll back through.
            Wrap::Yes if by > 0 && self.selected >= last => 0,
            Wrap::Yes if by < 0 && self.selected == 0 => last,
            _ => self.selected.saturating_add_signed(by).min(last),
        };
        self.recompute_indices();
    }

    fn refilter(&mut self) {
        self.matched.clear();

        if self.query.is_empty() {
            // An empty query keeps the given order, which is the order the
            // caller thought worth showing: recent buffers, the command table.
            self.matched
                .extend((0..self.items.len()).map(|index| (index, 0)));
        } else {
            let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
            for (index, item) in self.items.iter().enumerate() {
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

        self.selected = self.selected.min(self.matched.len().saturating_sub(1));
        self.recompute_indices();
    }

    fn recompute_indices(&mut self) {
        self.indices.clear();
        if self.query.is_empty() {
            return;
        }
        let Some((index, _)) = self.matched.get(self.selected) else {
            return;
        };
        let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
        let haystack = Utf32Str::new(&self.items[*index].label, &mut self.haystack);
        pattern.indices(haystack, &mut self.matcher, &mut self.indices);
        self.indices.sort_unstable();
        self.indices.dedup();
    }
}
