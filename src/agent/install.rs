//! Installing an agent, in the background.
//!
//! Two shapes, and they can say different things while they run. A package
//! manager is asked to do the whole job and reports nothing until it is
//! finished, so the page can only say that it is running and for how long.
//! An archive is a download of a known length, so the page can say how far
//! through it is and how much longer it will take -- and it says so because
//! it is true, not because a progress bar is nice to look at.
//!
//! An agent is installed when the install says so, in writing. The last
//! thing `install::spawn` does is write `agents/<id>/installed.json` -- the
//! command, its arguments, and the version -- and every later question reads
//! that one file: is it installed, which version, how is it started. Nothing
//! infers an install from the files a package manager left, because that cannot
//! be done: `npm` writes a package's manifest before it links the executable,
//! so a run killed halfway leaves a directory shaped exactly like a finished
//! one. It did read them once, and the cost was a reader whose obelus was shut
//! mid-install and who then had a card reading "active" over an agent nothing
//! could start, with no button on it but the one that turned it off.
//! Working out what to run happens *inside* the install, where the registry's
//! entry is in hand: an install that cannot say how to start what it installed
//! has failed. So an interrupted install is simply not an install, `activate`
//! refuses an agent with no record, and a card says "active" only for one that
//! is really there.
//!
//! `agent::home` is the one place an id from the registry becomes a path, so it
//! is the one place that checks the name, and it returns `None` for one it will
//! not make a directory of.

use std::{
    path::{Path, PathBuf},
    sync::mpsc::Sender,
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};

use super::{Agent, Distribution};
use crate::event::Event;

/// How often to report progress.
///
/// Every chunk would wake the loop hundreds of times a second to redraw a
/// number that has not visibly changed.
const REPORT: Duration = Duration::from_millis(120);

/// How far an install has got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    /// Bytes fetched so far.
    pub done: u64,
    /// How many there are, when the server says.
    pub total: Option<u64>,
    /// How long it has been running.
    pub elapsed: Duration,
}

impl Progress {
    /// How much longer, if that can be known.
    ///
    /// From the rate so far, which is the only honest estimate available:
    /// nothing here knows the shape of the rest of the download. `None`
    /// when there is no length to divide by, which is exactly when a
    /// package manager is doing the work.
    #[must_use]
    pub fn remaining(self) -> Option<Duration> {
        let total = self.total?;
        if self.done == 0 || total <= self.done {
            return None;
        }
        let rate = self.done as f64 / self.elapsed.as_secs_f64().max(0.001);
        let left = (total - self.done) as f64 / rate;
        left.is_finite().then(|| Duration::from_secs_f64(left))
    }

    /// How far through, from nothing to one.
    #[must_use]
    pub fn fraction(self) -> Option<f64> {
        let total = self.total?;
        (total > 0).then(|| (self.done as f64 / total as f64).clamp(0.0, 1.0))
    }
}

/// Installs an agent on its own thread.
///
/// Everything it has to say comes back as events: progress while it runs,
/// and once at the end with what happened. The thread owns the whole job --
/// download, checksum, unpack, or the package manager's own run -- because
/// a half-installed agent is not something the loop should have to reason
/// about.
///
/// And the last thing it does is write the record that says an agent is
/// installed: the files a package manager leaves behind are not an answer
/// to that -- `npm` writes a package's manifest before it links the
/// executable, so a run that was killed halfway leaves a directory shaped
/// exactly like a finished one. Working out what to run is part of
/// finishing, not something to be attempted later: an install that cannot
/// say how to start the thing it installed has failed, and says so where a
/// reader is looking.
pub fn spawn(agent: &Agent, root: &Path, sender: Sender<Event>) {
    let agent = agent.clone();
    let root = root.to_path_buf();
    crate::runtime::handle().spawn(async move {
        let started = Instant::now();
        let outcome = install(&agent, &root, started, &sender).await;
        match &outcome {
            Ok(()) => tracing::info!(id = agent.id, "installed an agent"),
            Err(why) => tracing::warn!(id = agent.id, why, "an agent did not install"),
        }
        let _ = sender.send(Event::Installed {
            id: agent.id,
            failure: outcome.err(),
        });
    });
}

