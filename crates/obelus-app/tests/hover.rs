//! What a server says a place is, and what Obelus does with it.
//!
//! Against values rather than against a server: the interesting rules are
//! where the answer is drawn, what it marks, and what happens when the
//! reader has moved on -- and a server cannot be made to answer late on
//! demand.

mod support;

use crossterm::event::KeyCode;
use obelus_app::app::App;
use obelus_buffer::Buffer;
use serde_json::json;

fn editing(name: &str, contents: &str) -> (support::Scratch, App) {
    let scratch = support::Scratch::new(name);
    let path = scratch.path().join("sample.rs");
    std::fs::write(&path, contents).expect("writing the file");
    let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, 76, 18);
    (scratch, app)
}

/// An answer in the shape rust-analyzer sends: a fenced signature, a rule,
/// and the documentation under it.
fn answered(range: Option<(u32, u32, u32)>) -> serde_json::Value {
    let mut answer = json!({
        "contents": {
            "kind": "markdown",
            "value": "```rust\npub fn iter(&self) -> Iter<'_, T>\n```\n\n---\n\nReturns an iterator over the slice.\n\nIt yields all items from start to end.\n\nAnd a third paragraph, so there is more of it than fits.\n\nAnd a fourth.\n\nAnd a fifth, which is off the bottom.\n\nAnd a sixth.\n\nAnd a seventh.\n\nAnd an eighth.\n"
        }
    });
    if let Some((line, from, to)) = range {
        answer["range"] = json!({
            "start": { "line": line, "character": from },
            "end": { "line": line, "character": to },
        });
    }
    answer
}

/// Puts the caret in `rows.iter()`, which is what the answers are about.
fn at_the_word(app: &mut App) {
    support::press(app, KeyCode::Down);
    support::press(app, KeyCode::Down);
    for _ in 0.."for line in rows.it".len() {
        support::press(app, KeyCode::Right);
    }
}

const SOURCE: &str = "fn main() {\n    let rows = vec![1];\n    for line in rows.iter() {}\n}\n";

#[test]
fn the_answer_is_drawn_over_the_place_it_is_about() {
    let (_scratch, mut app) = editing("hover-drawn", SOURCE);
    at_the_word(&mut app);
    app.hover_for_test(answered(None));

    let dump = support::render(&mut app, 76, 18);
    assert!(
        dump.contains("pub fn iter(&self) -> Iter<'_, T>"),
        "the signature is not on screen:\n{dump}"
    );
    assert!(
        dump.contains("Returns an iterator over the slice."),
        "the documentation is not on screen:\n{dump}"
    );
    // In a box of its own, and the signature in a box inside it: the
    // markdown renderer draws a fenced block the way it draws one in a
    // README.
    let rows: Vec<&str> = support::text_block(&dump).lines().collect();
    let panel = rows
        .iter()
        .position(|row| row.contains(support::PANEL_CORNER))
        .expect("a box");
    let word = rows
        .iter()
        .position(|row| row.contains("rows.iter()"))
        .expect("the line");
    assert!(panel > word, "the answer is not below the line it is about");
}

/// The server says which characters it is talking about, and a reader
/// asking about a long line should be able to see which word they got an
/// answer about.
#[test]
fn the_characters_the_answer_is_about_are_marked() {
    let (_scratch, mut app) = editing("hover-marked", SOURCE);
    at_the_word(&mut app);
    app.hover_for_test(answered(Some((2, 21, 25))));

    let dump = support::render(&mut app, 76, 18);
    let rows: Vec<&str> = support::text_block(&dump).lines().collect();
    let styles: Vec<&str> = support::style_block(&dump).lines().collect();
    let at = rows
        .iter()
        .position(|row| row.contains("rows.iter()"))
        .expect("the line");
    let row = rows[at];
    let cells = &styles[at][row.find('|').expect("a divider") + 1..];
    let text = &row[row.find('|').expect("a divider") + 1..];
    let column = text.find("iter").expect("the word");

    // The *background*, not the style: what marks a run is a background,
    // because the characters keep the colours the file gives them -- and
    // the cell beside it is a different colour either way, so a test that
    // compared styles would pass with nothing marked at all.
    let ground = |letter: char| {
        support::legend_block(&dump)
            .lines()
            .find(|entry| entry.starts_with(letter))
            .and_then(|entry| entry.split("bg=").nth(1))
            .map(str::to_string)
            .unwrap_or_else(|| panic!("no legend for {letter:?}:\n{dump}"))
    };
    let letter = |column: usize| cells.chars().nth(column).expect("a style");
    let marked = ground(letter(column));
    for offset in 1..4 {
        assert_eq!(
            ground(letter(column + offset)),
            marked,
            "the mark is not one run:\n{dump}"
        );
    }
    assert_ne!(
        ground(letter(column + 4)),
        marked,
        "the mark runs past the characters the server named:\n{dump}"
    );
    assert_ne!(
        ground(letter(column.saturating_sub(1))),
        marked,
        "the mark starts before them:\n{dump}"
    );
}

