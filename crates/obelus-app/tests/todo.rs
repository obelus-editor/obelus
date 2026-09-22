//! What a tree means to come back to: the file, the view, and the keys.

mod support;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use obelus_app::{
    app::{App, dispatch},
    event::Event,
};
use obelus_command::Command;
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
    // Whichever pair is drawn -- the Nerd Font's where there is one, the
    // plain ones where there is not. What has to be true is that a note to
    // do and a note that is done are not the same box.
    let boxes: Vec<char> = text
        .lines()
        .filter_map(|row| row.split_once('|').map(|(_, rest)| rest))
        .filter_map(|row| row.chars().nth(1))
        .filter(|glyph| !glyph.is_whitespace())
        .collect();
    assert!(boxes.len() >= 3, "not a box per note:\n{dump}");
    assert_eq!(boxes[0], boxes[1], "two notes to do are drawn differently");
    assert_ne!(
        boxes[0], boxes[2],
        "a note that is done is drawn like one that is not:\n{dump}"
    );
    // On a row of its own, under what the note says.
    let rows: Vec<&str> = text.lines().collect();
    let at = |needle: &str| {
        rows.iter()
            .position(|row| row.contains(needle))
            .unwrap_or_else(|| panic!("no {needle:?}:\n{dump}"))
    };
    assert_eq!(
        at("sample.rs:2"),
        at("changes the colours") + 1,
        "the place is not under the note it belongs to:\n{dump}"
    );
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

    // And escape keeps it: there is no moment the reader says "done with
    // this note", so every way out of it is one. What escape does *not* do
    // any more is close anything -- the notes are a document, and escape
    // leaves whatever is over the document being read.
    press(&mut app, KeyCode::Esc);
    assert!(
        app.notes().is_some(),
        "escape closed a document, which is not what escape is for"
    );
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
    // Into the second note, down its own three lines, and then over the row
    // saying where it points -- that is a fact about the note rather than a
    // line of it, so there is nowhere on it for a caret to be.
    for expected in [1, 2, 3, 5] {
        press(&mut app, KeyCode::Down);
        assert_eq!(row(&mut app), expected, "the caret did not walk down");
    }
    press(&mut app, KeyCode::Up);
    assert_eq!(row(&mut app), 3);
}

/// What the reader has hold of is marked, and `ctrl+c` takes a copy of it.
///
/// Both in one test because they are one thing: a selection nobody can see
/// is a selection nobody makes, and one that cannot be copied out is a
/// selection with nothing to do.
#[test]
fn what_is_held_in_a_note_is_marked_and_copied() {
    use obelus_theme::builtin::DARK;

    let _turn = support::clipboard_turn();
    obelus_clipboard::use_provider_for_test(obelus_clipboard::Provider::Kept);

    let scratch = tree("held", THREE);
    let mut app = open(&scratch, 76, 18);
    // The caret opens at the start of the first note, so this takes "wire".
    for _ in 0..4 {
        support::press_shift(&mut app, KeyCode::Right);
    }

    let dump = support::render(&mut app, 76, 18);
    let held = format!("bg={}", support::spelled(DARK.selection_background));
    assert!(
        support::legend_for(&dump, 'w').ends_with(&held),
        "the first letter of what is held is not marked:\n{dump}"
    );
    // The first `t` on the page is the one in "the", a character past the
    // end of what is held.
    assert!(
        !support::legend_for(&dump, 't').ends_with(&held),
        "the mark ran past what is held:\n{dump}"
    );

    support::press_control_key(&mut app, KeyCode::Char('c'));
    assert_eq!(
        obelus_clipboard::paste().as_deref(),
        Some("wire"),
        "what was held did not reach the clipboard"
    );

    // And with nothing held, the whole note -- the way the file copies the
    // whole line rather than nothing at all.
    press(&mut app, KeyCode::Right);
    support::press_control_key(&mut app, KeyCode::Char('c'));
    assert_eq!(
        obelus_clipboard::paste().as_deref(),
        Some("wire the counts tree up to the search"),
    );
}

/// The other three keys a reader brings with them: all of it, out, and back.
#[test]
fn a_note_takes_the_clipboard_keys_too() {
    let _turn = support::clipboard_turn();
    obelus_clipboard::use_provider_for_test(obelus_clipboard::Provider::Kept);

    let scratch = tree("clipboard", THREE);
    let mut app = open(&scratch, 76, 18);

    // All of the first note, and then out of it. The note stays -- it is
    // the text that was taken, not the row -- and it says nothing.
    support::press_control_key(&mut app, KeyCode::Char('a'));
    support::press_control_key(&mut app, KeyCode::Char('x'));
    let dump = support::render(&mut app, 76, 18);
    assert!(
        !support::text_block(&dump).contains("wire the counts"),
        "what was cut is still on the page:\n{dump}"
    );
    assert_eq!(
        obelus_clipboard::paste().as_deref(),
        Some("wire the counts tree up to the search")
    );

    // And back in, where the caret was left.
    support::press_control_key(&mut app, KeyCode::Char('v'));
    let dump = support::render(&mut app, 76, 18);
    assert!(
        support::text_block(&dump).contains("wire the counts tree up to the search"),
        "the paste did not put it back:\n{dump}"
    );
}

/// `ctrl+x` with nothing held takes the whole note, the way copy takes it.
#[test]
fn cutting_with_nothing_held_takes_the_note() {
    let _turn = support::clipboard_turn();
    obelus_clipboard::use_provider_for_test(obelus_clipboard::Provider::Kept);

    let scratch = tree("cut-note", THREE);
    let mut app = open(&scratch, 76, 18);
    support::press_control_key(&mut app, KeyCode::Char('x'));

    assert_eq!(
        obelus_clipboard::paste().as_deref(),
        Some("wire the counts tree up to the search")
    );
    let written = std::fs::read_to_string(scratch.path().join(".obelus").join("todo.toml"))
        .expect("the notes");
    assert!(
        !written.contains("wire the counts"),
        "the cut note was not written away: {written}"
    );
}

/// What the terminal pastes lands in the note, not in the file.
///
/// Broken deliberately by sending every paste straight to the buffer: the
/// notes are a whole screen of their own, and the reader's text went into a
/// file they could not see.
///
/// The file is asked for by its place in the list rather than through
/// `current_buffer`, because the notes *are* what is being read now: a
/// document that is not a file answers `None` to everything that wants
/// one, which is the whole point of the type.
#[test]
fn what_the_terminal_pastes_lands_in_the_note() {
    let scratch = tree("bracketed", THREE);
    let mut app = open(&scratch, 76, 18);
    let file = obelus_buffer::DocumentId::new(0);
    let was = app
        .file(file)
        .expect("the file it was opened on")
        .text()
        .rope()
        .to_string();

    app.handle(Event::Paste(" and the blame".to_string()));

    let dump = support::render(&mut app, 76, 18);
    assert!(
        support::text_block(&dump).contains(" and the blamewire the counts"),
        "the paste did not reach the note:\n{dump}"
    );
    assert_eq!(
        app.file(file).expect("the file").text().rope(),
        &was,
        "the paste went into the file instead of the note"
    );
}

/// A note is what it says, without the blank line a trailing newline leaves.
///
/// Broken deliberately by laying the notes out through the text they are
/// really held in: a note written with TOML's multi-line quotes ends in a
/// break, and the page grew a row nobody had typed under it.
#[test]
fn a_note_does_not_end_in_a_blank_row() {
    let scratch = tree("blank-end", THREE);
    let mut app = open(&scratch, 76, 18);
    let dump = support::render(&mut app, 76, 18);
    let rows: Vec<&str> = support::text_block(&dump).lines().collect();
    let at = |needle: &str| {
        rows.iter()
            .position(|row| row.contains(needle))
            .unwrap_or_else(|| panic!("no {needle:?}:\n{dump}"))
    };
    assert_eq!(
        at("sample.rs:2"),
        at("changes the colours") + 1,
        "a blank row came between the note and where it points:\n{dump}"
    );
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
    assert!(!text.contains("Go there"), "{text}");
    assert!(text.contains("Another"), "{text}");

    press(&mut app, KeyCode::Down);
    let text = support::text_block(&support::render(&mut app, 76, 18)).to_string();
    assert!(
        text.contains("Go there"),
        "the key that works is missing:\n{text}"
    );
}

/// A foot that ran out of room says so, rather than looking complete.
///
/// The notes answer to eight keys. On a narrow terminal the row can hold
/// three of them, and what it used to do with the rest was stop -- leaving
/// a foot indistinguishable from one that had said everything it had. A
/// reader was told this view answers to three keys.
///
/// Here the mark is the whole of the trace, because a document's foot has
/// no card behind it to point at: it says the terminal is too narrow for
/// all of this, and promises nothing else.
///
/// Broken deliberately by returning from `ui::row_of_keys` without calling
/// `cut`: the narrow row looks exactly like the wide one and this goes red.
#[test]
fn a_foot_that_ran_out_of_room_says_so() {
    let scratch = tree("cut", THREE);
    let mut app = open(&scratch, 76, 18);

    let foot = |app: &mut App, width: u16| -> String {
        let dump = support::render(app, width, 18);
        let text = support::text_block(&dump);
        let rows: Vec<&str> = text.lines().collect();
        rows[rows.len() - 3].to_string()
    };

    let whole = foot(&mut app, 76);
    assert!(
        whole.contains("Move"),
        "the wide row is not the whole row:\n{whole}"
    );
    assert!(
        !whole.contains('\u{2026}'),
        "a row with room for everything is marked as cut:\n{whole}"
    );

    let narrow = foot(&mut app, 40);
    assert!(
        !narrow.contains("Move"),
        "nothing was dropped, so this proves nothing:\n{narrow}"
    );
    assert!(
        narrow.contains('\u{2026}'),
        "the row was cut and did not say so:\n{narrow}"
    );
}

