//! Against a real language server.
//!
//! Everything else about the protocol is tested against values rather than
//! against a server, which is right: the interesting rules are about what to
//! do with an answer, and a server cannot be made to give a stale one on
//! demand. What no amount of that catches is an assumption about real servers
//! being wrong — that they will negotiate byte offsets, that they declare the
//! questions they answer — and those are assumptions Obelus is built on.
//!
//! Skipped where rust-analyzer is not installed. The handshake is fast enough
//! to run every time; waiting for a project to be indexed is not, so the round
//! trip through a query is marked `ignore` and run with
//! `cargo test -- --ignored`.
//!
//! Skipped, and saying so where nobody is listening: the test harness keeps
//! what a passing test printed, so a machine with no rust-analyzer reports
//! thirteen of these as passing and the reason goes nowhere. Which is right
//! on a laptop and wrong on a machine whose job is to run them, and the two
//! look identical from in here -- both have no rust-analyzer. So the
//! difference is said rather than worked out: set `OBELUS_REQUIRE_LSP` and a
//! server that is missing fails these instead of excusing them.

use std::{
    sync::mpsc::{Receiver, RecvTimeoutError},
    time::{Duration, Instant},
};

use obelus_lsp::{
    Message, action,
    action::SymbolAction,
    client::{Client, Reply},
};
use obelus_syntax::LanguageId;
use unicode_width::UnicodeWidthStr;

/// Says that a rust-analyzer which is not here is a failure rather than a
/// reason to skip.
///
/// Whether one is installed is a question the machine can answer, and
/// [`usable`] does. Whether this run was *meant* to reach a real server is
/// not: a laptop without one and a CI machine that failed to install one
/// look the same from here. So it is declared by whoever knows.
const REQUIRED: &str = "OBELUS_REQUIRE_LSP";

/// Long enough for a cold cargo metadata on a slow machine.
const HANDSHAKE: Duration = Duration::from_secs(30);

/// Long enough for this project to be indexed. Measured at about three
/// seconds warm.
const INDEXED: Duration = Duration::from_secs(120);

fn root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("OBELUS_TREE"))
}

/// Pumps messages into the client until `done` says so, or the deadline.
fn pump<F>(
    client: &mut Client,
    events: &Receiver<Message>,
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
            Ok(Message { message, .. }) => {
                if let Some(reply) = client.on_message(&message)
                    && done(client, Some(&reply))
                {
                    return Some(reply);
                }
            }
            Err(RecvTimeoutError::Timeout) => panic!("the server went quiet"),
            Err(RecvTimeoutError::Disconnected) => panic!("the server ended"),
        }
    }
}

fn start() -> Option<(Client, Receiver<Message>)> {
    if !usable() {
        assert!(
            std::env::var_os(REQUIRED).is_none(),
            "{REQUIRED} is set and there is no rust-analyzer here that answers.\n\
             Install it, or unset {REQUIRED} to let these be skipped."
        );
        eprintln!("skipped: there is no rust-analyzer here that answers");
        return None;
    }
    let (sender, events) = std::sync::mpsc::channel::<Message>();
    let server = obelus_lsp::server_for(LanguageId::Rust).expect("a server for Rust");
    let client =
        Client::start(LanguageId::Rust, server, &root(), sender).expect("starting rust-analyzer");
    Some((client, events))
}

/// Whether there is a rust-analyzer here that will actually answer.
///
/// Being on the path is not the same as working, and the difference is not
/// hypothetical: rustup installs a proxy under this name for every tool it
/// knows of, whether or not the component is there. So the file exists on any
/// machine with rustup, is executable, and exits saying `Unknown binary` --
/// which arrives in these tests as a server that ended before it spoke, and
/// as a dozen failures on a machine that simply has not got the thing they
/// are about. Asked by running it, because nothing short of running it tells
/// a program from a message about one.
fn usable() -> bool {
    obelus_lsp::on_path("rust-analyzer")
        && std::process::Command::new("rust-analyzer")
            .arg("--version")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
}

/// The assumption the whole coordinate story rests on: offered bytes and
/// UTF-16, a real server takes bytes. If it stopped, every position Obelus
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
        "a server that will not count bytes puts Obelus on the UTF-16 path"
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
            action.method()
        );
    }
    client.shutdown();
}

