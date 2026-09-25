//! The `obg` binary: Obelus drawn in a window.
//!
//! The same Obelus. What is here is the window and nothing else -- opening
//! it, telling it what the reader did, putting its cells on the screen --
//! and everything that happens before there is a screen is
//! [`obelus_app::startup`], which is shared with `ob` and does not know
//! there is a window at all.
//!
//! Why there is a second binary, when a terminal draws Obelus perfectly
//! well: two things a terminal cannot give. A terminal cannot tell `ctrl+i`
//! from `Tab`, `ctrl+m` from `Enter` or `ctrl+[` from `Escape` -- one byte
//! each -- and most of them send nothing for `ctrl+shift+X`, so a key table
//! is written around what the terminal will pass on. And a terminal draws
//! with the font the reader installed, which is why the marks are behind a
//! switch that is a guess about somebody else's machine. Here the presses
//! arrive as themselves and the marks are compiled into the binary.

mod font;
mod grid;
mod keys;
mod paint;
mod window;

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use obelus_app::startup;

/// A code reader in a window. It doesn't want you to type.
#[derive(Parser)]
// Named rather than left to the package, the same as `ob`: clap takes the
// name from the crate, and `obelus-gui --version` is a name nobody has.
#[command(name = "obg", version, about)]
struct Arguments {
    /// What to open: a file, or a directory to work in.
    ///
    /// A file names the tree it is in and is opened. A directory is the
    /// tree itself, and Obelus opens on the list of what is in it.
    paths: Vec<PathBuf>,
}

fn main() -> Result<()> {
    let arguments = Arguments::parse();

    // Held until main returns, so buffered log lines are flushed on the way
    // out.
    let _log_guard = obelus_logging::install();
    // Before anything that can panic, so a panic on the way up is in the
    // log as well.
    obelus_logging::catch_panics();

    // Opened before there is a window, so that a bad path reports itself on
    // the console the reader started Obelus from rather than flashing past
    // inside a window that is about to close. The order inside is argued
    // where it lives.
    let mut app = startup::start(&arguments.paths, env!("OBELUS_BUILD"))?;
    // The window's half of the line `startup::start` just wrote, which is
    // the first thing to suspect when a key or a colour did not do what it
    // should.
    tracing::info!(
        session = ?std::env::var("XDG_SESSION_TYPE").ok(),
        "drawn in a window"
    );

    // Said rather than left to a default: a picture on the agents page is
    // drawn by handing a terminal pixels in one of three protocols, and a
    // window speaks none of them. The glyph is what is drawn instead, which
    // is the same fallback a terminal that cannot answer gets -- and the
    // marks are carried in the binary, so it is not the compromise it is
    // there.
    app.use_images(obelus_ui::image::Images::none());
    // The marks, which here are a fact about the binary rather than a guess
    // about the machine.
    obelus_icons::use_glyphs(true);

    window::show(app)
}
