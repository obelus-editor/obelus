//! Going to another of the repository's worktrees, from the list of what
//! is open: enter puts this Obelus there, and `ctrl+enter` a window of its
//! own.
//!
//! Against real git and a front end that writes down what it was asked:
//! what a window does with a request -- a process started, a compositor
//! asked -- is the window's and is not here. What is here is everything
//! the application decides, and the knock that crosses from one window to
//! another, which two applications in one process can do for real.

mod support;

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc::Receiver},
    time::{Duration, Instant},
};

use crossterm::event::KeyCode;
use obelus_app::{
    app::{App, Door, Windows, dispatch},
    event::Event,
};
use obelus_command::Command;
use support::{Scratch, press, press_control_key, press_function};

/// A front end that does nothing but write down what it was asked.
#[derive(Debug, Default)]
struct Asked {
    opened: Mutex<Vec<PathBuf>>,
    brought: Mutex<Vec<Door>>,
    came: Mutex<Vec<Option<String>>>,
    cannot_bring: bool,
}

impl Windows for Asked {
    fn open(&self, tree: &Path) {
        self.opened
            .lock()
            .expect("the list")
            .push(tree.to_path_buf());
    }

    fn bring(&self, door: &Door) {
        self.brought.lock().expect("the list").push(door.clone());
    }

    fn come_forward(&self, token: Option<String>) {
        self.came.lock().expect("the list").push(token);
    }

    fn can_bring(&self) -> bool {
        !self.cannot_bring
    }
}

/// Runs git in a directory, with an identity of its own.
fn git(directory: &Path, arguments: &[&str]) {
    let outcome = std::process::Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(arguments)
        .env("GIT_AUTHOR_NAME", "obelus")
        .env("GIT_AUTHOR_EMAIL", "obelus@example.invalid")
        .env("GIT_COMMITTER_NAME", "obelus")
        .env("GIT_COMMITTER_EMAIL", "obelus@example.invalid")
        .output()
        .expect("running git");
    assert!(
        outcome.status.success(),
        "git {arguments:?} failed: {}",
        String::from_utf8_lossy(&outcome.stderr)
    );
}

/// A repository with two linked worktrees beside it, all three in one
/// directory: `main` on `master`, `feature` on `feature`, `spare` on
/// `spare`.
fn repository(scratch: &Scratch) -> (PathBuf, PathBuf, PathBuf) {
    let main = scratch.join("main");
    std::fs::create_dir_all(&main).expect("the main checkout");
    git(&main, &["init", "--quiet", "--initial-branch=master"]);
    std::fs::write(main.join("file.rs"), "fn main() {}\n").expect("a file");
    git(&main, &["add", "file.rs"]);
    git(&main, &["commit", "--quiet", "-m", "committed"]);
    let (feature, spare) = (scratch.join("feature"), scratch.join("spare"));
    for (path, branch) in [(&feature, "feature"), (&spare, "spare")] {
        git(
            &main,
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                branch,
                path.to_str().expect("a path"),
            ],
        );
    }
    (main, feature, spare)
}

/// A window on `tree`, saying where it is to the others.
fn window_on(tree: &Path, asked: &Arc<Asked>) -> (App, Receiver<Event>) {
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(tree.to_path_buf());
    let events = support::drive(&mut app);
    app.windowed_by(Arc::clone(asked) as Arc<dyn Windows>);
    support::lay_out(&mut app, 80, 24);
    (app, events)
}

/// The tabs of the list on screen, and which one is showing.
fn tabs(app: &App) -> (Vec<String>, usize) {
    let picker = app.picker().expect("a list is showing");
    (picker.tabs().to_vec(), picker.tab())
}

/// One row of the list on screen: what it says, what follows it, what
/// stands at its end, whether it is marked, and whether it can be chosen.
type Row = (String, Option<String>, Option<String>, bool, bool);

/// The rows of the list on screen.
fn rows(app: &App) -> Vec<Row> {
    app.picker()
        .expect("a list is showing")
        .matches()
        .map(|item| {
            (
                item.label.clone(),
                item.detail.clone(),
                item.trailing.clone(),
                item.marker.is_some(),
                item.enabled,
            )
        })
        .collect()
}

