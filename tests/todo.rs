//! What a tree means to come back to: the file, the view, and the keys.

mod support;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use obelus::{
    app::App,
    command::{Command, dispatch},
    event::Event,
};
use support::press;

/// A tree with notes already in it.
fn tree(name: &str, notes: &str) -> support::Scratch {
    let scratch = support::Scratch::new(&format!("todo-{name}"));
    std::fs::create_dir_all(scratch.path().join(".obelus")).expect("the directory");
    std::fs::write(scratch.path().join(".obelus").join("todo.toml"), notes).expect("the notes");
    scratch
}

fn open(scratch: &support::Scratch, width: u16, height: u16) -> App {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, width, height);
    dispatch::dispatch(&mut app, Command::TodoOpen);
    app
}

fn alt(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::ALT))
}

const THREE: &str = r#"
[[todo]]
said = "wire the counts tree up to the search"
done = false

[[todo]]
said = """
this cache does not notice a theme
rows caches (width, rows), and a theme
changes the colours, not the count.
"""
done = false
at = "sample.rs"
line = 2

[[todo]]
said = "settings live in a directory now"
done = true
"#;

/// The notes are rows, done is said in a box, and where a note points is at
/// the right-hand end.
#[test]
fn the_notes_are_a_list_of_what_to_come_back_to() {
    let scratch = tree("list", THREE);
    let mut app = open(&scratch, 76, 18);
    let dump = support::render(&mut app, 76, 18);
    let text = support::text_block(&dump);

    assert!(text.contains("wire the counts tree"), "{dump}");
    assert!(
        text.contains("\u{25a1} wire"),
        "a note that is not done has no empty box:\n{dump}"
    );
    assert!(
        text.contains("\u{2611} settings live"),
        "a note that is done has no ticked box:\n{dump}"
    );
    assert!(
        text.contains("sample.rs:2"),
        "the place a note is about is not on its row:\n{dump}"
    );
    // Only its first line, until it is opened.
    assert!(
        !text.contains("rows caches"),
        "a note's body is on screen unasked:\n{dump}"
    );
}

/// What the keys do is at the foot of the view, because three of them do
/// nothing anywhere else in obelus.
#[test]
fn the_view_says_what_its_keys_do() {
    let scratch = tree("hints", THREE);
    let mut app = open(&scratch, 76, 18);
    // On a note that points somewhere, so "go" is a key that does something.
    press(&mut app, KeyCode::Down);
    let text = support::text_block(&support::render(&mut app, 76, 18)).to_string();
    // The common ones that can be pressed, and the way to the rest.
    for word in ["go", "done", "new", "leave", "keys"] {
        assert!(text.contains(word), "{word:?} is not at the foot:\n{text}");
    }
    // Not the rest of them: those are a keypress away, not a row of the
    // reader's screen.
    assert!(
        !text.contains("write this one over"),
        "the foot is the whole list:\n{text}"
    );
}

/// A note with more behind it folds, on the mark and the key every other
/// folding thing in obelus uses.
#[test]
fn a_note_with_a_body_folds_open() {
    let scratch = tree("fold", THREE);
    let mut app = open(&scratch, 76, 18);
    press(&mut app, KeyCode::Down);
    app.handle(alt(KeyCode::Char('f')));

    let dump = support::render(&mut app, 76, 18);
    let text = support::text_block(&dump);
    assert!(text.contains("rows caches"), "it would not open:\n{dump}");
    assert!(text.contains('\u{25be}'), "the mark did not turn:\n{dump}");

    app.handle(alt(KeyCode::Char('f')));
    assert!(
        !support::text_block(&support::render(&mut app, 76, 18)).contains("rows caches"),
        "it would not fold again"
    );
}

