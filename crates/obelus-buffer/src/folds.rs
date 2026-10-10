//! What the reader has folded away, and what is left to see.
//!
//! Two lists, and the difference between them is the point. What the *tree*
//! offers is a fact about the file and is replaced whenever the file is
//! parsed; what is *folded* is the reader's, one line at a time, and is
//! theirs until they say otherwise or the file is read again.
//!
//! Nothing here knows about screens. A fold hides lines, and every question
//! the views ask -- how tall is this line, which line is above that one --
//! goes on being answered by the same arithmetic with one term set to zero.
//!
//! A folded line is a line with no rows. Folding hides lines; an opened
//! hunk adds rows the file does not have. Both are the same arithmetic --
//! `screen_rows_of` and `step_rows` are where a view asks how tall a line is --
//! so folding is that one term going to zero rather than a second set of counts
//! beside the first. Everything falls out of it: the caret lands on the row the
//! line is drawn on, paging moves by what is on screen, the view walks past
//! what is hidden. What does *not* fall out is where the cursor may rest, which
//! is why folding over the reader walks them back to the line the run starts on
//! -- the one line of it still there -- and why arriving somewhere
//! (`place_cursor`) opens whatever hid it. Walking is the other thing: a step
//! goes around a fold, because the reader asked for the next line they can see.
//!
//! What folds comes from the indentation, and where it ends comes from the
//! bracket. Two other sources were built and thrown away, and the reason both
//! failed is the same half of the question. Deriving *which* lines fold from
//! tree-sitter's node shapes works; deriving what the folded row should then
//! *show* does not, because that needs to know a Rust block closes with `}` and
//! a Python one closes with nothing -- a table per language, wrong the day a
//! grammar changes, and every rule that guessed it from the text was wrong
//! somewhere (a "closing mark is at most four characters" test reads the `def`
//! at the end of a Python function as one). Asking a language server answers
//! both, but only for files a server will answer about, and its ranges are its
//! own: rust-analyzer ends a block one character *past* the `}`, so taking the
//! range at its word drops the brace, and it sends two runs for an `if` -- one
//! from the keyword, one from the brace.
//!
//! Indentation gives both halves at once, and it is what Zed settled on too. A
//! run opens on a line whose next non-blank line is deeper, and closes on the
//! first line that is no deeper. It starts at the *end* of the line that opens
//! it, so that line stays whole, `{` and all. It ends just before the closing
//! bracket when the line it closes on begins with one -- so the bracket comes
//! up beside the mark and the row reads as `if ready { … }` -- and at the last
//! line with anything on it when there is none, which is how `def ready(): …`
//! comes out of the same rule without a word about Python in it. Blank lines
//! are walked past inside a run and left outside it at the end: they belong to
//! whatever comes next.
//!
//! The price is that a file with nothing indented folds nowhere. A TOML file is
//! a list of tables at column zero, and so is most markdown and so is a
//! paragraph of `///` comments: there is no block for a reader to close, and a
//! mark offering to hide "the rest of the file from here" is a different offer.
//!
//! `obelus_syntax::brackets` knows the three pairs already, for the key that
//! matches them. What folding needs of it is narrower still -- a line that
//! *begins* with one of `)`, `]` or `}` closes something -- and that is true
//! without knowing what was opened or where.
//!
//! A hunk opens where it is, and so does the next one. `Buffer::blocks` is a
//! list by the line each hangs above, not one slot: a reader comparing two
//! changes wants both on screen, and the two they most want side by side are
//! the two they are deciding between. Which one a key acts on is then a
//! question the key has to answer, and the answer is not simply "the one above
//! this line": the caret's own block comes first, then the one belonging to the
//! hunk the reader is standing in -- which hangs above that hunk's *first* line
//! however far down it they have walked -- and then one hanging just below
//! them, which is where a reader who walked out of the top of one is left. A
//! selection is drawn in the block it was made in and nowhere else, because a
//! span is a pair of offsets into one text and against another it marks
//! whichever characters happen to sit there.
//!
//! A fold across an edit is not a fold across a re-read. `offer` throws away
//! everything the reader folded, which is right when the lines were replaced
//! and unusable per keystroke -- it means a file that unfolds itself as it is
//! typed into. `keep_across` moves them instead.