/// Escape closes it, and every other key closes it *and* does what it was
/// going to do: a key that had to be pressed twice would be a key that
/// does nothing.
#[test]
fn escape_closes_it_and_everything_else_closes_it_on_the_way_past() {
    let (_scratch, mut app) = editing("hover-keys", SOURCE);
    at_the_word(&mut app);
    app.hover_for_test(answered(None));
    assert!(app.hover().is_some(), "the panel never opened");

    support::press(&mut app, KeyCode::Esc);
    assert!(app.hover().is_none(), "escape left it up");

    app.hover_for_test(answered(None));
    let before = app.current_buffer().expect("a buffer").cursor().line.get();
    support::press(&mut app, KeyCode::Down);
    assert!(app.hover().is_none(), "the arrow left it up");
    assert_eq!(
        app.current_buffer().expect("a buffer").cursor().line.get(),
        before + 1,
        "the arrow was swallowed by the panel"
    );
}

/// The paging keys read the rest of it, which is what the twelve-row cap
/// makes necessary.
#[test]
fn the_paging_keys_read_the_rest_of_it() {
    let (_scratch, mut app) = editing("hover-paging", SOURCE);
    at_the_word(&mut app);
    app.hover_for_test(answered(None));
    let first = support::render(&mut app, 76, 18);
    assert!(
        first.contains("Returns an iterator"),
        "the answer does not start at the top:\n{first}"
    );

    support::press(&mut app, KeyCode::PageDown);
    let paged = support::render(&mut app, 76, 18);
    assert!(app.hover().is_some(), "paging closed it");
    assert_ne!(
        support::text_block(&first),
        support::text_block(&paged),
        "the paging key moved nothing"
    );
    assert!(
        !paged.contains("Returns an iterator"),
        "it did not move past the first screenful:\n{paged}"
    );

    support::press(&mut app, KeyCode::PageUp);
    let back = support::render(&mut app, 76, 18);
    assert_eq!(
        support::text_block(&first),
        support::text_block(&back),
        "paging back did not come back"
    );
}

/// Nothing inside a panel touches its border, and the panel is on the
/// page's own colour.
///
/// A hover holds a README, and a README's fenced blocks are boxes of their
/// own -- so without the blank there were two lines side by side with
/// nothing between them, one Obelus's and one the document's.
///
/// The colour is the page's because the frame is what says a panel is not
/// the file, and in a window its ground is the colour of its glass: a
/// ground a shade off the page was a grey box over glass that is the
/// page's colour everywhere else.
///
/// Broken deliberately by setting `ui::PANEL_INSET` to 1, or by giving
/// `ui::panel` the ground `theme.raised_background`: the first puts the
/// fence against the side, the second puts the grey box back.
#[test]
fn a_panel_holds_its_contents_off_its_own_edge() {
    let (_scratch, mut app) = editing("hover-inset", SOURCE);
    at_the_word(&mut app);
    app.hover_for_test(answered(None));

    let dump = support::render(&mut app, 76, 18);
    let text = support::text_block(&dump);
    let rows: Vec<&str> = text.lines().collect();
    let at = rows
        .iter()
        .position(|row| row.contains(support::PANEL_CORNER))
        .unwrap_or_else(|| panic!("no box:\n{dump}"));

    // Every row of it, because what reaches the edge is whichever row is
    // widest and the test should not have to know which.
    for row in &rows[at + 1..] {
        let Some(side) = row.find('\u{2502}') else {
            break;
        };
        let after = row[side + '\u{2502}'.len_utf8()..]
            .chars()
            .next()
            .unwrap_or(' ');
        assert_eq!(after, ' ', "{after:?} is against the panel's side:\n{dump}");
    }

    // And the panel's own cells wear the page's colour -- asked of the
    // cells and not of the legend, because the legend has the page's
    // colour whatever the panel wears: the code round it is on it.
    let ground = support::spelled(app.theme().background);
    let letters: Vec<char> = support::legend_block(&dump)
        .lines()
        .filter(|entry| entry.contains(&format!("bg={ground}")))
        .filter_map(|entry| entry.chars().next())
        .collect();
    let styles: Vec<&str> = support::style_block(&dump).lines().collect();
    let corner = support::column_of(rows[at], &support::PANEL_CORNER.to_string());
    let under = styles[at + 1]
        .split_once('|')
        .map_or(styles[at + 1], |(_, rest)| rest)
        .chars()
        .nth(corner + 1)
        .expect("a cell inside the panel");
    assert!(
        letters.contains(&under),
        "the panel is not on the page's colour:\n{dump}"
    );
}

