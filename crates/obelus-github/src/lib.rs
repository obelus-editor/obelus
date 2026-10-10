//! The repository's open pull requests and issues, as `gh` tells them.
//!
//! **Asked of `gh`, not of GitHub.** Signing in is `gh`'s, and so is knowing
//! which repository on GitHub this checkout is -- a remote can be named
//! anything, point at a fork, or be one of three. Asking the API directly
//! would mean a token Obelus keeps and a guess at which remote is the one;
//! `gh` already has both answers.
//!
//! Its own crate rather than a part of `obelus-git`, which reads a repository
//! through gix and runs nothing: this runs a program, and what it reads is a
//! service's rather than the checkout's. Everything here waits on `gh`, so it
//! is asked from a thread; what the list makes of the answers is the
//! application's.

use std::path::{Path, PathBuf};

/// What GitHub is asked for about each pull request in the list: what a row
/// draws and what an opening tells the agent, and nothing only a preview
/// wants.
///
/// Through `gh api graphql` rather than `gh pr list`, which has no cursor
/// to carry on from and so can only ask for the first so many. `{owner}`
/// and `{repo}` are `gh`'s to fill, from the remote it would pick for `gh pr
/// view` -- which is what a preview and a review are asked of.
const PULL_FIELDS: &str = "number title author{login} headRefName baseRefName headRefOid isDraft reviewDecision updatedAt";

/// And about each issue.
const ISSUE_FIELDS: &str = "number title author{login} labels(first:100){nodes{name}} updatedAt";

/// How many rows GitHub's search gives for one query, however many it
/// counts: past this there is no next page to ask for.
pub const SEARCH_GIVES: usize = 1000;

/// How many rows a page holds.
pub const PAGE: usize = 100;

/// How many pages of what has changed are walked before the list is asked
/// for again from the top instead: a window left open for a week on a busy
/// repository has more to catch up on than a first page is worth.
const CHANGED_PAGES: usize = 3;

/// What `gh pr view` is asked about the row the reader is on: its
/// description and how much it changes, as well as what has happened on it.
///
/// Not in the list. The counts are what GitHub gave up on: asked of every
/// row of a thousand, a page took a minute and ended in a 502.
const VIEWED: &str =
    "body,additions,deletions,changedFiles,updatedAt,comments,reviews,statusCheckRollup";

/// And `gh issue view`, which has no counts and no checks to say.
const ISSUE_VIEWED: &str = "body,updatedAt,comments";

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
    /// And as GitHub says it, which what was kept about it is checked
    /// against.
    pub stamp: String,
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

/// One open issue, as `gh` described it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    /// Its number, which is what the answer is claimed and kept by.
    pub number: u64,
    /// What its author called it.
    pub title: String,
    /// Who opened it, by their login.
    pub author: String,
    /// What it has been labelled.
    pub labels: Vec<String>,
    /// When it last changed, as seconds since the epoch.
    pub updated: Option<i64>,
    /// And as GitHub says it -- what an answer written down was told, so
    /// that a comment since is something to say again.
    pub stamp: String,
}

/// What `gh` is asked about once a row is chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    /// A pull request: its checks, and what was said on it.
    PullRequest(u64),
    /// An issue: what was said on it.
    Issue(u64),
}

impl Asked {
    /// Its number, which GitHub shares between the two.
    #[must_use]
    pub const fn number(self) -> u64 {
        match self {
            Self::PullRequest(number) | Self::Issue(number) => number,
        }
    }
}

/// Which way a list is asked to grow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Toward {
    /// The newest page, from nothing.
    First,
    /// The page after the one this cursor ends.
    Older(String),
    /// Everything that has changed since this `updatedAt`.
    Newer(String),
    /// A page of what GitHub's search finds for a query.
    Found {
        /// The query.
        query: String,
        /// Which asking of it: a query typed away from and back to is asked
        /// again, and the first asking's answers are not the second's.
        asking: u64,
        /// The cursor it comes after, where there is one.
        after: Option<String>,
    },
}

