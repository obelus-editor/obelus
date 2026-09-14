//! Conversions between the coordinate spaces.
//!
//! Every case here contains a wide glyph or a tab, because that is where the
//! spaces stop agreeing. On pure ASCII all four are the same number and a
//! broken conversion still passes.

use obelus::{
    coordinates::{ByteOffset, CharColumn, CharOffset, DisplayColumn, LineNumber},
    text::Text,
};

/// Line 1 mixes indentation, wide glyphs and ASCII, so no two coordinate
/// spaces agree about any position past its fourth character.
const SAMPLE: &str = "fn main() {\n    你好 world\n}\n";

fn sample() -> Text {
    Text::from_string(SAMPLE)
}

#[test]
fn a_trailing_newline_makes_a_final_empty_line() {
    let text = sample();
    assert_eq!(text.line_count(), 4);
    assert_eq!(text.line(LineNumber::new(3)).len_chars(), 0);
    assert_eq!(text.last_line(), LineNumber::new(3));
}

#[test]
fn lines_come_back_without_their_endings() {
    let text = sample();
    assert_eq!(text.line(LineNumber::new(0)), "fn main() {");
    assert_eq!(text.line(LineNumber::new(2)), "}");

    let crlf = Text::from_string("one\r\ntwo\r\n");
    assert_eq!(crlf.line(LineNumber::new(0)), "one");
    assert_eq!(crlf.line(LineNumber::new(1)), "two");
}

#[test]
fn line_length_counts_characters_not_bytes_or_cells() {
    let text = sample();
    // Four spaces, two wide glyphs, a space, then "world".
    assert_eq!(text.line_length(LineNumber::new(1)), CharColumn::new(12));
    // The same line is fourteen cells wide, because each wide glyph takes two.
    assert_eq!(
        text.line_display_width(LineNumber::new(1)),
        DisplayColumn::new(14)
    );
}

#[test]
fn position_and_char_offset_are_inverses() {
    let text = sample();
    for line in 0..text.line_count() {
        let line = LineNumber::new(line);
        for column in 0..=text.line_length(line).get() {
            let column = CharColumn::new(column);
            let offset = text.char_offset(line, column);
            assert_eq!(
                text.position(offset),
                (line, column),
                "round trip failed at {line:?} {column:?}"
            );
        }
    }
}

#[test]
fn byte_and_char_offsets_are_inverses() {
    let text = sample();
    for character in 0..=SAMPLE.chars().count() {
        let offset = CharOffset::new(character);
        assert_eq!(text.char_of_byte(text.byte_of_char(offset)), offset);
    }
}

#[test]
fn a_wide_glyph_costs_three_bytes_and_one_character() {
    let text = sample();
    // Start of line 1, then four spaces: char 16, byte 16.
    let before = text.char_offset(LineNumber::new(1), CharColumn::new(4));
    assert_eq!(before, CharOffset::new(16));
    assert_eq!(text.byte_of_char(before), ByteOffset::new(16));

    // One character later is three bytes later.
    let after = text.char_offset(LineNumber::new(1), CharColumn::new(5));
    assert_eq!(after, CharOffset::new(17));
    assert_eq!(text.byte_of_char(after), ByteOffset::new(19));
}

#[test]
fn line_start_bytes_account_for_wide_glyphs() {
    let text = sample();
    assert_eq!(text.line_start_byte(LineNumber::new(0)), ByteOffset::new(0));
    // "fn main() {\n" is twelve ASCII bytes.
    assert_eq!(
        text.line_start_byte(LineNumber::new(1)),
        ByteOffset::new(12)
    );
    // Plus "    你好 world\n": 4 + 3 + 3 + 1 + 5 + 1 bytes.
    assert_eq!(
        text.line_start_byte(LineNumber::new(2)),
        ByteOffset::new(29)
    );
}

#[test]
fn display_columns_double_for_wide_glyphs() {
    let text = sample();
    let line = LineNumber::new(1);
    let at = |column| text.display_column(line, CharColumn::new(column));

    assert_eq!(at(4), DisplayColumn::new(4)); // before the first wide glyph
    assert_eq!(at(5), DisplayColumn::new(6)); // after it
    assert_eq!(at(6), DisplayColumn::new(8)); // after the second
    assert_eq!(at(7), DisplayColumn::new(9)); // after the space
    assert_eq!(at(12), DisplayColumn::new(14)); // end of line
}

