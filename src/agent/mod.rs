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

use std::path::{Path, PathBuf};

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

/// Where one agent's install goes.
///
/// A directory of its own, whatever it is made of. `npm` wants a prefix and
/// writes a `node_modules`, a `package.json` and a lock file into it; an
/// archive unpacks into one. Two agents sharing a directory would share
/// those three files, so installing the second would rewrite the first's
/// manifest and removing either would be impossible to do cleanly.
///
/// `None` for a name obelus will not make a directory of. This is the one
/// place an id from the registry -- somebody else's string -- becomes a
/// path, so it is the one place that has to check.
#[must_use]
pub fn home(id: &str, root: &std::path::Path) -> Option<PathBuf> {
    if id.is_empty()
        || id.starts_with('.')
        || !id.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_' || byte == b'.'
        })
    {
        return None;
    }
    Some(root.join(id))
}

/// The file that says an install is under way, inside the agent's own
/// directory.
pub const CLAIM: &str = "installing";

/// How long a claim is believed before it is taken over.
///
/// An obelus that was killed mid-install leaves one behind, and nothing
/// else will ever remove it. Ten minutes is longer than any install obelus
/// has seen and short enough that a reader who kills one and tries again
/// does not have to wonder what is wrong.
const CLAIMED_FOR: std::time::Duration = std::time::Duration::from_secs(600);

/// Says this process is installing an agent, unless another one is.
///
/// Several obelus processes on one machine share this directory, and an
/// install is a program writing a tree into it: two at once is two `npm`s
/// with one prefix, and the mixed tree they leave says nothing about which
/// half is which. The record written at the end still keeps the *status*
/// honest -- a half-finished install never counts as installed -- but the
/// directory is worth not mixing in the first place.
///
/// Created exclusively, which is the one filesystem operation that settles
/// a race between processes without asking anybody to agree first.
pub fn claim(id: &str, root: &std::path::Path) -> Result<Claim, String> {
    let Some(home) = home(id, root) else {
        return Err(format!("{id} is not a name obelus can keep a directory of"));
    };
    std::fs::create_dir_all(&home).map_err(|error| format!("{home:?}: {error}"))?;
    let path = home.join(CLAIM);
    match std::fs::File::create_new(&path) {
        Ok(_) => Ok(Claim { path }),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            // Somebody's, or nobody's: an install that was killed leaves a
            // claim that will never be given up, so one old enough to be
            // that is taken over rather than waited on for ever.
            let stale = std::fs::metadata(&path)
                .and_then(|file| file.modified())
                .ok()
                .and_then(|when| when.elapsed().ok())
                .is_some_and(|since| since > CLAIMED_FOR);
            if !stale {
                return Err(format!("another obelus is installing {id}"));
            }
            tracing::warn!(id, "taking over an install that was left behind");
            Ok(Claim { path })
        }
        Err(error) => Err(format!("{path:?}: {error}")),
    }
}

/// An install this process has claimed, which it gives up by being dropped.
///
/// Dropped rather than given up by hand: an install that returns early --
/// and every step of one can -- would otherwise leave the claim behind and
/// lock the reader out of their own agent for ten minutes.
#[derive(Debug)]
pub struct Claim {
    path: PathBuf,
}

impl Drop for Claim {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_file(&self.path) {
            tracing::warn!(%error, path = %self.path.display(), "a claim outlived its install");
        }
    }
}

/// What an install left behind: how to start the agent, and what it was.
///
/// Written by the install as its last act and read by everything else, which
/// is the point of it. What is on disk otherwise cannot be trusted to answer
/// "is this installed": `npm` builds its tree in an order of its own -- the
/// package's manifest first, the `node_modules/.bin` link after it -- so an
/// install killed halfway through leaves a directory that looks exactly like
/// a finished one. This file exists only where obelus saw the install finish
/// *and* could work out what to run, so an interrupted install is simply not
/// an install, and the card offers to do it again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installation {
    /// The program to run.
    pub command: PathBuf,
    /// And what to pass it.
    pub arguments: Vec<String>,
    /// Which version was installed.
    ///
    /// Recorded rather than read back off the install: an archive says
    /// nothing about itself once it is a directory, and a reader whose
    /// version obelus cannot name can never be told there is a newer one.
    pub version: String,
}

