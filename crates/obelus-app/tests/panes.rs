//! Where a pane is, as a window is told it.
//!
//! A binary of its own because who is drawing is said once for the whole
//! process (`obelus_ui::shapes::drawn_by`): a test beside these that drew
//! anything would have its panes written down here as well.

mod support;

use std::{
    sync::{Arc, Mutex, OnceLock},
    thread::ThreadId,
};

use obelus_app::app::App;
use obelus_component::picker::{PickerItem, PickerLayout, PickerValue};
use obelus_ui::shapes::Joined;
use ratatui::{buffer::Cell, layout::Rect, style::Color};

/// A front end that writes down where each pane was.
#[derive(Default)]
struct Heard {
    /// The mark the light runs across, and the two colours it runs
    /// between.
    marks: Mutex<Vec<(Rect, Color, Color)>>,
    /// And where one thing was said to stop and the next to begin.
    partings: Mutex<Vec<Rect>>,
    /// Every band and every pane, in the order they were said, with the
    /// thread that said them: the order is the claim, and the tests in
    /// this binary run side by side into the one recorder.
    order: Mutex<Vec<(ThreadId, Told)>>,
}

/// A band or a pane, as it was said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Told {
    Band(Rect),
    Pane(Rect, Joined),
    Page(Rect),
}

impl Heard {
    fn told(&self, told: Told) {
        if let Ok(mut order) = self.order.lock() {
            order.push((std::thread::current().id(), told));
        }
    }
}

impl obelus_ui::shapes::Shapes for Heard {
    fn behind(&self, area: Rect, joined: Joined, _ground: Color, _cells: &[Cell]) {
        self.told(Told::Pane(area, joined));
    }

    fn scrolled(&self, area: Rect, _top: i64, _bar: Option<obelus_ui::shapes::Bar>) {
        self.told(Told::Band(area));
    }

    fn paged(&self, area: Rect) {
        self.told(Told::Page(area));
    }

    fn ticked(&self, _area: Rect, _on: bool) {}

    fn ruled(&self, _area: Rect) {}

    fn capped(&self, _keys: &str, _area: Rect, _cap: Color, _page: Color, _edge: Color) {}

    fn barred(&self, _bar: obelus_ui::shapes::Bar) {}

    fn parted(&self, area: Rect) {
        if let Ok(mut partings) = self.partings.lock() {
            partings.push(area);
        }
    }

    fn sheened(&self, area: Rect, from: Color, to: Color) {
        if let Ok(mut marks) = self.marks.lock() {
            marks.push((area, from, to));
        }
    }
    fn stroked(&self, _stroke: obelus_ui::shapes::Stroke) {}
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
            section: None,
        })
        .collect()
}