/// The foot points at no card, and takes back the room the pointer had.
///
/// Every other view with a foot ends it with `f1 keys`, because every other
/// view with a foot is a layer and has a card behind it. Pointing at one
/// from here would be a key that appears to do nothing -- and would cost
/// the row the eight cells the pointer sits in, which is a hint.
///
/// Broken deliberately by calling `ui::foot` from `ui::todo` instead of
/// `ui::foot_without_a_card`: the pointer comes back and this goes red.
#[test]
fn the_foot_of_the_notes_points_at_no_card() {
    let scratch = tree("no-card", THREE);
    let mut app = open(&scratch, 76, 18);
    let text = support::text_block(&support::render(&mut app, 76, 18)).to_string();
    let foot = text
        .lines()
        .find(|row| row.contains("Another"))
        .unwrap_or_else(|| panic!("no foot at all:\n{text}"));
    assert!(
        !foot.contains("Keys"),
        "the foot still points at a card:\n{foot}"
    );
}

/// `f1` over the notes opens a file, the way it does from anywhere else.
///
/// It used to put up a card of every key here. A card is a layer's: a thing
/// opened over the reader's work, which owns the keyboard while it is up.
/// The notes are a document -- the reader is *in* them -- and a document
/// that swallows `f1` leaves them with no way to open a file without
/// leaving first.
///
/// Broken deliberately by giving `component::todo` an `F(1)` arm again: the
/// picker never opens and this goes red.
#[test]
fn f1_over_the_notes_opens_a_file() {
    let scratch = tree("keys", THREE);
    let mut app = open(&scratch, 76, 18);
    assert!(app.picker().is_none(), "something was already open");

    support::press_function(&mut app, 1);
    assert!(
        app.picker().is_some(),
        "f1 did not open the file picker from the notes"
    );

    // And nothing that reads like a card of keys went up in its place.
    let dump = support::render(&mut app, 76, 18);
    assert!(
        !support::text_block(&dump).contains("The keys here"),
        "the card is still there:\n{dump}"
    );
}

