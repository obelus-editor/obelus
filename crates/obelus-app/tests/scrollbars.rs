//! A bar taken hold of with the pointer.
//!
//! Both front ends hand Obelus the same press, drag and release, so this is
//! the whole of it for both: what a press on a bar lands on is the bar the
//! frame left there, and what the drag moves is the view beside it -- never
//! the cursor, and never what a list has chosen.

mod support;

use crossterm::event::KeyCode;
use obelus_app::{
    app::{App, dispatch},
    event::{Event, Pointer},
};
use obelus_buffer::Buffer;
use obelus_command::Command;

const WIDTH: u16 = 60;
const HEIGHT: u16 = 16;

/// An application over a file of `lines` lines, laid out.
fn reading(name: &str, lines: usize) -> (support::Scratch, App) {
    let text: String = (0..lines).map(|line| format!("line {line}\n")).collect();
    over(name, &text, "")
}

/// An application over a file of the test's own, with settings of its own.
fn over(name: &str, text: &str, settings: &str) -> (support::Scratch, App) {
    let scratch = support::Scratch::new(name);
    std::fs::create_dir_all(scratch.join(".obelus")).expect("the directory");
    std::fs::write(scratch.join(".obelus/config.toml"), settings).expect("the settings");
    let path = scratch.path().join("sample.txt");
    std::fs::write(&path, text).expect("writing the file");
    let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, WIDTH, HEIGHT);
    (scratch, app)
}

fn pointer(app: &mut App, kind: Pointer, x: u16, y: u16) {
    app.handle(Event::Pointer { kind, x, y });
    // The frame between, which is where the next move is measured from.
    support::lay_out(app, WIDTH, HEIGHT);
}

/// The rows of the screen as the dump shows them.
fn screen(app: &mut App) -> Vec<Vec<char>> {
    let dump = support::render(app, WIDTH, HEIGHT);
    support::text_block(&dump)
        .lines()
        .filter_map(|row| {
            row.split_once('|')
                .map(|(_, cells)| cells.chars().collect())
        })
        .collect()
}

/// Which rows of a column are a bar's, top to bottom.
fn bar_rows(app: &mut App, x: u16) -> Vec<u16> {
    screen(app)
        .iter()
        .enumerate()
        .filter(|(_, row)| row.get(usize::from(x)) == Some(&'\u{2588}'))
        .map(|(y, _)| u16::try_from(y).expect("a row"))
        .collect()
}

fn top_line(app: &App) -> usize {
    app.current_buffer().expect("a buffer").viewport().top.get()
}

fn lines(app: &App) -> usize {
    app.current_buffer().expect("a buffer").text().line_count()
}

fn caret(app: &App) -> usize {
    app.current_buffer().expect("a buffer").cursor().line.get()
}

/// Dragging the file's bar to the foot shows the end of the file, and the
/// caret stays where the reader put it.
///
/// Deliberate break: return `false` from `pointer_on_a_bar` for a drag. The
/// press still moves the file -- to where the mark was taken hold of -- and
/// the drag goes on to select text instead, so the end is never reached.
#[test]
fn dragging_the_files_bar_to_the_foot_shows_its_end() {
    let (_scratch, mut app) = reading("bar-drag-file", 200);
    let x = WIDTH - 1;
    let rows = bar_rows(&mut app, x);
    let (first, last) = (rows[0], *rows.last().expect("a bar"));

    pointer(&mut app, Pointer::Pressed, x, first);
    assert_eq!(top_line(&app), 0, "a press on the mark moved it");
    pointer(&mut app, Pointer::Dragged, x, last);
    let end = top_line(&app);
    // The last screenful: the file's lines -- the empty one after the
    // last newline among them -- less the fourteen rows of the editor.
    assert_eq!(end, lines(&app) - 14, "the drag did not reach the end");
    assert_eq!(caret(&app), 0, "the caret went with the view");

    // Wandering off the column is still holding it.
    pointer(&mut app, Pointer::Dragged, 3, first);
    assert_eq!(top_line(&app), 0, "the bar let go when the pointer left it");

    // And once let go, a drag is a drag over the text again.
    pointer(&mut app, Pointer::Released, 3, first);
    pointer(&mut app, Pointer::Dragged, x, last);
    assert_eq!(top_line(&app), 0, "the bar was still held after release");
}

