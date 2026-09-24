//! What a server would have the reader know that the file does not say.
//!
//! Cells drawn in a line that the line does not contain: a type nobody
//! wrote down, the name of the parameter an argument goes to. The file is
//! complete without them, so nothing in one can be stood on, selected or
//! copied -- and everything after one is drawn that much further along.

mod support;

use obelus_app::app::App;
use obelus_buffer::Buffer;
use serde_json::json;

fn coding(name: &str, text: &str) -> (support::Scratch, App) {
    let scratch = support::Scratch::new(name);
    let path = scratch.path().join("one.rs");
    std::fs::write(&path, text).expect("writing it");
    let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, 76, 18);
    (scratch, app)
}

/// A hint is drawn where the server put it, in front of the character it
/// names, and everything after it moves along by however wide it is.
#[test]
fn a_hint_is_drawn_and_the_line_makes_room_for_it() {
    let (_scratch, mut app) = coding("hint-drawn", "let x = compute();\n");
    app.hints_for_test(json!([{
        "position": { "line": 0, "character": 5 },
        "label": ": i32",
        "kind": 1
    }]));

    let dump = support::render(&mut app, 76, 18);
    let rows: Vec<&str> = support::text_block(&dump).lines().collect();
    let row = rows
        .iter()
        .find(|row| row.contains("compute"))
        .expect("the line");
    assert!(
        row.contains("let x: i32 = compute();"),
        "the hint is not drawn where the server put it:\n{dump}"
    );
}

/// It is not code, and is drawn as something a reader skimming the file can
/// skip without reading.
#[test]
fn a_hint_is_drawn_in_its_own_colour() {
    let (_scratch, mut app) = coding("hint-coloured", "let x = compute();\n");
    app.hints_for_test(json!([{
        "position": { "line": 0, "character": 5 },
        "label": ": i32"
    }]));

    let dump = support::render(&mut app, 76, 18);
    let rows: Vec<&str> = support::text_block(&dump).lines().collect();
    let styles: Vec<&str> = support::style_block(&dump).lines().collect();
    let at = rows
        .iter()
        .position(|row| row.contains("compute"))
        .expect("the line");
    let divider = rows[at].find('|').expect("a divider") + 1;
    let text = &rows[at][divider..];
    let cells = &styles[at][divider..];
    let hint = text.find(": i32").expect("the hint");
    let code = text.find("compute").expect("the call");
    assert_ne!(
        cells.chars().nth(hint).expect("a style"),
        cells.chars().nth(code).expect("a style"),
        "the hint is drawn as though it were code:\n{dump}"
    );
}

/// Nothing in it can be stood on: the caret walks characters, and a hint is
/// not one.
///
/// Which is the whole reason these are cells rather than text. Walking to
/// the end of the line lands on the last character of the file's line, not
/// somewhere inside what a server made up.
#[test]
fn the_caret_walks_past_a_hint_without_entering_it() {
    use crossterm::event::KeyCode;

    let (_scratch, mut app) = coding("hint-caret", "let x = compute();\n");
    app.hints_for_test(json!([{
        "position": { "line": 0, "character": 5 },
        "label": ": i32"
    }]));

    // Onto the character the hint sits in front of, then one more.
    for _ in 0..5 {
        support::press(&mut app, KeyCode::Right);
    }
    let cursor = app.current_buffer().expect("a file").cursor();
    assert_eq!(cursor.column.get(), 5);
    support::press(&mut app, KeyCode::Right);
    assert_eq!(
        app.current_buffer().expect("a file").cursor().column.get(),
        6,
        "the caret stopped inside a hint"
    );
}

