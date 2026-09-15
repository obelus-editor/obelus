//! The one list-with-a-prompt, in its four instantiations.

mod support;

use crossterm::event::KeyCode;
use obelus::{
    app::App,
    component::picker::{Picker, PickerItem, PickerLayout, PickerOutcome, PickerValue},
    event::Event,
};
use support::{press, press_alt_key, press_control, press_control_key, press_function, type_text};

fn app() -> App {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    // A clean tree, whatever the checkout these tests are running in looks
    // like: the file list grows a tab for the changed files when there are
    // any, and a test of how a *row* is drawn should not depend on whether
    // someone is working in the repository.
    app.statuses_for_test(std::collections::HashMap::new());
    app
}

fn items(labels: &[&str]) -> Vec<PickerItem> {
    labels
        .iter()
        .map(|label| PickerItem {
            prose: false,
            marker: None,
            icon: None,
            label: (*label).to_string(),
            detail: None,
            trailing: None,
            changed: None,
            value: PickerValue::File(label.into()),
            enabled: true,
            colours: None,
            status: None,
            depth: 0,
            kind: None,
            tab: None,
        })
        .collect()
}

fn key(code: KeyCode) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
}

fn control(code: KeyCode) -> crossterm::event::KeyEvent {
    crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::CONTROL)
}

/// A page in these tests, which is what a ten-row window gives.
const PAGE: u16 = 10;

fn many(count: usize) -> Vec<PickerItem> {
    (0..count)
        .map(|index| PickerItem {
            prose: false,
            marker: None,
            icon: None,
            label: format!("item-{index:03}"),
            detail: None,
            trailing: None,
            changed: None,
            value: PickerValue::File(format!("item-{index:03}").into()),
            enabled: true,
            colours: None,
            status: None,
            depth: 0,
            kind: None,
            tab: None,
        })
        .collect()
}

#[test]
fn the_command_palette_hugs_the_status_bar_and_leaves_the_code_visible() {
    let mut app = app();
    press_control(&mut app, 'p');
    // Tall enough to show what the layout is for: ten rows of list, the tabs
    // above them, and the code still readable over the top. On a screen too
    // short for that the list takes what there is, which is the same rule
    // arriving at a different answer.
    support::check("palette_60x20", &support::render(&mut app, 60, 20));
}

/// The other half of the same rule: the mark a buffer wears for work that
/// is not on disk does *not* recede. It is the one thing in a list of open
/// files a reader must not walk past, and it gets the colour the status row
/// marks the same fact in.
///
/// Paired with `the_arrow_on_a_commit_recedes` over in the git tests. Between
/// them they say that a mark is drawn by what it means, and that the two
/// meanings in obelus today do not look alike.
#[test]
fn the_mark_on_an_unwritten_buffer_does_not_recede() {
    use obelus::theme::builtin::DARK;

    let mut app = app();
    support::type_text(&mut app, "x");
    press_function(&mut app, 2);

    let dump = support::render(&mut app, 60, 12);
    let mark = support::colour_under(&dump, obelus::icons::ui::UNSAVED);
    assert_eq!(
        mark,
        support::spelled(DARK.status_stale),
        "the unwritten mark is not in the colour the status row uses:\n{dump}"
    );
    assert_ne!(
        mark,
        support::spelled(DARK.gutter),
        "the unwritten mark receded into the gutter's grey"
    );
}

#[test]
fn a_full_area_picker_covers_the_code() {
    let mut app = app();
    press_function(&mut app, 2);
    support::check("buffers_60x12", &support::render(&mut app, 60, 12));
}

/// A list shorter than the region still has to cover it. Painting only the
/// rows with items leaves the code showing underneath, which reads as a
/// half-drawn screen rather than as a short list.
#[test]
fn a_full_area_picker_with_one_item_still_covers_the_code() {
    let mut app = app();
    press_function(&mut app, 2);
    let dump = support::render(&mut app, 60, 12);
    let text = support::text_block(&dump);

    assert!(text.contains("sample.rs"), "the one buffer is not listed");
    assert!(
        !text.contains("fn main()"),
        "the code is still showing through:\n{dump}"
    );
}

#[test]
fn typing_narrows_the_palette_and_the_count_follows() {
    let mut app = app();
    press_control(&mut app, 'p');
    let all = support::render(&mut app, 60, 12);

    type_text(&mut app, "theme");
    let narrowed = support::render(&mut app, 60, 12);

    assert!(support::text_block(&all).contains("reload-file"));
    assert!(
        !support::text_block(&narrowed).contains("reload-file"),
        "the query did not narrow the list:\n{narrowed}"
    );
    assert!(support::text_block(&narrowed).contains("choose-theme"));
}

/// The matched characters of the selected row get a background of their
/// own, which is the only thing that says *why* a row matched a query it
/// does not contain literally.
///
/// A background rather than a colour: a row can be a line of code, carrying
/// the file's own syntax colours, and a match painted over those would be
/// one more hue among seven.
#[test]
fn the_matched_characters_of_the_selected_row_are_coloured() {
    let mut app = app();
    press_function(&mut app, 2);
    let unmatched = support::render(&mut app, 60, 12);
    assert!(
        !support::legend_block(&unmatched).contains("#38577f"),
        "the match background is on screen before anything has been typed:\n{unmatched}"
    );

    // A fuzzy query: the label is `tests/fixtures/sample.rs` and contains
    // neither "smpl" nor its letters adjacently.
    type_text(&mut app, "smpl");
    let dump = support::render(&mut app, 60, 12);

    assert_eq!(
        support::text_block(&dump).matches("sample.rs").count(),
        1,
        "the fuzzy query matched nothing:\n{dump}"
    );
    assert!(
        support::legend_block(&dump).contains("bg=#38577f"),
        "no characters were marked as matched:\n{dump}"
    );
}

/// Every row on screen gets its matched characters coloured, not only the
/// selected one. The colouring is the only thing that says why a row is in a
/// list it does not literally contain the query of, and that question is
/// asked of the rows the reader is comparing -- which is all of them.
#[test]
fn the_matched_characters_of_every_visible_row_are_coloured() {
    // The match background, wherever it appears: on the selected row and on
    // an ordinary one, whatever colour the characters themselves have.
    fn match_letters(dump: &str) -> Vec<char> {
        support::legend_block(dump)
            .lines()
            .filter(|line| line.contains("bg=#38577f"))
            .filter_map(|line| line.trim().chars().next())
            .collect()
    }

    let mut app = app();
    press_function(&mut app, 1);
    app.handle(Event::FilesFound {
        generation: 1,
        paths: vec![
            "src/one/alpha.rs".into(),
            "src/two/beta.rs".into(),
            "src/three/gamma.rs".into(),
            "src/four/delta.rs".into(),
        ],
    });
    type_text(&mut app, "src");

    let dump = support::render(&mut app, 60, 24);
    let letters = match_letters(&dump);
    assert!(!letters.is_empty(), "nothing was coloured at all:\n{dump}");

    let rows: Vec<&str> = support::style_block(&dump)
        .lines()
        .filter(|row| !row.is_empty())
        .collect();
    for (index, row) in rows.iter().take(4).enumerate() {
        let coloured = row.chars().filter(|cell| letters.contains(cell)).count();
        assert_eq!(
            coloured, 3,
            "row {index} coloured {coloured} of the three characters of `src`:\n{dump}"
        );
    }

    // And the rows at the far end of a list long enough to scroll. The
    // positions are worked out for the window being drawn, so a window that
    // has moved has to have moved them with it.
    app.handle(Event::FilesFound {
        generation: 1,
        paths: (0..40)
            .map(|number| format!("src/dir_{number:02}/file.rs").into())
            .collect(),
    });
    support::press_control_key(&mut app, KeyCode::End);

    let dump = support::render(&mut app, 60, 22);
    let letters = match_letters(&dump);
    let rows: Vec<&str> = support::style_block(&dump)
        .lines()
        .filter(|row| !row.is_empty())
        .collect();
    for (index, row) in rows.iter().take(10).enumerate() {
        let coloured = row.chars().filter(|cell| letters.contains(cell)).count();
        assert_eq!(
            coloured, 3,
            "row {index} of a scrolled window coloured {coloured}:\n{dump}"
        );
    }
}

#[test]
fn escape_leaves_the_code_as_it_was() {
    let mut app = app();
    let before = support::render(&mut app, 60, 12);

    press_control(&mut app, 'p');
    press(&mut app, KeyCode::Esc);

    assert_eq!(support::render(&mut app, 60, 12), before);
}

/// Choosing `choose-theme` from the palette opens the theme picker. Nothing
/// about that transition is special-cased: accepting a command runs it, and
/// running that one opens a picker.
#[test]
fn the_palette_can_open_another_picker() {
    let mut app = app();
    press_control(&mut app, 'p');
    type_text(&mut app, "theme");
    press(&mut app, KeyCode::Enter);

    let dump = support::render(&mut app, 60, 12);
    let text = support::text_block(&dump);
    assert!(text.contains("dark") && text.contains("light"), "{dump}");
}

#[test]
fn choosing_a_theme_changes_the_colours() {
    let mut app = app();
    let dark = support::render(&mut app, 60, 12);

    press_control(&mut app, 'p');
    type_text(&mut app, "theme");
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    let light = support::render(&mut app, 60, 12);

    assert_eq!(support::text_block(&dark), support::text_block(&light));
    assert_ne!(support::legend_block(&dark), support::legend_block(&light));
}

/// A list is a dialog: it takes its own keys and nothing else.
///
/// A key the list does not want has nowhere else to go, because the key
/// table reached from inside a list is a way to put a second list on top of
/// the first -- and then two escapes to leave, with nothing on screen to
/// say which of the two a key would reach. The way out is escape, and the
/// key works again after it.
#[test]
fn a_key_the_picker_does_not_want_goes_nowhere() {
    let mut app = app();
    press_control(&mut app, 'p');
    let palette = support::render(&mut app, 60, 12);
    assert!(!app.should_quit());

    press_control(&mut app, 'q');
    assert!(
        !app.should_quit(),
        "ctrl+q reached the key table from inside a list"
    );
    press_function(&mut app, 1);
    let after = support::render(&mut app, 60, 12);
    assert_eq!(
        support::text_block(&palette),
        support::text_block(&after),
        "a second list opened over the first"
    );

    press(&mut app, KeyCode::Esc);
    press_control(&mut app, 'q');
    assert!(app.should_quit(), "escape did not give the key table back");
}

/// The one key the list of open files adds does not bring the others.
#[test]
fn the_buffer_list_takes_its_own_key_and_not_the_global_ones() {
    let mut app = app();
    press_function(&mut app, 2);
    press_control(&mut app, 'q');
    assert!(
        !app.should_quit(),
        "the list of open files reached the global keys"
    );
}

/// Arrow keys move the selection, not the cursor, while a picker is open.
#[test]
fn arrows_move_the_selection_rather_than_the_cursor() {
    let mut app = app();
    let cursor = app.current_buffer().expect("a buffer").cursor();

    press_control(&mut app, 'p');
    for _ in 0..3 {
        press(&mut app, KeyCode::Down);
    }

    let after = app.current_buffer().expect("a buffer").cursor();
    assert_eq!(after.line, cursor.line, "the cursor moved");
}

/// Batches from a walk whose picker has already closed must not appear in the
/// next one.
#[test]
fn paths_from_a_superseded_walk_are_dropped() {
    let mut app = app();
    press_function(&mut app, 1);
    press(&mut app, KeyCode::Esc);
    press_function(&mut app, 1);

    // Generation one belongs to the first open; the second bumped it.
    app.handle(Event::FilesFound {
        generation: 1,
        paths: vec!["stale/from/the/first/walk.rs".into()],
    });

    let dump = support::render(&mut app, 60, 12);
    assert!(
        !support::text_block(&dump).contains("stale/from/the/first/walk.rs"),
        "a batch from the closed walk was merged in:\n{dump}"
    );
}

#[test]
fn paths_from_the_current_walk_are_listed() {
    let mut app = app();
    press_function(&mut app, 1);
    app.handle(Event::FilesFound {
        generation: 1,
        paths: vec!["src/somewhere.rs".into()],
    });

    let dump = support::render(&mut app, 60, 12);
    assert!(
        support::text_block(&dump).contains("src/somewhere.rs"),
        "{dump}"
    );
}

#[test]
fn the_selection_wraps_at_both_ends() {
    let mut picker = Picker::new(items(&["a", "b", "c"]), PickerLayout::FullArea);
    assert_eq!(picker.selected(), 0);

    picker.handle_key(&key(KeyCode::Up), PAGE);
    assert_eq!(
        picker.selected(),
        2,
        "up from the first row wraps to the last"
    );

    picker.handle_key(&key(KeyCode::Down), PAGE);
    assert_eq!(
        picker.selected(),
        0,
        "down from the last row wraps to the first"
    );
}

#[test]
fn an_empty_list_accepts_nothing_and_does_not_panic() {
    let mut picker = Picker::new(Vec::new(), PickerLayout::FullArea);
    assert!(matches!(
        picker.handle_key(&key(KeyCode::Enter), PAGE),
        PickerOutcome::Consumed
    ));
    picker.handle_key(&key(KeyCode::Down), PAGE);
    picker.handle_key(&key(KeyCode::Up), PAGE);
    assert_eq!(picker.selected(), 0);
}

/// A selection past the end of a narrowed list would index out of bounds.
#[test]
fn narrowing_the_list_pulls_the_selection_back_into_it() {
    let mut picker = Picker::new(items(&["alpha", "beta", "gamma"]), PickerLayout::FullArea);
    picker.handle_key(&key(KeyCode::Down), PAGE);
    picker.handle_key(&key(KeyCode::Down), PAGE);
    assert_eq!(picker.selected(), 2);

    picker.handle_key(&key(KeyCode::Char('a')), PAGE);
    picker.handle_key(&key(KeyCode::Char('l')), PAGE);
    assert_eq!(picker.match_count(), 1, "only alpha contains a then l");
    assert_eq!(picker.selected(), 0);
}

#[test]
fn backspace_widens_the_list_again() {
    let mut picker = Picker::new(items(&["alpha", "beta"]), PickerLayout::FullArea);
    picker.handle_key(&key(KeyCode::Char('a')), PAGE);
    picker.handle_key(&key(KeyCode::Char('l')), PAGE);
    assert_eq!(picker.match_count(), 1);

    picker.handle_key(&key(KeyCode::Backspace), PAGE);
    assert_eq!(picker.query(), "a");
    assert_eq!(picker.match_count(), 2);
}

/// An empty query keeps the order the caller chose, which is the order it
/// thought worth showing.
#[test]
fn an_empty_query_preserves_the_given_order() {
    let picker = Picker::new(items(&["zebra", "apple", "mango"]), PickerLayout::FullArea);
    let labels: Vec<&str> = picker.matches().map(|item| item.label.as_str()).collect();
    assert_eq!(labels, ["zebra", "apple", "mango"]);
}