/// The whole job: fetch it, then write down what there is.
///
/// A task rather than a thread, and the two halves of it go where they
/// belong: the download only waits, so it is awaited here; `npm`, the
/// unpacking and the hashing are work, so each is a blocking job. There is
/// no `block_on` anywhere in it, which an earlier shape needed -- and that
/// shape could not be a job of its own, because `block_on` from inside a
/// worker panics.
async fn install(
    agent: &Agent,
    root: &Path,
    started: Instant,
    sender: &Sender<Event>,
) -> Result<(), String> {
    // Said first, so that a second obelus asked for the same agent stops
    // here rather than running a second `npm` into the same directory.
    // Held to the end of this function and given up by being dropped,
    // however it ends.
    let _claim = super::claim(&agent.id, root)?;
    let Some(home) = super::home(&agent.id, root) else {
        return Err(format!(
            "{} is not a name obelus can keep a directory of",
            agent.id
        ));
    };
    match &agent.distribution {
        Distribution::Node { package, .. } => node(package, &home).await?,
        // `uvx` fetches the pinned version the first time it runs and
        // caches it for itself, so there is nothing to fetch here -- only
        // the record to write, which is the reader having asked for it.
        Distribution::Python { .. } => {}
        Distribution::Archive {
            archive,
            sha256,
            command,
            ..
        } => {
            download(
                archive,
                sha256.as_deref(),
                command,
                &home,
                started,
                &agent.id,
                sender,
            )
            .await?;
        }
    }

    let Some((command, arguments)) = super::command_for(agent, root) else {
        return Err("Installed, but there is no telling what to run".to_string());
    };
    super::remember(&agent.id, &command, &arguments, &agent.version, root)
}

/// Asks `npm` for a package, into this agent's own directory.
///
/// `--prefix` and nothing global: obelus installing into a place the reader
/// shares with everything else on the machine is obelus deciding for them.
/// And a prefix per agent, because the prefix is where npm keeps the
/// manifest -- one shared between agents would be rewritten by whichever
/// was installed last.
async fn node(package: &str, home: &Path) -> Result<(), String> {
    let (package, home) = (package.to_string(), home.to_path_buf());
    // A blocking job: `npm install` is a subprocess that reads the network
    // and writes a tree, and none of that is waiting obelus can do
    // anything else during.
    crate::runtime::handle()
        .spawn_blocking(move || node_now(&package, &home))
        .await
        .unwrap_or_else(|error| Err(error.to_string()))
}

/// The same, where it is allowed to block.
fn node_now(package: &str, home: &Path) -> Result<(), String> {
    std::fs::create_dir_all(home).map_err(|error| format!("{home:?}: {error}"))?;
    let outcome = std::process::Command::new("npm")
        .arg("install")
        .arg("--prefix")
        .arg(home)
        .arg("--no-fund")
        .arg("--no-audit")
        .arg(package)
        .output()
        .map_err(|error| match error.kind() {
            // The ordinary way this fails, and the one worth naming: node
            // is not installed.
            std::io::ErrorKind::NotFound => "There is no npm on the path".to_string(),
            _ => error.to_string(),
        })?;
    if outcome.status.success() {
        return Ok(());
    }
    // The last line npm said, which is where it puts the reason.
    let complaint = String::from_utf8_lossy(&outcome.stderr);
    Err(complaint
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("No reason from npm")
        .trim()
        .to_string())
}

