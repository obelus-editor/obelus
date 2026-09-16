//! Every use of the name under the caret, in this file.
//!
//! The protocol calls it a document highlight, which names the drawing
//! rather than the question. The question is the one a reader asks by
//! putting the caret on a name: where else is this? -- and the answer is
//! worth having without asking, because it is the cheapest thing a server
//! knows and the one most often wanted.

use lsp_types::{DocumentHighlight, PositionEncodingKind, ServerCapabilities};
use serde_json::Value;

use crate::{coordinates::Span, text::Text};

/// Whether the server answers `textDocument/documentHighlight`.
#[must_use]
pub const fn supported(capabilities: &ServerCapabilities) -> bool {
    capabilities.document_highlight_provider.is_some()
}

/// The runs a server's answer names.
///
/// The kinds -- read, write, text -- are dropped. Marking a write
/// differently from a read is a thing some editors do and a thing obelus
/// has nowhere to put: the mark is a background, there is one of them, and
/// a second would be a colour a reader has to learn.
#[must_use]
pub fn in_reply(
    result: &Result<Value, String>,
    text: &Text,
    encoding: &PositionEncodingKind,
) -> Vec<Span> {
    let Ok(value) = result else {
        return Vec::new();
    };
    let Ok(found) = serde_json::from_value::<Option<Vec<DocumentHighlight>>>(value.clone()) else {
        return Vec::new();
    };
    found
        .unwrap_or_default()
        .into_iter()
        .map(|highlight| {
            let (line, column) = super::position::from_lsp(text, highlight.range.start, encoding);
            let (end_line, end_column) =
                super::position::from_lsp(text, highlight.range.end, encoding);
            Span {
                line,
                column,
                end_line,
                end_column,
            }
        })
        .collect()
}
