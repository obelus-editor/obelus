//! What a server says is wrong with a file.
//!
//! Against values rather than against a server: what is interesting is
//! what Obelus does with a notification -- where it draws it, what it
//! counts, what it lists -- and a server cannot be made to find a mistake
//! on demand.

mod support;

use crossterm::event::KeyCode;
use obelus_app::app::App;
use obelus_buffer::Buffer;
use serde_json::json;

fn editing(name: &str, contents: &str) -> (support::Scratch, App, std::path::PathBuf) {
    let scratch = support::Scratch::new(name);
    let path = scratch.path().join("sample.rs");
    std::fs::write(&path, contents).expect("writing the file");
    let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, 60, 16);
    (scratch, app, path)
}

/// One diagnostic over the given range, as a server sends them.
fn published(
    path: &std::path::Path,
    line: u32,
    from: u32,
    to: u32,
    severity: u8,
) -> serde_json::Value {
    json!({
        "uri": support::uri_for(path),
        "diagnostics": [{
            "range": { "start": { "line": line, "character": from },
                       "end": { "line": line, "character": to } },
            "severity": severity,
            "source": "rustc",
            "message": "cannot find value `nmae` in this scope\nhelp: a local variable with a similar name exists"
        }]
    })
}

/// Puts the caret on a place, which is where the box that says what is
/// wrong with it opens.
///
/// On the *span* and not on the line: the box is about the thing the
/// reader is standing on, so a test that pressed `Down` and stopped at
/// column zero is a test about a caret beside the word rather than on it.
fn stand_on(app: &mut App, line: usize, column: usize) {
    support::press_control_key(app, KeyCode::Home);
    for _ in 0..line {
        support::press(app, KeyCode::Down);
    }
    // `Home` stops at the first character that is not a blank, so where it
    // lands is the indent and the walk from there goes either way.
    support::press(app, KeyCode::Home);
    let at = |app: &App| {
        app.current_buffer()
            .map_or(0, |buffer| buffer.cursor().column.get())
    };
    for _ in 0..at(app).saturating_sub(column) {
        support::press(app, KeyCode::Left);
    }
    for _ in 0..column.saturating_sub(at(app)) {
        support::press(app, KeyCode::Right);
    }
    let landed = app
        .current_buffer()
        .map(|buffer| (buffer.cursor().line.get(), buffer.cursor().column.get()));
    assert_eq!(
        landed,
        Some((line, column)),
        "the caret would not stand where the trouble is"
    );
}

#[test]
fn what_is_wrong_is_underlined_where_it_is_wrong() {
    let (_scratch, mut app, path) = editing("trouble-underline", "fn main() {\n    nmae;\n}\n");
    app.publish_for_test(published(&path, 1, 4, 8, 1));

    // The cells rather than the dump: what says a character is underlined
    // is a modifier, and the dump names a style by its colours.
    let cells = support::cells_of(&mut app, 60, 16);
    let dump = support::render(&mut app, 60, 16);
    // Past the blank line the section starts with, so a row of the dump is
    // a row of the screen.
    let rows: Vec<&str> = support::text_block(&dump)
        .lines()
        .filter(|row| row.contains('|'))
        .collect();
    let at = rows
        .iter()
        .position(|row| row.contains("nmae"))
        .expect("the line");
    let row = rows[at];
    let text = &row[row.find('|').expect("a divider") + 1..];
    // Counted in characters, not in bytes: what comes before the text is a
    // half-block glyph three bytes wide, so a byte offset is two columns
    // out the moment the margin has anything in it.
    let before = text.find("nmae").expect("the word");
    let column = u16::try_from(text[..before].chars().count()).expect("a column");
    let y = u16::try_from(at).expect("a row");
    let underlined = |x: u16| {
        cells
            .cell((x, y))
            .expect("a cell")
            .modifier
            .contains(ratatui::style::Modifier::UNDERLINED)
    };

    for offset in 0..4 {
        assert!(
            underlined(column + offset),
            "the character at {offset} of what the server named is not underlined:\n{dump}"
        );
    }
    assert!(
        !underlined(column + 4),
        "the underline runs past what the server named:\n{dump}"
    );
    assert!(
        !underlined(column.saturating_sub(1)),
        "the underline starts before what the server named:\n{dump}"
    );
    // And in the colour that kind of trouble is written in, not the plain
    // one: a warning and an error have to be told apart without reading
    // them.
    assert!(
        cells
            .cell((column, y))
            .expect("a cell")
            .underline_color
            .ne(&ratatui::style::Color::Reset),
        "the underline has no colour of its own:\n{dump}"
    );
}

/// A count on the status row, because a reader who has not opened the list
/// still has to know there is one.
#[test]
fn the_status_row_says_how_many() {
    let (_scratch, mut app, path) = editing("trouble-count", "fn main() {\n    nmae;\n}\n");
    app.publish_for_test(published(&path, 1, 4, 8, 1));
    let dump = support::render(&mut app, 60, 16);
    let status = support::text_block(&dump)
        .lines()
        .next_back()
        .expect("the status row");
    assert!(
        status.contains('\u{00d7}') && status.contains('1'),
        "the count is not on the status row: {status:?}"
    );

    // And a clean file says nothing at all.
    app.publish_for_test(json!({
        "uri": support::uri_for(path),
        "diagnostics": []
    }));
    let dump = support::render(&mut app, 60, 16);
    let status = support::text_block(&dump)
        .lines()
        .next_back()
        .expect("the status row");
    assert!(
        !status.contains('\u{00d7}'),
        "a file the server called clean still says something is wrong: {status:?}"
    );
}

#[test]
fn the_list_names_what_is_wrong_and_goes_there() {
    let (_scratch, mut app, path) = editing("trouble-list", "fn main() {\n    nmae;\n}\n");
    app.publish_for_test(published(&path, 1, 4, 8, 1));
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::SymbolTroubles);

    let dump = support::render(&mut app, 60, 16);
    assert!(
        dump.contains("cannot find value"),
        "the message is not in the list:\n{dump}"
    );
    // The first line of it, not the whole paragraph. Asked of the row
    // rather than of the screen: a row too long for the list is cut, and a
    // paragraph that went in whole would be cut to look the same.
    let label = app
        .picker()
        .expect("the list")
        .matches()
        .next()
        .expect("a row")
        .label
        .clone();
    assert!(
        !label.contains("a local variable"),
        "the whole paragraph went into one row: {label:?}"
    );

    support::press(&mut app, crossterm::event::KeyCode::Enter);
    let cursor = app.current_buffer().expect("a buffer").cursor();
    assert_eq!(
        (cursor.line.get(), cursor.column.get()),
        (1, 4),
        "choosing a row did not go to what it names"
    );
}

/// A server is entitled to talk about files that are not open -- after a
/// `cargo check`, rust-analyzer talks about the whole project -- and what
/// it says about them is the answer to "where is this project broken".
///
/// Nothing of it is placed in the file being read: a range becomes a place
/// by being counted against the text it is in, and this is somebody else's
/// text. So the file's own list, its underlines and its count stay exactly
/// as empty as they were.
///
/// Broken deliberately by dropping the notification in `on_published` the
/// way it used to be dropped: the project tab goes empty and says nobody
/// has said anything.
#[test]
fn a_notification_about_a_file_that_is_not_open_is_kept_for_the_project() {
    let (_scratch, mut app, path) = editing("trouble-elsewhere", "fn main() {}\n");
    let elsewhere = path.with_file_name("other.rs");
    std::fs::write(&elsewhere, "fn other() {\n    nmae;\n}\n").expect("writing the other file");
    app.publish_for_test(published(&elsewhere, 1, 4, 8, 1));
    assert!(
        app.troubles().is_empty(),
        "something was placed in a file it is not about"
    );

    // This file is clean, so the list opens on the radius that has
    // something to say rather than on an empty tab.
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::SymbolTroubles);
    let dump = support::render(&mut app, 70, 16);
    assert!(
        dump.contains("cannot find value"),
        "what is wrong elsewhere is not in the list:\n{dump}"
    );
    let row = app
        .picker()
        .expect("the list")
        .matches()
        .next()
        .expect("a row")
        .clone();
    assert_eq!(
        row.trailing.as_deref(),
        Some("other.rs:2"),
        "the row does not say which file it is in"
    );

    // And choosing it goes there, which is a file that was not open.
    support::press(&mut app, crossterm::event::KeyCode::Enter);
    let buffer = app.current_buffer().expect("a buffer");
    assert_eq!(buffer.path(), elsewhere, "choosing the row opened nothing");
    assert_eq!(
        (buffer.cursor().line.get(), buffer.cursor().column.get()),
        (1, 4),
        "choosing the row did not go to what it names"
    );
}