/// The panel is offered after `.` and `::` because the server says those
/// characters mean something. Obelus has no table of its own -- it asks
/// about letters and about whatever the server names -- so a server that
/// stopped declaring them would leave `std::` offering nothing, with
/// nothing anywhere saying why.
#[test]
fn a_real_server_declares_the_characters_that_ask_for_a_completion() {
    let Some((mut client, events)) = start() else {
        return;
    };
    pump(&mut client, &events, HANDSHAKE, |client, _| {
        client.is_ready()
    });

    let capabilities = client.capabilities().expect("ready means capabilities");
    assert!(
        obelus_lsp::complete::supported(capabilities),
        "rust-analyzer no longer answers textDocument/completion"
    );
    for character in ['.', ':'] {
        assert!(
            obelus_lsp::complete::triggered_by(capabilities, character),
            "rust-analyzer no longer says {character:?} is worth asking about"
        );
    }
    // Not resolving is a thing a server is allowed to say, and
    // rust-analyzer says it: `resolve_provider: false`, because it puts the
    // documentation in the answer itself. The panel asks only where a
    // server says it would answer, which is why that is not asserted here.
    // The same for the call the cursor is inside: `(` and `,` are what
    // rust-analyzer says ask about one, and Obelus has no list of its own.
    assert!(
        obelus_lsp::signature::supported(capabilities),
        "rust-analyzer no longer answers textDocument/signatureHelp"
    );
    for character in ['(', ','] {
        assert!(
            obelus_lsp::signature::triggered_by(capabilities, character),
            "rust-analyzer no longer says {character:?} asks what a call takes"
        );
    }
    // Renaming, and what can be done here: the two that change files.
    assert!(
        capabilities.rename_provider.is_some(),
        "rust-analyzer no longer renames"
    );
    assert!(
        obelus_lsp::actions::supported(capabilities),
        "rust-analyzer no longer offers anything to do"
    );
    // Where else a name is used, which Obelus asks without being asked.
    assert!(
        obelus_lsp::uses::supported(capabilities),
        "rust-analyzer no longer answers textDocument/documentHighlight"
    );
    // And what a place is, which is the third thing the panels ask.
    assert!(
        obelus_lsp::hover::supported(capabilities),
        "rust-analyzer no longer answers textDocument/hover"
    );
    // And filling one in, which it advertises only to a client that has
    // said it wants the filling: undeclared, Obelus was told no and the
    // whole `completionItem/resolve` path it has was never reached.
    assert!(
        obelus_lsp::complete::resolves(capabilities),
        "rust-analyzer no longer fills a completion item in, or Obelus has \
         stopped saying which parts of one it wants"
    );
    client.shutdown();
}

/// A request from the server is answered *on the wire*, not merely
/// answerable: the shape of the answer is tested against values elsewhere,
/// and what this pins is that the client sends one at all.
///
/// Against a real client because that is what the wiring is: a message
/// arrives, and one goes out.
#[test]
fn a_question_from_the_server_is_answered_on_the_wire() {
    let Some((mut client, events)) = start() else {
        return;
    };
    pump(&mut client, &events, HANDSHAKE, |client, _| {
        client.is_ready()
    });

    let before = client.sent();
    let answered = client.on_message(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 4242,
        "method": "workspace/configuration",
        "params": { "items": [{ "section": "obelus" }] }
    }));
    assert!(
        answered.is_none(),
        "a question from the server was taken for an answer to one of ours"
    );
    assert_eq!(
        client.sent(),
        before + 1,
        "nothing went back, so the server is still waiting"
    );
    client.shutdown();
}

/// Taking a question back is a message, not a hope: the client sends
/// `$/cancelRequest` and stops waiting.
#[test]
fn a_question_can_be_taken_back() {
    let Some((mut client, events)) = start() else {
        return;
    };
    pump(&mut client, &events, HANDSHAKE, |client, _| {
        client.is_ready()
    });

    let id = client
        .request("textDocument/documentSymbol", &serde_json::json!({}))
        .expect("asking");
    let before = client.sent();
    client.cancel(id);
    assert_eq!(
        client.sent(),
        before + 1,
        "the server was never told to stop"
    );
    client.shutdown();
}

