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

/// Every line of every note, always: nothing is folded away.
///
/// Broken deliberately by putting only a note's first line on the page: a
/// note of three lines read as a note of one, and the reader had to open
/// each one to find out what it said.
#[test]
fn every_line_of_every_note_is_on_the_page() {
    let scratch = tree("list", THREE);
    let mut app = open(&scratch, 76, 18);
    let dump = support::render(&mut app, 76, 18);
    let text = support::text_block(&dump);

    assert!(text.contains("wire the counts tree"), "{dump}");
    assert!(text.contains("\u{25a1} wire"), "no empty box:\n{dump}");
    assert!(
        text.contains("\u{2611} settings live"),
        "no ticked box:\n{dump}"
    );
    assert!(text.contains("sample.rs:2"), "no place on the row:\n{dump}");
    assert!(
        text.contains("rows caches"),
        "a note's second line is not on the page:\n{dump}"
    );
}

/// It opens with the caret in it, and a letter is a letter.
///
/// Broken deliberately by keeping the box shut until a key opened it: every
/// change cost two keys, and the page looked like a list until the reader
/// found out otherwise.
#[test]
fn there_is_no_mode_to_get_into() {
    let scratch = tree("modeless", THREE);
    let mut app = open(&scratch, 76, 18);
    let dump = support::render(&mut app, 76, 18);
    assert!(
        support::cursor_line(&dump).contains(','),
        "it opened with no caret:\n{dump}"
    );

    support::type_text(&mut app, "!!");
    let dump = support::render(&mut app, 76, 18);
    assert!(
        support::text_block(&dump).contains("!!wire the counts"),
        "typing did not reach the note:\n{dump}"
    );

    // And leaving keeps it: there is no moment the reader says "done with
    // this note", so every way out of the view is one.
    press(&mut app, KeyCode::Esc);
    assert!(app.notes().is_none(), "escape did not leave");
    let written = std::fs::read_to_string(scratch.path().join(".obelus").join("todo.toml"))
        .expect("the notes");
    assert!(written.contains("!!wire the counts"), "{written}");
}

/// Enter starts another note where the caret is, and `shift+enter` is a line
/// inside one.
#[test]
fn enter_starts_another_note_and_shift_enter_a_line() {
    let scratch = tree("another", THREE);
    let mut app = open(&scratch, 76, 18);

    press(&mut app, KeyCode::Enter);
    support::type_text(&mut app, "a fresh one");
    app.handle(Event::Key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::SHIFT,
    )));
    support::type_text(&mut app, "and more of it");

    let dump = support::render(&mut app, 76, 18);
    let rows: Vec<&str> = support::text_block(&dump).lines().collect();
    let at = |needle: &str| {
        rows.iter()
            .position(|row| row.contains(needle))
            .unwrap_or_else(|| panic!("no {needle:?}:\n{dump}"))
    };
    // Right after the note it was started from, not at the end of the list.
    assert_eq!(at("a fresh one"), at("wire the counts") + 1);
    assert_eq!(at("and more of it"), at("a fresh one") + 1);

    press(&mut app, KeyCode::Esc);
    let written = std::fs::read_to_string(scratch.path().join(".obelus").join("todo.toml"))
        .expect("the notes");
    assert!(
        written.contains("a fresh one\nand more of it"),
        "the two lines are not one note: {written}"
    );
}

/// Up and down walk a note's own lines, and step to the next note when there
/// are none left.
#[test]
fn the_arrows_walk_the_lines_then_the_notes() {
    let scratch = tree("walk", THREE);
    let mut app = open(&scratch, 76, 18);
    let row = |app: &mut App| {
        let dump = support::render(app, 76, 18);
        support::cursor_line(&dump)
            .split_once(',')
            .and_then(|(_, y)| y.trim().parse::<u16>().ok())
            .unwrap_or(0)
    };
    assert_eq!(row(&mut app), 0);
    // Into the second note, then down its own two lines.
    for expected in [1, 2, 3, 4] {
        press(&mut app, KeyCode::Down);
        assert_eq!(row(&mut app), expected, "the caret did not walk down");
    }
    press(&mut app, KeyCode::Up);
    assert_eq!(row(&mut app), 3);
}

