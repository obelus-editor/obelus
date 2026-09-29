//! Where a pane is, as a window is told it.
//!
//! A binary of its own because who is drawing is said once for the whole
//! process (`obelus_ui::shapes::drawn_by`): a test beside these that drew
//! anything would have its panes written down here as well.

mod support;

use std::sync::{Arc, Mutex, OnceLock};

use obelus_app::app::App;
use obelus_component::picker::{PickerItem, PickerLayout, PickerValue};
use obelus_ui::shapes::Joined;
use ratatui::{buffer::Cell, layout::Rect, style::Color};

/// A front end that writes down where each pane was.
#[derive(Default)]
struct Heard {
    panes: Mutex<Vec<(Rect, Joined)>>,
    /// The mark the light runs across, and the two colours it runs
    /// between.
    marks: Mutex<Vec<(Rect, Color, Color)>>,
}

impl obelus_ui::shapes::Shapes for Heard {
    fn behind(&self, area: Rect, joined: Joined, _ground: Color, _cells: &[Cell]) {
        if let Ok(mut panes) = self.panes.lock() {
            panes.push((area, joined));
        }
    }

    fn scrolled(&self, _area: Rect, _top: i64, _bar: Option<obelus_ui::shapes::Bar>) {}

    fn ticked(&self, _area: Rect, _on: bool) {}

    fn ruled(&self, _area: Rect) {}

    fn capped(&self, _keys: &str, _area: Rect, _cap: Color, _page: Color, _edge: Color) {}

    fn barred(&self, _bar: obelus_ui::shapes::Bar) {}

    fn sheened(&self, area: Rect, from: Color, to: Color) {
        if let Ok(mut marks) = self.marks.lock() {
            marks.push((area, from, to));
        }
    }
}

fn heard() -> &'static Arc<Heard> {
    static HEARD: OnceLock<Arc<Heard>> = OnceLock::new();
    HEARD.get_or_init(|| {
        let heard = Arc::new(Heard::default());
        obelus_ui::shapes::drawn_by(heard.clone());
        heard
    })
}

fn items(labels: &[&str]) -> Vec<PickerItem> {
    labels
        .iter()
        .map(|label| PickerItem {
            prose: false,
            marker: None,
            icon: None,
            label: (*label).to_string(),
            detail: None,
            trailing: None,
            changed: None,
            value: PickerValue::File(label.into()),
            enabled: true,
            colours: None,
            status: None,
            depth: 0,
            opens: None,
            kind: None,
            tab: None,
        })
        .collect()
}

/// A compact list's pane runs from the rule over it to the rule under it,
/// because those are its edges: a window draws each line in the middle of
/// its row and ends the glass there.
///
/// Deliberate breaks: pass `region` rather than `with_its_rules(region,
/// room, edge)` in `list_over`, and the pane starts a row below the line
/// over it; answer `band.bottom()` for the bottom in `with_its_rules`,
/// and it stops a row above the one under it. Either way a window's glass
/// stops short of an edge a line says the list has -- the gap this was
/// written to close, found at the top first and at the foot after.
#[test]
fn a_compact_list_is_a_pane_from_rule_to_rule() {
    let heard = heard();
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.statuses_for_test(std::collections::HashMap::new());
    app.open_picker_for_test(
        items(&["alpha", "beta", "gamma"]),
        PickerLayout::Compact { rows: 10 },
    );
    let cells = support::cells_of(&mut app, 60, 24);

    let panes = heard.panes.lock().expect("nothing poisoned it").clone();
    let standing: Vec<Rect> = panes
        .into_iter()
        .filter(|(_, joined)| *joined == Joined::Below)
        .map(|(area, _)| area)
        .collect();
    assert_eq!(standing.len(), 1, "one list, one pane: {standing:?}");
    let pane = standing[0];
    assert!(pane.y > 0, "there is code above it: {pane:?}");
    // The pane's first row is the rule, in the cells a terminal draws --
    // which is what the window checks the line against.
    for x in pane.left()..pane.right() {
        assert_eq!(cells[(x, pane.y)].symbol(), "\u{2500}", "at {x}");
    }
    // And its last row is the rule over the status row, which the list
    // stands on.
    let last = pane.bottom() - 1;
    for x in pane.left()..pane.right() {
        assert_eq!(cells[(x, last)].symbol(), "\u{2500}", "at {x} on the foot");
    }
    assert_eq!(last, 22, "the row over the status row");
    // And the list is between them, inside the same pane.
    let under: String = (pane.left()..pane.right())
        .map(|x| cells[(x, pane.y + 1)].symbol())
        .collect();
    let rows: String = (pane.y + 1..last)
        .flat_map(|y| (pane.left()..pane.right()).map(move |x| (x, y)))
        .map(|at| cells[at].symbol())
        .collect();
    assert!(
        rows.contains("alpha") && rows.contains("gamma"),
        "the rows are in the pane, starting with {under:?}"
    );
}

/// The mark on the welcome screen says where it is and what the light on
/// it runs between.
///
/// Both directions, the way everything on this channel is. A window with
/// nothing said has a plate and no reason to light it -- it would draw the
/// eight bands a terminal draws and leave the pixel it has unused. And the
/// cells have to be that ramp whether or not anybody heard, because that
/// is the invariant the whole channel rests on.
///
/// The colours are checked against the cells rather than against the theme
/// they came from, which is what "said" has to mean here: a front end that
/// was handed two colours the mark is not drawn in would light it to
/// somewhere it never goes.
///
/// Deliberate break: drop the `shapes::sheened` in `lavish` and the first
/// assertion goes; hand it `self.theme.foreground` for either colour and
/// the last one does, because the letters on screen are nowhere near it.
#[test]
fn the_mark_says_where_it_is_and_what_the_light_runs_between() {
    let heard = heard();
    heard.marks.lock().expect("the marks").clear();

    let mut app = App::new(Vec::new());
    app.working_directory_for_test(std::path::PathBuf::from("/tmp/obelus"));
    let cells = support::cells_of(&mut app, 64, 20);

    let marks = heard.marks.lock().expect("the marks").clone();
    assert_eq!(marks.len(), 1, "not one mark: {marks:?}");
    let (area, from, to) = marks[0];

    let (Color::Rgb(fr, fg, fb), Color::Rgb(tr, tg, tb)) = (from, to) else {
        panic!("the light runs between two colours, not {from:?} and {to:?}");
    };
    // Every letter of the mark is somewhere on the ramp between them,
    // because that is what the cells hold: a step of it per column, out
    // and back.
    let between = |a: u8, b: u8, at: u8| at >= a.min(b) && at <= a.max(b);
    let mut letters = 0;
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let cell = &cells[(x, y)];
            if cell.symbol().trim().is_empty() {
                continue;
            }
            let Color::Rgb(r, g, b) = cell.fg else {
                panic!("a letter of the mark is {:?}, not a colour", cell.fg);
            };
            assert!(
                between(fr, tr, r) && between(fg, tg, g) && between(fb, tb, b),
                "the light is said to run between {from:?} and {to:?}, and {:?} at {x},{y} is on neither",
                cell.fg
            );
            letters += 1;
        }
    }
    assert!(letters > 100, "the mark is barely on the screen: {letters}");
}