/// One list at two radii: the file being read, and everything anybody has
/// said about the project around it.
///
/// Walked with `tab`, which is what walks the tabs everywhere else. The
/// rows are two different sets read from two different places, so this is
/// also the test that walking onto a tab is what asks its question.
///
/// Broken deliberately by leaving `refresh_troubles` out of the tab change
/// in `picker_key`: the file's rows stay on screen under the project's tab.
#[test]
fn the_list_of_problems_is_the_file_and_the_project() {
    let (_scratch, mut app, path) = editing("trouble-radii", "fn main() {\n    nmae;\n}\n");
    let elsewhere = path.with_file_name("other.rs");
    std::fs::write(&elsewhere, "fn other() {\n    oops;\n}\n").expect("writing the other file");
    app.publish_for_test(published(&path, 1, 4, 8, 1));
    app.publish_for_test(json!({
        "uri": support::uri_for(&elsewhere),
        "diagnostics": [{
            "range": { "start": { "line": 1, "character": 4 },
                       "end": { "line": 1, "character": 8 } },
            "severity": 1,
            "source": "rustc",
            "message": "cannot find value `oops` in this scope"
        }]
    }));

    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::SymbolTroubles);
    let dump = support::render(&mut app, 70, 16);
    assert!(
        !dump.contains("oops"),
        "the file's own list has somebody else's problem in it:\n{dump}"
    );
    let rows = |app: &App| app.picker().expect("the list").matches().count();
    assert_eq!(rows(&app), 1, "the file's list is not the file's problems");

    support::press(&mut app, crossterm::event::KeyCode::Tab);
    let dump = support::render(&mut app, 70, 16);
    assert!(
        dump.contains("oops"),
        "walking onto the project tab did not ask the project:\n{dump}"
    );
    assert_eq!(
        rows(&app),
        2,
        "the project's list is not both files:\n{dump}"
    );
    // Opened on the reader's own file, which sorts second: a list of the
    // whole project asked from here starts here.
    let chosen = app
        .picker()
        .expect("the list")
        .selected_item()
        .expect("a row")
        .trailing
        .clone();
    assert_eq!(
        chosen.as_deref(),
        Some("sample.rs:2"),
        "the project's list opened in somebody else's file:\n{dump}"
    );

    // Back again, and the file's own list is the file's own again.
    support::press(&mut app, crossterm::event::KeyCode::BackTab);
    let dump = support::render(&mut app, 70, 16);
    assert!(
        !dump.contains("oops"),
        "walking back left the project's rows under the file's tab:\n{dump}"
    );
}

/// A file with nothing wrong with it and nothing said about anything else
/// is not an empty list: it is a sentence saying which of those it is.
#[test]
fn nothing_wrong_anywhere_is_said_rather_than_listed() {
    let (_scratch, mut app, _path) = editing("trouble-nothing", "fn main() {}\n");
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::SymbolTroubles);
    assert!(
        app.picker().is_none(),
        "an empty list was opened instead of a sentence"
    );
    let dump = support::render(&mut app, 70, 16);
    assert!(
        dump.contains("No language server for this file"),
        "the sentence does not say why the list is empty:\n{dump}"
    );
}

/// What a severity is, and what a uri says, which are the two things the
/// notification is made of.
#[test]
fn the_shapes_a_notification_arrives_in() {
    use obelus_lsp::trouble::{Severity, path_of};

    // A uri with an escape in it, which is what a path with a space comes
    // back as.
    assert_eq!(
        path_of(&json!({ "uri": support::fake_uri("/tmp/a b/one.rs") })),
        Some(support::fake_path("/tmp/a b/one.rs"))
    );
    assert_eq!(path_of(&json!({ "uri": "untitled:nowhere" })), None);

    // A server that says nothing about how bad it is has said the thing
    // that gets looked at.
    assert!(
        Severity::Error < Severity::Warning,
        "the order is the point"
    );
    assert_eq!(Severity::Error.title(), "Error");
}

/// What is wrong with the line the caret is on is opened under the reader's
/// eye, in the words the server used.
///
/// The underline says *that* something is wrong and costs no room; this
/// says *what*, and covers the code under the line. Which is why it is
/// only ever where the caret is standing: a file with thirty of these is
/// a file whose shape is the complaints rather than the code.
///
/// On the *span* and not on the line. A line holds a great deal and the
/// server said this about one word of it, so a caret at the start of that
/// line is a caret beside the word rather than on it -- and a box that
/// opened there would open on every line a reader walked down through.
///
/// Broken deliberately two ways. Leaving `show_what_is_wrong` uncalled
/// puts the words nowhere on the page. And asking `what_is_wrong` for the
/// whole line -- `span.line == line` rather than `span.contains` -- opens
/// it with the caret at the line's own start, which is the third
/// assertion here.
#[test]
fn what_is_wrong_with_this_line_is_opened_under_it() {
    let (_scratch, mut app, path) = editing("trouble-block", "fn main() {\n    nmae;\n}\n");
    app.publish_for_test(published(&path, 1, 4, 8, 1));

    // The caret starts on the first line, which has nothing wrong with it.
    let dump = support::render(&mut app, 60, 16);
    assert!(
        !support::text_block(&dump).contains("cannot find value"),
        "a complaint about a line the caret is not on:\n{dump}"
    );

    // Onto the line that has, but at its start rather than on the word:
    // the line is not enough, because the box is about the thing the
    // caret is standing on.
    stand_on(&mut app, 1, 0);
    let dump = support::render(&mut app, 60, 16);
    assert!(
        !support::text_block(&dump).contains("cannot find value"),
        "a complaint about a word the caret is beside rather than on:\n{dump}"
    );

    // And onto the word itself.
    stand_on(&mut app, 1, 4);
    let dump = support::render(&mut app, 60, 16);
    assert!(
        support::text_block(&dump).contains("cannot find value"),
        "the words the server used are not under the line:\n{dump}"
    );

    // And away again -- one key, because there is nothing in the file to
    // step over: the words are a box floated over the page, not rows.
    support::press(&mut app, crossterm::event::KeyCode::Down);
    assert_eq!(
        app.current_buffer()
            .map(|buffer| buffer.cursor().line.get()),
        Some(2),
        "one key did not step past the complaint"
    );
    let dump = support::render(&mut app, 60, 16);
    assert!(
        !support::text_block(&dump).contains("cannot find value"),
        "the complaint stayed on a line the caret has left:\n{dump}"
    );
}

/// A complaint about the file's last line opens over it instead.
///
/// Under the line is where a complaint belongs, and the last line of a
/// file scrolled to the bottom of the screen has nothing under it -- so
/// the box goes above it, which is the other half of the same rule: a
/// complaint nobody can see belongs nowhere. It is what a hover does when
/// it runs out of room, and the reason this one does not simply prefer
/// above is that a complaint is about the line it hangs off.
///
/// Reached by walking there rather than by starting there, in a file
/// taller than the screen, because the room below is only nothing once
/// the view has had to scroll.
///
/// Broken deliberately by hanging it under the line whatever the room --
/// `if below >= height` made unconditional -- which draws it past the
/// bottom of the region, where it is clipped away and the words are
/// nowhere on the page at all.
#[test]
fn what_is_wrong_with_the_last_line_opens_over_it_instead() {
    // Long enough that the last line is only reached by scrolling, and no
    // newline at the end so that line really is the last one: with a
    // trailing newline there is an empty line after it, which has a row
    // under it and is the ordinary case rather than this one.
    let mut file = "fn main() {\n".to_string();
    for _ in 0..40 {
        file.push_str("    let _ = 1;\n");
    }
    file.push_str("    nmae");
    let (_scratch, mut app, path) = editing("trouble-last", &file);
    app.publish_for_test(published(&path, 41, 4, 8, 1));
    stand_on(&mut app, 41, 4);

    let dump = support::render(&mut app, 60, 16);
    let rows: Vec<&str> = support::text_block(&dump)
        .lines()
        .filter(|row| row.contains('|'))
        .collect();
    let said = rows
        .iter()
        .position(|row| row.contains("cannot find value"))
        .unwrap_or_else(|| panic!("the words are not on the page:\n{dump}"));
    // From the bottom: what the server said has the word in it too, and
    // the row being looked for is the file's.
    let about = rows
        .iter()
        .rposition(|row| row.contains("nmae"))
        .expect("the line it is about");
    assert!(
        said < about,
        "the complaint about the last line opened where there was no room:\n{dump}"
    );
}

