//! What the application does differently where a window is drawing.
//!
//! A binary of its own because `obelus_config::drawn_in_a_window` is said
//! once for the whole process and never unsaid -- the same cost every
//! global in Obelus is paid for in, and the reason the glyph switch's
//! tests take turns. A test beside these would be answering for a window
//! it never asked to be.

mod support;

use std::sync::{Arc, Mutex, OnceLock};

use obelus_app::app::App;
use ratatui::{buffer::Cell, layout::Rect, style::Color};

/// A front end that writes down where each key's cap was.
#[derive(Default)]
struct Heard {
    caps: Mutex<Vec<Rect>>,
}

impl obelus_ui::shapes::Shapes for Heard {
    fn paged(&self, _area: Rect) {}

    fn behind(&self, _area: Rect, _joined: obelus_ui::shapes::Joined, _g: Color, _c: &[Cell]) {}

    fn scrolled(&self, _area: Rect, _top: i64, _bar: Option<obelus_ui::shapes::Bar>) {}

    fn ticked(&self, _area: Rect, _on: bool) {}

    fn ruled(&self, _area: Rect) {}

    fn capped(&self, _keys: &str, area: Rect, _cap: Color, _page: Color, _edge: Color) {
        if let Ok(mut caps) = self.caps.lock() {
            caps.push(area);
        }
    }

    fn barred(&self, _bar: obelus_ui::shapes::Bar) {}

    fn parted(&self, _area: Rect) {}

    fn sheened(&self, _area: Rect, _from: Color, _to: Color) {}

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

/// A turn at the screen.
///
/// There is one front end for the process, so two tests drawing at once
/// write their shapes into the same list -- and the second read twelve
/// caps for six keys. Taking turns is what a global costs its tests, the
/// same as the glyph switch above.
fn turn() -> std::sync::MutexGuard<'static, ()> {
    static DRAWING: Mutex<()> = Mutex::new(());
    DRAWING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The welcome screen's sheen does not run the application's clock where a
/// window is drawing it.
///
/// What that clock moves is the ramp written into the cells, eight bands
/// of it sliding a step a tick, which is the sheen a terminal can draw. A
/// window draws the light itself, on its own clock and out of the two
/// colours the view said -- so the ramp is the one thing on that screen it
/// does not read, and a ticker for it is twelve pages a second pushed at a
/// front end that throws them away. On the one screen a reader leaves up
/// while they decide what to open.
///
/// Deliberate break: drop the `!obelus_config::in_a_window()`. The
/// application then asks to be woken twelve times a second for a colour
/// nobody reads.
#[test]
fn a_window_does_not_run_the_welcome_screens_clock() {
    let _turn = turn();
    obelus_config::drawn_in_a_window();
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(std::path::PathBuf::from("/tmp/obelus"));

    // Drawn, because whether anything wants animating is settled while a
    // frame is laid out: it is a fact about what is on the screen.
    let dump = support::render(&mut app, 64, 20);
    assert!(
        support::text_block(&dump).contains("Open a file"),
        "not the welcome screen:\n{dump}"
    );
    assert!(
        !app.is_waking(),
        "the window is drawing the sheen and the application is ticking for it too"
    );
}

/// The cap round a key on the welcome screen does not touch the mark
/// beside it.
///
/// `cap_around` lays its shape over the cell either side of the key,
/// because that is where a cap's own blanks would have been -- so the one
/// cell of air the screen leaves after a key is, in a window, entirely
/// cap. The mark then sits against the cap's edge and the two read as one
/// object. A terminal draws no cap here and one cell there is one cell,
/// which is why the gap is asked of the front end rather than being a
/// number.
///
/// Deliberate break: answer `1` for a window in `welcome::beside` and the
/// column after the cap is the mark rather than air.
#[test]
fn a_keys_cap_does_not_touch_the_mark_beside_it() {
    let _turn = turn();
    obelus_config::drawn_in_a_window();
    obelus_icons::use_glyphs(true);
    let heard = heard();
    heard.caps.lock().expect("the caps").clear();

    let mut app = App::new(Vec::new());
    app.working_directory_for_test(std::path::PathBuf::from("/tmp/obelus"));
    let dump = support::render(&mut app, 80, 24);
    // The cells alone: a dump's rows carry the row number in front of a
    // bar, and a column counted over that is three columns out.
    let rows: Vec<&str> = support::text_block(&dump)
        .lines()
        .filter_map(|row| row.split_once('|'))
        .map(|(_, cells)| cells)
        .collect();

    let caps = heard.caps.lock().expect("the caps").clone();
    // Six keys are offered, and a screen this size shows all of them.
    assert_eq!(
        caps.len(),
        6,
        "not the welcome screen's keys: {caps:?}\n{dump}"
    );
    for cap in caps {
        let row = rows
            .get(usize::from(cap.y))
            .unwrap_or_else(|| panic!("no row {}:\n{dump}", cap.y));
        let after = row
            .chars()
            .nth(usize::from(cap.right()))
            .unwrap_or_else(|| panic!("the cap runs off the row: {cap:?}\n{dump}"));
        assert_eq!(after, ' ', "the cap at {cap:?} touches {after:?}:\n{dump}");
    }
}

/// What only a window can do is offered in one: the palette lists
/// `switch-worktree`, and the keys page has a row to bind it on.
///
/// The other half of the terminal's leaving it out, in the binary that can
/// say it is a window. Broken deliberately by saying `InATerminal` for it in
/// `Command::drawn`: both of these go.
#[test]
fn a_window_offers_what_only_a_window_can_do() {
    let _turn = turn();
    obelus_config::drawn_in_a_window();
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 60, 12);
    support::press_control(&mut app, 'p');
    let listed = app
        .picker()
        .expect("the palette is open")
        .matches()
        .any(|item| item.label == "switch-worktree");
    assert!(listed, "a window's palette leaves out switch-worktree");
    support::press(&mut app, crossterm::event::KeyCode::Esc);

    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::ConfigOpen);
    support::press(&mut app, crossterm::event::KeyCode::Tab);
    let keys = app.settings().expect("the settings").key_rows();
    assert!(
        keys.contains(&obelus_command::Command::WorktreeList),
        "a window's keys page has no row for switch-worktree"
    );
}
