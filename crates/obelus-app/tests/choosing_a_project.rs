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

/// One row, written the way the screen would write it.
///
/// These are all outside `$HOME`, so what the row shows is the path
/// itself and the test does not depend on whose machine it runs on.
fn known(path: &str, last: Option<i64>) -> Known {
    Known {
        path: std::path::PathBuf::from(path),
        shown: path.to_string(),
        last,
    }
}

/// One asking about two projects, neither of which is anywhere real.
fn asking() -> App {
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(std::path::PathBuf::from("/tmp/obelus"));
    app.ask_about_these_projects_for_test(vec![
        known("/tmp/obelus/alpha", Some(2_000)),
        known("/tmp/obelus/beta", Some(1_000)),
    ]);
    app
}

/// Goes to the row that opens a project not in the list, and opens it.
///
/// `End`, because that row is the last: under the projects, which the
/// reader starts on.
fn open_another(app: &mut App) {
    press(app, KeyCode::End);
    press(app, KeyCode::Enter);
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

/// The row the reader is on is not lit under a list opened over the page:
/// the keys are the list's.
///
/// Deliberate break: light the page's row whatever is over it -- the
/// project wears the list's own mark beside it.
#[test]
fn the_row_under_a_list_is_not_lit() {
    let mut app = asking();
    let row = "alpha";
    let dump = support::render(&mut app, 60, 20);
    let lit = support::drawn_in(&dump, row);

    // The themes, narrowed to the one in force: short enough to leave the
    // row in sight, and nobody else's colours.
    obelus_app::app::dispatch::dispatch(&mut app, Command::ThemeSelect);
    support::type_text(&mut app, "dark");
    let dump = support::render(&mut app, 60, 20);
    assert!(app.picker().is_some(), "the themes did not open:\n{dump}");
    assert_ne!(
        support::drawn_in(&dump, row),
        lit,
        "the project is lit under the list:\n{dump}"
    );

    press(&mut app, KeyCode::Esc);
    let dump = support::render(&mut app, 60, 20);
    assert_eq!(
        support::drawn_in(&dump, row),
        lit,
        "the row did not take the mark back:\n{dump}"
    );
}

/// The reader starts on the newest project, so enter alone takes them
/// back to where they were last.
///
/// The commonest answer to "which project". The row that opens one not in
/// the list used to be first, and every start from a launcher cost a key
/// to get past it.
///
/// Broken deliberately by starting `at` at `known.len()` in
/// `Chooser::new`: the reader opens on the row under the projects.
#[test]
fn it_opens_on_the_newest_project() {
    let app = asking();
    let choosing = app.choosing().expect("being asked");

    assert_eq!(choosing.known.len(), 2, "the projects are not offered");
    assert_eq!(choosing.at, 0, "it does not open on the first project");
    assert_eq!(
        choosing.known[0].path, "/tmp/obelus/alpha",
        "the first project is not the newest"
    );
}

/// And enter on it opens it.
///
/// Broken deliberately by putting `KeyCode::Enter if self.at == 0` back
/// in `Chooser::choosing`, which is what the opening row used to answer
/// to: enter turns the foot into a path box instead.
#[test]
fn enter_opens_the_newest_project() {
    let scratch = support::Scratch::new("choosing-newest");
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(std::path::PathBuf::from("/tmp/obelus"));
    app.ask_about_these_projects_for_test(vec![
        Known {
            path: scratch.path().to_path_buf(),
            shown: scratch.path().display().to_string(),
            last: Some(2_000),
        },
        known("/tmp/obelus/beta", Some(1_000)),
    ]);

    press(&mut app, KeyCode::Enter);

    assert!(app.choosing().is_none(), "it is still asking");
    assert_eq!(
        app.working_directory(),
        scratch.path(),
        "enter did not open the project the reader started on"
    );
}

/// The filter narrows the projects and never the row under them.
///
/// A filter that could take that row away would be a screen with no way
/// off it: type four letters that match nothing and there is nothing left
/// to press. So the test is not that the row is drawn -- it is that it
/// still *works*, which is the thing a reader would find out the hard
/// way.
///
/// Broken deliberately by making the opening row conditional on the
/// filter -- `KeyCode::Enter if self.on_the_opening_row() &&
/// self.filter.is_empty()`,
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

/// Typing puts the reader back at the top of what is left.
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

/// The opening row turns the foot into a path box, and it starts empty.
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
    open_another(&mut app);

    let choosing = app.choosing().expect("asking");
    assert!(choosing.naming, "the foot is not a path box");
    assert_eq!(choosing.typed, "", "the filter's word was carried over");
}