/// Walks the selection to the row that says `label`.
fn walk_to(app: &mut App, label: &str) {
    press(app, KeyCode::Home);
    for _ in 0..rows(app).len() {
        let selected = app
            .picker()
            .and_then(obelus_component::picker::Picker::selected_item)
            .map(|item| item.label.clone());
        if selected.as_deref() == Some(label) {
            return;
        }
        press(app, KeyCode::Down);
    }
    panic!("no row says {label}: {:?}", rows(app));
}

/// Walks the selection to the row that says `label`, and chooses it.
fn choose(app: &mut App, label: &str) {
    walk_to(app, label);
    press(app, KeyCode::Enter);
}

/// The same, for a window of its own.
fn choose_elsewhere(app: &mut App, label: &str) {
    walk_to(app, label);
    press_control_key(app, KeyCode::Enter);
}

/// The same place, however it is spelled.
fn resolved(path: &Path) -> PathBuf {
    path.canonicalize().expect("a tree that is there")
}

/// An Obelus in a terminal on `tree`: no front end to say what it can do
/// about windows, because a terminal can do nothing about them -- so it is
/// started as one is, which is where it says where it is.
fn terminal_on(tree: &Path) -> (App, Receiver<Event>) {
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(tree.to_path_buf());
    let (sender, events) = obelus_app::event::channel();
    app.start(sender);
    support::lay_out(&mut app, 80, 24);
    (app, events)
}

/// A terminal lists the worktrees too, and says which tree it is on -- but
/// `ctrl+enter`, which is a window of its own, does nothing there.
///
/// Broken deliberately three ways: `another_worktree` asking for a front
/// end again (no tab), `say_where_this_window_is` claiming only where there
/// is a door (the window does not see the terminal on `feature`), and the
/// list saying it goes elsewhere whatever it is drawn on (the terminal's
/// foot offers a window).
#[test]
fn a_terminal_lists_the_worktrees_and_says_where_it_is() {
    let scratch = Scratch::new("worktrees-terminal");
    let (main, feature, _) = repository(&scratch);
    let (mut app, _events) = terminal_on(&feature);

    assert!(app.offers(Command::WorktreeList));
    press_function(&mut app, 2);
    assert_eq!(
        tabs(&app).0,
        ["Documents", "Worktrees"],
        "a terminal's list has no worktrees"
    );
    dispatch::dispatch(&mut app, Command::WorktreeList);
    let screen = support::render(&mut app, 80, 24);
    assert!(
        !screen.contains("New window"),
        "a terminal offers a window of its own:\n{screen}"
    );
    choose_elsewhere(&mut app, "main");
    assert_eq!(
        resolved(app.working_directory()),
        resolved(&feature),
        "ctrl+enter went somewhere from a terminal"
    );

    let asked = Arc::new(Asked::default());
    let (mut window, _window_events) = window_on(&main, &asked);
    dispatch::dispatch(&mut window, Command::WorktreeList);
    let row = rows(&window)
        .into_iter()
        .find(|row| row.0 == "feature")
        .expect("a row for the tree");
    assert!(row.3, "a tree a terminal is on is not marked");
    // And a window on it is still a window to open: a terminal has no
    // door to knock on.
    choose_elsewhere(&mut window, "feature");
    assert_eq!(asked.opened.lock().expect("the list").len(), 1);
    assert!(asked.brought.lock().expect("the list").is_empty());
}

/// Enter puts this Obelus on the tree, closing what was open in the one it
/// left and saying where it is now.
///
/// Broken deliberately twice: `go_to_worktree` going elsewhere as it used
/// to (nothing moves here), and `move_to_tree` letting go without settling
/// again (the window is on no tree, and nobody sees it on `feature`).
#[test]
fn enter_puts_this_obelus_on_the_tree() {
    let scratch = Scratch::new("worktrees-switch");
    let (main, feature, _) = repository(&scratch);
    let asked = Arc::new(Asked::default());
    let (mut app, _events) = window_on(&main, &asked);
    app.open_for_test(&main.join("file.rs"));

    dispatch::dispatch(&mut app, Command::WorktreeList);
    choose(&mut app, "feature");
    assert_eq!(
        resolved(app.working_directory()),
        resolved(&feature),
        "this window did not go to the tree"
    );
    assert!(app.picker().is_none(), "the list is still up");
    assert!(
        app.reading_nothing(),
        "what was open in the tree it left is still open"
    );
    assert!(asked.opened.lock().expect("the list").is_empty());
    assert!(asked.brought.lock().expect("the list").is_empty());

    let (mut other, _other_events) = terminal_on(&main);
    dispatch::dispatch(&mut other, Command::WorktreeList);
    let marked: Vec<String> = rows(&other)
        .into_iter()
        .filter(|row| row.3)
        .map(|row| row.0)
        .collect();
    assert_eq!(
        marked,
        ["main", "feature"],
        "the window that went is not said to be where it went"
    );
}

