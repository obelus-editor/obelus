//! Who calls this, and what it calls.
//!
//! A tree rather than a list, because the answer is one: every name in it
//! can be asked the same question again. The protocol asks it one item at
//! a time, so what is on screen is only ever as much as the reader opened
//! -- and opening is the request.

mod support;

use crossterm::event::KeyCode;
use obelus::{
    app::{App, dispatch},
    buffer::Buffer,
    command::Command,
    syntax::LanguageId,
};
use serde_json::json;

/// A file with something to call in it, and the app reading it.
fn reading(name: &str) -> (support::Scratch, App) {
    let scratch = support::Scratch::new(name);
    let path = scratch.path().join("one.rs");
    std::fs::write(&path, "fn run() {}\n\nfn main() {\n    run();\n}\n").expect("writing it");
    let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, 80, 24);
    (scratch, app)
}

/// One item, as a server hands them over: the name, where it is, and the
/// `data` the protocol says is the server's own.
fn item(path: &std::path::Path, name: &str, line: u32) -> serde_json::Value {
    json!({
        "name": name,
        "kind": 12,
        "uri": support::uri_for(path),
        "range": { "start": { "line": line, "character": 0 },
                   "end": { "line": line + 2, "character": 1 } },
        "selectionRange": { "start": { "line": line, "character": 3 },
                            "end": { "line": line, "character": 3 + name.len() as u32 } },
        "data": { "server": name }
    })
}

/// The tree opens on the item the server prepared, and the first question
/// is answered under it: a list of one row saying the name the cursor was
/// already on would be no answer at all.
#[test]
fn the_tree_opens_on_what_was_prepared_and_what_calls_it() {
    let (scratch, mut app) = reading("calls-opened");
    let path = scratch.join("one.rs");
    app.prepared_for_test(json!([item(&path, "run", 0)]));
    assert_eq!(app.call_tree_for_test(), [(0, "run".to_string())]);

    app.called_for_test(
        0,
        json!([{
            "from": item(&path, "main", 2),
            "fromRanges": [{ "start": { "line": 3, "character": 4 },
                             "end": { "line": 3, "character": 7 } }]
        }]),
    );
    assert_eq!(
        app.call_tree_for_test(),
        [(0, "run".to_string()), (1, "main".to_string())],
        "the caller did not land under what it calls"
    );
}

/// What it looks like: names indented under what reaches them, each with
/// the mark that says whether it opens, and the place it is at the end of
/// the row.
#[test]
fn a_tree_of_calls_on_screen() {
    let (scratch, mut app) = reading("calls-drawn");
    let path = scratch.join("one.rs");
    app.prepared_for_test(json!([item(&path, "run", 0)]));
    app.called_for_test(
        0,
        json!([{
            "from": item(&path, "main", 2),
            "fromRanges": [{ "start": { "line": 3, "character": 4 },
                             "end": { "line": 3, "character": 7 } }]
        }]),
    );
    // Open the caller, and let it come back round to the root: a ring is
    // the one thing a tree like this has to draw and cannot open.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    app.called_for_test(
        1,
        json!([{ "from": item(&path, "run", 0), "fromRanges": [] }]),
    );

    let dump = support::render(&mut app, 80, 24);
    support::check("calls_80x24", &dump);

    // The call the selected row names is marked in the preview under it,
    // which is what a row carrying a span rather than a point is for: the
    // row says `one.rs:4`, and the preview says *where* on line four.
    let rows: Vec<&str> = support::text_block(&dump).lines().collect();
    let styles: Vec<&str> = support::style_block(&dump).lines().collect();
    let at = rows
        .iter()
        .position(|row| row.contains("run();"))
        .expect("the call in the preview");
    // After the dump's own row prefix, which the style rows carry too.
    let divider = rows[at].find('|').expect("a divider") + 1;
    let column = support::column_of(&rows[at][divider..], "run(");
    let cells: Vec<char> = styles[at][divider..].chars().collect();
    let marked = support::legend_of(&dump, cells[column]);
    let plain = support::legend_of(&dump, cells[0]);
    assert_ne!(
        marked, plain,
        "the call is drawn the same as the rest of the line:\n{dump}"
    );
}

