//! The themes registry: what it lists, and the themes taken from it.
//!
//! One document, `themes/index.json`, published with the packages it names
//! by the `obelus-editor/registry` repository -- which keeps a directory for
//! each kind of thing it lists, and themes are the first: the themes written
//! for Obelus, and
//! omarchy's own, turned into Obelus's by the repository's CI through the
//! same template omarchy renders Obelus's theme with. Obelus reads that one
//! document and knows nothing of how either group was made.
//!
//! **Fetched when the reader opens the page, and kept.** The way the agents'
//! registry is: what was kept from last time arrives first, so a page opened
//! with no network still lists what it listed, and the fetch replaces it
//! when it lands. Never on a clock -- a theme published while nobody is
//! looking is found the next time somebody looks.
//!
//! **Read field by field, and an entry Obelus cannot use is left out.** The
//! document is the registry's and it will grow: an older Obelus reading a
//! newer index should list the themes it understands, not refuse the page
//! over one it does not.
//!
//! A theme taken from it is a directory of its own under
//! [`root`] -- not beside the reader's settings, where their own themes are
//! and where a theme of theirs by the same name goes first.

pub mod install;

use std::path::{Path, PathBuf};

use obelus_sink::Sink;
use serde_json::Value;

/// Where the index is published.
pub const URL: &str = "https://obelus-editor.github.io/registry/themes/index.json";

/// How long to wait for it.
///
/// The same as the agents' registry, for the same reason: a page still
/// saying "fetching" after this has told the reader what they need to know.
const PATIENCE: std::time::Duration = std::time::Duration::from_secs(20);

/// How much of it to read: room for thousands of themes, and a limit on
/// what a wrong address can do to memory.
const MOST: u64 = 1024 * 1024;

/// Something Obelus found out about the registry.
#[derive(Debug)]
pub enum Event {
    /// The index, from the disk or from the network.
    Index {
        /// Every theme it lists that Obelus can make sense of.
        themes: Vec<Listed>,
        /// Why nothing was fetched, where nothing was.
        failure: Option<String>,
    },
    /// How far an install has got.
    Installing {
        /// Which theme, by its name.
        name: String,
        /// What is known about how far along it is.
        progress: install::Progress,
    },
    /// An install is over: done, or why not.
    Installed {
        /// Which theme, by its name.
        name: String,
        /// Why it is not installed, where it is not.
        failure: Option<String>,
    },
}

/// Which of the registry's two shelves a theme is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Group {
    /// Written for Obelus.
    Obelus,
    /// omarchy's own, turned into Obelus's.
    Omarchy,
}

impl Group {
    /// Both, in the order they are shown: Obelus's own first.
    pub const ALL: [Self; 2] = [Self::Obelus, Self::Omarchy];

    /// The word the index writes it as.
    fn named(word: &str) -> Option<Self> {
        match word {
            "obelus" => Some(Self::Obelus),
            "omarchy" => Some(Self::Omarchy),
            _ => None,
        }
    }

    /// And the word, the other way.
    const fn word(self) -> &'static str {
        match self {
            Self::Obelus => "obelus",
            Self::Omarchy => "omarchy",
        }
    }

    /// What the page calls it, as a heading: a name, written the way it is
    /// written everywhere.
    #[must_use]
    pub const fn heading(self) -> &'static str {
        match self {
            Self::Obelus => "Obelus",
            Self::Omarchy => "omarchy",
        }
    }
}

/// One theme as the page shows it: what the registry says, beside what is
/// true on this machine.
#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    /// What the registry lists -- or, for a theme it no longer lists, what
    /// is known of it here: its name and its group.
    pub theme: Listed,
    /// Where it stands.
    pub status: Status,
    /// Whether it is installed -- which an update that did not take leaves
    /// it, whatever its status says about the last try.
    pub installed: bool,
    /// Whether it is the theme being worn.
    pub worn: bool,
    /// Whether a theme of the reader's own by the same name is the one
    /// that answers to it, so this one is installed and not the one worn.
    pub shadowed: bool,
}

impl Card {
    /// Whether its theme may be taken away: installed, not on its way in,
    /// and not the one being worn -- a theme taken away from under the
    /// reader is the screen changing to one nobody chose.
    ///
    /// The one answer, for the key that does it and the foot that offers
    /// it: a key the foot does not show has to be a key that does nothing.
    #[must_use]
    pub fn may_be_taken_away(&self) -> bool {
        self.installed && !self.worn && !matches!(self.status, Status::Installing(_))
    }

