//! The hunks between two versions of a file, and what to draw for them.
//!
//! Pure: two strings in, a list of hunks out. Everything about how a change
//! is shown is decided here as data -- which line carries which marker, and
//! what text a deletion would show if it were opened -- so the view has
//! nothing to work out and the rules can be tested without a screen.

use imara_diff::{Algorithm, Diff, InternedInput};

use crate::coordinates::LineNumber;

/// One run of lines that differs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    /// The first line of the run in the file as it is now.
    ///
    /// For a deletion this is the line the removed text *was* in front of,
    /// which is the seam the marker hugs.
    pub line: LineNumber,
    /// How many lines the run covers now. Zero for a deletion, which covers
    /// none: that is what makes it a seam rather than a run.
    pub lines: usize,
    /// The lines the last commit had here, if it had any.
    ///
    /// Kept so a hunk can be opened in place without going back to git. It
    /// is the whole of what a deletion has to say, and half of what a
    /// modification has.
    pub removed: Vec<String>,
}

impl Hunk {
    /// What to draw in the margin for it.
    #[must_use]
    pub const fn marker(&self) -> Marker {
        match (self.lines, self.removed.is_empty()) {
            // Lines here now, and nothing here before.
            (1.., true) => Marker::Added,
            // Lines here now, and different lines before.
            (1.., false) => Marker::Modified,
            // Nothing here now: the lines are gone, and what is left is the
            // seam they were between.
            (0, _) => Marker::Removed,
        }
    }

    /// Whether a line is inside the run.
    ///
    /// A deletion covers the line it sits in front of, so that the reader
    /// standing there can open it: a seam with nothing selectable next to it
    /// is a mark that cannot be acted on.
    #[must_use]
    pub const fn covers(&self, line: LineNumber) -> bool {
        let at = line.get();
        let start = self.line.get();
        at >= start && at < start + if self.lines == 0 { 1 } else { self.lines }
    }
}

/// What a line's marker means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    /// The line is new since the last commit.
    Added,
    /// The line replaced something.
    Modified,
    /// Lines were removed from in front of this one.
    ///
    /// The only one that is about a *boundary* rather than about the line
    /// itself, which is why it is drawn differently: a full-height bar would
    /// claim the line changed, and it did not.
    Removed,
}

/// Every difference between the committed file and the one on disk.
#[derive(Clone, Debug, Default)]
pub struct Changes {
    hunks: Vec<Hunk>,
}

impl Changes {
    /// Diffs the committed text against the working tree's.
    #[must_use]
    pub fn between(before: &str, after: &str) -> Self {
        let input = InternedInput::new(before, after);
        let diff = Diff::compute(Algorithm::Histogram, &input);

        let lines: Vec<&str> = before.lines().collect();
        let hunks = diff
            .hunks()
            .map(|hunk| {
                let removed = hunk
                    .before
                    .clone()
                    .filter_map(|at| lines.get(at as usize).map(|line| (*line).to_string()))
                    .collect();
                Hunk {
                    line: LineNumber::new(hunk.after.start as usize),
                    lines: hunk.after.len(),
                    removed,
                }
            })
            .collect();
        Self { hunks }
    }

    /// Whether anything differs.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.hunks.is_empty()
    }

    /// Every hunk, in the order they appear in the file.
    #[must_use]
    pub fn hunks(&self) -> &[Hunk] {
        &self.hunks
    }

    /// What to draw in the margin beside a line.
    ///
    /// The same hunk [`Changes::hunk_at`] finds, which is what makes the
    /// mark and the thing the command opens the same thing. A deletion
    /// covers exactly the line it sits in front of, so it needs no case of
    /// its own here -- one was written, and breaking it deliberately changed
    /// nothing, which is how it turned out to be the same rule.
    #[must_use]
    pub fn marker_at(&self, line: LineNumber) -> Option<Marker> {
        self.hunk_at(line).map(Hunk::marker)
    }

    /// The hunk a line belongs to, for opening it in place.
    #[must_use]
    pub fn hunk_at(&self, line: LineNumber) -> Option<&Hunk> {
        self.hunks.iter().find(|hunk| hunk.covers(line))
    }
}
