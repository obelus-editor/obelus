//! The readings a file's bytes have.
//!
//! A reading is what a file is *for*: markdown laid out as prose, a log put
//! in columns. Which one a file has is the file's own business -- that is
//! what keeps a buffer's `Mode` down to two -- and this is where that
//! question is answered.
//!
//! About *files*, which is what tells this from the laying out itself. A
//! language server's hover and an agent's message are markdown too, and
//! neither of them is a file with a reading: they go straight to
//! [`obelus_markdown`] and never come past here. What is here is the
//! question only a file can be asked.

/// A log file, read as the entries it is made of.
///
/// Not obelus's own logging, which is the log it *writes* about itself.
/// This one is a file somebody opens.
pub mod log;

use std::path::Path;

use obelus_row::Row;
use obelus_text::Text;

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

/// The reading a file's bytes have, if they have one.
///
/// A path and the text rather than the buffer holding them: what the
/// question needs is a name and some lines, and asking for the buffer
/// would mean everything that can answer it has to know what a buffer
/// is.
///
/// Markdown by its extension, because markdown looks like the text it came
/// from and sniffing it would be guessing. A log by its *lines*, because a
/// log file is called `syslog` or `access.log` or anything else at all, and
/// the format is the only thing that can say.
#[must_use]
pub fn of(path: &Path, text: &Text) -> Option<Reading> {
    if is_markdown(path) {
        return Some(Reading::Markdown);
    }
    let head: String = text
        .rope()
        .lines()
        .take(HEAD_LINES)
        .map(|line| line.to_string())
        .collect();
    crate::log::format_of(&head).map(|_| Reading::Log)
}

/// Lays a reading out for a width.
///
/// The one place a reading becomes rows, so everything that shows one --
/// the frame, the scrolling, the status bar's count -- works the same for
/// every reading there is.
#[must_use]
pub fn render(reading: Reading, source: &str, width: u16) -> Vec<Row> {
    match reading {
        Reading::Markdown => obelus_markdown::render(source, width),
        Reading::Log => crate::log::render(source, width),
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