/// `alt+t` from a line of code opens the notes with one started against it.
#[test]
fn a_note_made_while_reading_carries_the_line() {
    let scratch = support::Scratch::new("todo-made");
    let file = scratch.path().join("thing.rs");
    std::fs::write(&file, "one\ntwo\nthree\nfour\n").expect("the file");

    let mut app = App::new(vec![
        obelus_buffer::Buffer::open(&file).expect("opening it"),
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
        support::text_block(&dump).contains("Nothing to come back to"),
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
    let starts = support::column_of(row, "wire the counts");

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

/// Taking a whole note away is at the foot: backspace on its own is a
/// letter here, so it is the one thing a reader will go looking for and not
/// find. There is nowhere else it could be said -- this view has no card.
#[test]
fn dropping_a_note_is_at_the_foot() {
    let scratch = tree("drop-foot", THREE);
    let mut app = open(&scratch, 76, 14);
    let text = support::text_block(&support::render(&mut app, 76, 14)).to_string();
    assert!(text.contains("Drop"), "the foot does not say how:\n{text}");
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
            obelus_config::Config {
                wrap,
                ..obelus_config::Config::default()
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
        wrapped[1].contains("narrow terminal"),
        "it did not wrap:\n{}",
        wrapped.join("\n")
    );
    // The continuation starts under the first row's words rather than under
    // the box, so a note reads as one thing.
    //
    // Measured as where each row's words begin rather than by naming a
    // word of them: which words land on the second row is a fact about how
    // wide the column is, and this is not a test about that.
    let words_at = |row: &str| {
        row.chars()
            .position(char::is_alphanumeric)
            .unwrap_or_else(|| panic!("no words on {row:?}"))
    };
    assert_eq!(
        words_at(&wrapped[1]),
        words_at(&wrapped[0]),
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

/// Where a note points is a row of its own, so what it says is laid out the
/// same whether it points anywhere or not.
///
/// Broken deliberately by hanging the place off the end of the first line:
/// a note long enough was wrapped to the whole width, laid out under the
/// place, and then drawn over by it.
#[test]
fn a_place_is_a_row_of_its_own() {
    let long = "this cache does not notice a theme change, and the rows it keeps go \
                on saying what they said";
    let scratch = support::Scratch::new("todo-place");
    std::fs::create_dir_all(scratch.path().join(".obelus")).expect("the directory");
    std::fs::write(
        scratch.path().join(".obelus").join("todo.toml"),
        format!(
            "[[todo]]\nsaid = \"{long}\"\ndone = false\nat = \"src/ui/picker.rs\"\nline = 412\n\
             \n[[todo]]\nsaid = \"{long}\"\ndone = false\n"
        ),
    )
    .expect("the notes");

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.configure(
        obelus_config::Config {
            wrap: true,
            ..obelus_config::Config::default()
        },
        Vec::new(),
    );
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, 70, 14);
    dispatch::dispatch(&mut app, Command::TodoOpen);

    let dump = support::render(&mut app, 70, 14);
    let rows: Vec<&str> = support::text_block(&dump)
        .lines()
        .filter(|row| !row.is_empty())
        .collect();
    // The place is on its own, under the note, and nothing of the note is on
    // that row.
    let place = rows
        .iter()
        .position(|row| row.contains("picker.rs:412"))
        .unwrap_or_else(|| panic!("no place:\n{dump}"));
    assert!(
        !rows[place].contains("cache") && !rows[place].contains("said"),
        "the place shares a row with the note:\n{dump}"
    );

    // And the two notes say the same thing, so they take the same rows to
    // say it: one of them pointing somewhere changes nothing about how what
    // it says is laid out.
    let said_rows = |note: usize| {
        app.notes()
            .expect("the view")
            .rows()
            .iter()
            .filter(|row| row.note == note && !row.place)
            .count()
    };
    assert_eq!(
        said_rows(0),
        said_rows(1),
        "the note that points somewhere was laid out differently:\n{dump}"
    );
}

/// The keys a reader learnt in a file work in a note, because a note is the
/// same thing with less attached to it.
///
/// Broken deliberately by giving the box its own stepping over its own
/// `Vec<String>`: `ctrl+left` walked a word in the file and did nothing
/// here, and every key added to one side was a key the other did not have.
#[test]
fn the_words_keys_work_in_a_note_too() {
    let scratch = tree("words", THREE);
    let mut app = open(&scratch, 60, 10);
    let ctrl = |code| Event::Key(KeyEvent::new(code, KeyModifiers::CONTROL));
    let column = |app: &mut App| {
        support::cursor_line(&support::render(app, 60, 10))
            .split_once(',')
            .and_then(|(x, _)| x.trim().parse::<usize>().ok())
            .unwrap_or(0)
    };

    // "wire the counts tree up to the search", from its first letter.
    let start = column(&mut app);
    app.handle(ctrl(KeyCode::Right));
    let word = column(&mut app);
    assert!(word > start + 1, "ctrl+right did not walk a word");
    app.handle(ctrl(KeyCode::Right));
    assert!(column(&mut app) > word, "it stopped after one");
    app.handle(ctrl(KeyCode::Left));
    assert!(
        column(&mut app) < word + 2,
        "ctrl+left did not come back a word"
    );

    // And a word at a time out, which is the pair of the same rule.
    app.handle(ctrl(KeyCode::Delete));
    let said = app.notes().expect("the view").rows()[0].said.clone();
    assert!(
        !said.contains("the counts"),
        "ctrl+delete did not take a word out: {said:?}"
    );
    assert!(
        said.starts_with("wire"),
        "it took out more than a word: {said:?}"
    );
}

/// A note somebody else added while the list was open is not written over.
///
/// obelus writes the whole file from what it holds, so the window that
/// matters is exactly this: the list is open, another obelus -- or the
/// reader's own editor -- writes the file, and then something here saves.
/// Without hearing about the change, the save puts the file back the way it
/// was and the other note is gone.
///
/// Driven through `Event::FileChanged`, which is what the watcher sends: a
/// real watch would take a moment to notice and a test should not be about
/// how long.
#[test]
fn a_note_added_from_outside_survives_the_next_save() {
    let scratch = tree("outside", "[[todo]]\nsaid = \"the first\"\ndone = false\n");
    let mut app = open(&scratch, 76, 24);
    let file = scratch.path().join(".obelus").join("todo.toml");

    // Somebody else, with the list open. Their file keeps obelus's note --
    // they read it before writing, as obelus would -- and adds one.
    let theirs = std::fs::read_to_string(&file).expect("the notes");
    std::fs::write(
        &file,
        format!("{theirs}\n[[todo]]\nsaid = \"theirs\"\ndone = false\n"),
    )
    .expect("their write");
    app.handle(Event::Watched(obelus_watch::Changed { path: file.clone() }));

    // And now the reader does something that saves: leaving does.
    press(&mut app, KeyCode::Esc);

    let after = std::fs::read_to_string(&file).expect("the notes");
    assert!(
        after.contains("theirs"),
        "the note added from outside was written over:\n{after}"
    );
    assert!(after.contains("the first"), "obelus lost its own:\n{after}");
}

/// And what the reader is part-way through typing survives it too.
///
/// The re-read swaps the notes underneath them, so the two things that are
/// theirs -- the note the caret is in and the words in the box -- are put
/// back by name. Losing a half-written note to somebody else's save would
/// be a worse bug than the one the watch is here to fix.
#[test]
fn a_note_being_written_survives_someone_else_saving() {
    let scratch = tree("writing", "[[todo]]\nsaid = \"the first\"\ndone = false\n");
    let mut app = open(&scratch, 76, 24);
    let file = scratch.path().join(".obelus").join("todo.toml");

    // The caret opens at the very start of the first note, so this goes in
    // front of what is there. What matters is that it is still there after.
    support::type_text(&mut app, "half-written ");

    let theirs = std::fs::read_to_string(&file).expect("the notes");
    std::fs::write(
        &file,
        format!("{theirs}\n[[todo]]\nsaid = \"theirs\"\ndone = false\n"),
    )
    .expect("their write");
    app.handle(Event::Watched(obelus_watch::Changed { path: file.clone() }));

    press(&mut app, KeyCode::Esc);

    let after = std::fs::read_to_string(&file).expect("the notes");
    assert!(
        after.contains("half-written the first"),
        "what was being typed was lost:\n{after}"
    );
    assert!(after.contains("theirs"), "their note was lost:\n{after}");
}

/// Talking about a note opens a conversation of that note's own.
///
/// Keyed by the note's name rather than its place: the list is read from
/// the file every time it opens, so a note added above would otherwise hand
/// the reader somebody else's conversation.
#[test]
fn each_note_gets_a_conversation_of_its_own() {
    let scratch = tree(
        "own",
        "[[todo]]\nsaid = \"the first\"\n\n[[todo]]\nsaid = \"the second\"\n",
    );
    let mut app = open(&scratch, 76, 24);

    support::press_alt(&mut app, 'a');
    let first = app.current_document_for_test().expect("a document");
    assert!(app.chat().is_some(), "no conversation about the first note");

    // Back to the list, which lands on the note this one came out of -- and
    // then on to the other note.
    support::press_alt(&mut app, 't');
    assert!(app.notes().is_some(), "alt+t did not bring the notes back");
    support::press(&mut app, KeyCode::Down);
    support::press_alt(&mut app, 'a');
    let second = app.current_document_for_test().expect("a document");

    assert_ne!(
        first, second,
        "both notes were given one conversation between them"
    );

    // And asking for the first one again comes back to the first, rather
    // than starting a third.
    support::press_alt(&mut app, 't');
    support::press(&mut app, KeyCode::Up);
    support::press_alt(&mut app, 'a');
    assert_eq!(
        app.current_document_for_test(),
        Some(first),
        "asking about a note twice made two conversations"
    );
}

/// The conversation says which note it is about.
///
/// A header earns its row by carrying something. A reader with four
/// conversations open has four screens that differ only in what was said in
/// them, and the agent's name is the same on all four -- so what tells them
/// apart is the note, in the words the reader wrote.
#[test]
fn a_conversation_says_which_note_it_is_about() {
    let scratch = tree("header", "[[todo]]\nsaid = \"wire the counts tree up\"\n");
    let mut app = open(&scratch, 76, 24);
    support::press_alt(&mut app, 'a');

    let screen = support::text_block(&support::render(&mut app, 76, 24)).to_string();
    assert!(
        screen.contains("wire the counts tree up"),
        "the conversation does not say what it is about:\n{screen}"
    );
}

/// And says how to get back to it.
///
/// The key was added because the round trip had an outward leg and no
/// return. A return leg nothing says exists is the same gap one level up:
/// the notes page names `alt+a` in its own hints, so the conversation names
/// the key that comes back.
#[test]
fn a_conversation_says_how_to_get_back_to_its_note() {
    let scratch = tree("way-back", "[[todo]]\nsaid = \"wire the counts tree up\"\n");
    let mut app = open(&scratch, 76, 24);
    support::press_alt(&mut app, 'a');

    let screen = support::text_block(&support::render(&mut app, 76, 24)).to_string();
    assert!(
        screen.contains("the note"),
        "the conversation does not say how to get back:\n{screen}"
    );
}

/// A tree of notes, already indented.
const NESTED: &str = r#"
[[todo]]
said = "the counts tree"
done = false
depth = 0

[[todo]]
said = "walk it once"
done = false
depth = 1

[[todo]]
said = "and cache the walk"
done = false
depth = 2

[[todo]]
said = "then draw it"
done = false
depth = 1

[[todo]]
said = "the settings page"
done = false
depth = 0
"#;

/// The depths of the notes as the file has them, after the view wrote back.
fn depths(scratch: &support::Scratch) -> Vec<u16> {
    obelus_git::todo::Todo::read(scratch.path())
        .notes
        .iter()
        .map(|note| note.depth)
        .collect()
}

/// What each note says, in the order the file has them.
fn titles(scratch: &support::Scratch) -> Vec<String> {
    obelus_git::todo::Todo::read(scratch.path())
        .notes
        .iter()
        .map(|note| note.title().to_string())
        .collect()
}

/// Tab takes a note one level in, and only one.
///
/// The first note has nothing above it to hang under, so it will not go in
/// at all; and a note cannot step past the one above it, however deep the
/// note happens to be that comes immediately before it on the page.
#[test]
fn tab_takes_a_note_one_level_in_and_no_further() {
    let scratch = tree("indent", NESTED);
    let mut app = open(&scratch, 76, 20);

    // The first note: nothing above it, so nothing happens.
    press(&mut app, KeyCode::Tab);
    assert_eq!(
        depths(&scratch),
        vec![0, 1, 2, 1, 0],
        "the first note moved"
    );

    // The last note, whose neighbour above is two levels deep: one level in
    // is one level, not two.
    for _ in 0..4 {
        press(&mut app, KeyCode::Down);
    }
    press(&mut app, KeyCode::Tab);
    assert_eq!(depths(&scratch), vec![0, 1, 2, 1, 1]);
}

/// And what hangs under it comes with it, in or out.
///
/// Broken deliberately by shifting only the note the selection is on: the
/// child stayed where it was and became the child of whatever the note used
/// to hang under.
///
/// The two steps do not undo each other, and that is the right answer rather
/// than a fault in either: stepping "walk it once" out to the top puts "then
/// draw it" -- which was its sibling and is still a level deeper than it --
/// underneath it, so the second step has three notes to carry rather than
/// two. What hangs under a note is read off the list, and stepping out
/// changed the list.
#[test]
fn a_note_takes_what_hangs_under_it_in_and_out_with_it() {
    let scratch = tree("shift", NESTED);
    let mut app = open(&scratch, 76, 20);

    // Onto "walk it once", which has one note under it.
    press(&mut app, KeyCode::Down);
    app.handle(Event::Key(KeyEvent::new(
        KeyCode::BackTab,
        KeyModifiers::SHIFT,
    )));
    assert_eq!(
        depths(&scratch),
        vec![0, 0, 1, 1, 0],
        "the child stayed behind"
    );

    press(&mut app, KeyCode::Tab);
    assert_eq!(
        depths(&scratch),
        vec![0, 1, 2, 2, 0],
        "what hangs under it now did not come with it"
    );
}

/// Enter starts the next note after the whole of what hangs under this one.
///
/// At its depth, so it is the next thing at that level. Between a parent and
/// its children it would have been adopted without the reader asking.
#[test]
fn a_new_note_goes_after_the_children_and_not_among_them() {
    let scratch = tree("after", NESTED);
    let mut app = open(&scratch, 76, 20);

    // Onto "walk it once", which has "and cache the walk" under it.
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    support::type_text(&mut app, "a fresh one");
    press(&mut app, KeyCode::Esc);

    assert_eq!(
        titles(&scratch),
        vec![
            "the counts tree",
            "walk it once",
            "and cache the walk",
            "a fresh one",
            "then draw it",
            "the settings page",
        ]
    );
    assert_eq!(depths(&scratch), vec![0, 1, 2, 1, 1, 0]);
}

/// Moving one steps over the neighbour at its own level, children and all.
///
/// Broken deliberately by swapping single notes: the parent stepped over
/// its own first child and left the rest of them behind it.
#[test]
fn moving_a_note_steps_over_the_whole_of_its_neighbour() {
    let scratch = tree("move-subtree", NESTED);
    let mut app = open(&scratch, 76, 20);

    // "the counts tree" down past "the settings page": it takes three notes
    // with it, and there is one note at its level to step over.
    app.handle(alt(KeyCode::Down));
    press(&mut app, KeyCode::Esc);
    assert_eq!(
        titles(&scratch),
        vec![
            "the settings page",
            "the counts tree",
            "walk it once",
            "and cache the walk",
            "then draw it",
        ]
    );
    assert_eq!(depths(&scratch), vec![0, 0, 1, 2, 1], "the shape changed");
}

/// The first of a parent's children has nobody above it at its own level.
#[test]
fn a_first_child_has_nowhere_up_to_go() {
    let scratch = tree("first-child", NESTED);
    let mut app = open(&scratch, 76, 20);

    press(&mut app, KeyCode::Down);
    app.handle(alt(KeyCode::Up));
    press(&mut app, KeyCode::Esc);
    assert_eq!(
        titles(&scratch)[0],
        "the counts tree",
        "the child climbed out of its parent"
    );
}

/// Taking a note away takes what hangs under it.
///
/// The key means "take this note away", and a note and its children are one
/// thing on the screen. Left behind, they would hang under whatever was
/// above -- which is not on the screen at the moment the key is pressed.
#[test]
fn taking_a_note_away_takes_its_children() {
    let scratch = tree("drop-subtree", NESTED);
    let mut app = open(&scratch, 76, 20);

    press(&mut app, KeyCode::Down);
    app.handle(alt(KeyCode::Backspace));
    press(&mut app, KeyCode::Esc);
    assert_eq!(
        titles(&scratch),
        vec!["the counts tree", "then draw it", "the settings page"]
    );
}

/// But emptying a note's text brings its children up rather than taking
/// them.
///
/// Two doors, because they are two different things being asked for. The key
/// says "take this note away"; emptying the text asks nothing about the
/// children at all -- the note goes because a note with nothing in it is not
/// a note.
#[test]
fn emptying_a_note_brings_its_children_up_a_level() {
    let scratch = tree("empty-parent", NESTED);
    let mut app = open(&scratch, 76, 20);

    // Onto "walk it once" and take every character of it out.
    press(&mut app, KeyCode::Down);
    app.handle(Event::Key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)));
    for _ in 0.."walk it once".len() {
        press(&mut app, KeyCode::Backspace);
    }
    press(&mut app, KeyCode::Esc);

    assert_eq!(
        titles(&scratch),
        vec![
            "the counts tree",
            "and cache the walk",
            "then draw it",
            "the settings page",
        ]
    );
    assert_eq!(depths(&scratch), vec![0, 1, 1, 0]);
}

/// A step in or out is offered only where there is one to take.
///
/// What the card reads to decide whether to draw the key dim, so a key it
/// shows lit is a key that moves something. Asked of the view rather than
/// read off the cells: the card draws every key this page has and greys the
/// ones that would do nothing, so what is on screen says nothing about
/// which of them those are.
///
/// The four answers, on four notes of one list. Going in needs a note above
/// at this note's own depth or deeper -- a note already hard against its
/// parent has nothing between them to go under.
#[test]
fn a_step_is_offered_only_where_there_is_one() {
    let scratch = tree("steps", NESTED);
    let mut app = open(&scratch, 76, 24);
    let can = |app: &App, outwards: bool| app.notes().expect("the view").can_shift(outwards);
    let down = |app: &mut App, by: usize| {
        for _ in 0..by {
            press(app, KeyCode::Down);
        }
    };

    // "the counts tree": nothing above it to go under, and nothing to come
    // out of.
    assert!(!can(&app, false), "the first note was offered a step in");
    assert!(!can(&app, true), "a note at the top was offered a step out");

    // "walk it once", hard against the note it hangs under: there is nothing
    // between them for it to go under instead.
    down(&mut app, 1);
    assert!(
        !can(&app, false),
        "a note was offered a step it cannot take"
    );
    assert!(can(&app, true));

    // "then draw it", whose neighbour above is a level deeper: it can go
    // under that one, and it can come out.
    down(&mut app, 2);
    assert!(can(&app, false));
    assert!(can(&app, true));

    // "the settings page": in, but nowhere further out.
    down(&mut app, 1);
    assert!(can(&app, false));
    assert!(!can(&app, true), "a note at the top was offered a step out");
}

/// A note that hangs under another starts further in, box and all.
///
/// Broken deliberately by indenting the words and leaving the box in a
/// column down the edge: the page read as one flat list of boxes with ragged
/// words beside it, which says nothing about what is under what.
#[test]
fn a_nested_note_is_drawn_further_in_than_the_one_it_hangs_under() {
    let scratch = tree("indented", NESTED);
    let mut app = open(&scratch, 76, 20);

    let dump = support::render(&mut app, 76, 20);
    let rows: Vec<&str> = support::text_block(&dump).lines().collect();
    let at = |needle: &str| {
        rows.iter()
            .find(|row| row.contains(needle))
            .unwrap_or_else(|| panic!("no {needle:?}:\n{dump}"))
    };
    // The row, less its first cell -- which is the mark beside the note
    // the keys are on. That column is outside the list at every depth and
    // is not indentation, whatever it happens to be drawn with.
    let listed = |needle: &str| {
        let row = at(needle);
        let (_, said) = row.split_once('|').expect("the row number");
        let after_the_mark = said
            .char_indices()
            .nth(1)
            .map_or(said.len(), |(index, _)| index);
        &said[after_the_mark..]
    };
    // Where the words start, counted from the row's own left edge.
    let starts = |needle: &str| {
        let said = listed(needle);
        said.len() - said.trim_start().len()
    };

    let top = starts("the counts tree");
    assert_eq!(
        starts("walk it once"),
        top + 2,
        "one level is not two cells"
    );
    assert_eq!(starts("and cache the walk"), top + 4);
    assert_eq!(starts("then draw it"), top + 2);
    assert_eq!(starts("the settings page"), top, "a top note was indented");

    // And the box with them: the mark is the note's own, not a column down
    // the edge.
    let boxes = |needle: &str| {
        let said = listed(needle);
        said.find(['\u{f0130}', '\u{f0131}', '[', ' '])
            .map(|_| said.len() - said.trim_start().len())
    };
    assert_eq!(boxes("walk it once"), Some(top + 2));
}

/// A deep note's words are wrapped to fit where they are drawn.
///
/// The room one level of nesting takes comes off the column every note wraps
/// in, and off every note's rather than off its own: one width is what the
/// rows, the caret and the wrapping all read, and three widths for three
/// depths would be three answers to disagree over. What it buys is this -- a
/// note laid out at the full width and drawn two cells in would have those
/// two cells cut off the end of every row of it.
///
/// Broken deliberately by wrapping at the full width anyway: the indented
/// note's rows ran past the edge and came back with their ends missing.
#[test]
fn a_deep_notes_words_are_wrapped_to_fit_where_they_are_drawn() {
    // Unbroken runs rather than sentences: they wrap hard against the edge,
    // every row exactly as wide as the column, which is where the cells an
    // indent takes are the difference between fitting and not. And two
    // different letters, or a row of the one note would be found on screen
    // in a row of the other and the test would prove nothing.
    let (top, under) = ("a".repeat(150), "b".repeat(150));
    let scratch = tree(
        "one-column",
        &format!(
            "[[todo]]\nsaid = \"{top}\"\ndone = false\ndepth = 0\n\n\
             [[todo]]\nsaid = \"{under}\"\ndone = false\ndepth = 1\n"
        ),
    );
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.configure(
        obelus_config::Config {
            wrap: true,
            ..obelus_config::Config::default()
        },
        Vec::new(),
    );
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, 56, 20);
    dispatch::dispatch(&mut app, Command::TodoOpen);

    let dump = support::render(&mut app, 56, 20);
    let said: Vec<String> = app
        .notes()
        .expect("the view")
        .rows()
        .iter()
        .map(|row| row.said.clone())
        .collect();
    assert!(said.len() >= 4, "the notes did not wrap:\n{dump}");

    // Every row the view laid out is on the screen whole. A row laid out
    // wider than where it is drawn comes back with its end cut off.
    let screen = support::text_block(&dump);
    for row in &said {
        assert!(
            screen.contains(row.trim_end()),
            "{row:?} was cut off at the edge:\n{dump}"
        );
    }

    // And both notes broke into rows of one width, which is the other half
    // of it: the indented one is not wrapped tighter than the note above.
    //
    // By the widest row of each rather than by every row being the same
    // width, which held only while the letters happened to divide evenly
    // into the column: the last row of a note is whatever is left over and
    // says nothing about where the note wraps.
    let widest = |letter: char| {
        said.iter()
            .filter(|row| row.starts_with(letter))
            .map(|row| row.chars().count())
            .max()
            .unwrap_or_else(|| panic!("no rows of {letter:?}:\n{dump}"))
    };
    assert_eq!(
        widest('a'),
        widest('b'),
        "the notes wrapped in different columns:\n{dump}"
    );
}

/// A note started and not yet typed into can still be put under the one
/// above it.
///
/// Starting one and stepping it in is the order a reader does it in: they
/// press enter, see where it landed, and move it. The step went through the
/// door that switches to another note, and that door puts away whatever was
/// being written -- which for a note nobody has typed into yet means taking
/// it away, because a note that says nothing is not a note. So the key
/// looked as though it did nothing, and had in fact dropped the note.
#[test]
fn a_new_note_can_be_stepped_in_before_it_says_anything() {
    let scratch = tree("new-then-tab", NESTED);
    let mut app = open(&scratch, 76, 20);

    // Onto "the settings page", the last note, and start another under it.
    for _ in 0..4 {
        press(&mut app, KeyCode::Down);
    }
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Tab);
    support::type_text(&mut app, "and its keys");
    press(&mut app, KeyCode::Esc);

    assert_eq!(
        titles(&scratch),
        vec![
            "the counts tree",
            "walk it once",
            "and cache the walk",
            "then draw it",
            "the settings page",
            "and its keys",
        ]
    );
    assert_eq!(depths(&scratch), vec![0, 1, 2, 1, 0, 1]);
}

