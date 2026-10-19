//! A pull request or an issue, read as a page.
//!
//! **Reading one and asking an agent about it are two different acts.**
//! Choosing one from the list used to open the conversation that reviews
//! it, so the only way to read a pull request past what the list's preview
//! showed was to start a review of it. Now choosing one opens it, here, and
//! the conversation is a key of this page (`alt+a`, the notes' key for the
//! same act), which hands it back to the application as [`Outcome::Talk`].
//!
//! **A page with a cursor, like the transcript.** What a reader does with a
//! pull request on screen is read it, take a copy of a line of it and open
//! what is folded in it, so the cursor is [`crate::page`]'s: the arrows walk
//! the words, shift holds them and enter does what the row is for. And
//! nothing is written here: it is GitHub's page, read through `gh`, and a
//! key that is typing goes nowhere.
//!
//! **What enter does is open or shut, or copy.** A comment's heading opens
//! and shuts what it says, the checks' heading opens the checks that passed
//! -- which nobody has to do anything about, so they are counted until the
//! reader asks for their names -- and a block of code is copied whole, as
//! it is in the transcript, because holding it from corner to corner takes
//! its box and the width's breaks with it.
//!
//! **Laid out once and kept.** The rows are made at a width and kept until
//! the width or what they are made of changes, because the keys, the caret
//! and the drawing all ask for them on every keypress. What the reader has
//! hold of is marked on them afterwards, so a drag does not lay the page out
//! again on every report the terminal sends.

use std::ops::Range;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
pub use obelus_github::{
    About, Asked, Check, Comment, Did, Discussion, Issue, PullRequest, Stands, Unlisted,
};
use obelus_row::{Ink, Span};

use crate::{
    page::{Hold, Place},
    window::Window,
};

/// Which text of the page a row was laid out from.
///
/// In the order they are drawn, which is what puts the two ends of a
/// selection in order: the description is above every comment, and the
/// comments are drawn in the order they are kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Part {
    /// The title and the description, laid out as one piece of markdown.
    Description,
    /// What one comment says, by its place among them.
    Comment(usize),
}

/// What a heading opens and shuts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fold {
    /// The checks that passed or were skipped, by name.
    Checks,
    /// What one comment says, by its place among them.
    Comment(usize),
    /// A `<details>` in the description or a comment.
    Details {
        /// Which text it is in.
        of: Part,
        /// Where it begins in that text, in bytes.
        at: usize,
        /// Whether it asks to start open.
        asked_open: bool,
    },
}

/// What the reader has opened and shut on a page.
///
/// Its own thing rather than fields of the page, because the list's preview
/// lays the same rows out with nothing opened or shut: a preview that showed
/// something other than what choosing the row gives is a promise Obelus does
/// not keep.
#[derive(Clone, Debug, Default)]
pub struct Folding {
    /// Whether every check is named, or only those that did not pass.
    every_check: bool,
    /// The comments shut, by who said them and when.
    ///
    /// Not by their place: the newest comes first, so a comment arriving
    /// moves every place down one, and the one the reader shut became the
    /// one above it -- shut, while theirs stood open.
    shut: std::collections::HashSet<(String, Option<i64>)>,
    /// The `<details>` the reader has turned from how they started: by the
    /// comment they are in -- known the way a shut comment is, for the same
    /// reason -- or none for the description, and where in it.
    turned: std::collections::HashSet<(Option<Whose>, usize)>,
}

/// What a `<details>` is known by while the reader has it turned: which
/// text it is in, and where -- or nothing where that text is not there.
fn which_details(of: Part, at: usize, said: &[Comment]) -> Option<(Option<Whose>, usize)> {
    match of {
        Part::Description => Some((None, at)),
        Part::Comment(index) => said.get(index).map(|comment| (Some(whose(comment)), at)),
    }
}

/// What a comment is known by while the reader has it shut: who said it,
/// and when.
type Whose = (String, Option<i64>);

/// What a comment is known by while the reader has it shut.
fn whose(comment: &Comment) -> Whose {
    (comment.author.clone(), comment.when)
}

impl Folding {
    /// Whether what a heading folds is open, among the comments `said`.
    #[must_use]
    pub fn is_open(&self, fold: Fold, said: &[Comment]) -> bool {
        match fold {
            Fold::Checks => self.every_check,
            Fold::Comment(at) => said
                .get(at)
                .is_none_or(|comment| !self.shut.contains(&whose(comment))),
            Fold::Details { of, at, asked_open } => {
                let turned =
                    which_details(of, at, said).is_some_and(|key| self.turned.contains(&key));
                asked_open != turned
            }
        }
    }

