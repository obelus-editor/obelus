//! The repository's open pull requests and issues, and the review of one or
//! the answer to the other.
//!
//! **Two tabs of one list, because they are one errand.** Both are what is
//! on its way into the history, asked of `gh` the same way, kept the same
//! way and drawn by the same rows and the same preview; what differs is
//! what the agent is asked to do with one, which is the opening's.
//!
//! **Asked of `gh`, not of GitHub.** Signing in is `gh`'s, and so is knowing
//! which repository on GitHub this checkout is -- a remote can be named
//! anything, point at a fork, or be one of three. Asking the API directly
//! would mean a token Obelus keeps and a guess at which remote is the one;
//! `gh` already has both answers, and the workflow a project chooses already
//! leans on it for pull requests. Where it is missing or signed out, the
//! list says which, because the reader can act on either.
//!
//! **A page when the reader gets to the end of the last one.** A repository
//! with fourteen hundred open pull requests and ten thousand issues is
//! minutes of walking and megabytes of somebody's data for a list read from
//! the top, so the newest hundred are asked for, and the next hundred once
//! the last row is on screen. Opened again, the list asks only what has
//! changed since: what is still open goes to the top, and what has closed
//! goes. What a reader types is matched here first, as every list matches,
//! and asked of GitHub's search once they stop typing, for the rows not yet
//! fetched -- whose answers are matched here too, so a row is in the list
//! for the same reason whichever way it came. A row carries only what a row
//! draws: what a preview wants is asked for the row the reader is on, and
//! kept until the list says that row has changed.
//!
//! **Choosing one opens the review, and says nothing yet.** The conversation
//! is a note's shape -- claimed by the pull request's number before there is
//! a session, and found again by it -- and the box offers the words that
//! start the review, the way a note's offers `Look into this`: a reader who
//! wants it looked at for one thing in particular says so before anything
//! is read.
//!
//! **What goes to GitHub goes on the reader's word.** The opening asks the
//! agent for the review on the page first and a card before it is sent, and
//! sending is the agent running `gh pr review`, which its own permission
//! asks about. Obelus sends nothing itself.

use obelus_command::Command;
use obelus_component::picker::{
    Marking, Picker, PickerItem, PickerLayout, PickerValue, Remark, Said,
};
pub use obelus_github::{
    Asked, Check, Comment, Decision, Did, Discussion, Issue, Listed, PAGE, Page, PullRequest,
    SEARCH_GIVES, Stands, Toward, Unlisted,
};

use super::*;
use crate::event::Event;

/// How long the reader's typing has to stop for before GitHub is asked
/// about it: a question a keystroke would be a question about half a word,
/// and GitHub matches whole ones.
const SETTLES_AFTER: std::time::Duration = std::time::Duration::from_millis(300);

/// The list's tabs, in the order they sit in.
const TABS: [&str; 2] = ["Pull requests", "Issues"];

/// A row of a list: its number, and when it last changed.
trait Listable {
    /// The number GitHub gave it.
    fn number(&self) -> u64;
    /// When it last changed, as seconds since the epoch.
    fn updated(&self) -> Option<i64>;
    /// And as GitHub says it.
    fn stamp(&self) -> &str;
}

impl Listable for PullRequest {
    fn number(&self) -> u64 {
        self.number
    }
    fn updated(&self) -> Option<i64> {
        self.updated
    }
    fn stamp(&self) -> &str {
        &self.stamp
    }
}

impl Listable for Issue {
    fn number(&self) -> u64 {
        self.number
    }
    fn updated(&self) -> Option<i64> {
        self.updated
    }
    fn stamp(&self) -> &str {
        &self.stamp
    }
}

/// What GitHub's search found for the query in the box.
#[derive(Debug)]
struct Search {
    /// The query, as it was asked.
    query: String,
    /// Which asking of it this is.
    which: u64,
    /// Whether a page of it is on its way.
    asking: bool,
    /// Whether any of it has come.
    answered: bool,
    /// Which rows it has brought: a first page landing after them keeps
    /// them, because nothing will ask GitHub for them again.
    found: std::collections::HashSet<u64>,
    /// How many GitHub says match.
    total: u64,
    /// Where its next page starts, while there is one.
    older: Option<String>,
    /// Why the last page asked for did not come.
    stalled: Option<Unlisted>,
}

impl Search {
    /// The asking `which` of `query`, before anything has been asked.
    fn new(query: String, which: u64) -> Self {
        Self {
            query,
            which,
            asking: false,
            answered: false,
            found: std::collections::HashSet::new(),
            total: 0,
            older: None,
            stalled: None,
        }
    }
}

/// One of the list's tabs, as `gh` last answered it.
#[derive(Debug)]
struct Listing<T> {
    /// What `gh` last said, kept after the list closes: an opening is made
    /// from it, and so is whether the thing has moved since.
    listed: Vec<T>,
    /// Whether `listed` is an answer at all: none of it before `gh` has
    /// first answered, and none of it after an answer that was a refusal.
    answered: bool,
    /// What is on its way from the top -- the first page, or what has
    /// changed. Apart from `paging`, so that an opening while a page is on
    /// its way still asks what has changed.
    asking: Option<Toward>,
    /// Whether the next older page is on its way.
    paging: bool,
    /// Why the first page was no list, where it was.
    unlisted: Option<Unlisted>,
    /// Where the next older page starts, while GitHub has one.
    older: Option<String>,
    /// How many are open, as GitHub last counted.
    total: Option<u64>,
    /// The newest `updatedAt` a walk from the top has seen, which the next
    /// opening asks what has changed since. Not the newest row's: a search
    /// can bring one in from above a stretch nobody has asked about since.
    newest: String,
    /// Why the last older page did not come, with what came before it
    /// kept: those rows are still true.
    stalled: Option<Unlisted>,
    /// Why what has changed could not be asked: the rows are what they
    /// were, and may no longer be what is open. The next opening asks
    /// again.
    stale: Option<Unlisted>,
    /// What GitHub's search found for the query in the box.
    search: Option<Search>,
}

impl<T> Default for Listing<T> {
    fn default() -> Self {
        Self {
            listed: Vec::new(),
            answered: false,
            asking: None,
            paging: false,
            unlisted: None,
            older: None,
            total: None,
            newest: String::new(),
            stalled: None,
            stale: None,
            search: None,
        }
    }
}

/// What a tab says about itself beside its rows.
#[derive(Debug, PartialEq, Eq)]
struct Saying {
    /// How much of it there is, or what is being done about the rest.
    tally: Option<obelus_component::picker::Tally>,
    /// Whether something is on its way, which turns the mark.
    turning: bool,
    /// What it says with no rows, and whether that is so whatever is typed.
    empty: (String, bool),
    /// Whether there is more of it than it has.
    unfinished: bool,
}

impl<T: Listable> Listing<T> {
    /// Which way to ask as the list opens: what has changed since it was
    /// last answered, or its first page where it never has been -- and
    /// nothing while either is already on its way.
    fn opening(&self) -> Option<Toward> {
        match (&self.asking, self.answered && !self.newest.is_empty()) {
            (Some(_), _) => None,
            (None, true) => Some(Toward::Newer(self.newest.clone())),
            (None, false) => Some(Toward::First),
        }
    }

    /// The search for `query`, where there is one.
    fn search_for(&self, query: &str) -> Option<&Search> {
        self.search.as_ref().filter(|search| search.query == query)
    }

    /// Whether a query typed here is worth asking GitHub's search about:
    /// not where the list itself was refused, for the same reason.
    fn searchable(&self) -> bool {
        self.answered || self.unlisted.is_none()
    }

