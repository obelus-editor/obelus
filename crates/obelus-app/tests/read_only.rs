//! A reader who has the agent make every change.
//!
//! Their own keys change no file -- typing, a paste, undo, the commands that
//! rewrite lines -- and the boxes they type into are not files: a message to
//! the agent is how the code gets changed at all.

mod support;

use crossterm::event::KeyCode;
use obelus_app::{app::App, event::Event};
use obelus_buffer::Buffer;
use obelus_command::Command;

/// A file with one line in it, open, under a reader whose keys change nothing.
fn read_only(name: &str, contents: &str) -> (support::Scratch, App) {
    let scratch = support::Scratch::new(name);
    let path = scratch.path().join("line.txt");
    std::fs::write(&path, contents).expect("writing it");
    let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.configure(
        obelus_config::Config {
            read_only: true,
            ..obelus_config::Config::default()
        },
        Vec::new(),
    );
    support::lay_out(&mut app, 60, 10);
    (scratch, app)
}

/// What the open file says now.
fn text(app: &App) -> String {
    app.current_buffer()
        .expect("the file")
        .text()
        .rope()
        .to_string()
}

/// Deliberate break: dropping the guard in `App::editor_key` puts `xy` in
/// and takes a character out with the backspace.
#[test]
fn what_is_typed_goes_nowhere() {
    let (_scratch, mut app) = read_only("read-only-typing", "abcd\n");
    support::press(&mut app, KeyCode::Right);
    support::type_text(&mut app, "xy");
    support::press(&mut app, KeyCode::Backspace);
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(text(&app), "abcd\n");
}

/// A terminal's own paste and a window's input method arrive as text rather
/// than as a command, so the palette's refusal never sees them.
///
/// Deliberate break: dropping the guard in `App::paste_text` puts `pasted`
/// at the start of the line.
#[test]
fn a_paste_into_the_file_goes_nowhere() {
    let (_scratch, mut app) = read_only("read-only-paste", "abcd\n");
    app.handle(Event::Paste("pasted".to_string()));
    assert_eq!(text(&app), "abcd\n");
}

/// Deliberate break: putting the guard in `App::paste_text` above the
/// layers, rather than at the file, leaves the palette's query empty.
#[test]
fn a_paste_into_a_box_still_arrives() {
    let (_scratch, mut app) = read_only("read-only-box", "abcd\n");
    obelus_app::app::dispatch::dispatch(&mut app, Command::CommandPalette);
    app.handle(Event::Paste("pasted".to_string()));
    assert_eq!(app.picker().expect("the palette").query(), "pasted");
    assert_eq!(text(&app), "abcd\n");
}

/// Undo too: what is in the file's history is mostly the agent's now, and
/// the reader asked that their keys take nothing out of a file either.
///
/// Deliberate break: dropping the guard in `App::offers` takes the `x`
/// back out with `ctrl+z`.
#[test]
fn undo_takes_nothing_back() {
    let (_scratch, mut app) = read_only("read-only-undo", "abcd\n");
    app.configure(obelus_config::Config::default(), Vec::new());
    support::type_text(&mut app, "x");
    app.configure(
        obelus_config::Config {
            read_only: true,
            ..obelus_config::Config::default()
        },
        Vec::new(),
    );
    assert!(!app.offers(Command::Undo), "undo is offered");
    support::press_control(&mut app, 'z');
    assert_eq!(text(&app), "xabcd\n");
}

/// Deliberate break: dropping the guard in `App::offers` offers every one of
/// them.
#[test]
fn the_commands_that_change_a_file_are_dim() {
    let (_scratch, app) = read_only("read-only-commands", "abcd\n");
    for command in [
        Command::Paste,
        Command::SelectionCut,
        Command::LineUp,
        Command::LineDown,
        Command::ReplaceToggle,
        Command::SymbolRename,
        Command::FileRename,
    ] {
        assert!(!app.offers(command), "{} is offered", command.name());
    }
    // What an agent wrote into an open file reaches the disk by being
    // saved, so saving is the one thing a reader's keys still do to it.
    assert!(app.offers(Command::FileSave), "saving is refused");
    assert!(app.offers(Command::SelectionCopy), "copying is refused");
}

/// Deliberate break: dropping `into_a_box` from `App::offers` refuses the
/// paste with the palette open.
#[test]
fn a_paste_is_offered_where_there_is_a_box() {
    let (_scratch, mut app) = read_only("read-only-offered", "abcd\n");
    obelus_app::app::dispatch::dispatch(&mut app, Command::CommandPalette);
    assert!(app.offers(Command::Paste));
}

/// A letter that goes nowhere has to have a reason on screen.
///
/// Deliberate break: dropping the word from `StatusView::render_file`.
#[test]
fn the_status_row_says_so() {
    let (_scratch, mut app) = read_only("read-only-status", "abcd\n");
    let dump = support::render(&mut app, 60, 10);
    assert!(
        support::text_block(&dump).contains("Read only"),
        "nothing says so:\n{dump}"
    );
}
