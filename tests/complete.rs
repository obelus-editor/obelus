//! What a server offers to type next.
//!
//! Against values rather than against a server: what is interesting is what
//! obelus does with an answer -- which word it thinks the answer is about,
//! what it puts in, and what it does when the reader has gone on typing --
//! and a server cannot be made to answer late on demand.

mod support;

use obelus::{app::App, buffer::Buffer};
use serde_json::json;

/// An application over a file of the test's own, opened for editing.
fn editing(name: &str, contents: &str) -> (support::Scratch, App) {
    let scratch = support::Scratch::new(name);
    let path = scratch.path().join("sample.rs");
    std::fs::write(&path, contents).expect("writing the file");
    let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, 60, 16);
    (scratch, app)
}

/// The text of the file being edited.
fn text(app: &App) -> String {
    app.current_buffer()
        .expect("a buffer")
        .text()
        .rope()
        .to_string()
}

/// A plain answer of labels.
fn labels(names: &[&str]) -> serde_json::Value {
    json!(
        names
            .iter()
            .map(|name| json!({ "label": name, "kind": 3 }))
            .collect::<Vec<_>>()
    )
}

#[test]
fn a_panel_opens_beside_the_word_being_typed() {
    let (_scratch, mut app) = editing("complete-open", "fn main() {\n    pu\n}\n");
    support::press(&mut app, crossterm::event::KeyCode::Down);
    support::press(&mut app, crossterm::event::KeyCode::End);
    app.complete_for_test(labels(&["push_str", "pop", "parse"]));

    let dump = support::render(&mut app, 60, 16);
    assert!(
        dump.contains("push_str"),
        "the candidates are not on screen:\n{dump}"
    );
    // Under the word, not at the left margin: the panel's left edge is
    // where the word starts.
    let rows: Vec<&str> = support::text_block(&dump).lines().collect();
    let box_row = rows
        .iter()
        .find(|row| row.contains(support::PANEL_CORNER))
        .unwrap_or_else(|| panic!("no panel:\n{dump}"));
    let corner = box_row.find(support::PANEL_CORNER).expect("a corner");
    let word = rows
        .iter()
        .find(|row| row.contains("    pu"))
        .and_then(|row| row.find("pu"))
        .expect("the word");
    assert_eq!(
        corner, word,
        "the panel is not under the word being typed:\n{dump}"
    );
}

/// The panel opens on the server's best answer, so one key takes it.
#[test]
fn the_first_candidate_is_selected_from_the_start() {
    let (_scratch, mut app) = editing("complete-selected", "fn main() {\n    p\n}\n");
    support::press(&mut app, crossterm::event::KeyCode::Down);
    support::press(&mut app, crossterm::event::KeyCode::End);
    app.complete_for_test(labels(&["parse", "pop"]));

    support::press(&mut app, crossterm::event::KeyCode::Enter);
    assert_eq!(
        text(&app),
        "fn main() {\n    parse\n}\n",
        "enter did not take the row the panel opened on"
    );

    // And the arrows move that selection rather than making one.
    let (_scratch, mut app) = editing("complete-second", "fn main() {\n    p\n}\n");
    support::press(&mut app, crossterm::event::KeyCode::Down);
    support::press(&mut app, crossterm::event::KeyCode::End);
    app.complete_for_test(labels(&["parse", "pop"]));
    support::press(&mut app, crossterm::event::KeyCode::Down);
    support::press(&mut app, crossterm::event::KeyCode::Enter);
    assert_eq!(
        text(&app),
        "fn main() {\n    pop\n}\n",
        "the arrow did not move the selection on by one"
    );
}

#[test]
fn a_candidate_replaces_the_word_it_was_offered_for() {
    let (_scratch, mut app) = editing("complete-accept", "fn main() {\n    pu\n}\n");
    support::press(&mut app, crossterm::event::KeyCode::Down);
    support::press(&mut app, crossterm::event::KeyCode::End);
    app.complete_for_test(labels(&["push_str", "pop"]));
    // Enter, and only enter: the panel comes up already chosen, and `tab`
    // means something else everywhere it is used.
    support::press(&mut app, crossterm::event::KeyCode::Enter);

    assert_eq!(
        text(&app),
        "fn main() {\n    push_str\n}\n",
        "the word was not replaced by the candidate"
    );
    // One press of undo takes the whole thing back, not the letters one at
    // a time.
    support::press_control(&mut app, 'z');
    assert_eq!(text(&app), "fn main() {\n    pu\n}\n");
}

