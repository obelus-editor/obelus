//! What the status row says about a language server.
//!
//! Against a stand-in rather than a real one: what is interesting is which
//! of the server's news reaches the row, and a real server cannot be made
//! to be halfway through reading a project on demand.

mod support;

use obelus_app::{app::App, event::Event};
use obelus_syntax::LanguageId;
use serde_json::json;

/// A file with a server Obelus knows the command for.
fn reading(name: &str) -> (support::Scratch, App) {
    let scratch = support::Scratch::new(&format!("serving-{name}"));
    let path = scratch.path().join("main.rs");
    std::fs::write(&path, "fn main() {}\n").expect("the file");
    let mut app = App::new(vec![
        obelus_buffer::Buffer::open(&path).expect("opening it"),
    ]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, 100, 12);
    assert!(
        app.stand_in_server_for_test(LanguageId::Rust, "cat"),
        "the stand-in server would not start"
    );
    (scratch, app)
}

/// What a server says it is doing does not go on the status row; that it is
/// doing something does.
///
/// `rust-analyzer` sends a few hundred of these over a cold start -- every
/// crate it scans, every file it indexes -- and each of them landed between
/// the file's name and the cursor's position, which are the two things a
/// reader looks at that row to read. They flickered for a minute and said
/// nothing the reader could act on.
///
/// What is left is the bit: the badge that names the server turns while it
/// is busy. Which is what the words were really for -- an empty answer
/// while a server is reading the project and an empty answer about a symbol
/// with no definition are the same message on the wire, and the row is the
/// only thing that tells them apart.
///
/// Broken deliberately two ways. Putting `server_working_on` back into the
/// row's `middle` shows the message again. And drawing the badge's own
/// glyph while it is busy leaves the row saying nothing about it at all.
#[test]
fn a_busy_server_turns_rather_than_talking() {
    let (_scratch, mut app) = reading("busy");
    let quiet = support::render(&mut app, 100, 12);

    app.handle(Event::Lsp(obelus_lsp::Message {
        language: LanguageId::Rust,
        message: json!({
            "jsonrpc": "2.0", "method": "$/progress",
            "params": {
                "token": "rustAnalyzer/Indexing",
                "value": { "kind": "begin", "title": "Indexing", "message": "1/240 (core)" }
            }
        }),
    }));
    assert!(app.server_busy(), "Obelus did not notice it was busy");

    let busy = support::render(&mut app, 100, 12);
    for said in ["Indexing", "1/240", "core"] {
        assert!(
            !busy.contains(said),
            "the server's own commentary is on the row: {said:?}\n{busy}"
        );
    }
    // The badge turns instead, in braille -- which is drawn whether or not
    // glyphs are, like every other mark in Obelus that turns.
    let turning = |dump: &str| {
        support::text_block(dump)
            .chars()
            .any(|character| ('\u{2800}'..='\u{28ff}').contains(&character))
    };
    assert!(turning(&busy), "nothing on the row is turning:\n{busy}");
    assert!(
        !turning(&quiet),
        "it was turning before it was busy:\n{quiet}"
    );

    // And it stops when the server does.
    app.handle(Event::Lsp(obelus_lsp::Message {
        language: LanguageId::Rust,
        message: json!({
            "jsonrpc": "2.0", "method": "$/progress",
            "params": { "token": "rustAnalyzer/Indexing", "value": { "kind": "end" } }
        }),
    }));
    assert!(!app.server_busy());
    let done = support::render(&mut app, 100, 12);
    assert!(!turning(&done), "it is still turning:\n{done}");
}

/// What a server says went wrong reaches the reader; the rest of its talk
/// does not.
///
/// All four kinds of `window/showMessage` went to the log at `debug` and
/// nowhere else, which for the loudest of them is the wrong place: a
/// workspace `rust-analyzer` could not discover is why every question for
/// the rest of the session comes back empty, and a reader who cannot see
/// that is a reader wondering what they broke. The other three are a
/// diary, and the log is what a diary is for.
///
/// Broken deliberately by taking the `type` check out, which puts the
/// diary on the row -- the second message here lands there too.
#[test]
fn only_what_a_server_calls_an_error_reaches_the_row() {
    let (_scratch, mut app) = reading("complaining");
    let said = |app: &mut App, kind: i64, text: &str| {
        app.handle(Event::Lsp(obelus_lsp::Message {
            language: LanguageId::Rust,
            message: json!({
                "jsonrpc": "2.0", "method": "window/showMessage",
                "params": { "type": kind, "message": text }
            }),
        }));
    };

    // An error, which the reader has to see.
    said(&mut app, 1, "failed to discover workspace");
    let dump = support::render(&mut app, 100, 12);
    assert!(
        dump.contains("failed to discover workspace"),
        "the reader was not told:\n{dump}"
    );
    // Named, because the row says nothing else about who is complaining.
    assert!(
        dump.contains("rust-analyzer: failed"),
        "it does not say which server said it:\n{dump}"
    );

    // And a remark, which they do not.
    said(&mut app, 3, "building proc-macros: serde");
    let dump = support::render(&mut app, 100, 12);
    assert!(
        !dump.contains("proc-macros"),
        "the server's diary is on the row:\n{dump}"
    );
}