/// An answer about a document that has moved since is refused: the places
/// it names are places in the file as it was, and Obelus has no way to tell
/// where they went.
///
/// Which is not the same as what an edit does to the hints already drawn --
/// those it carries along, because it knows exactly how far everything
/// moved.
#[test]
fn a_hint_about_a_file_that_has_changed_is_dropped() {
    let (_scratch, mut app) = coding("hint-stale", "let x = compute();\n");
    support::type_text(&mut app, "y");
    let behind = app.current_buffer().expect("a buffer").version() - 1;
    app.hints_at_version_for_test(
        json!([{
            "position": { "line": 0, "character": 5 },
            "label": ": i32"
        }]),
        behind,
    );
    assert_eq!(
        app.hints_for_test_count(),
        0,
        "an answer about the file as it was was kept"
    );
}

/// The question that goes out is the one the protocol names, about the
/// whole file.
#[test]
fn the_question_that_goes_out_is_the_one_the_protocol_names() {
    use obelus_app::app::dispatch;
    use obelus_command::Command;
    use obelus_syntax::LanguageId;

    let (_scratch, mut app) = coding("hint-asked", "let x = compute();\n");
    let (sender, heard) = obelus_app::event::channel();
    app.events_for_test(sender);
    assert!(
        app.stand_in_server_for_test(LanguageId::Rust, "cat"),
        "the echo would not start"
    );
    app.declared_for_test(LanguageId::Rust, json!({ "inlayHintProvider": true }));

    support::type_text(&mut app, " ");
    dispatch::dispatch(&mut app, Command::FileSave);

    let asked = support::heard_requests(&heard, "textDocument/inlayHint", 1);
    assert_eq!(asked.len(), 1, "Obelus asked nobody: {asked:?}");
    assert_eq!(
        asked[0]["params"]["range"]["start"]["line"], 0,
        "the question does not start at the top of the file: {:?}",
        asked[0]
    );
    // And it ends at the document's own end rather than one past it. A
    // line count is one more than the last line number wherever a file
    // ends in a newline, and a server handed a range that ends past the
    // file refuses the *whole* request -- which reads from here as a
    // language with nothing to work out.
    let last = app
        .current_buffer()
        .expect("a file")
        .text()
        .last_line()
        .get();
    assert_eq!(
        asked[0]["params"]["range"]["end"]["line"], last,
        "the question runs past the end of the file: {:?}",
        asked[0]
    );
}

/// A selection lands on the characters the reader selected, on a line a
/// hint has made longer.
///
/// Which is the invariant the whole arrangement rests on: a hint moves
/// where a character is *drawn* and moves nothing about which character it
/// is. Everything painted by a run of columns -- the selection, a search
/// mark, the underline under a diagnostic -- asks the same question, so
/// one of them landing right is all of them landing right.
#[test]
fn a_selection_lands_on_the_right_characters_past_a_hint() {
    use crossterm::event::KeyCode;

    let (_scratch, mut app) = coding("hint-selected", "let x = compute();\n");
    app.hints_for_test(json!([{
        "position": { "line": 0, "character": 5 },
        "label": ": i32"
    }]));

    // Onto `compute`, and select the whole of it.
    for _ in 0..8 {
        support::press(&mut app, KeyCode::Right);
    }
    for _ in 0.."compute".len() {
        support::press_shift(&mut app, KeyCode::Right);
    }

    let dump = support::render(&mut app, 76, 18);
    let rows: Vec<&str> = support::text_block(&dump).lines().collect();
    let styles: Vec<&str> = support::style_block(&dump).lines().collect();
    let at = rows
        .iter()
        .position(|row| row.contains("compute"))
        .expect("the line");
    let divider = rows[at].find('|').expect("a divider") + 1;
    let text = &rows[at][divider..];
    let cells: Vec<char> = styles[at][divider..].chars().collect();
    let word = text.find("compute").expect("the call");

    // Whatever the selection is drawn in, the run of it starts where
    // `compute` is drawn and is exactly as long.
    let selected = cells[word];
    let start = cells
        .iter()
        .position(|glyph| *glyph == selected)
        .expect("the run");
    assert_eq!(
        start,
        word,
        "the selection is drawn {} cells off the characters it is about:\n{dump}",
        word.abs_diff(start)
    );
    assert!(
        cells[word.."compute".len() + word]
            .iter()
            .all(|glyph| *glyph == selected),
        "the selection stops partway through what was selected:\n{dump}"
    );
}

