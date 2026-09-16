//! What is over the file, and what it does and does not let past.
//!
//! Every one of these is driven through `App::handle` and asserted on the
//! application rather than on cells. A golden screen says "this looks like
//! this"; it can never say "no screen can look like both", which is what a
//! rule about stacking needs.
//!
//! Every one of them walks `layers::STACK` rather than a list of its own, so
//! a view added without an answer here is a view these tests will not
//! compile without.

mod support;

use crossterm::event::KeyCode;
use obelus::{
    app::{App, layers::Layer},
    command::{Command, dispatch},
};
use support::press;

/// Wide enough for the settings page's two columns, tall enough for a list.
const WIDTH: u16 = 76;
const HEIGHT: u16 = 24;

/// A reader with one file open.
///
/// One and not two, so that choosing from the list of open files lands back
/// on the file the test started with: what is asserted is that the *file*
/// did not change, and a test that also switched documents would be
/// asserting two things and failing for either.
fn reading() -> App {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, WIDTH, HEIGHT);
    app
}

/// Opens one, however that one is opened.
///
/// Exhaustive on purpose: a layer added without a way to open it here is a
/// layer these tests would silently skip.
fn open(app: &mut App, layer: Layer) {
    match layer {
        Layer::Chat => app.open_agent(),
        Layer::Counts => dispatch::dispatch(app, Command::CountLines),
        Layer::Notes => dispatch::dispatch(app, Command::TodoOpen),
        Layer::Settings => dispatch::dispatch(app, Command::ConfigOpen),
        Layer::Picker => dispatch::dispatch(app, Command::BufferList),
        Layer::Prompt => dispatch::dispatch(app, Command::GoLine),
    }
}

/// What a reader is in, after opening one of them.
///
/// The derivation and the opening have to agree, and this is the only test
/// that checks they do: everything below it trusts `layers()` to say what is
/// showing.
#[test]
fn opening_one_puts_the_reader_in_it() {
    for layer in obelus::app::layers::STACK {
        let mut app = reading();
        assert_eq!(app.layers().nearest(), None, "{layer:?}: not a clean start");
        open(&mut app, layer);
        assert_eq!(
            app.layers().nearest(),
            Some(layer),
            "{layer:?} did not open, or opened as something else"
        );
    }
}

/// Escape from any of them comes back to the file, from every starting point.
///
/// This is the invariant the whole arrangement exists for. The reader is
/// never made to work out how many things are stacked up, because there is
/// never more than one thing to leave.
#[test]
fn escape_from_anywhere_comes_back_to_the_file() {
    for layer in obelus::app::layers::STACK {
        let mut app = reading();
        open(&mut app, layer);
        press(&mut app, KeyCode::Esc);
        assert!(
            !app.layers().any(),
            "{layer:?} was still showing after escape"
        );
        assert!(
            app.current_buffer().is_some(),
            "{layer:?}: escape left no file being read"
        );
    }
}

/// No layer lets a key reach the file behind it.
///
/// Broken deliberately by putting the old condition back, and it is the
/// counts this catches: with a file open and the counts over it, a letter
/// went into the file nobody could see, and so did `backspace`, `delete`
/// and `tab`. The notes were just as unguarded and escaped by luck -- they
/// have a box that swallows characters. Which is the reason every layer is
/// asked rather than the one that looked suspicious: what makes a hole
/// harmless here is an accident of another view's keys, and accidents do
/// not hold still.
///
/// `Enter` is left out on purpose. It means "choose this" in half of these,
/// so a test that pressed it would be asserting about what choosing does.
/// The refusal it would be testing does not read the key at all: it is one
/// condition on what is showing, so a letter getting through and `enter`
/// getting through are the same bug.
#[test]
fn nothing_typed_over_a_layer_reaches_the_file() {
    for layer in obelus::app::layers::STACK {
        let mut app = reading();
        let before = app
            .current_buffer()
            .expect("a file to read")
            .text()
            .rope()
            .to_string();

        open(&mut app, layer);
        for code in [
            KeyCode::Char('x'),
            KeyCode::Backspace,
            KeyCode::Delete,
            KeyCode::Tab,
        ] {
            press(&mut app, code);
        }

        let after = app
            .current_buffer()
            .expect("a file to read")
            .text()
            .rope()
            .to_string();
        assert_eq!(after, before, "{layer:?} let typing through to the file");
    }
}

/// And no layer lets a motion reach it either.
///
/// The same hole, one door along, and the notes fall into this one: their
/// box swallows a letter but not `ctrl+left`, so a word motion over the
/// notes page walked a cursor in the file behind it. Worse than the typing,
/// because nothing on screen changes and the damage is found later.
#[test]
fn no_motion_over_a_layer_moves_the_hidden_cursor() {
    for layer in obelus::app::layers::STACK {
        let mut app = reading();
        // Somewhere with room to move in both directions, so a motion that
        // does get through has somewhere to go.
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Right);
        let at = |app: &App| {
            let cursor = app.current_buffer().expect("a file to read").cursor();
            (cursor.line, cursor.column)
        };
        let before = at(&app);

        open(&mut app, layer);
        press(&mut app, KeyCode::End);
        support::press_control_key(&mut app, KeyCode::Right);
        support::press_control_key(&mut app, KeyCode::Left);

        assert_eq!(at(&app), before, "{layer:?} moved the cursor behind it");
    }
}

/// A question on the status bar is over the file without covering it.
///
/// The distinction the pointer needs and the keys do not: every line is
/// still on screen, and a line the reader can see is a line they can click.
/// Everything else here takes the screen, so a click that reached the code
/// behind it would move a caret nobody can see.
#[test]
fn only_a_question_leaves_the_file_pointable() {
    for layer in obelus::app::layers::STACK {
        let mut app = reading();
        open(&mut app, layer);
        let covering = app.layers().covering();
        match layer {
            Layer::Prompt => assert!(!covering, "a question is one row, not a screen"),
            _ => assert!(covering, "{layer:?} does not cover the file it is over"),
        }
    }
}
