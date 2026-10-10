//! What a server says is wrong with a file.
//!
//! Diagnostics arrive unasked: a server sends them when it has looked at a
//! document, again when it has looked again, and the last set it sent for a
//! file is the whole truth about that file -- there is no "one more" and no
//! way to ask for them. So they are kept per path and replaced wholesale.
//!
//! The ranges are turned into the document's own coordinates on the way in,
//! like every other range the protocol sends: a position in the encoding a
//! server agreed to is a thing only [`crate`] should hold.
//!
//! **A diagnostic is a mark against a piece of a file, and a server is not
//! the only thing that can make one.** Obelus marks what it cannot make of a
//! file it reads for its own sake into the same list a server's go in, as a
//! [`Trouble`] like these, and from there down they are the same thing:
//! nothing that draws one has to be told which kind it is holding. `source`
//! is what says who noticed, and Obelus fills in its own name -- which is
//! also what tells its own from a server's when one of them is taken away
//! again, because they keep different rules. A server's set for a path is
//! replaced whole when it publishes another; that is the protocol's, not
//! Obelus's to apply on a server's behalf. Placed already, unlike a
//! server's, because Obelus has the text in its hand at the moment it finds
//! the fault. The application's half is `App::obelus_says`.

use lsp_types::{DiagnosticSeverity, PositionEncodingKind, PublishDiagnosticsParams};
pub use obelus_text::severity::Severity;
use obelus_text::{Text, coordinates::Span};
use serde_json::Value;

/// How bad the protocol's word for it is.
fn severity_of(severity: Option<DiagnosticSeverity>) -> Severity {
    match severity {
        Some(DiagnosticSeverity::WARNING) => Severity::Warning,
        Some(DiagnosticSeverity::INFORMATION) => Severity::Information,
        Some(DiagnosticSeverity::HINT) => Severity::Hint,
        // Unset means the server did not say, and the protocol leaves
        // it to the client. An error is the reading that gets looked
        // at, which is the right way round to be wrong.
        _ => Severity::Error,
    }
}

/// One thing a server says about a piece of a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trouble {
    /// What it is about.
    pub span: Span,
    /// How bad.
    pub severity: Severity,
    /// What the server said, as it said it.
    pub message: String,
    /// Which tool said it: `rustc`, `clippy`, a linter behind the server.
    pub source: Option<String>,
    /// The diagnostic as it arrived.
    ///
    /// Kept whole because it goes back: a code action is asked for *about*
    /// diagnostics, and the server matches them by every field it sent --
    /// the code, the data, the related information -- not by the message.
    pub item: Value,
}

/// Everything a server just said about one file, placed in it.
///
/// The same news [`reported`] reads, counted against the text it is about,
/// and carrying each diagnostic as it arrived: a code action is asked
/// *about* a diagnostic and the server matches it by every field it sent.
///
/// In arrival order, which is the order the raw diagnostics are in and the
/// only order the two can be paired in. Whoever keeps them puts them in
/// the file's order.
#[must_use]
pub fn published(params: &Value, text: &Text, encoding: &PositionEncodingKind) -> Vec<Trouble> {
    let raw = diagnostics_of(params);
    reported(params)
        .iter()
        .enumerate()
        .map(|(at, trouble)| Trouble {
            item: raw.get(at).cloned().unwrap_or(Value::Null),
            ..trouble.placed(text, encoding)
        })
        .collect()
}

/// The diagnostics of a notification, untouched.
fn diagnostics_of(params: &Value) -> Vec<Value> {
    params
        .get("diagnostics")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// The path a `publishDiagnostics` notification is about.
#[must_use]
pub fn path_of(params: &Value) -> Option<std::path::PathBuf> {
    super::path_of_uri(params.get("uri")?.as_str()?)
}

/// One thing a server says about a file, in the units it said it in.
///
/// The same news as a [`Trouble`] and none of the placing. A range becomes
/// a place in a document by counting against that document's text, and a
/// file Obelus has not read is one there is nothing to count against -- so
/// for those nothing is counted, and what arrived is kept as it arrived.
///
/// Which costs a list nothing: a row that names a place carries the
/// protocol's units anyway. This is the form the whole project is held in;
/// [`Reported::placed`] makes the other one, wherever a column has to line
/// up with a character on screen -- under the caret in the file being read,
/// or under a line of a preview of somewhere else.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reported {
    /// Where it starts, as the server counts.
    pub line: u32,
    /// The column it starts at, in the encoding the server agreed to.
    pub character: u32,
    /// Where it ends.
    pub end_line: u32,
    /// The column it ends at.
    pub end_character: u32,
    /// How bad.
    pub severity: Severity,
    /// What the server said, as it said it.
    pub message: String,
    /// Which tool said it.
    pub source: Option<String>,
}

impl Reported {
    /// The first line of the message.
    ///
    /// rustc writes paragraphs -- the explanation, the help, the note --
    /// and a row of a list has one line. The rest is in the message itself
    /// for whatever shows the whole of it.
    #[must_use]
    pub fn summary(&self) -> &str {
        self.message.lines().next().unwrap_or_default()
    }

    /// Where it is in a document, once there is a document to count it
    /// against.
    ///
    /// Without the diagnostic it arrived as: placing is for drawing, and
    /// what is drawn -- an underline, the words under a line -- asks
    /// nobody anything. [`published`] fills that in for the copy a code
    /// action can be asked about.
    #[must_use]
    pub fn placed(&self, text: &Text, encoding: &PositionEncodingKind) -> Trouble {
        let place = |line, character| {
            super::position::from_lsp(text, lsp_types::Position { line, character }, encoding)
        };
        let (line, column) = place(self.line, self.character);
        let (end_line, end_column) = place(self.end_line, self.end_character);
        Trouble {
            span: Span {
                line,
                column,
                end_line,
                end_column,
            },
            severity: self.severity,
            message: self.message.clone(),
            source: self.source.clone(),
            item: Value::Null,
        }
    }
}

/// Everything a server just said about one file, unplaced.
///
/// No text and no encoding, because neither is needed and neither is to be
/// had: this is the reading of a notification that works for a file Obelus
/// does not have open.
///
/// In the order they were sent, which is not the order they are in the
/// file: whoever keeps them sorts them, and does it for both readings at
/// once so that the two cannot disagree.
#[must_use]
pub fn reported(params: &Value) -> Vec<Reported> {
    let Ok(params) = serde_json::from_value::<PublishDiagnosticsParams>(params.clone()) else {
        return Vec::new();
    };
    params
        .diagnostics
        .into_iter()
        .map(|diagnostic| Reported {
            line: diagnostic.range.start.line,
            character: diagnostic.range.start.character,
            end_line: diagnostic.range.end.line,
            end_character: diagnostic.range.end.character,
            severity: severity_of(diagnostic.severity),
            message: diagnostic.message,
            source: diagnostic.source,
        })
        .collect()
}
