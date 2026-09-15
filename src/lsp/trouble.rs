//! What a server says is wrong with a file.
//!
//! Diagnostics arrive unasked: a server sends them when it has looked at a
//! document, again when it has looked again, and the last set it sent for a
//! file is the whole truth about that file -- there is no "one more" and no
//! way to ask for them. So they are kept per path and replaced wholesale.
//!
//! The ranges are turned into the document's own coordinates on the way in,
//! like every other range the protocol sends: a position in the encoding a
//! server agreed to is a thing only [`crate::lsp`] should hold.

use lsp_types::{DiagnosticSeverity, PositionEncodingKind, PublishDiagnosticsParams};
use serde_json::Value;

use crate::{coordinates::Span, text::Text, theme::SyntaxKind};

/// How bad a server says something is.
///
/// Four, because the protocol has four and a reader can tell them apart:
/// what stops the build, what is worth reading, and two kinds of remark.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Something that will not compile.
    Error,
    /// Something that will, and should not.
    Warning,
    /// A remark.
    Information,
    /// A suggestion, usually about style.
    Hint,
}

impl Severity {
    /// The colour it is drawn in.
    ///
    /// The file's own colours, as everything in obelus is: an error is
    /// what an error in a log is, a warning what a warning is. The two
    /// quieter ones take the colour of a comment, which is what they read
    /// as -- something written beside the code rather than about it.
    #[must_use]
    pub const fn kind(self) -> SyntaxKind {
        match self {
            Self::Error => SyntaxKind::Error,
            Self::Warning => SyntaxKind::Warning,
            Self::Information | Self::Hint => SyntaxKind::Comment,
        }
    }

    /// The mark that stands for it where there is no room for a word.
    ///
    /// Ordinary Unicode rather than a Nerd Font glyph: this goes on the
    /// status row, which is on screen the whole time, so it cannot depend
    /// on a font obelus has not been told about.
    #[must_use]
    pub const fn mark(self) -> char {
        match self {
            Self::Error => '\u{00d7}',
            Self::Warning => '\u{0021}',
            Self::Information | Self::Hint => '\u{00b7}',
        }
    }

    /// What to call it.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Information => "information",
            Self::Hint => "hint",
        }
    }

    fn of(severity: Option<DiagnosticSeverity>) -> Self {
        match severity {
            Some(DiagnosticSeverity::WARNING) => Self::Warning,
            Some(DiagnosticSeverity::INFORMATION) => Self::Information,
            Some(DiagnosticSeverity::HINT) => Self::Hint,
            // Unset means the server did not say, and the protocol leaves
            // it to the client. An error is the reading that gets looked
            // at, which is the right way round to be wrong.
            _ => Self::Error,
        }
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
}

impl Trouble {
    /// The first line of the message.
    ///
    /// rustc writes paragraphs -- the explanation, the help, the note --
    /// and a row of a list has one line. The rest is in the message itself
    /// for whatever shows the whole of it.
    #[must_use]
    pub fn summary(&self) -> &str {
        self.message.lines().next().unwrap_or_default()
    }
}

/// Everything a server just said about one file.
///
/// `None` for a notification about a file obelus does not have open, or
/// one it cannot make a path of: there is nowhere to put it and nothing
/// that would read it.
#[must_use]
pub fn published(params: &Value, text: &Text, encoding: &PositionEncodingKind) -> Vec<Trouble> {
    let Ok(params) = serde_json::from_value::<PublishDiagnosticsParams>(params.clone()) else {
        return Vec::new();
    };
    params
        .diagnostics
        .into_iter()
        .map(|diagnostic| {
            let (line, column) = super::position::from_lsp(text, diagnostic.range.start, encoding);
            let (end_line, end_column) =
                super::position::from_lsp(text, diagnostic.range.end, encoding);
            Trouble {
                span: Span {
                    line,
                    column,
                    end_line,
                    end_column,
                },
                severity: Severity::of(diagnostic.severity),
                message: diagnostic.message,
                source: diagnostic.source,
            }
        })
        .collect()
}

/// The path a `publishDiagnostics` notification is about.
#[must_use]
pub fn path_of(params: &Value) -> Option<std::path::PathBuf> {
    let uri = params.get("uri")?.as_str()?;
    let path = uri.strip_prefix("file://")?;
    Some(std::path::PathBuf::from(
        percent_decode(path).unwrap_or_else(|| path.to_string()),
    ))
}

/// A uri's escapes, undone.
///
/// Only the ones a path can carry: a server sends back the uri obelus gave
/// it, and obelus builds those with the same escaping.
fn percent_decode(text: &str) -> Option<String> {
    if !text.contains('%') {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        if character != '%' {
            out.push(character);
            continue;
        }
        let high = characters.next()?.to_digit(16)?;
        let low = characters.next()?.to_digit(16)?;
        out.push(char::from_u32(high * 16 + low)?);
    }
    Some(out)
}
