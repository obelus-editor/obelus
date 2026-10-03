//! What was open comes back when the tree is opened again.
//!
//! Each test puts one Obelus on a tree, closes it, and starts another on
//! the same tree -- the way a reader leaves and comes back. The files go
//! through `startup::start`, which is where a reader comes in; whether to
//! is said in the tree's own `.obelus/config.toml`, so the answer does not
//! hang on the settings of the machine the suite runs on.

mod support;

use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use obelus_app::{
    app::{App, document::Document},
    startup,
};
use obelus_buffer::DocumentId;

/// What the binary would hand in, standing in for a commit nobody built.
const BUILT: &str = "abc1234";

/// A worktree with two files in it, saying whether to reopen.
///
/// A worktree because only a worktree keeps a record: git has to have
/// heard of it, and nothing has to be committed for that.
fn tree(name: &str, reopen: bool) -> support::Scratch {
    let scratch = support::Scratch::new(name);
    let outcome = std::process::Command::new("git")
        .arg("-C")
        .arg(scratch.path())
        .args(["init", "--quiet"])
        .output()
        .expect("running git");
    assert!(outcome.status.success(), "git init failed");
    let lines: String = (1..=60).map(|line| format!("line {line}\n")).collect();
    std::fs::write(scratch.path().join("a.rs"), &lines).expect("writing a");
    std::fs::write(scratch.path().join("b.rs"), &lines).expect("writing b");
    std::fs::create_dir_all(scratch.path().join(".obelus")).expect("making .obelus");
    std::fs::write(
        scratch.path().join(".obelus/config.toml"),
        format!("reopen = {reopen}\n"),
    )
    .expect("writing the settings");
    scratch
}

/// Draws a frame, which is when what is open is written down.
fn frame(app: &mut App) {
    support::lay_out(app, 80, 24);
}

