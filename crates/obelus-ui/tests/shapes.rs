//! What a view says about a region, and what it writes there.
//!
//! A binary of its own because who is drawing is said once for the whole
//! process (`obelus_ui::shapes::drawn_by`), which is the cost every global
//! in Obelus is paid for in: a test beside these that set a different one
//! would move it underneath them.
//!
//! The two halves are one claim in two directions, and each passes with
//! the other broken. A cap has to be *said*, or a window has nothing to
//! draw the shape from; and the cells have to be the cap a terminal draws
//! whether or not anybody heard, because that is the invariant the whole
//! channel rests on -- ignore every word of it and the screen is the one
//! Obelus drew before any of this existed.

use std::sync::{Arc, Mutex, OnceLock};

use obelus_editing::keymap::KeyChord;
use obelus_theme::builtin::DARK;
use ratatui::{buffer::Buffer as CellBuffer, layout::Rect, style::Color};

/// One cap, as it was said.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Said {
    keys: String,
    area: Rect,
    cap: Color,
    page: Color,
    edge: Color,
}

/// A front end that writes down what it is told.
#[derive(Default)]
struct Heard(Mutex<Vec<Said>>);

impl obelus_ui::shapes::Shapes for Heard {
    fn behind(&self, _area: Rect, _ground: Color, _cells: &[ratatui::buffer::Cell]) {}

    fn capped(&self, keys: &str, area: Rect, cap: Color, page: Color, edge: Color) {
        if let Ok(mut said) = self.0.lock() {
            said.push(Said {
                keys: keys.to_string(),
                area,
                cap,
                page,
                edge,
            });
        }
    }
}

/// The one front end this binary has, said once however many tests ask.
fn heard() -> &'static Arc<Heard> {
    static HEARD: OnceLock<Arc<Heard>> = OnceLock::new();
    HEARD.get_or_init(|| {
        let heard = Arc::new(Heard::default());
        obelus_ui::shapes::drawn_by(heard.clone());
        heard
    })
}

/// A foot with one key on it, drawn into a grid this test owns.
///
/// Tall enough for the rule and the row under it, and wide enough that the
/// key is not the one that ran out of room.
fn a_foot(rows: u16) -> CellBuffer {
    let hints = [obelus_ui::Hint::common(
        KeyChord::parse("f1").expect("f1 is a key"),
        "Read it",
    )];
    let area = Rect {
        x: 0,
        y: 0,
        width: 60,
        height: rows,
    };
    let mut cells = CellBuffer::empty(area);
    obelus_ui::foot_without_a_card(&mut cells, area, &hints, &DARK);
    cells
}

/// Break: make `shapes::capped` do nothing, and a window has a foot of
/// keys with nothing saying which cells are caps.
#[test]
fn a_foot_says_where_its_caps_are() {
    let heard = heard();
    // Its own row, so that a test drawing its own foot beside this one
    // cannot be mistaken for it.
    let rows = 9;
    let _cells = a_foot(rows);
    let said = heard.0.lock().expect("nothing poisoned it").clone();
    let mine: Vec<Said> = said
        .into_iter()
        .filter(|said| said.area.y == rows - 1)
        .collect();
    assert_eq!(mine.len(), 1, "one key, one cap: {mine:?}");
    let cap = &mine[0];
    // `f1` with a blank either side of it, where the row starts.
    assert_eq!((cap.area.x, cap.area.width), (2, 4), "{cap:?}");
    assert_eq!(cap.keys, "f1");
    assert_eq!(cap.cap, DARK.raised_background);
    assert_eq!(cap.page, DARK.background);
    assert_eq!(cap.edge, DARK.gutter);
}

/// Break: have `capped` say where the cap is without writing the cells,
/// and `ob` loses the one cap a terminal has -- which is the invariant
/// everything said on this channel rests on.
#[test]
fn the_cells_are_the_cap_a_terminal_draws() {
    let rows = 7;
    let cells = a_foot(rows);
    let y = rows - 1;
    for x in 2..6 {
        let cell = &cells[(x, y)];
        assert_eq!(
            cell.bg, DARK.raised_background,
            "the cap's ground at {x}: {cell:?}"
        );
    }
    let inside: String = (2..6).map(|x| cells[(x, y)].symbol()).collect();
    assert_eq!(inside, " f1 ");
}

/// Break: drop the `cap_around` beside the card's `write`, and the one
/// place in Obelus that is nothing but a list of keys is the one place a
/// key is not drawn as one.
#[test]
fn the_card_of_every_key_says_a_cap_round_each() {
    assert!(
        !obelus_icons::enabled(),
        "a test that turned the glyphs on would change what a key is spelled as"
    );
    let heard = heard();
    let hints = [obelus_ui::Hint::common(
        KeyChord::parse("alt+m").expect("alt+m is a key"),
        "Go",
    )];
    // Twenty rows, so the card is centred where no foot of this binary's
    // is: the recorder is shared, and a test that took somebody else's cap
    // for its own would pass with this one broken.
    let area = Rect {
        x: 0,
        y: 0,
        width: 60,
        height: 20,
    };
    let mut cells = CellBuffer::empty(area);
    obelus_ui::keys_card(&mut cells, area, &hints, &DARK);

    let said = heard.0.lock().expect("nothing poisoned it").clone();
    let mine: Vec<Said> = said.into_iter().filter(|said| said.area.y == 10).collect();
    assert_eq!(mine.len(), 1, "one key, one cap: {mine:?}");
    // `alt+m` with the blank either side that the card leaves it, which is
    // the page's rather than the cap's: the cells are untouched.
    assert_eq!(mine[0].area.width, 7, "{:?}", mine[0]);
    assert_eq!(mine[0].keys, "alt+m");
    assert_eq!(
        mine[0].cap, DARK.raised_background,
        "the panel's own ground"
    );
    assert_eq!(mine[0].page, DARK.raised_background);
}