/// Fetches an archive, checks it, and unpacks it.
async fn download(
    archive: &str,
    sha256: Option<&str>,
    command: &str,
    into: &Path,
    started: Instant,
    id: &str,
    sender: &Sender<Event>,
) -> Result<(), String> {
    // The one part of an install that only waits, so it is the one part
    // that is awaited: the bytes arrive in chunks, and a chunk is a chance
    // to say how far along it is.
    let bytes = {
        use futures::StreamExt as _;

        let http = reqwest::Client::builder()
            .user_agent(concat!("obelus/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| format!("fetching it: {error}"))?;
        let response = http
            .get(archive)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|error| format!("fetching it: {error}"))?;
        let total = response.content_length();

        let mut bytes: Vec<u8> = Vec::with_capacity(total.unwrap_or(0) as usize);
        let mut body = response.bytes_stream();
        let mut said = Instant::now();
        while let Some(chunk) = body.next().await {
            let chunk = chunk.map_err(|error| format!("reading it: {error}"))?;
            bytes.extend_from_slice(&chunk);
            if said.elapsed() >= REPORT {
                said = Instant::now();
                let _ = sender.send(Event::Installing {
                    id: id.to_string(),
                    progress: Progress {
                        done: bytes.len() as u64,
                        total,
                        elapsed: started.elapsed(),
                    },
                });
            }
        }
        bytes
    };

    // Hashing a download and unpacking it are work, not waiting, so they
    // go where work goes. Together, because they are one step: the check
    // is what says the bytes may be unpacked at all.
    let (archive, command, into) = (archive.to_string(), command.to_string(), into.to_path_buf());
    let expected = sha256.map(str::to_string);
    let said = id.to_string();
    crate::runtime::handle()
        .spawn_blocking(move || {
            settle(
                &bytes,
                &archive,
                expected.as_deref(),
                &command,
                &into,
                &said,
            )
        })
        .await
        .unwrap_or_else(|error| Err(error.to_string()))
}

/// Checks a download and puts it where it goes.
///
/// Its own function because it is the blocking half of an install, and the
/// half that has to be told apart from the waiting half: everything here
/// hashes, deletes, writes or unpacks.
fn settle(
    bytes: &[u8],
    archive: &str,
    sha256: Option<&str>,
    command: &str,
    into: &Path,
    id: &str,
) -> Result<(), String> {
    if let Some(expected) = sha256 {
        let digest = Sha256::digest(bytes);
        let got: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        if !got.eq_ignore_ascii_case(expected) {
            return Err("It is not the file the registry describes".to_string());
        }
    } else {
        // Half the registry's entries have no checksum. Not a reason to
        // refuse -- the URL came from the registry over TLS -- but worth
        // saying somewhere.
        tracing::debug!(id, "the registry gave no checksum for this one");
    }

    // Whatever was there before, gone: an archive unpacked over an older
    // one leaves both, and the older one's files are not this version.
    let _ = std::fs::remove_dir_all(into);
    std::fs::create_dir_all(into).map_err(|error| format!("{into:?}: {error}"))?;
    unpack(bytes, archive, into)?;

    // Unpacked, and the thing to run has to be there and be runnable: an
    // archive that unpacked to something else is a failure now rather than
    // a mystery the first time the reader tries to talk to it.
    let program = into.join(command.trim_start_matches("./"));
    if !program.exists() {
        return Err(format!("no {command} in the archive"));
    }
    make_runnable(&program);
    Ok(())
}

/// Unpacks whichever shape the archive is, by what its name says it is.
fn unpack(bytes: &[u8], name: &str, into: &Path) -> Result<(), String> {
    let name = name.rsplit('/').next().unwrap_or(name).to_lowercase();
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        let reader = flate2::read::GzDecoder::new(bytes);
        return tar::Archive::new(reader)
            .unpack(into)
            .map_err(|error| format!("unpacking it: {error}"));
    }
    if name.ends_with(".zip") {
        let reader = std::io::Cursor::new(bytes);
        let mut zip =
            zip::ZipArchive::new(reader).map_err(|error| format!("unpacking it: {error}"))?;
        return zip
            .extract(into)
            .map_err(|error| format!("unpacking it: {error}"));
    }
    // `.tar.bz2` is four of the registry's hundred entries, and a shape
    // obelus has no unpacker for. Saying which shape beats saying "failed".
    Err(format!("obelus cannot unpack {name}"))
}

