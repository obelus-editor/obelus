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

/// Every language actually produces highlights.
///
/// A query that compiles but matches nothing is the failure this catches: the
/// file opens, the parse succeeds, and the screen shows plain text. That is
/// indistinguishable from a language obelus has never heard of, so nothing
/// else would report it. It is a real risk here because several of these
/// queries are two upstream queries concatenated -- TypeScript's covers only
/// what TypeScript adds to JavaScript.
#[test]
fn every_language_highlights_its_own_sample() {
    // One snippet per language, each holding a comment, a string and a
    // keyword or a tag, which are the three things every one of these has.
    let samples: &[(LanguageId, &str)] = &[
        (LanguageId::Rust, "// c\nfn main() { let s = \"hi\"; }\n"),
        (LanguageId::Toml, "# c\n[table]\nkey = \"hi\"\n"),
        (LanguageId::Json, "{\"key\": \"hi\", \"n\": 1}\n"),
        (
            LanguageId::Python,
            "# c\ndef greet(name):\n    return f\"hi {name}\"\n",
        ),
        (
            LanguageId::JavaScript,
            "// c\nfunction greet(name) { return `hi ${name}`; }\n",
        ),
        (
            LanguageId::TypeScript,
            "// c\nfunction greet(name: string): string { return \"hi\"; }\n",
        ),
        (
            LanguageId::Tsx,
            "// c\nconst App = (): JSX.Element => <div className=\"a\">hi</div>;\n",
        ),
        (
            LanguageId::Go,
            "// c\npackage main\nfunc main() { s := \"hi\" }\n",
        ),
        (
            LanguageId::C,
            "// c\n#include <stdio.h>\nint main(void) { return puts(\"hi\"); }\n",
        ),
        (
            LanguageId::Cpp,
            "// c\n#include <string>\nint main() { std::string s = \"hi\"; return 0; }\n",
        ),
        (LanguageId::Bash, "# c\nname=\"hi\"\necho \"${name}\"\n"),
        (
            LanguageId::Css,
            "/* c */\ndiv.a { color: red; content: \"hi\"; }\n",
        ),
        (LanguageId::Html, "<!-- c -->\n<div class=\"a\">hi</div>\n"),
        (LanguageId::Yaml, "# c\nkey: \"hi\"\nlist:\n  - 1\n"),
        // Markdown has no comments and no strings of its own: the *block*
        // grammar is what obelus parses, and what it names is structure --
        // a heading, a fenced block, a list. So this one is checked by the
        // exception below rather than by the three assertions.
        (
            LanguageId::Markdown,
            "# Heading\n\nA paragraph.\n\n```rust\nfn main() {}\n```\n",
        ),
    ];

    assert_eq!(
        samples.len(),
        LanguageId::ALL.len(),
        "a language has no sample here, so nothing checks that it highlights"
    );

    for (language, source) in samples {
        let text = Text::from_string(source);
        let state = SyntaxState::new(*language, &text).expect("parsing");
        let kinds = kinds(&text, &state);
        let found: std::collections::HashSet<SyntaxKind> = kinds.into_iter().flatten().collect();

        // Markdown, whose block grammar names structure rather than tokens.
        // A heading is highlighted; a comment and a string are not things it
        // has.
        if *language == LanguageId::Markdown {
            assert!(
                found.contains(&SyntaxKind::Keyword),
                "markdown did not highlight its heading: {found:?}"
            );
            assert!(
                found.contains(&SyntaxKind::String),
                "markdown did not highlight its fenced block: {found:?}"
            );
            continue;
        }

        // JSON has no comments, which is the one thing this list cannot ask
        // of every language on it.
        if *language != LanguageId::Json {
            assert!(
                found.contains(&SyntaxKind::Comment),
                "{} did not highlight its comment: {found:?}",
                language.name()
            );
        }
        assert!(
            found.contains(&SyntaxKind::String),
            "{} did not highlight its string: {found:?}",
            language.name()
        );
        // Something beyond the two kinds every query in the world finds. A
        // query matching only comments and strings has matched the parts that
        // look the same in every language and nothing about this one -- which
        // is exactly what a half-applied concatenated query looks like.
        assert!(
            found
                .iter()
                .any(|kind| !matches!(kind, SyntaxKind::Comment | SyntaxKind::String)),
            "{} highlighted nothing but comments and strings: {found:?}",
            language.name()
        );
    }
}

