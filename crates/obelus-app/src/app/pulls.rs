//! The repository's open pull requests, and the review of one.
//!
//! **Asked of `gh`, not of GitHub.** Signing in is `gh`'s, and so is knowing
//! which repository on GitHub this checkout is -- a remote can be named
//! anything, point at a fork, or be one of three. Asking the API directly
//! would mean a token Obelus keeps and a guess at which remote is the one;
//! `gh` already has both answers, and the workflow a project chooses already
//! leans on it for pull requests. Where it is missing or signed out, the
//! list says which, because the reader can act on either.
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

use super::*;
use crate::event::Event;

/// What `gh` is asked for each pull request.
///
/// Named rather than left to `gh`'s default, which has no `--json` at all:
/// its table is for people and changes shape with the terminal.
///
/// The description and the counts with the rest, in the one call: the
/// preview walks with the selection, and a question to GitHub per row
/// walked would be a preview that is always arriving.
const FIELDS: &str = "number,title,author,headRefName,baseRefName,headRefOid,isDraft,\
                      reviewDecision,updatedAt,body,additions,deletions,changedFiles";

/// How many to ask for.
///
/// A limit on a list is a limit on what can be found in it, so this is
/// `gh`'s own pagination asked to go as far as a repository plausibly has
/// open, not a page.
const LIMIT: &str = "1000";

/// One open pull request, as `gh` described it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullRequest {
    /// Its number, which is what the review is claimed and kept by.
    pub number: u64,
    /// What its author called it.
    pub title: String,
    /// Who opened it, by their login.
    pub author: String,
    /// The branch it would merge.
    pub head: String,
    /// The branch it would merge into.
    pub base: String,
    /// The commit its head is at -- what a review written down was told.
    pub sha: String,
    /// Whether it is a draft.
    pub draft: bool,
    /// What the reviews on GitHub have come to, where they have come to
    /// anything.
    pub decision: Option<Decision>,
    /// When it last changed, as seconds since the epoch.
    pub updated: Option<i64>,
    /// What its author wrote about it, as the markdown they wrote.
    pub body: String,
    /// How many lines it adds.
    pub additions: u64,
    /// How many it takes away.
    pub deletions: u64,
    /// How many files it changes.
    pub files: u64,
}

/// What the reviews of a pull request have come to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Approved.
    Approved,
    /// Changes asked for.
    ChangesRequested,
}

/// Why there is no list of pull requests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unlisted {
    /// `gh` is not on this machine.
    NoGh,
    /// It is, and is not signed in.
    SignedOut,
    /// It answered with an error of its own, which is its first line.
    Failed(String),
}

impl Unlisted {
    /// What the empty list says.
    fn said(&self) -> String {
        match self {
            Self::NoGh => "Listing pull requests needs gh, which is not installed".to_string(),
            Self::SignedOut => "Not signed in to GitHub: gh auth login signs in".to_string(),
            Self::Failed(why) => format!("GitHub would not answer: {why}"),
        }
    }
}

/// What has happened on a pull request since it was opened: how its checks
/// stand, and what has been said on it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Discussion {
    /// Every check, as the last commit has them.
    pub checks: Vec<Check>,
    /// What has been said on it -- comments, and reviews that said
    /// something of their own -- newest first.
    pub said: Vec<Comment>,
}

/// One check on a pull request's last commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Check {
    /// What it is called: a job's name, or a status's context.
    pub name: String,
    /// Where it stands.
    pub stands: Stands,
}

/// Where a check stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stands {
    /// It passed.
    Passed,
    /// It failed, timed out, was cancelled or wants something done.
    Failed,
    /// It has not finished.
    Running,
    /// It was skipped, or finished saying nothing either way.
    Skipped,
}

/// One thing said on a pull request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    /// Who said it, by their login.
    pub author: String,
    /// What they did by saying it.
    pub did: Did,
    /// When, as seconds since the epoch.
    pub when: Option<i64>,
    /// What they wrote, as the markdown they wrote.
    pub body: String,
}