/// The files open, by name, in the order they are in the list.
fn files(app: &App) -> Vec<String> {
    app.buffers_for_test()
        .into_iter()
        .map(|(path, _)| {
            path.file_name()
                .expect("a file")
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

/// The name of the file on screen.
fn showing(app: &App) -> Option<String> {
    let buffer = app.current_buffer()?;
    Some(buffer.path().file_name()?.to_string_lossy().into_owned())
}

/// Every document in the list.
fn documents(app: &App) -> Vec<&Document> {
    (0..)
        .map(DocumentId::new)
        .map_while(|id| app.document(id))
        .collect()
}

/// Starts on a tree that has a record, and waits for what it names to be
/// read -- which happens on a thread, as soon as there is a channel.
fn start_again(paths: &[std::path::PathBuf]) -> App {
    let mut app = startup::start(paths, BUILT).expect("starting again");
    arrive(&mut app);
    app
}

/// Gives a window its channel and waits for the files it is reopening.
fn arrive(app: &mut App) {
    let events = support::drive(app);
    loop {
        let event = events
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("what was open never arrived");
        let reopened = matches!(event, obelus_app::event::Event::Reopened(_));
        app.handle(event);
        if reopened {
            return;
        }
    }
}

/// Opens `b.rs`, walks five lines down it, and goes back to `a.rs`.
///
/// Going back is a change of which document is on screen, so the frame
/// after it writes the record -- carrying where the caret was in `b.rs`.
fn read_down_b(app: &mut App, root: &Path) {
    app.open_for_test(&root.join("b.rs"));
    // Laid out first: a key moves the caret through rows, and there are no
    // rows until there is a screen.
    frame(app);
    for _ in 0..5 {
        support::press(app, KeyCode::Down);
    }
    app.open_for_test(&root.join("a.rs"));
    frame(app);
}

/// The files a tree had open come back, with the caret where it was in
/// each, and the reader is on the one they were reading.
///
/// Broken deliberately twice: dropping the call in `startup::start`
/// reopened nothing, and dropping `place_cursor` from `reopen_file` left
/// `b.rs`'s caret on its first line.
#[test]
fn the_files_come_back_with_their_carets() {
    let scratch = tree("reopening-files", true);
    let root = scratch.path();

    let mut first = startup::start(&[root.join("a.rs")], BUILT).expect("starting");
    read_down_b(&mut first, root);
    drop(first);

    // The tree and nothing else: no file named, so what is on screen is
    // what was.
    let mut second = start_again(&[root.to_path_buf()]);
    assert_eq!(files(&second), ["a.rs", "b.rs"], "not what was open");
    assert_eq!(
        showing(&second).as_deref(),
        Some("a.rs"),
        "not where the reader was"
    );
    second.open_for_test(&root.join("b.rs"));
    let caret = second.current_buffer().expect("b.rs").cursor();
    assert_eq!(caret.line.get(), 5, "the caret is not where it was in b.rs");
}

/// A file named on the command line is where the reader lands, and what
/// was open comes back beside it.
///
/// Broken deliberately by going to the record's own document whether or
/// not anything was named: the reader landed on `b.rs`.
#[test]
fn a_file_named_is_where_the_reader_lands() {
    let scratch = tree("reopening-named", true);
    let root = scratch.path();
    let c = root.join("c.rs");
    std::fs::write(&c, "fn c() {}\n").expect("writing c");

    let mut first = startup::start(&[root.join("b.rs")], BUILT).expect("starting");
    first.open_for_test(&root.join("a.rs"));
    first.open_for_test(&root.join("b.rs"));
    frame(&mut first);
    drop(first);

    let second = start_again(std::slice::from_ref(&c));
    assert_eq!(
        showing(&second).as_deref(),
        Some("c.rs"),
        "not on the file named"
    );
    let mut open = files(&second);
    open.sort();
    assert_eq!(
        open,
        ["a.rs", "b.rs", "c.rs"],
        "what was open did not come back"
    );
}

/// The notes come back on the note the reader was on.
///
/// And still there once the page has read its file again, which a window
/// does as soon as its watcher is up: reading again puts the selection back
/// on the note the caret is in, so a note only selected and not entered
/// went back to the first.
///
/// Broken deliberately twice: not entering the note left it on the first,
/// and selecting it with `focus` instead left it on the first after the
/// file was read again.
#[test]
fn the_notes_come_back_on_the_note_the_reader_was_on() {
    let scratch = tree("reopening-notes", true);
    let root = scratch.path();
    support::make_room_for_notes(root);
    std::fs::write(
        obelus_git::todo::path(root).expect("a tree that is there"),
        "[[todo]]\nid = \"0123456R\"\nsaid = \"the first\"\ndone = false\ndepth = 0\n\n\
         [[todo]]\nid = \"0123456S\"\nsaid = \"the second\"\ndone = false\ndepth = 0\n\n\
         [[todo]]\nid = \"0123456T\"\nsaid = \"the third\"\ndone = false\ndepth = 0\n",
    )
    .expect("the notes");

    let mut first = startup::start(&[root.join("a.rs")], BUILT).expect("starting");
    first.open_todo();
    frame(&mut first);
    support::press(&mut first, KeyCode::Down);
    // Walking the notes is not a change to what is open; going to a file is.
    first.open_for_test(&root.join("a.rs"));
    frame(&mut first);
    drop(first);

    let mut second = start_again(&[root.to_path_buf()]);
    frame(&mut second);
    second.handle(obelus_app::event::Event::Watched(obelus_watch::Changed {
        path: obelus_git::todo::path(root).expect("a tree that is there"),
    }));
    frame(&mut second);
    let on = documents(&second)
        .into_iter()
        .find_map(Document::notes)
        .expect("the notes came back")
        .selected_note()
        .map(|note| note.id.as_str().to_string());
    assert_eq!(
        on.as_deref(),
        Some("0123456S"),
        "not on the note the reader was on"
    );
}

/// The first screen does not wait for the files: they are read on a
/// thread and put in the list when they arrive. Until then nothing is
/// written, and a reader who has gone somewhere meanwhile is left there.
///
/// Broken deliberately three times: putting the files in the list as the
/// record is read failed the first assertion; writing while they were being
/// read wrote a record of `c.rs` alone, and the next start had nothing
/// else; and going to the record's own document whatever the screen had on
/// it took the reader off `c.rs`.
#[test]
fn the_files_are_read_after_the_first_screen() {
    let scratch = tree("reopening-later", true);
    let root = scratch.path();
    let c = root.join("c.rs");
    std::fs::write(&c, "fn c() {}\n").expect("writing c");

    let mut first = startup::start(&[root.join("a.rs")], BUILT).expect("starting");
    read_down_b(&mut first, root);
    drop(first);

    let mut second = startup::start(&[root.to_path_buf()], BUILT).expect("starting again");
    assert!(
        files(&second).is_empty(),
        "the files were read before the first screen"
    );
    // The reader goes somewhere while they are read, and a frame is drawn.
    second.open_for_test(&c);
    frame(&mut second);
    drop(second);

    let third = start_again(&[root.to_path_buf()]);
    assert_eq!(
        files(&third),
        ["a.rs", "b.rs"],
        "a window that never had them wrote a record over them"
    );
    drop(third);
    let mut fourth = startup::start(&[root.to_path_buf()], BUILT).expect("a fourth");
    fourth.open_for_test(&c);
    arrive(&mut fourth);
    assert_eq!(
        showing(&fourth).as_deref(),
        Some("c.rs"),
        "the reader was taken off where they went"
    );
}

/// A tree that has gone takes its record with it, the next time any window
/// starts -- `git worktree remove` is done in a shell, with nothing of
/// Obelus's on the tree to see it go.
///
/// Broken deliberately by not sweeping after the files are read: the
/// record of the removed tree was still there.
#[test]
fn a_tree_that_has_gone_takes_its_record_with_it() {
    let gone = tree("reopening-removed", true);
    let mut first = startup::start(&[gone.path().join("a.rs")], BUILT).expect("starting");
    frame(&mut first);
    drop(first);
    let record = support::record_of_what_was_open(&gone);
    std::fs::remove_dir_all(gone.path()).expect("removing the tree");

    let here = tree("reopening-elsewhere", true);
    let mut other = startup::start(&[here.path().join("a.rs")], BUILT).expect("starting");
    frame(&mut other);
    drop(other);
    let _again = start_again(&[here.path().to_path_buf()]);
    assert!(
        !record.exists(),
        "the record of a tree that has gone is still there"
    );
}

/// Switched off, nothing comes back.
///
/// Broken deliberately by taking the setting out of the check in
/// `reopen_what_was_open`: both files came back.
#[test]
fn switched_off_nothing_comes_back() {
    let scratch = tree("reopening-off", true);
    let root = scratch.path();

    let mut first = startup::start(&[root.join("a.rs")], BUILT).expect("starting");
    read_down_b(&mut first, root);
    drop(first);
    std::fs::write(root.join(".obelus/config.toml"), "reopen = false\n").expect("switching off");

    let second = startup::start(&[root.to_path_buf()], BUILT).expect("starting again");
    assert!(
        files(&second).is_empty(),
        "something came back with the setting off"
    );
}

/// The way out says where the caret is now, which no frame had written:
/// moving the caret is not a change to what is open.
///
/// Broken deliberately by taking the call out of `app::run`: `a.rs` came
/// back with its caret on the first line.
#[test]
fn leaving_says_where_the_caret_is() {
    let scratch = tree("reopening-leaving", true);
    let root = scratch.path();

    let mut first = startup::start(&[root.join("a.rs")], BUILT).expect("starting");
    frame(&mut first);
    for _ in 0..7 {
        support::press(&mut first, KeyCode::Down);
    }
    leave(&mut first);
    drop(first);

    let second = start_again(&[root.to_path_buf()]);
    let caret = second.current_buffer().expect("a.rs").cursor();
    assert_eq!(
        caret.line.get(),
        7,
        "the caret is where the last frame left it"
    );
}

/// Two windows on one tree: the last to change what it has open wins, and
/// the other leaving afterwards does not put its own back.
///
/// Broken deliberately by writing on the way out whatever the record says:
/// the third window opened on `a.rs` alone.
#[test]
fn the_last_window_to_change_wins() {
    let scratch = tree("reopening-two", true);
    let root = scratch.path();

    let mut one = startup::start(&[root.join("a.rs")], BUILT).expect("one");
    frame(&mut one);
    let mut two = start_again(&[root.join("b.rs")]);
    // Two came up with a.rs from one's record beside b.rs; it closes a.rs,
    // which is a change, after anything one did.
    two.open_for_test(&root.join("a.rs"));
    two.close_current();
    frame(&mut two);
    assert_eq!(files(&two), ["b.rs"]);
    drop(two);
    // And one only reads, then leaves.
    support::press(&mut one, KeyCode::Down);
    frame(&mut one);
    leave(&mut one);
    drop(one);

    let three = start_again(&[root.to_path_buf()]);
    assert_eq!(files(&three), ["b.rs"], "the window that left last won");
}

/// A conversation comes back, claimed, unless another window has it.
///
/// Not through `startup::start`, because which agent is in use is the
/// reader's own setting and no project may say it: the agent is named
/// here and never started -- a conversation is taken up when it is shown,
/// and nothing here asks for a session.
///
/// Broken deliberately twice: recording no conversations left the second
/// window with the notes alone, and claiming nothing before reopening one
/// put it in the third window while the second still had it.
#[test]
fn a_conversation_comes_back_unless_another_window_has_it() {
    let scratch = tree("reopening-conversation", true);
    let root = scratch.path();
    let note = obelus_git::todo::NoteId::read("0123456R").expect("a name");
    support::make_room_for_notes(root);
    std::fs::write(
        obelus_git::todo::path(root).expect("a tree that is there"),
        "[[todo]]\nid = \"0123456R\"\nsaid = \"reopen what was open\"\n\
         done = false\ndepth = 0\n",
    )
    .expect("the notes");
    obelus_agent::acp::sessions::change(root, None, |remembered| {
        remembered.put(
            &obelus_agent::chats::ChatId::Note(note.clone()),
            "fake",
            root,
            obelus_agent::acp::sessions::Kept {
                session: "s-1".to_string(),
                title: None,
                told: None,
                introduced: false,
                last: Some(1_700_000_000),
            },
        );
    });
    let window = || {
        let mut app = App::new(Vec::new());
        app.configure(
            obelus_config::Config {
                agent: Some("fake".to_string()),
                ..obelus_config::Config::default()
            },
            Vec::new(),
        );
        app.working_directory_for_test(root.to_path_buf());
        app
    };
    let about = |app: &App| {
        app.current_document_for_test()
            .and_then(|id| app.document(id))
            .and_then(Document::chat)
            .map(|talk| talk.topic.clone())
    };

    let mut first = window();
    first.open_todo();
    support::press_alt(&mut first, 'a');
    assert!(
        about(&first).is_some(),
        "the key did not open the conversation"
    );
    frame(&mut first);
    drop(first);

    let mut second = window();
    second.reopen_what_was_open();
    arrive(&mut second);
    assert_eq!(
        about(&second),
        Some(obelus_app::conversation::Topic::Note(note.clone())),
        "the conversation did not come back"
    );
    assert!(
        documents(&second)
            .iter()
            .any(|document| document.notes().is_some()),
        "the notes did not come back"
    );

    // While the second has it, a third gets the notes and not the
    // conversation: it is one window's at a time.
    let mut third = window();
    third.reopen_what_was_open();
    arrive(&mut third);
    assert!(
        documents(&third)
            .iter()
            .all(|document| document.chat().is_none()),
        "a conversation another window has came back here as well"
    );
    assert!(
        third.notes().is_some(),
        "the notes were not what it landed on"
    );
}

/// Leaves the way a reader does, through the loop.
fn leave(app: &mut App) {
    let (sender, events) = std::sync::mpsc::channel();
    sender
        .send(obelus_app::event::Event::Key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::CONTROL,
        )))
        .expect("sending the key");
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).expect("a terminal");
    obelus_app::app::run(&mut terminal, app, events).expect("running");
}
