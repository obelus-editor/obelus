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

use obelus_keymap::KeyChord;
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
    turning: Mutex<Vec<Rect>>,
    bars: Mutex<Vec<obelus_ui::shapes::Bar>>,
    strokes: Mutex<Vec<obelus_ui::shapes::Stroke>>,
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

    fn stroked(&self, stroke: obelus_ui::shapes::Stroke) {
        if let Ok(mut strokes) = self.strokes.lock() {
            strokes.push(stroke);
        }
    }

    fn ticked(&self, area: Rect, on: bool) {
        if let Ok(mut switches) = self.switches.lock() {
            switches.push((area, on));
        }
    }

    fn spun(&self, area: Rect) {
        if let Ok(mut turning) = self.turning.lock() {
            turning.push(area);
        }
    }

    fn ruled(&self, area: Rect) {
        if let Ok(mut rules) = self.rules.lock() {
            rules.push(area);
        }
    }

    fn linked(&self, _area: Rect) {}

    fn parted(&self, _area: Rect) {}

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

/// Break: drop the `shapes::spun` after the glyph in `still_working`, and
/// a window has a mark with nothing saying it turns -- so it draws the
/// braille a frame a tick, which is the terminal's answer and not its own.
#[test]
fn the_mark_that_turns_says_which_cell_it_is_in() {
    let heard = heard();
    // A row of its own, for the reason the switch's test has one.
    let row = 17;
    let area = Rect {
        x: 0,
        y: row,
        width: 40,
        height: 1,
    };
    let mut cells = CellBuffer::empty(Rect {
        height: row + 1,
        y: 0,
        ..area
    });
    obelus_ui::status::still_working(&mut cells, area, None, "ab", None, 3, &DARK);

    let said = heard.turning.lock().expect("nothing poisoned it").clone();
    let mine: Vec<Rect> = said.into_iter().filter(|area| area.y == row).collect();
    assert_eq!(mine.len(), 1, "one mark: {mine:?}");
    let cell = mine[0];
    assert_eq!((cell.width, cell.height), (1, 1));
    assert_eq!(
        cells[(cell.x, cell.y)].symbol(),
        obelus_ui::spinning(3).to_string(),
        "and the cell still says it, which is all a terminal has"
    );
}

