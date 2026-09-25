//! Typing over what is there, and the caret that says so.
//!
//! Two halves of one mode: what the next character does, and how a reader
//! knows which it will do before they press the key.

mod support;

use crossterm::event::KeyCode;
use obelus_app::app::{App, Caret};
use obelus_buffer::Buffer;

/// A file with one short line in it, open, with the cursor at its start.
fn opened(name: &str, contents: &str) -> (support::Scratch, App) {
    let scratch = support::Scratch::new(name);
    let path = scratch.path().join("line.txt");
    std::fs::write(&path, contents).expect("writing it");
    let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
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

/// What is typed takes the place of what is under the cursor.
///
/// Deliberate break: leaving the span empty for a character -- which is
/// what it was before there was a mode -- makes this `Xabcd`.
#[test]
fn what_is_typed_takes_the_place_of_what_is_under_the_cursor() {
    let (_scratch, mut app) = opened("replacing", "abcd\n");
    app.toggle_replacing();
    support::press(&mut app, KeyCode::Char('X'));
    assert_eq!(text(&app), "Xbcd\n");
}

/// And it only does it while the mode is on.
///
/// Deliberate break: typing over whatever the mode says -- reading the
/// span from the character rather than from `replacing` -- makes this
/// `Xbcd`.
#[test]
fn typing_puts_a_character_in_where_the_mode_is_off() {
    let (_scratch, mut app) = opened("inserting", "abcd\n");
    support::press(&mut app, KeyCode::Char('X'));
    assert_eq!(text(&app), "Xabcd\n");
}

/// One character, and never the line ending after it.
///
/// The one thing typing over must not do is take the newline with it and
/// pull the next line up. Two places hold that: the span is one character
/// wide, and at the end of a line `clamp_column` makes it none at all --
/// where there is nothing to type over, the character goes in, which is
/// what every editor does and what a reader adding to the end of a line
/// means.
///
/// Deliberate break: a span two characters wide (`saturating_add(2)`)
/// joins the two lines, which is what this catches. The end-of-line half
/// is held here *and* under this, in the buffer's own clamping, so taking
/// the `clamp_column` out alone does not show: it is asserted because it
/// is the behaviour, not because this file is the only thing that could
/// break it.
#[test]
fn typing_over_takes_one_character_and_not_the_line_ending() {
    // A line with room in it, so that a span one character too wide shows
    // as a character eaten rather than being clamped back by the line's
    // own end.
    let (_scratch, mut app) = opened("replacing-end", "abcd\nef\n");
    app.toggle_replacing();
    support::press(&mut app, KeyCode::Right);
    support::press(&mut app, KeyCode::Char('X'));
    assert_eq!(text(&app), "aXcd\nef\n");

    // And past the last character there is nothing to take.
    support::press(&mut app, KeyCode::End);
    support::press(&mut app, KeyCode::Char('Y'));
    assert_eq!(text(&app), "aXcdY\nef\n");
}

/// The caret says which mode it is in, and says it only about the file.
///
/// Three assertions, because each passes with the others broken: the shape
/// has to follow the mode, and it has to stop following it where the caret
/// is in a box -- a query is one short line and typing over it means
/// nothing, so a block there would be the caret saying something untrue.
///
/// Deliberate break: answering `Caret::Block` from the mode alone makes
/// the last of these a block.
#[test]
fn the_caret_says_what_the_next_character_will_do() {
    let (_scratch, mut app) = opened("caret", "abcd\n");
    assert_eq!(app.caret(), Caret::Bar);
    app.toggle_replacing();
    assert_eq!(app.caret(), Caret::Block);
    // A box over the file: the palette's query.
    support::press_control(&mut app, 'p');
    assert_eq!(app.caret(), Caret::Bar);
}

/// And the status row says it in a word, which is the half a terminal has.
///
/// A terminal draws its own caret and Obelus cannot change its shape, so
/// the word is what tells a reader there which mode they are in.
///
/// Deliberate break: taking the line out of the status row leaves the row
/// saying nothing about it.
#[test]
fn the_status_row_says_it_too() {
    let (_scratch, mut app) = opened("status", "abcd\n");
    assert!(!support::render(&mut app, 60, 10).contains("Replacing"));
    app.toggle_replacing();
    assert!(support::render(&mut app, 60, 10).contains("Replacing"));
}
