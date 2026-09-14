//! Who last changed each line, and how long ago.
//!
//! A walk of history, which is why it happens on a thread and why the answer
//! is about the *committed* file: blame is a question about commits, and the
//! lines a reader has changed since are not in any of them.

use std::{collections::HashMap, path::Path, sync::mpsc::Sender, time::SystemTime};

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
    /// Which commit wrote it, for going there and asking why.
    pub id: gix::ObjectId,
    /// Which line this was in that commit's version of the file.
    ///
    /// Not the line it is on now: everything added above it since has
    /// pushed it down. Kept per line rather than worked out later, because
    /// the blame is the only thing that knows, and it knows while it is
    /// being read.
    pub line: u32,
}

/// Which line of the blamed version a line of the text on screen is.
///
/// One implementation, because two callers need it -- the margin, to write
/// a name beside a line, and the key that opens the commit that wrote it --
/// and two would drift into disagreeing about which line a name belongs to.
///
/// `blamed_here` says the text on screen *is* what was blamed, which is
/// true of a commit's own version: its lines line up. A file on disk has
/// moved on from the commit it was blamed at, so its lines are carried back
/// through the changes, or everything added since shifts every name below
/// it.
#[must_use]
pub fn line_of(
    line: crate::coordinates::LineNumber,
    changes: Option<&crate::git::Changes>,
    blamed_here: bool,
) -> Option<crate::coordinates::LineNumber> {
    match changes {
        Some(changes) if !blamed_here => changes.committed_line(line),
        _ => Some(line),
    }
}

/// Blames a version of a file on its own thread.
///
/// One question per version, because that is what a blame is: a whole file
/// at once, from one walk of the history that reaches it. `at` is the commit
/// to look back from, or `None` for `HEAD` -- a file and that same file as
/// some commit had it are two different questions with two different
/// answers, and they share a path.
///
/// The answer arrives as an event like a language server's would, and says
/// what it is about, because the reader may be somewhere else by then.
pub fn spawn_blame(path: &Path, at: Option<gix::ObjectId>, sender: Sender<Event>) {
    let path = path.to_path_buf();
    let outcome = std::thread::Builder::new()
        .name("obelus-blame".to_string())
        .spawn(move || {
            let lines = lines_of(&path, at).unwrap_or_default();
            let _ = sender.send(Event::Blamed { path, at, lines });
        });
    if let Err(error) = outcome {
        tracing::warn!(%error, "not blaming");
    }
}

/// Who changed each line of a version of a file.
///
/// One entry per line of the file *as `at` had it*, from its first line --
/// or as the last commit has it, where `at` is `None`. `None` for a line no
/// commit accounts for, which the blame can return for a file that is only
/// partly in history.
///
/// `None` overall for every ordinary way this has no answer: not a
/// repository, a file git has never seen, a repository with no commits.
#[must_use]
pub fn lines_of(path: &Path, at: Option<gix::ObjectId>) -> Option<Vec<Option<Blamed>>> {
    let repository = super::repository(path)?;
    let relative = super::in_repository(&repository, path)?;
    let relative = gix::path::into_bstr(relative.as_path());
    let from = match at {
        Some(id) => id,
        None => repository.head_id().ok()?.detach(),
    };

    let outcome = repository
        .blame_file(
            relative.as_ref(),
            from,
            gix::repository::blame_file::Options::default(),
        )
        .ok()?;

    // One lookup per commit rather than per hunk: a file whose every line
    // came from the same commit would otherwise decode it once a hunk.
    let mut authors: HashMap<gix::ObjectId, Option<(String, i64)>> = HashMap::new();
    let mut lines: Vec<Option<Blamed>> = Vec::new();
    for entry in &outcome.entries {
        let author = authors
            .entry(entry.commit_id)
            .or_insert_with(|| {
                let commit = repository.find_commit(entry.commit_id).ok()?;
                let author = commit.author().ok()?;
                Some((author.name.to_string(), author.time().ok()?.seconds))
            })
            .clone();

        let start = entry.start_in_blamed_file as usize;
        let end = start + entry.len.get() as usize;
        if lines.len() < end {
            lines.resize(end, None);
        }
        // Per line rather than per run, because where a line sits in the
        // commit that wrote it is its own: the run began somewhere else in
        // that file, and every line of it has moved by the same amount.
        for (offset, line) in lines[start..end].iter_mut().enumerate() {
            *line = author.as_ref().map(|(who, when)| Blamed {
                who: who.clone(),
                when: *when,
                id: entry.commit_id,
                line: entry.start_in_source_file + offset as u32,
            });
        }
    }
    Some(lines)
}

/// What to show at the end of a line, or nothing for a line no commit
/// accounts for.
#[must_use]
pub fn label(blamed: Option<&Blamed>, now: SystemTime) -> Option<String> {
    let blamed = blamed?;
    Some(format!(
        "{} \u{b7} {}",
        blamed.who,
        crate::git::how_long_ago(blamed.when, now)
    ))
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    /// One unit, the largest that is at least one, rounded down: this sits
    /// at the end of a line of code, where a glance is all it gets.
    #[test]
    fn a_time_is_said_in_one_unit() {
        let now = UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        let ago = |seconds: i64| crate::git::how_long_ago(1_000_000_000 - seconds, now);

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
        assert_eq!(crate::git::how_long_ago(2_000, now), "just now");
    }
}