/// The whole way through for the panel: open a document, ask what could be
/// typed after a `self.`, and get the type's own fields and methods back
/// with what each of them is.
///
/// Ignored for the reason its sibling below is: a server answers nothing
/// until it has read the project.
#[test]
#[ignore = "waits for the project to be indexed"]
fn a_real_server_offers_what_could_be_typed() {
    let Some((mut client, events)) = start() else {
        return;
    };
    pump(&mut client, &events, HANDSHAKE, |client, _| {
        client.is_ready()
    });

    // A file of the repository's own, and a place in it that already has
    // the punctuation the panel is offered after. A file written for the
    // test would be a file outside the crate's module tree, which is a
    // thing rust-analyzer has nothing to say about.
    let path = root().join("crates/obelus-app/src/jump.rs");
    let source = std::fs::read_to_string(&path).expect("reading the file");
    let uri = obelus_lsp::client::uri_for(&path).expect("a uri");
    client
        .notify(
            "textDocument/didOpen",
            &serde_json::json!({
                "textDocument": {
                    "uri": uri, "languageId": "rust", "version": 1, "text": source,
                }
            }),
        )
        .expect("opening the document");

    // Just past a `self.`, which is where typing the trigger character
    // leaves the cursor -- and the case worth asking about, because what
    // comes back is fields and methods with their types. Found rather than
    // written down: a line number here would be a test that fails when
    // somebody adds a line above it.
    let at = source.find("self.").expect("a receiver to complete after") + "self.".len();
    let line = source[..at].matches('\n').count();
    let character = at - source[..at].rfind('\n').map_or(0, |start| start + 1);

    let text = obelus_text::Text::from_string(&source);
    let encoding = client.encoding().clone();
    let deadline = Instant::now() + INDEXED;
    loop {
        assert!(Instant::now() < deadline, "never indexed");
        let id = client
            .request(
                "textDocument/completion",
                &serde_json::json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": line, "character": character },
                }),
            )
            .expect("asking");
        let reply = pump(&mut client, &events, INDEXED, |_, reply| {
            reply.is_some_and(|reply| reply.id == id)
        })
        .expect("An answer");

        let offer = obelus_lsp::complete::offer_in(&reply.result, &text, &encoding);
        if offer.candidates.is_empty() {
            // Still reading the project, which is the same empty answer a
            // question with no answer gets: asking again is what Obelus
            // does.
            std::thread::sleep(Duration::from_millis(300));
            continue;
        }
        assert!(
            offer
                .candidates
                .iter()
                .any(|candidate| candidate.label == "entries"),
            "nothing of the type's own fields, so the answer is about somewhere else"
        );
        // What the panel draws beside the label, and what it puts in.
        assert!(
            offer
                .candidates
                .iter()
                .any(|candidate| candidate.detail.is_some()),
            "no candidate says what it is"
        );
        assert!(
            offer
                .candidates
                .iter()
                .all(|candidate| !candidate.insert.is_empty()),
            "a candidate that puts nothing in"
        );
        // And what the documentation half shows, which now takes a second
        // question. A server told which parts of an offer the client will
        // ask for later sends the light form and keeps the rest: that is
        // what `resolveSupport` buys, and it is why a thousand candidates
        // no longer arrive carrying a thousand doc comments. So the panel
        // asks about the one the reader is looking at.
        let mut candidate = offer
            .candidates
            .iter()
            .find(|candidate| candidate.label == "entries")
            .expect("the field")
            .clone();
        let id = client
            .request(
                "completionItem/resolve",
                &obelus_lsp::complete::resolve_params(&candidate),
            )
            .expect("asking");
        let reply = pump(&mut client, &events, INDEXED, |_, reply| {
            reply.is_some_and(|reply| reply.id == id)
        })
        .expect("An answer");
        obelus_lsp::complete::resolved_into(&mut candidate, &reply.result, &text, &encoding);
        assert!(
            candidate.documentation.is_some() || candidate.detail.is_some(),
            "a filled-in offer carries neither documentation nor what it is: {candidate:?}"
        );
        break;
    }
    client.shutdown();
}