    /// Puts `rows` in with what is listed, once each and newest first.
    ///
    /// Where both have a row, the one that changed later is kept: a page
    /// and a search can both bring one, and the older copy's `updatedAt`
    /// is what a preview kept about it would be checked against.
    fn merge(&mut self, rows: Vec<T>) {
        let mut at: std::collections::HashMap<u64, usize> = self
            .listed
            .iter()
            .enumerate()
            .map(|(at, row)| (row.number(), at))
            .collect();
        for row in rows {
            match at.get(&row.number()) {
                Some(&was) if row.updated() > self.listed[was].updated() => self.listed[was] = row,
                Some(_) => {}
                None => {
                    at.insert(row.number(), self.listed.len());
                    self.listed.push(row);
                }
            }
        }
        self.listed
            .sort_by_key(|row| std::cmp::Reverse(row.updated()));
    }

    /// Takes what `gh` said.
    fn answer(&mut self, answer: Listed<T>) {
        match answer {
            Listed::First(page) => {
                self.asking = None;
                // Nothing of a last answer -- it is only asked for where
                // there was none -- but what a search found while it was on
                // its way stays: nothing will ask for those again.
                let found = self.search.as_ref().map(|search| &search.found);
                self.listed
                    .retain(|row| found.is_some_and(|found| found.contains(&row.number())));
                self.newest = page
                    .rows
                    .iter()
                    .map(|row| row.stamp().to_string())
                    .max()
                    .unwrap_or_default();
                self.merge(page.rows);
                self.older = page.older;
                self.total = Some(page.total);
                self.answered = true;
                self.unlisted = None;
                self.stalled = None;
                self.stale = None;
            }
            Listed::Older(page) => {
                self.paging = false;
                self.merge(page.rows);
                self.older = page.older;
                self.total = Some(page.total);
                self.stalled = None;
            }
            Listed::Changed {
                open,
                gone,
                newest,
                total,
            } => {
                self.asking = None;
                // What is open goes where it now sorts, rather than staying
                // where it was: it is newer than it was, which is the thing
                // the list is sorted by.
                let moved: std::collections::HashSet<u64> = open
                    .iter()
                    .map(Listable::number)
                    .chain(gone.iter().copied())
                    .collect();
                self.listed.retain(|row| !moved.contains(&row.number()));
                self.merge(open);
                self.newest = self.newest.clone().max(newest);
                self.total = Some(total);
                self.stale = None;
            }
            Listed::Found {
                query,
                asking,
                page,
            } => {
                let Some(search) = self
                    .search
                    .as_mut()
                    .filter(|search| search.query == query && search.which == asking)
                else {
                    // A query the reader has typed past, or an asking of it
                    // from before they typed it again.
                    return;
                };
                search.asking = false;
                search.answered = true;
                search.found.extend(page.rows.iter().map(Listable::number));
                search.older = page.older;
                search.total = page.total;
                search.stalled = None;
                self.merge(page.rows);
            }
            Listed::Refused { toward, why } => {
                tracing::info!(?why, ?toward, "no list from gh");
                match toward {
                    Toward::First => {
                        self.asking = None;
                        // And none of the last one: rows from an answer
                        // before this one would be a list saying what was
                        // open then.
                        self.listed.clear();
                        self.answered = false;
                        self.unlisted = Some(why);
                    }
                    Toward::Older(_) => {
                        self.paging = false;
                        self.stalled = Some(why);
                    }
                    Toward::Newer(_) => {
                        self.asking = None;
                        self.stale = Some(why);
                    }
                    Toward::Found { query, asking, .. } => {
                        if let Some(search) = self
                            .search
                            .as_mut()
                            .filter(|search| search.query == query && search.which == asking)
                        {
                            search.asking = false;
                            search.stalled = Some(why);
                        }
                    }
                }
            }
        }
    }

    /// Writes down that `toward` has been asked for.
    fn asked(&mut self, toward: &Toward) {
        match toward {
            Toward::First | Toward::Newer(_) => self.asking = Some(toward.clone()),
            Toward::Older(_) => self.paging = true,
            Toward::Found { query, asking, .. } => {
                if let Some(search) = self
                    .search
                    .as_mut()
                    .filter(|search| &search.query == query && search.which == *asking)
                {
                    search.asking = true;
                }
            }
        }
    }

    /// Lets what did not come for `query` be asked for again.
    fn try_again(&mut self, query: &str) {
        match query.is_empty() {
            true => self.stalled = None,
            false => {
                if let Some(search) = self.search.as_mut().filter(|search| search.query == query) {
                    search.stalled = None;
                }
            }
        }
    }

    /// Whether what did not come for `query` is waiting on the reader to
    /// say try again.
    fn stuck(&self, query: &str) -> bool {
        match query.is_empty() {
            true => self.stalled.is_some(),
            false => self
                .search_for(query)
                .is_some_and(|search| search.stalled.is_some()),
        }
    }

    /// Which way to ask now that the last row is on screen, with `query`
    /// in the box: the next page of the list, or of what GitHub's search
    /// found for it -- its first, where that did not come -- and nothing
    /// while one is on its way, or after one that did not come, until the
    /// reader says to try again.
    fn further(&self, query: &str) -> Option<Toward> {
        if query.is_empty() {
            let ready = self.answered && !self.paging && self.stalled.is_none();
            return self.older.clone().filter(|_| ready).map(Toward::Older);
        }
        let search = self.search_for(query)?;
        if search.asking || search.stalled.is_some() {
            return None;
        }
        let after = match search.answered {
            true => Some(search.older.clone()?),
            false => None,
        };
        Some(Toward::Found {
            query: query.to_string(),
            asking: search.which,
            after,
        })
    }

    /// What the tab says about itself, with `query` in the box, the
    /// reader's typing `settling` before GitHub is asked about it, and
    /// `matched` rows of the list matching it; `none` is what it says when
    /// nothing is open.
    ///
    /// What a search found is counted by the rows it left in the list, not
    /// by GitHub's count: GitHub matches the body and the comments as well,
    /// and a number at the foot that is not the number of rows above it is
    /// a number the reader cannot check.
    fn says(&self, query: &str, settling: bool, matched: usize, none: &str) -> Saying {
        use obelus_component::picker::Tally;

        let words = |words: String| Some(Tally { words, key: None });
        let stuck = |why: &Unlisted| {
            Some(Tally {
                words: unlisted(why),
                key: Some(("\u{2193}".to_string(), "Try again".to_string())),
            })
        };
        let search = self.search_for(query);
        // Settling only where this tab has not asked about the query yet: a
        // tab walked back onto has, and nothing more will be asked.
        let searching = !query.is_empty()
            && self.searchable()
            && search.map_or(settling, |search| search.asking);
        let matching = match matched {
            1 => "1 matches".to_string(),
            matched => format!("{matched} match"),
        };
        let tally = match (query.is_empty(), search) {
            (false, _) if !self.searchable() => None,
            (false, _) if searching => words(format!("Asking GitHub about \"{query}\"")),
            (false, Some(search)) => match (&search.stalled, &search.older) {
                (Some(why), _) => stuck(why),
                (None, _) if !search.answered => None,
                (None, Some(_)) => words(format!("{matching} so far")),
                // GitHub's search stops at a thousand, whatever it counts.
                (None, None)
                    if search.found.len() >= SEARCH_GIVES
                        && search.total > search.found.len() as u64 =>
                {
                    words(format!("{matching} \u{b7} GitHub gives no more"))
                }
                (None, None) => words(matching),
            },
            (false, None) => None,
            (true, _) => match (&self.asking, self.paging, &self.stalled, &self.stale) {
                (_, true, ..) => words(format!("Asking GitHub for {PAGE} more")),
                (Some(Toward::Newer(_)), ..) => words("Asking GitHub what has changed".to_string()),
                (Some(_), ..) => None,
                (None, false, Some(why), _) => stuck(why),
                _ if !self.answered => None,
                // Nothing to press: the rows are what they were, and the
                // next opening asks again.
                (None, false, None, Some(why)) => {
                    words(format!("May be out of date \u{b7} {}", unlisted(why)))
                }
                (None, false, None, None) => {
                    let listed = self.listed.len();
                    match (self.older.is_some(), self.total) {
                        (true, Some(total)) => words(format!("{listed} of {total} open")),
                        _ => words(format!("{listed} open")),
                    }
                }
            },
        };
        // The waiting and the refusal are facts about the world, and "No
        // match" would be a fact about a query nothing was asked of. With a
        // list, and GitHub done with the query, a query that matches none of
        // it is the query's to say.
        let unasked = search
            .filter(|search| !search.answered)
            .and_then(|search| search.stalled.as_ref());
        let empty = match (&self.asking, &self.unlisted, unasked) {
            (Some(Toward::First), ..) => ("Still asking GitHub".to_string(), true),
            (_, Some(why), _) if !self.answered => (unlisted(why), true),
            (_, _, Some(why)) if !query.is_empty() => (unlisted(why), true),
            _ if searching => ("Still asking GitHub".to_string(), true),
            _ => (none.to_string(), false),
        };
        let unfinished = match query.is_empty() {
            true => self.older.is_some(),
            false => search.is_some_and(|search| search.older.is_some()),
        };
        Saying {
            tally,
            turning: self.asking.is_some() || self.paging || searching,
            empty,
            unfinished,
        }
    }
}

