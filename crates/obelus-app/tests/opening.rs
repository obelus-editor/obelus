//! What a path on the command line means.
//!
//! Three questions in one answer: which project obelus works in, which files
//! it opens, and whether the question left over is "which file". A file
//! names the project it is in; a directory *is* the project, and the list is
//! what answers the rest.

mod support;

use std::path::PathBuf;

use obelus_app::{app, app::App};
use obelus_buffer::Buffer;

/// A file names the project it is in, and is opened.
#[test]
fn a_file_says_which_tree_it_is_in() {
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

/// Nothing on the command line changes nothing: the directory obelus was
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

/// A relative path becomes absolute, because every path obelus shows is
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
    assert_eq!(
        root,
        std::env::current_dir().expect("a directory"),
        "a bare file name did not land in the directory obelus was started in"
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
        "a relative directory did not land under the directory obelus was started in"
    );
}

/// And what the application does with it: the project it works in is the one
/// the file is in, not the one obelus was started in.
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

/// A directory on the command line opens obelus on the list of what is
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
    // channel, and there is no channel until obelus starts.
    assert!(
        app.picker().is_none(),
        "a list opened before it could be filled"
    );

    let (sender, _events) = obelus_app::event::channel();
    app.start(sender);
    assert!(
        app.picker().is_some(),
        "obelus opened on nothing, with no file named and no list"
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
