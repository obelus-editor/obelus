//! The runs of a document that are written in some other language.
//!
//! A markdown file holds programs, an HTML file holds a script and a
//! stylesheet, a Rust macro holds Rust. Each grammar says where its own
//! second languages are, in an injection query shipped beside its highlight
//! one, and this reads that query and answers with the ranges.
//!
//! Ranges rather than a byte range: what a grammar hands over is not always
//! contiguous. A fenced block inside a block quote has the quote's own
//! markers running down the middle of it, and the language in the fence has
//! never heard of them.

use obelus_text::Text;
use tree_sitter::{Node, Query, QueryCursor, Range, StreamingIterator as _, Tree};

use crate::{LanguageId, rope_chunks};

/// A run of a document written in another language.
#[derive(Debug)]
pub struct Injection {
    /// What it is written in.
    pub language: LanguageId,
    /// The parts of the document it is made of, in order.
    ///
    /// More than one where something belonging to the outer language runs
    /// through it. The parser is given these as its included ranges, which
    /// is also what keeps every offset the inner tree reports a place in
    /// the whole document rather than in a copy of a slice of it.
    pub ranges: Vec<Range>,
}

/// Every injection a tree holds.
///
/// A language with no injection query has none, which is most of them.
#[must_use]
pub fn found(language: LanguageId, tree: &Tree, text: &Text) -> Vec<Injection> {
    let Some(query) = query(language) else {
        return Vec::new();
    };

    let rope = text.rope();
    let mut provider = |node: Node<'_>| rope_chunks(rope, node);
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), &mut provider);

    let mut found = Vec::new();
    while let Some(matched) = matches.next() {
        let settings = query.property_settings(matched.pattern_index);
        let property = |key: &str| {
            settings
                .iter()
                .find(|property| &*property.key == key)
                .map(|property| property.value.as_deref().unwrap_or_default())
        };
        // Set by the pattern, or read out of the document where the pattern
        // captured a name instead: a fence says what is in it, and an HTML
        // `<script>` is known to be JavaScript by whoever wrote the query.
        let mut named = property("injection.language").map(str::to_string);
        // Every content capture of one match, joined. A pattern can capture
        // several -- the pieces of a template string either side of what is
        // interpolated into it -- and those are one program with a hole in
        // it rather than two programs.
        let mut content: Vec<Node<'_>> = Vec::new();
        for capture in matched.captures() {
            match query.capture_names()[capture.index as usize] {
                "injection.language" if named.is_none() => {
                    named = Some(
                        text.rope()
                            .byte_slice(capture.node.byte_range())
                            .to_string(),
                    );
                }
                "injection.content" => content.push(capture.node),
                _ => {}
            }
        }

        // A name Obelus has no grammar for: the run stays as the outer
        // language drew it, which is the same thing that happens to a file
        // in that language. A fence saying `ruby` is not a broken fence.
        let Some(language) = named
            .as_deref()
            .map(str::to_lowercase)
            .and_then(|name| LanguageId::for_injection(&name))
        else {
            continue;
        };
        // What the outer language keeps for itself is cut out, unless the
        // query says otherwise: the block quote's markers, the continuation
        // of a list item. `include-children` is how a grammar says the
        // opposite -- a Rust macro's body is every token inside it, and
        // cutting the named ones out would leave the punctuation.
        let whole = property("injection.include-children").is_some();
        let ranges: Vec<Range> = content
            .into_iter()
            .flat_map(|node| ranges_of(node, whole))
            .collect();
        if ranges.is_empty() {
            continue;
        }
        found.push(Injection { language, ranges });
    }
    found
}

/// The parts of a node that belong to the language inside it.
///
/// Everything the node covers, less whatever its own grammar named inside
/// it. What is left is the text: a grammar names the things it understands,
/// and by definition it does not understand what it is handing over.
fn ranges_of(node: Node<'_>, whole: bool) -> Vec<Range> {
    if whole {
        return vec![node.range()];
    }
    let mut ranges = Vec::new();
    let mut from = node.start_byte();
    let mut at = node.start_position();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.start_byte() > from {
            ranges.push(Range {
                start_byte: from,
                start_point: at,
                end_byte: child.start_byte(),
                end_point: child.start_position(),
            });
        }
        from = child.end_byte();
        at = child.end_position();
    }
    if node.end_byte() > from {
        ranges.push(Range {
            start_byte: from,
            start_point: at,
            end_byte: node.end_byte(),
            end_point: node.end_position(),
        });
    }
    ranges
}

