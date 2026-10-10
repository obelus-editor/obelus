//! The file list as a tree.
//!
//! One list with two shapes: a tree of the project while nothing is typed,
//! and the flat filtered list the moment something is. Which is not two
//! views -- typing is how a reader says they know what they are looking
//! for, and a tree is what they read when they do not.

mod support;

use obelus_component::picker::files;

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

    let found = files::inside(root, root, false, false);
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

    let found = files::inside(root, root, false, false);
    assert!(
        !found.iter().any(|entry| entry.path.ends_with("target")),
        "an ignored directory is in the tree: {found:?}"
    );

    // And with the switch on, it is there and it holds something.
    let found = files::inside(root, root, true, false);
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

    let found = files::inside(root, root, false, false);
    let one = found.first().expect("the directory");
    assert_eq!(one.path.display().to_string(), "one");
    assert!(one.holds, "it holds `two`, which is a row");

    // And the empty-looking middle of it holds the one below it.
    let found = files::inside(root, &root.join("one/two"), false, false);
    assert_eq!(found.len(), 1);
    assert!(found[0].holds, "it holds the file under it");
}

/// What it looks like: the project's shape, opened down to the file being
/// read.
#[test]
fn a_tree_on_screen() {
    use obelus_app::app::App;
    use obelus_buffer::Buffer;

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
fn a_tree(name: &str) -> (support::Scratch, obelus_app::app::App) {
    use obelus_app::app::App;
    use obelus_buffer::Buffer;

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
fn standing_on(app: &obelus_app::app::App) -> String {
    app.picker()
        .and_then(obelus_component::picker::Picker::selected_item)
        .map(|item| item.label.clone())
        .unwrap_or_default()
}

/// The path behind it, which is what tells two rows of the same name apart.
///
/// The path itself rather than the string it displays as. A path written
/// out in a test is written with `/`, and the one the walk found wears
/// whatever this platform puts between two parts of a path -- so the
/// comparison has to be about the parts and not about the letters between
/// them.
fn path_of(app: &obelus_app::app::App) -> std::path::PathBuf {
    use obelus_component::picker::PickerValue;

    match app
        .picker()
        .and_then(obelus_component::picker::Picker::selected_item)
        .map(|item| &item.value)
    {
        Some(PickerValue::File(path) | PickerValue::Directory(path)) => path.clone(),
        _ => std::path::PathBuf::new(),
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
        obelus_git::FileStatus::Changed.into(),
    )]));
    support::press(&mut app, KeyCode::Esc);
    support::press_function(&mut app, 1);
    assert_eq!(
        app.picker()
            .map(|picker| obelus_component::picker::Picker::tabs(picker).len()),
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

    // More presses than the tree has rows. Both walks below are bounded
    // because a walk towards a row that cannot be recognised is a test
    // that hangs rather than one that fails -- and a suite that does not
    // finish says nothing about any of the rest of itself. Which is what
    // this cost while the comparison was against a written-out string:
    // the row was there, and `src\lsp\mod.rs` is not `src/lsp/mod.rs`.
    const ENOUGH: usize = 40;

    let (_scratch, mut app) = a_tree("tree-same-name");
    // Open `src/app` as well, so both of the `mod.rs` rows are on screen.
    for _ in 0..ENOUGH {
        if standing_on(&app) == "app" {
            break;
        }
        support::press(&mut app, KeyCode::Up);
    }
    assert_eq!(
        standing_on(&app),
        "app",
        "there is no second mod.rs to tell the first one from"
    );
    support::press(&mut app, KeyCode::Enter);

    let wanted = std::path::Path::new("src/lsp/mod.rs");
    for _ in 0..ENOUGH {
        if standing_on(&app) == "mod.rs" && path_of(&app) == wanted {
            break;
        }
        support::press(&mut app, KeyCode::Down);
    }
    assert_eq!(
        path_of(&app),
        wanted,
        "the walk never arrived at the row this is about"
    );

    support::type_text(&mut app, "z");
    support::press(&mut app, KeyCode::Backspace);

    assert_eq!(
        path_of(&app),
        wanted,
        "it went back to a row with the right name and the wrong path"
    );
}