/// Space ticks a note, and the file says so straight away: a note ticked in
/// obelus and lost when it closed would be worse than no tick at all.
#[test]
fn space_ticks_a_note_and_writes_it_down() {
    let scratch = tree("tick", THREE);
    let mut app = open(&scratch, 76, 18);
    press(&mut app, KeyCode::Char(' '));

    let written = std::fs::read_to_string(scratch.path().join(".obelus").join("todo.toml"))
        .expect("the notes");
    let table = written.parse::<toml::Table>().expect("it parses");
    let notes = table["todo"].as_array().expect("the notes");
    assert_eq!(
        notes[0]["done"].as_bool(),
        Some(true),
        "the tick was not written down: {written}"
    );
    // And the others are left as they were.
    assert_eq!(notes[1]["done"].as_bool(), Some(false));
}

/// Delete takes a note away, and the file loses it too.
#[test]
fn delete_takes_a_note_away() {
    let scratch = tree("drop", THREE);
    let mut app = open(&scratch, 76, 18);
    press(&mut app, KeyCode::Delete);

    let written = std::fs::read_to_string(scratch.path().join(".obelus").join("todo.toml"))
        .expect("the notes");
    assert!(
        !written.contains("wire the counts tree"),
        "it is still there: {written}"
    );
    assert!(
        written.contains("settings live in a directory"),
        "it took the others with it: {written}"
    );
}

/// Enter goes where a note points, and the view gets out of the way.
///
/// Broken deliberately by leaving the view open behind the file: the line
/// was read under a list nobody had asked to keep.
#[test]
fn enter_goes_to_what_a_note_is_about() {
    let scratch = tree("go", THREE);
    // The file the note points at, in the tree the notes belong to.
    std::fs::write(scratch.path().join("sample.rs"), "one\ntwo\nthree\n").expect("the file");

    let mut app = open(&scratch, 76, 18);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);

    assert!(app.notes().is_none(), "the view stayed over the file");
    let buffer = app.current_buffer().expect("nothing was opened");
    assert!(
        buffer.path().ends_with("sample.rs"),
        "opened {} instead",
        buffer.path().display()
    );
    assert_eq!(
        buffer.cursor().line.get(),
        1,
        "it did not land on the line the note is about"
    );
}

/// A note about the project has nowhere to go, and enter says so by doing
/// nothing rather than by going somewhere arbitrary.
#[test]
fn enter_on_a_note_about_nothing_goes_nowhere() {
    let scratch = tree("nowhere", THREE);
    let mut app = open(&scratch, 76, 18);
    press(&mut app, KeyCode::Enter);
    assert!(
        app.notes().is_some(),
        "a note with no place took the reader somewhere"
    );
}

/// A note is made while reading, on the status bar, and carries where the
/// reader was.
#[test]
fn a_note_made_while_reading_carries_the_line() {
    let scratch = support::Scratch::new("todo-made");
    let file = scratch.path().join("thing.rs");
    std::fs::write(&file, "one\ntwo\nthree\nfour\n").expect("the file");

    let mut app = App::new(vec![
        obelus::buffer::Buffer::open(&file).expect("opening it"),
    ]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, 76, 18);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Down);

    dispatch::dispatch(&mut app, Command::TodoAdd);
    assert!(app.notes().is_some(), "it did not open the notes");
    support::type_text(&mut app, "look at this again");
    press(&mut app, KeyCode::Enter);

    let written = std::fs::read_to_string(scratch.path().join(".obelus").join("todo.toml"))
        .expect("the notes");
    let table = written.parse::<toml::Table>().expect("it parses");
    let note = &table["todo"].as_array().expect("the notes")[0];
    assert_eq!(note["said"].as_str(), Some("look at this again"));
    assert_eq!(note["at"].as_str(), Some("thing.rs"));
    assert_eq!(
        note["line"].as_integer(),
        Some(3),
        "not the line the reader was on: {written}"
    );
}

