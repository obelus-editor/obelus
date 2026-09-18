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
    use obelus::theme::builtin::DARK;

    let _turn = support::clipboard_turn();
    obelus::clipboard::use_provider_for_test(obelus::clipboard::Provider::Kept);

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
        obelus::clipboard::paste().as_deref(),
        Some("wire"),
        "what was held did not reach the clipboard"
    );

    // And with nothing held, the whole note -- the way the file copies the
    // whole line rather than nothing at all.
    press(&mut app, KeyCode::Right);
    support::press_control_key(&mut app, KeyCode::Char('c'));
    assert_eq!(
        obelus::clipboard::paste().as_deref(),
        Some("wire the counts tree up to the search"),
    );
}

/// The other three keys a reader brings with them: all of it, out, and back.
#[test]
fn a_note_takes_the_clipboard_keys_too() {
    let _turn = support::clipboard_turn();
    obelus::clipboard::use_provider_for_test(obelus::clipboard::Provider::Kept);

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
        obelus::clipboard::paste().as_deref(),
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
    obelus::clipboard::use_provider_for_test(obelus::clipboard::Provider::Kept);

    let scratch = tree("cut-note", THREE);
    let mut app = open(&scratch, 76, 18);
    support::press_control_key(&mut app, KeyCode::Char('x'));

    assert_eq!(
        obelus::clipboard::paste().as_deref(),
        Some("wire the counts tree up to the search")
    );
    let written = std::fs::read_to_string(scratch.path().join(".obelus").join("todo.toml"))
        .expect("the notes");
    assert!(
        !written.contains("wire the counts"),
        "the cut note was not written away: {written}"
    );
}

/// What the terminal pastes lands in the note, not in the file behind it.
///
/// Broken deliberately by sending every paste straight to the buffer: the
/// notes are a whole screen of their own, and the reader's text went into a
/// file they could not see.
#[test]
fn what_the_terminal_pastes_lands_in_the_note() {
    let scratch = tree("bracketed", THREE);
    let mut app = open(&scratch, 76, 18);
    let was = app
        .current_buffer()
        .expect("a buffer")
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
        app.current_buffer().expect("a buffer").text().rope(),
        &was,
        "the paste went into the file behind the notes"
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
    // the box, so a note reads as one thing.
    assert_eq!(
        support::column_of(&wrapped[1], "of a narrow"),
        support::column_of(&wrapped[0], "a note"),
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
        obelus::config::Config {
            wrap: true,
            ..obelus::config::Config::default()
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
    app.handle(Event::FileChanged { path: file.clone() });

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
    app.handle(Event::FileChanged { path: file.clone() });

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
    obelus::todo::Todo::read(scratch.path())
        .notes
        .iter()
        .map(|note| note.depth)
        .collect()
}

/// What each note says, in the order the file has them.
fn titles(scratch: &support::Scratch) -> Vec<String> {
    obelus::todo::Todo::read(scratch.path())
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
    // Where the words start, counted from the row's own left edge.
    let starts = |needle: &str| {
        let row = at(needle);
        let (_, said) = row.split_once('|').expect("the row number");
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
        let row = at(needle);
        let (_, said) = row.split_once('|').expect("the row number");
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
        obelus::config::Config {
            wrap: true,
            ..obelus::config::Config::default()
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
    let widths: Vec<usize> = said
        .iter()
        .map(|row| row.chars().count())
        .filter(|width| *width > 1)
        .collect();
    assert!(
        widths.windows(2).all(|pair| pair[0] == pair[1]),
        "the notes wrapped in different columns: {widths:?}\n{dump}"
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
    app.handle(Event::FileChanged { path: file.clone() });
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
    let scratch = tree("width-now", &format!("[[todo]]\nsaid = \"{long}\"\ndone = false\n"));
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.configure(
        obelus::config::Config {
            wrap: true,
            ..obelus::config::Config::default()
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
    let scratch = tree("pasted-stays", "[[todo]]\nsaid = \"first note\"\ndone = false\n");
    let mut app = open(&scratch, 76, 18);
    press(&mut app, KeyCode::Enter);
    app.handle(Event::Paste("the words the reader pasted".to_string()));

    // Somebody else writes the file, and obelus takes it again.
    let path = scratch.path().join(".obelus").join("todo.toml");
    let written = std::fs::read_to_string(&path).expect("the notes");
    std::fs::write(&path, format!("{written}\n[[todo]]\nsaid = \"theirs\"\ndone = false\n"))
        .expect("the notes");
    app.handle(Event::FileChanged { path });

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
    let scratch = tree("half-made", "[[todo]]\nsaid = \"first note\"\ndone = false\n");
    let mut app = open(&scratch, 76, 18);
    let path = scratch.path().join(".obelus").join("todo.toml");
    press(&mut app, KeyCode::Enter);

    // Another writer, and obelus has not heard about it yet.
    let written = std::fs::read_to_string(&path).expect("the notes");
    std::fs::write(&path, format!("{written}\n[[todo]]\nsaid = \"theirs\"\ndone = false\n"))
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
