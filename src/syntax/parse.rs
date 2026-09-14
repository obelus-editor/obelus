//! Parsing, and reparsing after an edit.

use ropey::Rope;
use tree_sitter::{InputEdit, Parser, Point, Tree};

use crate::{
    coordinates::{ByteOffset, Place},
    syntax::{LanguageId, grammar},
    text::{Edit, Text},
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
        // A keyword is a leaf made of letters, so the test below takes
        // `match` and `return` for names -- and then every question in the
        // menu is offered on them, goes to the server, and comes back with
        // nothing. Tree-sitter already tells them apart: what a grammar
        // gives a name to is a thing in the language, and a keyword is
        // spelled by the grammar rather than by whoever wrote the file.
        if !node.is_named() {
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
    pub fn reparse(&mut self, text: &Text, edit: &Edit) {
        self.tree.edit(&input_edit(edit));
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
pub fn edit_between(old: &Text, new: &Text) -> Option<Edit> {
    let prefix = old.common_prefix(new);
    let suffix = old.common_suffix(new, prefix);

    let old_end = ByteOffset::new(old.byte_length().get() - suffix);
    let new_end = ByteOffset::new(new.byte_length().get() - suffix);
    if prefix == old_end && prefix == new_end {
        return None;
    }

    Some(Edit {
        start: old.place(prefix),
        old_end: old.place(old_end),
        new_end: new.place(new_end),
    })
}

/// An edit as tree-sitter wants it.
///
/// The translation is nothing but renaming, which is the point: the places
/// were taken by whoever made the edit, at the moment each of them still
/// existed, and nothing here has to work any of them out again.
#[must_use]
fn input_edit(edit: &Edit) -> InputEdit {
    InputEdit {
        start_byte: edit.start.byte.get(),
        old_end_byte: edit.old_end.byte.get(),
        new_end_byte: edit.new_end.byte.get(),
        start_position: point(edit.start),
        old_end_position: point(edit.old_end),
        new_end_position: point(edit.new_end),
    }
}

/// A place as tree-sitter's `Point`, which is a row and a *byte* into it.
const fn point(place: Place) -> Point {
    Point {
        row: place.row,
        column: place.column,
    }
}

#[cfg(test)]
mod tests {
    use super::{Text, edit_between, input_edit};

    /// What tree-sitter is handed, rather than what the edit says.
    ///
    /// The two are one renaming apart, and a renaming is exactly the kind of
    /// thing that can be got wrong without anything else noticing: a tree
    /// built on a column that counted characters parses cleanly and points at
    /// the wrong bytes. Asserted here rather than on the `Edit` because the
    /// `Edit` is not what the parser reads.
    #[test]
    fn the_parser_is_handed_byte_columns() {
        // `let s = "` is nine bytes and nine characters; the two glyphs after
        // it are six bytes and two characters. Only one of those numbers is
        // the answer, and on ASCII they would be the same number.
        let before = Text::from_string("let s = \"\u{4f60}\u{597d}\";\n");
        let after = Text::from_string("let s = \"\u{4f60}\u{597d}\u{4e16}\u{754c}\";\n");
        let edit = input_edit(&edit_between(&before, &after).expect("an edit"));

        assert_eq!(edit.start_byte, 15);
        // The two ends, which are a pair it is easy to hand over the wrong
        // way round: nothing was taken out, so the old end is the start, and
        // the new end is six bytes past it.
        assert_eq!(edit.old_end_byte, 15, "the old end is not where it was");
        assert_eq!(edit.new_end_byte, 21, "the new end is not where it is");
        assert_eq!(
            edit.start_position.column, 15,
            "the parser was handed a character column"
        );
        assert_eq!(edit.start_position.row, 0);
        assert_eq!(edit.old_end_position.column, 15);
        assert_eq!(
            edit.new_end_position.column, 21,
            "the replacement is two glyphs longer, which is six bytes"
        );
    }

    /// A place on a later row, where a row and a column can be swapped
    /// without the numbers looking wrong.
    #[test]
    fn a_row_and_a_column_are_not_interchangeable() {
        let before = Text::from_string("one\ntwo\nthree\n");
        let after = Text::from_string("one\ntwo\nthr!ee\n");
        let edit = input_edit(&edit_between(&before, &after).expect("an edit"));

        assert_eq!(edit.start_position.row, 2, "the row is not the row");
        assert_eq!(
            edit.start_position.column, 3,
            "the column is not the column"
        );
    }
}