/// Something unwritten is asked about before going, the way leaving asks:
/// cancelling stays, and saving writes it and then goes.
///
/// Broken deliberately twice: going without asking (the file is never
/// written and the window has gone), and the saving answer forgetting the
/// tree it was asked about (it writes and stays).
#[test]
fn something_unwritten_is_asked_about_before_going() {
    let scratch = Scratch::new("worktrees-unsaved");
    let (main, feature, _) = repository(&scratch);
    let asked = Arc::new(Asked::default());
    let (mut app, _events) = window_on(&main, &asked);
    let file = main.join("file.rs");
    app.open_for_test(&file);
    support::type_text(&mut app, "// ");

    dispatch::dispatch(&mut app, Command::WorktreeList);
    choose(&mut app, "feature");
    assert_eq!(
        support::ways(&app),
        [
            "Save everything and switch",
            "Switch without saving",
            "cancel"
        ],
        "going with something unwritten did not ask"
    );
    support::answer(&mut app, "cancel");
    assert_eq!(resolved(app.working_directory()), resolved(&main));

    dispatch::dispatch(&mut app, Command::WorktreeList);
    choose(&mut app, "feature");
    support::answer(&mut app, "Save everything and switch");
    assert_eq!(
        std::fs::read_to_string(&file).expect("the file"),
        "// fn main() {}\n",
        "what was unwritten was not written"
    );
    assert_eq!(
        resolved(app.working_directory()),
        resolved(&feature),
        "saving did not go on to the tree"
    );
}

/// The worktrees are the second tab of the list of what is open, with the
/// tree this window is on said and stood on.
///
/// Broken deliberately by building the rows without the main checkout,
/// and by leaving out the word that says which tree is this one's: each
/// fails its own line below.
#[test]
fn the_worktrees_are_a_tab_of_what_is_open() {
    let scratch = Scratch::new("worktrees-tab");
    let (main, _, _) = repository(&scratch);
    let asked = Arc::new(Asked::default());
    let (mut app, _events) = window_on(&main, &asked);

    press_function(&mut app, 2);
    assert_eq!(
        tabs(&app),
        (vec!["Documents".to_string(), "Worktrees".to_string()], 0),
        "the list of what is open does not start on what is open"
    );

    // The command walks to the tab rather than opening the list again.
    dispatch::dispatch(&mut app, Command::WorktreeList);
    assert_eq!(
        tabs(&app).1,
        1,
        "switch-worktree did not land on the worktrees"
    );
    let said = rows(&app);
    let named: Vec<&str> = said.iter().map(|row| row.0.as_str()).collect();
    assert_eq!(named, ["main", "feature", "spare"], "{said:?}");
    assert_eq!(
        said[0],
        (
            "main".to_string(),
            Some("master".to_string()),
            Some("This window".to_string()),
            true,
            true
        ),
        "this window's own tree is not said"
    );
    assert_eq!(said[1].1.as_deref(), Some("feature"));
    let selected = app
        .picker()
        .and_then(obelus_component::picker::Picker::selected_item)
        .map(|item| item.label.clone());
    assert_eq!(
        selected.as_deref(),
        Some("main"),
        "the list did not open on this tree"
    );

    // And the key that names the first tab goes back to it.
    press_function(&mut app, 2);
    assert_eq!(
        tabs(&app).1,
        0,
        "f2 inside the worktrees did not go back to what is open"
    );
}

