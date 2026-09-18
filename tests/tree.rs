//! The file list as a tree.
//!
//! One list with two shapes: a tree of the project while nothing is typed,
//! and the flat filtered list the moment something is. Which is not two
//! views -- typing is how a reader says they know what they are looking
//! for, and a tree is what they read when they do not.

mod support;

use obelus::component::picker::files;

/// A directory says it opens only where opening it would show something.
///
/// The mark is the only thing a row says about itself before it is pressed,
/// and a mark that offers to open an empty directory is a mark nobody
/// presses twice.
#[test]
fn a_directory_with_nothing_in_it_does_not_offer_to_open() {
    let scratch = support::Scratch::new("tree-holds");
    let root = scratch.path();
    std::fs::create_dir_all(root.join("src/lsp")).expect("making it");
    std::fs::create_dir_all(root.join("empty")).expect("making it");
    std::fs::write(root.join("src/lsp/hint.rs"), "").expect("writing it");
    std::fs::write(root.join("Cargo.toml"), "").expect("writing it");

    let found = files::inside(root, root, false);
    let said: Vec<(String, bool, bool)> = found
        .iter()
        .map(|entry| {
            (
                entry.path.display().to_string(),
                entry.directory,
                entry.holds,
            )
        })
        .collect();
    assert_eq!(
        said,
        [
            ("empty".to_string(), true, false),
            ("src".to_string(), true, true),
            ("Cargo.toml".to_string(), false, false),
        ],
        "the directories are not sorted first, or one of them is lying"
    );
}

/// What a tree was told to ignore is not there, and a directory holding
/// nothing else holds nothing.
#[test]
fn a_directory_of_ignored_files_holds_nothing() {
    let scratch = support::Scratch::new("tree-ignored");
    let root = scratch.path();
    // A `.gitignore` is only read inside a repository, which is the rule
    // the flat listing lives by too.
    std::fs::create_dir(root.join(".git")).expect("making it a repository");
    std::fs::write(root.join(".gitignore"), "target\n").expect("writing it");
    std::fs::create_dir_all(root.join("target/debug")).expect("making it");
    std::fs::write(root.join("target/debug/obelus"), "").expect("writing it");
    std::fs::write(root.join("Cargo.toml"), "").expect("writing it");

    let found = files::inside(root, root, false);
    assert!(
        !found.iter().any(|entry| entry.path.ends_with("target")),
        "an ignored directory is in the tree: {found:?}"
    );

    // And with the switch on, it is there and it holds something.
    let found = files::inside(root, root, true);
    let target = found
        .iter()
        .find(|entry| entry.path.ends_with("target"))
        .expect("the ignored directory");
    assert!(target.holds, "it has something in it and says it has not");
}

/// One level, and no further: what the mark says is that opening this row
/// shows a row, not that there is a file somewhere under it.
#[test]
fn what_it_holds_is_one_level_and_no_further() {
    let scratch = support::Scratch::new("tree-deep");
    let root = scratch.path();
    std::fs::create_dir_all(root.join("one/two/three")).expect("making it");
    std::fs::write(root.join("one/two/three/deep.rs"), "").expect("writing it");

    let found = files::inside(root, root, false);
    let one = found.first().expect("the directory");
    assert_eq!(one.path.display().to_string(), "one");
    assert!(one.holds, "it holds `two`, which is a row");

    // And the empty-looking middle of it holds the one below it.
    let found = files::inside(root, &root.join("one/two"), false);
    assert_eq!(found.len(), 1);
    assert!(found[0].holds, "it holds the file under it");
}

/// What it looks like: the project's shape, opened down to the file being
/// read.
#[test]
fn a_tree_on_screen() {
    use obelus::{app::App, buffer::Buffer};

    let scratch = support::Scratch::new("tree-drawn");
    let root = scratch.path();
    for directory in ["src/lsp", "src/app", "tests", "empty"] {
        std::fs::create_dir_all(root.join(directory)).expect("making it");
    }
    for file in [
        "Cargo.toml",
        "src/main.rs",
        "src/lsp/hint.rs",
        "src/lsp/colour.rs",
        "src/app/mod.rs",
        "tests/tree.rs",
    ] {
        std::fs::write(root.join(file), "").expect("writing it");
    }

    let mut app = App::new(vec![
        Buffer::open(&root.join("src/lsp/hint.rs")).expect("opening it"),
    ]);
    app.working_directory_for_test(root.to_path_buf());
    app.statuses_for_test(std::collections::HashMap::new());
    support::lay_out(&mut app, 60, 18);
    support::press_function(&mut app, 1);

    support::check("tree_60x18", &support::render(&mut app, 60, 18));
}
