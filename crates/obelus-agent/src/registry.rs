//! The list of agents, as its authors publish it.
//!
//! One JSON document, fetched once when the reader opens the page and kept
//! on disk so that the next session -- or a session with no network -- still
//! has a list. What is cached is shown first and the fetch replaces it when
//! it lands: a list that waits for the network is a page that is empty
//! exactly when a reader is looking at it.
//!
//! Read field by field rather than deserialized into a struct. The registry
//! grows fields and gains distribution kinds, and an older Obelus reading a
//! newer registry should show the entries it understands rather than
//! refusing the whole document because one entry has something new in it.

use std::path::{Path, PathBuf};

use obelus_sink::Sink;
use serde_json::Value;

use super::{Agent, Distribution};
use crate::Event;

/// Where the registry lives.
const URL: &str = "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";

/// How long to wait for it.
///
/// A page that is still saying "fetching" after this has told the reader
/// what they need to know: the list is the cached one.
const PATIENCE: std::time::Duration = std::time::Duration::from_secs(20);

/// How much of it to read.
///
/// The document is about sixty kilobytes. A megabyte is room for it to grow
/// by an order of magnitude and a limit on what a wrong URL can do to
/// memory.
const MOST: u64 = 1024 * 1024;

/// Where the copy is kept between sessions.
#[must_use]
pub fn cache() -> Option<PathBuf> {
    Some(dirs::cache_dir()?.join("obelus").join("registry.json"))
}

/// The agents in a registry document.
///
/// Entries Obelus cannot make sense of are left out rather than failing the
/// document: this is somebody else's file and it will grow.
#[must_use]
pub fn agents_in(text: &str) -> Vec<Agent> {
    let Ok(document) = serde_json::from_str::<Value>(text) else {
        tracing::warn!("the agent registry is not json");
        return Vec::new();
    };
    let Some(entries) = document.get("agents").and_then(Value::as_array) else {
        return Vec::new();
    };
    entries.iter().filter_map(agent).collect()
}

