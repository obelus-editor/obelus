//! Taking where a file list's row is, the way the list says it and the way
//! the machine does.

use crossterm::event::KeyCode;
use obelus_app::app::App;
use obelus_buffer::Buffer;

use crate::support;

/// A project with a file open in it, `src/main.rs` changed as git sees it,
/// and the file list open.
fn opened(name: &str) -> (support::Scratch, App) {
    let scratch = support::Scratch::new(name);
    scratch.write("src/hint.rs", "fn hint() {}\n");
    scratch.write("src/main.rs", "fn main() {}\n");
    let mut app = App::new(vec![
        Buffer::open(&scratch.join("src/hint.rs")).expect("opening it"),
    ]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.statuses_for_test(
        [(
            std::path::PathBuf::from("src/main.rs"),
            obelus_git::FileStatus::Changed.into(),
        )]
        .into_iter()
        .collect(),
    );
    support::lay_out(&mut app, 76, 18);
    support::press_function(&mut app, 1);
    (scratch, app)
}

/// What the clipboard holds, with the separators one way round, so that a
/// test reads the same on Windows.
fn held() -> Option<String> {
    obelus_clipboard::paste().map(|text| text.replace('\\', "/"))
}

/// The same list, on the file nobody has open.
fn listing(name: &str) -> (support::Scratch, App) {
    let (scratch, mut app) = opened(name);
    // The tree opens on the file being read; one step down is `main.rs`,
    // so a copy of the file being read is not mistaken for a copy of the
    // row.
    support::press(&mut app, KeyCode::Down);
    assert_eq!(
        app.picker()
            .expect("the list")
            .selected_item()
            .map(|row| row.label.as_str()),
        Some("main.rs"),
        "the reader is not standing on the file this is about"
    );
    (scratch, app)
}

/// `alt+c` takes the path the list writes, against the project.
///
/// The deliberate break: the `alt+c` arm in `App::listing_key` taken out,
/// and the clipboard holds nothing; `selected_path` swapped for the file
/// being read, and it holds `src/hint.rs`.
#[test]
fn the_list_copies_where_its_row_is() {
    let _turn = support::clipboard_turn();
    obelus_clipboard::use_provider_for_test(obelus_clipboard::Provider::Kept);
    let (_scratch, mut app) = listing("copy-relative-path");

    support::press_alt_key(&mut app, KeyCode::Char('c'));

    assert_eq!(
        held().as_deref(),
        Some("src/main.rs"),
        "the clipboard does not hold the row's path"
    );
    // And the list is still open: a copy is not a choice.
    assert!(app.picker().is_some(), "copying closed the list");
}

/// `alt+a` takes all of it, from the root of the machine.
///
/// The deliberate break: the full path copied through `relative` like the
/// other one, and the clipboard holds `src/main.rs`.
#[test]
fn the_list_copies_all_of_where_its_row_is() {
    let _turn = support::clipboard_turn();
    obelus_clipboard::use_provider_for_test(obelus_clipboard::Provider::Kept);
    let (scratch, mut app) = listing("copy-full-path");

    support::press_alt_key(&mut app, KeyCode::Char('a'));

    assert_eq!(
        obelus_clipboard::paste().as_deref(),
        Some(
            scratch
                .join("src")
                .join("main.rs")
                .display()
                .to_string()
                .as_str()
        ),
        "the clipboard does not hold the row's whole path"
    );
}

/// The tab of what has changed is a list of files too, and its rows are
/// written against the project already.
///
/// The deliberate break: the two keys offered only where `offers_ignored`
/// is, as `alt+n` is -- on this tab the key went to nothing, and the
/// clipboard held what it held before.
#[test]
fn the_changed_tab_copies_where_its_row_is() {
    let _turn = support::clipboard_turn();
    obelus_clipboard::use_provider_for_test(obelus_clipboard::Provider::Kept);
    let (_scratch, mut app) = opened("copy-changed-path");
    obelus_clipboard::copy("before").expect("the clipboard");
    support::press(&mut app, KeyCode::Tab);

    support::press_alt_key(&mut app, KeyCode::Char('c'));

    assert_eq!(held().as_deref(), Some("src/main.rs"));
}

/// A folder is somewhere to paste as much as a file is.
///
/// The deliberate break: `PickerValue::place` answering only for a file,
/// and the clipboard held what it held before.
#[test]
fn a_folder_is_copied_as_well() {
    let _turn = support::clipboard_turn();
    obelus_clipboard::use_provider_for_test(obelus_clipboard::Provider::Kept);
    let (_scratch, mut app) = opened("copy-folder-path");
    // The tree opens on the file being read, and the row above it is the
    // folder it is in.
    support::press(&mut app, KeyCode::Up);
    assert_eq!(
        app.picker()
            .expect("the list")
            .selected_item()
            .map(|row| row.label.trim_end_matches(['/', '\\'])),
        Some("src"),
        "the reader is not standing on the folder"
    );

    support::press_alt_key(&mut app, KeyCode::Char('c'));

    assert_eq!(held().as_deref(), Some("src"));
}

/// Where the reader is on nothing, the palette draws the keys dim -- and a
/// key drawn dim does nothing.
///
/// The deliberate break: the keys offered whether or not a row is under
/// the reader (`.when` taken off), and the palette said they would work.
#[test]
fn on_no_row_there_is_nothing_to_copy() {
    let _turn = support::clipboard_turn();
    obelus_clipboard::use_provider_for_test(obelus_clipboard::Provider::Kept);
    let (_scratch, mut app) = opened("copy-no-row");
    support::type_text(&mut app, "nothing-is-called-this");
    assert!(
        app.picker().expect("the list").selected_item().is_none(),
        "something matched"
    );
    let copies: Vec<bool> = support::keys_here(&mut app)
        .into_iter()
        .filter(|key| key.does.starts_with("Copy"))
        .map(|key| key.usable)
        .collect();
    assert_eq!(
        copies,
        [false, false],
        "the palette offers a copy of nothing"
    );

    obelus_clipboard::copy("before").expect("the clipboard");
    support::press_alt_key(&mut app, KeyCode::Char('c'));
    support::press_alt_key(&mut app, KeyCode::Char('a'));
    assert_eq!(held().as_deref(), Some("before"));
    assert!(app.picker().is_some(), "the keys closed the list");
}

/// What a copy says is drawn while the list is still open, on the row the
/// list owns, after what is typed there.
///
/// The deliberate break: the note left out of `StatusView::render_prompt`,
/// as it was, and the row said nothing -- which for a copy that failed was
/// the only word about it.
#[test]
fn the_list_says_what_it_copied() {
    let _turn = support::clipboard_turn();
    obelus_clipboard::use_provider_for_test(obelus_clipboard::Provider::Kept);
    let (_scratch, mut app) = listing("copy-says-so");

    support::press_alt_key(&mut app, KeyCode::Char('a'));

    let dump = support::render(&mut app, 76, 18);
    let text = support::text_block(&dump);
    let last = text.lines().last().unwrap_or_default();
    assert!(
        last.contains("Copied full path"),
        "the list's row does not say what was copied:
{dump}"
    );
    // And the note goes at the next key, the way it does on every row.
    support::press(&mut app, KeyCode::Down);
    let dump = support::render(&mut app, 76, 18);
    assert!(
        !support::text_block(&dump).contains("Copied full path"),
        "the note outlived the key after it:
{dump}"
    );
}
