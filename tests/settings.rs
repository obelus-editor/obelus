//! The settings view: tabs, a filter, and a control per row.

mod support;

use crossterm::event::KeyCode;
use obelus::{
    app::App,
    command::{Command, dispatch},
    config,
};

/// Applying a setting touches process-wide state -- the glyph switch is one
/// switch the drawing code can read without a flag threaded into every
/// function -- so these tests take turns. Tests in one file share a process,
/// and every test here applies a setting.
static SETTINGS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A directory of its own for one test.
fn temporary(name: &str) -> std::path::PathBuf {
    let directory =
        std::env::temp_dir().join(format!("obelus-settings-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("a directory");
    directory.join("config.toml")
}

fn open(file: &std::path::Path) -> App {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.config_file_for_test(file.to_path_buf());
    support::lay_out(&mut app, 66, 12);
    dispatch::dispatch(&mut app, Command::ConfigOpen);
    app
}

/// A switch changes the setting, the running program, and the file -- in
/// that order and all at once. A setting that took effect on restart would
/// be a setting nobody can tell they have changed.
#[test]
fn a_switch_takes_effect_and_is_written_down() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let file = temporary("switch");
    let mut app = open(&file);

    // Down onto the second row of the appearance tab, which is the glyphs.
    support::press(&mut app, KeyCode::Down);
    assert!(obelus::icons::enabled(), "the glyphs start on");
    support::press(&mut app, KeyCode::Char(' '));

    assert!(!obelus::icons::enabled(), "the glyphs are still on");
    assert!(!app.config().icons, "the setting did not change");
    let written = std::fs::read_to_string(&file).expect("the file was written");
    assert!(
        !config::from_toml(&written).icons,
        "the file does not say so: {written:?}"
    );

    // And enter does the same thing as space, because a switch is a thing
    // you toggle and both are what a reader reaches for.
    support::press(&mut app, KeyCode::Enter);
    assert!(obelus::icons::enabled(), "enter did not toggle it back");

    // Put the glyphs back for whatever runs next: this is process-wide
    // state, which is the price of a switch the drawing code can read.
    obelus::icons::use_glyphs(true);
}

/// A setting with choices opens the ordinary compact list over the view:
/// the same list the symbol menu is, so it filters by typing and scrolls
/// and marks its selection the way every other list does.
#[test]
fn a_choice_opens_the_list_every_other_choice_uses() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let file = temporary("droplist");
    let mut app = open(&file);

    // The theme is the first row.
    support::press(&mut app, KeyCode::Enter);
    let picker = app.picker().expect("the choices");
    assert_eq!(
        picker
            .matches()
            .map(|item| item.label.clone())
            .collect::<Vec<_>>(),
        ["dark", "light"],
        "not the theme's choices"
    );
    // Opened on the one in force, so the list starts by saying which.
    assert_eq!(
        picker.selected_item().map(|item| item.label.clone()),
        Some("dark".to_string())
    );
    // And the settings are still there, underneath.
    assert!(app.settings().is_some(), "the view went away");
    let dump = support::render(&mut app, 66, 12);
    assert!(
        support::text_block(&dump).contains("appearance"),
        "the settings are not behind the list:\n{dump}"
    );

    // It filters by typing, which is the whole reason it is this list.
    support::type_text(&mut app, "li");
    let picker = app.picker().expect("the choices");
    assert_eq!(picker.match_count(), 1, "the query narrowed nothing");
    support::press(&mut app, KeyCode::Enter);

    assert!(app.picker().is_none(), "the list stayed open");
    assert_eq!(app.theme().name, "light", "the theme did not change");
    assert_eq!(
        config::from_toml(&std::fs::read_to_string(&file).expect("the file")).theme,
        "light"
    );

    // Escape closes the list and leaves the setting alone -- and leaves the
    // settings view open, because that is what is behind it.
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Esc);
    assert!(app.picker().is_none(), "escape left the list open");
    assert_eq!(app.theme().name, "light", "escape chose something");
    assert!(app.settings().is_some(), "escape closed the view as well");
}