/// What this window knows about the repository's pull requests and issues.
#[derive(Debug, Default)]
pub(super) struct Pulls {
    /// The pull requests.
    pulls: Listing<PullRequest>,
    /// The issues.
    issues: Listing<Issue>,
    /// What each pull request or issue says beyond its row, as `gh` last
    /// said, by its number -- which GitHub shares between the two, so one
    /// key is one thing. Kept for the window's life the way the list is:
    /// shown at once the next time the row is chosen, and asked about again
    /// only where the list says it has changed since (`App::kept_is_current`).
    discussions: std::collections::HashMap<u64, Discussion>,
    /// Which have been asked about since the list opened, so that walking
    /// back onto a row does not ask again -- a check still running is asked
    /// about once an opening, not once a frame.
    asked_since_opening: std::collections::HashSet<u64>,
    /// Which one's discussion is on its way. One at a time: a reader
    /// walking the list passes rows they will not stop on.
    asking_about: Option<u64>,
    /// Why the last asking about one got nothing, by its number, for a
    /// preview with nothing kept to show instead.
    refused: std::collections::HashMap<u64, Unlisted>,
    /// What to run in `gh`'s place, and what to tell it first, for a test:
    /// one that ran the real `gh` would be asking GitHub about whatever
    /// directory it ran in.
    instead: Option<(std::path::PathBuf, Vec<String>)>,
    /// The clock on the reader's typing, while it runs: GitHub is asked
    /// about the query once it has stopped moving.
    settling: Option<crate::event::Pause>,
    /// How many searches this window has asked, which numbers each.
    searches: u64,
}

impl App {
    /// Opens the list of open pull requests and asks `gh` for them.
    ///
    /// With what `gh` said the last time this window asked, at once, and
    /// the new answer put in under the reader when it lands: asking takes
    /// seconds, and the pull requests open an hour ago are nearly always the
    /// ones open now. Put in by [`Picker::renew`], which keeps the reader on
    /// the pull request they were on -- rows moving under a selection that
    /// stayed put would be a row they did not choose. Only this window's:
    /// the first time a window asks, it waits.
    pub fn review_a_pull_request(&mut self) {
        let mut picker = Picker::new(Vec::new(), PickerLayout::FullArea);
        picker.before_typing("Filter by title or number");
        // Newest first whatever is typed: a query says which are left, and
        // a ranking would put a pull request from last year above this
        // morning's because its title scored better.
        picker.keeps_order(true);
        picker.opened_by(Command::PullRequestReview);
        // Scopes rather than groups: each tab's rows are its own answer
        // from `gh`, put in when the reader walks onto it.
        picker.with_scopes(&TABS);
        // What the selection says about itself, under the list: a title is
        // a line, and which to take up is decided by the rest.
        picker.previews();
        self.show_list(picker);
        // Both asked at once, so that walking onto the other tab finds its
        // answer already there. A page that did not come is worth trying
        // again on a new opening, and a query from the last one is gone
        // with the box it was typed in.
        self.pulls.settling = None;
        self.pulls.pulls.stalled = None;
        self.pulls.pulls.search = None;
        self.pulls.issues.stalled = None;
        self.pulls.issues.search = None;
        if let Some(toward) = self.pulls.pulls.opening() {
            self.ask_for_the_list(false, toward);
        }
        if let Some(toward) = self.pulls.issues.opening() {
            self.ask_for_the_list(true, toward);
        }
        // Every row is worth asking about once more, where what was kept
        // turns out not to be current: the list it is checked against is
        // the one just asked for.
        self.pulls.asked_since_opening.clear();
        self.show_pull_requests();
    }

    /// Whether the list is on its issues tab.
    fn showing_issues(&self) -> bool {
        self.picker.as_ref().is_some_and(|picker| picker.tab() == 1)
    }

    /// Asks what has happened on the pull request the reader is on, unless
    /// it has been asked since the list opened.
    ///
    /// From the preview, which is built for the row the reader is on and so
    /// is the one place that knows which that is -- every frame, and asked
    /// once per opening. One at a time: a reader walking the list passes
    /// rows they will not stop on, so nothing is asked while an answer is on
    /// its way, and the first frame after it lands asks about whichever row
    /// the reader has stopped on by then.
    pub(super) fn ask_about(&mut self, asked: Asked) {
        let number = asked.number();
        if self.pulls.asked_since_opening.contains(&number)
            || self.pulls.asking_about.is_some()
            || self.kept_is_current(asked)
        {
            return;
        }
        let Some(sender) = self.events.clone() else {
            return;
        };
        self.pulls.asked_since_opening.insert(number);
        self.pulls.asking_about = Some(number);
        let root = self.working_directory.clone();
        let instead = self.pulls.instead.clone();
        obelus_runtime::handle().spawn_blocking(move || {
            let answer = obelus_github::discussion(&root, instead, asked);
            let _ = sender.send(Event::PullRequestDiscussion { number, answer });
        });
    }

    /// Whether what was kept about one is still what GitHub would say: the
    /// list's word on when it last changed is the word it was kept at.
    ///
    /// A comment, a review, a push and an edit to the description all move
    /// that; a check finishing does not, so a pull request with one still
    /// running is asked about again as if nothing were kept.
    fn kept_is_current(&self, asked: Asked) -> bool {
        let number = asked.number();
        let Some(kept) = self.pulls.discussions.get(&number) else {
            return false;
        };
        let listed = match asked {
            Asked::PullRequest(_) => self.pull_request(number).map(|pull| pull.stamp.as_str()),
            Asked::Issue(_) => self.issue(number).map(|issue| issue.stamp.as_str()),
        };
        !kept.stamp.is_empty()
            && listed == Some(kept.stamp.as_str())
            && !kept
                .checks
                .iter()
                .any(|check| check.stands == Stands::Running)
    }

