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
    keys: Mutex<Vec<(String, Rect)>>,
    scrolls: Mutex<Vec<(Rect, i64)>>,
    links: Mutex<Vec<Rect>>,
}

impl obelus_ui::shapes::Shapes for Heard {
    fn behind(&self, _area: Rect, _joined: obelus_ui::shapes::Joined, _g: Color, _c: &[Cell]) {}

    fn scrolled(&self, area: Rect, top: i64, _bar: Option<obelus_ui::shapes::Bar>) {
        if let Ok(mut scrolls) = self.scrolls.lock() {
            scrolls.push((area, top));
        }
    }

    fn ticked(&self, _area: Rect, _on: bool) {}

    fn spun(&self, _area: Rect) {}

    fn ruled(&self, _area: Rect) {}

    fn capped(&self, keys: &str, area: Rect, _cap: Color, _page: Color, _edge: Color) {
        if let Ok(mut caps) = self.caps.lock() {
            caps.push(area);
        }
        if let Ok(mut heard) = self.keys.lock() {
            heard.push((keys.to_string(), area));
        }
    }

    fn barred(&self, _bar: obelus_ui::shapes::Bar) {}

    fn linked(&self, area: Rect) {
        if let Ok(mut links) = self.links.lock() {
            links.push(area);
        }
    }

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

/// A glyph on the status row has one blank after it in a window, not two.
///
/// Two is a terminal's: its Nerd Font draws the glyph two cells wide in the
/// one cell it was given, and the first blank is the half it bleeds into.
/// The window draws the `Mono` face it carries, one cell wide, so the
/// second blank was a cell of nothing between a file's glyph and its path.
///
/// Deliberate break: answer two blanks for a window in `gap_after_a_glyph`.
#[test]
fn a_glyph_has_one_blank_after_it_in_a_window() {
    let _turn = turn();
    obelus_config::drawn_in_a_window();
    obelus_icons::use_glyphs(true);

    let path = std::path::Path::new("tests/fixtures/long.rs");
    let mut app = App::new(vec![obelus_buffer::Buffer::open(path).expect("opening it")]);
    let dump = support::render(&mut app, 80, 12);
    let status = support::text_block(&dump)
        .lines()
        .filter_map(|row| row.split_once('|'))
        .map(|(_, cells)| cells.to_string())
        .next_back()
        .expect("a status row");
    let glyph = obelus_icons::for_path(path);
    let after: String = status
        .split_once(glyph)
        .unwrap_or_else(|| panic!("no file glyph on {status:?}"))
        .1
        .chars()
        .take(2)
        .collect();
    assert!(
        after.starts_with(' ') && !after.ends_with(' '),
        "not one blank between the glyph and the path: {status:?}"
    );
}

/// And so does the speaker's glyph in a conversation: the words begin one
/// blank after it, where a terminal leaves two.
///
/// Asked of the screen rather than of `reading_width`, which works its
/// width out from the very indent this is about.
///
/// Deliberate break: put `indent` back to a constant three, and the words
/// begin a cell further right with two blanks before them.
#[test]
fn a_conversations_words_begin_one_blank_after_the_speaker() {
    use crossterm::event::KeyCode;

    let _turn = turn();
    obelus_config::drawn_in_a_window();
    obelus_icons::use_glyphs(true);

    let (sender, events) = std::sync::mpsc::channel();
    let mut app = App::new(Vec::new());
    app.events_for_test(sender);
    app.agents_root_for_test(
        std::env::temp_dir().join(format!("obelus-window-indent-{}", std::process::id())),
    );
    let (width, height) = (76, 24);
    support::lay_out(&mut app, width, height);
    app.talk_to(
        "fake",
        std::path::Path::new(support::sh()),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    app.new_conversation();
    app.open_a_session_for_test();
    // Until the message is on the screen, however many events that takes:
    // the handshake, the session, and the words going into the transcript.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let mut said = false;
    let row = loop {
        let dump = support::render(&mut app, width, height);
        let found = support::text_block(&dump)
            .lines()
            .filter_map(|row| row.split_once('|'))
            .map(|(_, cells)| cells.to_string())
            // From the foot: the header names the conversation by its
            // first words too.
            .rfind(|row| row.contains("words of mine"));
        if let Some(row) = found {
            break row;
        }
        if !said && app.talking() == obelus_agent::Talking::Ready {
            support::type_text(&mut app, "words of mine");
            support::press(&mut app, KeyCode::Enter);
            said = true;
            continue;
        }
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        assert!(!left.is_zero(), "the message never arrived:\n{dump}");
        let event = events
            .recv_timeout(left)
            .unwrap_or_else(|_| panic!("nothing arrived, and the screen is:\n{dump}"));
        app.handle(event);
    };
    let before: Vec<char> = row.chars().take(4).collect();
    assert!(
        before[1] != ' ' && before[2] == ' ' && before[3] == 'w',
        "not the speaker, one blank and the words: {row:?}"
    );
}

/// A conversation tells the window where the words of each link are, and
/// nowhere else, so that the pointer can be a hand over them.
///
/// Deliberate breaks: saying nothing in the transcript's drawing leaves
/// the window no links; and saying each from the start of its row puts
/// the second link's area over the words before it.
#[test]
fn a_conversation_says_where_its_links_are() {
    use crossterm::event::KeyCode;

    let _turn = turn();
    obelus_config::drawn_in_a_window();
    let heard = heard();

    let (sender, events) = std::sync::mpsc::channel();
    let mut app = App::new(Vec::new());
    app.events_for_test(sender);
    app.agents_root_for_test(
        std::env::temp_dir().join(format!("obelus-window-links-{}", std::process::id())),
    );
    let (width, height) = (76, 24);
    support::lay_out(&mut app, width, height);
    app.talk_to(
        "fake",
        std::path::Path::new(support::sh()),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    app.new_conversation();
    app.open_a_session_for_test();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let mut said = false;
    let row = loop {
        if let Ok(mut links) = heard.links.lock() {
            links.clear();
        }
        let dump = support::render(&mut app, width, height);
        if let Some(row) = support::text_block(&dump)
            .lines()
            .find(|row| row.contains("the tool calls"))
        {
            break row.to_string();
        }
        if !said && app.talking() == obelus_agent::Talking::Ready {
            support::type_text(&mut app, "/links");
            support::press(&mut app, KeyCode::Enter);
            said = true;
            continue;
        }
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        assert!(!left.is_zero(), "the answer never arrived:\n{dump}");
        let event = events
            .recv_timeout(left)
            .unwrap_or_else(|_| panic!("nothing arrived, and the screen is:\n{dump}"));
        app.handle(event);
    };
    let y: u16 = row
        .split('|')
        .next()
        .and_then(|number| number.trim().parse().ok())
        .expect("a row number");
    let at = |words: &str| {
        Rect::new(
            u16::try_from(support::column_of(&row, words)).expect("a column"),
            y,
            u16::try_from(words.len()).expect("short"),
            1,
        )
    };
    // That row's: the answer goes on to a call with a link of its own.
    let mut links: Vec<Rect> = heard
        .links
        .lock()
        .map(|links| links.iter().filter(|area| area.y == y).copied().collect())
        .unwrap_or_default();
    links.sort_by_key(|area| area.x);
    links.dedup();
    assert_eq!(links, [at("the prompt turn"), at("the tool calls")]);
}

/// The welcome screen tells the window where its website's address is, so
/// that the pointer is a hand over it as it is over a conversation's links.
///
/// Deliberate breaks: saying nothing in `site` leaves the window no link;
/// and saying the whole of `at` puts the blanks round the address in it.
#[test]
fn the_welcome_screen_says_where_its_address_is() {
    let _turn = turn();
    obelus_config::drawn_in_a_window();
    let heard = heard();

    let mut app = App::new(Vec::new());
    app.working_directory_for_test(std::path::PathBuf::from("/tmp/obelus"));
    if let Ok(mut links) = heard.links.lock() {
        links.clear();
    }
    let dump = support::render(&mut app, 80, 24);
    let address = "obelus-editor.github.io/obelus";
    let row = support::text_block(&dump)
        .lines()
        .find(|row| row.contains(address))
        .unwrap_or_else(|| panic!("no address on the welcome screen:\n{dump}"))
        .to_string();
    let y: u16 = row
        .split('|')
        .next()
        .and_then(|number| number.trim().parse().ok())
        .expect("a row number");
    let mut links = heard
        .links
        .lock()
        .map(|links| links.clone())
        .unwrap_or_default();
    links.dedup();
    assert_eq!(
        links,
        [Rect::new(
            u16::try_from(support::column_of(&row, address)).expect("a column"),
            y,
            u16::try_from(address.len()).expect("short"),
            1,
        )]
    );
}

/// An agent at work does not run the application's clock where a window
/// is drawing its mark, and does again where the reader turned animation
/// off.
///
/// The window turns the mark itself, on its own clock; what this clock
/// moves is the braille in the cell, which the window does not draw -- so
/// it was twelve pages a second for as long as an agent worked, pushed at
/// a front end that threw them away. With animation off the window turns
/// the mark a frame a tick, and the tick is this clock's.
///
/// Deliberate break: drop `window_turns` from `wants_animating` and the
/// first assertion goes; leave out its `config.animation` and the second.
#[test]
fn a_window_turns_an_agents_mark_on_its_own_clock() {
    use crossterm::event::KeyCode;

    let _turn = turn();
    obelus_config::drawn_in_a_window();
    obelus_icons::use_glyphs(false);

    let (sender, events) = std::sync::mpsc::channel();
    let mut app = App::new(Vec::new());
    app.events_for_test(sender);
    app.agents_root_for_test(
        std::env::temp_dir().join(format!("obelus-window-clock-{}", std::process::id())),
    );
    let (width, height) = (76, 24);
    support::lay_out(&mut app, width, height);
    app.talk_to(
        "fake",
        std::path::Path::new(support::sh()),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    app.new_conversation();
    app.open_a_session_for_test();
    let pump = |app: &mut App, what: &str, until: obelus_agent::Talking| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while app.talking() != until {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            assert!(!left.is_zero(), "gave up waiting for {what}");
            let event = events
                .recv_timeout(left)
                .unwrap_or_else(|_| panic!("nothing arrived while waiting for {what}"));
            app.handle(event);
            support::lay_out(app, width, height);
        }
    };
    pump(&mut app, "the handshake", obelus_agent::Talking::Ready);
    support::type_text(&mut app, "/forever");
    support::press(&mut app, KeyCode::Enter);
    pump(
        &mut app,
        "it to start thinking",
        obelus_agent::Talking::Thinking,
    );

    support::render(&mut app, width, height);
    assert!(
        !app.is_waking(),
        "the window turns the mark and the application is ticking for it too"
    );

    let still = obelus_config::Config {
        animation: false,
        ..obelus_config::Config::default()
    };
    app.configure(still, vec!["animation"]);
    support::render(&mut app, width, height);
    assert!(
        app.is_waking(),
        "with animation off the mark turns on this clock, and it has stopped"
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

/// Every key a conversation names in words wears a cap in a window: the
/// one beside the row that says it is working, the one in the box, enter
/// on a row it hands back, and the way back to the end -- each on the row
/// it is written on.
///
/// They were plain words in the gutter's ink, so in a window the one key
/// a reader could not guess was the one key on the page not drawn as a
/// key.
///
/// Deliberate break: take the `cap_the_keys` out of the working row, out
/// of `offer_to_send_now`, out of `offer_enter`, or the `cap_around` out
/// of `the_way_back` -- each
/// leaves its key with no cap, and this names which. And counting one
/// blank fewer before the way back's key puts its cap a cell to the left,
/// and one blank before its arrow puts the arrow against the cap.
#[test]
fn the_keys_a_conversation_names_wear_caps() {
    use crossterm::event::KeyCode;

    let _turn = turn();
    obelus_config::drawn_in_a_window();
    obelus_icons::use_glyphs(false);
    let heard = heard();

    let (sender, events) = std::sync::mpsc::channel();
    let mut app = App::new(Vec::new());
    app.events_for_test(sender);
    app.agents_root_for_test(
        std::env::temp_dir().join(format!("obelus-window-tests-{}", std::process::id())),
    );
    let (width, height) = (76, 24);
    // The width the view wraps the transcript to, asked of the view: it is
    // a cell wider in a window than in a terminal, where the gap after the
    // speaker's glyph is two.
    let reading = obelus_ui::chat::reading_width(Rect::new(0, 0, width, height));
    support::lay_out(&mut app, width, height);
    app.talk_to(
        "fake",
        std::path::Path::new(support::sh()),
        &["tests/fixtures/fake-agent.sh".to_string()],
    );
    app.new_conversation();
    app.open_a_session_for_test();
    let pump = |app: &mut App, what: &str, until: &dyn Fn(&App) -> bool| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while !until(app) {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            assert!(!left.is_zero(), "gave up waiting for {what}");
            let event = events
                .recv_timeout(left)
                .unwrap_or_else(|_| panic!("nothing arrived while waiting for {what}"));
            app.handle(event);
            support::lay_out(app, width, height);
        }
    };
    // Which keys were capped on the row that says `words`, in one frame.
    let capped_beside = |app: &mut App, words: &str| -> Vec<String> {
        heard.keys.lock().expect("the keys").clear();
        let dump = support::render(app, width, height);
        let rows: Vec<String> = support::text_block(&dump)
            .lines()
            .filter_map(|row| row.split_once('|'))
            .map(|(_, cells)| cells.to_string())
            .collect();
        let y = rows
            .iter()
            .position(|row| row.contains(words))
            .unwrap_or_else(|| panic!("nothing says {words:?}:\n{dump}"));
        let keys: Vec<(String, Rect)> = heard
            .keys
            .lock()
            .expect("the keys")
            .iter()
            .filter(|(_, area)| usize::from(area.y) == y)
            .cloned()
            .collect();
        // Round the key, and not the cell beside it: a cap is said over
        // the blank either side of what it holds. And with air after it,
        // or what follows sits against its edge.
        for (keys, area) in &keys {
            let under: String = rows[y]
                .chars()
                .skip(usize::from(area.x) + 1)
                .take(keys.chars().count())
                .collect();
            assert_eq!(
                &under, keys,
                "the cap at {area:?} is not round its key:\n{dump}"
            );
            let after = rows[y].chars().nth(usize::from(area.right()));
            assert_eq!(
                after,
                Some(' '),
                "the cap at {area:?} touches what follows:\n{dump}"
            );
        }
        keys.into_iter().map(|(keys, _)| keys).collect()
    };

    pump(&mut app, "the handshake", &|app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::type_text(&mut app, "/filler");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, "something to scroll", &|app| {
        app.chat()
            .is_some_and(|chat| chat.rows(reading).len() > usize::from(height))
    });
    pump(&mut app, "that turn to end", &|app| {
        app.talking() == obelus_agent::Talking::Ready
    });
    support::press_control_key(&mut app, KeyCode::Home);
    assert_eq!(
        capped_beside(&mut app, "To the end"),
        ["Ctrl+End"],
        "the way back to the end"
    );
    support::press_control_key(&mut app, KeyCode::End);

    support::type_text(&mut app, "/forever");
    support::press(&mut app, KeyCode::Enter);
    pump(&mut app, "it to start thinking", &|app| {
        app.talking() == obelus_agent::Talking::Thinking
    });
    support::type_text(&mut app, "and this");
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(
        capped_beside(&mut app, "Stops it"),
        ["Esc"],
        "the key beside the working row"
    );
    // Words in the box, which is what it offers to send.
    support::type_text(&mut app, "more");
    assert_eq!(
        capped_beside(&mut app, "Sends it now"),
        ["Ctrl+Enter"],
        "the key in the box"
    );
    for _ in 0.."more".len() {
        support::press(&mut app, KeyCode::Backspace);
    }

    // Up, a row at a time, to the row waiting: the cursor walks the words,
    // the row that says it is working and the blank above it among them.
    for _ in 0..4 {
        let on = app.chat().map(obelus_component::chat::Chat::focus);
        if let Some(obelus_component::chat::Focus::Transcript(place)) = on
            && app.chat().is_some_and(|chat| {
                chat.rows(reading)
                    .get(place.row)
                    .is_some_and(|row| row.unsent.is_some())
            })
        {
            break;
        }
        support::press(&mut app, KeyCode::Up);
        support::render(&mut app, width, height);
    }
    assert_eq!(
        capped_beside(&mut app, "Takes it back"),
        ["Enter"],
        "the key on a row that hands something back"
    );
}

/// A terminal says where its view has got to, so a window slides the
/// screen as a program writes up it -- the way a file and a conversation do.
///
/// Broken deliberately by taking the declaration out of `TerminalView`: no
/// band the size of the terminal is ever said.
#[cfg(unix)]
#[test]
fn a_terminal_says_how_far_the_program_moved_it() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use obelus_app::event::Event;

    let _turn = turn();
    let heard = heard();
    let (sender, events) = std::sync::mpsc::channel();
    let mut app = App::new(Vec::new());
    app.events_for_test(sender);
    app.shell_for_test(std::path::PathBuf::from("/bin/sh"));
    let (width, height) = (60, 20);
    support::lay_out(&mut app, width, height);
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::TerminalOpen);
    let area = obelus_ui::editor_canvas(Rect::new(0, 0, width, height));
    let last = || {
        heard
            .scrolls
            .lock()
            .expect("the list")
            .iter()
            .rev()
            .find(|(band, _)| *band == area)
            .map(|(_, top)| *top)
    };
    let shown = |app: &App, words: &str| {
        app.terminal()
            .is_some_and(|terminal| terminal.screen().contents().contains(words))
    };
    let run = |app: &mut App, line: &str, until: &str| {
        support::type_text(app, line);
        app.handle(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while !shown(app, until) {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            assert!(!left.is_zero(), "gave up waiting for {until}");
            let event = events
                .recv_timeout(left)
                .expect("the shell to say something");
            app.handle(event);
        }
        // And then whatever is left of it, so the frame is of all of it --
        // for a moment and not until it is quiet, because a window's clock
        // is never quiet.
        let settled = std::time::Instant::now() + std::time::Duration::from_millis(300);
        while let Ok(event) =
            events.recv_timeout(settled.saturating_duration_since(std::time::Instant::now()))
        {
            app.handle(event);
        }
        support::lay_out(app, width, height);
    };
    // Off the bottom first, so every row after this is a row up.
    run(&mut app, "seq 1 40; echo filled-$((1 + 1))", "filled-2");
    let before = last().expect("the terminal never said where it was");
    // Five rows: the line that asked goes up as enter is pressed, then the
    // three numbers and the line saying it is done. The prompt after them
    // is written on the row that last one left, and moves nothing.
    run(&mut app, "seq 1 3; echo moved-$((2 + 2))", "moved-4");
    let after = last().expect("the terminal never said where it was");
    assert_eq!(after - before, 5, "the view moved {before} to {after}");
}
