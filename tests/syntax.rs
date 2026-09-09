//! Parsing, reparsing, and the highlight kinds that come out.

use obelus::{
    coordinates::ByteOffset,
    syntax::{
        LanguageId,
        highlight::Highlights,
        parse::{self, SyntaxState},
    },
    text::Text,
    theme::SyntaxKind,
};

const SOURCE: &str = "\
fn main() {
\tlet greeting = \"你好\"; // 一句注释
\tprintln!(\"{greeting} world\");
}
";

fn kinds(text: &Text, state: &SyntaxState) -> Vec<Option<SyntaxKind>> {
    let mut highlights = Highlights::default();
    highlights.refresh(state, text, ByteOffset::new(0)..text.byte_length());
    (0..text.byte_length().get())
        .map(|byte| highlights.kind_at(ByteOffset::new(byte)))
        .collect()
}

fn parsed(source: &str) -> (Text, SyntaxState) {
    let text = Text::from_string(source);
    let state = SyntaxState::new(LanguageId::Rust, &text).expect("parsing");
    (text, state)
}

#[test]
fn the_obvious_things_are_highlighted() {
    let (text, state) = parsed(SOURCE);
    let kinds = kinds(&text, &state);
    let at = |needle: &str, offset: usize| {
        let byte = SOURCE.find(needle).expect("needle is in the source") + offset;
        kinds[byte]
    };

    assert_eq!(at("fn", 0), Some(SyntaxKind::Keyword));
    assert_eq!(at("main", 0), Some(SyntaxKind::Function));
    assert_eq!(at("let", 0), Some(SyntaxKind::Keyword));
    assert_eq!(at("\"你好\"", 0), Some(SyntaxKind::String));
    // Inside the wide glyph, not just at its first byte.
    assert_eq!(at("\"你好\"", 2), Some(SyntaxKind::String));
    assert_eq!(at("// ", 0), Some(SyntaxKind::Comment));
    assert_eq!(at("println", 0), Some(SyntaxKind::Function));
}

/// The rule for overlapping captures is that the innermost wins. A string
/// containing an escape is the case where getting it backwards is invisible in
/// most files and wrong in all of them.
#[test]
fn an_inner_capture_wins_over_the_string_around_it() {
    let source = "fn f() { let s = \"a\\nb\"; }\n";
    let (text, state) = parsed(source);
    let kinds = kinds(&text, &state);
    let quote = source.find('"').expect("a quote");

    assert_eq!(kinds[quote], Some(SyntaxKind::String));
    assert_eq!(kinds[quote + 1], Some(SyntaxKind::String));
    assert_eq!(kinds[quote + 2], Some(SyntaxKind::Escape), "\\n is escaped");
    assert_eq!(kinds[quote + 3], Some(SyntaxKind::Escape));
    assert_eq!(kinds[quote + 4], Some(SyntaxKind::String));
}

/// Incremental reparsing has one failure mode and it is silent: skip the
/// `Tree::edit` or give `Point.column` a character count instead of a byte
/// count, and the tree still parses, still has the right shape, and points at
/// the wrong bytes. Only comparing against a parse from scratch finds it.
#[test]
fn reparsing_incrementally_agrees_with_parsing_from_scratch() {
    let cases: &[(&str, &str)] = &[
        (
            "inserting at the start of a line",
            "// leading\nfn main() {}\n",
        ),
        (
            "deleting across two lines",
            "fn main() {\n\tprintln!(\"{greeting} world\");\n}\n",
        ),
        (
            "editing inside a string",
            "fn main() {\n\tlet greeting = \"你好世界\"; // 一句注释\n\tprintln!(\"{greeting} world\");\n}\n",
        ),
        (
            "editing inside a comment",
            "fn main() {\n\tlet greeting = \"你好\"; // 改过的注释\n\tprintln!(\"{greeting} world\");\n}\n",
        ),
        (
            "editing on a line that is not ASCII",
            "fn main() {\n\tlet greeting = \"你好\"; // 一句注释!\n\tprintln!(\"{greeting} world\");\n}\n",
        ),
        (
            "appending at the end of the file",
            "fn main() {\n\tlet greeting = \"你好\"; // 一句注释\n\tprintln!(\"{greeting} world\");\n}\nfn other() {}\n",
        ),
        ("replacing everything", "const N: u32 = 1;\n"),
    ];

    for (what, after) in cases {
        let (before_text, mut state) = parsed(SOURCE);
        let after_text = Text::from_string(after);

        let edit = parse::edit_between(&before_text, &after_text)
            .unwrap_or_else(|| panic!("{what}: the two versions differ, so there is an edit"));
        state.reparse(&after_text, &edit);

        let (fresh_text, fresh_state) = parsed(after);
        assert_eq!(
            kinds(&after_text, &state),
            kinds(&fresh_text, &fresh_state),
            "{what}: incremental and full parses disagree"
        );
    }
}

