//! What a server would have you read that the file does not say.
//!
//! A type nobody wrote down, the name of the parameter an argument is being
//! passed to. The file is complete without them -- they are what the
//! compiler worked out -- and a reader who has to hold all of it in their
//! head is reading the file the compiler has rather than the one on disk.
//!
//! Cells the file does not contain, which obelus draws through
//! [`crate::text::Phantom`]: every column after one of them is drawn one
//! cell further along, and that arithmetic happens in the one module that
//! is allowed to do it. Which is also what keeps the caret out of them --
//! a hint occupies no character, so there is nothing in it to stand on,
//! nothing in it to select and nothing in it to copy.

use lsp_types::{InlayHint, InlayHintLabel, PositionEncodingKind, ServerCapabilities};
use serde_json::Value;
use unicode_width::UnicodeWidthStr;

use crate::{
    coordinates::{CharColumn, LineNumber},
    text::Text,
};

/// One of them: what it says, and where it is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hinted {
    /// Which line it is drawn in.
    pub line: LineNumber,
    /// The character it is drawn in front of.
    pub column: CharColumn,
    /// What it says, with whatever blanks the server asked for either
    /// side.
    ///
    /// One string, although the protocol may send it in parts. The parts
    /// carry a place to jump to and a tooltip each, which is a thing to
    /// build when there is a key that uses them; what they say is the
    /// whole of what is drawn.
    pub label: String,
    /// What sort of hint it is, for the colour it is drawn in.
    pub kind: Kind,
}

/// What a hint is telling the reader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A type nobody wrote down.
    Type,
    /// The name of the parameter an argument is passed to.
    Parameter,
    /// Something else. A server may send a hint with no kind at all, and
    /// the protocol says nothing about what it is then.
    Other,
}

impl Hinted {
    /// How many cells it takes on screen.
    ///
    /// The width of what is written rather than the length of it: a hint
    /// is a piece of somebody's source language and may be full of
    /// characters a terminal draws two cells wide.
    #[must_use]
    pub fn cells(&self) -> usize {
        self.label.width()
    }
}

/// Whether the server answers `textDocument/inlayHint`.
#[must_use]
pub const fn supported(capabilities: &ServerCapabilities) -> bool {
    capabilities.inlay_hint_provider.is_some()
}

/// What a server said, in this document's own coordinates.
///
/// Nothing for an answer that is not a list, which is what a server with
/// nothing to say sends: a file of comments has no types to work out.
#[must_use]
pub fn in_reply(
    result: &Result<Value, String>,
    text: &Text,
    encoding: &PositionEncodingKind,
) -> Vec<Hinted> {
    let Ok(value) = result else {
        return Vec::new();
    };
    let Ok(found) = serde_json::from_value::<Vec<InlayHint>>(value.clone()) else {
        return Vec::new();
    };
    found
        .into_iter()
        .filter_map(|hint| {
            let label = said(&hint.label, hint.padding_left, hint.padding_right);
            // A hint with nothing in it is a cell taken from the reader to
            // say nothing, and servers do send them.
            if label.is_empty() {
                return None;
            }
            let (line, column) = super::position::from_lsp(text, hint.position, encoding);
            Some(Hinted {
                line,
                column,
                label,
                kind: match hint.kind {
                    Some(lsp_types::InlayHintKind::TYPE) => Kind::Type,
                    Some(lsp_types::InlayHintKind::PARAMETER) => Kind::Parameter,
                    _ => Kind::Other,
                },
            })
        })
        .collect()
}