#[test]
fn paging_moves_the_selection_by_a_screenful() {
    let mut picker = Picker::new(many(50), PickerLayout::FullArea);

    picker.handle_key(&key(KeyCode::PageDown), PAGE);
    assert_eq!(picker.selected(), usize::from(PAGE));

    picker.handle_key(&key(KeyCode::PageDown), PAGE);
    assert_eq!(picker.selected(), usize::from(PAGE) * 2);

    picker.handle_key(&key(KeyCode::PageUp), PAGE);
    assert_eq!(picker.selected(), usize::from(PAGE));
}

/// A single step wraps, because the other end of a list is faster to reach
/// than to scroll back through. A page does not: paging is how you get to the
/// end of a long list, and a page that wraps past it overshoots the thing you
/// were reaching for.
#[test]
fn paging_stops_at_the_ends_rather_than_wrapping() {
    let mut picker = Picker::new(many(50), PickerLayout::FullArea);

    picker.handle_key(&key(KeyCode::PageUp), PAGE);
    assert_eq!(picker.selected(), 0, "paging up from the first row stays");

    for _ in 0..10 {
        picker.handle_key(&key(KeyCode::PageDown), PAGE);
    }
    assert_eq!(
        picker.selected(),
        49,
        "paging down past the end stops there"
    );
}

#[test]
fn home_and_end_reach_both_ends_at_once() {
    let mut picker = Picker::new(many(50), PickerLayout::FullArea);

    picker.handle_key(&key(KeyCode::End), PAGE);
    assert_eq!(picker.selected(), 49);

    picker.handle_key(&key(KeyCode::Home), PAGE);
    assert_eq!(picker.selected(), 0);
}

#[test]
fn home_and_end_do_not_panic_on_an_empty_list() {
    let mut picker = Picker::new(Vec::new(), PickerLayout::FullArea);
    picker.handle_key(&key(KeyCode::End), PAGE);
    picker.handle_key(&key(KeyCode::Home), PAGE);
    assert_eq!(picker.selected(), 0);
}

/// A list can say what it is about, above its rows.
///
/// For a question the reader did not start: an agent asking to run a
/// command is a question, and three options with no account of what they
/// answer is that question with the words missing. The prompt row holds a
/// line of it; this holds the part that does not fit on a row -- which
/// command, on which file.
#[test]
fn a_list_can_say_what_it_is_about() {
    let mut asking = Picker::new(
        items(&["Allow once", "Reject"]),
        PickerLayout::Compact { rows: 10 },
    );
    assert_eq!(
        asking.about_rows(40),
        0,
        "a list with nothing to say took rows"
    );

    asking.about("cargo test --all-features");
    // The words, and the rule that makes the rows below read as answers
    // rather than as more of the sentence.
    assert_eq!(asking.about_rows(40), 2);
    assert_eq!(
        asking.visible_rows(20, 40),
        4,
        "the block did not grow by what it says"
    );

    // Wrapped at the width it is drawn in, so a narrower screen takes more
    // rows for the same words.
    assert!(
        asking.about_rows(12) > asking.about_rows(40),
        "the words are not wrapped to the room"
    );

    // And capped: an agent explaining itself at length must not push the
    // list it belongs to off the screen.
    let mut wordy = Picker::new(
        items(&["Allow once", "Reject"]),
        PickerLayout::Compact { rows: 10 },
    );
    wordy.about(&"a very long explanation ".repeat(40));
    assert_eq!(wordy.about_rows(40), 6, "the words are not capped");
}

/// The page a key moves by has to be the number of rows actually on screen, or
/// paging moves by not quite a screenful and the list appears to skip.
#[test]
fn a_page_is_the_number_of_rows_on_screen() {
    let full = Picker::new(many(50), PickerLayout::FullArea);
    assert_eq!(
        full.visible_rows(11, 60),
        11,
        "a full-area list takes the room"
    );

    let compact = Picker::new(many(50), PickerLayout::Compact { rows: 10 });
    assert_eq!(compact.visible_rows(11, 60), 10, "capped by the layout");
    assert_eq!(compact.visible_rows(4, 60), 4, "and by the room");

    let short = Picker::new(many(3), PickerLayout::Compact { rows: 10 });
    assert_eq!(short.visible_rows(11, 60), 3, "and by the candidates");
}

/// The rows stay still while the cursor walks through them, and move only
/// when it is against an edge -- by one row, which is the least that puts it
/// back on screen. A window worked out from the selection instead slides the
/// whole list under a cursor that never moves, and a reader loses track of
/// where they are.
#[test]
fn the_list_moves_only_when_the_cursor_reaches_an_edge() {
    let mut app = app();
    press_function(&mut app, 1);
    app.handle(Event::FilesFound {
        generation: 1,
        paths: (0..40)
            .map(|index| format!("file-{index:03}.rs").into())
            .collect(),
    });

    // How many rows this screen shows, asked of the screen rather than
    // assumed: the rule is about the last row, whichever row that is.
    let rows_of = |app: &mut App| {
        let dump = support::render(app, 60, 12);
        support::text_block(&dump)
            .lines()
            .filter_map(|row| {
                let at = row.find("file-")?;
                Some(row[at..at + "file-000.rs".len()].to_string())
            })
            .collect::<Vec<String>>()
    };
    let first = rows_of(&mut app);
    let height = first.len();
    assert!(height > 3, "not enough rows to tell anything apart");
    let top = |rows: &[String]| rows.first().cloned().unwrap_or_default();

    // Down to the last visible row: the rows have not moved.
    for _ in 0..height - 1 {
        press(&mut app, KeyCode::Down);
    }
    let held = rows_of(&mut app);
    assert_eq!(
        top(&held),
        top(&first),
        "the list moved while the cursor was still walking into it"
    );

    // One more, and it moves by exactly one row.
    press(&mut app, KeyCode::Down);
    let moved = rows_of(&mut app);
    assert_eq!(
        top(&moved),
        held[1],
        "the list did not follow the cursor by one row"
    );

    // Back up through the window: still nothing moves until the cursor is
    // on the top row and asked to go further.
    for _ in 0..height - 1 {
        press(&mut app, KeyCode::Up);
    }
    assert_eq!(
        top(&rows_of(&mut app)),
        top(&moved),
        "the list moved on the way back up"
    );
    press(&mut app, KeyCode::Up);
    assert_eq!(
        top(&rows_of(&mut app)),
        top(&first),
        "the list did not follow the cursor back"
    );

    // A query that narrows the list brings the window back inside it: the
    // rows the window was showing may not exist any more.
    press(&mut app, control(KeyCode::End).code);
    type_text(&mut app, "file-03");
    let narrowed = rows_of(&mut app);
    assert!(
        !narrowed.is_empty(),
        "the window is past the end of the narrowed list"
    );
    for _ in 0.."file-03".len() {
        press(&mut app, KeyCode::Backspace);
    }

    // And a wrap around the end brings the window with it: the cursor is on
    // the first row, so the first row has to be on screen.
    press(&mut app, control(KeyCode::End).code);
    press(&mut app, KeyCode::Down);
    assert_eq!(
        top(&rows_of(&mut app)),
        top(&first),
        "wrapping to the top left the window at the bottom"
    );
}

/// The file list says at its foot that it has a key of its own, and `f1`
/// says the rest.
///
/// Only that list: every other one answers to the arrows, enter and escape,
/// and a row saying so would be a row spent on what the reader just did.
#[test]
fn the_file_list_says_what_its_own_key_does() {
    let mut app = app();
    press_function(&mut app, 1);
    let dump = support::render(&mut app, 72, 24);
    assert!(
        support::text_block(&dump).contains("ignored files"),
        "the file list has no foot:\n{dump}"
    );

    press_function(&mut app, 1);
    let card = support::render(&mut app, 72, 24);
    assert!(
        support::text_block(&card).contains("offer the files the tree ignores"),
        "f1 said nothing:\n{card}"
    );
}

/// And no foot over a list that has no key of its own.
#[test]
fn a_list_with_nothing_of_its_own_to_say_says_nothing() {
    let mut app = app();
    press_control(&mut app, 'p');
    let dump = support::render(&mut app, 72, 24);
    assert!(
        !support::text_block(&dump).contains("ignored files"),
        "the palette grew a foot:\n{dump}"
    );
}

/// `alt+i` flips the setting, and the setting is what the walk obeys.
#[test]
fn alt_i_turns_the_ignored_files_on_and_off() {
    let scratch = support::Scratch::new("picker-ignored");
    let file = scratch.join("config.toml");
    let mut app = app();
    app.config_file_for_test(file.clone());
    support::lay_out(&mut app, 72, 24);

    press_function(&mut app, 1);
    assert!(
        !app.config().ignored_files,
        "a tree's ignored files are offered before anybody asked"
    );

    press_alt_key(&mut app, KeyCode::Char('i'));
    assert!(app.config().ignored_files, "the key did nothing");
    let written = std::fs::read_to_string(&file).expect("the settings file");
    assert!(
        written.contains("ignored_files = true"),
        "the setting was not written down: {written}"
    );

    press_alt_key(&mut app, KeyCode::Char('i'));
    assert!(!app.config().ignored_files, "the key only goes one way");
}

/// The foot says which way the key is set, so nobody has to press it to
/// find out.
///
/// Broken deliberately by drawing the key and its word and nothing else:
/// "ignored files" says what `alt+i` is about and not one thing about
/// whether they are being offered, and a switch a reader has to flip to
/// read is not a switch.
#[test]
fn the_foot_says_which_way_the_key_is_set() {
    let scratch = support::Scratch::new("picker-switch");
    let mut app = app();
    app.config_file_for_test(scratch.join("config.toml"));
    support::lay_out(&mut app, 72, 24);
    press_function(&mut app, 1);

    let knob = |app: &mut App| -> usize {
        let dump = support::render(app, 72, 24);
        let row = support::text_block(&dump)
            .lines()
            .find(|row| row.contains("ignored files"))
            .unwrap_or_else(|| panic!("no foot:\n{dump}"))
            .to_string();
        support::column_of(&row, "\u{25a0}")
    };

    let off = knob(&mut app);
    press_alt_key(&mut app, KeyCode::Char('i'));
    let on = knob(&mut app);
    assert!(
        on > off,
        "the switch did not slide: the knob was at {off} and is at {on}"
    );
    press_alt_key(&mut app, KeyCode::Char('i'));
    assert_eq!(knob(&mut app), off, "it did not slide back");
}

/// And on the changed tab it says nothing, because there the key means
/// nothing: those rows are git's answer, and git does not report a file it
/// was told to ignore.
#[test]
fn the_key_is_not_offered_where_it_would_do_nothing() {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.statuses_for_test(
        [(
            std::path::PathBuf::from("src/main.rs"),
            obelus::git::FileStatus::Changed,
        )]
        .into_iter()
        .collect(),
    );
    support::lay_out(&mut app, 72, 24);
    press_function(&mut app, 1);
    let all = support::render(&mut app, 72, 24);
    assert!(
        support::text_block(&all).contains("ignored files"),
        "the key is not offered on the tab it works on:\n{all}"
    );

    press(&mut app, KeyCode::Right);
    let changed = support::render(&mut app, 72, 24);
    assert!(
        !support::text_block(&changed).contains("ignored files"),
        "the foot offers a key that would do nothing:\n{changed}"
    );
}

/// And the walk leaves them out, or does not.
#[test]
fn the_walk_offers_the_ignored_files_only_when_asked() {
    let scratch = support::Scratch::new("picker-walk");
    scratch.write("keep.rs", "fn keep() {}\n");
    scratch.write("skip.rs", "fn skip() {}\n");
    scratch.write(".ignore", "skip.rs\n");

    let found = |ignored: bool, generation: u64| -> Vec<String> {
        let (sender, events) = std::sync::mpsc::channel();
        obelus::component::picker::files::spawn_walk(scratch.path(), generation, ignored, sender);
        let mut names = Vec::new();
        while let Ok(Event::FilesFound { paths, .. }) = events.recv() {
            names.extend(paths.into_iter().map(|path| path.display().to_string()));
        }
        names.sort();
        names
    };

    // `.ignore` itself is not offered either way: it is a hidden file, and
    // hidden is the one thing this key does not change.
    assert_eq!(found(false, 1), vec!["keep.rs"]);
    assert_eq!(found(true, 2), vec!["keep.rs", "skip.rs"]);
}

/// Paging through the list has to bring the rows with it.
#[test]
fn paging_scrolls_the_window() {
    let mut app = app();
    press_function(&mut app, 1);
    app.handle(Event::FilesFound {
        generation: 1,
        paths: (0..40)
            .map(|index| format!("file-{index:03}.rs").into())
            .collect(),
    });

    let first = support::render(&mut app, 60, 12);
    assert!(support::text_block(&first).contains("file-000.rs"));

    press(&mut app, KeyCode::PageDown);
    press(&mut app, KeyCode::PageDown);
    let paged = support::render(&mut app, 60, 12);
    let text = support::text_block(&paged);

    assert!(
        !text.contains("file-000.rs"),
        "the window did not move:\n{paged}"
    );
    // Two screenfuls down, and the selection rides along: the list has eight
    // rows at this size, so the row it lands on is the sixteenth.
    assert!(
        text.contains("file-016.rs"),
        "the selection is off screen:\n{paged}"
    );
}

/// The same keys the editor uses to reach the ends of a document reach the
/// ends of a list. A key should not mean one thing in one view and nothing in
/// the next.
#[test]
fn control_home_and_end_reach_both_ends_of_the_list() {
    let mut picker = Picker::new(many(50), PickerLayout::FullArea);

    picker.handle_key(&control(KeyCode::End), PAGE);
    assert_eq!(picker.selected(), 49);

    picker.handle_key(&control(KeyCode::Home), PAGE);
    assert_eq!(picker.selected(), 0);
}

/// And the plain keys still page, or there would be no way to move a
/// screenful.
#[test]
fn plain_paging_in_a_picker_still_moves_one_screenful() {
    let mut picker = Picker::new(many(50), PickerLayout::FullArea);
    picker.handle_key(&key(KeyCode::PageDown), PAGE);
    assert_eq!(picker.selected(), usize::from(PAGE));
}

#[test]
fn control_home_and_end_do_not_panic_on_an_empty_list() {
    let mut picker = Picker::new(Vec::new(), PickerLayout::FullArea);
    picker.handle_key(&control(KeyCode::End), PAGE);
    picker.handle_key(&control(KeyCode::Home), PAGE);
    assert_eq!(picker.selected(), 0);
}

/// A file's glyph goes in its own two columns before the name, so the query
/// never matches it and the matched characters of the name still line up.
#[test]
fn the_file_picker_shows_a_glyph_for_each_file() {
    let mut app = app();
    press_function(&mut app, 1);
    app.handle(Event::FilesFound {
        generation: 1,
        paths: vec![
            "src/app.rs".into(),
            "Cargo.toml".into(),
            "mystery.qqq".into(),
        ],
    });

    let dump = support::render(&mut app, 40, 8);
    let text = support::text_block(&dump);

    // The Rust glyph, the config glyph, and the generic one.
    assert!(text.contains('\u{e7a8}'), "no Rust glyph:\n{dump}");
    assert!(text.contains('\u{e615}'), "no config glyph:\n{dump}");
    assert!(text.contains('\u{f15b}'), "no generic glyph:\n{dump}");
}

