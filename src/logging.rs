//! Logging, which must never reach stdout.
//!
//! stdout is the drawing surface. A stray line written to it lands in the
//! middle of the rendered frame and stays there, so the subscriber writes to a
//! file and failing to open that file is not allowed to stop obelus starting.

use std::path::{Path, PathBuf};

use tracing_appender::{
    non_blocking::WorkerGuard,
    rolling::{RollingFileAppender, Rotation},
};
use tracing_subscriber::{
    EnvFilter, Layer, layer::SubscriberExt as _, util::SubscriberInitExt as _,
};

/// What obelus writes about itself.
pub const OBELUS: &str = "obelus";

/// What the language servers say, and what obelus said to them.
///
/// Its own file because it is somebody else's program talking: a handshake,
/// a stream of requests and whatever the server writes to its stderr, at a
/// volume that would bury the dozen lines obelus has to say about itself.
/// Two files, two commands, and each of them is readable.
pub const SERVERS: &str = "lsp";

/// Whether an event belongs to the servers' log rather than obelus's own.
///
/// By the module it came from, which `tracing` uses as an event's target
/// unless one is given -- so nothing at the call sites has to know which
/// file it is writing to, and a module moved into `lsp` moves its lines
/// with it.
#[must_use]
pub fn is_server(target: &str) -> bool {
    target == "obelus::lsp" || target.starts_with("obelus::lsp::")
}

/// Installs the file subscriber and returns its flush guards.
///
/// The guards must be held for as long as logging is wanted: dropping them
/// flushes and shuts down the writer threads.
///
/// Verbosity comes from `RUST_LOG`, which is `tracing-subscriber`'s own
/// convention rather than a setting obelus invents.
#[must_use]
pub fn install() -> Option<(WorkerGuard, WorkerGuard)> {
    let directory = log_directory()?;
    std::fs::create_dir_all(&directory).ok()?;

    let (ours, kept) = writer(&directory, OBELUS)?;
    let (theirs, also_kept) = writer(&directory, SERVERS)?;

    // `ob` as well as `obelus`: the binary is its own crate, so the lines
    // main writes -- what started, and that it left -- carry that target
    // and were filtered out of their own log.
    //
    // `tokei=off` because counting a tree warns once per file whose
    // extension it does not know -- `Cargo.lock` alone does it on this
    // repository -- and that is a fact about the tree rather than anything
    // obelus has to say about itself. A line per unrecognised file would
    // bury the dozen obelus writes, which is the thing this log is for.
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("warn,obelus=info,ob=info,tokei=off"));

    // Two layers over one registry, each taking the events the other does
    // not: the split is by target, so an event goes to exactly one file and
    // neither file has to be read with the other in mind.
    //
    // Boxed into a list because a per-layer filter fixes the subscriber it
    // belongs to at the moment it is built, so two of them cannot be
    // stacked one after the other -- they go on together or not at all.
    let ours = tracing_subscriber::fmt::layer()
        // No escape sequences: this is a file, and a pager should not have
        // to strip colour out of it.
        .with_ansi(false)
        .with_writer(Marked(ours))
        .with_filter(tracing_subscriber::filter::filter_fn(|event| {
            !is_server(event.target())
        }))
        .boxed();
    let theirs = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(Marked(theirs))
        .with_filter(tracing_subscriber::filter::filter_fn(|event| {
            is_server(event.target())
        }))
        .boxed();
    tracing_subscriber::registry()
        .with(vec![ours, theirs])
        .with(filter)
        .init();

    Some((kept, also_kept))
}

/// One day-rotated file, and the guard that flushes it.
fn writer(
    directory: &Path,
    prefix: &str,
) -> Option<(tracing_appender::non_blocking::NonBlocking, WorkerGuard)> {
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(prefix)
        .filename_suffix("log")
        .max_log_files(5)
        .build(directory)
        .ok()?;
    Some(tracing_appender::non_blocking(appender))
}

/// A writer that puts this process's number in front of every line.
///
/// obelus does not split its own window -- the terminal does that -- so
/// several of them on one project is the ordinary way to work, and they
/// share one log. Two interleaved stories with nothing to tell them apart
/// are neither of them readable.
///
/// On the line rather than in the filename: one file still rotates as one
/// file, and a reader following what happened does not have to open one
/// per window and put them back in order by hand.
struct Marked<M>(M);

impl<'writer, M: tracing_subscriber::fmt::MakeWriter<'writer>>
    tracing_subscriber::fmt::MakeWriter<'writer> for Marked<M>
{
    type Writer = Whose<M::Writer>;

    fn make_writer(&'writer self) -> Self::Writer {
        Whose {
            inner: self.0.make_writer(),
            said: false,
        }
    }
}

/// One event's writer, which says whose it is before it says anything else.
struct Whose<W> {
    inner: W,
    said: bool,
}

impl<W: std::io::Write> std::io::Write for Whose<W> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        // Once per event: `make_writer` is called for each of them, so the
        // first write of each is the head of a line.
        if !self.said {
            self.said = true;
            write!(self.inner, "{} ", std::process::id())?;
        }
        self.inner.write(buffer)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// Chains a panic hook that writes the panic to the log.
///
/// A panic is the one thing a log has to have and the one thing it had none
/// of: the message goes to stderr, which is behind the alternate screen
/// while obelus is drawing, and the log simply stopped mid-session with no
/// reason in it. Chained, like every other hook here, so whatever was
/// already installed still runs.
pub fn catch_panics() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        // The location rather than a backtrace: a backtrace needs symbols
        // and an environment variable, and the line that panicked is what
        // says where to look.
        match panic.location() {
            Some(where_it_was) => {
                tracing::error!(at = %where_it_was, "panicked: {}", panic);
            }
            None => tracing::error!("panicked: {}", panic),
        }
        previous(panic);
    }));
}

