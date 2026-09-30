//! Parsing, reparsing, and the highlight kinds that come out.

mod support;

use obelus_syntax::{LanguageId, highlight::Highlights, parse, parse::SyntaxState};
use obelus_text::{Text, coordinates::ByteOffset, kind::SyntaxKind};

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

/// A run written in another language is parsed when somebody looks at it,
/// and not before.
///
/// A file of a hundred fenced blocks shows two of them. Parsing the other
/// ninety-eight put eight milliseconds between a reader's keystroke and
/// their letters, on every keystroke, for colours nobody could see -- and it
/// pushed a five kilobyte README over the line where the highlighting stops
/// keeping up with typing at all.
///
/// Broken deliberately by parsing every run as the file is read: the first
/// assertion goes red, and with it the reason the other two are cheap.
#[test]
fn the_runs_nobody_is_looking_at_are_not_parsed() {
    let block = "```rust\nfn main() {}\n```\n\nsome prose\n\n";
    let source = block.repeat(20);
    let text = Text::from_string(&source);
    let state = SyntaxState::new(LanguageId::Markdown, &text).expect("parsing");
    assert!(
        state.injected().is_empty(),
        "runs were parsed before anybody asked for them"
    );

    // One screenful: the first block and no more.
    let mut highlights = Highlights::default();
    highlights.refresh(
        &state,
        &text,
        ByteOffset::new(0)..ByteOffset::new(block.len()),
    );
    let few = state.injected().len();
    assert!(few > 0, "nothing was parsed for the range being drawn");
    assert!(
        few < 20,
        "the whole file was parsed for one screen of it: {few}"
    );
    // And the colours of what was asked for are there all the same.
    let at = source.find("fn ").expect("the first block");
    assert_eq!(
        highlights.kind_at(ByteOffset::new(at)),
        Some(SyntaxKind::Keyword),
        "the run being drawn was not coloured"
    );

    // Reading further parses further, and what was parsed stays parsed.
    highlights.refresh(&state, &text, ByteOffset::new(0)..text.byte_length());
    let all = state.injected().len();
    assert!(
        all > few,
        "looking at the rest of the file parsed nothing more: {all} against {few}"
    );
}

/// And the text moving under them throws them away.
///
/// Where a run is is a fact about the document's structure, and an edit
/// changes that: a line typed above a fence moves it, three backticks make a
/// new one. What is kept is the finding, which is a query over a tree that
/// is already there; what goes is the reading.
#[test]
fn an_edit_forgets_the_runs_that_were_read() {
    let source = "```rust\nfn main() {}\n```\n";
    let text = Text::from_string(source);
    let mut state = SyntaxState::new(LanguageId::Markdown, &text).expect("parsing");
    let mut highlights = Highlights::default();
    highlights.refresh(&state, &text, ByteOffset::new(0)..text.byte_length());
    assert!(!state.injected().is_empty(), "nothing was read");

    let after = Text::from_string(&format!("x{source}"));
    let edit = parse::edit_between(&text, &after).expect("an edit");
    state.reparse(&after, &edit);
    assert!(
        state.injected().is_empty(),
        "a run parsed before the edit was kept after it"
    );
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

/// Whether a byte offset is the start of a character.
///
/// `Rope::try_byte_to_char` will not answer this: it errors only when the
/// offset is out of bounds and otherwise rounds down into the character the
/// byte belongs to, so asking it is the same as asking nothing.
fn on_boundary(text: &Text, byte: usize) -> bool {
    let rope = text.rope();
    byte <= rope.len_bytes() && rope.char_to_byte(rope.byte_to_char(byte)) == byte
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
            on_boundary(&before, edit.start.byte.get()),
            "{what}: start_byte {} splits a character",
            edit.start.byte.get()
        );
        assert!(
            on_boundary(&before, edit.old_end.byte.get()),
            "{what}: old_end_byte {} splits a character",
            edit.old_end.byte.get()
        );
        assert!(
            on_boundary(&after, edit.new_end.byte.get()),
            "{what}: new_end_byte {} splits a character",
            edit.new_end.byte.get()
        );

        // And it still has to be an edit: rounding outwards must not collapse
        // the region to nothing.
        assert!(
            edit.start.byte.get() < edit.old_end.byte.get()
                || edit.start.byte.get() < edit.new_end.byte.get(),
            "{what}: rounding collapsed the edit"
        );
    }
}