/// A colour and a hint in one file are drawn as themselves, not as each
/// other.
///
/// They are cells of the same line pointing into one list, because a cell
/// points at one entry and cannot say which of two lists it meant. Which
/// is only right as long as both are put in that list in the order the
/// cells were numbered against.
#[test]
fn a_colour_and_a_hint_in_one_file_do_not_swap_places() {
    let (_scratch, mut app) = coding("hint-and-colour", "let c = \"#3264eb\";\n");
    app.colours_for_test(json!([{
        "range": { "start": { "line": 0, "character": 9 },
                   "end": { "line": 0, "character": 16 } },
        "color": { "red": 0.196, "green": 0.392, "blue": 0.922, "alpha": 1.0 }
    }]));
    app.hints_for_test(json!([{
        "position": { "line": 0, "character": 5 },
        "label": ": &str"
    }]));

    let dump = support::render(&mut app, 76, 18);
    let rows: Vec<&str> = support::text_block(&dump).lines().collect();
    let styles: Vec<&str> = support::style_block(&dump).lines().collect();
    let at = rows
        .iter()
        .position(|row| row.contains("#3264eb"))
        .expect("the line");
    let divider = rows[at].find('|').expect("a divider") + 1;
    let text = &rows[at][divider..];
    let cells: Vec<char> = styles[at][divider..].chars().collect();

    // The hint says what the server said, in front of the `=`.
    assert!(
        text.contains("let c: &str = "),
        "the hint is not drawn as a hint:\n{dump}"
    );
    // And the cell in front of the literal is a square of the colour
    // itself. Cells, not bytes: that square is three bytes and one cell.
    let literal = support::column_of(text, "#3264eb");
    assert_eq!(
        text.chars().nth(literal - 1),
        Some('\u{25a0}'),
        "there is no square in front of the literal:\n{dump}"
    );
    let legend = support::legend_of(&dump, cells[literal - 1]);
    assert!(
        legend.contains("fg=#3264eb"),
        "the square is not the colour the server named: {legend:?}\n{dump}"
    );
}

/// A file opened before its server was ready is asked about as soon as it
/// is.
///
/// Every standing question is refused while a server cannot say what it
/// answers, and opening a file is when they are all asked -- which is
/// before the handshake finishes on every cold start there is. Without
/// somebody asking again, the first thing that draws a hint is the reader
/// saving.
#[test]
fn a_file_opened_before_its_server_was_ready_is_asked_about() {
    use obelus_syntax::LanguageId;

    let (_scratch, mut app) = coding("hint-cold", "let x = compute();\n");
    let (sender, heard) = obelus_app::event::channel();
    app.events_for_test(sender);
    // Started, and not yet able to say what it answers -- which is every
    // cold start.
    assert!(
        app.stand_in_server_for_test(LanguageId::Rust, "cat"),
        "the echo would not start"
    );
    app.serve_current_for_test();
    assert!(
        support::heard_requests(&heard, "textDocument/inlayHint", 0).is_empty(),
        "a question went out to a server that had not answered its handshake"
    );

    // And now it has.
    app.declared_for_test(LanguageId::Rust, json!({ "inlayHintProvider": true }));
    let asked = support::heard_requests(&heard, "textDocument/inlayHint", 1);
    assert_eq!(
        asked.len(),
        1,
        "nothing asked once the server could be asked: {asked:?}"
    );
}