/// What that file is called, inside the agent's own directory.
///
/// Inside it, so that removing an agent is removing a directory and the
/// record cannot outlive the thing it describes.
pub const RECORD: &str = "installed.json";

/// Writes down that an agent is installed, and how to start it.
///
/// [`command_for`] needs the registry's entry -- which package, which
/// archive -- and the registry is fetched when a reader opens the agents
/// page, which is not something they do before every conversation. So the
/// answer is written at the moment it is known, and talking to an agent
/// afterwards is a local matter.
pub fn remember(
    id: &str,
    command: &std::path::Path,
    arguments: &[String],
    version: &str,
    root: &std::path::Path,
) -> Result<(), String> {
    let Some(home) = home(id, root) else {
        return Err(format!("{id} is not a name obelus can keep a directory of"));
    };
    let record = serde_json::json!({
        "command": command,
        "arguments": arguments,
        "version": version,
    });
    std::fs::create_dir_all(&home).map_err(|error| format!("{home:?}: {error}"))?;
    std::fs::write(home.join(RECORD), record.to_string())
        .map_err(|error| format!("{:?}: {error}", home.join(RECORD)))
}

/// What an agent's install left behind, if it left anything.
///
/// The one answer to "is this installed, which version, and how is it
/// started": three questions with one answer, because an install that can
/// only answer two of them is not one a reader can use.
#[must_use]
pub fn installation(id: &str, root: &std::path::Path) -> Option<Installation> {
    let text = std::fs::read_to_string(home(id, root)?.join(RECORD)).ok()?;
    let record: serde_json::Value = serde_json::from_str(&text).ok()?;
    let command = PathBuf::from(record.get("command")?.as_str()?);
    // Gone from disk since it was written -- the reader removed the
    // directory, or npm did. Nothing to start, so nothing is installed.
    // A relative command is somebody on the path (`uvx`), which is not
    // obelus's to check.
    if command.is_absolute() && !command.exists() {
        return None;
    }
    Some(Installation {
        arguments: record
            .get("arguments")
            .and_then(serde_json::Value::as_array)
            .map(|arguments| {
                arguments
                    .iter()
                    .filter_map(|argument| argument.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        version: record
            .get("version")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string(),
        command,
    })
}

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

/// What to run for an agent that has just been installed.
///
/// Read from what the install left behind rather than guessed from the
/// package's name: a node package says in its own manifest what its
/// executable is called, and the name of the package is often not it.
///
/// Asked once, by the install, and the answer written down as the
/// [`Installation`] -- so the question needs the registry's entry and
/// everything afterwards does not.
#[must_use]
pub fn command_for(agent: &Agent, root: &std::path::Path) -> Option<(PathBuf, Vec<String>)> {
    match &agent.distribution {
        Distribution::Node { package, arguments } => {
            let name = package_name(package);
            let home = home(&agent.id, root)?;
            let manifest = home.join("node_modules").join(&name).join("package.json");
            let text = std::fs::read_to_string(manifest).ok()?;
            let manifest: serde_json::Value = serde_json::from_str(&text).ok()?;
            let binary = binary_name(&manifest, &name)?;
            let command = shim(&home.join("node_modules").join(".bin"), &binary)?;
            Some((command, arguments.clone()))
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
            let path = home(&agent.id, root)?.join(command.trim_start_matches("./"));
            path.exists().then(|| (path, arguments.clone()))
        }
    }
}

/// The file npm left in `.bin` that this machine can start.
///
/// One file on unix, named after the program. On Windows npm writes three --
/// a shell script under the bare name, a `.cmd` and a `.ps1` -- and the bare
/// one is the one nothing there can start: it is read as an executable, its
/// `#!` line is not machine code, and the answer is that the file is not a
/// valid application. So the `.cmd` is asked for first.
///
/// The bare name is still the answer where there is nothing else, because
/// that is every other platform and because an install obelus has not seen
/// before is better started and found wanting than not tried.
fn shim(beside: &Path, binary: &str) -> Option<PathBuf> {
    let named = |ending: &str| {
        let path = beside.join(format!("{binary}{ending}"));
        path.exists().then_some(path)
    };
    match cfg!(windows) {
        true => named(".cmd").or_else(|| named("")),
        false => named(""),
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
    /// Two obelus processes asked for the same agent: one installs it.
    ///
    /// They share this directory, and an install is a program writing a
    /// tree into it -- two at once is two `npm`s with one prefix. The claim
    /// is given up by being dropped, so the second one can have it the
    /// moment the first is finished, however it finished.
    #[test]
    fn only_one_obelus_installs_an_agent_at_a_time() {
        let root = std::env::temp_dir().join(format!("obelus-claim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        let mine = super::claim("some-agent", &root).expect("the first claim");
        let theirs = super::claim("some-agent", &root);
        assert!(
            theirs.is_err_and(|why| why.contains("another obelus")),
            "two of them installed it at once"
        );
        // And another agent is another install, which this says nothing
        // about.
        assert!(
            super::claim("other-agent", &root).is_ok(),
            "one install stopped every other"
        );

        drop(mine);
        assert!(
            super::claim("some-agent", &root).is_ok(),
            "the claim outlived the install"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A claim nobody gave up is taken over once it is old enough.
    ///
    /// An obelus that was killed mid-install leaves one behind, and nothing
    /// else will ever remove it: without this, one kill locks a reader out
    /// of their own agent for good.
    #[test]
    fn a_claim_left_behind_is_taken_over() {
        let root = std::env::temp_dir().join(format!("obelus-stale-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = super::home("some-agent", &root).expect("a directory");
        std::fs::create_dir_all(&home).expect("the directory");
        let path = home.join(super::CLAIM);
        std::fs::write(&path, "").expect("a claim nobody will give up");

        // Old enough to be nobody's. The file's own time is what says so,
        // which is what a second obelus has to go on.
        let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        let file = std::fs::File::options()
            .write(true)
            .open(&path)
            .expect("the claim");
        file.set_modified(long_ago).expect("aging it");
        drop(file);

        assert!(
            super::claim("some-agent", &root).is_ok(),
            "a claim nobody will ever give up locked the agent out"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    use super::{Agent, Distribution, binary_name, command_for, package_name};

    /// Every agent installs into a directory of its own.
    ///
    /// `npm` writes a `node_modules`, a manifest and a lock file into
    /// whatever prefix it is given, so two agents sharing a prefix would
    /// share all three: installing the second would rewrite the first's
    /// answer to what is installed there, and neither could be removed
    /// without taking the other with it.
    #[test]
    fn a_node_agent_lives_under_its_own_id() {
        let root = std::env::temp_dir().join(format!("obelus-home-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let agent = Agent {
            id: "someone".to_string(),
            name: "Someone".to_string(),
            version: "1.0.0".to_string(),
            description: String::new(),
            authors: Vec::new(),
            license: String::new(),
            website: None,
            icon: None,
            distribution: Distribution::Node {
                package: "thing@1.0.0".to_string(),
                arguments: Vec::new(),
            },
        };
        // What npm leaves behind, in the place obelus now asks it to work.
        let install = |prefix: &std::path::Path| {
            let package = prefix.join("node_modules").join("thing");
            std::fs::create_dir_all(&package).expect("a directory");
            std::fs::write(
                package.join("package.json"),
                "{\"version\":\"1.0.0\",\"bin\":\"thing.js\"}",
            )
            .expect("a manifest");
            let binaries = prefix.join("node_modules").join(".bin");
            std::fs::create_dir_all(&binaries).expect("a directory");
            std::fs::write(binaries.join("thing"), "").expect("a binary");
        };

        // Installed at the top of the agents directory, as it used to be:
        // not this agent's install, and not found.
        install(&root);
        assert!(
            command_for(&agent, &root).is_none(),
            "an install nobody owns was taken for this agent's"
        );

        let home = super::home(&agent.id, &root).expect("a directory for it");
        install(&home);
        let (command, _) = command_for(&agent, &root).expect("the command");
        assert_eq!(
            command,
            root.join("someone")
                .join("node_modules")
                .join(".bin")
                .join("thing")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// An id is a path segment, and it comes from somebody else's file. The
    /// one place it becomes a path is the one place that has to check.
    #[test]
    fn a_name_obelus_would_not_make_a_directory_of_is_refused() {
        let root = std::path::Path::new("/tmp/agents");
        for id in [
            "",
            ".",
            "..",
            ".hidden",
            "../escape",
            "with/slash",
            "sp ace",
        ] {
            assert!(
                super::home(id, root).is_none(),
                "{id:?} was taken for a directory name"
            );
        }
        assert_eq!(
            super::home("claude-acp", root),
            Some(root.join("claude-acp"))
        );
    }

    /// The install's record is the one answer to "is this installed": what
    /// it was, and how to start it. And it is only an answer while the
    /// thing it names is there -- a reader who removed the directory has
    /// removed the agent.
    #[test]
    fn the_record_says_what_was_installed_and_how_to_start_it() {
        let root = std::env::temp_dir().join(format!("obelus-record-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = super::home("someone", &root).expect("a directory for it");
        std::fs::create_dir_all(&home).expect("a directory");
        let program = home.join("run-me");
        std::fs::write(&program, "").expect("a program");

        assert_eq!(
            super::installation("someone", &root),
            None,
            "an agent with no record read as installed"
        );

        super::remember("someone", &program, &["--acp".to_string()], "1.2.3", &root)
            .expect("writing the record");
        let installed = super::installation("someone", &root).expect("the record");
        assert_eq!(installed.command, program);
        assert_eq!(installed.arguments, ["--acp"]);
        assert_eq!(installed.version, "1.2.3");

        // The program taken away: the record is still there and describes
        // nothing, which is not an install.
        std::fs::remove_file(&program).expect("removing it");
        assert_eq!(
            super::installation("someone", &root),
            None,
            "a record whose program is gone read as installed"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

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

    /// And the file in `.bin` is the one this machine can start.
    ///
    /// npm writes one file on unix and three on Windows, and the bare name
    /// -- the one that is there on both -- is the one Windows cannot run:
    /// it holds `#!/bin/sh`, which is read there as machine code. Both are
    /// written here, so the answer says which was preferred rather than
    /// which happened to exist.
    ///
    /// Broken deliberately by asking for the bare name first: the shell
    /// script comes back, and every agent npm installed fails to start
    /// with "not a valid application".
    #[test]
    fn the_shim_is_the_one_this_machine_can_start() {
        let beside = std::env::temp_dir().join(format!("obelus-bin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&beside);
        std::fs::create_dir_all(&beside).expect("a directory");
        std::fs::write(
            beside.join("agent"),
            "#!/bin/sh
",
        )
        .expect("the script");
        std::fs::write(
            beside.join("agent.cmd"),
            "@echo off
",
        )
        .expect("the shim");

        let wanted = match cfg!(windows) {
            true => "agent.cmd",
            false => "agent",
        };
        assert_eq!(
            super::shim(&beside, "agent"),
            Some(beside.join(wanted)),
            "the shim chosen is not one this machine can start"
        );
        // And nothing where npm wrote nothing, which is what says an
        // install did not finish.
        assert_eq!(super::shim(&beside, "other"), None);

        let _ = std::fs::remove_dir_all(&beside);
    }
}