/// What is written on the first row inside the box, which is what
/// scrolling it changes.
///
/// Inside the box: past the row number the dump puts in front of every
/// row, past the code the box is drawn over, and past the box's own side
/// and the bar down its inner edge. A test that compared the whole row
/// would be comparing the gutter.
fn inside_the_top(dump: &str) -> String {
    let rows: Vec<&str> = support::text_block(dump).lines().collect();
    let at = rows
        .iter()
        .position(|row| row.contains(support::PANEL_CORNER))
        .unwrap_or_else(|| panic!("no box:\n{dump}"));
    let row = rows[at + 1];
    let from = row
        .find('\u{2502}')
        .unwrap_or_else(|| panic!("no side:\n{dump}"));
    let to = row.rfind('\u{2502}').unwrap_or(from);
    row[from..to]
        .trim_matches([' ', '\u{2502}', '\u{2588}'])
        .to_string()
}

/// Whether the box has anything written in it at all.
///
/// Inside its own sides: a row of the dump also holds the gutter and the
/// code the box is drawn over, so a test that looked at the whole row
/// would find the file and call it the answer.
fn says_something(dump: &str) -> bool {
    let rows: Vec<&str> = support::text_block(dump).lines().collect();
    let at = rows
        .iter()
        .position(|row| row.contains(support::PANEL_CORNER))
        .unwrap_or_else(|| panic!("no box:\n{dump}"));
    rows[at + 1..]
        .iter()
        .take_while(|row| !row.contains('\u{2514}'))
        .any(|row| {
            let Some(from) = row.find('\u{2502}') else {
                return false;
            };
            let to = row.rfind('\u{2502}').unwrap_or(from);
            row[from..to].chars().any(char::is_alphanumeric)
        })
}

/// An answer the pointer asked for belongs beside the word the pointer is
/// over, which is not where the caret is: a box drawn at the caret would
/// be a box about somewhere the reader is not looking.
#[test]
fn a_pointed_answer_is_drawn_beside_the_word_it_is_about() {
    use obelus_app::event::{Event, Pointer};

    // Wide, so that a box the width of the answer still fits to the right
    // of the word and is not slid along to fit: where it slides to is its
    // own rule, and this is about where it is hung from.
    let (_scratch, mut app) = editing("hover-pointed", SOURCE);
    support::lay_out(&mut app, 120, 18);
    let cells = |app: &mut App| -> Vec<String> {
        support::text_block(&support::render(app, 120, 18))
            .lines()
            .filter_map(|row| row.split_once('|').map(|(_, cells)| cells.to_string()))
            .collect()
    };
    // The caret stays at the top of the file; the pointer goes to a word
    // two lines down.
    let rows = cells(&mut app);
    let (y, x) = rows
        .iter()
        .enumerate()
        .find_map(|(y, row)| row.find("iter").map(|x| (y, x)))
        .expect("the word");
    app.handle(Event::Pointer {
        kind: Pointer::Moved,
        x: u16::try_from(x).expect("a column"),
        y: u16::try_from(y).expect("a row"),
    });
    app.hover_pointed_for_test(answered(None));
    assert!(app.hover().is_some(), "the answer was dropped");

    let rows = cells(&mut app);
    let corner = rows
        .iter()
        .enumerate()
        .find_map(|(at, row)| row.find(support::PANEL_CORNER).map(|x| (at, x)))
        .unwrap_or_else(|| panic!("no box: {rows:?}"));
    assert_eq!(
        corner,
        (y + 1, x),
        "the box is not hung off the word the pointer is over: {rows:?}"
    );
    // And the caret is where it was, at the top of the file: a box hung
    // off *that* would be on the second row.
    assert_eq!(
        app.current_buffer().expect("a buffer").cursor().line.get(),
        0
    );
    assert_ne!(corner.0, 1, "the box is hung off the caret: {rows:?}");
}