/// The switch reaches the screen both ways round.
///
/// A switch that only works one way is a switch a reader has to restart to
/// use: off has to take away what is already drawn, and on has to ask for
/// what was never asked for.
#[test]
fn the_switch_takes_them_away_and_brings_them_back() {
    use obelus_syntax::LanguageId;

    let (_scratch, mut app) = coding("hint-switched", "let x = compute();\n");
    let (sender, heard) = obelus_app::event::channel();
    app.events_for_test(sender);
    assert!(
        app.stand_in_server_for_test(LanguageId::Rust, "cat"),
        "the echo would not start"
    );
    app.declared_for_test(LanguageId::Rust, json!({ "inlayHintProvider": true }));
    app.hints_for_test(json!([{
        "position": { "line": 0, "character": 5 },
        "label": ": i32"
    }]));
    let drawn = |app: &mut App| {
        let dump = support::render(app, 76, 18);
        support::text_block(&dump)
            .lines()
            .any(|row| row.contains(": i32"))
    };
    assert!(drawn(&mut app), "the reader asked for them and got nothing");

    // Off: what is on screen goes, without waiting for anything else to
    // happen.
    app.configure(
        obelus_config::Config {
            inlay_hints: false,
            ..obelus_config::Config::default()
        },
        Vec::new(),
    );
    assert!(!drawn(&mut app), "turning them off left them on the screen");

    // And on again asks, rather than waiting for the reader to save.
    let _ = support::heard_requests(&heard, "textDocument/inlayHint", 0);
    app.configure(obelus_config::Config::default(), Vec::new());
    let asked = support::heard_requests(&heard, "textDocument/inlayHint", 1);
    assert_eq!(
        asked.len(),
        1,
        "turning them on asked nobody for them: {asked:?}"
    );
}

/// The whole way through, against a real server: a file open in Obelus, a
/// language server started under it, and a type nobody wrote down drawn in
/// the line it belongs to.
///
/// Everything else here hands Obelus an answer. This is the one that says
/// the answers arrive at all -- which is the half that was broken twice:
/// once because nothing asked again once a server could be asked, and once
/// because the reader had not asked for them.
#[test]
#[ignore = "waits for the project to be indexed"]
fn a_real_server_draws_a_type_nobody_wrote_down() {
    use std::time::{Duration, Instant};

    use obelus_app::event::Event;
    use obelus_syntax::LanguageId;

    if !obelus_lsp::on_path("rust-analyzer") {
        eprintln!("skipped: rust-analyzer is not on PATH");
        return;
    }

    // A file of this project, so the server has a workspace to read.
    let path = std::path::PathBuf::from(env!("OBELUS_TREE")).join("crates/obelus-lsp/src/hint.rs");
    let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
    app.working_directory_for_test(std::path::PathBuf::from(env!("OBELUS_TREE")));
    support::lay_out(&mut app, 120, 40);
    let events = support::drive(&mut app);
    assert!(
        app.stand_in_server_for_test(LanguageId::Rust, "rust-analyzer"),
        "rust-analyzer would not start"
    );
    app.serve_current_for_test();

    // The loop Obelus runs, until something a server worked out is on
    // screen.
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        assert!(
            Instant::now() < deadline,
            "nothing a server worked out was ever drawn"
        );
        match events.recv_timeout(Duration::from_secs(5)) {
            Ok(Event::Lsp(obelus_lsp::Message { language, message })) => {
                app.handle(Event::Lsp(obelus_lsp::Message { language, message }))
            }
            Ok(_) => {}
            Err(_) => {}
        }
        if app.hints_for_test_count() > 0 {
            break;
        }
    }

    // And it is drawn where the file is, in a line the file has.
    let dump = support::render(&mut app, 120, 40);
    let rows: Vec<&str> = support::text_block(&dump).lines().collect();
    assert!(
        rows.iter().any(|row| row.contains(": ")),
        "the hints arrived and nothing drew them:\n{dump}"
    );
}

/// An edit carries them along with the text they are drawn in, rather than
/// taking them off the screen.
///
/// Dropped instead, every keystroke would take every hint off the screen
/// and put them all back a fifth of a second later -- so a line they made
/// wrap would unwrap and wrap again under the reader as they typed.
#[test]
fn an_edit_carries_the_hints_along_with_the_text() {
    let (_scratch, mut app) = coding("hint-stale-rebuild", "let x = compute();\n");
    app.hints_for_test(json!([{
        "position": { "line": 0, "character": 5 },
        "label": ": i32"
    }]));
    assert_eq!(app.hints_for_test_count(), 1);

    // The reader types in front of it.
    support::type_text(&mut app, "y");

    let dump = support::render(&mut app, 76, 18);
    let row = support::text_block(&dump)
        .lines()
        .find(|row| row.contains("compute"))
        .expect("the line")
        .to_string();
    assert!(
        row.contains("ylet x: i32 = "),
        "the hint did not move along with what was typed in front of it:\n{dump}"
    );
}

