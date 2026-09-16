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

/// The panel is offered after `.` and `::` because the server says those
/// characters mean something. obelus has no table of its own -- it asks
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
        obelus::lsp::complete::supported(capabilities),
        "rust-analyzer no longer answers textDocument/completion"
    );
    for character in ['.', ':'] {
        assert!(
            obelus::lsp::complete::triggered_by(capabilities, character),
            "rust-analyzer no longer says {character:?} is worth asking about"
        );
    }
    // Not resolving is a thing a server is allowed to say, and
    // rust-analyzer says it: `resolve_provider: false`, because it puts the
    // documentation in the answer itself. The panel asks only where a
    // server says it would answer, which is why that is not asserted here.
    // The same for the call the cursor is inside: `(` and `,` are what
    // rust-analyzer says ask about one, and obelus has no list of its own.
    assert!(
        obelus::lsp::signature::supported(capabilities),
        "rust-analyzer no longer answers textDocument/signatureHelp"
    );
    for character in ['(', ','] {
        assert!(
            obelus::lsp::signature::triggered_by(capabilities, character),
            "rust-analyzer no longer says {character:?} asks what a call takes"
        );
    }
    // Renaming, and what can be done here: the two that change files.
    assert!(
        capabilities.rename_provider.is_some(),
        "rust-analyzer no longer renames"
    );
    assert!(
        obelus::lsp::actions::supported(capabilities),
        "rust-analyzer no longer offers anything to do"
    );
    // Where else a name is used, which obelus asks without being asked.
    assert!(
        obelus::lsp::uses::supported(capabilities),
        "rust-analyzer no longer answers textDocument/documentHighlight"
    );
    // And what a place is, which is the third thing the panels ask.
    assert!(
        obelus::lsp::hover::supported(capabilities),
        "rust-analyzer no longer answers textDocument/hover"
    );
    assert!(
        !obelus::lsp::complete::resolves(capabilities),
        "rust-analyzer now resolves items, so the panel should be asking it to"
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
    let path = root().join("src/jump.rs");
    let source = std::fs::read_to_string(&path).expect("reading the file");
    let uri = obelus::lsp::client::uri_for(&path).expect("a uri");
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

    let text = obelus::text::Text::from_string(&source);
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

        let offer = obelus::lsp::complete::offer_in(&reply.result, &text, &encoding);
        if offer.candidates.is_empty() {
            // Still reading the project, which is the same empty answer a
            // question with no answer gets: asking again is what obelus
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
        // And what the documentation half shows. rust-analyzer says it
        // does not resolve an item, which is the same server saying it has
        // already sent everything it has: the documentation is in the
        // answer itself, and a panel that waited for a resolve would show
        // the signature and nothing else for every Rust file.
        assert!(
            offer
                .candidates
                .iter()
                .any(|candidate| candidate.documentation.is_some()),
            "no candidate carries documentation, and the server will not resolve one"
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

/// A real server offers something to do on ordinary code.
///
/// The assumption `alt+a` rests on, and the one that was wrong: obelus
/// declared nothing about code actions, and rust-analyzer answers `null`
/// to every `textDocument/codeAction` from a client that has not said it
/// understands the literal shape -- whatever the file, wherever the
/// range, mistake on the line or not. Nothing about that is visible from
/// this side: it is a well-formed answer meaning "nothing to do here",
/// which is also what a line with genuinely nothing to do says.
///
/// So the line this asks about is chosen to have nothing wrong with it.
/// An answer with offers in it is the declaration being honoured; an
/// empty one is the whole feature quietly gone.
#[test]
fn a_real_server_offers_something_to_do_on_ordinary_code() {
    use obelus::lsp::actions;

    let Some((mut client, events)) = start() else {
        return;
    };
    let path = root().join("src/lsp/actions.rs");
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
    .expect("an answer");
    client.shutdown();

    let offered = actions::offered_in(&reply.result);
    assert!(
        !offered.is_empty(),
        "a real server offered nothing to do on a struct declaration, which          is what it answers when obelus has not declared it understands          code actions: {:?}",
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

/// A server obelus started is a server obelus stops.
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

/// And a server obelus lets go of is a server obelus stops, which is the
/// half that was missing: stopping one on purpose was never the leak.
///
/// Broken deliberately by leaving it to `Child`'s own `Drop`, which does
/// nothing at all. Every other way a client went -- obelus ending, a test
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
        .expect("an answer");
        let found = reply
            .result
            .ok()
            .map(|value| obelus::lsp::outline::found_in(Ok(value), Some(&root())))
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

/// Whether a process is still there, asked the way `kill -0` asks.
fn alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}
