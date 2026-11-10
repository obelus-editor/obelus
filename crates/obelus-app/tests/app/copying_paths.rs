//! Taking where a file list's row is, the way the list says it and the way
//! the machine does.

use crossterm::event::KeyCode;
use obelus_app::app::App;
use obelus_buffer::Buffer;

use crate::support;

/// A project with a file open in it, and the file list open over it on the
/// file nobody has open.
fn listing(name: &str) -> (support::Scratch, App) {
    let scratch = support::Scratch::new(name);
    scratch.write("src/hint.rs", "fn hint() {}\n");
    scratch.write("src/main.rs", "fn main() {}\n");
    let mut app = App::new(vec![
        Buffer::open(&scratch.join("src/hint.rs")).expect("opening it"),
    ]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.statuses_for_test(std::collections::HashMap::new());
    support::lay_out(&mut app, 76, 18);
    support::press_function(&mut app, 1);
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

    let expected = std::path::Path::new("src").join("main.rs");
    assert_eq!(
        obelus_clipboard::paste().as_deref(),
        Some(expected.display().to_string().as_str()),
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
