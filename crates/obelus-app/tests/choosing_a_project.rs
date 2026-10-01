//! Being asked which project, and what cannot be done until it is
//! answered.
//!
//! The rows are the test's own rather than the machine's: reading the real
//! list would make every assertion here depend on which projects whoever
//! ran it has opened.

mod support;

use crossterm::event::KeyCode;
use obelus_app::app::App;
use obelus_command::Command;
use obelus_component::chooser::Known;
use obelus_ui::Screen as _;
use support::press;

/// One asking about two projects, neither of which is anywhere real.
fn asking() -> App {
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(std::path::PathBuf::from("/tmp/obelus"));
    app.ask_about_these_projects_for_test(vec![
        Known {
            path: std::path::PathBuf::from("/tmp/obelus/alpha"),
            last: Some(2_000),
        },
        Known {
            path: std::path::PathBuf::from("/tmp/obelus/beta"),
            last: Some(1_000),
        },
    ]);
    app
}

/// Nothing that is about a project can be done until there is one.
///
/// Each of these took the directory the process happened to begin in,
/// which for a start from a desktop menu is the home directory: the file
/// list walked the whole of it, and the notes and the conversations were
/// filed under a place nobody works in.
///
/// Asked through `offers`, which is the one judgement -- the palette draws
/// a row it refuses as dim and `handle_key` asks the same question before
/// dispatching, so a command cannot be off in one place and live in the
/// other.
///
/// Broken deliberately by answering `true` for `Requires::AProject` in
/// `App::offers`: every one of these comes back offered.
#[test]
fn nothing_about_a_project_is_offered_until_there_is_one() {
    let app = asking();

    for command in [
        Command::FileOpen,
        Command::FileNew,
        Command::DocumentList,
        Command::SearchProject,
        Command::CountLines,
        Command::ConversationNew,
        Command::ConversationSelect,
        Command::TodoOpen,
        Command::ConfigProject,
        // These two were already dim here, but by walking a repository
        // that was not there and finding nothing in it.
        Command::FileChanged,
        Command::HistoryProject,
    ] {
        assert!(
            !app.offers(command),
            "{} is offered with no project",
            command.name()
        );
    }
}

/// And what is not about a project still works.
///
/// A reader on a machine Obelus has never run on would otherwise be left
/// with a screen and no way off it: they must be able to leave, whatever
/// else is refused.
///
/// Broken deliberately by putting `Command::Quit` on `Requires::AProject`:
/// the one key out of this screen goes dim.
#[test]
fn leaving_is_still_offered() {
    let app = asking();

    for command in [Command::Quit, Command::ThemeSelect, Command::ConfigOpen] {
        assert!(
            app.offers(command),
            "{} is refused with no project",
            command.name()
        );
    }
}

/// The first row is always there, and it is not one of the projects.
///
/// Which is what makes the screen answerable on a machine with nothing
/// remembered: the list is never empty.
///
/// Broken deliberately by starting `at` at 1 in `Chooser::new`: the reader
/// opens standing on a project rather than on the way to any of them.
#[test]
fn it_opens_on_the_row_that_opens_a_project() {
    let app = asking();
    let choosing = app.choosing().expect("being asked");

    assert_eq!(choosing.at, 0, "it does not open on the first row");
    assert_eq!(choosing.known.len(), 2, "the projects are not offered");
}

/// The filter narrows the projects and never the row above them.
///
/// A filter that could take that row away would be a screen with no way
/// off it: type four letters that match nothing and there is nothing left
/// to press. So the test is not that the row is drawn -- it is that it
/// still *works*, which is the thing a reader would find out the hard
/// way.
///
/// Broken deliberately by making the opening row conditional on the
/// filter -- `KeyCode::Enter if self.at == 0 && self.filter.is_empty()`,
/// which reads like tidiness -- and enter on the only row left does
/// nothing at all.
#[test]
fn the_filter_cannot_hide_the_way_out() {
    let mut app = asking();
    for character in "zzzz".chars() {
        press(&mut app, KeyCode::Char(character));
    }
    let choosing = app.choosing().expect("still being asked");
    assert!(choosing.known.is_empty(), "the filter matched something");
    assert_eq!(choosing.at, 0, "the reader is not on the row that is left");

    press(&mut app, KeyCode::Enter);

    assert!(
        app.choosing().expect("asking").naming,
        "the only row left did nothing, so the screen cannot be answered"
    );
}

/// Typing puts the reader back on the row that is always the same one.
///
/// The rows underneath have moved, so standing on the fifth of them is
/// standing on a different project than it was a keystroke ago -- and
/// pressing enter would open one the reader never looked at.
///
/// Broken deliberately by taking `self.at = 0` out of the typing arm in
/// `Chooser::choosing`: the reader stays on row two while what row two
/// means changes under them.
#[test]
fn typing_moves_the_reader_off_a_row_that_has_changed() {
    let mut app = asking();
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Down);
    assert_eq!(app.choosing().expect("asking").at, 2, "it did not move");

    press(&mut app, KeyCode::Char('a'));

    assert_eq!(
        app.choosing().expect("asking").at,
        0,
        "the reader was left on a row the filter had moved"
    );
}

