//! Against a real language server.
//!
//! Everything else about the protocol is tested against values rather than
//! against a server, which is right: the interesting rules are about what to
//! do with an answer, and a server cannot be made to give a stale one on
//! demand. What no amount of that catches is an assumption about real servers
//! being wrong — that they will negotiate byte offsets, that they declare the
//! questions they answer — and those are assumptions obelus is built on.
//!
//! Skipped where rust-analyzer is not installed. The handshake is fast enough
//! to run every time; waiting for a project to be indexed is not, so the round
//! trip through a query is marked `ignore` and run with
//! `cargo test -- --ignored`.

use std::{
    sync::mpsc::{Receiver, RecvTimeoutError},
    time::{Duration, Instant},
};

use obelus::{
    event::Event,
    lsp::{
        action::{self, SymbolAction},
        client::{Client, Reply},
    },
    syntax::LanguageId,
};

/// Long enough for a cold cargo metadata on a slow machine.
const HANDSHAKE: Duration = Duration::from_secs(30);

/// Long enough for this project to be indexed. Measured at about three
/// seconds warm.
const INDEXED: Duration = Duration::from_secs(120);

fn root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Pumps messages into the client until `done` says so, or the deadline.
fn pump<F>(
    client: &mut Client,
    events: &Receiver<Event>,
    limit: Duration,
    mut done: F,
) -> Option<Reply>
where
    F: FnMut(&Client, Option<&Reply>) -> bool,
{
    let deadline = Instant::now() + limit;
    loop {
        if done(client, None) {
            return None;
        }
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            panic!("the server said nothing useful within {limit:?}");
        };
        match events.recv_timeout(remaining) {
            Ok(Event::Lsp { message, .. }) => {
                if let Some(reply) = client.on_message(&message)
                    && done(client, Some(&reply))
                {
                    return Some(reply);
                }
            }
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout) => panic!("the server went quiet"),
            Err(RecvTimeoutError::Disconnected) => panic!("the server ended"),
        }
    }
}

fn start() -> Option<(Client, Receiver<Event>)> {
    if !obelus::lsp::on_path("rust-analyzer") {
        eprintln!("skipped: rust-analyzer is not on PATH");
        return None;
    }
    let (sender, events) = obelus::event::channel();
    let server = obelus::lsp::server_for(LanguageId::Rust).expect("a server for Rust");
    let client =
        Client::start(LanguageId::Rust, server, &root(), sender).expect("starting rust-analyzer");
    Some((client, events))
}

/// The assumption the whole coordinate story rests on: offered bytes and
/// UTF-16, a real server takes bytes. If it stopped, every position obelus
/// sends would be in the wrong units and every jump would land a few
/// characters off on any line that is not plain ASCII.
#[test]
fn a_real_server_negotiates_byte_offsets() {
    let Some((mut client, events)) = start() else {
        return;
    };
    pump(&mut client, &events, HANDSHAKE, |client, _| {
        client.is_ready()
    });

    assert_eq!(
        client.encoding(),
        &lsp_types::PositionEncodingKind::UTF8,
        "a server that will not count bytes puts obelus on the UTF-16 path"
    );
    client.shutdown();
}

/// The menu lists what the server says it answers, so a server that stopped
/// declaring these would leave it empty rather than wrong — but empty is
/// still a broken reader.
#[test]
fn a_real_server_declares_the_questions_the_menu_offers() {
    let Some((mut client, events)) = start() else {
        return;
    };
    pump(&mut client, &events, HANDSHAKE, |client, _| {
        client.is_ready()
    });

    let capabilities = client.capabilities().expect("ready means capabilities");
    for action in action::ALL {
        assert!(
            action.supported(capabilities),
            "rust-analyzer no longer declares {}",
            action.title()
        );
    }
    client.shutdown();
}