/// And escape comes back out of it, because now there is something under.
///
/// Escape gives up on the nearest thing, and the nearest thing is the box
/// rather than the screen.
///
/// And back on the row the box was opened from, rather than on a project
/// the reader had moved away from.
///
/// Broken deliberately twice: answering `Ignored` for escape in
/// `Chooser::naming`, and the reader is stuck in the path box; and
/// setting `self.at = 0` there, and they come back out on the newest
/// project.
#[test]
fn escape_comes_back_out_of_the_path_box() {
    let mut app = asking();
    open_another(&mut app);
    assert!(app.choosing().expect("asking").naming, "not naming");

    press(&mut app, KeyCode::Esc);

    let choosing = app.choosing().expect("asking");
    assert!(!choosing.naming, "escape did not leave the path box");
    assert_eq!(choosing.known.len(), 2, "the projects did not come back");
    assert_eq!(
        choosing.at, 2,
        "the reader is not back on the row the box was opened from"
    );
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
    open_another(&mut app);
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
        //
        // Spelled with this platform's own separator, because that is
        // what Obelus writes onto the end of a directory's name -- it
        // reads both and writes one.
        Some(format!("inner{}", std::path::MAIN_SEPARATOR).as_str()),
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
    open_another(&mut app);
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
    open_another(&mut app);
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
    open_another(&mut app);
    for character in typed.chars() {
        press(&mut app, KeyCode::Char(character));
    }

    let list = app
        .naming_list()
        .expect("a directory typed with forward slashes offered nothing");
    assert_eq!(
        list.selected_item().map(|item| item.label.as_str()),
        // The one thing in there, so it is both the only match and the
        // chosen one -- spelled with the separator Obelus writes, which
        // is not the one that was typed. Reading `/` and writing `\` on
        // Windows is the whole point of the test above it.
        Some(format!("inside{}", std::path::MAIN_SEPARATOR).as_str()),
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
    open_another(&mut app);
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
    open_another(&mut app);
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

/// Rubbing the box out closes the list.
///
/// What was read belongs to a path that is no longer in the box, and an
/// empty query matches every one of its rows -- so the list did not
/// merely linger, it opened out into the whole of a directory the reader
/// had just left.
///
/// Which directory the box is about is one answer now, asked both by the
/// key that decides whether to read and by the frame that decides
/// whether what was read still applies. Worked out twice, the two
/// disagreed about exactly this.
///
/// Broken deliberately by taking the `directory_named` check out of
/// `settle_the_naming_list`: an empty box offers everything in the
/// directory it used to name.
#[test]
fn rubbing_the_box_out_closes_the_list() {
    let scratch = support::Scratch::new("choosing-emptied");
    std::fs::create_dir_all(scratch.path().join("alpha")).expect("a directory");
    let mut app = asking();
    open_another(&mut app);
    let typed = format!("{}/", scratch.path().display());
    for character in typed.chars() {
        press(&mut app, KeyCode::Char(character));
    }
    assert!(app.naming_list().is_some(), "the directory offered nothing");

    for _ in 0..typed.chars().count() {
        press(&mut app, KeyCode::Backspace);
    }

    assert_eq!(
        app.choosing().expect("asking").typed,
        "",
        "the box is not empty"
    );
    assert!(
        app.naming_list().is_none(),
        "an empty box still offers what the directory it used to name holds"
    );
}

/// Rubbing back past a separator moves the list up a directory.
///
/// Not the same as emptying the box, which is why both are here: the box
/// still names a directory, it is just a different one, so what was read
/// is dropped and the parent is read instead. A list that stayed would
/// be the child's contents under a box naming the parent.
///
/// Broken deliberately by keeping the list across a change of directory
/// in `look_in`: the rows are still the child's.
#[test]
fn rubbing_back_past_a_separator_moves_the_list_up() {
    let scratch = support::Scratch::new("choosing-back-up");
    std::fs::create_dir_all(scratch.path().join("alpha").join("inner")).expect("directories");
    let mut app = asking();
    open_another(&mut app);
    for character in format!("{}/alpha/", scratch.path().display()).chars() {
        press(&mut app, KeyCode::Char(character));
    }
    assert_eq!(
        app.naming_list()
            .and_then(Picker::selected_item)
            .map(|item| item.label.as_str()),
        Some(format!("inner{}", std::path::MAIN_SEPARATOR).as_str()),
        "the child directory was not read"
    );

    press(&mut app, KeyCode::Backspace);

    assert_eq!(
        app.naming_list()
            .and_then(Picker::selected_item)
            .map(|item| item.label.as_str()),
        // The parent's own row, narrowed by the `alpha` still in the box.
        Some(format!("alpha{}", std::path::MAIN_SEPARATOR).as_str()),
        "the list is still the child's under a box naming the parent"
    );
}

/// A path that is not there is not a project.
///
/// The worst way this could go wrong, and the way it did: `opening`
/// answers what a path on the command line *means*, and a path that is
/// not there means "a file to make, in the directory above it". Right
/// for `ob notes.md`; here it put Obelus on the directory the process
/// began in, which from a desktop menu is the home directory -- the one
/// answer this whole screen exists to avoid. One letter wrong and the
/// reader was back where they started, with no sign anything had gone
/// amiss.
///
/// Broken deliberately by taking the `path.exists()` check out of
/// `settle_on`: the screen closes and Obelus is working in whatever
/// directory the box's path happened to sit in.
#[test]
fn a_path_that_is_not_there_is_refused() {
    let scratch = support::Scratch::new("choosing-unreal");
    let mut app = asking();
    let before = app.working_directory().to_path_buf();
    open_another(&mut app);
    for character in format!("{}/nothing-here", scratch.path().display()).chars() {
        press(&mut app, KeyCode::Char(character));
    }
    // Said in the ink before the key is pressed, which is the half that
    // keeps enter from looking broken.
    assert!(
        !app.choosing().expect("asking").there,
        "a path that is not there is not drawn as one"
    );

    // No escape first, and that is the point: the name matches nothing
    // in the directory, so there is no list in front of the box and
    // enter is the box's already. Pressing escape here would leave the
    // box altogether, and the test would pass without the refusal ever
    // being asked for -- which is how it first went green.
    assert!(app.naming_list().is_none(), "there is a list in the way");
    press(&mut app, KeyCode::Enter);

    assert!(app.choosing().is_some(), "it stopped asking");
    assert_eq!(
        app.working_directory(),
        before,
        "Obelus moved to a directory nobody named"
    );
}

/// And a remembered project whose directory has gone is taken off the
/// list.
///
/// A row says what it was; whether it still is comes from trying to open
/// it, which is why nothing stats twenty paths on the way to this
/// screen. When the trying fails, the row goes -- a reader pressing a
/// row and seeing nothing happen has been told nothing.
///
/// Broken deliberately by taking the `chooser.forget(path)` out: the row
/// stays and enter on it does nothing, for ever.
#[test]
fn a_remembered_project_that_has_gone_is_dropped() {
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(std::path::PathBuf::from("/tmp/obelus"));
    app.ask_about_these_projects_for_test(vec![known(
        "/tmp/obelus/gone-since-it-was-written",
        None,
    )]);
    assert_eq!(app.choosing().expect("asking").known.len(), 1, "no row");

    press(&mut app, KeyCode::Enter);

    assert!(app.choosing().is_some(), "it opened something that is gone");
    assert!(
        app.choosing().expect("asking").known.is_empty(),
        "a row that cannot be opened is still offered"
    );
}

/// The filter marks what it matched, and matches what is on the row.
///
/// Two halves of one fault. A list whose matched characters are not
/// marked is a list that looks as though nothing happened -- which is
/// the thing `ui::write_marked` exists to stop, and this was the newest
/// list that had forgotten it.
///
/// And it is run against what the row *shows* and not against the path
/// behind it. On an ordinary machine every path begins `/home/<name>/`,
/// so `home` matched every row while no row had the word on it -- and a
/// mark counted in one string and painted onto another lands on the
/// wrong letters.
///
/// Broken deliberately twice: matching `known.path` instead of
/// `known.shown`, so a row shown as `~/...` is matched on `/home/...`;
/// and answering `None` for the range, so nothing is marked.
#[test]
fn the_filter_marks_what_it_matched_on_the_row_as_shown() {
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(std::path::PathBuf::from("/tmp/obelus"));
    app.ask_about_these_projects_for_test(vec![Known {
        path: std::path::PathBuf::from("/home/somebody/Work/obelus"),
        // What the row shows, which is where `/home/somebody` has gone.
        shown: "~/Work/obelus".to_string(),
        last: None,
    }]);

    // A word that is in the path and not on the row: it must not match.
    press(&mut app, KeyCode::Char('h'));
    press(&mut app, KeyCode::Char('o'));
    press(&mut app, KeyCode::Char('m'));
    assert!(
        app.choosing().expect("asking").known.is_empty(),
        "the filter matched a word that is nowhere on the row"
    );

    for _ in 0..3 {
        press(&mut app, KeyCode::Backspace);
    }
    for character in "Work".chars() {
        press(&mut app, KeyCode::Char(character));
    }

    let choosing = app.choosing().expect("asking");
    assert_eq!(choosing.known.len(), 1, "the filter matched nothing");
    assert_eq!(
        choosing.known[0].matched,
        // `~/` is two characters, so `Work` begins at the third.
        Some((2, 6)),
        "what matched is not marked, or is marked in the wrong place"
    );
}

/// A path that goes nowhere says so, and only once there is nothing left
/// to suggest.
///
/// Both halves matter. Silence on enter is a key that looks broken --
/// the reader has pressed it and been told nothing. But saying it from
/// the first letter would be a complaint about typing: `/tmp/o` is not
/// there either, and the list below is offering `obelus/` at the time.
/// So the row speaks where there is nothing to suggest *and* nothing at
/// the path, which is a reader who has actually gone wrong.
///
/// Broken deliberately twice: dropping `!choosing.offering` from the
/// condition, so the row complains while a name is half typed; and
/// having `App::note_what_the_box_names` answer `true` always, so it
/// never complains at all.
#[test]
fn a_path_that_goes_nowhere_says_so_once_nothing_is_left_to_suggest() {
    let scratch = support::Scratch::new("choosing-nowhere");
    std::fs::create_dir_all(scratch.path().join("alpha")).expect("a directory");
    let mut app = asking();
    open_another(&mut app);
    for character in format!("{}/", scratch.path().display()).chars() {
        press(&mut app, KeyCode::Char(character));
    }

    // Half a name, with the list offering the whole of it: nothing to
    // complain about yet.
    press(&mut app, KeyCode::Char('a'));
    let choosing = app.choosing().expect("asking");
    assert!(!choosing.there, "`alph` is not a directory");
    assert!(
        choosing.offering,
        "the list stopped offering what is being typed"
    );

    // And a name that is not going anywhere.
    for character in "bsent".chars() {
        press(&mut app, KeyCode::Char(character));
    }
    let choosing = app.choosing().expect("asking");
    assert!(!choosing.there, "`absent` is somehow a directory");
    assert!(
        !choosing.offering,
        "something is still being offered for a name that matches nothing"
    );
}

/// Choosing from the list leaves the row saying the right thing about
/// what it put there.
///
/// The key that fills the box from the list left by a different door
/// from the keys that type into it, and only one of those doors worked
/// out whether what is in the box is there. So a candidate accepted into
/// the box carried whatever the ink had said about the half-typed name
/// before it.
///
/// Broken deliberately by taking the `note_what_the_box_names` call out
/// of the branch that handles the list's keys.
#[test]
fn choosing_from_the_list_leaves_the_row_telling_the_truth() {
    let scratch = support::Scratch::new("choosing-stale-ink");
    std::fs::create_dir_all(scratch.path().join("alpha")).expect("a directory");
    let mut app = asking();
    open_another(&mut app);
    // Half the name: not a directory, and the list is offering the
    // whole of it. The two have to disagree here or the stale answer
    // and the true one are the same and nothing is being tested -- a
    // box holding the directory itself is there either way, which is
    // how this first went green.
    for character in format!("{}/alph", scratch.path().display()).chars() {
        press(&mut app, KeyCode::Char(character));
    }
    let choosing = app.choosing().expect("asking");
    assert!(!choosing.there, "`alph` is a directory");
    assert!(choosing.offering, "the list is not offering `alpha`");

    press(&mut app, KeyCode::Enter);

    assert!(
        app.choosing().expect("asking").there,
        "the box holds a directory the list just put there, and the row still says it is not"
    );
}

/// Nothing about the project is started until there is one.
///
/// `App::start` runs after the arguments have been read, so on a start
/// with nothing to go on everything rooted at the project was rooted at
/// the directory the process began in -- the home directory, from a
/// desktop launcher -- and nothing moved it when the reader answered.
/// An agent was offered tools for the wrong project, and git's `HEAD`
/// and `index` went unwatched, so `forget_what_git_said` never fired
/// and the branch on the status row and the marks in the margin were
/// whatever they had been for the rest of the session.
///
/// The tools are what this watches, because an address is a thing a
/// test can read and a watch is not. They are the same gate: one `if`
/// in `App::start` and one pair of calls in `settle_on`.
///
/// Broken deliberately by taking the `self.chooser.is_none()` guard out
/// of `App::start`: the tools are offered for the home directory before
/// anybody has named a project.
#[test]
fn nothing_about_the_project_is_started_until_there_is_one() {
    let scratch = support::Scratch::new("choosing-starting");
    let mut app = asking();
    let (sender, _events) = std::sync::mpsc::channel();
    app.start(sender);

    assert!(
        app.tools_url().is_none(),
        "an agent is being offered tools for a project nobody has named"
    );

    open_another(&mut app);
    for character in scratch.path().display().to_string().chars() {
        press(&mut app, KeyCode::Char(character));
    }
    press(&mut app, KeyCode::Esc);
    press(&mut app, KeyCode::Enter);
    assert!(app.choosing().is_none(), "it is still asking");

    assert!(
        app.tools_url().is_some(),
        "the project was chosen and nothing about it was started"
    );
}

/// A paste goes into whichever box the page is showing -- which is also
/// where a word an input method spelled goes, because it arrives as one.
///
/// And the input method is on for both, the filter with nothing in it as
/// well: no caret there, and the first letter still goes in.
///
/// Deliberate break: taking the chooser's arm out of `App::paste_text`,
/// and the paste falls through to a file there is none of; and answering
/// the chooser with `false` in `App::takes_text`.
#[test]
fn a_paste_goes_into_the_box_the_page_is_showing() {
    let mut app = asking();
    assert!(app.takes_text(), "the filter is said to take nothing");
    app.handle(obelus_app::event::Event::Paste("文档".to_string()));
    assert_eq!(
        app.choosing().expect("asking").typed,
        "文档",
        "the paste did not reach the filter"
    );

    press(&mut app, KeyCode::Esc);
    open_another(&mut app);
    assert!(app.takes_text(), "the path box is said to take nothing");
    app.handle(obelus_app::event::Event::Paste("/tmp/文档".to_string()));
    let choosing = app.choosing().expect("asking");
    assert!(choosing.naming, "not in the path box");
    assert_eq!(
        choosing.typed, "/tmp/文档",
        "the paste did not reach the path box"
    );
}
