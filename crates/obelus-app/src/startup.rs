//! Everything obelus does before there is a screen.
//!
//! The order is the part worth having a name: the paths are read before
//! anything takes the screen, the tree is settled before the settings that
//! belong to it, and the first line of the log is written before either. It
//! lived in `main` while there was one `main`, where it read as a hundred
//! lines that happened to be in that order rather than as an order with
//! reasons.
//!
//! What is deliberately *not* here is anything that knows how obelus is
//! drawn. Taking over a terminal -- the alternate screen, raw mode, the
//! mouse, the keyboard protocol -- and handing it back belong to whoever
//! does the drawing, and so does asking it what it can do.

use std::path::PathBuf;

use anyhow::Result;
use obelus_buffer::Buffer;

use crate::app::{self, App};

/// Opens what the command line named, and builds the application round it.
///
/// Called before there is a screen, and that is not incidental: a path that
/// cannot be read reports itself on an ordinary screen rather than flashing
/// past inside an alternate one. Whoever calls this takes the screen
/// afterwards, not before.
///
/// `built` is the commit obelus was built at, which only the binary knows:
/// it comes from a build script, and a build script's `rustc-env` reaches
/// the crate it belongs to and nothing else. Handed in, the same way
/// [`App::built_at`] already takes it.
pub fn start(paths: &[PathBuf], built: &'static str) -> Result<App> {
    // What the paths mean: which tree, which files, and whether the
    // question left over is "which file".
    let opening = app::opening(paths);

    // The first line of every session, and the one a reader of the log
    // needs before any other: which obelus this is, where it was run, and
    // what it was asked for. Without it there is no telling which run is
    // being read, or that a run happened at all.
    //
    // What was *drawing* it is not here, because this does not know: the
    // frontend says that in a line of its own, next.
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        // And which build, because the version does not move between
        // releases and a day's work is a hundred builds of `0.1.0`. The
        // commit it was built at, and nothing about whether the tree had
        // been edited since -- see the build script for why that cannot be
        // answered from there.
        built,
        directory = ?std::env::current_dir().ok(),
        // And the tree obelus settled on, which the arguments may have
        // moved: every path in the rest of the log is relative to it.
        tree = ?opening.root,
        paths = paths.len(),
        opens = opening.files.len(),
        list = opening.list,
        "obelus starting"
    );

    let buffers = opening
        .files
        .iter()
        .map(|path| Buffer::open(path))
        .collect::<Result<Vec<_>>>()?;

    let mut app = App::new(buffers);
    // Before the settings, because a tree has settings of its own and
    // reading those means knowing which tree.
    if let Some(root) = opening.root {
        app.work_in(root);
    }
    if opening.list {
        app.list_at_start();
    }
    // Read here rather than in `App::new`, so that a test gets the defaults
    // rather than whatever the machine it runs on has in `~/.config`.
    app.load_config();
    // The welcome screen says it too, because the log is not where a reader
    // looks when they want to know what they are looking at.
    app.built_at(built);
    Ok(app)
}

/// Says how the session ended.
///
/// The other end of the line [`start`] wrote, and said before the screen is
/// handed back: a log that stops without one of these ended in a panic or a
/// kill, and handing the screen back is itself a thing that can fail.
pub fn finish(outcome: &Result<()>) {
    match outcome {
        Ok(()) => tracing::info!("obelus leaving"),
        Err(error) => tracing::error!(%error, "obelus stopping on an error"),
    }
}
