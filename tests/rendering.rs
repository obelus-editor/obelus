//! What the editor region and the status bar actually put on screen.

mod support;

use crossterm::event::KeyCode;
use obelus::{
    app::App,
    coordinates::{CharColumn, LineNumber},
};
use support::{press, press_shift};

/// The screen these tests use unless they say otherwise.
const WIDTH: u16 = 40;
const HEIGHT: u16 = 8;

/// A file with a tab-indented body and a wide glyph, so the gutter, tab
/// expansion and double-width cells are all exercised by the same fixture.
fn app() -> App {
    App::new(vec![support::open_fixture("sample.rs")])
}

/// An application whose geometry is already known.
///
/// The loop draws before it waits for a key, so the geometry always exists by
/// the time a key arrives. A test that presses first would be moving a cursor
/// through a zero-sized screen — which, with wrapping, is a screen one cell
/// wide, and every line in it is one character tall.
fn app_on_screen(width: u16, height: u16) -> App {
    let mut app = app();
    support::lay_out(&mut app, width, height);
    app
}

#[test]
fn a_file_renders_with_a_gutter_and_a_status_bar() {
    let mut app = app();
    support::check("sample_40x8", &support::render(&mut app, 40, 8));
}

#[test]
fn the_cursor_line_number_is_brighter_than_the_others() {
    let mut app = app_on_screen(WIDTH, HEIGHT);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Down);
    support::check(
        "sample_cursor_line_3_40x8",
        &support::render(&mut app, 40, 8),
    );
}

#[test]
fn a_narrow_screen_truncates_the_path_from_the_left() {
    let mut app = app();
    support::check("sample_narrow_24x6", &support::render(&mut app, 24, 6));
}

/// Starting with nothing open is the one moment a reader needs telling what
/// the keys are.
#[test]
fn starting_with_nothing_open_shows_a_welcome_screen() {
    let mut app = App::new(Vec::new());
    support::check("welcome_64x20", &support::render(&mut app, 64, 20));
}

/// A screen with no room for the wordmark still gets the keys.
#[test]
fn a_narrow_screen_gets_the_keys_without_the_wordmark() {
    let mut app = App::new(Vec::new());
    let dump = support::render(&mut app, 34, 10);

    let control_o = obelus::keymap::control('o').label();
    assert!(support::text_block(&dump).contains(&control_o), "{dump}");
    assert!(
        !support::text_block(&dump).contains('\u{2588}'),
        "the wordmark was drawn into a screen too narrow for it:\n{dump}"
    );
    support::check("welcome_narrow_34x10", &dump);
}

/// The keys on it come from the key table, so a rebound key changes the screen
/// rather than leaving a hint that lies.
#[test]
fn the_welcome_screen_reads_the_key_table() {
    use crossterm::event::{KeyCode, KeyModifiers};
    use obelus::{
        command::Command,
        keymap::{Binding, Context, KeyChord, Keymap},
    };

    let mut app = App::new(Vec::new());
    let shown = support::render(&mut app, 64, 20);
    let control_o = KeyChord::new(KeyCode::Char('o'), KeyModifiers::CONTROL).label();
    assert!(support::text_block(&shown).contains(&control_o), "{shown}");

    app.set_keymap(Keymap::from_bindings(vec![Binding {
        command: Command::FileOpen,
        context: Context::Normal,
        chord: KeyChord::new(KeyCode::Char('z'), KeyModifiers::ALT),
    }]));
    let rebound = support::render(&mut app, 64, 20);
    let text = support::text_block(&rebound);

    assert!(
        text.contains(&KeyChord::new(KeyCode::Char('z'), KeyModifiers::ALT).label()),
        "the rebound key is not shown:\n{rebound}"
    );
    assert!(
        !text.contains(&control_o),
        "the old key is still being offered:\n{rebound}"
    );
    assert!(
        !text.contains(&KeyChord::new(KeyCode::Char('q'), KeyModifiers::CONTROL).label()),
        "a command with no binding left is still listed:\n{rebound}"
    );
}

/// Too small to read at a glance is worse than nothing.
#[test]
fn a_screen_too_small_for_the_welcome_block_shows_none_of_it() {
    let mut app = App::new(Vec::new());
    support::check("empty_20x4", &support::render(&mut app, 20, 4));
}

#[test]
fn the_status_bar_reports_a_one_based_character_column() {
    let mut app = app_on_screen(WIDTH, HEIGHT);
    press(&mut app, KeyCode::Down);
    for _ in 0..17 {
        press(&mut app, KeyCode::Right);
    }

    let cursor = app.current_buffer().expect("a buffer").cursor();
    assert_eq!(cursor.line, LineNumber::new(1));
    // Seventeen characters in, which is past the wide pair: the cell column
    // there is nineteen, and the status bar must not report that one.
    assert_eq!(cursor.column, CharColumn::new(17));
}

/// Switching theme must change colours and nothing else.
///
/// The claim being tested is architectural: what is cached per byte is which
/// kind of thing it is, not what colour, so a theme change is one field and
/// the next frame. If a reparse or a cache were involved, the cheapest way for
/// that to go wrong is for the text to come out different too.
#[test]
fn switching_theme_repaints_without_moving_anything() {
    let mut app = app();
    let dark = support::render(&mut app, 40, 8);

    app.set_theme(&obelus::theme::builtin::LIGHT);
    let light = support::render(&mut app, 40, 8);

    assert_eq!(
        support::text_block(&dark),
        support::text_block(&light),
        "the theme moved something"
    );
    // Even the style *structure* is identical: the same runs of cells share a
    // style, because which kind of thing each byte is did not change. Only the
    // colours those styles resolve to did.
    assert_eq!(
        support::style_block(&dark),
        support::style_block(&light),
        "the theme regrouped the styles, so more than the colours changed"
    );
    assert_ne!(
        support::legend_block(&dark),
        support::legend_block(&light),
        "the theme changed nothing"
    );

    support::check("sample_light_40x8", &light);
}

