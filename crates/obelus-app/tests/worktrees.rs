//! Going to another of the repository's worktrees, from the list of what
//! is open.
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
use support::{Scratch, press, press_function};

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

/// Walks the selection to the row that says `label`, and chooses it.
fn choose(app: &mut App, label: &str) {
    press(app, KeyCode::Home);
    for _ in 0..rows(app).len() {
        let selected = app
            .picker()
            .and_then(obelus_component::picker::Picker::selected_item)
            .map(|item| item.label.clone());
        if selected.as_deref() == Some(label) {
            press(app, KeyCode::Enter);
            return;
        }
        press(app, KeyCode::Down);
    }
    panic!("no row says {label}: {:?}", rows(app));
}

/// A terminal has no worktrees to offer: it cannot start a window or bring
/// one forward.
///
/// Broken deliberately by letting `another_worktree` answer without a
/// front end: the list grows a tab a terminal can do nothing with.
#[test]
fn a_terminal_has_no_worktrees_tab() {
    let scratch = Scratch::new("worktrees-terminal");
    let (main, _, _) = repository(&scratch);
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(main);
    support::lay_out(&mut app, 80, 24);

    assert!(!app.offers(Command::WorktreeList));
    press_function(&mut app, 2);
    assert!(
        tabs(&app).0.is_empty(),
        "a terminal's list has tabs: {:?}",
        tabs(&app)
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
    // more row of rule across the screen than one that does not.
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
        ruled(&worktrees) + 1,
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

/// A tree no window is on is opened in a new one; this window's own is
/// where the reader already is.
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
    choose(&mut app, "main");
    assert!(
        app.picker().is_none(),
        "choosing this tree did not close the list"
    );
    dispatch::dispatch(&mut app, Command::WorktreeList);
    choose(&mut app, "feature");

    // Resolved, because the path is the one git wrote down and git spells
    // a place its own way: through a mac's `/private`, and with forward
    // slashes and a long name on Windows.
    let resolved = |path: &Path| path.canonicalize().expect("a tree that is there");
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

/// A tree another window is on is marked, and choosing it brings that
/// window forward -- through a knock the other window hears.
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
    choose(&mut app, "feature");
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
    choose(&mut third, "feature");
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