    /// Opens what is shut and shuts what is open, among the comments
    /// `said`.
    pub fn flip(&mut self, fold: Fold, said: &[Comment]) {
        match fold {
            Fold::Checks => self.every_check = !self.every_check,
            Fold::Comment(at) => {
                let Some(comment) = said.get(at) else {
                    return;
                };
                let key = whose(comment);
                if !self.shut.remove(&key) {
                    self.shut.insert(key);
                }
            }
            Fold::Details { of, at, .. } => {
                let Some(key) = which_details(of, at, said) else {
                    return;
                };
                if !self.turned.remove(&key) {
                    self.turned.insert(key);
                }
            }
        }
    }
}

/// One row of the page, laid out at a width.
#[derive(Clone, Debug, Default)]
pub struct Row {
    /// What it says and how, and the block of code it is part of.
    pub row: obelus_row::Row,
    /// Which text it was laid out from, or nothing for a row the page made
    /// up: who wrote it and when, the checks, a blank.
    pub from: Option<Part>,
    /// What it opens and shuts, where it is a heading.
    pub folds: Option<Fold>,
    /// Whether that is open, for the mark that says so.
    pub open: bool,
    /// Whether it says something is on its way, which a mark that turns is
    /// drawn at the head of: the rows are kept, and a mark in them would
    /// stand still.
    pub waiting: bool,
    /// Which of its characters the reader has hold of.
    pub held: Option<Range<usize>>,
}

impl Row {
    /// A row of runs Obelus wrote, from no text of the page's.
    fn said(spans: Vec<Span>) -> Self {
        Self {
            row: obelus_row::Row::of(spans),
            ..Self::default()
        }
    }

    /// What the row says, with nothing about how it is drawn.
    #[must_use]
    pub fn text(&self) -> String {
        crate::page::text(self)
    }
}

impl crate::page::Line for Row {
    type Text = Part;

    fn spans(&self) -> &[Span] {
        &self.row.spans
    }

    fn laid_from(&self) -> Option<Part> {
        self.from
    }

    fn indent(&self) -> u16 {
        0
    }

    /// A block of code is one thing whichever row of it the cursor is on;
    /// every other row that acts is a heading, which is one row.
    fn acting(rows: &[Self], row: usize) -> Option<Range<usize>> {
        let here = rows.get(row)?;
        if let Some(code) = &here.row.code {
            let same = |row: &&Self| {
                row.row.code.as_ref().is_some_and(|it| it.at == code.at) && row.from == here.from
            };
            let start = row - rows[..row].iter().rev().take_while(same).count();
            let end = row + rows[row..].iter().take_while(same).count();
            return Some(start..end);
        }
        here.folds.is_some().then(|| row..row + 1)
    }

    fn held(&self) -> Option<&Range<usize>> {
        self.held.as_ref()
    }

    fn hold(&mut self, held: Range<usize>) {
        self.held = Some(held);
    }
}