    /// Whether pressing it would install it, or update it: not installed,
    /// or behind, or tried and not taken -- and not on its way already, nor
    /// one the registry no longer has to give.
    #[must_use]
    pub const fn may_be_installed(&self) -> bool {
        matches!(
            self.status,
            Status::Missing | Status::Outdated | Status::Failed(_)
        )
    }
}

/// Where a theme the page shows stands, on this machine.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// Not installed.
    Missing,
    /// Installed, and the one listed.
    Installed,
    /// Installed, and the registry lists a newer one.
    Outdated,
    /// On its way, and how far it has got where that is known.
    Installing(Option<install::Progress>),
    /// The last install did not take, and why.
    Failed(String),
    /// Installed, and the registry no longer lists it.
    Delisted,
}

/// One theme the registry lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    /// The name it answers to, which is the name it is installed under.
    pub name: String,
    /// Which shelf it is on.
    pub group: Group,
    /// Whether it is a dark theme or a light one.
    pub dark: bool,
    /// Who made it.
    pub author: String,
    /// A line about it.
    pub description: String,
    /// The picture it carries, where it carries one: on what terms, and
    /// where it came from.
    pub wallpaper: Option<Credit>,
    /// Where its package is, whole.
    pub package: String,
    /// What the package's bytes hash to, which is what says it is the
    /// package the registry describes.
    pub sha256: String,
    /// The commit it was packed from, which is what says whether the one
    /// installed is the one listed.
    pub commit: String,
    /// When that commit was made, as the registry writes a day.
    pub updated: String,
}

/// Where a picture came from, and on what terms it is handed on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Credit {
    /// Its licence, as an SPDX identifier.
    pub license: String,
    /// Where it came from.
    pub source: String,
}

/// The themes in an index document published at `published`.
///
/// A package named relative to the document is relative to where the
/// document is, which is how the registry writes them: one directory, so a
/// mirror of it works wherever it is put.
#[must_use]
pub fn themes_in(text: &str, published: &str) -> Vec<Listed> {
    let Ok(document) = serde_json::from_str::<Value>(text) else {
        tracing::warn!("the themes index is not json");
        return Vec::new();
    };
    let Some(entries) = document.get("themes").and_then(Value::as_array) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| theme(entry, published))
        .collect()
}

/// One entry, or nothing if it is missing what a card and an install need.
fn theme(entry: &Value, published: &str) -> Option<Listed> {
    let text = |key: &str| entry.get(key).and_then(Value::as_str);
    let name = text("name")?;
    // The directory it goes in is named for it, so the name is held to what
    // a directory may safely be called: nothing that climbs out, hides, or
    // means something to a shell.
    if !named_safely(name) {
        tracing::warn!(
            name,
            "a theme in the index with a name Obelus will not install under"
        );
        return None;
    }
    let wallpaper = entry.get("wallpaper").and_then(|credit| {
        Some(Credit {
            license: credit.get("license")?.as_str()?.to_string(),
            source: credit.get("source")?.as_str()?.to_string(),
        })
    });
    Some(Listed {
        name: name.to_string(),
        group: Group::named(text("group")?)?,
        dark: match text("mode")? {
            "dark" => true,
            "light" => false,
            _ => return None,
        },
        author: text("author").unwrap_or_default().to_string(),
        description: text("description").unwrap_or_default().to_string(),
        wallpaper,
        package: beside(published, text("package")?),
        sha256: text("sha256")?.to_ascii_lowercase(),
        commit: text("commit")?.to_string(),
        updated: text("updated").unwrap_or_default().to_string(),
    })
}

/// Whether a name may be a directory of its own under [`root`].
#[must_use]
pub fn named_safely(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// `name` where it is relative to a document published at `published`, or
/// as it is where it is already whole.
fn beside(published: &str, name: &str) -> String {
    if name.contains("://") {
        return name.to_string();
    }
    match published.rsplit_once('/') {
        Some((directory, _)) => format!("{directory}/{name}"),
        None => name.to_string(),
    }
}

/// Where the copy of the index is kept between sessions.
#[must_use]
pub fn cache() -> Option<PathBuf> {
    Some(dirs::cache_dir()?.join("obelus").join("themes.json"))
}

/// Where the themes taken from the registry are kept, a directory each.
///
/// The data directory, and a name of its own: the settings' directory is
/// the same directory on macOS and Windows, and the reader's own themes are
/// in `themes` beside the settings -- so `themes` here would put a download
/// among the reader's own, where it could not be told from them.
#[must_use]
pub fn root() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("obelus").join("installed-themes"))
}

/// What is on disk from a previous session, if anything.
fn cached() -> Vec<Listed> {
    let Some(path) = cache() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    themes_in(&text, URL)
}

