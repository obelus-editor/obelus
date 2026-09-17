//! Making the changes a server asks for: a rename, and the things it
//! offers to do about where the reader is.
//!
//! Against values rather than against a server: what is interesting is
//! which files obelus changes and how -- every one of them opened and
//! edited, none of them written, one act of undo for the lot -- and a
//! server cannot be made to offer a particular edit on demand.

mod support;

use crossterm::event::KeyCode;
use obelus::{app::App, buffer::Buffer};
use serde_json::json;

/// Two files, one of them open.
fn project(
    name: &str,
) -> (
    support::Scratch,
    App,
    std::path::PathBuf,
    std::path::PathBuf,
) {
    let scratch = support::Scratch::new(name);
    let open = scratch.path().join("open.rs");
    let closed = scratch.path().join("closed.rs");
    std::fs::write(&open, "fn thing() {}\n\nfn main() {\n    thing();\n}\n").expect("writing it");
    std::fs::write(&closed, "fn other() {\n    super::thing();\n}\n").expect("writing it");
    let mut app = App::new(vec![Buffer::open(&open).expect("opening it")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, 76, 18);
    (scratch, app, open, closed)
}

fn text_of(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).expect("reading it")
}

fn open_text(app: &App) -> String {
    app.current_buffer()
        .expect("a buffer")
        .text()
        .rope()
        .to_string()
}

/// An edit naming both files, in the shape a server sends.
fn renaming(open: &std::path::Path, closed: &std::path::Path) -> serde_json::Value {
    json!({
        "changes": {
            format!("file://{}", open.display()): [
                { "range": { "start": { "line": 0, "character": 3 },
                             "end": { "line": 0, "character": 8 } }, "newText": "widget" },
                { "range": { "start": { "line": 3, "character": 4 },
                             "end": { "line": 3, "character": 9 } }, "newText": "widget" }
            ],
            format!("file://{}", closed.display()): [
                { "range": { "start": { "line": 1, "character": 11 },
                             "end": { "line": 1, "character": 16 } }, "newText": "widget" }
            ]
        }
    })
}

/// Every file a rename touches becomes a document: the change lands in
/// the reader's undo, it is on screen, and nothing reaches the disk until
/// they say so.
#[test]
fn every_file_a_rename_touches_is_opened_and_left_unwritten() {
    let (_scratch, mut app, open, closed) = project("rename-both");
    let before = app.file_count_for_test();
    app.rename_for_test(renaming(&open, &closed));

    assert_eq!(
        open_text(&app),
        "fn widget() {}\n\nfn main() {\n    widget();\n}\n",
        "the open file was not edited"
    );
    assert_eq!(
        app.file_count_for_test(),
        before + 1,
        "the file that was not open did not become a document"
    );
    // Neither file is on disk yet: a rename is a change the reader looks
    // at before it is written, like every other change.
    assert_eq!(
        text_of(&open),
        "fn thing() {}\n\nfn main() {\n    thing();\n}\n",
        "the open file was written without being asked"
    );
    assert_eq!(
        text_of(&closed),
        "fn other() {\n    super::thing();\n}\n",
        "the other file was written without being asked"
    );
    // And the one the reader cannot see says so: it is unwritten, and the
    // keys that leave obelus ask about it.
    assert!(
        app.buffers_for_test().iter().all(|(_, dirty)| *dirty),
        "a file the rename changed is not marked unsaved: {:?}",
        app.buffers_for_test()
    );

    // The status row says what happened, by the numbers. Read before the
    // undo below: a key press is obelus taking it that whatever it had to
    // say has been read.
    let note = app.note().unwrap_or_default().to_string();
    assert!(
        note.contains('2') && note.contains("nothing written"),
        "the note does not say what changed: {note:?}"
    );

    // One key was pressed, so one press of undo takes it back -- in the
    // file the reader can see, which is the one the key is about.
    support::press_control(&mut app, 'z');
    assert_eq!(
        open_text(&app),
        "fn thing() {}\n\nfn main() {\n    thing();\n}\n"
    );
}