/// What `gh` has said about it so far.
#[derive(Clone, Copy, Debug)]
pub enum Heard<'a> {
    /// Nothing yet: it is being asked.
    Asking,
    /// What it said.
    Said(&'a Discussion),
    /// Why it said nothing.
    Refused(&'a Unlisted),
}

/// The page, laid out at `width`.
///
/// Two rows naming it, in the shape a commit's message names its commit --
/// which, who, from where to where, how long ago, and how much it changes --
/// and then its title and its description as the markdown they are, laid
/// out by the same code a markdown file is. Then, each after a rule, how its
/// checks stand and what has been said on it: or, while `gh` has not said,
/// one row saying so.
///
/// The list's preview and the page are both this, so the two cannot show
/// different things.
///
/// `about` is the row naming it, which a page opened by its number alone
/// has not got until `gh` says: until then the number is all that names it.
#[must_use]
pub fn lay_out(
    asked: Asked,
    about: Option<&About>,
    heard: Heard<'_>,
    folding: &Folding,
    width: u16,
) -> Vec<Row> {
    let said = match heard {
        Heard::Said(said) => Some(said),
        Heard::Asking | Heard::Refused(_) => None,
    };
    let now = std::time::SystemTime::now();
    let mut rows = Vec::new();
    let named = match about {
        Some(About::PullRequest(pull)) => {
            let named = format!(
                "#{}   {}   {} \u{2192} {}",
                pull.number, pull.author, pull.head, pull.base
            );
            (named, pull.updated)
        }
        Some(About::Issue(issue)) => (
            format!("#{}   {}", issue.number, issue.author),
            issue.updated,
        ),
        None => (format!("#{}", asked.number()), None),
    };
    let (mut named, updated) = named;
    if let Some(updated) = updated {
        named.push_str(&format!("   {}", obelus_git::how_long_ago(updated, now)));
    }
    rows.push(Row::said(vec![Span::new(named, Ink::Aside)]));
    match (asked, said) {
        (Asked::PullRequest(_), Some(said)) => {
            let files = match said.files {
                1 => "1 file".to_string(),
                files => format!("{files} files"),
            };
            rows.push(Row::said(vec![
                Span::new(format!("+{}", said.additions), Ink::Added),
                Span::new(" ", Ink::Aside),
                Span::new(format!("\u{2212}{}", said.deletions), Ink::Removed),
                Span::new(format!(" \u{b7} {files}"), Ink::Aside),
            ]));
        }
        _ => {
            if let Some(About::Issue(issue)) = about
                && !issue.labels.is_empty()
            {
                rows.push(Row::said(vec![Span::new(
                    issue.labels.join(" \u{b7} "),
                    Ink::Key,
                )]));
            }
        }
    }
    rows.push(Row::default());
    rows.extend(described(
        about.map(About::title),
        said.map(|said| said.body.as_str()),
        folding,
        width,
    ));
    let rule = Row {
        row: obelus_row::Row {
            spans: Vec::new(),
            rule: true,
            code: None,
            links: Vec::new(),
            details: None,
        },
        ..Row::default()
    };
    rows.push(rule.clone());
    let said = match heard {
        Heard::Said(said) => said,
        Heard::Refused(why) => {
            rows.push(Row::said(vec![Span::new(unlisted(why), Ink::Aside)]));
            return rows;
        }
        Heard::Asking => {
            // Two blanks in front, where the view puts the mark and the one
            // blank after it.
            let waiting = match asked {
                Asked::PullRequest(_) => "  Asking GitHub for its description, checks and comments",
                Asked::Issue(_) => "  Asking GitHub for its description and comments",
            };
            rows.push(Row {
                waiting: true,
                ..Row::said(vec![Span::new(waiting, Ink::Aside)])
            });
            return rows;
        }
    };
    if matches!(asked, Asked::PullRequest(_)) {
        rows.extend(checks(&said.checks, folding));
        rows.push(rule);
    }
    rows.extend(comments(&said.said, folding, width));
    rows
}

/// A title as the heading it is, and the description after it as its
/// author wrote it -- a description with headings of its own keeps them
/// under this one.
///
/// With no description until `gh` has said what it is: "No description" is
/// a thing to say about one that came back empty, not one still asked.
///
/// With no title where nobody has said it yet, rather than a heading with
/// nothing in it.
fn described(title: Option<&str>, body: Option<&str>, folding: &Folding, width: u16) -> Vec<Row> {
    let heading = match title {
        Some(title) => format!("# {title}\n\n"),
        None => String::new(),
    };
    let source = format!("{heading}{}", body.unwrap_or_default());
    // A `<details>` is known by where it is in the description, not in the
    // title and the description together: a title edited would have moved
    // every one of them and turned them all back.
    let at = |at: usize| at.saturating_sub(heading.len());
    let rows = obelus_markdown::render_folding(&source, width, &|found, asked_open| {
        folding.is_open(
            Fold::Details {
                of: Part::Description,
                at: at(found),
                asked_open,
            },
            &[],
        )
    });
    let mut rows: Vec<Row> = rows
        .into_iter()
        .map(|row| Row {
            folds: row.details.map(|details| Fold::Details {
                of: Part::Description,
                at: at(details.at),
                asked_open: details.asked_open,
            }),
            open: row.details.is_some_and(|details| details.open),
            row,
            from: Some(Part::Description),
            ..Row::default()
        })
        .collect();
    if body.is_some_and(|body| body.trim().is_empty()) {
        rows.push(Row::default());
        rows.push(Row::said(vec![Span::new("No description", Ink::Aside)]));
    }
    rows
}

/// How a pull request's checks stand, as rows: a count of each kind on the
/// heading, and under it by name every one that has not passed -- or every
/// one, where the reader has opened them.
///
/// Not every name until then: a check that passed is one nobody has to do
/// anything about, and fourteen rows of them would push what was said off
/// the screen for a fact one count says.
fn checks(checks: &[Check], folding: &Folding) -> Vec<Row> {
    let heading = Span::new("Checks  ", Ink::Heading(2));
    if checks.is_empty() {
        return vec![Row::said(vec![heading, Span::new("None", Ink::Aside)])];
    }
    let count = |stands: Stands| checks.iter().filter(|check| check.stands == stands).count();
    // Worst first, so the count that matters is the one the eye lands on.
    let kinds = [
        (Stands::Failed, "\u{2717}", "failed", Ink::Removed),
        (Stands::Running, "\u{25cc}", "running", Ink::Doubtful),
        (Stands::Passed, "\u{2713}", "passed", Ink::Added),
        (Stands::Skipped, "\u{2013}", "skipped", Ink::Aside),
    ];
    let mut summary = vec![heading];
    for (stands, mark, said, ink) in kinds {
        let count = count(stands);
        if count == 0 {
            continue;
        }
        if summary.len() > 1 {
            summary.push(Span::new(" \u{b7} ", Ink::Aside));
        }
        summary.push(Span::new(format!("{mark} {count} {said}"), ink));
    }
    // A heading folds only where opening it shows something: with nothing
    // but failures and checks still running, every name is already there.
    let hidden = count(Stands::Passed) + count(Stands::Skipped) > 0;
    let open = folding.is_open(Fold::Checks, &[]);
    let mut rows = vec![Row {
        folds: hidden.then_some(Fold::Checks),
        open,
        ..Row::said(summary)
    }];
    let named = match open {
        true => &kinds[..],
        false => &kinds[..2],
    };
    for (stands, mark, _, ink) in named {
        for check in checks.iter().filter(|check| check.stands == *stands) {
            rows.push(Row::said(vec![
                Span::new(format!("  {mark} "), *ink),
                Span::new(check.name.clone(), Ink::Plain),
            ]));
        }
    }
    rows
}

/// What has been said on it, as rows, newest first: who and what they did,
/// which opens and shuts what they wrote, and then what they wrote, laid
/// out as the markdown it is.
fn comments(said: &[Comment], folding: &Folding, width: u16) -> Vec<Row> {
    if said.is_empty() {
        return vec![Row::said(vec![Span::new("No comments", Ink::Aside)])];
    }
    let now = std::time::SystemTime::now();
    let mut rows = Vec::new();
    for (at, comment) in said.iter().enumerate() {
        if at > 0 {
            rows.push(Row::default());
        }
        let mut did = format!("  {}", what_it_did(comment.did));
        if let Some(when) = comment.when {
            did.push_str(&format!(" \u{b7} {}", obelus_git::how_long_ago(when, now)));
        }
        let open = folding.is_open(Fold::Comment(at), said);
        rows.push(Row {
            folds: Some(Fold::Comment(at)),
            open,
            ..Row::said(vec![
                Span::new(comment.author.clone(), Ink::Name),
                Span::new(did, Ink::Aside),
            ])
        });
        if open {
            let of = Part::Comment(at);
            let laid =
                obelus_markdown::render_folding(&comment.body, width, &|found, asked_open| {
                    folding.is_open(
                        Fold::Details {
                            of,
                            at: found,
                            asked_open,
                        },
                        said,
                    )
                });
            rows.extend(laid.into_iter().map(|row| Row {
                folds: row.details.map(|details| Fold::Details {
                    of,
                    at: details.at,
                    asked_open: details.asked_open,
                }),
                open: row.details.is_some_and(|details| details.open),
                row,
                from: Some(of),
                ..Row::default()
            }));
        }
    }
    rows
}

/// What a page says about why `gh` said nothing.
#[must_use]
pub fn unlisted(why: &Unlisted) -> String {
    match why {
        Unlisted::NoGh => "Listing pull requests needs gh, which is not installed".to_string(),
        Unlisted::SignedOut => "Not signed in to GitHub: gh auth login signs in".to_string(),
        Unlisted::Failed(why) => format!("GitHub would not answer: {why}"),
    }
}

/// What the row naming a comment says it did.
const fn what_it_did(did: Did) -> &'static str {
    match did {
        Did::Commented => "commented",
        Did::Approved => "approved",
        Did::RequestedChanges => "requested changes",
        Did::Dismissed => "was dismissed",
    }
}