use obelus_text::{
    Text,
    coordinates::{CharColumn, LineNumber},
};

/// One run of lines that folds into its first one.
///
/// A *range*, not a run of whole lines, which is the difference that makes
/// everything else fall out. The line it starts on stays on screen; what is
/// left of the line it ends on -- a `}`, a `);`, a `</div>` -- stays with
/// it, on the same row, beside the mark. A block then reads as a block,
/// `if ready { … }`, and a language whose blocks close with nothing reads
/// as `def ready(): …`, without Obelus knowing which language it is looking
/// at. The server said where the run ends; that is the whole of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fold {
    /// The line that stays on screen.
    pub from: LineNumber,
    /// The last line of the run.
    pub to: LineNumber,
    /// Where the run stops on that line, or `None` when it takes the whole
    /// of it.
    ///
    /// `Some` is a line that closes with a bracket: the run stops just
    /// before it, so the bracket is what is left of the line and comes up
    /// beside the mark.
    pub tail: Option<CharColumn>,
}

/// Every run of lines this file offers to fold, one per line they start
/// on and in the order they start.
///
/// From the indentation, and from the shape of the line a block closes on.
/// Not from the syntax tree: a tree says which nodes span several lines, but
/// not what a folded row should then *show* -- that needs to know a Rust
/// block closes with `}` and a Python one closes with nothing, which is a
/// table per language and wrong the day a grammar changes. Not from a
/// language server either: a run is wanted the moment a file is on screen,
/// for every file, including the ones no server will ever answer about.
///
/// What indentation gives instead is both halves at once. A run starts at
/// the *end* of the line that opens it, so that line stays whole, `{` and
/// all. It ends just before the closing bracket when the line it closes on
/// has one, so that bracket comes up beside the mark and the row reads as
/// `if ready { … }`; and at the last line with anything on it when there is
/// no bracket, which is how `def ready(): …` comes out of the same rule
/// without a word about Python.
///
/// The price is that a file with no indentation has nothing to fold. A TOML
/// file is a list of tables at column zero, and so is most markdown: there
/// is no block for a reader to close, and a mark offering to hide "the rest
/// of the file from here" is not the same offer.
///
/// Worked out for the whole file in one pass over it, because it is asked
/// again after every edit: the shape of a run depends on the lines under it,
/// and asking that line by line is a scan down the file per line -- which on
/// a file of any size is the slowest thing a keystroke does. Everything the
/// answer needs is gathered first, and then the runs fall out of it.
#[must_use]
pub fn of(text: &Text) -> Vec<Fold> {
    let last = text.line_count().saturating_sub(1);
    let shapes = Shapes::of(text);
    (0..last)
        .filter_map(|row| shapes.run_at(row, last))
        .collect()
}

/// A line number, or none, in four bytes.
///
/// [`Shapes`] is five arrays the length of the file, built and thrown away
/// whenever a line's shape changes. `Option<usize>` says the same thing in
/// sixteen bytes a line, and a file of any size is better off not moving
/// four times the memory to say it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Row(u32);

impl Row {
    /// No line. A file with four billion lines in it is not a file.
    const NONE: Self = Self(u32::MAX);

    fn at(row: usize) -> Self {
        u32::try_from(row).map_or(Self::NONE, Self)
    }

    const fn get(self) -> Option<usize> {
        match self.0 {
            u32::MAX => None,
            row => Some(row as usize),
        }
    }
}

/// What every line of a file says about the runs around it, and the three
/// questions a run's own shape needs asked of the lines under it.
///
/// Gathered for the whole file in a few passes over it rather than asked
/// line by line, because the shape of a run depends on the lines under it:
/// asking one line at a time is a scan down the file per line, which on a
/// file of any size was the slowest thing a keystroke did.
struct Shapes {
    /// How far each line is indented, or [`Row::NONE`] for one with nothing
    /// on it. A line number's worth of room, holding a column: the same
    /// four bytes, and the same "there is no answer".
    indents: Vec<Row>,
    /// Whether what each line starts with closes a block.
    closes: Vec<bool>,
    /// The next line with anything on it, which is the one that says
    /// whether a line opens anything: blank lines say nothing either way.
    below: Vec<Row>,
    /// The last line with anything on it at or above each line, for
    /// trimming the blanks off the end of a run: they belong to whatever
    /// comes next rather than to the run above them.
    above: Vec<Row>,
    /// Where each run ends: the first line under it that is no deeper.
    shallower: Vec<Row>,
}

