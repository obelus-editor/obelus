//! The `ob` binary.

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use obelus::{
    app::{self, App},
    buffer::Buffer,
    logging,
};

/// A terminal code reader. It doesn't want you to type.
#[derive(Parser)]
#[command(version, about)]
struct Arguments {
    /// Files to open.
    paths: Vec<PathBuf>,
}

fn main() -> Result<()> {
    let arguments = Arguments::parse();

    // Held until main returns, so buffered log lines are flushed on the way
    // out.
    let _log_guard = logging::install();

    // Opened before the terminal is taken over, so a bad path reports itself
    // on a normal screen rather than flashing past inside an alternate one.
    let buffers = arguments
        .paths
        .iter()
        .map(|path| Buffer::open(path))
        .collect::<Result<Vec<_>>>()?;

    // `ratatui::try_init` enters the alternate screen, turns on raw mode, and
    // chains a panic hook that undoes both before the previous hook runs.
    // Without that chaining a panic leaves the terminal in raw mode and the
    // backtrace unreadable.
    let mut terminal = ratatui::try_init()?;
    let mut app = App::new(buffers);
    let outcome = app::run(&mut terminal, &mut app);
    ratatui::try_restore()?;

    outcome
}
