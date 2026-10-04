//! A key that names a whole-screen view goes there from inside another, and
//! from a list the reader opened over the file.
//!
//! One view swapped for the other, never one opened over the other: after
//! every switch there is one thing on screen and one escape back to the
//! file, which is what a view keeping its keys to itself was for.

mod support;

use crossterm::event::KeyCode;
use obelus_app::app::{App, dispatch};
use obelus_command::Command;
use support::{press, press_control, press_function, type_text};

/// An application over the fixture file, with one file changed so the file
/// list has both its tabs.
fn reading() -> App {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    let changed = app.working_directory().join("src/app.rs");
    app.statuses_for_test(
        [(changed, obelus_git::FileStatus::Changed.into())]
            .into_iter()
            .collect(),
    );
    support::lay_out(&mut app, 80, 24);
    app
}

/// The tabs of the list on screen, by name, and which one is showing.
fn showing(app: &App) -> Option<(Vec<String>, usize)> {
    app.picker()
        .map(|picker| (picker.tabs().to_vec(), picker.tab()))
}

/// The key of another view goes to that view, and escape then goes back to
/// the file rather than to the view that was left.
///
/// From the search to the files, which is also the key the card used to be
/// on: `f1` inside a view is the files now, and the card is `ctrl+k`.
///
/// Broken deliberately by taking the switch out of `App::handle_key`: `f1`
/// is refused inside a view, as it always was, and the search stays.
#[test]
fn a_view_s_key_goes_to_that_view_in_place_of_this_one() {
    let mut app = reading();
    press_function(&mut app, 5);
    assert!(
        app.picker().is_some_and(|picker| picker.is_searching()),
        "the search did not open"
    );

    press_function(&mut app, 1);
    assert!(
        app.picker().is_some_and(|picker| picker.is_listing()),
        "f1 in the search did not go to the files: {:?}",
        showing(&app)
    );

    press(&mut app, KeyCode::Esc);
    assert!(
        app.picker().is_none(),
        "escape went back to the search, so the files were opened over it"
    );
}

/// A key naming a tab of the view already showing walks to that tab, and
/// what was typed stays.
///
/// Broken deliberately by having `tab_for` answer `None` for every command:
/// the view is closed and opened again on the tab, and the query is gone.
#[test]
fn a_key_naming_a_tab_here_walks_to_it_with_the_query() {
    let mut app = reading();
    press_function(&mut app, 5);
    type_text(&mut app, "ma");
    press_function(&mut app, 6);
    let picker = app.picker().expect("the search");
    assert_eq!(
        picker.tabs()[picker.tab()],
        "Project",
        "f6 did not walk to the project"
    );
    assert_eq!(picker.query(), "ma", "the query did not come along");

    let mut app = reading();
    press_function(&mut app, 1);
    type_text(&mut app, "ma");
    press_function(&mut app, 3);
    let picker = app.picker().expect("the files");
    assert_eq!(
        picker.tabs()[picker.tab()],
        "Changed",
        "f3 did not walk to the changed files"
    );
    assert_eq!(picker.query(), "ma", "the query did not come along");
}

/// A key whose command is dim does nothing, and nothing includes leaving the
/// reader where they were.
///
/// Broken deliberately by taking the `offers` question out of
/// `switch_view`: the files are closed for a list that has nothing to show,
/// and the reader is dropped back in the file.
#[test]
fn a_dim_key_leaves_the_view_where_it_was() {
    let mut app = reading();
    app.statuses_for_test(std::collections::HashMap::new());
    press_function(&mut app, 1);
    press_function(&mut app, 3);
    assert!(
        app.picker().is_some_and(|picker| picker.is_listing()),
        "f3 on a tree with nothing changed took the files away"
    );
}

/// The settings are a whole-screen view too, and give way the same.
///
/// Broken deliberately by leaving `Layer::Settings` out of
/// `gives_way_to_a_view`: `f5` is refused on the settings page.
#[test]
fn the_settings_give_way_to_a_view_s_key() {
    let mut app = reading();
    dispatch::dispatch(&mut app, Command::ConfigOpen);
    assert!(app.settings().is_some(), "the settings did not open");
    press_function(&mut app, 5);
    assert!(app.settings().is_none(), "the settings are still showing");
    assert!(
        app.picker().is_some_and(|picker| picker.is_searching()),
        "f5 on the settings did not go to the search"
    );
}