/// The whole way through: open a document, ask where a symbol is defined, and
/// get a place in the file that defines it.
///
/// Ignored by default because it waits for the project to be indexed. A
/// server answers with nothing until then, which is the same answer it gives
/// for a symbol that has no definition — so the retry here is the same thing
/// Obelus does, and its absence would make this test assert that indexing is
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

    // `buffer::DocumentId` on line three of jump.rs, at column twelve.
    let path = root().join("crates/obelus-app/src/jump.rs");
    let text = std::fs::read_to_string(&path).expect("reading the file");
    let uri = obelus_lsp::client::uri_for(&path).expect("a uri");

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
        assert!(
            Instant::now() < deadline,
            "the server never found the definition"
        );
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
        .expect("An answer");

        let indexing = client.working_on().is_some();
        match action::outcome_of(reply.result, 1, Some(1), indexing) {
            action::Outcome::Places(places) => {
                let named = &places[0];
                // The buffer module, however it is spelled on disk: it
                // has been a file, a directory and a crate root, and which
                // of those it is has nothing to do with what this is
                // testing.
                assert!(
                    named.path.ends_with("src/buffer.rs")
                        || named.path.ends_with("src/buffer/mod.rs")
                        || named.path.ends_with("obelus-buffer/src/lib.rs"),
                    "expected the buffer module, got {}",
                    named.path.display()
                );
                break;
            }
            // Still reading the project. Asking again is what Obelus does.
            action::Outcome::NotYet => std::thread::sleep(Duration::from_millis(300)),
            // Nothing, which is also what a server that has not finished
            // loading answers: it reports no progress until it starts, so
            // "not indexing" and "indexed" look the same from here.
            // `DocumentId` has a definition, so the only question is when --
            // and the deadline above is what says never.
            action::Outcome::Nothing => std::thread::sleep(Duration::from_millis(300)),
            // "content modified" is a server saying the document moved
            // under it while it was working the answer out, which is what
            // it says while it is still loading the project. The same
            // retryable state as `NotYet`, arriving as an error instead
            // of an empty answer.
            action::Outcome::Failed(ref why) if why.contains("content modified") => {
                std::thread::sleep(Duration::from_millis(300));
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
/// file is open. What is being checked is that Obelus does not send a server
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
    use obelus_lsp::ServerState;

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
    // A file that is not there, named the way this platform names one: the
    // server is asked about it and the answer is not what this is about.
    let nowhere = std::env::temp_dir().join("nowhere.rs");
    let params = serde_json::json!({
        "textDocument": {
            "uri": obelus_lsp::client::uri_for(&nowhere)
                .expect("a uri")
                .as_str()
        },
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
/// Two assumptions Obelus is built on, neither of which a value-based test
/// can check. The protocol lets a server answer `documentSymbol` with a flat
/// list whose positions are the *definitions* -- which start at an attribute
/// or a doc comment as often as at the name -- unless the client declares it
/// understands the nested shape. Obelus declares it; this is what says the
/// declaration is still being honoured.
#[test]
fn a_real_server_outlines_a_file_by_name_and_by_nesting() {
    use obelus_lsp::outline;

    let Some((mut client, events)) = start() else {
        return;
    };
    let path = root().join("crates/obelus-app/src/jump.rs");
    let uri = obelus_lsp::client::uri_for(&path).expect("a uri");
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
    .expect("An answer");
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

/// A real server offers something to do on ordinary code.
///
/// The assumption `alt+a` rests on, and the one that was wrong: Obelus
/// declared nothing about code actions, and rust-analyzer answers `null`
/// to every `textDocument/codeAction` from a client that has not said it
/// understands the literal shape -- whatever the file, wherever the
/// range, mistake on the line or not. Nothing about that is visible from
/// this side: it is a well-formed answer meaning "Nothing to do here",
/// which is also what a line with genuinely nothing to do says.
///
/// So the line this asks about is chosen to have nothing wrong with it.
/// An answer with offers in it is the declaration being honoured; an
/// empty one is the whole feature quietly gone.
#[test]
fn a_real_server_offers_something_to_do_on_ordinary_code() {
    use obelus_lsp::actions;

    let Some((mut client, events)) = start() else {
        return;
    };
    let path = root().join("crates/obelus-lsp/src/actions.rs");
    let uri = obelus_lsp::client::uri_for(&path).expect("a uri");
    let text = std::fs::read_to_string(&path).expect("reading the file");
    client
        .notify(
            "textDocument/didOpen",
            &serde_json::json!({ "textDocument": {
                "uri": uri, "languageId": "rust", "version": 1, "text": text,
            }}),
        )
        .expect("saying the file is open");

    // `pub struct Action {` -- a declaration with nothing wrong with it,
    // so nothing here is a quick fix for a diagnostic.
    let line = text
        .lines()
        .position(|line| line.contains("pub struct Action {"))
        .expect("the line is still in there");
    let length = text.lines().nth(line).expect("the line").len();
    let asked = client
        .request(
            "textDocument/codeAction",
            &serde_json::json!({
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": line, "character": 0 },
                    "end": { "line": line, "character": length },
                },
                "context": { "diagnostics": [] },
            }),
        )
        .expect("asking");

    let reply = pump(&mut client, &events, INDEXED, |_, reply| {
        reply.is_some_and(|reply| reply.id == asked)
    })
    .expect("An answer");
    client.shutdown();

    let offered = actions::offered_in(&reply.result);
    assert!(
        !offered.is_empty(),
        "a real server offered nothing to do on a struct declaration, which          is what it answers when Obelus has not declared it understands          code actions: {:?}",
        reply.result
    );
    // And they arrive as actions rather than as bare commands, which is
    // what the declaration was for: a bare command has no kind and no
    // edit to read.
    assert!(
        offered
            .iter()
            .any(|action| action.kind.is_some() || action.unresolved()),
        "every offer came back as a bare command: {offered:?}"
    );
}

/// A server Obelus started is a server Obelus stops.
#[test]
fn a_server_does_not_outlive_the_shutdown() {
    let Some((mut client, events)) = start() else {
        return;
    };
    pump(&mut client, &events, HANDSHAKE, |client, _| {
        client.is_ready()
    });
    let pid = client.pid().expect("the server's process");
    assert!(alive(pid), "the server was not running to begin with");

    client.shutdown();
    // Asked politely and killed if it will not go, so by here it is gone
    // either way -- and `shutdown` reaps it, so the pid is not a zombie
    // answering to signal zero.
    assert!(!alive(pid), "the server outlived the shutdown");
}

/// And a server Obelus lets go of is a server Obelus stops, which is the
/// half that was missing: stopping one on purpose was never the leak.
///
/// Broken deliberately by leaving it to `Child`'s own `Drop`, which does
/// nothing at all. Every other way a client went -- Obelus ending, a test
/// finishing, one server replacing another -- left the process running.
#[test]
fn a_server_does_not_outlive_its_client() {
    let Some((mut client, events)) = start() else {
        return;
    };
    pump(&mut client, &events, HANDSHAKE, |client, _| {
        client.is_ready()
    });
    let pid = client.pid().expect("the server's process");
    assert!(alive(pid));

    drop(client);
    assert!(!alive(pid), "the server outlived the client that held it");
}

/// The same, once the server has finished loading the project.
///
/// The case that matters and the one a quick test cannot reach: a
/// rust-analyzer that is still starting up exits when the pipe to it
/// closes, and one that has finished loading does not. Eighty-four of the
/// second kind is what this looked like from the outside.
#[test]
#[ignore = "waits for the project to be indexed"]
fn a_loaded_server_does_not_outlive_its_client() {
    let Some((mut client, events)) = start() else {
        return;
    };
    pump(&mut client, &events, HANDSHAKE, |client, _| {
        client.is_ready()
    });
    let pid = client.pid().expect("the server's process");

    // Loaded, which is a question with an answer rather than a sleep: a
    // server that is still indexing says so, and one that has finished
    // answers about a name in the project.
    let deadline = Instant::now() + INDEXED;
    loop {
        assert!(Instant::now() < deadline, "never indexed");
        let id = client
            .request(
                "workspace/symbol",
                &serde_json::json!({ "query": "Buffer" }),
            )
            .expect("asking");
        let reply = pump(&mut client, &events, INDEXED, |_, reply| {
            reply.is_some_and(|reply| reply.id == id)
        })
        .expect("An answer");
        let found = reply
            .result
            .ok()
            .map(|value| obelus_lsp::outline::found_in(Ok(value), Some(&root())))
            .unwrap_or_default();
        if !found.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }

    drop(client);
    assert!(
        !alive(pid),
        "a loaded server outlived the client that held it"
    );
}

/// The two halves of a call hierarchy, on the wire: a server is asked to
/// prepare the name under the cursor, and the item it hands back is what
/// the next question is asked with.
///
/// This is the part no answer fed in by hand can check. The item goes back
/// as `params.item`, whole and including whatever `data` the server put in
/// it, and a client that rebuilt it or wrapped it differently would be
/// answered with nothing -- which looks exactly like a function nobody
/// calls.
#[test]
#[ignore = "waits for the project to be indexed"]
fn a_real_server_says_who_calls_something() {
    use obelus_lsp::{hierarchy, hierarchy::Direction};

    let Some((mut client, events)) = start() else {
        return;
    };
    pump(&mut client, &events, HANDSHAKE, |client, _| {
        client.is_ready()
    });

    // A function with a caller in its own file, found by looking rather
    // than written down: a line number here would be a test that breaks
    // when the file above it is edited.
    let path = root().join("crates/obelus-lsp/src/hierarchy.rs");
    let text = std::fs::read_to_string(&path).expect("reading the file");
    let uri = obelus_lsp::client::uri_for(&path).expect("a uri");
    let line = text
        .lines()
        .position(|line| line.starts_with("fn one_call"))
        .expect("the function is still called that");
    let at = serde_json::json!({ "line": line, "character": 3 });

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

    // Until the project is indexed a server prepares nothing, which is the
    // same answer it gives for a comma -- so this waits the way Obelus
    // does.
    let deadline = Instant::now() + INDEXED;
    let item = loop {
        assert!(Instant::now() < deadline, "the server prepared nothing");
        let id = client
            .request(
                SymbolAction::Calls.method(),
                &serde_json::json!({ "textDocument": { "uri": uri }, "position": at }),
            )
            .expect("asking");
        let reply = pump(&mut client, &events, INDEXED, |_, reply| {
            reply.is_some_and(|reply| reply.id == id)
        })
        .expect("An answer");
        if let Some(item) = hierarchy::prepared(&reply.result) {
            break item;
        }
        std::thread::sleep(Duration::from_millis(500));
    };
    assert_eq!(
        hierarchy::root_of(&item)
            .expect("an item Obelus can read")
            .name,
        "one_call",
        "the server prepared something else"
    );

    let id = client
        .request(
            Direction::Callers.method(),
            &serde_json::json!({ "item": item }),
        )
        .expect("asking who calls it");
    let reply = pump(&mut client, &events, INDEXED, |_, reply| {
        reply.is_some_and(|reply| reply.id == id)
    })
    .expect("An answer");

    let callers = hierarchy::called_in(&reply.result, Direction::Callers);
    assert!(
        callers.iter().any(|called| called.name == "called_in"),
        "the one function that calls it is not among {:?}",
        callers
            .iter()
            .map(|called| &called.name)
            .collect::<Vec<_>>()
    );
    client.shutdown();
}

/// Whether a process is still there, asked of the system.
///
/// Of the system and not of Obelus: what these tests are about is that a
/// server Obelus let go of is gone, and [`Client::check_alive`] is Obelus's
/// own answer to that question -- the thing under test. A test that asked
/// it would be asking the rule what it expects.
///
/// It ran `kill -0` until it was pointed out that the tests were meant to
/// run on Windows too, where there is no such command. The `kill.exe` a
/// POSIX shell ships there is worse than nothing: it counts in that
/// shell's own process numbers and `pid` is the system's, so it would
/// answer, and answer about somebody else.
fn alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // Signal zero, which is what `kill -0` sent: it delivers nothing
        // and reports whether the process is there. A process that has
        // ended but not been waited for still answers to it -- which is
        // the same answer the command gave, so these tests read as they
        // always did, and they pass because Obelus reaps what it kills.
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return false;
        };
        // Safety: `kill` reads nothing through a pointer and this signal
        // writes nothing anywhere.
        unsafe { libc::kill(pid, 0) == 0 }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, WAIT_TIMEOUT},
            // `SYNCHRONIZE` is a standard access right, which every kind
            // of kernel object takes and no kind owns -- and windows-sys
            // generates it under the first module that names it, which is
            // the file system's. Asking `System::Threading` for it reads
            // right and does not compile.
            Storage::FileSystem::SYNCHRONIZE,
            System::Threading::{OpenProcess, WaitForSingleObject},
        };

        // Safety: a handle is asked for, waited on for no time at all, and
        // closed. Nothing is read through a pointer.
        unsafe {
            let process = OpenProcess(SYNCHRONIZE, 0, pid);
            if process.is_null() {
                // No such process -- or one this test may not look at,
                // which for a server it started itself does not happen.
                return false;
            }
            // Waiting no time at all: a process handle is signalled once
            // the process has ended, so this says which it is without
            // blocking. `GetExitCodeProcess` would be the other way and is
            // the wrong one -- it reports 259 for "still running", and a
            // process that exited *with* 259 is indistinguishable.
            let waited = WaitForSingleObject(process, 0);
            CloseHandle(process);
            waited == WAIT_TIMEOUT
        }
    }
}