/// The worktrees tab shows no preview, and the documents tab beside it
/// still does.
///
/// A tree is a whole checkout and no one file of it, so the file being
/// read under its rows said nothing about any of them.
///
/// Broken deliberately by leaving the list previewing whichever tab it is
/// on: the list is cut in two by a rule with an empty pane under it. And
/// by previewing a tree as the file being read as well, which draws that
/// file there.
#[test]
fn the_worktrees_tab_previews_nothing() {
    let scratch = Scratch::new("worktrees-no-preview");
    let (main, _, _) = repository(&scratch);
    let asked = Arc::new(Asked::default());
    let (mut app, _events) = window_on(&main, &asked);
    app.open_for_test(&main.join("file.rs"));

    press_function(&mut app, 2);
    let documents = support::render(&mut app, 80, 24);
    assert!(
        documents.contains("fn main() {}"),
        "the documents tab does not preview what is open:\n{documents}"
    );

    press(&mut app, KeyCode::Tab);
    assert_eq!(tabs(&app).1, 1, "tab did not walk to the worktrees");
    let worktrees = support::render(&mut app, 80, 24);
    assert!(
        !worktrees.contains("fn main() {}"),
        "the worktrees tab previews the file being read:\n{worktrees}"
    );
    // A preview is under a rule of its own, so a list that previews has one
    // more row of rule across the screen than one that does not -- and in a
    // window the worktrees have a foot instead, with a rule of its own,
    // which is the one rule the tabs have the same number of.
    assert!(
        worktrees.contains("New window"),
        "the worktrees in a window do not say what ctrl+enter does:\n{worktrees}"
    );
    let ruled = |screen: &str| {
        screen
            .lines()
            .filter(|line| {
                line.split_once('|')
                    .is_some_and(|(_, row)| !row.is_empty() && row.chars().all(|c| c == '─'))
            })
            .count()
    };
    assert_eq!(
        ruled(&worktrees),
        ruled(&documents),
        "the worktrees tab is still cut in two for a preview:\n{worktrees}"
    );
}

/// A tree inside the main checkout is named the way the ones beside it
/// are: from the directory the main checkout sits in.
///
/// Broken deliberately by going back to naming only a tree beside the
/// main checkout by a word: the one under `.worktree` is called by its
/// whole path while `main` is a name.
#[test]
fn a_tree_inside_the_main_checkout_is_named_from_where_it_sits() {
    let scratch = Scratch::new("worktrees-inside");
    let (main, _, _) = repository(&scratch);
    let nested = main.join(".worktree").join("nested");
    git(
        &main,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "nested",
            nested.to_str().expect("a path"),
        ],
    );
    let asked = Arc::new(Asked::default());
    let (mut app, _events) = window_on(&main, &asked);

    dispatch::dispatch(&mut app, Command::WorktreeList);
    let said = rows(&app);
    // Sorted, because the order of the linked trees is git's to choose.
    let mut named: Vec<String> = said.iter().map(|row| row.0.clone()).collect();
    named.sort();
    let inside = Path::new("main").join(".worktree").join("nested");
    assert_eq!(
        named,
        ["feature", "main", &inside.display().to_string(), "spare"],
        "{said:?}"
    );
}

/// `ctrl+enter` on a tree no window is on opens a new one, and leaves this
/// window where it was; this window's own is where the reader already is.
///
/// Broken deliberately by opening a window for whatever row is chosen: the
/// tree this window is on gets a second one.
#[test]
fn a_tree_no_window_is_on_is_opened_in_one() {
    let scratch = Scratch::new("worktrees-open");
    let (main, feature, _) = repository(&scratch);
    let asked = Arc::new(Asked::default());
    let (mut app, _events) = window_on(&main, &asked);

    dispatch::dispatch(&mut app, Command::WorktreeList);
    choose_elsewhere(&mut app, "main");
    assert!(
        app.picker().is_none(),
        "choosing this tree did not close the list"
    );
    dispatch::dispatch(&mut app, Command::WorktreeList);
    choose_elsewhere(&mut app, "feature");
    assert!(
        app.picker().is_none(),
        "opening a window did not close the list"
    );
    assert_eq!(resolved(app.working_directory()), resolved(&main));

    // Resolved, because the path is the one git wrote down and git spells
    // a place its own way: through a mac's `/private`, and with forward
    // slashes and a long name on Windows.
    let opened: Vec<PathBuf> = asked
        .opened
        .lock()
        .expect("the list")
        .iter()
        .map(|path| resolved(path))
        .collect();
    assert_eq!(
        opened,
        [resolved(&feature)],
        "the wrong windows were opened"
    );
    assert!(asked.brought.lock().expect("the list").is_empty());
}

