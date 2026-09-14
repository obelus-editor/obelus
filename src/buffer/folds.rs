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

use crate::{
    coordinates::{CharColumn, LineNumber},
    text::Text,
};

/// One run of lines that folds into its first one.
///
/// A *range*, not a run of whole lines, which is the difference that makes
/// everything else fall out. The line it starts on stays on screen; what is
/// left of the line it ends on -- a `}`, a `);`, a `</div>` -- stays with
/// it, on the same row, beside the mark. A block then reads as a block,
/// `if ready { … }`, and a language whose blocks close with nothing reads
/// as `def ready(): …`, without obelus knowing which language it is looking
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

/// The brackets a closing line may begin with.
///
/// The same three [`crate::syntax::brackets`] pairs. What is wanted here is
/// narrower than matching them: a line that *begins* with one of these is a
/// line that closes something, and that is true of `}`, `);` and `]` alike
/// without knowing what was opened or where.
const CLOSERS: [char; 3] = [')', ']', '}'];

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
#[must_use]
pub fn of(text: &Text) -> Vec<Fold> {
    (0..text.line_count())
        .filter_map(|row| at(text, LineNumber::new(row)))
        .collect()
}

/// The run starting on a line, if one does.
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
            Some(next) => break next >= indent,
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
fn indent_of(text: &Text, line: LineNumber) -> Option<usize> {
    let text = text.line(line).to_string();
    let indent = text
        .chars()
        .take_while(|character| character.is_whitespace());
    let indent = indent.count();
    (indent < text.chars().count()).then_some(indent)
}

/// Whether a line begins with something that closes a block.
fn starts_closed(text: &Text, line: LineNumber) -> bool {
    text.line(line)
        .to_string()
        .trim_start()
        .starts_with(CLOSERS)
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
