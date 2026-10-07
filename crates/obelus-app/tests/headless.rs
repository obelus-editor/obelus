//! Obelus with nobody at the screen: what it will not start without, what
//! it stops on, and what of the reader's it leaves alone.
//!
//! What it needs of the chat, and the chat going wrong under it, are in
//! `tests/remote.rs` beside the fake platform.

mod support;

use std::path::Path;

use obelus_app::{app::App, event::Event, startup};

/// What the binary would hand in, standing in for a commit nobody built.
const BUILT: &str = "abc1234";

/// A file named is refused before anything starts: an agent's writes to an
/// open file wait in it for a save, and nobody is there to make one.
///
/// Broken deliberately by taking the refusal out of `startup::build`: the
/// start went on, and stopped on something else.
#[test]
fn headless_opens_no_file() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.rs");
    let outcome = startup::start_headless(&[file], BUILT);
    let Err(error) = outcome else {
        panic!("a file was opened with nobody at the screen");
    };
    assert!(
        error.to_string().contains("opens no file"),
        "it stopped on {error}"
    );
}

/// Told to stop, it stops, and asks nothing on the way: whoever sent the
/// signal is not at the screen to answer.
///
/// Broken deliberately by handling `Event::Stopped` as `Event::Closed`: the
/// unwritten file was asked about and the loop went on.
#[test]
fn told_to_stop_it_stops_without_asking() {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 80, 24);
    support::type_text(&mut app, "unwritten");
    assert!(
        app.current_buffer().is_some_and(|buffer| buffer.is_dirty()),
        "nothing was left unwritten to ask about"
    );
    app.handle(Event::Stopped);
    assert!(app.should_quit(), "it is still running");
}

/// A directory named opens no list of its files, which nobody would choose
/// from and whose walk is the whole project.
///
/// Broken deliberately by taking `!self.headless` out of `App::start`: the
/// list was open.
#[test]
fn a_directory_opens_no_list() {
    let mut app = App::new(Vec::new());
    app.headless();
    app.working_directory_for_test(Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf());
    app.list_at_start();
    let (sender, _events) = obelus_app::event::channel();
    app.start(sender);
    assert!(app.picker().is_none(), "the list of files is open");
}

/// Told to stop from outside -- a `kill`, or `ctrl+c` on the terminal it
/// was started from -- it hears it as an event, rather than being ended
/// where it stands.
///
/// The test's own process is the one told, so a listener that was never
/// installed is a test binary killed by its own signal.
///
/// Broken deliberately by listening for `hangup` instead of `terminate`:
/// the binary died of the signal.
#[cfg(unix)]
#[test]
fn a_kill_is_heard_as_being_told_to_stop() {
    let (sender, events) = obelus_app::event::channel();
    obelus_app::event::stop_when_told(sender);
    let sent = std::process::Command::new("kill")
        .args(["-TERM", &std::process::id().to_string()])
        .status()
        .expect("running kill");
    assert!(sent.success(), "kill failed");
    let heard = events
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("nothing was heard");
    assert!(matches!(heard, Event::Stopped), "heard {heard:?}");
}

/// The tree going ends it, saying so, rather than putting up the page that
/// waits for a key.
///
/// Broken deliberately by taking `give_up_unseen` out of
/// `App::the_tree_has_gone`: the page went up and nothing ended.
#[test]
fn the_tree_going_ends_it() {
    let tree = std::env::temp_dir().join(format!("obelus-headless-gone-{}", std::process::id()));
    std::fs::create_dir_all(&tree).expect("a tree");
    let mut app = App::new(Vec::new());
    app.headless();
    app.working_directory_for_test(tree.clone());
    std::fs::remove_dir_all(&tree).expect("the tree going");
    app.handle(Event::Watched(obelus_watch::Changed {
        path: tree.join("anything"),
    }));
    assert!(app.should_quit(), "it waits on the page for a key");
    assert!(
        app.why_it_stopped()
            .is_some_and(|why| why.contains("has gone")),
        "it said {:?}",
        app.why_it_stopped()
    );
}

/// A worktree with two files in it, which reopens what was open.
fn tree(name: &str) -> support::Scratch {
    let scratch = support::Scratch::new(name);
    let outcome = support::git()
        .arg("-C")
        .arg(scratch.path())
        .args(["init", "--quiet"])
        .output()
        .expect("running git");
    assert!(outcome.status.success(), "git init failed");
    std::fs::write(scratch.path().join("a.rs"), "fn a() {}\n").expect("writing a");
    std::fs::write(scratch.path().join("b.rs"), "fn b() {}\n").expect("writing b");
    std::fs::create_dir_all(scratch.path().join(".obelus")).expect("making .obelus");
    std::fs::write(
        scratch.path().join(".obelus/config.toml"),
        "reopen = true\n",
    )
    .expect("writing the settings");
    scratch
}

/// The files open, by name.
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

/// Starts a window on the tree and waits for what it reopens.
fn a_window(root: &Path) -> App {
    let mut app = startup::start(&[root.to_path_buf()], BUILT).expect("starting");
    let events = support::drive(&mut app);
    loop {
        let event = events
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("what was open never arrived");
        let reopened = matches!(event, Event::Reopened { .. });
        app.handle(event);
        if reopened {
            return app;
        }
    }
}

/// What a window had open is neither opened again with nobody at the
/// screen, nor written over: the record is the last window's to change,
/// and a headless Obelus is not a window.
///
/// Broken deliberately twice: dropping `self.headless` from
/// `reopen_what_was_open`, and `b.rs` came back; and from `where_to_write`,
/// and the next window opened on `b.rs` alone, which is what the headless
/// one had open.
#[test]
fn what_a_window_had_open_is_left_alone() {
    let scratch = tree("headless-reopening");
    let root = scratch.path();

    let mut window = startup::start(&[root.join("a.rs")], BUILT).expect("starting");
    window.open_for_test(&root.join("b.rs"));
    support::lay_out(&mut window, 80, 24);
    drop(window);

    let mut unseen = App::new(Vec::new());
    unseen.headless();
    unseen.work_in(root.to_path_buf());
    unseen.load_config();
    unseen.reopen_what_was_open();
    let events = support::drive(&mut unseen);
    // Long enough for a record to be read on its thread and arrive, which
    // the window above took milliseconds over.
    while let Ok(event) = events.recv_timeout(std::time::Duration::from_millis(500)) {
        unseen.handle(event);
    }
    assert!(files(&unseen).is_empty(), "{:?} came back", files(&unseen));
    unseen.open_for_test(&root.join("b.rs"));
    support::lay_out(&mut unseen, 80, 24);
    drop(unseen);

    let again = a_window(root);
    assert_eq!(files(&again), ["a.rs", "b.rs"]);
}