/// A tree another window is on is marked, and `ctrl+enter` on it brings
/// that window forward -- through a knock the other window hears.
///
/// Both halves in one test, because they are one format: what a window
/// writes about itself, what another reads, and what it says through the
/// door it read. Broken deliberately three ways: the other window's claim
/// left unread (no mark, and a second window opened), the key checked on
/// the way in left out of the knock (nothing comes forward), and the
/// compositor that cannot bring a window forward asked to anyway.
#[test]
fn a_tree_another_window_is_on_brings_that_window_forward() {
    let scratch = Scratch::new("worktrees-bring");
    let (main, feature, _) = repository(&scratch);
    let here = Arc::new(Asked::default());
    let there = Arc::new(Asked::default());
    let (mut app, _events) = window_on(&main, &here);
    let (mut other, other_events) = window_on(&feature, &there);

    dispatch::dispatch(&mut app, Command::WorktreeList);
    let feature_row = rows(&app)
        .into_iter()
        .find(|row| row.0 == "feature")
        .expect("a row for the tree");
    assert!(feature_row.3, "a tree another window is on is not marked");
    choose_elsewhere(&mut app, "feature");
    assert!(
        here.opened.lock().expect("the list").is_empty(),
        "a second window was opened"
    );
    let brought = here.brought.lock().expect("the list").clone();
    assert_eq!(brought.len(), 1, "nothing was brought forward");

    obelus_app::app::knock(&brought[0], Some("a token"));
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let Ok(event) = other_events.recv_timeout(Duration::from_millis(100)) else {
            continue;
        };
        if matches!(event, Event::Summoned(_)) {
            other.handle(event);
            break;
        }
    }
    assert_eq!(
        there.came.lock().expect("the list").clone(),
        [Some("a token".to_string())],
        "the other window was not told to come forward"
    );
    let _ = feature;

    // And where nothing can bring a window forward, a new one opens rather
    // than a key doing nothing at the far end.
    let unable = Arc::new(Asked {
        cannot_bring: true,
        ..Asked::default()
    });
    let (mut third, _third_events) = window_on(&main, &unable);
    dispatch::dispatch(&mut third, Command::WorktreeList);
    choose_elsewhere(&mut third, "feature");
    assert_eq!(unable.opened.lock().expect("the list").len(), 1);
    assert!(unable.brought.lock().expect("the list").is_empty());
}

/// A tree deleted behind git's back is listed, says so, and goes nowhere --
/// including where a window was on it: that window has stopped saying so.
///
/// Broken deliberately twice: a tree that has gone left enabled (a window
/// is opened on nothing), and a window whose tree went keeping its claim
/// (the row is marked as somewhere to go). And on Windows a third, which
/// only Windows can show: naming a row from `resolved` rather than
/// `resolved_as_far_as_it_goes` takes the tree that has gone as git wrote
/// it, which is not the `\\?\` spelling the main checkout resolves to, so
/// the row is called by its whole path and no row says `spare`.
#[test]
fn a_tree_that_has_gone_is_missing_and_goes_nowhere() {
    let scratch = Scratch::new("worktrees-gone");
    let (main, _, spare) = repository(&scratch);
    let here = Arc::new(Asked::default());
    let there = Arc::new(Asked::default());
    let (mut app, _events) = window_on(&main, &here);
    let (mut other, _other_events) = window_on(&spare, &there);

    std::fs::remove_dir_all(&spare).expect("deleting a worktree behind git's back");
    other.handle(Event::Watched(obelus_watch::Changed {
        path: spare.join("file.rs"),
    }));
    assert!(other.tree_has_gone());

    dispatch::dispatch(&mut app, Command::WorktreeList);
    let row = rows(&app)
        .into_iter()
        .find(|row| row.0 == "spare")
        .expect("git still lists it");
    assert_eq!(row.2.as_deref(), Some("Missing"), "{row:?}");
    assert!(!row.3, "a window on a tree that has gone is still offered");
    // Dim, which is the whole of the refusal: the selection walks past a
    // row that cannot be chosen, so there is no key to press on it.
    assert!(!row.4, "a tree that has gone can be chosen");
}