/// A compact list's pane runs from the rule over it to the row it types
/// into, because the list and that row are one dialog: it arrives as one
/// thing, slid up from the foot of the screen, rather than as a list that
/// moves and a query that was already there.
///
/// The rule over it is an edge and a window ends the glass in the middle of
/// that row. The rule *under* it is not an edge any more -- it is inside
/// the pane, between the list and its own row.
///
/// Deliberate breaks: pass `region` rather than `with_its_rules(region,
/// room, edge)` in `list_over`, and the pane starts a row below the line
/// over it; hand `None` for `own_row`, and it stops at the rule with its
/// query left behind on a row that does not travel.
#[test]
fn a_compact_list_is_a_pane_from_rule_to_rule() {
    let since = told_so_far();
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.statuses_for_test(std::collections::HashMap::new());
    app.open_picker_for_test(
        items(&["alpha", "beta", "gamma"]),
        PickerLayout::Compact { rows: 10 },
    );
    let cells = support::cells_of(&mut app, 60, 24);

    // This thread's only: the names stand on the foot of a screen this
    // size too, and their test runs beside this one.
    let standing: Vec<Rect> = told_since(since)
        .into_iter()
        .filter_map(|told| match told {
            Told::Pane(area, Joined::Below) => Some(area),
            _ => None,
        })
        .collect();
    assert_eq!(standing.len(), 1, "one list, one pane: {standing:?}");
    let pane = standing[0];
    assert!(pane.y > 0, "there is code above it: {pane:?}");
    // The pane's first row is the rule, in the cells a terminal draws --
    // which is what the window checks the line against.
    for x in pane.left()..pane.right() {
        assert_eq!(cells[(x, pane.y)].symbol(), "\u{2500}", "at {x}");
    }
    // Its last row is the one the list types into, and the rule is the
    // row above that -- inside the pane, not the end of it.
    assert_eq!(pane.bottom() - 1, 23, "the row the list types into");
    let last = pane.bottom() - 2;
    for x in pane.left()..pane.right() {
        assert_eq!(cells[(x, last)].symbol(), "\u{2500}", "at {x} on the foot");
    }
    assert_eq!(last, 22, "the rule between the list and its own row");
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

/// What one test's frame said, in order: only this thread's, and only
/// what came after `since` of them.
fn told_since(since: usize) -> Vec<Told> {
    let me = std::thread::current().id();
    heard()
        .order
        .lock()
        .expect("nothing poisoned it")
        .iter()
        .filter(|(whose, _)| *whose == me)
        .skip(since)
        .map(|(_, told)| *told)
        .collect()
}

/// How much this thread has said so far, so that a frame drawn while
/// setting a test up is not taken for the one under test.
fn told_so_far() -> usize {
    told_since(0).len()
}

/// The last pane a frame said, with the bands said before it and the
/// bands said after it.
fn either_side_of_the_pane(told: &[Told]) -> (Rect, Joined, Vec<Rect>, Vec<Rect>) {
    let at = told
        .iter()
        .rposition(|told| matches!(told, Told::Pane(..)))
        .unwrap_or_else(|| panic!("a pane: {told:?}"));
    let Told::Pane(pane, joined) = told[at] else {
        unreachable!("found as a pane")
    };
    let bands = |told: &[Told]| -> Vec<Rect> {
        told.iter()
            .filter_map(|told| match told {
                Told::Band(area) => Some(*area),
                Told::Pane(..) | Told::Page(_) => None,
            })
            .collect()
    };
    (pane, joined, bands(&told[..at]), bands(&told[at + 1..]))
}

/// Whether any of these bands has a row with `word` in it.
fn shows(bands: &[Rect], cells: &ratatui::buffer::Buffer, word: &str) -> bool {
    bands.iter().any(|band| {
        (band.top()..band.bottom()).any(|y| {
            let row: String = (band.left()..band.right())
                .map(|x| cells[(x, y)].symbol())
                .collect();
            row.contains(word)
        })
    })
}

/// What a frame with a dialog over a file said: the file's band before the
/// pane and the dialog's own after it, with `word` in the dialog's.
///
/// `inside` for a dialog that covers the file whole, which is the case the
/// order is the only way to tell: the file's band is inside the pane as
/// much as the dialog's is.
fn the_file_is_said_before_the_pane(app: &mut App, word: &str, inside: bool) -> (Rect, Joined) {
    let since = told_so_far();
    let cells = support::cells_of(app, 60, 24);
    let told = told_since(since);
    let (pane, joined, under, own) = either_side_of_the_pane(&told);
    assert!(
        under
            .iter()
            .any(|band| { band.height > 3 && (!inside || pane.intersection(*band) == *band) }),
        "the file under the dialog, before its pane {pane:?}: {told:?}"
    );
    assert!(
        shows(&own, &cells, word),
        "the dialog's own rows, with {word:?} in them, after its pane {pane:?}: {told:?}"
    );
    (pane, joined)
}

/// A band under a pane is said before the pane, and the pane's own bands
/// after it -- for every dialog there is.
///
/// Which is how a window tells a file scrolling on under a dialog from the
/// dialog's list scrolling: by where they are, it cannot, once the dialog
/// is a full one -- the file's band is inside the pane as much as the
/// list's is. So a window that asked where they were slid the whole dialog
/// with the conversation behind it, a shudder on every line an agent
/// wrote, and asks the order instead (`obelus_gui::window`).
///
/// Deliberate breaks, one per dialog, each moving its `shapes::behind`
/// below what draws it: in `list_over`, below the `PickerView`; for the
/// settings, below `view.render`; for the counts, the same; for the names,
/// below `NamesView`. Each puts the dialog's own band before its pane, as
/// though the pane were over it.
#[test]
fn a_band_under_a_pane_is_said_before_it() {
    let mut app = App::new(vec![support::open_fixture("long.rs")]);
    app.statuses_for_test(std::collections::HashMap::new());
    app.open_picker_for_test(items(&["alpha", "beta", "gamma"]), PickerLayout::FullArea);
    the_file_is_said_before_the_pane(&mut app, "alpha", true);
}

/// The preview under a list is a page of its own inside the list's pane,
/// with the file it previews in it.
///
/// The pane's own colour is the page's too, and a window makes every cell
/// of a pane wearing it glass -- so the preview was glass wherever it said
/// nothing, and the file the list was opened over showed through behind
/// the one it previewed.
///
/// Deliberate break: take `shapes::paged` out of `list_over`, and nothing
/// says the preview is a page.
#[test]
fn a_preview_inside_a_pane_is_a_page_of_its_own() {
    let mut app = App::new(vec![support::open_fixture("long.rs")]);
    app.statuses_for_test(std::collections::HashMap::new());
    support::press_function(&mut app, 2);
    let since = told_so_far();
    let cells = support::cells_of(&mut app, 60, 24);
    let told = told_since(since);

    let (pane, _, _, _) = either_side_of_the_pane(&told);
    let pages: Vec<Rect> = told
        .iter()
        .filter_map(|told| match told {
            Told::Page(area) => Some(*area),
            _ => None,
        })
        .collect();
    assert_eq!(pages.len(), 1, "one preview, one page: {told:?}");
    let page = pages[0];
    assert_eq!(pane.intersection(page), page, "inside the pane {pane:?}");
    assert!(page.y > pane.y, "under the list's rows: {page:?}");
    assert!(
        shows(&[page], &cells, "fn before()"),
        "the page is not where the preview is: {page:?}"
    );
}

/// The settings, which take the whole screen.
#[test]
fn a_band_under_the_settings_is_said_before_them() {
    let mut app = App::new(vec![support::open_fixture("long.rs")]);
    app.statuses_for_test(std::collections::HashMap::new());
    // A file that is not there, which reads as the defaults: the settings
    // page is about a file, and this one is nobody's.
    app.config_file_for_test(
        std::env::temp_dir()
            .join(format!("obelus-panes-{}", std::process::id()))
            .join("config.toml"),
    );
    support::lay_out(&mut app, 60, 24);
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::ConfigOpen);
    let (_, joined) = the_file_is_said_before_the_pane(&mut app, "Theme", true);
    assert_eq!(joined, Joined::Screen, "the settings are the whole screen");
}