/// What a key did on the page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Not a key this page knows; try the key table.
    Ignored,
    /// Something moved or opened. Redraw.
    Consumed,
    /// Put this on the clipboard: a block of code, as it was written.
    Copy(String),
    /// Talk to the agent about this.
    Talk,
}

/// A pull request or an issue, while it is open.
#[derive(Debug)]
pub struct DiscussionView {
    /// Which one is being read.
    asked: Asked,
    /// What its row says, once the list or `gh` has said.
    about: Option<About>,
    /// What `gh` last said about it.
    said: Option<Discussion>,
    /// Why it said nothing, where it said nothing the last time it was
    /// asked. What was said before that stays, and is still true as far as
    /// anybody here knows.
    refused: Option<Unlisted>,
    /// What the reader has opened and shut.
    folding: Folding,
    /// The rows as they were last laid out, and the width they were laid
    /// out at. Dropped by everything that changes what there is to lay out.
    laid: std::cell::RefCell<Option<(u16, Vec<Row>)>>,
    /// Where the cursor is.
    cursor: Place,
    /// What the reader has hold of.
    held: Hold<Part>,
    /// Which rows are on screen.
    window: Window,
}

impl DiscussionView {
    /// A page about `about`, with nothing said about it yet.
    #[must_use]
    pub fn new(about: About) -> Self {
        Self {
            about: Some(about.clone()),
            ..Self::asking(about.asked())
        }
    }

    /// A page about one known by its number alone -- opened again after a
    /// restart, or from its conversation in a window whose list has not
    /// been asked -- until `gh` says what it is.
    #[must_use]
    pub fn asking(asked: Asked) -> Self {
        Self {
            asked,
            about: None,
            said: None,
            refused: None,
            folding: Folding::default(),
            laid: std::cell::RefCell::new(None),
            cursor: Place::default(),
            held: Hold::default(),
            window: Window::new(),
        }
    }

