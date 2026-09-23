//! What obelus does before there is a screen.
//!
//! The steps are small and the order between them is not: a path is read
//! before anything takes the screen, the tree is settled before the
//! settings that belong to it, and what the command line asked for decides
//! whether obelus opens on a file or on the list. Each of those was a line
//! in `main` with a comment explaining itself and nothing checking it.

mod support;

use obelus_app::startup;

/// What the binary would hand in, standing in for a commit nobody built.
const BUILT: &str = "abc1234";

/// A path that cannot be read stops obelus before it starts.
///
/// The whole reason this runs before the screen is taken: an error here
/// reaches a reader on an ordinary terminal. An obelus that shrugged and
/// carried on would open on an empty screen with the reason behind the
/// alternate screen, which is how a missing file looks like a broken
/// program.
///
/// Broken deliberately by collecting the buffers with `flatten` instead of
/// `collect::<Result<_>>()?`: the start then succeeds with no documents.
#[test]
fn a_path_that_cannot_be_read_stops_the_start() {
    let scratch = support::Scratch::new("starting-missing");
    let missing = scratch.path().join("not-here.rs");

    let outcome = startup::start(std::slice::from_ref(&missing), BUILT);
    assert!(
        outcome.is_err(),
        "a path that is not there started obelus anyway, and the reason for \
         the empty screen went behind the alternate one"
    );
}

/// A file named is opened, and the tree is the one it is in.
///
/// Broken deliberately by dropping the `app.work_in(root)` call: the
/// document is still open, and the working directory is wherever the tests
/// happen to run.
#[test]
fn a_file_is_opened_in_the_tree_it_is_in() {
    let scratch = support::Scratch::new("starting-file");
    let inner = scratch.path().join("src");
    std::fs::create_dir_all(&inner).expect("making it");
    let path = inner.join("one.rs");
    std::fs::write(&path, "fn main() {}\n").expect("writing it");

    let app = startup::start(std::slice::from_ref(&path), BUILT).expect("starting");

    let buffer = app.current_buffer().expect("the file that was named");
    assert_eq!(buffer.path(), path, "some other file was opened");
    assert_eq!(
        app.working_directory(),
        inner,
        "the tree is not the directory the file is in"
    );
}

/// A directory named opens nothing and leaves the list to answer which
/// file.
///
/// Broken deliberately by dropping the `app.list_at_start()` call: nothing
/// is open and no list opens over it, so obelus starts on an empty screen.
#[test]
fn a_tree_opens_on_the_list() {
    let scratch = support::Scratch::new("starting-tree");
    std::fs::write(scratch.path().join("one.rs"), "fn main() {}\n").expect("writing it");

    let mut app = startup::start(&[scratch.path().to_path_buf()], BUILT).expect("starting");
    assert!(
        app.current_buffer().is_none(),
        "a directory was opened as a document"
    );

    // The list is filled by a walk that sends on the loop's channel, so it
    // is not there until obelus starts -- which is what `app.start` is.
    support::lay_out(&mut app, 76, 18);
    let (sender, _events) = obelus_app::event::channel();
    app.start(sender);
    assert!(
        app.picker().is_some(),
        "obelus opened on nothing, with no file named and no list"
    );
}

/// The tree is settled before its settings are read.
///
/// A tree keeps settings of its own in `.obelus/config.toml`, and they are
/// found by looking in the directory obelus is working in -- so reading
/// them before `work_in` looks in whatever directory the process happens to
/// have, finds nothing, and the tree's file does nothing with no sign that
/// it was even looked for. One line's worth of order, and the failure it
/// prevents is silent.
///
/// [`obelus_app::app::App::pinned`] is what this asserts on rather than the
/// theme itself, because the reader's own file has a theme too and a
/// machine whose reader had already chosen this one would pass a broken
/// start. What the tree *named* cannot be arrived at any other way.
///
/// Broken deliberately by moving `app.load_config()` above the
/// `app.work_in(root)` call: `pinned` comes back empty.
#[test]
fn the_tree_is_settled_before_its_settings_are_read() {
    let scratch = support::Scratch::new("starting-tree-settings");
    std::fs::create_dir_all(scratch.path().join(".obelus")).expect("making it");
    std::fs::write(
        scratch.path().join(".obelus").join("config.toml"),
        "theme = \"light\"\n",
    )
    .expect("writing it");

    let app = startup::start(&[scratch.path().to_path_buf()], BUILT).expect("starting");

    assert!(
        app.pinned().contains(&"theme"),
        "the tree's settings were not read, so its file did nothing: {:?}",
        app.pinned()
    );
    assert_eq!(
        app.project_config(),
        Some(scratch.path().join(".obelus").join("config.toml").as_path()),
        "obelus looked for the tree's settings somewhere else"
    );
    assert_eq!(
        app.theme_name(),
        "light",
        "the tree named a theme and obelus is not wearing it"
    );
}
