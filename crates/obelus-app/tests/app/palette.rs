//! The palette over a view: the view's own keys are its first tab, and the
//! view is still there when it goes.

use crossterm::event::KeyCode;
use obelus_app::app::App;
use obelus_component::{layers::Layer, picker::PickerValue};

use crate::support;

const WIDTH: u16 = 76;
const HEIGHT: u16 = 24;

/// A reader in the search of the file they have open, with something typed.
fn searching() -> App {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, WIDTH, HEIGHT);
    support::press_function(&mut app, 5);
    support::type_text(&mut app, "fn");
    app
}

/// The palette opens over a list rather than instead of it, and escape is
/// back in the list with what was typed there.
///
/// Broken deliberately by opening it with `show_list` whatever was showing,
/// which is what it did before: the search went, and escape went to the
/// file.
#[test]
fn the_palette_opens_over_a_list_and_escape_is_back_in_it() {
    let mut app = searching();
    support::press_control(&mut app, 'p');
    assert_eq!(app.layers().nearest(), Some(Layer::Palette));
    assert_eq!(
        app.picker().map(|search| search.query()),
        Some("fn".into()),
        "the search was not kept under the palette"
    );

    support::press(&mut app, KeyCode::Esc);
    assert!(
        app.palette().is_none(),
        "escape did not give the palette up"
    );
    assert_eq!(app.layers().nearest(), Some(Layer::Picker));
    assert_eq!(
        app.picker().map(|search| search.query()),
        Some("fn".into()),
        "escape did not come back to the search as it was"
    );
}

/// The first tab is the view's own keys and "All" is every command and
/// none of them: a key of the search means nothing once the search has gone.
///
/// Broken deliberately by letting the "All" tab show every row, as it
/// does in a list with no tab of its own: the search's switches were among
/// the commands. By drawing the symbols tab's switch on this one, dim
/// (`only_where` left off "Outside"): it was a row of a tab it means
/// nothing on. And by leaving the sentence off a row (`detail: None` in
/// `keys_as_rows`).
#[test]
fn the_first_tab_is_the_views_keys_and_all_is_the_commands() {
    let mut app = searching();
    support::press_control(&mut app, 'p');
    let palette = app.palette().expect("the palette");
    assert_eq!(&palette.tabs()[..2], ["Here", "All"]);
    assert_eq!(palette.tab(), 0, "it did not open on the view's own keys");
    let here: Vec<String> = palette.matches().map(|row| row.label.clone()).collect();
    // Not "Outside": a search of one file has no outside to reach, and that
    // switch is the symbols tab's.
    assert_eq!(here, ["Regex", "Word", "Case"], "{here:?}");
    // Each with a sentence after its word, where a command has its title.
    assert!(
        palette.matches().all(|row| row.detail.is_some()),
        "a key of the view does not say what it does"
    );
    assert!(
        palette
            .matches()
            .all(|row| matches!(row.value, PickerValue::Key(_))),
        "a row of the first tab is not a key"
    );

    support::press(&mut app, KeyCode::Tab);
    let palette = app.palette().expect("the palette");
    assert_eq!(palette.tab(), 1);
    assert!(palette.match_count() > 0, "all is empty");
    assert!(
        palette
            .matches()
            .all(|row| matches!(row.value, PickerValue::Command(_))),
        "a key of the view is among the commands"
    );
}

/// A key chosen from the first tab is pressed in the view, which is still
/// there to take it.
///
/// Broken deliberately by doing nothing with a chosen key: the palette
/// went and the search was as it was.
#[test]
fn a_key_chosen_from_the_palette_is_pressed_in_the_view() {
    let mut app = searching();
    support::press_control(&mut app, 'p');
    support::type_text(&mut app, "Regex");
    support::press(&mut app, KeyCode::Enter);

    assert!(app.palette().is_none(), "the palette stayed up");
    let search = app.picker().expect("the search");
    assert!(
        search.looks_how().is_some_and(|how| how.regex),
        "the pattern switch is not on"
    );
    assert_eq!(search.query(), "fn", "what was typed in the search went");
}

/// A switch that is on is said on the row the query is typed into, behind
/// a ticked box, and nothing is said there while they are all off.
///
/// Broken deliberately by not drawing them (`status::switches`): the row
/// said nothing with the pattern switch on. And by writing the word with
/// no box in front of it: the word read as a word.
#[test]
fn a_switch_that_is_on_is_said_where_the_query_is() {
    let mut app = searching();
    let row = |app: &mut App| {
        let dump = support::render(app, WIDTH, HEIGHT);
        support::text_block(&dump)
            .lines()
            .last()
            .expect("the status row")
            .to_string()
    };
    assert!(!row(&mut app).contains("Regex"), "{}", row(&mut app));

    support::press_alt(&mut app, 'r');
    assert!(row(&mut app).contains("Regex"), "{}", row(&mut app));
    support::press_alt(&mut app, 'c');
    // Each behind a box, and the box ticked: the glyph in front of the
    // word is the one the palette draws for a switch that is on.
    let said = row(&mut app);
    assert!(said.contains("Regex") && said.contains("Case"), "{said}");
    for word in ["Regex", "Case"] {
        assert_eq!(
            support::glyph_before(&said, word),
            obelus_ui::tick(true),
            "{word} has no ticked box in front of it: {said}"
        );
    }
}

/// A key that names another view, from the palette over a view, is asked
/// about that view: the palette goes and the search walks to its tab.
///
/// Broken deliberately by not giving the palette up first in
/// `switch_view`: the search walked to its tab under a palette still open.
#[test]
fn a_key_naming_a_view_goes_past_the_palette() {
    let mut app = searching();
    support::press_control(&mut app, 'p');
    support::press_function(&mut app, 6);
    assert!(app.palette().is_none(), "the palette stayed up");
    assert_eq!(app.picker().map(|search| search.tab()), Some(1));
}