    /// Which one is being read.
    #[must_use]
    pub const fn asked(&self) -> Asked {
        self.asked
    }

    /// What its row says, once something has said.
    #[must_use]
    pub const fn about(&self) -> Option<&About> {
        self.about.as_ref()
    }

    /// Its number.
    #[must_use]
    pub const fn number(&self) -> u64 {
        self.asked.number()
    }

    /// What `gh` has said about it so far.
    #[must_use]
    pub fn heard(&self) -> Heard<'_> {
        match (&self.said, &self.refused) {
            (Some(said), _) => Heard::Said(said),
            (None, Some(why)) => Heard::Refused(why),
            (None, None) => Heard::Asking,
        }
    }

    /// Whether it is still waiting on `gh`, which is when a mark turns.
    #[must_use]
    pub const fn still_asking(&self) -> bool {
        self.said.is_none() && self.refused.is_none()
    }

    /// Takes what the list now says it is: a title edited, a push.
    pub fn listed_as(&mut self, about: About) {
        self.about = Some(about);
        self.forget_the_layout();
    }

    /// Takes what `gh` said about it.
    pub fn told(&mut self, said: Discussion) {
        // What is held is a place in a comment, by its place among them:
        // where they are no longer the comments they were, it would be the
        // same characters of another one.
        let whose_before: Vec<_> = self
            .said
            .as_ref()
            .map(|before| before.said.iter().map(whose).collect())
            .unwrap_or_default();
        let whose_now: Vec<_> = said.said.iter().map(whose).collect();
        if whose_before != whose_now {
            self.held.let_go();
        }
        // What it is called, said with the rest: as new as the list's word,
        // and the only word a page opened by its number gets.
        if let Some(about) = said.about.clone() {
            self.about = Some(about);
        }
        self.said = Some(said);
        self.refused = None;
        self.forget_the_layout();
    }

    /// Takes why `gh` said nothing.
    pub fn refused(&mut self, why: Unlisted) {
        self.refused = Some(why);
        self.forget_the_layout();
    }

    /// The rows at a width, with what the reader has hold of marked on them.
    #[must_use]
    pub fn rows(&self, width: u16) -> Vec<Row> {
        let mut rows = self.laid_out(width);
        self.held.mark(&mut rows);
        rows
    }

    /// The rows, laid out or remembered from the last time they were.
    fn laid_out(&self, width: u16) -> Vec<Row> {
        if let Some(rows) = self
            .laid
            .borrow()
            .as_ref()
            .filter(|(at, _)| *at == width)
            .map(|(_, rows)| rows.clone())
        {
            return rows;
        }
        let rows = lay_out(
            self.asked,
            self.about.as_ref(),
            self.heard(),
            &self.folding,
            width,
        );
        *self.laid.borrow_mut() = Some((width, rows.clone()));
        rows
    }

    /// Forgets the rows, because what they are made of has changed.
    fn forget_the_layout(&mut self) {
        *self.laid.get_mut() = None;
    }

    /// Where the cursor is.
    #[must_use]
    pub const fn cursor(&self) -> Place {
        self.cursor
    }

    /// Puts the cursor somewhere, for a press there.
    pub const fn stand(&mut self, at: Place) {
        self.cursor = at;
    }

    /// The first row on screen.
    #[must_use]
    pub const fn top(&self) -> usize {
        self.window.top()
    }

    /// Whether there is more than fits in `height` rows.
    #[must_use]
    pub fn scrollable(&self, height: u16) -> bool {
        self.window.scrollable(height)
    }

    /// Keeps the window within the rows, once a frame, and the cursor on
    /// one of them: a comment shut above it, or an answer with fewer rows
    /// than the waiting it replaced, can leave it past the end.
    pub fn settle(&mut self, rows: &[Row], height: u16) {
        self.window.set_count(rows.len());
        self.window.settle(height);
        let last = rows.len().saturating_sub(1);
        if self.cursor.row > last {
            self.cursor = Place {
                row: last,
                character: rows.get(last).map_or(0, crate::page::characters),
            };
        }
    }

    /// Scrolls, for the wheel: the view moves and the cursor stays where it
    /// was.
    pub fn scroll(&mut self, rows: isize) {
        self.window.scroll(rows);
    }

    /// Drags the view to a row, for a press on the scrollbar.
    pub fn drag_to(&mut self, top: usize) {
        self.window.drag_to(top);
    }

    /// Opens what is shut and shuts what is open.
    pub fn fold(&mut self, fold: Fold) {
        let said = self
            .said
            .as_ref()
            .map_or(&[][..], |said| said.said.as_slice());
        self.folding.flip(fold, said);
        self.forget_the_layout();
    }

    /// Takes hold of a place, letting go of anything else.
    pub fn hold_from(&mut self, spot: crate::page::Spot<Part>) {
        self.held.from(spot);
    }

    /// Drags the far end of what is held.
    pub fn hold_to(&mut self, spot: crate::page::Spot<Part>) {
        self.held.to(spot);
    }

    /// Lets go of whatever was held.
    pub fn let_go(&mut self) {
        self.held.let_go();
    }

    /// Lets go of a hold with nothing in it.
    pub fn let_go_of_nothing(&mut self) {
        self.held.let_go_of_nothing();
    }

    /// Whether what is held is a press at `at` that never became a drag.
    #[must_use]
    pub fn clicked(&self, at: crate::page::Spot<Part>) -> bool {
        self.held.clicked(at)
    }

    /// Whether anything is held.
    #[must_use]
    pub const fn holding(&self) -> bool {
        self.held.holding()
    }

    /// What is held, as it is drawn.
    #[must_use]
    pub fn held_text(&self, width: u16) -> Option<String> {
        if !self.held.holding() {
            return None;
        }
        crate::page::held_text(&self.rows(width))
    }

    /// Handles a key, on a page `room` big.
    ///
    /// Everything with a modifier on it is the application's, but for the
    /// ones that are the cursor's -- shift holding, control and the two
    /// ends -- and `alt+a`, which is this page's: the page is a document,
    /// so `ctrl+w` closes it and `f2` lists what is open from inside it.
    pub fn handle_key(&mut self, key: &KeyEvent, room: crate::page::Room) -> Outcome {
        let Some(modifiers) = obelus_keymap::modifiers_of(key) else {
            return Outcome::Ignored;
        };
        let bare = modifiers == KeyModifiers::NONE;
        // Talk about this one. `alt+a` because it is the notes' key for the
        // same act, and the agent's other keys are on `a`.
        if key.code == KeyCode::Char('a') && modifiers == KeyModifiers::ALT {
            return Outcome::Talk;
        }
        let ends = matches!(key.code, KeyCode::Home | KeyCode::End);
        let cursor = bare
            || modifiers == KeyModifiers::SHIFT
            || (ends && modifiers == KeyModifiers::CONTROL);
        let rows = self.rows(room.reading);
        if cursor
            && let Some(walked) = crate::page::walk(
                self.cursor,
                key.code,
                modifiers,
                &rows,
                room,
                &mut self.held,
                &mut self.window,
            )
        {
            if let crate::page::Walked::To { place, .. } = walked {
                self.cursor = place;
            }
            return Outcome::Consumed;
        }
        match key.code {
            // Whatever the row is for: a heading opens and shuts what is
            // under it, and a block of code is copied. On a row that is
            // only words it does nothing, because there is nothing to do.
            KeyCode::Enter if bare => {
                let Some(on) = <Row as crate::page::Line>::acting(&rows, self.cursor.row) else {
                    return Outcome::Consumed;
                };
                let Some(first) = rows.get(on.start) else {
                    return Outcome::Consumed;
                };
                if let Some(code) = &first.row.code {
                    return Outcome::Copy(code.text.to_string());
                }
                if let Some(fold) = first.folds {
                    // The heading stays under the reader: what moved is
                    // what is below it.
                    self.fold(fold);
                }
                Outcome::Consumed
            }
            // What is held goes first: escape gives up on the nearest
            // thing. With nothing held the page is a document, and leaving
            // it is not giving anything up.
            KeyCode::Esc if bare && self.held.holding() => {
                self.held.let_go();
                Outcome::Consumed
            }
            _ => Outcome::Ignored,
        }
    }

    /// The mark it wears wherever it is named -- its tab, its row in the
    /// list of what is open, the status row -- which says which of the two
    /// it is.
    #[must_use]
    pub fn icon(&self) -> char {
        match self.asked {
            Asked::PullRequest(_) => obelus_icons::ui::PULL_REQUEST,
            Asked::Issue(_) => obelus_icons::ui::ISSUE,
        }
    }

    /// What its tab and its row in the list of what is open are called.
    #[must_use]
    pub fn name(&self) -> String {
        match &self.about {
            Some(about) => format!("#{} {}", about.number(), about.title()),
            None => format!("#{}", self.asked.number()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pull() -> PullRequest {
        PullRequest {
            number: 7,
            title: "Fold the checks".to_string(),
            author: "someone".to_string(),
            head: "fold".to_string(),
            base: "master".to_string(),
            sha: "abc".to_string(),
            draft: false,
            decision: None,
            updated: None,
            stamp: String::new(),
        }
    }

    fn check(name: &str, stands: Stands) -> Check {
        Check {
            name: name.to_string(),
            stands,
        }
    }

    fn said() -> Discussion {
        Discussion {
            about: None,
            stamp: String::new(),
            body: "What it does.\n\n```\ncargo test\n```\n".to_string(),
            additions: 3,
            deletions: 1,
            files: 1,
            checks: vec![
                check("clippy", Stands::Failed),
                check("test", Stands::Passed),
            ],
            said: vec![Comment {
                author: "reviewer".to_string(),
                did: Did::Commented,
                when: None,
                body: "Looks right to me.".to_string(),
            }],
        }
    }

    const ROOM: crate::page::Room = crate::page::Room {
        height: 40,
        reading: 60,
    };

    fn press(page: &mut DiscussionView, code: KeyCode, modifiers: KeyModifiers) -> Outcome {
        page.handle_key(&KeyEvent::new(code, modifiers), ROOM)
    }

    fn texts(page: &DiscussionView) -> Vec<String> {
        page.rows(ROOM.reading).iter().map(Row::text).collect()
    }

    /// A `<details>` on the page starts shut and opens on enter, in the
    /// description and in a comment; and one the reader opened in a comment
    /// stays open when another comment arrives in front of it.
    ///
    /// Broken deliberately three ways. Leaving `folds` off the rows
    /// `described` makes: enter on the heading did nothing. Knowing a
    /// comment's `<details>` by the comment's place: the one opened shut
    /// again when the new comment arrived. And keying the description's by
    /// where it is in the title and the description together, with a
    /// title edited between: it shut.
    #[test]
    fn a_details_on_the_page_opens_on_enter() {
        let log = "<details>\n<summary>Full log</summary>\n\nThe log itself.\n\n</details>\n";
        let mut page = DiscussionView::new(About::PullRequest(pull()));
        let mut told = said();
        told.body = log.to_string();
        told.said[0].body = log.replace("log itself", "comment's log");
        page.told(told.clone());
        let before = texts(&page);
        assert!(
            !before
                .iter()
                .any(|row| row.contains("log itself") || row.contains("comment's log")),
            "a details started open: {before:?}"
        );

        let headings: Vec<usize> = before
            .iter()
            .enumerate()
            .filter(|(_, row)| row.starts_with("Full log"))
            .map(|(at, _)| at)
            .collect();
        assert_eq!(headings.len(), 2, "{before:?}");
        // The comment's first, while the rows are where they were.
        for heading in headings.into_iter().rev() {
            page.stand(Place {
                row: heading,
                character: 0,
            });
            let _ = press(&mut page, KeyCode::Enter, KeyModifiers::NONE);
        }
        let opened = texts(&page);
        assert!(
            opened.iter().any(|row| row.contains("The log itself.")),
            "enter did not open the description's: {opened:?}"
        );
        assert!(
            opened.iter().any(|row| row.contains("The comment's log.")),
            "enter did not open the comment's: {opened:?}"
        );

        told.said.insert(
            0,
            Comment {
                author: "someone else".to_string(),
                did: Did::Commented,
                when: Some(1),
                body: "A second look.".to_string(),
            },
        );
        page.told(told);
        let mut renamed = pull();
        renamed.title = "A title edited since".to_string();
        page.listed_as(About::PullRequest(renamed));
        let after = texts(&page);
        assert!(
            after.iter().any(|row| row.contains("The comment's log.")),
            "the comment's shut when another arrived: {after:?}"
        );
        assert!(
            after.iter().any(|row| row.contains("The log itself.")),
            "the description's shut when the title changed: {after:?}"
        );
    }

    /// Enter does what the row the cursor is on is for: copies the block of
    /// code, names the check that passed, shuts the comment.
    ///
    /// Broken deliberately by leaving `folds` off the checks' heading, so
    /// enter on it does nothing; and by leaving `fold` out of enter's arm,
    /// so the passed check is never named and the comment never shuts.
    #[test]
    fn enter_does_what_the_row_is_for() {
        let mut page = DiscussionView::new(About::PullRequest(pull()));
        page.told(said());
        let stand_on = |page: &mut DiscussionView, starting: &str| {
            let row = texts(page)
                .iter()
                .position(|row| row.contains(starting))
                .unwrap_or_else(|| panic!("no row says {starting:?}: {:?}", texts(page)));
            page.stand(Place { row, character: 0 });
        };

        stand_on(&mut page, "cargo test");
        assert_eq!(
            press(&mut page, KeyCode::Enter, KeyModifiers::NONE),
            Outcome::Copy("cargo test".to_string()),
            "enter on the block did not copy it"
        );

        stand_on(&mut page, "Checks");
        let _ = press(&mut page, KeyCode::Enter, KeyModifiers::NONE);
        assert!(
            texts(&page).iter().any(|row| row.trim() == "\u{2713} test"),
            "opening the checks did not name the one that passed: {:?}",
            texts(&page)
        );

        stand_on(&mut page, "reviewer");
        assert!(texts(&page).iter().any(|row| row.contains("Looks right")));
        let _ = press(&mut page, KeyCode::Enter, KeyModifiers::NONE);
        assert!(
            !texts(&page).iter().any(|row| row.contains("Looks right")),
            "the comment did not shut"
        );
    }

    /// A comment the reader shut stays shut when another arrives in front
    /// of it, and the one that arrived is open; and what was held in the
    /// comments is let go, because the places it named are other comments'.
    ///
    /// Broken deliberately two ways. Knowing every comment by one name
    /// (`whose` answering the same for all): shutting one shut the one that
    /// arrived. And not letting go in `told`: the hold stayed.
    #[test]
    fn a_comment_shut_stays_shut_when_another_arrives() {
        let mut page = DiscussionView::new(About::PullRequest(pull()));
        page.told(said());
        let heading = texts(&page)
            .iter()
            .position(|row| row.starts_with("reviewer"))
            .expect("the comment's heading");
        page.stand(Place {
            row: heading,
            character: 0,
        });
        let _ = press(&mut page, KeyCode::Enter, KeyModifiers::NONE);
        assert!(!texts(&page).iter().any(|row| row.contains("Looks right")));
        page.hold_from(crate::page::Spot {
            of: Part::Description,
            at: 0,
        });

        let mut newer = said();
        newer.said.insert(
            0,
            Comment {
                author: "someone else".to_string(),
                did: Did::Commented,
                when: Some(1),
                body: "A second look.".to_string(),
            },
        );
        page.told(newer);
        let now = texts(&page);
        assert!(
            !now.iter().any(|row| row.contains("Looks right")),
            "the comment the reader shut opened: {now:?}"
        );
        assert!(
            now.iter().any(|row| row.contains("A second look.")),
            "the comment that arrived is shut: {now:?}"
        );
        assert!(!page.holding(), "the hold stayed on comments that moved");
    }

    /// A key that is not the page's goes to the key table, and `alt+a` is
    /// the page's: it asks for the conversation.
    ///
    /// Broken deliberately by taking every key the page does not know as
    /// its own (`_ => Outcome::Consumed`): `ctrl+w` was swallowed and the
    /// page could not be closed. Handing control to the cursor is not a
    /// break -- `walk` answers nothing for `w`, and the key falls through
    /// the same.
    #[test]
    fn a_chord_falls_through_and_alt_a_asks_to_talk() {
        let mut page = DiscussionView::new(About::PullRequest(pull()));
        page.told(said());
        assert_eq!(
            press(&mut page, KeyCode::Char('w'), KeyModifiers::CONTROL),
            Outcome::Ignored
        );
        assert_eq!(
            press(&mut page, KeyCode::Char('a'), KeyModifiers::ALT),
            Outcome::Talk
        );
        assert_eq!(
            press(&mut page, KeyCode::Char('x'), KeyModifiers::NONE),
            Outcome::Ignored,
            "typing on the page went somewhere"
        );
    }

    /// An issue's page wears a mark of its own, not the pull request's.
    ///
    /// Broken deliberately by answering the pull request's mark for both:
    /// the issue's tab and status row were a pull request's.
    #[test]
    fn an_issue_wears_its_own_mark() {
        let pull = DiscussionView::new(About::PullRequest(pull()));
        let issue = DiscussionView::new(About::Issue(Issue {
            number: 3,
            title: "It breaks".to_string(),
            author: "someone".to_string(),
            labels: Vec::new(),
            updated: None,
            stamp: String::new(),
        }));
        assert_eq!(issue.icon(), obelus_icons::ui::ISSUE);
        assert_ne!(issue.icon(), pull.icon());
    }

    /// Shift and the arrows hold the words of the description, and what is
    /// held is what is on the screen.
    ///
    /// Broken deliberately by leaving `from` off the description's rows:
    /// nothing in them has a place in the words, so nothing is held.
    #[test]
    fn shift_holds_the_words_and_a_copy_takes_them() {
        let mut page = DiscussionView::new(About::Issue(Issue {
            number: 3,
            title: "It breaks".to_string(),
            author: "someone".to_string(),
            labels: Vec::new(),
            updated: None,
            stamp: String::new(),
        }));
        page.told(Discussion {
            body: "When I press it.".to_string(),
            ..said()
        });
        let rows = page.rows(ROOM.reading);
        let title = rows
            .iter()
            .position(|row| row.text().contains("It breaks"))
            .unwrap();
        page.stand(Place {
            row: title,
            character: 0,
        });
        let _ = press(&mut page, KeyCode::End, KeyModifiers::SHIFT);
        assert_eq!(page.held_text(ROOM.reading).as_deref(), Some("It breaks"));
        assert_eq!(
            press(&mut page, KeyCode::Esc, KeyModifiers::NONE),
            Outcome::Consumed
        );
        assert!(!page.holding(), "escape kept hold");
        assert_eq!(
            press(&mut page, KeyCode::Esc, KeyModifiers::NONE),
            Outcome::Ignored,
            "escape with nothing held was taken"
        );
    }
}