/// An edit the server asks for unprompted is made, and the server is told
/// so truthfully.
///
/// This is how the refactorings a server works out for itself arrive: the
/// action carried a command rather than an edit, obelus asked the server
/// to run it, and the change comes back the other way round.
#[test]
fn an_edit_the_server_asks_for_is_made_and_the_answer_says_so() {
    let (_scratch, mut app, open, closed) = project("apply-edit");
    let answer = app.asked_edit_for_test(renaming(&open, &closed));

    assert_eq!(
        answer["result"]["applied"],
        json!(true),
        "the server was told its edit did not happen: {answer}"
    );
    assert_eq!(
        open_text(&app),
        "fn widget() {}\n\nfn main() {\n    widget();\n}\n",
        "the edit the server asked for was not made"
    );
    // The same rules as every other change: on screen, undoable, unwritten.
    assert_eq!(
        text_of(&open),
        "fn thing() {}\n\nfn main() {\n    thing();\n}\n",
        "the file was written without being asked"
    );
    support::press_control(&mut app, 'z');
    assert!(open_text(&app).contains("fn thing()"), "there is no undo");
}

/// And it arrives the way a server sends it: a message on the pipe,
/// carrying a request rather than an answer to anything obelus asked.
#[test]
fn an_edit_from_the_server_walks_the_whole_way_in() {
    use obelus::{event::Event, syntax::LanguageId};

    let (_scratch, mut app, open, closed) = project("apply-event");
    assert!(
        app.stand_in_server_for_test(LanguageId::Rust, "cat"),
        "the stand-in server would not start"
    );
    app.handle(Event::Lsp {
        language: LanguageId::Rust,
        message: json!({
            "jsonrpc": "2.0", "id": 3, "method": "workspace/applyEdit",
            "params": { "edit": renaming(&open, &closed) }
        }),
    });
    assert_eq!(
        open_text(&app),
        "fn widget() {}\n\nfn main() {\n    widget();\n}\n",
        "an edit that came in the ordinary way was not made"
    );
}

/// And one obelus will not make is declined rather than claimed: a server
/// told its refactoring landed goes on to the next step of it.
#[test]
fn an_edit_obelus_will_not_make_is_declined() {
    let (_scratch, mut app, open, _closed) = project("apply-refused");
    let answer = app.asked_edit_for_test(json!({
        "documentChanges": [
            { "kind": "delete", "uri": format!("file://{}", open.display()) }
        ]
    }));

    assert_eq!(
        answer["result"]["applied"],
        json!(false),
        "obelus said it had deleted a file: {answer}"
    );
    assert!(
        answer["result"]["failureReason"]
            .as_str()
            .is_some_and(|why| why.contains("deleting a file")),
        "the server was not told what obelus would not do: {answer}"
    );
    assert!(open.exists(), "the file was deleted");
}