/// The same key from inside the view makes a note about the project, because
/// there is no line under a list.
#[test]
fn a_note_made_from_the_list_is_about_the_project() {
    let scratch = tree("project", THREE);
    let mut app = open(&scratch, 76, 18);
    app.handle(alt(KeyCode::Char('n')));
    support::type_text(&mut app, "think about this");
    press(&mut app, KeyCode::Enter);

    let written = std::fs::read_to_string(scratch.path().join(".obelus").join("todo.toml"))
        .expect("the notes");
    let table = written.parse::<toml::Table>().expect("it parses");
    let notes = table["todo"].as_array().expect("the notes");
    let last = notes.last().expect("the new one");
    assert_eq!(last["said"].as_str(), Some("think about this"));
    assert!(
        last.get("at").is_none(),
        "a note made from the list carried a place: {written}"
    );
}

/// `alt+e` opens a note in the box a message is written in, and enter
/// finishes it.
#[test]
fn a_note_can_be_written_over() {
    let scratch = tree("write", THREE);
    let mut app = open(&scratch, 76, 18);
    app.handle(alt(KeyCode::Char('e')));
    assert!(
        app.notes()
            .and_then(obelus::component::todo::TodoView::writing)
            .is_some(),
        "the box did not open"
    );

    // Everything typable goes in the box, including a space, which is a key
    // the list itself has.
    support::type_text(&mut app, " and soon");
    press(&mut app, KeyCode::Enter);

    let written = std::fs::read_to_string(scratch.path().join(".obelus").join("todo.toml"))
        .expect("the notes");
    assert!(
        written.contains("and soon"),
        "what was written did not reach the file: {written}"
    );
}

/// A tree with nothing to come back to says so, rather than showing an empty
/// region and leaving the reader to work out whether it is broken.
#[test]
fn a_tree_with_no_notes_says_so() {
    let scratch = support::Scratch::new("todo-empty");
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, 76, 18);
    dispatch::dispatch(&mut app, Command::TodoOpen);

    let dump = support::render(&mut app, 76, 18);
    assert!(
        support::text_block(&dump).contains("nothing to come back to"),
        "an empty view says nothing:\n{dump}"
    );
}

/// The foot says only what can be pressed now, and `f1` says all of it.
///
/// Broken deliberately by drawing every common hint whatever the selection
/// is on: "go" sat at the foot over a note about the project, which has
/// nowhere to go, and pressing it did nothing.
#[test]
fn the_foot_drops_a_key_that_would_do_nothing() {
    let scratch = tree("usable", THREE);
    let mut app = open(&scratch, 76, 18);

    // The first note is about the project, so there is nowhere to go.
    let text = support::text_block(&support::render(&mut app, 76, 18)).to_string();
    assert!(
        !text.contains("\u{f0311} go"),
        "the foot offered a key with nowhere to go:\n{text}"
    );

    // The second points at a file, and the key comes back.
    press(&mut app, KeyCode::Down);
    let text = support::text_block(&support::render(&mut app, 76, 18)).to_string();
    assert!(
        text.contains("go"),
        "the foot dropped a key that works here:\n{text}"
    );
}

/// `f1` shows every key, including the ones the foot left out.
#[test]
fn f1_shows_every_key_this_view_has() {
    let scratch = tree("keys", THREE);
    let mut app = open(&scratch, 76, 18);
    press(&mut app, KeyCode::F(1));

    let dump = support::render(&mut app, 76, 18);
    let text = support::text_block(&dump);
    assert!(text.contains("the keys here"), "no card:\n{dump}");
    for word in [
        "write this one over",
        "show what is behind it",
        "move it up or down",
        "take it away",
    ] {
        assert!(text.contains(word), "{word:?} is not on the card:\n{dump}");
    }

    // And the same key closes it, before it closes the view.
    press(&mut app, KeyCode::Esc);
    assert!(app.notes().is_some(), "escape left the view, not the card");
    assert!(
        !support::text_block(&support::render(&mut app, 76, 18)).contains("the keys here"),
        "the card stayed"
    );
}

