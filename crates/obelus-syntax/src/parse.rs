//! Parsing, and reparsing after an edit.

use obelus_text::{
    Edit, Text,
    coordinates::{ByteOffset, Place},
};
use ropey::Rope;
use tree_sitter::{InputEdit, Parser, Point, Tree};

use crate::{LanguageId, grammar, inject};

/// How deep a language inside a language may go.
///
/// Markdown holds a fence of HTML, which holds a script, which holds a
/// template string of HTML again -- and a grammar that injects itself, as
/// Rust's macros do, would otherwise go down for ever. Three is past
/// anything a reader meets and is a number rather than a proof.
const DEEPEST: usize = 3;

/// A parsed document, kept alongside its text.
pub struct SyntaxState {
    language: LanguageId,
    parser: Parser,
    tree: Tree,
    /// A second parser, for the runs written in some other language.
    ///
    /// Its own rather than the one above reconfigured, because the two are
    /// asked in turn on every reparse: this one is set to a different
    /// language and a different set of ranges for each injection, and the
    /// one above would have to be set back to the file's own language after
    /// every one of them.
    injector: Parser,
    /// The trees for the runs of this document written in another language,
    /// outermost first.
    injected: Vec<Injected>,
    /// Whether the tree has been told where the text moved but not what it
    /// means there.
    behind: bool,
    /// How long the last parse took, which is what decides whether the next
    /// one waits for a pause in the typing.
    took: std::time::Duration,
}

/// One run of a document in another language, parsed.
pub struct Injected {
    language: LanguageId,
    tree: Tree,
}

impl Injected {
    /// What this run is written in.
    #[must_use]
    pub const fn language(&self) -> LanguageId {
        self.language
    }

