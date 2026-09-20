//! What a server said about a place, while it is on screen.
//!
//! Not a list and not a panel that keys go into: one answer about one
//! place, which the reader reads and then leaves. So it owns three keys --
//! the two that scroll it and the one that closes it -- and everything
//! else closes it and goes where it was going, because every other key
//! moves the reader off the place the answer is about.

use obelus_lsp::hover::Hovered;
use obelus_reading::Row;
use obelus_text::coordinates::{CharColumn, LineNumber, Span};

/// The most rows of it to show at once.
///
/// A hover about a trait method can be a page; twelve rows is a paragraph
/// and a signature, which is what a reader asked for, and the rest is a
/// page down away.
pub const MOST_ROWS: u16 = 12;

/// What was said, and how much of it is on screen.
pub struct Hover {
    /// What the server said, as markdown.
    markdown: String,
    /// The characters it is about, as a list because that is the shape the
    /// view marks runs in.
    range: Vec<Span>,
    /// Where it was asked about.
    at: (LineNumber, CharColumn),
    /// Whether the pointer asked, rather than a key.
    ///
    /// What closes it differs: a pointer that has moved off the word has
    /// stopped asking, while a caret that has not moved is still there.
    pointed: bool,
    /// How far down it the reader has read.
    scrolled: usize,
    /// The markdown, laid out for the width it was last drawn at.
    rendered: Option<(u16, Vec<Row>)>,
}

impl std::fmt::Debug for Hover {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Hover")
            .field("at", &self.at)
            .field("pointed", &self.pointed)
            .field("markdown", &self.markdown.len())
            .finish_non_exhaustive()
    }
}

impl Hover {
    /// What a server said, ready to be drawn.
    #[must_use]
    pub fn new(hovered: Hovered, at: (LineNumber, CharColumn), pointed: bool) -> Self {
        Self {
            markdown: hovered.markdown,
            range: hovered.range.into_iter().collect(),
            at,
            pointed,
            scrolled: 0,
            rendered: None,
        }
    }

    /// Where it was asked about.
    #[must_use]
    pub const fn at(&self) -> (LineNumber, CharColumn) {
        self.at
    }

    /// Whether the pointer asked for it.
    #[must_use]
    pub const fn pointed(&self) -> bool {
        self.pointed
    }

    /// The characters the server said it is about.
    #[must_use]
    pub fn range(&self) -> &[Span] {
        &self.range
    }

    /// How far down it the reader has read.
    #[must_use]
    pub const fn scrolled(&self) -> usize {
        self.scrolled
    }

    /// Lays the markdown out for a width, if it is not laid out already.
    pub fn settle(&mut self, width: u16, height: u16) {
        if self.rendered.as_ref().is_none_or(|(was, _)| *was != width) {
            self.rendered = Some((
                width,
                obelus_reading::markdown::render(&self.markdown, width),
            ));
        }
        let rows = self.rows().len();
        self.scrolled = self.scrolled.min(rows.saturating_sub(usize::from(height)));
    }

    /// What [`Hover::settle`] laid out.
    #[must_use]
    pub fn rows(&self) -> &[Row] {
        self.rendered
            .as_ref()
            .map_or(&[], |(_, rows)| rows.as_slice())
    }

    /// How many rows it wants, up to what it is allowed.
    #[must_use]
    pub fn wanted(&self) -> u16 {
        u16::try_from(self.rows().len())
            .unwrap_or(MOST_ROWS)
            .min(MOST_ROWS)
    }

    /// Moves it by a notch of the wheel.
    ///
    /// Only forward: how far it can go is [`Hover::settle`]'s, because the
    /// answer is laid out afresh for whatever width it is drawn at and the
    /// number of rows it has is not known here.
    pub fn scroll(&mut self, rows: isize) {
        self.scrolled = self.scrolled.saturating_add_signed(rows);
    }

    /// Moves a screenful of it.
    ///
    /// How far it can go is the settling's, as it is for the wheel: one
    /// answer to "how much of this is there" rather than three.
    pub fn page(&mut self, down: bool, height: u16) {
        let page = usize::from(height).max(1);
        self.scrolled = match down {
            true => self.scrolled + page,
            false => self.scrolled.saturating_sub(page),
        };
    }
}
