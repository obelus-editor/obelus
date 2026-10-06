//! The pointer, over the text.
//!
//! A terminal reports where a button went down and nothing else -- no
//! double click, no idea what is under it -- so everything here is
//! Obelus's own arithmetic, and the part worth testing is that a cell on
//! screen becomes the place in the file a reader was pointing at.

mod support;

use crossterm::event::KeyCode;
use obelus_app::{
    app::App,
    event::{Event, Pointer},
};
use obelus_buffer::Buffer;

/// An application over a file of the test's own, laid out.
fn editing(name: &str, contents: &str) -> (support::Scratch, App) {
    let scratch = support::Scratch::new(name);
    let path = scratch.path().join("sample.rs");
    std::fs::write(&path, contents).expect("writing the file");
    let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, 60, 16);
    (scratch, app)
}

fn press_at(app: &mut App, x: u16, y: u16) {
    app.handle(Event::Pointer {
        kind: Pointer::Pressed,
        x,
        y,
    });
}

fn drag_to(app: &mut App, x: u16, y: u16) {
    app.handle(Event::Pointer {
        kind: Pointer::Dragged,
        x,
        y,
    });
}

fn caret(app: &App) -> (usize, usize) {
    let cursor = app.current_buffer().expect("a buffer").cursor();
    (cursor.line.get(), cursor.column.get())
}

/// Where a character is on screen, as the dump shows it.
fn cell_of(app: &mut App, needle: &str) -> (u16, u16) {
    let dump = support::render(app, 60, 16);
    let rows: Vec<String> = support::text_block(&dump)
        .lines()
        .filter_map(|row| row.split_once('|').map(|(_, cells)| cells.to_string()))
        .collect();
    let y = rows
        .iter()
        .position(|row| row.contains(needle))
        .unwrap_or_else(|| panic!("{needle:?} is not on screen:\n{dump}"));
    let x = rows[y].find(needle).expect("the cell");
    (
        u16::try_from(x).expect("a column"),
        u16::try_from(y).expect("a row"),
    )
}

#[test]
fn a_click_puts_the_caret_where_it_landed() {
    let (_scratch, mut app) = editing("pointer-click", "fn main() {\n    let name = 1;\n}\n");
    let (x, y) = cell_of(&mut app, "name");
    press_at(&mut app, x + 2, y);
    assert_eq!(caret(&app), (1, 10), "the caret is not under the pointer");

    // Past the end of a line is the end of it, not the line below.
    press_at(&mut app, 58, y);
    assert_eq!(caret(&app), (1, 17));
}

/// A click on the gutter is a reader pointing at a line, not at nothing.
#[test]
fn a_click_left_of_the_text_lands_at_the_start_of_the_line() {
    let (_scratch, mut app) = editing("pointer-gutter", "fn main() {\n    let name = 1;\n}\n");
    let (_, y) = cell_of(&mut app, "let name");
    press_at(&mut app, 0, y);
    assert_eq!(caret(&app), (1, 0));
}

#[test]
fn dragging_selects_what_it_is_dragged_over() {
    let (_scratch, mut app) = editing("pointer-drag", "fn main() {\n    let name = 1;\n}\n");
    let (x, y) = cell_of(&mut app, "name");
    press_at(&mut app, x, y);
    drag_to(&mut app, x + 4, y);
    assert_eq!(
        app.current_buffer()
            .and_then(|buffer| buffer.selected_text()),
        Some("name".to_string())
    );

    // And on past the end of the line, which is the row below.
    drag_to(&mut app, x, y + 1);
    let selected = app
        .current_buffer()
        .and_then(|buffer| buffer.selected_text())
        .expect("a selection");
    assert!(
        selected.starts_with("name = 1;\n"),
        "the drag did not reach the next row: {selected:?}"
    );
}