/// Enter opens the row the reader is on, and closes it again -- the key a
/// directory in the counted tree and a commit in a history already open on.
/// Which is the whole of the interface: the protocol asks one item at a
/// time, so opening a row is exactly one question.
#[test]
fn enter_opens_a_row_and_closes_it() {
    let (scratch, mut app) = reading("calls-folded");
    let path = scratch.join("one.rs");
    app.prepared_for_test(json!([item(&path, "run", 0)]));
    app.called_for_test(
        0,
        json!([{ "from": item(&path, "main", 2), "fromRanges": [] }]),
    );

    // Onto the caller, and open it.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    app.called_for_test(
        1,
        json!([{ "from": item(&path, "start", 8), "fromRanges": [] }]),
    );
    assert_eq!(
        app.call_tree_for_test(),
        [
            (0, "run".to_string()),
            (1, "main".to_string()),
            (2, "start".to_string())
        ],
        "the key did not open the row under the reader"
    );

    // And again, which closes it: the same key, the same row.
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.call_tree_for_test(),
        [(0, "run".to_string()), (1, "main".to_string())],
        "the key that opened it would not close it"
    );
}

/// Choosing a row goes to the call rather than to the caller: a reader who
/// asked who calls this wants the line that makes the call, not the first
/// line of whoever makes it.
#[test]
fn choosing_a_caller_lands_on_the_call() {
    let (scratch, mut app) = reading("calls-chosen");
    let path = scratch.join("one.rs");
    app.prepared_for_test(json!([item(&path, "run", 0)]));
    app.called_for_test(
        0,
        json!([{
            "from": item(&path, "main", 2),
            "fromRanges": [{ "start": { "line": 3, "character": 4 },
                             "end": { "line": 3, "character": 7 } }]
        }]),
    );

    support::press(&mut app, KeyCode::Down);
    support::press_alt_key(&mut app, KeyCode::Enter);
    let cursor = app.current_buffer().expect("a file").cursor();
    assert_eq!(
        (cursor.line.get(), cursor.column.get()),
        (3, 4),
        "it landed on the caller rather than on the call"
    );
}

/// A tree of calls has no bottom where anything is recursive, so an item
/// that is already above itself is shown and left shut: pressing the key on
/// it asks nobody anything.
#[test]
fn something_already_above_itself_is_not_asked_about_again() {
    let (scratch, mut app) = reading("calls-looping");
    let (sender, heard) = obelus::event::channel();
    app.events_for_test(sender);
    assert!(
        app.stand_in_server_for_test(LanguageId::Rust, "cat"),
        "the echo would not start"
    );
    // Until a server has answered the handshake nothing else is written to
    // it, so what it can do has to be settled before anything is asked.
    app.declared_for_test(LanguageId::Rust, json!({ "callHierarchyProvider": true }));
    let path = scratch.join("one.rs");
    app.prepared_for_test(json!([item(&path, "run", 0)]));
    // `run` calls itself, so the answer about `run` contains `run`.
    app.called_for_test(
        0,
        json!([{ "from": item(&path, "run", 0), "fromRanges": [] }]),
    );
    assert_eq!(
        app.call_tree_for_test(),
        [(0, "run".to_string()), (1, "run".to_string())]
    );

    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    // One question, which is the one that opened the tree: pressing the key
    // on the row that came back round asked nobody anything.
    let asked = support::heard_requests(&heard, "callHierarchy/incomingCalls", 2);
    assert_eq!(
        asked.len(),
        1,
        "the ring was followed round again: {asked:?}"
    );
}