/// And the glyph wears the name's colour. The icon is part of the name: a
/// file git says has changed is a changed file picture and all, and a glyph
/// left in the plain foreground reads as a second thing on the row.
#[test]
fn a_glyph_is_the_colour_of_the_name_beside_it() {
    use obelus::git::FileStatus;

    let mut app = app();
    let root = app.working_directory().to_path_buf();
    // Two statuses, so the colours are two: a test where every row is the
    // same colour cannot tell the name's colour from the plain one.
    app.statuses_for_test(
        [
            (root.join("new.rs"), FileStatus::New),
            (root.join("old.rs"), FileStatus::Changed),
        ]
        .into_iter()
        .collect(),
    );
    support::lay_out(&mut app, 60, 12);
    press_function(&mut app, 1);
    // The changed listing, which is the one whose rows git has coloured.
    press(&mut app, KeyCode::Right);

    let dump = support::render(&mut app, 60, 12);
    let text: Vec<&str> = support::text_block(&dump).lines().collect();
    let styles: Vec<&str> = support::style_block(&dump).lines().collect();

    // Where the glyph is and where the name starts, read off the row itself
    // rather than counted out here: what this is about is the two wearing
    // one colour, not which column either is in.
    let colours = |name: &str| {
        let row = text
            .iter()
            .position(|row| row.contains(name))
            .unwrap_or_else(|| panic!("no row for {name}:\n{dump}"));
        let glyph = text[row]
            .char_indices()
            .find(|(_, character)| ('\u{e000}'..='\u{f8ff}').contains(character))
            .map(|(index, _)| text[row][..index].chars().count())
            .unwrap_or_else(|| panic!("no glyph on the row for {name}:\n{dump}"));
        let label = text[row]
            .find(name)
            .map(|index| text[row][..index].chars().count())
            .expect("the name");
        let at = |column: usize| styles[row].chars().nth(column).unwrap_or(' ');
        (at(glyph), at(label))
    };

    let (new_glyph, new_label) = colours("new.rs");
    let (old_glyph, old_label) = colours("old.rs");
    assert_eq!(
        new_glyph, new_label,
        "the glyph is not the colour of the name beside it:\n{dump}"
    );
    assert_eq!(
        old_glyph, old_label,
        "the glyph is not the colour of the name beside it:\n{dump}"
    );
    assert_ne!(
        new_glyph, old_glyph,
        "both glyphs are one colour, so neither is the name's:\n{dump}"
    );
}

/// The glyph is not in the haystack. Nothing a reader types is a private-use
/// codepoint, and having one in there would only skew the scores.
#[test]
fn a_query_matches_the_name_and_not_the_glyph() {
    let mut app = app();
    press_function(&mut app, 1);
    app.handle(Event::FilesFound {
        generation: 1,
        paths: vec!["src/app.rs".into()],
    });

    type_text(&mut app, "app");
    let dump = support::render(&mut app, 40, 8);
    assert!(support::text_block(&dump).contains("src/app.rs"), "{dump}");
    assert!(
        support::legend_block(&dump).contains("bg=#38577f"),
        "the name's matched characters lost their background:\n{dump}"
    );

    // And a query of the glyph itself matches nothing.
    for _ in 0..3 {
        press(&mut app, KeyCode::Backspace);
    }
    type_text(&mut app, "\u{e7a8}");
    let dump = support::render(&mut app, 40, 8);
    assert!(
        !support::text_block(&dump).contains("src/app.rs"),
        "the glyph was matchable:\n{dump}"
    );
}

/// Commands and themes are not files; a glyph for each would be decoration.
#[test]
fn the_command_palette_has_no_glyphs() {
    let mut app = app();
    press_control(&mut app, 'p');
    // One row taller than the list needs, because the screen keeps one for
    // the rule over the status bar.
    let dump = support::render(&mut app, 60, 13);
    let text = support::text_block(&dump);
    assert!(
        !text
            .chars()
            .any(|character| ('\u{e000}'..='\u{f8ff}').contains(&character)),
        "a private-use codepoint reached the palette:\n{dump}"
    );
}

/// A path too long for the row loses its head, not its tail. The file name is
/// what is being looked for; the directories above it are what is already
/// known.
#[test]
fn a_long_path_keeps_its_end_and_marks_the_cut() {
    let mut app = app();
    press_function(&mut app, 1);
    app.handle(Event::FilesFound {
        generation: 1,
        paths: vec!["a/very/deep/directory/tree/leading/to/the_file.rs".into()],
    });

    let dump = support::render(&mut app, 30, 6);
    let text = support::text_block(&dump);

    assert!(
        text.contains("the_file.rs"),
        "the name was cut off:\n{dump}"
    );
    assert!(text.contains('\u{2026}'), "the cut is not marked:\n{dump}");
    assert!(
        !text.contains("a/very"),
        "the head was kept, so the tail must have been cut:\n{dump}"
    );
}

/// A row keeps the blank column on its right that the layout reserves. An
/// off-by-one in the room left for the label shows up here and nowhere else:
/// writing past the grid is simply dropped, so it cannot be caught by looking
/// for damage.
#[test]
fn a_truncated_row_keeps_the_padding_on_its_right() {
    let mut app = app();
    press_function(&mut app, 1);
    app.handle(Event::FilesFound {
        generation: 1,
        paths: vec!["a/very/deep/directory/tree/leading/to/the_file.rs".into()],
    });

    // ASCII throughout, so one character in the dump is one cell.
    for width in 12..40u16 {
        let dump = support::render(&mut app, width, 6);
        // The one listed item is the first row; at a narrow width even
        // `the_file.rs` has lost its head, so the row cannot be found by name.
        let row = support::text_block(&dump)
            .lines()
            .find(|row| row.starts_with(" 0|"))
            .unwrap_or_else(|| panic!("no first row at width {width}:\n{dump}"));
        assert!(
            row.contains(".rs"),
            "the extension went before the directories did at width {width}:\n{dump}"
        );
        let drawn = &row[3..];
        assert_eq!(
            drawn.chars().count(),
            usize::from(width),
            "the dump should be one character per cell at width {width}"
        );
        // The last column is the scrollbar's, and this list of one row has
        // nowhere to scroll so nothing is drawn on it -- but the column is
        // still reserved, and so is the one before it, which is the row's
        // own padding.
        let reserved: String = drawn.chars().skip(usize::from(width) - 2).collect();
        assert_eq!(
            reserved, "  ",
            "the two reserved columns on the right were written into at width {width}:\n{dump}"
        );
    }
}

/// The matched positions are counted from the start of the whole label, so a
/// truncated head must not shift the colouring along the tail.
#[test]
fn truncation_does_not_move_the_matched_characters() {
    let mut app = app();
    press_function(&mut app, 1);
    app.handle(Event::FilesFound {
        generation: 1,
        paths: vec!["a/very/deep/directory/tree/leading/to/the_file.rs".into()],
    });

    // A query that matches only in the tail, which is the part still on screen.
    type_text(&mut app, "thefile");
    let dump = support::render(&mut app, 30, 6);
    assert!(support::text_block(&dump).contains("the_file.rs"), "{dump}");
    assert!(
        support::legend_block(&dump).contains("bg=#38577f"),
        "the tail's matched characters were not marked:\n{dump}"
    );
}

/// And a query that matches only in the part that was cut away leaves the row
/// listed with nothing coloured, rather than colouring the wrong characters.
#[test]
fn a_match_in_the_cut_away_head_colours_nothing() {
    let mut app = app();
    press_function(&mut app, 1);
    app.handle(Event::FilesFound {
        generation: 1,
        paths: vec!["averydeepdirectory/tree/leading/to/x.rs".into()],
    });

    type_text(&mut app, "averydeep");
    let dump = support::render(&mut app, 24, 6);
    let text = support::text_block(&dump);

    assert!(text.contains("x.rs"), "the row was dropped:\n{dump}");
    assert!(
        text.contains('\u{2026}'),
        "the head is still on screen:\n{dump}"
    );
    assert!(
        !support::legend_block(&dump).contains("bg=#38577f"),
        "characters outside the match were marked:\n{dump}"
    );
}

/// The palette shows what each command is bound to, and shows nothing for a
/// command that is bound to nothing — which is worth being able to see, since
/// the palette is then the only way to reach it.
#[test]
fn the_palette_shows_the_key_each_command_is_bound_to() {
    let mut app = app();
    press_control(&mut app, 'p');
    // Tall enough for the whole list: what is asserted below is about which
    // rows show a key, not about which rows fit.
    let dump = support::render(&mut app, 60, 26);
    let text = support::text_block(&dump);

    let row = |text: &str, needle: &str| {
        text.lines()
            .find(|row| row.contains(needle))
            .map(str::to_string)
            .unwrap_or_else(|| panic!("{needle:?} is not listed:\n{dump}"))
    };

    let label = |command| {
        obelus::keymap::Keymap::new()
            .chord_for(command)
            .map(obelus::keymap::KeyChord::label)
            .expect("the command is bound")
    };

    assert!(
        row(text, "open-file").contains(&label(obelus::command::Command::FileOpen)),
        "{dump}"
    );
    // A command bound to nothing shows nothing. Whatever the label of a bound
    // key looks like, an unbound row cannot hold the one thing every label
    // has, which is a key. Trimmed of the scrollbar and the padding, so what
    // is left is the row's own text.
    let unbound = row(text, "choose-theme");
    let ends_with_description = unbound
        .trim_end_matches(['\u{2502}', '\u{2588}', ' '])
        .ends_with("colours");
    assert!(
        ends_with_description,
        "choose-theme has no binding, so it should show no key:\n{dump}"
    );

    // Past the ten rows a compact list shows, so it is reached the way a
    // reader reaches it: by typing.
    type_text(&mut app, "log");
    let narrowed = support::render(&mut app, 60, 26);
    assert!(
        row(support::text_block(&narrowed), "open-log")
            .trim_end_matches(['\u{2502}', '\u{2588}', ' '])
            .ends_with("log"),
        "a command bound to nothing showed a key:\n{narrowed}"
    );
    for _ in 0..3 {
        press(&mut app, KeyCode::Backspace);
    }

    // More commands than a compact list has rows, so the rest are reached by
    // typing rather than by scrolling.
    type_text(&mut app, "quit");
    let narrowed = support::render(&mut app, 60, 12);
    assert!(
        row(support::text_block(&narrowed), "quit")
            .contains(&label(obelus::command::Command::Quit)),
        "{narrowed}"
    );
}

/// The right-aligned text never takes more than half a row. It used to take
/// whatever it needed and let the label have the rest, which was right while
/// it held a key hint or a line number -- but a search row's trailing is a
/// path, and one longer than the row left the label with no columns at all:
/// a list of icons with nothing beside them.
#[test]
fn a_long_trailing_does_not_eat_the_label() {
    let long = "src/component/picker/very/deep/place/of/its/own/mod.rs:132";
    let mut rows = items(&["pub fn spawn_scan(root: &Path) {"]);
    rows[0].trailing = Some(long.to_string());
    rows[0].icon = Some('\u{f0349}');
    let mut app = app();
    app.open_picker_for_test(rows, PickerLayout::FullArea);

    let dump = support::render(&mut app, 60, 8);
    let row = support::text_block(&dump)
        .lines()
        .find(|row| row.contains('\u{f0349}'))
        .unwrap_or_else(|| panic!("no row:\n{dump}"))
        .to_string();
    assert!(
        row.contains("spawn_scan"),
        "the label was squeezed out by the path:\n{dump}"
    );
    // And the path loses its head, not its tail: the file name and the line
    // are the part that says where to go.
    assert!(
        row.contains("own/mod.rs:132"),
        "the path lost the end that matters:\n{dump}"
    );
    assert!(
        !row.contains("src/component"),
        "a path longer than half the row was drawn whole:\n{dump}"
    );
}

/// Right-aligned, so the keys form a column rather than trailing each
/// description at whatever length it happens to be.
///
/// Their *right* edges, which is what right-aligned means and what the
/// table now makes visible: `f1` is two columns and `ctrl+p` is three, so a
/// list that lined their left edges up would leave the column ragged.
#[test]
fn the_keys_line_up_in_a_column() {
    use obelus::{
        command::Command,
        keymap::{KeyChord, Keymap},
    };

    let mut app = app();
    press_control(&mut app, 'p');
    // One row taller than the list needs, because the screen keeps one for
    // the rule over the status bar.
    let dump = support::render(&mut app, 60, 13);
    let text = support::text_block(&dump);

    // Where the row's own key ends, in characters: a glyph is four bytes
    // and one column.
    let ends_at = |name: &str, command: Command| {
        let key = Keymap::new()
            .chord_for(command)
            .map(KeyChord::label)
            .unwrap_or_else(|| panic!("{name} is not bound"));
        text.lines()
            .find(|row| row.contains(name))
            .and_then(|row| {
                row.rfind(&key)
                    .map(|byte| row[..byte].chars().count() + key.chars().count())
            })
            .unwrap_or_else(|| panic!("no {key:?} on the {name:?} row:\n{dump}"))
    };

    let file = ends_at("open-file", Command::FileOpen);
    assert_eq!(
        file,
        ends_at("run-command", Command::CommandPalette),
        "a function key and a chord do not end in the same column:\n{dump}"
    );
    // Rows within the compact list's ten. There are more commands than that
    // now, and the ones past it are reached by typing rather than scrolling.
    assert_eq!(file, ends_at("reload-file", Command::FileReload), "{dump}");
}

/// The keys come from the key table, like the welcome screen's. A rebound key
/// changes the palette rather than leaving it lying.
#[test]
fn the_palette_reads_the_key_table() {
    use crossterm::event::KeyModifiers;
    use obelus::{
        command::Command,
        keymap::{Binding, Context, KeyChord, Keymap},
    };

    let mut app = app();
    app.set_keymap(Keymap::from_bindings(vec![Binding {
        command: Command::CommandPalette,
        context: Context::Normal,
        chord: KeyChord::new(KeyCode::Char('k'), KeyModifiers::ALT),
    }]));

    // The palette can still be opened by the command it is now bound to.
    app.handle(Event::Key(crossterm::event::KeyEvent::new(
        KeyCode::Char('k'),
        KeyModifiers::ALT,
    )));
    // Tall enough to reach the palette's own row: the list is in the order
    // of the command table, and a command added above it pushes it down.
    let dump = support::render(&mut app, 60, 20);
    let text = support::text_block(&dump);

    let rebound = KeyChord::new(KeyCode::Char('k'), KeyModifiers::ALT).label();
    let old = KeyChord::new(KeyCode::Char('p'), KeyModifiers::CONTROL).label();
    assert!(
        text.contains(&rebound),
        "the rebound key is not shown:\n{dump}"
    );
    assert!(!text.contains(&old), "the old key is still shown:\n{dump}");
    assert!(
        !text.contains(&KeyChord::new(KeyCode::Char('f'), KeyModifiers::CONTROL).label()),
        "a command with no binding left still shows one:\n{dump}"
    );
}