/// `alt+space` ticks a note, and the file says so.
#[test]
fn alt_space_ticks_a_note() {
    let scratch = tree("tick", THREE);
    let mut app = open(&scratch, 76, 18);
    app.handle(alt(KeyCode::Char(' ')));

    let written = std::fs::read_to_string(scratch.path().join(".obelus").join("todo.toml"))
        .expect("the notes");
    let table = written.parse::<toml::Table>().expect("it parses");
    let notes = table["todo"].as_array().expect("the notes");
    assert_eq!(notes[0]["done"].as_bool(), Some(true), "{written}");
    assert_eq!(notes[1]["done"].as_bool(), Some(false));
}

/// `alt+backspace` takes the whole note away -- backspace on its own is a
/// character here.
#[test]
fn alt_backspace_takes_a_note_away() {
    let scratch = tree("drop", THREE);
    let mut app = open(&scratch, 76, 18);
    app.handle(alt(KeyCode::Backspace));

    let written = std::fs::read_to_string(scratch.path().join(".obelus").join("todo.toml"))
        .expect("the notes");
    assert!(!written.contains("wire the counts tree"), "{written}");
    assert!(
        written.contains("settings live in a directory"),
        "{written}"
    );
}

/// `alt+enter` goes where a note points, and the view gets out of the way.
#[test]
fn alt_enter_goes_to_what_a_note_is_about() {
    let scratch = tree("go", THREE);
    std::fs::write(scratch.path().join("sample.rs"), "one\ntwo\nthree\n").expect("the file");

    let mut app = open(&scratch, 76, 18);
    press(&mut app, KeyCode::Down);
    app.handle(alt(KeyCode::Enter));

    assert!(app.notes().is_none(), "the view stayed over the file");
    let buffer = app.current_buffer().expect("nothing was opened");
    assert!(
        buffer.path().ends_with("sample.rs"),
        "{}",
        buffer.path().display()
    );
    assert_eq!(buffer.cursor().line.get(), 1, "not the line it is about");
}

/// A note about the project has nowhere to go, and nothing is the answer.
#[test]
fn alt_enter_on_a_note_about_nothing_goes_nowhere() {
    let scratch = tree("nowhere", THREE);
    let mut app = open(&scratch, 76, 18);
    app.handle(alt(KeyCode::Enter));
    assert!(app.notes().is_some(), "it went somewhere");
}

/// Where a note sits is the reader's to decide.
#[test]
fn alt_and_an_arrow_moves_a_note() {
    let scratch = tree("move", THREE);
    let mut app = open(&scratch, 76, 18);
    let heads = |app: &App| -> Vec<String> {
        app.notes()
            .expect("the view")
            .rows()
            .iter()
            .filter(|row| row.head)
            .map(|row| row.said.clone())
            .collect()
    };
    let first = heads(&app)[0].clone();

    app.handle(alt(KeyCode::Down));
    assert_eq!(heads(&app)[1], first, "it did not move down");
    app.handle(alt(KeyCode::Down));
    assert_eq!(heads(&app)[2], first, "the caret did not follow it");
    app.handle(alt(KeyCode::Up));
    assert_eq!(heads(&app)[1], first);

    let written = std::fs::read_to_string(scratch.path().join(".obelus").join("todo.toml"))
        .expect("the notes");
    let table = written.parse::<toml::Table>().expect("it parses");
    assert_eq!(
        table["todo"].as_array().expect("the notes")[1]["said"].as_str(),
        Some(first.as_str()),
        "the order was not written down: {written}"
    );
}