/// Enter on a ring turns back to the row it repeats.
///
/// There is nothing to open -- what is behind it is what is behind the row
/// above -- so the key answers the question the reader actually has, which
/// is where that row is.
#[test]
fn enter_on_a_ring_turns_back_to_what_it_repeats() {
    let (scratch, mut app) = reading("calls-ring");
    let path = scratch.join("one.rs");
    app.prepared_for_test(json!([item(&path, "run", 0)]));
    app.called_for_test(
        0,
        json!([{ "from": item(&path, "main", 2), "fromRanges": [] }]),
    );
    // `main` is called by `run`, which is the root: a ring.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    app.called_for_test(
        1,
        json!([{ "from": item(&path, "run", 0), "fromRanges": [] }]),
    );
    assert_eq!(
        app.call_tree_for_test(),
        [
            (0, "run".to_string()),
            (1, "main".to_string()),
            (2, "run".to_string())
        ]
    );

    // Down onto the ring, which is the deepest row, and press it.
    support::press(&mut app, KeyCode::Down);
    assert_eq!(
        app.selected_call_for_test(),
        Some((2, "run".to_string())),
        "the reader is not standing on the ring"
    );
    let dump = support::render(&mut app, 80, 24);
    let rows = support::text_block(&dump);
    assert!(
        rows.lines().any(|row| row.contains('\u{21a9}')),
        "nothing on screen is drawn as a ring:\n{rows}"
    );

    support::press(&mut app, KeyCode::Enter);
    // The root, two levels up -- and it has the same name as the ring,
    // which is what makes the depth the only thing that can tell them
    // apart.
    assert_eq!(
        app.selected_call_for_test(),
        Some((0, "run".to_string())),
        "it did not turn back to the row the ring repeats"
    );
    // The root, which is where the ring came from -- not the ring itself.
    assert_eq!(
        app.call_tree_for_test().len(),
        3,
        "the ring was followed round after all"
    );
}

/// The two tabs are the two directions, and walking onto one asks the other
/// question -- of the same root, which is what the two of them share.
#[test]
fn the_tabs_are_the_two_directions() {
    let (scratch, mut app) = reading("calls-turned");
    let (sender, heard) = obelus::event::channel();
    app.events_for_test(sender);
    assert!(
        app.stand_in_server_for_test(LanguageId::Rust, "cat"),
        "the echo would not start"
    );
    app.declared_for_test(LanguageId::Rust, json!({ "callHierarchyProvider": true }));
    let path = scratch.join("one.rs");
    app.prepared_for_test(json!([item(&path, "run", 0)]));
    assert_eq!(app.calls_direction_for_test(), Some("Callers"));

    support::press(&mut app, KeyCode::Tab);
    assert_eq!(
        app.calls_direction_for_test(),
        Some("Calls"),
        "the tab did not turn the question round"
    );
    let asked = support::heard_requests(&heard, "callHierarchy/outgoingCalls", 1);
    assert_eq!(asked.len(), 1, "nobody was asked what it calls: {asked:?}");
    // The item goes back as it came: `data` is the server's own, and the
    // protocol says it is preserved between preparing an item and asking
    // about it.
    assert_eq!(
        asked[0]["params"]["item"]["data"]["server"], "run",
        "the item was rebuilt rather than carried: {:?}",
        asked[0]
    );
}

/// An answer that arrives after the reader turned the tree round is
/// dropped: callers hung under a row of callees would be an answer to a
/// question nobody asked.
#[test]
fn an_answer_to_the_question_before_the_turn_is_dropped() {
    let (scratch, mut app) = reading("calls-late");
    let path = scratch.join("one.rs");
    app.prepared_for_test(json!([item(&path, "run", 0)]));
    // An answer the other question could have given too: one that only
    // the tree on screen can read would be dropped by the reading rather
    // than by the rule being tested.
    app.late_call_for_test(
        0,
        json!([{ "from": item(&path, "main", 2),
                 "to": item(&path, "helper", 6),
                 "fromRanges": [] }]),
    );
    assert_eq!(
        app.call_tree_for_test(),
        [(0, "run".to_string())],
        "the other question's answer was hung under this tree"
    );
}

/// A server that could not answer says why, rather than reading as a
/// function nobody calls: the reader pressed a key to find out which of
/// those it was, and only one of them is worth pressing again.
#[test]
fn a_server_that_refuses_says_so_rather_than_saying_nothing() {
    let (scratch, mut app) = reading("calls-refused");
    let path = scratch.join("one.rs");
    app.prepared_for_test(json!([item(&path, "run", 0)]));

    app.refused_call_for_test(0, "the server is busy");
    assert_eq!(
        app.note(),
        Some("the server is busy"),
        "a refusal read as an empty answer"
    );

    // And the other way round, on a tree of its own: an answer with
    // nothing in it says that instead.
    let (scratch, mut app) = reading("calls-empty");
    let path = scratch.join("one.rs");
    app.prepared_for_test(json!([item(&path, "run", 0)]));
    app.called_for_test(0, json!([]));
    assert_eq!(app.note(), Some("Nothing calls that"));
}