/// One entry, or nothing if it is missing what a row needs.
fn agent(entry: &Value) -> Option<Agent> {
    let text = |key: &str| entry.get(key).and_then(Value::as_str).map(str::to_string);
    Some(Agent {
        id: text("id")?,
        name: text("name")?,
        version: text("version").unwrap_or_default(),
        description: text("description").unwrap_or_default(),
        authors: entry
            .get("authors")
            .and_then(Value::as_array)
            .map(|authors| {
                authors
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        license: text("license").unwrap_or_default(),
        website: text("website").or_else(|| text("repository")),
        icon: text("icon"),
        distribution: distribution(entry.get("distribution")?)?,
    })
}

/// How to get an agent, for this machine.
///
/// The kinds are tried in the order Obelus would rather have them: a package
/// manager over a download, because a package manager is what keeps it up to
/// date afterwards.
fn distribution(entry: &Value) -> Option<Distribution> {
    let arguments = |value: &Value| {
        value
            .get("args")
            .and_then(Value::as_array)
            .map(|args| {
                args.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    };

    if let Some(npx) = entry.get("npx")
        && let Some(package) = npx.get("package").and_then(Value::as_str)
    {
        return Some(Distribution::Node {
            package: package.to_string(),
            arguments: arguments(npx),
        });
    }
    if let Some(uvx) = entry.get("uvx")
        && let Some(package) = uvx.get("package").and_then(Value::as_str)
    {
        return Some(Distribution::Python {
            package: package.to_string(),
            arguments: arguments(uvx),
        });
    }
    if let Some(binary) = entry.get("binary") {
        // Only this machine's, because the others are not offers to this
        // reader: a row that offers a Windows build on Linux is a row that
        // cannot be pressed.
        let entry = binary.get(target())?;
        let archive = entry.get("archive").and_then(Value::as_str)?.to_string();
        return Some(Distribution::Archive {
            archive,
            command: entry
                .get("cmd")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            arguments: arguments(entry),
            sha256: entry
                .get("sha256")
                .and_then(Value::as_str)
                .map(str::to_string),
        });
    }
    None
}

/// Which platform key in the registry is this machine.
///
/// The registry's own spelling, which is not Rust's: `linux-x86_64` where
/// the target triple says `x86_64-unknown-linux-gnu`.
#[must_use]
pub fn target() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux-x86_64",
        ("linux", "aarch64") => "linux-aarch64",
        ("macos", "x86_64") => "darwin-x86_64",
        ("macos", "aarch64") => "darwin-aarch64",
        ("windows", "x86_64") => "windows-x86_64",
        ("windows", "aarch64") => "windows-aarch64",
        // A machine the registry has no builds for. Nothing matches, so
        // every binary-only agent says it has nothing for this platform --
        // which is the truth.
        _ => "",
    }
}

/// What is on disk from a previous session, if anything.
#[must_use]
pub fn cached() -> Vec<Agent> {
    let Some(path) = cache() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    agents_in(&text)
}

/// Reads the registry on its own thread: the cached copy, then the network.
///
/// Both on the thread, including the read from disk. Sixty kilobytes off a
/// local disk is fast, but it is still a file read, and the frame that
/// opens the settings is not the place for one -- the page draws whatever
/// has arrived, which is nothing for the first frame and the cached list
/// for the next.
///
/// A failure comes back as one. The page has the cached list or an empty
/// one, and a page that says "fetching the list" for ever is lying by then.
pub fn spawn_fetch(sender: impl Sink<Event>) {
    obelus_runtime::handle().spawn(async move {
        let cached = cached();
        if !cached.is_empty()
            && sender
                .send(Event::Registry {
                    agents: cached,
                    failure: None,
                })
                .is_err()
        {
            return;
        }

        match fetch().await {
            Ok(text) => {
                if let Some(path) = cache() {
                    if let Some(directory) = path.parent() {
                        let _ = std::fs::create_dir_all(directory);
                    }
                    if let Err(error) = std::fs::write(&path, &text) {
                        tracing::debug!(%error, "not caching the registry");
                    }
                }
                let _ = sender.send(Event::Registry {
                    agents: agents_in(&text),
                    failure: None,
                });
            }
            Err(error) => {
                tracing::warn!(%error, "not fetching the agent registry");
                let _ = sender.send(Event::Registry {
                    agents: Vec::new(),
                    failure: Some(error.to_string()),
                });
            }
        }
    });
}

/// The document, over the network.
async fn fetch() -> Result<String, reqwest::Error> {
    let client = reqwest::Client::builder()
        .timeout(PATIENCE)
        .user_agent(concat!("obelus/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let response = client.get(URL).send().await?.error_for_status()?;
    crate::icon::text_within(response, MOST).await
}

/// Whether a path holds a file, for deciding an agent is installed.
#[must_use]
pub fn exists(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::{agents_in, target};
    use crate::Distribution;

    /// The document's own shape, cut down to one entry of each kind: the
    /// three the registry has today, and one Obelus cannot use.
    const SAMPLE: &str = r#"{
      "version": "1.0.0",
      "agents": [
        {
          "id": "claude-acp", "name": "Claude Agent", "version": "0.76.0",
          "description": "The Claude agent over ACP",
          "authors": ["Anthropic"], "license": "MIT",
          "repository": "https://example.invalid/claude",
          "icon": "https://example.invalid/claude.svg",
          "distribution": { "npx": { "package": "@acp/claude-agent-acp@0.76.0" } }
        },
        {
          "id": "gemini", "name": "Gemini CLI", "version": "0.59.0",
          "description": "Google's CLI", "authors": ["Google"], "license": "Apache-2.0",
          "website": "https://example.invalid/gemini",
          "distribution": { "npx": { "package": "@google/gemini-cli@0.59.0", "args": ["--acp"] } }
        },
        {
          "id": "pyagent", "name": "Py Agent", "version": "2.0.0",
          "description": "A python one", "authors": ["Someone"], "license": "MIT",
          "distribution": { "uvx": { "package": "py-agent==2.0.0", "args": ["serve"] } }
        },
        {
          "id": "amp", "name": "Amp", "version": "0.9.0",
          "description": "A downloaded one", "authors": ["tao"], "license": "Apache-2.0",
          "distribution": { "binary": {
            "linux-x86_64": { "archive": "https://example.invalid/amp-linux-x86_64.tar.gz",
                              "cmd": "./amp-acp", "sha256": "abc" },
            "linux-aarch64": { "archive": "https://example.invalid/amp-linux-aarch64.tar.gz",
                               "cmd": "./amp-acp", "sha256": "def" },
            "darwin-x86_64": { "archive": "https://example.invalid/amp-darwin-x86_64.tar.gz",
                               "cmd": "./amp-acp", "sha256": "ghi" },
            "darwin-aarch64": { "archive": "https://example.invalid/amp-darwin-aarch64.tar.gz",
                                "cmd": "./amp-acp", "sha256": "jkl" },
            "windows-x86_64": { "archive": "https://example.invalid/amp-windows-x86_64.zip",
                                "cmd": "amp-acp.exe" },
            "windows-aarch64": { "archive": "https://example.invalid/amp-windows-aarch64.zip",
                                 "cmd": "amp-acp.exe" }
          } }
        },
        { "id": "nothing", "name": "Nothing", "distribution": {} }
      ]
    }"#;

    /// Every kind the registry has, read as the kind Obelus would install
    /// it with -- and the entry with a distribution Obelus knows nothing
    /// about left out rather than failing the document.
    #[test]
    fn every_kind_of_entry_is_read() {
        let agents = agents_in(SAMPLE);
        let names: Vec<&str> = agents.iter().map(|agent| agent.name.as_str()).collect();
        assert_eq!(names, ["Claude Agent", "Gemini CLI", "Py Agent", "Amp"]);

        // The mark, where an entry has one. Every entry in the registry
        // does today, but it is somebody else's file: an entry without one
        // is a card that wears a glyph, not an entry Obelus drops.
        assert_eq!(
            agents[0].icon.as_deref(),
            Some("https://example.invalid/claude.svg")
        );
        assert!(agents[1].icon.is_none());

        assert_eq!(
            agents[0].distribution,
            Distribution::Node {
                package: "@acp/claude-agent-acp@0.76.0".to_string(),
                arguments: Vec::new()
            }
        );
        assert_eq!(
            agents[1].distribution,
            Distribution::Node {
                package: "@google/gemini-cli@0.59.0".to_string(),
                arguments: vec!["--acp".to_string()]
            }
        );
        assert!(matches!(
            agents[2].distribution,
            Distribution::Python { .. }
        ));
        // The archive for *this* machine, not the first one in the table: a
        // row offering a Windows build on Linux cannot be pressed.
        //
        // Every target the table above offers is one [`target`] can name,
        // and each archive is named after its own. Two of them were not:
        // the Windows build was `amp-windows.zip` and the Apple Silicon one
        // `amp-darwin.tar.gz`, so this said the entry was not that machine's
        // -- on those two machines, and nowhere the suite was usually run.
        let Distribution::Archive { archive, .. } = &agents[3].distribution else {
            panic!("not an archive: {:?}", agents[3].distribution);
        };
        assert!(
            archive.contains(target()),
            "{archive} is not this machine's, which is {}",
            target()
        );
    }

    /// A website to read more at, falling back to the repository: two of
    /// three entries have both, and the ones with neither say nothing
    /// rather than a placeholder.
    #[test]
    fn a_link_is_the_website_or_the_repository() {
        let agents = agents_in(SAMPLE);
        assert_eq!(
            agents[0].website.as_deref(),
            Some("https://example.invalid/claude"),
            "the repository is not offered when there is no website"
        );
        assert_eq!(
            agents[1].website.as_deref(),
            Some("https://example.invalid/gemini")
        );
        assert_eq!(agents[2].website, None);
    }

    /// Nonsense is an empty list, not a panic: this is somebody else's file
    /// and it arrives over a network.
    #[test]
    fn nonsense_is_no_agents() {
        assert!(agents_in("not json at all [").is_empty());
        assert!(agents_in("{}").is_empty());
        assert!(agents_in(r#"{"agents": "not a list"}"#).is_empty());
    }
}
