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
use obelus_component::{chooser::Known, picker::Picker};
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

/// What a directory holds is offered, the first of them chosen, and
/// enter puts it in the box.
///
/// The arrangement the agent's own commands settled, and the rule every
/// completion in Obelus follows: the list comes up already chosen, so
/// enter takes it without a key in between, and what it takes goes into
/// the box rather than being acted on. A directory takes a separator
/// with it, so the next level arrives without the reader typing one --
/// which makes walking down a tree one key per level.
///
/// Broken deliberately twice: `Chooser::put` leaving the separator off a
/// directory, so no further candidates are asked for; and `look_in`
/// keeping the list it had, so what is on screen after walking in is
/// still the directory above.
#[test]
fn enter_puts_the_chosen_row_in_the_box() {
    let scratch = support::Scratch::new("choosing-enter");
    std::fs::create_dir_all(scratch.path().join("alpha").join("inner")).expect("directories");
    let mut app = asking();
    press(&mut app, KeyCode::Enter);
    for character in format!("{}/", scratch.path().display()).chars() {
        press(&mut app, KeyCode::Char(character));
    }
    assert!(app.naming_list().is_some(), "the directory offered nothing");

    press(&mut app, KeyCode::Enter);

    let typed = app.choosing().expect("asking").typed;
    assert!(
        typed.ends_with(&format!("alpha{}", std::path::MAIN_SEPARATOR)),
        "enter did not put the chosen row in the box with its separator: {typed:?}"
    );
    assert!(
        app.choosing().is_some(),
        "enter opened something instead of typing it"
    );
    assert_eq!(
        app.naming_list()
            .and_then(Picker::selected_item)
            .map(|item| item.label.as_str()),
        // What is *inside* it, and not what was beside it: a list kept
        // across the separator would still be the directory above,
        // narrowed by letters belonging to a name somewhere else.
        Some("inner/"),
        "the list is not the chosen directory's"
    );
}

/// Escape shuts the list, and then enter opens what is in the box.
///
/// The one place a path differs from the agent's commands: a command's
/// name settles itself with a blank after it, and a path never does --
/// every directory chosen opens the next level. So there has to be a key
/// that says "this one", and escape already means "give up on the
/// nearest thing", which the list is.
///
/// Escape reaches the list and not the box, which is the half worth
/// asserting: one key further and the reader is back among the projects
/// with what they typed thrown away.
///
/// Broken deliberately by answering `false` for escape in
/// `App::naming_list_key`: the key falls through to the box, the path
/// box closes, and the reader is back on the list of projects.
#[test]
fn escape_shuts_the_list_and_then_enter_opens_what_is_typed() {
    let scratch = support::Scratch::new("choosing-escape-list");
    std::fs::create_dir_all(scratch.path().join("alpha")).expect("a directory");
    let mut app = asking();
    press(&mut app, KeyCode::Enter);
    for character in format!("{}/", scratch.path().display()).chars() {
        press(&mut app, KeyCode::Char(character));
    }
    assert!(app.naming_list().is_some(), "no list");

    press(&mut app, KeyCode::Esc);
    assert!(app.naming_list().is_none(), "the list did not shut");
    assert!(
        app.choosing().expect("asking").naming,
        "escape left the box as well as the list"
    );

    press(&mut app, KeyCode::Enter);

    assert!(app.choosing().is_none(), "it is still asking");
    assert_eq!(
        app.working_directory(),
        scratch.path(),
        "it did not open what was in the box"
    );
}

/// Choosing a project is the end of being asked.
///
/// Escape before enter, because what is typed names a directory that
/// exists and so has a list of its own in front of it -- and enter
/// belongs to the list while there is one. That chain is the subject of
/// the test above; here it is only how a reader gets to the thing being
/// tested.
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
    press(&mut app, KeyCode::Esc);
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

    let list = app
        .naming_list()
        .expect("a directory typed with forward slashes offered nothing");
    assert_eq!(
        list.selected_item().map(|item| item.label.as_str()),
        // The one thing in there, so it is both the only match and the
        // chosen one.
        Some("inside/"),
        "what the directory holds was not offered"
    );
}

/// A list the reader shut comes back when they type.
///
/// Escape takes the list away and the next letter asks again, which is
/// what `component::completion` does and what a reader expects of
/// anything that completes. Shutting it for good made escape a key that
/// could not be undone -- and the only way back was to type a separator,
/// which means naming a different directory.
///
/// Broken deliberately by leaving `self.naming_shut` set when the box
/// moves: the list never comes back and escape is final.
#[test]
fn a_list_that_was_shut_comes_back_on_the_next_letter() {
    let scratch = support::Scratch::new("choosing-reopen");
    std::fs::create_dir_all(scratch.path().join("alpha")).expect("a directory");
    let mut app = asking();
    press(&mut app, KeyCode::Enter);
    for character in format!("{}/", scratch.path().display()).chars() {
        press(&mut app, KeyCode::Char(character));
    }
    press(&mut app, KeyCode::Esc);
    assert!(app.naming_list().is_none(), "the list did not shut");

    press(&mut app, KeyCode::Char('a'));

    assert!(
        app.naming_list().is_some(),
        "a list the reader shut never came back, so escape could not be undone"
    );
}

/// And one that matched nothing comes back when the letter goes.
///
/// The same fault wearing different clothes: a list is gone for two
/// ordinary reasons and both have to be undoable, so what the directory
/// read found is kept apart from the list made of it. Held together, a
/// letter too many could not be rubbed out.
///
/// Broken deliberately by clearing `self.naming_read` beside
/// `self.naming_list` when nothing matches: backspace leaves the box
/// naming a real directory with nothing offered.
#[test]
fn a_list_that_matched_nothing_comes_back_when_the_letter_goes() {
    let scratch = support::Scratch::new("choosing-backspace");
    std::fs::create_dir_all(scratch.path().join("alpha")).expect("a directory");
    let mut app = asking();
    press(&mut app, KeyCode::Enter);
    for character in format!("{}/", scratch.path().display()).chars() {
        press(&mut app, KeyCode::Char(character));
    }
    press(&mut app, KeyCode::Char('z'));
    assert!(app.naming_list().is_none(), "`z` matched something");

    press(&mut app, KeyCode::Backspace);

    assert!(
        app.naming_list().is_some(),
        "rubbing the letter out did not bring back what the directory holds"
    );
}
