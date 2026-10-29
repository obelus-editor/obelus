//! What Obelus says about the key just pressed, and which kind of thing it
//! is.
//!
//! One row, one place, two kinds. `Saved` and `Not saved` are the same
//! words in the same corner of the same row, and until the ink told them
//! apart a reader glancing at it read them the same -- which is the whole
//! of what the status row's note is for: half of what a language server
//! does is answer with nothing, and nothing is invisible.

use obelus_app::app::{App, dispatch};
use obelus_buffer::Buffer;
use obelus_command::Command;
use obelus_theme::builtin::DARK;
use ratatui::style::Color;

use crate::support;

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
/// every note. A report goes red. Break the other way by having it answer
/// `None` always, and `Nothing to undo` goes back to the ink a report is
/// in.
#[test]
fn a_refusal_is_in_the_ink_of_things_that_went_wrong() {
    let (_scratch, mut app) = reading("said-wrong");

    // A refusal: nothing has been changed, so there is nothing to put back.
    dispatch::dispatch(&mut app, Command::Undo);
    assert_eq!(app.note(), Some("Nothing to undo"));
    assert_eq!(ink(&mut app), wrong_ink(), "a refusal is not in the red");

    // And a report, from the same row in the same corner. `Copied` and
    // `Could not copy` are a pair a reader glancing at the row would read
    // the same; through the door a report goes through, because copying
    // for real is the machine's clipboard.
    app.say_for_test("Copied selection");
    assert_ne!(
        ink(&mut app),
        wrong_ink(),
        "a report is drawn as though it went wrong"
    );
    // In the row's own ink: while it is there it is the whole row, so
    // there is nothing for it to be an aside to.
    assert_eq!(
        ink(&mut app),
        DARK.status_foreground,
        "the note is not in the row's ink"
    );
}

/// What succeeded and is already on screen is not said: saving takes
/// `unsaved` off the row, which says it, and a note would have the row and
/// cover the file it is about until the next key.
///
/// And what was said while it was on its way goes with it: a save that
/// asked a server to lay the file out first says so, and that was once
/// said over by `Saved` -- left alone, it says the save is still going.
///
/// Deliberate breaks: say `Saved` again in `write_now`; and take the
/// `quiet` out of it, which leaves `Laying it out…` on the row.
#[test]
fn a_save_says_nothing_the_row_does_not() {
    let (_scratch, mut app) = reading("said-saved");
    support::type_text(&mut app, "// ");
    // What a save that is laying the file out says first, through the same
    // door: the server that would answer is a stand-in's job, and what is
    // asserted here is what the save does with what was said before it.
    app.say_for_test("Laying it out\u{2026}");
    dispatch::dispatch(&mut app, Command::FileSave);
    assert_eq!(app.note(), None, "saving was said");
    let dump = support::render(&mut app, 76, 12);
    let status = support::text_block(&dump)
        .lines()
        .last()
        .expect("a status row")
        .to_string();
    assert!(
        status.contains("hint.rs") && !status.contains("unsaved"),
        "the row does not say it was written:\n{dump}"
    );
}

/// An agent changing a file the reader has open says nothing on the status
/// row: the change is on screen as text, the row says the file is not
/// written, and the transcript says which call did it.
///
/// Deliberate break: say `The agent changed this file` again where the
/// agent's write goes through the buffer.
#[test]
fn an_agents_change_to_an_open_file_says_nothing_the_screen_does_not() {
    let (scratch, mut app) = reading("said-agent-wrote");
    let path = scratch.join("src/hint.rs");
    assert!(app.write_for_agent_for_test(&path, "fn hint() { 1 }\n"));
    assert_eq!(app.note(), None, "the agent's change was said");
    let dump = support::render(&mut app, 76, 12);
    assert!(
        support::text_block(&dump).contains("fn hint() { 1 }"),
        "the change is not on screen:\n{dump}"
    );
    let status = support::text_block(&dump)
        .lines()
        .last()
        .expect("a status row")
        .to_string();
    assert!(
        status.contains("unsaved"),
        "the row does not say the file is not written:\n{dump}"
    );
}

/// While Obelus has something to say it is the whole row: the file's name,
/// the position and the surroundings come back at the next key, which is
/// the one that takes it.
///
/// One place for it on every row, and only the row's width to cut it --
/// where each row once put it somewhere of its own, and dropped it whole
/// where it did not fit.
///
/// Deliberate break: take the arm for `middle` out of `StatusView::render`.
/// The note goes back beside nothing, and the file's row is drawn without
/// it.
#[test]
fn a_note_is_the_whole_row() {
    let (_scratch, mut app) = reading("said-whole");
    app.say_for_test("Saved");
    let dump = support::render(&mut app, 76, 12);
    let status = support::text_block(&dump)
        .lines()
        .last()
        .expect("a status row")
        .to_string();
    assert!(
        status.contains("Saved"),
        "the note is not on the row:
{dump}"
    );
    assert!(
        !status.contains("hint.rs") && !status.contains("1:1"),
        "the row says more than the note:
{dump}"
    );

    // And gone at the next key, with the row back.
    support::press(&mut app, crossterm::event::KeyCode::Right);
    let dump = support::render(&mut app, 76, 12);
    let status = support::text_block(&dump)
        .lines()
        .last()
        .expect("a status row")
        .to_string();
    assert!(
        status.contains("hint.rs") && !status.contains("Saved"),
        "the row did not come back:
{dump}"
    );
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
/// Deliberate break: lowercase any one of `Nothing to undo`, `Renamed to
/// {}` or `Thinking\u{2026}`. Which is how this was found: five of them
/// had been lowercase since they were written, against the one sentence in
/// the guide about how Obelus writes.
#[test]
fn what_is_said_reads_as_copy() {
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
