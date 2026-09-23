//! The list of what could be typed next.
//!
//! Not a [`crate::picker::Picker`], for one reason that decides
//! everything else: every key a picker does not want, it keeps anyway --
//! a list is what the reader is doing. Here the reader is typing into the
//! document, and the list is a thing that appeared beside the cursor. So it
//! takes six keys and lets every other one through, and the letters that
//! reach the document come back to it as a narrower query.
//!
//! The first candidate is selected the moment it opens, so `enter` takes
//! the server's best answer without a key in between. What that costs is
//! the return key: while the panel is up, `enter` accepts rather than
//! breaking the line, and `escape` is how a reader who wanted a new line
//! gets one. The panel is only ever up because the reader is in the middle
//! of a word, which is the one moment they were not about to press it.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use nucleo_matcher::{
    Matcher, Utf32Str,
    pattern::{CaseMatching, Normalization, Pattern},
};
use obelus_buffer::DocumentId;
use obelus_lsp::complete::{Candidate, Offer};
use obelus_row::Row;
use obelus_text::coordinates::{CharColumn, LineNumber};

use crate::window::{Move, Window, Wrap};

/// The columns an icon takes: the glyph, and the blank it bleeds into.
pub const ICON_COLUMNS: usize = 2;

/// The blank between a label and the detail after it.
pub const DETAIL_GAP: usize = 2;

/// The most rows of candidates, however many there are.
///
/// Fewer than a compact list's ten: what this covers is the code the reader
/// is in the middle of writing, and eight is already most of a paragraph.
pub const MOST_ROWS: u16 = 8;

/// The most rows of documentation.
pub const MOST_DOCUMENTATION: u16 = 10;

/// Fewer rows than this and the documentation is not worth the room.
///
/// Two lines of a paragraph that carries on is less use than the code they
/// would have covered.
pub const LEAST_DOCUMENTATION: u16 = 3;

/// What a key did to the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompletionOutcome {
    /// Not a key the list wants; it belongs to the document.
    Ignored,
    /// Handled. Redraw.
    Consumed,
    /// The reader accepted the candidate that is selected.
    Accepted,
    /// The reader dismissed the list.
    Cancelled,
}

/// A markdown rendering, kept until what it is about changes.
#[derive(Debug)]
struct Rendered {
    /// Which candidate, at what width, and whether its documentation had
    /// arrived yet.
    at: (usize, u16, bool),
    rows: Vec<Row>,
}

/// What a server offered, and where the reader is in it.
pub struct Completion {
    /// The document this is about. A list offered for one file means
    /// nothing in another.
    buffer: DocumentId,
    /// Where the word being completed starts.
    ///
    /// The anchor for everything: the panel is drawn from this column, the
    /// query is the text between it and the cursor, and a cursor that is no
    /// longer after it is a reader who has walked away.
    from: (LineNumber, CharColumn),
    /// What language the file is in, for the fence the signature goes in.
    language: Option<&'static str>,
    /// What has been typed since `from`.
    query: String,
    candidates: Vec<Candidate>,
    /// Whether another letter needs a fresh question.
    incomplete: bool,
    /// Indices into `candidates`, best first.
    matched: Vec<usize>,
    /// Which characters matched, for the rows on screen.
    indices: Vec<(usize, Vec<u32>)>,
    window: Window,
    /// How far the documentation is scrolled.
    scrolled: usize,
    /// How many rows the panel's two halves were last given.
    room: (u16, u16),
    /// The documentation, laid out for the width it was last drawn at.
    rendered: Option<Rendered>,
    /// How many cells the widest label wants, its icon included.
    ///
    /// The column the details start in, so that they line up down the list
    /// -- they are read as a column of types, and a column that stepped in
    /// and out with the length of each name would not be one.
    ///
    /// Worked out when the list changes rather than per frame: it is a pass
    /// over every candidate, and the answer only moves when the query does.
    labels: usize,
    /// And how many the widest row wants altogether.
    width: usize,
    matcher: Matcher,
    haystack: Vec<char>,
}

impl std::fmt::Debug for Completion {
    /// `Matcher` has no `Debug` and is large.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Completion")
            .field("query", &self.query)
            .field("candidates", &self.candidates.len())
            .field("matched", &self.matched.len())
            .finish_non_exhaustive()
    }
}