#[test]
fn syntax_is_highlighted() {
    let mut app = app();
    let dump = support::render(&mut app, 40, 8);
    let legend = support::legend_block(&dump);

    // Six distinct foregrounds at least: plain text, the gutter, the cursor's
    // gutter line, the status bar, and at least two syntax colours. Without
    // highlighting there would be four.
    let colours = legend.lines().filter(|line| !line.is_empty()).count();
    assert!(
        colours >= 6,
        "only {colours} styles on screen, so nothing is being highlighted:\n{legend}"
    );
}

/// The cursor is the terminal's own, positioned rather than painted. That is
/// what makes it take the shape and blink the reader configured, and what
/// makes it go hollow by itself when the window loses focus — a cell grid
/// cannot express an outline over a cell.
#[test]
fn the_terminal_is_told_where_to_put_its_cursor() {
    let mut app = app();
    let dump = support::render(&mut app, WIDTH, HEIGHT);

    // A five-cell gutter, so the first character of the first line.
    assert_eq!(support::cursor_line(&dump), "5,0", "{dump}");
    support::check("sample_40x8", &dump);
}

#[test]
fn the_cursor_follows_the_keys() {
    let mut app = app_on_screen(WIDTH, HEIGHT);
    press(&mut app, KeyCode::Down);
    for _ in 0..5 {
        press(&mut app, KeyCode::Right);
    }

    // Line 1 is `\tlet greeting = ...`. The tab is four cells, then `let `
    // is four more, so character five sits at cell eight — and the gutter is
    // five columns before that. The tab is why the two numbers differ.
    let dump = support::render(&mut app, WIDTH, HEIGHT);
    assert_eq!(support::cursor_line(&dump), "13,1", "{dump}");
}

/// Shift extends the selection from where the reader began, while an ordinary
/// motion starts a new place and clears it.
#[test]
fn shift_arrows_select_and_plain_motion_clears_the_selection() {
    let mut app = app_on_screen(WIDTH, HEIGHT);
    press_shift(&mut app, KeyCode::Right);
    press_shift(&mut app, KeyCode::Right);

    let selected = app.current_buffer().and_then(|buffer| buffer.selection());
    assert_eq!(
        selected,
        Some(obelus::coordinates::Span {
            line: LineNumber::new(0),
            column: CharColumn::new(0),
            end_line: LineNumber::new(0),
            end_column: CharColumn::new(2),
        })
    );
    assert_eq!(
        app.current_buffer()
            .and_then(|buffer| buffer.selected_text()),
        Some("fn".to_string())
    );

    let selected_dump = support::render(&mut app, WIDTH, HEIGHT);
    // The two selected characters, found by their text rather than by a
    // column: what is to the left of them -- the margin, the gutter -- is
    // not what this test is about.
    let row = support::text_block(&selected_dump)
        .lines()
        .find(|row| row.contains("fn main"))
        .expect("the first row");
    let styles = support::style_block(&selected_dump)
        .lines()
        .find(|row| !row.is_empty())
        .expect("its styles");
    let at = row.find("fn main").expect("the code");
    let style_at = |x: usize| styles.chars().nth(x).expect("a style cell");

    assert_eq!(
        style_at(at),
        style_at(at + 1),
        "the two selected characters do not match:\n{selected_dump}"
    );
    assert_ne!(
        style_at(at),
        style_at(at + 2),
        "the character after the selection is selected too:\n{selected_dump}"
    );

    press(&mut app, KeyCode::Right);
    let buffer = app.current_buffer().expect("a buffer");
    assert!(
        buffer.selection().is_none(),
        "the selection survived a plain move"
    );
    assert_eq!(buffer.cursor().column, CharColumn::new(3));
}

/// Copying without a selection reports the reason before opening a clipboard,
/// which also makes the answer useful on a headless machine.
#[test]
fn copying_without_a_selection_explains_why() {
    let mut app = app_on_screen(WIDTH, HEIGHT);
    support::press_control(&mut app, 'c');
    assert_eq!(app.note(), Some("nothing selected"));
}

/// Shift applies equally to the line and document-sized motions. A page moves
/// the cursor, so it extends a selection rather than merely moving the view.
#[test]
fn shift_home_end_and_paging_adjust_the_selection() {
    let mut line = app_on_screen(WIDTH, HEIGHT);
    for _ in 0..3 {
        press(&mut line, KeyCode::Right);
    }
    press_shift(&mut line, KeyCode::Home);
    assert_eq!(
        line.current_buffer().and_then(|buffer| buffer.selection()),
        Some(obelus::coordinates::Span {
            line: LineNumber::new(0),
            column: CharColumn::new(0),
            end_line: LineNumber::new(0),
            end_column: CharColumn::new(3),
        })
    );
    press_shift(&mut line, KeyCode::End);
    assert_eq!(
        line.current_buffer()
            .expect("a buffer")
            .selection()
            .expect("a selection")
            .end_column,
        line.current_buffer()
            .expect("a buffer")
            .text()
            .line_length(LineNumber::new(0))
    );

    let mut page = App::new(vec![support::open_fixture("long.rs")]);
    support::lay_out(&mut page, 40, 6);
    press_shift(&mut page, KeyCode::PageDown);
    let selected = page
        .current_buffer()
        .and_then(|buffer| buffer.selection())
        .expect("shift page down selected text");
    assert!(selected.end_line > selected.line, "{selected:?}");

    let mut page_up = App::new(vec![support::open_fixture("long.rs")]);
    support::lay_out(&mut page_up, 40, 6);
    press(&mut page_up, KeyCode::PageDown);
    press_shift(&mut page_up, KeyCode::PageUp);
    assert!(
        page_up
            .current_buffer()
            .and_then(|buffer| buffer.selection())
            .is_some(),
        "shift page up did not select text"
    );
}

/// Nothing to put a cursor in.
#[test]
fn an_empty_application_hides_the_cursor() {
    let mut app = App::new(Vec::new());
    let dump = support::render(&mut app, 20, 4);
    assert_eq!(support::cursor_line(&dump), "none", "{dump}");
}

