//! Calling a file something else, and moving one, which are the same act.

mod support;

use crossterm::event::KeyCode;
use obelus_app::app::{App, dispatch};
use obelus_buffer::Buffer;
use obelus_command::Command;

/// Says where it should be instead, over what is already in the question.
///
/// The question opens with where the file is now in it and the caret at
/// the end, which is what makes "call it something else" a word of typing
/// -- so a test that wants somewhere else entirely says so by clearing it.
fn answer(app: &mut App, path: &str) {
    for _ in 0..80 {
        support::press(app, KeyCode::Backspace);
    }
    support::type_text(app, path);
    support::press(app, KeyCode::Enter);
}

/// A project with a file open in it.
fn reading(name: &str) -> (support::Scratch, App) {
    let scratch = support::Scratch::new(name);
    scratch.write("src/hint.rs", "fn hint() {}\n");
    scratch.write("src/main.rs", "fn main() {}\n");
    let mut app = App::new(vec![
        Buffer::open(&scratch.join("src/hint.rs")).expect("opening it"),
    ]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.statuses_for_test(std::collections::HashMap::new());
    support::lay_out(&mut app, 76, 18);
    (scratch, app)
}

/// The one question moves a file and renames it, because they are the same
/// answer to "where should this be instead".
#[test]
fn the_answer_is_a_path_so_it_moves_as_well_as_renames() {
    let (scratch, mut app) = reading("move-path");
    dispatch::dispatch(&mut app, Command::FileRename);
    answer(&mut app, "src/lsp/hints.rs");

    assert!(
        scratch.join("src/lsp/hints.rs").exists(),
        "it is not where it was asked to go"
    );
    assert!(
        !scratch.join("src/hint.rs").exists(),
        "it is still where it was"
    );
    // The directory it was asked for did not exist, which is what taking a
    // path rather than a name is for.
    assert!(scratch.join("src/lsp").is_dir());
}

/// The document open on it is the same document afterwards: what it holds,
/// what has been undone and not redone, and where the caret is have nothing
/// to do with where the file sits.
#[test]
fn the_document_goes_with_it_and_keeps_its_undo() {
    let (scratch, mut app) = reading("move-undo");
    support::type_text(&mut app, "// ");

    dispatch::dispatch(&mut app, Command::FileRename);
    answer(&mut app, "src/hints.rs");

    let buffer = app.current_buffer().expect("a file");
    assert_eq!(
        buffer.path(),
        scratch.join("src/hints.rs"),
        "the document is still open on where the file was"
    );
    assert!(
        buffer.text().rope().to_string().starts_with("// fn hint"),
        "what the reader typed went with the move"
    );

    // And what they typed can still be taken back, which closing it and
    // opening the new path would have thrown away.
    dispatch::dispatch(&mut app, Command::Undo);
    assert!(
        app.current_buffer()
            .expect("a file")
            .text()
            .rope()
            .to_string()
            .starts_with("fn hint"),
        "the undo went with the old path"
    );
}

/// Writing over a file is the one thing here that cannot be put back, so it
/// is refused rather than asked about.
#[test]
fn it_refuses_to_move_onto_something_that_is_there() {
    let (scratch, mut app) = reading("move-onto");
    dispatch::dispatch(&mut app, Command::FileRename);
    answer(&mut app, "src/main.rs");

    assert_eq!(app.note(), Some("src/main.rs is already there"));
    assert!(scratch.join("src/hint.rs").exists(), "it moved anyway");
    assert_eq!(
        std::fs::read_to_string(scratch.join("src/main.rs")).expect("reading it"),
        "fn main() {}\n",
        "the file that was there was written over"
    );
}

/// The list moves what the reader is standing on, which is the case the
/// command cannot reach: a file nobody has open.
#[test]
fn the_list_moves_the_row_the_reader_is_on() {
    let (scratch, mut app) = reading("move-listed");
    support::press_function(&mut app, 1);
    // The tree opens on the file being read, so `src` is open, its files
    // are rows and the selection is already on `hint.rs`. One step down is
    // `main.rs`, which nobody has open.
    support::press(&mut app, KeyCode::Down);
    assert_eq!(
        app.picker()
            .expect("the list")
            .selected_item()
            .map(|row| row.label.as_str()),
        Some("main.rs"),
        "the reader is not standing on the file this is about"
    );

    support::press_alt_key(&mut app, KeyCode::Char('n'));
    answer(&mut app, "src/renamed.rs");

    assert!(
        scratch.join("src/renamed.rs").exists(),
        "the row the reader was on did not move"
    );
    assert!(
        !scratch.join("src/main.rs").exists(),
        "it is still where it was"
    );
    // And the one they have open was not the one that moved.
    assert_eq!(
        app.current_buffer().expect("a file").path(),
        scratch.join("src/hint.rs"),
        "the list moved the file being read rather than the row"
    );
}

/// A server that said it wants to hear about a file before it moves is
/// asked, and the file does not move until it answers.
///
/// The order is the whole point. A server asked after the file has gone
/// works out what to change against a project that no longer exists, and
/// answers about a file it can no longer read.
#[test]
fn a_server_is_asked_before_the_file_moves() {
    use obelus_syntax::LanguageId;
    use serde_json::json;

    let (scratch, mut app) = reading("move-asks");
    let (sender, heard) = obelus_app::event::channel();
    app.events_for_test(sender);
    assert!(
        app.stand_in_server_for_test(LanguageId::Rust, "cat"),
        "the echo would not start"
    );
    app.declared_for_test(LanguageId::Rust, will_rename_rust());

    dispatch::dispatch(&mut app, Command::FileRename);
    answer(&mut app, "src/lsp/hints.rs");

    let asked = support::heard_requests(&heard, "workspace/willRenameFiles", 1);
    assert_eq!(asked.len(), 1, "nothing was asked: {asked:?}");
    let files = &asked[0]["params"]["files"][0];
    assert_eq!(
        files["oldUri"],
        json!(support::uri_for(scratch.join("src/hint.rs"))),
    );
    assert_eq!(
        files["newUri"],
        json!(support::uri_for(scratch.join("src/lsp/hints.rs"))),
    );
    // And not yet moved: the answer is what the move is waiting for.
    assert!(
        scratch.join("src/hint.rs").exists(),
        "it moved before the server had said anything"
    );
}

/// The answer's edits are made, and then the file moves.
///
/// Both halves of one act, and only one of them can be taken back: the
/// edits land in documents that the reader can undo, and the move is on
/// disk and said to be final.
#[test]
fn the_answer_changes_the_files_that_named_it() {
    use obelus_syntax::LanguageId;
    use serde_json::json;

    let (scratch, mut app) = reading("move-applies");
    let (sender, heard) = obelus_app::event::channel();
    app.events_for_test(sender);
    assert!(app.stand_in_server_for_test(LanguageId::Rust, "cat"));
    app.declared_for_test(LanguageId::Rust, will_rename_rust());
    let before = app.file_count_for_test();

    dispatch::dispatch(&mut app, Command::FileRename);
    answer(&mut app, "src/lsp/hints.rs");
    let id = request_id(&support::heard_requests(
        &heard,
        "workspace/willRenameFiles",
        1,
    ));
    // `src/main.rs` is not open, and says the module's old name.
    app.answer_for_test(
        LanguageId::Rust,
        id,
        Ok(json!({
            "documentChanges": [{
                "textDocument": {
                    "uri": support::uri_for(scratch.join("src/main.rs")),
                    "version": null
                },
                "edits": [{
                    "range": {
                        "start": { "line": 0, "character": 3 },
                        "end": { "line": 0, "character": 7 }
                    },
                    "newText": "hints"
                }]
            }]
        })),
    );

    assert!(
        scratch.join("src/lsp/hints.rs").exists(),
        "the file did not move once the server had answered"
    );
    assert_eq!(
        app.file_count_for_test(),
        before + 1,
        "the file the server wanted changed did not become a document"
    );
    assert!(
        app.buffers_for_test()
            .iter()
            .any(|(path, dirty)| path.ends_with("main.rs") && *dirty),
        "the edit was not made, or was written to disk: {:?}",
        app.buffers_for_test()
    );
    // Not written: the same rule as every other edit a server asks for.
    assert_eq!(
        std::fs::read_to_string(scratch.join("src/main.rs")).expect("reading it"),
        "fn main() {}\n",
        "the edit went straight to disk"
    );
}

/// A server that is still indexing answers `null`, which is not the same
/// as answering that nothing changes.
///
/// The move happens either way -- a reader is never held up by a
/// subprocess -- and the difference is said out loud, because a file whose
/// references were silently not updated is a build failure with no
/// explanation attached.
#[test]
fn a_server_that_is_not_ready_does_not_hold_the_move_up() {
    use obelus_syntax::LanguageId;
    use serde_json::json;

    let (scratch, mut app) = reading("move-indexing");
    let (sender, heard) = obelus_app::event::channel();
    app.events_for_test(sender);
    assert!(app.stand_in_server_for_test(LanguageId::Rust, "cat"));
    app.declared_for_test(LanguageId::Rust, will_rename_rust());

    dispatch::dispatch(&mut app, Command::FileRename);
    answer(&mut app, "src/lsp/hints.rs");
    let id = request_id(&support::heard_requests(
        &heard,
        "workspace/willRenameFiles",
        1,
    ));
    app.answer_for_test(LanguageId::Rust, id, Ok(json!(null)));

    assert!(scratch.join("src/lsp/hints.rs").exists(), "it did not move");
    let said = app.note().unwrap_or_default().to_string();
    assert!(
        said.contains("not ready"),
        "the reader was not told the references were left alone: {said}"
    );
}

/// A server is asked about the paths it registered and no others.
///
/// rust-analyzer asks for `**/*.rs`; a TypeScript server asks for `.ts`.
/// A move that matches nobody's filter is not a round trip, and the file
/// moves at once.
#[test]
fn a_path_no_server_registered_for_moves_at_once() {
    use obelus_syntax::LanguageId;
    use serde_json::json;

    let (scratch, mut app) = reading("move-unfiltered");
    assert!(app.stand_in_server_for_test(LanguageId::Rust, "cat"));
    app.declared_for_test(
        LanguageId::Rust,
        json!({
            "workspace": { "fileOperations": { "willRename": { "filters": [
                { "scheme": "file", "pattern": { "glob": "**/*.ts", "matches": "file" } }
            ]}}}
        }),
    );

    dispatch::dispatch(&mut app, Command::FileRename);
    answer(&mut app, "src/lsp/hints.rs");

    assert!(
        scratch.join("src/lsp/hints.rs").exists(),
        "the move waited on a server that had said it did not care"
    );
}

/// What rust-analyzer registers, taken off the wire.
fn will_rename_rust() -> serde_json::Value {
    serde_json::json!({
        "workspace": { "fileOperations": { "willRename": { "filters": [
            { "scheme": "file", "pattern": { "glob": "**/*.rs", "matches": "file" } },
            { "scheme": "file", "pattern": { "glob": "**", "matches": "folder" } }
        ]}}}
    })
}

/// The id obelus sent the question under, so the answer can come back
/// under it.
fn request_id(asked: &[serde_json::Value]) -> i64 {
    asked
        .first()
        .and_then(|message| message["id"].as_i64())
        .expect("a request with an id on it")
}

/// The question opens on the status row over the list, and the row says
/// what is being asked.
///
/// A question on the status row does not close the list it was asked from
/// -- the row the reader is standing on is what it is about, and it stays
/// on screen behind it. Which means two views want that row, and the one
/// that gets it has to be the one the keys are going to: a list's own
/// empty search box under the reader's caret is a row that says nothing
/// and takes their typing anyway.
#[test]
fn the_question_is_on_the_row_the_caret_is_in() {
    let (_scratch, mut app) = reading("move-prompt");
    support::press_function(&mut app, 1);
    support::press_alt_key(&mut app, KeyCode::Char('n'));

    let dump = support::render(&mut app, 76, 18);
    let rows = support::text_block(&dump);
    let last = rows.lines().last().unwrap_or_default();
    assert!(
        last.contains(&format!("Call it: {}", support::as_shown("src/hint.rs"))),
        "the status row is not the question:\n{dump}"
    );

    // And the list is still there, because it is what the question is
    // about.
    assert!(
        rows.lines().any(|row| row.contains("hint.rs")),
        "the list closed:\n{dump}"
    );
}

/// A rename waiting on a server keeps the clock awake.
///
/// The clock is what ends the wait when a server stops answering, and the
/// clock only runs while something says it is needed. Without this, the
/// five-second limit is a comment: a file the reader asked to rename sits
/// where it was until they happen to press something else.
#[test]
fn a_rename_waiting_on_a_server_keeps_the_clock_awake() {
    use obelus_syntax::LanguageId;
    use serde_json::json;

    let (_scratch, mut app) = reading("rename-clock");
    let (sender, heard) = obelus_app::event::channel();
    app.events_for_test(sender);
    assert!(app.stand_in_server_for_test(LanguageId::Rust, "cat"));
    app.declared_for_test(LanguageId::Rust, will_rename_rust());
    support::lay_out(&mut app, 76, 18);
    assert!(!app.is_waking(), "something was already waking the screen");

    dispatch::dispatch(&mut app, Command::FileRename);
    answer(&mut app, "src/lsp/hints.rs");
    support::lay_out(&mut app, 76, 18);
    assert!(
        app.is_waking(),
        "nothing will come back for a server that stops answering"
    );

    let id = request_id(&support::heard_requests(
        &heard,
        "workspace/willRenameFiles",
        1,
    ));
    app.answer_for_test(LanguageId::Rust, id, Ok(json!(null)));
    support::lay_out(&mut app, 76, 18);
    assert!(
        !app.is_waking(),
        "the screen is still being woken for a rename that has happened"
    );
}
