//! Positions as the protocol counts them.
//!
//! A protocol position is a line and a count into it, and which count depends
//! on what the server agreed to. The cases here all contain a wide glyph or a
//! tab, because that is where the two encodings stop agreeing -- on pure
//! ASCII a broken conversion still passes.

use obelus_text::{
    Text,
    coordinates::{CharColumn, LineNumber},
};

/// Getting the units wrong points at a different character on every line
/// that is not plain ASCII.
#[test]
fn protocol_positions_round_trip_in_both_encodings() {
    use lsp_types::PositionEncodingKind;
    use obelus_lsp::position::{from_lsp, to_lsp};

    let samples = [
        "plain ascii here\n",
        "let s = \"\u{4f60}\u{597d}\u{4e16}\u{754c}\";\n",
        "let party = \"\u{1f389}\u{1f680}\";\n",
        "\tlet indented = 1;\n",
    ];
    for encoding in [PositionEncodingKind::UTF8, PositionEncodingKind::UTF16] {
        for source in samples {
            let text = Text::from_string(source);
            for line in 0..text.line_count() {
                let line = LineNumber::new(line);
                for column in 0..=text.line_length(line).get() {
                    let column = CharColumn::new(column);
                    let position = to_lsp(&text, line, column, &encoding);
                    assert_eq!(
                        from_lsp(&text, position, &encoding),
                        (line, column),
                        "{encoding:?} {source:?} {line:?} {column:?}"
                    );
                }
            }
        }
    }
}

/// The two encodings disagree, and the point of keeping them apart is that
/// the numbers really are different.
#[test]
fn the_two_encodings_give_different_numbers_past_a_wide_character() {
    use lsp_types::PositionEncodingKind;
    use obelus_lsp::position::to_lsp;

    let text = Text::from_string("let s = \"\u{4f60}\u{597d}\";\n");
    let line = LineNumber::new(0);
    // Just past the two Han characters.
    let column = CharColumn::new(11);

    let bytes = to_lsp(&text, line, column, &PositionEncodingKind::UTF8);
    let units = to_lsp(&text, line, column, &PositionEncodingKind::UTF16);
    assert_eq!(bytes.character, 15, "three bytes each");
    assert_eq!(units.character, 11, "one code unit each");
}

