//! The symbols a file defines, from its syntax tree.
//!
//! The floor under the outline. A language server knows more -- nesting,
//! containers, the difference between a method and a free function that
//! happens to look like one -- but it has to be installed, started and
//! finished indexing, and obelus can highlight fourteen languages while
//! knowing how to start a server for nine of them. The tree is already in
//! the buffer, so this answer costs a query over one file and is available
//! the moment the file is open.
//!
//! It comes from each grammar's own `TAGS_QUERY`, which is shipped for the
//! languages people write code in and absent for the ones they write data
//! in: seven of the fourteen have one. A language without one has no
//! outline, which the caller has to say out loud.

use obelus_text::{
    Text,
    coordinates::{ByteOffset, CharColumn, LineNumber},
    kind::SyntaxKind,
};
use tree_sitter::{QueryCursor, StreamingIterator as _};

use crate::{LanguageId, parse::SyntaxState};

/// Something a file defines, and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    /// What it is called.
    pub name: String,
    /// What sort of thing it is, as a colour rather than as a word: the
    /// outline is a list of names, and the only honest way to highlight a
    /// name is by what it names.
    pub kind: SyntaxKind,
    /// Which line the name is on.
    pub line: LineNumber,
    /// Where the name starts on it.
    pub column: CharColumn,
    /// Where the name ends, so a preview can mark exactly it.
    pub end_column: CharColumn,
    /// How many definitions enclose this one.
    ///
    /// A method inside an impl block is one deep. Worked out from the ranges
    /// of the definitions themselves rather than from the query, which has
    /// no notion of nesting: a tags query is a flat list of patterns.
    pub depth: usize,
}

/// Every symbol a file defines, in the order they appear.
///
/// Empty for a language whose grammar ships no tags query, which is not the
/// same as a file that defines nothing -- the caller has to be able to tell
/// those apart, which is what [`has_tags`] is for.
#[must_use]
pub fn outline(state: &SyntaxState, text: &Text) -> Vec<Symbol> {
    let Some(query) = tags_query(state.language()) else {
        return Vec::new();
    };

    let mut found: Vec<(std::ops::Range<ByteOffset>, Symbol)> = Vec::new();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(
        query,
        state.tree().root_node(),
        |node: tree_sitter::Node| {
            // The rope, one chunk at a time: the query needs the bytes and the
            // document is not a `String`.
            text.rope()
                .chunks_at_byte(node.start_byte().min(text.rope().len_bytes()))
                .0
        },
    );

    while let Some(matched) = matches.next() {
        // A pattern is a definition or a reference, and only one of the two
        // is an outline. The kind is on the *pattern's* capture, the position
        // on the `@name` inside it.
        let mut kind = None;
        let mut name = None;
        for capture in matched.captures() {
            let capture_name = &query.capture_names()[capture.index as usize];
            if let Some(rest) = capture_name.strip_prefix("definition.") {
                // The pattern's own node as well as its kind: what encloses
                // what is what makes an outline a tree, and only the whole
                // definition has the extent to say.
                kind = Some((
                    kind_of(rest),
                    ByteOffset::new(capture.node.start_byte())
                        ..ByteOffset::new(capture.node.end_byte()),
                ));
            } else if *capture_name == "name" {
                name = Some(capture.node);
            }
        }
        let (Some((kind, whole)), Some(node)) = (kind, name) else {
            continue;
        };

        let start = ByteOffset::new(node.start_byte());
        let end = ByteOffset::new(node.end_byte());
        let (line, column) = text.position(text.char_of_byte(start));
        let (end_line, end_column) = text.position(text.char_of_byte(end));
        found.push((
            whole,
            Symbol {
                name: text
                    .rope()
                    .byte_slice(start.get()..end.get())
                    .chunks()
                    .collect(),
                kind,
                line,
                column,
                // A name spanning lines is not a name obelus can mark, so the
                // mark stops at the end of the first one.
                end_column: if end_line == line {
                    end_column
                } else {
                    text.line_length(line)
                },
                depth: 0,
            },
        ));
    }

    // In the order they appear in the file, which is the order a reader
    // reads them in.
    //
    // `QueryCursor` already walks the tree in document order, so this sort
    // changes nothing today -- breaking it deliberately fails no test. It is
    // here for the line below: `dedup_by_key` only removes *adjacent*
    // duplicates, and the guarantee it needs is sortedness, not whatever
    // order the cursor happens to yield.
    found.sort_by_key(|(_, symbol)| (symbol.line.get(), symbol.column.get()));
    // One row per symbol. Upstream's queries have several patterns matching
    // the same node -- a Rust method is both `definition.method` inside an
    // impl and `definition.function` anywhere -- and the tags they were
    // written for want every match. An outline that listed each function
    // twice would look like the file had been pasted over itself.
    found.dedup_by_key(|(_, symbol)| (symbol.line.get(), symbol.column.get()));

    // Depth by containment, over a list that is already in document order:
    // whatever is still open when a definition starts is what encloses it.
    let mut open: Vec<ByteOffset> = Vec::new();
    let mut symbols = Vec::with_capacity(found.len());
    for (whole, mut symbol) in found {
        open.retain(|end| *end > whole.start);
        symbol.depth = open.len();
        open.push(whole.end);
        symbols.push(symbol);
    }
    symbols
}