#[test]
fn highlighting_only_the_visible_range_gives_the_same_answer() {
    let (text, state) = parsed(SOURCE);
    let whole = kinds(&text, &state);

    // The second line only, the way a scrolled viewport would ask for it.
    let start = text.line_start_byte(obelus_text::coordinates::LineNumber::new(1));
    let end = text.line_start_byte(obelus_text::coordinates::LineNumber::new(2));
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
    assert_eq!(edit.start.byte.get(), 15);
    assert_eq!(edit.start.column, 15, "the start column must count bytes");
    assert_eq!(edit.old_end.column, 15);
    assert_eq!(
        edit.new_end.column, 21,
        "the replacement is two glyphs, so six bytes, longer"
    );
    assert_eq!(edit.start.row, 0);
}

/// Every language actually produces highlights.
///
/// A query that compiles but matches nothing is the failure this catches: the
/// file opens, the parse succeeds, and the screen shows plain text. That is
/// indistinguishable from a language Obelus has never heard of, so nothing
/// else would report it. It is a real risk here because several of these
/// queries are two upstream queries concatenated -- TypeScript's covers only
/// what TypeScript adds to JavaScript.
///
/// Broken deliberately three ways: by emptying
/// `obelus-syntax/queries/julia/highlights.scm`, which is the one query that
/// is a file of Obelus's own, and by handing PHP and OCaml the wrong one of
/// their two grammars each. The comment on the parse assertion below says
/// what the last of those does and does not catch.
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
        (
            LanguageId::Agda,
            "-- c\nmodule Hello where\n\nname : String\nname = \"hi\"\n",
        ),
        (
            LanguageId::CSharp,
            "// c\nclass Greeter { string name = \"hi\"; }\n",
        ),
        (
            LanguageId::Haskell,
            "-- c\nmain :: IO ()\nmain = putStrLn \"hi\"\n",
        ),
        (
            LanguageId::Java,
            "// c\nclass Greeter { String name = \"hi\"; }\n",
        ),
        (
            LanguageId::Julia,
            "# c\nfunction greet(name)\n    return \"hi $name\"\nend\n",
        ),
        (LanguageId::Ocaml, "(* c *)\nlet greet name = \"hi\"\n"),
        // An interface file declares and does not define, so it has no string
        // in it the way the others do -- except in an `external`, which names
        // the C symbol it binds to. Which is the sort of line a `.mli` is
        // actually full of.
        (
            LanguageId::OcamlInterface,
            "(* c *)\nval greet : string -> string\nexternal now : unit -> float = \"caml_now\"\n",
        ),
        // With HTML either side of the tags, which is what a `.php` file
        // actually looks like -- and is the only reason this sample can tell
        // the two PHP grammars apart: `LANGUAGE_PHP_ONLY` parses everything
        // between `<?php` and `?>` exactly as well, and errors on the text
        // around it.
        (
            LanguageId::Php,
            "<p>hi</p>\n<?php\n// c\n$name = \"hi\";\n?>\n<p>bye</p>\n",
        ),
        (
            LanguageId::Ruby,
            "# c\ndef greet(name)\n  \"hi #{name}\"\nend\n",
        ),
        (
            LanguageId::Scala,
            "// c\nobject Greeter { val name = \"hi\" }\n",
        ),
        // Markdown has no comments and no strings of its own: the *block*
        // grammar is what Obelus parses, and what it names is structure --
        // a heading, a fenced block, a list. So this one is checked by the
        // exception below rather than by the three assertions.
        (
            LanguageId::Markdown,
            "# Heading\n\nA paragraph.\n\n```rust\nfn main() {}\n```\n",
        ),
        // What is inside a paragraph, which is never a file: it is where the
        // block grammar points, and this is the sort of run it points at. No
        // comment, for the reason JSON has none.
        (
            LanguageId::MarkdownInline,
            "A *word*, a `span` and a [link](https://example.com).\n",
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
        // The right grammar, and not merely one that finds a comment and a
        // string in the wreckage. tree-sitter recovers from anything, so the
        // three assertions below all pass with the wrong grammar of a pair --
        // PHP has two and OCaml has two -- and an error node is the thing
        // that does not. Every sample here is valid, so a tree with one in it
        // is a tree built by the wrong parser.
        //
        // Which catches three of the four ways round: PHP either way, and
        // `.ml` given the interface grammar, because that one rejects `let`.
        // The fourth cannot be caught by any sample, and it is worth saying
        // so rather than looking covered -- `tree-sitter-ocaml`'s
        // implementation grammar accepts everything an interface file
        // contains, `val` and `external` included, so a `.mli` handed to it
        // parses identically. The two grammars really are the right pair for
        // the two extensions; only one direction of getting it wrong shows.
        assert!(
            !state.tree().root_node().has_error(),
            "{} did not parse its own sample, so this is the wrong grammar for it",
            language.name()
        );
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

        // JSON has no comments, and neither has the inside of a paragraph:
        // the one thing this list cannot ask of every language on it.
        if !matches!(language, LanguageId::Json | LanguageId::MarkdownInline) {
            assert!(
                found.contains(&SyntaxKind::Comment),
                "{} did not highlight its comment: {found:?}",
                language.name()
            );
        }
        // Agda is the one language here whose query names no string: upstream
        // captures integers, names, keywords, pragmas and comments, and a
        // `"hi"` in an Agda file is drawn plain. A fact about that query, not
        // about Obelus -- and the two assertions either side of this still
        // say the query works.
        if *language != LanguageId::Agda {
            assert!(
                found.contains(&SyntaxKind::String),
                "{} did not highlight its string: {found:?}",
                language.name()
            );
        }
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

/// The same again for a file with other languages inside it.
///
/// Every run in another language has a tree of its own, and where those runs
/// *are* is a fact about the outer tree: a line typed above a fence moves
/// it, three backticks make a new one, and a word typed into an info string
/// changes what the fence is written in. Asking again after the outer tree
/// has settled is the whole of keeping up, and not asking is silent -- the
/// colours simply stay where the fences used to be.
///
/// Broken deliberately by not looking for the injections again in
/// `SyntaxState::settle`: four of the five cases came back different from
/// the same document parsed fresh.
#[test]
fn reparsing_a_file_with_languages_inside_it_agrees_with_parsing_from_scratch() {
    const BEFORE: &str = "\
# Title

```rust
fn main() {}
```

A paragraph with *emphasis*.

```python
x = 1
```
";
    let cases: &[(&str, &str)] = &[
        (
            "editing inside a fence",
            "# Title\n\n```rust\nfn main() { let s = \"hi\"; }\n```\n\nA paragraph with *emphasis*.\n\n```python\nx = 1\n```\n",
        ),
        (
            "a line inserted above every fence, which moves them all",
            "# Title\n\nFirst.\n\n```rust\nfn main() {}\n```\n\nA paragraph with *emphasis*.\n\n```python\nx = 1\n```\n",
        ),
        (
            "a fence added between two others",
            "# Title\n\n```rust\nfn main() {}\n```\n\n```go\nfunc main() {}\n```\n\nA paragraph with *emphasis*.\n\n```python\nx = 1\n```\n",
        ),
        (
            "the language of a fence changed",
            "# Title\n\n```python\nfn main() {}\n```\n\nA paragraph with *emphasis*.\n\n```python\nx = 1\n```\n",
        ),
        (
            "a fence taken away",
            "# Title\n\nA paragraph with *emphasis*.\n\n```python\nx = 1\n```\n",
        ),
    ];

    for (what, after) in cases {
        let before_text = Text::from_string(BEFORE);
        let mut state = SyntaxState::new(LanguageId::Markdown, &before_text).expect("parsing");
        let after_text = Text::from_string(after);

        let edit = parse::edit_between(&before_text, &after_text)
            .unwrap_or_else(|| panic!("{what}: the two versions differ, so there is an edit"));
        state.reparse(&after_text, &edit);

        let fresh_text = Text::from_string(after);
        let fresh = SyntaxState::new(LanguageId::Markdown, &fresh_text).expect("parsing");
        assert_eq!(
            kinds(&after_text, &state),
            kinds(&fresh_text, &fresh),
            "{what}: reparsing gave different colours from parsing the same text from scratch"
        );
    }
}

/// A fenced block is drawn as whatever language its fence says it is.
///
/// The block grammar draws the fence as a literal and stops there -- the
/// code inside it is not markdown and markdown does not pretend to know it.
/// What reaches it is the injection: the block tree says which ranges are
/// Rust, a second tree is parsed from exactly those ranges, and its captures
/// land on the same bytes because the parser was given the ranges rather
/// than a copy of the text between them.
///
/// Broken deliberately by not painting the injected trees in
/// `Highlights::refresh`: `fn` came back a string, which is the colour of
/// the fence around it, instead of a keyword.
#[test]
fn a_fenced_block_is_highlighted_as_the_language_it_names() {
    let source = "# Heading\n\n```rust\nfn main() { let s = \"hi\"; }\n```\n";
    let text = Text::from_string(source);
    let state = SyntaxState::new(LanguageId::Markdown, &text).expect("parsing");
    let kinds = kinds(&text, &state);
    let at = |needle: &str| kinds[source.find(needle).expect("it is in the source")];

    assert_eq!(at("fn "), Some(SyntaxKind::Keyword), "not Rust's keyword");
    assert_eq!(at("main"), Some(SyntaxKind::Function));
    assert_eq!(at("\"hi\""), Some(SyntaxKind::String));
    // And the heading above it is still markdown's own.
    assert_eq!(at("Heading"), Some(SyntaxKind::Keyword));
}

/// Inside the fence, what the inner language says nothing about is plain.
///
/// The fence is a literal to markdown, so every byte of it is painted the
/// colour of a string before the Rust tree gets there -- and Rust captures
/// tokens, not the spaces between them. Markdown's query marks the inside of
/// the fence `@none` for exactly this, which has to *clear* what the fence
/// painted rather than merely decline to paint it.
///
/// Broken deliberately by skipping `Paint::Plain` the way an unknown capture
/// is skipped: the spaces between the tokens came back strings, and a fenced
/// block read as code on a green background of nothing.
#[test]
fn what_the_inner_language_does_not_name_is_not_the_fence_colour() {
    let source = "```rust\nfn main() {}\n```\n";
    let text = Text::from_string(source);
    let state = SyntaxState::new(LanguageId::Markdown, &text).expect("parsing");
    let kinds = kinds(&text, &state);

    let space = source.find("fn main").expect("the line") + 2;
    assert_eq!(&source[space..=space], " ", "not the space this is about");
    assert_eq!(
        kinds[space], None,
        "the space between two tokens kept the fence's own colour"
    );
}

/// What is inside a paragraph is the second markdown grammar, reached the
/// same way a fence's language is.
///
/// Markdown is two grammars -- the structure of a document, and what is
/// inside a paragraph -- and the block one hands the second its ranges. The
/// emphasis markers are the visible half of it: before this, a paragraph was
/// one flat run of plain text.
#[test]
fn what_is_inside_a_paragraph_is_highlighted_too() {
    let source = "A word with *emphasis* in it.\n";
    let text = Text::from_string(source);
    let state = SyntaxState::new(LanguageId::Markdown, &text).expect("parsing");
    let kinds = kinds(&text, &state);

    let star = source.find('*').expect("a marker");
    assert_eq!(
        kinds[star],
        Some(SyntaxKind::Punctuation),
        "the emphasis marker is not punctuation, so the inline grammar never ran"
    );
    // And the prose either side of it is prose.
    assert_eq!(kinds[0], None, "the words of a paragraph were painted");
}

/// The outline a syntax tree gives, for the languages whose grammars ship a
/// tags query. It is the floor under the outline command: no server, no
/// indexing, and available the moment the file is open.
#[test]
fn the_tree_gives_an_outline_of_what_a_file_defines() {
    use obelus_syntax::tags;

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
/// and it took Obelus down with an out-of-range slice.
#[test]
fn an_empty_range_colours_nothing() {
    let (text, state) = parsed(SOURCE);
    let mut highlights = Highlights::default();

    // At the beginning, which is the case that took Obelus down: a range
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

/// The questions in the symbol menu are all about a thing the reader wrote,
/// and a keyword is not one. It is a leaf made of letters, which is what
/// tells a name from a bracket -- so without this every question would be
/// offered on `match`, asked of the server, and answered with nothing.
#[test]
fn a_keyword_is_not_a_name() {
    let text = Text::from_string("fn main() {\n    match x {\n        _ => {}\n    }\n}\n");
    let state = SyntaxState::new(LanguageId::Rust, &text).expect("a parse");
    let name_at = |byte: usize| state.is_name_at(&text, ByteOffset::new(byte));

    assert!(name_at(3), "the name of a function is a name");
    assert!(name_at(22), "the name of a variable is a name");
    assert!(!name_at(16), "`match` was taken for a name");
    assert!(!name_at(10), "a bracket was taken for a name");
}

/// A grammar that cannot reparse between one keystroke and the next does not
/// get to make the reader wait for it.
///
/// Markdown's block grammar re-parses the whole section a heading opens, so
/// in a long document a keystroke costs milliseconds -- and what a reader
/// typing needs is the letters, not the colours. The tree is told where the
/// text moved, which is cheap and keeps every node pointing at the right
/// bytes, and what the text now *means* waits for a pause.
mod catching_up {
    use crossterm::event::KeyCode;
    use obelus_app::{app::App, event::Event};
    use obelus_buffer::Buffer;

    use super::support;

    fn editing(name: &str, file: &str, contents: &str) -> (support::Scratch, App) {
        let scratch = support::Scratch::new(name);
        let path = scratch.path().join(file);
        std::fs::write(&path, contents).expect("writing the file");
        let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
        app.working_directory_for_test(scratch.path().to_path_buf());
        support::lay_out(&mut app, 60, 12);
        (scratch, app)
    }

    const SOURCE: &str = "fn main() {\n    let greeting = \"hello\";\n}\n";

    /// A grammar that keeps up is asked on every keystroke, as it always
    /// was: nothing waits, and nothing is owed.
    #[test]
    fn a_grammar_that_keeps_up_is_not_held_back() {
        let (_scratch, mut app) = editing("catch-quick", "sample.rs", SOURCE);
        support::type_text(&mut app, "x");
        assert!(
            !app.current_buffer().expect("a buffer").syntax_is_behind(),
            "a grammar that answers in microseconds was made to wait"
        );
    }

    /// One that cannot keep up is left owing, and the pause that runs out
    /// after the reader stops is what comes back for it.
    #[test]
    fn a_slow_grammar_catches_up_when_its_pause_runs_out() {
        let (_scratch, mut app) = editing("catch-slow", "sample.rs", SOURCE);
        app.current_buffer_mut()
            .expect("a buffer")
            .hold_syntax_back_for_test();

        support::type_text(&mut app, "x");
        assert!(
            app.current_buffer().expect("a buffer").syntax_is_behind(),
            "a grammar too slow to keep up was asked anyway"
        );

        app.handle(Event::SyntaxSettled);
        assert!(
            !app.current_buffer().expect("a buffer").syntax_is_behind(),
            "the tree never caught up"
        );
    }

    /// And it starts a clock to do it with, which is not the animation's.
    ///
    /// Nothing would come back for the tree until the reader pressed
    /// something else -- which for the last keystroke of a paragraph is
    /// never. That used to be the ticker, kept awake for as long as a tree
    /// owed an answer, and `Ticker::start` answers `None` over a network:
    /// on ssh the colours simply stopped arriving. Both halves, because
    /// either one passes with the other broken.
    #[test]
    fn a_slow_grammar_starts_a_clock_of_its_own() {
        let (_scratch, mut app) = editing("catch-ticker", "sample.rs", SOURCE);
        let (sender, heard) = obelus_app::event::channel();
        app.events_for_test(sender);
        app.current_buffer_mut()
            .expect("a buffer")
            .hold_syntax_back_for_test();
        support::lay_out(&mut app, 60, 12);
        assert!(!app.is_waking(), "something was already waking the screen");

        support::type_text(&mut app, "x");
        support::lay_out(&mut app, 60, 12);
        assert!(
            !app.is_waking(),
            "the tree that was left behind is being carried by the animation"
        );

        assert!(
            support::waited_for(&heard, |event| matches!(event, Event::SyntaxSettled)),
            "nothing will come back for the tree that was left behind"
        );
    }

    /// And what it catches up to is the screen it would have drawn all
    /// along: waiting changes when the colours arrive, not what they are.
    #[test]
    fn what_it_catches_up_to_is_the_same_screen() {
        let (_scratch, mut kept) = editing("catch-same-kept", "sample.rs", SOURCE);
        let (_other, mut held) = editing("catch-same-held", "sample.rs", SOURCE);
        held.current_buffer_mut()
            .expect("a buffer")
            .hold_syntax_back_for_test();

        for app in [&mut kept, &mut held] {
            support::press(app, KeyCode::Down);
            support::press(app, KeyCode::End);
            support::type_text(app, " // and a comment");
        }
        assert_ne!(
            support::render(&mut kept, 60, 12),
            support::render(&mut held, 60, 12),
            "this test is about a difference that was not there"
        );

        held.handle(Event::SyntaxSettled);
        assert_eq!(
            support::text_block(&support::render(&mut kept, 60, 12)),
            support::text_block(&support::render(&mut held, 60, 12)),
            "the text is not even the same"
        );
        assert_eq!(
            support::style_block(&support::render(&mut kept, 60, 12)),
            support::style_block(&support::render(&mut held, 60, 12)),
            "the colours the tree caught up to are not the ones it would have had"
        );
    }
}