/// What the view writes is already a depth the file can be read back at.
///
/// A note the reader is writing that somebody else deletes is kept, at the
/// end of the list -- and the note it used to hang under is not there any
/// more, so the depth it was written down with may be deeper than the end
/// can carry. Written that way it comes back a level shallower the next time
/// the file is read, which is the note moving on its own between one open
/// and the next.
#[test]
fn a_note_that_outlives_its_parent_is_written_at_a_depth_it_reads_back_at() {
    let scratch = tree("outlives", NESTED);
    let mut app = open(&scratch, 76, 20);

    // Into "and cache the walk", two levels in, and put a hand on it.
    for _ in 0..2 {
        press(&mut app, KeyCode::Down);
    }
    support::type_text(&mut app, "!");

    // Somebody else rewrites the file without it, and with nothing it could
    // hang under.
    let file = scratch.path().join(".obelus").join("todo.toml");
    std::fs::write(
        &file,
        "[[todo]]\nsaid = \"only this\"\ndone = false\ndepth = 0\n",
    )
    .expect("the notes");
    app.handle(Event::Watched(obelus_watch::Changed { path: file.clone() }));
    press(&mut app, KeyCode::Esc);

    let raw = std::fs::read_to_string(&file).expect("the notes");
    let written: Vec<u16> = raw
        .lines()
        .filter_map(|line| line.strip_prefix("depth = "))
        .filter_map(|depth| depth.parse().ok())
        .collect();
    assert_eq!(
        written,
        depths(&scratch),
        "what was written is not what reading it gives back:\n{raw}"
    );
    assert_eq!(written, vec![0, 1]);
}