    /// Takes what `gh` said has happened on one pull request.
    pub(super) fn on_pull_request_discussion(
        &mut self,
        number: u64,
        answer: Result<Discussion, Unlisted>,
    ) {
        self.pulls.asking_about = None;
        match answer {
            Ok(discussion) => {
                self.pulls.discussions.insert(number, discussion);
                self.pulls.refused.remove(&number);
            }
            // What was kept stays, and is still true as far as anybody
            // here knows; with nothing kept, the preview says why.
            Err(why) => {
                tracing::info!(?why, number, "no word on a pull request");
                self.pulls.refused.insert(number, why);
            }
        }
        self.lay_the_preview_out_again(number);
    }

    /// Asks `gh`, on the blocking pool, for one tab's list to grow
    /// `toward` wherever it is asked to, and sends what it said to the loop.
    fn ask_for_the_list(&mut self, issues: bool, toward: Toward) {
        match issues {
            true => self.pulls.issues.asked(&toward),
            false => self.pulls.pulls.asked(&toward),
        }
        let Some(sender) = self.events.clone() else {
            // No loop to answer into: a test hands the answer over itself.
            return;
        };
        let root = self.working_directory.clone();
        let instead = self.pulls.instead.clone();
        obelus_runtime::handle().spawn_blocking(move || {
            let _ = sender.send(match issues {
                true => Event::Issues(obelus_github::issues(&root, instead, toward)),
                false => Event::PullRequests(obelus_github::pull_requests(&root, instead, toward)),
            });
        });
    }

    /// Takes what `gh` said the pull requests are, and puts them in the list
    /// if the list is up.
    pub(super) fn on_pull_requests(&mut self, answer: Listed<PullRequest>) {
        self.pulls.pulls.answer(answer);
        self.the_list_has_moved();
    }

    /// The same for the issues.
    pub(super) fn on_issues(&mut self, answer: Listed<Issue>) {
        self.pulls.issues.answer(answer);
        self.the_list_has_moved();
    }

    /// Asks for more of the list where its last row is on screen, the list
    /// being `rows` tall.
    ///
    /// Asked once a frame, after the window has settled, rather than from
    /// every key and notch and drag that can bring the end into view: one
    /// place asks, so a way of moving the list cannot forget to.
    pub(super) fn reach_further(&mut self, rows: u16) {
        if !self.listing_pull_requests() {
            return;
        }
        let Some(picker) = self.picker.as_ref() else {
            return;
        };
        if picker.window().visible(rows).end < picker.match_count() {
            return;
        }
        let query = picker.query().trim().to_string();
        let issues = self.showing_issues();
        let toward = match issues {
            true => self.pulls.issues.further(&query),
            false => self.pulls.pulls.further(&query),
        };
        if let Some(toward) = toward {
            self.ask_for_the_list(issues, toward);
            self.say_how_much();
        }
    }

    /// Hears the query, or the tab it is asked of, move: GitHub is asked
    /// about it once the typing has stopped, and what it said about the
    /// last one is let go.
    pub(super) fn the_query_has_moved(&mut self) {
        let Some(query) = self
            .picker
            .as_ref()
            .map(|picker| picker.query().trim().to_string())
        else {
            return;
        };
        for search in [&mut self.pulls.pulls.search, &mut self.pulls.issues.search] {
            if search.as_ref().is_some_and(|search| search.query != query) {
                *search = None;
            }
        }
        // Nothing to wait for where this tab has already asked: walking
        // back onto it is not typing.
        let asked = match self.showing_issues() {
            true => self.pulls.issues.search_for(&query).is_some(),
            false => self.pulls.pulls.search_for(&query).is_some(),
        };
        self.pulls.settling = match query.is_empty() || asked {
            true => None,
            false => self.come_back_in(SETTLES_AFTER, Event::PullRequestQuerySettled),
        };
        self.say_how_much();
    }

    /// Asks GitHub's search about the query in the box, which has stopped
    /// moving, for the tab showing -- unless it already has, or the list
    /// itself was refused.
    pub(super) fn on_the_query_settled(&mut self) {
        self.pulls.settling = None;
        if !self.listing_pull_requests() {
            return;
        }
        let Some(query) = self
            .picker
            .as_ref()
            .map(|picker| picker.query().trim().to_string())
        else {
            return;
        };
        let issues = self.showing_issues();
        let (searchable, search) = match issues {
            true => (
                self.pulls.issues.searchable(),
                &mut self.pulls.issues.search,
            ),
            false => (self.pulls.pulls.searchable(), &mut self.pulls.pulls.search),
        };
        let asked = search.as_ref().is_some_and(|search| search.query == query);
        if query.is_empty() || asked || !searchable {
            self.say_how_much();
            return;
        }
        self.pulls.searches += 1;
        let which = self.pulls.searches;
        *search = Some(Search::new(query.clone(), which));
        self.ask_for_the_list(
            issues,
            Toward::Found {
                query,
                asking: which,
                after: None,
            },
        );
        self.say_how_much();
    }

    /// Lets what did not come be asked for again, the next time the last
    /// row is on screen -- which it is, because this is the reader pressing
    /// down on it.
    pub(super) fn try_the_next_page_again(&mut self) {
        let query = self
            .picker
            .as_ref()
            .map(|picker| picker.query().trim().to_string())
            .unwrap_or_default();
        match self.showing_issues() {
            true => self.pulls.issues.try_again(&query),
            false => self.pulls.pulls.try_again(&query),
        }
        self.say_how_much();
    }

    /// Whether the reader is on the last row of a list whose next page did
    /// not come, which is where down tries again.
    pub(super) fn stuck_at_the_end(&self) -> bool {
        let Some(picker) = self.picker.as_ref() else {
            return false;
        };
        let query = picker.query().trim().to_string();
        let stuck = match self.showing_issues() {
            true => self.pulls.issues.stuck(&query),
            false => self.pulls.pulls.stuck(&query),
        };
        stuck && picker.selected() + 1 >= picker.match_count()
    }

    /// Puts a new answer on screen.
    ///
    /// A preview is kept by its subject, and the subject is a number: the
    /// same number with a new description is not a new subject, so what was
    /// laid out from the last answer has to go with it.
    fn the_list_has_moved(&mut self) {
        if let Some(previewing::Subject::PullRequest(number) | previewing::Subject::Issue(number)) =
            self.preview.as_ref().map(previewing::Preview::subject)
        {
            let number = *number;
            self.lay_the_preview_out_again(number);
        }
        self.show_pull_requests();
    }

    /// Whether the list showing is the pull requests.
    pub(super) fn listing_pull_requests(&self) -> bool {
        self.picker
            .as_ref()
            .is_some_and(|picker| picker.opener() == Some(Command::PullRequestReview))
    }

    /// Fills the list, for the tab showing, from what `gh` last said.
    pub(super) fn show_pull_requests(&mut self) {
        if !self.listing_pull_requests() {
            return;
        }
        // Who has which, looked at now: the list is the door into a review
        // or an answer, and a door that lets the reader into another
        // window's is the thing a claim is for.
        self.reread_who_holds_what();
        let now = std::time::SystemTime::now();
        // The last answer while the next is on its way, and nothing before
        // there has been one.
        let items: Vec<PickerItem> = match self.showing_issues() {
            true => match self.pulls.issues.answered {
                true => {
                    let numbered =
                        widest(self.pulls.issues.listed.iter().map(|issue| issue.number));
                    self.pulls
                        .issues
                        .listed
                        .iter()
                        .map(|issue| self.issue_row(issue, numbered, now))
                        .collect()
                }
                false => Vec::new(),
            },
            false => match self.pulls.pulls.answered {
                true => {
                    let numbered = widest(self.pulls.pulls.listed.iter().map(|pull| pull.number));
                    self.pulls
                        .pulls
                        .listed
                        .iter()
                        .map(|pull| self.pull_request_row(pull, numbered, now))
                        .collect()
                }
                false => Vec::new(),
            },
        };
        if let Some(picker) = self.picker.as_mut() {
            // Standing on the same one, wherever the new answer puts it --
            // see `review_a_pull_request`.
            picker.renew(items, |one, other| {
                matches!(
                    (one, other),
                    (PickerValue::PullRequest(one), PickerValue::PullRequest(other))
                        | (PickerValue::Issue(one), PickerValue::Issue(other))
                        if one == other
                )
            });
        }
        self.say_how_much();
    }

