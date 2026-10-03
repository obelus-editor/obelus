//! What Obelus offers an agent, over the wire an agent really uses.
//!
//! The server is started the way the loop starts it and spoken to with
//! `ureq`, which is already here for the registry: what is being tested is
//! that an agent asking in the ordinary way gets the ordinary answers, and a
//! test that called the handler directly would not have told anyone whether
//! the transport was mounted at all.

mod support;

use std::sync::mpsc::channel;

/// Obelus listening on a tree, and where the first conversation reaches
/// it -- which is the address an agent is told, not the server's own.
fn served(
    root: &std::path::Path,
    sender: std::sync::mpsc::Sender<obelus_app::event::Event>,
) -> String {
    let (server, listening) =
        obelus_mcp::serve(root, std::sync::Arc::new(sender)).expect("a socket");
    // Listening for the life of the test, which is what the loop's own
    // hold on it is for the life of the project.
    std::mem::forget(listening);
    obelus_mcp::address(&server, 0)
}

/// A tree with one note, and Obelus listening on it.
fn listening(name: &str) -> (support::Scratch, String) {
    let scratch = support::Scratch::new(name);
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"ABCDEFGH\"\nsaid = \"wire the counts tree up\"\ndone = false\n",
    )
    .expect("the notes");

    let (sender, events) = channel::<obelus_app::event::Event>();
    // Held for the life of the test: a server whose main loop has gone
    // answers a tool that asks the reader with "nobody is there", which is
    // not what these are about.
    std::mem::forget(events);
    let url = served(scratch.path(), sender);
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
    let mut answer = request.send(body).expect("An answer");
    let named = answer
        .headers()
        .get("mcp-session-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let said = answer.body_mut().read_to_string().expect("what it said");
    (said, named)
}

/// The handshake, and then what Obelus says it can do.
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
    assert!(said.contains("Obelus"), "it did not say who it is:\n{said}");
    let session = session.expect("a session of its own");

    let (listed, _) = ask(
        &url,
        Some(&session),
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
    );
    for tool in [
        "todo_list",
        "todo_finish",
        "todo_add",
        "todo_reword",
        "open_file",
        "read_workflow",
        "close_conversation",
    ] {
        assert!(listed.contains(tool), "{tool} was not offered:\n{listed}");
    }
    // And the schema that came out of the signature, rather than one written
    // beside it and able to fall behind it.
    assert!(
        listed.contains("\"note\""),
        "the schema did not carry the argument's name:\n{listed}"
    );
    for named in ["\"path\"", "\"line\""] {
        assert!(
            listed.contains(named),
            "the schema did not carry {named}:\n{listed}"
        );
    }

    // The two that only look say so, which is what spares the reader a
    // question about a tool whose whole act is to look something up. The
    // three that write the notes say nothing of the sort, and a
    // `readOnlyHint` on any of them would be Obelus telling an agent
    // something untrue about itself to buy a quieter turn.
    // Cut at the names rather than at the word, because a tool's
    // description may name another tool -- `todo_finish` names `todo_list`
    // in its own, which is where looking for the word found it.
    for only_reads in ["todo_list", "read_workflow"] {
        let reading = listed
            .split("\"name\":\"")
            .find(|tool| tool.starts_with(&format!("{only_reads}\"")))
            .unwrap_or_default();
        assert!(
            reading.contains("\"readOnlyHint\":true"),
            "{only_reads} only reads and did not say so:\n{listed}"
        );
    }
    assert_eq!(
        listed.matches("\"readOnlyHint\":true").count(),
        2,
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
/// to be done, which is a wait on Obelus rather than on a person.
#[test]
fn what_an_agent_writes_down_is_in_the_file() {
    use obelus_app::app::App;

    let scratch = support::Scratch::new("tools-written");
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"ABCDEFGH\"\nsaid = \"the one that was there\"\ndone = false\n",
    )
    .expect("the notes");

    let (sender, events) = channel::<obelus_app::event::Event>();
    let url = served(scratch.path(), sender);
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
    let todo = obelus_git::todo::read(scratch.path())
        .notes()
        .expect("the notes");
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
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"ABCDEFGH\"\nsaid = \"\"\"\nthe title\nand a body\nof two lines\n\"\"\"\ndone = false\n",
    )
    .expect("the notes");

    let (sender, events) = channel::<obelus_app::event::Event>();
    std::mem::forget(events);
    let url = served(scratch.path(), sender);
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
    use obelus_app::app::App;

    let scratch = support::Scratch::new("tools-too-deep");
    support::make_room_for_notes(scratch.path());
    let file = obelus_git::todo::path(scratch.path()).expect("a tree that is there");
    std::fs::write(
        &file,
        "[[todo]]\nid = \"ABCDEFGH\"\nsaid = \"the one\"\ndone = false\n",
    )
    .expect("the notes");

    let (sender, events) = channel::<obelus_app::event::Event>();
    let url = served(scratch.path(), sender);
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

