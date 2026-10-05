//! Every box a reader types in holds what is in it the same way.
//!
//! The keys were the file's everywhere the caret was a `Field` or a
//! `Composer` underneath -- and then whoever held the box took some of them
//! first. The settings sent `shift+end` to the bottom of the page, the list
//! of names sent `shift+up` down its rows, the message box sent
//! `ctrl+shift+home` to the top of the conversation, and `ctrl+a` reached a
//! box in none of them: over a question on the status row it took the
//! whole of the file behind. And where the keys did arrive, three of the
//! boxes held what they were asked to without drawing it, so a letter typed
//! next took away words the reader never saw were held.
//!
//! So one table of the places, and every one of them asked the same
//! questions: the keys, escape, the pointer, and what is drawn. The card an
//! agent asks on is the one place missing, because putting one up takes an
//! agent -- it is asked the same questions in `tests/agent.rs`.

mod support;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use obelus_app::{
    app::{App, dispatch},
    event::{Event, Pointer},
};
use obelus_buffer::Buffer;
use obelus_command::Command;
use obelus_ui::Screen as _;

/// What is typed into every one of them.
const SAID: &str = "one two three";

/// One place a reader types.
struct Place {
    name: &'static str,
    /// Puts it on screen, in a scratch directory of the test's own.
    open: fn(&support::Scratch) -> App,
    /// What the reader has hold of in it.
    held: fn(&App) -> Option<String>,
    /// What is in it.
    said: fn(&App) -> Option<String>,
    /// Whether it is still on screen.
    showing: fn(&App) -> bool,
}

/// The run of `said` a range of characters covers.
fn run_of(said: &str, held: Option<std::ops::Range<usize>>) -> Option<String> {
    let held = held?;
    Some(said.chars().skip(held.start).take(held.len()).collect())
}

/// Obelus on a file, which is behind most of them -- so that a key which
/// reaches the file instead of the box has somewhere to land.
fn on_a_file(scratch: &support::Scratch) -> App {
    let path = scratch.path().join("one.rs");
    std::fs::write(&path, "fn main() {}\n").expect("writing it");
    let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, 76, 18);
    app
}

/// Being asked which project, with one remembered.
fn asking(scratch: &support::Scratch) -> App {
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.ask_about_these_projects_for_test(vec![obelus_component::chooser::Known {
        path: scratch.path().join("alpha"),
        shown: "alpha".to_string(),
        last: Some(1_000),
    }]);
    support::lay_out(&mut app, 76, 18);
    app
}

