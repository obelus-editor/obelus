//! What the editor region and the status bar actually put on screen.

mod support;

use crossterm::event::KeyCode;
use obelus::{
    app::App,
    coordinates::{CharColumn, LineNumber},
};
use support::press;

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

    assert!(support::text_block(&dump).contains("ctrl+f"), "{dump}");
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
    assert!(support::text_block(&shown).contains("ctrl+f"), "{shown}");

    app.set_keymap(Keymap::from_bindings(vec![Binding {
        command: Command::FileOpen,
        context: Context::Normal,
        chord: KeyChord::new(KeyCode::Char('o'), KeyModifiers::ALT),
    }]));
    let rebound = support::render(&mut app, 64, 20);
    let text = support::text_block(&rebound);

    assert!(
        text.contains("alt+o"),
        "the rebound key is not shown:\n{rebound}"
    );
    assert!(
        !text.contains("ctrl+f"),
        "the old key is still being offered:\n{rebound}"
    );
    assert!(
        !text.contains("ctrl+q"),
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

    // Column one is the padding, then `> `, so the caret is at three; the
    // status row is the last one.
    assert_eq!(support::cursor_line(&dump), "3,11", "{dump}");

    support::type_text(&mut app, "theme");
    let dump = support::render(&mut app, 60, 12);
    assert_eq!(
        support::cursor_line(&dump),
        "8,11",
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

    let numbered = text
        .lines()
        .filter(|row| row.len() > 8 && row[3..8].trim().parse::<u32>().is_ok())
        .count();
    // Three lines of text plus the empty one a trailing newline leaves, and
    // six rows of the long line between them carrying no number at all.
    assert_eq!(numbered, 4, "one number per line, not per row:\n{dump}");
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
    let column_of = |needle: &str| {
        text.lines()
            .find_map(|row| row.find(needle))
            .unwrap_or_else(|| panic!("{needle:?} is not on screen:\n{dump}"))
    };

    assert_eq!(
        column_of("open a file"),
        column_of("leave obelus"),
        "the descriptions do not start together:\n{dump}"
    );
    assert!(column_of("esc") > column_of("ctrl+alt+o"), "{dump}");
}