impl Shapes {
    fn of(text: &Text) -> Self {
        // One walk down the rope rather than a descent of its tree per
        // line, asking each line the only two things a run is made of.
        let lines = text.line_count();
        let mut indents = Vec::with_capacity(lines);
        let mut closes = Vec::with_capacity(lines);
        for line in text.rope().lines() {
            let (indent, closed) = shape(line.chars());
            indents.push(indent.map_or(Row::NONE, Row::at));
            closes.push(closed);
        }
        Self {
            below: nearest_below(&indents),
            above: nearest_above(&indents),
            shallower: shallower_than(&indents),
            indents,
            closes,
        }
    }

    /// The run starting on a line, if one does.
    fn run_at(&self, row: usize, last: usize) -> Option<Fold> {
        let indent = self.indents[row];
        if indent == Row::NONE {
            return None;
        }
        // What follows has to be deeper for this line to be opening
        // anything.
        let under = self.below[row].get()?;
        if self.indents[under].0 <= indent.0 {
            return None;
        }
        match self.shallower[row].get() {
            // Closed by a bracket: the run stops just before it, so the
            // bracket is what is left of that line and comes up beside the
            // mark.
            Some(line) if self.closes[line] => Some(Fold {
                from: LineNumber::new(row),
                to: LineNumber::new(line),
                tail: self.indents[line].get().map(CharColumn::new),
            }),
            // Closed by something else: the run takes everything down to
            // the last line with anything on it, and the row has only the
            // mark.
            Some(line) => self.ending(row, line.saturating_sub(1)),
            None => self.ending(row, last),
        }
    }

    /// A run from `row` down to the last line at or above `until` that has
    /// anything on it.
    fn ending(&self, row: usize, until: usize) -> Option<Fold> {
        self.above[until]
            .get()
            .filter(|to| *to > row)
            .map(|to| Fold {
                from: LineNumber::new(row),
                to: LineNumber::new(to),
                tail: None,
            })
    }
}

/// What a line contributes to the shape of the runs around it.
///
/// How far it is indented -- `None` for a line with nothing on it -- and
/// whether what it starts with closes a block. Those two are the whole of
/// what [`of`] reads, so two versions of a file whose lines all agree on
/// this offer exactly the same runs.
#[must_use]
pub fn shape_of(text: &Text, line: LineNumber) -> (Option<usize>, bool) {
    shape(text.line(line).chars())
}

/// The same, of characters already in hand.
fn shape(characters: impl Iterator<Item = char>) -> (Option<usize>, bool) {
    let first = characters
        .enumerate()
        .find(|(_, character)| !character.is_whitespace());
    (
        first.map(|(at, _)| at),
        first.is_some_and(|(_, character)| obelus_syntax::brackets::closes(character)),
    )
}

/// For each line, the next one under it with anything on it.
fn nearest_below(indents: &[Row]) -> Vec<Row> {
    let mut below = vec![Row::NONE; indents.len()];
    let mut nearest = Row::NONE;
    for row in (0..indents.len()).rev() {
        below[row] = nearest;
        if indents[row] != Row::NONE {
            nearest = Row::at(row);
        }
    }
    below
}

/// For each line, the last one at or above it with anything on it.
fn nearest_above(indents: &[Row]) -> Vec<Row> {
    let mut above = vec![Row::NONE; indents.len()];
    let mut nearest = Row::NONE;
    for (row, last_seen) in above.iter_mut().enumerate() {
        if indents[row] != Row::NONE {
            nearest = Row::at(row);
        }
        *last_seen = nearest;
    }
    above
}

