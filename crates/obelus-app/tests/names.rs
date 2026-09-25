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

/// A list longer than the band scrolls, so the reader can reach the end of
/// it.
///
/// Where the window sits depends on how many rows are about to be drawn,
/// which only the frame knows -- so it is settled once a frame, like every
/// other list here.
///
/// Deliberate break: taking that out of `App::prepare` leaves the focus
/// walking off the bottom while the rows stay where they were, and the
/// name this asks for is never drawn.
#[test]
fn a_list_longer_than_the_band_scrolls() {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 60, 20);
    let many: Vec<String> = (0..30).map(|at| format!("Face {at:02}")).collect();
    app.handle(obelus_app::event::Event::Fonts(many));
    app.open_names("fonts");

    for _ in 0..20 {
        support::press(&mut app, KeyCode::Down);
    }
    let screen = support::render(&mut app, 60, 20);
    assert!(
        screen.contains("Face 20"),
        "the list did not follow the focus:\n{screen}"
    );
}