/// What a comment did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Did {
    /// Commented, in the conversation or as a review.
    Commented,
    /// Approved it.
    Approved,
    /// Asked for changes.
    RequestedChanges,
    /// A review since dismissed.
    Dismissed,
}

impl Did {
    /// What the row naming the comment says it did.
    const fn said(self) -> &'static str {
        match self {
            Self::Commented => "commented",
            Self::Approved => "approved",
            Self::RequestedChanges => "requested changes",
            Self::Dismissed => "was dismissed",
        }
    }
}

/// What this window knows about the repository's pull requests.
#[derive(Debug, Default)]
pub(super) struct Pulls {
    /// What `gh` last said, kept after the list closes: a review's opening
    /// is made from it, and so is whether the pull request has moved since.
    listed: Vec<PullRequest>,
    /// Whether `listed` is an answer at all: none of it before `gh` has
    /// first answered, and none of it after an answer that was a refusal.
    answered: bool,
    /// Whether an answer is on its way.
    asking: bool,
    /// What has happened on each pull request, as `gh` last said, kept for
    /// the window's life the way the list is: shown at once the next time
    /// the row is chosen, and asked about again under it.
    discussions: std::collections::HashMap<u64, Discussion>,
    /// Which pull requests have been asked about since the list opened, so
    /// that walking back onto a row does not ask again.
    asked_since_opening: std::collections::HashSet<u64>,
    /// Which pull request's discussion is on its way. One at a time: a
    /// reader walking the list passes rows they will not stop on.
    asking_about: Option<u64>,
    /// Why the last asking about a pull request got nothing, by its number,
    /// for a preview with nothing kept to show instead.
    refused: std::collections::HashMap<u64, Unlisted>,
    /// Why the last answer was no list, where it was.
    unlisted: Option<Unlisted>,
    /// What to run in `gh`'s place, and what to tell it first, for a test:
    /// one that ran the real `gh` would be asking GitHub about whatever
    /// directory it ran in.
    instead: Option<(std::path::PathBuf, Vec<String>)>,
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
        picker.before_typing("Filter pull requests");
        // Newest first whatever is typed: a query says which are left, and
        // a ranking would put a pull request from last year above this
        // morning's because its title scored better.
        picker.keeps_order(true);
        picker.opened_by(Command::PullRequestReview);
        // What the selection says about itself, under the list: a title is
        // a line, and which pull request to review is decided by the rest.
        picker.previews();
        self.show_list(picker);
        if !self.pulls.asking {
            self.pulls.asking = true;
            self.ask_for_pull_requests();
        }
        // Every row is worth asking about once more: what was kept is from
        // the last time the list was open.
        self.pulls.asked_since_opening.clear();
        self.show_pull_requests();
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
    pub(super) fn ask_about(&mut self, number: u64) {
        if self.pulls.asked_since_opening.contains(&number) || self.pulls.asking_about.is_some() {
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
            let answer = discussion(&root, instead, number);
            let _ = sender.send(Event::PullRequestDiscussion { number, answer });
        });
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

    /// Runs `gh` on the blocking pool, and sends what it said to the loop.
    fn ask_for_pull_requests(&self) {
        let Some(sender) = self.events.clone() else {
            // No loop to answer into: a test hands the answer over itself.
            return;
        };
        let root = self.working_directory.clone();
        let instead = self.pulls.instead.clone();
        obelus_runtime::handle().spawn_blocking(move || {
            let _ = sender.send(Event::PullRequests(list(&root, instead)));
        });
    }