/// The picture on a row is the one thing that separates two candidates a
/// colour cannot: a module and a keyword are both keyword-coloured.
#[test]
fn the_picture_on_a_row_says_what_the_candidate_is() {
    let (_scratch, mut app) = editing("complete-pictures", "fn main() {\n    p\n}\n");
    support::press(&mut app, crossterm::event::KeyCode::Down);
    support::press(&mut app, crossterm::event::KeyCode::End);
    app.complete_for_test(json!([
        { "label": "path", "kind": 9 },
        { "label": "pub", "kind": 14 },
    ]));

    let dump = support::render(&mut app, 60, 16);
    let rows: Vec<&str> = support::text_block(&dump).lines().collect();
    let picture = |name: &str| {
        rows.iter()
            .find(|row| row.contains(name))
            .and_then(|row| row.chars().find(|cell| *cell as u32 >= 0xf0000))
            .unwrap_or_else(|| panic!("{name} has no picture:\n{dump}"))
    };
    assert_ne!(
        picture("path"),
        picture("pub"),
        "a module and a keyword are drawn alike:\n{dump}"
    );
}

/// A server that says what to replace is obeyed: it knows things obelus
/// does not, like whether the dot before the word is part of what is being
/// completed.
#[test]
fn the_servers_own_range_is_what_is_replaced() {
    let (_scratch, mut app) = editing("complete-range", "fn main() {\n    x.pu\n}\n");
    support::press(&mut app, crossterm::event::KeyCode::Down);
    support::press(&mut app, crossterm::event::KeyCode::End);
    app.complete_for_test(json!([{
        "label": "push_str",
        "kind": 2,
        // From the dot, not from the word: the whole call is replaced.
        "textEdit": {
            "range": { "start": { "line": 1, "character": 5 },
                       "end": { "line": 1, "character": 8 } },
            "newText": ".push_str()"
        }
    }]));
    support::press(&mut app, crossterm::event::KeyCode::Enter);
    assert_eq!(text(&app), "fn main() {\n    x.push_str()\n}\n");
}

/// An import arriving with a candidate goes in with it, and comes back out
/// with it: two edits, one act, one press of undo.
#[test]
fn an_import_goes_in_with_the_candidate_and_comes_back_with_it() {
    let (_scratch, mut app) = editing(
        "complete-import",
        "use std::fmt;\n\nfn main() {\n    Pat\n}\n",
    );
    for _ in 0..3 {
        support::press(&mut app, crossterm::event::KeyCode::Down);
    }
    support::press(&mut app, crossterm::event::KeyCode::End);
    app.complete_for_test(json!([{
        "label": "PathBuf",
        "kind": 22,
        "additionalTextEdits": [{
            "range": { "start": { "line": 1, "character": 0 },
                       "end": { "line": 1, "character": 0 } },
            "newText": "use std::path::PathBuf;\n"
        }]
    }]));
    support::press(&mut app, crossterm::event::KeyCode::Enter);
    assert_eq!(
        text(&app),
        "use std::fmt;\nuse std::path::PathBuf;\n\nfn main() {\n    PathBuf\n}\n",
        "the import did not go in with the candidate"
    );
    // Where the reader is left is after the word, not in the import that
    // pushed it down.
    let cursor = app.current_buffer().expect("a buffer").cursor();
    assert_eq!(
        (cursor.line.get(), cursor.column.get()),
        (4, 11),
        "the cursor was left where the file used to be"
    );

    support::press_control(&mut app, 'z');
    assert_eq!(
        text(&app),
        "use std::fmt;\n\nfn main() {\n    Pat\n}\n",
        "undo took back one of the two edits"
    );
}