/// Walking from one problem to the next, without reading the list.
///
/// The list is the map and this is the walk: a reader working through what
/// a server said wants the caret on the line, the complaint under it and
/// the file still around it, and going back to a list between every two is
/// the thing that makes a reader stop after the first.
///
/// No wrapping, like the changes: a reader who steps past the last one and
/// lands back at the top has lost their place to a key that looked like it
/// did nothing.
///
/// Broken deliberately by taking the direction out of `trouble_from` -- by
/// always looking forward, or by letting it wrap -- and either way the
/// walk ends up somewhere this names.
#[test]
fn the_caret_walks_from_one_problem_to_the_next() {
    let mut file = String::new();
    for _ in 0..8 {
        file.push_str("    let _ = 1;\n");
    }
    let (_scratch, mut app, path) = editing("trouble-walk", &file);
    let one = |line: u32| {
        json!({
            "range": { "start": { "line": line, "character": 4 },
                       "end": { "line": line, "character": 8 } },
            "severity": 1,
            "source": "rustc",
            "message": "cannot find value `nmae` in this scope"
        })
    };
    app.publish_for_test(json!({
        "uri": support::uri_for(path),
        "diagnostics": [one(2), one(5)]
    }));

    let at = |app: &App| {
        app.current_buffer()
            .map(|buffer| buffer.cursor().line.get())
            .expect("a file")
    };
    assert_eq!(at(&app), 0, "the caret did not start at the top");

    support::press_alt(&mut app, ']');
    assert_eq!(at(&app), 2, "forward did not reach the first problem");
    support::press_alt(&mut app, ']');
    assert_eq!(at(&app), 5, "forward did not reach the second");
    support::press_alt(&mut app, ']');
    assert_eq!(at(&app), 5, "forward wrapped round past the last problem");

    support::press_alt(&mut app, '[');
    assert_eq!(at(&app), 2, "back did not reach the first problem");
    support::press_alt(&mut app, '[');
    assert_eq!(at(&app), 2, "back wrapped round past the first problem");

    // A leap across the file, so `go-back` comes back from it: the same
    // promise typing a line number makes.
    support::press_alt_key(&mut app, crossterm::event::KeyCode::Left);
    assert_eq!(
        at(&app),
        5,
        "the walk left nothing for go-back to return to"
    );
}

/// The words are framed under the word they are about, and the box says
/// how many more there are.
///
/// A complaint is prose about a place, and prose drawn in the plain
/// colour at the code's own left edge reads as a line of the file written
/// in English. The frame says it is not the file; where it starts says
/// which word it is about. Under the line and not on it: the line is what
/// the reader is looking at.
///
/// The frame is the panel's -- the one the completion list and a hover
/// wear -- so the count of the others on the line is a row *inside* it,
/// after the server's own words. It rode the bottom rail while Obelus drew
/// that frame itself, and a panel's rail is not Obelus's to write on.
///
/// Wide enough that the box is narrower than the room: at sixty columns a
/// sixty-four column box is the whole row, and a box that starts at the
/// left edge because there is nowhere else says nothing about the indent.
///
/// Broken deliberately three ways, one assertion each: hanging the box on
/// `anchor_y` rather than the row under it covers the line it is about;
/// putting it at `editor.x` drops the indent; and leaving the `others` row
/// out of `rows` takes the count away.
#[test]
fn a_complaint_is_framed_under_the_word_and_says_how_many_more() {
    let (_scratch, mut app, path) = editing("trouble-frame", "fn main() {\n    nmae;\n}\n");
    // Two of them on the one line, so there is something left to count.
    let one = |from: u32, to: u32, severity: u8, message: &str| {
        json!({
            "range": { "start": { "line": 1, "character": from },
                       "end": { "line": 1, "character": to } },
            "severity": severity,
            "source": "rustc",
            "message": message
        })
    };
    app.publish_for_test(json!({
        "uri": support::uri_for(path),
        "diagnostics": [one(4, 8, 1, "cannot find value `nmae` in this scope"),
                        one(4, 8, 2, "unused something")]
    }));
    stand_on(&mut app, 1, 4);

    let dump = support::render(&mut app, 100, 16);
    let rows: Vec<&str> = support::text_block(&dump)
        .lines()
        .filter(|row| row.contains('|'))
        .map(|row| &row[row.find('|').expect("a divider") + 1..])
        .collect();
    let column = |row: &str, of: &str| {
        row.find(of)
            .map(|byte| row[..byte].chars().count())
            .unwrap_or_else(|| panic!("{of:?} is not on {row:?}:\n{dump}"))
    };

    // The semicolon, because the word is in what the server said as well.
    let about = rows
        .iter()
        .position(|row| row.contains("nmae;"))
        .expect("the line it is about");
    let top = rows
        .iter()
        .position(|row| row.contains('\u{256d}'))
        .unwrap_or_else(|| panic!("the words are not framed:\n{dump}"));
    let bottom = rows
        .iter()
        .position(|row| row.contains('\u{2570}'))
        .unwrap_or_else(|| panic!("the frame has no bottom rail:\n{dump}"));
    let said = rows
        .iter()
        .position(|row| row.contains("cannot find value"))
        .expect("the words");
    assert_eq!(
        top,
        about + 1,
        "the box is not on the row under the line it is about:\n{dump}"
    );
    assert!(
        said > top && bottom > said,
        "the frame is not wrapped round the words:\n{dump}"
    );
    assert!(
        rows[said].contains('\u{2502}'),
        "the row the words are on has no rails:\n{dump}"
    );
    // Under the word it is about, not at the code's own left edge.
    assert_eq!(
        column(rows[top], "\u{256d}"),
        column(rows[about], "nmae;"),
        "the frame does not start under the word it is about:\n{dump}"
    );
    // Inside the frame and after what the server said, which is where
    // Obelus's own row goes now that the frame is not Obelus's.
    let count = rows
        .iter()
        .position(|row| row.contains("and 1 more here"))
        .unwrap_or_else(|| panic!("the box does not say how many more there are:\n{dump}"));
    assert!(
        count > said && count < bottom,
        "the count is not a row inside the frame:\n{dump}"
    );
}

