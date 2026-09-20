//! The hunks between two versions of a file, and what to draw for them.
//!
//! Pure: two strings in, a list of hunks out. Everything about how a change
//! is shown is decided here as data -- which line carries which marker, and
//! what text a deletion would show if it were opened -- so the view has
//! nothing to work out and the rules can be tested without a screen.

use imara_diff::{Algorithm, Diff, InternedInput};
use obelus_text::{coordinates::LineNumber, marker::Marker};

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

/// One row of a change as it is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// What the row is: gone, new, or neither.
    pub marker: Option<Marker>,
    /// The words.
    pub text: String,
}

/// A change nobody has made yet, as rows to draw.
///
/// For an agent asking to edit a file: the lines are in neither the file nor
/// the last commit, so there is nothing to work them out *from* -- the agent
/// sends the file as it is and as it would be, and this is the only place
/// those lines exist.
///
/// Through the same hunks the margin is drawn from, so a change that has not
/// happened is read the way every change that has is. Each hunk says which
/// line it is at, because that is what a reader would open.
#[must_use]
pub fn drawn(before: &str, after: &str) -> Vec<Line> {
    let lines: Vec<&str> = after.lines().collect();
    let mut rows = Vec::new();
    for hunk in Changes::between(before, after).hunks() {
        let at = hunk.line.get();
        rows.push(Line {
            marker: None,
            text: format!("line {}", at + 1),
        });
        rows.extend(hunk.removed.iter().map(|text| Line {
            marker: Some(Marker::Removed),
            text: text.clone(),
        }));
        rows.extend(lines.iter().skip(at).take(hunk.lines).map(|text| Line {
            marker: Some(Marker::Added),
            text: (*text).to_string(),
        }));
    }
    rows
}

/// How much a change adds and takes away.
#[must_use]
pub fn counted(lines: &[Line]) -> (usize, usize) {
    let count = |wanted| {
        lines
            .iter()
            .filter(|line| line.marker == Some(wanted))
            .count()
    };
    (count(Marker::Added), count(Marker::Removed))
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
        let mut diff = Diff::compute(Algorithm::Histogram, &input);
        // The tidying git does before it shows a diff, which a minimal
        // diff leaves undone: a block inserted where the lines around it
        // repeat can be written as starting a line or two earlier, and one
        // change can be written as two hunks with a line between them.
        // Every one of those readings is minimal, and only one of them is
        // where `git diff` draws the line -- which matters here, because
        // the marks down the margin are read beside it.
        //
        // Measured against this repository's own history: of seventy-nine
        // file diffs in the last dozen commits, nineteen are drawn
        // somewhere git does not draw them without this, and none is with
        // it.
        diff.postprocess_lines(&input);

        let was: Vec<&str> = before.lines().collect();
        let hunks = diff
            .hunks()
            .map(|hunk| {
                let removed: Vec<String> = hunk
                    .before
                    .clone()
                    .filter_map(|at| was.get(at as usize).map(|line| (*line).to_string()))
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

    /// Where a line of the working tree sits in the committed file, or
    /// `None` for a line that is not in the committed file at all.
    ///
    /// The smallest possible version of the map between two texts, and it
    /// exists because a blame is about the committed file while a reader is
    /// looking at this one: without it, every uncommitted line above the
    /// cursor would shift every name below it by one, and the answer would
    /// be confidently wrong rather than absent.
    ///
    /// A line inside an added or modified run has no committed counterpart,
    /// which is the honest `None`. A deletion shifts what follows it and
    /// makes no line uncommitted, so it needs no case of its own.
    #[must_use]
    pub fn committed_line(&self, line: LineNumber) -> Option<LineNumber> {
        let at = line.get();
        let mut offset: isize = 0;
        for hunk in &self.hunks {
            let start = hunk.line.get();
            if start > at {
                break;
            }
            if hunk.lines > 0 && at < start + hunk.lines {
                return None;
            }
            let removed = isize::try_from(hunk.removed.len()).unwrap_or(isize::MAX);
            let added = isize::try_from(hunk.lines).unwrap_or(isize::MAX);
            offset += removed - added;
        }
        usize::try_from(isize::try_from(at).unwrap_or(isize::MAX) + offset)
            .ok()
            .map(LineNumber::new)
    }

    /// Where a line of the committed file sits in this one, or `None` for a
    /// line the working tree no longer has.
    ///
    /// The other direction of [`Changes::committed_line`], and the one a
    /// note made while reading needs: it was written against the file as
    /// some commit had it, and what it points at has been moving ever since.
    /// Without this the note walks down the file as the lines above it are
    /// added to, which is worse than no line at all -- it is a line, and it
    /// is the wrong one.
    ///
    /// A line inside a run the commit removed is gone, which is the honest
    /// `None`: the note is about something that is not there any more, and
    /// saying so beats landing near it.
    #[must_use]
    pub fn working_line(&self, line: LineNumber) -> Option<LineNumber> {
        let at = isize::try_from(line.get()).unwrap_or(isize::MAX);
        // What has to be added to a committed line to get this file's, over
        // the hunks walked so far.
        let mut offset: isize = 0;
        for hunk in &self.hunks {
            // Where the hunk starts in the committed file, which is where it
            // starts here less everything the earlier hunks moved.
            let start = isize::try_from(hunk.line.get()).unwrap_or(isize::MAX) - offset;
            if start > at {
                break;
            }
            let removed = isize::try_from(hunk.removed.len()).unwrap_or(isize::MAX);
            if removed > 0 && at < start + removed {
                return None;
            }
            offset += isize::try_from(hunk.lines).unwrap_or(0) - removed;
        }
        usize::try_from(at + offset).ok().map(LineNumber::new)
    }

    /// The next change below a line, for stepping through them.
    ///
    /// Strictly below where it *starts*, so a cursor somewhere inside a
    /// long hunk moves to the next change rather than back to the top of
    /// the one it is already reading.
    #[must_use]
    pub fn hunk_after(&self, line: LineNumber) -> Option<&Hunk> {
        self.hunks.iter().find(|hunk| hunk.line.get() > line.get())
    }

    /// The next change above a line.
    ///
    /// The *last* one that starts above it, so from inside a hunk this is
    /// the top of that hunk -- which is where a reader stepping backwards
    /// through a long change wants to arrive, and one more press takes them
    /// to the change before it.
    #[must_use]
    pub fn hunk_before(&self, line: LineNumber) -> Option<&Hunk> {
        self.hunks
            .iter()
            .rev()
            .find(|hunk| hunk.line.get() < line.get())
    }
}
