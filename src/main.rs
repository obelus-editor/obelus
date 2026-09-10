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

    // Asked before the alternate screen and before raw mode, because
    // asking means writing an escape sequence to the terminal and reading
    // what it writes back. A terminal that answers can draw an agent's own
    // mark on the agents page; one that does not gets a glyph.
    let images = obelus::ui::image::Images::detect();

    // `ratatui::try_init` enters the alternate screen, turns on raw mode, and
    // chains a panic hook that undoes both before the previous hook runs.
    // Without that chaining a panic leaves the terminal in raw mode and the
    // backtrace unreadable.
    let mut terminal = ratatui::try_init()?;
    // Mouse reporting, for the wheel.
    //
    // Without it a terminal in the alternate screen translates the wheel
    // into arrow keys, which arrive indistinguishable from the arrow keys --
    // so the wheel moves the cursor and there is no way to tell it not to.
    // With it the wheel is a wheel and scrolls the view.
    //
    // The cost is the terminal's own text selection: while an application is
    // reading the mouse, dragging is the application's to interpret, and
    // every terminal puts its own selection behind a modifier (shift almost
    // everywhere). That is a real loss for a reader, and it buys the one
    // thing a reader does with a mouse far more often.
    let mouse = enable_mouse();
    let mut app = App::new(buffers);
    // Read here rather than in `App::new`, so that a test gets the defaults
    // rather than whatever the machine it runs on has in `~/.config`.
    app.load_config();
    app.use_images(images);
    let outcome = app::run(&mut terminal, &mut app);
    if mouse {
        let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture);
    }
    ratatui::try_restore()?;

    outcome
}

/// Turns on mouse reporting, and says whether it worked.
///
/// Best effort: a terminal that will not report the mouse is a terminal where
/// the wheel keeps sending arrow keys, which is how obelus behaved before it
/// asked. Not a reason to refuse to start.
fn enable_mouse() -> bool {
    match crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture) {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(%error, "no mouse reporting");
            false
        }
    }
}
