//! The bracket under the cursor, and the one that closes it -- or the pair
//! the cursor is inside, when it is on neither half.
//!
//! A scan rather than a tree walk. The tree knows the answer, but reaching
//! it means knowing which node kind each grammar uses for a delimited group,
//! which is fourteen answers; counting depth over the bytes is one, and it
//! is the same answer.
//!
//! Brackets inside strings and comments are skipped, which is the whole
//! difficulty and the reason this takes the highlights: `"("` is not an
//! unclosed bracket, and a scan that thought so would pair the wrong two
//! characters on every line of code that mentions one.

use std::ops::Range;

use obelus_text::{Text, coordinates::ByteOffset, kind::SyntaxKind};

use crate::highlight::Highlights;

/// The pairs Obelus matches.
///
/// Public because folding needs the closing half of them: a line that
/// *begins* with one closes something, whatever was opened and wherever.
/// Two lists would be two lists to keep right, and the day one grew a pair
/// the other did not would be the day folding stopped closing a block the
/// key could still jump across.
pub const PAIRS: [(char, char); 3] = [('(', ')'), ('[', ']'), ('{', '}')];

/// Whether a character closes one of them.
#[must_use]
pub fn closes(character: char) -> bool {
    PAIRS.iter().any(|(_, close)| *close == character)
}

/// Whether a character opens one of them.
#[must_use]
pub fn opens(character: char) -> bool {
    PAIRS.iter().any(|(open, _)| *open == character)
}

/// The bracket at `at` and its partner, if there is one within `within`.
///
/// `within` is the range on screen. A partner outside it needs no answer:
/// nothing would be drawn for it, and a scan that ran to the end of a large
/// file to decide that would be work spent on a cell nobody sees.
#[must_use]
pub fn pair_at(
    text: &Text,
    highlights: &Highlights,
    at: ByteOffset,
    within: Range<ByteOffset>,
) -> Option<(ByteOffset, ByteOffset)> {
    if at < within.start || at >= within.end {
        return None;
    }
    let here = char_at(text, at)?;
    if literal(highlights, at) {
        // The cursor is on a bracket inside a string or a comment. It has no
        // partner in any useful sense, and pretending otherwise would mark a
        // character from some other expression.
        return None;
    }

    let (open, close, forward) = PAIRS.iter().find_map(|(open, close)| {
        if here == *open {
            Some((*open, *close, true))
        } else if here == *close {
            Some((*open, *close, false))
        } else {
            None
        }
    })?;

    let mut depth = 0i32;
    let mut cursor = at;
    loop {
        let character = char_at(text, cursor)?;
        if !literal(highlights, cursor) {
            if character == open {
                depth += if forward { 1 } else { -1 };
            } else if character == close {
                depth += if forward { -1 } else { 1 };
            }
            if depth == 0 && cursor != at {
                return Some(if forward { (at, cursor) } else { (cursor, at) });
            }
        }
        cursor = step(text, cursor, forward, &within)?;
    }
}

/// The pair nearest the cursor: the bracket it is on and its partner, or
/// else the innermost pair it is inside.
///
/// The second is what the screen wants and the key does not. A reader
/// standing in the middle of an argument list is asking where it starts and
/// ends, and lighting a pair only when the caret happens to land on one half
/// of it answers that for one column in forty. Going to a bracket from
/// inside one would be a jump nobody could predict, so `go-to-bracket`
/// keeps [`pair_at`].
///
/// Both halves have to be within `within`, for the same reason as there: a
/// function taller than the screen gets no mark from inside it, and the call
/// on the line the caret is on still does.
#[must_use]
pub fn nearest(
    text: &Text,
    highlights: &Highlights,
    at: ByteOffset,
    within: Range<ByteOffset>,
) -> Option<(ByteOffset, ByteOffset)> {
    pair_at(text, highlights, at, within.clone()).or_else(|| around(text, highlights, at, within))
}

/// The innermost pair with the cursor between its halves.
///
/// Backwards to the first opening bracket nothing between closes -- counting
/// all three kinds as one depth, since `f(a[0])` is balanced whichever kind
/// is asked about -- and then that bracket's partner, which has to be ahead
/// of the cursor: in broken code it can be behind it, as in `( [ )`, and a
/// pair that ends before the caret is not one the caret is in.
fn around(
    text: &Text,
    highlights: &Highlights,
    at: ByteOffset,
    within: Range<ByteOffset>,
) -> Option<(ByteOffset, ByteOffset)> {
    if at < within.start || at > within.end {
        return None;
    }
    let mut depth = 0usize;
    let mut cursor = at;
    let open = loop {
        cursor = step(text, cursor, false, &within)?;
        if literal(highlights, cursor) {
            continue;
        }
        let character = char_at(text, cursor)?;
        if closes(character) {
            depth += 1;
        } else if opens(character) {
            if depth == 0 {
                break cursor;
            }
            depth -= 1;
        }
    };
    pair_at(text, highlights, open, within).filter(|(_, close)| *close > at)
}

/// Whether a byte is inside a string or a comment.
fn literal(highlights: &Highlights, at: ByteOffset) -> bool {
    matches!(
        highlights.kind_at(at),
        Some(SyntaxKind::String | SyntaxKind::Comment | SyntaxKind::Escape)
    )
}

/// The character starting at a byte, if one does.
fn char_at(text: &Text, at: ByteOffset) -> Option<char> {
    let rope = text.rope();
    if at.get() >= rope.len_bytes() {
        return None;
    }
    rope.byte_slice(at.get()..).chars().next()
}

/// The next character's byte, in whichever direction, staying inside the
/// range.
fn step(
    text: &Text,
    from: ByteOffset,
    forward: bool,
    within: &Range<ByteOffset>,
) -> Option<ByteOffset> {
    let rope = text.rope();
    if forward {
        let width = char_at(text, from)?.len_utf8();
        let next = from.saturating_add(width);
        (next < within.end).then_some(next)
    } else {
        if from <= within.start {
            return None;
        }
        // Back to the previous character boundary, which is not a fixed
        // distance: the byte before may be the middle of one.
        let character = rope.byte_to_char(from.get()).checked_sub(1)?;
        let next = ByteOffset::new(rope.char_to_byte(character));
        (next >= within.start).then_some(next)
    }
}