/// A probe, not a claim: after the reader types, do they come back on
/// their own, and how long does it take?
#[test]
#[ignore = "a measurement against a real server"]
fn probe_how_long_until_they_come_back() {
    use std::time::{Duration, Instant};

    use obelus_app::event::Event;
    use obelus_syntax::LanguageId;

    if !obelus_lsp::on_path("rust-analyzer") {
        return;
    }
    let path = std::path::PathBuf::from(env!("OBELUS_TREE")).join("crates/obelus-lsp/src/hint.rs");
    let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
    app.working_directory_for_test(std::path::PathBuf::from(env!("OBELUS_TREE")));
    support::lay_out(&mut app, 120, 40);
    let events = support::drive(&mut app);
    assert!(app.stand_in_server_for_test(LanguageId::Rust, "rust-analyzer"));
    app.serve_current_for_test();

    let pump = |app: &mut App, until: Duration| {
        let deadline = Instant::now() + until;
        while Instant::now() < deadline {
            if let Ok(Event::Lsp(obelus_lsp::Message { language, message })) =
                events.recv_timeout(Duration::from_millis(200))
            {
                app.handle(Event::Lsp(obelus_lsp::Message { language, message }));
            }
            if app.hints_for_test_count() > 0 {
                return Some(deadline - Instant::now());
            }
        }
        None
    };

    assert!(
        pump(&mut app, Duration::from_secs(120)).is_some(),
        "they never arrived at all"
    );
    eprintln!("MEASURE arrived, {} of them", app.hints_for_test_count());

    // The reader types one character.
    support::type_text(&mut app, "\n");
    eprintln!("MEASURE after a keystroke: {}", app.hints_for_test_count());

    let started = Instant::now();
    match pump(&mut app, Duration::from_secs(30)) {
        Some(_) => eprintln!(
            "MEASURE came back on their own after {:.1}s",
            started.elapsed().as_secs_f64()
        ),
        None => eprintln!("MEASURE never came back within 30s"),
    }
}

/// A file the reader has stopped changing is asked about again, without
/// their having saved it and without the server having said anything.
///
/// The one moment Obelus asks for these that depends on nothing but the
/// reader. A save asks, and so does a server finishing work of its own --
/// but a server that reports no work never finishes any, and the reader
/// who changed a colour and is looking at it has saved nothing.
#[test]
fn a_file_that_stops_changing_is_asked_about_again() {
    use obelus_syntax::LanguageId;

    let (_scratch, mut app) = coding("hint-settled", "let x = compute();\n");
    let (sender, heard) = obelus_app::event::channel();
    app.events_for_test(sender);
    assert!(
        app.stand_in_server_for_test(LanguageId::Rust, "cat"),
        "the echo would not start"
    );
    app.declared_for_test(LanguageId::Rust, json!({ "inlayHintProvider": true }));
    let _ = support::heard_requests(&heard, "textDocument/inlayHint", 1);

    // Typed, and nothing asked: the reader is still going.
    support::type_text(&mut app, "y");
    support::render(&mut app, 76, 18);
    assert!(
        support::heard_requests(&heard, "textDocument/inlayHint", 0).is_empty(),
        "a question went out between two keystrokes"
    );

    // And then they stop.
    std::thread::sleep(App::SETTLES_AFTER + std::time::Duration::from_millis(50));
    support::render(&mut app, 76, 18);
    let asked = support::heard_requests(&heard, "textDocument/inlayHint", 1);
    assert_eq!(
        asked.len(),
        1,
        "nothing asked once the reader stopped: {asked:?}"
    );
}