/// Reads the index off the thread that asked: the kept copy, then the
/// network.
///
/// Both there, the read from disk included, for the reason the agents'
/// registry gives: the frame that opens the settings is not the place for
/// a file read. A failure comes back as one, beside whatever was kept.
pub fn spawn_fetch(sender: impl Sink<Event>) {
    obelus_runtime::handle().spawn(async move {
        let cached = cached();
        if !cached.is_empty()
            && sender
                .send(Event::Index {
                    themes: cached,
                    failure: None,
                })
                .is_err()
        {
            return;
        }
        match fetch().await {
            Ok(text) => {
                keep(&text);
                let _ = sender.send(Event::Index {
                    themes: themes_in(&text, URL),
                    failure: None,
                });
            }
            Err(error) => {
                tracing::warn!(%error, "not fetching the themes index");
                let _ = sender.send(Event::Index {
                    themes: Vec::new(),
                    failure: Some(error.to_string()),
                });
            }
        }
    });
}

/// Keeps a fetched index for the next session.
fn keep(text: &str) {
    let Some(path) = cache() else {
        return;
    };
    if let Some(directory) = path.parent() {
        let _ = std::fs::create_dir_all(directory);
    }
    if let Err(error) = std::fs::write(&path, text) {
        tracing::debug!(%error, "not keeping the themes index");
    }
}

/// The document, over the network.
async fn fetch() -> Result<String, reqwest::Error> {
    let client = reqwest::Client::builder()
        .timeout(PATIENCE)
        .user_agent(concat!("obelus/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let response = client.get(URL).send().await?.error_for_status()?;
    obelus_download::text_within(response, MOST).await
}

/// The directory a theme taken from the registry is in, under `root`.
#[must_use]
pub fn home(name: &str, root: &Path) -> Option<PathBuf> {
    named_safely(name).then(|| root.join(name))
}

#[cfg(test)]
mod tests {
    use super::{Group, themes_in};

    const PUBLISHED: &str = "https://example.invalid/themes/index.json";

    /// An entry is read for what a card and an install need, a package
    /// beside the document is found beside it, and an entry missing what
    /// is needed -- or of a group this Obelus has not heard of, or with a
    /// name that would climb out of its directory -- is left out rather
    /// than refusing the rest.
    ///
    /// Deliberate breaks: the package taken as written (it is then not an
    /// address), the group not asked (the unknown one is listed), and the
    /// name not held to what a directory may be called (`../away` is
    /// listed).
    #[test]
    fn an_index_lists_what_it_can_and_leaves_out_what_it_cannot() {
        let text = r##"{"themes": [
            {"name": "rain", "group": "obelus", "mode": "dark", "author": "someone",
             "description": "Rain on a window", "package": "packages/rain-3f9c.tar.gz",
             "sha256": "ABC123", "commit": "3f9c", "updated": "2026-10-08",
             "wallpaper": {"license": "CC0-1.0", "source": "https://example.invalid/rain"}},
            {"name": "omarchy-nord", "group": "omarchy", "mode": "light",
             "package": "https://elsewhere.invalid/nord.tar.gz", "sha256": "def", "commit": "9a1"},
            {"name": "future", "group": "somebody-else", "mode": "dark",
             "package": "p.tar.gz", "sha256": "x", "commit": "y"},
            {"name": "../away", "group": "obelus", "mode": "dark",
             "package": "p.tar.gz", "sha256": "x", "commit": "y"},
            {"name": "half", "group": "obelus", "mode": "dark"}
        ]}"##;
        let themes = themes_in(text, PUBLISHED);
        let names: Vec<&str> = themes.iter().map(|theme| theme.name.as_str()).collect();
        assert_eq!(names, ["rain", "omarchy-nord"]);
        let rain = &themes[0];
        assert_eq!(
            rain.package,
            "https://example.invalid/themes/packages/rain-3f9c.tar.gz"
        );
        assert_eq!(rain.sha256, "abc123", "a hash is compared as one");
        assert!(rain.dark);
        assert_eq!(
            rain.wallpaper
                .as_ref()
                .map(|credit| credit.license.as_str()),
            Some("CC0-1.0")
        );
        let nord = &themes[1];
        assert_eq!(nord.group, Group::Omarchy);
        assert!(!nord.dark);
        assert_eq!(nord.package, "https://elsewhere.invalid/nord.tar.gz");
        assert!(nord.wallpaper.is_none());
    }

    /// A document that is not one lists nothing, rather than failing.
    #[test]
    fn a_document_that_is_not_an_index_lists_nothing() {
        assert!(themes_in("not json", PUBLISHED).is_empty());
        assert!(themes_in("{\"agents\": []}", PUBLISHED).is_empty());
    }
}