/// A box with less room to its right than it is wide slides left rather
/// than running off the page.
///
/// It hangs off the word it is about, and a word near the right-hand edge
/// has nowhere to hang from -- so the box slides left until it fits.
/// Half a frame says what these rows are worse than no frame at all, and
/// the frame is the whole of what says they are not the file.
///
/// At three widths, because the arithmetic has three answers: one where
/// the box fits where the word is, one where it has to slide, and one
/// where the room itself is narrower than the box wants and the box is
/// the room.
///
/// Broken deliberately by taking the clamp off `x` in `where_it_goes`:
/// the frame starts under the word at every width, and at two of them its
/// right-hand rail is off the page.
#[test]
fn a_box_with_no_room_to_its_right_slides_left() {
    // The word a long way in, so that where the box would like to start
    // is further right than a box's width from the edge.
    let indented = format!("{}step_99;\n", " ".repeat(30));
    let rows_at = |width: u16| {
        let scratch = support::Scratch::new(&format!("trouble-sliding-{width}"));
        let path = scratch.path().join("sample.rs");
        std::fs::write(&path, format!("fn main() {{\n{indented}}}\n")).expect("writing");
        let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
        app.working_directory_for_test(scratch.path().to_path_buf());
        support::lay_out(&mut app, width, 24);
        app.publish_for_test(json!({
            "uri": support::uri_for(path),
            "diagnostics": [{
                "range": { "start": { "line": 1, "character": 30 },
                           "end": { "line": 1, "character": 37 } },
                "severity": 1,
                "source": "rustc",
                "message": "cannot find function `step_99` in this scope"
            }]
        }));
        stand_on(&mut app, 1, 30);
        let dump = support::render(&mut app, width, 24);
        let rows: Vec<String> = support::text_block(&dump)
            .lines()
            .filter(|row| row.contains('|'))
            .map(|row| row[row.find('|').expect("a divider") + 1..].to_string())
            .collect();
        (rows, dump)
    };

    // Whole at every width, which is three facts: one top rail with both
    // its corners, one bottom rail with both of its, and a pair of side
    // rails on every row of words.
    let whole = |rows: &[String], dump: &str, width: u16| {
        for (corner, opposite, which) in [
            ('\u{256d}', '\u{256e}', "top"),
            ('\u{2570}', '\u{256f}', "bottom"),
        ] {
            let found: Vec<&String> = rows.iter().filter(|row| row.contains(corner)).collect();
            assert_eq!(
                found.len(),
                1,
                "at {width} the {which} rail is on {} rows:\n{dump}",
                found.len()
            );
            assert!(
                found[0].contains(opposite),
                "at {width} the {which} rail ran off the row it started on:\n{dump}"
            );
        }
        let words: Vec<&String> = rows
            .iter()
            .filter(|row| row.contains("cannot find") || row.contains("this scope"))
            .collect();
        assert!(
            !words.is_empty(),
            "at {width} the words are nowhere:\n{dump}"
        );
        for row in words {
            assert_eq!(
                row.matches('\u{2502}').count(),
                2,
                "at {width} a row of words is missing a rail:\n{dump}"
            );
        }
    };
    // Where the frame's left-hand rail starts, in characters.
    let starts = |rows: &[String]| {
        let row = rows
            .iter()
            .find(|row| row.contains('\u{256d}'))
            .expect("a top rail");
        let byte = row.find('\u{256d}').expect("the corner");
        row[..byte].chars().count()
    };
    // And where the word it is about is.
    let word = |rows: &[String]| {
        let row = rows
            .iter()
            .find(|row| row.contains("step_99;"))
            .expect("the line it is about");
        let byte = row.find("step_99;").expect("the word");
        row[..byte].chars().count()
    };

    let (roomy, dump) = rows_at(120);
    whole(&roomy, &dump, 120);
    assert_eq!(
        starts(&roomy),
        word(&roomy),
        "there was room and the box did not hang off the word:\n{dump}"
    );

    let (slid, dump) = rows_at(80);
    whole(&slid, &dump, 80);
    assert!(
        starts(&slid) < word(&slid),
        "the box stayed under the word with no room for it:\n{dump}"
    );

    // Narrower than the box would like to be, so it is the room -- and
    // starts at the region's own edge, gutter and all.
    let (tight, dump) = rows_at(40);
    whole(&tight, &dump, 40);
    assert_eq!(
        starts(&tight),
        0,
        "the box is narrower than the room it was given:\n{dump}"
    );
}

/// The list of problems shows each one in the file, and puts the view back
/// if the reader leaves without choosing.
///
/// It sits on the status bar rather than over the file, which is the whole
/// reason it is drawn that way: the code stays visible, so the file itself
/// is what a selection is shown in. A full-area list has a preview of its
/// own and this question does not arise for it.
///
/// A look and not a move. The caret stays where the reader left it, so
/// escaping is free -- and that is what makes walking a list of problems
/// something a reader will actually do.
///
/// It opens on the one nearest the caret. A reader asks this about where
/// they are, and a list that always started at line one would scroll the
/// file away from them before they had touched a key.
///
/// Broken deliberately by selecting the first row instead of the nearest,
/// by leaving the viewport alone while the selection moves, or by not
/// putting it back on the way out: each is a different assertion here.
#[test]
fn the_list_of_problems_shows_each_one_and_comes_back() {
    let mut file = String::new();
    for line in 0..120 {
        file.push_str(&format!("    let _ = {line};\n"));
    }
    let (_scratch, mut app, path) = editing("trouble-walk-list", &file);
    let one = |line: u32| {
        json!({
            "range": { "start": { "line": line, "character": 4 },
                       "end": { "line": line, "character": 8 } },
            "severity": 1,
            "source": "rustc",
            "message": "cannot find value"
        })
    };
    app.publish_for_test(json!({
        "uri": support::uri_for(path),
        "diagnostics": [one(5), one(100)]
    }));
    // Down near the second one, so the nearest is not the first.
    for _ in 0..90 {
        support::press(&mut app, crossterm::event::KeyCode::Down);
    }
    support::render(&mut app, 60, 20);
    let at = |app: &App| app.current_buffer().expect("a file").cursor().line.get();
    let top = |app: &App| app.current_buffer().expect("a file").viewport().top.get();
    let (was, looking) = (at(&app), top(&app));
    assert_eq!(was, 90, "the caret did not get to where this starts");

    // Opened on the nearest, which is the one below rather than the one at
    // the top of the file.
    support::press_alt(&mut app, 'e');
    let dump = support::render(&mut app, 60, 20);
    assert!(
        top(&app) > looking,
        "the list did not show the problem nearest the caret:\n{dump}"
    );
    assert_eq!(at(&app), was, "showing it moved the caret:\n{dump}");
    let shown = top(&app);

    // And walking the rows shows each one in turn.
    support::press(&mut app, crossterm::event::KeyCode::Up);
    let dump = support::render(&mut app, 60, 20);
    assert!(
        top(&app) < shown,
        "walking the list did not show the other problem:\n{dump}"
    );
    assert_eq!(at(&app), was, "walking the list moved the caret:\n{dump}");

    // Leaving without choosing puts the view back exactly, because the
    // reader never went anywhere.
    support::press(&mut app, crossterm::event::KeyCode::Esc);
    let dump = support::render(&mut app, 60, 20);
    assert_eq!(
        top(&app),
        looking,
        "escaping left the reader somewhere they did not choose to be:\n{dump}"
    );
    assert_eq!(at(&app), was, "escaping moved the caret:\n{dump}");

    // Choosing one is what actually goes there.
    support::press_alt(&mut app, 'e');
    support::render(&mut app, 60, 20);
    support::press(&mut app, crossterm::event::KeyCode::Enter);
    let dump = support::render(&mut app, 60, 20);
    assert_eq!(
        at(&app),
        100,
        "choosing the nearest problem did not go to it:\n{dump}"
    );

    // And the place they were looking from is forgotten when they choose,
    // not kept: escaping out of the *next* list must put them back here,
    // not at a place they left on purpose two lists ago.
    let chosen = top(&app);
    support::press_alt(&mut app, 'e');
    support::render(&mut app, 60, 20);
    support::press(&mut app, crossterm::event::KeyCode::Esc);
    let dump = support::render(&mut app, 60, 20);
    assert_eq!(
        top(&app),
        chosen,
        "escaping a later list went back to where an earlier one started:\n{dump}"
    );
}

/// The list is in the order the problems are in the file, whatever order
/// they arrived in.
///
/// A server reports what it found in the order it found it: rustc walks
/// its own passes, rust-analyzer forwards that, and a file whose errors
/// come out at lines 100, 10, 40 is an ordinary file rather than a strange
/// one. Listed that way the rows are a bag, and the row nearest the caret
/// is somewhere in the middle of it -- which is what "why did it pick that
/// one" looks like from the outside.
///
/// Broken deliberately by taking the sort out of `on_published`: the rows
/// come back in the order they were sent and the nearest is no longer the
/// first.
#[test]
fn the_problems_are_listed_in_the_order_they_are_in_the_file() {
    let mut file = String::new();
    for line in 0..120 {
        file.push_str(&format!("    let _ = {line};\n"));
    }
    let (_scratch, mut app, path) = editing("trouble-order", &file);
    let one = |line: u32| {
        json!({
            "range": { "start": { "line": line, "character": 4 },
                       "end": { "line": line, "character": 8 } },
            "severity": 1,
            "source": "rustc",
            "message": format!("wrong at {line}")
        })
    };
    // Out of order, the way they arrive.
    app.publish_for_test(json!({
        "uri": support::uri_for(path),
        "diagnostics": [one(100), one(10), one(40)]
    }));
    support::press_alt(&mut app, 'e');
    let dump = support::render(&mut app, 60, 20);
    let rows: Vec<&str> = support::text_block(&dump)
        .lines()
        // The list's own rows, which are the ones naming the tool that
        // said it: the complaint framed under the selection quotes the
        // same words, and it is not a row of the list.
        .filter(|row| row.contains("wrong at ") && row.contains("rustc"))
        .collect();
    let said: Vec<&str> = rows
        .iter()
        .filter_map(|row| row.split("wrong at ").nth(1))
        .map(|rest| rest.split_whitespace().next().unwrap_or(""))
        .collect();
    assert_eq!(
        said,
        vec!["10", "40", "100"],
        "the rows are in the order they arrived, not the order they are in:\n{dump}"
    );

    // And the caret is at the top, so the nearest is the first row: which
    // is a thing worth asserting only because the order makes it true.
    let picker = app.picker().expect("the list");
    assert_eq!(
        picker.selected(),
        0,
        "the list did not open on the problem nearest the caret:\n{dump}"
    );
}

