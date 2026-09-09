//! Logging, which must never reach stdout.
//!
//! stdout is the drawing surface. A stray line written to it lands in the
//! middle of the rendered frame and stays there, so the subscriber writes to a
//! file and failing to open that file is not allowed to stop obelus starting.

use std::path::PathBuf;

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

/// Where the log files go.
fn log_directory() -> Option<PathBuf> {
    if let Some(state) = std::env::var_os("XDG_STATE_HOME") {
        return Some(PathBuf::from(state).join("obelus"));
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".local/state/obelus"))
}