/// With a picker open the cursor belongs in the prompt, which is where the
/// keys are going. One cursor, and the terminal moves it.
#[test]
fn a_picker_takes_the_cursor_into_its_prompt() {
    let mut app = app_on_screen(60, 12);
    support::press_control(&mut app, 'p');
    let dump = support::render(&mut app, 60, 12);

    // Inside the prompt on the status row, past the padding and the prompt
    // itself. How wide the prompt is depends on whether it is a glyph or a
    // `>`, so what is pinned here is that the caret is in it and that it
    // follows what is typed.
    let at = |dump: &str| {
        let (x, y) = support::cursor_line(dump)
            .split_once(',')
            .expect("a cursor position");
        (
            x.parse::<usize>().expect("a column"),
            y.parse::<usize>().expect("a row"),
        )
    };
    let (column, row) = at(&dump);
    assert_eq!(row, 11, "the caret is not on the status row:\n{dump}");
    assert!(column >= 2, "the caret is in the padding:\n{dump}");

    support::type_text(&mut app, "theme");
    let dump = support::render(&mut app, 60, 12);
    assert_eq!(
        at(&dump),
        (column + "theme".len(), row),
        "the caret did not follow the query:\n{dump}"
    );
}

/// A wrapped line puts the cursor on the row it actually wrapped onto.
#[test]
fn the_cursor_follows_a_wrapped_line_down_its_rows() {
    let mut app = App::new(vec![support::open_fixture("long.rs")]);
    support::lay_out(&mut app, 40, 10);

    press(&mut app, KeyCode::Down);
    let first = support::render(&mut app, 40, 10);
    assert_eq!(support::cursor_line(&first), "5,1", "{first}");

    press(&mut app, KeyCode::Down);
    let second = support::render(&mut app, 40, 10);
    // The second row of the long line, indented to nothing since the line has
    // no indentation of its own.
    assert_eq!(support::cursor_line(&second), "5,2", "{second}");
}

#[test]
fn home_and_end_move_to_the_ends_of_the_line() {
    let mut app = app_on_screen(WIDTH, HEIGHT);
    press(&mut app, KeyCode::Down);

    press(&mut app, KeyCode::End);
    let cursor = app.current_buffer().expect("a buffer").cursor();
    // `\tlet greeting = "你好";` is twenty-one characters.
    assert_eq!(cursor.column, CharColumn::new(21));

    press(&mut app, KeyCode::Home);
    assert_eq!(
        app.current_buffer().expect("a buffer").cursor().column,
        CharColumn::new(0)
    );
}

/// Home and End are horizontal, so they replace the column the cursor is
/// aiming for while moving vertically rather than preserving it.
#[test]
fn end_then_down_keeps_the_end_column_it_landed_on() {
    let mut app = app_on_screen(WIDTH, HEIGHT);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::End);
    press(&mut app, KeyCode::Down);

    let cursor = app.current_buffer().expect("a buffer").cursor();
    assert_eq!(cursor.line, LineNumber::new(2));
    // Line 2 is longer than line 1, so the remembered cell is line 1's end,
    // not line 2's.
    let text = app.current_buffer().expect("a buffer").text();
    assert!(
        cursor.column < text.line_length(LineNumber::new(2)),
        "End moved the aim to the far right rather than to line one's end"
    );
}

/// Wrapping is on: a line longer than the screen continues on the next row
/// rather than running off the edge.
#[test]
fn a_long_line_continues_on_the_next_row() {
    let mut app = App::new(vec![support::open_fixture("long.rs")]);
    support::check("long_40x10", &support::render(&mut app, 40, 10));
}

/// Only the first row of a wrapped line is numbered. Repeating the number on
/// every row is how a wrapped view stops being readable.
#[test]
fn continuation_rows_have_no_line_number() {
    let mut app = App::new(vec![support::open_fixture("long.rs")]);
    let dump = support::render(&mut app, 40, 10);
    let text = support::text_block(&dump);

    // The numbers that appear, in the order they appear. Found by looking
    // for a number rather than at a fixed column: what is to the left of the
    // gutter depends on whether obelus has anything to say about the file,
    // and how many rows a long line wraps into depends on how wide the text
    // is -- neither of which this test is about.
    let numbers: Vec<u32> = text
        .lines()
        .filter(|row| !row.is_empty())
        .filter_map(|row| {
            row.chars()
                .skip(3)
                .take_while(|character| !character.is_alphabetic())
                .collect::<String>()
                .trim()
                .parse::<u32>()
                .ok()
        })
        .collect();
    let rows = text.lines().filter(|row| !row.is_empty()).count();

    // One number per line, counting from one, and no number repeated: the
    // rows in between belong to a line that already has its number.
    assert!(numbers.len() >= 2, "not enough lines to tell:\n{dump}");
    assert!(
        numbers.len() < rows - 1,
        "every row is numbered, so the long line was numbered on each of \
         its rows:\n{dump}"
    );
    assert!(
        numbers.windows(2).all(|pair| pair[1] > pair[0]),
        "a number repeated:\n{dump}"
    );
    assert_eq!(numbers[0], 1, "{dump}");
}

/// Down steps one visual row, not one line. With a line many rows tall,
/// stepping over all of them at once is not what pressing down once looks
/// like it should do.
#[test]
fn down_moves_by_one_visual_row_inside_a_wrapped_line() {
    let mut app = App::new(vec![support::open_fixture("long.rs")]);
    support::lay_out(&mut app, 40, 10);

    press(&mut app, KeyCode::Down);
    let cursor = app.current_buffer().expect("a buffer").cursor();
    assert_eq!(cursor.line, LineNumber::new(1), "onto the long line");
    assert_eq!(cursor.column, CharColumn::new(0), "at its first row");

    press(&mut app, KeyCode::Down);
    let cursor = app.current_buffer().expect("a buffer").cursor();
    assert_eq!(
        cursor.line,
        LineNumber::new(1),
        "still on the long line, one row further down"
    );
    assert!(
        cursor.column.get() > 0,
        "the cursor did not move into the line's second row"
    );
}

