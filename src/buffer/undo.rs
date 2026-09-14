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
            Doing::Whole => false,
        }
    }
}

/// One group of steps, and what tells it from every other group.
#[derive(Debug)]
struct Group {
    /// Never reused, which is what makes it an answer to "is the document
    /// where it was when it was written".
    id: u64,
    /// What it did, in the order it did it.
    steps: Vec<Step>,
}

/// Everything a document can be put back to.
#[derive(Debug, Default)]
pub struct Undo {
    /// Groups already made, oldest first. The last is the open one.
    done: Vec<Group>,
    /// Groups undone, for redoing. Emptied by any new edit.
    undone: Vec<Group>,
    /// Whether the last group is still taking steps.
    ///
    /// Apart from the group itself, because a group that has been closed is
    /// not an empty group -- it is a finished one, and the next step starts
    /// another.
    open: bool,
    /// The id the next group will have.
    next: u64,
    /// Which group the document was on when it was last written.
    ///
    /// `None` for a document that has never been written and for one just
    /// read from disk, which are the same state: no group made yet.
    saved: Option<u64>,
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
                .and_then(|group| group.steps.last())
                .is_some_and(|last| last.runs_into(&step));
        match self.done.last_mut().filter(|_| joins) {
            Some(group) => group.steps.push(step),
            None => {
                self.done.push(Group {
                    id: self.next,
                    steps: vec![step],
                });
                // Never given out twice, so a group that was undone and
                // replaced by new work cannot be mistaken for the one the
                // document was written at.
                self.next = self.next.saturating_add(1);
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
        let steps = group.steps.clone();
        self.undone.push(group);
        Some(steps)
    }

    /// The most recently undone group, to be done again.
    pub fn redo(&mut self) -> Option<Vec<Step>> {
        let group = self.undone.pop()?;
        self.open = false;
        let steps = group.steps.clone();
        self.done.push(group);
        Some(steps)
    }

    /// Says the document as it stands is what is on disk.
    ///
    /// And closes the group, because a save is something the reader did
    /// between one edit and the next: typing that carried on across it
    /// would undo back past the thing they wrote.
    pub fn settled(&mut self) {
        self.saved = self.done.last().map(|group| group.id);
        self.open = false;
    }

    /// Whether the document differs from what was last written.
    ///
    /// Which group the document is on rather than how many it has made:
    /// undoing back to the group it was written at is being back at what is
    /// on disk, however many edits and undos it took to get there, and new
    /// work after an undo lands on a group id that has never been seen
    /// before rather than on the number one happened to have.
    #[must_use]
    pub fn changed(&self) -> bool {
        self.done.last().map(|group| group.id) != self.saved
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
        // No group made, which is the state a document read from disk is
        // in: it is what is on disk. `next` is not put back, so an id is
        // still never given out twice.
        self.saved = None;
    }
}