/// A snippet is a shape with holes in it, and `tab` is how a reader goes
/// through them.
#[test]
fn a_snippet_leaves_the_reader_in_its_holes() {
    let (_scratch, mut app) = editing("complete-snippet", "fn main() {\n    pri\n}\n");
    support::press(&mut app, crossterm::event::KeyCode::Down);
    support::press(&mut app, crossterm::event::KeyCode::End);
    app.complete_for_test(json!([{
        "label": "println!",
        "kind": 15,
        "insertText": "println!(\"$1\", $2)$0",
        "insertTextFormat": 2
    }]));
    support::press(&mut app, crossterm::event::KeyCode::Enter);
    assert_eq!(
        text(&app),
        "fn main() {\n    println!(\"\", )\n}\n",
        "the placeholders were put in as they were written"
    );
    assert!(app.filling_for_test(), "the snippet's holes were forgotten");

    // In the first hole, so typing goes between the quotes.
    support::type_text(&mut app, "hi");
    assert_eq!(text(&app), "fn main() {\n    println!(\"hi\", )\n}\n");

    // And the next hole is past what was just typed, not where it was
    // before the text moved.
    support::press(&mut app, crossterm::event::KeyCode::Tab);
    support::type_text(&mut app, "x");
    assert_eq!(text(&app), "fn main() {\n    println!(\"hi\", x)\n}\n");

    // The last stop is where the snippet says to end up, after which `tab`
    // is an indent again.
    support::press(&mut app, crossterm::event::KeyCode::Tab);
    let cursor = app.current_buffer().expect("a buffer").cursor();
    assert_eq!((cursor.line.get(), cursor.column.get()), (1, 21));
}

/// The panel is about the word the cursor is in, so it goes as soon as the
/// cursor is somewhere else.
#[test]
fn the_panel_goes_when_the_reader_leaves_the_word() {
    let (_scratch, mut app) = editing("complete-leave", "fn main() {\n    pu\n}\n");
    support::press(&mut app, crossterm::event::KeyCode::Down);
    support::press(&mut app, crossterm::event::KeyCode::End);
    app.complete_for_test(labels(&["push_str", "pop"]));
    assert!(
        support::render(&mut app, 60, 16).contains("push_str"),
        "the panel never opened"
    );

    // Not an arrow, which belongs to the list while it is up: the keys
    // that move the cursor out from under it are the ones the panel does
    // not want. And not a bare `home` either -- that lands on the first
    // thing written on the line, which here is the word being completed.
    support::press_control_key(&mut app, crossterm::event::KeyCode::Home);
    let dump = support::render(&mut app, 60, 16);
    assert!(
        !dump.contains("push_str"),
        "the panel stayed open over a cursor that had left:\n{dump}"
    );
}

/// Typing narrows the list rather than asking again: a server that sent
/// everything it had said so, and the letters since are a query.
#[test]
fn typing_narrows_what_is_showing() {
    let (_scratch, mut app) = editing("complete-narrow", "fn main() {\n    p\n}\n");
    support::press(&mut app, crossterm::event::KeyCode::Down);
    support::press(&mut app, crossterm::event::KeyCode::End);
    app.complete_for_test(labels(&["push_str", "pop", "parse"]));

    support::type_text(&mut app, "o");
    let dump = support::render(&mut app, 60, 16);
    assert!(dump.contains("pop"), "the match went missing:\n{dump}");
    assert!(
        !dump.contains("parse"),
        "a candidate that no longer matches is still listed:\n{dump}"
    );

    // And the selection is the first row of the narrowed list, not
    // whichever row the old selection's number now lands on.
    let (_scratch, mut app) = editing("complete-narrowed-pick", "fn main() {\n    p\n}\n");
    support::press(&mut app, crossterm::event::KeyCode::Down);
    support::press(&mut app, crossterm::event::KeyCode::End);
    // Four, so that the narrowed list is still long enough for the old
    // row number to name a different candidate.
    app.complete_for_test(labels(&["parse", "pop", "push", "pull"]));
    support::press(&mut app, crossterm::event::KeyCode::Down);
    support::type_text(&mut app, "u");
    assert_eq!(
        text(&app),
        "fn main() {\n    pu\n}\n",
        "the letter did not reach the document"
    );
    support::press(&mut app, crossterm::event::KeyCode::Enter);
    assert_eq!(
        text(&app),
        "fn main() {\n    pull\n}\n",
        "enter took a row the narrowed list does not start with"
    );

    // Narrowed to nothing is not a panel with nothing in it.
    support::type_text(&mut app, "zz");
    let dump = support::render(&mut app, 60, 16);
    assert!(
        !dump.contains(support::PANEL_CORNER),
        "an empty panel is still on screen:\n{dump}"
    );
}