/// A save asks what the server would do to the whole file, and says
/// which kinds it means.
///
/// The only code action obelus sends without being asked, so it is the
/// one that has to say `only`: a request with no kind on it comes back
/// with every refactoring near the cursor, and applying one of those to a
/// file somebody pressed save on would be obelus rewriting their code on
/// its own initiative.
#[test]
fn a_save_asks_only_for_what_a_server_does_to_a_whole_file() {
    use obelus::syntax::LanguageId;

    let (_scratch, mut app, open, _closed) = project("save-imports");
    let (sender, heard) = obelus::event::channel();
    app.events_for_test(sender);
    assert!(
        app.stand_in_server_for_test(LanguageId::Rust, "cat"),
        "the echo would not start"
    );
    app.declared_for_test(LanguageId::Rust, json!({ "codeActionProvider": true }));
    app.configure(
        obelus::config::Config {
            code_actions_on_save: true,
            ..obelus::config::Config::default()
        },
        Vec::new(),
    );

    support::type_text(&mut app, "\n");
    support::press_control(&mut app, 's');

    // Nothing is written yet: the save is waiting on the answer.
    assert_eq!(
        text_of(&open),
        "fn thing() {}\n\nfn main() {\n    thing();\n}\n",
        "the file was written before the server had answered"
    );

    // Every kind in turn, each asked only once the last has been made:
    // two edits out of one answer would be two edits worked out against
    // the same text, and the first of them moves what the second is
    // measured against.
    let kinds = App::kinds_asked_on_save_for_test();
    for (at, kind) in kinds.iter().enumerate() {
        app.saving_for_test(
            at,
            json!([
                { "title": "Whatever this one is", "kind": kind,
                  "edit": { "changes": { format!("file://{}", open.display()): [
                      { "range": { "start": { "line": 0, "character": 0 },
                                   "end": { "line": 0, "character": 0 } },
                        "newText": format!("// {kind}\n") }
                  ] } } }
            ]),
        );
    }

    // One request per kind, each naming its own and only its own.
    let asked = heard_requests(&heard, "textDocument/codeAction", kinds.len());
    assert_eq!(
        asked.len(),
        kinds.len(),
        "the save did not ask about every kind: {asked:?}"
    );
    let named: Vec<String> = asked
        .iter()
        .map(|one| {
            let only = one["params"]["context"]["only"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            assert_eq!(only.len(), 1, "a save asked for more than one kind at once");
            only[0].as_str().unwrap_or_default().to_string()
        })
        .collect();
    assert_eq!(named, kinds, "not the kinds a save is meant to ask about");

    // Every edit reached the file, and the file reached the disk: each
    // answer made its own change and then picked the save back up.
    let written = text_of(&open);
    for kind in kinds {
        assert!(
            written.contains(&format!("// {kind}\n")),
            "what the server said about {kind} is not in the file: {written:?}"
        );
    }
    // And the question was about the whole file, with nothing of the
    // reader's cursor in it.
    assert_eq!(
        asked[0]["params"]["range"]["start"],
        json!({ "line": 0, "character": 0 }),
        "the question is not about the whole file: {:?}",
        asked[0]
    );
    assert!(
        asked[0]["params"]["context"]["diagnostics"]
            .as_array()
            .is_some_and(Vec::is_empty),
        "a question about the whole file carried diagnostics: {:?}",
        asked[0]
    );
    assert!(
        written.contains("fn thing() {}"),
        "the file lost what was in it: {written:?}"
    );
    assert!(
        !app.current_buffer().expect("a buffer").is_dirty(),
        "the save never finished"
    );
}

/// A kind the server has nothing for is not the end of the save.
///
/// Most servers answer most of these with nothing, so this is the
/// ordinary path rather than the odd one: the save has to walk past it to
/// the next kind and then to the writing.
#[test]
fn a_kind_the_server_has_nothing_for_does_not_end_the_save() {
    use obelus::syntax::LanguageId;

    let (_scratch, mut app, open, _closed) = project("save-nothing");
    let (sender, heard) = obelus::event::channel();
    app.events_for_test(sender);
    assert!(app.stand_in_server_for_test(LanguageId::Rust, "cat"));
    app.declared_for_test(LanguageId::Rust, json!({ "codeActionProvider": true }));
    app.configure(
        obelus::config::Config {
            code_actions_on_save: true,
            ..obelus::config::Config::default()
        },
        Vec::new(),
    );

    support::type_text(&mut app, "\n");
    support::press_control(&mut app, 's');

    // Nothing for any of them, which is what rust-analyzer says to all of
    // these -- and the file still has to be written.
    let kinds = App::kinds_asked_on_save_for_test();
    for at in 0..kinds.len() {
        app.saving_for_test(at, json!([]));
    }
    assert_eq!(
        heard_requests(&heard, "textDocument/codeAction", kinds.len()).len(),
        kinds.len(),
        "a kind with nothing to offer ended the save"
    );
    assert!(
        text_of(&open).starts_with('\n'),
        "the file was never written: {:?}",
        text_of(&open)
    );
    assert!(
        !app.current_buffer().expect("a buffer").is_dirty(),
        "the save never finished"
    );
}

/// A save does not take an offer the server said it will not carry out.
///
/// Nobody is watching a save the way they watch a menu: the row that
/// would have been dim is not on screen at all, so the only thing that
/// stops obelus doing what the server said cannot be done is the save
/// itself checking.
#[test]
fn a_save_passes_over_an_offer_the_server_will_not_carry_out() {
    use obelus::syntax::LanguageId;

    let (_scratch, mut app, open, _closed) = project("save-disabled");
    let (sender, heard) = obelus::event::channel();
    app.events_for_test(sender);
    assert!(app.stand_in_server_for_test(LanguageId::Rust, "cat"));
    app.declared_for_test(LanguageId::Rust, json!({ "codeActionProvider": true }));
    app.configure(
        obelus::config::Config {
            code_actions_on_save: true,
            ..obelus::config::Config::default()
        },
        Vec::new(),
    );

    support::type_text(&mut app, "\n");
    support::press_control(&mut app, 's');

    // An offer of the right kind, carrying an edit, that the server has
    // marked as one it will not make. Taking it would be obelus doing
    // what it was told could not be done.
    let kinds = App::kinds_asked_on_save_for_test();
    for (at, kind) in kinds.iter().enumerate() {
        app.saving_for_test(
            at,
            json!([
                { "title": "Something that cannot be done", "kind": kind,
                  "disabled": { "reason": "the file is generated" },
                  "edit": { "changes": { format!("file://{}", open.display()): [
                      { "range": { "start": { "line": 0, "character": 0 },
                                   "end": { "line": 0, "character": 0 } },
                        "newText": "// SHOULD NOT BE HERE\n" }
                  ] } } }
            ]),
        );
    }

    let written = text_of(&open);
    assert!(
        !written.contains("SHOULD NOT BE HERE"),
        "the save made an edit the server said it would not: {written:?}"
    );
    // And it still asked about every kind and still wrote the file: a
    // refusal is not the end of the save.
    assert_eq!(
        heard_requests(&heard, "textDocument/codeAction", kinds.len()).len(),
        kinds.len(),
        "a refused offer ended the save"
    );
    assert!(
        written.starts_with('\n'),
        "the file was never written: {written:?}"
    );
}

/// And with the switch off it asks nobody anything.
#[test]
fn a_save_leaves_the_imports_alone_unless_asked() {
    use obelus::syntax::LanguageId;

    let (_scratch, mut app, open, _closed) = project("save-plain");
    let (sender, heard) = obelus::event::channel();
    app.events_for_test(sender);
    assert!(app.stand_in_server_for_test(LanguageId::Rust, "cat"));
    app.declared_for_test(LanguageId::Rust, json!({ "codeActionProvider": true }));

    support::type_text(&mut app, "\n");
    support::press_control(&mut app, 's');

    assert!(
        heard_requests(&heard, "textDocument/codeAction", 1).is_empty(),
        "a setting nobody turned on asked a server something"
    );
    assert!(
        text_of(&open).starts_with('\n'),
        "the file was not written: {:?}",
        text_of(&open)
    );
}

/// Every edit lands where the server wrote it, which means making them
/// from the end backwards.
#[test]
fn edits_to_one_file_do_not_move_each_other() {
    let scratch = support::Scratch::new("rename-order");
    let path = scratch.path().join("one.rs");
    std::fs::write(&path, "aaa bbb ccc\n").expect("writing it");
    let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, 76, 18);

    // Three edits on one line, each longer than what it replaces: made
    // front to back, every one after the first would land short.
    app.rename_for_test(json!({
        "changes": {
            format!("file://{}", path.display()): [
                { "range": { "start": { "line": 0, "character": 0 },
                             "end": { "line": 0, "character": 3 } }, "newText": "first" },
                { "range": { "start": { "line": 0, "character": 4 },
                             "end": { "line": 0, "character": 7 } }, "newText": "second" },
                { "range": { "start": { "line": 0, "character": 8 },
                             "end": { "line": 0, "character": 11 } }, "newText": "third" }
            ]
        }
    }));
    assert_eq!(open_text(&app), "first second third\n");
}

