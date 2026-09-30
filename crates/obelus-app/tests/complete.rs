//! What a server offers to type next.
//!
//! Against values rather than against a server: what is interesting is what
//! Obelus does with an answer -- which word it thinks the answer is about,
//! what it puts in, and what it does when the reader has gone on typing --
//! and a server cannot be made to answer late on demand.

mod support;

use obelus_app::app::App;
use obelus_buffer::Buffer;
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

/// A server that says what to replace is obeyed: it knows things Obelus
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
    use obelus_lsp::snippet::parse;

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
        // Nothing Obelus can honestly fill in, so it fills in nothing --
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

    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::CommandPalette);
    let dump = support::render(&mut app, 60, 16);
    assert!(
        !dump.contains("push_str"),
        "the panel is still drawn under a list:\n{dump}"
    );
}

/// Against a real server, because the two rules below are the only ones
/// Obelus cannot state by itself: which punctuation is worth asking about
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

    use obelus_app::{app::App, event::Event};
    use obelus_buffer::Buffer;
    use obelus_lsp::ServerState;

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
        if !obelus_lsp::on_path("rust-analyzer") {
            return None;
        }
        let root = std::path::PathBuf::from(env!("OBELUS_TREE"));
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
    /// Asking until an answer comes is what Obelus itself does: a server
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
            obelus_app::app::dispatch::dispatch(app, obelus_command::Command::SymbolComplete);
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
    /// that are never answered sit in Obelus's table for the rest of the
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
    /// Obelus: a branch checked out under it, a build script's output, an
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
        app.handle(Event::Watched(obelus_watch::Changed {
            path: std::path::PathBuf::from(env!("OBELUS_TREE"))
                .join("crates/obelus-app/src/lib.rs"),
        }));
        assert_eq!(
            app.told_servers_for_test(),
            before + 1,
            "the server was not told the file changed"
        );
    }

    /// The pointer resting on a word asks what it is, which is the one
    /// thing in Obelus that happens because the reader did nothing.
    #[test]
    #[ignore = "starts a server and waits for the project to be read"]
    fn the_pointer_resting_on_a_word_asks_what_it_is() {
        use obelus_app::event::Pointer;

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
        let panel = obelus_ui::hover::layout(&app, app.editor_area_for_test())
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
        // what is being tested is what Obelus does with a candidate whose
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
    use obelus_app::app::App;
    use obelus_buffer::Buffer;
    use serde_json::json;

    use super::{editing, support, text};

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
        use obelus_lsp::signature::in_reply;

        let found = in_reply(&Ok(json!({
            "signatures": [{
                "label": "def greet(name, loud=False)",
                "parameters": [{ "label": "name" }, { "label": "loud=False" }],
            }],
            "activeParameter": 1
        })))
        .expect("a signature");
        let signature = found.signatures.first().expect("the active signature");
        let (from, to) = signature.active.expect("an active parameter");
        assert_eq!(&signature.label[from..to], "loud=False");
    }

    /// Several signatures, the one in use first and the rest under it.
    ///
    /// A name with overloads is a name the reader has to choose between,
    /// and a panel drawing one of them says there is one. Broken
    /// deliberately by keeping only `signatures[activeSignature]`, which is
    /// what `in_reply` did: the two labels the server also sent are then
    /// nowhere on screen, and so is the count of the ones that did not fit.
    #[test]
    fn every_signature_is_shown_with_the_one_in_use_first() {
        let (_scratch, mut app) = editing("signature-many", "fn main() {\n    write(\n}\n");
        support::press(&mut app, crossterm::event::KeyCode::Down);
        support::press(&mut app, crossterm::event::KeyCode::End);
        let overloads: Vec<serde_json::Value> = (0..7)
            .map(|which| {
                json!({
                    "label": format!("fn write(n{which}: u{which}2)"),
                    "parameters": [{ "label": [9, 15] }],
                })
            })
            .collect();
        app.signature_for_test(json!({
            "signatures": overloads,
            "activeSignature": 2,
            "activeParameter": 0
        }));

        let dump = support::render(&mut app, 60, 16);
        let rows: Vec<&str> = support::text_block(&dump).lines().collect();
        let at = rows
            .iter()
            .position(|row| row.contains("fn write(n2"))
            .expect("the signature the cursor is in");
        assert!(
            rows[at + 1].contains("fn write(n0"),
            "the one in use is not first, or the others are not shown:\n{dump}"
        );
        // Five of seven, so two are owed a count.
        assert!(
            dump.contains("+2"),
            "nothing says how many did not fit:\n{dump}"
        );
        assert!(
            !dump.contains("fn write(n5"),
            "more than the cap was drawn:\n{dump}"
        );
    }

    /// What the argument being typed is for, under a rule.
    ///
    /// The parameter's own documentation where it has one: the reader is
    /// filling in that argument, and the signature's own prose is the
    /// answer to a question the label above has already answered. Broken
    /// deliberately by taking the signature's first, which puts the wrong
    /// half of the answer on the screen.
    #[test]
    fn the_documentation_is_the_one_the_argument_has() {
        let (_scratch, mut app) = editing("signature-doc", "fn main() {\n    push_str(\n}\n");
        support::press(&mut app, crossterm::event::KeyCode::Down);
        support::press(&mut app, crossterm::event::KeyCode::End);
        app.signature_for_test(json!({
            "signatures": [{
                "label": "fn push_str(&mut self, string: &str)",
                "documentation": "About the whole function",
                "parameters": [
                    { "label": [12, 21] },
                    {
                        "label": [23, 35],
                        "documentation": "The slice to append"
                    }
                ],
                "activeParameter": 1
            }],
            "activeSignature": 0
        }));

        let dump = support::render(&mut app, 60, 16);
        assert!(
            dump.contains("The slice to append"),
            "what the argument is for is not on screen:\n{dump}"
        );
        assert!(
            !dump.contains("About the whole function"),
            "the signature's own prose won over the argument's:\n{dump}"
        );

        // And where the argument says nothing, the signature's is what is
        // left to say.
        app.signature_for_test(json!({
            "signatures": [{
                "label": "fn push_str(&mut self, string: &str)",
                "documentation": "About the whole function",
                "parameters": [{ "label": [23, 35] }],
                "activeParameter": 0
            }],
            "activeSignature": 0
        }));
        let dump = support::render(&mut app, 60, 16);
        assert!(
            dump.contains("About the whole function"),
            "nothing is said about the call at all:\n{dump}"
        );
    }

    /// A parameter nothing is on is not the first parameter.
    ///
    /// `null` is the protocol's way of saying "none of them" and an absent
    /// field falls through to the whole answer's -- two things `Option<u32>`
    /// cannot tell apart, which is why this is read off the answer as it
    /// arrived. Broken deliberately by going back to
    /// `signature.active_parameter.or(help.active_parameter).unwrap_or(0)`,
    /// which marks `&mut self` on a call whose cursor is past the last
    /// argument.
    #[test]
    fn a_parameter_the_server_says_is_none_marks_nothing() {
        use obelus_lsp::signature::in_reply;

        let said = |parameter: serde_json::Value| {
            json!({
                "signatures": [{
                    "label": "fn push_str(&mut self, string: &str)",
                    "parameters": [{ "label": [12, 21] }, { "label": [23, 35] }],
                    "activeParameter": parameter
                }],
                "activeParameter": 0,
                "activeSignature": 0
            })
        };

        let none = in_reply(&Ok(said(serde_json::Value::Null))).expect("a signature");
        assert_eq!(
            none.signatures[0].active, None,
            "a parameter the server said was none marked one anyway"
        );

        // And a number still means that one, so the reading of `null` has
        // not been bought by losing the ordinary case.
        let second = in_reply(&Ok(said(json!(1)))).expect("a signature");
        assert_eq!(second.signatures[0].active, Some((23, 35)));

        // Absent falls through to the whole answer's, which is the other
        // half of the same rule.
        let absent = in_reply(&Ok(json!({
            "signatures": [{
                "label": "fn push_str(&mut self, string: &str)",
                "parameters": [{ "label": [12, 21] }, { "label": [23, 35] }]
            }],
            "activeParameter": 1,
            "activeSignature": 0
        })))
        .expect("a signature");
        assert_eq!(absent.signatures[0].active, Some((23, 35)));
    }

    /// The reader leaving the line takes the panel with them.
    ///
    /// Asked once a frame, the way the completion panel and the hover are
    /// asked, rather than being told by each of the ways a cursor can move.
    /// Broken deliberately by taking `settle_signature` out of `prepare`:
    /// the panel then sits on a line the reader left, about a call that is
    /// not under them, until they happen to type a bracket.
    #[test]
    fn the_panel_goes_when_the_reader_leaves_the_line() {
        // A file Obelus has no grammar for, which is where the line is
        // still what says whether the reader has left: the call they are in
        // is a question for a tree, and there is not one. The same rule as
        // before for a text file, and no rule at all would be a panel that
        // sat there until something else closed it.
        let scratch = support::Scratch::new("signature-left");
        let path = scratch.path().join("notes.txt");
        std::fs::write(&path, "copy(\nsomewhere else\n").expect("writing the file");
        let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
        app.working_directory_for_test(scratch.path().to_path_buf());
        support::lay_out(&mut app, 60, 16);
        assert!(
            app.current_buffer()
                .is_some_and(|buffer| buffer.syntax().is_none()),
            "the file has a grammar, so this is not the case it is about"
        );

        support::press(&mut app, crossterm::event::KeyCode::End);
        app.signature_for_test(answered(0));
        assert!(app.signature().is_some(), "the panel never opened");

        support::press(&mut app, crossterm::event::KeyCode::Down);
        support::render(&mut app, 60, 16);
        assert!(
            app.signature().is_none(),
            "the panel stayed on a line the reader has left"
        );
    }

    /// And escape gives up on it, like everything else on screen.
    ///
    /// Broken deliberately by taking the arm out of `signature_key`: the
    /// key then falls through to the file, where it clears a selection
    /// nobody made, and the panel stays.
    #[test]
    fn escape_closes_the_panel() {
        let (_scratch, mut app) = editing("signature-escape", "fn main() {\n    push_str(\n}\n");
        support::press(&mut app, crossterm::event::KeyCode::Down);
        support::press(&mut app, crossterm::event::KeyCode::End);
        app.signature_for_test(answered(0));

        support::press(&mut app, crossterm::event::KeyCode::Esc);
        assert!(
            app.signature().is_none(),
            "escape left the panel where it was"
        );
    }

    /// A character the server asks to be re-asked on asks again.
    ///
    /// rust-analyzer's is `)`, and it is not an ending: the call it closes
    /// may be an argument of another one. So the question goes out again,
    /// saying which character asked and that one was already showing --
    /// which is the whole of `SignatureHelpContext`, and what the server
    /// needs to keep the reader on the signature they were looking at.
    ///
    /// Read off the wire, because nothing else can see it: a context built
    /// and not attached to the request looks exactly like one that was.
    /// Broken deliberately three ways -- dropping `"context"` from the
    /// params, going back to `)` closing the panel, and taking
    /// `retrigger_characters` out of the capability.
    #[test]
    fn a_character_the_server_asks_again_on_asks_again() {
        use obelus_syntax::LanguageId;

        let (_scratch, mut app) = editing("signature-retrigger", "fn main() {\n    f(g(\n}\n");
        let (sender, heard) = obelus_app::event::channel();
        app.events_for_test(sender);
        assert!(
            app.stand_in_server_for_test(LanguageId::Rust, "cat"),
            "the echo would not start"
        );
        app.declared_for_test(
            LanguageId::Rust,
            json!({
                "signatureHelpProvider": {
                    "triggerCharacters": ["(", ","],
                    "retriggerCharacters": [")"]
                }
            }),
        );
        support::press(&mut app, crossterm::event::KeyCode::Down);
        support::press(&mut app, crossterm::event::KeyCode::End);

        // A trigger character, which is a fresh question.
        support::type_text(&mut app, "(");
        let asked = support::heard_requests(&heard, "textDocument/signatureHelp", 1);
        assert_eq!(asked.len(), 1, "the trigger asked nobody: {asked:?}");
        assert_eq!(
            asked[0]["params"]["context"]["triggerCharacter"],
            json!("(")
        );
        assert_eq!(asked[0]["params"]["context"]["isRetrigger"], json!(false));

        // An answer, so that there is something showing for the retrigger
        // to be a retrigger of.
        app.signature_for_test(answered(0));
        assert!(app.signature().is_some(), "the panel never opened");

        support::type_text(&mut app, ")");
        let again = support::heard_requests(&heard, "textDocument/signatureHelp", 1);
        assert_eq!(again.len(), 1, "the retrigger asked nobody: {again:?}");
        assert_eq!(again[0]["params"]["context"]["isRetrigger"], json!(true));
        assert_eq!(
            again[0]["params"]["context"]["activeSignatureHelp"]["signatures"][0]["label"],
            json!("fn push_str(&mut self, string: &str)"),
            "the answer that was showing did not go back with the question: {:?}",
            again[0]["params"]["context"]
        );
    }

    /// Somebody else's prose may not take the signature off the screen.
    ///
    /// The box is as tall as what it holds, so a server that sends a
    /// paragraph makes a tall one -- and a tall one fits neither above the
    /// cursor nor below it on a short terminal. Giving up there is the doc
    /// comment winning over the thing it is about. So it is drawn on the
    /// roomier side with as much as that side holds, losing rows from the
    /// bottom, where the least important of them are.
    ///
    /// Broken deliberately by going back to `return None` where it fits
    /// neither side: at twelve rows the panel is still there and at ten it
    /// is gone, which is the cursor's own line deciding whether the reader
    /// gets an answer.
    #[test]
    fn a_long_answer_does_not_take_the_signature_off_the_screen() {
        // Seven rows is the floor of it: the box has one row inside, and it
        // goes on the call the cursor is in rather than on a count of the
        // ones there was no room for.
        for height in [7, 8, 10, 12, 16] {
            let (_scratch, mut app) = editing(
                &format!("signature-tall-{height}"),
                "fn main() {\n    write(\n}\n",
            );
            support::lay_out(&mut app, 50, height);
            support::press(&mut app, crossterm::event::KeyCode::Down);
            support::press(&mut app, crossterm::event::KeyCode::End);
            app.signature_for_test(json!({
                "signatures": [
                    { "label": "fn write(first: &str, second: &str)",
                      "documentation": "A paragraph of documentation long enough to wrap over several rows of a narrow panel, which is what makes the box tall.",
                      "parameters": [{ "label": [9, 20] }, { "label": [22, 34] }],
                      "activeParameter": 0 },
                    { "label": "fn write(n: u64)" },
                    { "label": "fn write(b: bool)" }
                ],
                "activeSignature": 0
            }));
            let dump = support::render(&mut app, 50, height);
            assert!(
                support::text_block(&dump).contains("fn write(first"),
                "at {height} rows the signature is nowhere:\n{dump}"
            );
        }
    }

    /// And a clipped box is as tall as what actually goes in it.
    ///
    /// The prose wants a rule as well as a row, so a box clipped to one row
    /// short of both loses both -- and a height that counted them anyway is
    /// a box with a blank row at the foot. Broken deliberately by taking the
    /// second `plan` out of `layout`, which is the height being measured
    /// from what there was and the filling from what there was room for.
    #[test]
    fn a_clipped_box_has_no_blank_row_in_it() {
        let (_scratch, mut app) = editing("signature-blank", "fn main() {\n    write(\n}\n");
        support::lay_out(&mut app, 46, 10);
        support::press(&mut app, crossterm::event::KeyCode::Down);
        support::press(&mut app, crossterm::event::KeyCode::End);
        app.signature_for_test(json!({
            "signatures": [
                { "label": "fn write(first: &str, second: &str)",
                  "documentation": "A paragraph long enough to wrap over several rows of a narrow panel, which is what makes the box tall.",
                  "parameters": [{ "label": [9, 20] }, { "label": [22, 34] }],
                  "activeParameter": 0 },
                { "label": "fn write(n: u64)" },
                { "label": "fn write(b: bool)" }
            ],
            "activeSignature": 0
        }));

        let dump = support::render(&mut app, 46, 10);
        let blank = support::text_block(&dump).lines().find(|row| {
            let Some(from) = row.find('\u{2502}') else {
                return false;
            };
            let Some(to) = row.rfind('\u{2502}') else {
                return false;
            };
            to > from && row[from + '\u{2502}'.len_utf8()..to].trim().is_empty()
        });
        assert!(
            blank.is_none(),
            "there is a row of the box with nothing in it:\n{dump}"
        );
    }

    /// And the count is about what was left out, not about the cap.
    ///
    /// A box too short for five labels draws the ones that fit, and what it
    /// says about the rest has to be the rest -- `+2` under three of seven
    /// is a panel saying two of them are missing when four are. Broken
    /// deliberately by writing `signature.more()`, which counts only the
    /// ones the cap dropped.
    #[test]
    fn the_count_is_about_what_was_left_out() {
        let (_scratch, mut app) = editing("signature-count", "fn main() {\n    write(\n}\n");
        support::lay_out(&mut app, 50, 9);
        support::press(&mut app, crossterm::event::KeyCode::Down);
        support::press(&mut app, crossterm::event::KeyCode::End);
        let overloads: Vec<serde_json::Value> = (0..7)
            .map(|which| json!({ "label": format!("fn write(n{which}: u8)") }))
            .collect();
        app.signature_for_test(json!({
            "signatures": overloads,
            "activeSignature": 0
        }));

        let dump = support::render(&mut app, 50, 9);
        let rows = support::text_block(&dump);
        let drawn = (0..7)
            .filter(|which| rows.contains(&format!("fn write(n{which}: u8)")))
            .count();
        assert!(
            drawn < 7,
            "the box was not short enough for this to be about anything:\n{dump}"
        );
        assert!(
            rows.contains(&format!("+{}", 7 - drawn)),
            "{drawn} of seven are drawn, so the count should say {}:\n{dump}",
            7 - drawn
        );
    }

    /// The bracket that opens a call does not open a list of the scope.
    ///
    /// rust-analyzer puts `(` in both of its lists -- `["(", ",", "<"]` for
    /// the call and `[":", ".", "'", "("]` for what could be typed -- so
    /// typing one used to ask both, and the panel that won was the one the
    /// reader did not need: `self::`, `crate::`, every macro in scope,
    /// drawn over the call they had just opened, with the signature behind
    /// it until they pressed escape. A character in both lists is the
    /// reader opening a call, and the nearer question there is what the
    /// call takes.
    ///
    /// Read off the wire, because what is being tested is a question that
    /// is *not* asked and nothing on screen can show that. Broken
    /// deliberately by dropping `&& !self.triggers_signature(character)`,
    /// which asks for both again.
    #[test]
    fn the_bracket_that_opens_a_call_does_not_open_the_list() {
        use obelus_syntax::LanguageId;

        let (_scratch, mut app) = editing("signature-not-both", "fn main() {\n    copy\n}\n");
        let (sender, heard) = obelus_app::event::channel();
        app.events_for_test(sender);
        assert!(
            app.stand_in_server_for_test(LanguageId::Rust, "cat"),
            "the echo would not start"
        );
        app.declared_for_test(
            LanguageId::Rust,
            json!({
                "signatureHelpProvider": { "triggerCharacters": ["(", ",", "<"] },
                "completionProvider": { "triggerCharacters": [":", ".", "'", "("] }
            }),
        );
        support::press(&mut app, crossterm::event::KeyCode::Down);
        support::press(&mut app, crossterm::event::KeyCode::End);

        support::type_text(&mut app, "(");

        // Both questions go down one channel, and the completion one -- if
        // it were asked -- would be written first, because that is the order
        // the typing asks them in. So this drains until the call's question
        // arrives and looks at what came before it.
        //
        // Asking the channel for one method and then for the other does not
        // work, and failing to notice that is how this test first shipped
        // vacuous: the reader of it throws away what it is not looking for,
        // so the first call ate the completion request and the second found
        // an empty channel whatever the code did. It passed with the rule
        // taken out.
        let mut before: Vec<String> = Vec::new();
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let left = until.saturating_duration_since(std::time::Instant::now());
            assert!(
                !left.is_zero(),
                "the call was never asked about; before it: {before:?}"
            );
            match heard.recv_timeout(left) {
                Ok(obelus_app::event::Event::Lsp(obelus_lsp::Message { message, .. })) => {
                    let Some(method) = message.get("method").and_then(serde_json::Value::as_str)
                    else {
                        continue;
                    };
                    if method == "textDocument/signatureHelp" {
                        break;
                    }
                    before.push(method.to_string());
                }
                Ok(_) => {}
                Err(_) => panic!("nothing more arrived; before the call: {before:?}"),
            }
        }
        assert!(
            !before
                .iter()
                .any(|method| method == "textDocument/completion"),
            "the whole scope was offered over the call: {before:?}"
        );

        // And the punctuation that is only a completion trigger still is
        // one: this is about the two lists overlapping, not about `.`.
        support::type_text(&mut app, "x.");
        let offered = support::heard_requests(&heard, "textDocument/completion", 1);
        assert_eq!(
            offered.len(),
            1,
            "a dot stopped offering what could follow it: {offered:?}"
        );
    }

    /// A candidate that lands the caret in a call asks what the call takes.
    ///
    /// Choosing `copy(…)` puts the reader between the brackets without
    /// anybody typing one, so the keystroke that would have asked never
    /// happens -- and the panel a reader most expects is the one after they
    /// have just picked the function. Broken deliberately by taking the
    /// `behind_cursor` block out of `accept_completion`: nothing is asked,
    /// and the reader sits in the brackets with no answer until they type a
    /// comma.
    #[test]
    fn a_candidate_that_lands_the_caret_in_a_call_asks_what_it_takes() {
        use obelus_syntax::LanguageId;

        let (_scratch, mut app) = editing("signature-accepted", "fn main() {\n    cop\n}\n");
        let (sender, heard) = obelus_app::event::channel();
        app.events_for_test(sender);
        assert!(
            app.stand_in_server_for_test(LanguageId::Rust, "cat"),
            "the echo would not start"
        );
        app.declared_for_test(
            LanguageId::Rust,
            json!({ "signatureHelpProvider": { "triggerCharacters": ["(", ",", "<"] } }),
        );
        support::press(&mut app, crossterm::event::KeyCode::Down);
        support::press(&mut app, crossterm::event::KeyCode::End);

        // The shape a real rust-analyzer sends, holes and all -- measured
        // against one rather than invented: `copy(${1:from}, ${2:to})$0`.
        // The difference is the whole of this test. A hole with a default
        // in it is *selected*, so the caret lands at the end of `from`
        // rather than against the bracket -- and the first version of this
        // test used `copy($1)$0`, whose empty hole puts the caret exactly
        // where the rule was then looking. It passed, and nothing worked.
        app.complete_for_test(json!([{
            "label": "copy(…)",
            "kind": 3,
            "insertText": "copy(${1:from}, ${2:to})$0",
            "insertTextFormat": 2
        }]));
        support::press(&mut app, crossterm::event::KeyCode::Enter);
        assert_eq!(
            text(&app),
            "fn main() {\n    copy(from, to)\n}\n",
            "the candidate did not go in as a snippet"
        );
        // And the caret is *not* against the bracket, which is what makes
        // this the case that was broken rather than the one that worked.
        assert!(
            app.current_buffer().is_some_and(|buffer| {
                buffer.cursor().column.get() != "    copy(".chars().count()
            }),
            "the caret is against the bracket, so this is not the shape a server sends"
        );

        let asked = support::heard_requests(&heard, "textDocument/signatureHelp", 1);
        assert_eq!(
            asked.len(),
            1,
            "nothing asked what the call the caret is now inside takes: {asked:?}"
        );
        // 3 is `ContentChange`: the document moved under the caret. Nobody
        // typed this bracket, and the reader did not ask either.
        assert_eq!(asked[0]["params"]["context"]["triggerKind"], json!(3));
    }

    /// An offset the protocol counts in UTF-16 is read as one.
    ///
    /// `ParameterInformation.label` offsets are code units, which the spec
    /// says in as many words -- and `as usize` reads them as characters.
    /// The two agree on everything in the basic multilingual plane and part
    /// company on the first astral one, and what is marked afterwards is
    /// the wrong part of the label.
    ///
    /// The helper has a test of its own in `obelus-text`; this one is about
    /// the wiring, because a conversion nothing calls is a conversion that
    /// fixes nothing. Broken deliberately by going back to `*from as usize`
    /// in `marked`, which marks `: &str, who` instead of the argument.
    #[test]
    fn an_offset_counted_in_utf16_is_read_as_one() {
        use obelus_lsp::signature::in_reply;

        let label = "fn wave(\u{1f44b}: &str, who: &str)";
        let at = label.find("who").expect("the second parameter");
        let units = |up_to: usize| -> usize { label[..up_to].chars().map(char::len_utf16).sum() };
        let found = in_reply(&Ok(json!({
            "signatures": [{
                "label": label,
                "parameters": [
                    { "label": [units(8), units(label.find(',').expect("the comma"))] },
                    { "label": [units(at), units(label.len() - 1)] }
                ],
                "activeParameter": 1
            }],
            "activeSignature": 0
        })))
        .expect("a signature");

        let (from, to) = found.signatures[0].active.expect("the mark");
        let characters: Vec<char> = label.chars().collect();
        let marked: String = characters[from..to].iter().collect();
        assert_eq!(
            marked, "who: &str",
            "the offsets were read as characters, so the mark landed elsewhere"
        );
    }

    /// The reader can ask, standing in a call somebody else wrote.
    ///
    /// Every other way in is a character: one the server named, or the
    /// bracket a candidate put the caret inside. A reader who arrows into a
    /// call that is already written types nothing, and used to have nowhere
    /// to ask from -- so `show-signature` is the way in, and the question it
    /// sends says the reader asked for it rather than naming a character
    /// nobody typed.
    ///
    /// Gated on there being a file rather than on there being an answer,
    /// like the completion command beside it: "it is still starting" is
    /// something a reader can act on, and a row that is dim for want of a
    /// server is a row that cannot say so.
    ///
    /// Broken deliberately by sending `triggerKind` 2 with no character,
    /// which is a context that says a character the reader never typed
    /// asked the question.
    #[test]
    fn the_reader_can_ask_what_a_call_they_are_standing_in_takes() {
        use obelus_app::app::dispatch;
        use obelus_command::Command;
        use obelus_syntax::LanguageId;

        let (_scratch, mut app) =
            editing("signature-invoked", "fn main() {\n    copy(from, to)\n}\n");
        let (sender, heard) = obelus_app::event::channel();
        app.events_for_test(sender);
        assert!(
            app.stand_in_server_for_test(LanguageId::Rust, "cat"),
            "the echo would not start"
        );
        app.declared_for_test(
            LanguageId::Rust,
            json!({ "signatureHelpProvider": { "triggerCharacters": ["(", ",", "<"] } }),
        );

        // Standing inside the call, having typed nothing.
        support::press(&mut app, crossterm::event::KeyCode::Down);
        support::press(&mut app, crossterm::event::KeyCode::End);
        support::press(&mut app, crossterm::event::KeyCode::Left);
        dispatch::dispatch(&mut app, Command::SymbolSignature);

        let asked = support::heard_requests(&heard, "textDocument/signatureHelp", 1);
        assert_eq!(asked.len(), 1, "the key asked nobody: {asked:?}");
        // 1 is `Invoked`: the reader asked, and no character did.
        assert_eq!(asked[0]["params"]["context"]["triggerKind"], json!(1));
        assert_eq!(
            asked[0]["params"]["context"]["triggerCharacter"],
            serde_json::Value::Null,
            "a character nobody typed is named as having asked: {:?}",
            asked[0]["params"]["context"]
        );
    }

    /// The mark follows the caret from one argument to the next.
    ///
    /// Which argument is being typed is the server's to say, and it says it
    /// about a position -- so a caret that has moved needs the question
    /// asked again. Nothing types when a reader steps between arguments
    /// with an arrow or a tab, so nothing else asks, and the panel went on
    /// marking the argument they had left: the one thing it is for.
    ///
    /// Asked once they have stopped, which is what `SignatureSettled` is.
    /// Broken deliberately by taking the clock out of `settle_signature`,
    /// and again by having `ask_signature_again` do nothing: either way the
    /// second question never goes out.
    #[test]
    fn the_mark_follows_the_caret_from_one_argument_to_the_next() {
        use obelus_syntax::LanguageId;

        let (_scratch, mut app) =
            editing("signature-moved", "fn main() {\n    copy(from, to)\n}\n");
        let (sender, heard) = obelus_app::event::channel();
        app.events_for_test(sender);
        assert!(
            app.stand_in_server_for_test(LanguageId::Rust, "cat"),
            "the echo would not start"
        );
        app.declared_for_test(
            LanguageId::Rust,
            json!({ "signatureHelpProvider": { "triggerCharacters": ["(", ",", "<"] } }),
        );

        // Standing on the first argument, with an answer about it.
        support::press(&mut app, crossterm::event::KeyCode::Down);
        support::press(&mut app, crossterm::event::KeyCode::End);
        for _ in 0.."from, to)".chars().count() {
            support::press(&mut app, crossterm::event::KeyCode::Left);
        }
        app.signature_for_test(json!({
            "signatures": [{
                "label": "fn copy(from: P, to: Q)",
                "parameters": [{ "label": [8, 15] }, { "label": [17, 22] }],
                "activeParameter": 0
            }],
            "activeSignature": 0
        }));
        let shown = app.signature().expect("the panel");
        assert_eq!(
            shown.shown()[0].active,
            Some((8, 15)),
            "the first argument is not the one marked"
        );
        let _ = support::heard_requests(&heard, "textDocument/signatureHelp", 1);

        // The reader steps to the second argument, typing nothing.
        for _ in 0.."from, ".chars().count() {
            support::press(&mut app, crossterm::event::KeyCode::Right);
        }
        support::render(&mut app, 60, 16);

        // And the clock is waited for rather than fired by hand. Handing
        // the application its own event is what the first version of this
        // did, and it proved only that the handler works: with the clock
        // never started the test still passed, which is the whole of what
        // it is about.
        let settled = {
            let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                let left = until.saturating_duration_since(std::time::Instant::now());
                assert!(!left.is_zero(), "the caret moved and no clock was started");
                match heard.recv_timeout(left) {
                    Ok(event @ obelus_app::event::Event::SignatureSettled) => break event,
                    Ok(_) => {}
                    Err(_) => panic!("nothing more arrived, and no clock was started"),
                }
            }
        };
        app.handle(settled);

        let asked = support::heard_requests(&heard, "textDocument/signatureHelp", 1);
        assert_eq!(
            asked.len(),
            1,
            "the caret moved to another argument and nothing asked again: {asked:?}"
        );
        // 3 is `ContentChange`: what is under the caret is not what it was.
        assert_eq!(asked[0]["params"]["context"]["triggerKind"], json!(3));

        // And the answer about where they are now moves the mark.
        app.signature_for_test(json!({
            "signatures": [{
                "label": "fn copy(from: P, to: Q)",
                "parameters": [{ "label": [8, 15] }, { "label": [17, 22] }],
                "activeParameter": 1
            }],
            "activeSignature": 0
        }));
        let shown = app.signature().expect("the panel");
        assert_eq!(
            shown.shown()[0].active,
            Some((17, 22)),
            "the mark stayed on the argument the reader has left"
        );
    }

    /// A call written over several lines is one call.
    ///
    /// The panel used to be about a *line*: a newline closed it, and the
    /// settling threw it away as soon as the caret was on another one. So a
    /// reader breaking a long argument list over four lines -- which is how
    /// a long argument list is written -- lost the panel at the first
    /// comma, in the middle of the thing it was about.
    ///
    /// What it is about is the call, which the tree can say and a line
    /// cannot: the brackets around the caret. Broken deliberately by going
    /// back to the line, and by putting the newline back among the things
    /// that close it.
    #[test]
    fn a_call_written_over_several_lines_keeps_the_panel() {
        let (_scratch, mut app) =
            editing("signature-multiline", "fn main() {\n    copy(from)\n}\n");
        support::press(&mut app, crossterm::event::KeyCode::Down);
        support::press(&mut app, crossterm::event::KeyCode::End);
        support::press(&mut app, crossterm::event::KeyCode::Left);
        app.signature_for_test(json!({
            "signatures": [{
                "label": "fn copy(from: P, to: Q)",
                "parameters": [{ "label": [8, 15] }, { "label": [17, 22] }],
                "activeParameter": 0
            }],
            "activeSignature": 0
        }));
        assert!(app.signature().is_some(), "the panel never opened");

        // The reader breaks the call over two lines, still inside it. The
        // comma is typed and the line is *pressed*: `type_text` sends a
        // `\n` as a character, which is not the newline the editor hears
        // from the key -- so a test that typed one never went near the arm
        // that used to close the panel on it.
        support::type_text(&mut app, ",");
        support::press(&mut app, crossterm::event::KeyCode::Enter);
        support::render(&mut app, 60, 16);
        assert!(
            app.signature().is_some(),
            "a newline inside the call took the panel away"
        );

        // And walking back up to the first line is still the same call.
        support::press(&mut app, crossterm::event::KeyCode::Up);
        support::render(&mut app, 60, 16);
        assert!(
            app.signature().is_some(),
            "moving between the call's own lines took the panel away"
        );

        // Out of the call altogether, which is where it does go.
        support::press(&mut app, crossterm::event::KeyCode::Down);
        support::press(&mut app, crossterm::event::KeyCode::Down);
        support::press(&mut app, crossterm::event::KeyCode::End);
        support::render(&mut app, 60, 16);
        assert!(
            app.signature().is_none(),
            "the panel stayed after the reader left the call"
        );
    }

    /// A closing bracket ends the call, so it ends the panel.
    ///
    /// Where the server did not name it as one to ask again on: with no
    /// `retriggerCharacters` there is nobody to ask, and a panel about a
    /// call the reader has closed is a panel about somewhere they have left.
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