/// What a real server would have the reader know, on the wire.
///
/// The request carries a range and the answer carries positions, and both
/// are things Obelus can only get wrong silently: a range spelt wrong is
/// answered with nothing, which from this side looks exactly like a file
/// with no types to work out.
#[test]
#[ignore = "waits for the project to be indexed"]
fn a_real_server_works_out_what_the_file_does_not_say() {
    use obelus_lsp::hint;

    let Some((mut client, events)) = start() else {
        return;
    };
    pump(&mut client, &events, HANDSHAKE, |client, _| {
        client.is_ready()
    });
    let capabilities = client.capabilities().expect("ready means capabilities");
    assert!(
        hint::supported(capabilities),
        "rust-analyzer no longer declares that it works these out"
    );

    // A file with a `let` in it whose type nobody wrote down.
    let path = root().join("crates/obelus-lsp/src/hint.rs");
    let text = std::fs::read_to_string(&path).expect("reading the file");
    let uri = obelus_lsp::client::uri_for(&path).expect("a uri");
    client
        .notify(
            "textDocument/didOpen",
            &serde_json::json!({
                "textDocument": { "uri": uri, "languageId": "rust", "version": 1, "text": text }
            }),
        )
        .expect("opening the document");

    let lines = text.lines().count();
    let deadline = Instant::now() + INDEXED;
    let found = loop {
        assert!(Instant::now() < deadline, "the server worked out nothing");
        let id = client
            .request(
                "textDocument/inlayHint",
                &serde_json::json!({
                    "textDocument": { "uri": uri },
                    "range": {
                        "start": { "line": 0, "character": 0 },
                        "end": { "line": lines, "character": 0 },
                    },
                }),
            )
            .expect("asking");
        let reply = pump(&mut client, &events, INDEXED, |_, reply| {
            reply.is_some_and(|reply| reply.id == id)
        })
        .expect("An answer");
        let found = hint::in_reply(
            &reply.result,
            &obelus_text::Text::from_string(&text),
            client.encoding(),
        );
        if !found.is_empty() {
            break found;
        }
        std::thread::sleep(Duration::from_millis(500));
    };

    // Every one of them lands on a line the file has, and says something.
    let most = text.lines().count();
    for hint in &found {
        assert!(
            hint.line.get() < most,
            "a hint for a line the file does not have: {hint:?}"
        );
        assert!(!hint.label.is_empty(), "a hint saying nothing: {hint:?}");
        assert_eq!(hint.cells(), hint.label.width(), "a width nothing drew");
    }
    // And at least one of them is a type nobody wrote down, which is what
    // this file is full of.
    assert!(
        found.iter().any(|hint| hint.kind == hint::Kind::Type),
        "nothing was a type: {:?}",
        found.iter().map(|hint| &hint.label).collect::<Vec<_>>()
    );
    client.shutdown();
}