/// A place the reader can already see does not move the view, and a place
/// hidden behind the list is not one of them.
///
/// The same rule `go-to-next-change` follows: a short hop is not worth
/// throwing away where they were looking. It matters more here, because
/// the list shows its selection in the file itself -- so a reader standing
/// at the top of a file, opening the list on a problem ten lines down,
/// would have watched it scroll away for nothing.
///
/// The room is what is *visible*, not what the editor draws: the list is
/// drawn over the foot of the editor, so a line under it is a line the
/// reader cannot see. Measured against the whole height, a problem behind
/// the list reads as one the reader can see and the view stays put --
/// leaving them looking at a list whose selection is nowhere on screen.
///
/// Broken deliberately by looking at the selection unconditionally, which
/// moves the view in the first case, or by measuring against the editor's
/// whole height, which fails to move it in the second. Both need a line
/// far enough down that centring on it is not clamped back to the top,
/// which is why neither of these is three lines in.
#[test]
fn the_view_moves_only_for_a_problem_the_reader_cannot_see() {
    let opened_on = |name: &str, line: u32| {
        let mut file = String::new();
        for at in 0..120 {
            file.push_str(&format!("    let _ = {at};\n"));
        }
        let (scratch, mut app, path) = editing(name, &file);
        app.publish_for_test(json!({
            "uri": support::uri_for(path),
            "diagnostics": [{
                "range": { "start": { "line": line, "character": 4 },
                           "end": { "line": line, "character": 8 } },
                "severity": 1,
                "source": "rustc",
                "message": "wrong"
            }]
        }));
        support::render(&mut app, 60, 20);
        let was = app.current_buffer().expect("a file").viewport().top.get();
        support::press_alt(&mut app, 'e');
        let dump = support::render(&mut app, 60, 20);
        let now = app.current_buffer().expect("a file").viewport().top.get();
        (scratch, was, now, dump, app.text_area().height)
    };

    // Well inside the room the list leaves, and far enough down that
    // centring on it would move the view if anything asked it to.
    let (_scratch, was, now, dump, room) = opened_on("trouble-still", 6);
    assert_eq!(
        was, now,
        "the view moved for a problem the reader could already see:\n{dump}"
    );

    // Past the room the list leaves: drawn by the editor before this, and
    // covered, so the reader could not see it however many rows the editor
    // thought it had.
    let (_scratch, was, now, dump, _) = opened_on("trouble-hidden", u32::from(room) + 3);
    assert_ne!(
        was, now,
        "the view stayed put for a problem hidden behind the list:\n{dump}"
    );

    // And on the very last row the reader has, which is on the screen and
    // not somewhere they can read: the list is against it and there is no
    // file under it.
    let (_scratch, was, now, dump, _) = opened_on("trouble-edge", u32::from(room) - 1);
    assert_ne!(
        was, now,
        "the view left the problem pinned against the list:\n{dump}"
    );
}

/// The complaint follows the list's selection, not the caret.
///
/// The row in the list and the place in the file are the same thing, and
/// this is what says so: walking the rows scrolls the file to each one and
/// opens the server's words under it. Without it a reader has to pair a
/// row with a line number by eye, which is the work the list was meant to
/// save them.
///
/// It follows the selection back, too: leaving without choosing puts the
/// complaint where the caret is, because that is where the reader is again.
///
/// Broken deliberately by asking the caret for the line the complaint is
/// about, which is what it asked before.
#[test]
fn the_complaint_follows_the_list_rather_than_the_caret() {
    let mut file = String::new();
    for line in 0..120 {
        file.push_str(&format!("    let _ = {line};\n"));
    }
    let (_scratch, mut app, path) = editing("trouble-follow", &file);
    let one = |line: u32| {
        json!({
            "range": { "start": { "line": line, "character": 4 },
                       "end": { "line": line, "character": 8 } },
            "severity": 1,
            "source": "rustc",
            "message": format!("wrong at {line}")
        })
    };
    app.publish_for_test(json!({
        "uri": support::uri_for(path),
        "diagnostics": [one(4), one(60)]
    }));
    // On the first of them, so the caret has a complaint of its own to be
    // told apart from the list's.
    stand_on(&mut app, 4, 4);
    let dump = support::render(&mut app, 60, 20);
    let says = |dump: &str, what: &str| {
        support::text_block(dump)
            .lines()
            .any(|row| row.contains(what) && !row.contains("rustc"))
    };
    assert!(
        says(&dump, "wrong at 4"),
        "the caret's own line has no complaint under it:\n{dump}"
    );

    // Down the list, and the complaint goes with it.
    support::press_alt(&mut app, 'e');
    support::render(&mut app, 60, 20);
    support::press(&mut app, crossterm::event::KeyCode::Down);
    let dump = support::render(&mut app, 60, 20);
    assert!(
        says(&dump, "wrong at 60"),
        "the complaint did not follow the selection:\n{dump}"
    );
    assert!(
        !says(&dump, "wrong at 4"),
        "the caret's complaint stayed open under a line nobody is looking at:\n{dump}"
    );

    // And back to the caret's own when the list goes.
    support::press(&mut app, crossterm::event::KeyCode::Esc);
    let dump = support::render(&mut app, 60, 20);
    assert!(
        says(&dump, "wrong at 4"),
        "the complaint did not come back to the caret:\n{dump}"
    );
}

/// Two problems on one line are two rows, and two different complaints.
///
/// The list has a row per trouble and a line can hold several -- a server
/// reports the mistake and its own notes about it at the same place. Told
/// only which line the reader has selected, the complaint shows the worst
/// of them whichever row it is, and two rows saying different things look
/// like one thing said twice.
///
/// Broken deliberately by finding the trouble by its line alone: both rows
/// then show the first of them.
#[test]
fn two_problems_on_one_line_are_two_complaints() {
    let mut file = String::new();
    for line in 0..60 {
        file.push_str(&format!("    let _ = {line};\n"));
    }
    let (_scratch, mut app, path) = editing("trouble-two", &file);
    let one = |column: u32, severity: u8, message: &str| {
        json!({
            "range": { "start": { "line": 20, "character": column },
                       "end": { "line": 20, "character": column + 3 } },
            "severity": severity,
            "source": "rustc",
            "message": message
        })
    };
    // Two of them at two places on the one line, the second milder than
    // the first: told only the line, the complaint would show the first
    // for both rows. A remark rather than a hint, because a hint is a note
    // hung on another diagnostic and the list leaves those out.
    app.publish_for_test(json!({
        "uri": support::uri_for(path),
        "diagnostics": [one(4, 1, "cannot find value here"),
                        one(8, 3, "defined over here")]
    }));

    // The words of the complaint, which are the ones not on a row of the
    // list: a row names the tool that said them and the frame does not.
    let framed = |app: &mut App| {
        let dump = support::render(app, 60, 20);
        let said: String = support::text_block(&dump)
            .lines()
            .filter(|row| row.contains('\u{2502}'))
            .collect::<Vec<&str>>()
            .join("\n");
        (said, dump)
    };

    support::press_alt(&mut app, 'e');
    let (first, dump) = framed(&mut app);
    assert!(
        first.contains("cannot find value here") && !first.contains("defined over here"),
        "the first row's complaint is not the first row's:\n{dump}"
    );

    support::press(&mut app, crossterm::event::KeyCode::Down);
    let (second, dump) = framed(&mut app);
    assert!(
        second.contains("defined over here") && !second.contains("cannot find value here"),
        "the second row showed the first row's complaint:\n{dump}"
    );
    assert_ne!(
        first, second,
        "two rows saying different things showed the same complaint:\n{dump}"
    );
}