/// The rows that arrive are asked about without anybody pressing
/// anything, one at a time, so that their marks are true rather than
/// hopeful.
///
/// Which is the whole reason the mark can be trusted: the protocol has no
/// way to say whether an item has callers short of naming them, so the
/// only honest mark is one somebody has already paid for.
#[test]
fn what_arrives_is_asked_about_without_being_told_to() {
    let (scratch, mut app) = reading("calls-probed");
    let (sender, heard) = obelus::event::channel();
    app.events_for_test(sender);
    assert!(
        app.stand_in_server_for_test(LanguageId::Rust, "cat"),
        "the echo would not start"
    );
    app.declared_for_test(LanguageId::Rust, json!({ "callHierarchyProvider": true }));

    let path = scratch.join("one.rs");
    app.prepared_for_test(json!([item(&path, "run", 0)]));
    // One question so far: what calls the root, which the reader asked for
    // by opening the tree.
    let asked = support::heard_requests(&heard, "callHierarchy/incomingCalls", 1);
    assert_eq!(asked.len(), 1, "the tree opened without asking anything");

    // Two callers arrive, and nobody has pressed a key.
    app.called_for_test(
        0,
        json!([
            { "from": item(&path, "main", 2), "fromRanges": [] },
            { "from": item(&path, "restart", 6), "fromRanges": [] }
        ]),
    );
    let asked = support::heard_requests(&heard, "callHierarchy/incomingCalls", 1);
    assert_eq!(
        asked.len(),
        1,
        "nobody asked what calls the rows that just arrived"
    );
    assert_eq!(
        asked[0]["params"]["item"]["name"], "main",
        "the rows are not being asked about in the order they are read"
    );

    // One at a time: the second waits for the first to answer. What comes
    // back about the first is filed and not shown -- nobody asked to see
    // it.
    app.called_for_test(
        1,
        json!([
            { "from": item(&path, "start", 10), "fromRanges": [] },
            { "from": item(&path, "again", 14), "fromRanges": [] }
        ]),
    );
    assert_eq!(
        app.call_tree_for_test().len(),
        3,
        "a probe nobody asked for put rows on screen"
    );
    let asked = support::heard_requests(&heard, "callHierarchy/incomingCalls", 1);
    assert_eq!(asked.len(), 1, "the next row was not asked about");
    assert_eq!(asked[0]["params"]["item"]["name"], "restart");
    app.called_for_test(2, json!([]));

    // Opening what was filed costs nothing, and it moves every row after
    // it -- so what is asked about next is a question a row number could
    // not answer.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.call_tree_for_test(),
        [
            (0, "run".to_string()),
            (1, "main".to_string()),
            (2, "start".to_string()),
            (2, "again".to_string()),
            (1, "restart".to_string())
        ],
        "opening a row that had already been asked about went back to the server"
    );
    let asked = support::heard_requests(&heard, "callHierarchy/incomingCalls", 1);
    assert_eq!(
        asked[0]["params"]["item"]["name"], "start",
        "the question went out about whatever sat at that row's old number"
    );
}

/// The row the reader is waiting on turns, and the screen is woken to
/// turn it.
///
/// Both halves matter: a busy server takes seconds over one of these, and
/// a mark drawn once and never again says obelus has stopped rather than
/// that it is waiting.
#[test]
fn the_row_being_waited_on_turns() {
    let (scratch, mut app) = reading("calls-turning");
    let (sender, _heard) = obelus::event::channel();
    app.events_for_test(sender);
    assert!(
        app.stand_in_server_for_test(LanguageId::Rust, "cat"),
        "the echo would not start"
    );
    app.declared_for_test(LanguageId::Rust, json!({ "callHierarchyProvider": true }));

    let path = scratch.join("one.rs");
    app.prepared_for_test(json!([item(&path, "run", 0)]));
    // The root is what the reader is waiting on, from the moment the list
    // opens.
    support::render(&mut app, 80, 24);
    assert!(
        app.is_waking(),
        "the mark turns and nothing is waking the screen to turn it"
    );

    app.called_for_test(
        0,
        json!([{ "from": item(&path, "main", 2), "fromRanges": [] }]),
    );
    support::render(&mut app, 80, 24);
    assert!(
        !app.is_waking(),
        "the screen is still being woken with nothing moving on it"
    );
}