/// For each line, the next one under it that is no deeper.
///
/// A stack of the lines still looking for one, deepest on top, so that every
/// line is pushed and popped once rather than scanned towards.
fn shallower_than(indents: &[Row]) -> Vec<Row> {
    let mut shallower = vec![Row::NONE; indents.len()];
    let mut waiting: Vec<usize> = Vec::new();
    for row in (0..indents.len()).rev() {
        let indent = indents[row];
        // A line with nothing on it says nothing about how deep anything
        // is, so it never goes on the stack and is never an answer. No test
        // can tell this apart from leaving it out -- `Row::NONE` is the
        // largest number there is, so a blank on the stack would be popped
        // before it could be read -- and relying on that would be relying
        // on a coincidence of the sentinel's value.
        if indent == Row::NONE {
            continue;
        }
        while waiting
            .last()
            .is_some_and(|line| indents[*line].0 > indent.0)
        {
            waiting.pop();
        }
        shallower[row] = waiting.last().map_or(Row::NONE, |line| Row::at(*line));
        waiting.push(row);
    }
    shallower
}

/// The run starting on a line, if one does.
///
/// The plain reading of the rule, a line at a time and scanning down the
/// file for the answer. Kept because it is the plain reading: [`of`] is the
/// same rule arranged so that nothing is scanned twice, and a test holds the
/// two to each other.
impl obelus_editing::Hides for Folds {
    /// What a motion steps over: a line inside a closed run is not a place
    /// the caret can be, because it is not a line anybody can see.
    fn hides(&self, line: LineNumber) -> bool {
        Self::hides(self, line)
    }
}

#[cfg(test)]
fn at(text: &Text, row: LineNumber) -> Option<Fold> {
    let last = text.last_line();
    if row >= last {
        return None;
    }
    let indent = indent_of(text, row)?;

    // What follows has to be deeper for this line to be opening anything.
    // Blank lines say nothing either way and are walked past.
    let mut below = row.saturating_add(1);
    let deeper = loop {
        if below > last {
            break false;
        }
        match indent_of(text, below) {
            Some(next) => break next > indent,
            None => below = below.saturating_add(1),
        }
    };
    if !deeper {
        return None;
    }

    // Then on to the first line that is no deeper than this one, which is
    // where the block ends.
    let mut closing = None;
    let mut at = below;
    while at <= last {
        if let Some(next) = indent_of(text, at)
            && next <= indent
        {
            closing = Some((at, next));
            break;
        }
        at = at.saturating_add(1);
    }

    match closing {
        // Closed by a bracket: the run stops just before it, so the bracket
        // is what is left of that line and comes up beside the mark.
        Some((line, at)) if starts_closed(text, line) => Some(Fold {
            from: row,
            to: line,
            tail: Some(CharColumn::new(at)),
        }),
        // Closed by something else: the run takes everything down to the
        // last line with anything on it, and the row has only the mark.
        Some((line, _)) => ending(text, row, line.saturating_sub(1)),
        None => ending(text, row, last),
    }
}

/// A run from `row` down to the last line at or above `until` that has
/// anything on it.
///
/// Trailing blank lines belong to whatever comes next, not to the run above
/// them: folding them away would close the gap between two items.
#[cfg(test)]
fn ending(text: &Text, row: LineNumber, until: LineNumber) -> Option<Fold> {
    let mut to = until;
    while to > row && indent_of(text, to).is_none() {
        to = to.saturating_sub(1);
    }
    (to > row).then_some(Fold {
        from: row,
        to,
        tail: None,
    })
}

/// How far a line is indented, or `None` for one with nothing on it.
///
/// Counted off the rope rather than off a copy of the line. This is asked
/// at least twice for every line of a file when it is opened and again
/// whenever it is re-read, and a `String` per question is an allocation per
/// line of a file nobody asked to have copied.
#[cfg(test)]
fn indent_of(text: &Text, line: LineNumber) -> Option<usize> {
    text.line(line)
        .chars()
        .position(|character| !character.is_whitespace())
}

/// Whether a line begins with something that closes a block.
///
/// The closing half of the pairs [`obelus_syntax::brackets`] matches. What
/// is wanted here is narrower than matching them: a line that *begins* with
/// one closes something, and that is true of `}`, `);` and `]` alike
/// without knowing what was opened or where.
#[cfg(test)]
fn starts_closed(text: &Text, line: LineNumber) -> bool {
    text.line(line)
        .chars()
        .find(|character| !character.is_whitespace())
        .is_some_and(obelus_syntax::brackets::closes)
}

