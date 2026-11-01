//! What Obelus does before there is a screen.
//!
//! The steps are small and the order between them is not: a path is read
//! before anything takes the screen, the tree is settled before the
//! settings that belong to it, and what the command line asked for decides
//! whether Obelus opens on a file or on the list. Each of those was a line
//! in `main` with a comment explaining itself and nothing checking it.
//!
//! Not in the application's binary because a start reads the settings and
//! applies them to the whole process: the tree here that says `tab_width = 8`
//! lays every tab in it at eight, under any test beside it that draws one.

use obelus_app::startup;

use crate::support;

/// What the binary would hand in, standing in for a commit nobody built.
const BUILT: &str = "abc1234";

/// A path that cannot be read stops Obelus before it starts.
///
/// The whole reason this runs before the screen is taken: an error here
/// reaches a reader on an ordinary terminal. An Obelus that shrugged and
/// carried on would open on an empty screen with the reason behind the
/// alternate screen, which is how a missing file looks like a broken
/// program.
///
/// Broken deliberately by collecting the buffers with `flatten` instead of
/// `collect::<Result<_>>()?`: the start then succeeds with no documents.
#[test]
fn a_path_that_cannot_be_read_stops_the_start() {
    let _turn = crate::turn();
    let scratch = support::Scratch::new("starting-missing");
    let missing = scratch.path().join("not-here.rs");

    let outcome = startup::start(std::slice::from_ref(&missing), BUILT);
    assert!(
        outcome.is_err(),
        "a path that is not there started Obelus anyway, and the reason for \
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
    let _turn = crate::turn();
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

/// The tree is settled before its settings are read.
///
/// A tree keeps settings of its own in `.obelus/config.toml`, and they are
/// found by looking in the directory Obelus is working in -- so reading
/// them before `work_in` looks in whatever directory the process happens to
/// have, finds nothing, and the tree's file does nothing with no sign that
/// it was even looked for. One line's worth of order, and the failure it
/// prevents is silent.
///
/// [`obelus_app::app::App::pinned`] is what this asserts on rather than the
/// width itself, because the reader's own file may have a width too and a
/// machine whose reader had already chosen this one would pass a broken
/// start. What the tree *named* cannot be arrived at any other way. The
/// tab width rather than the theme it was once, which a tree may no longer
/// name.
///
/// Broken deliberately by moving `app.load_config()` above the
/// `app.work_in(root)` call: `pinned` comes back empty.
#[test]
fn the_tree_is_settled_before_its_settings_are_read() {
    let _turn = crate::turn();
    let scratch = support::Scratch::new("starting-tree-settings");
    std::fs::create_dir_all(scratch.path().join(".obelus")).expect("making it");
    std::fs::write(
        scratch.path().join(".obelus").join("config.toml"),
        "tab_width = 8\n",
    )
    .expect("writing it");

    let app = startup::start(&[scratch.path().to_path_buf()], BUILT).expect("starting");

    assert!(
        app.pinned().contains(&"tab_width"),
        "the tree's settings were not read, so its file did nothing: {:?}",
        app.pinned()
    );
    assert_eq!(
        app.project_config(),
        Some(scratch.path().join(".obelus").join("config.toml").as_path()),
        "Obelus looked for the tree's settings somewhere else"
    );
    assert_eq!(
        app.config().tab_width,
        8,
        "the tree named a width and Obelus is not using it"
    );
}

/// A start in an ssh session knows it is one, which is what keeps the
/// welcome screen's sheen still there and nothing else.
///
/// Here rather than beside the tests of what the flag does, because it
/// sets a variable the whole process reads -- and set back on the way out,
/// so nothing after this turn sees it.
///
/// Broken deliberately by dropping the `drawn_over_a_network` call from
/// `startup::start`: every test of what the flag does still passes, and
/// this one does not.
#[test]
fn a_start_over_ssh_knows_it_is_over_a_network() {
    let _turn = crate::turn();
    let scratch = support::Scratch::new("starting-over-ssh");
    let _ssh = Variable::set("SSH_TTY", "/dev/pts/0");

    let app = startup::start(&[scratch.path().to_path_buf()], BUILT).expect("starting");

    assert!(
        app.is_drawn_over_a_network(),
        "started in an ssh session, and Obelus does not know it"
    );
}

/// An environment variable, set for as long as this is held and put back
/// as it was when it goes -- a panic included.
struct Variable {
    name: &'static str,
    was: Option<std::ffi::OsString>,
}

impl Variable {
    fn set(name: &'static str, value: &str) -> Self {
        let was = std::env::var_os(name);
        // SAFETY: every test in this binary holds `crate::turn` while it
        // runs, so nothing beside this one is reading the environment.
        unsafe { std::env::set_var(name, value) };
        Self { name, was }
    }
}

impl Drop for Variable {
    fn drop(&mut self) {
        // SAFETY: as in `set`; this is dropped before the turn is.
        unsafe {
            match &self.was {
                Some(was) => std::env::set_var(self.name, was),
                None => std::env::remove_var(self.name),
            }
        }
    }
}