/// Twice is the word and three times is the line, which is what every
/// editor with a pointer has taught.
#[test]
fn two_clicks_take_the_word_and_three_take_the_line() {
    let (_scratch, mut app) = editing("pointer-counts", "fn main() {\n    let name = 1;\n}\n");
    let (x, y) = cell_of(&mut app, "name");
    press_at(&mut app, x + 1, y);
    press_at(&mut app, x + 1, y);
    assert_eq!(
        app.current_buffer()
            .and_then(|buffer| buffer.selected_text()),
        Some("name".to_string()),
        "two presses did not take the word"
    );

    press_at(&mut app, x + 1, y);
    assert_eq!(
        app.current_buffer()
            .and_then(|buffer| buffer.selected_text()),
        Some("    let name = 1;\n".to_string()),
        "three presses did not take the line, break and all"
    );

    // A press somewhere else is a first press again.
    let (other_x, other_y) = cell_of(&mut app, "fn main");
    press_at(&mut app, other_x, other_y);
    assert!(
        app.current_buffer()
            .and_then(|buffer| buffer.selection())
            .is_none(),
        "the count carried over to a press somewhere else"
    );
}

/// The pointer moving with nothing held down moves nothing: it is noted,
/// because a pointer that comes to rest is asking what is under it, and
/// noted is all it is.
#[test]
fn moving_the_pointer_moves_nothing() {
    let (_scratch, mut app) = editing("pointer-moved", "fn main() {\n    let name = 1;\n}\n");
    let (x, y) = cell_of(&mut app, "name");
    app.handle(Event::Pointer {
        kind: Pointer::Moved,
        x,
        y,
    });
    assert_eq!(caret(&app), (0, 0), "the caret followed the pointer");
    assert!(
        app.current_buffer()
            .and_then(|buffer| buffer.selection())
            .is_none(),
        "something was selected by a pointer with no button down"
    );
    assert!(app.hover().is_none(), "a question was asked with no server");
}

/// A list is what the screen is showing while it is up, so a click on the
/// code behind it would move a caret nobody can see.
#[test]
fn a_click_does_nothing_while_a_list_is_open() {
    let (_scratch, mut app) = editing("pointer-list", "fn main() {\n    let name = 1;\n}\n");
    let (x, y) = cell_of(&mut app, "name");
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::CommandPalette);
    press_at(&mut app, x, y);
    assert_eq!(caret(&app), (0, 0), "the click reached the file");
}

/// A file being read as markdown has no places for a caret: its rows are
/// not the file's lines.
#[test]
fn a_click_does_nothing_over_a_reading() {
    let scratch = support::Scratch::new("pointer-reading");
    let path = scratch.path().join("readme.md");
    std::fs::write(&path, "# Heading\n\nSome prose.\n").expect("writing it");
    let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, 60, 16);
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::PreviewToggle);

    press_at(&mut app, 4, 2);
    assert_eq!(caret(&app), (0, 0));

    // And the keys still do what they did.
    support::press(&mut app, KeyCode::Down);
}

/// A drag held past the bottom of the text keeps going.
///
/// A terminal reports a drag when the pointer moves and says nothing at
/// all while a held pointer is still, so a reader who runs out of screen
/// halfway through a selection used to be stuck: the rows they wanted were
/// below the edge, and pushing the pointer into the edge did nothing
/// because the pointer was not moving any more. What is remembered is that
/// the drag is out there, and every tick carries the text up under it.
///
/// Broken deliberately by taking `drag_on` out of the tick, which leaves
/// the selection stopped on the last row that was on screen when the
/// pointer reached the edge.
#[test]
fn a_drag_held_past_the_bottom_keeps_selecting() {
    let many: String = (1..=200).map(|line| format!("line {line}\n")).collect();
    let (_scratch, mut app) = editing("pointer-autoscroll", &many);

    let (x, y) = cell_of(&mut app, "line 1");
    press_at(&mut app, x, y);
    // Past the bottom of the screen, which is past the bottom of the text:
    // the reader has run out of room and is leaning on the edge.
    drag_to(&mut app, x, 40);
    let stopped = caret(&app).0;

    for _ in 0..5 {
        app.handle(Event::Tick);
    }
    let carried = caret(&app).0;
    assert!(
        carried > stopped + 5,
        "the selection stayed at line {stopped} while the pointer was held past the edge, \
         reaching only line {carried}"
    );

    // And letting go stops it: nothing more arrives from a pointer that is
    // no longer down, and a terminal that swallowed the release -- the
    // pointer left the window -- still has the next key to end it on.
    app.handle(Event::Pointer {
        kind: Pointer::Released,
        x,
        y: 10,
    });
    let let_go = caret(&app).0;
    for _ in 0..5 {
        app.handle(Event::Tick);
    }
    assert_eq!(
        caret(&app).0,
        let_go,
        "the selection carried on after the button came up"
    );

    // And a drag whose release never arrives ends on the next key. A
    // pointer that leaves the terminal takes its release with it, and a
    // drag nothing ever ended would go on scrolling under whatever the
    // reader did next.
    drag_to(&mut app, x, 40);
    support::press(&mut app, KeyCode::Left);
    let after = caret(&app).0;
    for _ in 0..5 {
        app.handle(Event::Tick);
    }
    assert_eq!(
        caret(&app).0,
        after,
        "the selection carried on after the reader went back to the keyboard"
    );
}