/// The key's room comes out of the label's before the label is truncated, so
/// the one part of a row that never gets cut is the key.
#[test]
fn a_long_label_never_overlaps_the_key() {
    let mut app = app();
    press_control(&mut app, 'p');

    for width in 24..60u16 {
        let dump = support::render(&mut app, width, 12);
        let text = support::text_block(&dump);
        let Some(row) = text.lines().find(|row| row.contains("ctrl+f")) else {
            continue;
        };
        let key = row.rfind("ctrl+f").expect("just found it");
        let before = &row[..key];
        assert!(
            before.ends_with("  ") || before.ends_with('|'),
            "at width {width} the label runs into the key:\n{dump}"
        );
    }
}

/// The theme picker is a short, fixed list, so it hugs the status bar like the
/// palette does. Covering the code to offer two choices would be the wrong
/// trade: the reason to change theme is usually the code you are looking at.
#[test]
fn the_theme_picker_leaves_the_code_visible() {
    let mut app = app();
    press_control(&mut app, 'p');
    type_text(&mut app, "theme");
    press(&mut app, KeyCode::Enter);

    let dump = support::render(&mut app, 60, 12);
    let text = support::text_block(&dump);

    assert!(text.contains("dark") && text.contains("light"), "{dump}");
    assert!(
        text.contains("fn main()"),
        "the code was covered by two rows of choices:\n{dump}"
    );
    support::check("themes_60x12", &dump);
}

/// A question no server can answer is dim rather than gone, and keeps the
/// key it is bound to. The row is plainly unavailable, so the key reads as
/// "this is how, when there is a server" -- which is what a reader wants to
/// know before there is one.
#[test]
fn a_question_with_no_server_is_dim_and_keeps_its_key() {
    use crossterm::event::KeyModifiers;
    use obelus::{
        command::Command,
        keymap::{Binding, Context, KeyChord, Keymap},
    };

    let mut app = app();
    app.set_keymap(Keymap::from_bindings(vec![
        // The palette's own key has to be in the table too: replacing the
        // table replaces all of it.
        Binding {
            command: Command::CommandPalette,
            context: Context::Normal,
            chord: KeyChord::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
        },
        Binding {
            command: Command::SymbolDefinition,
            context: Context::Normal,
            chord: KeyChord::new(KeyCode::Char('d'), KeyModifiers::ALT),
        },
    ]));

    press_control(&mut app, 'p');
    type_text(&mut app, "definition");
    let palette = support::render(&mut app, 60, 12);
    let row = app
        .picker()
        .expect("the palette is open")
        .matches()
        .find(|item| item.label == "go-to-definition")
        .expect("the row is listed whether or not it can run");
    assert!(
        !row.enabled,
        "a question with no server to answer it can be chosen:\n{palette}"
    );
    // Its key is still shown: the row is plainly dim, so the key reads as
    // "this is how, when there is a server" rather than as a promise.
    assert_eq!(
        row.trailing.as_deref(),
        Some(
            obelus::keymap::KeyChord::new(KeyCode::Char('d'), KeyModifiers::ALT)
                .label()
                .as_str()
        ),
        "the key it is bound to is not shown:\n{palette}"
    );
}

/// The whole rule, in one place: every command is listed, and one that
/// cannot do its job here is dim and cannot be chosen. A row that silently
/// fails is worse than no row at all -- but a list that hides what it cannot
/// do cannot be learned from, and a reader who never sees `show-change` does
/// not find out obelus has it.
///
/// With a plain Rust file open and no server, so what is dim is everything
/// needing a server, a selection, a bracket, a history, or markdown.
#[test]
fn the_palette_lists_everything_and_dims_what_cannot_run() {
    let mut app = app();
    support::lay_out(&mut app, 60, 12);
    press_control(&mut app, 'p');
    let rows: Vec<(String, bool)> = app
        .picker()
        .expect("the palette is open")
        .matches()
        .map(|item| (item.label.clone(), item.enabled))
        .collect();
    let listed = |name: &str| {
        rows.iter()
            .find(|(label, _)| label == name)
            .map(|(_, enabled)| *enabled)
    };

    // Every command obelus has, whatever it can do here.
    assert_eq!(
        rows.len(),
        obelus::command::ALL.len(),
        "the palette is not the whole command table: {rows:?}"
    );

    // Named here rather than taken from `requires`, which is the rule under
    // test: asking the rule what it expects makes the assertion agree with
    // itself whatever the rule says.
    for name in [
        "go-to-definition",
        "go-to-type-definition",
        "go-to-implementation",
        "find-references",
        "stop-server",
        // Not `copy-selection` or `cut-selection`: with nothing selected
        // they are about the line the cursor is on, so a file being open is
        // all they need.
        "clear-selection",
        "go-to-bracket",
        "go-back",
        "go-forward",
        "toggle-preview",
    ] {
        assert_eq!(
            listed(name),
            Some(false),
            "{name} can be chosen with nothing for it to do"
        );
    }

    // And everything else can be, including the two that are worth pressing
    // precisely when nothing is running.
    for name in [
        "open-file",
        "reload-file",
        "switch-file",
        "close-file",
        "choose-theme",
        "ask-about-symbol",
        "show-outline",
        "go-to-line",
        "open-log",
        "open-server-log",
        "restart-server",
        "quit",
    ] {
        assert_eq!(listed(name), Some(true), "{name} cannot be chosen");
    }
}

/// The selection walks past what cannot be chosen, and Enter on a dim row
/// does nothing: the rows are there to be read, not pressed.
#[test]
fn the_selection_walks_past_what_cannot_be_chosen() {
    let mut app = App::new(Vec::new());
    support::lay_out(&mut app, 60, 14);
    press_control(&mut app, 'p');

    // With nothing open, `reload-file` is the second row and cannot run, so
    // the selection has to be somewhere else.
    let chosen = |app: &App| {
        app.picker()
            .expect("the palette")
            .selected_item()
            .map(|item| (item.label.clone(), item.enabled))
    };
    assert_eq!(chosen(&app), Some(("open-file".to_string(), true)));
    press(&mut app, KeyCode::Down);
    let (label, enabled) = chosen(&app).expect("a row");
    assert!(enabled, "the selection landed on {label}, which cannot run");
    assert_ne!(label, "reload-file", "the selection stopped on a dim row");

    // Every row it walks through, going down and coming back, can be chosen.
    for _ in 0..30 {
        press(&mut app, KeyCode::Down);
        assert!(chosen(&app).expect("a row").1, "walked onto a dim row");
    }
    for _ in 0..30 {
        press(&mut app, KeyCode::Up);
        assert!(chosen(&app).expect("a row").1, "walked onto a dim row");
    }

    // A query narrows the list under the selection, and the selection comes
    // to rest on a row that can be chosen: "sec" matches the searches, the
    // selection commands and the project's history, and some of those can
    // run with nothing open while others cannot.
    type_text(&mut app, "sec");
    let (label, enabled) = chosen(&app).expect("a row");
    assert!(
        enabled,
        "the selection stayed on {label}, which cannot be chosen"
    );
    // Both ends of the narrowed list are dim rows, and neither end key
    // lands on one.
    press_control_key(&mut app, KeyCode::Home);
    assert!(chosen(&app).expect("a row").1, "home landed on a dim row");
    press_control_key(&mut app, KeyCode::End);
    assert!(chosen(&app).expect("a row").1, "end landed on a dim row");
    for _ in 0.."sec".len() {
        press(&mut app, KeyCode::Backspace);
    }

    // And a query that leaves only dim rows: Enter does nothing rather than
    // running something that reports why it could not.
    type_text(&mut app, "selec");
    let only_dim: Vec<String> = app
        .picker()
        .expect("the palette")
        .matches()
        .filter(|item| !item.enabled)
        .map(|item| item.label.clone())
        .collect();
    assert_eq!(
        only_dim.len(),
        5,
        "not the five selection commands: {only_dim:?}"
    );
    press(&mut app, KeyCode::Enter);
    assert!(
        app.picker().is_some(),
        "Enter chose a row that cannot be chosen"
    );
}

/// A key whose command the palette will not offer does nothing at all.
///
/// The palette draws such a row dim and refuses enter on it; a key is the
/// same row reached another way, and one judgement has to answer for both.
/// Before this, `f2` on a screen with no file open drew an empty list of
/// open files, and `f3` on a clean tree wrote "nothing has changed" across
/// the status row -- two answers to a question the palette had already said
/// could not be asked.
#[test]
fn a_key_does_nothing_where_its_command_is_dim() {
    use obelus::command::Command;

    // Nothing open, and a tree with nothing changed in it.
    let mut empty = App::new(Vec::new());
    empty.statuses_for_test(std::collections::HashMap::new());
    support::lay_out(&mut empty, 60, 12);

    for (key, command) in [(2u8, Command::BufferList), (3, Command::FileChanged)] {
        assert!(
            !empty.offers(command),
            "{} can run here, so this tests nothing",
            command.name()
        );
        press_function(&mut empty, key);
        assert!(
            empty.picker().is_none(),
            "f{key} opened a list its command was too dim to open"
        );
        assert_eq!(empty.note(), None, "f{key} said something instead");
    }

    // And the keys whose commands *can* run still work, or the rule would
    // have turned the table off.
    press_function(&mut empty, 1);
    assert!(empty.picker().is_some(), "f1 stopped opening the file list");
    press(&mut empty, KeyCode::Esc);

    // With a file open, the list of open files is offered again -- and its
    // key works again with it.
    let mut reading = app();
    support::lay_out(&mut reading, 60, 12);
    assert!(reading.offers(Command::BufferList));
    press_function(&mut reading, 2);
    assert!(
        reading.picker().is_some(),
        "f2 did nothing with a file open to list"
    );
}

/// And each condition turns its command back on when it is met. The palette
/// is rebuilt every time it opens, so what it lists is the answer to "what
/// can I do *now*".
#[test]
fn a_condition_met_puts_its_command_back() {
    let offered = |app: &App, name: &str| {
        app.picker()
            .expect("the palette is open")
            .matches()
            .any(|item| item.label == name && item.enabled)
    };

    // Something selected.
    let mut selecting = app();
    support::lay_out(&mut selecting, 60, 12);
    support::press_shift(&mut selecting, KeyCode::Right);
    press_control(&mut selecting, 'p');
    assert!(
        offered(&selecting, "copy-selection"),
        "a selection was not noticed"
    );
    assert!(offered(&selecting, "clear-selection"));

    // A bracket under the cursor: `sample.rs` line one is `fn main() {`.
    let mut bracket = app();
    support::lay_out(&mut bracket, 60, 12);
    for _ in 0..7 {
        press(&mut bracket, KeyCode::Right);
    }
    press_control(&mut bracket, 'p');
    assert!(
        offered(&bracket, "go-to-bracket"),
        "a bracket was not noticed"
    );

    // Somewhere to go back to.
    let mut jumped = app();
    support::lay_out(&mut jumped, 60, 12);
    press_control(&mut jumped, 'l');
    type_text(&mut jumped, "3");
    press(&mut jumped, KeyCode::Enter);
    press_control(&mut jumped, 'p');
    assert!(offered(&jumped, "go-back"), "a jump was not noticed");
    assert!(
        !offered(&jumped, "go-forward"),
        "nothing is in front until something goes back"
    );

    // A markdown file.
    let mut markdown = App::new(vec![support::open_fixture("sample.md")]);
    support::lay_out(&mut markdown, 60, 12);
    press_control(&mut markdown, 'p');
    assert!(
        offered(&markdown, "toggle-preview"),
        "a .md file was not noticed"
    );

    // And nothing open at all leaves only what works with nothing open.
    let mut empty = App::new(Vec::new());
    support::lay_out(&mut empty, 60, 12);
    press_control(&mut empty, 'p');
    assert!(offered(&empty, "open-file"));
    assert!(!offered(&empty, "reload-file"), "reloading what?");
    assert!(!offered(&empty, "go-to-line"), "into which file?");
    assert!(!offered(&empty, "show-outline"), "of what?");
}

/// Nothing to ask, so no menu: the reason goes on the status bar. A list with
/// one row explaining itself is still a list -- it covers the code, it has to
/// be dismissed, and it offers nothing.
#[test]
fn nothing_to_ask_means_a_note_and_no_menu() {
    let mut app = app();
    support::lay_out(&mut app, 60, 12);
    press_alt_key(&mut app, KeyCode::Enter);

    assert!(
        app.picker().is_none(),
        "a menu opened with nothing in it to choose"
    );
    let dump = support::render(&mut app, 60, 12);
    let text = support::text_block(&dump);
    assert!(
        text.contains("no symbol here")
            || text.contains("not installed")
            || text.contains("no server running")
            || text.contains("still starting")
            || text.contains("no language server"),
        "no reason anywhere:\n{dump}"
    );
    // The code is still on screen, which is the point of not opening a list.
    assert!(text.contains("fn main"), "the code was covered:\n{dump}");
}

/// The first of the reasons: the cursor is not on a name. Every question in
/// the menu is about the thing under the cursor, and on a bracket or a blank
/// line there is no thing.
#[test]
fn the_menu_refuses_a_cursor_that_is_not_on_a_name() {
    let mut brace = app();
    support::lay_out(&mut brace, 60, 12);
    // `sample.rs` line one is `fn main() {`; the end of it is the brace.
    press(&mut brace, KeyCode::End);
    press_alt_key(&mut brace, KeyCode::Enter);

    assert!(brace.picker().is_none());
    assert_eq!(
        brace.note(),
        Some("no symbol here"),
        "the reason was not the one about the cursor"
    );

    // And a blank line, where the smallest node covering the cursor is the
    // file itself -- whose text does start with a letter, so it is the
    // leaf-ness that refuses this one and not the first character.
    let mut blank = app();
    support::lay_out(&mut blank, 60, 12);
    for _ in 0..4 {
        press(&mut blank, KeyCode::Down);
    }
    press_alt_key(&mut blank, KeyCode::Enter);
    assert!(blank.picker().is_none());
    assert_eq!(blank.note(), Some("no symbol here"), "on a blank line");
}

/// The file picker shows what the selection names, below the list, drawn by
/// the editor's own view.
#[test]
fn the_file_picker_previews_the_selected_file() {
    let mut app = app();
    press_function(&mut app, 1);
    app.handle(Event::FilesFound {
        generation: 1,
        paths: vec![
            "tests/fixtures/sample.rs".into(),
            "tests/fixtures/long.rs".into(),
        ],
    });

    support::check("preview_60x22", &support::render(&mut app, 60, 22));
}

/// Moving the selection changes what is previewed. Reading the file once and
/// keeping it would show the first row's file for the whole list.
#[test]
fn the_preview_follows_the_selection() {
    let mut app = app();
    press_function(&mut app, 1);
    app.handle(Event::FilesFound {
        generation: 1,
        paths: vec![
            "tests/fixtures/sample.rs".into(),
            "tests/fixtures/long.rs".into(),
        ],
    });

    let first = support::text_block(&support::render(&mut app, 60, 22)).to_string();
    assert!(first.contains("fn main()"), "{first}");
    assert!(!first.contains("fn before()"), "{first}");

    press(&mut app, KeyCode::Down);
    let second = support::text_block(&support::render(&mut app, 60, 22)).to_string();
    assert!(
        second.contains("fn before()"),
        "the preview did not follow the selection:\n{second}"
    );
}