/// A switch is a slider, and enter flips it: the knob moves to the other
/// end. The arrows are not it -- they walk the tabs, as they do in every
/// other view with tabs on it.
#[test]
fn a_switch_is_a_slider_that_enter_flips() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let file = temporary("slider");
    let mut app = open(&file);
    support::press(&mut app, KeyCode::Down);

    // Where the knob is drawn on the row.
    let knob = |app: &mut App| {
        let dump = support::render(app, 66, 12);
        support::text_block(&dump)
            .lines()
            .find(|row| row.contains("Nerd Font"))
            .and_then(|row| row.find('\u{25a0}'))
            .expect("the knob")
    };

    assert!(app.config().icons, "the glyphs start on");
    let on = knob(&mut app);
    support::press(&mut app, KeyCode::Enter);
    assert!(!app.config().icons, "enter did not flip it");
    let off = knob(&mut app);
    assert!(off < on, "the knob did not move: {off} then {on}");

    // The arrows do not touch it: they are the tabs'.
    support::press(&mut app, KeyCode::Right);
    assert!(!app.config().icons, "an arrow flipped the switch");
    support::press(&mut app, KeyCode::Left);
    assert!(!app.config().icons, "an arrow flipped the switch");

    obelus::icons::use_glyphs(true);
}

/// The tabs are the groups, the arrows walk them -- only the arrows, the way
/// they do in every other view with tabs on it -- and the focus comes back
/// inside the rows the new tab has.
#[test]
fn the_tabs_are_the_groups() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let file = temporary("tabs");
    let mut app = open(&file);

    let rows = |app: &App| {
        app.settings()
            .expect("the settings")
            .rows()
            .iter()
            .map(|setting| setting.label.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        rows(&app),
        [
            "Colour theme",
            "Nerd Font glyphs in lists and on the status bar"
        ]
    );

    // Onto the last row, then to the next tab: the focus cannot stay on a
    // row the new tab does not have.
    support::press(&mut app, KeyCode::Down);
    assert_eq!(app.settings().expect("the settings").focus(), 1);
    support::press(&mut app, KeyCode::Right);
    assert_eq!(
        rows(&app),
        [
            "Wrap a line too long for the screen onto the next row",
            "Who last changed the line the cursor is on"
        ]
    );
    assert!(
        app.settings().expect("the settings").focus() < 2,
        "the focus is on a row this tab does not have"
    );

    // And back, and round: two tabs, so either arrow reaches the other one.
    support::press(&mut app, KeyCode::Left);
    assert_eq!(
        rows(&app),
        [
            "Colour theme",
            "Nerd Font glyphs in lists and on the status bar"
        ]
    );
    support::press(&mut app, KeyCode::Left);
    assert_eq!(
        rows(&app),
        [
            "Wrap a line too long for the screen onto the next row",
            "Who last changed the line the cursor is on"
        ]
    );

    // Tab is not one of them: one way to walk them is the way every other
    // tabbed view here works.
    support::press(&mut app, KeyCode::Tab);
    assert_eq!(
        rows(&app),
        [
            "Wrap a line too long for the screen onto the next row",
            "Who last changed the line the cursor is on"
        ]
    );
}