/// The reader's own question opens what it answers, and the probe's does
/// not -- which is decided by the kind the question was remembered as,
/// not by anything the answer carries.
///
/// Driven through the door the event loop uses, because that is where the
/// two kinds are told apart: a test that calls the handler itself has
/// already made the choice obelus is supposed to be making.
#[test]
fn the_readers_question_is_the_one_that_opens() {
    let (scratch, mut app) = reading("calls-kinds");
    let (sender, heard) = obelus::event::channel();
    app.events_for_test(sender);
    assert!(
        app.stand_in_server_for_test(LanguageId::Rust, "cat"),
        "the echo would not start"
    );
    app.declared_for_test(LanguageId::Rust, json!({ "callHierarchyProvider": true }));

    let path = scratch.join("one.rs");
    app.prepared_for_test(json!([item(&path, "run", 0)]));

    // The question obelus asked about the root, read off the wire.
    let asked = support::heard_requests(&heard, "callHierarchy/incomingCalls", 1);
    let id = asked[0]["id"].as_i64().expect("a request id");
    app.answer_for_test(
        LanguageId::Rust,
        id,
        Ok(json!([{ "from": item(&path, "main", 2), "fromRanges": [] }])),
    );
    assert_eq!(
        app.call_tree_for_test(),
        [(0, "run".to_string()), (1, "main".to_string())],
        "the answer to what the reader asked for was filed instead of opened"
    );

    // And the one nobody asked for, which obelus sent by itself: it says
    // whether the row opens, and opens nothing.
    let asked = support::heard_requests(&heard, "callHierarchy/incomingCalls", 1);
    let id = asked[0]["id"].as_i64().expect("a request id");
    app.answer_for_test(
        LanguageId::Rust,
        id,
        Ok(json!([{ "from": item(&path, "start", 8), "fromRanges": [] }])),
    );
    assert_eq!(
        app.call_tree_for_test(),
        [(0, "run".to_string()), (1, "main".to_string())],
        "a probe nobody asked for opened a row"
    );
    // What it did do is put the mark on, which is the whole point of it.
    let dump = support::render(&mut app, 80, 24);
    let row = support::text_block(&dump)
        .lines()
        .find(|row| row.contains("main"))
        .expect("the row")
        .to_string();
    assert!(
        row.contains('\u{25b8}'),
        "the probe did not say the row has something behind it: {row:?}"
    );
}

/// A row that came back empty stops saying it opens.
///
/// The mark is the only thing a row says about itself that outlives the
/// keypress: a status line is wiped by the next key, and a row drawn open
/// with nothing under it is a row promising children that are not there.
/// Which is what a test function looks like from here -- nothing calls it,
/// and the reader has to be able to see that a second later.
#[test]
fn a_row_with_nothing_behind_it_stops_offering_to_open() {
    let (scratch, mut app) = reading("calls-childless");
    let path = scratch.join("one.rs");
    app.prepared_for_test(json!([item(&path, "run", 0)]));
    app.called_for_test(
        0,
        json!([{ "from": item(&path, "main", 2), "fromRanges": [] }]),
    );

    // Open the caller, and let the server say nobody calls it.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    app.called_for_test(1, json!([]));

    let dump = support::render(&mut app, 80, 24);
    let row = support::text_block(&dump)
        .lines()
        .find(|row| row.contains("main"))
        .expect("the row")
        .to_string();
    assert!(
        !row.contains('\u{25be}') && !row.contains('\u{25b8}'),
        "it still offers to open, and has nothing to open: {row:?}"
    );

    // And pressing it again says why rather than putting the mark back.
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(app.note(), Some("Nothing calls that"));
    let dump = support::render(&mut app, 80, 24);
    let row = support::text_block(&dump)
        .lines()
        .find(|row| row.contains("main"))
        .expect("the row")
        .to_string();
    assert!(
        !row.contains('\u{25b8}'),
        "the second press offered the same empty answer again: {row:?}"
    );
}