/// A new note is written where it will live, and nothing is on the status
/// bar about it.
#[test]
fn a_new_note_is_written_in_the_list() {
    let scratch = tree("inline", THREE);
    let mut app = open(&scratch, 76, 18);
    app.handle(alt(KeyCode::Char('n')));

    // An empty row at the end, with the caret in it: what is typed shows up
    // on the row rather than anywhere else.
    support::type_text(&mut app, "a new one");
    let dump = support::render(&mut app, 76, 18);
    assert!(
        support::text_block(&dump).contains("a new one"),
        "the typing is not on its row:\n{dump}"
    );
    // The foot is the box's keys now, not the list's.
    assert!(
        support::text_block(&dump).contains("keep it"),
        "the foot still offers the list's keys:\n{dump}"
    );

    press(&mut app, KeyCode::Enter);
    let written = std::fs::read_to_string(scratch.path().join(".obelus").join("todo.toml"))
        .expect("the notes");
    assert!(written.contains("a new one"), "{written}");
}

/// A note that says nothing is not a note, however it got that way.
#[test]
fn a_note_with_nothing_in_it_is_dropped() {
    let scratch = tree("empty", THREE);
    let mut app = open(&scratch, 76, 18);
    let before = app.notes().expect("the view").rows().len();

    // Started and given up on.
    app.handle(alt(KeyCode::Char('n')));
    press(&mut app, KeyCode::Esc);
    assert_eq!(
        app.notes().expect("the view").rows().len(),
        before,
        "an abandoned note stayed"
    );

    // Started, typed blanks into, and kept.
    app.handle(alt(KeyCode::Char('n')));
    support::type_text(&mut app, "   ");
    press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.notes().expect("the view").rows().len(),
        before,
        "a note of blanks was kept"
    );

    // And an existing one emptied is taken away.
    app.handle(alt(KeyCode::Char('e')));
    for _ in 0..80 {
        press(&mut app, KeyCode::Backspace);
    }
    press(&mut app, KeyCode::Enter);
    let written = std::fs::read_to_string(scratch.path().join(".obelus").join("todo.toml"))
        .expect("the notes");
    assert!(
        !written.contains("wire the counts tree"),
        "emptying a note kept it: {written}"
    );
}

/// Where a note sits is the reader's to decide.
#[test]
fn alt_and_an_arrow_moves_a_note() {
    let scratch = tree("move", THREE);
    let mut app = open(&scratch, 76, 18);
    let said = |app: &App| -> Vec<String> {
        app.notes()
            .expect("the view")
            .rows()
            .iter()
            .filter(|row| row.head)
            .map(|row| row.said.clone())
            .collect()
    };
    let first = said(&app)[0].clone();

    app.handle(alt(KeyCode::Down));
    assert_eq!(said(&app)[1], first, "it did not move down");
    // The selection goes with it, which is what makes a second press move it
    // again rather than move whatever landed under the cursor.
    app.handle(alt(KeyCode::Down));
    assert_eq!(said(&app)[2], first, "the selection did not follow it");

    app.handle(alt(KeyCode::Up));
    assert_eq!(said(&app)[1], first);

    let written = std::fs::read_to_string(scratch.path().join(".obelus").join("todo.toml"))
        .expect("the notes");
    let table = written.parse::<toml::Table>().expect("it parses");
    let notes = table["todo"].as_array().expect("the notes");
    assert_eq!(
        notes[1]["said"].as_str(),
        Some(first.as_str()),
        "the order was not written down: {written}"
    );
}

/// The list starts at the first row: there are no tabs here and nothing to
/// filter by, so a title would be a row of the reader's screen spent saying
/// what they just asked for.
#[test]
fn the_list_starts_at_the_first_row() {
    let scratch = tree("notitle", THREE);
    let mut app = open(&scratch, 76, 18);
    let dump = support::render(&mut app, 76, 18);
    let first = support::text_block(&dump)
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default();
    assert!(
        first.contains("wire the counts tree"),
        "something is above the list:\n{dump}"
    );
}
