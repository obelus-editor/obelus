//! What obelus offers an agent, over the wire an agent really uses.
//!
//! The server is started the way the loop starts it and spoken to with
//! `ureq`, which is already here for the registry: what is being tested is
//! that an agent asking in the ordinary way gets the ordinary answers, and a
//! test that called the handler directly would not have told anyone whether
//! the transport was mounted at all.

mod support;

use std::sync::mpsc::channel;

/// A tree with one note, and obelus listening on it.
fn listening(name: &str) -> (support::Scratch, String) {
    let scratch = support::Scratch::new(name);
    std::fs::create_dir_all(scratch.path().join(".obelus")).expect("the directory");
    std::fs::write(
        scratch.path().join(".obelus").join("todo.toml"),
        "[[todo]]\nid = \"ABCDEFGH\"\nsaid = \"wire the counts tree up\"\ndone = false\n",
    )
    .expect("the notes");

    let (sender, events) = channel();
    // Held for the life of the test: a server whose main loop has gone
    // answers a tool that asks the reader with "nobody is there", which is
    // not what these are about.
    std::mem::forget(events);
    let url = obelus::mcp::serve(scratch.path(), sender).expect("a socket");
    (scratch, url)
}

/// One request, and what came back.
fn ask(url: &str, session: Option<&str>, body: &str) -> (String, Option<String>) {
    let mut request = ureq::post(url)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream");
    if let Some(session) = session {
        request = request.header("mcp-session-id", session);
    }
    let mut answer = request.send(body).expect("an answer");
    let named = answer
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let said = answer.body_mut().read_to_string().expect("what it said");
    (said, named)
}

/// The handshake, and then what obelus says it can do.
///
/// The tools come out of the function signatures in `src/mcp.rs` -- that is
/// the whole reason the protocol crate is here rather than the dispatch
/// being written out -- so this is also what says the generation worked.
#[test]
fn an_agent_is_told_what_obelus_can_do() {
    let (_scratch, url) = listening("tools-listed");

    let (said, session) = ask(
        &url,
        None,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"a test","version":"0"}}}"#,
    );
    assert!(said.contains("obelus"), "it did not say who it is:\n{said}");
    let session = session.expect("a session of its own");

    let (listed, _) = ask(
        &url,
        Some(&session),
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
    );
    for tool in ["todo_list", "todo_finish", "todo_add"] {
        assert!(listed.contains(tool), "{tool} was not offered:\n{listed}");
    }
    // And the schema that came out of the signature, rather than one written
    // beside it and able to fall behind it.
    assert!(
        listed.contains("\"note\""),
        "the schema did not carry the argument's name:\n{listed}"
    );
}

/// The notes, as an agent reads them.
///
/// Every note carries the name the other tools take, which is what makes
/// the three of them one thing a reader can follow rather than three.
#[test]
fn an_agent_can_read_what_the_tree_means_to_come_back_to() {
    let (_scratch, url) = listening("tools-read");

    let (_, session) = ask(
        &url,
        None,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"a test","version":"0"}}}"#,
    );
    let session = session.expect("a session of its own");

    let (said, _) = ask(
        &url,
        Some(&session),
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"todo_list","arguments":{}}}"#,
    );
    assert!(
        said.contains("wire the counts tree up"),
        "the note was not in the answer:\n{said}"
    );
    assert!(
        said.contains("ABCDEFGH"),
        "the note's name was not in the answer, so nothing else could take it:\n{said}"
    );
}