/// Escape clears the filter, and then does nothing at all.
///
/// Nothing is nearer than this screen, so there is nothing to give up on
/// -- but a reader who has narrowed to nothing has exactly one key they
/// would reach for, and it is this one.
///
/// Broken deliberately by letting escape leave: the second press answers
/// `None` to `choosing` and Obelus is sitting on a project it was never
/// told.
#[test]
fn escape_clears_the_filter_and_never_leaves() {
    let mut app = asking();
    press(&mut app, KeyCode::Char('a'));
    assert_eq!(app.choosing().expect("asking").typed, "a", "no filter");

    press(&mut app, KeyCode::Esc);
    assert_eq!(app.choosing().expect("asking").typed, "", "not cleared");

    press(&mut app, KeyCode::Esc);
    assert!(
        app.choosing().is_some(),
        "escape left a screen there is nothing behind"
    );
}

/// The first row turns the foot into a path box, and it starts empty.
///
/// Two boxes and two meanings: what was typed to narrow a list of places
/// a reader has been is not the beginning of a path, and carrying it over
/// is the one thing that would make the two read as one.
///
/// Broken deliberately by building the new `Field` with
/// `Field::about(&self.filter.said(), ..)`: the path box opens holding
/// the filter's word.
#[test]
fn naming_a_path_starts_from_nothing() {
    let mut app = asking();
    press(&mut app, KeyCode::Char('a'));
    press(&mut app, KeyCode::Enter);

    let choosing = app.choosing().expect("asking");
    assert!(choosing.naming, "the foot is not a path box");
    assert_eq!(choosing.typed, "", "the filter's word was carried over");
}

/// And escape comes back out of it, because now there is something under.
///
/// Escape gives up on the nearest thing, and the nearest thing is the box
/// rather than the screen.
///
/// Broken deliberately by answering `Ignored` for escape in
/// `Chooser::naming`: the reader is stuck in the path box.
#[test]
fn escape_comes_back_out_of_the_path_box() {
    let mut app = asking();
    press(&mut app, KeyCode::Enter);
    assert!(app.choosing().expect("asking").naming, "not naming");

    press(&mut app, KeyCode::Esc);

    let choosing = app.choosing().expect("asking");
    assert!(!choosing.naming, "escape did not leave the path box");
    assert_eq!(choosing.known.len(), 2, "the projects did not come back");
}

/// What a directory holds is offered, and `Tab` finishes what is certain.
///
/// The half of `Tab` that is not choosing: where every candidate starts
/// the same way, that much is certain and typing it again is work the
/// machine can do.
///
/// Broken deliberately by having `finish_it` answer `Ignored` always: the
/// box keeps what was typed and the reader finishes the name by hand.
#[test]
fn tab_puts_in_what_every_candidate_agrees_on() {
    let scratch = support::Scratch::new("choosing-tab");
    // Two that share a beginning, so there is something certain and
    // something still to choose.
    for name in ["shared-one", "shared-two"] {
        std::fs::create_dir_all(scratch.path().join(name)).expect("a directory");
    }
    let mut app = asking();
    press(&mut app, KeyCode::Enter);
    for character in format!("{}/", scratch.path().display()).chars() {
        press(&mut app, KeyCode::Char(character));
    }
    assert_eq!(
        app.choosing().expect("asking").candidates.len(),
        2,
        "the directory was not read"
    );

    press(&mut app, KeyCode::Tab);

    let typed = app.choosing().expect("asking").typed;
    assert!(
        typed.ends_with("shared-"),
        "tab did not put in what both of them start with: {typed:?}"
    );
}

/// Choosing a project is the end of being asked.
///
/// Broken deliberately by leaving `self.chooser` alone in `settle_on`:
/// the screen goes on asking over a project Obelus has already been put
/// on, and every command stays dim.
#[test]
fn choosing_a_project_settles_it() {
    let scratch = support::Scratch::new("choosing-settles");
    let mut app = asking();
    press(&mut app, KeyCode::Enter);
    for character in scratch.path().display().to_string().chars() {
        press(&mut app, KeyCode::Char(character));
    }
    press(&mut app, KeyCode::Enter);

    assert!(app.choosing().is_none(), "it is still asking");
    assert_eq!(
        app.working_directory(),
        scratch.path(),
        "Obelus was not put on the project that was chosen"
    );
    assert!(
        app.offers(Command::FileOpen),
        "the keys are still dim after a project was chosen"
    );
}

/// A path typed with forward slashes finds its directory on every
/// platform.
///
/// Windows takes `/` as well as `\`, and a reader pasting a path from a
/// url, a log line or a shell that writes them that way is typing the
/// one this platform does not write. Looking only for
/// `std::path::MAIN_SEPARATOR` left them with a box that never offered
/// anything.
///
/// Only the Windows job can go red for this -- on Unix `/` *is* the
/// separator -- which is exactly what that job is for.
///
/// Broken deliberately by putting `rfind(std::path::MAIN_SEPARATOR)` back
/// in `Chooser::wants`: no candidates arrive on Windows and this fails
/// there while staying green here.
#[test]
fn a_path_typed_with_forward_slashes_is_still_a_path() {
    let scratch = support::Scratch::new("choosing-slashes");
    std::fs::create_dir_all(scratch.path().join("inside")).expect("a directory");
    // Written with forward slashes whatever this platform writes, which
    // is the whole point.
    let typed = format!("{}/", scratch.path().display()).replace('\\', "/");

    let mut app = asking();
    press(&mut app, KeyCode::Enter);
    for character in typed.chars() {
        press(&mut app, KeyCode::Char(character));
    }

    let choosing = app.choosing().expect("asking");
    assert!(
        choosing
            .candidates
            .iter()
            .any(|name| name.starts_with("inside")),
        "a directory typed with forward slashes offered nothing: {:?}",
        choosing.candidates
    );
}
