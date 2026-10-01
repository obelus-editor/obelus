//! The `ob` binary: Obelus drawn on a terminal.
//!
//! What is here is the terminal and nothing else -- asking it what it can
//! draw, taking it over, handing it back. Everything that happens before
//! there is a screen is [`obelus_app::startup`], which does not know there
//! is a terminal at all.

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use obelus_app::{app, event, startup};

/// A terminal code reader. It doesn't want you to type.
#[derive(Parser)]
// Named rather than left to the package: clap takes the name from
// `CARGO_PKG_NAME`, which is the crate this binary is built from and not
// the command a reader types. `obelus-cli --version` is a name nobody has.
#[command(name = "ob", version, about)]
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

    // Opened before the terminal is taken over, so a bad path reports
    // itself on a normal screen rather than flashing past inside an
    // alternate one. The order inside is argued where it lives.
    let mut app = startup::start(&arguments.paths, env!("OBELUS_BUILD"))?;
    // The terminal's half of the line `startup::start` just wrote: what was
    // drawing Obelus, which a log read a week later has no other way to
    // learn, and which is the first thing to suspect when a key or a colour
    // did not do what it should.
    tracing::info!(
        term = ?std::env::var("TERM").ok(),
        colours = ?std::env::var("COLORTERM").ok(),
        "drawn on a terminal"
    );

    // Asked before the alternate screen and before raw mode, because
    // asking means writing an escape sequence to the terminal and reading
    // what it writes back. A terminal that answers can draw an agent's own
    // mark on the agents page; one that does not gets a glyph.
    let images = obelus_ui::image::Images::detect(obelus_app::event::remote());

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
    if !mouse {
        // The one thing that goes wrong here and stays wrong: the wheel
        // keeps sending arrow keys, and nothing on screen would otherwise
        // say why.
        app.amiss("The wheel will not be reported, so it moves the cursor");
    }
    // And what the terminal pastes, wrapped so it can be told from typing.
    enable_paste(&mut app);
    // And the keyboard, for the one key Obelus needs that a terminal
    // cannot otherwise report.
    let keyboard = enable_keyboard();
    app.use_images(images);

    // The channel is made here, and the thread that fills it with what the
    // terminal says is started here, because that thread is the terminal's
    // half of the loop and this file is where the terminal lives. The loop
    // itself is handed the far end and does not know which front end it is
    // waiting on.
    let (sender, events) = event::channel();
    event::spawn_terminal_reader(sender.clone());
    app.start(sender);

    let outcome = app::run(&mut terminal, &mut app, events);
    startup::finish(&outcome);

    if mouse {
        let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture);
    }
    let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableBracketedPaste);
    if keyboard {
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::event::PopKeyboardEnhancementFlags
        );
    }
    if let Err(error) = ratatui::try_restore() {
        tracing::warn!(%error, "the terminal was not put back");
    }

    outcome
}

/// Asks the terminal to tell Obelus which key was pressed, and says whether
/// the request went out.
///
/// A traditional terminal sends the same byte for `enter` and `shift+enter`
/// -- a carriage return -- so a program cannot tell them apart, and
/// `shift+enter` is how a paragraph is written in the box a message to an
/// agent goes in. The way out is the kitty keyboard protocol, and the
/// narrowest flag of it is enough: disambiguate the escape codes, which
/// makes a modified key arrive as itself and a modifier rather than as
/// somebody else's byte. It does not turn on key releases, or every
/// modifier press, which would be an event a frame for nothing.
///
/// foot, kitty, ghostty, wezterm and alacritty implement it; a terminal that
/// does not ignores the sequence, and there `shift+enter` sends the message
/// -- which is why `alt+enter` breaks the line as well, and always has.
///
/// Popped on the way out, and from a panic hook: these flags are terminal
/// state, and a program that leaves them pushed leaves the reader's shell
/// receiving escape sequences it does not expect.
///
/// Nothing else in Obelus depends on the protocol.
fn enable_keyboard() -> bool {
    use crossterm::event::{KeyboardEnhancementFlags, PushKeyboardEnhancementFlags};

    let pushed = crossterm::execute!(
        std::io::stdout(),
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    );
    if let Err(error) = pushed {
        tracing::warn!(%error, "no keyboard protocol");
        return false;
    }
    // Wrapped around whatever hook is already there -- ratatui's, which
    // leaves the alternate screen -- so this runs first and the screen is
    // put back after.
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic| {
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::event::PopKeyboardEnhancementFlags
        );
        previous(panic);
    }));
    true
}

/// Asks the terminal to wrap what it pastes.
///
/// Without it a pasted function arrives as somebody typing very fast, and
/// every newline in it does whatever `Enter` does. With it, the whole of it
/// arrives at once and goes in as one change a reader can undo in one step.
///
/// Best effort, like the mouse: a terminal that does not know the mode says
/// nothing and pastes the old way.
fn enable_paste(app: &mut obelus_app::app::App) {
    if let Err(error) =
        crossterm::execute!(std::io::stdout(), crossterm::event::EnableBracketedPaste)
    {
        tracing::warn!(%error, "no bracketed paste");
        app.amiss("Pasting will arrive a character at a time, not in one go");
    }
}

/// Turns on mouse reporting, and says whether it worked.
///
/// Best effort: a terminal that will not report the mouse is a terminal where
/// the wheel keeps sending arrow keys, which is how Obelus behaved before it
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