    /// Takes what `gh` said, and puts it in the list if the list is up.
    pub(super) fn on_pull_requests(&mut self, answer: Result<Vec<PullRequest>, Unlisted>) {
        self.pulls.asking = false;
        // A preview is kept by its subject, and the subject is a number: the
        // same number with a new description is not a new subject, so what
        // was laid out from the last answer has to go with it.
        if let Some(previewing::Subject::PullRequest(number)) =
            self.preview.as_ref().map(previewing::Preview::subject)
        {
            let number = *number;
            self.lay_the_preview_out_again(number);
        }
        match answer {
            Ok(listed) => {
                self.pulls.listed = listed;
                self.pulls.answered = true;
                self.pulls.unlisted = None;
            }
            Err(why) => {
                tracing::info!(?why, "no list of pull requests");
                // And none of the last one: rows from an answer before this
                // one would be a list saying what was open then.
                self.pulls.listed.clear();
                self.pulls.answered = false;
                self.pulls.unlisted = Some(why);
            }
        }
        self.show_pull_requests();
    }

    /// Whether the list showing is the pull requests.
    pub(super) fn listing_pull_requests(&self) -> bool {
        self.picker
            .as_ref()
            .is_some_and(|picker| picker.opener() == Some(Command::PullRequestReview))
    }

    /// Fills the list from what `gh` last said.
    fn show_pull_requests(&mut self) {
        if !self.listing_pull_requests() {
            return;
        }
        // Who has which review, looked at now: the list is the door into
        // one, and a door that lets the reader into another window's
        // review is the thing a claim is for.
        self.reread_who_holds_what();
        let now = std::time::SystemTime::now();
        // The last answer while the next is on its way, and nothing before
        // there has been one.
        let items: Vec<PickerItem> = match self.pulls.answered {
            true => self
                .pulls
                .listed
                .iter()
                .map(|pull| self.pull_request_row(pull, now))
                .collect(),
            false => Vec::new(),
        };
        // Whatever is typed, while there is no list to type at: the waiting
        // and the refusal are facts about the world, and "No match" would be
        // a fact about a query nothing was asked of. With a list, a query
        // that matches none of it is the query's to say.
        let (empty, whatever_is_typed) = match (&self.pulls.unlisted, self.pulls.asking) {
            (_, true) => ("Still asking GitHub".to_string(), true),
            (Some(why), false) => (why.said(), true),
            (None, false) => ("No pull request is open".to_string(), false),
        };
        // And the mark that turns while the answer is on its way, which
        // every list still waiting on its rows wears: in front of that line
        // where there are no rows, and on the row under the list where the
        // last answer's are standing in.
        let filling = self
            .pulls
            .asking
            .then(|| "Asking GitHub\u{2026}".to_string());
        if let Some(picker) = self.picker.as_mut() {
            // Standing on the same pull request, wherever the new answer
            // puts it -- see `review_a_pull_request`.
            picker.renew(items, |one, other| {
                matches!(
                    (one, other),
                    (PickerValue::PullRequest(one), PickerValue::PullRequest(other)) if one == other
                )
            });
            match whatever_is_typed {
                true => picker.while_empty(&empty),
                false => picker.when_empty(&empty),
            }
            picker.filling(filling);
        }
    }