/// What a hint says, whichever of the two shapes it arrived in.
///
/// With the blanks the server asked for, folded in rather than kept beside
/// it: they are part of how wide the hint is, and a width worked out from
/// the label alone would be a width the line is measured by and drawn at
/// differently.
fn said(label: &InlayHintLabel, before: Option<bool>, after: Option<bool>) -> String {
    let mut said = String::new();
    if before == Some(true) {
        said.push(' ');
    }
    match label {
        InlayHintLabel::String(whole) => said.push_str(whole),
        InlayHintLabel::LabelParts(parts) => {
            for part in parts {
                said.push_str(&part.value);
            }
        }
    }
    if after == Some(true) {
        said.push(' ');
    }
    // A hint is drawn on one row. A label with a newline in it would put
    // the rest of the line somewhere the arithmetic does not expect.
    said.retain(|character| character != '\n' && character != '\r');
    said
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text() -> Text {
        Text::from_string("let x = compute();\n")
    }

    /// The two shapes a label arrives in are one string by the time
    /// anything draws it.
    #[test]
    fn a_label_is_read_whole_whichever_shape_it_came_in() {
        let found = in_reply(
            &Ok(serde_json::json!([
                {
                    "position": { "line": 0, "character": 5 },
                    "label": ": i32",
                    "kind": 1
                },
                {
                    "position": { "line": 0, "character": 16 },
                    "label": [{ "value": "count" }, { "value": ": " }],
                    "kind": 2
                }
            ])),
            &text(),
            &PositionEncodingKind::UTF16,
        );
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].label, ": i32");
        assert_eq!(found[0].kind, Kind::Type);
        assert_eq!(found[1].label, "count: ", "the parts were not read whole");
        assert_eq!(found[1].kind, Kind::Parameter);
    }

    /// The blanks a server asks for are part of the hint, because they are
    /// part of how wide it is drawn.
    #[test]
    fn the_padding_a_server_asks_for_is_part_of_the_width() {
        let found = in_reply(
            &Ok(serde_json::json!([{
                "position": { "line": 0, "character": 5 },
                "label": "i32",
                "paddingLeft": true,
                "paddingRight": true
            }])),
            &text(),
            &PositionEncodingKind::UTF16,
        );
        assert_eq!(found[0].label, " i32 ");
        assert_eq!(found[0].cells(), 5, "the blanks were not counted");
    }

    /// How wide it is drawn is what the terminal draws it as, which is not
    /// how many characters it has.
    #[test]
    fn a_hint_is_as_wide_as_the_terminal_draws_it() {
        let hint = Hinted {
            line: LineNumber::new(0),
            column: CharColumn::new(0),
            label: ": 文字".to_string(),
            kind: Kind::Type,
        };
        assert_eq!(hint.label.chars().count(), 4);
        assert_eq!(hint.cells(), 6, "two of those are drawn two cells wide");
    }

    /// A hint saying nothing is a cell taken from the reader to say
    /// nothing.
    #[test]
    fn a_hint_with_nothing_in_it_is_not_drawn() {
        let found = in_reply(
            &Ok(serde_json::json!([
                { "position": { "line": 0, "character": 5 }, "label": "" },
                { "position": { "line": 0, "character": 5 }, "label": [] }
            ])),
            &text(),
            &PositionEncodingKind::UTF16,
        );
        assert!(found.is_empty());
    }

    /// A label drawn across two rows would put the rest of the line
    /// somewhere the arithmetic does not expect.
    #[test]
    fn a_label_is_one_row_however_it_arrived() {
        let found = in_reply(
            &Ok(serde_json::json!([{
                "position": { "line": 0, "character": 5 },
                "label": ": Result<\n    i32,\n>"
            }])),
            &text(),
            &PositionEncodingKind::UTF16,
        );
        assert_eq!(found[0].label, ": Result<    i32,>");
        assert_eq!(found[0].cells(), found[0].label.width());
    }

    /// A server with nothing to say says it in several ways.
    #[test]
    fn nothing_said_is_nothing_drawn() {
        for answer in [
            Ok(serde_json::json!(null)),
            Ok(serde_json::json!([])),
            Err("no".to_string()),
        ] {
            assert!(in_reply(&answer, &text(), &PositionEncodingKind::UTF16).is_empty());
        }
    }
}
