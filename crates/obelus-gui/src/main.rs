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
//!
//! **Obelus is drawn on two things, and the loop knows neither.** `ob` is
//! the terminal and `obg` is a window -- gvim's relation to vim, not a
//! second program: the same grid, the same `component/` and `ui/`, the same
//! `App`. The seam is one trait and one channel. `ratatui::backend::Backend`
//! is where a screenful of cells becomes escape sequences or becomes quads
//! on a texture (`grid`), and `app::run` is handed the *receiving* end of
//! the loop's channel, because the other end belongs to whichever front end
//! is running (`window`).
//!
//! **A window is not a console program.** On Windows a program says which it
//! is, and one that says console is given a console wherever it starts --
//! from a menu or a double-click, a black window beside Obelus's own for as
//! long as it runs. So `obg` says window. What a console program had for
//! nothing -- `--help`, and a bad path saying so -- comes back by taking the
//! console of whatever started it, where something did; and what it starts
//! is told to put up no console of its own (`obelus_program`).

// Not in a test binary: cargo reads what a test says on the console it
// started it in.
#![cfg_attr(not(test), windows_subsystem = "windows")]

mod blink;
mod cascade;
mod clipboard;
mod elsewhere;
mod faces;
mod font;
mod grid;
mod keys;
mod monospace;
mod motion;
mod paint;
mod title;
mod window;

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use obelus_app::startup;

/// A code editor in a window, with the agent built in.
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

    /// Connect to the chat set in the settings once started, as
    /// `connect-remote` does.
    #[arg(long)]
    connect_remote: bool,
}

fn main() -> Result<()> {
    // Before the arguments, because `--help` is said on it.
    #[cfg(windows)]
    take_the_console_it_was_started_from();
    let arguments = Arguments::parse();
    // Before anything else starts a thread, because changing the
    // environment while another thread reads it is undefined. And before
    // the log, which starts one -- so what happened is said after it.
    #[cfg(unix)]
    let environment = take_the_shells_environment();

    // Held until main returns, so buffered log lines are flushed on the way
    // out.
    let _log_guard = obelus_logging::install(false);
    // Before anything that can panic, so a panic on the way up is in the
    // log as well.
    obelus_logging::catch_panics();
    #[cfg(unix)]
    match environment {
        Ok(Environment::Terminals) => {}
        Ok(Environment::PassedOn) => tracing::info!("kept the environment the window before had"),
        Ok(Environment::Taken(taken)) => {
            tracing::info!(taken, "took the login shell's environment")
        }
        Err(why) => tracing::warn!(%why, "kept the environment the desktop gave"),
    }

    // Before the settings are read, because what Obelus is drawn on
    // decides what some of them mean: the marks are carried here rather
    // than installed, and how big the text is is a question a terminal
    // does not have.
    obelus_config::drawn_in_a_window();

    // Opened before there is a window, so that a bad path reports itself on
    // the console the reader started Obelus from rather than flashing past
    // inside a window that is about to close. The order inside is argued
    // where it lives.
    let mut app = startup::start(&arguments.paths, env!("OBELUS_BUILD"))?;
    if arguments.connect_remote {
        app.remote_at_start();
    }
    // The window's half of the line `startup::start` just wrote, which is
    // the first thing to suspect when a key or a colour did not do what it
    // should.
    tracing::info!(
        session = ?std::env::var("XDG_SESSION_TYPE").ok(),
        "drawn in a window"
    );

    window::show(app)
}

/// Where the environment `obg` runs with came from.
#[cfg(unix)]
enum Environment {
    /// The terminal it was started from, which read the reader's files.
    Terminals,
    /// The window that started this one, which had one of the others.
    PassedOn,
    /// The reader's login shell, this many variables of it.
    Taken(usize),
}

/// Takes the environment the reader's login shell has, where `obg` was not
/// started from one.
///
/// A window opened from the Dock or a launcher has the desktop's
/// environment, which has none of what the reader's shell files set: no
/// `/opt/homebrew/bin` on the path, so no `npm` and no `node` for an agent
/// to run on. Why the shell is asked the way it is, is argued in
/// `obelus_program::login`. Not Windows, where the path is the registry's
/// and the desktop hands every program all of it.
///
/// Any of the three standard files on a terminal is the sign of a shell:
/// `obg` typed into one already has everything it set, and asking would
/// put the files' answer over the terminal's -- a virtualenv's path, a
/// variable exported a moment ago -- even where its input was sent
/// somewhere else. And a window started by another window has whichever
/// of the two that one had, and is told so (`elsewhere`).
#[cfg(unix)]
fn take_the_shells_environment() -> Result<Environment, String> {
    use std::io::IsTerminal as _;
    let passed_on = std::env::var_os(obelus_program::login::PASSED_ON).is_some();
    if passed_on {
        // Safety: nothing else is running yet -- see `main`. Taken off so
        // that a window this one's agent or terminal starts is not told so.
        unsafe { std::env::remove_var(obelus_program::login::PASSED_ON) };
        return Ok(Environment::PassedOn);
    }
    if std::io::stdin().is_terminal()
        || std::io::stdout().is_terminal()
        || std::io::stderr().is_terminal()
    {
        return Ok(Environment::Terminals);
    }
    let shell = obelus_program::login::the_readers_shell()
        .ok_or("nothing says which shell is the reader's")?;
    let found = obelus_program::login::environment(&shell, std::time::Duration::from_secs(5))?;
    let taken = found.len();
    for (name, value) in found {
        // Safety: nothing else is running yet -- see `main`. The thread
        // `environment` reads the shell's pipe on touches nothing else.
        unsafe { std::env::set_var(name, value) };
    }
    Ok(Environment::Taken(taken))
}

/// Says what follows on the console `obg` was started from, where it was
/// started from one.
///
/// Nothing where it was not -- a start from Explorer or a menu -- which is
/// right: there is nobody to say it to, and a console put up to say it would
/// be the window this is here to be rid of. Nor where its output was sent
/// somewhere already, which it keeps.
#[cfg(windows)]
fn take_the_console_it_was_started_from() {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    // Safety: it takes a constant, and a failure is only an answer.
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(test)]
mod tests {
    /// A window offers the settings a window has, and not the ones a
    /// terminal has.
    ///
    /// Both halves, because each passes with the other broken. The glyph
    /// switch is a terminal's question -- here the face is carried in the
    /// binary and there is nothing to decide -- and the text size is the
    /// opposite: a terminal's font belongs to the terminal.
    ///
    /// Deliberate break: `Setting::shown` answering `true` for everything,
    /// which is what it did before there were two front ends.
    #[test]
    fn a_window_offers_a_window_s_settings() {
        obelus_config::drawn_in_a_window();
        let shown = |key: &str| {
            obelus_config::Setting::named(key)
                .expect("a setting Obelus has")
                .shown()
        };
        assert!(!shown("icons"));
        assert!(shown("font_size"));
        // And what both of them are drawn with is nobody's front end.
        assert!(shown("theme"));
    }
}