    /// Its tree.
    ///
    /// Its nodes are at their places in the whole document, not in a copy of
    /// the run: the parser was given the run as its included ranges rather
    /// than as a string of its own.
    #[must_use]
    pub const fn tree(&self) -> &Tree {
        &self.tree
    }
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
        let started = std::time::Instant::now();
        let tree = parse(&mut parser, text.rope(), None)?;
        let mut state = Self {
            language,
            parser,
            injector: Parser::new(),
            tree,
            injected: Vec::new(),
            behind: false,
            took: std::time::Duration::ZERO,
        };
        state.inject(text);
        // The first parse is the only measurement there is to go on, and it
        // is the honest one: a grammar that takes ten milliseconds over a
        // file does not take one over the same file a character later. The
        // languages inside this one are inside the measurement, because they
        // are inside the wait: a reader typing does not care which of the
        // parsers between their key and the colours was the slow one.
        state.took = started.elapsed();
        Some(state)
    }

    /// The runs of this document written in another language.
    #[must_use]
    pub fn injected(&self) -> &[Injected] {
        &self.injected
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
        self.note(edit);
        self.settle(text);
    }

    /// Tells the tree where the text moved, without parsing it again.
    ///
    /// The cheap half, and the half that cannot wait: every node after the
    /// edit is at a different offset now, and a tree that has not been told
    /// points at the wrong bytes. What it does not do is work out what the
    /// new bytes *mean*, which is the dear half.
    pub fn note(&mut self, edit: &Edit) {
        let edit = input_edit(edit);
        self.tree.edit(&edit);
        // And every tree inside it. They are thrown away and parsed again
        // when this settles, so this is not about reusing them -- it is
        // about the frames in between, which are drawn from these trees
        // exactly as they are drawn from the one above, and would otherwise
        // paint a fence's colours where the fence no longer is.
        for injected in &mut self.injected {
            injected.tree.edit(&edit);
        }
        self.behind = true;
    }

    /// Works out what the text means now, if that is still owed.
    ///
    /// Times itself, because whether this can be done between one keystroke
    /// and the next is not a property of obelus -- it is a property of the
    /// grammar, and they differ by two orders of magnitude.
    pub fn settle(&mut self, text: &Text) {
        if !self.behind {
            return;
        }
        let started = std::time::Instant::now();
        if let Some(tree) = parse(&mut self.parser, text.rope(), Some(&self.tree)) {
            self.tree = tree;
        }
        self.inject(text);
        self.took = started.elapsed();
        self.behind = false;
    }

    /// Whether the tree is older than the text.
    #[must_use]
    pub const fn is_behind(&self) -> bool {
        self.behind
    }

    /// Says this grammar is too slow to keep up, whatever it really costs.
    ///
    /// A test cannot make a machine slow, and what is worth testing is not
    /// the threshold but what happens on either side of it.
    pub const fn hold_back_for_test(&mut self) {
        self.took = std::time::Duration::from_secs(3600);
    }

    /// Whether this grammar answers fast enough to be asked on every
    /// keystroke.
    ///
    /// From the last answer rather than from a list of grammars: a file's
    /// size is half of the question, and no table of names knows it.
    #[must_use]
    pub const fn is_quick(&self) -> bool {
        self.took.as_micros() < QUICK.as_micros()
    }

    /// Parses the runs written in another language.
    ///
    /// From the tree that has just been parsed, every time, and from
    /// scratch. Where the injections are is a fact about the document's
    /// structure and an edit changes that structure -- a line typed above a
    /// fence moves it, three backticks make a new one -- so the question has
    /// to be asked again either way.
    ///
    /// Handing each parse the tree that run had a keystroke ago was tried
    /// and is not here: on a fifty kilobyte markdown file it saved twelve
    /// per cent of a cost that is nearly all in the parsing itself, and it
    /// cost an offset that had to be moved by every edit and a tree that
    /// could be matched to the wrong run. Two silent failures for a tenth of
    /// the work is the wrong trade; the honest saving, if this ever needs
    /// one, is not parsing the runs nobody is looking at.
    fn inject(&mut self, text: &Text) {
        self.injected.clear();
        let mut found = inject::found(self.language, &self.tree, text);
        for _ in 0..DEEPEST {
            if found.is_empty() {
                break;
            }
            let mut deeper = Vec::new();
            for injection in found {
                if self
                    .injector
                    .set_language(grammar(injection.language).language())
                    .is_err()
                {
                    continue;
                }
                // Ranges rather than a slice of the text: this is what keeps
                // every offset the inner tree reports a place in the whole
                // document, so nothing downstream has to add a base back on.
                // It refuses ranges out of order, which a query cannot
                // produce and which would be worth hearing about if it did.
                if let Err(error) = self.injector.set_included_ranges(&injection.ranges) {
                    tracing::debug!(%error, "an injection whose ranges the parser refused");
                    continue;
                }
                let Some(tree) = parse(&mut self.injector, text.rope(), None) else {
                    continue;
                };
                deeper.extend(inject::found(injection.language, &tree, text));
                self.injected.push(Injected {
                    language: injection.language,
                    tree,
                });
            }
            found = deeper;
        }
    }
}

/// How long a reparse may take and still be worth doing between one
/// keystroke and the next.
///
/// Two milliseconds: a comfortable share of a frame, and far above what
/// every grammar here but one costs. Markdown's block grammar re-parses the
/// whole section a heading opens, which in a document with a long one is
/// six milliseconds a keystroke -- and a reader typing does not need the
/// colours to have caught up, they need the letters to appear.
///
/// Markdown is further past it now that what is inside a paragraph is
/// parsed as well: measured on this project's own `AGENTS.md`, fifty
/// kilobytes of prose, six milliseconds became twenty-three, and a five
/// kilobyte README crossed the line from one side to the other. That is
/// what this threshold is for -- the reader carries on typing against the
/// tree they had -- and it is the reason the injections are not parsed
/// eagerly for ever: the saving left on the table is not parsing the runs
/// nobody is looking at.
const QUICK: std::time::Duration = std::time::Duration::from_millis(2);

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