/// A file that moves takes its meaning with it, and the server says which
/// paths it wants to hear about before that happens.
///
/// Two filters, and Obelus reads both: `**/*.rs` for files and `**` for
/// folders. A server that stopped declaring them would leave every move
/// silently breaking the references to it, and the only sign would be the
/// next build.
#[test]
fn a_real_server_says_which_files_it_wants_told_about_before_they_move() {
    use obelus_lsp::renaming;

    let Some((mut client, events)) = start() else {
        return;
    };
    pump(&mut client, &events, HANDSHAKE, |client, _| {
        client.is_ready()
    });

    let capabilities = client.capabilities().expect("ready means capabilities");
    assert!(
        renaming::asked_before(
            capabilities,
            &root().join("crates/obelus-lsp/src/hint.rs"),
            false
        ),
        "rust-analyzer no longer wants asking about a Rust file that moves"
    );
    assert!(
        renaming::asked_before(capabilities, &root().join("crates/obelus-lsp/src"), true),
        "rust-analyzer no longer wants asking about a directory that moves"
    );
    // And the filters are read rather than assumed: it registered `*.rs`
    // for files, so a file that is not one is nobody's business.
    assert!(
        !renaming::asked_before(capabilities, &root().join("README.md"), false),
        "Obelus is asking about files the server did not register for"
    );
    client.shutdown();
}