/// A window opening on a tree while the list is up marks that row then and
/// there, from the watcher.
///
/// Against a real watcher, because what is being tested is that the watch
/// was taken: an event handed over by hand proves the handler and nothing
/// else. Broken deliberately by never wanting the watch on the windows.
#[test]
fn a_window_opening_while_the_list_is_up_is_marked() {
    let scratch = Scratch::new("worktrees-watch");
    let (main, feature, _) = repository(&scratch);
    let here = Arc::new(Asked::default());
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(main);
    let (sender, events) = obelus_app::event::channel();
    app.windowed_by(Arc::clone(&here) as Arc<dyn Windows>);
    app.start(sender);
    support::lay_out(&mut app, 80, 24);

    dispatch::dispatch(&mut app, Command::WorktreeList);
    // A frame, which is where the watches are settled.
    let _ = support::render(&mut app, 80, 24);
    let marked = |app: &App| rows(app).into_iter().any(|row| row.0 == "feature" && row.3);
    assert!(!marked(&app), "a tree nobody is on is marked");

    let there = Arc::new(Asked::default());
    let (_other, _other_events) = window_on(&feature, &there);
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && !marked(&app) {
        if let Ok(event) = events.recv_timeout(Duration::from_millis(100)) {
            app.handle(event);
        }
    }
    assert!(marked(&app), "a window that opened on a tree was not heard");
}

/// A window that died without tidying up leaves a file nobody holds, and
/// the next look takes it away rather than reading it for ever.
///
/// The file is written here by hand, which is what a killed window leaves:
/// its words and no lock. Broken deliberately by leaving such a file where
/// it is: it is still there after the list has looked.
#[test]
fn a_window_that_died_is_tidied_away() {
    let scratch = Scratch::new("worktrees-died");
    let (main, feature, _) = repository(&scratch);
    let directory = obelus_logging::state_directory()
        .expect("a state directory")
        .join("windows")
        .join(obelus_git::project(&main).expect("a project"));
    std::fs::create_dir_all(&directory).expect("the directory");
    let left = directory.join("1-deadbeef-0");
    std::fs::write(
        &left,
        format!("{}\n127.0.0.1:9\nnobodys\n", feature.display()),
    )
    .expect("what a killed window leaves");

    let asked = Arc::new(Asked::default());
    let (mut app, _events) = window_on(&main, &asked);
    dispatch::dispatch(&mut app, Command::WorktreeList);
    let row = rows(&app)
        .into_iter()
        .find(|row| row.0 == "feature")
        .expect("a row for the tree");
    assert!(!row.3, "a window that died is marked as there");
    assert!(!left.exists(), "what a dead window left is still there");
}

/// Going away from a tree and back opens again what was open there, the
/// way leaving and starting again does.
///
/// Whether to reopen is said in each tree's own settings, so the answer
/// does not hang on the machine the suite runs on. Broken deliberately by
/// leaving out the writing down in `move_to_tree`: the record still names
/// nothing when the window comes back, and the file never arrives.
#[test]
fn going_back_opens_what_was_open() {
    let scratch = Scratch::new("worktrees-reopen");
    let (main, feature, _) = repository(&scratch);
    for tree in [&main, &feature] {
        std::fs::create_dir_all(tree.join(".obelus")).expect("making .obelus");
        std::fs::write(tree.join(".obelus/config.toml"), "reopen = true\n")
            .expect("writing the settings");
    }
    let asked = Arc::new(Asked::default());
    let (mut app, events) = window_on(&main, &asked);
    app.open_for_test(&main.join("file.rs"));

    dispatch::dispatch(&mut app, Command::WorktreeList);
    choose(&mut app, "feature");
    assert!(app.reading_nothing());
    dispatch::dispatch(&mut app, Command::WorktreeList);
    choose(&mut app, "main");

    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && app.reading_nothing() {
        if let Ok(event) = events.recv_timeout(Duration::from_millis(100)) {
            app.handle(event);
        }
    }
    let open: Vec<PathBuf> = app
        .buffers_for_test()
        .into_iter()
        .map(|(path, _)| resolved(&path))
        .collect();
    assert_eq!(
        open,
        [resolved(&main.join("file.rs"))],
        "what was open in the tree was not opened again"
    );
}

