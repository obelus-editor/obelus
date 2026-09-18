//! Putting back what an edit took away.
//!
//! A journal of what changed rather than of what the document was: a copy of
//! the whole text per keystroke is the obvious thing and is unaffordable in a
//! file worth reading, and a diff between two of them would be working out
//! again what the edit knew at the time.
//!
//! Steps are kept in groups, because a reader who typed a word and presses
//! undo once means the word. What makes a group is not time -- a pause is not
//! a decision, and a test of one cannot be written that does not sleep -- but
//! what the reader was doing: a run of typing is a group, a run of deleting
//! is another, and anything else is a group of its own.
//!
//! Undo groups by what the reader was doing, not by when. A pause is not a
//! decision, and a test of one cannot be written without sleeping in it.

use crate::coordinates::CharOffset;

/// What the reader was doing, for deciding where one group ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Doing {
    /// Putting characters in one at a time.
    Typing,
    /// Taking them out one at a time, from either side of the cursor.
    Deleting,
    /// Everything else -- a paste, a cut, a replaced selection.
    ///
    /// Never joins anything and nothing joins it: a reader who pastes a
    /// function and then types is owed two steps back, not one.
    Whole,
    /// Part of the act before it, wherever in the document it landed.
    ///
    /// One action that edits in two places -- a completion accepted with
    /// the import it needs -- is one press of undo. The caller says so,
    /// because only the caller knows the two edits are one act; nothing
    /// about the offsets says it, and an import is nowhere near the word
    /// that wanted it.
    Joined,
}

/// One change, and enough to undo it.
#[derive(Clone, Debug)]
pub struct Step {
    /// Where it happened, in the document as it was.
    pub at: CharOffset,
    /// What was there, to put back.
    pub removed: String,
    /// What is there now, to take out.
    pub inserted: String,
    /// What the reader was doing when they made it.
    pub doing: Doing,
}

impl Step {
    /// Where what it put in ends.
    fn ends(&self) -> CharOffset {
        CharOffset::new(self.at.get() + self.inserted.chars().count())
    }

    /// Whether `next` carries straight on from this one.
    ///
    /// Typing carries on where it left off. Deleting carries on from either
    /// side: backspace eats backwards so the next removal ends where this one
    /// began, and the delete key eats forwards so the next one begins where
    /// this one did.
    fn runs_into(&self, next: &Self) -> bool {
        // An edit that says it belongs to what came before it joins
        // whatever that was, however far away it landed.
        if next.doing == Doing::Joined {
            return true;
        }
        if self.doing != next.doing || self.doing == Doing::Whole {
            return false;
        }
        match self.doing {
            // A newline is a place a reader would expect undo to stop, and
            // it is the one punctuation of a run of typing.
            Doing::Typing => !self.inserted.contains('\n') && next.at == self.ends(),
            Doing::Deleting => {
                let back = next.at.get() + next.removed.chars().count() == self.at.get();
                back || next.at == self.at
            }
            Doing::Whole | Doing::Joined => false,
        }
    }
}

/// Everything a document can be put back to.
#[derive(Debug, Default)]
pub struct Undo {
    /// Groups already made, oldest first. The last is the open one.
    done: Vec<Vec<Step>>,
    /// Groups undone, for redoing. Emptied by any new edit.
    undone: Vec<Vec<Step>>,
    /// Whether the last group is still taking steps.
    ///
    /// Apart from the group itself, because a group that has been closed is
    /// not an empty group -- it is a finished one, and the next step starts
    /// another.
    open: bool,
}

impl Undo {
    /// Writes a step down.
    ///
    /// Anything the reader can undo is something they can no longer redo:
    /// the future they had is not the future they are making.
    pub fn record(&mut self, step: Step) {
        self.undone.clear();
        let joins = self.open
            && self
                .done
                .last()
                .and_then(|group| group.last())
                .is_some_and(|last| last.runs_into(&step));
        match self.done.last_mut().filter(|_| joins) {
            Some(group) => group.push(step),
            None => {
                self.done.push(vec![step]);
                self.open = true;
            }
        }
    }

    /// Says the reader did something else, so the next step starts a group.
    ///
    /// Moving the cursor, saving, leaving. Not the cursor moving *because* of
    /// an edit -- that is the edit, and a group that closed on it would be a
    /// group of one step.
    pub const fn close(&mut self) {
        self.open = false;
    }

    /// The most recent group, taken off to be put back.
    pub fn undo(&mut self) -> Option<Vec<Step>> {
        let group = self.done.pop()?;
        self.open = false;
        self.undone.push(group.clone());
        Some(group)
    }

    /// The most recently undone group, to be done again.
    pub fn redo(&mut self) -> Option<Vec<Step>> {
        let group = self.undone.pop()?;
        self.open = false;
        self.done.push(group.clone());
        Some(group)
    }

    /// Whether there is anything to undo.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.done.is_empty()
    }

    /// Whether there is anything to redo.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }

    /// Forgets everything, for a document that has been replaced wholesale.
    ///
    /// A re-read is not an edit anybody made, and the steps before it are
    /// about text that is not there any more.
    pub fn forget(&mut self) {
        self.done.clear();
        self.undone.clear();
        self.open = false;
    }
}
