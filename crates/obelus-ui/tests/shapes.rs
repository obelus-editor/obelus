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
struct Heard {
    caps: Mutex<Vec<Said>>,
    switches: Mutex<Vec<(Rect, bool)>>,
    bars: Mutex<Vec<obelus_ui::shapes::Bar>>,
    rules: Mutex<Vec<Rect>>,
    panes: Mutex<Vec<(Rect, obelus_ui::shapes::Joined, Color, String)>>,
}

impl obelus_ui::shapes::Shapes for Heard {
    fn behind(
        &self,
        area: Rect,
        joined: obelus_ui::shapes::Joined,
        ground: Color,
        cells: &[ratatui::buffer::Cell],
    ) {
        if let Ok(mut panes) = self.panes.lock() {
            let under = cells.iter().map(ratatui::buffer::Cell::symbol).collect();
            panes.push((area, joined, ground, under));
        }
    }

    fn scrolled(&self, _area: Rect, _top: i64, _bar: Option<obelus_ui::shapes::Bar>) {}

    fn barred(&self, bar: obelus_ui::shapes::Bar) {
        if let Ok(mut bars) = self.bars.lock() {
            bars.push(bar);
        }
    }

    fn ticked(&self, area: Rect, on: bool) {
        if let Ok(mut switches) = self.switches.lock() {
            switches.push((area, on));
        }
    }

    fn ruled(&self, area: Rect) {
        if let Ok(mut rules) = self.rules.lock() {
            rules.push(area);
        }
    }

    fn sheened(&self, _area: Rect, _from: Color, _to: Color) {}