/// The notes a compiler hangs on its diagnostics are not rows of their own.
///
/// rustc answers with one diagnostic and several sub-diagnostics: `this
/// function takes 2 arguments but 1 was supplied` comes with `function
/// defined here`, `cannot find function step_99` with `a function with a
/// similar name exists`. rust-analyzer sends each of those as a diagnostic
/// of its own, at the place it points at and one severity down -- so
/// `function defined here` arrives as a hint eighty lines from the error
/// it belongs to, and a file with seven errors lists sixteen rows.
///
/// Read alone they say nothing, and the error each is an answer to is in
/// the list anyway. So they are left out of the two places a reader works
/// through what is wrong -- this list, and the keys that walk it -- and
/// left in everywhere the reader is asking about a particular place.
///
/// Broken deliberately by listing or walking `troubles` rather than
/// `problems`.
#[test]
fn the_notes_hung_on_a_diagnostic_are_not_problems_of_their_own() {
    let mut file = String::new();
    for line in 0..40 {
        file.push_str(&format!("    let _ = {line};\n"));
    }
    let (_scratch, mut app, path) = editing("trouble-hints", &file);
    let one = |line: u32, severity: u8, message: &str| {
        json!({
            "range": { "start": { "line": line, "character": 4 },
                       "end": { "line": line, "character": 8 } },
            "severity": severity,
            "source": "rustc",
            "message": message
        })
    };
    // A note pointing back at an error further down, the error itself, a
    // second note, and a warning: the shape rustc's children arrive in.
    app.publish_for_test(json!({
        "uri": support::uri_for(path),
        "diagnostics": [one(5, 4, "function defined here"),
                        one(10, 1, "cannot find it"),
                        one(12, 4, "a function with a similar name exists"),
                        one(20, 2, "unused")]
    }));

    support::press_alt(&mut app, 'e');
    let dump = support::render(&mut app, 60, 20);
    let rows: Vec<&str> = support::text_block(&dump)
        .lines()
        .filter(|row| row.contains("rustc"))
        .collect();
    assert_eq!(rows.len(), 2, "the notes were listed as problems:\n{dump}");
    assert!(
        rows.iter().any(|row| row.contains("cannot find it"))
            && rows.iter().any(|row| row.contains("unused")),
        "the list lost a problem along with the notes:\n{dump}"
    );
    support::press(&mut app, crossterm::event::KeyCode::Esc);

    // And the keys walk the same set. From the top, the first stop is the
    // error rather than the note five lines in.
    support::press_alt(&mut app, ']');
    assert_eq!(
        app.current_buffer()
            .map(|buffer| buffer.cursor().line.get()),
        Some(10),
        "walking stopped at a note"
    );

    // What is wrong with the line the reader is on is still answered in
    // full: a note is underlined where it is, and a mark a reader can see
    // and cannot ask about would be worse than a row they can skip.
    let dump = support::render(&mut app, 60, 20);
    assert_eq!(
        app.troubles().len(),
        4,
        "the notes were dropped rather than left out of the list:\n{dump}"
    );
}

/// A row in another file shows that file where this one is drawn.
///
/// The list stays where it is -- ten rows on the status bar -- and what
/// changes is which file is above it. A project's problems are mostly in
/// files nobody has opened, and a list that could only say `other.rs:12`
/// about them is a list the reader has to leave to read.
///
/// The reader's own file is not previewed over itself: a row in it scrolls
/// the real one, caret and complaint and all.
///
/// Broken deliberately by leaving `previews` off the list in
/// `open_troubles`: the row moves and the file above it does not change.
#[test]
fn a_row_in_another_file_shows_that_file_above_the_list() {
    let (_scratch, mut app, path) = editing("trouble-preview", "fn main() {\n    nmae;\n}\n");
    let elsewhere = path.with_file_name("other.rs");
    std::fs::write(&elsewhere, "fn other() {\n    let oops = 1;\n}\n").expect("writing");
    app.publish_for_test(published(&path, 1, 4, 8, 1));
    app.publish_for_test(json!({
        "uri": support::uri_for(&elsewhere),
        "diagnostics": [{
            "range": { "start": { "line": 1, "character": 8 },
                       "end": { "line": 1, "character": 12 } },
            "severity": 2,
            "source": "clippy",
            "message": "unused variable `oops`"
        }]
    }));

    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::SymbolTroubles);
    support::press(&mut app, crossterm::event::KeyCode::Tab);
    let dump = support::render(&mut app, 74, 18);
    // Opened on the reader's own file, which is the real one: nothing is
    // read a second time to be shown over itself.
    assert!(
        app.preview().is_none(),
        "the file being read was previewed over itself:\n{dump}"
    );
    assert!(
        support::text_block(&dump).contains("fn main"),
        "the file being read is not behind the list:\n{dump}"
    );

    // Onto the other file's row, which is somewhere the reader has not
    // opened and does not have to.
    support::press(&mut app, crossterm::event::KeyCode::Up);
    let dump = support::render(&mut app, 74, 18);
    let text = support::text_block(&dump);
    assert!(
        text.contains("fn other") && text.contains("let oops"),
        "the row's own file is not above the list:\n{dump}"
    );
    assert!(
        !text.contains("fn main"),
        "two files at once above the list:\n{dump}"
    );
    assert_eq!(
        app.current_buffer().expect("a buffer").path(),
        path,
        "looking at the row opened a file the reader did not choose"
    );

    // And back: the reader's own file is theirs again, untouched.
    support::press(&mut app, crossterm::event::KeyCode::Down);
    let dump = support::render(&mut app, 74, 18);
    assert!(
        support::text_block(&dump).contains("fn main"),
        "walking back did not put the file being read back:\n{dump}"
    );
    support::press(&mut app, crossterm::event::KeyCode::Esc);
    let dump = support::render(&mut app, 74, 18);
    assert!(
        support::text_block(&dump).contains("fn main"),
        "leaving the list left somebody else's file on screen:\n{dump}"
    );
}

/// A previewed problem says what it is, in the box the editor says it in.
///
/// A row of the list is one line -- rustc writes paragraphs and a row has
/// room for a sentence -- so a reader who has walked to a problem in
/// somebody else's file can see where it is and not what it says. The box
/// is where the words fit, and it is the editor's own box: the same frame,
/// the same wrapping, hanging under the same line.
///
/// Broken deliberately by taking `what_is_wrong` out of `refresh_preview`:
/// the first line is on the row and the rest is nowhere.
#[test]
fn a_previewed_problem_opens_its_own_words_under_it() {
    let (_scratch, mut app, path) = editing("trouble-preview-words", "fn main() {}\n");
    let elsewhere = path.with_file_name("other.rs");
    std::fs::write(&elsewhere, "fn other() {\n    let oops = 1;\n}\n").expect("writing");
    app.publish_for_test(json!({
        "uri": support::uri_for(&elsewhere),
        "diagnostics": [{
            "range": { "start": { "line": 1, "character": 8 },
                       "end": { "line": 1, "character": 12 } },
            "severity": 2,
            "source": "clippy",
            "message": "unused variable `oops`\nhelp: prefix it with an underscore"
        }]
    }));

    // This file is clean, so the list opens on the project, on the only row
    // there is -- which is in a file nobody has opened.
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::SymbolTroubles);
    let dump = support::render(&mut app, 74, 20);
    let text = support::text_block(&dump);
    assert!(
        text.contains("prefix it with an underscore"),
        "the rest of what the server said is nowhere:\n{dump}"
    );
    // The row itself is still one line of it: the box is what holds the
    // paragraph.
    let label = app
        .picker()
        .expect("the list")
        .matches()
        .next()
        .expect("a row")
        .label
        .clone();
    assert!(
        !label.contains("prefix it"),
        "the whole paragraph went into the row: {label:?}"
    );
    // And it hangs under the line it is about, inside the frame that says
    // these rows are not the file.
    let rows: Vec<&str> = text.lines().filter(|row| row.contains('|')).collect();
    let said = rows
        .iter()
        .position(|row| row.contains("unused variable"))
        .expect("the words");
    let about = rows
        .iter()
        .position(|row| row.contains("let oops"))
        .expect("the line");
    assert!(
        said > about,
        "the words are not under the line they are about:\n{dump}"
    );
    assert!(
        rows[said].contains('\u{2502}'),
        "the words are not in a frame of their own:\n{dump}"
    );
}