/// The wheel reads the rest of it. While the answer is up it is what the
/// reader is looking at, and the file behind it is not going anywhere.
#[test]
fn the_wheel_scrolls_the_answer_rather_than_the_file() {
    let (_scratch, mut app) = editing("hover-wheel", SOURCE);
    at_the_word(&mut app);
    app.hover_for_test(answered(None));
    let first = support::render(&mut app, 76, 18);
    let top = app.current_buffer().expect("a buffer").viewport().top.get();

    app.handle(obelus_app::event::Event::Scroll(3));
    let rolled = support::render(&mut app, 76, 18);
    assert_ne!(
        support::text_block(&first),
        support::text_block(&rolled),
        "the wheel moved nothing"
    );
    assert_ne!(
        inside_the_top(&rolled),
        inside_the_top(&first),
        "the answer did not move:\n{rolled}"
    );
    assert_eq!(
        app.current_buffer().expect("a buffer").viewport().top.get(),
        top,
        "the file behind it scrolled instead"
    );

    // Rolling on past the end stops at the end: an answer scrolled past
    // itself is an empty box, and the notches spent going past it are
    // notches to spend coming back.
    for _ in 0..10 {
        app.handle(obelus_app::event::Event::Scroll(3));
    }
    let far = support::render(&mut app, 76, 18);
    assert!(
        says_something(&far),
        "the answer was scrolled past itself:\n{far}"
    );

    // And back up, which stops at the top rather than running past it.
    for _ in 0..12 {
        app.handle(obelus_app::event::Event::Scroll(-3));
    }
    let back = support::render(&mut app, 76, 18);
    assert_eq!(
        support::text_block(&first),
        support::text_block(&back),
        "rolling back did not come back"
    );
}

/// An answer about a place the reader has left is dropped: a box appearing
/// over code somebody has moved on from is the same mistake as a late
/// completion.
#[test]
fn an_answer_about_somewhere_the_reader_has_left_is_dropped() {
    let (_scratch, mut app) = editing("hover-late", SOURCE);
    at_the_word(&mut app);
    let asked = app.current_buffer().expect("a buffer").cursor();
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Home);

    app.hover_late_for_test(answered(None), asked.line.get(), asked.column.get());
    assert!(
        app.hover().is_none(),
        "an answer about the line above opened a panel"
    );
}

/// And when the rest's clock goes off, the rest asks.
///
/// The other half of the one below: that a clock is started says nothing
/// about what its arrival does, and the two break apart. Nothing else in
/// the suite covers it -- every other assertion about a rest is that it has
/// *not* asked -- so `Event::PointerRested` doing nothing was a break that
/// left the whole suite green.
///
/// No frame between the rest and the clock, which is the point: a frame
/// would notice the dwell by itself, and what is being asked here is
/// whether anything causes one.
#[test]
fn the_rest_asks_when_its_clock_goes_off() {
    use obelus_app::event::{Event, Pointer};

    let (scratch, mut app) = editing("hover-asks", SOURCE);
    let (sender, _heard) = obelus_app::event::channel();
    app.events_for_test(sender);
    // Short enough to wait out in a test, long enough that the frame just
    // below the pointer's arrival is inside it.
    let settings = scratch.path().join("config.toml");
    std::fs::write(&settings, "hover_delay = 50\n").expect("writing the settings");
    app.config_file_for_test(settings);

    let dump = support::render(&mut app, 76, 18);
    let (y, x) = support::text_block(&dump)
        .lines()
        .filter_map(|row| row.split_once('|').map(|(_, cells)| cells.to_string()))
        .enumerate()
        .find_map(|(y, row)| row.find("iter").map(|x| (y, x)))
        .expect("a word to rest on");
    app.handle(Event::Pointer {
        kind: Pointer::Moved,
        x: u16::try_from(x).expect("a column"),
        y: u16::try_from(y).expect("a row"),
    });
    support::lay_out(&mut app, 76, 18);
    assert!(
        !app.rest_has_asked_for_test(),
        "the question was asked before the rest was long enough"
    );

    std::thread::sleep(std::time::Duration::from_millis(120));
    app.handle(Event::PointerRested);
    assert!(
        app.rest_has_asked_for_test(),
        "the rest's clock went off and nothing asked what was under it"
    );
}