/// What obelus will not do, said rather than done.
#[test]
fn moving_a_file_is_refused_and_reported() {
    let (_scratch, mut app, open, _closed) = project("rename-refused");
    app.rename_for_test(json!({
        "documentChanges": [
            { "textDocument": { "uri": format!("file://{}", open.display()), "version": null },
              "edits": [ { "range": { "start": { "line": 0, "character": 3 },
                                      "end": { "line": 0, "character": 8 } },
                           "newText": "widget" } ] },
            { "kind": "rename",
              "oldUri": format!("file://{}", open.display()),
              "newUri": format!("file://{}", open.with_file_name("moved.rs").display()) }
        ]
    }));

    // The edit was made, the move was not, and the reader is told.
    assert!(open_text(&app).starts_with("fn widget()"));
    assert!(open.exists(), "the file was moved");
    let note = app.note().unwrap_or_default().to_string();
    assert!(
        note.contains("moving a file"),
        "the refusal is not on the status row: {note:?}"
    );
}

/// An answer about a document that has moved since is refused: the places
/// it names are places in the file as it was.
#[test]
fn a_rename_of_a_file_that_has_changed_is_refused() {
    let (_scratch, mut app, open, closed) = project("rename-stale");
    let answer = renaming(&open, &closed);
    support::press(&mut app, KeyCode::End);
    support::type_text(&mut app, "x");
    let version = app.current_buffer().expect("a buffer").version();

    app.rename_at_version_for_test(answer, version - 1);
    assert!(
        open_text(&app).contains("thing"),
        "a rename worked out against the file as it was went in anyway"
    );
    assert!(
        app.note().unwrap_or_default().contains("changed while"),
        "the reader was not told why nothing happened"
    );
}