/// The worst thing this feature can do is cover code somebody has finished
/// writing with an answer to a question they have stopped asking.
#[test]
fn an_answer_the_reader_has_typed_past_is_dropped() {
    let (_scratch, mut app) = editing("complete-late", "fn main() {\n    pu();\n}\n");
    support::press(&mut app, crossterm::event::KeyCode::Down);
    support::press(&mut app, crossterm::event::KeyCode::End);
    // Asked when the word started at column 4; the cursor is past the
    // brackets and the semicolon by the time the answer lands.
    app.complete_late_for_test(labels(&["push_str", "pop"]), 1, 4);
    let dump = support::render(&mut app, 60, 16);
    assert!(
        !dump.contains("push_str"),
        "a late answer opened a panel over finished code:\n{dump}"
    );

    // A space is enough: what stands between the word's start and the
    // cursor has to be the word itself, not a query that happens to match.
    let (_scratch, mut app) = editing("complete-late-words", "fn main() {\n    pu str\n}\n");
    support::press(&mut app, crossterm::event::KeyCode::Down);
    support::press(&mut app, crossterm::event::KeyCode::End);
    app.complete_late_for_test(labels(&["push_str", "pop"]), 1, 4);
    let dump = support::render(&mut app, 60, 16);
    assert!(
        !dump.contains("push_str"),
        "a late answer opened a panel over two words:\n{dump}"
    );

    // And the other way of having walked away: the cursor is in a word
    // again, but not in *that* word.
    let (_scratch, mut app) = editing("complete-late-line", "fn main() {\n    pu\n    other\n}\n");
    for _ in 0..2 {
        support::press(&mut app, crossterm::event::KeyCode::Down);
    }
    support::press(&mut app, crossterm::event::KeyCode::End);
    app.complete_late_for_test(labels(&["push_str", "pop"]), 1, 4);
    let dump = support::render(&mut app, 60, 16);
    assert!(
        !dump.contains("push_str"),
        "a late answer opened a panel about the line above:\n{dump}"
    );
}

/// Escape closes it and leaves what has been typed.
#[test]
fn escape_closes_the_panel_and_keeps_the_word() {
    let (_scratch, mut app) = editing("complete-escape", "fn main() {\n    pu\n}\n");
    support::press(&mut app, crossterm::event::KeyCode::Down);
    support::press(&mut app, crossterm::event::KeyCode::End);
    app.complete_for_test(labels(&["push_str", "pop"]));
    support::press(&mut app, crossterm::event::KeyCode::Esc);

    let dump = support::render(&mut app, 60, 16);
    assert!(
        !dump.contains("push_str"),
        "escape left the panel up:\n{dump}"
    );
    assert_eq!(text(&app), "fn main() {\n    pu\n}\n");
}

/// What a resolve adds is what the panel shows: most servers send the
/// documentation only when asked about one candidate.
#[test]
fn documentation_that_arrives_late_is_shown() {
    let (_scratch, mut app) = editing("complete-docs", "fn main() {\n    pu\n}\n");
    support::press(&mut app, crossterm::event::KeyCode::Down);
    support::press(&mut app, crossterm::event::KeyCode::End);
    app.complete_for_test(json!([{
        "label": "push_str",
        "kind": 2,
        "detail": "fn(&mut self, string: &str)"
    }]));
    let dump = support::render(&mut app, 60, 16);
    assert!(
        !dump.contains("Appends"),
        "documentation nobody sent is on screen:\n{dump}"
    );

    app.resolve_for_test(
        0,
        json!({
            "label": "push_str",
            "documentation": { "kind": "markdown", "value": "Appends a slice." }
        }),
    );
    let dump = support::render(&mut app, 60, 16);
    assert!(
        dump.contains("Appends a slice."),
        "what the resolve added is not on screen:\n{dump}"
    );
}

/// With no room under the cursor the panel hangs above it. The list is
/// always the half against the cursor, so the documentation goes over the
/// top of it rather than between the two.
#[test]
fn the_panel_hangs_above_the_cursor_when_it_has_to() {
    let mut source = String::new();
    for _ in 0..12 {
        source.push_str("//\n");
    }
    // No newline at the end, so the document ends in the word and
    // `ctrl+end` lands in it.
    source.push_str("    pu");
    let (_scratch, mut app) = editing("complete-above", &source);
    support::press_control_key(&mut app, crossterm::event::KeyCode::End);
    app.complete_for_test(labels(&["push_str", "pop"]));

    let dump = support::render(&mut app, 60, 16);
    let rows: Vec<&str> = support::text_block(&dump).lines().collect();
    let word = rows
        .iter()
        .position(|row| row.contains("    pu"))
        .unwrap_or_else(|| panic!("the word is not on screen:\n{dump}"));
    let panel = rows
        .iter()
        .position(|row| row.contains(support::PANEL_CORNER))
        .unwrap_or_else(|| panic!("no panel:\n{dump}"));
    assert!(
        panel < word,
        "the panel was drawn off the bottom of the screen:\n{dump}"
    );
}