    /// Says, on the list showing, how much of it there is and what is being
    /// done about the rest -- without touching its rows, which a query
    /// moving has not changed.
    pub(super) fn say_how_much(&mut self) {
        if !self.listing_pull_requests() {
            return;
        }
        let Some(query) = self
            .picker
            .as_ref()
            .map(|picker| picker.query().trim().to_string())
        else {
            return;
        };
        let settling = self.pulls.settling.is_some();
        let matched = self
            .picker
            .as_ref()
            .map_or(0, obelus_component::picker::Picker::match_count);
        let said = match self.showing_issues() {
            true => self
                .pulls
                .issues
                .says(&query, settling, matched, "No issue is open"),
            false => self
                .pulls
                .pulls
                .says(&query, settling, matched, "No pull request is open"),
        };
        if let Some(picker) = self.picker.as_mut() {
            let (empty, whatever_is_typed) = &said.empty;
            match whatever_is_typed {
                true => picker.while_empty(empty),
                false => picker.when_empty(empty),
            }
            // And the mark that turns while an answer is on its way, which
            // every list still waiting on its rows wears: in front of that
            // line where there are no rows, and after the box where the
            // rows already there are standing in.
            picker.filling(said.turning.then(|| "Asking GitHub\u{2026}".to_string()));
            picker.tally(said.tally);
            picker.unfinished(said.unfinished);
        }
    }

    /// One pull request as a row of the list, its number as wide as the
    /// widest the list has.
    fn pull_request_row(
        &self,
        pull: &PullRequest,
        numbered: usize,
        now: std::time::SystemTime,
    ) -> PickerItem {
        let Said {
            marker,
            enabled,
            trailing,
        } = self.what_a_pull_request_row_says(pull, now);
        // The number in the label, in front of the title, because the label
        // is what a query is matched against: a pull request is named by
        // its number as often as by what it says, and a number on the far
        // side of the row could be read and not typed. Quieter than the
        // title, in the colour a comment is -- it says which, and the title
        // says what.
        //
        // And as wide as the widest, set to the right, so every title starts
        // in one column: a ragged edge reads as rows missing words, and a
        // reader running down the titles has to find each one's start.
        let number = format!("{:>numbered$}", format!("#{}", pull.number));
        let quiet = u16::try_from(number.chars().count()).unwrap_or(u16::MAX);
        PickerItem {
            // A sentence, which loses its end where it has to lose anything.
            prose: true,
            icon: None,
            marker,
            label: format!("{number} {}", pull.title),
            detail: pull.draft.then(|| "Draft".to_string()),
            trailing,
            changed: None,
            version: None,
            value: PickerValue::PullRequest(pull.number),
            depth: 0,
            opens: None,
            status: None,
            enabled,
            colours: Some(vec![(0, quiet, obelus_text::kind::SyntaxKind::Comment)]),
            kind: None,
            tab: None,
            section: None,
        }
    }

    /// What a row says about itself now: its mark, whether it can be
    /// chosen, and the words at its end.
    ///
    /// The part of a row that is about *now* -- who has its review -- so it
    /// is asked again every frame by [`App::freshen_the_pull_request_rows`],
    /// the way a conversation's row is: another window letting go of a
    /// review is not this reader's keystroke, and the row has to say so
    /// before they press.
    fn what_a_pull_request_row_says(&self, pull: &PullRequest, now: std::time::SystemTime) -> Said {
        let elsewhere =
            self.taken_up_elsewhere(&obelus_agent::chats::ChatId::PullRequest(pull.number));
        // The lock over what GitHub says: a review the reader cannot enter
        // is the first thing the row has to say.
        let marker = match (elsewhere, pull.decision) {
            (true, _) => Some(super::conversations::locked()),
            (false, Some(Decision::Approved)) => Some((Marking::Aside, "\u{2713}".to_string())),
            (false, Some(Decision::ChangesRequested)) => {
                Some((Marking::Aside, "\u{2717}".to_string()))
            }
            (false, None) => None,
        };
        // Who and when, where the width is taken out of the title's before
        // it is cut. The number is in the label, where it can be typed.
        let mut trailing = pull.author.clone();
        if let Some(updated) = pull.updated {
            trailing.push_str(&format!(
                " \u{b7} {}",
                obelus_git::how_long_ago(updated, now)
            ));
        }
        Said {
            marker,
            enabled: !elsewhere,
            trailing: Some(trailing),
        }
    }

    /// One issue as a row of the list: a pull request's row, with what it
    /// has been labelled where a pull request says it is a draft.
    fn issue_row(&self, issue: &Issue, numbered: usize, now: std::time::SystemTime) -> PickerItem {
        let Said {
            marker,
            enabled,
            trailing,
        } = self.what_an_issue_row_says(issue, now);
        let number = format!("{:>numbered$}", format!("#{}", issue.number));
        let quiet = u16::try_from(number.chars().count()).unwrap_or(u16::MAX);
        PickerItem {
            prose: true,
            icon: None,
            marker,
            label: format!("{number} {}", issue.title),
            detail: (!issue.labels.is_empty()).then(|| issue.labels.join(" \u{b7} ")),
            trailing,
            changed: None,
            version: None,
            value: PickerValue::Issue(issue.number),
            depth: 0,
            opens: None,
            status: None,
            enabled,
            colours: Some(vec![(0, quiet, obelus_text::kind::SyntaxKind::Comment)]),
            kind: None,
            tab: None,
            section: None,
        }
    }

    /// What an issue's row says about itself now -- see
    /// [`App::what_a_pull_request_row_says`].
    fn what_an_issue_row_says(&self, issue: &Issue, now: std::time::SystemTime) -> Said {
        let elsewhere = self.taken_up_elsewhere(&obelus_agent::chats::ChatId::Issue(issue.number));
        let mut trailing = issue.author.clone();
        if let Some(updated) = issue.updated {
            trailing.push_str(&format!(
                " \u{b7} {}",
                obelus_git::how_long_ago(updated, now)
            ));
        }
        Said {
            marker: elsewhere.then(super::conversations::locked),
            enabled: !elsewhere,
            trailing: Some(trailing),
        }
    }

    /// Whether another Obelus has this conversation, and this one does not.
    fn taken_up_elsewhere(&self, which: &obelus_agent::chats::ChatId) -> bool {
        let mine = self
            .documents
            .iter()
            .flatten()
            .filter_map(Document::chat)
            .any(|talk| talk.which().as_ref() == Some(which));
        !mine && self.held_now().contains_key(which)
    }