/// The whole way through: ask what moving a module would change, and get
/// back the `mod` line in another file that names it.
///
/// Ignored for the reason its siblings are -- a server answers `null`
/// until it has read the project, which is the state Obelus reports as
/// "not ready" rather than as "Nothing to change". Nothing is moved: the
/// question is asked about a move that does not happen, which is exactly
/// what the protocol is for.
#[test]
#[ignore = "waits for the project to be indexed"]
fn a_real_server_works_out_what_moving_a_file_would_change() {
    use obelus_lsp::{edits, renaming};

    let Some((mut client, events)) = start() else {
        return;
    };
    pump(&mut client, &events, HANDSHAKE, |client, _| {
        client.is_ready()
    });

    let from = root().join("crates/obelus-lsp/src/hint.rs");
    let to = root().join("crates/obelus-lsp/src/hints.rs");
    let params = renaming::params(&from, &to).expect("two uris");

    let deadline = Instant::now() + INDEXED;
    let wanted = loop {
        assert!(
            Instant::now() < deadline,
            "the server never worked out what the move changes"
        );
        let id = client
            .request("workspace/willRenameFiles", &params)
            .expect("asking");
        let reply = pump(&mut client, &events, INDEXED, |_, reply| {
            reply.is_some_and(|reply| reply.id == id)
        })
        .expect("An answer");
        let Ok(result) = reply.result else {
            std::thread::sleep(Duration::from_millis(300));
            continue;
        };
        // `null` while it is still reading the project, which is the
        // answer Obelus distinguishes from an empty edit.
        let wanted = edits::wanted_in(&result);
        if wanted.is_empty() {
            std::thread::sleep(Duration::from_millis(300));
            continue;
        }
        break wanted;
    };

    // The module list that names it. Whatever else the server finds, this
    // is the line that would stop compiling.
    assert!(
        wanted
            .changes
            .iter()
            .any(|change| change.path.ends_with("src/lsp/mod.rs") && change.text == "hints"),
        "the `mod hint;` line was not among the changes: {:?}",
        wanted.changes
    );
    // And no file operations: rust-analyzer leaves the move itself to
    // Obelus and only says what the text has to become. If that ever
    // changed, Obelus would be refusing an operation it had asked for.
    assert!(
        wanted.refused.is_empty(),
        "the server asked Obelus to move files itself: {:?}",
        wanted.refused
    );
    client.shutdown();
}

/// A real server says what is wrong after an edit, with nothing saved.
///
/// The question a reader asks as "do I have to press ctrl+s for the
/// underline". Obelus sends the whole document on every keystroke and draws
/// the underline from whatever was published, so this is the server's half
/// of that answer -- and it cannot be learnt from a value, because it is
/// about a real server choosing to speak. Nothing else here asked it, and
/// the chain it covers is the whole of `didOpen` and `didChange`: a document
/// the server never took is a document it says nothing about.
///
/// A *syntax* error, because rust-analyzer has two sources and only one of
/// them answers an unsaved edit. Measured, both at 1.0 against this project:
/// a syntax error comes back within seconds of the `didChange`, and a type
/// error (`let _: u32 = "not a number"`) comes back never -- that one is
/// `cargo check`, which `checkOnSave` runs on a save. So a reader whose type
/// errors wait for ctrl+s is watching rust-analyzer's default and not
/// Obelus, and the underline really is immediate for what the server
/// answers immediately.
///
/// The save half is deliberately not asserted here: `cargo check` over this
/// workspace is minutes cold, which is a test that fails by taking too long
/// on the machine least able to spare it.
///
/// Broken deliberately by dropping the `didChange`: the server is then left
/// with the file as it is on disk, which parses, and this waits the full
/// two minutes and finds nothing.
///
/// Dropping the `didOpen` instead does *not* break it, which was worth
/// finding out and is why it is written down: rust-analyzer takes a
/// `didChange` for a document it was never told was opened, and answers it.
/// So this covers `didChange` and says nothing about `didOpen` -- if that
/// notification ever needs a test, it needs its own.
#[test]
#[ignore = "waits for rust-analyzer to index this project"]
fn a_real_server_says_what_is_wrong_without_a_save() {
    let Some((mut client, events)) = start() else {
        return;
    };
    pump(&mut client, &events, HANDSHAKE, |client, _| {
        client.is_ready()
    });

    let path = root().join("crates/obelus-lsp/src/lib.rs");
    let uri = obelus_lsp::client::uri_for(&path)
        .expect("a uri")
        .to_string();
    let text = std::fs::read_to_string(&path).expect("the file");

    let _ = client.notify(
        "textDocument/didOpen",
        &serde_json::json!({"textDocument": {
            "uri": uri, "languageId": "rust", "version": 1, "text": text,
        }}),
    );

    // Sent the way typing sends it: the whole document, the version bumped,
    // and no `didSave` anywhere in this test.
    let unclosed = text.replace("pub mod action;", "pub mod action;\nfn obelus_probe( {");
    assert_ne!(
        unclosed, text,
        "the probe edited nothing, so it proves nothing"
    );
    let _ = client.notify(
        "textDocument/didChange",
        &serde_json::json!({
            "textDocument": {"uri": uri, "version": 2},
            "contentChanges": [{"text": unclosed}],
        }),
    );

    // Pumped by hand rather than with `pump`, which lends its closure a
    // `&Client`: taking what was published needs a `&mut`.
    let mut said: Vec<String> = Vec::new();
    let deadline = Instant::now() + INDEXED;
    while said.is_empty() {
        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
            break;
        };
        match events.recv_timeout(left) {
            Ok(Message { message, .. }) => {
                let _ = client.on_message(&message);
                for published in client.take_published() {
                    if published["uri"].as_str() != Some(uri.as_str()) {
                        continue;
                    }
                    said.extend(
                        published["diagnostics"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|one| one["message"].as_str())
                            .map(str::to_string),
                    );
                }
            }
            Err(RecvTimeoutError::Timeout) => break,
            Err(RecvTimeoutError::Disconnected) => panic!("the server ended"),
        }
    }

    assert!(
        !said.is_empty(),
        "rust-analyzer said nothing about an edit it was told about and never \
         saw saved, so nothing reaches a reader until they press ctrl+s"
    );
}

