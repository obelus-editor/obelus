//! What a start with nothing on the command line settles on.
//!
//! A binary of its own, because the answer is the process's own directory
//! and asking it means moving that: every other test in a binary shared
//! with this one would be moved too.

mod support;

use obelus_app::startup;

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
    let scratch = support::Scratch::new("starting-bare");
    let git = |arguments: &[&str]| {
        let outcome = std::process::Command::new("git")
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
