//! The agents obelus can talk to, and where they come from.
//!
//! An agent is another program that speaks the Agent Client Protocol over
//! its own standard input and output -- the same shape as a language
//! server, and for the same reason: obelus does not implement anybody's
//! model, it talks to whatever the reader already has.
//!
//! Which agents exist is not obelus's to decide. There is a registry, kept
//! by the protocol's own authors, and [`registry`] reads it: forty entries
//! with a name, a version, a description and how to install each one. What
//! is here is the shape of an entry, where obelus keeps what it installs,
//! and how it tells whether it has.

pub mod icon;
pub mod install;
pub mod registry;

use std::path::PathBuf;

/// One agent, as the registry describes it.
///
/// Every field the registry promises, and nothing invented: a reader
/// choosing between forty of these is choosing on what it says about
/// itself, so the view shows what is there and says nothing where there is
/// nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Agent {
    /// The registry's own name for it, which is what obelus stores when a
    /// reader picks one.
    pub id: String,
    /// What to call it on screen.
    pub name: String,
    /// Which version the registry currently offers.
    pub version: String,
    /// One line about what it is.
    pub description: String,
    /// Who wrote it.
    pub authors: Vec<String>,
    /// Under what licence.
    pub license: String,
    /// Where to read more, if it says.
    pub website: Option<String>,
    /// Where its mark is, if it has one. Every entry in the registry today
    /// carries one: a monochrome sixteen-pixel SVG drawn in `currentColor`,
    /// meant to be inked in whatever colour it is put on.
    pub icon: Option<String>,
    /// How to get it and how to run it.
    pub distribution: Distribution,
}

/// How an agent is installed and started.
///
/// Three kinds in the registry, and obelus can install two of them. The
/// third needs a download, a checksum and an archive of the right shape for
/// this machine, which is a stack of dependencies obelus has not earned yet
/// -- so those say so and offer their website instead of a button that
/// would not work.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Distribution {
    /// A node package, installed with `npm` and run from its own `bin`.
    Node {
        /// The package, with the version the registry named.
        package: String,
        /// What to pass it on the command line.
        arguments: Vec<String>,
    },
    /// A python package, run through `uvx`.
    Python {
        /// The package, with its version.
        package: String,
        /// What to pass it.
        arguments: Vec<String>,
    },
    /// A downloaded archive, with the checksum the registry gave for it if
    /// it gave one -- about half of them do.
    Archive {
        /// Where to get it.
        archive: String,
        /// What to run once it is unpacked, relative to where it unpacked.
        command: String,
        /// What to pass it.
        arguments: Vec<String>,
        /// What it should hash to, if the registry says.
        sha256: Option<String>,
    },
}

impl Distribution {
    /// Whether obelus can install this one.
    ///
    /// All three kinds, now that it can download and unpack: what it cannot
    /// do is an archive in a shape it has no unpacker for, and that is
    /// found out when the archive arrives rather than guessed from a name.
    #[must_use]
    pub const fn installable(&self) -> bool {
        true
    }
}

/// Where obelus keeps the agents it installs.
///
/// Its own directory under the reader's data directory, never the machine's
/// global `npm` prefix: obelus installing something into a place the reader
/// shares with everything else is obelus deciding for them. Removing an
/// agent is removing a directory.
#[must_use]
pub fn root() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("obelus").join("agents"))
}

/// Which version of an agent is installed, if one is.
///
/// From what the install left behind rather than remembered: a node package
/// says its version in its own manifest, and an archive gets a file written
/// beside it saying which one it was -- because an archive says nothing
/// about itself once it is unpacked.
#[must_use]
pub fn installed_version(agent: &Agent, root: &std::path::Path) -> Option<String> {
    match &agent.distribution {
        Distribution::Node { package, .. } => {
            let manifest = root
                .join("node_modules")
                .join(package_name(package))
                .join("package.json");
            let text = std::fs::read_to_string(manifest).ok()?;
            let manifest: serde_json::Value = serde_json::from_str(&text).ok()?;
            Some(manifest.get("version")?.as_str()?.to_string())
        }
        Distribution::Archive { .. } => {
            let stamp = root.join(&agent.id).join(STAMP);
            std::fs::read_to_string(stamp)
                .ok()
                .map(|version| version.trim().to_string())
        }
        // `uvx` fetches the pinned version each time it runs, so what is
        // installed is whatever the registry last said.
        Distribution::Python { .. } => Some(agent.version.clone()),
    }
}

/// The file an archive install leaves saying which version it unpacked.
pub const STAMP: &str = ".obelus-version";

