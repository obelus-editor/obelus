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
    /// And where one thing was said to stop and the next to begin, with
    /// the thread that said it: a second test here opens the notes too.
    partings: Mutex<Vec<(ThreadId, Rect)>>,
    /// Every band and every pane, in the order they were said, with the
    /// thread that said them: the order is the claim, and the tests in
    /// this binary run side by side into the one recorder.
    order: Mutex<Vec<(ThreadId, Told)>>,
    /// The change marks, with the thread that said them: the editor says
    /// its margin in other tests' frames too.
    strokes: Mutex<Vec<(ThreadId, obelus_ui::shapes::Stroke)>>,
    /// What was under each pane, with the thread that said it: the glass
    /// is drawn from these cells, so a cell nobody wrote is a hole in it.
    under: Mutex<Vec<Under>>,
}

/// A pane, what was under it, and the thread that said it.
type Under = (ThreadId, Rect, Joined, Vec<Cell>);

/// A band or a pane, as it was said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Told {
    Band(Rect),
    Pane(Rect, Joined),
}

impl Heard {
    fn told(&self, told: Told) {
        if let Ok(mut order) = self.order.lock() {
            order.push((std::thread::current().id(), told));
        }
    }
}

impl obelus_ui::shapes::Shapes for Heard {
    fn behind(&self, area: Rect, joined: Joined, _ground: Color, cells: &[Cell]) {
        self.told(Told::Pane(area, joined));
        if let Ok(mut under) = self.under.lock() {
            under.push((std::thread::current().id(), area, joined, cells.to_vec()));
        }
    }

    fn scrolled(&self, area: Rect, _top: i64, _bar: Option<obelus_ui::shapes::Bar>) {
        self.told(Told::Band(area));
    }

    fn ticked(&self, _area: Rect, _on: bool) {}

    fn spun(&self, _area: Rect) {}

    fn ruled(&self, _area: Rect) {}

    fn capped(&self, _keys: &str, _area: Rect, _cap: Color, _page: Color, _edge: Color) {}

    fn barred(&self, _bar: obelus_ui::shapes::Bar) {}

    fn linked(&self, _area: Rect) {}

    fn parted(&self, area: Rect) {
        if let Ok(mut partings) = self.partings.lock() {
            partings.push((std::thread::current().id(), area));
        }
    }