/// The paging keys go to the half that typing cannot narrow.
#[test]
fn the_paging_keys_scroll_the_documentation() {
    let (_scratch, mut app) = editing("complete-page", "fn main() {\n    pu\n}\n");
    support::press(&mut app, crossterm::event::KeyCode::Down);
    support::press(&mut app, crossterm::event::KeyCode::End);
    let long: String = (1..=20).map(|n| format!("line number {n}\n\n")).collect();
    app.complete_for_test(json!([{
        "label": "push_str",
        "kind": 2,
        "documentation": { "kind": "markdown", "value": long }
    }]));
    let first = documentation_top(&support::render(&mut app, 60, 16));
    assert!(
        first.contains("line number 1"),
        "the documentation does not start at the top: {first:?}"
    );

    support::press(&mut app, crossterm::event::KeyCode::PageDown);
    let paged = documentation_top(&support::render(&mut app, 60, 16));
    assert_ne!(
        paged, first,
        "the paging key moved something other than the documentation"
    );
    assert!(
        paged.contains("line number"),
        "the documentation paged past its own end: {paged:?}"
    );

    // And back.
    support::press(&mut app, crossterm::event::KeyCode::PageUp);
    assert_eq!(
        documentation_top(&support::render(&mut app, 60, 16)),
        first,
        "paging back did not come back"
    );
}

/// The first row of the documentation half, which is the row under the line
/// that divides the panel.
fn documentation_top(dump: &str) -> String {
    let rows: Vec<&str> = support::text_block(dump).lines().collect();
    let divider = rows
        .iter()
        .position(|row| row.contains('\u{251c}'))
        .unwrap_or_else(|| panic!("the panel has no documentation half:\n{dump}"));
    rows[divider + 1].trim().to_string()
}

/// The snippet syntax, which is small and has three rules worth pinning.
mod snippets {
    use obelus::lsp::snippet::parse;

    #[test]
    fn the_holes_are_left_in_tab_order() {
        // `$2` is written first and is still the second stop, and `$0` is
        // last however early it appears.
        let parsed = parse("a$0b$2c$1d");
        assert_eq!(parsed.text, "abcd");
        assert_eq!(parsed.stops, vec![(3, 3), (2, 2), (1, 1)]);
    }

    #[test]
    fn a_default_is_text_and_a_hole_at_once() {
        let parsed = parse("for ${1:item} in ${2:items} {}");
        assert_eq!(parsed.text, "for item in items {}");
        assert_eq!(parsed.stops, vec![(4, 8), (12, 17)]);
    }

    #[test]
    fn a_variable_expands_to_nothing_and_an_escape_to_itself() {
        // Nothing obelus can honestly fill in, so it fills in nothing --
        // and the text around it survives either way.
        assert_eq!(parse("$TM_FILENAME:$1").text, ":");
        assert_eq!(parse("cost: \\$5").text, "cost: $5");
        assert!(parse("cost: \\$5").stops.is_empty());
    }
}

/// A list takes every key while it is open, so a panel under one would be
/// a panel nothing can reach -- drawn over the thing the reader is using.
#[test]
fn a_list_opened_over_the_file_takes_the_panel_away() {
    let (_scratch, mut app) = editing("complete-under-list", "fn main() {\n    pu\n}\n");
    support::press(&mut app, crossterm::event::KeyCode::Down);
    support::press(&mut app, crossterm::event::KeyCode::End);
    app.complete_for_test(labels(&["push_str", "pop"]));
    assert!(
        support::render(&mut app, 60, 16).contains("push_str"),
        "the panel never opened"
    );

    obelus::app::dispatch::dispatch(&mut app, obelus::command::Command::CommandPalette);
    let dump = support::render(&mut app, 60, 16);
    assert!(
        !dump.contains("push_str"),
        "the panel is still drawn under a list:\n{dump}"
    );
}