/// The counts, which take the whole screen as well.
#[test]
fn a_band_under_the_counts_is_said_before_them() {
    let mut app = App::new(vec![support::open_fixture("long.rs")]);
    app.statuses_for_test(std::collections::HashMap::new());
    app.working_directory_for_test(std::path::PathBuf::from("/tmp/obelus"));
    support::lay_out(&mut app, 60, 24);
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::CountLines);
    let tally = |code| obelus_search::counts::Tally {
        code,
        comments: 0,
        blanks: 0,
    };
    let rust = |files| obelus_search::counts::Language {
        name: "Rust",
        files,
        extension: Some("rs"),
        tally: tally(files * 100),
        children: Vec::new(),
    };
    app.handle(obelus_app::event::Event::Counted(Box::new(
        obelus_search::counts::Counted {
            languages: vec![rust(40)],
            files: Vec::new(),
            total: tally(4000),
        },
    )));
    let (_, joined) = the_file_is_said_before_the_pane(&mut app, "Rust", true);
    assert_eq!(joined, Joined::Screen, "the counts are the whole screen");
}

/// The names, which stand on the foot of the screen like a compact list.
#[test]
fn a_band_under_the_names_is_said_before_them() {
    let mut app = App::new(vec![support::open_fixture("long.rs")]);
    app.statuses_for_test(std::collections::HashMap::new());
    support::lay_out(&mut app, 60, 24);
    app.handle(obelus_app::event::Event::Fonts {
        here: vec!["Iosevka".to_string()],
        otherwise: None,
    });
    app.open_names("fonts");
    the_file_is_said_before_the_pane(&mut app, "Iosevka", false);
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

/// The notes say where one stops and the next begins, across the list.
///
/// The blank between two notes is a row in both front ends -- see
/// `Row::gap` -- and a window draws a hairline through the middle of it,
/// which a grid of cells cannot. Said by the view, because nothing in a
/// cell says which row is that blank; drawn only by the window, because
/// the terminal's answer is the blank itself. See
/// `obelus_ui::shapes::parted`.
///
/// Across the list whatever the note's depth, and short of the column the
/// bar is in. What the indent says the indent says -- the words step in
/// and the box steps in with them -- and a line starting in a different
/// column every time is a second thing to read down the one edge a reader
/// runs their eye along.
///
/// Deliberate break: say it on every row rather than on `row.gap` and the
/// page is ruled like a table. Put the note's indent back in the `x` and
/// the seams come out ragged down the left. Take the `SCROLLBAR_WIDTH`
/// off and a seam runs through the bar's own column.
#[test]
fn the_notes_say_where_one_stops_and_the_next_begins() {
    let heard = heard();
    heard.partings.lock().expect("the partings").clear();

    let scratch = support::Scratch::new("panes-parted");
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        // The middle one says three lines and points at a place, so most
        // of the rows on this page are *not* a note's first: without one
        // like it, every row is a head row and "only a note's first" is
        // not a claim this could tell from "every row".
        r#"
[[todo]]
said = "the first, which nothing is above"
done = false

[[todo]]
said = """
the second, which says three lines
so that the rows between its first
and the next note are nobody's head
"""
done = false
at = "sample.rs"
line = 2

[[todo]]
said = "under the second"
done = false
depth = 1
"#,
    )
    .expect("the notes");

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, 76, 18);
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    let dump = support::render(&mut app, 76, 18);

    let said = heard.partings.lock().expect("the partings").clone();
    // Two, for three notes: the first on screen has the page's own rule
    // above it and needs no blank. And two rather than one per row: the
    // middle note is four rows of screen and none of them is a blank.
    assert_eq!(said.len(), 2, "not one line between each pair:\n{dump}");
    assert!(said[0].y < said[1].y, "out of order: {said:?}");
    // One of them is a note hanging under another and the other is not,
    // and they are the same line: a seam says "a new note here" and says
    // nothing about whose.
    assert_eq!(said[0].x, said[1].x, "ragged down the left: {said:?}");
    assert_eq!(said[0].width, said[1].width, "and down the right: {said:?}");
    // And it stops where the rows do: that column is the bar's for the
    // whole page, and a line through it would show either side of the
    // mark.
    assert_eq!(
        said[0].right(),
        76 - obelus_ui::editor::SCROLLBAR_WIDTH,
        "through the bar's own column: {said:?}"
    );
}
