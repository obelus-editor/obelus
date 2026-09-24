//! Where a colour is written down, and which colour it is.
//!
//! A server that knows a language knows its colours: `#3264eb` in CSS,
//! `rgba(0, 0, 0, .5)`, a named constant a framework resolves. What it
//! sends back is a range and three numbers, and what Obelus draws from them
//! is a cell of that colour in front of the literal -- so a reader looking
//! at a stylesheet sees the colours rather than reads them.
//!
//! In front of it rather than over it. Painting the literal said the same
//! thing over seven characters, took its syntax colour away to say it, and
//! over the sixteen of an `rgba(0, 0, 0, .5)` said it over half a line.
//!
//! A cell in front of a literal is a cell the file does not contain, which
//! is a thing Obelus refused to draw for a long time: a screen whose
//! columns are not the file's columns is a screen that lies about where
//! things are. What makes it honest now is that the lie is told in one
//! place -- [`obelus_text::Phantom`], in the module that is the only place
//! a coordinate turns into another one -- so the cursor, the pointer, the
//! wrapping and every mark painted on a line all agree about where the
//! fourth character of a line is drawn.

use lsp_types::{ColorInformation, PositionEncodingKind, ServerCapabilities};
use obelus_text::{Text, coordinates::Span};
use ratatui::style::Color;
use serde_json::Value;

/// A colour, and the characters that say it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Coloured {
    /// The characters the server pointed at.
    pub span: Span,
    /// What they mean, ready to paint with.
    pub colour: Color,
}

/// Whether the server answers `textDocument/documentColor`.
#[must_use]
pub const fn supported(capabilities: &ServerCapabilities) -> bool {
    capabilities.color_provider.is_some()
}

/// What a server said, in this document's own coordinates.
///
/// Nothing for an answer that is not a list, which is what a server with
/// nothing to say sends: an empty file has no colours in it and neither
/// does a language without any.
#[must_use]
pub fn in_reply(
    result: &Result<Value, String>,
    text: &Text,
    encoding: &PositionEncodingKind,
) -> Vec<Coloured> {
    let Ok(value) = result else {
        return Vec::new();
    };
    let Ok(found) = serde_json::from_value::<Vec<ColorInformation>>(value.clone()) else {
        return Vec::new();
    };
    found
        .into_iter()
        .map(|found| {
            let (line, column) = super::position::from_lsp(text, found.range.start, encoding);
            let (end_line, end_column) = super::position::from_lsp(text, found.range.end, encoding);
            let colour = rgb(found.color);
            Coloured {
                span: Span {
                    line,
                    column,
                    end_line,
                    end_column,
                },
                colour,
            }
        })
        .collect()
}

/// The protocol's colour as a terminal one.
///
/// The alpha is dropped. There is nothing behind a cell to blend with --
/// the terminal paints one background per cell -- and a half-transparent
/// colour shown at full strength is nearer the truth than one blended
/// against a guess about what is underneath.
fn rgb(colour: lsp_types::Color) -> Color {
    Color::Rgb(byte(colour.red), byte(colour.green), byte(colour.blue))
}

/// One channel, as the protocol gives it: a fraction of full.
fn byte(channel: f32) -> u8 {
    // Clamped because the protocol says [0, 1] and says nothing about what
    // a server that ignores that sends.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped into the byte range on the line above"
    )]
    {
        (channel.clamp(0.0, 1.0) * 255.0).round() as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text() -> Text {
        Text::from_string("a { color: #3264eb; }\n")
    }

    /// The range a server names becomes the characters Obelus paints.
    #[test]
    fn a_colour_is_where_the_server_said_and_what_it_said() {
        let found = in_reply(
            &Ok(serde_json::json!([{
                "range": { "start": { "line": 0, "character": 11 },
                           "end": { "line": 0, "character": 18 } },
                "color": { "red": 0.196, "green": 0.392, "blue": 0.922, "alpha": 1.0 }
            }])),
            &text(),
            &PositionEncodingKind::UTF16,
        );
        assert_eq!(found.len(), 1);
        assert_eq!(
            (found[0].span.column.get(), found[0].span.end_column.get()),
            (11, 18)
        );
        assert_eq!(found[0].colour, Color::Rgb(50, 100, 235));
    }

    /// A server with nothing to say says it in several ways, and none of
    /// them is a colour.
    #[test]
    fn nothing_said_is_no_colours() {
        for answer in [
            Ok(serde_json::json!(null)),
            Ok(serde_json::json!([])),
            Err("no".to_string()),
        ] {
            assert!(in_reply(&answer, &text(), &PositionEncodingKind::UTF16).is_empty());
        }
    }
}