/// The welcome screen's website is pressed to open, and says so under the
/// pointer.
///
/// The one thing on that screen a press does, so it is raised while the
/// pointer is over it and only then -- and a press on the plate above it is
/// still nothing.
///
/// Broken deliberately three ways. Taking out the arm in `on_pointer` left
/// the press on the address opening nothing. Making the arm take the whole
/// region instead of the address's cells opened the site from a press on the
/// plate. And giving `WelcomeView::site` the page's colour whatever the
/// pointer was doing left nothing raised under it.
#[test]
fn the_welcome_screen_opens_the_website_where_it_is_pressed() {
    const WIDTH: u16 = 64;
    const HEIGHT: u16 = 20;
    obelus_clipboard::links::use_opener_for_test(obelus_clipboard::links::Opener::Kept);
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(std::path::PathBuf::from("/tmp/obelus"));

    // Found where it is drawn, not asked of the arithmetic that places it.
    let cells = support::cells_of(&mut app, WIDTH, HEIGHT);
    let (x, y) = (0..HEIGHT)
        .find_map(|y| {
            let row: String = (0..WIDTH).map(|x| cells[(x, y)].symbol()).collect();
            let from = row.find("obelus-editor.github.io/obelus")?;
            Some((u16::try_from(row[..from].chars().count()).ok()?, y))
        })
        .expect("the welcome screen does not say where the website is");
    let ground_under = |app: &mut App, at: (u16, u16)| support::cells_of(app, WIDTH, HEIGHT)[at].bg;
    let point = |app: &mut App, kind: Pointer, x: u16, y: u16| {
        app.handle(Event::Pointer { kind, x, y });
    };

    let page = app.theme().background;
    let raised = app.theme().raised_background;
    assert_ne!(
        page, raised,
        "a theme where the two are one says nothing here"
    );
    assert_eq!(
        ground_under(&mut app, (x, y)),
        page,
        "raised before the pointer came"
    );
    point(&mut app, Pointer::Moved, x + 5, y);
    assert_eq!(
        ground_under(&mut app, (x, y)),
        raised,
        "nothing is raised under the pointer"
    );
    point(&mut app, Pointer::Moved, x + 5, 4);
    assert_eq!(
        ground_under(&mut app, (x, y)),
        page,
        "still raised after the pointer left"
    );

    // The plate, above it, is nothing.
    point(&mut app, Pointer::Pressed, x + 5, 4);
    point(&mut app, Pointer::Released, x + 5, 4);
    assert_eq!(
        obelus_clipboard::links::opened(),
        None,
        "a press on the plate opened something"
    );

    point(&mut app, Pointer::Pressed, x + 5, y);
    point(&mut app, Pointer::Released, x + 5, y);
    assert_eq!(
        obelus_clipboard::links::opened().as_deref(),
        Some("https://obelus-editor.github.io/obelus/"),
        "a press on the address did not open the website"
    );
}
