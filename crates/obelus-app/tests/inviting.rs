//! What a row that is typed into says before anything has been.
//!
//! Every list in Obelus is typed into and none of them said so: the row was
//! a prompt glyph and nothing else, so a reader who had not been told had
//! no way to find out but to try. Which is the same argument the empty
//! line makes -- what the reader cannot see, the row has to say.

mod support;

use crossterm::event::KeyCode;
use obelus_app::app::{App, dispatch};
use obelus_buffer::Buffer;
use obelus_command::Command;
use obelus_theme::builtin::DARK;

/// A project with a file open in it.
fn reading(name: &str) -> (support::Scratch, App) {
    let scratch = support::Scratch::new(name);
    scratch.write("src/hint.rs", "fn hint() {}\n");
    let mut app = App::new(vec![
        Buffer::open(&scratch.join("src/hint.rs")).expect("opening it"),
    ]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.statuses_for_test(std::collections::HashMap::new());
    support::lay_out(&mut app, 70, 14);
    (scratch, app)
}

/// The row a list is typed into, as it is drawn.
fn row(app: &mut App) -> String {
    let dump = support::render(app, 70, 14);
    support::text_block(&dump)
        .lines()
        .rfind(|row| row.contains('|'))
        .map(|row| row[row.find('|').expect("a divider") + 1..].trim())
        .unwrap_or_default()
        .to_string()
}

/// Each list says what typing into it does, and says it in its own words.
///
/// Its own, because the verb differs: a file list narrows what it is
/// already showing and a search goes and looks. One sentence for all of
/// them would be wrong on half.
///
/// Deliberate break: drop the `before_typing` from any one of these. That
/// row goes back to a prompt glyph on an empty line, which is what every
/// one of them was.
#[test]
fn each_list_says_what_typing_into_it_does() {
    for (at, (command, said)) in [
        (Command::FileOpen, "Filter files"),
        (Command::DocumentList, "Filter open documents"),
        (Command::CommandPalette, "Filter commands"),
        (Command::ThemeSelect, "Filter themes"),
        (Command::SymbolOutline, "Filter symbols"),
        // The search's is the one that changes with the tab, because the
        // same keystroke looks in one file or in every file.
        (Command::SearchFile, "Search this file"),
        (Command::SearchProject, "Search the project"),
    ]
    .into_iter()
    .enumerate()
    {
        // A directory each: the harness refuses two tests that would clear
        // each other's, and a loop is as many tests as it has turns.
        let (_scratch, mut app) = reading(&format!("invited-{at}"));
        dispatch::dispatch(&mut app, command);
        assert_eq!(row(&mut app), format!("> {said}"), "{command:?}");
    }
}

/// The first character replaces it, because by then the reader knows.
///
/// Deliberate break: have `Picker::invitation` answer whatever it was
/// given rather than `None` once something is typed. The words stay under
/// what is being typed, which on a row with a caret in it is two things in
/// one place.
#[test]
fn the_first_character_takes_it_away() {
    let (_scratch, mut app) = reading("invited-typed");
    dispatch::dispatch(&mut app, Command::FileOpen);
    assert_eq!(row(&mut app), "> Filter files");

    support::type_text(&mut app, "h");
    assert_eq!(row(&mut app), "> h");

    // And back again where the line is cleared, because the reader is
    // where they started.
    support::press(&mut app, KeyCode::Backspace);
    assert_eq!(row(&mut app), "> Filter files");
}

/// It is drawn in the ink an aside on this row is drawn in, so it reads as
/// the row telling the reader what to do rather than as something already
/// typed.
///
/// Deliberate break: hand `hint` the row's own style. What stands where
/// the typing goes is then the same colour as typing, and a reader
/// glancing at it has been shown a query they did not write.
#[test]
fn it_is_not_drawn_as_though_it_were_typed() {
    let (_scratch, mut app) = reading("invited-ink");
    dispatch::dispatch(&mut app, Command::FileOpen);

    let dump = support::render(&mut app, 70, 14);
    let at = support::column_of(
        support::text_block(&dump)
            .lines()
            .rfind(|row| row.contains("Filter files"))
            .expect("the row"),
        "Filter files",
    );
    let cells = support::cells_of(&mut app, 70, 14);
    let ink = cells
        .cell((u16::try_from(at).expect("a column"), 13))
        .expect("a cell")
        .fg;
    assert_eq!(ink, DARK.gutter, "not the dim ink an aside is written in");
    assert_ne!(ink, DARK.status_foreground, "drawn as though it were typed");
}

/// A list that is a *question* says the question and not this.
///
/// An agent asking to be allowed something puts its words in front of the
/// prompt already, and two sets of words in one row is neither.
///
/// Deliberate break: drop the `question.is_none()` from
/// `Picker::invitation`. The row reads `Run the tests?  > Filter commands`,
/// which is a question with an instruction written across it.
#[test]
fn a_list_that_is_a_question_says_the_question() {
    let (_scratch, mut app) = reading("invited-asked");
    dispatch::dispatch(&mut app, Command::CommandPalette);
    let picker = app.picker().expect("a list");
    assert_eq!(picker.invitation(), Some("Filter commands"));

    // The same list, asked as a question.
    let mut asked = obelus_component::picker::Picker::new(
        Vec::new(),
        obelus_component::picker::PickerLayout::Compact { rows: 3 },
    );
    asked.before_typing("Filter commands");
    asked.ask("Run the tests?");
    assert_eq!(
        asked.invitation(),
        None,
        "a question and an instruction in one row"
    );
}

/// The settings page says it too, and says which of its four lists.
///
/// The foot already says a key filters; this says what it would filter, on
/// the row the reader would type into. Which is not the same sentence: a
/// key's word at the foot says a key exists.
///
/// Deliberate break: have `what_is_filtered` answer the same words on
/// every tab. The keys page and the agents page then say they filter
/// settings, which is two of the three lists lying about themselves.
#[test]
fn the_settings_page_says_which_of_its_lists_is_filtered() {
    let (_scratch, mut app) = reading("invited-settings");
    dispatch::dispatch(&mut app, Command::ConfigOpen);
    assert_eq!(row(&mut app), "> Filter settings");

    support::press(&mut app, KeyCode::BackTab);
    assert_eq!(row(&mut app), "> Filter agents");

    support::press(&mut app, KeyCode::BackTab);
    assert_eq!(row(&mut app), "> Filter remote settings");

    support::press(&mut app, KeyCode::BackTab);
    assert_eq!(row(&mut app), "> Filter keys");

    // And the reader's own words take its place, the same as a list's.
    support::type_text(&mut app, "op");
    assert_eq!(row(&mut app), "> op");
}