/// The whole way through: open a document, ask where a symbol is defined, and
/// get a place in the file that defines it.
///
/// Ignored by default because it waits for the project to be indexed. A
/// server answers with nothing until then, which is the same answer it gives
/// for a symbol that has no definition — so the retry here is the same thing
/// obelus does, and its absence would make this test assert that indexing is
/// instant.
#[test]
#[ignore = "waits for the project to be indexed"]
fn a_real_server_finds_a_definition() {
    let Some((mut client, events)) = start() else {
        return;
    };
    pump(&mut client, &events, HANDSHAKE, |client, _| {
        client.is_ready()
    });

    // `buffer::BufferId` on line three of jump.rs, at column twelve.
    let path = root().join("src/jump.rs");
    let text = std::fs::read_to_string(&path).expect("reading the file");
    let uri = obelus::lsp::client::uri_for(&path).expect("a uri");

    client
        .notify(
            "textDocument/didOpen",
            &serde_json::json!({
                "textDocument": {
                    "uri": uri, "languageId": "rust", "version": 1, "text": text,
                }
            }),
        )
        .expect("opening the document");

    let deadline = Instant::now() + INDEXED;
    loop {
        assert!(Instant::now() < deadline, "never indexed");
        let id = client
            .request(
                SymbolAction::Definition.method(),
                &serde_json::json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": 3, "character": 12 },
                }),
            )
            .expect("asking");

        let reply = pump(&mut client, &events, INDEXED, |_, reply| {
            reply.is_some_and(|reply| reply.id == id)
        })
        .expect("an answer");

        let indexing = client.working_on().is_some();
        match action::outcome_of(reply.result, 1, Some(1), indexing) {
            action::Outcome::Places(places) => {
                let named = &places[0];
                assert!(
                    named.path.ends_with("src/buffer.rs"),
                    "expected buffer.rs, got {}",
                    named.path.display()
                );
                break;
            }
            // Still reading the project. Asking again is what obelus does.
            action::Outcome::NotYet => std::thread::sleep(Duration::from_millis(300)),
            // Finished reading, and still nothing. `BufferId` has a
            // definition, so this is the server never having heard of the
            // document rather than a symbol without one.
            action::Outcome::Nothing => {
                panic!("the server has finished indexing and found no definition")
            }
            other => panic!("{other:?}"),
        }
    }
    client.shutdown();
}

/// Anything sent before the handshake finishes is held until it does.
///
/// The queue has no other observable effect: rust-analyzer reads the
/// workspace from disk, so it answers the same whether or not it was told a
/// file is open. What is being checked is that obelus does not send a server
/// something it is entitled to refuse — so the check is on what has not gone
/// out yet, not on what came back.
#[test]
fn messages_sent_before_the_handshake_are_held_until_it_finishes() {
    let Some((mut client, events)) = start() else {
        return;
    };
    assert!(!client.is_ready(), "the handshake cannot be done already");

    let before = client.sent();
    client
        .notify("textDocument/didOpen", &serde_json::json!({}))
        .expect("queueing");
    assert_eq!(client.queued(), 1, "sent to a server that has not answered");
    assert_eq!(client.sent(), before, "it went out before the handshake");

    pump(&mut client, &events, HANDSHAKE, |client, _| {
        client.is_ready()
    });
    assert_eq!(client.queued(), 0, "still held after the handshake");
    // Emptying the queue without sending it would leave the queue empty too,
    // which is why this counts what went out rather than what is left.
    assert!(
        client.sent() >= before + 2,
        "the queue was emptied without being sent: {before} then {}",
        client.sent()
    );

    // And after the handshake, nothing is held.
    let after = client.sent();
    client
        .notify("textDocument/didClose", &serde_json::json!({}))
        .expect("sending");
    assert_eq!(client.queued(), 0);
    assert_eq!(client.sent(), after + 1);
    client.shutdown();
}

