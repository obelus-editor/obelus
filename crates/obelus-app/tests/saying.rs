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
///
/// And closing a file says nothing on the way, and leaves nothing said
/// about it: the reader did it, and is looking at what is there instead.
/// Broken deliberately by saying `Closed {}` again where a file is shut,
/// or by taking the `quiet` out of `close`.
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
    // Something said about the file before it goes, the way a press on the
    // row or a card's answer leaves it -- neither of which quiets the row
    // the way a key does on its way in.
    app.say_for_test("Saved");
    dispatch::dispatch(&mut app, Command::DocumentClose);
    // Closing says nothing, and takes what was said about the file with it:
    // the reader is looking at what is there instead.
    assert_eq!(app.note(), None, "closing a file left something said");
    app.say_for_test("Saved");
    assert_eq!(
        ink(&mut app),
        DARK.status_foreground,
        "alone, it is the row"
    );
}

/// Whether a note reads as something Obelus wrote.
///
/// A capital, or a name kept in the spelling it has everywhere else. The
/// second half is the rule and not an excuse: `rust-analyzer is not
/// installed` starts lowercase and is right, because rewording it round
/// the name would be Obelus misspelling somebody else's -- which is what
/// the rule says to do instead of capitalising one.
fn reads_as_copy(said: &str, name: Option<&str>) -> bool {
    said.starts_with(char::is_uppercase) || name.is_some_and(|name| said.starts_with(name))
}

/// Everything Obelus says on the status row begins with a capital, unless
/// a name begins it.
///
/// A handful of notes rather than a list of every one, and each from a
/// different corner: what a command reports, what it refuses, what a file
/// being called something else is said as, and what leads with a path. A list
/// of the notes that exist would be a test that asks the rule what it expects.
///
/// One app per case, because what is being checked is the note and not the
/// state a previous case left behind -- a close that asks about unwritten
/// work is a key that answered something else.
///
/// Deliberate break: lowercase any one of `Saved`, `Renamed to {}` or
/// `Thinking\u{2026}`. Which is how this was found: five of them
/// had been lowercase since they were written, against the one sentence in
/// the guide about how Obelus writes.
#[test]
fn what_is_said_reads_as_copy() {
    // What a command reports.
    let (_a, mut app) = reading("copy-saved");
    support::type_text(&mut app, "// ");
    dispatch::dispatch(&mut app, Command::FileSave);
    let said = app.note().unwrap_or_default();
    assert!(reads_as_copy(said, None), "saving: {said:?}");

    // What it refuses.
    let (_b, mut app) = reading("copy-refused");
    dispatch::dispatch(&mut app, Command::Undo);
    let said = app.note().unwrap_or_default();
    assert!(reads_as_copy(said, None), "undoing nothing: {said:?}");

    // What a file being called something else is said as.
    let (_d, mut app) = reading("copy-renamed");
    dispatch::dispatch(&mut app, Command::FileRename);
    for _ in 0..80 {
        support::press(&mut app, crossterm::event::KeyCode::Backspace);
    }
    support::type_text(&mut app, "src/called.rs");
    support::press(&mut app, crossterm::event::KeyCode::Enter);
    let said = app.note().unwrap_or_default();
    assert!(reads_as_copy(said, None), "renaming: {said:?}");

    // And what leads with a path, which keeps its own spelling: the rule
    // says to reword round a name rather than capitalise one.
    let (_e, mut app) = reading("copy-named");
    dispatch::dispatch(&mut app, Command::FileNew);
    for _ in 0..80 {
        support::press(&mut app, crossterm::event::KeyCode::Backspace);
    }
    support::type_text(&mut app, "src/hint.rs");
    support::press(&mut app, crossterm::event::KeyCode::Enter);
    let said = app.note().unwrap_or_default();
    // In the spelling it was typed in, not this platform's: Windows takes
    // `/` as a separator and `Path::display` writes back what it was
    // given, so the name in the sentence is the reader's own. `as_shown`
    // is for the paths Obelus builds and spells itself.
    assert!(reads_as_copy(said, Some("src/hint.rs")), "a path: {said:?}");
    assert!(
        !said.starts_with(char::is_uppercase),
        "the name was rewritten rather than led with: {said:?}"
    );
}