/// A press on a candidate takes it.
///
/// The panel is drawn over the file at the caret, which is the one place a
/// press was sure to land -- and the press went straight through it to the
/// text, moving the caret out from under the very word the panel is about.
///
/// Taking the candidate outright, because that is what the list is for: it
/// is up only while the reader is in the middle of a word, it covers the
/// word it is about, and there is nothing in it to browse past.
///
/// Broken deliberately by letting the press through to the file, which
/// moves the caret and leaves the word half-typed.
#[test]
fn a_press_on_a_candidate_takes_it() {
    let (_scratch, mut app) = editing("complete-press", "fn main() {\n    p\n}\n");
    support::press(&mut app, crossterm::event::KeyCode::Down);
    support::press(&mut app, crossterm::event::KeyCode::End);
    app.complete_for_test(labels(&["parse", "pop"]));

    // The row the second candidate is drawn on, read off the screen.
    let dump = support::render(&mut app, 60, 16);
    let y = support::text_block(&dump)
        .lines()
        .find(|row| row.contains("pop"))
        .and_then(|row| row.split_once('|'))
        .and_then(|(at, _)| at.trim().parse::<u16>().ok())
        .expect("the row the second candidate is on");
    let x = support::column_of(
        support::text_block(&dump)
            .lines()
            .find(|row| row.contains("pop"))
            .expect("the row"),
        "pop",
    );

    app.handle(obelus_app::event::Event::Pointer {
        kind: obelus_app::event::Pointer::Pressed,
        x: u16::try_from(x).expect("a column"),
        y,
    });
    assert_eq!(
        text(&app),
        "fn main() {\n    pop\n}\n",
        "the press did not take the candidate it landed on"
    );
    assert!(
        app.completion().is_none(),
        "the list is still up after taking one"
    );
}