/// A palette of commands has an edge above it, because it sits on top of the
/// code, and nothing below it, because there is nothing to preview. Two rules
/// would mean a border round a preview that does not exist.
#[test]
fn the_palette_has_an_edge_above_it_and_no_preview_below() {
    let mut app = app();
    press_control(&mut app, 'p');
    let dump = support::render(&mut app, 60, 23);
    let rows: Vec<&str> = support::text_block(&dump)
        .lines()
        .filter(|row| !row.is_empty())
        .collect();

    let rules: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.contains('\u{2500}'))
        .map(|(index, _)| index)
        .collect();
    // Three: the edge of the whole block, the one under the tabs, and the
    // one over the row the reader types in -- which is the screen's own,
    // between whatever is showing and the status bar. What there is not is
    // a fourth, below the list, which would be a border round a preview
    // that does not exist.
    assert_eq!(rules.len(), 3, "not the three expected rules:\n{dump}");

    let rule = rules[0];
    assert!(
        rows[rule + 1].contains("all"),
        "the tabs are not directly under the block's edge:\n{dump}"
    );
    assert_eq!(
        rules[1],
        rule + 2,
        "the tabs have no rule under them:\n{dump}"
    );
    assert!(
        rows[rules[1] + 1].contains("open-file"),
        "the list does not start under the tabs' rule:\n{dump}"
    );
    assert!(
        rows[..rule].iter().any(|row| row.contains("fn main")),
        "the code above it is gone, so the list is not compact:\n{dump}"
    );
}

/// A preview that leaves three candidates showing has stopped being a preview
/// and become the thing in the way of the list.
#[test]
fn a_short_screen_gets_the_list_and_no_preview() {
    let mut app = app();
    press_function(&mut app, 1);
    app.handle(Event::FilesFound {
        generation: 1,
        paths: vec!["tests/fixtures/sample.rs".into()],
    });

    let dump = support::render(&mut app, 60, 10);
    let text = support::text_block(&dump);
    assert!(text.contains("sample.rs"), "the list went missing:\n{dump}");
    assert!(
        !text.contains("fn main()"),
        "a preview was squeezed into a screen with no room:\n{dump}"
    );
}

/// A row that names a file is previewed from its top; a row that names a
/// *place* stops two lines above it, so the line in question has something to
/// be read in the context of.
///
/// The place half of that is exercised by the references list in the next
/// milestone. What can be pinned here is that a plain file starts at line one
/// rather than wherever a previous preview happened to be looking.
#[test]
fn a_file_is_previewed_from_its_first_line() {
    let mut app = app();
    press_function(&mut app, 1);
    // Neither of these is the file being read, so the row that starts
    // selected is the first one rather than the one the picker opens on.
    app.handle(Event::FilesFound {
        generation: 1,
        paths: vec![
            "tests/fixtures/long.rs".into(),
            "tests/fixtures/indented.rs".into(),
        ],
    });

    let dump = support::render(&mut app, 60, 22);
    let text = support::text_block(&dump);
    assert!(text.contains("fn before()"), "{dump}");
    assert!(
        text.contains("  1 fn before()"),
        "the preview did not start at the first line:\n{dump}"
    );
}

/// A preview is set aside for a list that said its rows name something to
/// show, and not for one that merely takes the whole region.
///
/// Those were the same seven lists, which is why the layout stood in for the
/// question -- two sets that happen to coincide, with nothing making them.
/// The next full-area list of commands or settings would have given up half
/// the screen to show nothing in it.
#[test]
fn a_full_area_list_previews_only_if_it_said_it_would() {
    use obelus::{
        component::picker::{Picker, PickerLayout},
        ui::picker::preview_region,
    };
    use ratatui::layout::Rect;

    let editor = Rect::new(0, 0, 80, 30);
    let quiet = Picker::new(items(&["a", "b", "c"]), PickerLayout::FullArea);
    assert!(
        preview_region(Some(&quiet), editor).is_none(),
        "a list that never asked for a preview was given room for one"
    );

    let mut shows = Picker::new(items(&["a", "b", "c"]), PickerLayout::FullArea);
    shows.previews();
    assert!(
        preview_region(Some(&shows), editor).is_some(),
        "a list that asked for a preview was given nowhere to put it"
    );

    // And asking is not enough on a layout with nowhere to put one: a compact
    // list sits on the status bar with nothing under it.
    let mut compact = Picker::new(items(&["a"]), PickerLayout::Compact { rows: 10 });
    compact.previews();
    assert!(
        preview_region(Some(&compact), editor).is_none(),
        "a compact list was given a region below rows that end the screen"
    );
}

/// A list of references is read by looking at the symbol in each one, so the
/// preview marks it. Saying only which line leaves the reader finding it
/// again on every row.
#[test]
fn a_place_preview_marks_the_symbol_it_is_about() {
    use obelus::component::picker::{PickerItem, PickerLayout, PickerValue};

    let mut app = app();
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/long.rs");
    // Line 1 of `long.rs` is the long `const NAMES` line; `NAMES` is at
    // characters six to eleven.
    app.open_picker_for_test(
        vec![PickerItem {
            prose: false,
            marker: None,
            icon: None,
            label: "long.rs:2:7".to_string(),
            detail: None,
            trailing: None,
            changed: None,
            value: PickerValue::Place {
                path,
                line: 1,
                character: 6,
                end_line: 1,
                end_character: 11,
            },
            enabled: true,
            colours: None,
            status: None,
            depth: 0,
            kind: None,
            tab: None,
        }],
        PickerLayout::FullArea,
    );

    let dump = support::render(&mut app, 60, 22);
    assert!(
        support::legend_block(&dump).contains("#1e3a5f"),
        "the symbol was not marked:\n{dump}"
    );
    support::check("preview_marked_60x22", &dump);
}

/// The list takes its ten rows and the preview takes the rest, so a taller
/// terminal buys more of the file rather than more file names. The list is
/// filtered by typing; the preview is not.
#[test]
fn a_taller_screen_gives_the_extra_rows_to_the_preview() {
    let mut app = app();
    press_function(&mut app, 1);
    // More names than either list has room for, and the first of them is a
    // file with more lines than either preview has room for: both counts
    // are then the room rather than what there is to show.
    let mut paths: Vec<std::path::PathBuf> = vec!["tests/fixtures/many_lines.rs".into()];
    paths.extend((1..=30).map(|number| format!("src/other{number:02}.rs").into()));
    app.handle(Event::FilesFound {
        generation: 1,
        paths,
    });

    let shown = |dump: &str| {
        support::text_block(dump)
            .lines()
            .filter(|row| row.contains("pub const LINE_"))
            .count()
    };

    let short = support::render(&mut app, 60, 20);
    let tall = support::render(&mut app, 60, 34);
    assert!(
        shown(&tall) > shown(&short),
        "the taller screen showed no more of the file:\n{tall}"
    );
    // The list stayed the same size while the preview grew, which is the
    // point: a taller terminal buys more of the file, not more file names.
    let rows_of_list = |dump: &str| {
        support::text_block(dump)
            .lines()
            .filter(|row| row.contains(".rs"))
            .count()
    };
    assert_eq!(rows_of_list(&short), rows_of_list(&tall));
    assert_eq!(rows_of_list(&tall), 10, "the list is not ten rows:\n{tall}");

    support::check("preview_tall_60x34", &tall);
}

/// Ten rows to walk, tabs or no tabs.
///
/// A list with tabs spends its first two rows on them -- the tabs and the
/// rule under them -- so a region ten rows tall drew eight names while the
/// window and the paging were told ten. What that looked like was a list
/// that scrolled a row before the last one on screen.
#[test]
fn a_list_with_tabs_still_walks_ten_rows() {
    use obelus::git::FileStatus;

    let mut app = app();
    let root = app.working_directory().to_path_buf();
    app.statuses_for_test(
        [(
            root.join("tests/fixtures/many_lines.rs"),
            FileStatus::Changed,
        )]
        .into_iter()
        .collect(),
    );
    press_function(&mut app, 1);
    let mut paths: Vec<std::path::PathBuf> = vec!["tests/fixtures/many_lines.rs".into()];
    paths.extend((1..=30).map(|number| format!("src/other{number:02}.rs").into()));
    app.handle(Event::FilesFound {
        generation: 1,
        paths,
    });

    let names = |dump: &str| {
        support::text_block(dump)
            .lines()
            .filter(|row| row.contains(".rs"))
            .count()
    };
    let dump = support::render(&mut app, 60, 34);
    assert!(
        dump.contains("changed"),
        "this list has no tabs, so it tests nothing:\n{dump}"
    );
    assert_eq!(names(&dump), 10, "the list is not ten rows:\n{dump}");

    // Nine steps stay inside those ten rows, so the top of the list has not
    // moved; the tenth is the row it scrolls on.
    let first = |dump: &str| {
        support::text_block(dump)
            .lines()
            .find(|row| row.contains(".rs"))
            .expect("a row")
            .to_string()
    };
    for _ in 0..9 {
        press(&mut app, KeyCode::Down);
    }
    let walked = support::render(&mut app, 60, 34);
    assert_eq!(
        first(&walked),
        first(&dump),
        "the list scrolled before the last row on screen:\n{walked}"
    );
    press(&mut app, KeyCode::Down);
    let scrolled = support::render(&mut app, 60, 34);
    assert_ne!(
        first(&scrolled),
        first(&dump),
        "the list did not scroll at the last row on screen:\n{scrolled}"
    );
    assert_eq!(names(&scrolled), 10, "the list lost a row:\n{scrolled}");

    // And a page is those ten rows, not the region they are drawn in: a
    // page of twelve would step past two rows the reader never saw. Under
    // control, because bare it pages the preview under the list.
    support::press_control_key(&mut app, KeyCode::Home);
    support::press_control_key(&mut app, KeyCode::PageDown);
    assert_eq!(
        app.picker()
            .and_then(|picker| picker.selected_item())
            .map(|item| item.label.as_str()),
        Some("src/other10.rs"),
        "a page is not the ten rows on screen"
    );
}

/// The paging keys scroll the preview, not the list: a screenful is what the
/// thing being *read* moves by, and the list above it is ten rows walked one
/// at a time with its ends a keypress away. Reading a candidate and choosing
/// between candidates are different jobs, and a list of references is read by
/// doing both at once.
#[test]
fn paging_scrolls_the_preview_and_not_the_list() {
    let mut app = app();
    press_function(&mut app, 1);
    app.handle(Event::FilesFound {
        generation: 1,
        paths: vec!["tests/fixtures/many_lines.rs".into()],
    });

    let at_rest = support::render(&mut app, 60, 22);
    assert!(
        support::text_block(&at_rest).contains("LINE_01"),
        "{at_rest}"
    );

    press(&mut app, KeyCode::PageDown);
    let scrolled = support::render(&mut app, 60, 22);
    let text = support::text_block(&scrolled);
    assert!(
        !text.contains("LINE_01"),
        "the preview did not move:\n{scrolled}"
    );
    assert!(
        text.contains("tests/fixtures/many_lines.rs"),
        "the list moved instead:\n{scrolled}"
    );

    support::check("preview_scrolled_60x22", &scrolled);
    assert_eq!(
        support::text_block(&support::render(&mut app, 60, 22)),
        support::text_block(&scrolled),
        "the scroll did not survive the next frame"
    );

    press(&mut app, KeyCode::PageUp);
    assert_eq!(
        support::text_block(&support::render(&mut app, 60, 22)),
        support::text_block(&at_rest),
        "scrolling back did not come back"
    );
}

/// A list with nothing under it keeps the keys: a compact list has no
/// preview, so there is nothing for a paging key to move but the list, and a
/// key that did nothing there would be a key that stopped working when the
/// reader opened a different kind of list.
#[test]
fn a_list_with_no_preview_still_pages_itself() {
    let mut app = app();
    support::lay_out(&mut app, 60, 22);
    press_control(&mut app, 'p');
    let first = app
        .picker()
        .and_then(|picker| picker.selected_item())
        .map(|item| item.label.clone())
        .expect("a row");

    press(&mut app, KeyCode::PageDown);
    let paged = app
        .picker()
        .and_then(|picker| picker.selected_item())
        .map(|item| item.label.clone())
        .expect("a row");
    assert_ne!(
        paged, first,
        "the palette did not page, and it has no preview to have paged instead"
    );
}

/// The top of the file is as far up as it goes, and pressing past it does not
/// bank a debt: one press the other way moves. Letting the offset run past the
/// top spends the next several presses coming back with nothing happening.
#[test]
fn the_preview_stops_at_the_top_of_the_file() {
    let mut app = app();
    press_function(&mut app, 1);
    app.handle(Event::FilesFound {
        generation: 1,
        paths: vec!["tests/fixtures/many_lines.rs".into()],
    });

    let at_rest = support::text_block(&support::render(&mut app, 60, 22)).to_string();
    for _ in 0..5 {
        press(&mut app, KeyCode::PageUp);
    }
    assert_eq!(
        support::text_block(&support::render(&mut app, 60, 22)),
        at_rest,
        "the top of the file is not where it stopped"
    );

    press(&mut app, KeyCode::PageDown);
    let after = support::render(&mut app, 60, 22);
    assert!(
        !support::text_block(&after).contains("LINE_01"),
        "the presses above the top were still being paid back:\n{after}"
    );
}

/// Moving to a different row is a different subject, so whatever was scrolled
/// to belongs to the row that was left. Two rows in the same file, because
/// that is the case the file's identity cannot answer -- and it is the common
/// one in a list of references.
#[test]
fn moving_the_selection_forgets_the_scrolling() {
    use obelus::component::picker::{PickerItem, PickerLayout, PickerValue};

    let path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/many_lines.rs");
    let place = |line: u32| PickerItem {
        prose: false,
        marker: None,
        icon: None,
        label: format!("many_lines.rs:{}", line + 1),
        detail: None,
        trailing: None,
        changed: None,
        value: PickerValue::Place {
            path: path.clone(),
            line,
            character: 10,
            end_line: line,
            end_character: 17,
        },
        enabled: true,
        colours: None,
        status: None,
        depth: 0,
        kind: None,
        tab: None,
    };

    let mut app = app();
    app.open_picker_for_test(vec![place(1), place(2)], PickerLayout::FullArea);

    assert!(support::text_block(&support::render(&mut app, 60, 22)).contains("LINE_01"));
    press(&mut app, KeyCode::PageDown);
    assert!(!support::text_block(&support::render(&mut app, 60, 22)).contains("LINE_01"));

    press(&mut app, KeyCode::Down);
    let moved = support::render(&mut app, 60, 22);
    assert!(
        support::text_block(&moved).contains("LINE_01"),
        "the row that was selected kept the scrolling of the row that was left:\n{moved}"
    );
}