/// What a real server puts in for a callable, which is what the panel after
/// it rests on.
///
/// Obelus asks what a call takes when a candidate puts one in, and it reads
/// that off the text that went in. The shape of that text is the server's:
/// rust-analyzer sends `fmt(${1:f})$0` -- a bracket, a hole with a default
/// in it, and the end stop after the closing bracket.
///
/// Worth a test against a real one because the shape is the whole of it and
/// it cannot be reasoned out. The rule this backs used to read the character
/// *behind the caret* instead, which is `m` here and not `(`: a hole with a
/// default is selected, so the caret sits at the end of `from`. It never
/// fired, and the test that said it did fed it `copy($1)$0` -- an empty
/// hole, which is the one shape that puts the caret against the bracket.
/// A stand-in server cannot say this; only this can.
#[test]
#[ignore = "runs a real rust-analyzer"]
fn a_real_server_puts_a_bracket_in_for_a_callable() {
    let Some((mut client, events)) = start() else {
        return;
    };
    pump(&mut client, &events, HANDSHAKE, |client, _| {
        client.is_ready()
    });

    let path = root().join("crates/obelus-app/src/jump.rs");
    let source = std::fs::read_to_string(&path).expect("reading the file");
    let uri = obelus_lsp::client::uri_for(&path).expect("a uri");
    client
        .notify(
            "textDocument/didOpen",
            &serde_json::json!({
                "textDocument": { "uri": uri, "languageId": "rust", "version": 1, "text": source }
            }),
        )
        .expect("opening the document");

    // Just past a `self.`, where what comes back is the type's own methods
    // -- which are callables, which is what this is about.
    let at = source.find("self.").expect("a receiver") + "self.".len();
    let line = source[..at].matches('\n').count();
    let character = at - source[..at].rfind('\n').map_or(0, |start| start + 1);

    let text = obelus_text::Text::from_string(&source);
    let encoding = client.encoding().clone();
    let deadline = Instant::now() + INDEXED;
    loop {
        assert!(Instant::now() < deadline, "never indexed");
        let id = client
            .request(
                "textDocument/completion",
                &serde_json::json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": line, "character": character },
                }),
            )
            .expect("asking");
        let reply = pump(&mut client, &events, INDEXED, |_, reply| {
            reply.is_some_and(|reply| reply.id == id)
        })
        .expect("an answer");
        let offer = obelus_lsp::complete::offer_in(&reply.result, &text, &encoding);
        if offer.candidates.is_empty() {
            std::thread::sleep(Duration::from_millis(300));
            continue;
        }

        let callable = offer
            .candidates
            .iter()
            .find(|candidate| candidate.label.ends_with("(…)"))
            .expect("a candidate that takes arguments");
        assert!(
            callable.snippet,
            "a callable arrived as plain text, so nothing puts a bracket in: {:?}",
            callable.insert
        );
        assert!(
            callable.insert.contains('('),
            "no bracket in what goes in, so there is no call to ask about: {:?}",
            callable.insert
        );
        // And the hole has a default in it, which is what puts the caret
        // somewhere other than against the bracket.
        assert!(
            callable.insert.contains("${1:"),
            "no default in the first hole, which is the shape that hid this: {:?}",
            callable.insert
        );
        break;
    }
}