/// A note that says nothing is not a note, and leaving one is how it goes.
#[test]
fn a_note_with_nothing_in_it_is_dropped() {
    let scratch = tree("empty", THREE);
    let mut app = open(&scratch, 76, 18);
    let notes = |app: &App| {
        app.notes()
            .expect("the view")
            .rows()
            .iter()
            .filter(|row| row.head)
            .count()
    };
    let before = notes(&app);

    // Started and walked away from.
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Down);
    assert_eq!(notes(&app), before, "an empty note stayed");

    // Started, typed blanks into, and left.
    press(&mut app, KeyCode::Enter);
    support::type_text(&mut app, "   ");
    press(&mut app, KeyCode::Esc);
    let written = std::fs::read_to_string(scratch.path().join(".obelus").join("todo.toml"))
        .expect("the notes");
    let table = written.parse::<toml::Table>().expect("it parses");
    assert_eq!(
        table["todo"].as_array().expect("the notes").len(),
        before,
        "a note of blanks was kept: {written}"
    );
}

/// The foot says only what can be pressed, and `f1` says all of it.
#[test]
fn the_foot_drops_a_key_that_would_do_nothing() {
    let scratch = tree("usable", THREE);
    let mut app = open(&scratch, 76, 18);

    // The first note is about the project, so there is nowhere to go.
    let text = support::text_block(&support::render(&mut app, 76, 18)).to_string();
    assert!(!text.contains("go there"), "{text}");
    assert!(text.contains("another") && text.contains("leave"), "{text}");

    press(&mut app, KeyCode::Down);
    let text = support::text_block(&support::render(&mut app, 76, 18)).to_string();
    assert!(
        text.contains("go there"),
        "the key that works is missing:\n{text}"
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
    for word in ["move it up or down", "take the whole note away"] {
        assert!(text.contains(word), "{word:?} is not on the card:\n{dump}");
    }

    press(&mut app, KeyCode::Esc);
    assert!(app.notes().is_some(), "escape left the view, not the card");
    assert!(
        !support::text_block(&support::render(&mut app, 76, 18)).contains("the keys here"),
        "the card stayed"
    );
}

/// `alt+t` from a line of code opens the notes with one started against it.
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
    // Straight into it: no key between asking and typing.
    support::type_text(&mut app, "look at this again");
    press(&mut app, KeyCode::Esc);

    let written = std::fs::read_to_string(scratch.path().join(".obelus").join("todo.toml"))
        .expect("the notes");
    let table = written.parse::<toml::Table>().expect("it parses");
    let note = &table["todo"].as_array().expect("the notes")[0];
    assert_eq!(note["said"].as_str(), Some("look at this again"));
    assert_eq!(note["at"].as_str(), Some("thing.rs"));
    assert_eq!(note["line"].as_integer(), Some(3), "{written}");
}

/// A tree with nothing to come back to says so.
#[test]
fn a_tree_with_no_notes_says_so() {
    let scratch = support::Scratch::new("todo-none");
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, 76, 18);
    dispatch::dispatch(&mut app, Command::TodoOpen);

    let dump = support::render(&mut app, 76, 18);
    assert!(
        support::text_block(&dump).contains("nothing to come back to"),
        "{dump}"
    );
    // And enter starts the first one.
    press(&mut app, KeyCode::Enter);
    support::type_text(&mut app, "the first");
    press(&mut app, KeyCode::Esc);
    let written = std::fs::read_to_string(scratch.path().join(".obelus").join("todo.toml"))
        .expect("the notes");
    assert!(written.contains("the first"), "{written}");
}