/// A line taller than the screen has to be scrollable through, which is why
/// the viewport is anchored to a row and not only to a line.
#[test]
fn a_line_taller_than_the_screen_can_be_scrolled_through() {
    let mut app = App::new(vec![support::open_fixture("long.rs")]);
    // Four rows of text, and the long line takes more than that.
    support::lay_out(&mut app, 20, 5);

    let first = support::render(&mut app, 20, 5);
    for _ in 0..6 {
        press(&mut app, KeyCode::Down);
    }
    let scrolled = support::render(&mut app, 20, 5);

    let viewport = app.current_buffer().expect("a buffer").viewport();
    assert_eq!(
        viewport.top,
        LineNumber::new(1),
        "still inside the long line"
    );
    assert!(
        viewport.top_row > 0,
        "the viewport is still anchored to the line's first row"
    );
    assert_ne!(
        support::text_block(&first),
        support::text_block(&scrolled),
        "the screen did not move:\n{scrolled}"
    );
}

/// A wrapped statement keeps its indentation, so it still reads as being
/// inside its block rather than as a new top-level line.
#[test]
fn continuation_rows_keep_the_lines_indentation() {
    let mut app = App::new(vec![support::open_fixture("indented.rs")]);
    support::check("indented_46x8", &support::render(&mut app, 46, 8));
}

/// `ctrl+Home` and `ctrl+End` reach the ends of the document; the plain keys
/// stay on the line.
#[test]
fn control_home_and_end_reach_the_ends_of_the_document() {
    let mut app = App::new(vec![support::open_fixture("long.rs")]);
    support::lay_out(&mut app, 40, 6);

    support::press_control_key(&mut app, KeyCode::End);
    let buffer = app.current_buffer().expect("a buffer");
    assert_eq!(buffer.cursor().line, buffer.text().last_line());
    assert_eq!(buffer.cursor().column, CharColumn::new(0));

    support::press_control_key(&mut app, KeyCode::Home);
    let buffer = app.current_buffer().expect("a buffer");
    assert_eq!(buffer.cursor().line, LineNumber::new(0));
    assert_eq!(buffer.cursor().column, CharColumn::new(0));
}

/// The plain keys stay on the line. If the modifier were ignored, `End` would
/// leave the line it is on and there would be no way to reach the end of one.
#[test]
fn plain_home_and_end_stay_on_the_line() {
    let mut app = App::new(vec![support::open_fixture("long.rs")]);
    support::lay_out(&mut app, 40, 6);

    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::End);
    let buffer = app.current_buffer().expect("a buffer");
    assert_eq!(
        buffer.cursor().line,
        LineNumber::new(1),
        "a plain End jumped to the end of the document"
    );
    assert!(
        buffer.cursor().column.get() > 0,
        "and it moved along the line"
    );
}

/// Reaching the end has to bring the screen with it.
#[test]
fn the_end_of_the_file_is_on_screen_after_control_end() {
    let mut app = App::new(vec![support::open_fixture("long.rs")]);
    support::lay_out(&mut app, 40, 6);

    support::press_control_key(&mut app, KeyCode::End);
    let dump = support::render(&mut app, 40, 6);

    assert!(
        support::text_block(&dump).contains("fn after()"),
        "the end of the file is not on screen:\n{dump}"
    );
    assert!(
        !support::text_block(&dump).contains("fn before()"),
        "the screen never moved:\n{dump}"
    );
}

