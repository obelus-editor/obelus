//! The list a setting's names are built in, on screen.
//!
//! One fixture, because the whole of what is interesting here is the
//! drawing: two sections of one list, a boundary that says what the lower
//! one is, and the query on the status row like every other list.

mod support;

use crossterm::event::KeyCode;
use obelus_app::app::App;

/// A reader who has chosen two faces, on a machine that has four.
fn building() -> App {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 60, 20);
    app.handle(obelus_app::event::Event::Fonts(vec![
        "Iosevka".to_string(),
        "JetBrains Mono".to_string(),
        "Noto Color Emoji".to_string(),
        "Noto Sans CJK SC".to_string(),
    ]));
    app.open_names("fonts");
    app
}

/// The list hugs the status row, says which half is which, and leaves the
/// code above it readable.
#[test]
fn the_two_halves_are_told_apart_and_the_code_stays_visible() {
    let mut app = building();
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Down);
    support::check("names_60x20", &support::render(&mut app, 60, 20));
}

/// What is typed narrows the lower half and marks what matched, and a name
/// no face here is called is still offered.
#[test]
fn what_is_typed_narrows_it_and_is_itself_an_answer() {
    let mut app = building();
    for character in "noto".chars() {
        support::press(&mut app, KeyCode::Char(character));
    }
    support::check("names_typed_60x20", &support::render(&mut app, 60, 20));
}
