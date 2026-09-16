//! What obelus says to a language server without being asked, and what it
//! says back when the server asks it something.
//!
//! Against values rather than against a language server: these are rules
//! about what goes on the wire, and the wire is where they can be read.
//! Where the bytes obelus writes are the point, the server is a program
//! that says back what it is told, which is enough to read them.

mod support;

use obelus::{
    app::App,
    buffer::Buffer,
    lsp::{client::Client, uses},
};
use serde_json::json;

fn editing(name: &str, contents: &str) -> (support::Scratch, App) {
    let scratch = support::Scratch::new(name);
    let path = scratch.path().join("sample.rs");
    std::fs::write(&path, contents).expect("writing the file");
    let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, 76, 18);
    (scratch, app)
}

/// A request from the server gets an answer. The protocol says every one
/// does, and a server that waits for one waits for ever.
#[test]
fn a_question_from_the_server_is_answered() {
    let sent = Client::answered_for_test(&json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "workspace/configuration",
        "params": { "items": [{ "section": "rust-analyzer" }, { "section": "other" }] }
    }))
    .expect("an answer");
    assert_eq!(sent["id"], json!(7), "the answer is to another question");
    assert_eq!(
        sent["result"],
        json!([null, null]),
        "not one answer per thing asked about"
    );

    // A method obelus has no answer for is refused, which is an answer.
    let sent = Client::answered_for_test(&json!({
        "jsonrpc": "2.0", "id": 8, "method": "window/showDocument", "params": {}
    }))
    .expect("an answer");
    assert_eq!(sent["error"]["code"], json!(-32601), "not method-not-found");

    // A notification -- no id -- is not answered: an answer to one is a
    // message the server has no question for.
    assert!(
        Client::answered_for_test(&json!({
            "jsonrpc": "2.0", "method": "window/logMessage", "params": { "message": "hi" }
        }))
        .is_none(),
        "a notification was answered"
    );
}

/// What obelus tells a server it can do, which is the half of the
/// conversation that fails silently: a server does not complain about a
/// capability that is missing, it answers less.
///
/// `code_action` in particular. Undeclared, rust-analyzer answers `null`
/// to every `textDocument/codeAction` -- measured: twenty-six requests
/// over three minutes, on ordinary code, every one `null` -- so `alt+a`
/// had nothing to show anywhere, on a line with a mistake on it or not.
/// Declared, the same three requests come back with two, three and five
/// offers.
#[test]
fn what_obelus_says_it_can_do() {
    let declared = obelus::lsp::client::client_capabilities();
    let text = declared
        .text_document
        .expect("what it says about documents");

    let actions = text.code_action.expect("nothing said about code actions");
    let literals = actions
        .code_action_literal_support
        .expect("a server may then answer with commands, and rust-analyzer answers null");
    assert!(
        literals
            .code_action_kind
            .value_set
            .iter()
            .any(|kind| kind == "quickfix"),
        "the kinds obelus can show are not among the ones it asked for"
    );
    // The two fields the list is built out of: one sorts it, and the
    // other is what an offer that arrived without its edit is known by.
    assert_eq!(actions.is_preferred_support, Some(true));
    assert_eq!(actions.data_support, Some(true));
    assert!(
        actions
            .resolve_support
            .is_some_and(|resolve| resolve.properties.iter().any(|name| name == "edit")),
        "obelus asks the server to fill an action in, and has not said so"
    );
    // And not this one: a server told obelus reads it sends the offers it
    // knows cannot be taken, and obelus has nowhere to put the reason --
    // so they would sit in the list looking like the ones that work.
    assert_eq!(
        actions.disabled_support, None,
        "obelus would be sent offers it shows as though they could be chosen"
    );

    // The one that was there first, and for the same kind of reason:
    // without it rust-analyzer answers `documentSymbol` flat.
    assert_eq!(
        text.document_symbol
            .and_then(|symbols| symbols.hierarchical_document_symbol_support),
        Some(true)
    );
}