impl Completion {
    /// A panel over what a server offered, or nothing to show.
    ///
    /// Nothing is what an empty answer comes to, and what an answer that
    /// the reader has already typed past comes to as well: a panel with no
    /// rows in it is a box that says the server had no ideas, which is not
    /// worth covering code with.
    #[must_use]
    pub fn new(
        buffer: DocumentId,
        from: (LineNumber, CharColumn),
        language: Option<&'static str>,
        offer: Offer,
        query: &str,
    ) -> Option<Self> {
        let mut completion = Self {
            buffer,
            from,
            language,
            query: query.to_string(),
            candidates: offer.candidates,
            incomplete: offer.incomplete,
            matched: Vec::new(),
            indices: Vec::new(),
            window: Window::new(),
            scrolled: 0,
            room: (MOST_ROWS, 0),
            rendered: None,
            labels: 0,
            width: 0,
            matcher: Matcher::default(),
            haystack: Vec::new(),
        };
        completion.refilter();
        (!completion.matched.is_empty()).then_some(completion)
    }

    /// Which document it belongs to.
    #[must_use]
    pub const fn buffer(&self) -> DocumentId {
        self.buffer
    }

    /// Where the word being completed starts.
    #[must_use]
    pub const fn from(&self) -> (LineNumber, CharColumn) {
        self.from
    }

    /// What has been typed since.
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Whether another letter needs the server asking again.
    #[must_use]
    pub const fn incomplete(&self) -> bool {
        self.incomplete
    }

    /// Narrows the list to a longer query. Says whether anything is left.
    #[must_use]
    pub fn narrow(&mut self, query: &str) -> bool {
        if self.query == query {
            return !self.matched.is_empty();
        }
        query.clone_into(&mut self.query);
        // Back to the top: the row that was selected was selected out of a
        // different list, and keeping the row number would leave the
        // selection on whatever slid under it -- which is the thing `enter`
        // would then put in.
        self.window.set_focus(0);
        self.refilter();
        !self.matched.is_empty()
    }

    /// How many cells the widest row wants: its icon, its label and its
    /// detail, with the blank between them.
    #[must_use]
    pub const fn width(&self) -> usize {
        self.width
    }

    /// Which column the details line up in.
    #[must_use]
    pub const fn labels(&self) -> usize {
        self.labels
    }

    /// How many candidates match.
    #[must_use]
    pub fn count(&self) -> usize {
        self.matched.len()
    }

    /// The candidates to draw, from the top of the window.
    pub fn visible(&self, height: u16) -> impl Iterator<Item = (usize, &Candidate)> {
        self.window
            .visible(height)
            .filter_map(|row| Some((row, self.candidates.get(*self.matched.get(row)?)?)))
    }

    /// Which row is selected, which is the first one until it is moved.
    #[must_use]
    pub const fn selected(&self) -> usize {
        self.window.focus()
    }

    /// The candidate that selection names.
    #[must_use]
    pub fn chosen(&self) -> Option<&Candidate> {
        self.candidates.get(self.chosen_index()?)
    }

    /// Which candidate that is, by its place in the answer.
    #[must_use]
    pub fn chosen_index(&self) -> Option<usize> {
        self.matched.get(self.selected()).copied()
    }

    /// Puts the choice on one of the candidates.
    ///
    /// For a pointer: the keys step through them and have no use for naming
    /// one outright, and a press names one.
    pub fn choose_row(&mut self, row: usize) {
        self.window
            .set_focus(row.min(self.count().saturating_sub(1)));
    }

    /// Where the window starts.
    #[must_use]
    pub const fn top(&self) -> usize {
        self.window.top()
    }

    /// How far the documentation is scrolled.
    #[must_use]
    pub const fn scrolled(&self) -> usize {
        self.scrolled
    }

    /// Which characters of a row matched the query.
    #[must_use]
    pub fn indices_at(&self, row: usize) -> &[u32] {
        self.indices
            .iter()
            .find(|(at, _)| *at == row)
            .map_or(&[], |(_, indices)| indices.as_slice())
    }