/// The notes are laid out against the room they have now.
///
/// Read off the frame being drawn rather than the one before it. The width
/// was taken from what the last frame had and only then replaced, so every
/// frame that changed the geometry -- the view opening, a terminal resized,
/// a region growing as something over it closes -- laid the notes out at a
/// width they no longer had, and the next redraw put it right. A reader saw
/// their words go and come back.
#[test]
fn the_notes_are_wrapped_to_the_width_of_the_frame_being_drawn() {
    let long = "a note long enough that where it wraps says which width it was laid out at";
    let scratch = tree(
        "width-now",
        &format!("[[todo]]\nsaid = \"{long}\"\ndone = false\n"),
    );
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.configure(
        obelus_config::Config {
            wrap: true,
            ..obelus_config::Config::default()
        },
        Vec::new(),
    );
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, 100, 18);
    dispatch::dispatch(&mut app, Command::TodoOpen);
    support::lay_out(&mut app, 100, 18);
    let wide = app.notes().expect("the view").rows().len();

    // Narrower, and the very first frame at the new width: the same note
    // takes more rows, and taking the old width's number is the bug.
    let dump = support::render(&mut app, 40, 18);
    let narrow = app.notes().expect("the view").rows().len();
    assert!(
        narrow > wide,
        "the first frame at the new width used the old one: {wide} rows wide, {narrow} narrow\n{dump}"
    );
}

/// What was pasted stays on the page when the file is read again.
///
/// The notes file is watched while the page is open, so anything that
/// writes it -- another obelus, the reader's own editor, obelus itself --
/// comes back as a reread. A reread keeps the box the reader is typing in,
/// and a note the caret is in is laid out from that box; rebuilding the
/// rows before the box was put back laid it out from the file instead, and
/// the pasted words went off the page until the next thing rebuilt it.
///
/// Broken deliberately by putting `self.writing = writing` after the
/// `rebuild` in `TodoView::reread` again: the row goes back to the file's
/// words and this goes red.
#[test]
fn what_was_pasted_stays_on_the_page_when_the_file_is_read_again() {
    let scratch = tree(
        "pasted-stays",
        "[[todo]]\nsaid = \"first note\"\ndone = false\n",
    );
    let mut app = open(&scratch, 76, 18);
    press(&mut app, KeyCode::Enter);
    app.handle(Event::Paste("the words the reader pasted".to_string()));

    // Somebody else writes the file, and obelus takes it again.
    let path = scratch.path().join(".obelus").join("todo.toml");
    let written = std::fs::read_to_string(&path).expect("the notes");
    std::fs::write(
        &path,
        format!("{written}\n[[todo]]\nsaid = \"theirs\"\ndone = false\n"),
    )
    .expect("the notes");
    app.handle(Event::Watched(obelus_watch::Changed { path }));

    let dump = support::render(&mut app, 76, 18);
    assert!(
        support::text_block(&dump).contains("the words the reader pasted"),
        "reading the file again took what was pasted off the page:\n{dump}"
    );
}

/// A paste does not write the notes file over somebody else's change.
///
/// obelus writes the file whole, from what it holds, so every save is a
/// save over whatever else has been written since -- which is why the page
/// hears about the file while it is open. A paste that saved wrote the
/// whole file on a keystroke, before the reader could have heard anything,
/// and wrote a note that did not even have the paste in it yet: the box is
/// not the note until the reader leaves it.
///
/// Broken deliberately by saving in `paste_into_notes` again: the other
/// writer's note goes and this goes red.
#[test]
fn a_paste_does_not_write_the_notes_file_over_somebody_elses_change() {
    let scratch = tree(
        "half-made",
        "[[todo]]\nsaid = \"first note\"\ndone = false\n",
    );
    let mut app = open(&scratch, 76, 18);
    let path = scratch.path().join(".obelus").join("todo.toml");
    press(&mut app, KeyCode::Enter);

    // Another writer, and obelus has not heard about it yet.
    let written = std::fs::read_to_string(&path).expect("the notes");
    std::fs::write(
        &path,
        format!("{written}\n[[todo]]\nsaid = \"theirs\"\ndone = false\n"),
    )
    .expect("the notes");

    app.handle(Event::Paste("the words the reader pasted".to_string()));
    let written = std::fs::read_to_string(&path).expect("the notes");
    assert!(
        written.contains("theirs"),
        "the paste wrote the file over the other writer's note: {written}"
    );

    // And leaving does write the paste down.
    press(&mut app, KeyCode::Esc);
    let written = std::fs::read_to_string(&path).expect("the notes");
    assert!(
        written.contains("the words the reader pasted"),
        "leaving did not write the pasted words down: {written}"
    );
}

/// Starting a note does not put a blank one in the file.
///
/// Enter keeps the note being typed and starts an empty one. The keep is
/// typing, written down when the reader leaves; the empty note is not a
/// note at all, and it was reaching the file on the keystroke -- where an
/// agent asking for the list found a blank entry. `alt+t` starts one the
/// same way and has never saved.
///
/// Broken deliberately by saying `TodoOutcome::Changed` for bare enter
/// again: the blank note reaches the file and this goes red.
#[test]
fn starting_a_note_does_not_put_a_blank_one_in_the_file() {
    let scratch = tree("blank", "[[todo]]\nsaid = \"first note\"\ndone = false\n");
    let mut app = open(&scratch, 76, 18);
    let path = scratch.path().join(".obelus").join("todo.toml");
    press(&mut app, KeyCode::Enter);

    let written = std::fs::read_to_string(&path).expect("the notes");
    assert!(
        !written.contains("said = \"\""),
        "a note with nothing in it was written down: {written}"
    );

    // And what was being typed is not lost by not saving here: it reaches
    // the file when the reader leaves, along with the note they went on to.
    press(&mut app, KeyCode::Char('a'));
    press(&mut app, KeyCode::Char('b'));
    press(&mut app, KeyCode::Esc);
    let written = std::fs::read_to_string(&path).expect("the notes");
    assert!(
        written.contains("first note") && written.contains("ab"),
        "leaving did not write both notes down: {written}"
    );
}

/// What obelus writes down is what the reader has on the page.
///
/// The words are in the box until the reader leaves the note, so every
/// save that asked the view for its notes wrote the note as it *stood* --
/// without them. Anything that saves while a note is open therefore wrote
/// the file a keystroke behind the page, and another obelus or an agent
/// reading it at that moment was handed the older words.
///
/// Broken deliberately by asking for `todo().clone()` instead of
/// `as_written()` in the `Changed` arm: the words are not in the file and
/// this goes red.
#[test]
fn what_is_written_down_is_what_the_reader_has_on_the_page() {
    let scratch = tree(
        "as-written",
        "[[todo]]\nsaid = \"first note\"\ndone = false\n",
    );
    let mut app = open(&scratch, 76, 18);
    let path = scratch.path().join(".obelus").join("todo.toml");

    // A second note, typed into, and then a key that saves: tab, which
    // puts it one level in under the first.
    press(&mut app, KeyCode::Enter);
    for letter in "under it".chars() {
        press(&mut app, KeyCode::Char(letter));
    }
    press(&mut app, KeyCode::Tab);

    let written = std::fs::read_to_string(&path).expect("the notes");
    assert!(
        written.contains("under it"),
        "the save wrote the note without what was being typed into it: {written}"
    );
}

