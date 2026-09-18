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

    // The one that only looks says so, which is what spares the reader a
    // question about a tool whose whole act is to look something up. The
    // two that write the notes say nothing of the sort, and a `readOnlyHint`
    // on either of them would be obelus telling an agent something untrue
    // about itself to buy a quieter turn.
    // Cut at the names rather than at the word, because a tool's
    // description may name another tool -- `todo_finish` names `todo_list`
    // in its own, which is where looking for the word found it.
    let reading = listed
        .split("\"name\":\"")
        .find(|tool| tool.starts_with("todo_list\""))
        .unwrap_or_default();
    assert!(
        reading.contains("\"readOnlyHint\":true"),
        "the tool that only reads did not say so:\n{listed}"
    );
    assert_eq!(
        listed.matches("\"readOnlyHint\":true").count(),
        1,
        "a tool that writes the notes says it only reads:\n{listed}"
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

/// What an agent writes down reaches the tree's file.
///
/// The half that was missing: the tools asked the reader and reported what
/// they chose, and nothing was ever written. A note offered and kept was a
/// note nobody had.
///
/// Driven over the wire and answered by a loop of this test's own, because
/// the writing is the loop's: the server hands it the act and waits for it
/// to be done, which is a wait on obelus rather than on a person.
#[test]
fn what_an_agent_writes_down_is_in_the_file() {
    use obelus::app::App;

    let scratch = support::Scratch::new("tools-written");
    std::fs::create_dir_all(scratch.path().join(".obelus")).expect("the directory");
    std::fs::write(
        scratch.path().join(".obelus").join("todo.toml"),
        "[[todo]]\nid = \"ABCDEFGH\"\nsaid = \"the one that was there\"\ndone = false\n",
    )
    .expect("the notes");

    let (sender, events) = channel();
    let url = obelus::mcp::serve(scratch.path(), sender).expect("a socket");
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(scratch.path().to_path_buf());

    // The server is on its own thread and waits on the loop, so the asking
    // goes in one and the answering happens here.
    let asking = std::thread::spawn({
        let url = url.clone();
        move || {
            let (_, session) = ask(
                &url,
                None,
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"a test","version":"0"}}}"#,
            );
            let session = session.expect("a session of its own");
            let (wrote, _) = ask(
                &url,
                Some(&session),
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"todo_add","arguments":{"under":"ABCDEFGH","notes":[{"said":"one worth coming back to"},{"said":"and another","depth":1}]}}}"#,
            );
            let (ticked, _) = ask(
                &url,
                Some(&session),
                r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"todo_finish","arguments":{"note":"ABCDEFGH"}}}"#,
            );
            (wrote, ticked)
        }
    });

    // Two acts to answer, and the thread is blocked on each in turn.
    for _ in 0..2 {
        let event = events
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the server asked the loop for something");
        app.handle(event);
    }
    let (wrote, ticked) = asking.join().expect("the agent's side");
    assert!(
        wrote.contains("written down: 2"),
        "it did not say so: {wrote}"
    );
    assert!(ticked.contains("ticked off"), "it did not say so: {ticked}");

    // And the file has them, which is the whole of what the tools are for.
    let todo = obelus::todo::Todo::read(scratch.path());
    let said: Vec<&str> = todo.notes.iter().map(|note| note.said.as_str()).collect();
    assert_eq!(
        said,
        vec![
            "the one that was there",
            "one worth coming back to",
            "and another"
        ]
    );
    // Under the one it named, and the second under the first of them: a
    // depth is counted from the top of what is being written down, and
    // landing under a note at the top puts the batch one level in.
    assert_eq!(
        todo.notes.iter().map(|note| note.depth).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert!(todo.notes[0].done, "the one it ticked is not ticked");
    assert!(!todo.notes[1].done, "it ticked one nobody asked it to");
}

