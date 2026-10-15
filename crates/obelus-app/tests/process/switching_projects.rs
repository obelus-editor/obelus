//! Going to a project opened before, from a list over the one the window is
//! on -- and the runs of days that list and the page asking which project
//! are both headed by.
//!
//! Here because the list is the one file the page that asks keeps, which
//! these tests write and read: see `choosing_a_project`.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use crossterm::event::KeyCode;
use obelus_app::app::{App, Door, Windows, dispatch};
use obelus_command::Command;
use obelus_component::chooser::Known;
use obelus_ui::Screen as _;
use support::{Scratch, press};

use crate::support;

/// A front end that writes down which trees it was asked to open.
#[derive(Debug, Default)]
struct Asked {
    opened: Mutex<Vec<PathBuf>>,
}

impl Windows for Asked {
    fn open(&self, tree: &Path) {
        self.opened
            .lock()
            .expect("the list")
            .push(tree.to_path_buf());
    }

    fn bring(&self, _door: &Door) {}

    fn come_forward(&self, _token: Option<String>) {}

    fn can_bring(&self) -> bool {
        true
    }
}

/// Seconds since the epoch, now.
fn now() -> i64 {
    jiff::Timestamp::now().as_second()
}

/// Writes the list of projects the way Obelus writes it, newest first.
fn keep(projects: &[(&Path, i64)]) {
    let directory = obelus_logging::state_directory().expect("somewhere to keep state");
    std::fs::create_dir_all(&directory).expect("the state directory");
    let mut text = String::new();
    for (path, last) in projects {
        text.push_str("[[opened]]\n");
        text.push_str(&format!(
            "path = {}\nlast = {last}\n\n",
            toml::Value::String(path.to_string_lossy().into_owned())
        ));
    }
    std::fs::write(directory.join("projects.toml"), text).expect("the list");
}

/// The paths on the list, as the file says, newest first.
fn kept() -> Vec<PathBuf> {
    let list = obelus_logging::state_directory()
        .expect("somewhere to keep state")
        .join("projects.toml");
    let text = std::fs::read_to_string(list).expect("the list");
    text.parse::<toml::Table>()
        .expect("the list reads")
        .get("opened")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|row| row.get("path")?.as_str())
        .map(|path| std::fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path)))
        .collect()
}

/// A repository of its own in a scratch directory, which is a project.
fn project(scratch: &Scratch, name: &str) -> PathBuf {
    let path = scratch.join(name);
    std::fs::create_dir_all(&path).expect("the project");
    gix::init(&path).expect("a repository");
    path
}

/// A window on `tree`, in a terminal.
fn on(tree: &Path) -> App {
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(tree.to_path_buf());
    support::lay_out(&mut app, 80, 30);
    app
}

/// What the list showing says, row by row: its name and the run it is in.
fn rows(app: &App) -> Vec<(String, Option<String>)> {
    app.picker()
        .expect("a list is showing")
        .matches()
        .map(|item| (item.label.clone(), item.section.clone()))
        .collect()
}

/// The list is every project opened before but the one the window is on,
/// newest first, each in the run of days it was last open in -- and the
/// runs are headed on screen.
///
/// Broken deliberately by taking the filter on `here` out of
/// `open_recent_projects`: the project the window is on is the first row.
/// And by giving every row `section: None`: neither heading is drawn.
#[test]
fn the_list_is_every_other_project_in_runs_of_days() {
    let _turn = crate::turn();
    let scratch = Scratch::new("switching-lists");
    let (here, recent, old) = (
        project(&scratch, "here"),
        project(&scratch, "recent"),
        project(&scratch, "old"),
    );
    keep(&[(&here, now()), (&recent, now()), (&old, 2_000)]);
    let mut app = on(&here);

    dispatch::dispatch(&mut app, Command::ProjectList);

    assert_eq!(
        rows(&app),
        [
            ("recent".to_string(), Some("Today".to_string())),
            ("old".to_string(), Some("Earlier".to_string())),
        ],
        "not the other projects, newest first, in their runs"
    );
    let dump = support::render(&mut app, 80, 30);
    let text = support::text_block(&dump);
    let at = |needle: &str| {
        text.lines()
            .position(|row| row.contains(needle))
            .unwrap_or_else(|| panic!("{needle:?} is not on screen:\n{dump}"))
    };
    assert!(at("Today") < at("recent"), "{dump}");
    assert!(at("recent") < at("Earlier"), "{dump}");
    assert!(at("Earlier") < at("old"), "{dump}");
}

/// A window started inside a project is on the project, and its row is
/// left out too: `ob` typed in `crates` is on the tree whose top the list
/// remembers.
///
/// Broken deliberately by comparing the rows against the working directory
/// rather than `this_tree` in `open_recent_projects`: the project the
/// window is on is offered to it.
#[test]
fn a_window_started_inside_its_project_is_not_offered_it() {
    let _turn = crate::turn();
    let scratch = Scratch::new("switching-inside");
    let (here, other) = (project(&scratch, "here"), project(&scratch, "other"));
    let inside = here.join("crates");
    std::fs::create_dir_all(&inside).expect("a directory inside");
    keep(&[(&here, now()), (&other, now())]);
    let mut app = on(&inside);

    dispatch::dispatch(&mut app, Command::ProjectList);

    assert_eq!(
        rows(&app),
        [("other".to_string(), Some("Today".to_string()))],
        "the project the window is on was offered to it"
    );
}

