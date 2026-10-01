//! Whether a newer Obelus is out.
//!
//! Asked once, on the way up, and answered on the welcome screen: the one
//! place a reader looks at Obelus rather than at their code, and where the
//! version already is. Nothing is said anywhere else -- a reader in the
//! middle of a file has not asked to be told about Obelus.
//!
//! Once a *day*, not once a start. Several Obelus processes on one project
//! is the normal case and `ob` is started per file, so a question asked on
//! every start is a question asked dozens of times an hour -- against an
//! address that stops answering after sixty. The answer is kept on disk
//! beside the registry's, and a copy younger than a day is the answer.
//!
//! A failure says nothing on the screen. What it would say is "Obelus could
//! not find out whether it is out of date", which is not something that
//! went wrong starting up, and a reader on a train would be told it every
//! time. The log has it.

use std::{
    path::PathBuf,
    sync::mpsc::Sender,
    time::{Duration, SystemTime},
};

use super::App;
use crate::event::Event;

/// Where the newest release is said.
///
/// GitHub's own answer, which leaves out drafts and pre-releases: a
/// reader on a release has not asked to be offered a release candidate.
const URL: &str = "https://api.github.com/repos/obelus-editor/obelus/releases/latest";

/// How long an answer is good for.
const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// How long to wait for one.
///
/// Nobody is waiting: the welcome screen is drawn without it and takes it
/// when it lands. This is only how long a dead connection holds a task.
const PATIENCE: Duration = Duration::from_secs(20);

/// What the application knows about releases.
#[derive(Debug, Default)]
pub(super) struct Releases {
    /// Whether this session has asked.
    asked: bool,
    /// The newer version, once one has been heard of.
    newer: Option<String>,
}

impl App {
    /// Asks whether a newer Obelus is out, if the reader wants to know and
    /// nobody has asked yet.
    ///
    /// From [`App::start`], and again whenever the settings are applied:
    /// a reader who turns the switch on has asked now, not at the next start.
    pub(super) fn ask_about_releases(&mut self) {
        if !self.config().new_versions || self.releases.asked {
            return;
        }
        let Some(sender) = self.events.clone() else {
            return;
        };
        self.releases.asked = true;
        spawn_ask(sender);
    }

    /// Takes what the newest release is called.
    pub(super) fn on_released(&mut self, tag: &str) {
        self.releases.newer = newer(env!("CARGO_PKG_VERSION"), tag);
        if let Some(newer) = &self.releases.newer {
            tracing::info!(newer, "a newer Obelus is out");
        }
    }

    /// The version a newer Obelus is, for the welcome screen.
    ///
    /// Nothing while the switch is off, even with an answer in hand: a
    /// reader who turned it off mid-session has said they do not want to
    /// be told.
    #[must_use]
    pub fn newer_release(&self) -> Option<&str> {
        self.releases
            .newer
            .as_deref()
            .filter(|_| self.config().new_versions)
    }
}

/// Asks on the runtime, from the copy on disk where it is fresh.
fn spawn_ask(sender: Sender<Event>) {
    obelus_runtime::handle().spawn(async move {
        let tag = match recent() {
            Some(tag) => tag,
            None => match fetch().await {
                Ok(tag) => {
                    keep(&tag);
                    tag
                }
                Err(error) => {
                    tracing::info!(%error, "not finding out whether a newer Obelus is out");
                    return;
                }
            },
        };
        let _ = sender.send(Event::Released(tag));
    });
}

/// Where the last answer is kept.
fn cache() -> Option<PathBuf> {
    Some(dirs::cache_dir()?.join("obelus").join("latest-release"))
}

/// The last answer, if it is younger than a day.
///
/// The file's own time is when it was asked, which is the whole of what
/// has to be known about it besides the name.
fn recent() -> Option<String> {
    let path = cache()?;
    let age = std::fs::metadata(&path)
        .ok()?
        .modified()
        .ok()
        .and_then(|at| SystemTime::now().duration_since(at).ok())?;
    if age > DAY {
        return None;
    }
    let tag = std::fs::read_to_string(&path).ok()?.trim().to_string();
    (!tag.is_empty()).then_some(tag)
}

/// Keeps an answer for the next start.
///
/// Beside the file and renamed over it, because another Obelus may be
/// reading it at this moment and an empty file is a day of not asking.
fn keep(tag: &str) {
    let Some(path) = cache() else {
        return;
    };
    let Some(directory) = path.parent() else {
        return;
    };
    let beside = directory.join(format!("latest-release.{}", std::process::id()));
    let kept = std::fs::create_dir_all(directory)
        .and_then(|()| std::fs::write(&beside, tag))
        .and_then(|()| std::fs::rename(&beside, &path));
    if let Err(error) = kept {
        tracing::debug!(%error, "not keeping what the newest release is called");
        let _ = std::fs::remove_file(&beside);
    }
}

/// What the newest release is called, over the network.
async fn fetch() -> anyhow::Result<String> {
    let client = reqwest::Client::builder()
        .timeout(PATIENCE)
        // GitHub refuses a request that does not say who is asking.
        .user_agent(concat!("obelus/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let text = client
        .get(URL)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let document: serde_json::Value = serde_json::from_str(&text)?;
    document
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("the newest release has no tag"))
}

/// The released version, if it is newer than the running one.
///
/// Newer, not different: a build from `master` is ahead of every release,
/// and telling it about the last one would be telling it to go backwards.
/// A tag that is not a version is nothing, because a version Obelus cannot
/// order is not one it can say is newer.
fn newer(running: &str, tag: &str) -> Option<String> {
    let released = semver::Version::parse(tag.strip_prefix('v').unwrap_or(tag)).ok()?;
    let running = semver::Version::parse(running).ok()?;
    (released > running).then(|| released.to_string())
}

#[cfg(test)]
mod tests {
    use super::newer;

    /// Only a version that is higher, compared as numbers.
    ///
    /// Broken deliberately by comparing the two as strings, which says
    /// `0.9.0` is newer than `0.10.0`: the second assertion fails. And by
    /// comparing the three numbers alone, which says a release is not news
    /// to its own candidate: the last assertion fails.
    #[test]
    fn only_a_higher_version_is_newer() {
        assert_eq!(newer("0.9.0", "v0.9.1").as_deref(), Some("0.9.1"));
        assert_eq!(newer("0.9.0", "v0.10.0").as_deref(), Some("0.10.0"));
        assert_eq!(newer("0.9.0", "v1.0.0").as_deref(), Some("1.0.0"));
        assert_eq!(newer("0.10.0", "v0.9.0"), None, "an older one is not newer");
        assert_eq!(newer("0.9.0", "v0.9.0"), None, "the same one is not newer");
        assert_eq!(
            newer("0.10.0-rc.1", "v0.10.0").as_deref(),
            Some("0.10.0"),
            "a release is newer than its candidate"
        );
    }

    /// A tag Obelus cannot read says nothing rather than something wrong.
    #[test]
    fn a_tag_that_is_not_a_version_is_nothing() {
        assert_eq!(newer("0.9.0", "nightly"), None);
        assert_eq!(newer("0.9.0", "v1.0"), None);
        assert_eq!(newer("0.9.0", "v1.0.0.0"), None);
        assert_eq!(newer("0.9.0", ""), None);
    }
}