/// A press moves the selection, and a press on a row's arrow opens it.
///
/// The lists Obelus draws over a file took no press at all. The wheel
/// reached them -- it is its own event, and goes to whichever layer is
/// nearest -- and the query box did too, because the box is on the status
/// row and the status row is asked first. So the box was clickable and the
/// list under it was not.
///
/// A press moves the selection and nothing else. These are drawn over
/// something the reader was reading, and a mis-aimed press that *chose* a
/// row would take them somewhere they never asked to go; choosing is a
/// second press on the row, which nobody aims badly twice.
///
/// Except a row's own arrow, which says the row opens. Pressing that does
/// what pressing an arrow means everywhere, and it cannot take the reader
/// anywhere: the arrow is declared on the rows that open and on no others.
///
/// Broken deliberately by handing the press back to nothing, which leaves
/// the selection where it was; or by opening on a press anywhere in the
/// row, which turns a press meant to look at a directory into one that
/// walks into it.
#[test]
fn a_press_moves_the_selection_and_the_arrow_opens_a_row() {
    let (_scratch, mut app) = a_tree("tree-press");
    let _ = support::render(&mut app, 60, 18);

    // The rows: which of them say they open, and how far in each is drawn,
    // because a row's arrow is drawn after its indent.
    let rows: Vec<(Option<bool>, u16)> = app
        .picker()
        .expect("the tree")
        .matches()
        .map(|item| (item.opens, item.depth))
        .collect();
    let opens: Vec<Option<bool>> = rows.iter().map(|(opens, _)| *opens).collect();
    // One that is shut, so that pressing it has to open: the tree starts
    // open along the path to the file being read, and a press on one of
    // those would be a press that shuts.
    let directory = opens
        .iter()
        .position(|opens| *opens == Some(false))
        .expect("a row that is shut");
    // And a row that is a file, which is what a press must not choose: a
    // row that opens nothing and is not a file -- a directory with nothing
    // in it -- would take this press and do nothing either way.
    let other = app
        .picker()
        .expect("the tree")
        .matches()
        .position(|item| matches!(item.value, obelus_component::picker::PickerValue::File(_)))
        .expect("a file among the rows");
    assert_ne!(directory, other, "every row opens, so this proves nothing");

    let area = app.editor_area_for_test();
    let top = obelus_ui::picker::rows_region(app.picker().expect("the tree"), area).y;
    let press = |app: &mut obelus_app::app::App, row: usize, x: u16| {
        app.handle(obelus_app::event::Event::Pointer {
            kind: obelus_app::event::Pointer::Pressed,
            x,
            y: top + u16::try_from(row).expect("a row"),
        });
    };

    // A press on the words of a row moves the selection there, and does
    // nothing else: the list is still the list.
    let open_before = app.file_count_for_test();
    press(&mut app, other, area.x + 20);
    assert_eq!(
        app.picker().expect("the tree").selected_row(),
        Some(other),
        "the press did not move the selection"
    );
    assert_eq!(
        app.file_count_for_test(),
        open_before,
        "the press on the words of a file opened it"
    );

    // And a press on the arrow of a row that opens opens it. Two columns a
    // level and one to stand clear of the edge, which is where the list
    // draws it.
    let arrow = 1 + rows[directory].1 * 2;
    let before = app.picker().expect("the tree").matches().count();
    press(&mut app, directory, area.x + arrow);
    let after = app.picker().expect("the tree").matches().count();
    assert!(
        after > before,
        "the press on the arrow did not open the row: {before} rows, then {after}"
    );
}

/// A second press on a row chooses it, the way enter does.
///
/// One press only moves the selection, because the list is drawn over a
/// file and a mis-aimed press must not take the reader anywhere. Two in
/// the same cell are not mis-aimed, and a reader who had to reach for
/// enter after pointing at a row had the pointer doing half a job.
///
/// Broken deliberately by dropping `|| twice` from `press_in_picker`: the
/// file stays shut and the list stays up.
#[test]
fn a_double_click_on_a_row_chooses_it() {
    let (scratch, mut app) = a_tree("tree-double-click");
    // Words on every row, so that a press on the file once it is open
    // lands on some.
    std::fs::write(
        scratch.path().join("src/main.rs"),
        "let words_long_enough_to_reach_the_pointer = 1;\n".repeat(30),
    )
    .expect("writing it");
    let _ = support::render(&mut app, 60, 18);

    let file = app
        .picker()
        .expect("the tree")
        .matches()
        .position(|item| {
            matches!(&item.value, obelus_component::picker::PickerValue::File(path)
                if path.ends_with("main.rs"))
        })
        .expect("main.rs among the rows");
    let area = app.editor_area_for_test();
    let top = obelus_ui::picker::rows_region(app.picker().expect("the tree"), area).y;
    let press = |app: &mut obelus_app::app::App| {
        app.handle(obelus_app::event::Event::Pointer {
            kind: obelus_app::event::Pointer::Pressed,
            x: area.x + 20,
            y: top + u16::try_from(file).expect("a row"),
        });
    };

    let open_before = app.file_count_for_test();
    press(&mut app);
    assert!(app.picker().is_some(), "one press chose the row");
    press(&mut app);
    assert!(app.picker().is_none(), "the second press left the list up");
    assert_eq!(
        app.file_count_for_test(),
        open_before + 1,
        "the second press did not open the file"
    );

    // A hand that moves before letting go is a drag in the file the
    // second press opened, and selects nothing there: the press it would
    // extend was in the list.
    //
    // Broken deliberately by taking the `pressed_in_the_file` arm out of
    // `on_pointer`: the drag selects from where the file opened.
    app.handle(obelus_app::event::Event::Pointer {
        kind: obelus_app::event::Pointer::Dragged,
        x: area.x + 30,
        y: top + u16::try_from(file).expect("a row") + 1,
    });
    assert!(
        app.current_buffer()
            .and_then(obelus_buffer::Buffer::selection)
            .is_none(),
        "a drag after a double click selected in the file it opened"
    );

    // And a third, landing on the file the second one opened, is a first
    // press there: not the third of a click that would take a whole line.
    //
    // Broken deliberately by not forgetting the count in
    // `pointer_in_a_layer`: the line under the pointer is taken.
    press(&mut app);
    assert!(
        app.current_buffer()
            .and_then(obelus_buffer::Buffer::selection)
            .is_none(),
        "the press after a double click took hold of a line"
    );
}