    fn capped(&self, keys: &str, area: Rect, cap: Color, page: Color, edge: Color) {
        if let Ok(mut said) = self.caps.lock() {
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

/// Break: drop the `shapes::ticked` beside the glyph in `ticked`, and a
/// window has a switch with nothing saying it is one -- so it draws the
/// character standing in for the box instead of the box.
#[test]
fn a_switch_says_which_cell_it_is_in_and_which_way_it_is_set() {
    let heard = heard();
    let mut hint =
        obelus_ui::Hint::common(KeyChord::parse("alt+i").expect("alt+i is a key"), "Ignored");
    hint.switched = Some(true);
    // Its own row again, so that another test's foot is not taken for
    // this one: the recorder is shared by everything in this binary.
    let rows = 13;
    let area = Rect {
        x: 0,
        y: 0,
        width: 60,
        height: rows,
    };
    let mut cells = CellBuffer::empty(area);
    obelus_ui::foot_without_a_card(&mut cells, area, &[hint], &DARK);

    let said = heard.switches.lock().expect("nothing poisoned it").clone();
    let mine: Vec<(Rect, bool)> = said
        .into_iter()
        .filter(|(area, _)| area.y == rows - 1)
        .collect();
    assert_eq!(mine.len(), 1, "one switch: {mine:?}");
    let (where_it_is, on) = mine[0];
    assert!(on, "the one it was given");
    // One cell, which is the one the glyph is in: the blank after it
    // belongs to the glyph.
    assert_eq!((where_it_is.width, where_it_is.height), (1, 1));
    assert_eq!(
        cells[(where_it_is.x, where_it_is.y)].symbol(),
        obelus_ui::tick(true).to_string(),
        "and the cell still says it, which is all a terminal has"
    );
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
    let said = heard.caps.lock().expect("nothing poisoned it").clone();
    let mine: Vec<Said> = said
        .into_iter()
        .filter(|said| said.area.y == rows - 1)
        .collect();
    assert_eq!(mine.len(), 1, "one key, one cap: {mine:?}");
    let cap = &mine[0];
    // `f1` with a blank either side of it, where the row starts.
    assert_eq!((cap.area.x, cap.area.width), (2, 4), "{cap:?}");
    assert_eq!(cap.keys, "F1");
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
    assert_eq!(inside, " F1 ");
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

    let said = heard.caps.lock().expect("nothing poisoned it").clone();
    let mine: Vec<Said> = said.into_iter().filter(|said| said.area.y == 10).collect();
    assert_eq!(mine.len(), 1, "one key, one cap: {mine:?}");
    // `alt+m` with the blank either side that the card leaves it, which is
    // the page's rather than the cap's: the cells are untouched.
    assert_eq!(mine[0].area.width, 7, "{:?}", mine[0]);
    assert_eq!(mine[0].keys, "Alt+m");
    assert_eq!(mine[0].cap, DARK.background, "the panel's own ground");
    assert_eq!(mine[0].page, DARK.background);
}

/// Break: drop the `shapes::ruled` at the end of `rule`, and a window has
/// a row of `─` with nothing saying it is a line -- so it spells it in the
/// font, half a row from the edge of any pane the line is the edge of.
#[test]
fn a_rule_says_which_row_it_is_on() {
    let heard = heard();
    // Its own height, so that the rule is on a row no other test's is.
    let rows = 17;
    let cells = a_foot(rows);
    let said = heard.rules.lock().expect("nothing poisoned it").clone();
    let mine: Vec<Rect> = said.into_iter().filter(|area| area.y == rows - 2).collect();
    assert_eq!(mine.len(), 1, "one rule over the foot: {mine:?}");
    // One row, and every cell of it the glyph a terminal draws: what the
    // window checks it against.
    assert_eq!(mine[0].height, 1);
    for x in mine[0].left()..mine[0].right() {
        assert_eq!(cells[(x, mine[0].y)].symbol(), "\u{2500}", "at {x}");
    }
}

/// A panel says what it was put over, and that is the whole of what a
/// window needs to draw its frame: which cells are the ring, and what
/// shows outside the line.
///
/// Break: move the `shapes::behind` in `panel` below its `fill`, and what
/// a window is told is under the card of every key is the card itself --
/// the glass inside its frame shows a blurred copy of the panel instead
/// of the list it was opened over. Break again by dropping it, and the
/// card is spelled in `╭─╮` on a square of its own ground, an opaque box
/// whatever the window can do.
#[test]
fn a_panel_says_what_it_was_put_over() {
    let heard = heard();
    let hints = [obelus_ui::Hint::common(
        KeyChord::parse("alt+w").expect("alt+w is a key"),
        "Go",
    )];
    // Forty rows, so the card is centred where no other test's is.
    let area = Rect {
        x: 0,
        y: 0,
        width: 60,
        height: 40,
    };
    let mut cells = CellBuffer::empty(area);
    // What it is put over: a page of one letter.
    for y in 0..area.height {
        for x in 0..area.width {
            cells[(x, y)].set_symbol("z");
        }
    }
    obelus_ui::keys_card(&mut cells, area, &hints, &DARK);

    let panes = heard.panes.lock().expect("nothing poisoned it").clone();
    let mine: Vec<_> = panes
        .into_iter()
        .filter(|(pane, ..)| pane.y == 17)
        .collect();
    assert_eq!(mine.len(), 1, "one card, one pane: {mine:?}");
    let (frame, joined, ground, under) = &mine[0];
    assert_eq!(
        *joined,
        obelus_ui::shapes::Joined::Nowhere,
        "edges all round"
    );
    assert_eq!(*ground, DARK.background, "the card's own ground");
    assert!(
        under.chars().all(|letter| letter == 'z'),
        "what was there before the card: {under:?}"
    );
    // The frame is the pane's outermost ring, which is what the window
    // checks the pane against.
    let (right, bottom) = (frame.right() - 1, frame.bottom() - 1);
    assert_eq!(cells[(frame.x, frame.y)].symbol(), "\u{256d}");
    assert_eq!(cells[(right, bottom)].symbol(), "\u{256f}");
}

/// A reading longer than its region, drawn into a grid this test owns.
///
/// A reading because it is the shortest way to a bar: rows in, cells out,
/// and the one condition for drawing a bar is that there are more rows
/// than there is room.
fn a_long_reading(y: u16, height: u16, rows: usize) -> (CellBuffer, Rect) {
    let area = Rect {
        x: 0,
        y,
        width: 40,
        height,
    };
    let mut cells = CellBuffer::empty(Rect {
        x: 0,
        y: 0,
        width: 40,
        height: y + height,
    });
    let rows: Vec<obelus_row::Row> = (0..rows).map(|_| obelus_row::Row::of(Vec::new())).collect();
    obelus_ui::reading::draw(&mut cells, area, &rows, 0, &DARK, DARK.background);
    (cells, area)
}

/// Break: drop the `shapes::barred` at the foot of `scrollbar`, and every
/// bar in Obelus goes back to being a column of cells with nothing saying
/// it is a bar -- so a window draws the block a terminal draws and none of
/// the ten lists gets a shape of its own.
#[test]
fn a_bar_says_where_it_is_and_how_much_of_it_is_the_mark() {
    let heard = heard();
    // Its own row again: the recorder is shared by everything in this
    // binary, and every other test here draws at the bottom of its grid.
    let y = 21;
    let (_cells, area) = a_long_reading(y, 10, 40);

    let said = heard.bars.lock().expect("nothing poisoned it").clone();
    let mine: Vec<obelus_ui::shapes::Bar> =
        said.into_iter().filter(|bar| bar.area.y == y).collect();
    assert_eq!(mine.len(), 1, "one reading, one bar: {mine:?}");
    let bar = mine[0];

    // The last column of the region and nothing else: a bar that claimed
    // the whole width would have a window painting over the text.
    assert_eq!(
        (bar.area.x, bar.area.width),
        (area.right() - 1, 1),
        "{bar:?}"
    );
    assert_eq!(bar.area.height, area.height);
    // Ten rows of forty, so a quarter of the bar, at the top of it.
    assert_eq!((bar.mark, bar.thumb), (0, 2), "{bar:?}");
}

/// Break: make `BAR` the line the editor's note says this was once drawn
/// as, and `ob` is back to a column every rule that crosses it has to
/// decide about. Or drop the `put` and keep the `barred`, and the terminal
/// has nothing at all -- which is the invariant everything said on this
/// channel rests on.
///
/// The other half of the test above, and each passes with the other
/// broken: that one holds what was *said* about the mark, this one holds
/// the rows it was actually drawn on. A `thumb` that stopped agreeing with
/// `bar_reach` fails the first; cells that stopped being a bar fail this.
#[test]
fn the_cells_are_the_bar_a_terminal_draws() {
    let y = 33;
    let height = 10;
    let (cells, area) = a_long_reading(y, height, 40);
    let x = area.right() - 1;

    // Every row of the column is a full block, which is the shape the
    // editor's own note says a bar has to be: a block meets a rule and
    // needs nothing from it, where a line would have to decide.
    for row in 0..height {
        let cell = &cells[(x, y + row)];
        assert_eq!(cell.symbol(), "\u{2588}", "row {row}: {cell:?}");
    }

    // And the rows the mark covers are the brighter of the two colours,
    // which is what a front end that hears nothing has to go on.
    let marked: Vec<u16> = (0..height)
        .filter(|row| cells[(x, y + row)].fg == DARK.gutter_current)
        .collect();
    assert_eq!(marked, vec![0, 1], "the mark's rows");
    for row in 2..height {
        assert_eq!(
            cells[(x, y + row)].fg,
            DARK.scrollbar_track,
            "the track at {row}"
        );
    }
}