/// The one request a server makes that this layer does not answer: an
/// edit is answered by making it, so it is kept for the side that has the
/// documents and nothing goes back until that side has said what happened.
#[test]
fn an_edit_the_server_asks_for_is_kept_rather_than_refused() {
    use obelus::{
        event::Event,
        lsp::{Server, client::edit_answer},
        syntax::LanguageId,
    };

    let scratch = support::Scratch::new("apply-wire");
    let (sender, events) = obelus::event::channel();
    // A server that says back whatever it is told, which is how what
    // obelus writes can be read: the shape on the wire is the whole of
    // what this test is about.
    let mut client = Client::start(
        LanguageId::Rust,
        Server {
            command: "cat",
            arguments: &[],
        },
        scratch.path(),
        sender,
    )
    .expect("starting the echo");
    assert!(
        client
            .on_message(&json!({ "id": 0, "result": { "capabilities": {} } }))
            .is_none(),
        "the handshake reply was handed back as an answer to a question"
    );
    let before = client.sent();

    assert!(
        client
            .on_message(&json!({
                "jsonrpc": "2.0", "id": 11, "method": "workspace/applyEdit",
                "params": { "label": "Extract into function", "edit": { "changes": {} } }
            }))
            .is_none(),
        "an edit was handed back as an answer to a question obelus asked"
    );
    assert_eq!(
        client.sent(),
        before,
        "something went back before anybody had looked at the edit"
    );

    let asked = client.take_asked_edits();
    assert_eq!(asked.len(), 1, "the edit was dropped instead of kept");
    assert_eq!(asked[0].label.as_deref(), Some("Extract into function"));
    assert_eq!(asked[0].id, json!(11), "the answer would go to nobody");

    // And once it has been made, the answer is what goes out.
    client.answer_request(&edit_answer(&asked[0].id, true, "made it"));
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let sent = loop {
        assert!(std::time::Instant::now() < until, "nothing came back");
        let left = until - std::time::Instant::now();
        match events.recv_timeout(left) {
            Ok(Event::Lsp { message, .. }) if message.get("result").is_some() => break message,
            Ok(_) => {}
            Err(_) => panic!("nothing came back"),
        }
    };
    assert_eq!(sent["id"], json!(11), "the answer is to another question");
    assert_eq!(sent["result"]["applied"], json!(true));
}

/// A path becomes a uri and comes back the same path.
///
/// The two halves are only correct together, and they were not: the
/// unescaping the diagnostics, renames and code actions went through
/// turned each escaped *byte* into a character, so `caf%C3%A9` came back
/// as `cafÃ©` -- a file nobody has. Every path obelus sends a server is
/// escaped a byte at a time, which is the half that was right.
#[test]
fn a_path_survives_the_round_trip_through_a_uri() {
    use obelus::lsp::{client::uri_for, path_of_uri};

    for path in [
        "/tmp/plain/one.rs",
        "/tmp/a b/with a space.rs",
        "/tmp/café/naïve.rs",
        "/tmp/\u{8def}\u{5f84}/\u{6587}\u{4ef6}.rs",
        "/tmp/hash#and?query/one.rs",
    ] {
        let path = std::path::PathBuf::from(path);
        let uri = uri_for(&path).expect("a uri");
        assert_eq!(
            path_of_uri(uri.as_str()).as_deref(),
            Some(path.as_path()),
            "{} came back as something else through {}",
            path.display(),
            uri.as_str()
        );
    }

    // And a scheme obelus cannot open is nothing rather than a guess.
    assert_eq!(path_of_uri("untitled:nowhere"), None);
}

/// What a server is told about a file that changed on disk, which turns
/// on what is there now rather than on what the watcher said.
#[test]
fn a_file_that_is_gone_is_reported_as_gone() {
    let scratch = support::Scratch::new("watched");
    let there = scratch.path().join("there.rs");
    std::fs::write(&there, "fn there() {}\n").expect("writing it");
    let gone = scratch.path().join("gone.rs");

    let told = obelus::lsp::watched_change(&there).expect("something to say");
    assert_eq!(told["changes"][0]["type"], json!(2), "not a change");
    let told = obelus::lsp::watched_change(&gone).expect("something to say");
    assert_eq!(
        told["changes"][0]["type"],
        json!(3),
        "a file that is gone was reported as one that changed, which a server would try to read"
    );
    assert!(
        told["changes"][0]["uri"]
            .as_str()
            .is_some_and(|uri| uri.ends_with("gone.rs")),
        "not the file that went: {told}"
    );
}