/// The badge on the status bar claims a server is running, so what it reads
/// has to be the process and not just the handshake. A server that has died
/// is otherwise silent: its reader thread stops and questions simply go
/// unanswered.
#[test]
fn a_real_server_reads_as_running_and_then_as_gone() {
    use obelus::lsp::ServerState;

    let Some((mut client, events)) = start() else {
        return;
    };
    assert_eq!(
        client.state(),
        ServerState::Starting,
        "a server is not ready before it has said so"
    );

    pump(&mut client, &events, HANDSHAKE, |client, _| {
        client.is_ready()
    });
    assert!(client.check_alive(), "the server is not running");
    assert_eq!(client.state(), ServerState::Ready);

    client.shutdown();
    // Shutdown waits for the process, so the next look finds it gone. This is
    // the state the badge exists to show.
    assert!(
        !client.check_alive(),
        "a stopped server still reads as alive"
    );
    assert_eq!(client.state(), ServerState::Gone);
}

/// Request ids are each server's own and start again from zero, so two
/// incarnations of the same server hand out the same ids. That is why a
/// question waiting for an answer is keyed by the server as well as by the
/// id: keyed by the id alone, the first answer after a restart is matched to
/// a question asked of a process that no longer exists.
#[test]
fn a_restarted_server_hands_out_the_same_ids_again() {
    let params = serde_json::json!({
        "textDocument": { "uri": "file:///nowhere.rs" },
    });

    let Some((mut first, _events)) = start() else {
        return;
    };
    let before = first
        .request("textDocument/documentSymbol", &params)
        .expect("the first request");
    first.shutdown();

    let Some((mut second, _events)) = start() else {
        return;
    };
    let after = second
        .request("textDocument/documentSymbol", &params)
        .expect("the second request");
    second.shutdown();

    assert_eq!(
        before, after,
        "ids no longer collide across restarts, so the key can be simplified"
    );
}

/// A real server's outline: nested, and about the names.
///
/// Two assumptions obelus is built on, neither of which a value-based test
/// can check. The protocol lets a server answer `documentSymbol` with a flat
/// list whose positions are the *definitions* -- which start at an attribute
/// or a doc comment as often as at the name -- unless the client declares it
/// understands the nested shape. obelus declares it; this is what says the
/// declaration is still being honoured.
#[test]
fn a_real_server_outlines_a_file_by_name_and_by_nesting() {
    use obelus::lsp::outline;

    let Some((mut client, events)) = start() else {
        return;
    };
    let path = root().join("src/jump.rs");
    let uri = obelus::lsp::client::uri_for(&path).expect("a uri");
    let text = std::fs::read_to_string(&path).expect("reading the file");
    client
        .notify(
            "textDocument/didOpen",
            &serde_json::json!({ "textDocument": {
                "uri": uri, "languageId": "rust", "version": 1, "text": text,
            }}),
        )
        .expect("saying the file is open");
    // Asked before the handshake has finished, which the client queues.
    let asked = client
        .request(
            "textDocument/documentSymbol",
            &serde_json::json!({ "textDocument": { "uri": uri } }),
        )
        .expect("asking");

    let reply = pump(&mut client, &events, INDEXED, |_, reply| {
        reply.is_some_and(|reply| reply.id == asked)
    })
    .expect("an answer");
    client.shutdown();

    let symbols = outline::symbols_in(reply.result);
    assert!(!symbols.is_empty(), "the server outlined nothing");

    // `JumpList` is a struct with fields, and `push` is a method on it, so
    // the answer has to have two levels in it.
    assert!(
        symbols.iter().any(|symbol| symbol.depth > 0),
        "every symbol came back at the top level, so the nested shape was \
         not used: {symbols:?}"
    );

    // And a name's position is the name's. `pub struct Jump` puts the name
    // eleven columns in; the line above it is an attribute, which is where
    // the flat shape would have pointed.
    let jump = symbols
        .iter()
        .find(|symbol| symbol.name == "Jump")
        .expect("Jump is in there");
    assert!(
        jump.end_character > jump.character,
        "the name has no extent: {jump:?}"
    );
    assert_eq!(
        usize::try_from(jump.end_character - jump.character).unwrap_or_default(),
        "Jump".len(),
        "the span is not the name: {jump:?}"
    );
    let line = text
        .lines()
        .nth(usize::try_from(jump.line).unwrap_or_default())
        .expect("the line");
    assert!(
        line.contains("struct Jump"),
        "the position is not on the line the name is on: {line:?}"
    );
}