/// A note that is a paragraph is still one note in the listing.
///
/// The list says a line beginning with a name is a note, and printing the
/// whole of a paragraph where a line was expected broke that: a three-line
/// note read as three notes, two of them nameless. Its first line goes
/// beside the name and the rest under it.
#[test]
fn a_note_of_several_lines_is_one_entry() {
    let scratch = support::Scratch::new("tools-paragraph");
    std::fs::create_dir_all(scratch.path().join(".obelus")).expect("the directory");
    std::fs::write(
        scratch.path().join(".obelus").join("todo.toml"),
        "[[todo]]\nid = \"ABCDEFGH\"\nsaid = \"\"\"\nthe title\nand a body\nof two lines\n\"\"\"\ndone = false\n",
    )
    .expect("the notes");

    let (sender, events) = channel();
    std::mem::forget(events);
    let url = obelus::mcp::serve(scratch.path(), sender).expect("a socket");
    let (_, session) = ask(
        &url,
        None,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"a test","version":"0"}}}"#,
    );
    let session = session.expect("a session of its own");
    let (listed, _) = ask(
        &url,
        Some(&session),
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"todo_list","arguments":{}}}"#,
    );

    // The name is on the line its first words are on, and nowhere else: the
    // two lines under it carry no name, which is what says they belong to it.
    // Out of the event stream the transport frames answers in: the note's
    // own newlines are escaped inside it, so the text has to be taken out
    // before there are lines to count.
    // The first `data:` is the stream opening and carries nothing, so it is
    // the first one that parses rather than the first one there is.
    let said = listed
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|json| serde_json::from_str::<serde_json::Value>(json).ok())
        .find_map(|value| {
            value["result"]["content"][0]["text"]
                .as_str()
                .map(str::to_string)
        })
        .unwrap_or_else(|| panic!("no answer in:\n{listed}"));
    let lines: Vec<&str> = said.lines().collect();
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.contains("ABCDEFGH"))
            .count(),
        1,
        "the name is not on one line of it:\n{said}"
    );
    let named = lines
        .iter()
        .position(|line| line.contains("ABCDEFGH"))
        .expect("the note");
    assert!(
        lines[named].ends_with("the title"),
        "the first line is not beside the name:\n{said}"
    );
    // Indented, which is the whole of what says they belong to the note
    // above rather than starting one of their own. Compared with their
    // spaces on: trimming them off is throwing away the thing being tested.
    assert_eq!(
        &lines[named + 1..=named + 2],
        ["  and a body", "  of two lines"],
        "the rest of it is not written under the name:\n{said}"
    );
}

/// A depth an agent asked for that nothing could hang at is brought up.
///
/// An agent counts from the top of what it is writing down and cannot know
/// what it will land under, so what it asks for has to be made into
/// something the file can carry: at most one deeper than the note above,
/// and never past the deepest a note may be. Written down as asked, it
/// would come back a level shallower the next time the file was read -- the
/// note moving on its own between one open and the next.
///
/// One call, and the file read as bytes: reading clamps, and a second call
/// would read and write the file again and launder the very thing this is
/// looking for.
#[test]
fn a_depth_nothing_could_hang_at_is_brought_up_before_it_is_written() {
    use obelus::app::App;

    let scratch = support::Scratch::new("tools-too-deep");
    std::fs::create_dir_all(scratch.path().join(".obelus")).expect("the directory");
    let file = scratch.path().join(".obelus").join("todo.toml");
    std::fs::write(
        &file,
        "[[todo]]\nid = \"ABCDEFGH\"\nsaid = \"the one\"\ndone = false\n",
    )
    .expect("the notes");

    let (sender, events) = channel();
    let url = obelus::mcp::serve(scratch.path(), sender).expect("a socket");
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(scratch.path().to_path_buf());

    let asking = std::thread::spawn(move || {
        let (_, session) = ask(
            &url,
            None,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"a test","version":"0"}}}"#,
        );
        let session = session.expect("a session of its own");
        ask(
            &url,
            Some(&session),
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"todo_add","arguments":{"notes":[{"said":"far too deep","depth":9}]}}}"#,
        );
    });
    let event = events
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("the server asked the loop for something");
    app.handle(event);
    asking.join().expect("the agent's side");

    let raw = std::fs::read_to_string(&file).expect("the notes");
    let written: Vec<u16> = raw
        .lines()
        .filter_map(|line| line.strip_prefix("depth = "))
        .filter_map(|depth| depth.parse().ok())
        .collect();
    assert_eq!(
        written,
        vec![0, 1],
        "what was written is not what reading it gives back:\n{raw}"
    );
}