/// In a terminal, enter puts this window on the project -- and choosing it
/// puts it at the top of the list, as choosing one on the page does.
///
/// Broken deliberately by taking the `remember` out of `go_to_project`:
/// the window goes, and the list still has the other project first.
#[test]
fn enter_in_a_terminal_goes_there_and_remembers_it() {
    let _turn = crate::turn();
    let scratch = Scratch::new("switching-goes");
    let (here, newer, chosen) = (
        project(&scratch, "here"),
        project(&scratch, "newer"),
        project(&scratch, "chosen"),
    );
    keep(&[(&here, now()), (&newer, now()), (&chosen, 2_000)]);
    let mut app = on(&here);

    dispatch::dispatch(&mut app, Command::ProjectList);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);

    assert!(app.picker().is_none(), "the list is still up");
    assert_eq!(
        app.working_directory(),
        chosen,
        "the window is not on the project chosen"
    );
    assert_eq!(
        kept().first(),
        Some(&std::fs::canonicalize(&chosen).expect("the project is there")),
        "choosing it did not put it at the top of the list"
    );
}

/// Going asks first about a program still running in a terminal, which
/// would go with the project -- the question closing the project asks.
///
/// A shell that has only just started is running, which is all this needs:
/// what it has printed is `terminal.rs`'s, which is Unix's alone.
///
/// Broken deliberately by leaving the count of running terminals out of
/// `App::go_to_tree`: the window goes at the first enter, and the shell
/// with it.
#[test]
fn going_asks_about_a_running_terminal_first() {
    let _turn = crate::turn();
    let scratch = Scratch::new("switching-running");
    let (here, there) = (project(&scratch, "here"), project(&scratch, "there"));
    keep(&[(&here, now()), (&there, now())]);
    let mut app = on(&here);
    let _events = support::drive(&mut app);
    app.shell_for_test(PathBuf::from(match cfg!(windows) {
        true => "cmd.exe",
        false => "/bin/sh",
    }));
    dispatch::dispatch(&mut app, Command::TerminalOpen);
    assert!(app.terminal().is_some(), "no terminal opened");

    dispatch::dispatch(&mut app, Command::ProjectList);
    press(&mut app, KeyCode::Enter);

    assert_eq!(app.working_directory(), here, "went without asking");
    assert_eq!(
        support::ways(&app).first().map(String::as_str),
        Some("Stop it and switch"),
        "the question was not about the terminal"
    );
    support::answer(&mut app, "Stop it and switch");
    assert_eq!(
        app.working_directory(),
        there,
        "the answer did not go there"
    );
}

/// In a window, enter opens another window on the project and leaves this
/// one where it is; `ctrl+enter` is this one going.
///
/// Broken deliberately by making every row `WorktreeEnter::Switch` in
/// `open_recent_projects`: enter takes this window, and no other opens.
#[test]
fn in_a_window_enter_is_another_window_and_control_enter_is_this_one() {
    let _turn = crate::turn();
    let scratch = Scratch::new("switching-windows");
    let (here, there) = (project(&scratch, "here"), project(&scratch, "there"));
    keep(&[(&here, now()), (&there, now())]);
    let asked = Arc::new(Asked::default());
    let mut app = on(&here);
    app.windowed_by(Arc::clone(&asked) as Arc<dyn Windows>);

    dispatch::dispatch(&mut app, Command::ProjectList);
    press(&mut app, KeyCode::Enter);

    assert_eq!(
        *asked.opened.lock().expect("the list"),
        std::slice::from_ref(&there),
        "enter did not open a window on it"
    );
    assert_eq!(app.working_directory(), here, "this window went as well");

    dispatch::dispatch(&mut app, Command::ProjectList);
    support::press_control_key(&mut app, KeyCode::Enter);

    assert_eq!(
        app.working_directory(),
        there,
        "control and enter did not put this window there"
    );
    assert_eq!(
        asked.opened.lock().expect("the list").len(),
        1,
        "control and enter opened another window"
    );
}

