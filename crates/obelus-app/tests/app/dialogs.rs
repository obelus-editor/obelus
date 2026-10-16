//! What a full-screen dialog draws, cell for cell.
//!
//! These pages had no golden fixture at all, and two things went wrong in
//! them that nothing in the suite could see: a grey band across the foot of
//! the page, which was the window's glass reading a row nobody had written,
//! and the rule between a page's foot and its own row painted out. Both were
//! found by looking at a screenshot, which is not a thing that happens on
//! its own.
//!
//! A cell grid catches the second and the colours catch neither -- a hole is
//! a hole in the *window*, and the grid a terminal is handed says nothing
//! about it. What this is for is everything else: that the row is there,
//! that the rule above it is, and that what is written on it belongs to the
//! page rather than to the file behind it.

use crossterm::event::KeyCode;
use obelus_app::app::App;
use support::{press, press_control, type_text};

use crate::support;

fn app() -> App {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.statuses_for_test(std::collections::HashMap::new());
    app
}

/// Opens one of Obelus's own pages through the palette, by name.
fn page(app: &mut App, command: &str) {
    let _ = support::render(app, 60, 16);
    press_control(app, 'p');
    type_text(app, command);
    press(app, KeyCode::Enter);
}

/// The settings page ends in a row of its own.
///
/// The row under it says `Filter settings`, which is the page's, and the
/// rule above that row is the one every boundary in Obelus gets. What it
/// must never say is what the file behind it is called: a dialog does not
/// borrow the status row, because a row belonging to what is behind it is a
/// prompt with somebody else's words in it.
///
/// Broken deliberately by putting `Self::Settings => Room::Region` back in
/// `Layer::room`: Obelus draws the file's own status row under the page
/// again, and the path to `sample.rs` turns up on the last row.
#[test]
fn the_settings_page_ends_in_a_row_of_its_own() {
    let mut app = app();
    page(&mut app, "open-settings");
    let dump = support::render(&mut app, 60, 16);
    let text = support::text_block(&dump);
    let rows: Vec<&str> = text.lines().collect();

    assert!(
        rows.last()
            .is_some_and(|row| row.contains("Filter settings")),
        "the page does not end in its own row:\n{dump}"
    );
    assert!(
        !text.contains("sample.rs"),
        "the file behind the page is still named on it:\n{dump}"
    );
    support::check("settings_60x16", &dump);
}

/// And so does the counts page, which has always taken the whole screen.
///
/// Here for the same reason as the one above and to hold what was already
/// true: it is the page this rule was read off, and it had no fixture
/// either.
#[test]
fn the_counts_page_ends_in_a_row_of_its_own() {
    let mut app = app();
    page(&mut app, "count-the-code");
    let dump = support::render(&mut app, 60, 16);
    assert!(
        !support::text_block(&dump).contains("sample.rs"),
        "the file behind the page is still named on it:\n{dump}"
    );
    support::check("counts_60x16", &dump);
}

/// A page of settings longer than the screen has a bar, however few
/// settings it is.
///
/// Forty rows hold every setting's name but not what each one says: there
/// are fewer entries than rows, and more rows than the screen. Whether
/// the page scrolled was asked of the window, which counts entries, so
/// this page said it fitted -- no bar, the rows drawn into its column, and
/// in a window nothing at all on the right to say there was more below.
///
/// Broken deliberately by asking `window.scrollable(region.height)` again
/// in `SettingsView::column`: the page has no bar.
#[test]
fn a_page_of_settings_longer_than_the_screen_has_a_bar() {
    let mut app = app();
    page(&mut app, "open-settings");
    let dump = support::render(&mut app, 60, 40);
    // Down the page's last column, from its first row to its last -- the
    // one above the rule over the status row.
    let bar = app
        .bars()
        .iter()
        .find(|drawn| drawn.whose == obelus_ui::bars::Whose::Settings)
        .unwrap_or_else(|| panic!("the page has no bar:\n{dump}"))
        .bar;
    assert_eq!(
        (bar.area.x, bar.area.y, bar.area.height),
        (59, 2, 36),
        "{bar:?}\n{dump}"
    );
}

/// A setting's value stays in its column when the page stops scrolling.
///
/// The column is measured as if there were always a bar, so a query that
/// narrows a long page until it fits -- and gives the bar's column back to
/// the rows -- leaves every value where the reader's eye already is.
///
/// Broken deliberately by measuring `control_column` from the room the
/// page has now, bar or no bar: the value moves a cell to the right.
#[test]
fn a_value_stays_in_its_column_when_the_page_stops_scrolling() {
    let mut app = app();
    page(&mut app, "open-settings");
    let column_of = |dump: &str| {
        let text = support::text_block(dump);
        let row = text
            .lines()
            .find(|line| line.contains(" Theme "))
            .expect("the theme's row");
        let (_, row) = row.split_once('|').expect("a numbered row");
        let at = row.find("dark").expect("the theme's value");
        row[..at].chars().count()
    };
    let long = support::render(&mut app, 100, 30);
    assert!(
        support::text_block(&long)
            .lines()
            .any(|line| line.ends_with('\u{2588}')),
        "the whole page fits, so it says nothing about a bar going:\n{long}"
    );
    type_text(&mut app, "theme");
    let short = support::render(&mut app, 100, 30);
    assert!(
        !support::text_block(&short)
            .lines()
            .any(|line| line.ends_with('\u{2588}')),
        "the narrowed page still scrolls:\n{short}"
    );
    assert_eq!(column_of(&long), column_of(&short), "{long}\n{short}");
}