/// One file's underlines are not drawn on another file's lines.
///
/// A preview underlines what a server said about the file *it* is showing.
/// What it must never wear is the underlines of the file it is drawn over:
/// a style is patched onto a cell rather than replacing what is there --
/// that is how a caller sets a background and keeps a foreground -- and a
/// modifier patched that way is one nothing takes off again. So the file
/// being read left its underlines at the columns they were at *there*, on
/// lines of somebody else's file, which is a complaint about a line that
/// has never seen one.
///
/// Broken deliberately by leaving the modifiers alone in `fill`: the
/// underlines of the two files are drawn at once and only one of them is
/// about anything on screen.
#[test]
fn a_preview_does_not_wear_the_underlines_of_the_file_under_it() {
    let mut under = String::new();
    for line in 0..30 {
        under.push_str(&format!("    let UNDER_{line} = {line};\n"));
    }
    let (_scratch, mut app, path) = editing("trouble-underline-leak", &under);
    let elsewhere = path.with_file_name("other.rs");
    std::fs::write(&elsewhere, "fn other() {\n    let oops = 1;\n}\n").expect("writing");
    // Wrong on the first lines of the file being read, over a longer run
    // than the other file's, so a leak shows up as an underline that runs
    // past what the preview's own server named.
    app.publish_for_test(json!({
        "uri": support::uri_for(&path),
        "diagnostics": [
            { "range": { "start": { "line": 0, "character": 8 },
                         "end": { "line": 0, "character": 15 } },
              "severity": 1, "source": "rustc", "message": "wrong here" },
            { "range": { "start": { "line": 1, "character": 8 },
                         "end": { "line": 1, "character": 15 } },
              "severity": 1, "source": "rustc", "message": "wrong there" }
        ]
    }));
    app.publish_for_test(published(&elsewhere, 1, 8, 12, 1));

    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::SymbolTroubles);
    support::press(&mut app, crossterm::event::KeyCode::Tab);
    support::press(&mut app, crossterm::event::KeyCode::Up);
    let dump = support::render(&mut app, 74, 20);
    let cells = support::cells_of(&mut app, 74, 20);
    let rows: Vec<&str> = support::text_block(&dump)
        .lines()
        .filter(|row| row.contains('|'))
        .collect();
    // Where the preview's own problem is: the word its server named, on
    // the row the preview drew it.
    let at = rows
        .iter()
        .position(|row| row.contains("let oops"))
        .expect("the previewed line");
    let row = rows[at];
    let text = &row[row.find('|').expect("a divider") + 1..];
    let before = text.find("oops").expect("the word");
    let column = u16::try_from(text[..before].chars().count()).expect("a column");
    let y = u16::try_from(at).expect("a row");
    let its_own: Vec<(u16, u16)> = (0..4).map(|offset| (column + offset, y)).collect();

    let underlined: Vec<(u16, u16)> = (0..u16::try_from(rows.len()).expect("rows"))
        .flat_map(|y| (0..74u16).map(move |x| (x, y)))
        .filter(|(x, y)| {
            cells
                .cell((*x, *y))
                .expect("a cell")
                .modifier
                .contains(ratatui::style::Modifier::UNDERLINED)
        })
        .collect();
    assert_eq!(
        underlined, its_own,
        "the underlines on screen are not exactly what this file's server named:\n{dump}"
    );
}

/// A problem is not marked in its own preview.
///
/// The box under the line says which line, which column and what was said.
/// A run of colour over the same characters says it again -- and says it
/// differently, because a mark is one colour whatever the severity. It
/// would also be the odd one out: a row in the file the reader is already
/// in has no preview to mark, so half a list of the project would be
/// highlighted and half of it not, for no reason a reader could see.
///
/// Broken deliberately by keeping the resolved runs in `refresh_preview`:
/// the previewed characters come back in the marked background.
#[test]
fn a_problem_is_not_marked_in_its_own_preview() {
    let (_scratch, mut app, path) = editing("trouble-unmarked", "fn main() {}\n");
    let elsewhere = path.with_file_name("other.rs");
    std::fs::write(&elsewhere, "fn other() {\n    let oops = 1;\n}\n").expect("writing");
    app.publish_for_test(published(&elsewhere, 1, 8, 12, 1));

    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::SymbolTroubles);
    let dump = support::render(&mut app, 74, 20);
    assert!(
        support::text_block(&dump).contains("let oops"),
        "the problem's file is not being previewed:\n{dump}"
    );
    // The colour a mark is drawn in, which is the one thing this is about.
    assert!(
        !support::legend_block(&dump).contains("#1e3a5f"),
        "the previewed problem is marked as well as said:\n{dump}"
    );
}

/// What is wrong is underlined wherever the file is drawn, not only where
/// it is being read.
///
/// A reader walking a list of the project is looking straight at a broken
/// line in a file they have not opened. Without this they would have to
/// open it to find out which characters anybody was talking about -- which
/// is the list asking them to go there to find out whether to go there.
///
/// Broken deliberately by handing `EditorView::for_buffer` no troubles.
#[test]
fn a_previewed_problem_is_underlined_where_it_is() {
    let (_scratch, mut app, path) = editing("trouble-preview-underline", "fn main() {}\n");
    let elsewhere = path.with_file_name("other.rs");
    std::fs::write(&elsewhere, "fn other() {\n    let oops = 1;\n}\n").expect("writing");
    app.publish_for_test(published(&elsewhere, 1, 8, 12, 2));

    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::SymbolTroubles);
    let dump = support::render(&mut app, 74, 20);
    let cells = support::cells_of(&mut app, 74, 20);
    let rows: Vec<&str> = support::text_block(&dump)
        .lines()
        .filter(|row| row.contains('|'))
        .collect();
    let at = rows
        .iter()
        .position(|row| row.contains("let oops"))
        .expect("the previewed line");
    let row = rows[at];
    let text = &row[row.find('|').expect("a divider") + 1..];
    let before = text.find("oops").expect("the word");
    let column = u16::try_from(text[..before].chars().count()).expect("a column");
    let y = u16::try_from(at).expect("a row");
    let underlined = |x: u16| {
        cells
            .cell((x, y))
            .expect("a cell")
            .modifier
            .contains(ratatui::style::Modifier::UNDERLINED)
    };
    for offset in 0..4 {
        assert!(
            underlined(column + offset),
            "character {offset} of what the server named is not underlined in the preview:\n{dump}"
        );
    }
    assert!(
        !underlined(column + 4),
        "the underline runs past what the server named:\n{dump}"
    );
    // And in the colour that kind of trouble is written in: a warning and
    // an error have to be told apart without reading them, here as well.
    assert!(
        cells
            .cell((column, y))
            .expect("a cell")
            .underline_color
            .ne(&ratatui::style::Color::Reset),
        "the underline has no colour of its own:\n{dump}"
    );
}

/// The box belongs to the list that names the problem, and goes when that
/// list does.
///
/// The underline is every buffer's -- what is wrong is wrong wherever the
/// line is drawn -- but four rows of words about something a reader did not
/// ask after are four rows of the preview they did ask for. And a preview
/// is kept while the file it shows is: the box one list opened must not
/// still be hanging there under the next.
///
/// Broken deliberately by opening the box for every list in
/// `refresh_preview`: the search's preview grows a complaint about the line
/// its match happens to be on.
#[test]
fn only_a_list_of_problems_opens_the_words_in_its_preview() {
    let (_scratch, mut app, path) = editing("trouble-preview-scope", "fn main() {}\n");
    let elsewhere = path.with_file_name("other.rs");
    std::fs::write(&elsewhere, "fn other() {\n    let oops = 1;\n}\n").expect("writing");
    app.publish_for_test(json!({
        "uri": support::uri_for(&elsewhere),
        "diagnostics": [{
            "range": { "start": { "line": 1, "character": 8 },
                       "end": { "line": 1, "character": 12 } },
            "severity": 2,
            "source": "clippy",
            "message": "unused variable `oops`\nhelp: prefix it with an underscore"
        }]
    }));

    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::SymbolTroubles);
    let dump = support::render(&mut app, 74, 20);
    assert!(
        support::text_block(&dump).contains("prefix it with an underscore"),
        "the list of problems did not open the words:\n{dump}"
    );

    // The same file, previewed from a list that is about something else.
    support::press(&mut app, crossterm::event::KeyCode::Esc);
    app.open_picker_for_test(
        vec![obelus_component::picker::PickerItem {
            prose: false,
            marker: None,
            icon: None,
            label: "other.rs:2".to_string(),
            detail: None,
            trailing: None,
            changed: None,
            value: obelus_component::picker::PickerValue::Place {
                path: elsewhere,
                line: 1,
                character: 8,
                end_line: 1,
                end_character: 12,
            },
            enabled: true,
            colours: None,
            status: None,
            depth: 0,
            opens: None,
            kind: None,
            tab: None,
            section: None,
        }],
        obelus_component::picker::PickerLayout::FullArea,
    );
    let dump = support::render(&mut app, 74, 20);
    let text = support::text_block(&dump);
    assert!(
        text.contains("let oops"),
        "the file is not being previewed at all:\n{dump}"
    );
    assert!(
        !text.contains("prefix it with an underscore"),
        "another list's preview kept the box:\n{dump}"
    );
}