/// Zero is the reader saying the pointer asks nothing: the key still
/// does, and nothing waits on a clock.
#[test]
fn a_rest_of_nothing_asks_nothing() {
    use obelus_app::event::{Event, Pointer};

    let (scratch, mut app) = editing("hover-never", SOURCE);
    // A clock needs the loop's channel, and the rest is timed by one.
    let (sender, _heard) = obelus_app::event::channel();
    app.events_for_test(sender);
    let dump = support::render(&mut app, 76, 18);
    let (y, x) = support::text_block(&dump)
        .lines()
        .filter_map(|row| row.split_once('|').map(|(_, cells)| cells.to_string()))
        .enumerate()
        .find_map(|(y, row)| row.find("iter").map(|x| (y, x)))
        .expect("a word to rest on");
    let point = |app: &mut App, x: usize, y: usize| {
        app.handle(Event::Pointer {
            kind: Pointer::Moved,
            x: u16::try_from(x).expect("a column"),
            y: u16::try_from(y).expect("a row"),
        });
        support::lay_out(app, 76, 18);
    };

    // A clock of the rest's own is what times it, which is the only thing
    // about a rest that can be seen without a server. It used to be the
    // animation asked to keep waking -- and over a network nothing is
    // animated, so the pointer could rest for ever and never ask.
    point(&mut app, x, y);
    assert!(
        app.rest_has_a_clock_for_test(),
        "nothing is waiting for the rest to be long enough"
    );
    assert!(
        !app.is_waking(),
        "the rest is still being timed by the animation"
    );
    assert!(
        !app.rest_has_asked_for_test(),
        "the question was asked before the rest was long enough"
    );

    // The reader's own file, saying the pointer asks nothing.
    let settings = scratch.path().join("config.toml");
    std::fs::write(&settings, "hover_delay = 0\n").expect("writing the settings");
    app.config_file_for_test(settings);
    point(&mut app, x + 1, y);
    assert!(
        !app.rest_has_a_clock_for_test(),
        "a rest of nothing is still being timed"
    );
    assert!(
        !app.rest_has_asked_for_test(),
        "a rest of nothing asked at once, which is the opposite of what it says"
    );
}

/// The shapes an answer arrives in, which the protocol has three of.
#[test]
fn the_shapes_an_answer_arrives_in() {
    use obelus_lsp::hover::in_reply;
    use obelus_text::Text;

    let text = Text::from_string("fn main() {}\n");
    let encoding = lsp_types::PositionEncodingKind::UTF16;
    let markdown = |value: serde_json::Value| {
        in_reply(&Ok(value), &text, &encoding).map(|hovered| hovered.markdown)
    };

    // The shape every server sends now.
    assert_eq!(
        markdown(json!({ "contents": { "kind": "markdown", "value": "**it**" } })).as_deref(),
        Some("**it**")
    );
    // The two the protocol keeps for compatibility: a bare string, and a
    // language with a string, which is a fenced block written out.
    assert_eq!(
        markdown(json!({ "contents": "plain" })).as_deref(),
        Some("plain")
    );
    assert_eq!(
        markdown(json!({ "contents": { "language": "rust", "value": "fn f()" } })).as_deref(),
        Some("```rust\nfn f()\n```")
    );
    // Nothing to say is no box: one that said nothing would cover code to
    // do it.
    assert_eq!(
        markdown(json!({ "contents": { "kind": "markdown", "value": "  " } })),
        None
    );
    assert_eq!(markdown(json!(null)), None);
}