/// Every use of the name the pointer is resting on, marked where they
/// are -- and gone when the pointer is.
#[test]
fn the_uses_of_a_name_are_marked_where_the_pointer_rests() {
    use obelus::event::{Event, Pointer};

    let (_scratch, mut app) = editing(
        "uses-marked",
        "fn main() {\n    let name = 1;\n    let other = name + name;\n}\n",
    );
    // Over the `name` on the second line, which is what is being asked
    // about: the caret stays at the top of the file.
    let dump = support::render(&mut app, 76, 18);
    let rows: Vec<String> = support::text_block(&dump)
        .lines()
        .filter_map(|row| row.split_once('|').map(|(_, cells)| cells.to_string()))
        .collect();
    let (y, x) = rows
        .iter()
        .enumerate()
        .find_map(|(y, row)| row.find("name = 1").map(|x| (y, x)))
        .expect("the word");
    app.handle(Event::Pointer {
        kind: Pointer::Moved,
        x: u16::try_from(x).expect("a column"),
        y: u16::try_from(y).expect("a row"),
    });
    app.uses_for_test(json!([
        { "range": { "start": { "line": 1, "character": 8 },
                     "end": { "line": 1, "character": 12 } }, "kind": 3 },
        { "range": { "start": { "line": 2, "character": 16 },
                     "end": { "line": 2, "character": 20 } }, "kind": 2 },
        { "range": { "start": { "line": 2, "character": 23 },
                     "end": { "line": 2, "character": 27 } }, "kind": 2 }
    ]));

    let dump = support::render(&mut app, 76, 18);
    let rows: Vec<&str> = support::text_block(&dump).lines().collect();
    let styles: Vec<&str> = support::style_block(&dump).lines().collect();
    let ground = |letter: char| {
        support::legend_block(&dump)
            .lines()
            .find(|entry| entry.starts_with(letter))
            .and_then(|entry| entry.split("bg=").nth(1))
            .map(str::to_string)
            .unwrap_or_else(|| panic!("no legend for {letter:?}:\n{dump}"))
    };
    let at = |needle: &str, offset: usize| {
        let row = rows
            .iter()
            .position(|row| row.contains(needle))
            .unwrap_or_else(|| panic!("{needle:?} is not on screen:\n{dump}"));
        let text = &rows[row][rows[row].find('|').expect("a divider") + 1..];
        let cells = &styles[row][rows[row].find('|').expect("a divider") + 1..];
        let column = text.find(needle).expect("the text") + offset;
        ground(cells.chars().nth(column).expect("a style"))
    };

    // All three uses wear the same ground, and the code beside them does
    // not: the mark is a background, so the colours of the code survive it.
    let marked = at("let name", "let ".len());
    assert_eq!(at("name + name", 0), marked, "the second use is not marked");
    assert_eq!(at("name + name", 7), marked, "the third use is not marked");
    assert_ne!(
        at("let other", "let ".len()),
        marked,
        "something else is marked"
    );

    // Moving the caret changes nothing: this is the pointer's answer, and
    // a file whose colours moved under every arrow key would be a file
    // nobody could read.
    support::press(&mut app, crossterm::event::KeyCode::Down);
    assert_eq!(
        app.marked_runs().len(),
        3,
        "the marks follow the caret rather than the pointer"
    );

    // And moving the pointer off the word takes them away.
    app.handle(Event::Pointer {
        kind: Pointer::Moved,
        x: 1,
        y: u16::try_from(y).expect("a row"),
    });
    assert!(
        app.marked_runs().is_empty(),
        "the marks outlived the pointer"
    );
}

/// What a server said about a file it has never been told about/// What a
/// server said about a file it has never been told about is not worth keeping,
/// and neither is what it said about a version that has moved.
#[test]
fn an_answer_about_a_document_that_has_moved_is_dropped() {
    let (_scratch, mut app) = editing("uses-stale", "fn main() {\n    let name = 1;\n}\n");
    support::press(&mut app, crossterm::event::KeyCode::Down);
    support::press(&mut app, crossterm::event::KeyCode::End);
    support::type_text(&mut app, " ");
    let stale = app.current_buffer().expect("a buffer").version() - 1;
    app.uses_at_version_for_test(
        json!([{ "range": { "start": { "line": 1, "character": 8 },
                            "end": { "line": 1, "character": 12 } } }]),
        stale,
    );
    assert!(
        app.marked_runs().is_empty(),
        "an answer about the document as it was is on screen"
    );
}

/// The kinds a server may send, all of which are one mark here.
#[test]
fn the_shapes_an_answer_arrives_in() {
    use obelus::text::Text;

    let text = Text::from_string("fn main() {}\n");
    let encoding = lsp_types::PositionEncodingKind::UTF16;
    let found = uses::in_reply(
        &Ok(json!([{ "range": { "start": { "line": 0, "character": 3 },
                                "end": { "line": 0, "character": 7 } } }])),
        &text,
        &encoding,
    );
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].column.get(), found[0].end_column.get()), (3, 7));

    // A server with nothing to say says null, which is not one run of
    // nothing.
    assert!(uses::in_reply(&Ok(json!(null)), &text, &encoding).is_empty());
    assert!(uses::in_reply(&Err("no".to_string()), &text, &encoding).is_empty());
}