/// The keys are right-aligned into their column so the descriptions start
/// together. Invisible with the shipped bindings, which all happen to be six
/// characters wide — and about to matter the moment one of them is rebound.
#[test]
fn the_welcome_screen_lines_up_keys_of_different_widths() {
    use crossterm::event::{KeyCode, KeyModifiers};
    use obelus::{
        command::Command,
        keymap::{Binding, Context, KeyChord, Keymap},
    };

    let mut app = App::new(Vec::new());
    app.set_keymap(Keymap::from_bindings(vec![
        Binding {
            command: Command::FileOpen,
            context: Context::Normal,
            // Eleven characters against four.
            chord: KeyChord::new(
                KeyCode::Char('o'),
                KeyModifiers::CONTROL | KeyModifiers::ALT,
            ),
        },
        Binding {
            command: Command::Quit,
            context: Context::Always,
            chord: KeyChord::new(KeyCode::Esc, KeyModifiers::NONE),
        },
    ]));

    let dump = support::render(&mut app, 64, 20);
    let text = support::text_block(&dump);
    // Counted in characters, not bytes: a glyph from the private use area is
    // four bytes and one column, so byte offsets on two rows with different
    // numbers of glyphs are not comparable.
    let column_of = |needle: &str| {
        text.lines()
            .find_map(|row| row.find(needle).map(|byte| row[..byte].chars().count()))
            .unwrap_or_else(|| panic!("{needle:?} is not on screen:\n{dump}"))
    };

    assert_eq!(
        column_of("open a file"),
        column_of("leave obelus"),
        "the descriptions do not start together:\n{dump}"
    );
    // The wider chord's key starts further left, which is what right-aligning
    // a column of keys means.
    let wide = KeyChord::new(
        KeyCode::Char('o'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    )
    .label();
    let narrow = KeyChord::new(KeyCode::Esc, KeyModifiers::NONE).label();
    assert!(column_of(&narrow) > column_of(&wide), "{dump}");
}

/// A motion key held with a modifier obelus has no meaning for does nothing.
/// Reading it as the plain key would make `super+End` jump somewhere the
/// reader did not ask to go, and `ctrl+super+Home` leave the file entirely.
#[test]
fn a_motion_with_an_unknown_modifier_does_not_move() {
    use crossterm::event::{KeyEvent, KeyModifiers};

    let mut app = App::new(vec![support::open_fixture("long.rs")]);
    support::lay_out(&mut app, 40, 6);

    press(&mut app, KeyCode::Down);
    let buffer = app.current_buffer().expect("a buffer");
    let (line, column) = (buffer.cursor().line, buffer.cursor().column);

    for (code, modifier) in [
        (KeyCode::Down, KeyModifiers::SUPER),
        (KeyCode::End, KeyModifiers::SUPER),
        (KeyCode::PageDown, KeyModifiers::HYPER),
        (KeyCode::Home, KeyModifiers::CONTROL | KeyModifiers::SUPER),
        (KeyCode::End, KeyModifiers::CONTROL | KeyModifiers::META),
    ] {
        app.handle(obelus::event::Event::Key(KeyEvent::new(code, modifier)));
        let buffer = app.current_buffer().expect("a buffer");
        assert_eq!(
            (buffer.cursor().line, buffer.cursor().column),
            (line, column),
            "{code:?} with {modifier:?} moved the cursor"
        );
    }
}

/// Arriving at a place a server named puts it in the middle of the screen.
/// Scrolling the least amount would leave the definition on the bottom row
/// with everything above it -- its signature, its doc comment -- off screen,
/// which is the half the reader came for.
#[test]
fn a_jump_lands_in_the_middle_of_the_screen() {
    use obelus::component::picker::{PickerItem, PickerLayout, PickerValue};

    let path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/many_lines.rs");
    let mut app = App::new(vec![support::open_fixture("many_lines.rs")]);
    support::lay_out(&mut app, 40, 12);

    app.open_picker_for_test(
        vec![PickerItem {
            icon: None,
            label: "many_lines.rs:31".to_string(),
            detail: None,
            trailing: None,
            value: PickerValue::Place {
                path,
                line: 30,
                character: 10,
                end_line: 30,
                end_character: 17,
            },
            status: None,
            depth: 0,
            kind: None,
            tab: None,
        }],
        PickerLayout::FullArea,
    );
    press(&mut app, KeyCode::Enter);

    // Eleven rows of text under the status bar, so the middle one is the
    // sixth: five rows of what leads up to the definition.
    let dump = support::render(&mut app, 40, 12);
    assert_eq!(support::cursor_line(&dump), "15,5", "{dump}");

    let rows: Vec<&str> = support::text_block(&dump)
        .lines()
        .filter(|row| !row.is_empty())
        .collect();
    assert!(
        rows[5].contains("LINE_30"),
        "the definition is not there:\n{dump}"
    );
    assert!(
        rows[0].contains("LINE_25"),
        "what leads up to it is not there:\n{dump}"
    );
}

/// Coming back from a jump is arriving too, so the line the reader left gets
/// the middle of the screen as well. Scrolling the least amount would put it
/// on the top row, which is the same loss of context the other way up.
#[test]
fn a_jump_back_lands_in_the_middle_too() {
    use crossterm::event::{KeyEvent, KeyModifiers};
    use obelus::component::picker::{PickerItem, PickerLayout, PickerValue};

    let path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/many_lines.rs");
    let mut app = App::new(vec![support::open_fixture("many_lines.rs")]);
    support::lay_out(&mut app, 40, 12);

    // Away from line one, where centring has nowhere to go and would look
    // the same as not centring at all. Eight *visual* rows, which is some
    // line further down: the doc comment at the top wraps, and how many rows
    // it wraps into depends on the width -- so the line it lands on is read
    // back rather than assumed.
    for _ in 0..8 {
        press(&mut app, KeyCode::Down);
    }
    let left_behind = app.current_buffer().expect("a buffer").cursor().line.get();
    app.open_picker_for_test(
        vec![PickerItem {
            icon: None,
            label: "many_lines.rs:31".to_string(),
            detail: None,
            trailing: None,
            value: PickerValue::Place {
                path,
                line: 30,
                character: 10,
                end_line: 30,
                end_character: 17,
            },
            status: None,
            depth: 0,
            kind: None,
            tab: None,
        }],
        PickerLayout::FullArea,
    );
    press(&mut app, KeyCode::Enter);

    app.handle(obelus::event::Event::Key(KeyEvent::new(
        KeyCode::Left,
        KeyModifiers::ALT,
    )));
    let dump = support::render(&mut app, 40, 12);
    // The middle row of eleven. The column depends on the gutter and on
    // whether there is a margin, neither of which this test is about.
    let (_, row) = support::cursor_line(&dump)
        .split_once(',')
        .expect("the cursor is on screen");
    assert_eq!(row.parse::<u16>().expect("a row"), 5, "{dump}");

    let rows: Vec<&str> = support::text_block(&dump)
        .lines()
        .filter(|row| !row.is_empty())
        .collect();
    // `many_lines.rs` names each line after itself, so the line left behind
    // says which line it is.
    let name = format!("LINE_{left_behind:02}");
    assert!(
        rows[5].contains(&name),
        "{name} is not in the middle:\n{dump}"
    );
    // And there are rows above it: the least amount of scrolling would have
    // put it on the top row and thrown them away.
    assert!(
        rows[0].contains("LINE_") && !rows[0].contains(&name),
        "nothing above it:\n{dump}"
    );
}

/// The wordmark's colours travel with time. One tick, one step: the ramp runs
/// out and back over a fixed number of steps, so it slides across the letters
/// without a seam and comes back to where it began.
#[test]
fn the_wordmark_shimmers_and_comes_back_round() {
    let mut app = App::new(Vec::new());
    let first = support::render(&mut app, 64, 20);

    app.handle(obelus::event::Event::Tick);
    let moved = support::render(&mut app, 64, 20);
    // The legend, not the style map: the map's letters are handed out per
    // distinct style in the order they are met, so a palette that has slid
    // along one step gets the same letters in the same places and only the
    // colours behind them differ.
    assert_ne!(
        support::legend_block(&moved),
        support::legend_block(&first),
        "a tick did not move the colours"
    );
    assert_eq!(
        support::text_block(&moved),
        support::text_block(&first),
        "a tick moved something other than the colours"
    );

    // Round the cycle. Sixteen steps out and back, so the seventeenth frame
    // is the first one again -- a ramp that wrapped instead would have a
    // visible edge crossing the letters.
    for _ in 1..16 {
        app.handle(obelus::event::Event::Tick);
    }
    let round = support::render(&mut app, 64, 20);
    assert_eq!(
        support::legend_block(&round),
        support::legend_block(&first),
        "the cycle does not close"
    );
    assert_eq!(support::style_block(&round), support::style_block(&first));

    // Out and back, not round. The ramp runs to its far colour and returns,
    // so every frame of the cycle is a gradient. Letting the position run
    // past the end and clamping it there instead would leave whole frames
    // painted in one flat colour -- which is what a wrapped ramp looks like
    // from inside.
    let mut app = App::new(Vec::new());
    for phase in 0..16 {
        let dump = support::render(&mut app, 64, 20);
        let text: Vec<&str> = support::text_block(&dump).lines().collect();
        let styles: Vec<&str> = support::style_block(&dump).lines().collect();
        let colours: std::collections::HashSet<char> = text
            .iter()
            .zip(&styles)
            .filter(|(row, _)| row.contains('\u{2588}'))
            .flat_map(|(row, style)| {
                // Only the cells the wordmark painted: the rest of the row is
                // the background, in the background's own style.
                row.chars()
                    .zip(style.chars())
                    .filter(|(character, _)| *character == '\u{2588}')
                    .map(|(_, letter)| letter)
            })
            .collect();
        assert!(
            colours.len() > 2,
            "frame {phase} of the cycle is nearly flat: {colours:?}\n{dump}"
        );
        app.handle(obelus::event::Event::Tick);
    }
}

/// Nothing animates behind an open file. The ticker is dropped when one
/// opens, and even a tick that arrives before it stops must leave the screen
/// alone: a redraw a reader did not ask for can only get in the way.
#[test]
fn a_tick_changes_nothing_once_a_file_is_open() {
    let mut app = app();
    support::lay_out(&mut app, 40, 8);
    let before = support::render(&mut app, 40, 8);

    for _ in 0..5 {
        app.handle(obelus::event::Event::Tick);
    }
    assert_eq!(support::render(&mut app, 40, 8), before);
}

/// Paging takes the cursor along, keeping its place *on the screen*: the row
/// of the window it was on is the row it is on after the page. A cursor left
/// behind means the next arrow key throws the page away; a cursor dropped at
/// the top of the new screen loses where on the page you were.
#[test]
fn paging_keeps_the_cursor_on_the_same_row_of_the_screen() {
    let mut app = App::new(vec![support::open_fixture("many_lines.rs")]);
    support::lay_out(&mut app, 40, 12);
    // Down to the middle of the screen, so keeping the row is visible.
    for _ in 0..5 {
        press(&mut app, KeyCode::Down);
    }

    let row_of = |dump: &str| {
        support::cursor_line(dump)
            .split_once(',')
            .map(|(_, y)| y.parse::<u16>().expect("a row"))
            .expect("the cursor is on screen")
    };
    let line_of = |app: &App| app.current_buffer().expect("a buffer").cursor().line.get();

    let before = support::render(&mut app, 40, 12);
    let (row, line) = (row_of(&before), line_of(&app));
    assert_eq!(row, 5, "not where this test meant to start:\n{before}");

    press(&mut app, KeyCode::PageDown);
    let after = support::render(&mut app, 40, 12);
    assert_eq!(
        row_of(&after),
        row,
        "the cursor changed rows on the screen:\n{after}"
    );
    assert!(
        line_of(&app) > line,
        "the cursor did not come along: {} then {}",
        line,
        line_of(&app)
    );
    // A screenful further down the file, which is what a page is.
    assert_eq!(line_of(&app) - line, 11, "not a screenful");

    // And back, to exactly where it was: a page down and a page up is
    // nowhere.
    press(&mut app, KeyCode::PageUp);
    let back = support::render(&mut app, 40, 12);
    assert_eq!(line_of(&app), line);
    assert_eq!(
        support::text_block(&back),
        support::text_block(&before),
        "the screen did not come back"
    );
}

/// Paging up at the top of a file, and down at the end, stop there. A page
/// that ran off would leave a screen of nothing with no way to tell why.
#[test]
fn paging_stops_at_both_ends() {
    let mut app = App::new(vec![support::open_fixture("many_lines.rs")]);
    support::lay_out(&mut app, 40, 12);

    let start = support::render(&mut app, 40, 12);
    for _ in 0..3 {
        press(&mut app, KeyCode::PageUp);
    }
    assert_eq!(
        support::text_block(&support::render(&mut app, 40, 12)),
        support::text_block(&start),
        "paging up from the first line moved somewhere"
    );

    for _ in 0..20 {
        press(&mut app, KeyCode::PageDown);
    }
    let far = support::render(&mut app, 40, 12);
    assert!(
        support::text_block(&far).contains("LINE_38"),
        "paging down ran past the end of the file:\n{far}"
    );
}

/// The thumb reaches the bottom of the track when the last line is on
/// screen. That is the one position a reader checks it against: a bar that
/// stops short says there is more below when there is not.
#[test]
fn the_scrollbar_reaches_both_ends() {
    let mut app = App::new(vec![support::open_fixture("many_lines.rs")]);
    support::lay_out(&mut app, 40, 12);

    // The thumb, wherever the bar is: it is no longer the last column, since
    // the map of changes has one of its own to the right of it.
    let thumb_rows = |dump: &str| -> Vec<usize> {
        support::text_block(dump)
            .lines()
            .filter(|row| !row.is_empty())
            .enumerate()
            .filter(|(_, row)| row.contains('\u{2588}'))
            .map(|(index, _)| index)
            .collect()
    };

    let top = support::render(&mut app, 40, 12);
    let at_top = thumb_rows(&top);
    assert_eq!(
        at_top.first(),
        Some(&0),
        "the thumb is not at the top of the track:\n{top}"
    );

    // The editor is eleven rows tall, so the last of them is row ten.
    for _ in 0..10 {
        press(&mut app, KeyCode::PageDown);
    }
    let bottom = support::render(&mut app, 40, 12);
    let at_bottom = thumb_rows(&bottom);
    assert!(
        support::text_block(&bottom).contains("LINE_38"),
        "not actually at the end of the file:\n{bottom}"
    );
    assert_eq!(
        at_bottom.last(),
        Some(&10),
        "the thumb stopped short of the bottom:\n{bottom}"
    );
}

/// The wheel moves the view, not the cursor. Which is only knowable because
/// obelus asks the terminal to report the mouse: without that the wheel
/// arrives as arrow keys and there is no way to tell it not to.
#[test]
fn the_wheel_scrolls_without_moving_the_cursor() {
    let mut app = App::new(vec![support::open_fixture("many_lines.rs")]);
    support::lay_out(&mut app, 40, 12);

    let start = support::render(&mut app, 40, 12);
    let status = |dump: &str| {
        support::text_block(dump)
            .lines()
            .last()
            .expect("a status row")
            .to_string()
    };
    let before = status(&start);

    app.handle(obelus::event::Event::Scroll(3));
    let scrolled = support::render(&mut app, 40, 12);
    assert_eq!(
        status(&scrolled),
        before,
        "the wheel moved the cursor:\n{scrolled}"
    );
    assert_ne!(
        support::text_block(&scrolled),
        support::text_block(&start),
        "the wheel moved nothing"
    );

    // Back up, and the screen is where it started: three rows down and three
    // rows up is nowhere.
    app.handle(obelus::event::Event::Scroll(-3));
    assert_eq!(
        support::text_block(&support::render(&mut app, 40, 12)),
        support::text_block(&start),
        "rolling back did not come back"
    );

    // And a cursor move afterwards brings the screen back to the cursor,
    // like any other detour.
    app.handle(obelus::event::Event::Scroll(9));
    press(&mut app, KeyCode::Down);
    let back = support::render(&mut app, 40, 12);
    assert!(
        support::cursor_line(&back) != "none",
        "the cursor is still off screen after moving it:\n{back}"
    );
}

/// A list under a wheel scrolls, one row a notch. With the mouse reported the
/// wheel no longer arrives as arrow keys, so a picker that ignored it would
/// have lost something it used to do.
#[test]
fn the_wheel_moves_a_list_by_one_row() {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 60, 20);
    support::press_control(&mut app, 'p');

    let first = app
        .picker()
        .and_then(|picker| picker.selected_item())
        .map(|item| item.label.clone())
        .expect("a row");
    app.handle(obelus::event::Event::Scroll(3));
    let second = app
        .picker()
        .and_then(|picker| picker.selected_item())
        .map(|item| item.label.clone())
        .expect("a row");
    assert_ne!(first, second, "the wheel did not move the list");

    // And it stops at the top rather than wrapping round: a wheel is rolled
    // without looking.
    for _ in 0..10 {
        app.handle(obelus::event::Event::Scroll(-3));
    }
    assert_eq!(
        app.picker()
            .and_then(|picker| picker.selected_item())
            .map(|item| item.label.clone()),
        Some(first),
        "the wheel wrapped past the top of the list"
    );
}