    /// Says again, on every row of the list, who has which review or
    /// answer.
    ///
    /// From the claims as Obelus last looked, which the watch on them keeps
    /// level; marking rather than rebuilding, so the reader's row and what
    /// they typed stay where they are.
    pub(super) fn freshen_the_pull_request_rows(&mut self) {
        if !self.listing_pull_requests() {
            return;
        }
        let now = std::time::SystemTime::now();
        let pulls: std::collections::HashMap<u64, Said> = self
            .pulls
            .pulls
            .listed
            .iter()
            .map(|pull| (pull.number, self.what_a_pull_request_row_says(pull, now)))
            .collect();
        let issues: std::collections::HashMap<u64, Said> = self
            .pulls
            .issues
            .listed
            .iter()
            .map(|issue| (issue.number, self.what_an_issue_row_says(issue, now)))
            .collect();
        if let Some(picker) = self.picker.as_mut() {
            picker.remark(|value| {
                let said = match value {
                    PickerValue::PullRequest(number) => pulls.get(number),
                    PickerValue::Issue(number) => issues.get(number),
                    _ => None,
                };
                said.map_or(Remark::Keep, |now| Remark::Now(now.clone()))
            });
        }
    }

    /// What `gh` last said about one pull request.
    #[must_use]
    pub(super) fn pull_request(&self, number: u64) -> Option<&PullRequest> {
        self.pulls
            .pulls
            .listed
            .iter()
            .find(|pull| pull.number == number)
    }

    /// What `gh` last said about one issue.
    #[must_use]
    pub(super) fn issue(&self, number: u64) -> Option<&Issue> {
        self.pulls
            .issues
            .listed
            .iter()
            .find(|issue| issue.number == number)
    }

    /// What a pull request says about itself, laid out at `width` for the
    /// preview under the list.
    ///
    /// Two rows naming it, in the shape a commit's message names its commit
    /// -- which, who, from where to where, how long ago, and how much it
    /// changes -- and then its title and its description as the markdown
    /// they are, laid out by the same code a markdown file is. How much it
    /// changes and what it says are asked for the row the reader is on
    /// (`VIEWED`), so until `gh` has said the title stands alone.
    ///
    /// Then, each after a rule, how its checks stand and what has been said
    /// on it -- or, while `gh` has not said yet, one row saying so, whose
    /// place is handed back so that the view can turn a mark at its head.
    /// The rows are laid out once and kept, and a mark in them would stand
    /// still.
    pub(super) fn pull_request_reading(
        &self,
        number: u64,
        width: u16,
    ) -> (Vec<obelus_row::Row>, Option<usize>) {
        use obelus_row::{Ink, Row, Span};

        let Some(pull) = self.pull_request(number) else {
            return (Vec::new(), None);
        };
        let mut named = format!(
            "#{}   {}   {} \u{2192} {}",
            pull.number, pull.author, pull.head, pull.base
        );
        if let Some(updated) = pull.updated {
            named.push_str(&format!(
                "   {}",
                obelus_git::how_long_ago(updated, std::time::SystemTime::now())
            ));
        }
        let kept = self.pulls.discussions.get(&number);
        let mut rows = vec![Row::of(vec![Span::new(named, Ink::Aside)])];
        if let Some(kept) = kept {
            let files = match kept.files {
                1 => "1 file".to_string(),
                files => format!("{files} files"),
            };
            rows.push(Row::of(vec![
                Span::new(format!("+{}", kept.additions), Ink::Added),
                Span::new(" ", Ink::Aside),
                Span::new(format!("\u{2212}{}", kept.deletions), Ink::Removed),
                Span::new(format!(" \u{b7} {files}"), Ink::Aside),
            ]));
        }
        rows.push(Row::default());
        rows.extend(described(
            &pull.title,
            kept.map(|kept| kept.body.as_str()),
            width,
        ));
        self.what_has_happened_since(rows, number, true, width)
    }

    /// What an issue says about itself, laid out at `width`: a pull
    /// request's reading, with what it has been labelled where the counts
    /// would be, and no checks -- an issue has no commit to run them on.
    pub(super) fn issue_reading(
        &self,
        number: u64,
        width: u16,
    ) -> (Vec<obelus_row::Row>, Option<usize>) {
        use obelus_row::{Ink, Row, Span};

        let Some(issue) = self.issue(number) else {
            return (Vec::new(), None);
        };
        let mut named = format!("#{}   {}", issue.number, issue.author);
        if let Some(updated) = issue.updated {
            named.push_str(&format!(
                "   {}",
                obelus_git::how_long_ago(updated, std::time::SystemTime::now())
            ));
        }
        let mut rows = vec![Row::of(vec![Span::new(named, Ink::Aside)])];
        if !issue.labels.is_empty() {
            rows.push(Row::of(vec![Span::new(
                issue.labels.join(" \u{b7} "),
                Ink::Key,
            )]));
        }
        rows.push(Row::default());
        rows.extend(described(
            &issue.title,
            self.pulls
                .discussions
                .get(&number)
                .map(|kept| kept.body.as_str()),
            width,
        ));
        self.what_has_happened_since(rows, number, false, width)
    }

    /// Whether `gh` has yet to say anything about a pull request or an
    /// issue, which is when its reading ends in a line that waits.
    pub(super) fn still_asking_about(&self, number: u64) -> bool {
        !self.pulls.discussions.contains_key(&number) && !self.pulls.refused.contains_key(&number)
    }

    /// The parts of a reading after the description: how the checks stand
    /// where there are any to ask about, and what has been said -- each
    /// after a rule, the way markdown's `---` is drawn, because they are
    /// separate things and not one long page.
    ///
    /// Or, while `gh` has not said yet, one row saying so, whose place is
    /// handed back so that the view can turn a mark at its head. The rows
    /// are laid out once and kept, and a mark in them would stand still.
    fn what_has_happened_since(
        &self,
        mut rows: Vec<obelus_row::Row>,
        number: u64,
        with_checks: bool,
        width: u16,
    ) -> (Vec<obelus_row::Row>, Option<usize>) {
        use obelus_row::{Ink, Row, Span};

        let rule = Row {
            spans: Vec::new(),
            rule: true,
            code: None,
            links: Vec::new(),
        };
        rows.push(rule.clone());
        let Some(discussion) = self.pulls.discussions.get(&number) else {
            match self.pulls.refused.get(&number) {
                Some(why) => rows.push(Row::of(vec![Span::new(unlisted(why), Ink::Aside)])),
                None => {
                    let turning = rows.len();
                    // Two blanks in front, where the view puts the mark and
                    // the one blank after it.
                    let waiting = match with_checks {
                        true => "  Asking GitHub for its description, checks and comments",
                        false => "  Asking GitHub for its description and comments",
                    };
                    rows.push(Row::of(vec![Span::new(waiting, Ink::Aside)]));
                    return (rows, Some(turning));
                }
            }
            return (rows, None);
        };
        if with_checks {
            rows.extend(checks(&discussion.checks));
            rows.push(rule);
        }
        rows.extend(comments(&discussion.said, width));
        (rows, None)
    }

    /// What a review is called before the agent has called it anything.
    #[must_use]
    pub(super) fn what_a_review_is_called(&self, number: u64) -> String {
        match self.pull_request(number) {
            Some(pull) => format!("Review #{number}: {}", pull.title),
            None => format!("Review #{number}"),
        }
    }

    /// What an answer to an issue is called before the agent has called it
    /// anything.
    #[must_use]
    pub(super) fn what_an_answer_is_called(&self, number: u64) -> String {
        match self.issue(number) {
            Some(issue) => format!("Issue #{number}: {}", issue.title),
            None => format!("Issue #{number}"),
        }
    }

