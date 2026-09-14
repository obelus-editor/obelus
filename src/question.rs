//! Stopping to ask.
//!
//! A few of the things obelus does would throw work away if it simply did
//! them: closing a document with something unwritten in it, leaving with
//! several, saving over a file that moved while it was being edited. Each of
//! those stops and asks, in the compact list that is already how obelus asks
//! an agent's questions.
//!
//! Adding another one is three small things and no plumbing: an enum of its
//! own ways out, a variant of [`Answer`] carrying it, and an arm in
//! `App::answered` doing what was chosen. The list, the cancel row, the
//! escape key, the drawing and the preview all already know what a question
//! is.

use crate::buffer::BufferId;

/// A question, and the ways out of it in the order they are offered.
///
/// Built where it is asked and read by the picker, which is why it carries
/// words rather than rows: what a question is has nothing to do with how a
/// list draws one, and a question that built its own rows would have to
/// know about icons, tabs, depths and matching.
#[derive(Clone, Debug)]
pub struct Question {
    /// What is being asked, in words, shown in front of the prompt.
    ///
    /// One row, and the whole of what a question says about itself. A
    /// block of explanation above the ways out was tried and taken out
    /// again: what it had to add was either already in the prompt or not
    /// worth the rows, and the reader is deciding under a key they may
    /// have pressed by accident.
    prompt: String,
    /// What can be answered.
    ///
    /// Cancel is not among them: every question has it, it means the same
    /// thing in all of them, and the list puts it last itself.
    ways: Vec<Way>,
}

/// One way out of a question.
#[derive(Clone, Debug)]
pub struct Way {
    /// What the row says.
    pub label: String,
    /// What it says about itself, dimmed after the label.
    ///
    /// For the ways out that lose something, so that the row says so where
    /// the reader is already looking.
    pub detail: Option<String>,
    /// What choosing it means.
    pub answer: Answer,
}

impl Question {
    /// A question with no ways out yet.
    #[must_use]
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            ways: Vec::new(),
        }
    }

    /// Adds a way out, after the ones already there.
    ///
    /// Order is the order they are offered, and the first is the one the
    /// list opens on -- so the safe way out goes first, because a reader
    /// who answers a question they only half read should not lose anything
    /// by it.
    #[must_use]
    pub fn way(mut self, label: impl Into<String>, answer: Answer) -> Self {
        self.ways.push(Way {
            label: label.into(),
            detail: None,
            answer,
        });
        self
    }

    /// Says more about the way out just added.
    ///
    /// Attached to the last way rather than passed to [`Question::way`], so
    /// that the ways that have nothing to add -- most of them -- read as one
    /// line each.
    #[must_use]
    pub fn saying(mut self, detail: impl Into<String>) -> Self {
        if let Some(way) = self.ways.last_mut() {
            way.detail = Some(detail.into());
        }
        self
    }

    /// What is being asked.
    #[must_use]
    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    /// The ways out, in the order they are offered.
    #[must_use]
    pub fn ways(&self) -> &[Way] {
        &self.ways
    }
}

/// A question that was asked, and what it was told.
///
/// One variant per question, each carrying that question's own answers.
/// They really are different answers: a flat list of every question crossed
/// with every way out of it grows by three each time something new has to
/// ask, and nothing that matches on it can be sure which of its arms are
/// reachable.
///
/// [`Answer::Cancel`] is the exception and is shared, because it means the
/// same thing whatever was asked: do none of it. It is what escape does, and
/// it is a row of its own as well so that a reader walking the list with the
/// arrow keys can arrive at "no".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// Closing a document with something unwritten in it.
    Closing(BufferId, Closing),
    /// Leaving with something unwritten anywhere.
    Leaving(Leaving),
    /// Saving over a file that moved while it was being edited.
    Saving(BufferId, Saving),
    /// Saving a file somebody else took away.
    Writing(BufferId, Writing),
    /// Do none of it, whatever it was.
    Cancel,
}

/// The ways out of closing a document with unwritten changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Closing {
    /// Write it, then close it.
    Save,
    /// Close it, and lose what was not written.
    Discard,
}

/// The ways out of leaving with unwritten changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Leaving {
    /// Write all of them, then go.
    SaveAll,
    /// Go, and lose whatever was not written.
    Discard,
}

/// The ways out of a file that was deleted while it was being edited.
///
/// Its own question rather than a third way out of [`Saving`], because two
/// of that one's three answers are not available: there is nothing on disk
/// to take, and nothing there to write *over*.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Writing {
    /// Put it back where it was.
    Back,
    /// Accept that it is gone, and close the document with it.
    LetGo,
}

/// The ways out of a file that moved while it was being edited.
///
/// Not a safe way and a dangerous one, which the other two questions have:
/// each of these keeps one of the two versions and loses the other, and only
/// the reader knows which one matters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Saving {
    /// Keep what is in front of the reader, over what is on disk.
    Mine,
    /// Keep what is on disk, and re-read it.
    Theirs,
}