/// The picker judges modifiers the way the key table does, so a chord obelus
/// has no name for is not text, not a motion, and not a selection — it falls
/// through. A picker that typed `super+f` into the prompt would be the same
/// bug as one that paged on `ctrl+pagedown`.
#[test]
fn a_key_with_an_unknown_modifier_falls_through() {
    use crossterm::event::{KeyEvent, KeyModifiers};

    let mut picker = Picker::new(many(20), PickerLayout::FullArea);
    picker.handle_key(&key(KeyCode::Down), PAGE);
    let before = picker.selected_item().map(|item| item.label.clone());

    for (code, modifier) in [
        (KeyCode::Char('f'), KeyModifiers::SUPER),
        (
            KeyCode::Char('f'),
            KeyModifiers::CONTROL | KeyModifiers::HYPER,
        ),
        (KeyCode::Down, KeyModifiers::SUPER),
        (
            KeyCode::PageDown,
            KeyModifiers::CONTROL | KeyModifiers::META,
        ),
        (KeyCode::Esc, KeyModifiers::SUPER),
        (KeyCode::Enter, KeyModifiers::SUPER),
        (KeyCode::Backspace, KeyModifiers::SUPER),
        (KeyCode::Home, KeyModifiers::CONTROL | KeyModifiers::SUPER),
    ] {
        let event = KeyEvent::new(code, modifier);
        assert!(
            matches!(picker.handle_key(&event, PAGE), PickerOutcome::Ignored),
            "{code:?} with {modifier:?} was taken"
        );
    }

    assert_eq!(
        picker.selected_item().map(|item| item.label.clone()),
        before,
        "the selection moved"
    );
    assert_eq!(picker.query(), "", "something reached the prompt");
}

/// A place in the middle of a file is previewed in the middle of the preview.
/// The rows above a definition -- its signature, its doc comment -- are what
/// the reader is looking for, and putting the line at the top spends half the
/// room on the half nobody asked for.
#[test]
fn a_place_in_the_middle_of_a_file_is_previewed_in_the_middle() {
    use obelus::component::picker::{PickerItem, PickerLayout, PickerValue};

    let path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/many_lines.rs");
    let mut app = app();
    app.open_picker_for_test(
        vec![PickerItem {
            prose: false,
            marker: None,
            icon: None,
            label: "many_lines.rs:31".to_string(),
            detail: None,
            trailing: None,
            changed: None,
            value: PickerValue::Place {
                path,
                line: 30,
                character: 10,
                end_line: 30,
                end_character: 17,
            },
            enabled: true,
            colours: None,
            status: None,
            depth: 0,
            kind: None,
            tab: None,
        }],
        PickerLayout::FullArea,
    );

    // Worked out from the two rules rather than counted out here: the list
    // takes half the room and the preview the rest, and this is a test
    // about where in the preview the line lands.
    let dump = support::render(&mut app, 60, 35);
    let rows: Vec<&str> = support::text_block(&dump)
        .lines()
        .filter(|row| !row.is_empty())
        .collect();
    let under_list = rows
        .iter()
        .position(|row| row.contains('\u{2500}'))
        .expect("a rule");
    let over_status = rows
        .iter()
        .rposition(|row| row.contains('\u{2500}'))
        .expect("the edge");
    let preview = over_status - under_list - 1;
    assert!(
        rows[under_list + 1 + preview / 2].contains("LINE_30"),
        "the place is not in the middle of the preview:\n{dump}"
    );
    assert!(
        rows[under_list + 1].contains(&format!("LINE_{}", 30 - preview / 2)),
        "what leads up to it is not there:\n{dump}"
    );
    support::check("preview_middle_60x35", &dump);

    // Up from the middle of a file: the only case that pins the direction
    // down. Reaching the top of the file leaves the offset at zero, and an
    // offset of zero is the same view whichever way the rows were counted.
    press(&mut app, KeyCode::PageUp);
    let up = support::render(&mut app, 60, 34);
    let rows: Vec<&str> = support::text_block(&up)
        .lines()
        .filter(|row| !row.is_empty())
        .collect();
    assert!(
        !rows.iter().any(|row| row.contains("LINE_30")),
        "paging up did not leave the place behind:\n{up}"
    );
    let under_list = rows
        .iter()
        .position(|row| row.contains('\u{2500}'))
        .expect("a rule");
    assert!(
        rows[under_list + 1].contains("Forty lines"),
        "paging up did not reach the top of the file:\n{up}"
    );
}

/// The symbol menu and the theme picker are compact too, so they get the same
/// edge. One rule drawn for one layout and not the others would read as an
/// inconsistency rather than as a feature.
#[test]
fn every_compact_list_has_an_edge_above_it() {
    // The palette and the theme picker. The symbol menu is not among them
    // any more: it opens only when it has something to offer, which in a
    // test with no server is never.
    for key in ['p', 'h'] {
        let mut app = app();
        if key == 'h' {
            // The theme picker has no key of its own; the palette is the way
            // in, which is also what the palette is for.
            press_control(&mut app, 'p');
            type_text(&mut app, "choose-theme");
            press(&mut app, KeyCode::Enter);
        } else {
            press_control(&mut app, key);
        }

        let dump = support::render(&mut app, 60, 22);
        let rows: Vec<&str> = support::text_block(&dump)
            .lines()
            .filter(|row| !row.is_empty())
            .collect();
        let rule = rows
            .iter()
            .position(|row| row.contains('\u{2500}'))
            .unwrap_or_else(|| panic!("no edge for the list opened by {key:?}:\n{dump}"));
        assert!(
            !rows[rule + 1].trim().is_empty(),
            "the edge has nothing under it:\n{dump}"
        );
        assert!(
            rows[..rule].iter().any(|row| row.contains("fn main")),
            "the code above the edge is gone:\n{dump}"
        );
    }
}

/// The file picker opens on the file being read, even when the walk finds it
/// in a later batch. A list of every file in a project, opened at the top,
/// starts by pointing at something arbitrary.
#[test]
fn the_file_picker_opens_on_the_file_being_read() {
    let mut app = app();
    press_function(&mut app, 1);

    // The walk arrives in batches and the current file is in the second of
    // them, which is the case a picker that only looked once would miss.
    app.handle(Event::FilesFound {
        generation: 1,
        paths: vec!["src/one.rs".into(), "src/two.rs".into()],
    });
    app.handle(Event::FilesFound {
        generation: 1,
        paths: vec!["tests/fixtures/sample.rs".into(), "src/three.rs".into()],
    });

    let picker = app.picker().expect("the picker is open");
    assert_eq!(
        picker
            .matches()
            .nth(picker.selected())
            .map(|item| item.label.as_str()),
        Some("tests/fixtures/sample.rs"),
        "the picker did not open on the file being read"
    );

    // And the window puts it in the middle, which is what makes the rows
    // around it the ones worth looking at.
    let dump = support::render(&mut app, 60, 22);
    let rows: Vec<&str> = support::text_block(&dump)
        .lines()
        .filter(|row| !row.is_empty())
        .collect();
    assert!(
        rows[2].contains("sample.rs"),
        "the selected row is not where the window centres:\n{dump}"
    );
}

/// Once the reader has typed, the list is theirs: a batch arriving afterwards
/// must not pull the selection back to the file being read.
#[test]
fn a_late_batch_does_not_move_a_selection_the_reader_has_touched() {
    let mut app = app();
    press_function(&mut app, 1);
    app.handle(Event::FilesFound {
        generation: 1,
        paths: vec!["src/one.rs".into(), "src/two.rs".into()],
    });

    press(&mut app, KeyCode::Down);
    let chosen = app
        .picker()
        .and_then(|picker| picker.selected_item())
        .map(|item| item.label.clone())
        .expect("a row is selected");

    app.handle(Event::FilesFound {
        generation: 1,
        paths: vec!["tests/fixtures/sample.rs".into()],
    });
    assert_eq!(
        app.picker()
            .and_then(|picker| picker.selected_item())
            .map(|item| item.label.clone()),
        Some(chosen),
        "an arriving batch moved the selection"
    );
}

/// Stopping and restarting always say what happened. Both are asked for when
/// nothing is answering, which is exactly when silence is the one thing that
/// cannot be told from a key that did nothing.
#[test]
fn stopping_and_restarting_say_what_happened() {
    use obelus::command::Command;

    let mut empty = App::new(vec![]);
    support::lay_out(&mut empty, 60, 12);
    obelus::command::dispatch::dispatch(&mut empty, Command::LspStop);
    assert_eq!(empty.note(), Some("no file to stop a server for"));
    obelus::command::dispatch::dispatch(&mut empty, Command::LspRestart);
    assert_eq!(empty.note(), Some("no file to restart a server for"));

    // A language obelus highlights but has no server for. The reason is the
    // useful part: "nothing happened" is not.
    let mut toml = App::new(vec![support::open_fixture("sample.toml")]);
    support::lay_out(&mut toml, 60, 12);
    obelus::command::dispatch::dispatch(&mut toml, Command::LspStop);
    assert_eq!(toml.note(), Some("no language server for toml"));
    obelus::command::dispatch::dispatch(&mut toml, Command::LspRestart);
    assert_eq!(toml.note(), Some("no language server for toml"));

    // And the note is on screen, which is the only place it is of any use.
    let dump = support::render(&mut toml, 60, 12);
    assert!(
        support::text_block(&dump).contains("no language server for toml"),
        "the note is not on the status bar:\n{dump}"
    );

    // A language with a server, none of it running: still a reason, and it
    // names the program so that "not installed" can be acted on.
    let mut rust = app();
    support::lay_out(&mut rust, 60, 12);
    obelus::command::dispatch::dispatch(&mut rust, Command::LspStop);
    let note = rust.note().unwrap_or_default().to_string();
    assert!(note.contains("rust-analyzer"), "{note:?}");
}

/// The theme picker opens on the theme that is on, previews each one by
/// wearing it, and puts the old one back if the reader escapes. A list of
/// colour-scheme names is not a choice between colour schemes: the only
/// honest preview of a theme is the screen in it.
#[test]
fn the_theme_picker_previews_and_can_be_backed_out_of() {
    let mut app = app();
    let before = support::render(&mut app, 60, 12);

    press_control(&mut app, 'p');
    type_text(&mut app, "choose-theme");
    press(&mut app, KeyCode::Enter);

    // On the theme that is on, whichever that is.
    let opened = app.picker().expect("the theme picker");
    assert_eq!(
        opened.selected_item().map(|item| item.label.clone()),
        Some(app.theme().name.to_string()),
        "the picker did not open on the current theme"
    );

    // Moving previews: the code behind the list is repainted.
    press(&mut app, KeyCode::Down);
    let previewing = support::render(&mut app, 60, 12);
    assert_ne!(
        support::legend_block(&previewing),
        support::legend_block(&before),
        "moving the selection did not change any colour"
    );

    // And escaping puts back exactly what was there.
    press(&mut app, KeyCode::Esc);
    assert_eq!(
        support::render(&mut app, 60, 12),
        before,
        "escaping left the previewed theme on"
    );
}

/// Choosing keeps it, which is the other half: an undo that also undid a
/// choice would make the picker impossible to use.
#[test]
fn choosing_a_theme_keeps_it() {
    let mut app = app();
    let before = support::render(&mut app, 60, 12);

    press_control(&mut app, 'p');
    type_text(&mut app, "choose-theme");
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);

    let after = support::render(&mut app, 60, 12);
    assert_ne!(
        support::legend_block(&after),
        support::legend_block(&before),
        "the chosen theme was not kept"
    );

    // And a later escape from something else does not resurrect the old one.
    press_control(&mut app, 'p');
    press(&mut app, KeyCode::Esc);
    assert_eq!(
        support::legend_block(&support::render(&mut app, 60, 12)),
        support::legend_block(&after),
        "escaping another picker put the old theme back"
    );
}

/// The palette's tabs group the commands, and the arrows walk them. Fourteen
/// commands is more than a compact list shows at once, and the groups are
/// what a reader is choosing between when they do not already know the name.
#[test]
fn the_palette_groups_its_commands_into_tabs() {
    let mut app = app();
    press_control(&mut app, 'p');

    let names: Vec<String> = app
        .picker()
        .expect("the palette")
        .tabs()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(names[0], "all", "the first tab is not everything");

    let listed = |app: &App| -> Vec<String> {
        app.picker()
            .expect("the palette")
            .matches()
            .map(|item| item.label.clone())
            .collect()
    };
    let everything = listed(&app);
    assert!(everything.len() > 5, "{everything:?}");

    // One step right is the first group, which is a subset and not the whole
    // list.
    press(&mut app, KeyCode::Right);
    let files = listed(&app);
    assert!(files.contains(&"open-file".to_string()), "{files:?}");
    assert!(!files.contains(&"quit".to_string()), "{files:?}");
    assert!(files.len() < everything.len(), "{files:?}");

    // And the row of tabs is on screen with the one showing marked, which is
    // the only thing that says which list this is.
    let dump = support::render(&mut app, 62, 14);
    let row = support::text_block(&dump)
        .lines()
        .find(|row| row.contains("files") && row.contains("code"))
        .unwrap_or_else(|| panic!("no tabs on screen:\n{dump}"))
        .to_string();
    assert!(row.contains("all"), "{dump}");

    // Left from the first group wraps to the last, so walking the tabs never
    // dead-ends.
    press(&mut app, KeyCode::Left);
    press(&mut app, KeyCode::Left);
    let last = listed(&app);
    assert!(last.contains(&"quit".to_string()), "{last:?}");

    // And the block is the same height whichever tab is showing. Sizing it
    // to the tab's contents would move the rows out from under a reader
    // walking the tabs -- and the tabs themselves with them.
    let row_of_tabs = |dump: &str| {
        support::text_block(dump)
            .lines()
            .filter(|row| !row.is_empty())
            .position(|row| row.contains("all") && row.contains("code"))
            .unwrap_or_else(|| panic!("no tabs on screen:\n{dump}"))
    };
    let one = row_of_tabs(&support::render(&mut app, 62, 20));
    press(&mut app, KeyCode::Right);
    let two = row_of_tabs(&support::render(&mut app, 62, 20));
    assert_eq!(one, two, "the block changed height with the tab");
}

/// A tab with nothing in it says so, like every other empty list. Walking
/// into a blank region would read as the arrow key having broken something.
#[test]
fn a_list_with_nothing_in_it_says_why() {
    // A tree with nothing in it to offer, which is the walk having
    // finished with nothing rather than not having started: no batch of
    // paths ever arrives here.
    let mut empty = app();
    support::lay_out(&mut empty, 50, 8);
    press_function(&mut empty, 1);
    let dump = support::render(&mut empty, 50, 8);
    assert!(
        support::text_block(&dump).contains("no files under this directory"),
        "an empty file list said nothing:\n{dump}"
    );

    // A query that matches nothing is a fact about the query, and the list
    // says that instead.
    let mut app = app();
    support::lay_out(&mut app, 50, 8);
    press_control(&mut app, 'p');
    type_text(&mut app, "zzzz");
    let dump = support::render(&mut app, 50, 8);
    assert!(
        support::text_block(&dump).contains("no match"),
        "a query that matched nothing said nothing:\n{dump}"
    );
}