#[test]
fn a_tab_advances_to_the_next_tab_stop() {
    let leading = Text::from_string("\tab");
    let line = LineNumber::new(0);
    assert_eq!(
        leading.display_column(line, CharColumn::new(1)),
        DisplayColumn::new(4)
    );

    // From column one, a tab is worth three cells, not four.
    let inner = Text::from_string("a\tb");
    assert_eq!(
        inner.display_column(line, CharColumn::new(2)),
        DisplayColumn::new(4)
    );
    assert_eq!(
        inner.display_column(line, CharColumn::new(3)),
        DisplayColumn::new(5)
    );
}

#[test]
fn the_second_cell_of_a_wide_glyph_resolves_to_that_glyph() {
    let text = sample();
    let line = LineNumber::new(1);
    let at = |cells| text.column_at_display(line, DisplayColumn::new(cells));

    assert_eq!(at(4), CharColumn::new(4)); // first cell of the glyph
    assert_eq!(at(5), CharColumn::new(4)); // second cell, same glyph
    assert_eq!(at(6), CharColumn::new(5)); // next glyph
    assert_eq!(at(7), CharColumn::new(5));
    assert_eq!(at(8), CharColumn::new(6));
}

#[test]
fn display_columns_and_character_columns_round_trip_through_every_cell() {
    let text = sample();
    for line in 0..text.line_count() {
        let line = LineNumber::new(line);
        for column in 0..text.line_length(line).get() {
            let column = CharColumn::new(column);
            let cells = text.display_column(line, column);
            assert_eq!(
                text.column_at_display(line, cells),
                column,
                "round trip failed at {line:?} {column:?}"
            );
        }
    }
}

#[test]
fn out_of_range_positions_clamp_rather_than_panic() {
    let text = sample();
    // A line number held across a reload that shortened the document.
    assert_eq!(text.line(LineNumber::new(99)), text.line(text.last_line()));
    assert_eq!(text.clamp_line(LineNumber::new(99)), text.last_line());

    // A column past the end of its line.
    let line = LineNumber::new(2);
    assert_eq!(
        text.clamp_column(line, CharColumn::new(99)),
        text.line_length(line)
    );
}

#[test]
fn byte_columns_count_bytes_and_character_columns_count_characters() {
    let text = sample();
    let line = LineNumber::new(1);

    // Four spaces then two wide glyphs: character eight of the document-wide
    // offset for column six is six characters in and twelve bytes in.
    let offset = text.char_offset(line, CharColumn::new(6));
    let byte = text.byte_of_char(offset);
    assert_eq!(text.line_of_byte(byte), line);
    assert_eq!(text.byte_column(byte), 10);
    assert_eq!(text.position(offset).1, CharColumn::new(6));
}

#[test]
fn byte_length_is_bytes_not_characters() {
    let text = sample();
    assert_eq!(text.byte_length().get(), SAMPLE.len());
    assert_ne!(text.byte_length().get(), SAMPLE.chars().count());
}

/// Wrapping must not split a two-cell glyph. With one cell left before the
/// margin the glyph moves to the next row and that cell stays blank — a row
/// one cell short is right, and half a glyph is not a character.
#[test]
fn wrapping_never_splits_a_wide_glyph() {
    use obelus::text::WrapRow;

    // Four cells of wide glyphs, then two ASCII: 你好世界ab.
    let text = Text::from_string("\u{4f60}\u{597d}\u{4e16}\u{754c}ab\n");
    let rows = text.wrap_rows(LineNumber::new(0), 5);

    assert_eq!(
        rows,
        vec![
            // Four cells of glyphs; the fifth stays blank because the next
            // glyph needs two and only one is left.
            WrapRow {
                first: CharColumn::new(0),
                end: CharColumn::new(2),
                indent: 0,
            },
            // Again four, and the `ab` after it is a word: the fifth cell
            // could hold the `a`, and taking it would split the word.
            WrapRow {
                first: CharColumn::new(2),
                end: CharColumn::new(4),
                indent: 0,
            },
            WrapRow {
                first: CharColumn::new(4),
                end: CharColumn::new(6),
                indent: 0,
            },
        ]
    );
}

/// The property behind the case above, over every width a terminal might have.
#[test]
fn no_wrapped_row_is_wider_than_the_room_it_has() {
    let text = Text::from_string(SAMPLE);
    for width in 1..40u16 {
        for line in 0..text.line_count() {
            let line = LineNumber::new(line);
            for row in text.wrap_rows(line, width) {
                let from = text.display_column(line, row.first).get();
                let to = text.display_column(line, row.end).get();
                let cells = to - from;
                // The one exception: a glyph wider than the whole row still
                // has to go somewhere.
                let single = row.end.get() == row.first.get() + 1;
                assert!(
                    cells <= width || single,
                    "at width {width}, {line:?} has a row of {cells} cells"
                );
            }
        }
    }
}

