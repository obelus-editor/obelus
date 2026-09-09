//! Parsing, and reparsing after an edit.

use ropey::Rope;
use tree_sitter::{InputEdit, Parser, Point, Tree};

use crate::{
    coordinates::ByteOffset,
    syntax::{LanguageId, grammar},
    text::Text,
};

/// A parsed document, kept alongside its text.
pub struct SyntaxState {
    language: LanguageId,
    parser: Parser,
    tree: Tree,
}

impl std::fmt::Debug for SyntaxState {
    /// `Parser` has no `Debug`, and the tree's is a whole s-expression.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SyntaxState")
            .field("language", &self.language)
            .finish_non_exhaustive()
    }
}

impl SyntaxState {
    /// Parses a document from scratch.
    ///
    /// Returns `None` if the grammar declines the input, which tree-sitter
    /// only does when a parse is cancelled or the language is unset — neither
    /// of which can happen here, but the API allows it.
    #[must_use]
    pub fn new(language: LanguageId, text: &Text) -> Option<Self> {
        let mut parser = Parser::new();
        parser
            .set_language(grammar(language).language())
            .expect("a grammar shipped with obelus should load");
        let tree = parse(&mut parser, text.rope(), None)?;
        Some(Self {
            language,
            parser,
            tree,
        })
    }

    /// The language this was parsed as.
    #[must_use]
    pub const fn language(&self) -> LanguageId {
        self.language
    }

    /// The syntax tree.
    #[must_use]
    pub const fn tree(&self) -> &Tree {
        &self.tree
    }

    /// Whether a byte is inside a name.
    ///
    /// The smallest node covering it, and then two questions about that
    /// node: is it a leaf -- whitespace and the gaps between tokens belong
    /// to containers, not to leaves -- and does its text start the way a
    /// name does. Language-agnostic on purpose: it holds for every grammar
    /// obelus has, and a table of each language's identifier node kinds
    /// would be fourteen rows to keep right.
    ///
    /// Deliberately generous. A keyword and the inside of a string both pass,
    /// and a server then answers nothing about them, which is the mild
    /// failure. Refusing to ask about something that would have answered is
    /// the bad one.
    #[must_use]
    pub fn is_name_at(&self, text: &Text, byte: ByteOffset) -> bool {
        let Some(node) = self
            .tree
            .root_node()
            .descendant_for_byte_range(byte.get(), byte.get())
        else {
            return false;
        };
        if node.child_count() > 0 {
            return false;
        }
        text.rope()
            .byte_slice(node.start_byte()..node.end_byte())
            .chars()
            .next()
            .is_some_and(|first| first.is_alphabetic() || first == '_')
    }

    /// Reparses after an edit, reusing the parts of the tree the edit did not
    /// reach.
    ///
    /// The order matters and getting it wrong is silent: without the
    /// `Tree::edit` first, tree-sitter reuses nodes whose positions the edit
    /// has already moved, and the result is a tree that parses cleanly and
    /// points at the wrong bytes.
    pub fn reparse(&mut self, text: &Text, edit: &InputEdit) {
        self.tree.edit(edit);
        if let Some(tree) = parse(&mut self.parser, text.rope(), Some(&self.tree)) {
            self.tree = tree;
        }
    }
}

/// Runs the parser over a rope without flattening it into a `String`.
///
/// tree-sitter asks for bytes from an offset and takes whatever contiguous run
/// it is given, which is exactly what a rope chunk is. Copying the whole
/// document into a `String` first would be tolerable once and is not
/// tolerable on every reparse.
fn parse(parser: &mut Parser, rope: &Rope, old: Option<&Tree>) -> Option<Tree> {
    let mut callback = |byte: usize, _position: tree_sitter::Point| -> &[u8] {
        if byte >= rope.len_bytes() {
            return &[];
        }
        let (chunk, chunk_start, _, _) = rope.chunk_at_byte(byte);
        &chunk.as_bytes()[byte - chunk_start..]
    };
    parser.parse_with_options(&mut callback, old, None)
}

/// The smallest edit that turns `old` into `new`.
///
/// Found by trimming the shared head and tail rather than by diffing: the two
/// versions of a file an agent just rewrote share nearly all of their bytes,
/// and the point of this is only to give tree-sitter a region small enough
/// that the rest of the tree can be reused.
///
/// Returns `None` when the two are identical, since there is nothing to
/// reparse.
#[must_use]
pub fn edit_between(old: &Text, new: &Text) -> Option<InputEdit> {
    let prefix = old.common_prefix(new);
    let suffix = old.common_suffix(new, prefix);

    let old_end = ByteOffset::new(old.byte_length().get() - suffix);
    let new_end = ByteOffset::new(new.byte_length().get() - suffix);
    if prefix == old_end && prefix == new_end {
        return None;
    }

    Some(InputEdit {
        start_byte: prefix.get(),
        old_end_byte: old_end.get(),
        new_end_byte: new_end.get(),
        start_position: point(old, prefix),
        old_end_position: point(old, old_end),
        new_end_position: point(new, new_end),
    })
}

/// A position as tree-sitter counts it: a line, and bytes into that line.
fn point(text: &Text, byte: ByteOffset) -> Point {
    Point {
        row: text.line_of_byte(byte).get(),
        column: text.byte_column(byte),
    }
}