/// A list over the file gives way the same: the palette is somewhere the
/// reader is choosing, and a key naming a view is them choosing that instead.
/// And escape from what it gave way to goes back to the file, not to the
/// palette.
///
/// Broken deliberately by answering only for a full-screen list in
/// `gives_way_to_a_view`, which is what it did before: `f5` in the palette
/// is refused, and the palette stays.
#[test]
fn a_list_over_the_file_gives_way_to_a_view_s_key() {
    let mut app = reading();
    press_control(&mut app, 'p');
    assert!(app.picker().is_some(), "the palette did not open");
    press_function(&mut app, 5);
    assert!(
        app.picker().is_some_and(|picker| picker.is_searching()),
        "f5 in the palette did not go to the search"
    );
    press(&mut app, KeyCode::Esc);
    assert!(
        app.picker().is_none(),
        "escape went back to the palette, so the search was opened over it"
    );
}

/// The conversations are a list over the file, and `f1` goes from them to
/// the files -- the one view the files could not be reached from while a
/// list over the file kept its keys. `f4` there is the list it is already
/// in, so the reader keeps what they typed.
///
/// Broken deliberately twice. Answering only for a full-screen list in
/// `gives_way_to_a_view`: `f1` is refused and the conversations stay. And
/// taking `opened_by` out of `open_conversation_picker`: `f4` closes the
/// list and opens it again, empty.
#[test]
fn the_conversations_give_way_and_f4_keeps_them() {
    let mut app = reading();
    dispatch::dispatch(&mut app, Command::ConversationSelect);
    assert!(
        app.picker().is_some_and(|picker| !picker.is_listing()),
        "the conversations did not open"
    );
    type_text(&mut app, "zz");
    press_function(&mut app, 4);
    assert_eq!(
        app.picker().map(|picker| picker.query().to_string()),
        Some("zz".to_string()),
        "f4 in its own list did not leave the reader where they were"
    );

    press_function(&mut app, 1);
    assert!(
        app.picker().is_some_and(|picker| picker.is_listing()),
        "f1 in the conversations did not go to the files"
    );
}

/// The palette takes a view's place as well: it is the one list every command
/// is on, and a reader inside a view who wants one should not have to leave it
/// first. And `ctrl+p` in the palette is the palette, with what was typed.
///
/// Broken deliberately twice. Taking the palette out of
/// `Command::opens_a_list`: `ctrl+p` in the files is refused and the files
/// stay. And taking `opened_by` out of `open_command_palette`: `ctrl+p` in
/// the palette closes it and opens it again, empty.
#[test]
fn the_palette_takes_a_view_s_place() {
    let mut app = reading();
    press_function(&mut app, 1);
    press_control(&mut app, 'p');
    assert!(
        app.picker()
            .is_some_and(|picker| picker.opener() == Some(Command::CommandPalette)),
        "ctrl+p in the files did not go to the palette"
    );
    type_text(&mut app, "sav");
    press_control(&mut app, 'p');
    assert_eq!(
        app.picker().map(|picker| picker.query().to_string()),
        Some("sav".to_string()),
        "ctrl+p in the palette did not leave the reader where they were"
    );
    press(&mut app, KeyCode::Esc);
    assert!(
        app.picker().is_none(),
        "escape went back to the files, so the palette was opened over them"
    );
}

/// A question keeps its keys: leaving it by any other key is an answer the
/// reader never gave, so `f1` does nothing until it is answered.
///
/// Broken deliberately by taking `will_not_give_way` out of
/// `Picker::asking`: `f1` puts the files where the question was, and the
/// file it was asking about is still open with nothing decided.
#[test]
fn a_question_keeps_its_keys() {
    let mut app = reading();
    type_text(&mut app, "x");
    dispatch::dispatch(&mut app, Command::DocumentClose);
    let asking = |app: &App| {
        app.picker()
            .and_then(|picker| picker.selected_item())
            .is_some_and(|row| row.label.contains("Save"))
    };
    assert!(asking(&app), "closing an unsaved file did not ask");
    press_function(&mut app, 1);
    assert!(asking(&app), "f1 took the question away");
}