/// A markdown file opens as text, like every other file, and the command
/// turns it into a rendering. The mode is a *reading* of the same bytes, so
/// it can be turned off again, and the status bar names it while it is on.
#[test]
fn markdown_is_a_mode_over_the_same_file() {
    use obelus::{buffer::Mode, command::Command};

    let mut app = App::new(vec![support::open_fixture("sample.md")]);
    support::lay_out(&mut app, 60, 14);

    // Text first: opening a README should show what is in it.
    let source = support::render(&mut app, 60, 14);
    assert!(
        support::text_block(&source).contains("# Title"),
        "a markdown file did not open as its own text:\n{source}"
    );
    assert_eq!(
        app.current_buffer().expect("a buffer").mode(),
        Mode::Edit,
        "a file opened in some mode other than its bytes"
    );

    obelus::command::dispatch::dispatch(&mut app, Command::MarkdownPreview);
    let rendered = support::render(&mut app, 60, 14);
    let text = support::text_block(&rendered);
    assert!(text.contains("Title"), "{rendered}");
    assert!(
        !text.contains("# Title"),
        "the hashes are still there, so nothing was rendered:\n{rendered}"
    );
    assert!(
        text.contains('\u{2022}'),
        "the list has no bullets:\n{rendered}"
    );
    // The mode is named, because a screen showing something other than the
    // file has to say so.
    assert!(
        text.contains("markdown"),
        "the status bar does not name the mode:\n{rendered}"
    );
    // And no cursor: the rows are not the file's lines, so there is nowhere
    // in them the cursor honestly is.
    assert_eq!(support::cursor_line(&rendered), "none", "{rendered}");

    // The same command again puts the file back.
    obelus::command::dispatch::dispatch(&mut app, Command::MarkdownPreview);
    assert_eq!(
        support::text_block(&support::render(&mut app, 60, 14)),
        support::text_block(&source),
        "the file did not come back"
    );
}

