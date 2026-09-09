//! The one list-with-a-prompt, in its four instantiations.

mod support;

use crossterm::event::KeyCode;
use obelus::{
    app::App,
    event::Event,
    picker::{Picker, PickerItem, PickerLayout, PickerOutcome, PickerValue},
};
use support::{press, press_control, type_text};

fn app() -> App {
    App::new(vec![support::open_fixture("sample.rs")])
}

fn items(labels: &[&str]) -> Vec<PickerItem> {
    labels
        .iter()
        .map(|label| PickerItem {
            icon: None,
            label: (*label).to_string(),
            detail: None,
            trailing: None,
            value: PickerValue::File(label.into()),
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
            icon: None,
            label: format!("item-{index:03}"),
            detail: None,
            trailing: None,
            value: PickerValue::File(format!("item-{index:03}").into()),
        })
        .collect()
}

#[test]
fn the_command_palette_hugs_the_status_bar_and_leaves_the_code_visible() {
    let mut app = app();
    press_control(&mut app, 'p');
    support::check("palette_60x12", &support::render(&mut app, 60, 12));
}

#[test]
fn a_full_area_picker_covers_the_code() {
    let mut app = app();
    press_control(&mut app, 'e');
    support::check("buffers_60x12", &support::render(&mut app, 60, 12));
}

/// A list shorter than the region still has to cover it. Painting only the
/// rows with items leaves the code showing underneath, which reads as a
/// half-drawn screen rather than as a short list.
#[test]
fn a_full_area_picker_with_one_item_still_covers_the_code() {
    let mut app = app();
    press_control(&mut app, 'e');
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

    assert!(support::text_block(&all).contains("file.reload"));
    assert!(
        !support::text_block(&narrowed).contains("file.reload"),
        "the query did not narrow the list:\n{narrowed}"
    );
    assert!(support::text_block(&narrowed).contains("theme.select"));
}

/// The matched characters of the selected row get their own colour, which is
/// the only thing that says *why* a row matched a query it does not contain
/// literally.
///
/// Through a full-area picker, not the palette: `picker_match` and the colour
/// of a function name are the same in the dark theme, and the palette leaves
/// the code visible above it, so the assertion would pass on `main` and
/// `println!` whether or not anything was marked as matched.
#[test]
fn the_matched_characters_of_the_selected_row_are_coloured() {
    let mut app = app();
    press_control(&mut app, 'e');
    let unmatched = support::render(&mut app, 60, 12);
    assert!(
        !support::legend_block(&unmatched).contains("#60a5fa"),
        "the match colour is on screen before anything has been typed:\n{unmatched}"
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
        support::legend_block(&dump).contains("#60a5fa"),
        "no characters were marked as matched:\n{dump}"
    );
}

#[test]
fn escape_leaves_the_code_as_it_was() {
    let mut app = app();
    let before = support::render(&mut app, 60, 12);

    press_control(&mut app, 'p');
    press(&mut app, KeyCode::Esc);

    assert_eq!(support::render(&mut app, 60, 12), before);
}

/// Choosing `theme.select` from the palette opens the theme picker. Nothing
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

