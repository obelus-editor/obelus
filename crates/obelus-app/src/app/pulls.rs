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
use obelus_component::picker::{Marking, Picker, PickerItem, PickerLayout, PickerValue};

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

/// What this window knows about the repository's pull requests.
#[derive(Debug, Default)]
pub(super) struct Pulls {
    /// What `gh` last said, kept after the list closes: a review's opening
    /// is made from it, and so is whether the pull request has moved since.
    listed: Vec<PullRequest>,
    /// Whether an answer is on its way.
    asking: bool,
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
    /// Empty until the answer lands, rather than showing the last answer and
    /// replacing it: rows that move under the reader's selection once they
    /// have started walking it are rows they did not choose.
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
        self.show_pull_requests();
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
        if matches!(
            self.preview.as_ref().map(previewing::Preview::subject),
            Some(previewing::Subject::PullRequest(_))
        ) {
            self.preview = None;
        }
        match answer {
            Ok(listed) => {
                self.pulls.listed = listed;
                self.pulls.unlisted = None;
            }
            Err(why) => {
                tracing::info!(?why, "no list of pull requests");
                self.pulls.unlisted = Some(why);
            }
        }
        self.show_pull_requests();
    }

    /// Whether the list showing is the pull requests.
    fn listing_pull_requests(&self) -> bool {
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
        let items: Vec<PickerItem> = match self.pulls.asking {
            true => Vec::new(),
            false => self
                .pulls
                .listed
                .iter()
                .map(|pull| self.pull_request_row(pull, now))
                .collect(),
        };
        let empty = match (&self.pulls.unlisted, self.pulls.asking) {
            (_, true) => "Still asking GitHub".to_string(),
            (Some(why), false) => why.said(),
            (None, false) => "No pull request is open".to_string(),
        };
        if let Some(picker) = self.picker.as_mut() {
            picker.replace(items);
            picker.while_empty(&empty);
        }
    }

    /// One pull request as a row of the list.
    fn pull_request_row(&self, pull: &PullRequest, now: std::time::SystemTime) -> PickerItem {
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
        // The number first, where the width is taken out of the title's
        // before it is cut: it is how a pull request is found again.
        let mut trailing = format!("#{} \u{b7} {}", pull.number, pull.author);
        if let Some(updated) = pull.updated {
            trailing.push_str(&format!(
                " \u{b7} {}",
                obelus_git::how_long_ago(updated, now)
            ));
        }
        PickerItem {
            // A sentence, which loses its end where it has to lose anything.
            prose: true,
            icon: None,
            marker,
            label: pull.title.clone(),
            detail: pull.draft.then(|| "Draft".to_string()),
            trailing: Some(trailing),
            changed: None,
            value: PickerValue::PullRequest(pull.number),
            depth: 0,
            opens: None,
            status: None,
            enabled: !elsewhere,
            colours: None,
            kind: None,
            tab: None,
            section: None,
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
    pub(super) fn pull_request_reading(&self, number: u64, width: u16) -> Vec<obelus_row::Row> {
        use obelus_row::{Ink, Row, Span};

        let Some(pull) = self.pull_request(number) else {
            return Vec::new();
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
        rows
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
                    self.show_pull_requests();
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

/// Asks `gh` for the open pull requests of the repository `root` is in.
fn list(
    root: &std::path::Path,
    instead: Option<(std::path::PathBuf, Vec<String>)>,
) -> Result<Vec<PullRequest>, Unlisted> {
    let (gh, mut asked) = match instead {
        Some(instead) => instead,
        None => (
            obelus_program::found("gh").ok_or(Unlisted::NoGh)?,
            Vec::new(),
        ),
    };
    asked.extend(
        [
            "pr", "list", "--state", "open", "--limit", LIMIT, "--json", FIELDS,
        ]
        .map(str::to_string),
    );
    let (program, arguments) = obelus_program::as_started_here(&gh, &asked);
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
        // with a key the reader can press about it.
        if output.status.code() == Some(4) || said.contains("gh auth login") {
            return Err(Unlisted::SignedOut);
        }
        let first = said.lines().find(|line| !line.trim().is_empty());
        return Err(Unlisted::Failed(
            first.unwrap_or("it gave no reason").trim().to_string(),
        ));
    }
    read(&String::from_utf8_lossy(&output.stdout))
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
                body: text(row, "body"),
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
}