    fn sheened(&self, area: Rect, from: Color, to: Color) {
        if let Ok(mut marks) = self.marks.lock() {
            marks.push((area, from, to));
        }
    }
    fn stroked(&self, stroke: obelus_ui::shapes::Stroke) {
        if let Ok(mut strokes) = self.strokes.lock() {
            strokes.push((std::thread::current().id(), stroke));
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
            version: None,
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
                Told::Pane(..) => None,
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

/// The status row under the settings is the page's colour before the glass
/// is laid over it, while a setting's words are being typed into that row
/// as much as while the page's filter is.
///
/// The row is written last, by whoever owns it, so a cell grid is the same
/// either way: what differed was only what the glass was drawn from. It was
/// filled only when the nearest thing took the row, and the line a setting
/// is typed on does not take the row -- it is the row -- so the glass read
/// cells nobody had written and drew a grey band with a dark edge under
/// the box.
///
/// Broken deliberately by asking `layers.taking_the_status_row()` again
/// before the fill in `obelus_ui::draw`: the row under the pane comes back
/// unwritten.
#[test]
fn the_row_a_setting_is_typed_on_is_there_under_the_glass() {
    let mut app = App::new(vec![support::open_fixture("long.rs")]);
    app.statuses_for_test(std::collections::HashMap::new());
    app.config_file_for_test(
        std::env::temp_dir()
            .join(format!("obelus-panes-typed-{}", std::process::id()))
            .join("config.toml"),
    );
    support::lay_out(&mut app, 60, 24);
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::ConfigOpen);
    support::type_text(&mut app, "speaks");
    support::press(&mut app, crossterm::event::KeyCode::Enter);
    the_status_row_is_under_the_glass(&mut app, "Speaks as", Joined::Screen);
}

/// And the same under a list, which a question on the status row opens
/// over without putting it away: renaming the row the reader is on in the
/// file list. The list's pane runs down to the row it types into, so the
/// glass is over the status row here as well.
///
/// Broken deliberately the same way, by asking
/// `layers.taking_the_status_row()` before the fill: the question is
/// nearest, it does not take the row, and the row under the list's pane
/// comes back unwritten.
#[test]
fn the_row_a_rename_is_typed_on_is_there_under_a_list() {
    let scratch = support::Scratch::new("panes-rename");
    scratch.write("src/hint.rs", "fn hint() {}\n");
    scratch.write("src/main.rs", "fn main() {}\n");
    let mut app = App::new(vec![
        obelus_buffer::Buffer::open(&scratch.join("src/hint.rs")).expect("opening it"),
    ]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    app.statuses_for_test(std::collections::HashMap::new());
    support::lay_out(&mut app, 60, 24);
    support::press_function(&mut app, 1);
    let joined = {
        let since = heard_under_so_far();
        let _ = support::cells_of(&mut app, 60, 24);
        last_pane_since(since).1
    };
    support::press_alt_key(&mut app, crossterm::event::KeyCode::Char('n'));
    assert!(app.picker().is_some(), "the question put the list away");
    the_status_row_is_under_the_glass(&mut app, "hint.rs", joined);
}

/// Draws a frame with `word` on its status row and says that the last
/// pane of the kind `joined` says reached that row and had the page's
/// colour under it there.
///
/// The kind and not merely the last, because a panel beside the caret is
/// a pane too, and one drawn after the dialog's would be taken for it.
fn the_status_row_is_under_the_glass(app: &mut App, word: &str, joined: Joined) {
    let since = heard_under_so_far();
    let cells = support::cells_of(app, 60, 24);
    let row: String = (0..60).map(|x| cells[(x, 23)].symbol()).collect();
    assert!(
        row.contains(word),
        "nothing is being typed on the row: {row:?}"
    );

    let me = std::thread::current().id();
    let under = heard().under.lock().expect("nothing poisoned it");
    let (pane, cells) = under
        .iter()
        .filter(|(whose, ..)| *whose == me)
        .skip(since)
        .filter(|(_, _, said, _)| *said == joined)
        .map(|(_, pane, _, cells)| (*pane, cells))
        .last()
        .unwrap_or_else(|| panic!("no {joined:?} pane was said"));
    assert_eq!(
        pane.bottom(),
        24,
        "the pane reaches the status row: {pane:?}"
    );
    let width = usize::from(pane.width);
    let last = cells.chunks(width).last().expect("the pane has rows");
    let page = app.theme().background;
    assert!(
        last.iter().all(|cell| cell.bg == page),
        "the row under the glass is not the page: {:?}",
        last.iter().map(|cell| cell.bg).collect::<Vec<_>>()
    );
}

/// The last pane this thread said after `since` of them, and its kind.
fn last_pane_since(since: usize) -> (Rect, Joined) {
    let me = std::thread::current().id();
    heard()
        .under
        .lock()
        .expect("nothing poisoned it")
        .iter()
        .filter(|(whose, ..)| *whose == me)
        .skip(since)
        .map(|(_, pane, joined, _)| (*pane, *joined))
        .last()
        .expect("a pane was said")
}

/// How many panes this thread has said what is under so far.
fn heard_under_so_far() -> usize {
    let me = std::thread::current().id();
    heard()
        .under
        .lock()
        .expect("nothing poisoned it")
        .iter()
        .filter(|(whose, ..)| *whose == me)
        .count()
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

/// The page saying the project has gone, which is the whole screen and is
/// laid over what the reader was in: the file under it is said first, so
/// a window has it to draw the glass from.
///
/// Broken deliberately by taking `shapes::behind` out of the page's arm in
/// the frame: no pane is said at all.
#[test]
fn the_page_saying_the_project_has_gone_is_glass_over_the_file() {
    let scratch = support::Scratch::new("panes-gone");
    let mut app = App::new(vec![support::open_fixture("long.rs")]);
    app.statuses_for_test(std::collections::HashMap::new());
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, 60, 24);
    std::fs::remove_dir_all(scratch.path()).expect("the tree going");
    app.handle(obelus_app::event::Event::Watched(obelus_watch::Changed {
        path: scratch.path().join("anything"),
    }));

    let since = told_so_far();
    let _ = support::cells_of(&mut app, 60, 24);
    let told = told_since(since);
    let (pane, joined, under, _) = either_side_of_the_pane(&told);
    assert_eq!(joined, Joined::Screen, "the page is the whole screen");
    assert_eq!(pane, Rect::new(0, 0, 60, 24), "the page is not the screen");
    assert!(
        under.iter().any(|band| band.height > 3),
        "the file under the page, before its pane: {told:?}"
    );
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
    let me = std::thread::current().id();
    let since = heard.partings.lock().expect("the partings").len();

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

    let said: Vec<Rect> = heard
        .partings
        .lock()
        .expect("the partings")
        .iter()
        .skip(since)
        .filter(|(whose, _)| *whose == me)
        .map(|(_, area)| *area)
        .collect();
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

/// The note the keys are on is marked by one stroke down the whole of it,
/// which is the figure the editor's margin draws a hunk in: a window draws
/// a run of rows as one rounded bar, and a mark that was a half cell per
/// row came out there as a stack of square blocks beside the note.
///
/// And the cells under it are the glyph a window checks the stroke against
/// (`Stroked::holds`, in `obg`), inked in the selection's colour: what the
/// window draws a stroke in is the cell's ink.
///
/// Deliberate breaks: take the `shapes::stroked` call out of the notes'
/// `render` and nothing is said; say it once per selected row and it is
/// four strokes; put `HALF` back to `\u{2590}`, the right half, and the
/// cells no longer say the left-hand stroke is there; ink it in the page's
/// colour on a ground of the selection's, which is what it was, and the
/// window draws a stroke the colour of the page.
#[test]
fn the_note_the_keys_are_on_is_one_stroke() {
    let heard = heard();
    let me = std::thread::current().id();

    let scratch = support::Scratch::new("panes-stroked");
    support::make_room_for_notes(scratch.path());
    std::fs::write(
        obelus_git::todo::path(scratch.path()).expect("a tree that is there"),
        // The second says three lines and points at a place: four rows of
        // screen, so one stroke and one per row are different answers.
        r#"
[[todo]]
said = "the first"
done = false

[[todo]]
said = """
the second, which says three lines
so that the note is taller
than one row of the screen
"""
done = false
at = "sample.rs"
line = 2
"#,
    )
    .expect("the notes");

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, 76, 18);
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TodoOpen);
    support::press(&mut app, crossterm::event::KeyCode::Down);
    let since = heard.strokes.lock().expect("the strokes").len();
    let cells = support::cells_of(&mut app, 76, 18);

    let said: Vec<obelus_ui::shapes::Stroke> = heard
        .strokes
        .lock()
        .expect("the strokes")
        .iter()
        .skip(since)
        .filter(|(whose, _)| *whose == me)
        .map(|(_, stroke)| *stroke)
        .collect();
    assert_eq!(said.len(), 1, "not one stroke for the note: {said:?}");
    let stroke = said[0];
    assert_eq!(stroke.side, obelus_ui::shapes::Side::Left);
    assert_eq!(stroke.about, obelus_ui::shapes::About::Rows);
    assert_eq!(stroke.area.height, 4, "not the whole note: {stroke:?}");
    let row = |y: u16| -> String { (0..76).map(|x| cells[(x, y)].symbol()).collect() };
    assert!(
        row(stroke.area.y).contains("the second"),
        "beside some other note: {}",
        row(stroke.area.y)
    );
    let held = app.theme().selection_background;
    for y in stroke.area.top()..stroke.area.bottom() {
        let cell = &cells[(stroke.area.x, y)];
        assert_eq!(cell.symbol(), "\u{258c}", "row {y} is not the stroke's");
        assert_eq!(cell.fg, held, "row {y} is not inked in the selection");
    }
}