/// A press on the track, off the mark, brings the mark to it.
///
/// Deliberate break: grip the mark at `row - mark` wherever the press is.
/// The mark then lands a whole mark's height short of the pointer, and the
/// view barely moves.
#[test]
fn a_press_on_the_track_brings_the_mark_to_it() {
    let (_scratch, mut app) = reading("bar-press-track", 200);
    let x = WIDTH - 1;
    let rows = bar_rows(&mut app, x);
    let middle = rows[rows.len() / 2];

    pointer(&mut app, Pointer::Pressed, x, middle);
    let top = top_line(&app);
    assert!(
        (80..=120).contains(&top),
        "a press half way down put line {top} on top"
    );
    assert_eq!(caret(&app), 0, "the caret went with the view");
}

/// A list's bar moves the window and leaves the selection where it was, and
/// a key brings the window back to it.
///
/// Deliberate break: leave `drag_to` off the window, so it is `settle`
/// that decides where the window is. The next frame puts it straight back
/// on the selection and the drag has moved nothing.
#[test]
fn dragging_a_lists_bar_leaves_the_selection_where_it_was() {
    let (_scratch, mut app) = reading("bar-drag-list", 3);
    support::press_control(&mut app, 'p');
    support::lay_out(&mut app, WIDTH, HEIGHT);
    let x = WIDTH - 1;
    let rows = bar_rows(&mut app, x);
    assert!(!rows.is_empty(), "the palette has no bar to drag");
    let picker = |app: &App| {
        let window = app.picker().expect("the palette").window();
        (window.top(), window.focus())
    };
    assert_eq!(picker(&app), (0, 0));

    pointer(&mut app, Pointer::Pressed, x, rows[0]);
    pointer(&mut app, Pointer::Dragged, x, *rows.last().expect("a bar"));
    let (top, focus) = picker(&app);
    assert!(top > 0, "the window did not move");
    assert_eq!(focus, 0, "dragging the bar chose something");
    assert!(app.picker().is_some(), "the press closed the list");

    pointer(&mut app, Pointer::Released, x, rows[0]);
    support::press(&mut app, KeyCode::Down);
    support::lay_out(&mut app, WIDTH, HEIGHT);
    // By the least that puts it back, which from below is the selection
    // on the top row.
    assert_eq!(
        picker(&app),
        (1, 1),
        "a key did not bring the window back to the selection"
    );
}

/// The file's bar under a page drawn over it is not the file's to take
/// hold of.
///
/// Deliberate break: keep every bar `collect` was told about, drawn over
/// or not. A press where the file's bar was, under the counts, then
/// scrolls the file the reader cannot see.
#[test]
fn a_bar_drawn_over_cannot_be_taken_hold_of() {
    let (_scratch, mut app) = reading("bar-drawn-over", 200);
    let x = WIDTH - 1;
    let before = bar_rows(&mut app, x);
    // A page over the whole screen, with nothing of its own to scroll.
    dispatch::dispatch(&mut app, Command::CountLines);
    support::lay_out(&mut app, WIDTH, HEIGHT);
    assert!(
        bar_rows(&mut app, x).is_empty(),
        "the counts drew a bar of their own"
    );

    pointer(&mut app, Pointer::Pressed, x, before[before.len() / 2]);
    assert_eq!(top_line(&app), 0, "the file scrolled under the counts");
}