/// The outline a syntax tree gives, for the languages whose grammars ship a
/// tags query. It is the floor under the outline command: no server, no
/// indexing, and available the moment the file is open.
#[test]
fn the_tree_gives_an_outline_of_what_a_file_defines() {
    use obelus::syntax::tags;

    // A function *before* a type, deliberately: the query reports its
    // matches pattern by pattern, and the Rust tags query has the type
    // patterns first. Anything that skipped sorting would come out in the
    // query's order, which is not the order the file reads in.
    let source = "\
fn free() {}

struct Thing {
    field: u32,
}

impl Thing {
    fn method(&self) -> u32 {
        self.field
    }
}

const NAMED: u32 = 1;
";
    let text = Text::from_string(source);
    let state = SyntaxState::new(LanguageId::Rust, &text).expect("parsing");
    let found = tags::outline(&state, &text);

    let names: Vec<&str> = found.iter().map(|symbol| symbol.name.as_str()).collect();
    assert_eq!(
        names,
        ["free", "Thing", "method", "NAMED"],
        "not the definitions, in the order they appear"
    );

    // One row per symbol. Upstream's queries have several patterns matching
    // the same node -- a method is both a method and a function -- and each
    // match would otherwise be a row.
    assert_eq!(
        names.len(),
        found
            .iter()
            .map(|symbol| (symbol.line.get(), symbol.column.get()))
            .collect::<std::collections::HashSet<_>>()
            .len(),
        "a symbol is listed more than once: {found:?}"
    );

    // The kind is a colour, because the row is a name and the only honest
    // way to highlight a name is by what it names.
    assert_eq!(found[0].kind, SyntaxKind::Function);
    assert_eq!(found[1].kind, SyntaxKind::Type);
    assert_eq!(found[3].kind, SyntaxKind::Constant);

    // The columns bracket the name itself, so a preview can mark exactly it.
    let thing = &found[1];
    assert_eq!(thing.line.get(), 2);
    assert_eq!(
        thing.end_column.get() - thing.column.get(),
        "Thing".len(),
        "the span is not the name"
    );

    // A language with no tags query has no outline, which is not the same as
    // a file that defines nothing.
    assert!(tags::has_tags(LanguageId::Rust));
    assert!(!tags::has_tags(LanguageId::Yaml));
    let yaml = Text::from_string("key: 1\n");
    let state = SyntaxState::new(LanguageId::Yaml, &yaml).expect("parsing");
    assert!(tags::outline(&state, &yaml).is_empty());
}

/// A range with nothing in it colours nothing, and does not panic.
///
/// An empty byte range is not a restriction as far as tree-sitter is
/// concerned: the query answers with captures from the whole document, and
/// every one of them is outside the nothing there is to write them into. It
/// is reachable two ways -- a screen with no room for the text at all, and a
/// viewport sitting on the empty last line a file ending in a newline has --
/// and it took obelus down with an out-of-range slice.
#[test]
fn an_empty_range_colours_nothing() {
    let (text, state) = parsed(SOURCE);
    let mut highlights = Highlights::default();

    // At the beginning, which is the case that took obelus down: a range
    // of `0..0` is indistinguishable from never having set one, so the
    // query answers for the whole document.
    let none = ByteOffset::new(0);
    highlights.refresh(&state, &text, none..none);
    assert_eq!(
        highlights.kind_at(none),
        None,
        "an empty range coloured something"
    );

    // And in the middle, where there is plenty to capture either way.
    let middle = ByteOffset::new(text.byte_length().get() / 2);
    highlights.refresh(&state, &text, middle..middle);
    assert_eq!(highlights.kind_at(middle), None);

    // And at the end, which is where a file ending in a newline puts it.
    let end = text.byte_length();
    highlights.refresh(&state, &text, end..end);
    assert_eq!(highlights.kind_at(end), None);

    // Then a real range again, to say the reuse still works: the allocation
    // is kept between calls and a bad one must not have left it wrong.
    highlights.refresh(&state, &text, ByteOffset::new(0)..text.byte_length());
    assert!(
        (0..text.byte_length().get())
            .any(|byte| highlights.kind_at(ByteOffset::new(byte)).is_some()),
        "nothing was coloured after an empty range"
    );
}
