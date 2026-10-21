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
//! **What enter does is decided by what the cursor is on, not by its row.**
//! On a link -- in the words, a check's name, a file's, the place a review's
//! thread names -- it follows the link: to the web, or to the file here.
//! Anywhere else it does what the row is for: a comment's heading opens and
//! shuts what it says, the checks' heading opens the checks that passed --
//! which nobody has to do anything about, so they are counted until the
//! reader asks for their names -- and a block of code is copied whole, as it
//! is in the transcript, because holding it from corner to corner takes its
//! box and the width's breaks with it. By the row, a heading with a link in
//! it said enter would open and shut it while the cursor stood on the link;
//! by what is under the cursor, both are a key away, and the row's end says
//! which. The transcript keeps its links the pointer's: there enter already
//! takes a message back, and a message that was only an address could not
//! have been.
//!
//! **A review's thread is a comment about its lines.** It goes among what
//! was said by when it was last answered, as GitHub's conversation does,
//! with its heading saying where it is and how it stands, the lines it was
//! said beside under that, and its answers after. One somebody resolved, or
//! whose lines have moved on, starts shut -- GitHub folds those away too.
//! The place is a link to the file at that line, by a click: enter on a
//! heading opens and shuts what is under it, here as on every comment.
//!
//! **Laid out once and kept.** The rows are made at a width and kept until
//! the width or what they are made of changes, because the keys, the caret
//! and the drawing all ask for them on every keypress. What the reader has
//! hold of is marked on them afterwards, so a drag does not lay the page out
//! again on every report the terminal sends.

use std::ops::Range;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
pub use obelus_github::{
    About, Asked, Changed, Check, Comment, Did, Discussion, Issue, PullRequest, Stands, Unlisted,
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
    /// What one comment says, by its place among them -- and which of its
    /// thread: `0` for the comment, and its replies after it, in order.
    Comment(usize, usize),
}

/// What a heading opens and shuts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fold {
    /// The files a pull request changes, by name.
    Files,
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
    /// Whether every file changed is named, or only how many.
    every_file: bool,
    /// The comments the reader turned from how they start -- open, or shut
    /// for a review's thread that is resolved or outdated -- by who said
    /// them and when.
    ///
    /// Not by their place: the newest comes first, so a comment arriving
    /// moves every place down one, and the one the reader shut became the
    /// one above it -- shut, while theirs stood open.
    comments_turned: std::collections::HashSet<(String, Option<i64>)>,
    /// The `<details>` the reader has turned from how they started: by the
    /// comment they are in -- known the way a turned comment is, for the
    /// same reason -- or none for the description, which of its thread, and
    /// where in it.
    turned: std::collections::HashSet<(Option<Whose>, usize, usize)>,
}

/// Whether a comment starts open: every one but a review's thread that
/// somebody resolved, or whose lines have moved on -- which is what GitHub
/// folds away too.
fn starts_open(comment: &Comment) -> bool {
    comment
        .on
        .as_ref()
        .is_none_or(|on| !on.resolved && !on.outdated)
}