    /// Goes to the review of one pull request, or the answer to one issue,
    /// opening one if there is none.
    ///
    /// The note's door, for either: claimed before it is opened, and
    /// `false` where another Obelus has it -- the row goes dim under the
    /// reader, which is the answer.
    pub(super) fn take_up(&mut self, wanted: crate::conversation::Topic) -> bool {
        let at = self.documents.iter().position(|document| {
            document
                .as_ref()
                .and_then(Document::chat)
                .is_some_and(|talk| talk.topic == wanted)
        });
        let at = match at {
            Some(at) => at,
            None => {
                let Some(which) = wanted.which() else {
                    return false;
                };
                let Some(claim) = obelus_agent::chats::claim(&self.working_directory, &which)
                else {
                    // Being refused is news fresher than the watch has: look
                    // again, and mark the rows rather than rebuild them, so
                    // the one the reader pressed goes dim under them and the
                    // selection stays on it.
                    self.reread_who_holds_what();
                    self.freshen_the_pull_request_rows();
                    return false;
                };
                let (told, introduced) = self.remembered_telling(&which);
                let talk = crate::conversation::Conversation {
                    told,
                    introduced,
                    topic: wanted,
                    claim: Some(claim),
                    ..crate::conversation::Conversation::default()
                };
                self.documents.push(Some(talk.into()));
                self.documents.len() - 1
            }
        };
        self.make_room(Room::Region);
        self.go_to_document(DocumentId::new(at));
        true
    }

    /// Runs `program` with `first` in front of `gh`'s own arguments, in
    /// `gh`'s place.
    pub fn gh_for_test(&mut self, program: std::path::PathBuf, first: Vec<String>) {
        self.pulls.instead = Some((program, first));
    }
}

/// How wide the widest `#number` of a list is, which every row's number is
/// set to.
fn widest(numbers: impl Iterator<Item = u64>) -> usize {
    numbers
        .map(|number| format!("#{number}").len())
        .max()
        .unwrap_or(0)
}

/// A title as the heading it is, and the description after it as its
/// author wrote it -- a description with headings of its own keeps them
/// under this one.
///
/// With no description until `gh` has said what it is: "No description"
/// is a thing to say about one that came back empty, not one still asked.
fn described(title: &str, body: Option<&str>, width: u16) -> Vec<obelus_row::Row> {
    use obelus_row::{Ink, Row, Span};

    let source = format!("# {title}\n\n{}", body.unwrap_or_default());
    let mut rows = obelus_markdown::render(&source, width);
    if body.is_some_and(|body| body.trim().is_empty()) {
        rows.push(Row::default());
        rows.push(Row::of(vec![Span::new("No description", Ink::Aside)]));
    }
    rows
}

