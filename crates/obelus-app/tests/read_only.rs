//! A reader who has the agent make every change.
//!
//! Their own keys change no file -- typing, a paste, undo, the commands that
//! rewrite lines -- and the boxes they type into are not files: a message to
//! the agent is how the code gets changed at all.

mod support;

use crossterm::event::KeyCode;
use obelus_app::{
    app::{App, Caret},
    event::Event,
};
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

/// A window opens an input method only where what it spells will land, so
/// one spelling into a file would draw the word and then lose it.
///
/// Deliberate break: dropping the guard from `App::takes_text`.
#[test]
fn no_input_method_spells_into_the_file() {
    let (_scratch, app) = read_only("read-only-spelling", "abcd\n");
    assert!(!app.takes_text());
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

/// Every one of them offered with the keys on and refused with them off, so
/// what dims them is the setting and not the file. A Rust file, because a
/// comment is only offered where the language has one.
///
/// Deliberate break: taking any one of them out of
/// `Command::changes_a_file`, or dropping the guard in `App::offers`.
#[test]
fn the_commands_that_change_a_file_are_dim() {
    let scratch = support::Scratch::new("read-only-commands");
    let path = scratch.path().join("main.rs");
    std::fs::write(&path, "fn main() {}\n").expect("writing it");
    let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, 60, 10);
    // Something to undo and something to redo.
    support::type_text(&mut app, "x");
    support::press(&mut app, KeyCode::End);
    support::type_text(&mut app, "y");
    support::press_control(&mut app, 'z');

    let listed = [
        Command::Paste,
        Command::SelectionCut,
        Command::Undo,
        Command::Redo,
        Command::LineUp,
        Command::LineDown,
        Command::CommentToggle,
        Command::ReplaceToggle,
        Command::SymbolComplete,
        Command::SymbolRename,
        Command::CodeActions,
        Command::FileRename,
    ];
    for command in listed {
        assert!(
            app.offers(command),
            "{} is not offered at all",
            command.name()
        );
    }
    app.configure(
        obelus_config::Config {
            read_only: true,
            ..obelus_config::Config::default()
        },
        Vec::new(),
    );
    for command in listed {
        assert!(!app.offers(command), "{} is offered", command.name());
    }
    // What an agent wrote into an open file reaches the disk by being
    // saved, so saving is the one thing a reader's keys still do to it.
    assert!(app.offers(Command::FileSave), "saving is refused");
    assert!(app.offers(Command::SelectionCopy), "copying is refused");
}

/// Saving writes what is in the buffer, which under this setting is the
/// agent's work: refusing it would leave that work with nowhere to go.
///
/// Deliberate break: putting `FileSave` in `Command::changes_a_file`.
#[test]
fn what_is_unsaved_is_still_written() {
    let (_scratch, mut app) = read_only("read-only-save", "abcd\n");
    let path = app.current_buffer().expect("the file").path().to_path_buf();
    app.configure(obelus_config::Config::default(), Vec::new());
    support::type_text(&mut app, "x");
    app.configure(
        obelus_config::Config {
            read_only: true,
            ..obelus_config::Config::default()
        },
        Vec::new(),
    );
    support::press_control(&mut app, 's');
    assert_eq!(
        std::fs::read_to_string(&path).expect("reading it back"),
        "xabcd\n"
    );
}

/// Deliberate break: refusing every letter at the top of `App::handle_key`
/// rather than in `App::editor_key`.
#[test]
fn what_is_typed_into_a_box_still_arrives() {
    let (_scratch, mut app) = read_only("read-only-query", "abcd\n");
    obelus_app::app::dispatch::dispatch(&mut app, Command::CommandPalette);
    support::type_text(&mut app, "ab");
    assert_eq!(app.picker().expect("the palette").query(), "ab");
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

/// Replacing that was on before stays off while the key that turns it off is
/// refused, rather than sitting on the row and in the caret for nothing.
///
/// Deliberate break: `App::replacing` answering with the field alone.
#[test]
fn replacing_does_not_outlive_the_keys() {
    let (_scratch, mut app) = read_only("read-only-replacing", "abcd\n");
    app.configure(obelus_config::Config::default(), Vec::new());
    app.toggle_replacing();
    app.configure(
        obelus_config::Config {
            read_only: true,
            ..obelus_config::Config::default()
        },
        Vec::new(),
    );
    assert_eq!(app.caret(), Caret::Bar);
    let dump = support::render(&mut app, 60, 10);
    assert!(
        !support::text_block(&dump).contains("Replacing"),
        "the row still says so:\n{dump}"
    );
}
