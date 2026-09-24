//! What a server says a piece of a file is.
//!
//! One answer, in markdown, about one place -- which is what makes it the
//! simplest thing a language server sends and the one Obelus can show with
//! what it already has: rust-analyzer's hover is a fenced code block
//! holding the signature, a rule, and the documentation under it, and
//! Obelus renders exactly that for a README.
//!
//! The range is the other half of the answer and the half editors usually
//! throw away: the server says *which characters* it is talking about, and
//! a reader who asked about a long line should be able to see which word
//! they got an answer about.

use lsp_types::{Hover, HoverContents, MarkedString, MarkupContent, PositionEncodingKind};
use obelus_text::{Text, coordinates::Span};
use serde_json::Value;

/// What a server said about a place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hovered {
    /// What it said, as markdown.
    pub markdown: String,
    /// The characters it is about, where the server said.
    pub range: Option<Span>,
}

/// Whether the server answers `textDocument/hover`.
#[must_use]
pub const fn supported(capabilities: &lsp_types::ServerCapabilities) -> bool {
    capabilities.hover_provider.is_some()
}

/// What a server's answer says, if it says anything.
///
/// Nothing for an empty answer, and nothing for one whose markdown is
/// blank: a box with nothing in it is worse than no box, because it covers
/// code to say that there was nothing to say.
#[must_use]
pub fn in_reply(
    result: &Result<Value, String>,
    text: &Text,
    encoding: &PositionEncodingKind,
) -> Option<Hovered> {
    let Ok(value) = result else {
        return None;
    };
    let hover = serde_json::from_value::<Option<Hover>>(value.clone()).ok()??;
    let markdown = match hover.contents {
        HoverContents::Markup(MarkupContent { value, .. }) => value,
        // The two shapes the protocol has kept for compatibility. A
        // language and a string is a fenced block written the long way,
        // which is what it becomes.
        HoverContents::Scalar(one) => fenced(one),
        HoverContents::Array(many) => many
            .into_iter()
            .map(fenced)
            .collect::<Vec<_>>()
            .join("\n\n"),
    };
    if markdown.trim().is_empty() {
        return None;
    }
    let range = hover.range.map(|range| {
        let (line, column) = super::position::from_lsp(text, range.start, encoding);
        let (end_line, end_column) = super::position::from_lsp(text, range.end, encoding);
        Span {
            line,
            column,
            end_line,
            end_column,
        }
    });
    Some(Hovered { markdown, range })
}

/// One of the old shapes, as the markdown it stands for.
fn fenced(one: MarkedString) -> String {
    match one {
        MarkedString::String(text) => text,
        MarkedString::LanguageString(marked) => {
            format!("```{}\n{}\n```", marked.language, marked.value)
        }
    }
}