    /// One pull request as a row of the list.
    fn pull_request_row(&self, pull: &PullRequest, now: std::time::SystemTime) -> PickerItem {
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
        let number = format!("#{}", pull.number);
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
        use obelus_agent::chats::ChatId;

        let which = ChatId::PullRequest(pull.number);
        let mine = self
            .documents
            .iter()
            .flatten()
            .filter_map(Document::chat)
            .any(|talk| talk.which().as_ref() == Some(&which));
        let elsewhere = !mine && self.held_now().contains_key(&which);
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

    /// Says again, on every row of the list, who has which review.
    ///
    /// From the claims as Obelus last looked, which the watch on them keeps
    /// level; marking rather than rebuilding, so the reader's row and what
    /// they typed stay where they are.
    pub(super) fn freshen_the_pull_request_rows(&mut self) {
        if !self.listing_pull_requests() {
            return;
        }
        let now = std::time::SystemTime::now();
        let said: std::collections::HashMap<u64, Said> = self
            .pulls
            .listed
            .iter()
            .map(|pull| (pull.number, self.what_a_pull_request_row_says(pull, now)))
            .collect();
        if let Some(picker) = self.picker.as_mut() {
            picker.remark(|value| match value {
                PickerValue::PullRequest(number) => said
                    .get(number)
                    .map_or(Remark::Keep, |now| Remark::Now(now.clone())),
                _ => Remark::Keep,
            });
        }
    }

    /// What `gh` last said about one pull request.
    #[must_use]
    pub(super) fn pull_request(&self, number: u64) -> Option<&PullRequest> {
        self.pulls.listed.iter().find(|pull| pull.number == number)
    }

    /// What a pull request says about itself, laid out at `width` for the
    /// preview under the list.
    ///
    /// Two rows naming it, in the shape a commit's message names its commit
    /// -- which, who, from where to where, how long ago, and how much it
    /// changes -- and then its title and its description as the markdown
    /// they are, laid out by the same code a markdown file is.
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
        let files = match pull.files {
            1 => "1 file".to_string(),
            files => format!("{files} files"),
        };
        let mut rows = vec![
            Row::of(vec![Span::new(named, Ink::Aside)]),
            Row::of(vec![
                Span::new(format!("+{}", pull.additions), Ink::Added),
                Span::new(" ", Ink::Aside),
                Span::new(format!("\u{2212}{}", pull.deletions), Ink::Removed),
                Span::new(format!(" \u{b7} {files}"), Ink::Aside),
            ]),
            Row::default(),
        ];
        // The title as the heading it is, and the description after it as
        // its author wrote it -- a description with headings of its own
        // keeps them under this one.
        let source = format!("# {}\n\n{}", pull.title, pull.body);
        rows.extend(obelus_markdown::render(&source, width));
        if pull.body.trim().is_empty() {
            rows.push(Row::default());
            rows.push(Row::of(vec![Span::new("No description", Ink::Aside)]));
        }

        // A rule between one part and the next, the way markdown's `---`
        // is drawn: the parts are three things, not one long page.
        let rule = Row {
            spans: Vec::new(),
            rule: true,
        };
        rows.push(rule.clone());
        let Some(discussion) = self.pulls.discussions.get(&number) else {
            match self.pulls.refused.get(&number) {
                Some(why) => rows.push(Row::of(vec![Span::new(why.said(), Ink::Aside)])),
                None => {
                    let turning = rows.len();
                    // Two blanks in front, where the view puts the mark and
                    // the one blank after it.
                    rows.push(Row::of(vec![Span::new(
                        "  Asking GitHub for its checks and comments",
                        Ink::Aside,
                    )]));
                    return (rows, Some(turning));
                }
            }
            return (rows, None);
        };
        rows.extend(checks(&discussion.checks));
        rows.push(rule);
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

    /// Goes to the review of one pull request, opening one if there is none.
    ///
    /// The note's door, for a pull request: claimed before it is opened,
    /// and `false` where another Obelus has it -- the row goes dim under the
    /// reader, which is the answer.
    pub(super) fn review(&mut self, number: u64) -> bool {
        let wanted = crate::conversation::Topic::PullRequest(number);
        let at = self.documents.iter().position(|document| {
            document
                .as_ref()
                .and_then(Document::chat)
                .is_some_and(|talk| talk.topic == wanted)
        });
        let at = match at {
            Some(at) => at,
            None => {
                let which = obelus_agent::chats::ChatId::PullRequest(number);
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
        let mut did = format!("  {}", comment.did.said());
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

/// Asks `gh` for the open pull requests of the repository `root` is in.
fn list(
    root: &std::path::Path,
    instead: Option<(std::path::PathBuf, Vec<String>)>,
) -> Result<Vec<PullRequest>, Unlisted> {
    let said = gh(
        root,
        instead,
        &[
            "pr", "list", "--state", "open", "--limit", LIMIT, "--json", FIELDS,
        ],
    )?;
    read(&said)
}

/// Asks `gh` what has been said about one pull request since it was
/// opened, and how its checks stand.
fn discussion(
    root: &std::path::Path,
    instead: Option<(std::path::PathBuf, Vec<String>)>,
    number: u64,
) -> Result<Discussion, Unlisted> {
    let number = number.to_string();
    let said = gh(
        root,
        instead,
        &[
            "pr",
            "view",
            &number,
            "--json",
            "comments,reviews,statusCheckRollup",
        ],
    )?;
    read_discussion(&said)
}

/// What `gh pr view --json comments,reviews,statusCheckRollup` printed.
fn read_discussion(said: &str) -> Result<Discussion, Unlisted> {
    let read: serde_json::Value = serde_json::from_str(said)
        .map_err(|error| Unlisted::Failed(format!("its answer did not read: {error}")))?;
    let text = |value: &serde_json::Value, key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let list = |key: &str| {
        read.get(key)
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let when = |value: &serde_json::Value, key: &str| {
        text(value, key)
            .parse::<jiff::Timestamp>()
            .ok()
            .map(jiff::Timestamp::as_second)
    };
    let author = |value: &serde_json::Value| {
        value
            .get("author")
            .map(|author| text(author, "login"))
            .unwrap_or_default()
    };

    // A check run says whether it has finished and then how; a status from
    // outside Actions says both in one word. The two are told apart by
    // which they carry, not by a type name nobody promised to keep.
    let checks = list("statusCheckRollup")
        .iter()
        .map(|check| {
            let stands = match check.get("state").and_then(serde_json::Value::as_str) {
                Some("SUCCESS") => Stands::Passed,
                Some("PENDING" | "EXPECTED") => Stands::Running,
                Some(_) => Stands::Failed,
                None if text(check, "status") != "COMPLETED" => Stands::Running,
                None => match text(check, "conclusion").as_str() {
                    "SUCCESS" => Stands::Passed,
                    "SKIPPED" | "NEUTRAL" => Stands::Skipped,
                    _ => Stands::Failed,
                },
            };
            let name = match text(check, "name") {
                name if name.is_empty() => text(check, "context"),
                name => name,
            };
            Check { name, stands }
        })
        .collect();

    let mut said: Vec<Comment> = list("comments")
        .iter()
        .map(|comment| Comment {
            author: author(comment),
            did: Did::Commented,
            when: when(comment, "createdAt"),
            body: text(comment, "body").replace("\r\n", "\n"),
        })
        .collect();
    // A review with nothing written in it is the envelope a comment on a
    // line came in, and those are not shown here.
    said.extend(
        list("reviews")
            .iter()
            .filter(|review| !text(review, "body").trim().is_empty())
            .map(|review| Comment {
                author: author(review),
                did: match text(review, "state").as_str() {
                    "APPROVED" => Did::Approved,
                    "CHANGES_REQUESTED" => Did::RequestedChanges,
                    "DISMISSED" => Did::Dismissed,
                    _ => Did::Commented,
                },
                when: when(review, "submittedAt"),
                body: text(review, "body").replace("\r\n", "\n"),
            }),
    );
    // Newest first: what is being looked for is what happened last.
    said.sort_by_key(|comment| std::cmp::Reverse(comment.when));
    Ok(Discussion { checks, said })
}

/// Runs `gh` with `asked`, in the repository `root` is in, and hands back
/// what it printed -- or why there is nothing.
fn gh(
    root: &std::path::Path,
    instead: Option<(std::path::PathBuf, Vec<String>)>,
    asked: &[&str],
) -> Result<String, Unlisted> {
    let (gh, mut arguments) = match instead {
        Some(instead) => instead,
        None => (
            obelus_program::found("gh").ok_or(Unlisted::NoGh)?,
            Vec::new(),
        ),
    };
    arguments.extend(asked.iter().map(|word| (*word).to_string()));
    let (program, arguments) = obelus_program::as_started_here(&gh, &arguments);
    let mut command = std::process::Command::new(program);
    command
        .args(arguments)
        .current_dir(root)
        // Nothing to answer a question with: a `gh` that wants to ask
        // something fails instead, and says why.
        .env("GH_PROMPT_DISABLED", "1")
        .stdin(std::process::Stdio::null());
    obelus_program::without_a_window(&mut command);
    let output = command
        .output()
        .map_err(|error| Unlisted::Failed(error.to_string()))?;
    if !output.status.success() {
        let said = String::from_utf8_lossy(&output.stderr);
        // `gh` exits 4 when it needs signing in, which is the one failure
        // with a key the reader can press about it. By the code and not by
        // the words: a checkout whose remotes are not on GitHub at all is
        // also told to `gh auth login`, and exits 1 -- signing in is not
        // what that reader is missing.
        if output.status.code() == Some(4) {
            return Err(Unlisted::SignedOut);
        }
        let first = said.lines().find(|line| !line.trim().is_empty());
        return Err(Unlisted::Failed(
            first.unwrap_or("it gave no reason").trim().to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// What `gh pr list --json` printed, as pull requests.
fn read(said: &str) -> Result<Vec<PullRequest>, Unlisted> {
    let rows: Vec<serde_json::Value> = serde_json::from_str(said)
        .map_err(|error| Unlisted::Failed(format!("its answer did not read: {error}")))?;
    let text = |row: &serde_json::Value, key: &str| {
        row.get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let mut pulls: Vec<PullRequest> = rows
        .iter()
        .filter_map(|row| {
            let count = |key: &str| {
                row.get(key)
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0)
            };
            Some(PullRequest {
                number: row.get("number")?.as_u64()?,
                title: text(row, "title"),
                author: row
                    .get("author")
                    .and_then(|author| author.get("login"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                head: text(row, "headRefName"),
                base: text(row, "baseRefName"),
                sha: text(row, "headRefOid"),
                draft: row
                    .get("isDraft")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                decision: match text(row, "reviewDecision").as_str() {
                    "APPROVED" => Some(Decision::Approved),
                    "CHANGES_REQUESTED" => Some(Decision::ChangesRequested),
                    _ => None,
                },
                updated: text(row, "updatedAt")
                    .parse::<jiff::Timestamp>()
                    .ok()
                    .map(jiff::Timestamp::as_second),
                // As `\n`, which is what the markdown is laid out by: a
                // description written in GitHub's own box arrives with
                // `\r\n`, and the `\r` left on each line was drawn as a
                // blank and hid a line ending in two spaces from the break
                // it asks for.
                body: text(row, "body").replace("\r\n", "\n"),
                additions: count("additions"),
                deletions: count("deletions"),
                files: count("changedFiles"),
            })
        })
        .collect();
    // Newest first, by when each last changed rather than by number: a pull
    // request somebody pushed to this morning is the one being looked for.
    pulls.sort_by_key(|pull| std::cmp::Reverse(pull.updated));
    Ok(pulls)
}

#[cfg(test)]
mod tests {
    use super::{Decision, read};

    /// What `gh` prints reads as the pull requests it describes, newest
    /// first.
    ///
    /// Broken deliberately by reading `headRefOid` from `headRefName`: the
    /// commit a review is told about becomes a branch name.
    #[test]
    fn what_gh_prints_reads_as_pull_requests() {
        let said = r#"[
            {"number":7,"title":"Older","author":{"login":"bob"},"headRefName":"old","baseRefName":"master","headRefOid":"aaa","isDraft":true,"reviewDecision":"APPROVED","updatedAt":"2026-10-01T00:00:00Z"},
            {"number":9,"title":"Newer","author":{"login":"alice"},"headRefName":"new","baseRefName":"master","headRefOid":"bbb","isDraft":false,"reviewDecision":"","updatedAt":"2026-10-08T00:00:00Z"}
        ]"#;
        let pulls = read(said).expect("it reads");
        let numbers: Vec<u64> = pulls.iter().map(|pull| pull.number).collect();
        assert_eq!(numbers, [9, 7], "not newest first");
        assert_eq!(pulls[0].sha, "bbb");
        assert_eq!(pulls[0].author, "alice");
        assert_eq!(pulls[0].decision, None);
        assert_eq!(pulls[1].decision, Some(Decision::Approved));
        assert!(pulls[1].draft);
    }

    /// A description written in GitHub's own box arrives with `\r\n`, and
    /// is read as the `\n` the markdown is laid out by.
    ///
    /// Broken deliberately by taking the `replace` out of `read`: the `\r`
    /// stays at the end of every line.
    #[test]
    fn a_description_reads_with_its_lines_ended_as_markdown_ends_them() {
        let said = r#"[{"number":1,"title":"t","author":{"login":"a"},"headRefName":"h","baseRefName":"b","headRefOid":"s","isDraft":false,"reviewDecision":"","updatedAt":"","body":"one  \r\ntwo\r\n"}]"#;
        let pulls = read(said).expect("it reads");
        assert_eq!(pulls[0].body, "one  \ntwo\n");
    }

    /// What `gh pr view` prints reads as the checks and what was said:
    /// a check run by its status and then its conclusion, a status from
    /// outside Actions by its one word, comments and reviews together and
    /// newest first, and a review that said nothing of its own left out.
    ///
    /// Broken deliberately three ways, each failing its own assertion.
    /// Reading a check run's conclusion before its status calls one still
    /// running failed. Keeping reviews with no words of their own puts an
    /// empty comment in the list. And sorting oldest first puts the
    /// approval last.
    #[test]
    fn what_gh_says_about_one_pull_request_reads_as_its_discussion() {
        use super::{Did, Stands, read_discussion};

        let said = r#"{
            "statusCheckRollup": [
                {"__typename":"CheckRun","name":"fmt","status":"COMPLETED","conclusion":"SUCCESS"},
                {"__typename":"CheckRun","name":"test","status":"COMPLETED","conclusion":"FAILURE"},
                {"__typename":"CheckRun","name":"build","status":"IN_PROGRESS","conclusion":""},
                {"__typename":"CheckRun","name":"docs","status":"COMPLETED","conclusion":"SKIPPED"},
                {"__typename":"StatusContext","context":"ci/legacy","state":"PENDING"}
            ],
            "comments": [
                {"author":{"login":"bob"},"createdAt":"2026-10-01T00:00:00Z","body":"first\r\nsecond"}
            ],
            "reviews": [
                {"author":{"login":"alice"},"state":"APPROVED","submittedAt":"2026-10-03T00:00:00Z","body":"good"},
                {"author":{"login":"bob"},"state":"COMMENTED","submittedAt":"2026-10-02T00:00:00Z","body":""}
            ]
        }"#;
        let discussion = read_discussion(said).expect("it reads");
        let stands: Vec<(&str, Stands)> = discussion
            .checks
            .iter()
            .map(|check| (check.name.as_str(), check.stands))
            .collect();
        assert_eq!(
            stands,
            [
                ("fmt", Stands::Passed),
                ("test", Stands::Failed),
                ("build", Stands::Running),
                ("docs", Stands::Skipped),
                ("ci/legacy", Stands::Running),
            ]
        );
        let said: Vec<(&str, Did, &str)> = discussion
            .said
            .iter()
            .map(|comment| (comment.author.as_str(), comment.did, comment.body.as_str()))
            .collect();
        assert_eq!(
            said,
            [
                ("alice", Did::Approved, "good"),
                ("bob", Did::Commented, "first\nsecond"),
            ]
        );
    }
}
