//! Who last changed each line, and how long ago.
//!
//! A walk of history, which is why it happens on a thread and why the answer
//! is about the *committed* file: blame is a question about commits, and the
//! lines a reader has changed since are not in any of them.

use std::{
    collections::HashMap,
    path::Path,
    sync::mpsc::Sender,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::event::Event;

/// Who introduced a line, and when.
///
/// The name and the time only: what the margin says is "who, and how long
/// ago", and a commit id or a subject line is a different view's answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blamed {
    /// The author's name, as they wrote it in the commit.
    pub who: String,
    /// When they wrote it, in seconds since the epoch.
    pub when: i64,
}

/// Blames a file on its own thread.
///
/// One question per file, because that is what a blame is: the whole file at
/// once, from one walk of the history that reaches it. The answer arrives as
/// an event like a language server's would, and the path comes back with it
/// because the reader may be somewhere else by then.
pub fn spawn_blame(path: &Path, sender: Sender<Event>) {
    let path = path.to_path_buf();
    let outcome = std::thread::Builder::new()
        .name("obelus-blame".to_string())
        .spawn(move || {
            let lines = lines_of(&path).unwrap_or_default();
            let _ = sender.send(Event::Blamed { path, lines });
        });
    if let Err(error) = outcome {
        tracing::warn!(%error, "not blaming");
    }
}

/// Who changed each line of the file as the last commit has it.
///
/// One entry per line of the *committed* file, from its first line. `None`
/// for a line no commit accounts for, which the blame can return for a file
/// that is only partly in history.
///
/// `None` overall for every ordinary way this has no answer: not a
/// repository, a file git has never seen, a repository with no commits.
#[must_use]
pub fn lines_of(path: &Path) -> Option<Vec<Option<Blamed>>> {
    let repository = super::repository(path)?;
    let relative = super::in_repository(&repository, path)?;
    let relative = gix::path::into_bstr(relative.as_path());
    let head = repository.head_id().ok()?;

    let outcome = repository
        .blame_file(
            relative.as_ref(),
            head,
            gix::repository::blame_file::Options::default(),
        )
        .ok()?;

    // One lookup per commit rather than per hunk: a file whose every line
    // came from the same commit would otherwise decode it once a hunk.
    let mut authors: HashMap<gix::ObjectId, Option<Blamed>> = HashMap::new();
    let mut lines: Vec<Option<Blamed>> = Vec::new();
    for entry in &outcome.entries {
        let blamed = authors
            .entry(entry.commit_id)
            .or_insert_with(|| {
                let commit = repository.find_commit(entry.commit_id).ok()?;
                let author = commit.author().ok()?;
                Some(Blamed {
                    who: author.name.to_string(),
                    when: author.time().ok()?.seconds,
                })
            })
            .clone();

        let start = entry.start_in_blamed_file as usize;
        let end = start + entry.len.get() as usize;
        if lines.len() < end {
            lines.resize(end, None);
        }
        for line in &mut lines[start..end] {
            *line = blamed.clone();
        }
    }
    Some(lines)
}

/// How long ago something happened, in the fewest words that are true.
///
/// One unit, always the largest that gives a number of at least one: "3
/// days" rather than "3 days 4 hours", because this sits at the end of a
/// line of code and its job is to be readable at a glance rather than
/// precise. Rounded down, the way people say it.
#[must_use]
pub fn how_long_ago(when: i64, now: SystemTime) -> String {
    let now = now
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64);
    let seconds = now.saturating_sub(when);
    // A commit from the future is a clock that disagrees, not a fact about
    // the file. "Just now" is the least wrong thing to say about it.
    if seconds < 60 {
        return "just now".to_string();
    }
    for (unit, name) in [
        (60 * 60 * 24 * 365, "year"),
        (60 * 60 * 24 * 30, "month"),
        (60 * 60 * 24 * 7, "week"),
        (60 * 60 * 24, "day"),
        (60 * 60, "hour"),
        (60, "minute"),
    ] {
        let count = seconds / unit;
        if count >= 1 {
            let plural = if count == 1 { "" } else { "s" };
            return format!("{count} {name}{plural} ago");
        }
    }
    "just now".to_string()
}

/// What to show at the end of a line, or nothing for a line no commit
/// accounts for.
#[must_use]
pub fn label(blamed: Option<&Blamed>, now: SystemTime) -> Option<String> {
    let blamed = blamed?;
    Some(format!(
        "{} \u{b7} {}",
        blamed.who,
        how_long_ago(blamed.when, now)
    ))
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::how_long_ago;

    /// One unit, the largest that is at least one, rounded down: this sits
    /// at the end of a line of code, where a glance is all it gets.
    #[test]
    fn a_time_is_said_in_one_unit() {
        let now = UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        let ago = |seconds: i64| how_long_ago(1_000_000_000 - seconds, now);

        assert_eq!(ago(5), "just now");
        assert_eq!(ago(59), "just now");
        assert_eq!(ago(60), "1 minute ago");
        assert_eq!(ago(60 * 90), "1 hour ago");
        assert_eq!(ago(60 * 60 * 25), "1 day ago");
        assert_eq!(ago(60 * 60 * 24 * 9), "1 week ago");
        assert_eq!(ago(60 * 60 * 24 * 40), "1 month ago");
        assert_eq!(ago(60 * 60 * 24 * 400), "1 year ago");
        assert_eq!(ago(60 * 60 * 24 * 800), "2 years ago");
    }

    /// A commit from the future is a clock that disagrees, not a fact about
    /// the file: "in -3 days" is not something to put on a row.
    #[test]
    fn a_commit_from_the_future_is_just_now() {
        let now = UNIX_EPOCH + Duration::from_secs(1_000);
        assert_eq!(how_long_ago(2_000, now), "just now");
    }
}