/// A file that is not markdown gets a note, not a screen of nonsense. By
/// extension, case-insensitively, because the mode is a reading of the bytes
/// and a reading that does not fit them produces gibberish.
#[test]
fn only_a_markdown_file_can_be_rendered() {
    use obelus::command::Command;

    let mut app = app();
    support::lay_out(&mut app, 60, 14);
    obelus::command::dispatch::dispatch(&mut app, Command::MarkdownPreview);

    assert_eq!(app.note(), Some("not a markdown file"));

    // And the extension is read without regard to case: `README.MD` is one.
    let shouting = std::env::temp_dir().join(format!("obelus-{}-README.MD", std::process::id()));
    std::fs::write(&shouting, "# Title\n").expect("a file to render");
    let mut upper = App::new(vec![
        obelus::buffer::Buffer::open(&shouting).expect("opening it"),
    ]);
    support::lay_out(&mut upper, 60, 14);
    obelus::command::dispatch::dispatch(&mut upper, Command::MarkdownPreview);
    assert_eq!(
        upper.current_buffer().expect("a buffer").mode(),
        obelus::buffer::Mode::Markdown,
        "an upper-case extension was not recognized"
    );
    let _ = std::fs::remove_file(&shouting);
    let dump = support::render(&mut app, 60, 14);
    assert!(
        support::text_block(&dump).contains("fn main"),
        "the file was replaced anyway:\n{dump}"
    );
}

/// The rendering scrolls by rows, with the keys that scroll everything else.
#[test]
fn a_rendering_scrolls_by_rows() {
    use obelus::command::Command;

    let mut app = App::new(vec![support::open_fixture("sample.md")]);
    support::lay_out(&mut app, 60, 8);
    obelus::command::dispatch::dispatch(&mut app, Command::MarkdownPreview);

    let first = support::render(&mut app, 60, 8);
    assert!(support::text_block(&first).contains("Title"), "{first}");

    press(&mut app, KeyCode::PageDown);
    let paged = support::render(&mut app, 60, 8);
    assert!(
        !support::text_block(&paged).contains("Title"),
        "paging a rendering moved nothing:\n{paged}"
    );

    // And the right-hand corner counts rows rather than a cursor that is not
    // there.
    assert!(
        support::text_block(&paged)
            .lines()
            .last()
            .is_some_and(|status| status.contains('/')),
        "the status bar still reports a cursor:\n{paged}"
    );

    press(&mut app, KeyCode::PageUp);
    assert_eq!(
        support::text_block(&support::render(&mut app, 60, 8)),
        support::text_block(&first),
        "paging back did not come back"
    );
}