/// A note that says nothing is not written down.
///
/// The page has to have somewhere to type before there is anything to
/// type, so a note with no words in it is a real row -- the one just
/// started, and the one whose words the reader has just taken away. It is
/// nothing at all in a file: an agent asking for the list would be handed
/// a blank entry, and a reader coming back would find a note that says
/// nothing about anything.
///
/// Broken deliberately by writing every note in `Todo::to_toml` again: the
/// blank one reaches the file and this goes red.
#[test]
fn a_note_that_says_nothing_is_not_written_down() {
    let _turn = support::clipboard_turn();
    obelus_clipboard::use_provider_for_test(obelus_clipboard::Provider::Kept);

    let scratch = tree(
        "says-nothing",
        "[[todo]]\nsaid = \"first note\"\ndone = false\n",
    );
    let mut app = open(&scratch, 76, 18);
    let path = scratch.path().join(".obelus").join("todo.toml");

    // All of it out of the note, which leaves the row with nothing in it
    // and saves -- a cut is a change to the page.
    support::press_control_key(&mut app, KeyCode::Char('a'));
    support::press_control_key(&mut app, KeyCode::Char('x'));

    let written = std::fs::read_to_string(&path).expect("the notes");
    assert!(
        !written.contains("said = \"\""),
        "a note with nothing in it was written down: {written}"
    );
}

/// A note being typed into stays where the reader has it when the file is
/// read again.
///
/// A note that says nothing is not in anybody's file, because obelus does
/// not write one -- so its absence from a reread is not somebody having
/// taken it away. Treated as a deletion it was put back at the end, and a
/// note just started walked to the bottom of the list the moment anything
/// else wrote the file.
///
/// Broken deliberately by taking the empty-note arm out of
/// `TodoView::reread`: the note lands last and this goes red.
#[test]
fn a_note_being_started_stays_where_it_is_when_the_file_is_read_again() {
    let scratch = tree("stays-put", THREE);
    let mut app = open(&scratch, 76, 18);
    let path = scratch.path().join(".obelus").join("todo.toml");

    // Started on the first of three, so it is the second row.
    press(&mut app, KeyCode::Enter);
    // Somebody else writes the file, and obelus takes it again.
    let written = std::fs::read_to_string(&path).expect("the notes");
    std::fs::write(
        &path,
        format!("{written}\n[[todo]]\nsaid = \"theirs\"\ndone = false\n"),
    )
    .expect("the notes");
    app.handle(Event::Watched(obelus_watch::Changed { path }));

    for letter in "mine".chars() {
        press(&mut app, KeyCode::Char(letter));
    }
    let rows: Vec<&str> = app
        .notes()
        .expect("the view")
        .rows()
        .iter()
        .map(|row| row.said.as_str())
        .collect();
    assert_eq!(
        rows.first().copied(),
        Some("wire the counts tree up to the search")
    );
    assert_eq!(
        rows.get(1).copied(),
        Some("mine"),
        "reading the file again moved the note being started: {rows:?}"
    );
}

/// The note the keys are on is marked down its edge, not by its ground.
///
/// Every other list in obelus says "the keys are here" with a background,
/// and this is the one list whose rows the reader also selects text
/// *inside*. Two grounds on the same cells is the reader unable to see
/// where what they are holding begins or ends, which is the only thing a
/// selection has to say. So the mark is a stroke in the column outside the
/// words, beside every row of the note -- a note is one thing on this page,
/// and a mark on one row of it would claim the reader was holding a line.
///
/// In the selection's own colour, because the page has one idea of what is
/// picked out: a mark beside the words and a ground under them cannot be
/// taken for each other, and a third colour would be a third thing to
/// learn. Read off the cell's ground rather than its glyph -- the mark is
/// half a cell of that ground with the other half masked, so the glyph
/// says nothing about whether the mark is there.
///
/// Broken deliberately by filling the row with `selected_row_background`
/// again, or by marking only `at == window.focus()`: the first leaves a
/// ground for the selection to argue with, the second leaves the note's
/// other lines unmarked.
#[test]
fn the_note_the_keys_are_on_is_marked_down_its_edge() {
    let _turn = support::clipboard_turn();
    obelus_clipboard::use_provider_for_test(obelus_clipboard::Provider::Kept);

    let scratch = tree("edge", THREE);
    let mut app = open(&scratch, 60, 14);
    // Onto the note of several lines, with a run of it held: both marks
    // are on the page at once, which is where they used to collide.
    press(&mut app, KeyCode::Down);
    support::press_control_key(&mut app, KeyCode::Char('a'));
    let dump = support::render(&mut app, 60, 14);

    // Nothing in the list wears the ground a selected row wears, so the
    // one coloured ground among the notes is what the reader is holding.
    //
    // The *list*, and not the whole page: the foot draws each key in a cap,
    // and a cap's ground is the same colour as a selected row in the themes
    // obelus ships. Under a rule and among keys it is not something the
    // selection argues with.
    let ground = support::spelled(app.theme().selected_row_background);
    let letters: Vec<char> = support::legend_block(&dump)
        .lines()
        .filter(|entry| entry.contains(&format!("bg={ground}")))
        .filter_map(|entry| entry.chars().next())
        .collect();
    let styles: Vec<&str> = support::style_block(&dump).lines().collect();
    let list = &styles[..styles.len().saturating_sub(3)];
    assert!(
        !list
            .iter()
            .any(|row| row.chars().any(|cell| letters.contains(&cell))),
        "the list is drawing a ground the selection has to argue with:\n{dump}"
    );
    let held = support::spelled(app.theme().selection_background);
    assert!(
        support::legend_block(&dump).contains(&format!("bg={held}")),
        "nothing on the page is marked as held:\n{dump}"
    );

    // And the mark runs down the edge of the whole note -- its three lines
    // and the place it points at -- and beside no row of any other. Read
    // off the edge column of every row, against the text beside it.
    let words: Vec<&str> = support::text_block(&dump).lines().collect();
    let edges: Vec<&str> = support::style_block(&dump).lines().collect();
    let marked: Vec<&str> = edges
        .iter()
        .zip(&words)
        .filter(|(style, _)| {
            style
                .split_once('|')
                .and_then(|(_, cells)| cells.chars().next())
                .is_some_and(|letter| {
                    support::legend_of(&dump, letter).contains(&format!("bg={held}"))
                })
        })
        .map(|(_, row)| *row)
        .collect();
    let beside = |what: &str| marked.iter().any(|row| row.contains(what));
    assert!(
        beside("this cache does not notice a theme")
            && beside("rows caches")
            && beside("changes the colours")
            && beside("sample.rs:2"),
        "the mark is not down the edge of the whole note the keys are on:\n{dump}"
    );
    assert_eq!(
        marked.len(),
        4,
        "the mark is beside rows that are not the note the keys are on:\n{dump}"
    );
}

/// An arrow with nowhere to go still lets go of what was held.
///
/// A motion drops the anchor before it moves, so a key that could not move
/// the caret -- left at the start of a note, right at its end -- has
/// already let go by the time it answers "nothing happened". The view read
/// that answer as "not my key" and did not draw itself again, so the run
/// stayed coloured on a note that was no longer holding it, until the next
/// thing rebuilt the page.
///
/// Broken deliberately by answering `self.move_to(..)` alone in
/// `Holding::handle_key`: the run stays on the page and this goes red.
#[test]
fn an_arrow_with_nowhere_to_go_still_lets_go_of_what_is_held() {
    let scratch = tree("let-go", THREE);
    let mut app = open(&scratch, 60, 14);
    let holding = |app: &App| {
        app.notes()
            .expect("the view")
            .rows()
            .iter()
            .any(|row| row.held.is_some())
    };

    // Held back to the very start of the note, and then left again.
    press(&mut app, KeyCode::Right);
    support::press_shift(&mut app, KeyCode::Left);
    assert!(holding(&app), "shift+left took hold of nothing");
    press(&mut app, KeyCode::Left);
    assert!(
        !holding(&app),
        "left at the start of a note left the run coloured:\n{}",
        support::render(&mut app, 60, 14)
    );

    // And the whole of it held, caret at its end, and then right again.
    support::press_control_key(&mut app, KeyCode::Char('a'));
    assert!(holding(&app), "the whole note was not taken hold of");
    press(&mut app, KeyCode::Right);
    assert!(
        !holding(&app),
        "right at the end of a note left the run coloured:\n{}",
        support::render(&mut app, 60, 14)
    );
}

