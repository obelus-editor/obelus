//! Where the reader has been, and how to get back.

use obelus_buffer::DocumentId;
use obelus_text::coordinates::{CharColumn, LineNumber};

/// Somewhere the reader has been.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Jump {
    /// Which open document.
    pub document: DocumentId,
    /// Where in it, for a document that has a where.
    ///
    /// `None` for a conversation, which has no lines to be on. It is still
    /// somewhere the reader was, and leaving it for a file is still a jump
    /// they will want to come back from -- so it goes in the history with
    /// nothing where the line would be, rather than not going in at all.
    pub at: Option<(LineNumber, CharColumn)>,
}

/// The places jumped from, and where in that history the reader is.
///
/// A browser's history rather than a stack: going back and then somewhere new
/// throws away the forward entries, because a history that branches is one
/// nobody can predict.
#[derive(Debug, Default)]
pub struct JumpList {
    entries: Vec<Jump>,
    /// How many entries are behind the reader.
    ///
    /// Equal to the length when there is nothing to go forward to.
    at: usize,
}

impl JumpList {
    /// Records a place being left.
    pub fn push(&mut self, jump: Jump) {
        self.entries.truncate(self.at);
        // A jump from where the reader already is adds nothing to go back to.
        if self.entries.last() == Some(&jump) {
            return;
        }
        self.entries.push(jump);
        self.at = self.entries.len();
    }

    /// Whether there is anywhere behind the reader.
    ///
    /// Asked without moving, so the palette can leave the command out when
    /// there is nothing to go back to. [`JumpList::back`] takes `&mut self`
    /// because going back records where it went back *from*, which is not a
    /// question anyone can ask twice.
    #[must_use]
    pub const fn can_go_back(&self) -> bool {
        self.at > 0
    }

    /// And whether there is anywhere in front.
    #[must_use]
    pub const fn can_go_forward(&self) -> bool {
        self.at + 1 < self.entries.len()
    }

    /// Steps back, given where the cursor is now so it can be stepped forward
    /// to again.
    pub fn back(&mut self, current: Jump) -> Option<Jump> {
        if self.at == 0 {
            return None;
        }
        if self.at == self.entries.len() {
            // Standing at the end of the history: what the reader is looking
            // at now is not in it yet, and going forward has to come back
            // here.
            self.entries.push(current);
        }
        self.at -= 1;
        self.entries.get(self.at).copied()
    }

    /// Steps forward, if there is anywhere to go.
    pub fn forward(&mut self) -> Option<Jump> {
        if self.at + 1 >= self.entries.len() {
            return None;
        }
        self.at += 1;
        self.entries.get(self.at).copied()
    }

    /// Moves the places recorded in one document across an edit.
    ///
    /// A place is a line and a column, and an edit above one moves it: two
    /// lines put in at the top of a file and everything the reader might go
    /// back to is two lines further down. Without this, going back lands on
    /// whatever has drifted into those numbers -- which is the same
    /// confident wrongness a stale diff has, and harder to notice, because
    /// the reader asked to go somewhere and did go somewhere.
    ///
    /// `from` and `to` are the lines the edit covered, before it was made.
    /// A place above them is where it was; one below has moved by however
    /// many lines the edit added or took away; and one *inside* them is a
    /// place that is not there any more -- the nearest thing left to it is
    /// where the edit began.
    pub fn keep_across(
        &mut self,
        document: DocumentId,
        from: LineNumber,
        to: LineNumber,
        moved: isize,
    ) {
        for entry in &mut self.entries {
            let Some((line, _)) = entry.at else {
                continue;
            };
            if entry.document != document || line <= from {
                continue;
            }
            if line <= to {
                // The column belonged to a line that is gone.
                entry.at = Some((from, CharColumn::new(0)));
                continue;
            }
            entry.at = Some((
                line.saturating_add_signed(moved),
                entry
                    .at
                    .map_or_else(|| CharColumn::new(0), |(_, column)| column),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jump(line: usize) -> Jump {
        Jump {
            document: DocumentId::new(0),
            at: Some((LineNumber::new(line), CharColumn::new(0))),
        }
    }

    #[test]
    fn nothing_to_go_back_to_at_the_start() {
        let mut list = JumpList::default();
        assert_eq!(list.back(jump(9)), None);
        assert_eq!(list.forward(), None);
    }

    #[test]
    fn back_returns_where_the_jump_was_made_from() {
        let mut list = JumpList::default();
        list.push(jump(1));
        assert_eq!(list.back(jump(50)), Some(jump(1)));
    }

    /// The property that makes it a history rather than a stack: what was
    /// left has to be reachable again.
    #[test]
    fn forward_returns_to_where_back_was_pressed() {
        let mut list = JumpList::default();
        list.push(jump(1));
        assert_eq!(list.back(jump(50)), Some(jump(1)));
        assert_eq!(list.forward(), Some(jump(50)));
        assert_eq!(list.forward(), None);
    }

    #[test]
    fn a_run_of_jumps_walks_back_through_all_of_them() {
        let mut list = JumpList::default();
        for line in 1..=3 {
            list.push(jump(line));
        }
        assert_eq!(list.back(jump(99)), Some(jump(3)));
        assert_eq!(list.back(jump(99)), Some(jump(2)));
        assert_eq!(list.back(jump(99)), Some(jump(1)));
        assert_eq!(list.back(jump(99)), None);
    }

    /// Going back and then somewhere new throws the forward entries away. A
    /// history that branches is one nobody can predict.
    #[test]
    fn a_new_jump_after_going_back_forgets_the_way_forward() {
        let mut list = JumpList::default();
        list.push(jump(1));
        list.push(jump(2));
        assert_eq!(list.back(jump(99)), Some(jump(2)));

        list.push(jump(7));
        assert_eq!(list.forward(), None, "the forward entries should be gone");

        // Walking back has to skip what was discarded. Checking only that
        // forward is empty is not enough: leaving the entries in place makes
        // forward empty anyway, because the new jump moves the mark to the
        // end. They come back going the other way.
        assert_eq!(list.back(jump(50)), Some(jump(7)));
        assert_eq!(
            list.back(jump(50)),
            Some(jump(1)),
            "a discarded entry came back"
        );
        assert_eq!(list.back(jump(50)), None);
    }

    /// Jumping from where the cursor already is adds nothing to go back to,
    /// or pressing the same key twice would need pressing back twice.
    #[test]
    fn jumping_from_the_same_place_twice_records_it_once() {
        let mut list = JumpList::default();
        list.push(jump(4));
        list.push(jump(4));
        assert_eq!(list.back(jump(99)), Some(jump(4)));
        assert_eq!(list.back(jump(99)), None);
    }
}