/// A page of a list, as GitHub sent it.
#[derive(Debug)]
pub struct Page<T> {
    /// What it holds, newest first.
    pub rows: Vec<T>,
    /// Where the page after it starts, while there is one.
    pub older: Option<String>,
    /// How many there are in all: open, or matching.
    pub total: u64,
}

/// What one asking for a tab's list sends back.
#[derive(Debug)]
pub enum Listed<T> {
    /// The newest page, which what was listed gives way to.
    First(Page<T>),
    /// The next older page, under what is listed.
    Older(Page<T>),
    /// What has changed since the newest the list had seen.
    Changed {
        /// What is open, wherever it was before.
        open: Vec<T>,
        /// What has closed or merged since, by number.
        gone: Vec<u64>,
        /// The newest `updatedAt` among them.
        newest: String,
        /// How many are open now.
        total: u64,
    },
    /// A page of what GitHub's search found for a query.
    Found {
        /// The query.
        query: String,
        /// Which asking of it.
        asking: u64,
        /// What it found.
        page: Page<T>,
    },
    /// Why an asking got nothing.
    Refused {
        /// Which asking.
        toward: Toward,
        /// Why.
        why: Unlisted,
    },
}

/// What a pull request or an issue says beyond its row: what its author
/// wrote, how much it changes, how its checks stand, and what has been said
/// on it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Discussion {
    /// When it last changed, as GitHub says it -- which the list's word on
    /// it is checked against before asking again.
    pub stamp: String,
    /// What its author wrote, as the markdown they wrote.
    pub body: String,
    /// How many lines it adds; nothing for an issue.
    pub additions: u64,
    /// How many it takes away.
    pub deletions: u64,
    /// How many files it changes.
    pub files: u64,
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

/// The newest pull requests, or whichever page of them `toward` asks for, of
/// the repository `root` is in.
///
/// `instead` is a program to run in `gh`'s place, and the words to start it
/// with -- what a test stands in for GitHub with.
#[must_use]
pub fn pull_requests(
    root: &Path,
    instead: Option<(PathBuf, Vec<String>)>,
    toward: Toward,
) -> Listed<PullRequest> {
    fetch(root, instead, false, toward, read)
}

/// The same, for the issues.
#[must_use]
pub fn issues(
    root: &Path,
    instead: Option<(PathBuf, Vec<String>)>,
    toward: Toward,
) -> Listed<Issue> {
    fetch(root, instead, true, toward, read_issues)
}

/// Asks GitHub, through `gh`, for one tab's list of the repository `root` is
/// in to grow `toward` wherever it is asked to.
fn fetch<T>(
    root: &std::path::Path,
    instead: Option<(std::path::PathBuf, Vec<String>)>,
    issues: bool,
    toward: Toward,
    read: fn(&[serde_json::Value]) -> Vec<T>,
) -> Listed<T> {
    match fetched(root, instead, issues, &toward, read) {
        Ok(listed) => listed,
        Err(why) => Listed::Refused { toward, why },
    }
}