/// Every character on a line belongs to exactly one row, in order and with no
/// gaps. Without this a character can be drawn twice, or not at all.
#[test]
fn the_wrapped_rows_of_a_line_cover_it_exactly_once() {
    let text = Text::from_string(SAMPLE);
    for width in 1..40u16 {
        for line in 0..text.line_count() {
            let line = LineNumber::new(line);
            let mut expected = CharColumn::new(0);
            for row in text.wrap_rows(line, width) {
                assert_eq!(row.first, expected, "a gap or an overlap at width {width}");
                expected = row.end;
            }
            assert_eq!(
                expected,
                text.line_length(line),
                "the rows stop short of the end of {line:?} at width {width}"
            );
        }
    }
}

#[test]
fn a_position_and_its_visual_row_agree_at_every_width() {
    let text = Text::from_string(SAMPLE);
    for width in 1..40u16 {
        for line in 0..text.line_count() {
            let line = LineNumber::new(line);
            for column in 0..=text.line_length(line).get() {
                let column = CharColumn::new(column);
                let (row, cell) = text.visual_position(line, column, width);
                assert_eq!(
                    text.column_in_row(line, row, cell, width),
                    column,
                    "at width {width}, {line:?} {column:?} did not come back"
                );
            }
        }
    }
}

/// The point of word wrapping: a row ends where a break is allowed, not where
/// the margin happens to fall.
#[test]
fn a_row_ends_where_a_break_is_allowed() {
    let source = "const NAMES = [\"item-00\", \"item-01\", \"item-02\"];\n";
    let text = Text::from_string(source);
    let line = LineNumber::new(0);
    let contents: String = text.line(line).chars().collect();

    let rows = text.wrap_rows(line, 24);
    let pieces: Vec<String> = rows
        .iter()
        .map(|row| {
            contents
                .chars()
                .skip(row.first.get())
                .take(row.end.get() - row.first.get())
                .collect()
        })
        .collect();

    assert!(pieces.len() > 1, "the line is long enough to wrap");
    // In ASCII code with spaces in it, every row but the last ends on one:
    // the break goes after the space, so no token is cut in half. Rows of
    // Chinese end without one, which is why this is asserted on this input
    // rather than as a general property.
    for piece in pieces.iter().take(pieces.len() - 1) {
        assert!(piece.ends_with(' '), "a row was cut mid-token: {pieces:?}");
    }
}

/// A run with nowhere to break — no whitespace, no punctuation the algorithm
/// will separate — has to be broken at the margin. There is nothing better to
/// do with it, and letting it overflow would be worse.
#[test]
fn an_unbreakable_run_falls_back_to_the_margin() {
    let text = Text::from_string("foo(bar,baz,qux)\n");
    let line = LineNumber::new(0);
    let rows = text.wrap_rows(line, 6);

    assert!(rows.len() > 1, "sixteen characters do not fit in six cells");
    for row in rows.iter().take(rows.len() - 1) {
        assert_eq!(
            row.end.get() - row.first.get(),
            6,
            "a fallback row should fill the margin: {rows:?}"
        );
    }
}

/// Chinese has no spaces, so breaking only at whitespace would give a line of
/// it no break opportunity at all. The algorithm allows a break between two
/// Han characters — and still refuses to break an English word in the middle
/// of the same line, which is what makes mixed text the case that tells the
/// two apart. Pure Han text cannot: every character is a break opportunity
/// and two cells wide, so a margin break lands in the same places.
#[test]
fn mixed_chinese_and_english_breaks_between_han_but_not_inside_a_word() {
    let source =
        "// \u{8fd9}\u{4e2a} function \u{5904}\u{7406} wide glyph \u{7684}\u{60c5}\u{51b5}\n";
    let text = Text::from_string(source);
    let line = LineNumber::new(0);
    let contents: String = text.line(line).chars().collect();
    let characters: Vec<char> = contents.chars().collect();

    let rows = text.wrap_rows(line, 12);
    assert!(rows.len() >= 3, "the line is long enough to wrap: {rows:?}");

    let mut broke_between_han = false;
    for row in rows.iter().skip(1) {
        let before = characters[row.first.get() - 1];
        let after = characters[row.first.get()];

        // No row may start in the middle of an English word.
        let inside_a_word = before.is_ascii_alphanumeric() && after.is_ascii_alphanumeric();
        assert!(
            !inside_a_word,
            "a row starts inside a word, between {before:?} and {after:?}: {rows:?}"
        );

        if !before.is_ascii() && !after.is_ascii() {
            broke_between_han = true;
        }
    }
    assert!(
        broke_between_han,
        "no row began between two Han characters, so only whitespace was used: {rows:?}"
    );
}