/// What a rewording changes is the words, and the note is the same note.
///
/// The one act here that loses something, so what it must not lose as well
/// is everything that is not words: its name -- which every conversation
/// Obelus has written down is keyed to -- where it points, whether it is
/// ticked, and what hangs under it. Written as a remove and an insert this
/// would pass a test that only read the text back, and the reader would
/// find a note that had lost its place, its children and the conversation
/// they had about it.
///
/// Broken deliberately by minting a fresh name in the rewording arm, which
/// is what an insert would do: the text still reads back and the name
/// assertion fails.
#[test]
fn a_reworded_note_is_the_same_note() {
    use obelus_app::app::App;

    let scratch = support::Scratch::new("tools-reworded");
    support::make_room_for_notes(scratch.path());
    let file = obelus_git::todo::path(scratch.path()).expect("a tree that is there");
    std::fs::write(
        &file,
        "[[todo]]\nid = \"ABCDEFGH\"\nsaid = \"what it was written for\"\n\
         done = true\ndepth = 0\nat = \"src/counts.rs\"\nline = 12\n\n\
         [[todo]]\nid = \"JKMNPQRS\"\nsaid = \"the part under it\"\ndone = false\ndepth = 1\n\n\
         [[todo]]\nid = \"TVWXYZ01\"\nsaid = \"somebody else's note\"\ndone = false\ndepth = 0\n",
    )
    .expect("the notes");

    let (sender, events) = channel::<obelus_app::event::Event>();
    let url = served(scratch.path(), sender);
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
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"todo_reword","arguments":{"note":"ABCDEFGH","said":"what the work turned out to be about"}}}"#,
        )
    });
    let event = events
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("the server asked the loop for something");
    app.handle(event);
    let (answered, _) = asking.join().expect("the agent's side");
    assert!(
        answered.contains("reworded"),
        "it did not say so: {answered}"
    );

    let todo = obelus_git::todo::read(scratch.path())
        .notes()
        .expect("the notes");
    let said: Vec<&str> = todo.notes.iter().map(|note| note.said.as_str()).collect();
    assert_eq!(
        said,
        vec![
            "what the work turned out to be about",
            "the part under it",
            "somebody else's note"
        ],
        "the words are not what was asked for, or another note was touched"
    );
    // The same note, which is the half a rewording could quietly lose.
    let names: Vec<String> = todo.notes.iter().map(|note| note.id.to_string()).collect();
    assert_eq!(names, vec!["ABCDEFGH", "JKMNPQRS", "TVWXYZ01"]);
    assert_eq!(
        todo.notes.iter().map(|note| note.depth).collect::<Vec<_>>(),
        vec![0, 1, 0],
        "what hung under it does not hang under it any more"
    );
    assert!(
        todo.notes[0].done,
        "rewording a note unticked it, which nobody asked for"
    );
    let at = todo.notes[0].at.as_ref().expect("where it points");
    assert_eq!(at.path, std::path::Path::new("src/counts.rs"));
    assert_eq!(at.line.get(), 11, "the line it points at moved");
}