/// The same, with a refusal as an error.
fn fetched<T>(
    root: &std::path::Path,
    instead: Option<(std::path::PathBuf, Vec<String>)>,
    issues: bool,
    toward: &Toward,
    read: fn(&[serde_json::Value]) -> Vec<T>,
) -> Result<Listed<T>, Unlisted> {
    let (connection, fields) = match issues {
        true => ("issues", ISSUE_FIELDS),
        false => ("pullRequests", PULL_FIELDS),
    };
    let order = "orderBy:{field:UPDATED_AT,direction:DESC}";
    let repository = || {
        vec![
            ("-F", "owner={owner}".to_string()),
            ("-F", "name={repo}".to_string()),
        ]
    };
    let after = |cursor: &Option<String>| {
        cursor
            .as_ref()
            .map(|cursor| ("-f", format!("cursor={cursor}")))
    };
    // A page of what is open, after `cursor` where there is one.
    let open_page = |instead: Option<(std::path::PathBuf, Vec<String>)>, cursor: Option<String>| {
        let query = format!(
            "query($owner:String!,$name:String!,$cursor:String){{repository(owner:$owner,\
             name:$name){{list:{connection}(states:OPEN,first:{PAGE},after:$cursor,{order}){{\
             totalCount nodes{{{fields}}} pageInfo{{hasNextPage endCursor}}}}}}}}"
        );
        let mut variables = repository();
        variables.extend(after(&cursor));
        let said = asked(root, instead, &query, &variables)?;
        page(&said, "/data/repository/list", read)
    };
    match toward {
        Toward::First => Ok(Listed::First(open_page(instead, None)?)),
        Toward::Older(cursor) => Ok(Listed::Older(open_page(instead, Some(cursor.clone()))?)),
        Toward::Newer(since) => {
            // Closed and merged as well as open, because closing one is a
            // change to it: what has gone is what this is asked to find. An
            // issue can be asked for only what changed since; a pull
            // request cannot, so the walk stops at the first one older than
            // that -- the list is newest first.
            let (states, filter, declared) = match issues {
                true => (
                    String::new(),
                    "filterBy:{since:$since,states:[OPEN,CLOSED]},",
                    ",$since:DateTime",
                ),
                false => ("states:[OPEN,CLOSED,MERGED],".to_string(), "", ""),
            };
            let query = format!(
                "query($owner:String!,$name:String!,$cursor:String{declared}){{repository(\
                 owner:$owner,name:$name){{open:{connection}(states:OPEN){{totalCount}} \
                 list:{connection}({states}{filter}first:{PAGE},after:$cursor,{order}){{nodes{{\
                 state 
                 {fields}}} pageInfo{{hasNextPage endCursor}}}}}}}}"
            );
            let mut open = Vec::new();
            let mut gone = Vec::new();
            let mut newest = String::new();
            let mut total = 0;
            let mut cursor = None;
            let mut walked = 0;
            loop {
                walked += 1;
                let mut variables = repository();
                if issues {
                    variables.push(("-f", format!("since={since}")));
                }
                variables.extend(after(&cursor));
                let said = asked(root, instead.clone(), &query, &variables)?;
                total = said
                    .pointer("/data/repository/open/totalCount")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(total);
                let list = said.pointer("/data/repository/list").ok_or_else(unread)?;
                let nodes = list
                    .get("nodes")
                    .and_then(serde_json::Value::as_array)
                    .ok_or_else(unread)?;
                let mut past = false;
                for node in nodes {
                    let stamp = node
                        .get("updatedAt")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default();
                    // As strings: GitHub writes every one in the same
                    // shape, to the second and in UTC, so the order of the
                    // words is the order of the moments.
                    if stamp < since.as_str() {
                        past = true;
                        continue;
                    }
                    newest = newest.max(stamp.to_string());
                    match node.get("state").and_then(serde_json::Value::as_str) {
                        Some("OPEN") => open.push(node.clone()),
                        _ => gone.extend(node.get("number").and_then(serde_json::Value::as_u64)),
                    }
                }
                cursor = match (past, next(list)) {
                    // More changed than is worth catching up on a page at a
                    // time: the newest page instead, which the list gives way
                    // to.
                    (false, Some(_)) if walked >= CHANGED_PAGES => {
                        return Ok(Listed::First(open_page(instead, None)?));
                    }
                    (false, Some(next)) => Some(next),
                    _ => break,
                };
            }
            Ok(Listed::Changed {
                open: read(&open),
                gone,
                newest,
                total,
            })
        }
        Toward::Found {
            query: words,
            asking,
            after: cursor,
        } => {
            let (kind, on) = match issues {
                true => ("issue", "Issue"),
                false => ("pr", "PullRequest"),
            };
            let query = format!(
                "query($q:String!,$cursor:String){{search(type:ISSUE,query:$q,first:{PAGE},\
                 after:$cursor){{issueCount nodes{{... on {on}{{{fields}}}}} pageInfo{{\
                 hasNextPage endCursor}}}}}}"
            );
            let mut variables = vec![(
                "-F",
                format!(
                    "q=repo:{{owner}}/{{repo}} is:{kind} is:open sort:updated-desc {}",
                    looked_for(words)
                ),
            )];
            variables.extend(after(cursor));
            let said = asked(root, instead, &query, &variables)?;
            Ok(Listed::Found {
                query: words.clone(),
                asking: *asking,
                page: page(&said, "/data/search", read)?,
            })
        }
    }
}