/// With lines wrapped, the foot of the bar is still the end of the file.
///
/// Deliberate break: ask for `bar_top` at the foot too, rather than for
/// the end. The bar counts lines and the screen rows, so the last lines of
/// a file whose lines wrap are never reached.
#[test]
fn the_foot_of_the_bar_is_the_end_of_a_wrapped_file() {
    let text: String = (0..100)
        .map(|line| format!("L{line} {} E{line}\n", "x".repeat(130)))
        .collect();
    let (_scratch, mut app) = over("bar-wrapped", &text, "wrap = true\n");
    let x = WIDTH - 1;
    let rows = bar_rows(&mut app, x);
    pointer(&mut app, Pointer::Pressed, x, rows[0]);
    pointer(&mut app, Pointer::Dragged, x, *rows.last().expect("a bar"));
    let shown: String = screen(&mut app)
        .iter()
        .map(|row| row.iter().collect::<String>())
        .collect();
    assert!(shown.contains("E99"), "the end is not on screen:\n{shown}");
}

/// A press on the mark, where the wheel left it, takes hold and moves
/// nothing.
///
/// Deliberate break: drop the `None` for the row the mark is on in
/// `top_for`. The mark is drawn rounded up and comes back rounded down,
/// and the file jumps by a few lines under a pointer that did not move.
#[test]
fn a_press_on_the_mark_moves_nothing() {
    let (_scratch, mut app) = reading("bar-press-mark", 200);
    app.handle(Event::Scroll(15));
    support::lay_out(&mut app, WIDTH, HEIGHT);
    let before = top_line(&app);
    let x = WIDTH - 1;
    // The mark is the brighter run; on a bar this long it is one row, the
    // one whose row the wheel's top rounds up to.
    let mark = obelus_ui::bar_mark(
        u16::try_from(bar_rows(&mut app, x).len()).expect("a height"),
        before,
        lines(&app),
    );
    assert_ne!(
        obelus_ui::bar_top(14, mark, lines(&app)),
        before,
        "a top the bar draws exactly, which would not tell"
    );
    pointer(&mut app, Pointer::Pressed, x, mark);
    assert_eq!(top_line(&app), before, "a press on the mark moved the file");
}

/// A press that lands somewhere else lets go of a bar whose release went
/// missing.
///
/// Deliberate break: leave `holding` alone on a press that finds no bar.
/// The drag that follows, over the text, then moves the bar from before.
#[test]
fn a_press_elsewhere_lets_go_of_the_bar() {
    let (_scratch, mut app) = reading("bar-lost-release", 200);
    let x = WIDTH - 1;
    let rows = bar_rows(&mut app, x);
    pointer(&mut app, Pointer::Pressed, x, rows[0]);
    // No release: the button came up outside the window.
    pointer(&mut app, Pointer::Pressed, 8, 2);
    pointer(&mut app, Pointer::Dragged, 12, *rows.last().expect("a bar"));
    assert_eq!(top_line(&app), 0, "the drag over the text moved the bar");
}

/// Typing into a list whose bar was dragged brings the window back to the
/// selection, which is the row a press of enter would take.
///
/// Deliberate break: take `back_to_the_focus` out of `refilter_typed`.
/// The window stays where it was dragged and the selection is off the
/// screen while the reader types.
#[test]
fn typing_brings_a_dragged_list_back_to_the_selection() {
    let (_scratch, mut app) = reading("bar-then-type", 3);
    support::press_control(&mut app, 'p');
    support::lay_out(&mut app, WIDTH, HEIGHT);
    let x = WIDTH - 1;
    let rows = bar_rows(&mut app, x);
    pointer(&mut app, Pointer::Pressed, x, rows[0]);
    pointer(&mut app, Pointer::Dragged, x, *rows.last().expect("a bar"));
    pointer(&mut app, Pointer::Released, x, rows[0]);
    let window = |app: &App| *app.picker().expect("the palette").window();
    assert!(window(&app).top() > 0, "the drag did not move the list");

    support::press(&mut app, KeyCode::Char('e'));
    support::lay_out(&mut app, WIDTH, HEIGHT);
    let window = window(&app);
    assert!(
        window.top() <= window.focus(),
        "the selection is above the window after typing: {window:?}"
    );
}