/// Several Obelus on one tree are a row each under it, saying what each is
/// reading, and `ctrl+enter` on one brings that one forward and no other.
///
/// Three on `feature` -- a terminal, a window reading `file.rs` and one
/// reading `other.rs` -- and two on `main`, this one among them.
/// Broken deliberately three ways: never saying again what a window is
/// reading after it claimed its tree (no row says `file.rs`), bringing
/// forward whichever window is on the tree rather than the row's (the
/// wrong one comes, on one of the two presses), and leaving the reader on
/// the tree's row rather than on their own (the list opens on `main`).
#[test]
fn several_on_one_tree_are_a_row_each() {
    let scratch = Scratch::new("worktrees-several");
    let (main, feature, _) = repository(&scratch);
    let asked = Arc::new(Asked::default());
    let (mut app, _events) = window_on(&main, &asked);
    let (_beside, _beside_events) = terminal_on(&main);
    let (_terminal, _terminal_events) = terminal_on(&feature);
    let (mut reading, reading_events) = window_on(&feature, &Arc::new(Asked::default()));
    reading.open_for_test(&feature.join("file.rs"));
    // A frame, which is where a window says what it is reading.
    support::lay_out(&mut reading, 80, 24);
    std::fs::write(feature.join("other.rs"), "fn other() {}\n").expect("a second file");
    let (mut other, other_events) = window_on(&feature, &Arc::new(Asked::default()));
    other.open_for_test(&feature.join("other.rs"));
    support::lay_out(&mut other, 80, 24);

    dispatch::dispatch(&mut app, Command::WorktreeList);
    let said = rows(&app);
    let named: Vec<&str> = said.iter().map(|row| row.0.as_str()).collect();
    let under = |tree: &str| -> Vec<String> {
        let from = named
            .iter()
            .position(|name| *name == tree)
            .expect("the tree");
        let mut under: Vec<String> = said[from + 1..]
            .iter()
            .take_while(|row| row.1.is_none())
            .map(|row| format!("{} {}", row.0, row.2.as_deref().unwrap_or("-")))
            .collect();
        under.sort();
        under
    };
    assert_eq!(
        under("feature"),
        ["Nothing open -", "file.rs -", "other.rs -"],
        "{said:?}"
    );
    assert_eq!(
        under("main"),
        ["Nothing open -", "Nothing open This window"],
        "{said:?}"
    );
    assert_eq!(under("spare"), Vec::<String>::new(), "{said:?}");
    let selected = app
        .picker()
        .and_then(obelus_component::picker::Picker::selected_item)
        .map(|item| (item.label.clone(), item.trailing.clone()));
    assert_eq!(
        selected,
        Some(("Nothing open".to_string(), Some("This window".to_string()))),
        "the list did not open on this window"
    );

    // Each brought forward by its own row: the two windows on `feature`
    // are told apart by what they are reading, and by nothing else. Both
    // pressed, because which claim a directory lists first is the
    // directory's to say, and a key that took whichever came first would
    // be right about one of them.
    let heard = |events: &Receiver<Event>| {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(Event::Summoned(_)) = events.recv_timeout(Duration::from_millis(100)) {
                return true;
            }
        }
        false
    };
    for (row, comes, stays) in [
        ("file.rs", &reading_events, &other_events),
        ("other.rs", &other_events, &reading_events),
    ] {
        dispatch::dispatch(&mut app, Command::WorktreeList);
        choose_elsewhere(&mut app, row);
        let brought = asked.brought.lock().expect("the list").pop();
        let brought = brought.expect("nothing was brought forward");
        obelus_app::app::knock(&brought, None);
        assert!(heard(comes), "the window reading {row} did not come");
        assert!(
            !matches!(stays.try_recv(), Ok(Event::Summoned(_))),
            "a window the row was not came forward"
        );
    }
}