fn places() -> Vec<Place> {
    vec![
        Place {
            name: "a list's query",
            open: |scratch| {
                let mut app = on_a_file(scratch);
                dispatch::dispatch(&mut app, Command::SearchFile);
                app
            },
            held: |app| {
                let picker = app.picker()?;
                run_of(&picker.query(), picker.query_held())
            },
            said: |app| Some(app.picker()?.query()),
            showing: |app| app.picker().is_some(),
        },
        Place {
            name: "the settings' filter",
            open: |scratch| {
                let mut app = on_a_file(scratch);
                dispatch::dispatch(&mut app, Command::ConfigOpen);
                app
            },
            held: |app| {
                let settings = app.settings()?;
                run_of(&settings.query(), settings.query_held())
            },
            said: |app| Some(app.settings()?.query()),
            showing: |app| app.settings().is_some(),
        },
        Place {
            name: "a question on the status row",
            open: |scratch| {
                let mut app = on_a_file(scratch);
                dispatch::dispatch(&mut app, Command::FileNew);
                app
            },
            held: |app| {
                let prompt = app.prompt()?;
                run_of(&prompt.text(), prompt.held())
            },
            said: |app| Some(app.prompt()?.text()),
            showing: |app| app.prompt().is_some(),
        },
        Place {
            name: "the list of names",
            open: |scratch| {
                let mut app = on_a_file(scratch);
                app.handle(Event::Fonts {
                    here: vec!["Iosevka".to_string()],
                    otherwise: None,
                });
                app.open_names("fonts");
                app
            },
            held: |app| app.names()?.query().selected(),
            said: |app| Some(app.names()?.query().said()),
            showing: |app| app.names().is_some(),
        },
        Place {
            name: "the projects' filter",
            open: asking,
            held: |app| {
                let choosing = app.choosing()?;
                run_of(&choosing.typed, choosing.held)
            },
            said: |app| Some(app.choosing()?.typed),
            showing: |app| app.choosing().is_some_and(|choosing| !choosing.naming),
        },
        Place {
            name: "a project's path",
            open: |scratch| {
                let mut app = asking(scratch);
                support::press_control_key(&mut app, KeyCode::End);
                support::press(&mut app, KeyCode::Enter);
                app
            },
            held: |app| {
                let choosing = app.choosing()?;
                run_of(&choosing.typed, choosing.held)
            },
            said: |app| Some(app.choosing()?.typed),
            showing: |app| app.choosing().is_some_and(|choosing| choosing.naming),
        },
        Place {
            name: "a message",
            open: |scratch| {
                let mut app = on_a_file(scratch);
                dispatch::dispatch(&mut app, Command::ConversationNew);
                app
            },
            held: |app| app.chat()?.writing().selected(),
            said: |app| Some(app.chat()?.writing().text()),
            showing: |app| app.chat().is_some(),
        },
        Place {
            name: "a note",
            open: |scratch| {
                support::make_room_for_notes(scratch.path());
                let mut app = on_a_file(scratch);
                // Straight into a note of its own, the way `alt+n` starts
                // one from a line of the file.
                dispatch::dispatch(&mut app, Command::TodoAdd);
                app
            },
            held: |app| app.notes()?.writing()?.selected(),
            said: |app| Some(app.notes()?.writing()?.text()),
            showing: |app| app.notes().is_some_and(|notes| notes.writing().is_some()),
        },
    ]
}

/// Where the box is on screen: the last place what was typed is drawn,
/// because a list may say it again above its query -- the list of names
/// offers what is typed as an answer of its own.
fn box_of(app: &mut App) -> (u16, u16) {
    let dump = support::render(app, 76, 18);
    support::text_block(&dump)
        .lines()
        .filter_map(|row| row.split_once('|').map(|(_, cells)| cells.to_string()))
        .enumerate()
        .filter_map(|(y, row)| {
            row.find(SAID).map(|byte| {
                (
                    u16::try_from(y).expect("a row"),
                    u16::try_from(row[..byte].chars().count()).expect("a column"),
                )
            })
        })
        .last()
        .unwrap_or_else(|| panic!("{SAID:?} is not on screen:\n{dump}"))
}

fn chord(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    app.handle(Event::Key(KeyEvent::new(code, modifiers)));
}

const CONTROL_SHIFT: KeyModifiers = KeyModifiers::CONTROL.union(KeyModifiers::SHIFT);

