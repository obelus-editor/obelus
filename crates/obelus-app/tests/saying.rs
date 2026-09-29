//! What Obelus says about the key just pressed, and which kind of thing it
//! is.
//!
//! One row, one place, two kinds. `Saved` and `Not saved` are the same
//! words in the same corner of the same row, and until the ink told them
//! apart a reader glancing at it read them the same -- which is the whole
//! of what the status row's note is for: half of what a language server
//! does is answer with nothing, and nothing is invisible.

mod support;

use obelus_app::app::{App, dispatch};
use obelus_buffer::Buffer;
use obelus_command::Command;
use obelus_theme::builtin::DARK;
use ratatui::style::Color;

/// A project with a file open in it.
fn reading(name: &str) -> (support::Scratch, App) {
    let scratch = support::Scratch::new(name);
    scratch.write("src/hint.rs", "fn hint() {}\n");
    let mut app = App::new(vec![
        Buffer::open(&scratch.join("src/hint.rs")).expect("opening it"),
    ]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.statuses_for_test(std::collections::HashMap::new());
    support::lay_out(&mut app, 76, 12);
    (scratch, app)
}

/// The colour the note on the status row is drawn in.
///
/// Read off the cells rather than the dump, because what a note is written
/// in is a foreground and the dump names a style by its pair.
fn ink(app: &mut App) -> Color {
    let said = app.note().expect("something said").to_string();
    let dump = support::render(app, 76, 12);
    let row = support::text_block(&dump)
        .lines()
        .rfind(|row| row.contains(&said))
        .unwrap_or_else(|| panic!("what was said is not on the screen:\n{dump}"));
    let column = u16::try_from(support::column_of(row, &said)).expect("a column");
    let cells = support::cells_of(app, 76, 12);
    cells.cell((column, 11)).expect("a cell").fg
}

/// What everything that went wrong is written in, which is where a refusal
/// gets its colour from too.
fn wrong_ink() -> Color {
    DARK.colour_for(Some(obelus_text::kind::SyntaxKind::Error))
}

/// A refusal is in the ink everything that went wrong is in, and a report
/// is not.
///
/// The two halves are one claim and each passes with the other broken: a
/// note that never reached the red is the first, and one that is always
/// red is the second -- and the second is what colouring the whole channel
/// would have done, telling a reader that copying failed.
///
/// Deliberate break: have `wrong_ink` in `StatusView` answer `Some` for
/// every note. `Saved` goes red. Break the other way by having it answer
/// `None` always, and `Nothing to undo` goes back to the dim ink `Saved`
/// is in.
#[test]
fn a_refusal_is_in_the_ink_of_things_that_went_wrong() {
    let (_scratch, mut app) = reading("said-wrong");

    // A refusal: nothing has been changed, so there is nothing to put back.
    dispatch::dispatch(&mut app, Command::Undo);
    assert_eq!(app.note(), Some("Nothing to undo"));
    assert_eq!(ink(&mut app), wrong_ink(), "a refusal is not in the red");

    // And a report, from the same row in the same corner. `Saved` and
    // `Not saved` are the pair this is all about: a reader glancing at the
    // row read them the same.
    support::type_text(&mut app, "// ");
    dispatch::dispatch(&mut app, Command::FileSave);
    assert_eq!(app.note(), Some("Saved"));
    assert_ne!(
        ink(&mut app),
        wrong_ink(),
        "a report is drawn as though it went wrong"
    );
    assert_eq!(ink(&mut app), DARK.gutter, "beside a file it is an aside");
}

/// With nothing open the row is the note, so a report keeps the row's own
/// ink rather than the dim one an aside gets.
///
/// Deliberate break: hand the second caller `wrong_ink().unwrap_or(gutter)`
/// the way the first one is handed it. A note alone on an otherwise empty
/// row is then drawn in the colour of something standing beside
/// something else, with nothing there for it to stand beside.
#[test]
fn a_note_alone_on_the_row_is_the_row() {
    let scratch = support::Scratch::new("said-alone");
    scratch.write("src/hint.rs", "fn hint() {}\n");
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.statuses_for_test(std::collections::HashMap::new());
    support::lay_out(&mut app, 76, 12);

    // A refusal, which is what a reader with nothing open mostly gets.
    dispatch::dispatch(&mut app, Command::FileNew);
    support::type_text(&mut app, "src");
    support::press(&mut app, crossterm::event::KeyCode::Enter);
    assert_eq!(app.note(), Some("src is already there"));
    assert_eq!(ink(&mut app), wrong_ink(), "a refusal is not in the red");

    // And a report there is the row's own ink: there is no file's name
    // beside it for it to be an aside to.
    dispatch::dispatch(&mut app, Command::FileNew);
    support::type_text(&mut app, "made.rs");
    support::press(&mut app, crossterm::event::KeyCode::Enter);
    dispatch::dispatch(&mut app, Command::DocumentClose);
    assert!(
        app.note().is_some_and(|said| said.starts_with("closed")),
        "{:?}",
        app.note()
    );
    assert_eq!(
        ink(&mut app),
        DARK.status_foreground,
        "alone, it is the row"
    );
}