/// A list that goes away takes its unanswered question with it.
///
/// Otherwise the answer arrives with no tree of its own and finds whatever
/// tree is up by then -- and both trees start at callers, so the direction
/// is no help: the reader is shown one function's callers under another
/// function's name.
#[test]
fn closing_the_list_forgets_what_it_had_asked() {
    let (scratch, mut app) = reading("calls-closed");
    let (sender, _heard) = obelus::event::channel();
    app.events_for_test(sender);
    assert!(
        app.stand_in_server_for_test(LanguageId::Rust, "cat"),
        "the echo would not start"
    );
    app.declared_for_test(LanguageId::Rust, json!({ "callHierarchyProvider": true }));

    let path = scratch.join("one.rs");
    app.prepared_for_test(json!([item(&path, "run", 0)]));
    assert_eq!(
        app.outstanding_for_test(),
        1,
        "the tree opened without asking what calls its root"
    );

    support::press(&mut app, KeyCode::Esc);
    assert_eq!(
        app.outstanding_for_test(),
        0,
        "the question outlived the tree it was about"
    );
}

/// Only one question about a tree is ever out: they are remembered by a row
/// number, and opening a row renumbers the rows below it.
#[test]
fn a_second_tree_does_not_leave_the_first_one_asking() {
    let (scratch, mut app) = reading("calls-replaced");
    let (sender, _heard) = obelus::event::channel();
    app.events_for_test(sender);
    assert!(
        app.stand_in_server_for_test(LanguageId::Rust, "cat"),
        "the echo would not start"
    );
    app.declared_for_test(LanguageId::Rust, json!({ "callHierarchyProvider": true }));

    let path = scratch.join("one.rs");
    app.prepared_for_test(json!([item(&path, "run", 0)]));
    app.prepared_for_test(json!([item(&path, "helper", 6)]));
    assert_eq!(
        app.outstanding_for_test(),
        1,
        "the first tree's question is still out, and will answer into the second"
    );
}

/// The question that goes out is the one the protocol names, about the
/// place the cursor is on. Nothing that feeds an answer in by hand can
/// check that: a request spelt wrong is answered by nobody, which from this
/// side looks exactly like a server with nothing to say.
#[test]
fn the_question_that_goes_out_is_the_one_the_protocol_names() {
    let (_scratch, mut app) = reading("calls-asked");
    let (sender, heard) = obelus::event::channel();
    app.events_for_test(sender);
    assert!(
        app.stand_in_server_for_test(LanguageId::Rust, "cat"),
        "the echo would not start"
    );
    app.declared_for_test(LanguageId::Rust, json!({ "callHierarchyProvider": true }));

    dispatch::dispatch(&mut app, Command::SymbolCalls);
    let asked = support::heard_requests(&heard, "textDocument/prepareCallHierarchy", 1);
    assert_eq!(asked.len(), 1, "obelus asked nobody: {asked:?}");
    assert!(
        asked[0]["params"]["textDocument"]["uri"]
            .as_str()
            .is_some_and(|uri| uri.ends_with("one.rs")),
        "the question is about another file: {:?}",
        asked[0]
    );
}

/// A server that does not answer it is not offered it: a menu row that
/// always comes back empty is worse than a menu one row shorter.
#[test]
fn a_server_that_cannot_answer_is_not_offered_the_question() {
    let (_scratch, mut app) = reading("calls-unsupported");
    let (sender, _heard) = obelus::event::channel();
    app.events_for_test(sender);
    assert!(
        app.stand_in_server_for_test(LanguageId::Rust, "cat"),
        "the echo would not start"
    );

    // On the name, because every question in the menu is about the name
    // under the cursor and there is none on the `fn` in front of it.
    for _ in 0..3 {
        support::press(&mut app, KeyCode::Right);
    }

    app.declared_for_test(LanguageId::Rust, json!({ "referencesProvider": true }));
    assert!(
        !app.offers(Command::SymbolCalls),
        "a server that said nothing about it was asked anyway"
    );

    app.declared_for_test(LanguageId::Rust, json!({ "callHierarchyProvider": true }));
    assert!(
        app.offers(Command::SymbolCalls),
        "a server that answers it was not offered"
    );
}
