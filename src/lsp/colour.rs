//! Where a colour is written down, and which colour it is.
//!
//! A server that knows a language knows its colours: `#3264eb` in CSS,
//! `rgba(0, 0, 0, .5)`, a named constant a framework resolves. What it
//! sends back is a range and three numbers, and what obelus does with them
//! is paint the run itself -- so a reader looking at a stylesheet sees the
//! colours rather than reads them.
//!
//! Painted rather than marked with a swatch beside it. A glyph in front of
//! the literal would move every character after it one cell right, and a
//! screen whose columns are not the file's columns is a screen that lies
//! about where things are -- which is the one thing every other decoration
//! here is careful not to do.

use lsp_types::{ColorInformation, PositionEncodingKind, ServerCapabilities};
use ratatui::style::Color;
use serde_json::Value;

use crate::{coordinates::Span, text::Text};

/// A colour, and the characters that say it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Coloured {
    /// The characters the server pointed at.
    pub span: Span,
    /// What they mean, ready to paint with.
    pub colour: Color,
    /// What to write on it so the writing can still be read.
    ///
    /// Worked out here rather than where it is drawn: it is a fact about
    /// the colour, and the one place that knows the colour should be the
    /// one place that answers for it.
    pub ink: Color,
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
                ink: readable_on(found.color),
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

/// Black or white, whichever can be read on a colour.
///
/// By how bright the colour is to the eye rather than by its average: the
/// eye takes green for most of the brightness of a colour and blue for
/// almost none, so an average calls `#0000ff` light and puts black on it.
/// The weights are the ones every contrast rule uses.
fn readable_on(colour: lsp_types::Color) -> Color {
    let brightness = 0.299 * colour.red + 0.587 * colour.green + 0.114 * colour.blue;
    match brightness > 0.55 {
        true => Color::Rgb(0, 0, 0),
        false => Color::Rgb(255, 255, 255),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text() -> Text {
        Text::from_string("a { color: #3264eb; }\n")
    }

    /// The range a server names becomes the characters obelus paints.
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

    /// What is written on it can be read, which is not a question about
    /// the average of the three numbers.
    #[test]
    fn the_ink_is_whichever_can_be_read() {
        let on = |red, green, blue| {
            readable_on(lsp_types::Color {
                red,
                green,
                blue,
                alpha: 1.0,
            })
        };
        assert_eq!(on(1.0, 1.0, 1.0), Color::Rgb(0, 0, 0), "white");
        assert_eq!(on(0.0, 0.0, 0.0), Color::Rgb(255, 255, 255), "black");
        // Pure blue is dark to the eye however large the number is, and an
        // average of the three would call it light.
        assert_eq!(on(0.0, 0.0, 1.0), Color::Rgb(255, 255, 255), "blue");
        // And pure green is light, for the same reason the other way.
        assert_eq!(on(0.0, 1.0, 0.0), Color::Rgb(0, 0, 0), "green");
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