#[test]
fn an_unchanged_file_yields_no_edit() {
    let text = Text::from_string(SOURCE);
    assert!(parse::edit_between(&text, &Text::from_string(SOURCE)).is_none());
}

/// The trimmed region must not cut a multi-byte character in half:
/// tree-sitter is handed byte offsets and would be told to reparse from the
/// middle of a glyph.
///
/// Both cases need two *different* characters that share a byte, or the
/// byte-wise scan stops on a boundary by luck and the rounding is never
/// exercised at all. U+597D and U+5988 share their first byte, so the prefix
/// scan overshoots; U+00E9 and U+03A9 share their last, so the suffix scan
/// does.
/// Whether a byte offset is the start of a character.
///
/// `Rope::try_byte_to_char` will not answer this: it errors only when the
/// offset is out of bounds and otherwise rounds down into the character the
/// byte belongs to, so asking it is the same as asking nothing.
fn on_boundary(text: &Text, byte: usize) -> bool {
    let rope = text.rope();
    byte <= rope.len_bytes() && rope.char_to_byte(rope.byte_to_char(byte)) == byte
}

#[test]
fn the_trimmed_region_starts_and_ends_on_character_boundaries() {
    let cases: &[(&str, &str, &str)] = &[
        (
            "the prefix scan runs into a shared leading byte",
            "let s = \"\u{4f60}\u{597d}\";\n",
            "let s = \"\u{4f60}\u{5988}\";\n",
        ),
        (
            "the suffix scan runs into a shared trailing byte",
            "let s = \"\u{e9}\";\n",
            "let s = \"\u{3a9}\";\n",
        ),
    ];

    for (what, before, after) in cases {
        let before = Text::from_string(before);
        let after = Text::from_string(after);
        let edit = parse::edit_between(&before, &after).expect("an edit");

        assert!(
            on_boundary(&before, edit.start_byte),
            "{what}: start_byte {} splits a character",
            edit.start_byte
        );
        assert!(
            on_boundary(&before, edit.old_end_byte),
            "{what}: old_end_byte {} splits a character",
            edit.old_end_byte
        );
        assert!(
            on_boundary(&after, edit.new_end_byte),
            "{what}: new_end_byte {} splits a character",
            edit.new_end_byte
        );

        // And it still has to be an edit: rounding outwards must not collapse
        // the region to nothing.
        assert!(
            edit.start_byte < edit.old_end_byte || edit.start_byte < edit.new_end_byte,
            "{what}: rounding collapsed the edit"
        );
    }
}

#[test]
fn highlighting_only_the_visible_range_gives_the_same_answer() {
    let (text, state) = parsed(SOURCE);
    let whole = kinds(&text, &state);

    // The second line only, the way a scrolled viewport would ask for it.
    let start = text.line_start_byte(obelus::coordinates::LineNumber::new(1));
    let end = text.line_start_byte(obelus::coordinates::LineNumber::new(2));
    let mut window = Highlights::default();
    window.refresh(&state, &text, start..end);

    for (byte, expected) in whole.iter().enumerate().take(end.get()).skip(start.get()) {
        assert_eq!(
            window.kind_at(ByteOffset::new(byte)),
            *expected,
            "byte {byte} differs between a windowed and a whole-file query"
        );
    }
}

/// The companion to the test above, and the reason it needs one.
///
/// Highlighting reads byte ranges and nothing else, so an `InputEdit` whose
/// `Point`s are wrong produces identical highlights — the equivalence test
/// passes with `Point.column` counting characters instead of bytes. Nor do the
/// resulting trees differ: tree-sitter derives positions from the input for
/// everything it reparses, and the edit's points only shift nodes that are
/// reused on the edit's own row.
///
/// What is observable is the edit itself, so assert on that. The requirement
/// is real even though its effect is not visible here yet: `Point.column` is
/// specified as a byte count, and every later reader — position mapping to a
/// language server, point-range queries, the external scanners of grammars
/// that consult the column themselves — reads it as one.
#[test]
fn an_edits_points_are_byte_columns_not_character_columns() {
    let before = Text::from_string("let s = \"\u{4f60}\u{597d}\";\n");
    let after = Text::from_string("let s = \"\u{4f60}\u{597d}\u{4e16}\u{754c}\";\n");
    let edit = parse::edit_between(&before, &after).expect("an edit");

    // `let s = "` is nine bytes and nine characters; the two wide glyphs after
    // it are six bytes and two characters. So the edit starts at byte fifteen
    // and at character eleven, and only one of those is the right answer.
    assert_eq!(edit.start_byte, 15);
    assert_eq!(
        edit.start_position.column, 15,
        "start_position.column must count bytes"
    );
    assert_eq!(edit.old_end_position.column, 15);
    assert_eq!(
        edit.new_end_position.column, 21,
        "the replacement is two glyphs, so six bytes, longer"
    );
    assert_eq!(edit.start_position.row, 0);
}