/// What GitHub's search is asked for when the reader typed `words`.
///
/// By title where they typed words, which is what the list matches them
/// against -- the body and the comments would bring rows the list then
/// hides. Not where they typed a number, which `in:title` stops GitHub
/// finding by. And without braces, which `gh` would read as a place to put
/// the branch in -- failing outright on a detached `HEAD` -- and which
/// GitHub's search does not look at anyway.
fn looked_for(words: &str) -> String {
    let words: String = words.chars().filter(|c| !matches!(c, '{' | '}')).collect();
    let number = words.trim_start_matches('#');
    match !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()) {
        true => number.to_string(),
        false => format!("in:title {words}"),
    }
}

/// Runs `gh api graphql` with `query` and its `variables`, each a flag and
/// its `name=value`, and reads what it printed.
fn asked(
    root: &std::path::Path,
    instead: Option<(std::path::PathBuf, Vec<String>)>,
    query: &str,
    variables: &[(&str, String)],
) -> Result<serde_json::Value, Unlisted> {
    let query = format!("query={query}");
    let mut arguments = vec!["api", "graphql"];
    for (flag, value) in variables {
        arguments.push(flag);
        arguments.push(value);
    }
    arguments.push("-f");
    arguments.push(&query);
    let said = gh(root, instead, &arguments)?;
    serde_json::from_str(&said)
        .map_err(|error| Unlisted::Failed(format!("its answer did not read: {error}")))
}

