//! What a path on the command line means.
//!
//! Three questions in one answer: which project Obelus works in, which files
//! it opens, and whether the question left over is "which file". A file
//! names the *tree* it is in -- the repository, asked of git, and only the
//! directory it sits in where git has never heard of it; a directory *is*
//! the project, because naming one is a reader saying where to work. The
//! list is what answers the rest.

mod support;

use std::path::PathBuf;

use obelus_app::{app, app::App};
use obelus_buffer::Buffer;

/// A file with no repository over it names the directory it is in.
///
/// The scratch directory is under the system's temporary one, which is
/// nobody's repository -- so this is the fallback, and it is the whole of
/// what the fallback is for: a scratch note, something downloaded, a file
/// in `/tmp`.
///
/// Deliberate break: make the fallback `absolute(first)` rather than its
/// parent and the project is the file itself, which is not a directory
/// and which every path shown is then stripped against.
#[test]
fn a_file_with_no_repository_says_which_directory_it_is_in() {
    let scratch = support::Scratch::new("opening-file");
    let inner = scratch.path().join("src");
    std::fs::create_dir_all(&inner).expect("making it");
    let path = inner.join("one.rs");
    std::fs::write(&path, "fn main() {}\n").expect("writing it");

    let opening = app::opening(std::slice::from_ref(&path));
    assert_eq!(
        opening.root.as_deref(),
        Some(inner.as_path()),
        "the project is not the directory the file is in"
    );
    assert_eq!(opening.files, vec![path], "the file was not opened");
    assert!(!opening.list, "a named file left the question open");
}

/// Makes a repository where it is told to, with real git: what counts as
/// one is git's answer to that question, and a hand-made `.git` would only
/// ever agree with whatever this file assumed about it.
fn repository_at(path: &std::path::Path) {
    let outcome = support::git()
        .arg("-C")
        .arg(path)
        .args(["init", "--quiet", "--initial-branch=master"])
        .output()
        .expect("running git");
    assert!(outcome.status.success(), "git init failed");
}

/// A file names the repository it is in, not the directory it sits in.
///
/// Which is what a reader means by the project: everything Obelus opens
/// from here -- the file list, the search, what the project's own settings
/// are read from -- is about the tree, and a project of one directory is a
/// file list of one file.
///
/// Deliberate break: take `obelus_git::worktree` back out of `opening` and
/// the project comes back as `src`, which is the first assertion.
#[test]
fn a_file_names_the_repository_it_is_in() {
    let scratch = support::Scratch::new("opening-repository");
    repository_at(scratch.path());
    let inner = scratch.path().join("src");
    std::fs::create_dir_all(&inner).expect("making it");
    let path = inner.join("one.rs");
    std::fs::write(&path, "fn main() {}\n").expect("writing it");

    let opening = app::opening(std::slice::from_ref(&path));
    assert_eq!(
        opening.root.as_deref(),
        Some(scratch.path()),
        "the project is not the repository the file is in"
    );
    assert_eq!(opening.files, vec![path], "the file was not opened");
    assert!(!opening.list, "a named file left the question open");
}