/// The keys that hold, in every box: a word, an end of the line, and all of
/// it -- and escape lets go of what is held before it gives up on anything.
///
/// Deliberate breaks, one for each place the keys went astray:
/// - the settings without their `ctrl+shift` arm: the word is not held.
/// - the list of names asking `Move::of` whatever the modifiers: control, shift
///   and home walk its rows instead.
/// - the message box without its `ctrl+shift` arm: the word is not held.
/// - `select_all` back to the file alone, or `ctrl+a` unbound in a dialog: it
///   holds nothing in the list's query.
/// - `Move::under_a_box` giving bare home and end to the list again: the list
///   of names walks its rows and its query's caret stays put.
/// - the list of names taking delete for its rows whatever is in its query, as
///   it did: the character stays.
/// - any one box's escape arm taken out -- the list's, the settings', the
///   question's, the names', the two projects' boxes', the note's: that box is
///   given up on, or emptied, with something held.
#[test]
fn every_box_holds_with_the_same_keys() {
    for place in places() {
        let name = place.name;
        let scratch =
            support::Scratch::new(&format!("selecting-{}", name.replace([' ', '\''], "-")));
        let mut app = (place.open)(&scratch);
        assert!((place.showing)(&app), "{name} did not open");
        support::type_text(&mut app, SAID);

        chord(&mut app, KeyCode::Left, CONTROL_SHIFT);
        assert_eq!(
            (place.held)(&app).as_deref(),
            Some("three"),
            "control, shift and left did not hold a word in {name}"
        );
        chord(&mut app, KeyCode::Home, CONTROL_SHIFT);
        assert_eq!(
            (place.held)(&app).as_deref(),
            Some(SAID),
            "control, shift and home did not hold to the start in {name}"
        );

        // Escape lets go first, and the box is still there to type in.
        support::press(&mut app, KeyCode::Esc);
        assert_eq!((place.held)(&app), None, "escape did not let go in {name}");
        assert!(
            (place.showing)(&app),
            "escape gave up on {name} while something was held"
        );

        // From the start, where control, shift and home left the caret.
        chord(&mut app, KeyCode::End, KeyModifiers::SHIFT);
        assert_eq!(
            (place.held)(&app).as_deref(),
            Some(SAID),
            "shift and end did not hold to the end in {name}"
        );
        support::press(&mut app, KeyCode::Esc);
        chord(&mut app, KeyCode::Left, KeyModifiers::SHIFT);
        assert_eq!(
            (place.held)(&app).as_deref(),
            Some("e"),
            "shift and left did not hold a character in {name}"
        );
        chord(&mut app, KeyCode::Home, CONTROL_SHIFT);
        chord(&mut app, KeyCode::End, CONTROL_SHIFT);
        assert_eq!(
            (place.held)(&app).as_deref(),
            None,
            "control, shift and end did not take the hold back to the end in {name}"
        );

        support::press_control(&mut app, 'a');
        assert_eq!(
            (place.held)(&app).as_deref(),
            Some(SAID),
            "control and a did not hold all of {name}"
        );
        assert!(
            app.current_buffer()
                .is_none_or(|buffer| !buffer.has_selection()),
            "control and a took hold of the file behind {name}"
        );

        // And bare home is where the caret goes, in a list's box as in any
        // other: the list's own ends are control's. From the end, with
        // nothing held, or a home that went to the list would leave the
        // hold control and a made for shift and end to keep.
        support::press(&mut app, KeyCode::Esc);
        support::press(&mut app, KeyCode::Home);
        chord(&mut app, KeyCode::End, KeyModifiers::SHIFT);
        assert_eq!(
            (place.held)(&app).as_deref(),
            Some(SAID),
            "home did not take the caret to the start of {name}"
        );

        // Delete takes what is in front of the caret, and then what is
        // held: a box over a list shares the key with its rows, and it is
        // the box's while the box has something for it.
        support::press(&mut app, KeyCode::Esc);
        support::press(&mut app, KeyCode::Home);
        support::press(&mut app, KeyCode::Delete);
        assert_eq!(
            (place.said)(&app).as_deref(),
            Some("ne two three"),
            "delete did not take the character in front of the caret in {name}"
        );
        support::press_control(&mut app, 'a');
        support::press(&mut app, KeyCode::Delete);
        assert_eq!(
            (place.said)(&app).as_deref(),
            Some(""),
            "delete did not take what was held in {name}"
        );
    }
}

/// And a copy takes what is held, from the box rather than from anything
/// behind it -- which for the list of names was nothing at all, because it
/// took every key there was and let none through to the table.
///
/// Deliberate break: the list of names swallowing what its query refuses
/// again (`Outcome::Consumed` in place of `Ignored`): nothing is copied.
#[test]
fn a_copy_takes_what_is_held_in_any_box() {
    let _turn = support::clipboard_turn();
    obelus_clipboard::use_provider_for_test(obelus_clipboard::Provider::Kept);
    for place in places() {
        let name = place.name;
        let scratch = support::Scratch::new(&format!("copying-{}", name.replace([' ', '\''], "-")));
        let mut app = (place.open)(&scratch);
        support::type_text(&mut app, SAID);
        chord(&mut app, KeyCode::Left, CONTROL_SHIFT);
        support::press_control(&mut app, 'c');
        assert_eq!(
            app.note().unwrap_or_default(),
            "Copied selection",
            "control and c did not copy what was held in {name}"
        );
        assert_eq!(
            obelus_clipboard::paste().as_deref(),
            Some("three"),
            "what was copied out of {name} is not what was held"
        );
    }
}

