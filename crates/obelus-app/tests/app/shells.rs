//! The reader's shell going, on every platform: `terminal.rs` is Unix's
//! alone, and whether a shell is busy is asked another way on Windows -- so
//! what a reader does to a shell at its prompt is tested here, with `cmd`
//! there and `sh` everywhere else.

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use obelus_app::{app::App, event::Event};

use crate::support;

/// The reader's shell, open and being read.
fn a_shell() -> (App, std::sync::mpsc::Receiver<Event>) {
    let mut app = App::new(Vec::new());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 80, 24);
    app.shell_for_test(PathBuf::from(match cfg!(windows) {
        true => "cmd.exe",
        false => "/bin/sh",
    }));
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TerminalOpen);
    assert!(app.terminal().is_some(), "no terminal opened");
    (app, events)
}

/// A shell at its prompt closes on the key that closes anything, at the
/// first press and asking nothing.
///
/// Broken deliberately by judging the terminal by its program not having
/// ended in `App::asks_before_closing`, as it did once the question had
/// learnt otherwise: the question then had nothing to ask, the close
/// waited on it anyway, and the shell could not be closed at all.
#[test]
fn a_shell_at_its_prompt_closes_at_the_first_press() {
    let (mut app, _events) = a_shell();
    app.handle(Event::Key(KeyEvent::new(
        KeyCode::Char('w'),
        KeyModifiers::CONTROL,
    )));
    assert!(app.terminal().is_none(), "the shell is still open");
}

/// And its tab's cross, which is asked the same question.
///
/// Broken by the same break: the cross does nothing.
#[test]
fn a_shell_at_its_prompt_closes_at_its_cross() {
    let (mut app, _events) = a_shell();
    app.handle(Event::Shut(obelus_buffer::DocumentId::new(0)));
    assert!(app.terminal().is_none(), "the shell is still open");
}

/// Leaving with a shell at its prompt leaves.
///
/// Broken deliberately by counting every shell whose program has not ended
/// in `App::would_stop_something`: leaving asks to stop it.
#[test]
fn leaving_a_shell_at_its_prompt_leaves() {
    let (mut app, _events) = a_shell();
    app.handle(Event::Key(KeyEvent::new(
        KeyCode::Char('q'),
        KeyModifiers::CONTROL,
    )));
    assert!(
        app.should_quit(),
        "asked before leaving a shell at its prompt"
    );
}

/// A shell running something is still asked about, on the key that closes
/// it and on leaving.
///
/// Broken deliberately by answering `Terminal::is_busy` with "no": the
/// shell running `ping` (or `sleep`) is never busy, and the wait for it
/// runs out before either key is pressed.
#[test]
fn a_shell_running_something_is_asked_about() {
    let (mut app, events) = a_shell();
    support::busy_the_terminal(&mut app, &events);
    app.handle(Event::Key(KeyEvent::new(
        KeyCode::Char('w'),
        KeyModifiers::CONTROL,
    )));
    assert!(app.terminal().is_some(), "closed without asking");
    assert_eq!(support::ways(&app), ["Stop it and close", "Cancel"]);
    support::answer(&mut app, "Cancel");
    app.handle(Event::Key(KeyEvent::new(
        KeyCode::Char('q'),
        KeyModifiers::CONTROL,
    )));
    assert!(!app.should_quit(), "left without asking");
}