/// Makes an unpacked program executable.
///
/// A tar keeps its modes and a zip usually does not, so this is about zips:
/// a downloaded agent that cannot be run is an install that looks finished
/// and is not.
fn make_runnable(program: &PathBuf) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = std::fs::metadata(program) {
            let mut modes = metadata.permissions();
            modes.set_mode(modes.mode() | 0o755);
            let _ = std::fs::set_permissions(program, modes);
        }
    }
    #[cfg(not(unix))]
    let _ = program;
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{Agent, Distribution, Progress};

    /// An install's last act is the record that says it finished, and until
    /// it is written nothing is installed. A python one, because `uvx`
    /// fetches when it runs: there is nothing to download here, so what the
    /// test is left with is exactly the bookkeeping.
    #[tokio::test]
    async fn an_install_finishes_by_writing_down_what_it_installed() {
        let root = std::env::temp_dir().join(format!("obelus-install-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let agent = Agent {
            id: "py-agent".to_string(),
            name: "Python one".to_string(),
            version: "2.0.0".to_string(),
            description: String::new(),
            authors: Vec::new(),
            license: String::new(),
            website: None,
            icon: None,
            distribution: Distribution::Python {
                package: "py-agent==2.0.0".to_string(),
                arguments: vec!["serve".to_string()],
            },
        };

        assert_eq!(
            crate::agent::installation(&agent.id, &root),
            None,
            "installed before anything ran"
        );

        let (sender, events) = std::sync::mpsc::channel();
        super::spawn(&agent, &root, sender);
        let event = events
            .recv_timeout(Duration::from_secs(5))
            .expect("the install to say something");
        assert!(
            matches!(&event, crate::event::Event::Installed { id, failure: None } if id == "py-agent"),
            "not a finished install: {event:?}"
        );

        let installed = crate::agent::installation(&agent.id, &root).expect("the record");
        assert_eq!(installed.command, std::path::PathBuf::from("uvx"));
        assert_eq!(installed.arguments, ["py-agent==2.0.0", "serve"]);
        assert_eq!(installed.version, "2.0.0");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// An install that another obelus is already doing does not run twice.
    ///
    /// They share the agent's directory, and an install is a program
    /// writing a tree into it: two at once is two of them with one prefix,
    /// and the mixed tree they leave says nothing about which half is
    /// which. A python one again, because what is being tested is the
    /// claim rather than the fetching.
    #[tokio::test]
    async fn an_install_another_obelus_is_already_doing_does_not_run() {
        let root = std::env::temp_dir().join(format!("obelus-twice-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let agent = Agent {
            id: "py-agent".to_string(),
            name: "Python one".to_string(),
            version: "2.0.0".to_string(),
            description: String::new(),
            authors: Vec::new(),
            license: String::new(),
            website: None,
            icon: None,
            distribution: Distribution::Python {
                package: "py-agent==2.0.0".to_string(),
                arguments: Vec::new(),
            },
        };

        // The other obelus, holding the claim for as long as this is held.
        let theirs = crate::agent::claim(&agent.id, &root).expect("their claim");

        let (sender, _events) = std::sync::mpsc::channel();
        let outcome = super::install(&agent, &root, std::time::Instant::now(), &sender).await;
        assert!(
            outcome.is_err_and(|why| why.contains("another obelus")),
            "it installed over an install that was already running"
        );
        assert_eq!(
            crate::agent::installation(&agent.id, &root),
            None,
            "it wrote a record for an install it did not do"
        );

        // And once they are finished, it installs.
        drop(theirs);
        super::install(&agent, &root, std::time::Instant::now(), &sender)
            .await
            .expect("installing");
        assert!(
            crate::agent::installation(&agent.id, &root).is_some(),
            "it would not install after the other one finished"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A name obelus would not make a directory of is an install that fails
    /// rather than one that writes somewhere else.
    #[tokio::test]
    async fn an_install_under_an_impossible_name_fails() {
        let root = std::env::temp_dir().join(format!("obelus-escape-{}", std::process::id()));
        let agent = Agent {
            id: "../escape".to_string(),
            name: "Sneaky".to_string(),
            version: "1.0.0".to_string(),
            description: String::new(),
            authors: Vec::new(),
            license: String::new(),
            website: None,
            icon: None,
            distribution: Distribution::Python {
                package: "escape==1.0.0".to_string(),
                arguments: Vec::new(),
            },
        };
        let (sender, events) = std::sync::mpsc::channel();
        super::spawn(&agent, &root, sender);
        let event = events
            .recv_timeout(Duration::from_secs(5))
            .expect("the install to say something");
        assert!(
            matches!(
                &event,
                crate::event::Event::Installed {
                    failure: Some(_),
                    ..
                }
            ),
            "an install under that name did not fail: {event:?}"
        );
        assert!(!root.exists(), "it wrote something anyway");
    }

    /// How much longer, from the rate so far -- and nothing at all when
    /// there is no length to divide by, which is exactly when a package
    /// manager is doing the work.
    #[tokio::test]
    async fn what_is_left_comes_from_the_rate_so_far() {
        // Half of ten megabytes in five seconds: five more to go.
        let half = Progress {
            done: 5 * 1024 * 1024,
            total: Some(10 * 1024 * 1024),
            elapsed: Duration::from_secs(5),
        };
        assert_eq!(half.fraction(), Some(0.5));
        let left = half.remaining().expect("a rate to divide by");
        assert!(
            (left.as_secs_f64() - 5.0).abs() < 0.1,
            "not five seconds: {left:?}"
        );

        // No length: no fraction and no estimate. An invented one is worse
        // than none, because a reader plans around it.
        let blind = Progress {
            done: 1024,
            total: None,
            elapsed: Duration::from_secs(1),
        };
        assert_eq!(blind.fraction(), None);
        assert_eq!(blind.remaining(), None);

        // Nothing yet: there is no rate, so there is no estimate.
        let starting = Progress {
            done: 0,
            total: Some(1024),
            elapsed: Duration::from_millis(10),
        };
        assert_eq!(starting.remaining(), None);
        assert_eq!(starting.fraction(), Some(0.0));

        // Finished: nothing is left, rather than a number close to zero.
        let done = Progress {
            done: 1024,
            total: Some(1024),
            elapsed: Duration::from_secs(2),
        };
        assert_eq!(done.remaining(), None);
        assert_eq!(done.fraction(), Some(1.0));
    }
}