/// What is held is drawn, in the colour a selection is drawn in everywhere.
///
/// Deliberate break: the question on the status row, the list of names and
/// the projects' box drawn with `write` again, as they were: the word is
/// in the page's colour.
#[test]
fn what_is_held_in_any_box_is_drawn() {
    for place in places() {
        let name = place.name;
        let scratch = support::Scratch::new(&format!("drawn-{}", name.replace([' ', '\''], "-")));
        let mut app = (place.open)(&scratch);
        support::type_text(&mut app, SAID);
        chord(&mut app, KeyCode::Left, CONTROL_SHIFT);
        let (y, x) = box_of(&mut app);
        let cells = support::cells_of(&mut app, 76, 18);
        let held = app.theme().selection_background;
        let at = |offset: usize| &cells[(x + u16::try_from(offset).expect("a column"), y)];
        let three = SAID.find("three").expect("the word");
        assert_eq!(
            at(three).bg,
            held,
            "what is held in {name} is not drawn held"
        );
        assert_ne!(at(0).bg, held, "what is not held in {name} is drawn held");
    }
}

/// A drag over the box holds what it crosses, and two presses hold a word.
///
/// Deliberate break: the list of names and the projects' box left out of
/// `place_on_status`: the drag holds nothing in them.
#[test]
fn a_drag_holds_in_any_box() {
    for place in places() {
        let name = place.name;
        let scratch =
            support::Scratch::new(&format!("dragging-{}", name.replace([' ', '\''], "-")));
        let mut app = (place.open)(&scratch);
        support::type_text(&mut app, SAID);
        let (y, x) = box_of(&mut app);
        app.handle(Event::Pointer {
            kind: Pointer::Pressed,
            x,
            y,
        });
        app.handle(Event::Pointer {
            kind: Pointer::Dragged,
            x: x + 3,
            y,
        });
        assert_eq!(
            (place.held)(&app).as_deref(),
            Some("one"),
            "the drag did not hold what it crossed in {name}"
        );
    }
}

/// Shift and the ends hold in the settings' filter even with rows under it
/// to walk -- which is the case the table above cannot reach, because what
/// it types leaves the page with no rows at all.
///
/// Deliberate breaks: `bare ||` back on the settings' arm that walks the
/// page, so that shift and end go to its last row: nothing is held; and
/// `Move::under_a_box` giving bare home to the page: the caret stays put.
#[test]
fn shift_and_an_end_hold_in_the_settings_over_their_rows() {
    let scratch = support::Scratch::new("selecting-settings-rows");
    let mut app = on_a_file(&scratch);
    dispatch::dispatch(&mut app, Command::ConfigOpen);
    support::type_text(&mut app, "theme");
    assert!(
        support::render(&mut app, 76, 18).contains("Theme"),
        "the filter left no rows to walk"
    );
    chord(&mut app, KeyCode::Home, CONTROL_SHIFT);
    support::press(&mut app, KeyCode::Esc);
    chord(&mut app, KeyCode::End, KeyModifiers::SHIFT);
    let held = |app: &App| {
        let settings = app.settings().expect("the settings");
        run_of(&settings.query(), settings.query_held())
    };
    assert_eq!(
        held(&app).as_deref(),
        Some("theme"),
        "shift and end went to the page rather than holding the filter"
    );
    // And bare, the caret's, which is the arrangement a list's query has --
    // from the end and with nothing held, so that a home which went to the
    // page leaves nothing for shift and end to hold.
    support::press(&mut app, KeyCode::Esc);
    support::press(&mut app, KeyCode::Home);
    chord(&mut app, KeyCode::End, KeyModifiers::SHIFT);
    assert_eq!(
        held(&app).as_deref(),
        Some("theme"),
        "home went to the page rather than to the start of the filter"
    );
}
