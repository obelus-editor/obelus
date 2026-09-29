//! What a repository may not talk Obelus into doing.
//!
//! A binary of its own because it sets `GIT_CONFIG_GLOBAL` for the whole
//! process, which is what it takes to put a `safe.directory` in front of
//! git without touching the config of whoever is running the tests. A test
//! beside this one would be reading a global config it never asked for.
//!
//! Its own git setup for the same reason: `git.rs` has a `Repository` that
//! ninety-eight tests share, and moving it here to be shared twice would
//! be a change to all of them for the sake of one.

/// Runs git in a directory, with an identity of its own.
fn git(directory: &std::path::Path, arguments: &[&str]) {
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

/// A repository does not get to run a program because Obelus looked at it,
/// and `safe.directory` does not change that.
///
/// The reduction to `Trust::Reduced` reads as the whole of the answer and
/// is not: gix takes the level it is given and then puts it back to full
/// wherever `safe.directory` covers the path. Plenty of readers have that
/// as `*`, and GitHub's own runner image does -- which is how this was
/// found, by the suite going red on one machine and green on the one it
/// was written on. So the refusal cannot rest on the level, and does not:
/// see `nothing_the_repository_named`.
///
/// Deliberate break: drop the `filter_config_section` and this fails while
/// the one beside it in `git.rs` goes on passing, on any machine whose own
/// config says nothing about `safe.directory`. Which is the whole reason
/// this one is written down separately, and why it names the condition
/// itself instead of hoping the machine has it.
#[test]
fn safe_directory_does_not_let_a_repository_run_a_program() {
    let directory = std::env::temp_dir().join(format!("obelus-git-trust-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("a directory");

    let global = directory.join("gitconfig");
    std::fs::write(&global, "[safe]\n\tdirectory = *\n").expect("a global config");
    // Process-wide, which is why this binary holds one test. Set before
    // the repository is built, because git reads it as it goes.
    //
    // SAFETY: nothing else in this binary runs beside it.
    unsafe {
        std::env::set_var("GIT_CONFIG_GLOBAL", &global);
    }

    let repository = directory.join("clone");
    std::fs::create_dir_all(&repository).expect("the repository");
    git(&repository, &["init", "--quiet", "--initial-branch=master"]);
    // What this writes is what a checkout would write -- the same reason
    // `git.rs` turns it off in the repositories it builds. Git for Windows
    // installs `core.autocrlf=true` system-wide, and the diff base is the
    // blob a checkout would put on disk, so `hello\n` came back `hello\r\n`
    // and the line that says the content survived the refusal went red
    // without the refusal having anything to do with it.
    git(&repository, &["config", "core.autocrlf", "false"]);
    std::fs::write(repository.join("file.rs"), "hello\n").expect("the file");
    std::fs::write(repository.join(".gitattributes"), "* filter=evil\n").expect("the attributes");
    git(&repository, &["add", "--all"]);
    git(&repository, &["commit", "--quiet", "-m", "with a driver"]);

    let marker = repository.join("THE-DRIVER-RAN");
    git(
        &repository,
        &[
            "config",
            "filter.evil.smudge",
            &format!("sh -c 'touch {} ; cat'", marker.display()),
        ],
    );

    let base = obelus_git::head_text(&repository.join("file.rs")).expect("a diff base");
    assert_eq!(base, "hello\n", "the content did not survive the refusal");
    assert!(
        !marker.exists(),
        "a repository named `safe.directory` at Obelus and got its program run"
    );

    let _ = std::fs::remove_dir_all(&directory);
}
