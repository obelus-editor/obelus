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
use tracing_subscriber::EnvFilter;

/// Installs the file subscriber and returns its flush guard.
///
/// The guard must be held for as long as logging is wanted: dropping it flushes
/// and shuts down the writer thread.
///
/// Verbosity comes from `RUST_LOG`, which is `tracing-subscriber`'s own
/// convention rather than a setting obelus invents.
#[must_use]
pub fn install() -> Option<WorkerGuard> {
    let directory = log_directory()?;
    std::fs::create_dir_all(&directory).ok()?;

    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("obelus")
        .filename_suffix("log")
        .max_log_files(5)
        .build(&directory)
        .ok()?;
    let (writer, guard) = tracing_appender::non_blocking(appender);

    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn,obelus=info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        // No escape sequences: this is a file, and a pager should not have to
        // strip colour out of it.
        .with_ansi(false)
        .init();

    Some(guard)
}

/// The log file being written now, if there is one.
///
/// Found by date on disk rather than by rebuilding the name the appender
/// chose: the rotation and the naming belong to `tracing-appender`, and a
/// second copy of its convention here would be wrong on the first day it
/// changed. Newest wins, which is today's file.
#[must_use]
pub fn current_file() -> Option<PathBuf> {
    newest_log(&log_directory()?)
}

/// The newest log file in a directory.
///
/// Split out from [`current_file`] so it can be tested against a directory
/// that exists: the two rules in it -- which names count, and which of them
/// wins -- are both quiet when wrong, and a wrong answer here opens the wrong
/// file or none.
fn newest_log(directory: &Path) -> Option<PathBuf> {
    std::fs::read_dir(directory)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("obelus.") && name.ends_with(".log")
        })
        .filter_map(|entry| {
            let modified = entry.metadata().ok()?.modified().ok()?;
            Some((modified, entry.path()))
        })
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

/// Where the log files go.
fn log_directory() -> Option<PathBuf> {
    if let Some(state) = std::env::var_os("XDG_STATE_HOME") {
        return Some(PathBuf::from(state).join("obelus"));
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".local/state/obelus"))
}

#[cfg(test)]
mod tests {
    use std::{fs, time::Duration};

    use super::*;

    /// The appender's own naming decides which files are logs, and the newest
    /// is the one being written now. Both are read off the directory rather
    /// than rebuilt here, so both are worth pinning down.
    #[test]
    fn the_newest_log_wins_and_only_logs_count() {
        let directory =
            std::env::temp_dir().join(format!("obelus-log-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("a directory to test in");

        fs::write(directory.join("obelus.2026-01-01.log"), "old").expect("the old log");
        // Written second, so it is the newer of the two whatever the clock
        // does with the names.
        std::thread::sleep(Duration::from_millis(20));
        fs::write(directory.join("obelus.2026-01-02.log"), "new").expect("the new log");
        // Neither of these is a log, however recently it was written.
        std::thread::sleep(Duration::from_millis(20));
        fs::write(directory.join("obelus.log.swp"), "not a log").expect("a decoy");
        fs::write(directory.join("notes.log"), "not ours").expect("another decoy");

        assert_eq!(
            newest_log(&directory),
            Some(directory.join("obelus.2026-01-02.log"))
        );

        let empty = directory.join("empty");
        fs::create_dir_all(&empty).expect("an empty directory");
        assert_eq!(newest_log(&empty), None);
        assert_eq!(newest_log(&directory.join("gone")), None);

        let _ = fs::remove_dir_all(&directory);
    }
}