/// Offers worked out against a file that has moved since are refused.
///
/// The ranges in them are places in the text as it was, and the damage a
/// stale one does is quiet: an old line and column is usually still a
/// real place in the new text, so the edit lands somewhere wrong rather
/// than failing. The same guard a rename has, at both of this one's two
/// windows -- the list, and the round trip that fills an offer in.
#[test]
fn offers_about_a_file_that_has_changed_are_refused() {
    let (_scratch, mut app, open, _closed) = project("actions-stale");
    let offer = json!([
        { "title": "Remove unused import", "kind": "quickfix",
          "edit": { "changes": { format!("file://{}", open.display()): [
              { "range": { "start": { "line": 0, "character": 0 },
                           "end": { "line": 1, "character": 0 } }, "newText": "" }
          ] } } }
    ]);
    support::press(&mut app, KeyCode::End);
    support::type_text(&mut app, "x");
    let version = app.current_buffer().expect("a buffer").version();

    app.actions_at_version_for_test(offer.clone(), version - 1);
    assert!(app.picker().is_none(), "offers about the file as it was");
    assert!(
        app.note().unwrap_or_default().contains("changed while"),
        "the reader was not told why nothing happened"
    );

    // And the longer window: the list has closed, the server is filling
    // the offer in, and the reader has the keys back the whole time.
    app.actions_for_test(offer);
    support::press(&mut app, KeyCode::Esc);
    support::type_text(&mut app, "y");
    let text = open_text(&app);
    app.action_at_version_for_test(
        json!({ "title": "Remove unused import", "kind": "quickfix",
                "edit": { "changes": { format!("file://{}", open.display()): [
                    { "range": { "start": { "line": 0, "character": 0 },
                                 "end": { "line": 1, "character": 0 } }, "newText": "" }
                ] } } }),
        version,
    );
    assert_eq!(
        open_text(&app),
        text,
        "an edit worked out against the file as it was went in anyway"
    );
}

/// The things a server offers to do are a list, and choosing one does it.
#[test]
fn what_can_be_done_here_is_a_list_and_choosing_one_does_it() {
    let (_scratch, mut app, open, _closed) = project("actions-list");
    app.actions_for_test(json!([
        { "title": "Remove unused import", "kind": "quickfix",
          "edit": { "changes": { format!("file://{}", open.display()): [
              { "range": { "start": { "line": 0, "character": 0 },
                           "end": { "line": 1, "character": 0 } }, "newText": "" }
          ] } } },
        { "title": "Extract into function", "kind": "refactor.extract" }
    ]));

    let dump = support::render(&mut app, 76, 18);
    assert!(
        dump.contains("Remove unused import") && dump.contains("Extract into function"),
        "the offers are not on screen:\n{dump}"
    );

    support::press(&mut app, KeyCode::Enter);
    assert!(app.picker().is_none(), "the list stayed open");
    assert_eq!(
        open_text(&app),
        "\nfn main() {\n    thing();\n}\n",
        "the offer that was chosen was not carried out"
    );
}