/// Against a real server, because the two rules below are the only ones
/// obelus cannot state by itself: which punctuation is worth asking about
/// is the server's to say, and there is no way to find out what it says
/// without asking it.
///
/// Ignored by default: it starts rust-analyzer and waits for it to read the
/// project. Run with `cargo test -- --ignored`.
mod against_a_real_server {
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };

    use obelus::{app::App, buffer::Buffer, event::Event, lsp::ServerState};

    use crate::{support, text};

    /// Long enough for a cold start on a slow machine.
    const READY: Duration = Duration::from_secs(180);

    /// Long enough for one answer from a server that is already answering.
    const ANSWER: Duration = Duration::from_secs(30);

    /// An application over this repository, with a server running for it.
    ///
    /// The repository's own file rather than one the test writes: a file
    /// outside the crate's module tree is a file rust-analyzer has nothing
    /// to say about. Nothing is ever written back -- the buffer is edited
    /// in memory, which is what the server is told about.
    fn served() -> Option<(App, mpsc::Receiver<Event>)> {
        if !obelus::lsp::on_path("rust-analyzer") {
            return None;
        }
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut app = App::new(vec![
            Buffer::open(&root.join("src/jump.rs")).expect("opening it"),
        ]);
        app.working_directory_for_test(root);
        let (sender, events) = mpsc::channel();
        app.events_for_test(sender);
        app.serve_for_test();
        support::lay_out(&mut app, 80, 24);
        Some((app, events))
    }

    /// Pumps what the server says into the application until `done`.
    fn pump<F>(app: &mut App, events: &mpsc::Receiver<Event>, limit: Duration, mut done: F) -> bool
    where
        F: FnMut(&mut App) -> bool,
    {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            if done(app) {
                return true;
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            match events.recv_timeout(remaining.min(Duration::from_millis(250))) {
                Ok(event) => app.handle(event),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            // The frame is where a panel is settled, and a test that never
            // drew one would be testing an application nobody is looking at.
            support::lay_out(app, 80, 24);
        }
        done(app)
    }

    /// Leaves the cursor on a line of its own with `self` typed on it, and
    /// the server known to be answering questions about that place.
    ///
    /// Asking until an answer comes is what obelus itself does: a server
    /// that has not finished reading the project answers a question with
    /// nothing, which is the same answer a question with no answer gets.
    /// Everything below is about what a *key* asks, so the asking has to be
    /// known to work before a key is pressed.
    fn warm(app: &mut App, events: &mpsc::Receiver<Event>) {
        assert!(
            pump(app, events, READY, |app| {
                matches!(app.server_state(), Some((_, ServerState::Ready)))
            }),
            "rust-analyzer never finished its handshake"
        );

        // A line of its own after a statement with a receiver on it, so
        // that what follows a dot is that type's own fields and methods.
        let line = app
            .current_buffer()
            .expect("a buffer")
            .text()
            .rope()
            .to_string()
            .lines()
            .position(|line| line.trim_end().ends_with("self.entries.truncate(self.at);"))
            .expect("a line to type after");
        for _ in 0..line {
            support::press(app, crossterm::event::KeyCode::Down);
        }
        support::press(app, crossterm::event::KeyCode::End);
        support::press(app, crossterm::event::KeyCode::Enter);
        support::type_text(app, "self");

        let deadline = Instant::now() + READY;
        while Instant::now() < deadline {
            obelus::app::dispatch::dispatch(app, obelus::command::Command::SymbolComplete);
            if pump(app, events, Duration::from_secs(3), |app| {
                app.completion().is_some()
            }) {
                support::press(app, crossterm::event::KeyCode::Esc);
                assert!(app.completion().is_none(), "escape left the panel up");
                return;
            }
        }
        panic!("rust-analyzer never answered a question about this file");
    }

    /// Typing `.` asks a question, because rust-analyzer says that
    /// character is one.
    #[test]
    #[ignore = "starts a server and waits for the project to be read"]
    fn the_punctuation_a_server_names_asks_by_itself() {
        let Some((mut app, events)) = served() else {
            return;
        };
        warm(&mut app, &events);

        support::type_text(&mut app, ".");
        assert!(
            pump(&mut app, &events, ANSWER, |app| app.completion().is_some()),
            "typing a dot asked nothing, or the answer was dropped"
        );
        assert!(
            app.completion().expect("a panel").count() > 0,
            "the panel is up with nothing in it"
        );
    }

    /// A question nobody wants the answer to is taken back.
    ///
    /// A reader typing a word asks for a completion per letter. Without
    /// this the server computes every one of them in full, and the ones
    /// that are never answered sit in obelus's table for the rest of the
    /// session.
    #[test]
    #[ignore = "starts a server and waits for the project to be read"]
    fn a_question_that_has_been_superseded_is_taken_back() {
        let Some((mut app, events)) = served() else {
            return;
        };
        warm(&mut app, &events);
        support::press(&mut app, crossterm::event::KeyCode::Down);
        support::press(&mut app, crossterm::event::KeyCode::Home);

        // A word, typed fast: one question per letter, and only the last
        // of them is worth an answer.
        support::type_text(&mut app, "self.entr");
        assert!(
            app.outstanding_for_test() <= 2,
            "{} questions are still out, which is one per letter",
            app.outstanding_for_test()
        );

        // And the last one is answered, so the typing still completes.
        assert!(
            pump(&mut app, &events, ANSWER, |app| app.completion().is_some()),
            "the question that was not taken back went unanswered too"
        );
    }

    /// A file changing on disk is news to the server as much as to
    /// obelus: a branch checked out under it, a build script's output, an
    /// editor somewhere else.
    #[test]
    #[ignore = "starts a server and waits for the project to be read"]
    fn a_file_that_changed_on_disk_is_reported() {
        let Some((mut app, events)) = served() else {
            return;
        };
        assert!(
            pump(&mut app, &events, READY, |app| {
                matches!(app.server_state(), Some((_, ServerState::Ready)))
            }),
            "the server never started"
        );

        let before = app.told_servers_for_test();
        app.handle(Event::FileChanged {
            path: std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs"),
        });
        assert_eq!(
            app.told_servers_for_test(),
            before + 1,
            "the server was not told the file changed"
        );
    }

    /// The pointer resting on a word asks what it is, which is the one
    /// thing in obelus that happens because the reader did nothing.
    #[test]
    #[ignore = "starts a server and waits for the project to be read"]
    fn the_pointer_resting_on_a_word_asks_what_it_is() {
        use obelus::event::Pointer;

        let Some((mut app, events)) = served() else {
            return;
        };
        warm(&mut app, &events);
        // Off the word `warm` left behind: a question about it is still in
        // flight, and an answer landing later would put a list of
        // candidates over the answer this test is about. Moving the caret
        // away is what makes that answer be dropped.
        support::press(&mut app, crossterm::event::KeyCode::Down);
        support::press(&mut app, crossterm::event::KeyCode::Home);
        support::press(&mut app, crossterm::event::KeyCode::Esc);

        // Over a word of the file itself, found on screen rather than
        // counted: what the pointer is over is a cell, and which cell that
        // is depends on the gutter.
        let dump = support::render(&mut app, 80, 24);
        let rows: Vec<String> = support::text_block(&dump)
            .lines()
            .filter_map(|row| row.split_once('|').map(|(_, cells)| cells.to_string()))
            .collect();
        let (y, x) = rows
            .iter()
            .enumerate()
            .find_map(|(y, row)| row.find("JumpList").map(|x| (y, x)))
            .unwrap_or_else(|| panic!("no name to point at:\n{dump}"));
        app.handle(Event::Pointer {
            kind: Pointer::Moved,
            x: u16::try_from(x).expect("a column") + 2,
            y: u16::try_from(y).expect("a row"),
        });

        assert!(
            pump(&mut app, &events, ANSWER, |app| app.hover().is_some()),
            "the pointer rested on a name and nothing was asked"
        );
        // And the other half of the same question: where else that name
        // is. It is marked in the text rather than shown in the panel, so
        // it is asked for separately and arrives separately.
        assert!(
            pump(&mut app, &events, ANSWER, |app| {
                app.uses_marked_for_test() > 0
            }),
            "the pointer rested on a name and its other uses were not marked"
        );
        // Reaching for the answer with the pointer is not leaving it:
        // every cell on the way is a pointer that has left the word, and
        // an answer that vanished as you moved towards it could not be
        // read to the end.
        let panel = obelus::ui::hover::layout(&app, app.editor_area_for_test())
            .expect("the answer is drawn somewhere");
        app.handle(Event::Pointer {
            kind: Pointer::Moved,
            x: panel.x + 2,
            y: panel.y + 1,
        });
        support::lay_out(&mut app, 80, 24);
        assert!(
            app.hover().is_some(),
            "the answer went away as the pointer reached for it"
        );

        // And moving off it altogether takes it away.
        app.handle(Event::Pointer {
            kind: Pointer::Moved,
            x: 1,
            y: u16::try_from(y).expect("a row"),
        });
        assert!(
            app.hover().is_none(),
            "the answer stayed after the pointer left the word"
        );
    }

    /// A candidate that ends in that punctuation asks the next question as
    /// soon as it goes in: `std::` is the start of a name, not one.
    #[test]
    #[ignore = "starts a server and waits for the project to be read"]
    fn a_candidate_that_ends_in_one_asks_again() {
        let Some((mut app, events)) = served() else {
            return;
        };
        warm(&mut app, &events);
        // The two letters the candidate below is an answer to, in place of
        // the word `warm` left behind.
        for _ in 0..4 {
            support::press(&mut app, crossterm::event::KeyCode::Backspace);
        }
        support::type_text(&mut app, "st");
        // Let what the server says about `st` arrive and be done with, so
        // that what is on screen next is the test's own answer and nothing
        // else.
        pump(&mut app, &events, Duration::from_secs(3), |_| false);

        // The answer is the test's own, so that what is accepted is known:
        // what is being tested is what obelus does with a candidate whose
        // text ends in a trigger, not which candidates a server sends.
        app.complete_for_test(serde_json::json!([{
            "label": "std::", "kind": 9, "insertText": "std::"
        }]));
        assert!(app.completion().is_some(), "the panel did not open");
        support::press(&mut app, crossterm::event::KeyCode::Enter);
        assert!(text(&app).contains("std::"), "the candidate did not go in");

        assert!(
            pump(&mut app, &events, ANSWER, |app| app.completion().is_some()),
            "accepting `std::` asked nothing more"
        );
        let panel = app.completion().expect("a panel");
        assert!(
            panel.count() > 0 && panel.query().is_empty(),
            "the panel that came back is about the word before the `::`"
        );
    }
}

