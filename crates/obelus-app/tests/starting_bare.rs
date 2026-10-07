//! What a start with nothing on the command line settles on.
//!
//! A binary of its own, because the answer is the process's own directory
//! and asking it means moving that: every other test in a binary shared
//! with this one would be moved too.

mod support;

use obelus_app::startup;
use obelus_ui::Screen as _;

/// The tests here take turns.
///
/// Each of them moves the process's own directory, which is the one thing
/// a process has only one of: run at once they answer each other's
/// questions.
static TURNS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A bare `ob` knows which branch it is on from the first frame.
///
/// With no arguments there is no root to be told about, and the branch is
/// read where Obelus is told which directory it is working in -- so it was
/// not read at all, and the row drew no branch until the watcher heard git
/// write its index. Which a reader saw as a branch there some of the time.
///
/// A branch name no checkout would have, so a start that asked this
/// repository instead of the one it was put in could not pass by agreeing
/// with it.
///
/// Broken deliberately by putting back `if let Some(root) = opening.root`
/// around `app.work_in(root)`: `head` comes back `None`.
#[test]
fn a_bare_start_knows_which_branch_it_is_on() {
    let _turn = TURNS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = support::Scratch::new("starting-bare");
    let git = |arguments: &[&str]| {
        let outcome = support::git()
            .arg("-C")
            .arg(scratch.path())
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
    };
    git(&["init", "--quiet", "--initial-branch=zephyr"]);
    std::fs::write(scratch.path().join("one.rs"), "fn main() {}\n").expect("writing it");
    git(&["add", "one.rs"]);
    git(&["commit", "--quiet", "-m", "committed"]);

    std::env::set_current_dir(scratch.path()).expect("going there");
    let app = startup::start(&[], "abc1234").expect("starting");

    assert_eq!(
        app.head(),
        Some(&obelus_git::Head::Branch("zephyr".to_string())),
        "a start with no arguments did not read which branch it is on"
    );
}

/// A bare start where git has never heard of the directory asks which
/// project.
///
/// Which is what a desktop launcher does: no argument, and a process
/// begun in the home directory. Obelus took that for the project, so the
/// file list walked the whole of it and everything keyed on a project was
/// filed under a place nobody works in.
///
/// The question is "does Obelus know which project" and not "were there
/// arguments" -- the test above is the other half of that pair, and it
/// has no arguments either and is *not* asked, because it stands in a
/// worktree.
///
/// Broken deliberately by asking `opening.root.is_none()` instead of
/// `obelus_git::worktree(&here)`: the test above starts asking, and a
/// reader who typed `cd project && ob` is interrupted for an answer they
/// already gave.
#[test]
fn a_bare_start_outside_a_repository_asks_which_project() {
    let _turn = TURNS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = support::Scratch::new("starting-nowhere");

    std::env::set_current_dir(scratch.path()).expect("going there");
    let app = startup::start(&[], "abc1234").expect("starting");

    assert!(
        app.choosing().is_some(),
        "a start with nothing to go on took the directory it began in for a project"
    );
    assert!(
        !app.offers(obelus_command::Command::FileOpen),
        "the file list is offered over a project nobody named"
    );
}

/// With nobody at the screen, a start with nothing to go on is refused
/// rather than put on the page asking which project, where it would wait
/// for an answer nobody gives.
///
/// Broken deliberately by taking the `None if headless` arm out of
/// `startup::build`: the start asked, and stopped on something else.
#[test]
fn a_headless_start_with_nothing_to_go_on_is_refused() {
    let _turn = TURNS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let scratch = support::Scratch::new("starting-headless-nowhere");

    std::env::set_current_dir(scratch.path()).expect("going there");
    let Err(error) = startup::start_headless(&[], "abc1234") else {
        panic!("a headless start with no project started");
    };
    assert!(
        error.to_string().contains("needs a project"),
        "it stopped on {error}"
    );
}