/// A note cannot be reworded into saying nothing.
///
/// Which would be a deletion through another door: reading the file drops a
/// note with no words in it, so an empty rewording is the one act that is
/// the reader's, spelled differently. It is refused and the note is left
/// exactly as it was.
///
/// Broken deliberately by taking the emptiness check out of the rewording
/// arm. It answers "reworded" and this fails on that first; the file left
/// behind is empty, which is the note deleted and the reason the check is
/// there.
#[test]
fn a_note_cannot_be_reworded_into_nothing() {
    use obelus_app::app::App;

    let scratch = support::Scratch::new("tools-reworded-empty");
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        "[[todo]]\nid = \"ABCDEFGH\"\nsaid = \"the one that was there\"\ndone = false\ndepth = 0\n",
    )
    .expect("the notes");

    let (sender, events) = channel::<obelus_app::event::Event>();
    let url = served(scratch.path(), sender);
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
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"todo_reword","arguments":{"note":"ABCDEFGH","said":"   \n  "}}}"#,
        )
    });
    let event = events
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("the server asked the loop for something");
    app.handle(event);
    let (answered, _) = asking.join().expect("the agent's side");
    assert!(
        answered.contains("cannot be made to say nothing"),
        "it did not say why it would not: {answered}"
    );

    let todo = obelus_git::todo::read(scratch.path())
        .notes()
        .expect("the notes");
    assert_eq!(todo.notes.len(), 1, "the note went away");
    assert_eq!(todo.notes[0].said, "the one that was there");
}

/// A file an agent offers goes on the reader's screen, at the line it named.
///
/// The one tool that changes nothing: what it takes is the reader's
/// attention. It goes through the same door every other jump in Obelus goes
/// through -- so a file already open is the one they are taken to rather
/// than a second copy of it, which `picker` already holds to, and the jump
/// is recorded.
///
/// What is this tool's own is the two conversions between what an agent
/// says and what Obelus reads: a path against the project, and a line
/// counted from one.
///
/// Deliberate break: send the line straight through instead of taking one
/// off it and the reader lands a line past the one the agent meant, which
/// is silent because both are lines of the same file. Open the path as
/// given rather than against the project and nothing opens at all. Drop
/// the check that it landed and a file that is not there is reported as
/// being on their screen.
#[test]
fn a_file_an_agent_offers_is_on_the_readers_screen() {
    use obelus_app::app::App;

    let scratch = support::Scratch::new("tools-opened");
    support::make_room_for_notes(scratch.path());
    let file = scratch.path().join("sample.rs");
    std::fs::write(
        &file,
        (1..=40)
            .map(|n| format!("// line {n}\n"))
            .collect::<String>(),
    )
    .expect("a file to open");
    let other = scratch.path().join("other.rs");
    std::fs::write(&other, "// somewhere else\n").expect("another file");

    let (sender, events) = channel::<obelus_app::event::Event>();
    let url = served(scratch.path(), sender);
    // Open on the other file, so being taken to `sample.rs` is a move
    // rather than the only thing there is.
    let mut app = App::new(vec![
        obelus_buffer::Buffer::open(&other).expect("the other file"),
    ]);
    app.working_directory_for_test(scratch.path().to_path_buf());

    let asking = std::thread::spawn({
        let url = url.clone();
        move || {
            let (_, session) = ask(
                &url,
                None,
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"a test","version":"0"}}}"#,
            );
            let session = session.expect("a session of its own");
            let (opened, _) = ask(
                &url,
                Some(&session),
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"open_file","arguments":{"path":"sample.rs","line":20}}}"#,
            );
            let (missing, _) = ask(
                &url,
                Some(&session),
                r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"open_file","arguments":{"path":"nothing-here.rs"}}}"#,
            );
            (opened, missing)
        }
    });

    for _ in 0..2 {
        let event = events
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the server asked the loop for something");
        app.handle(event);
    }
    let (opened, missing) = asking.join().expect("the agent's side");

    assert!(
        opened.contains("on the reader's screen"),
        "it did not say it had: {opened}"
    );
    assert!(
        missing.contains("would not open"),
        "a file that is not there was said to be on their screen: {missing}"
    );

    // The file the agent named, found against the project, and the line
    // it named, counted from one by the agent and from zero by the buffer.
    let buffer = app.current_buffer().expect("a file to read");
    assert_eq!(buffer.path(), file, "the reader is on the wrong file");
    assert_eq!(
        buffer.cursor().line.get(),
        19,
        "the caret is not on the line the agent sent them to"
    );
}