/// What the call the cursor is inside takes, which is the question after
/// "what could be typed": the name is chosen and the arguments are not.
mod signatures {
    use serde_json::json;

    use super::{editing, support};

    /// One signature, in the shape rust-analyzer sends: offsets into the
    /// label rather than the parameter's own text.
    fn answered(active: u32) -> serde_json::Value {
        json!({
            "signatures": [{
                "label": "fn push_str(&mut self, string: &str)",
                "parameters": [
                    { "label": [12, 21] },
                    { "label": [23, 35] }
                ],
                "activeParameter": active
            }],
            "activeSignature": 0
        })
    }

    #[test]
    fn the_call_is_shown_with_the_argument_being_typed_marked() {
        let (_scratch, mut app) = editing("signature-shown", "fn main() {\n    push_str(\n}\n");
        support::press(&mut app, crossterm::event::KeyCode::Down);
        support::press(&mut app, crossterm::event::KeyCode::End);
        app.signature_for_test(answered(1));

        let dump = support::render(&mut app, 60, 16);
        assert!(
            dump.contains("fn push_str(&mut self, string: &str)"),
            "the signature is not on screen:\n{dump}"
        );

        // The argument being typed is drawn differently from the rest of
        // the line, which is the whole of what the panel is for.
        let rows: Vec<&str> = support::text_block(&dump).lines().collect();
        let styles: Vec<&str> = support::style_block(&dump).lines().collect();
        let at = rows
            .iter()
            .position(|row| row.contains("fn push_str(&mut"))
            .expect("the signature");
        let row = rows[at];
        let cells = &styles[at][row.find('|').expect("a divider") + 1..];
        let text = &row[row.find('|').expect("a divider") + 1..];
        let second = text.find("string: &str").expect("the second argument");
        let first = text.find("&mut self").expect("the first");
        assert_ne!(
            cells.chars().nth(second),
            cells.chars().nth(first),
            "the argument being typed is drawn as the rest of the line:\n{dump}"
        );
    }

    /// A parameter given as text rather than as offsets, which is the
    /// protocol's other shape.
    #[test]
    fn a_parameter_named_by_its_text_is_found_in_the_label() {
        use obelus::lsp::signature::in_reply;

        let found = in_reply(&Ok(json!({
            "signatures": [{
                "label": "def greet(name, loud=False)",
                "parameters": [{ "label": "name" }, { "label": "loud=False" }],
            }],
            "activeParameter": 1
        })))
        .expect("a signature");
        let (from, to) = found.active.expect("an active parameter");
        assert_eq!(&found.label[from..to], "loud=False");
    }

    /// A closing bracket ends the call, so it ends the panel.
    #[test]
    fn the_panel_goes_when_the_call_does() {
        let (_scratch, mut app) = editing("signature-closed", "fn main() {\n    push_str(\n}\n");
        support::press(&mut app, crossterm::event::KeyCode::Down);
        support::press(&mut app, crossterm::event::KeyCode::End);
        app.signature_for_test(answered(0));
        assert!(app.signature().is_some(), "the panel never opened");

        support::type_text(&mut app, ")");
        assert!(
            app.signature().is_none(),
            "the panel stayed up after the call was closed"
        );
    }
}
