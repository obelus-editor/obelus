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
    let scratch = support::Scratch::new(name);
    let path = scratch.path().join("sample.txt");
    let text: String = (0..lines).map(|line| format!("line {line}\n")).collect();
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
    assert!(end > 180, "the drag did not reach the end: top is {end}");
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