/// An offer the server will not carry out is shown, with its reason, and
/// cannot be chosen.
///
/// The point of it is that the reader learns the thing exists: a server
/// that left it out would leave them never finding out obelus can extract
/// a function, because the one time they wanted it their selection was
/// wrong. So the row is there, the reason is beside it, and stepping
/// through the list goes past it.
#[test]
fn an_offer_the_server_will_not_carry_out_is_shown_and_not_offered() {
    let (_scratch, mut app, open, _closed) = project("actions-disabled");
    app.actions_for_test(json!([
        { "title": "Extract into function", "kind": "refactor.extract",
          "disabled": { "reason": "the selection crosses a `?`" } },
        { "title": "Remove unused import", "kind": "quickfix",
          "edit": { "changes": { format!("file://{}", open.display()): [
              { "range": { "start": { "line": 0, "character": 0 },
                           "end": { "line": 1, "character": 0 } }, "newText": "" }
          ] } } }
    ]));

    let picker = app.picker().expect("the list");
    let rows: Vec<(String, Option<String>, bool)> = picker
        .matches()
        .map(|item| (item.label.clone(), item.detail.clone(), item.enabled))
        .collect();
    assert_eq!(
        rows,
        [
            ("Remove unused import".to_string(), None, true),
            (
                "Extract into function".to_string(),
                Some("the selection crosses a `?`".to_string()),
                false
            ),
        ],
        "the one that cannot be done is not last, not dim, or not saying why"
    );

    // Enter reaches the one that can be done, because the list steps over
    // the one that cannot.
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(
        open_text(&app),
        "\nfn main() {\n    thing();\n}\n",
        "the row the list can reach was not the one that was carried out"
    );
}

/// And where every offer is one the server will not carry out, the
/// reasons are the answer rather than a list nobody can use.
#[test]
fn offers_that_can_all_be_refused_are_said_rather_than_listed() {
    let (_scratch, mut app, _open, _closed) = project("actions-all-disabled");
    app.actions_for_test(json!([
        { "title": "Extract into function", "kind": "refactor.extract",
          "disabled": { "reason": "the selection crosses a `?`" } },
        { "title": "Inline variable", "kind": "refactor.inline",
          "disabled": { "reason": "it is used in a macro" } }
    ]));

    assert!(
        app.picker().is_none(),
        "a list opened in which every row is one the reader steps over"
    );
    let note = app.note().unwrap_or_default().to_string();
    assert!(
        note.contains("crosses a `?`") && note.contains("used in a macro"),
        "the reader was not told why nothing can be done: {note:?}"
    );
}

/// A server that marks one of them as the obvious one has said which it
/// would pick, and that is the row the list opens on.
#[test]
fn the_one_the_server_would_pick_is_first() {
    let (_scratch, mut app, _open, _closed) = project("actions-preferred");
    app.actions_for_test(json!([
        { "title": "Something else", "kind": "refactor" },
        { "title": "The obvious one", "kind": "quickfix", "isPreferred": true }
    ]));
    let picker = app.picker().expect("the list");
    assert_eq!(
        picker
            .matches()
            .map(|item| item.label.clone())
            .collect::<Vec<_>>(),
        ["The obvious one", "Something else"],
        "the one the server prefers is not first"
    );
}

/// Nothing offered is a sentence, not an empty list: a list with no rows
/// covers the code to say there was nothing to say.
#[test]
fn nothing_offered_is_said_rather_than_listed() {
    let (_scratch, mut app, _open, _closed) = project("actions-none");
    app.actions_for_test(json!([]));
    assert!(app.picker().is_none(), "an empty list was opened");
    assert!(
        app.note().unwrap_or_default().contains("nothing to do"),
        "the reader was not told"
    );
}

/// The messages of a kind that obelus wrote, as the echo gave them back.
///
/// Waits for `want` of them and then stops waiting, so a test that
/// expects none pays a moment and a test that expects two does not: there
/// is no event to wait for when the point is that none is coming.
fn heard_requests(
    heard: &std::sync::mpsc::Receiver<obelus::event::Event>,
    method: &str,
    want: usize,
) -> Vec<serde_json::Value> {
    let mut seen = Vec::new();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while seen.len() < want.max(1) {
        let Some(left) = until.checked_duration_since(std::time::Instant::now()) else {
            break;
        };
        match heard.recv_timeout(left) {
            Ok(obelus::event::Event::Lsp { message, .. })
                if message.get("method").and_then(serde_json::Value::as_str) == Some(method) =>
            {
                seen.push(message);
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    seen
}