/// The caret sits on the cell the next letter goes in.
///
/// Broken deliberately by giving the drawing and the caret their own idea of
/// where a note's text starts: the caret sat one cell right of the letter it
/// was about to put down, which is a caret lying about the only thing it
/// says.
#[test]
fn the_caret_is_on_the_cell_the_letter_goes_in() {
    let scratch = tree("align", THREE);
    let mut app = open(&scratch, 76, 14);
    let column = |app: &mut App| {
        let dump = support::render(app, 76, 14);
        support::cursor_line(&dump)
            .split_once(',')
            .and_then(|(x, _)| x.trim().parse::<usize>().ok())
            .unwrap_or(0)
    };
    // Where the note's own text begins on its row.
    let dump = support::render(&mut app, 76, 14);
    let row = support::text_block(&dump)
        .lines()
        .find(|row| row.contains("wire the counts"))
        .unwrap_or_default();
    let cells = row.split_once('|').map_or(row, |(_, rest)| rest);
    // Characters, not bytes: the box in front of a note is three bytes and
    // one cell, and a caret is counted in cells.
    let starts = cells
        .find("wire the counts")
        .map_or(0, |byte| cells[..byte].chars().count());

    assert_eq!(
        column(&mut app),
        starts,
        "the caret is not on the first letter"
    );
    press(&mut app, KeyCode::Right);
    press(&mut app, KeyCode::Right);
    press(&mut app, KeyCode::Right);
    assert_eq!(
        column(&mut app),
        starts + 3,
        "it did not move with the caret"
    );
}

/// Taking a whole note away is at the foot, not only on the card: backspace
/// on its own is a letter here, so it is the one thing a reader will go
/// looking for and not find.
#[test]
fn dropping_a_note_is_at_the_foot() {
    let scratch = tree("drop-foot", THREE);
    let mut app = open(&scratch, 76, 14);
    let text = support::text_block(&support::render(&mut app, 76, 14)).to_string();
    assert!(text.contains("drop"), "the foot does not say how:\n{text}");

    // And the card says it at length, which is what a card is for.
    press(&mut app, KeyCode::F(1));
    let dump = support::render(&mut app, 76, 16);
    assert!(
        support::text_block(&dump).contains("take the whole note away"),
        "the card only has the foot's word for it:\n{dump}"
    );
}

/// A note too long for the row wraps, where the reader asked for wrapping.
///
/// The same answer the file behind this view gives, because it is the same
/// question -- and this page is prose being written, where a line running
/// off the edge takes the caret with it.
#[test]
fn a_long_note_wraps_when_the_reader_wraps() {
    let long = "a note long enough that it will not sit on one row of a narrow \
                terminal, and so has to go somewhere";
    let scratch = support::Scratch::new("todo-wrap");
    std::fs::create_dir_all(scratch.path().join(".obelus")).expect("the directory");
    std::fs::write(
        scratch.path().join(".obelus").join("todo.toml"),
        format!("[[todo]]\nsaid = \"{long}\"\ndone = false\n"),
    )
    .expect("the notes");

    let showing = |wrap: bool| {
        let mut app = App::new(vec![support::open_fixture("sample.rs")]);
        app.configure(
            obelus::config::Config {
                wrap,
                ..obelus::config::Config::default()
            },
            Vec::new(),
        );
        app.working_directory_for_test(scratch.path().to_path_buf());
        support::lay_out(&mut app, 56, 12);
        dispatch::dispatch(&mut app, Command::TodoOpen);
        support::text_block(&support::render(&mut app, 56, 12))
            .lines()
            .filter(|row| !row.is_empty())
            .take(3)
            .map(str::to_string)
            .collect::<Vec<_>>()
    };

    let wrapped = showing(true);
    assert!(
        wrapped[1].contains("of a narrow terminal"),
        "it did not wrap:\n{}",
        wrapped.join("\n")
    );
    // The continuation starts under the first row's words rather than under
    // the box, so a note reads as one thing. Counted in characters: the box
    // in front of a note is three bytes and one cell.
    let column = |row: &str, needle: &str| {
        row.find(needle)
            .map(|byte| row[..byte].chars().count())
            .unwrap_or_else(|| panic!("no {needle:?} in {row:?}"))
    };
    assert_eq!(
        column(&wrapped[1], "of a narrow"),
        column(&wrapped[0], "a note"),
        "the second row is not under the first"
    );

    // And where the reader does not wrap, one row and the mark that says
    // there is more of it.
    let cut = showing(false);
    assert!(
        !cut[1].contains("of a narrow"),
        "it wrapped anyway:\n{}",
        cut.join("\n")
    );
    assert!(
        cut[0].contains('\u{2026}'),
        "nothing says it was cut:\n{}",
        cut[0]
    );
}