/// Which lines of a file are folded away.
#[derive(Debug, Default)]
pub struct Folds {
    /// What the tree says can fold, in the order the runs start.
    offered: Vec<Fold>,
    /// Which of them the reader has folded, likewise.
    folded: Vec<Fold>,
    /// The lines those cover, merged into as few runs as they make.
    ///
    /// Kept beside `folded` rather than worked out per question: "is this
    /// line hidden" is asked for every line of every frame, and folds
    /// change when a reader presses a key.
    hidden: Vec<(usize, usize)>,
}

impl Folds {
    /// Takes what a parse says can be folded.
    ///
    /// Everything the reader had folded is dropped with it. A fold is a
    /// claim about lines, the lines have just been replaced, and a fold
    /// kept across that would hide whichever lines now sit at those
    /// numbers -- which is a different file's fold.
    pub fn offer(&mut self, offered: Vec<Fold>) {
        self.offered = offered;
        self.folded.clear();
        self.hidden.clear();
    }

    /// Takes what a fresh parse offers, keeping what the reader folded.
    ///
    /// For an edit rather than a re-read. A re-read replaces the whole file
    /// and [`offer`](Self::offer) is right for it; an edit moves a known
    /// number of lines at a known place, so a run the reader folded below
    /// the edit is the same run one line further down and throwing it away
    /// would unfold the file on every keystroke.
    ///
    /// Two kinds of fold do not survive: one the edit reached into, because
    /// the run it described is not the run that is there now; and one the
    /// fresh parse no longer offers at all, because a fold hiding lines
    /// nothing says are foldable is a fold about a file that has gone.
    pub fn keep_across(&mut self, offered: Vec<Fold>, after: LineNumber, moved: isize) {
        let shift = |line: LineNumber| match line > after {
            true => line.saturating_add_signed(moved),
            false => line,
        };
        self.offered = offered;
        self.folded = std::mem::take(&mut self.folded)
            .into_iter()
            // The edit landed inside it, so what it covered is not what it
            // covers.
            .filter(|fold| !(fold.from <= after && after <= fold.to))
            .map(|fold| Fold {
                from: shift(fold.from),
                to: shift(fold.to),
                ..fold
            })
            .filter(|fold| self.offered.iter().any(|offer| offer.from == fold.from))
            .collect();
        self.remeasure();
    }