/// Up and down let go before they leave the note.
///
/// They are how a reader walks the list, so a press made with something
/// held used to do two things at once -- drop the selection and land on
/// another note -- and only the second was visible. Letting go is what the
/// key was asked for; leaving is what the next press is for.
///
/// Broken deliberately by answering `move_to` alone in `Composer::up` and
/// `Composer::down`, or by following the caret without rebuilding: the
/// first press lands on the next note, or leaves the run coloured.
#[test]
fn down_lets_go_of_what_is_held_before_it_leaves_the_note() {
    let scratch = tree("let-go-down", THREE);
    let mut app = open(&scratch, 60, 14);
    let held = |app: &App| {
        app.notes()
            .expect("the view")
            .rows()
            .iter()
            .any(|row| row.held.is_some())
    };
    let on = |app: &App| app.notes().expect("the view").window().focus();

    support::press_shift(&mut app, KeyCode::Right);
    support::press_shift(&mut app, KeyCode::Right);
    assert!(held(&app), "shift+right took hold of nothing");

    // The first press lets go, and stays.
    press(&mut app, KeyCode::Down);
    assert!(
        !held(&app),
        "the run is still coloured on a note the box has let go of:\n{}",
        support::render(&mut app, 60, 14)
    );
    assert_eq!(on(&app), 0, "the first press left the note as well");

    // The second walks the list, the way it always has.
    press(&mut app, KeyCode::Down);
    assert_eq!(
        on(&app),
        1,
        "the second press did not step to the next note"
    );
}

/// Typing is written down once the reader stops, not on every key.
///
/// Every other way the notes change is one act and is written the moment it
/// happens. Typing is not one act, and it used to be written when the
/// reader left the page -- there is no leaving a document, so the pause is
/// the moment instead.
#[test]
fn typing_is_written_down_once_the_reader_stops() {
    let scratch = tree("settles", THREE);
    let mut app = open(&scratch, 76, 18);
    let file = scratch.path().join(".obelus").join("todo.toml");

    support::type_text(&mut app, "!!");
    let at_once = std::fs::read_to_string(&file).expect("the notes");
    assert!(
        !at_once.contains("!!wire the counts"),
        "every keystroke wrote the file:\n{at_once}"
    );

    // The clock is what comes back for it, which is why it is kept awake.
    // Asked after a frame, because that is where the question is answered.
    support::lay_out(&mut app, 76, 18);
    assert!(app.is_waking(), "nothing will come back to write the notes");
    std::thread::sleep(std::time::Duration::from_millis(350));
    app.handle(Event::Tick);
    let after = std::fs::read_to_string(&file).expect("the notes");
    assert!(
        after.contains("!!wire the counts"),
        "the pause did not write the notes:\n{after}"
    );
    support::lay_out(&mut app, 76, 18);
    assert!(!app.is_waking(), "the clock is still being kept awake");
}

/// And closing the document writes what the pause has not yet.
///
/// A second between typing and `ctrl+w` is the one moment the pause has not
/// come. A note lives nowhere but the file.
#[test]
fn closing_the_notes_writes_what_was_typed() {
    let scratch = tree("closes", THREE);
    let mut app = open(&scratch, 76, 18);
    let file = scratch.path().join(".obelus").join("todo.toml");

    support::type_text(&mut app, "??");
    dispatch::dispatch(&mut app, Command::DocumentClose);

    assert!(app.notes().is_none(), "the notes did not close");
    let written = std::fs::read_to_string(&file).expect("the notes");
    assert!(
        written.contains("??wire the counts"),
        "closing lost what was typed:\n{written}"
    );
}

/// And so does leaving obelus, for the same reason and without asking.
///
/// An unwritten buffer is a decision -- the reader's change, or the file on
/// disk -- and there is no such decision here.
#[test]
fn leaving_obelus_writes_what_was_typed() {
    let scratch = tree("leaves", THREE);
    let mut app = open(&scratch, 76, 18);
    let file = scratch.path().join(".obelus").join("todo.toml");

    support::type_text(&mut app, "~~");
    app.request_quit();

    let written = std::fs::read_to_string(&file).expect("the notes");
    assert!(
        written.contains("~~wire the counts"),
        "leaving lost what was typed:\n{written}"
    );
}

/// The status row says which document this is, and how much is left in it.
///
/// A conversation carries no name on this row -- its identity is a header
/// inside the region -- and the notes cannot do that: there is one of them
/// and a header would be a row of the reader's screen spent on a constant.
/// So the mark goes where a file's path goes, which is what tells it from a
/// file at a glance, and the one number about it that changes goes where a
/// file's cursor position goes.
#[test]
fn the_status_row_says_it_is_the_notes_and_what_is_left() {
    let scratch = tree("status", THREE);
    let mut app = open(&scratch, 76, 18);
    let dump = support::render(&mut app, 76, 18);
    let rows = support::text_block(&dump);
    let status = rows.lines().last().unwrap_or_default();

    assert!(
        status.contains("Todo"),
        "the row does not say which document this is:\n{dump}"
    );
    assert!(
        status.contains("to come back to"),
        "the row does not say how much is left:\n{dump}"
    );
}

/// A note an agent writes while the reader is talking to it is not put back.
///
/// The notes are a document, and a document can be open without being the
/// one on screen. An agent writes notes at exactly that moment: the reader
/// is in the conversation, so the notes are one of the other documents.
///
/// A note lives nowhere but the file, and the page writes the whole file
/// from the copy it holds -- on the reader's next keystroke in it, on its
/// being closed, on obelus leaving. So a page that never heard about the
/// write held the file as it was before, and put it back. The agent ticked
/// a note off, said so, and was telling the truth; by the time the reader
/// went to look, obelus had undone it.
///
/// Obelus watches the file and re-reads it when anybody writes it, its own
/// writes included, and that was not the half that was broken: the
/// re-reading asked for "the notes" and got the notes *on screen*, so with
/// the conversation in front it returned at once. The watch was alive and
/// its answer was to do nothing.
///
/// Going back to the notes is what makes this bite rather than a second
/// reading saving it: asking for them again goes back to the page as the
/// reader left it, on purpose, because reading the file again would lose
/// their place in the list.
///
/// Broken deliberately by re-reading only the page on screen, which is what
/// it did: the file is written correctly and then put back the way it was.
#[test]
fn a_note_an_agent_writes_behind_the_conversation_is_not_put_back() {
    let scratch = support::Scratch::new("todo-agent-behind");
    std::fs::create_dir_all(scratch.path().join(".obelus")).expect("the directory");
    std::fs::write(
        scratch.path().join(".obelus").join("todo.toml"),
        "[[todo]]\nid = \"0123456W\"\nsaid = \"a note\"\ndone = false\ndepth = 0\n",
    )
    .expect("the notes");

    // A file open, the notes opened over it, and then the file in front
    // again -- which is the shape a reader talking to an agent is in.
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    dispatch::dispatch(&mut app, Command::TodoOpen);
    let _ = support::render(&mut app, 76, 18);
    dispatch::dispatch(&mut app, Command::DocumentList);
    press(&mut app, KeyCode::Up);
    press(&mut app, KeyCode::Enter);
    let _ = support::render(&mut app, 76, 18);
    assert!(
        app.notes().is_none(),
        "the notes are still the page on screen, so this proves nothing"
    );

    // And the agent ticks the note off, the way its tools do.
    let id = obelus_git::todo::NoteId::read("0123456W").expect("a name");
    let (answer, mut said) = futures::channel::oneshot::channel();
    app.handle(obelus_app::event::Event::Notes(obelus_mcp::Asked {
        doing: obelus_git::todo::Doing::Finish(id),
        answer,
    }));
    assert_eq!(
        said.try_recv().ok().flatten().as_deref(),
        Some("ticked off"),
        "the tool did not tick it off"
    );
    assert!(
        obelus_git::todo::Todo::read(scratch.path()).notes[0].done,
        "the file does not say the note is done"
    );

    // Then the reader goes back to their notes -- which goes back to the
    // page, not to the file -- and it is written down, the way closing it
    // and leaving obelus both write it down.
    dispatch::dispatch(&mut app, Command::TodoOpen);
    let _ = support::render(&mut app, 76, 18);
    assert!(app.notes().is_some(), "the notes are not back on screen");
    dispatch::dispatch(&mut app, Command::DocumentClose);
    assert!(
        obelus_git::todo::Todo::read(scratch.path()).notes[0].done,
        "the page put the note back the way it was before the agent touched it"
    );
}

/// Escape writes the note down and leaves the caret in it.
///
/// Escape everywhere in obelus leaves whatever is *over* what is being
/// read, and nothing is over this: the notes are a document rather than a
/// thing on top of one. So what it does here is the writing down -- which
/// is what leaving a note used to be the only moment for.
///
/// It went through the same call that leaving a note goes through, and that
/// one takes the box away, because leaving a note is what finishes it.
/// Nothing put the box back. The only thing that opens one is walking to
/// another note, so with one note in the list there was nothing to walk to:
/// the caret went out on escape and no key could bring it back. Not an
/// arrow, not a letter.
///
/// A note with nothing in it is still thrown away -- obelus does not write
/// one down -- and then the caret goes to the nearest note there still is,
/// because a list with notes in it and the caret nowhere is a list no key
/// can reach.
///
/// Broken deliberately two ways: by writing the note down the way leaving
/// it does, which takes the box and strands the reader; and by dropping the
/// empty note without landing anywhere, which strands them on the one path
/// where the box really does have to move.
#[test]
fn escape_writes_the_note_down_and_leaves_the_caret_in_it() {
    let scratch = tree(
        "esc-stays",
        "[[todo]]\nid = \"0123456X\"\nsaid = \"a note\"\ndone = false\ndepth = 0\n",
    );
    let mut app = open(&scratch, 76, 18);
    let caret = |app: &mut App| {
        let dump = support::render(app, 76, 18);
        dump.split("-- cursor --")
            .nth(1)
            .unwrap_or("?")
            .trim()
            .to_string()
    };

    // Something typed at the end of it -- the caret opens at the start --
    // and then escape.
    press(&mut app, KeyCode::End);
    for said in " and more".chars() {
        press(&mut app, KeyCode::Char(said));
    }
    let before = caret(&mut app);
    press(&mut app, KeyCode::Esc);
    assert_eq!(
        caret(&mut app),
        before,
        "escape took the caret out of the note"
    );
    // And what was typed is in the file, which is the whole of what escape
    // is for here.
    assert_eq!(
        obelus_git::todo::Todo::read(scratch.path()).notes[0].said,
        "a note and more",
        "escape did not write the note down"
    );
    // The keys still reach it: a letter is a letter.
    press(&mut app, KeyCode::Char('!'));
    assert_ne!(caret(&mut app), before, "the caret is stuck after escape");

    // And a note with nothing in it goes, with the caret landing on the
    // one that is left rather than nowhere.
    dispatch::dispatch(&mut app, Command::TodoAdd);
    let _ = support::render(&mut app, 76, 18);
    press(&mut app, KeyCode::Esc);
    let after = caret(&mut app);
    assert_ne!(
        after, "none",
        "the caret went nowhere when the empty note did"
    );
    assert_eq!(
        obelus_git::todo::Todo::read(scratch.path()).notes.len(),
        1,
        "the note with nothing in it was written down"
    );
}