/// And a file in a linked worktree names *that* worktree.
///
/// The other half of the same rule, and the one a shortcut gets wrong: the
/// name Obelus keeps a project under is deliberately the main checkout, so
/// that worktrees share one set of notes -- but the files on the screen are
/// the ones in front of the reader, and in a worktree those are its own.
///
/// Deliberate break: answer `worktree` from `main_checkout` instead. This
/// fails wherever the suite is run, because the two are different
/// directories here by construction. `the_project_is_absolute` goes with
/// it when the suite is run inside a linked worktree -- which is a fact
/// about where it was run and not something it asserts.
#[test]
fn a_file_in_a_worktree_names_that_worktree() {
    let scratch = support::Scratch::new("opening-worktree");
    repository_at(scratch.path());
    let file = scratch.path().join("one.rs");
    std::fs::write(&file, "fn main() {}\n").expect("writing it");
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
        assert!(outcome.status.success(), "git {arguments:?} failed");
    };
    // A worktree needs a commit to branch from.
    git(&["add", "one.rs"]);
    git(&["commit", "--quiet", "-m", "committed"]);
    let beside = scratch.path().with_file_name(format!(
        "obelus-opening-worktree-linked-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&beside);
    git(&[
        "worktree",
        "add",
        "--quiet",
        "-b",
        "elsewhere",
        beside.to_str().expect("a path"),
    ]);

    let opening = app::opening(&[beside.join("one.rs")]);
    assert_eq!(
        opening.root.as_deref(),
        Some(beside.as_path()),
        "a file in a worktree named somebody else's checkout"
    );
    let _ = std::fs::remove_dir_all(&beside);
}

/// A directory is the project, and nothing in it is opened: which file is
/// the question, and the list is the answer.
#[test]
fn a_directory_is_the_tree_and_leaves_the_list() {
    let scratch = support::Scratch::new("opening-tree");
    let path = scratch.path().to_path_buf();
    std::fs::write(path.join("one.rs"), "fn main() {}\n").expect("writing it");

    let opening = app::opening(std::slice::from_ref(&path));
    assert_eq!(opening.root.as_deref(), Some(path.as_path()));
    assert!(
        opening.files.is_empty(),
        "a directory was opened as a document"
    );
    assert!(
        opening.list,
        "the question of which file was left unanswered"
    );
}

/// Nothing on the command line changes nothing: the directory Obelus was
/// started in is the shell's answer to the same question.
#[test]
fn nothing_named_leaves_the_directory_alone() {
    let opening = app::opening(&[]);
    assert_eq!(opening.root, None);
    assert!(opening.files.is_empty());
    assert!(
        !opening.list,
        "a list opened over a reader who asked for none"
    );
}

/// The first path decides the project, and every file named is opened.
#[test]
fn the_first_path_decides_and_the_rest_are_opened() {
    let scratch = support::Scratch::new("opening-several");
    let inner = scratch.path().join("src");
    std::fs::create_dir_all(&inner).expect("making it");
    let one = inner.join("one.rs");
    let two = scratch.path().join("two.rs");
    for path in [&one, &two] {
        std::fs::write(path, "fn main() {}\n").expect("writing it");
    }

    let opening = app::opening(&[one.clone(), two.clone()]);
    assert_eq!(
        opening.root.as_deref(),
        Some(inner.as_path()),
        "the project is not the first path's"
    );
    assert_eq!(opening.files, vec![one, two.clone()]);
    assert!(!opening.list);

    // A project named first, with a file after it: the project is the project,
    // and the file is still opened, so there is nothing left for a list.
    let opening = app::opening(&[scratch.path().to_path_buf(), two.clone()]);
    assert_eq!(opening.root.as_deref(), Some(scratch.path()));
    assert_eq!(opening.files, vec![two]);
    assert!(!opening.list, "a list opened over a file that was named");
}

/// A relative path becomes absolute, because every path Obelus shows is
/// worked out by stripping the root off an absolute one -- and because
/// git is asked about the root from a process whose own directory
/// nothing here controls.
#[test]
fn the_project_is_absolute() {
    let opening = app::opening(&[PathBuf::from("Cargo.toml")]);
    let root = opening.root.expect("a project");
    assert!(
        root.is_absolute(),
        "a relative project, which every comparison against it fails quietly: {}",
        root.display()
    );
    // Where it landed is the tree this checkout is, which is the test
    // below and not this one -- what is asked here is that a bare file
    // name comes out absolute and above the directory Obelus was started
    // in. Said that way round so that it holds in any checkout.
    let here = std::env::current_dir().expect("a directory");
    assert!(
        here.starts_with(&root),
        "a bare file name landed outside the directory Obelus was started in: {}",
        root.display()
    );

    // And a relative *directory*, which is the case that reaches the
    // rule by a different road: a file's project is its parent and a parent
    // is absolute already, so this is the only path through `opening`
    // where the making-absolute is the thing doing the work.
    let opening = app::opening(&[PathBuf::from("src")]);
    let root = opening.root.expect("a project");
    assert!(
        root.is_absolute(),
        "a relative project, which every comparison against it fails quietly: {}",
        root.display()
    );
    assert_eq!(
        root,
        std::env::current_dir().expect("a directory").join("src"),
        "a relative directory did not land under the directory Obelus was started in"
    );
}

/// And what the application does with it: the project it works in is the one
/// the file is in, not the one Obelus was started in.
#[test]
fn the_application_works_in_the_tree_it_was_given() {
    let scratch = support::Scratch::new("opening-applied");
    let inner = scratch.path().join("src");
    std::fs::create_dir_all(&inner).expect("making it");
    let path = inner.join("one.rs");
    std::fs::write(&path, "fn main() {}\n").expect("writing it");

    let opening = app::opening(std::slice::from_ref(&path));
    let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
    app.work_in(opening.root.expect("a project"));
    support::lay_out(&mut app, 76, 18);

    assert_eq!(app.working_directory(), inner);
}

/// A directory on the command line opens Obelus on the list of what is
/// in it: the reader said which project and asked which file.
#[test]
fn a_tree_opens_on_the_list() {
    let scratch = support::Scratch::new("opening-list");
    std::fs::write(scratch.path().join("one.rs"), "fn main() {}\n").expect("writing it");

    let opening = app::opening(&[scratch.path().to_path_buf()]);
    let mut app = App::new(Vec::new());
    app.work_in(opening.root.expect("a project"));
    assert!(opening.list, "a directory left nothing to answer");
    app.list_at_start();
    support::lay_out(&mut app, 76, 18);

    // Nothing yet: the rows come from a walk that sends on the loop's
    // channel, and there is no channel until Obelus starts.
    assert!(
        app.picker().is_none(),
        "a list opened before it could be filled"
    );

    let (sender, _events) = obelus_app::event::channel();
    app.start(sender);
    assert!(
        app.picker().is_some(),
        "Obelus opened on nothing, with no file named and no list"
    );
}

/// And a file does not: the reader said which file.
#[test]
fn a_named_file_opens_on_the_file() {
    let scratch = support::Scratch::new("opening-no-list");
    let path = scratch.path().join("one.rs");
    std::fs::write(&path, "fn main() {}\n").expect("writing it");

    let opening = app::opening(std::slice::from_ref(&path));
    let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
    app.work_in(opening.root.expect("a project"));
    assert!(!opening.list);
    support::lay_out(&mut app, 76, 18);

    let (sender, _events) = obelus_app::event::channel();
    app.start(sender);
    assert!(
        app.picker().is_none(),
        "a list covered the file the reader named"
    );
}