/// Whether obelus can outline a language at all without a server.
#[must_use]
pub fn has_tags(language: LanguageId) -> bool {
    tags_query(language).is_some()
}

/// What a `definition.*` capture means, as a colour.
///
/// The capture names come from upstream's tags queries and are shared across
/// languages: `class` covers every named type, `interface` a trait, `module`
/// a namespace.
fn kind_of(definition: &str) -> SyntaxKind {
    match definition {
        "function" | "method" | "macro" => SyntaxKind::Function,
        "class" | "interface" | "struct" | "enum" | "type" | "union" | "trait" => SyntaxKind::Type,
        "constant" => SyntaxKind::Constant,
        "module" | "namespace" | "package" => SyntaxKind::Keyword,
        "field" | "property" | "member" => SyntaxKind::Property,
        // Something upstream added that obelus has not been taught. A name
        // with the ordinary foreground still reads; a panic would not.
        _ => SyntaxKind::Variable,
    }
}

/// The compiled tags query for a language, built once and shared.
///
/// Separate from [`crate::grammar`]'s highlight query: the two are
/// different queries over the same grammar, and only some grammars ship this
/// one.
fn tags_query(language: LanguageId) -> Option<&'static tree_sitter::Query> {
    use std::sync::OnceLock;

    macro_rules! compiled {
        ($cell:ident, $source:expr) => {{
            static $cell: OnceLock<tree_sitter::Query> = OnceLock::new();
            Some($cell.get_or_init(|| {
                tree_sitter::Query::new(crate::grammar(language).language(), $source)
                    .expect("a tags query shipped with obelus should compile")
            }))
        }};
    }

    /// What upstream's Rust tags query leaves out.
    ///
    /// It was written for code navigation tools that index callables and
    /// types, so it has no pattern for a constant. A reader looking for
    /// `COMPACT_ROWS` in an outline of this very file would not find it.
    /// Rust only, because Rust is the language obelus is written in and the
    /// one its own outline is read against every day -- a patch per language
    /// is a maintenance burden, and this is one line.
    const RUST_EXTRA: &str = "
        (const_item name: (identifier) @name) @definition.constant
        (static_item name: (identifier) @name) @definition.constant
    ";

    match language {
        LanguageId::Rust => compiled!(
            RUST,
            &format!("{}{RUST_EXTRA}", tree_sitter_rust::TAGS_QUERY)
        ),
        LanguageId::Python => compiled!(PYTHON, tree_sitter_python::TAGS_QUERY),
        LanguageId::Go => compiled!(GO, tree_sitter_go::TAGS_QUERY),
        LanguageId::C => compiled!(C, tree_sitter_c::TAGS_QUERY),
        LanguageId::Cpp => compiled!(CPP, tree_sitter_cpp::TAGS_QUERY),
        LanguageId::JavaScript => compiled!(JAVASCRIPT, tree_sitter_javascript::TAGS_QUERY),
        // TypeScript's own tags query, which unlike its highlights is
        // complete on its own: it names the nodes TypeScript adds and the
        // ones it shares with JavaScript.
        LanguageId::TypeScript => compiled!(TYPESCRIPT, tree_sitter_typescript::TAGS_QUERY),
        LanguageId::Tsx => compiled!(TSX, tree_sitter_typescript::TAGS_QUERY),
        // Data languages, whose grammars ship no tags query. A file of them
        // has structure but no *definitions*, and an outline of every key in
        // a YAML file is the file again.
        LanguageId::Toml
        | LanguageId::Json
        | LanguageId::Bash
        | LanguageId::Css
        | LanguageId::Html
        | LanguageId::Yaml
        // A markdown outline is its headings, and it would be a good one.
        // The block grammar has the nodes for it but ships no tags query, so
        // it would have to be written here -- which is a decision, not an
        // oversight.
        | LanguageId::Markdown => None,
    }
}