/// The log file being written now, if there is one.
///
/// Found by date on disk rather than by rebuilding the name the appender
/// chose: the rotation and the naming belong to `tracing-appender`, and a
/// second copy of its convention here would be wrong on the first day it
/// changed. Newest wins, which is today's file.
#[must_use]
pub fn current_file(prefix: &str) -> Option<PathBuf> {
    newest_log(&log_directory()?, prefix)
}

/// The newest log file in a directory.
///
/// Split out from [`current_file`] so it can be tested against a directory
/// that exists: the two rules in it -- which names count, and which of them
/// wins -- are both quiet when wrong, and a wrong answer here opens the wrong
/// file or none.
fn newest_log(directory: &Path, prefix: &str) -> Option<PathBuf> {
    let start = format!("{prefix}.");
    std::fs::read_dir(directory)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with(&start) && name.ends_with(".log")
        })
        .filter_map(|entry| {
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, entry.path()))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

/// Where obelus keeps what it has worked out and could work out again.
///
/// State rather than config or data, which is what the directory is for: a
/// reader who deleted all of this would lose nothing they wrote and nothing
/// they chose. The logs are here, and what obelus remembers about the
/// conversations it has had.
#[must_use]
pub fn state_directory() -> Option<PathBuf> {
    if let Some(state) = std::env::var_os("XDG_STATE_HOME") {
        return Some(PathBuf::from(state).join("obelus"));
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".local/state/obelus"))
}

/// Where the log files go.
fn log_directory() -> Option<PathBuf> {
    state_directory()
}

#[cfg(test)]
mod tests {
    /// Every line says which obelus wrote it.
    ///
    /// Several of them share the log, because several of them on one
    /// project is the ordinary way to work. A line that does not say whose
    /// it is belongs to whichever story the reader guesses.
    #[test]
    fn every_line_says_whose_it_is() {
        use std::io::Write as _;

        use tracing_subscriber::fmt::MakeWriter as _;

        #[derive(Clone, Default)]
        struct Kept(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
        impl std::io::Write for Kept {
            fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
                self.0.lock().expect("the lock").extend_from_slice(buffer);
                Ok(buffer.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        impl<'writer> tracing_subscriber::fmt::MakeWriter<'writer> for Kept {
            type Writer = Self;
            fn make_writer(&'writer self) -> Self::Writer {
                self.clone()
            }
        }

        let kept = Kept::default();
        let marked = super::Marked(kept.clone());
        // Two events, each written the way the subscriber writes one: a
        // fresh writer, then the pieces of the line.
        for words in ["starting\n", "leaving\n"] {
            let mut writer = marked.make_writer();
            write!(writer, "INFO ").expect("the level");
            write!(writer, "{words}").expect("the words");
        }

        let written = String::from_utf8(kept.0.lock().expect("the lock").clone()).expect("utf-8");
        let pid = std::process::id();
        assert_eq!(
            written,
            format!("{pid} INFO starting\n{pid} INFO leaving\n"),
            "a line does not say whose it is"
        );
    }

    /// Which file an event goes to, by the module it came from. The split
    /// is the whole point of having two, and it is silent when wrong: a
    /// server's stream in obelus's own log buries it, and obelus's lines in
    /// the servers' log are lost in it.
    #[test]
    fn the_servers_lines_are_told_apart_by_their_module() {
        assert!(super::is_server("obelus::lsp"));
        assert!(super::is_server("obelus::lsp::client"));
        assert!(super::is_server("obelus::lsp::outline"));
        assert!(!super::is_server("obelus::app"));
        assert!(!super::is_server("obelus::app::semantics"));
        assert!(!super::is_server("obelus"));
        // A module whose name starts the same way and is not it.
        assert!(!super::is_server("obelus::lspish"));
    }

    use std::{fs, time::Duration};

    use super::*;

    /// The appender's own naming decides which files are logs, and the newest
    /// is the one being written now. Both are read off the directory rather
    /// than rebuilt here, so both are worth pinning down -- and so is which
    /// of the two logs a name belongs to, because they share a directory.
    #[test]
    fn the_newest_log_wins_and_only_logs_count() {
        let directory =
            std::env::temp_dir().join(format!("obelus-log-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("a directory to test in");

        fs::write(directory.join("obelus.2026-01-01.log"), "old").expect("the old log");
        // The servers' log shares the directory, and is not obelus's own.
        std::thread::sleep(Duration::from_millis(20));
        fs::write(directory.join("lsp.2026-01-03.log"), "theirs").expect("the server log");
        // Written second, so it is the newer of the two whatever the clock
        // does with the names.
        std::thread::sleep(Duration::from_millis(20));
        fs::write(directory.join("obelus.2026-01-02.log"), "new").expect("the new log");
        // Neither of these is a log, however recently it was written.
        std::thread::sleep(Duration::from_millis(20));
        fs::write(directory.join("obelus.log.swp"), "not a log").expect("a decoy");
        fs::write(directory.join("notes.log"), "not ours").expect("another decoy");

        assert_eq!(
            newest_log(&directory, OBELUS),
            Some(directory.join("obelus.2026-01-02.log")),
            "the servers' log, or the older one, was taken for obelus's own"
        );
        assert_eq!(
            newest_log(&directory, SERVERS),
            Some(directory.join("lsp.2026-01-03.log"))
        );

        let empty = directory.join("empty");
        fs::create_dir_all(&empty).expect("an empty directory");
        assert_eq!(newest_log(&empty, OBELUS), None);
        assert_eq!(newest_log(&directory.join("gone"), OBELUS), None);

        let _ = fs::remove_dir_all(&directory);
    }
}
