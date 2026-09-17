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
/// One entry per thing obelus's own code reads, each with the code it
/// would kill. This is a canary rather than a tautology: the declaration
/// and the feature are written in different files by different hands, and
/// the only sign that they have come apart is a feature that quietly
/// stops happening on some servers and not others.
#[test]
fn what_obelus_says_it_can_do() {
    let declared = obelus::lsp::client::client_capabilities();
    let text = declared
        .text_document
        .expect("nothing said about documents");

    // `lsp::actions`. Undeclared, rust-analyzer answers `null` to every
    // `textDocument/codeAction` -- measured: twenty-six requests over
    // three minutes, on ordinary code, every one `null`.
    let actions = text.code_action.expect("nothing about code actions");
    let literals = actions
        .code_action_literal_support
        .expect("a server may then answer with commands, and some answer null");
    assert!(
        literals
            .code_action_kind
            .value_set
            .iter()
            .any(|kind| kind == "quickfix"),
        "the kinds obelus can show are not among the ones it asked for"
    );
    assert_eq!(actions.is_preferred_support, Some(true), "sorts the list");
    assert_eq!(actions.data_support, Some(true), "finds the unresolved");
    assert!(
        actions
            .resolve_support
            .is_some_and(|resolve| resolve.properties.iter().any(|name| name == "edit")),
        "obelus asks the server to fill an action in, and has not said so"
    );
    // The offers a server knows cannot be taken. Asked for so the reader
    // learns they exist: the reason is the row's detail, and the list
    // steps over the row.
    assert_eq!(
        actions.disabled_support,
        Some(true),
        "a server would leave out the offers it will not carry out, and the \
         reader would never learn they exist"
    );

    // `lsp::complete` and `lsp::snippet`. A server that has not been told
    // about snippets may not send one, which leaves the whole tab-stop
    // engine unreachable.
    let item = text
        .completion
        .expect("nothing about completion")
        .completion_item
        .expect("nothing about what an offer may carry");
    assert_eq!(item.snippet_support, Some(true), "lsp::snippet goes unused");
    assert_eq!(
        item.label_details_support,
        Some(true),
        "the detail beside an offer's name"
    );
    assert_eq!(
        item.insert_replace_support,
        Some(true),
        "completing mid-word replaces the word"
    );
    assert!(
        item.resolve_support.is_some_and(|resolve| {
            resolve
                .properties
                .iter()
                .any(|name| name == "documentation")
        }),
        "obelus asks for an offer's documentation, and has not said so"
    );

    // `lsp::hover` and the markdown renderer behind it.
    assert!(
        text.hover
            .and_then(|hover| hover.content_format)
            .is_some_and(|formats| formats.contains(&lsp_types::MarkupKind::Markdown)),
        "a hover would arrive as plain text"
    );

    // `lsp::signature`. Without the offsets a server sends the parameter
    // as a piece of text, and obelus has to find it in the label -- which
    // finds the wrong one when a name appears twice.
    assert_eq!(
        text.signature_help
            .and_then(|help| help.signature_information)
            .and_then(|information| information.parameter_information)
            .and_then(|parameter| parameter.label_offset_support),
        Some(true),
        "the panel would mark the wrong parameter"
    );

    // `lsp::outline`. Without it a server *may* answer flat, and
    // rust-analyzer does.
    assert_eq!(
        text.document_symbol
            .and_then(|symbols| symbols.hierarchical_document_symbol_support),
        Some(true),
        "the outline would be a flat list of whole definitions"
    );

    // `lsp::action` reads a `LocationLink`'s target, which is the shape
    // that carries the name's own range rather than the definition's.
    for (what, goto) in [
        ("definition", text.definition),
        ("type definition", text.type_definition),
        ("implementation", text.implementation),
    ] {
        assert_eq!(
            goto.and_then(|goto| goto.link_support),
            Some(true),
            "a jump to a {what} would land on the whole item"
        );
    }

    // `lsp::tokens` reads the server's own legend, so what is declared is
    // a floor -- but an empty floor is a server entitled to send nothing.
    let tokens = text.semantic_tokens.expect("nothing about semantic tokens");
    assert!(
        tokens.token_types.len() > 10,
        "obelus asked for almost no kinds of token"
    );
    assert!(
        tokens.formats.contains(&lsp_types::TokenFormat::RELATIVE),
        "obelus reads the packed form and has not said so"
    );

    // `lsp::colour`. Undeclared, a server is entitled to decide the
    // question is not worth answering, and the one that answers it is the
    // one a reader opens a stylesheet with.
    assert!(
        text.color_provider.is_some(),
        "obelus paints the colours a server finds and has not asked for them"
    );

    // `lsp::hierarchy`. Two requests behind one capability: undeclared, a
    // server is entitled to answer nothing to `prepareCallHierarchy`, and
    // the tree never has a root to grow from.
    assert!(
        text.call_hierarchy.is_some(),
        "obelus asks who calls this and has not said it can read the answer"
    );

    // `lsp::trouble` keeps a diagnostic whole because it goes back in a
    // code action's context, and a server matches it by every field.
    assert_eq!(
        text.publish_diagnostics
            .and_then(|published| published.data_support),
        Some(true),
        "a quick fix would be offered for a diagnostic the server cannot match"
    );

    let workspace = declared.workspace.expect("nothing said about the project");
    // `app::changing`. This is the one that permits the whole path from a
    // code action's command to a refactoring landing in the files: a
    // server that has not been told never sends the edit at all.
    assert_eq!(
        workspace.apply_edit,
        Some(true),
        "a server would never ask obelus to make the edit it worked out"
    );
    let edits = workspace
        .workspace_edit
        .expect("nothing about the edits themselves");
    assert_eq!(
        edits.document_changes,
        Some(true),
        "lsp::edits reads documentChanges and has not asked for them"
    );
    // Empty on purpose, and the same policy the code enforces: obelus
    // will not create, move or delete a file because a server said so.
    assert_eq!(
        edits.resource_operations,
        Some(Vec::new()),
        "obelus would be asked to do what it refuses to do"
    );
    assert!(workspace.symbol.is_some(), "obelus sends workspace/symbol");
    assert!(
        workspace.execute_command.is_some(),
        "obelus sends workspace/executeCommand"
    );
    assert!(
        workspace.did_change_watched_files.is_some(),
        "obelus tells servers about files that changed on disk"
    );

    // Progress, which is the only thing that tells a server still
    // indexing from one that has answered with nothing.
    assert_eq!(
        declared.window.and_then(|window| window.work_done_progress),
        Some(true),
        "the status row could not say a server is busy"
    );
}