/// Leaving the notes for somewhere and coming back leaves the caret in
/// them.
///
/// Two keys go somewhere from a note: one opens the conversation about it,
/// one goes to the place it is about. Neither closes the page. Both say so
/// -- the notes stay open, because a document they came from is a document
/// they come back to -- and the key that comes back goes to this view as
/// the reader left it, on purpose, because reading the file again would
/// lose their place in the list.
///
/// So both wrote the note down the way *leaving a note* writes it down,
/// which takes the box away because leaving a note is what finishes it.
/// Coming back, there was no box: the caret was gone, and no key in the box
/// could bring it back -- not home, not end, not an arrow, because those
/// are the box's own keys and there was no box for them to move in. Only
/// walking to another note opened one, which is the reader arriving at
/// their notes and being sent to a different one to get a caret at all.
///
/// Broken deliberately by writing the note down the way leaving it does, at
/// either of the two keys: the route out is fine and the route back has no
/// caret in it.
#[test]
fn leaving_the_notes_and_coming_back_leaves_the_caret_in_them() {
    let scratch = tree("come-back", THREE);
    let caret = |app: &mut App| {
        let dump = support::render(app, 76, 18);
        dump.split("-- cursor --")
            .nth(1)
            .unwrap_or("?")
            .trim()
            .to_string()
    };

    // Away to the conversation about the note, and back by the key the
    // conversation names.
    let mut app = open(&scratch, 76, 18);
    let in_a_note = caret(&mut app);
    assert_ne!(in_a_note, "none", "the notes opened with no caret");
    app.handle(alt(KeyCode::Char('a')));
    assert!(app.chat().is_some(), "alt+a did not open the conversation");
    app.handle(alt(KeyCode::Char('t')));
    assert!(
        app.notes().is_some(),
        "alt+t did not come back to the notes"
    );
    assert_eq!(
        caret(&mut app),
        in_a_note,
        "coming back from the conversation left the notes with no caret"
    );
    // Which is to say the box is there: its own keys move in it.
    press(&mut app, KeyCode::End);
    assert_ne!(caret(&mut app), "none", "there is no box to move in");

    // And away to the place a note is about, which is the other way out.
    // The note that points somewhere is the second, and it has to point at
    // a file that is there.
    std::fs::write(scratch.path().join("sample.rs"), "one\ntwo\nthree\n").expect("the file");
    let mut app = open(&scratch, 76, 18);
    press(&mut app, KeyCode::Down);
    let in_a_note = caret(&mut app);
    app.handle(alt(KeyCode::Enter));
    assert!(app.notes().is_none(), "alt+enter did not go to the file");
    dispatch::dispatch(&mut app, Command::TodoOpen);
    assert!(app.notes().is_some(), "the notes did not come back");
    assert_eq!(
        caret(&mut app),
        in_a_note,
        "coming back from the file left the notes with no caret"
    );
}

/// A press on a note's box ticks it off, and a press on its mark opens the
/// conversation about it.
///
/// A row of the notes draws two things beside the words that the reader can
/// *do* something to: the box saying whether the note is done, and the
/// marks saying somebody has talked about it. Both are one key away and both
/// are a picture of that key -- and a press on either did nothing, because the
/// only thing the pointer reached in this page was the note being written.
///
/// A press goes to the note first, because both keys ask about the note the
/// caret is in, and then down the key's own path: not a second way to tick
/// a note off and not a second way to open its conversation.
///
/// Broken deliberately by letting a press on either mark fall through to
/// the words, which puts the caret in the note and leaves it as it was.
#[test]
fn a_press_on_a_notes_box_ticks_it_and_on_its_mark_opens_the_conversation() {
    let scratch = tree(
        "note-marks",
        "[[todo]]\nid = \"0123456B\"\nsaid = \"the first\"\ndone = false\ndepth = 0\n\n[[todo]]\nid = \"0123456C\"\nsaid = \"the second\"\ndone = false\ndepth = 0\n",
    );
    let mut app = open(&scratch, 76, 18);
    let _ = support::render(&mut app, 76, 18);

    // The second note's row, so that the press has to move the caret to
    // reach it: the caret opens in the first.
    let rows = app.notes().expect("the notes").rows().len();
    assert!(rows >= 2, "there is only one row, so this proves nothing");
    let area = app.editor_area_for_test();
    let top = obelus_ui::todo::list_region(area, &[]).y;
    let press = |app: &mut obelus_app::app::App, row: u16, x: u16| {
        app.handle(obelus_app::event::Event::Pointer {
            kind: obelus_app::event::Pointer::Pressed,
            x,
            y: top + row,
        });
    };

    // The box, which is the fifth column: one clear of the edge, the two
    // that say what the conversation is doing, and the two that say there
    // is one.
    press(&mut app, 1, area.x + 5);
    let written = obelus_git::todo::Todo::read(scratch.path());
    assert!(
        written.notes[1].done,
        "the press on the box did not tick the note off"
    );
    assert!(
        !written.notes[0].done,
        "it ticked off a note the press was not on"
    );

    // And the mark, which opens the conversation about that note. There is
    // no agent here, so what is asserted is that obelus went to a
    // conversation rather than staying in the notes.
    press(&mut app, 1, area.x + 1);
    assert!(
        app.chat().is_some(),
        "the press on the mark did not open the conversation"
    );
}

/// A list of notes longer than the screen scrolls under the caret.
///
/// The window over the rows was told how wide a note's words are and never
/// how many rows the reader could see, so its top sat at zero for the life
/// of the view. The selection walked off the bottom and the page stayed
/// where it was: a reader pressing down went on typing into a note that was
/// no longer drawn, with no caret anywhere on screen to say where they
/// were. The scrollbar beside it was drawn the whole time, promising a view
/// that moved.
///
/// It makes three claims and was broken deliberately three times. Leaving
/// the window unsettled keeps the top at zero and walks the selection off
/// the bottom. Settling it against the whole region rather than the list
/// puts the last rows under the foot. And settling only on the way down
/// leaves a reader who walks back up looking at the middle of the list with
/// their caret above it.
#[test]
fn a_list_of_notes_taller_than_the_screen_scrolls_under_the_caret() {
    let many: String = (0..40)
        .map(|n| format!("[[todo]]\nsaid = \"note number {n}\"\ndone = false\ndepth = 0\n\n"))
        .collect();
    let scratch = tree("scrolling", &many);
    let mut app = open(&scratch, 76, 18);
    let _ = support::render(&mut app, 76, 18);
    let area = app.editor_area_for_test();
    let rows = obelus_ui::todo::list_region(
        area,
        &obelus_ui::todo::hints(app.notes().expect("the notes")),
    )
    .height;
    assert!(
        usize::from(rows) < app.notes().expect("the notes").rows().len(),
        "the list fits on the screen, so this proves nothing"
    );

    // Down, well past the last row the screen can hold.
    for _ in 0..20 {
        press(&mut app, KeyCode::Down);
    }
    let down = support::render(&mut app, 76, 18);
    let window = app.notes().expect("the notes").window();
    assert!(
        window.top() > 0,
        "the page did not move under a selection that walked off it:\n{down}"
    );
    assert!(
        window.focus() >= window.top() && window.focus() < window.top() + usize::from(rows),
        "the row the keys are on is not one of the rows drawn:\n{down}"
    );
    assert!(
        obelus_ui::todo::caret(area, app.notes().expect("the notes")).is_some(),
        "the reader is typing into a note with no caret on the page:\n{down}"
    );

    // And back up to the top, which is the half a window that only ever
    // scrolls one way fails.
    for _ in 0..20 {
        press(&mut app, KeyCode::Up);
    }
    let up = support::render(&mut app, 76, 18);
    let window = app.notes().expect("the notes").window();
    assert_eq!(
        (window.top(), window.focus()),
        (0, 0),
        "walking back up left the page where it was:\n{up}"
    );
}