/// The bracket under the cursor is highlighted along with its partner, and a
/// key goes to it. The highlight is what says which two characters are the
/// pair; the key is what saves reading down the screen to find out.
#[test]
fn a_bracket_pair_is_marked_and_can_be_jumped_between() {
    use crossterm::event::{KeyEvent, KeyModifiers};

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 40, 8);

    // `sample.rs` line one is `fn main() {`. Onto the opening bracket.
    for _ in 0..7 {
        press(&mut app, KeyCode::Right);
    }
    let dump = support::render(&mut app, 40, 8);
    let rows: Vec<&str> = support::text_block(&dump)
        .lines()
        .filter(|row| !row.is_empty())
        .collect();
    let styles: Vec<&str> = support::style_block(&dump)
        .lines()
        .filter(|row| !row.is_empty())
        .collect();

    // Both brackets share a *background* nothing else on the row has. The
    // background is the assertion: a foreground they happen to share is
    // just the punctuation colour, which they would have anyway.
    let background = |letter: char| {
        support::legend_block(&dump)
            .lines()
            .find(|line| line.trim_start().starts_with(letter))
            .and_then(|line| line.split("bg=").nth(1))
            .map(str::to_string)
            .unwrap_or_else(|| panic!("no legend for {letter:?}:\n{dump}"))
    };
    let letters: Vec<char> = rows[0]
        .chars()
        .zip(styles[0].chars())
        .filter(|(character, _)| *character == '(' || *character == ')')
        .map(|(_, letter)| letter)
        .collect();
    assert_eq!(letters.len(), 2, "{dump}");
    assert_eq!(letters[0], letters[1], "the pair is not one style:\n{dump}");
    let plain = rows[0]
        .chars()
        .zip(styles[0].chars())
        .find(|(character, _)| *character == 'm')
        .map(|(_, letter)| letter)
        .expect("a letter of the name");
    assert_ne!(
        background(letters[0]),
        background(plain),
        "the pair has no background of its own:\n{dump}"
    );

    // And the key goes to the other one.
    app.handle(obelus::event::Event::Key(KeyEvent::new(
        KeyCode::Char('m'),
        KeyModifiers::ALT,
    )));
    let cursor = app.current_buffer().expect("a buffer").cursor();
    assert_eq!(cursor.line.get(), 0);
    assert_eq!(cursor.column.get(), 8, "not on the closing bracket");

    // Back again, because the pair is symmetric.
    app.handle(obelus::event::Event::Key(KeyEvent::new(
        KeyCode::Char('m'),
        KeyModifiers::ALT,
    )));
    assert_eq!(
        app.current_buffer()
            .expect("a buffer")
            .cursor()
            .column
            .get(),
        7
    );
}

/// A cursor that is not on a bracket gets a note, not a silent nothing.
#[test]
fn nowhere_to_jump_says_so() {
    use crossterm::event::{KeyEvent, KeyModifiers};

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 40, 8);
    app.handle(obelus::event::Event::Key(KeyEvent::new(
        KeyCode::Char('m'),
        KeyModifiers::ALT,
    )));

    assert_eq!(app.note(), Some("no bracket here"));
    assert_eq!(
        app.current_buffer()
            .expect("a buffer")
            .cursor()
            .column
            .get(),
        0,
        "the cursor moved anyway"
    );
}

/// The ends of the file are the one place a selection could not reach: the
/// rule that a modifier obelus has no meaning for disqualifies the key made
/// `ctrl+shift+End` do nothing at all rather than extend to the end.
#[test]
fn shift_and_control_together_select_to_the_ends_of_the_file() {
    use crossterm::event::{KeyEvent, KeyModifiers};

    let mut app = App::new(vec![support::open_fixture("many_lines.rs")]);
    support::lay_out(&mut app, 40, 12);
    let both = KeyModifiers::CONTROL | KeyModifiers::SHIFT;

    app.handle(obelus::event::Event::Key(KeyEvent::new(KeyCode::End, both)));
    let buffer = app.current_buffer().expect("a buffer");
    let selection = buffer.selection().expect("a selection to the end");
    assert_eq!(selection.line, LineNumber::new(0));
    assert_eq!(selection.end_line, buffer.text().last_line());

    // And back to the start, which passes through the anchor and comes out
    // the other side rather than needing a special case.
    app.handle(obelus::event::Event::Key(KeyEvent::new(
        KeyCode::Home,
        both,
    )));
    let buffer = app.current_buffer().expect("a buffer");
    assert_eq!(buffer.cursor().line, LineNumber::new(0));
    assert!(
        buffer.selection().is_none(),
        "back at the anchor is nothing selected"
    );
}

/// Escape at the file itself gives up on the selection. It reaches the key
/// table only when no picker and no prompt is open, each of which takes it
/// first, so there is nothing else there for it to mean.
#[test]
fn escape_stops_selecting() {
    let mut app = App::new(vec![support::open_fixture("many_lines.rs")]);
    support::lay_out(&mut app, 40, 12);

    support::press_shift(&mut app, KeyCode::Right);
    support::press_shift(&mut app, KeyCode::Right);
    assert!(
        app.current_buffer()
            .and_then(obelus::buffer::Buffer::selection)
            .is_some()
    );

    press(&mut app, KeyCode::Esc);
    let buffer = app.current_buffer().expect("a buffer");
    assert!(buffer.selection().is_none(), "escape left it selected");
    // The cursor stays: giving up on the selection is not going back.
    assert_eq!(buffer.cursor().column, CharColumn::new(2));

    // And escape still belongs to a picker while one is open.
    support::press_control(&mut app, 'p');
    press(&mut app, KeyCode::Esc);
    assert!(app.picker().is_none(), "escape did not close the palette");
}