/// What Obelus says before the reader's first words names a tool an agent
/// really has.
///
/// The opening offers `open_file` in prose, and the tool's name comes out
/// of a function signature by way of a macro -- so a rename there leaves
/// the opening telling every agent about a tool that is not offered, which
/// nothing on either side would notice. A writer and a reader in the same
/// program have a test that the one reads the other.
///
/// The name and not the sentence. The opening is prose and is reworded by
/// whoever is tuning what an agent does; what may not drift is the name,
/// which is a name and is written the way it is written everywhere else.
///
/// Deliberate break: take the paragraph out of `always.txt` and an agent
/// is never told it can put a file in front of the reader, which is the
/// half of this that no tool listing can say.
#[test]
fn the_opening_names_the_tool_it_offers() {
    let always = include_str!("../src/app/always.txt");
    assert!(
        always.contains("open_file"),
        "the opening does not tell an agent it can open a file:\n{always}"
    );

    let (_scratch, url) = listening("tools-opening");
    let (_, session) = ask(
        &url,
        None,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"a test","version":"0"}}}"#,
    );
    let (listed, _) = ask(
        &url,
        session.as_deref(),
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
    );
    assert!(
        listed.contains("open_file"),
        "the opening names a tool nothing offers:\n{listed}"
    );
    // And the other tool the opening offers in prose, by the same rule.
    // Deliberate break: take the closing paragraph out of `always.txt`.
    assert!(
        always.contains("close_conversation"),
        "the opening does not tell an agent it can close a conversation:\n{always}"
    );
    assert!(
        listed.contains("close_conversation"),
        "the opening names a tool nothing offers:\n{listed}"
    );
}

/// A conversation an agent closes is the one whose address it called,
/// read off the request it came on.
///
/// Over the wire, because the number is in the path and nowhere else: a
/// test that handed the loop a number directly would say nothing about
/// whether the server reads one.
///
/// Deliberate break: have `conversation_in` answer `None` whatever the
/// path, and the agent is told its address names no conversation while
/// the conversation stays open.
#[test]
fn an_agent_closes_the_conversation_whose_address_it_called() {
    use obelus_app::app::App;

    let scratch = support::Scratch::new("tools-closed");
    support::make_room_for_notes(scratch.path());
    let (sender, events) = channel::<obelus_app::event::Event>();
    let url = served(scratch.path(), sender);
    // The first document, so it is the conversation `/mcp/0` names.
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.new_conversation();
    assert!(app.chat().is_some(), "no conversation to close");

    let asking = std::thread::spawn(move || {
        let (_, session) = ask(
            &url,
            None,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"a test","version":"0"}}}"#,
        );
        let session = session.expect("a session of its own");
        let (closed, _) = ask(
            &url,
            Some(&session),
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"close_conversation","arguments":{}}}"#,
        );
        closed
    });
    let event = events
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("the server asked the loop for something");
    app.handle(event);
    let closed = asking.join().expect("the agent's side");

    assert!(closed.contains("closed"), "it did not say it had: {closed}");
    assert!(app.chat().is_none(), "the conversation is still open");
}