/// `ctrl+w` closes whatever the screen is about: the row under the selection
/// while the buffer list is open, and the file being read otherwise. One key
/// meaning "close this" everywhere beats a second key that works in one list
/// only -- and the palette already says what this one is bound to.
#[test]
fn the_buffer_list_closes_the_selected_file_with_the_same_key() {
    let mut app = App::new(vec![
        support::open_fixture("sample.rs"),
        support::open_fixture("long.rs"),
    ]);
    support::lay_out(&mut app, 60, 12);
    press_function(&mut app, 2);

    // The second row, to prove it closes what is *selected* and not what was
    // being read.
    press(&mut app, KeyCode::Down);
    let chosen = app
        .picker()
        .and_then(|picker| picker.selected_item())
        .map(|item| item.label.clone())
        .expect("a row is selected");
    assert!(chosen.contains("long.rs"), "{chosen}");

    // The selected row goes, the list stays open, and the other row is still
    // there: closing several files one after another is why you are in here.
    press_control(&mut app, 'w');
    let after = support::render(&mut app, 60, 12);
    let text = support::text_block(&after);
    assert!(
        !text.contains("long.rs"),
        "the file is still listed:\n{after}"
    );
    assert!(
        text.contains("sample.rs"),
        "the list closed itself:\n{after}"
    );

    // And the file being read is the one that is left.
    press(&mut app, KeyCode::Esc);
    let reading = support::render(&mut app, 60, 12);
    assert!(
        support::text_block(&reading).contains("sample.rs"),
        "the reader was left on a closed file:\n{reading}"
    );
}

/// Closing the last file leaves the welcome screen, which is the only honest
/// thing to show: there is nothing to read.
#[test]
fn closing_the_last_file_goes_back_to_the_welcome_screen() {
    use obelus::command::Command;

    let mut app = app();
    support::lay_out(&mut app, 64, 20);
    obelus::command::dispatch::dispatch(&mut app, Command::BufferClose);

    let dump = support::render(&mut app, 64, 20);
    assert!(
        support::text_block(&dump).contains('\u{2588}'),
        "the welcome screen is not back:\n{dump}"
    );
    assert!(
        !support::text_block(&dump).contains("fn main"),
        "the closed file is still on screen:\n{dump}"
    );

    // And a stale id -- the jump list keeps them -- does nothing rather than
    // finding a different file.
    obelus::command::dispatch::dispatch(&mut app, Command::GoBack);
    let after = support::render(&mut app, 64, 20);
    assert!(
        !support::text_block(&after).contains("fn main"),
        "going back resurrected a closed file:\n{after}"
    );
}

/// The outline: every symbol the file defines, coloured by what it is, with
/// the symbol shown in its own code below. Everything except where the rows
/// come from is the file picker's machinery.
#[test]
fn the_outline_lists_what_a_file_defines_and_colours_it() {
    let mut app = App::new(vec![
        obelus::buffer::Buffer::open(std::path::Path::new("src/jump.rs")).expect("a file"),
    ]);
    support::lay_out(&mut app, 60, 24);
    // Down into the file first: an outline is opened to ask "where am I",
    // and a list that always started at the top would answer "at the
    // beginning", which is almost never true.
    for _ in 0..30 {
        press(&mut app, KeyCode::Down);
    }
    press_function(&mut app, 7);
    let here = app
        .picker()
        .and_then(|picker| picker.selected_item())
        .map(|item| item.label.clone())
        .expect("a row is selected");
    assert_eq!(
        here, "JumpList",
        "the outline did not open on the symbol the cursor is in"
    );

    let labels: Vec<String> = app
        .picker()
        .expect("the outline")
        .matches()
        .map(|item| item.label.clone())
        .collect();
    assert!(labels.contains(&"JumpList".to_string()), "{labels:?}");
    assert!(labels.contains(&"push".to_string()), "{labels:?}");

    let dump = support::render(&mut app, 60, 24);
    let rows: Vec<&str> = support::text_block(&dump)
        .lines()
        .filter(|row| !row.is_empty())
        .collect();
    let styles: Vec<&str> = support::style_block(&dump)
        .lines()
        .filter(|row| !row.is_empty())
        .collect();

    // A type and a function are different colours, because the colour is the
    // whole of what says which is which.
    let letter = |needle: &str| {
        let at = rows
            .iter()
            .position(|row| row.contains(needle))
            .unwrap_or_else(|| panic!("{needle:?} is not on screen:\n{dump}"));
        let column = rows[at].find(needle).expect("the label");
        let column = rows[at][..column].chars().count();
        styles[at].chars().nth(column).expect("a style")
    };
    assert_ne!(
        letter("JumpList"),
        letter("push"),
        "a type and a function are the same colour:\n{dump}"
    );

    // And the preview below shows the selected symbol in its own code, which
    // is what makes an outline readable rather than an index.
    assert!(
        rows.iter().any(|row| row.contains("pub struct Jump")),
        "the symbol is not previewed in its code:\n{dump}"
    );

    // Choosing one goes there.
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    let after = support::render(&mut app, 60, 24);
    assert!(
        support::text_block(&after).contains("pub struct JumpList"),
        "choosing a symbol did not go to it:\n{after}"
    );
}

/// Nesting shows as indentation. `mod tests` is a definition and the tests
/// inside it are inside it, so their rows start further right -- which is
/// the whole of what makes an outline a tree rather than a list.
///
/// Its own test because it has to look at the end of the list, where the
/// nested rows are: the list shows ten at a time, and a file that grows a
/// function would otherwise push them off the bottom.
#[test]
fn an_outline_indents_what_is_nested() {
    let mut app = App::new(vec![
        obelus::buffer::Buffer::open(std::path::Path::new("src/jump.rs")).expect("a file"),
    ]);
    support::lay_out(&mut app, 60, 24);
    press_function(&mut app, 7);
    press(&mut app, KeyCode::End);

    let dump = support::render(&mut app, 60, 24);
    let rows: Vec<&str> = support::text_block(&dump)
        .lines()
        .filter(|row| !row.is_empty())
        .collect();
    let starts_at = |needle: &str| {
        let at = rows
            .iter()
            .position(|row| row.contains(needle))
            .unwrap_or_else(|| panic!("{needle:?} is not on screen:\n{dump}"));
        let byte = rows[at].find(needle).expect("the label");
        rows[at][..byte].chars().count()
    };
    assert!(
        starts_at("nothing_to_go_back_to") > starts_at("tests"),
        "a nested symbol is not indented under the one that holds it:\n{dump}"
    );
}

/// A language whose grammar ships no tags query has no outline, and that is
/// a different fact from a file that defines nothing.
#[test]
fn a_language_with_no_tags_says_so_rather_than_looking_empty() {
    let mut app = App::new(vec![support::open_fixture("sample.toml")]);
    support::lay_out(&mut app, 60, 12);
    press_function(&mut app, 7);

    let dump = support::render(&mut app, 60, 12);
    assert!(
        support::text_block(&dump).contains("no outline for this language"),
        "an empty outline said nothing:\n{dump}"
    );
}

/// The buffer list holds still: the files in the order they were opened,
/// whatever the reader has been doing with them.
///
/// It used to be ordered by how often each had been come back to, so the
/// row a reader was reaching for moved every time they used it -- the list
/// reordered itself in the moment between deciding to press the key and
/// looking at what came up. The one file whose place they might otherwise
/// have to hunt for is the one they are in, and that is the row the list
/// opens on.
#[test]
fn the_buffer_list_keeps_the_order_the_files_were_opened_in() {
    let mut app = App::new(vec![
        support::open_fixture("sample.rs"),
        support::open_fixture("long.rs"),
        support::open_fixture("indented.rs"),
    ]);
    app.statuses_for_test(std::collections::HashMap::new());
    support::lay_out(&mut app, 60, 12);

    let listed = |app: &App| -> Vec<String> {
        app.picker()
            .expect("the buffer list")
            .matches()
            .map(|item| item.label.clone())
            .collect()
    };
    let visit = |app: &mut App, needle: &str| {
        press_function(app, 2);
        type_text(app, needle);
        press(app, KeyCode::Enter);
    };

    press_function(&mut app, 2);
    let opened = listed(&app);
    let names: Vec<&str> = opened
        .iter()
        .map(|label| label.rsplit('/').next().unwrap_or(label))
        .collect();
    assert_eq!(
        names,
        ["sample.rs", "long.rs", "indented.rs"],
        "not the order they were opened in: {opened:?}"
    );
    press(&mut app, KeyCode::Esc);

    // Whatever is visited, and however often, the list is the same list.
    visit(&mut app, "indented");
    visit(&mut app, "long");
    visit(&mut app, "indented");
    press_function(&mut app, 2);
    assert_eq!(
        listed(&app),
        opened,
        "the list reordered itself under the reader"
    );

    // And it opens on the file being read, which is where a reader who has
    // just arrived somewhere is looking.
    assert!(
        app.picker()
            .and_then(|picker| picker.selected_item())
            .is_some_and(|item| item.label.contains("indented.rs")),
        "the list did not open on the file being read"
    );
}

/// A line number is typed on the status bar, not chosen from a list. A list
/// of every line in the file would be the file, and a list of one row saying
/// "type a line number" is a list of nothing pretending to be a hint -- so
/// the prompt is its own thing and it covers no code.
#[test]
fn a_line_number_is_typed_on_the_status_bar() {
    let mut app = App::new(vec![support::open_fixture("many_lines.rs")]);
    support::lay_out(&mut app, 40, 12);

    let before = support::render(&mut app, 40, 12);
    press_control(&mut app, 'l');
    let asking = support::render(&mut app, 40, 12);
    let rows: Vec<&str> = support::text_block(&asking)
        .lines()
        .filter(|row| !row.is_empty())
        .collect();

    // The question is the status row, and it says what it wants.
    assert!(
        rows.last().is_some_and(|status| status.contains("line: ")),
        "the question is not on the status bar:\n{asking}"
    );
    // And nothing else moved: no list, no rule, no region over the code.
    let code = |dump: &str| {
        support::text_block(dump)
            .lines()
            .filter(|row| !row.is_empty())
            .take(11)
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(
        code(&asking),
        code(&before),
        "asking for a line number covered the code:\n{asking}"
    );
    // The caret is in the answer, on the status row.
    let (x, y) = support::cursor_line(&asking)
        .split_once(',')
        .expect("a caret");
    assert_eq!(y.parse::<u16>().expect("a row"), 11, "{asking}");
    assert_eq!(
        x.parse::<usize>().expect("a column"),
        1 + "line: ".len(),
        "the caret is not after the label:\n{asking}"
    );

    type_text(&mut app, "30");
    let typed = support::render(&mut app, 40, 12);
    assert!(
        support::text_block(&typed)
            .lines()
            .any(|row| row.contains("line: 30")),
        "what was typed is not shown:\n{typed}"
    );

    press(&mut app, KeyCode::Enter);
    let buffer = app.current_buffer().expect("a buffer");
    // One-based on the way in, because that is what the status bar shows.
    assert_eq!(buffer.cursor().line.get(), 29);
    assert!(app.prompt().is_none(), "the question stayed open");

    // Centred, like every other arrival.
    let after = support::render(&mut app, 40, 12);
    let (_, row) = support::cursor_line(&after)
        .split_once(',')
        .expect("the cursor is on screen");
    assert_eq!(row.parse::<u16>().expect("a row"), 5, "{after}");

    // And it is a jump, so going back comes back.
    obelus::command::dispatch::dispatch(&mut app, obelus::command::Command::GoBack);
    assert_eq!(
        app.current_buffer().expect("a buffer").cursor().line.get(),
        0,
        "typing a line number did not record where it left"
    );
}

/// Only digits get in, and a number past the end of the file is the end of
/// the file: `9999` in a short one means the last line.
#[test]
fn a_line_prompt_takes_digits_and_clamps_them() {
    let mut app = App::new(vec![support::open_fixture("many_lines.rs")]);
    support::lay_out(&mut app, 40, 12);

    // Letters never get in: the answer is digits, and a prompt that took
    // them and complained afterwards would tell the reader at the answer
    // what it could have told them at the keystroke.
    press_control(&mut app, 'l');
    type_text(&mut app, "zz");
    assert_eq!(
        app.prompt().map(obelus::component::prompt::Prompt::text),
        Some(""),
        "a letter got into a line number"
    );
    press(&mut app, KeyCode::Enter);
    assert!(app.prompt().is_some(), "an empty answer was accepted");
    assert_eq!(
        app.current_buffer().expect("a buffer").cursor().line.get(),
        0
    );

    // A number too big to be a line number is the one thing digits alone
    // cannot rule out.
    type_text(&mut app, "99999999999999999999");
    press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.note(),
        Some("\"99999999999999999999\" is not a line number")
    );
    assert!(app.prompt().is_none(), "the question stayed open");

    press_control(&mut app, 'l');
    type_text(&mut app, "9999");
    press(&mut app, KeyCode::Enter);
    let buffer = app.current_buffer().expect("a buffer");
    assert_eq!(
        buffer.cursor().line,
        buffer.text().last_line(),
        "a line past the end was not clamped to the end"
    );

    // An empty prompt answers nothing rather than something arbitrary, and
    // escape gives up on it.
    press_control(&mut app, 'l');
    press(&mut app, KeyCode::Enter);
    assert!(app.prompt().is_some(), "an empty prompt answered anyway");
    press(&mut app, KeyCode::Esc);
    assert!(app.prompt().is_none(), "escape did not give up on it");
}

/// Opening a file records where the reader was. The history is for leaps,
/// and switching files is one: without this, a session of opening files
/// leaves nothing to go back *to*, and `go-back` answers "nowhere further
/// back" to a reader who has been three files deep.
#[test]
fn opening_a_file_is_somewhere_to_come_back_from() {
    let mut app = App::new(vec![support::open_fixture("many_lines.rs")]);
    support::lay_out(&mut app, 60, 12);
    for _ in 0..6 {
        press(&mut app, KeyCode::Down);
    }
    let left = app.current_buffer().expect("a buffer").cursor().line;
    let first = app.current_buffer().expect("a buffer").path().to_path_buf();

    // Somewhere else, through the file picker.
    press_function(&mut app, 1);
    app.handle(Event::FilesFound {
        generation: 1,
        paths: vec!["tests/fixtures/long.rs".into()],
    });
    press(&mut app, KeyCode::Enter);
    assert!(
        app.current_buffer()
            .expect("a buffer")
            .path()
            .ends_with("long.rs"),
        "the file did not open"
    );

    // And back to the line that was being read, not to the top of it.
    obelus::command::dispatch::dispatch(&mut app, obelus::command::Command::GoBack);
    let buffer = app.current_buffer().expect("a buffer");
    assert_eq!(buffer.path(), first, "went back to the wrong file");
    assert_eq!(buffer.cursor().line, left, "went back to the wrong line");

    // The file picker choosing a file that is *already* open is the same
    // leap by a different route, and goes through a different branch.
    press_function(&mut app, 1);
    app.handle(Event::FilesFound {
        generation: 2,
        paths: vec!["tests/fixtures/long.rs".into()],
    });
    press(&mut app, KeyCode::Enter);
    assert!(
        app.current_buffer()
            .expect("a buffer")
            .path()
            .ends_with("long.rs"),
        "the already-open file was not switched to"
    );
    obelus::command::dispatch::dispatch(&mut app, obelus::command::Command::GoBack);
    assert_eq!(
        app.current_buffer().expect("a buffer").path(),
        first,
        "opening a file that was already open recorded nothing"
    );

    // The buffer list is the same kind of leap.
    press_function(&mut app, 2);
    type_text(&mut app, "long");
    press(&mut app, KeyCode::Enter);
    assert!(
        app.current_buffer()
            .expect("a buffer")
            .path()
            .ends_with("long.rs")
    );
    obelus::command::dispatch::dispatch(&mut app, obelus::command::Command::GoBack);
    assert_eq!(
        app.current_buffer().expect("a buffer").path(),
        first,
        "the buffer list recorded nothing"
    );

    // Re-opening the file already being read records nothing, or the
    // history would fill with the place the reader never left.
    let before = app.picker().is_none();
    assert!(before);
    press_function(&mut app, 2);
    type_text(&mut app, "many_lines");
    press(&mut app, KeyCode::Enter);
    obelus::command::dispatch::dispatch(&mut app, obelus::command::Command::GoForward);
    assert!(
        app.current_buffer()
            .expect("a buffer")
            .path()
            .ends_with("long.rs"),
        "re-opening the current file left a place in the history"
    );
}