    /// Whether the file has anything to fold at all.
    ///
    /// What decides whether the column is drawn: a file with nothing to
    /// fold spends no width saying so.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.offered.is_empty()
    }

    /// The innermost run that covers a line, if one does.
    ///
    /// Innermost because that is what a reader on a line means by "this":
    /// standing inside a method inside a class, the thing in front of them
    /// is the method.
    #[must_use]
    pub fn offered_at(&self, line: LineNumber) -> Option<Fold> {
        self.offered
            .iter()
            .filter(|fold| fold.from <= line && line <= fold.to)
            .max_by_key(|fold| fold.from)
            .cloned()
    }

    /// Whether anything at all is folded.
    ///
    /// What says the key that opens everything has something to open.
    #[must_use]
    pub fn any_folded(&self) -> bool {
        !self.folded.is_empty()
    }

    /// Whether every run on offer is folded.
    #[must_use]
    pub fn all_folded(&self) -> bool {
        !self.offered.is_empty() && self.folded.len() == self.offered.len()
    }

    /// Whether a fold starts on this line and is folded.
    ///
    /// What the column marks. Only the starting line, because it is the
    /// only line of the run still on screen.
    #[must_use]
    pub fn is_folded_at(&self, line: LineNumber) -> bool {
        self.folded_at(line).is_some()
    }

    /// The folded run that starts on this line, if one does.
    #[must_use]
    pub fn folded_at(&self, line: LineNumber) -> Option<Fold> {
        self.folded.iter().find(|fold| fold.from == line).cloned()
    }

    /// Whether a run starts on this line, folded or not.
    ///
    /// What the column marks. A run that is folded says so with the other
    /// mark; one that is open says it is there to fold, which is the only
    /// way a reader finds out that this line has more behind it.
    #[must_use]
    pub fn opens_at(&self, line: LineNumber) -> bool {
        self.offered.iter().any(|fold| fold.from == line)
    }

    /// Whether a line is hidden by some fold.
    #[must_use]
    pub fn hides(&self, line: LineNumber) -> bool {
        self.covering(line).is_some()
    }

    /// How many lines above this one are folded away.
    ///
    /// For the scrollbar and the change map, which are pictures of the
    /// document at the height of the screen: with a run closed they are
    /// pictures of a shorter document, and one drawn from the file's own
    /// line numbers says the reader is at the top of something long while
    /// the whole of it is in front of them.
    #[must_use]
    pub fn hidden_before(&self, line: LineNumber) -> usize {
        let line = line.get();
        self.hidden
            .iter()
            .take_while(|(from, _)| *from < line)
            .map(|(from, to)| (*to + 1).min(line).saturating_sub(*from))
            .sum()
    }

    /// How many lines of the file are folded away altogether.
    #[must_use]
    pub fn hidden_total(&self) -> usize {
        self.hidden.iter().map(|(from, to)| to + 1 - from).sum()
    }

    /// Which line is the `shown`-th of those not folded away, counting from
    /// nought.
    ///
    /// [`Folds::hidden_before`] the other way round, for the scrollbar: it
    /// counts in shown lines, and a pointer dragging its mark asks which
    /// line that many shown lines down is.
    #[must_use]
    pub fn nth_shown(&self, shown: usize) -> LineNumber {
        let mut line = shown;
        for (from, to) in &self.hidden {
            if *from > line {
                break;
            }
            line += to + 1 - from;
        }
        LineNumber::new(line)
    }

    /// The first line at or after this one that is not hidden.
    ///
    /// Past the end of the file for a fold reaching the last line, which
    /// the caller is already checking for: a view walks lines until it runs
    /// out of screen or out of file.
    #[must_use]
    pub fn first_shown(&self, line: LineNumber) -> LineNumber {
        let mut at = line;
        while let Some(end) = self.covering(at) {
            at = LineNumber::new(end.saturating_add(1));
        }
        at
    }

    /// Where the run hiding a line ends, if one hides it.
    fn covering(&self, line: LineNumber) -> Option<usize> {
        let line = line.get();
        self.hidden
            .binary_search_by(|(from, to)| {
                if line < *from {
                    std::cmp::Ordering::Greater
                } else if line > *to {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .ok()
            .map(|at| self.hidden[at].1)
    }

    /// Folds the run at a line, or unfolds it if it is folded already.
    ///
    /// Says whether anything changed, so a key that did nothing can say so
    /// rather than redrawing the same screen.
    pub fn toggle(&mut self, line: LineNumber) -> bool {
        // A folded run's own line is the one line of it still on screen, so
        // a reader standing there means that run and no other -- even when
        // an outer run covers the line as well.
        if self.is_folded_at(line) {
            self.folded.retain(|fold| fold.from != line);
            self.remeasure();
            return true;
        }
        let Some(fold) = self.offered_at(line) else {
            return false;
        };
        if self.folded.contains(&fold) {
            return false;
        }
        self.folded.push(fold);
        self.folded.sort_by_key(|fold| fold.from);
        self.remeasure();
        true
    }

    /// Unfolds whatever hides a line, so that it can be shown.
    ///
    /// For arriving somewhere rather than for walking there: a search hit,
    /// a definition, a line number typed in. Says whether anything changed.
    pub fn reveal(&mut self, line: LineNumber) -> bool {
        let before = self.folded.len();
        self.folded
            .retain(|fold| !(fold.from < line && line <= fold.to));
        if self.folded.len() == before {
            return false;
        }
        self.remeasure();
        true
    }

    /// Unfolds everything.
    pub fn unfold_all(&mut self) -> bool {
        if self.folded.is_empty() {
            return false;
        }
        self.folded.clear();
        self.hidden.clear();
        true
    }

    /// Folds every run the tree offers.
    pub fn fold_all(&mut self) -> bool {
        if self.offered.is_empty() || self.folded.len() == self.offered.len() {
            return false;
        }
        self.folded.clone_from(&self.offered);
        self.remeasure();
        true
    }

    /// Merges what the folded runs hide into as few runs as they make.
    ///
    /// Runs nest -- a folded method inside a folded class -- and asking
    /// each of them in turn is asking the same question several times. One
    /// sorted list of what is hidden answers it once.
    fn remeasure(&mut self) {
        self.hidden.clear();
        for fold in &self.folded {
            // The starting line stays on screen: it is what the reader
            // presses to get the rest back.
            let (from, to) = (fold.from.get() + 1, fold.to.get());
            if from > to {
                continue;
            }
            match self.hidden.last_mut() {
                Some((_, end)) if from <= *end + 1 => *end = (*end).max(to),
                _ => self.hidden.push((from, to)),
            }
        }
        self.hidden.sort_unstable();
        // Sorted by where they start, and a run inside another has already
        // been swallowed by the pass above unless its parent came later.
        let mut merged: Vec<(usize, usize)> = Vec::with_capacity(self.hidden.len());
        for (from, to) in self.hidden.drain(..) {
            match merged.last_mut() {
                Some((_, end)) if from <= *end + 1 => *end = (*end).max(to),
                _ => merged.push((from, to)),
            }
        }
        self.hidden = merged;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one-pass reading and the line-at-a-time one are the same rule,
    /// and a file is the only place to find out.
    fn agrees(source: &str) {
        let text = Text::from_string(source);
        let slow: Vec<Fold> = (0..text.line_count())
            .filter_map(|row| at(&text, LineNumber::new(row)))
            .collect();
        assert_eq!(of(&text), slow, "in:\n{source}");
    }

    #[test]
    fn the_two_readings_agree() {
        for source in [
            "",
            "\n",
            "one\n",
            "fn main() {\n    let x = 1;\n}\n",
            // A run closed by a bracket, and one closed by running out.
            "fn a() {\n    if b {\n        c();\n    }\n}\n\ndef d():\n    e()\n",
            // Blank lines inside a run and trailing it.
            "a:\n\n    b\n\n    c\n\n\nd:\n    e\n",
            // Nothing indented at all, which offers nothing.
            "one\ntwo\nthree\n",
            // Deeper and deeper, then out in one step.
            "a\n b\n  c\n   d\ne\n",
            // Out by more than one level at a time.
            "a\n    b\n        c\n    d\n",
            // No trailing newline.
            "a\n    b",
            // A line that is only blanks, which is not a line with
            // something on it however wide it is.
            "a\n    \n    b\n",
        ] {
            agrees(source);
        }
    }

    /// Counting shown lines skips what is folded, and is `hidden_before`
    /// read backwards.
    ///
    /// Deliberate break: start counting from the shown line and never add
    /// the runs. Below a closed run the answer is then a line inside it,
    /// and a bar dragged past one puts a hidden line on top.
    #[test]
    fn the_shown_lines_are_counted_past_what_is_folded() {
        let text = Text::from_string("a:\n    b\n    c\nd:\n    e\nf\n");
        let mut folds = Folds::default();
        folds.offer(of(&text));
        assert!(folds.toggle(LineNumber::new(0)), "nothing folded at a:");
        assert!(folds.toggle(LineNumber::new(3)), "nothing folded at d:");
        // a: d: f, and the end.
        let shown: Vec<usize> = (0..3).map(|at| folds.nth_shown(at).get()).collect();
        assert_eq!(shown, [0, 3, 5]);
        for at in shown {
            let line = LineNumber::new(at);
            assert_eq!(folds.nth_shown(at - folds.hidden_before(line)), line);
        }
    }

    /// And on real files, which have shapes nobody writes into a test.
    #[test]
    fn the_two_readings_agree_on_this_repository() {
        let root = std::path::Path::new(env!("OBELUS_TREE"));
        for file in [
            "crates/obelus-app/src/app/mod.rs",
            "crates/obelus-buffer/src/folds.rs",
            "crates/obelus-text/src/lib.rs",
            "Cargo.toml",
            "AGENTS.md",
        ] {
            let source = std::fs::read_to_string(root.join(file)).expect("a file of this repo");
            agrees(&source);
        }
    }
}