    /// The candidate a resolve is owed for, if one is.
    ///
    /// Only the selected one, and only once: the reader reads the panel one
    /// candidate at a time, and asking about the other nine hundred is a
    /// question nobody will see the answer to.
    #[must_use]
    pub fn unresolved(&self) -> Option<usize> {
        let index = self.chosen_index()?;
        (!self.candidates.get(index)?.resolved).then_some(index)
    }

    /// The candidate a resolve answered about, to be filled in.
    pub fn candidate_mut(&mut self, index: usize) -> Option<&mut Candidate> {
        // Whatever the answer adds, the rendering of it is about to be out
        // of date.
        self.rendered = None;
        self.candidates.get_mut(index)
    }

    /// Whether the panel has a documentation half to draw at all.
    ///
    /// Cheaper than asking for the text: the layout asks this every frame,
    /// and building the markdown to throw it away is work for nothing.
    #[must_use]
    pub fn has_documentation(&self) -> bool {
        self.chosen().is_some_and(|candidate| {
            candidate.detail.is_some() || candidate.documentation.is_some()
        })
    }

    /// What the documentation half of the panel says, as markdown.
    ///
    /// The signature goes in as a fenced block in the file's own language,
    /// which is how helix does it and why obelus's markdown grew fences
    /// that are coloured: the same renderer that shows a README shows the
    /// type of what is about to be typed, in the colours the code has.
    #[must_use]
    pub fn documentation(&self) -> Option<String> {
        let candidate = self.chosen()?;
        let signature = candidate
            .detail
            .as_ref()
            .map(|detail| format!("```{}\n{detail}\n```", self.language.unwrap_or_default()));
        match (signature, candidate.documentation.as_ref()) {
            (None, None) => None,
            (Some(signature), None) => Some(signature),
            (None, Some(documentation)) => Some(documentation.clone()),
            (Some(signature), Some(documentation)) => Some(format!("{signature}\n{documentation}")),
        }
    }

    /// The documentation, laid out for a width.
    ///
    /// Kept from frame to frame: laying markdown out is the expensive part,
    /// and the panel is redrawn on every keystroke that does not change
    /// which candidate is selected.
    pub fn settle_documentation(&mut self, width: u16) {
        let Some(index) = self.chosen_index() else {
            self.rendered = None;
            return;
        };
        let resolved = self.candidates.get(index).is_some_and(|it| it.resolved);
        if self
            .rendered
            .as_ref()
            .is_some_and(|rendered| rendered.at == (index, width, resolved))
        {
            return;
        }
        let rows = match self.documentation() {
            Some(source) => obelus_markdown::render(&source, width),
            None => Vec::new(),
        };
        self.rendered = Some(Rendered {
            at: (index, width, resolved),
            rows,
        });
    }

    /// What [`Completion::settle_documentation`] laid out.
    #[must_use]
    pub fn documentation_rows(&self) -> &[Row] {
        self.rendered
            .as_ref()
            .map_or(&[], |rendered| rendered.rows.as_slice())
    }

    /// Records the room the panel's halves have, and settles the windows in
    /// it.
    pub fn settle(&mut self, list: u16, documentation: u16) {
        self.room = (list, documentation);
        self.window.set_count(self.matched.len());
        self.window.settle(list);
        self.refresh_indices(list);
        let rows = self.documentation_rows().len();
        self.scrolled = self
            .scrolled
            .min(rows.saturating_sub(usize::from(documentation)));
    }