/// A key the picker does not want has to reach the key table, or there would
/// be no way out of one but Escape.
#[test]
fn a_key_the_picker_ignores_falls_through_to_the_key_table() {
    let mut app = app();
    press_control(&mut app, 'p');
    assert!(!app.should_quit());

    press_control(&mut app, 'q');
    assert!(app.should_quit(), "ctrl+q was swallowed by the picker");
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
    press_control(&mut app, 'f');
    press(&mut app, KeyCode::Esc);
    press_control(&mut app, 'f');

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
    press_control(&mut app, 'f');
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

/// The page a key moves by has to be the number of rows actually on screen, or
/// paging moves by not quite a screenful and the list appears to skip.
#[test]
fn a_page_is_the_number_of_rows_on_screen() {
    let full = Picker::new(many(50), PickerLayout::FullArea);
    assert_eq!(full.visible_rows(11), 11, "a full-area list takes the room");

    let compact = Picker::new(many(50), PickerLayout::Compact { rows: 10 });
    assert_eq!(compact.visible_rows(11), 10, "capped by the layout");
    assert_eq!(compact.visible_rows(4), 4, "and by the room");

    let short = Picker::new(many(3), PickerLayout::Compact { rows: 10 });
    assert_eq!(short.visible_rows(11), 3, "and by the candidates");
}

/// Paging through the list has to bring the rows with it.
#[test]
fn paging_scrolls_the_window() {
    let mut app = app();
    press_control(&mut app, 'f');
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
    assert!(
        text.contains("file-02"),
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
    press_control(&mut app, 'f');
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

/// The glyph is not in the haystack. Nothing a reader types is a private-use
/// codepoint, and having one in there would only skew the scores.
#[test]
fn a_query_matches_the_name_and_not_the_glyph() {
    let mut app = app();
    press_control(&mut app, 'f');
    app.handle(Event::FilesFound {
        generation: 1,
        paths: vec!["src/app.rs".into()],
    });

    type_text(&mut app, "app");
    let dump = support::render(&mut app, 40, 8);
    assert!(support::text_block(&dump).contains("src/app.rs"), "{dump}");
    assert!(
        support::legend_block(&dump).contains("#60a5fa"),
        "the name's matched characters lost their colour:\n{dump}"
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
    let dump = support::render(&mut app, 60, 12);
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
    press_control(&mut app, 'f');
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
    press_control(&mut app, 'f');
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
        assert!(
            drawn.ends_with(' '),
            "the reserved column on the right was written into at width {width}:\n{dump}"
        );
    }
}

/// The matched positions are counted from the start of the whole label, so a
/// truncated head must not shift the colouring along the tail.
#[test]
fn truncation_does_not_move_the_matched_characters() {
    let mut app = app();
    press_control(&mut app, 'f');
    app.handle(Event::FilesFound {
        generation: 1,
        paths: vec!["a/very/deep/directory/tree/leading/to/the_file.rs".into()],
    });

    // A query that matches only in the tail, which is the part still on screen.
    type_text(&mut app, "thefile");
    let dump = support::render(&mut app, 30, 6);
    assert!(support::text_block(&dump).contains("the_file.rs"), "{dump}");
    assert!(
        support::legend_block(&dump).contains("#60a5fa"),
        "the tail's matched characters were not coloured:\n{dump}"
    );
}

/// And a query that matches only in the part that was cut away leaves the row
/// listed with nothing coloured, rather than colouring the wrong characters.
#[test]
fn a_match_in_the_cut_away_head_colours_nothing() {
    let mut app = app();
    press_control(&mut app, 'f');
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
        !support::legend_block(&dump).contains("#60a5fa"),
        "characters outside the match were coloured:\n{dump}"
    );
}

/// The palette shows what each command is bound to, and shows nothing for a
/// command that is bound to nothing — which is worth being able to see, since
/// the palette is then the only way to reach it.
#[test]
fn the_palette_shows_the_key_each_command_is_bound_to() {
    let mut app = app();
    press_control(&mut app, 'p');
    let dump = support::render(&mut app, 60, 12);
    let text = support::text_block(&dump);

    let row = |text: &str, needle: &str| {
        text.lines()
            .find(|row| row.contains(needle))
            .map(str::to_string)
            .unwrap_or_else(|| panic!("{needle:?} is not listed:\n{dump}"))
    };

    assert!(row(text, "file.open").contains("ctrl+f"), "{dump}");
    assert!(
        !row(text, "theme.select").contains("ctrl"),
        "theme.select has no binding, so it should show no key:\n{dump}"
    );
    // The four symbol questions are commands with no key: the menu is the way
    // in, and a chord each would rebuild what the menu replaces.
    assert!(!row(text, "symbol.definition").contains("ctrl"), "{dump}");

    // More commands than a compact list has rows, so the rest are reached by
    // typing rather than by scrolling.
    type_text(&mut app, "quit");
    let narrowed = support::render(&mut app, 60, 12);
    assert!(
        row(support::text_block(&narrowed), "app.quit").contains("ctrl+q"),
        "{narrowed}"
    );
}

/// Right-aligned, so the keys form a column rather than trailing each
/// description at whatever length it happens to be.
#[test]
fn the_keys_line_up_in_a_column() {
    let mut app = app();
    press_control(&mut app, 'p');
    let dump = support::render(&mut app, 60, 12);
    let text = support::text_block(&dump);

    let column_of = |needle: &str| {
        text.lines()
            .find(|row| row.contains(needle))
            .and_then(|row| row.rfind("ctrl+"))
            .unwrap_or_else(|| panic!("no key on the {needle:?} row:\n{dump}"))
    };

    assert_eq!(
        column_of("file.open"),
        column_of("command.palette"),
        "{dump}"
    );
    // Rows within the compact list's ten. There are more commands than that
    // now, and the ones past it are reached by typing rather than scrolling.
    assert_eq!(column_of("file.open"), column_of("symbol.menu"), "{dump}");
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
    let dump = support::render(&mut app, 60, 12);
    let text = support::text_block(&dump);

    assert!(
        text.contains("alt+k"),
        "the rebound key is not shown:\n{dump}"
    );
    assert!(
        !text.contains("ctrl+p"),
        "the old key is still shown:\n{dump}"
    );
    assert!(
        !text.contains("ctrl+f"),
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

/// The symbol menu reads the same key table as the palette, so a question
/// bound to a key shows it in both and a rebind changes both.
#[test]
fn the_symbol_menu_shows_the_key_a_question_is_bound_to() {
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

    // The palette shows it.
    press_control(&mut app, 'p');
    type_text(&mut app, "definition");
    let palette = support::render(&mut app, 60, 12);
    assert!(
        support::text_block(&palette).contains("alt+d"),
        "the palette does not show the binding:\n{palette}"
    );
}

/// No server, so the menu says which of the several reasons it is rather than
/// coming up empty. An empty list says only that it is empty.
#[test]
fn the_symbol_menu_says_why_when_there_is_nothing_to_ask() {
    let mut app = app();
    press_control(&mut app, 'g');
    let dump = support::render(&mut app, 60, 12);
    let text = support::text_block(&dump);

    assert!(
        text.contains("not installed")
            || text.contains("no server running")
            || text.contains("still starting")
            || text.contains("no language server"),
        "the menu gave no reason:\n{dump}"
    );
    assert!(
        !text.contains("symbol.definition"),
        "a question was offered with no server to answer it:\n{dump}"
    );
}

/// The file picker shows what the selection names, below the list, drawn by
/// the editor's own view.
#[test]
fn the_file_picker_previews_the_selected_file() {
    let mut app = app();
    press_control(&mut app, 'f');
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
    press_control(&mut app, 'f');
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

/// A palette of commands has nothing to preview, and a compact list is
/// compact so the code stays visible.
#[test]
fn the_palette_has_no_preview() {
    let mut app = app();
    press_control(&mut app, 'p');
    let dump = support::render(&mut app, 60, 22);
    assert!(
        !support::text_block(&dump).contains('\u{2500}'),
        "a rule was drawn for a list with nothing to preview:\n{dump}"
    );
}

/// A preview that leaves three candidates showing has stopped being a preview
/// and become the thing in the way of the list.
#[test]
fn a_short_screen_gets_the_list_and_no_preview() {
    let mut app = app();
    press_control(&mut app, 'f');
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
    press_control(&mut app, 'f');
    app.handle(Event::FilesFound {
        generation: 1,
        paths: vec![
            "tests/fixtures/long.rs".into(),
            "tests/fixtures/sample.rs".into(),
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

/// A list of references is read by looking at the symbol in each one, so the
/// preview marks it. Saying only which line leaves the reader finding it
/// again on every row.
#[test]
fn a_place_preview_marks_the_symbol_it_is_about() {
    use obelus::picker::{PickerItem, PickerLayout, PickerValue};

    let mut app = app();
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/long.rs");
    // Line 1 of `long.rs` is the long `const NAMES` line; `NAMES` is at
    // characters six to eleven.
    app.open_picker_for_test(
        vec![PickerItem {
            icon: None,
            label: "long.rs:2:7".to_string(),
            detail: None,
            trailing: None,
            value: PickerValue::Place {
                path,
                line: 1,
                character: 6,
                end_line: 1,
                end_character: 11,
            },
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
    press_control(&mut app, 'f');
    app.handle(Event::FilesFound {
        generation: 1,
        paths: vec!["tests/fixtures/many_lines.rs".into()],
    });

    // A file with more lines than either preview has room for, so the count
    // is the room rather than the file.
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
            .position(|row| row.contains('\u{2500}'))
            .expect("a rule")
    };
    assert_eq!(rows_of_list(&short), rows_of_list(&tall));

    support::check("preview_tall_60x34", &tall);
}

/// The plain keys page the list; the same keys with control scroll the
/// preview. Reading a candidate and choosing between candidates are different
/// jobs, and a list of references is read by doing both at once.
#[test]
fn control_paging_scrolls_the_preview_and_not_the_list() {
    let mut app = app();
    press_control(&mut app, 'f');
    app.handle(Event::FilesFound {
        generation: 1,
        paths: vec!["tests/fixtures/many_lines.rs".into()],
    });

    let at_rest = support::render(&mut app, 60, 22);
    assert!(
        support::text_block(&at_rest).contains("LINE_01"),
        "{at_rest}"
    );

    support::press_control_key(&mut app, KeyCode::PageDown);
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

    support::press_control_key(&mut app, KeyCode::PageUp);
    assert_eq!(
        support::text_block(&support::render(&mut app, 60, 22)),
        support::text_block(&at_rest),
        "scrolling back did not come back"
    );
}

/// The top of the file is as far up as it goes, and pressing past it does not
/// bank a debt: one press the other way moves. Letting the offset run past the
/// top spends the next several presses coming back with nothing happening.
#[test]
fn the_preview_stops_at_the_top_of_the_file() {
    let mut app = app();
    press_control(&mut app, 'f');
    app.handle(Event::FilesFound {
        generation: 1,
        paths: vec!["tests/fixtures/many_lines.rs".into()],
    });

    let at_rest = support::text_block(&support::render(&mut app, 60, 22)).to_string();
    for _ in 0..5 {
        support::press_control_key(&mut app, KeyCode::PageUp);
    }
    assert_eq!(
        support::text_block(&support::render(&mut app, 60, 22)),
        at_rest,
        "the top of the file is not where it stopped"
    );

    support::press_control_key(&mut app, KeyCode::PageDown);
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
    use obelus::picker::{PickerItem, PickerLayout, PickerValue};

    let path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/many_lines.rs");
    let place = |line: u32| PickerItem {
        icon: None,
        label: format!("many_lines.rs:{}", line + 1),
        detail: None,
        trailing: None,
        value: PickerValue::Place {
            path: path.clone(),
            line,
            character: 10,
            end_line: line,
            end_character: 17,
        },
    };

    let mut app = app();
    app.open_picker_for_test(vec![place(1), place(2)], PickerLayout::FullArea);

    assert!(support::text_block(&support::render(&mut app, 60, 22)).contains("LINE_01"));
    support::press_control_key(&mut app, KeyCode::PageDown);
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