/// What a window asks of a cell it was told turns: a frame of the turn is
/// a place round it, each one further than the last, and anything else is
/// not the mark.
///
/// Break: answer `Some(0.0)` for every frame, and a window told not to
/// animate draws an arc that never moves while the terminal's turns.
#[test]
fn a_frame_of_the_turn_says_how_far_round_it_is() {
    let rounds: Vec<f32> = (0..10)
        .map(|phase| {
            obelus_ui::how_far_round(&obelus_ui::spinning(phase).to_string())
                .expect("a frame of the turn")
        })
        .collect();
    assert_eq!(rounds[0], 0.0, "the turn starts at the top: {rounds:?}");
    assert!(
        rounds
            .windows(2)
            .all(|pair| pair[0] < pair[1] && pair[1] < 1.0),
        "each frame further round than the last, short of a whole turn: {rounds:?}"
    );
    assert_eq!(obelus_ui::how_far_round("a"), None);
    assert_eq!(obelus_ui::how_far_round(" "), None);
    // A frame with something after it is a word that begins with one.
    assert_eq!(
        obelus_ui::how_far_round(&format!("{}x", obelus_ui::spinning(0))),
        None
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

/// How tall the region `a_changed_file` draws in is.
///
/// As many rows as the file has lines counting the empty one after its last
/// newline, so that a row of the map is a line of the file and the two
/// columns can be read against each other. At any other height they are the
/// same facts at two scales, which is true of the map and beside the point
/// here.
const ROWS: u16 = 8;

/// Whether a stroke is one of the ones drawn at this test's row.
///
/// The recorder is shared by everything in this binary and several of these
/// draw the same file, so a test that took "at or below my row" for its own
/// would count the strokes of every test that drew under it.
fn within(stroke: obelus_ui::shapes::Stroke, y: u16) -> bool {
    (y..y + ROWS).contains(&stroke.area.y)
}

/// A file with something changed in it, drawn as the editor draws it.
///
/// `y` is the row the region starts on, so that each test here reads only
/// its own strokes: the recorder is shared by everything in this binary.
///
/// Two lines replaced and one removed, with a line between the two so they
/// are two hunks: a run of more than one row, so coalescing has something
/// to coalesce, and a deletion, which is the one said differently in the
/// two columns. Adjacent, git reads the pair as one replacement of three
/// lines by two and there is no deletion in it at all.
fn a_changed_file(y: u16, width: u16) -> (CellBuffer, Rect) {
    let committed = "a\nb\nc\nd\ne\nf\ng\nh\n";
    let working = "a\nB\nC\nd\ne\ng\nh\n";
    let changes = obelus_git::Changes::between(committed, working);
    let buffer = obelus_buffer::Buffer::from_text(std::path::Path::new("changed.txt"), working);
    let area = Rect {
        x: 0,
        y,
        width,
        height: ROWS,
    };
    let mut cells = CellBuffer::empty(Rect {
        x: 0,
        y: 0,
        width,
        height: y + area.height,
    });
    let highlights = obelus_syntax::highlight::Highlights::default();
    ratatui::widgets::Widget::render(
        obelus_ui::editor::EditorView::for_buffer(
            &buffer,
            &highlights,
            &DARK,
            &[],
            Some(&changes),
            &[],
        ),
        area,
        &mut cells,
    );
    (cells, area)
}

/// Break: drop the `say_strokes` under the row loop in `editor::render`,
/// and a window has a margin with nothing saying which cells are change
/// marks -- so it spells the half blocks in the font and the reader gets
/// the terminal's margin in a window that could draw the shape.
///
/// Break again: join the seam to the run above it in `say_strokes` -- take
/// the `About::Seam` arm out, so a deletion coalesces like anything else --
/// and the arrow on the boundary becomes another row of bar, which is the
/// one claim the margin must not make: that the line is different when it
/// is not.
#[test]
fn the_margin_says_its_change_marks_in_runs() {
    let heard = heard();
    // Its own row, and one no other test in this binary draws at.
    let y = 51;
    let (_cells, _area) = a_changed_file(y, 40);

    let said = heard.strokes.lock().expect("nothing poisoned it").clone();
    let mine: Vec<obelus_ui::shapes::Stroke> = said
        .into_iter()
        .filter(|stroke| within(*stroke, y) && stroke.area.x == 0)
        .collect();
    assert_eq!(mine.len(), 2, "one run and one seam: {mine:?}");

    // The two replaced lines are one stroke of two rows, not two of one:
    // a hunk is one bar with two rounded ends.
    let run = mine[0];
    assert_eq!(run.about, obelus_ui::shapes::About::Rows, "{run:?}");
    assert_eq!((run.area.y, run.area.height), (y + 1, 2), "{run:?}");
    assert_eq!(run.area.width, 1, "one column: {run:?}");
    // Beside the text, which is the side the margin's own glyph leans.
    assert_eq!(run.side, obelus_ui::shapes::Side::Right, "{run:?}");

    // And the deletion is the boundary above the line that is now there,
    // which is a row of its own and never joined to anything.
    let seam = mine[1];
    assert_eq!(seam.about, obelus_ui::shapes::About::Seam, "{seam:?}");
    assert_eq!((seam.area.y, seam.area.height), (y + 5, 1), "{seam:?}");
    assert_eq!(seam.side, obelus_ui::shapes::Side::Right, "{seam:?}");
}

/// Break: say `About::Seam` for a deletion in `change_map` -- match on the
/// marker there the way `draw_marker` does -- and the map grows an arrow
/// pointing between two of its rows. Which is a lie at that scale: a row of
/// the map is a row of the *file* compressed, so the boundary between two
/// of them is the boundary between two dozen lines and not a place
/// anything can point at.
#[test]
fn the_map_says_every_change_the_same_way() {
    let heard = heard();
    // Its own row again.
    let y = 61;
    let width = 40;
    let (_cells, _area) = a_changed_file(y, width);

    // Just inside the bar, which is the last column of the region.
    let column = width - 2;
    let said = heard.strokes.lock().expect("nothing poisoned it").clone();
    let mine: Vec<obelus_ui::shapes::Stroke> = said
        .into_iter()
        .filter(|stroke| within(*stroke, y) && stroke.area.x == column)
        .collect();
    assert_eq!(mine.len(), 2, "the replacement and the deletion: {mine:?}");
    assert!(
        mine.iter()
            .all(|stroke| stroke.about == obelus_ui::shapes::About::Rows),
        "a deletion is a row here like any other: {mine:?}"
    );
    // Away from the bar, which is the side `MAP_MARK` leans -- the other
    // way from the margin's, and both of them the edge nearest the text.
    assert!(
        mine.iter()
            .all(|stroke| stroke.side == obelus_ui::shapes::Side::Left),
        "{mine:?}"
    );
    // Seven lines in seven rows, so the rows are the lines: the two
    // replaced ones as one stroke, and the deletion on its own.
    assert_eq!(
        (mine[0].area.y, mine[0].area.height),
        (y + 1, 2),
        "{mine:?}"
    );
    assert_eq!(
        (mine[1].area.y, mine[1].area.height),
        (y + 5, 1),
        "{mine:?}"
    );
}

/// Break: drop the `put` in `draw_marker` and keep the `marked.push`, or
/// the one in `change_map` and keep its own -- and `ob` loses the only
/// margin a terminal has, which is the invariant everything said on this
/// channel rests on. The window would go on drawing the strokes; the
/// terminal would have a blank column.
///
/// The other half of the two tests above, and each passes with the other
/// broken: they hold what was *said*, this holds what was written.
#[test]
fn the_cells_are_the_change_marks_a_terminal_draws() {
    let y = 71;
    let width = 40;
    let (cells, _area) = a_changed_file(y, width);
    let margin: String = (0..ROWS).map(|row| cells[(0, y + row)].symbol()).collect();
    // A bar beside each line that is there and differs, and the top edge
    // of the cell below the boundary where lines are not there at all.
    assert_eq!(margin, " \u{2590}\u{2590}  \u{2594}  ");

    let map: String = (0..ROWS)
        .map(|row| cells[(width - 2, y + row)].symbol())
        .collect();
    // Half a block for all three, leaning the other way: what kind of
    // change it is, this column says in the colour alone.
    assert_eq!(map, " \u{258c}\u{258c}  \u{258c}  ");
}
