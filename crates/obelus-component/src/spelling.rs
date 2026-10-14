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

    /// Takes hold of `said`, written in from `from`, where it is still
    /// there, and says whether it was.
    fn hold_written(&mut self, from: Cursor, said: &str) -> bool;

    /// Says what was just written in from `from` is what an input method
    /// is spelling -- which is what draws the line under it -- and hands
    /// back what that is.
    ///
    /// Which is not always `said`: a box that takes only some characters
    /// took only those, and the next spelling is written over what is
    /// there, not over what was offered. Where all of `said` went in, the
    /// caret goes `before` into it, where the input method has its own.
    fn spelling(&mut self, from: Cursor, said: &str, before: &str) -> String;

    /// Takes `said`, written in from `from`, back out, where it is still
    /// there -- and says whether it was.
    fn take_back(&mut self, from: Cursor, said: &str) -> bool;
}