/// A server publishing about a file keeps what Obelus said about it.
///
/// The two are in one list and keep different rules. A server's set for a
/// path is replaced whole when it publishes another -- that is the
/// protocol's rule about a *server's* own, and it says nothing about
/// anybody else's. A TOML server having an opinion about the settings must
/// not take Obelus's marks off them, and it would: they live at the same
/// path.
///
/// Deliberate break: `publish` inserting only what arrived, which is what
/// it did.
#[test]
fn a_server_publishing_keeps_what_obelus_said() {
    let scratch = support::Scratch::new("trouble-both");
    let path = scratch.path().join("sample.rs");
    std::fs::write(&path, "fn main() {\n    let x = nmae;\n}\n").expect("writing the file");
    let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, 60, 16);

    app.obelus_says_for_test(
        &path,
        obelus_text::coordinates::Span {
            line: obelus_text::coordinates::LineNumber::new(0),
            column: obelus_text::coordinates::CharColumn::new(0),
            end_line: obelus_text::coordinates::LineNumber::new(0),
            end_column: obelus_text::coordinates::CharColumn::new(2),
        },
        obelus_lsp::trouble::Severity::Warning,
        "Obelus has an opinion",
    );
    assert_eq!(app.problems().count(), 1);

    app.publish_for_test(published(&path, 1, 12, 16, 1));

    let said: Vec<Option<&str>> = app
        .problems()
        .map(|problem| problem.source.as_deref())
        .collect();
    assert_eq!(said, [Some("Obelus"), Some("rustc")], "{said:?}");
}

/// And a server with nothing left to say takes only its own away.
///
/// Deliberate break: the empty case going back to `remove`, which takes
/// the whole entry and Obelus's marks with it.
#[test]
fn a_server_with_nothing_to_say_leaves_what_obelus_said() {
    let scratch = support::Scratch::new("trouble-emptied");
    let path = scratch.path().join("sample.rs");
    std::fs::write(&path, "fn main() {}\n").expect("writing the file");
    let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, 60, 16);

    app.obelus_says_for_test(
        &path,
        obelus_text::coordinates::Span {
            line: obelus_text::coordinates::LineNumber::new(0),
            column: obelus_text::coordinates::CharColumn::new(0),
            end_line: obelus_text::coordinates::LineNumber::new(0),
            end_column: obelus_text::coordinates::CharColumn::new(2),
        },
        obelus_lsp::trouble::Severity::Warning,
        "Obelus has an opinion",
    );
    app.publish_for_test(json!({
        "uri": support::uri_for(&path),
        "diagnostics": []
    }));

    assert_eq!(
        app.problems().count(),
        1,
        "Obelus's mark went with the server's"
    );
}

/// What is wrong with the line being typed on is said while it is typed.
///
/// There was a rule here that held it back until the reader left the line,
/// on the grounds that a complaint was rows hung under the line and the
/// whole file below it moved as they opened and shut. The complaint became
/// a box floated over the line instead, which moves nothing, and the reason
/// went with it -- so the rule was two things a reader had to do before
/// Obelus would say what it already knew, and one of them (`ctrl+s`) it did
/// not even listen for.
///
/// Broken deliberately by putting the rule back -- the `typed_on` field,
/// the line recorded on every edit, and the check in `show_what_is_wrong`.
/// Nothing smaller does it: clearing `complaining` at the edit is a no-op,
/// because it is worked out again from the caret on the next frame, which
/// is why the rule needed a field to remember the line at all.
#[test]
fn what_is_wrong_is_said_on_the_line_being_typed_on() {
    let (_scratch, mut app, path) = editing("trouble-typing", "fn main() {\n    nmae;\n}\n");
    app.publish_for_test(published(&path, 1, 4, 8, 1));
    let said = |app: &mut App| {
        let dump = support::render(app, 60, 16);
        support::text_block(&dump).contains("cannot find value")
    };

    stand_on(&mut app, 1, 4);
    assert!(said(&mut app), "standing on it did not open it");

    // Still on the line, having just typed into it: no walking away, no
    // save, nothing the reader has to do first.
    support::type_text(&mut app, "x");
    assert!(
        said(&mut app),
        "the line being typed on kept what is wrong with it to itself"
    );
}

/// What a server said is wrong goes with the text it is about until the
/// server says again, and goes altogether with text that is taken away.
///
/// Broken deliberately two ways. Leaving `keep_troubles_across` uncalled
/// in `change_in` keeps the underline on line 1 after a line is put in
/// above it -- under code that is not what it was about. And dropping the
/// `to <= from` check keeps an empty trouble about text that is gone.
#[test]
fn what_is_wrong_moves_with_the_text_it_is_about() {
    let (_scratch, mut app, path) = editing("trouble-moving", "fn main() {\n    nmae;\n}\n");
    app.publish_for_test(published(&path, 1, 4, 8, 1));
    let spans = |app: &App| {
        app.troubles()
            .iter()
            .map(|trouble| {
                (
                    trouble.span.line.get(),
                    trouble.span.column.get(),
                    trouble.span.end_column.get(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(spans(&app), vec![(1, 4, 8)]);

    // A line above it.
    support::press(&mut app, crossterm::event::KeyCode::Enter);
    support::lay_out(&mut app, 60, 16);
    assert_eq!(spans(&app), vec![(2, 4, 8)], "it stayed where the text was");

    // And the text it is about, taken away from its end.
    support::press(&mut app, crossterm::event::KeyCode::Down);
    support::press(&mut app, crossterm::event::KeyCode::End);
    support::press(&mut app, crossterm::event::KeyCode::Left);
    for _ in 0..4 {
        support::press(&mut app, crossterm::event::KeyCode::Backspace);
    }
    support::lay_out(&mut app, 60, 16);
    assert_eq!(
        app.current_buffer()
            .map(|buffer| buffer.text().rope().line(2).to_string()),
        Some("    ;\n".to_string()),
        "the test did not take the word away"
    );
    assert!(
        spans(&app).is_empty(),
        "about text that is gone: {:?}",
        spans(&app)
    );
}

/// The problems go in a view's place, like any list over the file: a reader
/// in the files who asks what is wrong is not made to leave them first. And
/// the key in the problems is the problems, with what was typed.
///
/// Broken deliberately twice. Taking `SymbolTroubles` out of
/// `Command::opens_a_list`: `alt+e` in the files is refused and the files
/// stay. And taking `opened_by` out of `open_troubles`: `alt+e` in the
/// problems closes them and opens them again, empty.
#[test]
fn the_problems_take_a_view_s_place() {
    let (_scratch, mut app, path) = editing("trouble-swap", "fn main() {\n    nmae;\n}\n");
    app.publish_for_test(published(&path, 1, 4, 8, 1));
    support::press_function(&mut app, 1);
    assert!(
        app.picker().is_some_and(|picker| picker.is_listing()),
        "the files did not open"
    );
    support::press_alt(&mut app, 'e');
    assert!(
        app.picker()
            .is_some_and(|picker| picker.opener() == Some(obelus_command::Command::SymbolTroubles)),
        "alt+e in the files did not go to the problems"
    );
    support::type_text(&mut app, "nm");
    support::press_alt(&mut app, 'e');
    assert_eq!(
        app.picker().map(|picker| picker.query().to_string()),
        Some("nm".to_string()),
        "alt+e in the problems did not leave the reader where they were"
    );
}