/// A wrapped statement should still read as being inside its block.
#[test]
fn continuation_rows_are_indented_to_their_line() {
    let text = Text::from_string("    println!(\"{} {} {}\", alpha, beta, gamma);\n");
    let line = LineNumber::new(0);
    let rows = text.wrap_rows(line, 24);

    assert!(rows.len() > 1, "the line is long enough to wrap");
    assert_eq!(rows[0].indent, 0, "the first row has its own indentation");
    for row in rows.iter().skip(1) {
        assert_eq!(row.indent, 4, "a continuation row lost the indent");
    }
}

/// An indent taking more than half the room would leave almost nothing to
/// wrap into, and many short rows are harder to read than one unindented one.
#[test]
fn an_indent_wider_than_half_the_room_is_not_carried() {
    let text = Text::from_string("            deeply indented and long enough to wrap\n");
    let line = LineNumber::new(0);

    let roomy = text.wrap_rows(line, 40);
    assert_eq!(roomy[1].indent, 12, "twelve of forty is worth keeping");

    let cramped = text.wrap_rows(line, 20);
    assert_eq!(cramped[1].indent, 0, "twelve of twenty is not");
}

/// The fifth coordinate space: what the language server protocol counts in
/// when a server will not agree to bytes.
///
/// Everything about it is invisible until a character outside the basic
/// multilingual plane turns up. An emoji is one `char`, two UTF-16 code
/// units, four bytes and two display cells — the one sample where all five
/// spaces disagree.
#[test]
fn utf16_columns_differ_from_every_other_column_on_an_emoji() {
    use obelus::coordinates::Utf16Column;

    let text = Text::from_string("let party = \"\u{1f389}!\";\n");
    let line = LineNumber::new(0);
    let emoji = CharColumn::new(13);
    assert_eq!(text.line(line).char(emoji.get()), '\u{1f389}');

    // Before it, everything agrees.
    assert_eq!(text.utf16_column(line, emoji), Utf16Column::new(13));
    // After it, they do not.
    let after = CharColumn::new(14);
    assert_eq!(text.utf16_column(line, after), Utf16Column::new(15));
    assert_eq!(
        text.byte_column(text.byte_of_char(text.char_offset(line, after))),
        17
    );
    assert_eq!(text.display_column(line, after).get(), 15);
}

#[test]
fn utf16_columns_round_trip() {
    let samples = [
        "plain ascii\n",
        "\u{4f60}\u{597d} mixed \u{4e16}\u{754c}\n",
        "let party = \"\u{1f389}\u{1f680}\";\n",
        "\ttabbed \u{1f389}\n",
    ];
    for source in samples {
        let text = Text::from_string(source);
        for line in 0..text.line_count() {
            let line = LineNumber::new(line);
            for column in 0..=text.line_length(line).get() {
                let column = CharColumn::new(column);
                let units = text.utf16_column(line, column);
                assert_eq!(
                    text.column_at_utf16(line, units),
                    column,
                    "{source:?} {line:?} {column:?} came back wrong"
                );
            }
        }
    }
}

/// An offset landing on the second half of a surrogate pair resolves to the
/// character it belongs to, the way a display column landing on the second
/// cell of a wide glyph does.
#[test]
fn the_second_half_of_a_surrogate_pair_resolves_to_its_character() {
    use obelus::coordinates::Utf16Column;

    let text = Text::from_string("a\u{1f389}b\n");
    let line = LineNumber::new(0);

    assert_eq!(
        text.column_at_utf16(line, Utf16Column::new(0)),
        CharColumn::new(0)
    );
    assert_eq!(
        text.column_at_utf16(line, Utf16Column::new(1)),
        CharColumn::new(1)
    );
    assert_eq!(
        text.column_at_utf16(line, Utf16Column::new(2)),
        CharColumn::new(1),
        "the low surrogate belongs to the emoji"
    );
    assert_eq!(
        text.column_at_utf16(line, Utf16Column::new(3)),
        CharColumn::new(2)
    );
}

/// A protocol position is a line and a count into it, and which count depends
/// on what the server agreed to. Getting the units wrong points at a
/// different character on every line that is not plain ASCII.
#[test]
fn protocol_positions_round_trip_in_both_encodings() {
    use lsp_types::PositionEncodingKind;
    use obelus::lsp::position::{from_lsp, to_lsp};

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
    use obelus::lsp::position::to_lsp;

    let text = Text::from_string("let s = \"\u{4f60}\u{597d}\";\n");
    let line = LineNumber::new(0);
    // Just past the two Han characters.
    let column = CharColumn::new(11);

    let bytes = to_lsp(&text, line, column, &PositionEncodingKind::UTF8);
    let units = to_lsp(&text, line, column, &PositionEncodingKind::UTF16);
    assert_eq!(bytes.character, 15, "three bytes each");
    assert_eq!(units.character, 11, "one code unit each");
}

