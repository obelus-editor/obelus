//! Installing an agent, in the background.
//!
//! Two shapes, and they can say different things while they run. A package
//! manager is asked to do the whole job and reports nothing until it is
//! finished, so the page can only say that it is running and for how long.
//! An archive is a download of a known length, so the page can say how far
//! through it is and how much longer it will take -- and it says so because
//! it is true, not because a progress bar is nice to look at.

use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::mpsc::Sender,
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};

use super::{Agent, Distribution};
use crate::event::Event;

/// How much of a download to read at a time.
///
/// Big enough that the read is not the cost, small enough that the progress
/// moves: at a megabyte a chunk, a ten-megabyte agent would report five
/// times and look stuck in between.
const CHUNK: usize = 64 * 1024;

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
pub fn spawn(agent: &Agent, root: &Path, sender: Sender<Event>) {
    let agent = agent.clone();
    let root = root.to_path_buf();
    let outcome = std::thread::Builder::new()
        .name(format!("obelus-install-{}", agent.id))
        .spawn(move || {
            let started = Instant::now();
            let outcome = match &agent.distribution {
                Distribution::Node { package, .. } => node(package, &super::home(&agent.id, &root)),
                Distribution::Python { .. } => Ok(()),
                Distribution::Archive {
                    archive,
                    sha256,
                    command,
                    ..
                } => download(
                    &agent.id,
                    &agent.version,
                    archive,
                    sha256.as_deref(),
                    command,
                    &root,
                    started,
                    &sender,
                ),
            };
            let _ = sender.send(Event::Installed {
                id: agent.id,
                failure: outcome.err(),
            });
        });
    if let Err(error) = outcome {
        tracing::warn!(%error, "not installing");
    }
}

/// Asks `npm` for a package, into this agent's own directory.
///
/// `--prefix` and nothing global: obelus installing into a place the reader
/// shares with everything else on the machine is obelus deciding for them.
/// And a prefix per agent, because the prefix is where npm keeps the
/// manifest -- one shared between agents would be rewritten by whichever
/// was installed last.
fn node(package: &str, home: &Path) -> Result<(), String> {
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
            std::io::ErrorKind::NotFound => "npm is not on the path".to_string(),
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
        .unwrap_or("npm would not say why")
        .trim()
        .to_string())
}

/// Fetches an archive, checks it, and unpacks it.
#[expect(
    clippy::too_many_arguments,
    reason = "one job's worth of facts: what to fetch, what to check it against, where to put it, and who to tell"
)]
fn download(
    id: &str,
    version: &str,
    archive: &str,
    sha256: Option<&str>,
    command: &str,
    root: &Path,
    started: Instant,
    sender: &Sender<Event>,
) -> Result<(), String> {
    let http = ureq::Agent::config_builder()
        .user_agent(concat!("obelus/", env!("CARGO_PKG_VERSION")))
        .build()
        .new_agent();
    let mut response = http
        .get(archive)
        .call()
        .map_err(|error| format!("fetching it: {error}"))?;
    let total = response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());

    let mut body = response.body_mut().as_reader();
    let mut bytes: Vec<u8> = Vec::with_capacity(total.unwrap_or(0) as usize);
    let mut buffer = vec![0u8; CHUNK];
    let mut said = Instant::now();
    loop {
        let read = body
            .read(&mut buffer)
            .map_err(|error| format!("reading it: {error}"))?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
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

    if let Some(expected) = sha256 {
        let digest = Sha256::digest(&bytes);
        let got: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        if !got.eq_ignore_ascii_case(expected) {
            return Err("it is not the file the registry describes".to_string());
        }
    } else {
        // Half the registry's entries have no checksum. Not a reason to
        // refuse -- the URL came from the registry over TLS -- but worth
        // saying somewhere.
        tracing::debug!(id, "the registry gave no checksum for this one");
    }

    let into = super::home(id, root);
    let _ = std::fs::remove_dir_all(&into);
    std::fs::create_dir_all(&into).map_err(|error| format!("{into:?}: {error}"))?;
    unpack(&bytes, archive, &into)?;

    // Unpacked, and the thing to run has to be there and be runnable: an
    // archive that unpacked to something else is a failure now rather than
    // a mystery the first time the reader tries to talk to it.
    let program = into.join(command.trim_start_matches("./"));
    if !program.exists() {
        return Err(format!("no {command} in the archive"));
    }
    make_runnable(&program);
    // What was unpacked, written down: an archive says nothing about its
    // own version once it is a directory, and without this obelus could
    // never tell the reader that there is a newer one.
    let stamp = into.join(super::STAMP);
    if let Err(error) = std::fs::write(&stamp, version) {
        tracing::debug!(%error, "not recording which version was installed");
    }
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

    use super::Progress;

    /// How much longer, from the rate so far -- and nothing at all when
    /// there is no length to divide by, which is exactly when a package
    /// manager is doing the work.
    #[test]
    fn what_is_left_comes_from_the_rate_so_far() {
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