/// The page at `at` in what GitHub said: its rows, where the next one
/// starts, and how many there are in all.
fn page<T>(
    said: &serde_json::Value,
    at: &str,
    read: fn(&[serde_json::Value]) -> Vec<T>,
) -> Result<Page<T>, Unlisted> {
    let list = said.pointer(at).ok_or_else(unread)?;
    let rows = list
        .get("nodes")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(unread)?;
    Ok(Page {
        rows: read(rows),
        older: next(list),
        total: list
            .get("totalCount")
            .or_else(|| list.get("issueCount"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
    })
}

/// Where the page after `list` starts, while there is one.
fn next(list: &serde_json::Value) -> Option<String> {
    let info = list.get("pageInfo")?;
    info.get("hasNextPage")?
        .as_bool()?
        .then(|| info.get("endCursor")?.as_str().map(str::to_string))
        .flatten()
}

/// An answer that is not the shape it was asked for -- GitHub's own error
/// is said by `gh` on its way out, so this is the rarer thing.
fn unread() -> Unlisted {
    Unlisted::Failed("its answer did not read".to_string())
}

/// A page of issues as GitHub described them, newest first.
fn read_issues(rows: &[serde_json::Value]) -> Vec<Issue> {
    let text = |row: &serde_json::Value, key: &str| {
        row.get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let mut issues: Vec<Issue> = rows
        .iter()
        .filter_map(|row| {
            let stamp = text(row, "updatedAt");
            Some(Issue {
                number: row.get("number")?.as_u64()?,
                title: text(row, "title"),
                author: row
                    .get("author")
                    .map(|author| text(author, "login"))
                    .unwrap_or_default(),
                labels: row
                    .pointer("/labels/nodes")
                    .and_then(serde_json::Value::as_array)
                    .map(|labels| {
                        labels
                            .iter()
                            .map(|label| text(label, "name"))
                            .filter(|name| !name.is_empty())
                            .collect()
                    })
                    .unwrap_or_default(),
                updated: stamp
                    .parse::<jiff::Timestamp>()
                    .ok()
                    .map(jiff::Timestamp::as_second),
                stamp,
            })
        })
        .collect();
    issues.sort_by_key(|issue| std::cmp::Reverse(issue.updated));
    issues
}

/// Asks `gh` what has been said about one pull request or issue since it
/// was opened, and how a pull request's checks stand.
pub fn discussion(
    root: &std::path::Path,
    instead: Option<(std::path::PathBuf, Vec<String>)>,
    asked: Asked,
) -> Result<Discussion, Unlisted> {
    let number = asked.number().to_string();
    let said = match asked {
        Asked::PullRequest(_) => gh(root, instead, &["pr", "view", &number, "--json", VIEWED])?,
        Asked::Issue(_) => gh(
            root,
            instead,
            &["issue", "view", &number, "--json", ISSUE_VIEWED],
        )?,
    };
    read_discussion(&said)
}

/// What `gh pr view --json` printed with [`VIEWED`] -- or `gh issue view`
/// with [`ISSUE_VIEWED`], which is the same with what an issue has nothing
/// for left out.
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
    let count = |key: &str| {
        read.get(key)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
    };
    Ok(Discussion {
        stamp: text(&read, "updatedAt"),
        // As `\n`, which is what the markdown is laid out by: a
        // description written in GitHub's own box arrives with `\r\n`, and
        // the `\r` left on each line was drawn as a blank and hid a line
        // ending in two spaces from the break it asks for.
        body: text(&read, "body").replace("\r\n", "\n"),
        additions: count("additions"),
        deletions: count("deletions"),
        files: count("changedFiles"),
        checks,
        said,
    })
}

/// Runs `gh` with `asked`, in the repository `root` is in, and hands back
/// what it printed -- or why there is nothing.
fn gh(
    root: &std::path::Path,
    instead: Option<(std::path::PathBuf, Vec<String>)>,
    asked: &[&str],
) -> Result<String, Unlisted> {
    let output = command(root, instead, asked)?
        .output()
        .map_err(|error| Unlisted::Failed(error.to_string()))?;
    if !output.status.success() {
        return Err(refusal(
            output.status,
            &String::from_utf8_lossy(&output.stderr),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `gh` with `asked`, ready to run in the repository `root` is in.
fn command(
    root: &std::path::Path,
    instead: Option<(std::path::PathBuf, Vec<String>)>,
    asked: &[&str],
) -> Result<std::process::Command, Unlisted> {
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
    Ok(command)
}

/// Why `gh` ended as it did, from how it exited and what it `said` on
/// stderr.
fn refusal(status: std::process::ExitStatus, said: &str) -> Unlisted {
    // `gh` exits 4 when it needs signing in, which is the one failure with
    // a key the reader can press about it. By the code and not by the
    // words: a checkout whose remotes are not on GitHub at all is also told
    // to `gh auth login`, and exits 1 -- signing in is not what that reader
    // is missing.
    if status.code() == Some(4) {
        return Unlisted::SignedOut;
    }
    let first = said.lines().find(|line| !line.trim().is_empty());
    // `gh api` puts where it was filling `{owner}` in front of why it could
    // not, which is about the remotes and not about a value anybody typed.
    let first = first.map(|line| line.trim_start_matches("error parsing \"owner\" value: "));
    Unlisted::Failed(first.unwrap_or("it gave no reason").trim().to_string())
}

/// A page of pull requests as GitHub described them, newest first.
fn read(rows: &[serde_json::Value]) -> Vec<PullRequest> {
    let text = |row: &serde_json::Value, key: &str| {
        row.get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let mut pulls: Vec<PullRequest> = rows
        .iter()
        .filter_map(|row| {
            let stamp = text(row, "updatedAt");
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
                updated: stamp
                    .parse::<jiff::Timestamp>()
                    .ok()
                    .map(jiff::Timestamp::as_second),
                stamp,
            })
        })
        .collect();
    // Newest first, by when each last changed rather than by number: a pull
    // request somebody pushed to this morning is the one being looked for.
    pulls.sort_by_key(|pull| std::cmp::Reverse(pull.updated));
    pulls
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page of rows, as the JSON GitHub sends them in.
    fn rows(said: &str) -> Vec<serde_json::Value> {
        serde_json::from_str(said).expect("rows")
    }

    /// A page of issues reads as the issues it describes, newest first, with
    /// their labels by name and when each was updated kept as GitHub wrote
    /// it.
    ///
    /// Broken deliberately two ways. Keeping the label objects' ids rather
    /// than their names leaves the labels empty. And keeping the stamp as
    /// seconds, as `updated` is, gives an answer a `told` that can never
    /// equal what GitHub says next.
    #[test]
    fn what_gh_prints_reads_as_issues() {
        let said = r#"[
            {"number":3,"title":"Older","author":{"login":"bob"},"labels":{"nodes":[{"id":"x","name":"bug"}]},"updatedAt":"2026-10-01T00:00:00Z"},
            {"number":5,"title":"Newer","author":{"login":"alice"},"labels":{"nodes":[]},"updatedAt":"2026-10-08T00:00:00Z"}
        ]"#;
        let issues = read_issues(&rows(said));
        let numbers: Vec<u64> = issues.iter().map(|issue| issue.number).collect();
        assert_eq!(numbers, [5, 3], "not newest first");
        assert_eq!(issues[1].labels, ["bug"]);
        assert_eq!(issues[1].stamp, "2026-10-01T00:00:00Z");
        assert_eq!(issues[0].author, "alice");
    }

    /// A page of pull requests reads as the pull requests it describes,
    /// newest first.
    ///
    /// Broken deliberately by reading `headRefOid` from `headRefName`: the
    /// commit a review is told about becomes a branch name.
    #[test]
    fn what_gh_prints_reads_as_pull_requests() {
        let said = r#"[
            {"number":7,"title":"Older","author":{"login":"bob"},"headRefName":"old","baseRefName":"master","headRefOid":"aaa","isDraft":true,"reviewDecision":"APPROVED","updatedAt":"2026-10-01T00:00:00Z"},
            {"number":9,"title":"Newer","author":{"login":"alice"},"headRefName":"new","baseRefName":"master","headRefOid":"bbb","isDraft":false,"reviewDecision":"","updatedAt":"2026-10-08T00:00:00Z"}
        ]"#;
        let pulls = read(&rows(said));
        let numbers: Vec<u64> = pulls.iter().map(|pull| pull.number).collect();
        assert_eq!(numbers, [9, 7], "not newest first");
        assert_eq!(pulls[0].sha, "bbb");
        assert_eq!(pulls[0].author, "alice");
        assert_eq!(pulls[0].decision, None);
        assert_eq!(pulls[1].decision, Some(Decision::Approved));
        assert!(pulls[1].draft);
    }

    /// What GitHub's search is asked: by title for words, not for a
    /// number, and never with a brace `gh` would try to fill.
    ///
    /// Broken deliberately by leaving the braces in.
    #[test]
    fn a_query_is_asked_by_title_without_braces() {
        use super::looked_for;

        assert_eq!(looked_for("fold"), "in:title fold");
        assert_eq!(looked_for("#130"), "130");
        assert_eq!(looked_for("{branch} fold"), "in:title branch fold");
    }

    /// A description written in GitHub's own box arrives with `\r\n`, and
    /// is read as the `\n` the markdown is laid out by.
    ///
    /// Broken deliberately by taking the `replace` out of `read_discussion`:
    /// the `\r` stays at the end of every line.
    #[test]
    fn a_description_reads_with_its_lines_ended_as_markdown_ends_them() {
        let said = r#"{"body":"one  \r\ntwo\r\n"}"#;
        let discussion = super::read_discussion(said).expect("it reads");
        assert_eq!(discussion.body, "one  \ntwo\n");
    }

    /// What `gh pr view` prints reads as how much it changes, when it last
    /// changed, the checks and what was said: a check run by its status and
    /// then its conclusion, a status from outside Actions by its one word,
    /// comments and reviews together and newest first, and a review that
    /// said nothing of its own left out.
    ///
    /// Broken deliberately four ways, each failing its own assertion.
    /// Reading a check run's conclusion before its status calls one still
    /// running failed. Keeping reviews with no words of their own puts an
    /// empty comment in the list. Sorting oldest first puts the approval
    /// last. And reading `changedFiles` as `files`, the name Obelus keeps
    /// it by, counts none.
    #[test]
    fn what_gh_says_about_one_pull_request_reads_as_its_discussion() {
        use super::{Did, Stands, read_discussion};

        let said = r#"{
            "updatedAt": "2026-10-04T00:00:00Z",
            "additions": 142, "deletions": 18, "changedFiles": 12,
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
        assert_eq!(
            (
                discussion.stamp.as_str(),
                discussion.additions,
                discussion.deletions,
                discussion.files
            ),
            ("2026-10-04T00:00:00Z", 142, 18, 12)
        );
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