    /// Takes a key, if it is one of the six the panel owns.
    pub fn handle_key(&mut self, key: &KeyEvent) -> CompletionOutcome {
        let Some(modifiers) = obelus_editing::keymap::modifiers_of(key) else {
            return CompletionOutcome::Ignored;
        };
        if modifiers != KeyModifiers::NONE {
            return CompletionOutcome::Ignored;
        }
        match key.code {
            KeyCode::Down => {
                self.window.apply(Move::Down, self.room.0, Wrap::Yes);
                self.settle_focus();
                CompletionOutcome::Consumed
            }
            KeyCode::Up => {
                self.window.apply(Move::Up, self.room.0, Wrap::Yes);
                self.settle_focus();
                CompletionOutcome::Consumed
            }
            // The paging keys go to the half that cannot be narrowed by
            // typing. With no documentation showing there is nothing else
            // to give them, so they page the list.
            KeyCode::PageDown | KeyCode::PageUp => {
                let down = key.code == KeyCode::PageDown;
                if self.room.1 == 0 {
                    let movement = match down {
                        true => Move::PageDown,
                        false => Move::PageUp,
                    };
                    self.window.apply(movement, self.room.0, Wrap::No);
                    self.settle_focus();
                    return CompletionOutcome::Consumed;
                }
                let page = usize::from(self.room.1);
                let rows = self.documentation_rows().len();
                self.scrolled = match down {
                    true => (self.scrolled + page).min(rows.saturating_sub(page)),
                    false => self.scrolled.saturating_sub(page),
                };
                CompletionOutcome::Consumed
            }
            // Enter takes what is selected, and only enter. The panel comes
            // up already chosen, so one key is the whole of what it needs --
            // and `tab` is worth more elsewhere than it is as a second way
            // to do what enter does: it steps a snippet's holes, and it
            // walks the tabs of a list, neither of which has another key
            // that reads as well.
            KeyCode::Enter => CompletionOutcome::Accepted,
            KeyCode::Esc => CompletionOutcome::Cancelled,
            _ => CompletionOutcome::Ignored,
        }
    }

    /// Walks the list by a notch of the wheel.
    ///
    /// The list rather than the documentation: obelus is told that a notch
    /// happened and not where the pointer was, and the half a reader is
    /// choosing from is the one worth moving blind.
    pub fn scroll(&mut self, by: isize) {
        if by == 0 {
            return;
        }
        self.window.step(by, Wrap::No);
        self.settle_focus();
    }

    /// A row of documentation for a new candidate is a different document.
    fn settle_focus(&mut self) {
        self.scrolled = 0;
        self.window.settle(self.room.0);
        self.refresh_indices(self.room.0);
    }

    /// Which candidates match the query, best first.
    ///
    /// Ties go to the server's own order -- `sortText` is a server saying
    /// which of two equally good matches it would rather offer, and it
    /// knows things a matcher cannot, like which of them is in scope.
    fn refilter(&mut self) {
        self.matched.clear();
        if self.query.is_empty() {
            self.matched.extend(0..self.candidates.len());
        } else {
            let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
            let mut scored: Vec<(usize, u32)> = Vec::new();
            for (index, candidate) in self.candidates.iter().enumerate() {
                let haystack = Utf32Str::new(&candidate.filter, &mut self.haystack);
                if let Some(score) = pattern.score(haystack, &mut self.matcher) {
                    scored.push((index, score));
                }
            }
            scored.sort_by(|left, right| {
                right.1.cmp(&left.1).then_with(|| {
                    self.candidates[left.0]
                        .sort
                        .cmp(&self.candidates[right.0].sort)
                })
            });
            self.matched
                .extend(scored.into_iter().map(|(index, _)| index));
        }
        self.window.set_count(self.matched.len());
        let shown = || {
            self.matched
                .iter()
                .filter_map(|index| self.candidates.get(*index))
        };
        self.labels = shown()
            .map(|candidate| {
                usize::from(candidate.icon.is_some()) * ICON_COLUMNS
                    + obelus_text::text_width(&candidate.label)
            })
            .max()
            .unwrap_or(0);
        let detail = shown()
            .filter_map(|candidate| candidate.detail.as_deref())
            .map(obelus_text::text_width)
            .max()
            .unwrap_or(0);
        self.width = match detail {
            0 => self.labels,
            detail => self.labels + DETAIL_GAP + detail,
        };
        self.rendered = None;
    }

    /// Which characters matched, for the rows about to be drawn.
    fn refresh_indices(&mut self, height: u16) {
        self.indices.clear();
        if self.query.is_empty() {
            return;
        }
        let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
        for row in self.window.visible(height) {
            let Some(index) = self.matched.get(row) else {
                break;
            };
            let mut indices = Vec::new();
            let haystack = Utf32Str::new(&self.candidates[*index].label, &mut self.haystack);
            pattern.indices(haystack, &mut self.matcher, &mut indices);
            indices.sort_unstable();
            indices.dedup();
            self.indices.push((row, indices));
        }
    }
}
