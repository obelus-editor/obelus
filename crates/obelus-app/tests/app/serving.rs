//! What the status row says about a language server.
//!
//! Against a stand-in rather than a real one: what is interesting is which
//! of the server's news reaches the row, and a real server cannot be made
//! to be halfway through reading a project on demand.

use obelus_app::{app::App, event::Event};
use obelus_syntax::LanguageId;
use serde_json::json;

use crate::support;

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
        app.stand_in_server_for_test(LanguageId::Rust, support::cat()),
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

/// A language server is started knowing where the machine's pool of build
/// jobs is, so that what it builds -- rust-analyzer's `cargo check` --
/// builds inside it.
///
/// Unix only, for the stand-in: a server here is a program with no
/// arguments, and on Windows a script is not a program.
///
/// Broken deliberately by leaving `obelus_jobs::lend` out of
/// `Client::start`: the server writes down `none`.
#[cfg(unix)]
#[test]
fn a_language_server_is_told_where_the_pool_of_build_jobs_is() {
    let scratch = support::Scratch::new("serving-pooled");
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(scratch.path().to_path_buf());
    let (sender, _events) = std::sync::mpsc::channel();
    app.events_for_test(sender);
    support::lay_out(&mut app, 100, 12);
    app.configure(obelus_config::Config::default(), Vec::new());
    assert!(
        app.stand_in_server_for_test(
            LanguageId::Rust,
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/pooled-server.sh"
            ),
        ),
        "the stand-in server would not start"
    );
    // Read until the server has written the whole of it: the file appears
    // a moment before its words do.
    let written = scratch.path().join("pooled");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let told = loop {
        let told = std::fs::read_to_string(&written).unwrap_or_default();
        if !told.is_empty() || std::time::Instant::now() > deadline {
            break told;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    // This binary's own pool, by its process's number: run from inside an
    // Obelus, the server would inherit the outer one's whether or not it
    // was told anything.
    assert!(
        told.contains(&format!("obelus-jobs-{}-", std::process::id())),
        "the server was not told where this pool is: {told:?}"
    );
}

/// The settings with the language servers switched as `on` says.
fn servers(on: bool) -> obelus_config::Config {
    obelus_config::Config {
        language_servers: on,
        ..obelus_config::Config::default()
    }
}

/// Turning the switch off stops what is running, and a change to any other
/// setting with it off leaves alone a server the reader started by hand.
///
/// The settings are applied after every change to any of them, so the
/// second half is the one that is easy to lose: a theme chosen with the
/// switch off would take `restart-server`'s answer away again.
///
/// Broken deliberately two ways. Taking out the loop that stops them when
/// the switch goes off leaves the server running at the end. And acting on
/// what the switch *says* rather than on its moving -- dropping the
/// `on == self.servers_on` return -- stops the hand-started one the moment
/// another setting changes.
#[test]
fn turning_the_servers_off_stops_them_and_nothing_else_does() {
    let (_scratch, mut app) = reading("switched");
    app.configure(servers(false), Vec::new());
    assert!(
        app.server_state().is_some(),
        "a change to the settings that left the switch where it was stopped the server"
    );

    app.configure(servers(true), Vec::new());
    assert!(app.server_state().is_some(), "turning them on stopped one");
    app.configure(servers(false), Vec::new());
    assert!(
        app.server_state().is_none(),
        "the switch is off and the server is still running"
    );
}

/// The whole way round against a real server, because what is being said
/// is which program Obelus *starts*: off, opening a file starts nothing;
/// on, the open files get their server; and `restart-server` starts one
/// with the switch off, because the reader asked for that one.
///
/// Skipped where rust-analyzer is not installed, like the other tests that
/// need the real thing. Broken deliberately three ways: `serve` asked to
/// start whatever the switch says, which starts one with it off; the
/// switch's turning on starting nothing for what is already open; and
/// `restart-server` going through `serve`, which starts nothing with the
/// switch off.
#[test]
fn the_switch_says_whether_opening_a_file_starts_a_server() {
    if !obelus_lsp::on_path("rust-analyzer") {
        eprintln!("skipped: rust-analyzer is not on PATH");
        return;
    }
    let scratch = support::Scratch::new("serving-the-switch");
    let path = scratch.path().join("main.rs");
    std::fs::write(&path, "fn main() {}\n").expect("the file");
    let mut app = App::new(vec![
        obelus_buffer::Buffer::open(&path).expect("opening it"),
    ]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    let (sender, _events) = std::sync::mpsc::channel();
    app.events_for_test(sender);
    support::lay_out(&mut app, 100, 12);

    app.serve_for_test();
    assert!(
        app.server_state().is_none(),
        "a server started with the switch off"
    );

    app.configure(servers(true), Vec::new());
    assert!(
        app.server_state().is_some(),
        "turning the switch on started nothing for the file that is open"
    );

    app.configure(servers(false), Vec::new());
    assert!(
        app.server_state().is_none(),
        "turning it off stopped nothing"
    );
    app.restart_server();
    assert!(
        app.server_state().is_some(),
        "restart-server started nothing with the switch off"
    );
    app.configure(servers(true), Vec::new());
    app.configure(servers(false), Vec::new());
}

/// Turning the switch on does not open a file a second time with a server
/// that already has it -- one the reader started by hand while the switch
/// was off. The protocol does not let a file be opened twice without being
/// closed between, and a server may take it for an error.
///
/// Broken deliberately by serving every open file when the switch goes on,
/// whatever is running: the stand-in hears a second `didOpen`.
#[test]
fn turning_the_servers_on_opens_nothing_twice() {
    let scratch = support::Scratch::new("serving-opened-once");
    let path = scratch.path().join("main.rs");
    std::fs::write(&path, "fn main() {}\n").expect("the file");
    let mut app = App::new(vec![
        obelus_buffer::Buffer::open(&path).expect("opening it"),
    ]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    let (sender, heard) = obelus_app::event::channel();
    app.events_for_test(sender);
    support::lay_out(&mut app, 100, 12);
    assert!(
        app.stand_in_server_for_test(LanguageId::Rust, support::cat()),
        "the stand-in server would not start"
    );
    app.serve_current_for_test();
    app.declared_for_test(LanguageId::Rust, json!({}));
    assert_eq!(
        support::heard_requests(&heard, "textDocument/didOpen", 1).len(),
        1,
        "the file was never opened with the server"
    );

    app.configure(servers(true), Vec::new());
    let again = support::heard_requests(&heard, "textDocument/didOpen", 1);
    assert!(again.is_empty(), "the file was opened again: {again:?}");
}