/// What a `<details>` is known by while the reader has it turned: which
/// text it is in, and where -- or nothing where that text is not there.
fn which_details(of: Part, at: usize, said: &[Comment]) -> Option<(Option<Whose>, usize, usize)> {
    match of {
        Part::Description => Some((None, 0, at)),
        Part::Comment(index, reply) => said
            .get(index)
            .map(|comment| (Some(whose(comment)), reply, at)),
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
            Fold::Files => self.every_file,
            Fold::Comment(at) => said.get(at).is_none_or(|comment| {
                starts_open(comment) != self.comments_turned.contains(&whose(comment))
            }),
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
            Fold::Files => self.every_file = !self.every_file,
            Fold::Comment(at) => {
                let Some(comment) = said.get(at) else {
                    return;
                };
                let key = whose(comment);
                if !self.comments_turned.remove(&key) {
                    self.comments_turned.insert(key);
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

/// A file a row opens, as the pull request's last commit has it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opens {
    /// Its path, from the top of the repository.
    pub path: String,
    /// The line to stand on, counted from one, where the row is about one.
    pub line: Option<u64>,
    /// Whether enter opens it, or only a click on its underlined name: a
    /// thread's heading has enter for opening and shutting the thread.
    pub by_enter: bool,
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
    /// Where enter on it goes, on the web: a check's page.
    ///
    /// Only on a row that is one link and nothing else. A link in the
    /// words is opened by a click, as the transcript's are, because a row
    /// of words may hold several and enter cannot say which; a check's row
    /// is the check, and enter had nothing else to do on it.
    pub goes: Option<String>,
    /// The file enter on it opens, as the pull request's last commit has
    /// it: a row of the files it changes.
    pub opens: Option<Opens>,
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
        (here.folds.is_some()
            || here.goes.is_some()
            || here.opens.as_ref().is_some_and(|opens| opens.by_enter))
        .then(|| row..row + 1)
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
        rows.extend(files(said, folding, width));
        rows.push(rule.clone());
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

/// The files a pull request changes, as rows: how many and by how much on
/// the heading, and under it, once the reader opens it, each by name with
/// what it adds and takes away at the far end.
///
/// Shut until then: forty names would push what was said off the screen,
/// and the heading's count is what most readers come for.
fn files(said: &Discussion, folding: &Folding, width: u16) -> Vec<Row> {
    let heading = Span::new("Files  ", Ink::Heading(2));
    if said.changed.is_empty() && said.files == 0 {
        return vec![Row::said(vec![heading, Span::new("None", Ink::Aside)])];
    }
    let open = folding.is_open(Fold::Files, &[]);
    let mut rows = vec![Row {
        folds: (!said.changed.is_empty()).then_some(Fold::Files),
        open,
        ..Row::said(vec![
            heading,
            Span::new(format!("{} \u{b7} ", said.files), Ink::Aside),
            Span::new(format!("+{}", said.additions), Ink::Added),
            Span::new(" ", Ink::Aside),
            Span::new(format!("\u{2212}{}", said.deletions), Ink::Removed),
        ])
    }];
    if !open {
        return rows;
    }
    // Two blanks in front, as a check's name has its mark there; the names
    // in a column as wide as the widest, and the counts after it, so a
    // column of them reads down the page -- and short of the far end, which
    // is where the row the cursor is on says what enter does. Counts at the
    // far end left that no room, and a row that never said it could be
    // opened read as one that could not.
    let counted = |file: &Changed| format!("+{} \u{2212}{}", file.additions, file.deletions);
    let counts = said
        .changed
        .iter()
        .map(|file| obelus_text::text_width(&counted(file)))
        .max()
        .unwrap_or(0);
    let widest = said
        .changed
        .iter()
        .map(|file| obelus_text::text_width(&file.path))
        .max()
        .unwrap_or(0);
    let column = widest.min(
        usize::from(width)
            .saturating_sub(2 + 2 + counts + SAYS_WHAT_ENTER_DOES)
            .max(MIN_NAME),
    );
    for file in &said.changed {
        let added = format!("+{}", file.additions);
        let removed = format!("\u{2212}{}", file.deletions);
        let path = cut_from_the_left(&file.path, column);
        let gap = column.saturating_sub(obelus_text::text_width(&path)) + 2;
        // The name is a link, as a check's is -- underlined, and opened by a
        // click -- to the file rather than to the web: what it goes to is
        // the row's `opens`, and the pointer is told so.
        let name = 0..path.len();
        let mut row = Row {
            opens: Some(Opens {
                path: file.path.clone(),
                line: None,
                by_enter: true,
            }),
            ..Row::said(vec![
                Span::new("  ", Ink::Plain),
                Span::from_source(path, Ink::Plain, name.clone()),
                Span::new(" ".repeat(gap), Ink::Plain),
                Span::new(added, Ink::Added),
                Span::new(" ", Ink::Aside),
                Span::new(removed, Ink::Removed),
            ])
        };
        row.row.links.push((name, file.path.clone()));
        rows.push(row);
    }
    // `gh` names so many and no more, and a list that stopped without
    // saying so would say these were all.
    let more = usize::try_from(said.files)
        .unwrap_or(usize::MAX)
        .saturating_sub(said.changed.len());
    if more > 0 {
        rows.push(Row::said(vec![Span::new(
            format!("  And {more} more, on GitHub"),
            Ink::Aside,
        )]));
    }
    rows
}

/// How much of a file's row is kept clear at its far end, for the row the
/// cursor is on to say what enter does there.
const SAYS_WHAT_ENTER_DOES: usize = 20;

/// How few cells a file's name is cut to, however narrow the page: fewer,
/// and what is left is not a name.
const MIN_NAME: usize = 12;

/// A path cut to `room` cells by taking from its front, behind a mark that
/// says so: what tells a file from the others is its name, at the end.
fn cut_from_the_left(path: &str, room: usize) -> String {
    if obelus_text::text_width(path) <= room {
        return path.to_string();
    }
    let mut kept: Vec<char> = Vec::new();
    let mut used = 1;
    for character in path.chars().rev() {
        let wide = obelus_text::text_width(character.encode_utf8(&mut [0; 4]));
        if used + wide > room {
            break;
        }
        used += wide;
        kept.push(character);
    }
    std::iter::once('\u{2026}')
        .chain(kept.into_iter().rev())
        .collect()
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
            // The name is the link, where the check has a page: where it
            // failed is told there, and nowhere here.
            let name = 0..check.name.len();
            let mut row = Row::said(vec![
                Span::new(format!("  {mark} "), *ink),
                Span::from_source(check.name.clone(), Ink::Plain, name.clone()),
            ]);
            if let Some(link) = &check.link {
                row.row.links.push((name, link.clone()));
                row.goes = Some(link.clone());
            }
            rows.push(row);
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
    let ago = |when: Option<i64>| {
        when.map(|when| format!(" \u{b7} {}", obelus_git::how_long_ago(when, now)))
            .unwrap_or_default()
    };
    let mut rows = Vec::new();
    for (at, comment) in said.iter().enumerate() {
        if at > 0 {
            rows.push(Row::default());
        }
        let open = folding.is_open(Fold::Comment(at), said);
        let mut heading = Row {
            folds: Some(Fold::Comment(at)),
            open,
            ..Row::said(vec![Span::new(comment.author.clone(), Ink::Name)])
        };
        match &comment.on {
            None => heading.row.spans.push(Span::new(
                format!("  {}{}", what_it_did(comment.did), ago(comment.when)),
                Ink::Aside,
            )),
            // A thread on lines: where, and how it stands. The place is a
            // link to the file at that line -- by a click, because enter
            // on a heading opens and shuts what is under it -- except where
            // the lines have moved on and the number names other ones.
            Some(on) => {
                let linked = on.line.filter(|_| !on.outdated);
                let place = match linked {
                    Some(line) => format!("{}:{line}", on.path),
                    None => on.path.clone(),
                };
                let name = 0..place.len();
                heading.row.spans.push(Span::new("  on ", Ink::Aside));
                heading
                    .row
                    .spans
                    .push(Span::from_source(place, Ink::Plain, name.clone()));
                let mut after = ago(comment.when);
                match comment.replies.len() {
                    0 => {}
                    1 => after.push_str(" \u{b7} 1 reply"),
                    replies => after.push_str(&format!(" \u{b7} {replies} replies")),
                }
                if on.resolved {
                    after.push_str(" \u{b7} Resolved");
                } else if on.outdated {
                    after.push_str(" \u{b7} Outdated");
                }
                heading.row.spans.push(Span::new(after, Ink::Aside));
                if let Some(line) = linked {
                    heading.row.links.push((name, on.path.clone()));
                    heading.opens = Some(Opens {
                        path: on.path.clone(),
                        line: Some(line),
                        by_enter: false,
                    });
                }
            }
        }
        rows.push(heading);
        if !open {
            continue;
        }
        if let Some(on) = comment.on.as_ref().filter(|on| !on.hunk.is_empty()) {
            rows.extend(hunk(&on.hunk, width));
        }
        rows.extend(written(
            &comment.body,
            Part::Comment(at, 0),
            said,
            folding,
            width,
        ));
        for (reply, said_after) in comment.replies.iter().enumerate() {
            rows.push(Row::default());
            rows.push(Row::said(vec![
                Span::new(said_after.author.clone(), Ink::Name),
                Span::new(ago(said_after.when), Ink::Aside),
            ]));
            rows.extend(written(
                &said_after.body,
                Part::Comment(at, reply + 1),
                said,
                folding,
                width,
            ));
        }
    }
    rows
}

/// What one comment, or one reply in a thread, wrote: its markdown laid
/// out, with its `<details>` as the reader has them.
fn written(body: &str, of: Part, said: &[Comment], folding: &Folding, width: u16) -> Vec<Row> {
    let laid = obelus_markdown::render_folding(body, width, &|found, asked_open| {
        folding.is_open(
            Fold::Details {
                of,
                at: found,
                asked_open,
            },
            said,
        )
    });
    laid.into_iter()
        .map(|row| Row {
            folds: row.details.map(|details| Fold::Details {
                of,
                at: details.at,
                asked_open: details.asked_open,
            }),
            open: row.details.is_some_and(|details| details.open),
            row,
            from: Some(of),
            ..Row::default()
        })
        .collect()
}

/// The lines a thread was said beside, as a block of code -- a copy takes
/// them, as it takes any -- with what the change added and took away in
/// the colours the margin marks them in.
///
/// Coloured here because there is no grammar for a diff to colour it: a
/// line's first character is the whole of what says which it is.
fn hunk(lines: &str, width: u16) -> Vec<Row> {
    obelus_markdown::render(&format!("```\n{lines}\n```\n"), width)
        .into_iter()
        .map(|mut row| {
            let said = row
                .spans
                .iter()
                .filter(|span| span.ink != Ink::Mark)
                .find_map(|span| span.text.chars().next());
            let ink = match said {
                Some('+') => Some(Ink::Added),
                Some('-') => Some(Ink::Removed),
                _ => None,
            };
            if let Some(ink) = ink {
                for span in row.spans.iter_mut().filter(|span| span.ink != Ink::Mark) {
                    span.ink = ink;
                }
            }
            Row {
                row,
                ..Row::default()
            }
        })
        .collect()
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
    /// Open this address, in whatever the machine opens the web in.
    Open(String),
    /// Open this file as the pull request's last commit has it, by its path
    /// from the top of the repository -- on a line, counted from one, where
    /// what was followed was about one.
    OpenFile {
        /// The file.
        path: String,
        /// The line.
        line: Option<u64>,
    },
}

/// Where a link enter would follow goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Follows {
    /// A file, as the pull request's last commit has it.
    File(Opens),
    /// An address on the web.
    Web(String),
}

/// The link under `at`, where there is one: what enter there follows, and
/// what the row's end says it does -- one answer, so the two cannot differ.
#[must_use]
pub fn followed_at(rows: &[Row], at: Place) -> Option<Follows> {
    let row = rows.get(at.row)?;
    let (_, to) = row.row.links.iter().find(|(bytes, _)| {
        crate::page::row_held(&row.row.spans, bytes.start, bytes.end)
            .is_some_and(|characters| characters.contains(&at.character))
    })?;
    // A link the row opens as a file is the file's name; any other goes to
    // the web.
    Some(match row.opens.as_ref().filter(|opens| opens.path == *to) {
        Some(opens) => Follows::File(opens.clone()),
        None => Follows::Web(to.clone()),
    })
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
    /// Where the pointer went down, while it has not moved since: what
    /// tells a press on a link from a drag that ends on one, on a row that
    /// is none of the page's words and so has nothing to hold.
    pressed: Option<Place>,
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
            pressed: None,
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

    /// Puts the cursor somewhere and its row on screen, letting go of what
    /// was held: a search of the page, chosen.
    pub fn stand_in_view(&mut self, at: Place, room: crate::page::Room) {
        let count = self.laid_out(room.reading).len();
        self.held.let_go();
        self.cursor = at;
        self.window.set_count(count);
        crate::page::show(&mut self.window, at.row, count, room.height);
    }

    /// Puts a row on screen and leaves the cursor where it is: a search of
    /// the page, looked at from a list over it.
    pub fn look_at_row(&mut self, row: usize, room: crate::page::Room) {
        let count = self.laid_out(room.reading).len();
        self.window.set_count(count);
        crate::page::show(&mut self.window, row, count, room.height);
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

    /// Says the pointer went down here.
    pub const fn press_at(&mut self, at: Place) {
        self.pressed = Some(at);
    }

    /// Says the pointer went over here with the button down: anywhere but
    /// where it went down, and it was not a click.
    pub fn drag_over(&mut self, at: Place) {
        if self.pressed != Some(at) {
            self.pressed = None;
        }
    }

    /// Whether letting go here ends a click: the pointer went down here and
    /// has not been anywhere else since.
    #[must_use]
    pub fn pressed_here(&self, at: Place) -> bool {
        self.pressed == Some(at)
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
                match followed_at(&rows, self.cursor) {
                    Some(Follows::File(opens)) => {
                        return Outcome::OpenFile {
                            path: opens.path,
                            line: opens.line,
                        };
                    }
                    Some(Follows::Web(to)) => return Outcome::Open(to),
                    None => {}
                }
                let Some(on) = <Row as crate::page::Line>::acting(&rows, self.cursor.row) else {
                    return Outcome::Consumed;
                };
                let Some(first) = rows.get(on.start) else {
                    return Outcome::Consumed;
                };
                if let Some(code) = &first.row.code {
                    return Outcome::Copy(code.text.to_string());
                }
                if let Some(to) = &first.goes {
                    return Outcome::Open(to.clone());
                }
                if let Some(opens) = first.opens.as_ref().filter(|opens| opens.by_enter) {
                    return Outcome::OpenFile {
                        path: opens.path.clone(),
                        line: opens.line,
                    };
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
            link: None,
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
            changed: vec![Changed {
                path: "src/lib.rs".to_string(),
                additions: 3,
                deletions: 1,
            }],
            checks: vec![
                check("clippy", Stands::Failed),
                check("test", Stands::Passed),
            ],
            said: vec![Comment {
                author: "reviewer".to_string(),
                did: Did::Commented,
                when: None,
                body: "Looks right to me.".to_string(),
                on: None,
                replies: Vec::new(),
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

    /// Enter follows the link the cursor is on, and does what the row is
    /// for anywhere else on it: on a thread's heading, the file at its line
    /// or the thread shut; in the description, the address or nothing.
    ///
    /// Broken deliberately twice. Asking only the row, as this first did:
    /// enter on the place in the heading shut the thread. And taking the
    /// file's line off what is followed: the file opened on no line.
    #[test]
    fn enter_follows_the_link_the_cursor_is_on() {
        let mut told = said();
        told.body = "See [the docs](https://example.com/docs) first.".to_string();
        told.said = vec![Comment {
            author: "bob".to_string(),
            did: Did::Commented,
            when: Some(2),
            body: "Why?".to_string(),
            on: Some(obelus_github::On {
                id: "T".to_string(),
                path: "src/lib.rs".to_string(),
                line: Some(12),
                hunk: String::new(),
                resolved: false,
                outdated: false,
            }),
            replies: Vec::new(),
        }];
        let mut page = DiscussionView::new(About::PullRequest(pull()));
        page.told(told);
        let text = texts(&page);
        let on = |page: &mut DiscussionView, words: &str, into: usize| {
            let rows = texts(page);
            let row = rows
                .iter()
                .position(|row| row.contains(words))
                .unwrap_or_else(|| panic!("no {words:?}: {rows:?}"));
            let character = rows[row]
                .chars()
                .collect::<Vec<_>>()
                .windows(words.chars().count())
                .position(|window| window.iter().collect::<String>() == words)
                .expect("the words");
            page.stand(Place {
                row,
                character: character + into,
            });
            press(page, KeyCode::Enter, KeyModifiers::NONE)
        };
        assert_eq!(
            on(&mut page, "src/lib.rs:12", 2),
            Outcome::OpenFile {
                path: "src/lib.rs".to_string(),
                line: Some(12),
            },
            "{text:?}"
        );
        assert_eq!(
            on(&mut page, "the docs", 1),
            Outcome::Open("https://example.com/docs".to_string())
        );
        assert_eq!(on(&mut page, "See", 0), Outcome::Consumed);
        assert_eq!(on(&mut page, "bob", 0), Outcome::Consumed);
        assert!(
            !texts(&page).iter().any(|row| row.contains("Why?")),
            "enter beside the place did not shut the thread"
        );
    }

    /// A review's thread is a comment whose heading says where it is, how
    /// many answered and how it stands; open, the lines it is about are a
    /// block coloured as the change was, and the answers follow. A resolved
    /// one starts shut. Enter on the heading opens and shuts it, and the
    /// place is a link a click follows -- not enter.
    ///
    /// Broken deliberately five ways. Leaving the replies out: the answer
    /// was not on the page. Colouring the block by nothing: the line added
    /// was drawn as plain code. Letting a resolved thread start open: what
    /// it said was on the page. Opening by enter: enter on the heading
    /// opened the file rather than shutting the thread. And leaving the
    /// link off: the place was not underlined, and a click did nothing.
    #[test]
    fn a_review_thread_is_a_comment_about_its_lines() {
        let on = |resolved: bool| {
            Some(obelus_github::On {
                id: "T".to_string(),
                path: "src/lib.rs".to_string(),
                line: Some(12),
                hunk: " a\n-b\n+c".to_string(),
                resolved,
                outdated: false,
            })
        };
        let mut told = said();
        told.said = vec![
            Comment {
                author: "bob".to_string(),
                did: Did::Commented,
                when: Some(2),
                body: "Why?".to_string(),
                on: on(false),
                replies: vec![Comment {
                    author: "alice".to_string(),
                    did: Did::Commented,
                    when: Some(3),
                    body: "Because.".to_string(),
                    on: None,
                    replies: Vec::new(),
                }],
            },
            Comment {
                author: "carol".to_string(),
                did: Did::Commented,
                when: Some(1),
                body: "Fixed?".to_string(),
                on: on(true),
                replies: Vec::new(),
            },
        ];
        let mut page = DiscussionView::new(About::PullRequest(pull()));
        page.told(told);
        let rows = page.rows(ROOM.reading);
        let text: Vec<String> = rows.iter().map(Row::text).collect();
        let heading = text
            .iter()
            .position(|row| row.starts_with("bob  on src/lib.rs:12"))
            .unwrap_or_else(|| panic!("no thread: {text:?}"));
        assert!(text[heading].contains("1 reply"), "{:?}", text[heading]);
        assert_eq!(
            rows[heading].row.links.first().map(|(_, to)| to.as_str()),
            Some("src/lib.rs"),
            "the place is not a link"
        );
        let ink_of = |words: &str| {
            rows.iter()
                .find(|row| row.text().contains(words))
                .and_then(|row| {
                    row.row
                        .spans
                        .iter()
                        .find(|span| span.text.contains(words))
                        .map(|span| span.ink)
                })
        };
        assert_eq!(ink_of("+c"), Some(Ink::Added), "{text:?}");
        assert_eq!(ink_of("-b"), Some(Ink::Removed), "{text:?}");
        assert!(text.iter().any(|row| row.contains("Why?")), "{text:?}");
        assert!(
            text.iter().any(|row| row.starts_with("alice"))
                && text.iter().any(|row| row.contains("Because.")),
            "the answer is not under it: {text:?}"
        );
        assert!(
            text.iter()
                .any(|row| row.starts_with("carol") && row.contains("Resolved")),
            "{text:?}"
        );
        assert!(
            !text.iter().any(|row| row.contains("Fixed?")),
            "a resolved thread started open: {text:?}"
        );

        page.stand(Place {
            row: heading,
            character: 0,
        });
        assert_eq!(
            press(&mut page, KeyCode::Enter, KeyModifiers::NONE),
            Outcome::Consumed,
            "enter on the heading did something else than shut it"
        );
        assert!(
            !texts(&page).iter().any(|row| row.contains("Because.")),
            "enter did not shut the thread"
        );
    }

    /// The files a pull request changes are a heading that starts shut and
    /// counts them; opened, each is a row with its counts at the far end, a
    /// long path cut from the front, and enter on one opens it -- and a list
    /// `gh` cut short says how many it left out.
    ///
    /// Broken deliberately five ways. Leaving `opens` off a file's row:
    /// enter there did nothing. Cutting a long path from its end: the file's
    /// name went. Laying the counts against each name: they were not one
    /// column. Laying them at the far end, as this first did: the row had no
    /// room left to say what enter does, and read as one it did nothing on.
    /// And not saying what was left out: three files counted and two named,
    /// with nothing between.
    #[test]
    fn the_files_open_from_their_heading_and_each_opens() {
        let mut page = DiscussionView::new(About::PullRequest(pull()));
        let mut told = said();
        told.files = 3;
        told.changed = vec![
            Changed {
                path: "src/lib.rs".to_string(),
                additions: 12,
                deletions: 3,
            },
            Changed {
                path: "crates/a-crate-with-a-long-name/src/somewhere/deep/inside/the_file.rs"
                    .to_string(),
                additions: 1,
                deletions: 0,
            },
        ];
        page.told(told);
        let shut = texts(&page);
        let heading = shut
            .iter()
            .position(|row| row.starts_with("Files"))
            .unwrap_or_else(|| panic!("no files: {shut:?}"));
        assert!(
            !shut.iter().any(|row| row.contains("src/lib.rs")),
            "the files started open: {shut:?}"
        );
        page.stand(Place {
            row: heading,
            character: 0,
        });
        let _ = press(&mut page, KeyCode::Enter, KeyModifiers::NONE);
        let open = texts(&page);
        let first = &open[heading + 1];
        assert!(
            first.starts_with("  src/lib.rs") && first.ends_with("+12 \u{2212}3"),
            "{first:?}"
        );
        let long = &open[heading + 2];
        let column = |row: &str| row.chars().position(|character| character == '+');
        assert_eq!(
            column(first),
            column(long),
            "the counts are not in one column: {first:?} {long:?}"
        );
        assert!(
            first.chars().count() + SAYS_WHAT_ENTER_DOES <= usize::from(ROOM.reading),
            "no room at the end for what enter does: {first:?}"
        );
        assert!(
            long.contains("\u{2026}") && long.contains("the_file.rs"),
            "the long path lost its name: {long:?}"
        );
        assert!(
            open[heading + 3].contains("And 1 more"),
            "what was left out is not said: {open:?}"
        );

        page.stand(Place {
            row: heading + 1,
            character: 0,
        });
        assert_eq!(
            press(&mut page, KeyCode::Enter, KeyModifiers::NONE),
            Outcome::OpenFile {
                path: "src/lib.rs".to_string(),
                line: None,
            }
        );
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
                on: None,
                replies: Vec::new(),
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
                on: None,
                replies: Vec::new(),
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