/// And the preview shows each file where it is being read, not at its top.
/// Choosing the row takes the reader back to exactly that, so the list reads
/// as something folded over the open files rather than as a way to somewhere
/// new -- and a file's own place in it is the thing a reader remembers it by.
#[test]
fn the_buffer_list_previews_each_file_where_it_was_left() {
    let mut app = App::new(vec![
        support::open_fixture("many_lines.rs"),
        support::open_fixture("sample.rs"),
    ]);
    app.statuses_for_test(std::collections::HashMap::new());
    support::lay_out(&mut app, 60, 30);

    // Down the first file, then away to the second, so the place in the
    // first is somewhere only the buffer remembers.
    for _ in 0..24 {
        press(&mut app, KeyCode::Down);
    }
    press_function(&mut app, 2);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    assert!(
        app.current_buffer()
            .expect("a buffer")
            .path()
            .ends_with("sample.rs"),
        "the second file was not switched to"
    );

    // The list, opened on the file being read: its preview is that file.
    press_function(&mut app, 2);
    let sample = support::render(&mut app, 60, 30);
    assert!(
        support::text_block(&sample).contains("greeting"),
        "the preview is not the file being read:\n{sample}"
    );

    // And the row for the file that was left previews it there, twenty-odd
    // lines in.
    press(&mut app, KeyCode::Up);
    let other = support::render(&mut app, 60, 30);
    let text = support::text_block(&other);
    assert!(
        text.contains("LINE_24"),
        "the preview is not where the file was left:\n{other}"
    );
    assert!(
        !text.contains("LINE_01"),
        "the preview went back to the top of the file:\n{other}"
    );

    // Which is what choosing the row shows, so nothing moves under the
    // reader as the list closes.
    press(&mut app, KeyCode::Enter);
    let chosen = support::render(&mut app, 60, 30);
    assert!(
        support::text_block(&chosen).contains("LINE_24"),
        "the file opened somewhere else than its preview said:\n{chosen}"
    );
}

/// The file list gets a tab for the files that have changed, and only when
/// some have: "which file do I want" and "what have I been working on" are
/// different questions, and a reader coming back to a project asks the
/// second one first.
#[test]
fn the_file_list_has_a_tab_for_what_has_changed() {
    use obelus::{
        command::{Command, dispatch},
        git::FileStatus,
    };

    // A clean tree: one listing, and no row of tabs at all, because a row
    // of tabs with one tab on it says there is somewhere else to go.
    let mut clean = app();
    support::lay_out(&mut clean, 60, 12);
    press_function(&mut clean, 1);
    assert!(
        clean.picker().expect("the file list").tabs().is_empty(),
        "a tab row with nowhere to go"
    );

    // And the key that opens the changed listing says why it will not.
    press(&mut clean, KeyCode::Esc);
    dispatch::dispatch(&mut clean, Command::FileChanged);
    assert!(clean.picker().is_none(), "an empty listing opened");
    assert_eq!(clean.note(), Some("nothing has changed"));

    // A tree with changes in it: two tabs, and the changed one lists what
    // git said, by name, in the colours the status gives them.
    let mut dirty = app();
    let root = dirty.working_directory().to_path_buf();
    dirty.statuses_for_test(
        [
            (root.join("src/late.rs"), FileStatus::Changed),
            (root.join("src/early.rs"), FileStatus::New),
        ]
        .into_iter()
        .collect(),
    );
    support::lay_out(&mut dirty, 60, 12);
    press_function(&mut dirty, 1);
    let picker = dirty.picker().expect("the file list");
    assert_eq!(picker.tabs(), ["all", "changed"], "not the two listings");
    assert_eq!(picker.tab(), 0, "ctrl+o did not open the whole tree");

    // The right arrow moves to it, and its rows are the changed files --
    // sorted by name, because the order git walks the tree in is not an
    // order anyone can learn.
    press(&mut dirty, KeyCode::Right);
    let rows: Vec<(String, Option<FileStatus>)> = dirty
        .picker()
        .expect("the file list")
        .matches()
        .map(|item| (item.label.clone(), item.status))
        .collect();
    assert_eq!(
        rows,
        vec![
            ("src/early.rs".to_string(), Some(FileStatus::New)),
            ("src/late.rs".to_string(), Some(FileStatus::Changed)),
        ],
        "not the changed files"
    );

    // And back to everything: the rows are gone and a walk is running for
    // them again.
    press(&mut dirty, KeyCode::Left);
    let picker = dirty.picker().expect("the file list");
    assert_eq!(picker.tab(), 0);
    assert_eq!(picker.match_count(), 0, "the changed rows stayed");
    assert_eq!(
        picker.nothing_to_show(),
        Some("no files under this directory")
    );
}

/// A walk still running when the reader moves to the changed listing is
/// answering the other tab's question, and its batches must not land in
/// this one: nothing can stop a walk, so its answers have to be recognised
/// as stale.
#[test]
fn a_walk_in_flight_does_not_land_in_the_changed_listing() {
    use obelus::git::FileStatus;

    let mut app = app();
    let root = app.working_directory().to_path_buf();
    app.statuses_for_test(
        [(root.join("src/changed.rs"), FileStatus::Changed)]
            .into_iter()
            .collect(),
    );
    support::lay_out(&mut app, 60, 12);
    press_function(&mut app, 1);
    // The walk that opening the list started is the first one.
    app.handle(Event::FilesFound {
        generation: 1,
        paths: vec!["src/walked.rs".into()],
    });
    assert_eq!(app.picker().expect("the file list").match_count(), 1);

    press(&mut app, KeyCode::Right);
    app.handle(Event::FilesFound {
        generation: 1,
        paths: vec!["src/late.rs".into()],
    });
    let rows: Vec<String> = app
        .picker()
        .expect("the file list")
        .matches()
        .map(|item| item.label.clone())
        .collect();
    assert_eq!(
        rows,
        vec!["src/changed.rs".to_string()],
        "a batch from the other tab's walk landed here"
    );
}

/// `ctrl+d` opens the file list on the changed files, which is the whole
/// point of it having a key: the reader who wants it wants it now.
#[test]
fn a_key_opens_the_changed_files_directly() {
    use obelus::git::FileStatus;

    let mut app = app();
    let root = app.working_directory().to_path_buf();
    app.statuses_for_test(
        [(root.join("src/changed.rs"), FileStatus::Changed)]
            .into_iter()
            .collect(),
    );
    support::lay_out(&mut app, 60, 12);
    press_function(&mut app, 3);

    let picker = app.picker().expect("the file list");
    assert_eq!(picker.tabs(), ["all", "changed"]);
    assert_eq!(picker.tab(), 1, "ctrl+d did not open the changed listing");
    assert_eq!(
        picker
            .matches()
            .map(|item| item.label.clone())
            .collect::<Vec<_>>(),
        vec!["src/changed.rs".to_string()]
    );
}

/// The list of open files opens on the file being read.
///
/// The rows are in most-visited order, so the file the reader is in is not
/// necessarily the first of them: two files visited once each are listed in
/// the order they were opened, whichever one is being read. A list that
/// starts somewhere arbitrary makes the reader find their own file before
/// they can leave it.
#[test]
fn the_buffer_list_opens_on_the_current_file() {
    let mut app = App::new(vec![
        support::open_fixture("sample.rs"),
        support::open_fixture("many_lines.rs"),
    ]);
    app.statuses_for_test(std::collections::HashMap::new());
    support::lay_out(&mut app, 60, 20);

    // Visit the first, then the second: both have been visited once, so
    // they are listed in the order they were opened -- and the one being
    // read is the second of them.
    press_function(&mut app, 2);
    press(&mut app, KeyCode::Enter);
    press_function(&mut app, 2);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);

    press_function(&mut app, 2);
    let rows: Vec<String> = app
        .picker()
        .expect("the list")
        .matches()
        .map(|item| item.label.clone())
        .collect();
    assert!(
        rows.first().is_some_and(|row| row.contains("sample.rs")),
        "the file being read is first anyway, so this proves nothing: {rows:?}"
    );
    assert!(
        app.picker()
            .and_then(|picker| picker.selected_item())
            .is_some_and(|item| item.label.contains("many_lines.rs")),
        "the list did not open on the file being read: {rows:?}"
    );
}

/// The file list previews an open file where it is being read, too.
///
/// The same answer the list of open files gives, because it is the same
/// question: a file's place in it is the thing a reader remembers it by, and
/// a list that showed the top of a file they are twenty screens into would
/// be showing them somewhere they have not been for an hour. A file nothing
/// has opened has no such place, so it starts at the top.
#[test]
fn the_file_list_previews_an_open_file_where_it_is_being_read() {
    let mut app = App::new(vec![support::open_fixture("many_lines.rs")]);
    app.statuses_for_test(std::collections::HashMap::new());
    support::lay_out(&mut app, 60, 30);
    for _ in 0..24 {
        press(&mut app, KeyCode::Down);
    }

    // The file list, with the open file among the rows the walk found.
    press_function(&mut app, 1);
    app.handle(Event::FilesFound {
        generation: 1,
        paths: vec![
            "tests/fixtures/many_lines.rs".into(),
            "tests/fixtures/sample.rs".into(),
        ],
    });
    support::type_text(&mut app, "many");
    let dump = support::render(&mut app, 60, 30);
    let text = support::text_block(&dump);
    assert!(
        text.contains("LINE_24"),
        "the preview is not where the file is being read:\n{dump}"
    );
    assert!(
        !text.contains("LINE_01"),
        "the preview went back to the top of the file:\n{dump}"
    );

    // And a file nobody has open starts at the top, because there is
    // nowhere else it has been.
    for _ in 0..4 {
        press(&mut app, KeyCode::Backspace);
    }
    support::type_text(&mut app, "sample");
    let dump = support::render(&mut app, 60, 30);
    assert!(
        support::text_block(&dump).contains("greeting"),
        "a file nobody has open is not previewed from its top:\n{dump}"
    );
}

/// Every bar on a screen is in the same column.
///
/// A list opened over a file, with a preview under it, is three scrolling
/// things at once -- and the editor's bar used to sit one column short of
/// the others, because the change map had the last column. A bar that jumps
/// sideways across a rule reads as two different controls, which is what it
/// stopped being the moment the map moved to the left with the rest of the
/// news about changes.
#[test]
fn every_bar_on_the_screen_is_in_the_same_column() {
    let mut app = App::new(vec![support::open_fixture("many_lines.rs")]);
    app.statuses_for_test(std::collections::HashMap::new());
    support::lay_out(&mut app, 60, 22);

    // The file alone, which scrolls because it is forty lines in twenty.
    let dump = support::render(&mut app, 60, 22);
    // Which screen row each bar is on, and which column it is in.
    let bars = |dump: &str| -> Vec<(usize, usize)> {
        support::text_block(dump)
            .lines()
            .filter_map(|row| row.split_once('|'))
            .enumerate()
            .filter_map(|(at, (_, drawn))| {
                drawn
                    .chars()
                    .position(|glyph| matches!(glyph, '\u{2502}' | '\u{2588}'))
                    .map(|column| (at, column))
            })
            .collect()
    };
    let editor = bars(&dump);
    assert!(!editor.is_empty(), "the file does not scroll:\n{dump}");

    // And with a list over it, long enough to scroll, previewing a file
    // long enough to scroll as well.
    press_function(&mut app, 1);
    app.handle(Event::FilesFound {
        generation: 1,
        paths: std::iter::once("tests/fixtures/many_lines.rs".into())
            .chain((0..40).map(|number| format!("src/dir_{number:02}/file.rs").into()))
            .collect(),
    });
    let dump = support::render(&mut app, 60, 22);
    let listed = bars(&dump);
    // The rule between the two has no bar on it, so a gap in the rows is
    // what says both of them drew one.
    let gap = listed.windows(2).any(|pair| pair[1].0 > pair[0].0 + 1);
    assert!(gap, "the list and its preview do not both scroll:\n{dump}");

    let mut columns: Vec<usize> = editor
        .into_iter()
        .chain(listed)
        .map(|(_, column)| column)
        .collect();
    columns.sort_unstable();
    columns.dedup();
    assert_eq!(columns.len(), 1, "the bars are in {columns:?}:\n{dump}");
}

/// A rule is a rule, whatever happens to be drawn under it.
///
/// It used to close itself off against a scrollbar it crossed, which meant
/// deciding per cell whether the neighbour was a control -- and the only
/// question it could ask the grid was which glyph the cell held. A file's
/// own text answers that the same way a bar does: this repository is full of
/// golden grids drawn in box characters, and a rule over one grew a tick
/// everywhere the file had a stroke under it. The bar is a block now, which
/// is a surface rather than a line, and nothing has to join anything.
///
/// Broken deliberately by putting the junction back: the rule between the
/// list and the preview came out `\u{2500}\u{252c}\u{252c}\u{2500}\u{252c}` and
/// so on, which is what the reader saw.
#[test]
fn a_rule_is_a_rule_over_whatever_is_under_it() {
    let mut app = App::new(vec![support::open_fixture("boxes.txt")]);
    app.statuses_for_test(std::collections::HashMap::new());
    support::lay_out(&mut app, 60, 22);
    press_function(&mut app, 1);
    app.handle(Event::FilesFound {
        generation: 1,
        paths: std::iter::once("tests/fixtures/boxes.txt".into())
            .chain((0..40).map(|number| format!("src/dir_{number:02}/file.rs").into()))
            .collect(),
    });
    let dump = support::render(&mut app, 60, 22);

    let rules: Vec<&str> = support::text_block(&dump)
        .lines()
        .filter_map(|row| row.split_once('|'))
        .map(|(_, drawn)| drawn.trim_end())
        .filter(|drawn| drawn.contains('\u{2500}'))
        .collect();
    assert!(rules.len() >= 2, "not the screen this is about:\n{dump}");
    for rule in rules {
        assert!(
            rule.chars().all(|glyph| glyph == '\u{2500}'),
            "a rule grew a junction: {rule:?}\n{dump}"
        );
    }
}
