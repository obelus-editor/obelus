//! What the call the cursor is inside takes, while it is on screen.
//!
//! Not a list and not a panel keys go into: the reader is typing arguments,
//! and the panel is beside them saying what the arguments are. So it owns
//! one key -- the escape that closes it -- and everything it holds beyond
//! the server's answer is the one thing that says whether the answer is
//! still about where the reader is: which document, and which line.
//!
//! The line, because a call is written on one often enough and a reader who
//! has gone to another is writing something else. Kept here rather than
//! asked of the buffer every frame, because it is a fact about the
//! *question* -- the place the answer was asked about -- and the cursor has
//! moved on by the time anybody compares them.
//!
//! Somebody else's text is capped, as it is everywhere else in Obelus: five
//! signatures and five rows of documentation. A server may send twenty
//! overloads of `operator<<` and a doc comment the length of a page, and
//! neither may push the code the panel is about off the screen.

use obelus_buffer::DocumentId;
use obelus_lsp::signature::Answer;
use obelus_text::coordinates::{ByteOffset, CharColumn, LineNumber};

/// The most signatures to draw at once.
///
/// Five is enough to see that a name has several and which one is in use;
/// the rest is a count, which is what the transcript's rows do with a list
/// too long to draw.
pub const MOST_SIGNATURES: usize = 5;

/// The most rows of documentation to draw.
///
/// The same cap a card's own prose gets, for the same reason: it is
/// somebody else's writing, in a box over the code.
pub const MOST_ROWS: usize = 5;

/// A server's answer about a call, and where it is about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    /// What the server said.
    answer: Answer,
    /// The document it was asked about.
    buffer: DocumentId,
    /// The line the cursor was on when it was asked.
    ///
    /// What the panel is about is the call, not the line -- see `opened`.
    /// This is what a language Obelus cannot parse has instead, and what
    /// the answer is checked against when it arrives.
    line: LineNumber,
    /// Where the call's opening bracket is.
    ///
    /// Which call the panel is about, for the languages there is a tree
    /// for. A call written over four lines is one call, and the reader
    /// moving between its lines has not left it -- which a line cannot say
    /// and this can. The opening bracket rather than the pair: typing
    /// inside the call moves the closing one and leaves this where it is.
    opened: Option<ByteOffset>,
    /// And the column, which is which argument the answer is about.
    ///
    /// Kept because the caret moving along the line is the reader stepping
    /// between the arguments, and nothing types when they do: the mark
    /// would otherwise stay on the argument they have left.
    column: CharColumn,
}

impl Signature {
    /// A server's answer, ready to be drawn.
    #[must_use]
    pub const fn new(
        answer: Answer,
        buffer: DocumentId,
        line: LineNumber,
        column: CharColumn,
        opened: Option<ByteOffset>,
    ) -> Self {
        Self {
            answer,
            buffer,
            line,
            column,
            opened,
        }
    }

    /// Where it is about.
    #[must_use]
    pub const fn at(&self) -> (DocumentId, LineNumber, CharColumn) {
        (self.buffer, self.line, self.column)
    }

    /// Which call it is about, where that could be asked.
    #[must_use]
    pub const fn opened(&self) -> Option<ByteOffset> {
        self.opened
    }

    /// The signatures to draw, the active one first.
    #[must_use]
    pub fn shown(&self) -> &[obelus_lsp::signature::Signature] {
        let end = self.answer.signatures.len().min(MOST_SIGNATURES);
        &self.answer.signatures[..end]
    }

    /// How many there are that [`Signature::shown`] left out.
    #[must_use]
    pub fn more(&self) -> usize {
        self.answer.signatures.len().saturating_sub(MOST_SIGNATURES)
    }

    /// Whether the call says anything about itself.
    ///
    /// Asked where there is no width to wrap to yet: how wide the box is
    /// depends on whether there is prose in it, and the prose is wrapped to
    /// the box.
    #[must_use]
    pub const fn documented(&self) -> bool {
        self.answer.documentation.is_some()
    }

    /// What the call says about itself, wrapped to a width.
    ///
    /// Wrapped where it is drawn rather than kept: it is a doc comment
    /// rather than a transcript, which is the card's own bargain with the
    /// same kind of text.
    #[must_use]
    pub fn documentation(&self, width: u16) -> Vec<String> {
        let Some(documentation) = self.answer.documentation.as_deref() else {
            return Vec::new();
        };
        let mut rows = obelus_text::wrapped(documentation, width);
        rows.truncate(MOST_ROWS);
        rows
    }

    /// What the server said, whole.
    ///
    /// For the next question's context, which hands the answer back: the
    /// panel is what says one is showing, so the panel is what holds the
    /// answer to hand back.
    #[must_use]
    pub const fn answer(&self) -> &Answer {
        &self.answer
    }
}
