//! Which kind of thing each byte on screen is.
//!
//! Computed for the visible range only, from the tree, every frame. Two
//! consequences, both deliberate: a reparse invalidates nothing, because there
//! is no cache to invalidate; and a whole-file query never runs, so the cost
//! does not grow with the file.

use std::ops::Range;

use ropey::Rope;
use tree_sitter::{Node, QueryCursor, StreamingIterator as _};

use crate::{coordinates::ByteOffset, syntax::parse::SyntaxState, text::Text, theme::SyntaxKind};

/// The kind of every byte in one range of the document.
///
/// One entry per byte rather than a list of spans: the ranges overlap, the
/// rule for resolving that is "the innermost wins", and writing outer spans
/// first and letting inner ones overwrite is the whole implementation. The
/// range is a screenful, so this is a few kilobytes that get reused.
#[derive(Debug, Default)]
pub struct Highlights {
    start: usize,
    kinds: Vec<Option<SyntaxKind>>,
}

impl Highlights {
    /// Recomputes for a byte range, reusing the allocation.
    pub fn refresh(&mut self, state: &SyntaxState, text: &Text, range: Range<ByteOffset>) {
        let start = range.start.get();
        let end = range.end.get().max(start);

        self.start = start;
        self.kinds.clear();
        self.kinds.resize(end - start, None);

        let grammar = crate::syntax::grammar(state.language());
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(start..end);

        let rope = text.rope();
        let mut provider = |node: Node<'_>| rope_chunks(rope, node);

        // Outer spans first, so an inner one overwrites them. Sorting the
        // whole set would need the whole set; the query already yields
        // captures in document order, and within one position tree-sitter
        // gives the outermost node first, which is the same order.
        let mut captures =
            cursor.captures(grammar.query(), state.tree().root_node(), &mut provider);
        while let Some((matched, index)) = captures.next() {
            let capture = matched.captures()[*index];
            let Some(kind) = grammar.kind(capture.index) else {
                continue;
            };
            let node = capture.node.byte_range();
            let from = node.start.max(start);
            let to = node.end.min(end);
            for slot in &mut self.kinds[from - start..to.saturating_sub(start)] {
                *slot = Some(kind);
            }
        }
    }

    /// Forgets everything, for when there is nothing to highlight.
    pub fn clear(&mut self) {
        self.start = 0;
        self.kinds.clear();
    }

    /// What kind of thing a byte is, if it is one the theme colours.
    #[must_use]
    pub fn kind_at(&self, byte: ByteOffset) -> Option<SyntaxKind> {
        self.kinds
            .get(byte.get().checked_sub(self.start)?)
            .copied()
            .flatten()
    }
}

/// The bytes of a node, as the rope already stores them.
///
/// `use<'a>` because in edition 2024 an opaque return type captures every
/// input lifetime by default, and capturing the node's would tie the iterator
/// to a borrow that ends when the closure returns.
fn rope_chunks<'a>(rope: &'a Rope, node: Node<'_>) -> impl Iterator<Item = &'a [u8]> + use<'a> {
    let range = node.byte_range();
    let end = range.end.min(rope.len_bytes());
    let start = range.start.min(end);
    rope.byte_slice(start..end).chunks().map(str::as_bytes)
}