/// A key that asks a server something says why when there is nothing to
/// ask, rather than doing nothing at all.
///
/// Silence and a broken key look the same. These are the states where a
/// reader has something to do about it -- install the program, wait two
/// seconds, use another editor for this one file -- and the one thing
/// that stops them doing it is not being told.
#[test]
fn a_key_with_no_server_to_ask_says_so() {
    use obelus::{
        command::{Command, dispatch},
        syntax::LanguageId,
    };

    let asking = [
        Command::CodeActions,
        Command::SymbolRename,
        Command::SymbolHover,
        Command::SymbolComplete,
    ];

    // A language obelus knows and a server that is not running for it.
    let (_scratch, mut app) = editing("silent", "fn main() {\n    let name = 1;\n}\n");
    for command in asking {
        assert!(
            app.offers(command),
            "{command:?} does nothing at all, which is what a broken key does"
        );
        dispatch::dispatch(&mut app, command);
        let said = app.note().unwrap_or_default().to_string();
        assert!(
            said.contains("server"),
            "{command:?} said nothing about why: {said:?}"
        );
    }

    // Running and not ready, which is every server for a second or two
    // and the state where saying "it offers nothing" is a lie.
    assert!(
        app.stand_in_server_for_test(LanguageId::Rust, "cat"),
        "the stand-in would not start"
    );
    for command in asking {
        dispatch::dispatch(&mut app, command);
        assert!(
            app.note().unwrap_or_default().contains("is still starting"),
            "{command:?} blamed the server for a handshake that has not landed: {:?}",
            app.note()
        );
    }

    // And once it has said what it does not do, that is what is said.
    app.declared_for_test(LanguageId::Rust, json!({}));
    for command in asking {
        dispatch::dispatch(&mut app, command);
        let said = app.note().unwrap_or_default().to_string();
        assert!(
            said.starts_with("rust-analyzer does not"),
            "{command:?} did not say what the server cannot do: {said:?}"
        );
    }
}

/// And a letter typed into the same file says nothing at all.
///
/// The other half of the rule, and the reason there are two doors into
/// the same question: a key that was pressed to ask deserves an answer,
/// and a letter is not a question -- a reader typing a word in a file
/// with no server would be writing against a status row telling them so
/// once per keystroke.
#[test]
fn a_typed_letter_with_no_server_says_nothing() {
    let (_scratch, mut app) = editing("silent-typing", "fn main() {\n    \n}\n");
    support::press(&mut app, crossterm::event::KeyCode::Down);
    support::press(&mut app, crossterm::event::KeyCode::End);
    support::type_text(&mut app, "let name");
    assert_eq!(
        app.note(),
        None,
        "typing a word talked about language servers"
    );

    // A trigger character, which is the other way a letter asks.
    support::type_text(&mut app, ".");
    assert_eq!(
        app.note(),
        None,
        "a full stop talked about language servers"
    );

    // And the key that asks on purpose, in the same file, does say -- the
    // two doors into one question are the point, so one test holds both.
    obelus::command::dispatch::dispatch(&mut app, obelus::command::Command::SymbolComplete);
    assert!(
        app.note().unwrap_or_default().contains("server"),
        "the key that asks on purpose said nothing: {:?}",
        app.note()
    );
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
