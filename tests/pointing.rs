//! Pointing at the boxes a reader writes in.
//!
//! A box a reader can select in with the keyboard and not with the mouse
//! has half a selection. The file could always be pointed at; every other
//! box could not, because `on_pointer` returned for anything covering the
//! screen and nothing behind that ever saw a click.
//!
//! What each box needs is the inverse of its own laying-out -- which row
//! of the text a row on screen is, and which character a cell is. Those
//! live beside the functions that put the caret there, because they are
//! one geometry: the box is drawn from those numbers and the caret is put
//! from them, so where a click lands has to come from them too.

mod support;

use obelus::{
    app::App,
    app::dispatch,
    buffer::Buffer,
    command::Command,
    event::{Event, Pointer},
};

fn open(name: &str) -> (support::Scratch, App) {
    let scratch = support::Scratch::new(name);
    let path = scratch.path().join("one.rs");
    std::fs::write(&path, "fn main() {}\n").expect("writing it");
    let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, 76, 18);
    (scratch, app)
}

fn drag(app: &mut App, y: u16, from: u16, to: u16) {
    app.handle(Event::Pointer {
        kind: Pointer::Pressed,
        x: from,
        y,
    });
    app.handle(Event::Pointer {
        kind: Pointer::Dragged,
        x: to,
        y,
    });
}

/// The box a note is written in, which is where this was first noticed:
/// selecting with the mouse marked nothing, and the file behind the page
/// had no selection either -- the click reached neither.
#[test]
fn dragging_in_a_note_selects_in_it() {
    let (_scratch, mut app) = open("pointing-note");
    dispatch::dispatch(&mut app, Command::TodoAdd);
    support::type_text(&mut app, "hello world");
    let (y, at) = support::place_of(&mut app, "hello world");

    drag(&mut app, y, at + 6, at + 11);
    assert_eq!(
        app.notes()
            .and_then(obelus::component::todo::TodoView::writing)
            .and_then(obelus::component::composer::Composer::selected)
            .as_deref(),
        Some("world"),
        "the drag did not hold the word it crossed"
    );
    // The file by its place in the list, not through `current_buffer`: the
    // notes are the document being read, and a document that is not a file
    // answers `None` to everything that wants one.
    assert_eq!(
        app.file(obelus::buffer::DocumentId::new(0))
            .expect("the file it was opened on")
            .text()
            .rope()
            .to_string(),
        "fn main() {}\n",
        "the drag reached the file instead of the note"
    );
}

/// And the box a message to an agent is written in, which is the same
/// component under a different view.
#[test]
fn dragging_in_a_message_selects_in_it() {
    let (_scratch, mut app) = open("pointing-message");
    dispatch::dispatch(&mut app, Command::AgentOpen);
    support::type_text(&mut app, "hello world");
    let (y, at) = support::place_of(&mut app, "hello world");

    drag(&mut app, y, at + 6, at + 11);
    assert_eq!(
        app.chat()
            .map(obelus::component::chat::Chat::writing)
            .and_then(obelus::component::composer::Composer::selected)
            .as_deref(),
        Some("world"),
        "the drag did not hold the word it crossed"
    );
}

/// Twice is the word and three times is the line, the way it is in the
/// file -- and the way it is in the boxes on the status row.
#[test]
fn clicking_twice_holds_a_word_and_three_times_the_line() {
    let (_scratch, mut app) = open("pointing-clicks");
    dispatch::dispatch(&mut app, Command::TodoAdd);
    support::type_text(&mut app, "hello world");
    let (y, at) = support::place_of(&mut app, "hello world");

    let held = |app: &App| {
        app.notes()
            .and_then(obelus::component::todo::TodoView::writing)
            .and_then(obelus::component::composer::Composer::selected)
    };
    for _ in 0..2 {
        app.handle(Event::Pointer {
            kind: Pointer::Pressed,
            x: at + 8,
            y,
        });
    }
    assert_eq!(held(&app).as_deref(), Some("world"), "two clicks");

    app.handle(Event::Pointer {
        kind: Pointer::Pressed,
        x: at + 8,
        y,
    });
    assert_eq!(held(&app).as_deref(), Some("hello world"), "three clicks");
}

/// A box taller than the band it is drawn in shows its last rows, so the
/// first row on screen is not the first row of the text.
///
/// The same scrolling the caret is placed under, read backwards. Without
/// it every click in a long message lands that many rows too early, and
/// the longer the message the further off it is.
#[test]
fn a_click_in_a_scrolled_box_lands_where_it_points() {
    let (_scratch, mut app) = open("pointing-scrolled");
    dispatch::dispatch(&mut app, Command::AgentOpen);
    // More lines than the band has rows, so the box is scrolled and the
    // first line is off the top of it.
    for line in 0..8 {
        support::type_text(&mut app, &format!("line{line}"));
        app.handle(Event::Key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::SHIFT,
        )));
    }
    support::type_text(&mut app, "last");

    // The topmost row the box is showing, which is the ninth line of the
    // text and the first row of the band.
    let (y, at) = support::place_of(&mut app, "line3");
    app.handle(Event::Pointer {
        kind: Pointer::Pressed,
        x: at,
        y,
    });

    // One press and no drag: pressing moves the caret, and this box keeps
    // the caret on its last row, so the second half of a drag would be
    // measured against a box that had scrolled out from under the pointer.
    // What is being pinned here is the arithmetic, which is the first
    // half.
    let width = obelus::ui::chat::writing_width(app.editor_area_for_test());
    assert_eq!(
        app.chat()
            .map(obelus::component::chat::Chat::writing)
            .map(|writing| writing.caret(width).0),
        Some(3),
        "the click landed on the row the text starts at rather than the one on screen"
    );
}

/// A click below the note being written is not a click in it.
///
/// The rows under it belong to the next note, and a box that took them
/// would move a caret the reader cannot see for a click they made
/// somewhere else.
#[test]
fn a_click_below_the_box_is_not_a_click_in_it() {
    let (_scratch, mut app) = open("pointing-below");
    dispatch::dispatch(&mut app, Command::TodoAdd);
    support::type_text(&mut app, "hello world");
    let (y, at) = support::place_of(&mut app, "hello world");

    // Something held, so that a click that reached the box would be seen:
    // landing anywhere lets go of what is held.
    drag(&mut app, y, at + 6, at + 11);
    let held = |app: &App| {
        app.notes()
            .and_then(obelus::component::todo::TodoView::writing)
            .and_then(obelus::component::composer::Composer::selected)
    };
    assert_eq!(held(&app).as_deref(), Some("world"));

    app.handle(Event::Pointer {
        kind: Pointer::Pressed,
        x: at + 2,
        y: y + 1,
    });
    assert_eq!(
        held(&app).as_deref(),
        Some("world"),
        "a click on the row below the note reached into it"
    );
}

/// A click on the page but not in the box does nothing, and in particular
/// does not fall through to the code the page is covering.
#[test]
fn a_click_off_the_box_reaches_nothing_behind_it() {
    let (_scratch, mut app) = open("pointing-off");
    dispatch::dispatch(&mut app, Command::TodoAdd);
    support::type_text(&mut app, "hello");
    let (y, _) = support::place_of(&mut app, "hello");

    // Far to the right of the note, which is page and not box.
    app.handle(Event::Pointer {
        kind: Pointer::Pressed,
        x: 70,
        y,
    });
    assert!(
        app.current_buffer()
            .and_then(Buffer::selected_text)
            .is_none(),
        "a click on the page selected in the file behind it"
    );
}