/// An edit says where it happened in every unit at once, and the three
/// places are taken while each of them still exists: the old end is gone
/// once the edit has happened and the new end was not there before it.
mod editing {
    use obelus::{
        coordinates::{CharColumn, CharOffset, LineNumber, Span},
        text::Text,
    };

    use super::{SAMPLE, sample};

    /// The whole point of the sample: a wide glyph means the byte a place
    /// reports is not the character anybody counted.
    #[test]
    fn inserting_past_a_wide_glyph_answers_in_bytes() {
        let mut text = sample();
        // After the second 好, which is six bytes into the line but two
        // characters into it.
        let at = text.char_offset(LineNumber::new(1), CharColumn::new(6));
        let edit = text.insert(at, "!");

        assert_eq!(text.rope().to_string(), "fn main() {\n    你好! world\n}\n");
        assert_eq!(edit.start.row, 1);
        // Four spaces and two three-byte glyphs.
        assert_eq!(edit.start.column, 10, "the column is not in bytes");
        assert_eq!(edit.old_end, edit.start, "an insert took something out");
        assert_eq!(edit.new_end.column, 11);
        assert_eq!(edit.lines(), 0);
    }

    /// Where the new end has to be measured in the document as it is: a
    /// newline puts it on a row that did not exist a moment ago, and the
    /// same byte offset in the old text is still on the old row.
    #[test]
    fn inserting_a_line_break_ends_on_the_line_it_made() {
        let mut text = sample();
        let at = text.char_offset(LineNumber::new(1), CharColumn::new(6));
        let edit = text.insert(at, "\n");

        assert_eq!(edit.start.row, 1);
        assert_eq!(edit.start.column, 10);
        assert_eq!(
            edit.new_end.row, 2,
            "the new end was measured in the document as it was"
        );
        assert_eq!(edit.new_end.column, 0);
        assert_eq!(edit.lines(), 1);
    }

    /// A span that ends on another line, which is the case every offset
    /// arithmetic gets wrong first.
    #[test]
    fn removing_across_a_line_break_says_how_many_lines_went() {
        let mut text = sample();
        let (gone, edit) = text.remove(Span {
            line: LineNumber::new(0),
            column: CharColumn::new(10),
            end_line: LineNumber::new(2),
            end_column: CharColumn::new(0),
        });

        assert_eq!(gone, "{\n    你好 world\n");
        assert_eq!(text.rope().to_string(), "fn main() }\n");
        assert_eq!(edit.start.row, 0);
        assert_eq!(edit.old_end.row, 2, "the old end is in the text as it was");
        assert_eq!(
            edit.new_end, edit.start,
            "what is left does not begin where the span did"
        );
        assert_eq!(edit.lines(), -2, "two lines went and the count did not");
    }

    /// The end of the document is where a rope's bounds are easiest to walk
    /// off, and where an editor spends a great deal of its time.
    #[test]
    fn an_edit_at_the_end_stays_inside_the_document() {
        let mut text = sample();
        let end = CharOffset::new(SAMPLE.chars().count());
        let edit = text.insert(end, "\n");
        assert_eq!(
            text.rope().to_string(),
            "fn main() {\n    你好 world\n}\n\n"
        );
        assert_eq!(edit.lines(), 1);

        // And past the end, which a caller should not do and must not be
        // able to make fail.
        let mut text = sample();
        let edit = text.insert(CharOffset::new(9_999), "x");
        assert!(text.rope().to_string().ends_with("}\nx"));
        assert_eq!(edit.lines(), 0);
    }

    /// Undoing is putting back exactly what came out, so what came out has
    /// to be exactly what was there.
    #[test]
    fn what_a_removal_hands_back_puts_the_document_together_again() {
        let span = Span {
            line: LineNumber::new(1),
            column: CharColumn::new(2),
            end_line: LineNumber::new(1),
            end_column: CharColumn::new(9),
        };
        let mut text = sample();
        let (gone, edit) = text.remove(span);
        let mut back = Text::from_string(&text.rope().to_string());
        back.insert(
            back.char_offset(LineNumber::new(edit.start.row), CharColumn::new(2)),
            &gone,
        );
        assert_eq!(back.rope().to_string(), SAMPLE);
    }
}
