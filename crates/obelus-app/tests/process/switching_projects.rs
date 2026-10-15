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