/// The page that asks which project keeps a drag across frames, though it
/// says where its selection is once a frame.
///
/// Deliberate break: call `set_focus` every frame in `Chooser::settle`, as
/// it did. Every frame then chooses the row again and puts the window
/// back on it, so the drag lasts until the next frame.
#[test]
fn the_projects_keep_where_their_bar_was_dragged() {
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(std::path::PathBuf::from("/tmp/obelus"));
    app.ask_about_these_projects_for_test(
        (0..40)
            .map(|at| obelus_component::chooser::Known {
                path: std::path::PathBuf::from(format!("/tmp/obelus/p{at:02}")),
                shown: format!("/tmp/obelus/p{at:02}"),
                last: Some(10_000 - at),
            })
            .collect(),
    );
    support::lay_out(&mut app, WIDTH, HEIGHT);
    let shown = |app: &mut App| -> String {
        screen(app)
            .iter()
            .map(|row| row.iter().collect::<String>())
            .collect()
    };
    assert!(shown(&mut app).contains("p00"), "the newest is not shown");
    let x = WIDTH - 1;
    let rows = bar_rows(&mut app, x);
    pointer(&mut app, Pointer::Pressed, x, rows[0]);
    pointer(&mut app, Pointer::Dragged, x, *rows.last().expect("a bar"));
    support::lay_out(&mut app, WIDTH, HEIGHT);
    let after = shown(&mut app);
    assert!(
        !after.contains("p00") && after.contains("p39"),
        "the drag did not last:\n{after}"
    );
}

/// With a list open over the file, the file's bar above it is not the
/// file's to take hold of: the list owns the pointer as it owns the keys.
///
/// Deliberate break: answer `true` for every bar in `App::reaches`. The
/// press on the file's bar above the palette then scrolls the file.
#[test]
fn a_list_open_over_the_file_keeps_its_bar() {
    let (_scratch, mut app) = reading("bar-under-a-list", 200);
    support::press_control(&mut app, 'p');
    // Tall enough that the palette leaves a bar of the file's above it.
    let height = 30;
    let dump = support::render(&mut app, WIDTH, height);
    let x = WIDTH - 1;
    let above: Vec<u16> = support::text_block(&dump)
        .lines()
        .filter_map(|row| row.split_once('|').map(|(_, cells)| cells.to_string()))
        .take_while(|row| !row.starts_with('\u{2500}'))
        .enumerate()
        .filter(|(_, row)| row.chars().nth(usize::from(x)) == Some('\u{2588}'))
        .map(|(y, _)| u16::try_from(y).expect("a row"))
        .collect();
    assert!(
        above.len() > 2,
        "no bar of the file's above the list:\n{dump}"
    );

    app.handle(Event::Pointer {
        kind: Pointer::Pressed,
        x,
        y: *above.last().expect("a row"),
    });
    assert_eq!(top_line(&app), 0, "the file scrolled under the list");
    assert!(app.picker().is_some(), "the press closed the list");
}

/// A list left by choosing a row puts the file back where it was before the
/// list shortened it, the same as escape does: the scroll was the list's,
/// and a row that changes a setting is not the reader moving the file.
///
/// The themes, because choosing the one already worn goes nowhere and
/// changes nothing on the page -- what is left to see is the scroll.
///
/// Broken deliberately by forgetting where the reader was looking for every
/// row, as `accept` did: the file stays the rows up the list put it.
#[test]
fn a_row_chosen_puts_the_file_back_where_it_was() {
    let (_scratch, mut app) = reading("put-back", 100);
    // Near the foot of the screen, where a list on the status bar covers
    // the caret's line and the file has to scroll to keep it.
    for _ in 0..12 {
        support::press(&mut app, KeyCode::Down);
    }
    support::lay_out(&mut app, WIDTH, HEIGHT);
    let before = top_line(&app);

    dispatch::dispatch(&mut app, Command::ThemeSelect);
    support::lay_out(&mut app, WIDTH, HEIGHT);
    assert!(
        top_line(&app) > before,
        "the list did not scroll the file, so this proves nothing"
    );

    support::press(&mut app, KeyCode::Enter);
    support::lay_out(&mut app, WIDTH, HEIGHT);
    assert!(app.picker().is_none(), "the list is still open");
    assert_eq!(
        top_line(&app),
        before,
        "the file stayed where the list had scrolled it"
    );
}