/// How a pull request's checks stand, as rows: a count of each kind on one
/// row, and under it by name every one that has not passed.
///
/// Not every name: a check that passed is one nobody has to do anything
/// about, and fourteen rows of them would push what was said off the
/// preview for a fact one count says.
fn checks(checks: &[Check]) -> Vec<obelus_row::Row> {
    use obelus_row::{Ink, Row, Span};

    let heading = Span::new("Checks  ", Ink::Heading(2));
    if checks.is_empty() {
        return vec![Row::of(vec![heading, Span::new("None", Ink::Aside)])];
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
    let mut rows = vec![Row::of(summary)];
    for (stands, mark, _, ink) in &kinds[..2] {
        for check in checks.iter().filter(|check| check.stands == *stands) {
            rows.push(Row::of(vec![
                Span::new(format!("  {mark} "), *ink),
                Span::new(check.name.clone(), Ink::Plain),
            ]));
        }
    }
    rows
}

/// What has been said on a pull request, as rows, newest first: who and
/// what they did, then what they wrote, laid out as the markdown it is.
fn comments(said: &[Comment], width: u16) -> Vec<obelus_row::Row> {
    use obelus_row::{Ink, Row, Span};

    if said.is_empty() {
        return vec![Row::of(vec![Span::new("No comments", Ink::Aside)])];
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
        rows.push(Row::of(vec![
            Span::new(comment.author.clone(), Ink::Name),
            Span::new(did, Ink::Aside),
        ]));
        rows.extend(obelus_markdown::render(&comment.body, width));
    }
    rows
}

/// What the empty list says about why it is empty.
fn unlisted(why: &Unlisted) -> String {
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

#[cfg(test)]
mod tests {
    /// A pull request numbered `number` that last changed at `stamp`.
    fn pull(number: u64, stamp: &str) -> super::PullRequest {
        super::PullRequest {
            number,
            title: "t".to_string(),
            author: "a".to_string(),
            head: String::new(),
            base: String::new(),
            sha: String::new(),
            draft: false,
            decision: None,
            updated: stamp
                .parse::<jiff::Timestamp>()
                .ok()
                .map(jiff::Timestamp::as_second),
            stamp: stamp.to_string(),
        }
    }

    /// A tab with `numbers` listed, the higher the newer.
    fn listing(numbers: &[u64]) -> super::Listing<super::PullRequest> {
        let mut listing = super::Listing::default();
        listing.answer(super::Listed::First(super::Page {
            rows: numbers
                .iter()
                .map(|number| pull(*number, &format!("2026-10-0{}T00:00:00Z", number % 10)))
                .collect(),
            older: Some("next".to_string()),
            total: 70,
        }));
        listing
    }

    /// What the foot says: how many of how many while there are more, how
    /// many once there are not, how many rows a search left in the list --
    /// and whether GitHub has more, or stopped at its thousand -- and why a
    /// page did not come with the key that tries again.
    ///
    /// Broken deliberately four ways. Dropping the `older` test from the
    /// count says "of 70" about a list that has every row. Letting the
    /// thousand GitHub stops at read as every match says nothing about the
    /// rest. Counting by GitHub's number, as this first did, says 438 over
    /// 97 rows. And letting the typing settle on a tab that has already
    /// asked turns the mark for a question nobody will ask.
    #[test]
    fn the_foot_says_how_much_there_is() {
        let words = |listing: &super::Listing<super::PullRequest>, query: &str, matched| {
            listing
                .says(query, false, matched, "None")
                .tally
                .map(|tally| tally.words)
                .unwrap_or_default()
        };
        let mut tab = listing(&[3, 2]);
        assert_eq!(words(&tab, "", 2), "2 of 70 open");
        tab.older = None;
        assert_eq!(words(&tab, "", 2), "2 open");

        tab.search = Some(super::Search {
            answered: true,
            found: (0..100).collect(),
            total: 438,
            older: Some("next".to_string()),
            ..super::Search::new("fold".to_string(), 1)
        });
        assert_eq!(words(&tab, "fold", 97), "97 match so far");
        if let Some(search) = tab.search.as_mut() {
            search.found = (0..1000).collect();
            search.total = 4210;
            search.older = None;
        }
        assert_eq!(
            words(&tab, "fold", 97),
            "97 match \u{b7} GitHub gives no more"
        );
        if let Some(search) = tab.search.as_mut() {
            search.total = 1000;
        }
        assert_eq!(words(&tab, "fold", 97), "97 match");
        assert_eq!(words(&tab, "fold", 1), "1 matches");
        // The typing settling says GitHub is about to be asked -- but not on
        // a tab that has asked already, which walking back onto is.
        assert_eq!(words(&tab, "fold", 97), "97 match");
        assert_eq!(
            tab.says("fold", true, 97, "None")
                .tally
                .map(|tally| tally.words),
            Some("97 match".to_string())
        );
        tab.search = None;
        assert_eq!(
            tab.says("fold", true, 97, "None")
                .tally
                .map(|tally| tally.words),
            Some("Asking GitHub about \"fold\"".to_string())
        );

        tab.stalled = Some(super::Unlisted::Failed("EOF".to_string()));
        let stuck = tab.says("", false, 0, "None").tally.expect("a tally");
        assert_eq!(stuck.words, "GitHub would not answer: EOF");
        assert_eq!(
            stuck.key,
            Some(("\u{2193}".to_string(), "Try again".to_string()))
        );
    }

    /// What has changed goes where it now sorts, and what has gone goes;
    /// the next page is not asked for while one did not come.
    ///
    /// Broken deliberately by leaving `gone` out of what is taken out
    /// before the merge, which keeps #2; and by asking in `further` with a
    /// refusal standing.
    #[test]
    fn what_has_changed_moves_and_what_has_gone_goes() {
        let mut tab = listing(&[3, 2, 1]);
        tab.answer(super::Listed::Changed {
            open: vec![pull(1, "2026-10-09T00:00:00Z")],
            gone: vec![2],
            newest: "2026-10-09T00:00:00Z".to_string(),
            total: 2,
        });
        let numbers: Vec<u64> = tab.listed.iter().map(|pull| pull.number).collect();
        assert_eq!(numbers, [1, 3]);
        assert_eq!(tab.newest, "2026-10-09T00:00:00Z");

        assert_eq!(
            tab.further(""),
            Some(super::Toward::Older("next".to_string()))
        );
        tab.answer(super::Listed::Refused {
            toward: super::Toward::Older("next".to_string()),
            why: super::Unlisted::Failed("EOF".to_string()),
        });
        assert_eq!(
            tab.listed.len(),
            2,
            "a page that did not come took the rows"
        );
        assert_eq!(tab.further(""), None);
    }

    /// A tab whose search for "fold" is asking `which`, before anything of
    /// it has come.
    fn searching(numbers: &[u64], which: u64) -> super::Listing<super::PullRequest> {
        let mut tab = listing(numbers);
        tab.search = Some(super::Search::new("fold".to_string(), which));
        tab.asked(&super::Toward::Found {
            query: "fold".to_string(),
            asking: which,
            after: None,
        });
        tab
    }

    /// A page of what the search for "fold" found, for asking `which`.
    fn found(numbers: &[u64], which: u64) -> super::Listed<super::PullRequest> {
        super::Listed::Found {
            query: "fold".to_string(),
            asking: which,
            page: super::Page {
                rows: numbers
                    .iter()
                    .map(|number| pull(*number, &format!("2026-09-0{}T00:00:00Z", number % 10)))
                    .collect(),
                older: None,
                total: numbers.len() as u64,
            },
        }
    }

    /// What a search found before the first page landed is still in the
    /// list after it: nothing will ask for it again, and "No match" would be
    /// a lie about a row GitHub sent.
    ///
    /// Broken deliberately by clearing the list in `First` whatever was
    /// found, as it first did.
    #[test]
    fn a_first_page_after_a_search_keeps_what_it_found() {
        let mut tab = super::Listing {
            search: Some(super::Search::new("fold".to_string(), 1)),
            ..super::Listing::default()
        };
        tab.answer(found(&[5], 1));
        tab.answer(super::Listed::First(super::Page {
            rows: vec![pull(9, "2026-10-09T00:00:00Z")],
            older: None,
            total: 1,
        }));
        let numbers: Vec<u64> = tab.listed.iter().map(|pull| pull.number).collect();
        assert_eq!(numbers, [9, 5]);
    }

    /// A search whose first page did not come says why where the rows
    /// would be, rather than "No match", and is asked again once the reader
    /// says to.
    ///
    /// Broken deliberately two ways. Letting the empty line fall through to
    /// the query's own words says "No match" about a search nothing came
    /// back from. And asking `further` only for a page after one that came
    /// leaves the first never asked again.
    #[test]
    fn a_search_that_did_not_come_says_so_and_is_asked_again() {
        let mut tab = searching(&[3], 1);
        tab.answer(super::Listed::Refused {
            toward: super::Toward::Found {
                query: "fold".to_string(),
                asking: 1,
                after: None,
            },
            why: super::Unlisted::Failed("EOF".to_string()),
        });
        let said = tab.says("fold", false, 0, "None");
        assert_eq!(
            said.empty,
            ("GitHub would not answer: EOF".to_string(), true)
        );
        assert_eq!(tab.further("fold"), None, "asked again with nobody asking");
        tab.try_again("fold");
        assert_eq!(
            tab.further("fold"),
            Some(super::Toward::Found {
                query: "fold".to_string(),
                asking: 1,
                after: None,
            })
        );
    }

    /// What has changed not coming leaves the list saying it may be out of
    /// date, with no key that would ask for something else -- and does not
    /// stop the next page being asked for.
    ///
    /// Broken deliberately by writing that refusal down where an older
    /// page's goes, as it first was: the foot offers a key that asks for
    /// the wrong thing, and the paging stops.
    #[test]
    fn what_has_changed_not_coming_says_the_list_may_be_out_of_date() {
        let mut tab = listing(&[3, 2]);
        tab.asked(&super::Toward::Newer("2026-10-03T00:00:00Z".to_string()));
        tab.answer(super::Listed::Refused {
            toward: super::Toward::Newer("2026-10-03T00:00:00Z".to_string()),
            why: super::Unlisted::Failed("EOF".to_string()),
        });
        let tally = tab.says("", false, 2, "None").tally.expect("a tally");
        assert_eq!(
            tally.words,
            "May be out of date \u{b7} GitHub would not answer: EOF"
        );
        assert_eq!(tally.key, None);
        assert_eq!(
            tab.further(""),
            Some(super::Toward::Older("next".to_string()))
        );
    }

    /// An answer to an asking of a query from before the reader typed it
    /// again is not taken for the asking now.
    ///
    /// Broken deliberately by matching an answer to its search by the
    /// query alone.
    #[test]
    fn an_answer_to_an_earlier_asking_of_the_same_query_is_let_go() {
        let mut tab = searching(&[3], 2);
        tab.answer(found(&[7], 1));
        let search = tab.search.as_ref().expect("the search");
        assert!(search.asking, "the asking now was taken as answered");
        assert!(!tab.listed.iter().any(|pull| pull.number == 7));
    }

    /// An opening while an older page is on its way still asks what has
    /// changed.
    ///
    /// Broken deliberately by keeping the page's asking where the top's is,
    /// as it first was.
    #[test]
    fn an_opening_while_a_page_is_on_its_way_asks_what_has_changed() {
        let mut tab = listing(&[3, 2]);
        tab.asked(&super::Toward::Older("next".to_string()));
        assert!(matches!(tab.opening(), Some(super::Toward::Newer(_))));
    }

    /// A query typed over a list GitHub refused is not said to be asked
    /// about, nor offered a key to try again: the refusal where the rows
    /// would be is the whole of the answer.
    ///
    /// Broken deliberately by taking out the arm that says nothing where
    /// the list is not `searchable`.
    #[test]
    fn a_query_over_a_refused_list_says_only_the_refusal() {
        let mut tab: super::Listing<super::PullRequest> = super::Listing::default();
        tab.answer(super::Listed::Refused {
            toward: super::Toward::First,
            why: super::Unlisted::SignedOut,
        });
        tab.search = Some(super::Search::new("fold".to_string(), 1));
        tab.answer(super::Listed::Refused {
            toward: super::Toward::Found {
                query: "fold".to_string(),
                asking: 1,
                after: None,
            },
            why: super::Unlisted::SignedOut,
        });
        let said = tab.says("fold", true, 0, "None");
        assert_eq!(said.tally, None);
        assert!(!said.turning);
        assert_eq!(said.empty.0, super::unlisted(&super::Unlisted::SignedOut));
    }

    /// Of two copies of a row, the one that changed later is kept.
    ///
    /// Broken deliberately by keeping whichever was listed first.
    #[test]
    fn the_later_copy_of_a_row_is_kept() {
        let mut tab = listing(&[3]);
        tab.merge(vec![pull(3, "2026-10-09T00:00:00Z")]);
        assert_eq!(tab.listed[0].stamp, "2026-10-09T00:00:00Z");
    }
}
