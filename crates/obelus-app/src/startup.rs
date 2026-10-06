//! Everything Obelus does before there is a screen.
//!
//! The order is the part worth having a name: the paths are read before
//! anything takes the screen, the project is settled before the settings that
//! belong to it, and the first line of the log is written before either. It
//! lived in `main` while there was one `main`, where it read as a hundred
//! lines that happened to be in that order rather than as an order with
//! reasons.
//!
//! What is deliberately *not* here is anything that knows how Obelus is
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
/// `built` is the commit Obelus was built at, which only the binary knows:
/// it comes from a build script, and a build script's `rustc-env` reaches
/// the crate it belongs to and nothing else. Handed in, the same way
/// [`App::built_at`] already takes it.
pub fn start(paths: &[PathBuf], built: &'static str) -> Result<App> {
    build(paths, built, false)
}

/// The same, for an Obelus nobody is at the screen of: `ob --headless`.
///
/// Refused here, before anything has started, for what it cannot do
/// without a reader: a file named is a file it would open, and an agent's
/// writes to it would wait there for a save nobody makes; no project is a
/// page asking which; and a chat not set or not paired is nothing to be
/// reached from. See `app::headless`.
///
/// # Errors
///
/// The paths, as [`start`]; and what it cannot do, in the words it says
/// it in.
pub fn start_headless(paths: &[PathBuf], built: &'static str) -> Result<App> {
    let mut app = build(paths, built, true)?;
    app.ready_to_be_reached().map_err(anyhow::Error::msg)?;
    app.remote_at_start();
    Ok(app)
}

fn build(paths: &[PathBuf], built: &'static str, headless: bool) -> Result<App> {
    // What the paths mean: which project, which files, and whether the
    // question left over is "which file".
    let opening = app::opening(paths);

    // The first line of every session, and the one a reader of the log
    // needs before any other: which Obelus this is, where it was run, and
    // what it was asked for. Without it there is no telling which run is
    // being read, or that a run happened at all.
    //
    // What was *drawing* it is not here, because this does not know: the
    // frontend says that in a line of its own, next.
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        // And which build, because the version does not move between
        // releases and a day's work is a hundred builds of `0.1.0`. The
        // commit it was built at, and nothing about whether the project had
        // been edited since -- see the build script for why that cannot be
        // answered from there.
        built,
        directory = ?std::env::current_dir().ok(),
        // And the project Obelus settled on, which the arguments may have
        // moved: every path in the rest of the log is relative to it.
        project = ?opening.root,
        paths = paths.len(),
        opens = opening.files.len(),
        list = opening.list,
        "Obelus starting"
    );

    if headless && !opening.files.is_empty() {
        anyhow::bail!("Headless, Obelus opens no file: name the project's directory instead");
    }

    let buffers = opening
        .files
        .iter()
        .map(|path| Buffer::open(path))
        .collect::<Result<Vec<_>>>()?;

    let mut app = App::new(buffers);
    if headless {
        app.headless();
    }
    // Before the settings, because a project has settings of its own and
    // reading those means knowing which project.
    //
    // Told even where the arguments named none, and the answer is the
    // directory `App::new` already has: being told is what reads which
    // branch the tree is on, so a bare `ob` drew no branch until git
    // happened to write its index.
    // Which project, and whether anybody has said. An argument is one
    // answer; standing in a worktree is the other, and it is a real one --
    // a reader who typed `cd` has said where they work as plainly as a
    // reader who typed a path. Neither is a start with nothing to go on:
    // a desktop launcher begins the process in the home directory, which
    // git has never heard of, and Obelus used to take that for the
    // project and file everything about the session under it.
    //
    // Asked of the directory rather than of the arguments, which is the
    // question meant: `cd project && ob` has an answer and should not be
    // interrupted for one.
    let here = app.working_directory().to_path_buf();
    let root = opening
        .root
        .or_else(|| obelus_git::worktree(&here).map(|_| here.clone()));
    match root {
        Some(root) => {
            app.work_in(root);
            if opening.list {
                app.list_at_start();
            }
        }
        // Nothing to go on, so Obelus asks, and the asking is the whole
        // of the screen until it is answered. `work_in` is deliberately
        // not called: there is no project to be put on yet, and saying
        // there is would be the bug this replaces.
        None if headless => {
            anyhow::bail!(
                "Headless, Obelus needs a project: name its directory, or start it in one"
            )
        }
        None => app.ask_which_project(),
    }
    // Read here rather than in `App::new`, so that a test gets the defaults
    // rather than whatever the machine it runs on has in `~/.config`.
    app.load_config();
    // After the settings, because whether to is one of them -- and a
    // project's own file may say. Beside whatever the paths named.
    app.reopen_what_was_open();
    Ok(app)
}

/// Says how the session ended.
///
/// The other end of the line [`start`] wrote, and said before the screen is
/// handed back: a log that stops without one of these ended in a panic or a
/// kill, and handing the screen back is itself a thing that can fail.
pub fn finish(outcome: &Result<()>) {
    match outcome {
        Ok(()) => tracing::info!("Obelus leaving"),
        Err(error) => tracing::error!(%error, "Obelus stopping on an error"),
    }
}