/// What obelus knows about one agent locally.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// Not here, and installable.
    Missing,
    /// Being installed right now, on a thread.
    Installing,
    /// Here, and startable.
    Installed,
    /// Here, and the registry has a newer one.
    ///
    /// Its own state rather than a flag on `Installed`, because it is what
    /// the card offers: a reader looking at an agent they already have
    /// wants to know that there is something to press.
    Outdated {
        /// Which version is here.
        installed: String,
    },
    /// The install ran and did not work, with what it said.
    Failed(String),
    /// Not something obelus can install.
    Unavailable(&'static str),
}

/// The command that starts an installed agent, if it is installed.
///
/// Read from what the install left behind rather than guessed from the
/// package's name: a node package says in its own manifest what its
/// executable is called, and the name of the package is often not it.
#[must_use]
pub fn command_for(agent: &Agent, root: &std::path::Path) -> Option<(PathBuf, Vec<String>)> {
    match &agent.distribution {
        Distribution::Node { package, arguments } => {
            let name = package_name(package);
            let manifest = root.join("node_modules").join(&name).join("package.json");
            let text = std::fs::read_to_string(manifest).ok()?;
            let manifest: serde_json::Value = serde_json::from_str(&text).ok()?;
            let binary = binary_name(&manifest, &name)?;
            let command = root.join("node_modules").join(".bin").join(binary);
            command.exists().then(|| (command, arguments.clone()))
        }
        // `uvx` fetches on demand and caches for itself, so the command is
        // the same whether or not anything has been installed.
        Distribution::Python { package, arguments } => {
            let mut all = vec![package.clone()];
            all.extend(arguments.iter().cloned());
            Some((PathBuf::from("uvx"), all))
        }
        Distribution::Archive {
            command, arguments, ..
        } => {
            let path = root.join(&agent.id).join(command.trim_start_matches("./"));
            path.exists().then(|| (path, arguments.clone()))
        }
    }
}

/// The package's name without the version the registry pinned to it.
#[must_use]
pub fn package_name(package: &str) -> String {
    // A scope starts with `@` and carries a slash, so the version's `@` is
    // the last one and only when it is not the first character.
    match package.rfind('@') {
        Some(0) | None => package.to_string(),
        Some(at) => package[..at].to_string(),
    }
}

/// Which executable a package's manifest says it has.
///
/// `bin` is either a name or a table of them. A table with one entry is the
/// ordinary case; with several, the one named after the package is the one
/// meant to be run.
fn binary_name(manifest: &serde_json::Value, package: &str) -> Option<String> {
    let short = package.rsplit('/').next().unwrap_or(package);
    match manifest.get("bin")? {
        serde_json::Value::String(_) => Some(short.to_string()),
        serde_json::Value::Object(table) => {
            if table.contains_key(short) {
                return Some(short.to_string());
            }
            table.keys().next().cloned()
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{binary_name, package_name};

    /// The registry pins a version onto the package name, and a scoped
    /// package has an `@` of its own: taking the wrong one asks npm for a
    /// package that does not exist.
    #[test]
    fn a_version_comes_off_a_package_name() {
        assert_eq!(
            package_name("@agentclientprotocol/claude-agent-acp@0.76.0"),
            "@agentclientprotocol/claude-agent-acp"
        );
        assert_eq!(
            package_name("@google/gemini-cli@0.59.0"),
            "@google/gemini-cli"
        );
        assert_eq!(package_name("agoragentic-mcp@1.3.0"), "agoragentic-mcp");
        assert_eq!(package_name("@scope/thing"), "@scope/thing");
        assert_eq!(package_name("thing"), "thing");
    }

    /// A package says what its executable is called, and the name of the
    /// package is often not it.
    #[test]
    fn the_executable_comes_from_the_manifest() {
        let one = serde_json::json!({ "bin": { "claude-code-acp": "dist/index.js" } });
        assert_eq!(
            binary_name(&one, "@zed-industries/claude-code-acp").as_deref(),
            Some("claude-code-acp")
        );
        // A string `bin` is the package's own short name.
        let string = serde_json::json!({ "bin": "dist/index.js" });
        assert_eq!(
            binary_name(&string, "@scope/thing").as_deref(),
            Some("thing")
        );
        // Several, and none named after the package: the first is all there
        // is to go on.
        let several = serde_json::json!({ "bin": { "a": "a.js", "b": "b.js" } });
        assert_eq!(binary_name(&several, "@scope/thing").as_deref(), Some("a"));
        // And a package with no executable at all is not startable.
        assert_eq!(binary_name(&serde_json::json!({}), "thing"), None);
    }
}
