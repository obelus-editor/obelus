//! The readings a file's bytes have, and the rows they come out as.
//!
//! A reading is what a file is *for*: markdown laid out as prose, a log put
//! in columns. Which one a file has is the file's own business -- that is
//! what keeps [`Mode`](crate::buffer::Mode) down to two -- and this is where
//! that question is answered and where the answer is turned into rows.
//!
//! One row type for every reading, which is the point. What markdown holds
//! and what a log holds are different things while they are being laid out,
//! and by the time they are rows they are the same thing: runs of text with
//! a look. So one piece of code draws either, and adding a reading is a
//! `render` arm rather than a view of its own.

/// A log file, read as the entries it is made of.
///
/// Not [`crate::logging`], which is the log obelus *writes* about itself.
/// This one is a file somebody opens.
pub mod log;
/// Markdown, rendered rather than shown as its own source.
pub mod markdown;

use crate::buffer::Buffer;

/// What a run of text looks like.
///
/// The *screen's* vocabulary rather than markdown's or a log's: a heading
/// and a level are different ideas about the same thing, which is how a run
/// should be drawn. Roles rather than colours, for the reason the syntax
/// layer caches capture indices: switching theme then needs no re-render.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    /// The text itself.
    Plain,
    /// A heading, and how deep it is.
    Heading(u8),
    /// Something quoted from elsewhere: a code span, a field's value.
    Code,
    /// Quieter than the text: a quoted block, a timestamp, a level nobody
    /// reads until they have to.
    Aside,
    /// A mark the reading added rather than the author's words: a bullet, a
    /// table's border, the blanks that hold a column open.
    Mark,
    /// Who said it: a module, a host, a program.
    Name,
    /// What a value is called.
    Key,
    /// Something is wrong.
    Wrong,
    /// Something might be.
    Doubtful,
    /// Code, in the colour the code itself would be.
    ///
    /// What a fenced block in markdown is made of, once the grammar its
    /// fence names has said what each run is. A block with no language, or
    /// one obelus has no grammar for, stays [`Ink::Code`] -- one colour,
    /// which says "this is code" and nothing more.
    Syntax(crate::theme::SyntaxKind),
}

/// A run of text with one look.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    /// The characters.
    pub text: String,
    /// How they are drawn.
    pub ink: Ink,
    /// Whether they are emphasised.
    pub bold: bool,
    /// Whether they are emphasised the other way.
    pub italic: bool,
}

impl Span {
    /// A run with no emphasis, which is every run but markdown's.
    #[must_use]
    pub fn new(text: impl Into<String>, ink: Ink) -> Self {
        Self {
            text: text.into(),
            ink,
            bold: false,
            italic: false,
        }
    }
}

/// One row of a reading, ready to draw.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Row {
    /// The runs, left to right.
    pub spans: Vec<Span>,
    /// Whether the row is a horizontal rule, which has no text of its own.
    pub rule: bool,
}

impl Row {
    /// A row of runs, with no rule.
    #[must_use]
    pub const fn of(spans: Vec<Span>) -> Self {
        Self { spans, rule: false }
    }
}

/// Which reading a file's bytes have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reading {
    /// Markdown, laid out as prose.
    Markdown,
    /// A log, put in columns.
    Log,
}

/// How many lines of a file are read to decide whether it is a log.
///
/// Enough to be sure and few enough to do while a file is being opened.
const HEAD_LINES: usize = 20;

/// The reading a buffer's bytes have, if they have one.
///
/// Markdown by its extension, because markdown looks like the text it came
/// from and sniffing it would be guessing. A log by its *lines*, because a
/// log file is called `syslog` or `access.log` or anything else at all, and
/// the format is the only thing that can say.
#[must_use]
pub fn of(buffer: &Buffer) -> Option<Reading> {
    if is_markdown(buffer.path()) {
        return Some(Reading::Markdown);
    }
    let head: String = buffer
        .text()
        .rope()
        .lines()
        .take(HEAD_LINES)
        .map(|line| line.to_string())
        .collect();
    crate::reading::log::format_of(&head).map(|_| Reading::Log)
}

/// Lays a reading out for a width.
///
/// The one place a reading becomes rows, so everything that shows one --
/// the frame, the scrolling, the status bar's count -- works the same for
/// every reading there is.
#[must_use]
pub fn render(reading: Reading, source: &str, width: u16) -> Vec<Row> {
    match reading {
        Reading::Markdown => crate::reading::markdown::render(source, width),
        Reading::Log => crate::reading::log::render(source, width),
    }
}

/// Whether a path names a markdown file.
///
/// By extension and case-insensitively -- `README.MD` is one -- and by
/// nothing else. Sniffing the contents would be guessing, and markdown's
/// whole trick is that it looks like the text it came from.
#[must_use]
pub fn is_markdown(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}
