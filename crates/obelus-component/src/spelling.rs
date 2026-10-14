//! A word an input method is spelling, written into the box it is spelled
//! at.
//!
//! Written in for real, the way Zed does it, rather than drawn over the box:
//! a spelling drawn over the cells after the caret ran off the side of the
//! box, where the same letters typed went onto the next row and pushed what
//! followed them along. So each spelling is written over the one before it,
//! and the word an input method finally commits is written over the last.

use obelus_editing::Cursor;

/// A box a spelling can be written into and taken back out of.
///
/// Two questions, because what writes it is the box's own paste -- which
/// knows what else a change to that box has to do -- and all that paste
/// needs is for the last spelling to be held when it arrives.
pub trait Spells {
    /// Where what is written in now begins: the start of what is held, or
    /// the caret.
    fn writes_from(&self) -> Cursor;

    /// Takes hold of what runs from `from` to the caret, where that is
    /// `said`, and says whether it was.
    fn hold_back_to(&mut self, from: Cursor, said: &str) -> bool;
}