/// The compiled injection query for a language, built once and shared.
///
/// The third query over the same grammar, beside the highlight one and the
/// tags one, and like the tags one only some grammars ship it.
fn query(language: LanguageId) -> Option<&'static Query> {
    use std::sync::OnceLock;

    macro_rules! compiled {
        ($cell:ident, $source:expr) => {{
            static $cell: OnceLock<Query> = OnceLock::new();
            Some($cell.get_or_init(|| {
                Query::new(crate::grammar(language).language(), $source)
                    .expect("an injection query shipped with Obelus should compile")
            }))
        }};
    }

    match language {
        LanguageId::Markdown => compiled!(MARKDOWN, tree_sitter_md::INJECTION_QUERY_BLOCK),
        LanguageId::MarkdownInline => {
            compiled!(MARKDOWN_INLINE, tree_sitter_md::INJECTION_QUERY_INLINE)
        }
        LanguageId::Html => compiled!(HTML, tree_sitter_html::INJECTIONS_QUERY),
        // Nothing yet for the languages whose own injections are into
        // grammars Obelus does not have -- JavaScript's are regex and
        // jsdoc, Rust's is Rust inside a macro, and that last one is a
        // query over every macro call in the file for a little more colour
        // inside it. Both are a decision rather than an oversight: what is
        // here is what a reader can see the difference from.
        LanguageId::Rust
        | LanguageId::Toml
        | LanguageId::Json
        | LanguageId::Python
        | LanguageId::JavaScript
        | LanguageId::TypeScript
        | LanguageId::Tsx
        | LanguageId::Go
        | LanguageId::C
        | LanguageId::Cpp
        | LanguageId::Bash
        | LanguageId::Css
        | LanguageId::Yaml
        // Two of the new ones do ship one and are still not taken up, which
        // is the same decision as the rest of this list rather than an
        // omission: Haskell's names a dozen grammars behind a quasiquoter's
        // word, of which Obelus has two, and PHP's is the text around the
        // tags -- and that text is a whole HTML document, which the PHP
        // grammar hands over as one run per gap between `?>` and `<?php`.
        // Both are worth having and neither is a line of this table.
        | LanguageId::Agda
        | LanguageId::CSharp
        | LanguageId::Haskell
        | LanguageId::Java
        | LanguageId::Julia
        | LanguageId::Ocaml
        | LanguageId::OcamlInterface
        | LanguageId::Php
        | LanguageId::Ruby
        | LanguageId::Scala => None,
    }
}

#[cfg(test)]
mod tests {
    use obelus_text::Text;

    use super::{Injection, found};
    use crate::{LanguageId, parse::SyntaxState};

    /// Every injection a document holds, for a test to look through.
    fn injections(text: &str) -> Vec<Injection> {
        let text = Text::from_string(text);
        let state = SyntaxState::new(LanguageId::Markdown, &text).expect("a parse");
        found(LanguageId::Markdown, state.tree(), &text)
    }

    /// What a range actually covers, which is the only thing worth
    /// asserting: the numbers are offsets into this string and would be
    /// rewritten every time the fixture above them changed.
    fn covered<'a>(document: &'a str, injection: &Injection) -> Vec<&'a str> {
        injection
            .ranges
            .iter()
            .map(|range| &document[range.start_byte..range.end_byte])
            .collect()
    }

    /// A fence says what is in it, and that is the language its contents are
    /// parsed as.
    #[test]
    fn a_fence_is_an_injection_of_what_its_info_string_says() {
        let document = "# Title\n\n```rust\nfn main() {}\n```\n";
        let found = injections(document);
        let fence = found
            .iter()
            .find(|injection| injection.language == LanguageId::Rust)
            .expect("the fence is not an injection of Rust");
        assert_eq!(covered(document, fence), vec!["fn main() {}\n"]);
    }

    /// A language Obelus has no grammar for is no injection at all. The
    /// fence still reads -- the block grammar has drawn it -- and a reader
    /// opening a Lua file gets exactly the same thing.
    ///
    /// The fence said `ruby` until Ruby was one of the languages, which is
    /// the hazard in a test whose subject is a language that is *missing*:
    /// it passes for the wrong reason the moment somebody adds that one, and
    /// it fails loudly, which is what happened here.
    #[test]
    fn a_fence_in_a_language_obelus_does_not_have_is_left_alone() {
        let found = injections("```lua\nprint(1)\n```\n");
        assert!(
            found.iter().all(|injection| injection.ranges.is_empty()
                || injection.language == LanguageId::MarkdownInline),
            "something was injected for a language Obelus has no grammar for"
        );
    }

    /// The one that cannot be done by slicing. A fence inside a block quote
    /// has the quote's own markers running down the middle of it, and they
    /// belong to markdown: handing the whole span to the inner parser would
    /// hand it a `>` at the start of every line.
    ///
    /// Broken deliberately by returning `vec![node.range()]` from
    /// `ranges_of` whatever the query said: the ranges became one span and
    /// this test failed on the marker.
    #[test]
    fn a_fence_inside_a_quote_is_several_ranges_and_none_is_the_quote() {
        let document = "> ```rust\n> fn main() {}\n> ```\n";
        let found = injections(document);
        let fence = found
            .iter()
            .find(|injection| injection.language == LanguageId::Rust)
            .expect("the fence is not an injection of Rust");
        let covered = covered(document, fence);
        assert!(
            covered.iter().all(|part| !part.contains('>')),
            "the quote's own marker was handed to the inner language: {covered:?}"
        );
        assert_eq!(covered.concat(), "fn main() {}\n");
    }

    /// What is inside a paragraph is the second markdown grammar, reached
    /// the same way every other second language is.
    #[test]
    fn a_paragraph_is_an_injection_of_the_inline_grammar() {
        let document = "Some *emphasis* here.\n";
        let found = injections(document);
        let inline = found
            .iter()
            .find(|injection| injection.language == LanguageId::MarkdownInline)
            .expect("a paragraph is not an injection of the inline grammar");
        assert_eq!(covered(document, inline), vec!["Some *emphasis* here."]);
    }
}
