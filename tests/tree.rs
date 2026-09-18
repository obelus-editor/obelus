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

/// A project with a few directories in it, and the tree open on it.
fn a_tree(name: &str) -> (support::Scratch, obelus::app::App) {
    use obelus::{app::App, buffer::Buffer};

    let scratch = support::Scratch::new(name);
    let root = scratch.path();
    for directory in ["src/lsp", "src/app", "tests", "empty"] {
        std::fs::create_dir_all(root.join(directory)).expect("making it");
    }
    for file in [
        "Cargo.toml",
        "src/main.rs",
        "src/lsp/hint.rs",
        "src/lsp/colour.rs",
        // Two of them, because half the rows in a Rust project are called
        // this and telling them apart is the reason a row is remembered by
        // its path.
        "src/lsp/mod.rs",
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
    (scratch, app)
}

/// The label of the row the list is on.
fn standing_on(app: &obelus::app::App) -> String {
    app.picker()
        .and_then(obelus::component::picker::Picker::selected_item)
        .map(|item| item.label.clone())
        .unwrap_or_default()
}

/// The path behind it, which is what tells two rows of the same name apart.
fn path_of(app: &obelus::app::App) -> String {
    use obelus::component::picker::PickerValue;

    match app
        .picker()
        .and_then(obelus::component::picker::Picker::selected_item)
        .map(|item| &item.value)
    {
        Some(PickerValue::File(path) | PickerValue::Directory(path)) => path.display().to_string(),
        _ => String::new(),
    }
}

/// A query that came and went leaves the reader where it found them.
///
/// Typing turns the tree into a flat list of everything, and clearing it
/// turns it back. The tree is the same tree either side of that, so the
/// row is still there -- and a reader who typed a few letters, changed
/// their mind and took them back has not asked to be moved.
#[test]
fn clearing_a_query_puts_the_tree_back_where_it_was() {
    let (_scratch, mut app) = a_tree("tree-restores");
    // It opens on the file being read. One row up from it is its sibling,
    // which is somewhere the reader went rather than somewhere they were
    // put.
    assert_eq!(standing_on(&app), "hint.rs");
    support::press(&mut app, crossterm::event::KeyCode::Up);
    assert_eq!(standing_on(&app), "colour.rs");

    support::type_text(&mut app, "z");
    support::press(&mut app, crossterm::event::KeyCode::Backspace);

    assert_eq!(
        standing_on(&app),
        "colour.rs",
        "the tree came back on the file being read rather than where the reader was"
    );
}

/// And a list opened again starts on the file being read.
///
/// The row kept from a query is about the time the list was last open. A
/// reader who closed it, read somewhere else and opened it again is
/// somewhere else, and the list opening on where they used to be would be
/// the list remembering harder than they do.
#[test]
fn opening_the_list_again_starts_on_the_file_being_read() {
    use crossterm::event::KeyCode;

    let (_scratch, mut app) = a_tree("tree-reopens");
    support::press(&mut app, KeyCode::Up);
    assert_eq!(standing_on(&app), "colour.rs");
    // A query, so there is a row put away, and then away from the list
    // entirely.
    support::type_text(&mut app, "z");
    support::press(&mut app, KeyCode::Esc);

    support::press_function(&mut app, 1);
    assert_eq!(
        standing_on(&app),
        "hint.rs",
        "the list opened where it was left rather than on the file being read"
    );
}

/// Walking to the other tab and back is the same act: the rows change and
/// the reader has not moved.
#[test]
fn coming_back_from_the_other_tab_puts_the_tree_back() {
    use crossterm::event::KeyCode;

    let (scratch, mut app) = a_tree("tree-tabs");
    // A second tab, which is only there when something has changed.
    app.statuses_for_test(std::collections::HashMap::from([(
        scratch.join("src/main.rs"),
        obelus::git::FileStatus::Changed,
    )]));
    support::press(&mut app, KeyCode::Esc);
    support::press_function(&mut app, 1);
    assert_eq!(
        app.picker()
            .map(|picker| obelus::component::picker::Picker::tabs(picker).len()),
        Some(2),
        "the other tab is not there, so this tests nothing"
    );

    support::press(&mut app, KeyCode::Up);
    assert_eq!(standing_on(&app), "colour.rs");
    support::press(&mut app, KeyCode::Tab);
    assert_eq!(
        standing_on(&app),
        "src/main.rs",
        "the other tab did not open"
    );
    support::press(&mut app, KeyCode::Tab);
    assert_eq!(
        standing_on(&app),
        "colour.rs",
        "a walk to the other tab and back moved the reader"
    );
}

/// The row it goes back to is the row it left, not the first one that
/// happens to be called the same thing.
///
/// A tree draws the last part of a path, so half a Rust project's rows say
/// `mod.rs`. A list that remembered a name would send the reader to
/// whichever of them comes first.
#[test]
fn it_goes_back_to_the_row_and_not_to_the_name() {
    use crossterm::event::KeyCode;

    let (_scratch, mut app) = a_tree("tree-same-name");
    // Open `src/app` as well, so both of the `mod.rs` rows are on screen.
    while standing_on(&app) != "app" {
        support::press(&mut app, KeyCode::Up);
    }
    support::press(&mut app, KeyCode::Enter);
    while standing_on(&app) != "mod.rs" || path_of(&app) != "src/lsp/mod.rs" {
        support::press(&mut app, KeyCode::Down);
    }

    support::type_text(&mut app, "z");
    support::press(&mut app, KeyCode::Backspace);

    assert_eq!(
        path_of(&app),
        "src/lsp/mod.rs",
        "it went back to a row with the right name and the wrong path"
    );
}