/// The page asking which project heads its runs of days the way the list
/// does, and a press lands on the project it is drawn on -- the headings
/// are rows that are no project's.
///
/// Broken deliberately by putting back `choosing.top + (y - list.y)` in
/// `projects::row_at`: the press on the oldest project stands on nothing,
/// two headings down from where the count thinks it is.
#[test]
fn the_page_heads_its_runs_of_days_and_a_press_lands_under_them() {
    let _turn = crate::turn();
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(PathBuf::from("/tmp/obelus"));
    let known = |path: &str, last: i64| Known {
        path: PathBuf::from(path),
        shown: path.to_string(),
        last: Some(last),
    };
    app.ask_about_these_projects_for_test(vec![
        known("/tmp/obelus/today", now()),
        known("/tmp/obelus/yesterday", now() - 86_400),
        known("/tmp/obelus/long-ago", 2_000),
    ]);

    let dump = support::render(&mut app, 60, 20);
    let text = support::text_block(&dump);
    let row = |needle: &str| -> u16 {
        text.lines()
            .find(|row| row.contains(needle))
            .and_then(|row| row.split_once('|'))
            .and_then(|(at, _)| at.trim().parse().ok())
            .unwrap_or_else(|| panic!("{needle:?} is not on screen:\n{dump}"))
    };
    // Yesterday's project is a day ago by the clock and may be today's by
    // the calendar a second after midnight, so only the two that cannot
    // move are asked about.
    assert!(row("Today") < row(" today "), "{dump}");
    assert!(row("Earlier") < row("long-ago"), "{dump}");
    assert!(row(" today ") < row("Earlier"), "{dump}");

    app.handle(obelus_app::event::Event::Pointer {
        kind: obelus_app::event::Pointer::Pressed,
        x: 6,
        y: row("long-ago"),
    });
    assert_eq!(
        app.choosing().expect("still asking").at,
        2,
        "the press did not stand on the project it was drawn on"
    );

    // And the arrows walk the projects, never a heading.
    press(&mut app, KeyCode::Up);
    assert_eq!(app.choosing().expect("still asking").at, 1);
}

/// A page asking about these projects, `today` of them opened just now and
/// `earlier` long ago, drawn at 60 by `height`.
fn page_of(today: usize, earlier: usize, height: u16) -> (App, String) {
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(PathBuf::from("/tmp/obelus"));
    let known = |name: String, last: i64| Known {
        path: PathBuf::from(format!("/tmp/obelus/{name}")),
        shown: format!("/tmp/obelus/{name}"),
        last: Some(last),
    };
    app.ask_about_these_projects_for_test(
        (1..=today)
            .map(|at| known(format!("new-{at}"), now()))
            .chain((1..=earlier).map(|at| known(format!("old-{at}"), 2_000)))
            .collect(),
    );
    let dump = support::render(&mut app, 60, height);
    (app, dump)
}

/// Which row of the screen these words are on, if they are on it.
fn row_of(dump: &str, words: &str) -> Option<u16> {
    support::text_block(dump)
        .lines()
        .find(|row| row.contains(words))
        .and_then(|row| row.split_once('|'))
        .and_then(|(at, _)| at.trim().parse().ok())
}

/// On a page too short for every project, the window over them is settled
/// in rows: the last project, under two headings, is on screen once the
/// reader is on it.
///
/// Broken deliberately by settling the chooser's window by count
/// (`self.window.settle(room)`) in `Chooser::settle`: the window leaves
/// the last project's headings out of its sums, and the project is below
/// the foot of the list.
#[test]
fn a_page_too_short_for_its_projects_scrolls_to_the_last_of_them() {
    let _turn = crate::turn();
    let (mut app, _) = page_of(6, 6, 16);
    // From the first project, up is the row that opens another, and up
    // again the last project.
    press(&mut app, KeyCode::Up);
    press(&mut app, KeyCode::Up);
    assert_eq!(app.choosing().expect("still asking").at, 11);

    let dump = support::render(&mut app, 60, 16);
    assert!(
        row_of(&dump, "old-6").is_some(),
        "the project the reader is on is off the page:\n{dump}"
    );
}

/// A press on the rows a project did not fit in stands on nothing: what
/// was not drawn is not there to stand on.
///
/// The page is made so that the run of today's projects fills the list
/// but one row, and the first of the earlier ones -- a blank, its heading,
/// a blank and itself -- does not fit in it.
///
/// Broken deliberately by taking the `bottom > laid.list.bottom()` guard
/// out of `projects::row_at`: the press stands on the first earlier
/// project, which is nowhere on screen.
#[test]
fn a_press_where_a_project_did_not_fit_stands_on_nothing() {
    let _turn = crate::turn();
    // How tall the list is on this page, from where the row that opens
    // another lands under a list longer than it.
    let (_, dump) = page_of(20, 0, 16);
    let opening = row_of(&dump, "Open another").expect("the opening row");
    let room = usize::from(opening - 1 - 2);
    // A heading and a blank over the first, and the rest a row each.
    let (mut app, dump) = page_of(room - 3, 3, 16);
    assert!(row_of(&dump, "old-1").is_none(), "{dump}");
    let last = row_of(&dump, &format!("new-{}", room - 3)).expect("the last of today's");
    assert!(last + 1 < opening - 1, "no row is left over:\n{dump}");

    app.handle(obelus_app::event::Event::Pointer {
        kind: obelus_app::event::Pointer::Pressed,
        x: 6,
        y: last + 1,
    });
    assert_eq!(
        app.choosing().expect("still asking").at,
        0,
        "the press stood on a project that is not drawn"
    );
}