/// Typing narrows the rows, and the count on the status bar says how many
/// are left. Plainly by substring, because a reader typing "the" means the
/// word.
#[test]
fn typing_narrows_the_settings() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let file = temporary("filter");
    let mut app = open(&file);

    support::type_text(&mut app, "theme");
    let dump = support::render(&mut app, 66, 12);
    assert_eq!(
        app.settings().expect("the settings").rows().len(),
        1,
        "the query narrowed nothing:\n{dump}"
    );
    assert!(
        support::text_block(&dump).contains("Colour theme"),
        "not the row that matched:\n{dump}"
    );
    assert!(
        !support::text_block(&dump).contains("Nerd Font"),
        "a row that did not match is still there:\n{dump}"
    );

    // And the characters that matched carry the background every other list
    // marks a match with: a row in a narrowed list has to say why it is in
    // it. Only those characters -- the rest of the row is not a match.
    let styles: Vec<&str> = support::style_block(&dump)
        .lines()
        .filter(|row| !row.is_empty())
        .collect();
    let row = support::text_block(&dump)
        .lines()
        .filter(|row| !row.is_empty())
        .position(|row| row.contains("Colour theme"))
        .expect("the row that matched");
    // Cell one is the "C" of "Colour theme" -- cell zero is the row's own
    // left-hand padding -- and cell twelve is its last character.
    let letters: Vec<char> = styles[row].chars().skip(3 + 1).take(12).collect();
    let marked: std::collections::HashSet<char> = letters.iter().copied().collect();
    assert!(
        marked.len() > 1,
        "the whole name is one colour, so nothing was marked:\n{dump}"
    );
    // "theme" is the last five characters of the name, so the run is at its
    // end and its first character is not in it.
    assert_ne!(
        letters[0], letters[11],
        "the marked run covers the whole name:\n{dump}"
    );
    assert_eq!(
        letters[7], letters[11],
        "the run is not the five characters that matched:\n{dump}"
    );

    // Nothing matches: the view says so rather than showing an empty screen.
    support::type_text(&mut app, "zzz");
    let dump = support::render(&mut app, 66, 12);
    assert!(
        support::text_block(&dump).contains("no setting by that name"),
        "an empty screen with no reason:\n{dump}"
    );

    // Backspace brings them back.
    for _ in 0..3 {
        support::press(&mut app, KeyCode::Backspace);
    }
    assert_eq!(app.settings().expect("the settings").rows().len(), 1);
}

/// Escape closes the view, and the settings are read from the file the next
/// time it opens: what is on the screen is what is in the file.
#[test]
fn the_view_closes_and_the_file_is_what_it_shows() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let file = temporary("closes");
    let mut app = open(&file);
    support::press(&mut app, KeyCode::Esc);
    assert!(app.settings().is_none(), "the view stayed open");

    // A file written by someone else -- an editor, another obelus -- is what
    // a fresh application reads.
    std::fs::write(&file, "theme = \"light\"\nicons = false\nblame = false\n")
        .expect("writing the file");
    let mut second = App::new(vec![support::open_fixture("sample.rs")]);
    second.config_file_for_test(file.clone());
    assert_eq!(second.theme().name, "light");
    assert!(!second.config().blame);
    obelus::icons::use_glyphs(true);
}

/// An application that was never told where its settings live does not write
/// any: every test is one of those, and the reader's own file is not
/// something a test may touch.
#[test]
fn nothing_is_written_without_being_told_where() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 66, 12);
    dispatch::dispatch(&mut app, Command::ConfigOpen);
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Char(' '));
    // It still took effect, and there was nowhere to write it.
    assert!(!app.config().icons);
    assert_eq!(app.note(), None, "it complained about not saving");
    obelus::icons::use_glyphs(true);
}

/// Choosing a theme from the theme picker is kept too: a reader who picks
/// one and finds the old one back tomorrow was given a preview rather than
/// a choice.
#[test]
fn the_theme_picker_writes_its_choice_down() {
    let _turn = SETTINGS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let file = temporary("theme-picker");
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.config_file_for_test(file.clone());
    support::lay_out(&mut app, 60, 12);

    dispatch::dispatch(&mut app, Command::ThemeSelect);
    // The list opens on the theme in force, so the next one is the other.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);

    let chosen = app.theme().name.to_string();
    assert_eq!(
        config::from_toml(&std::fs::read_to_string(&file).expect("the file")).theme,
        chosen,
        "the file does not say which theme was chosen"
    );
}
