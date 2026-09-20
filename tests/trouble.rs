//! What a server says is wrong with a file.
//!
//! Against values rather than against a server: what is interesting is
//! what obelus does with a notification -- where it draws it, what it
//! counts, what it lists -- and a server cannot be made to find a mistake
//! on demand.

mod support;

use obelus::{app::App, buffer::Buffer};
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
        "uri": format!("file://{}", path.display()),
        "diagnostics": [{
            "range": { "start": { "line": line, "character": from },
                       "end": { "line": line, "character": to } },
            "severity": severity,
            "source": "rustc",
            "message": "cannot find value `nmae` in this scope\nhelp: a local variable with a similar name exists"
        }]
    })
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
    let column = u16::try_from(text.find("nmae").expect("the word")).expect("a column");
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
        "uri": format!("file://{}", path.display()),
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
    obelus::command::dispatch::dispatch(&mut app, obelus::command::Command::SymbolTroubles);

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
/// `cargo check`, rust-analyzer talks about the whole project.
#[test]
fn a_notification_about_a_file_that_is_not_open_is_dropped() {
    let (_scratch, mut app, path) = editing("trouble-elsewhere", "fn main() {}\n");
    let elsewhere = path.with_file_name("other.rs");
    app.publish_for_test(published(&elsewhere, 0, 0, 1, 1));
    assert!(
        app.troubles().is_empty(),
        "something was kept about a file obelus does not have"
    );
}

/// What a severity is, and what a uri says, which are the two things the
/// notification is made of.
#[test]
fn the_shapes_a_notification_arrives_in() {
    use obelus::lsp::trouble::{Severity, path_of};

    // A uri with an escape in it, which is what a path with a space comes
    // back as.
    assert_eq!(
        path_of(&json!({ "uri": "file:///tmp/a%20b/one.rs" })),
        Some(std::path::PathBuf::from("/tmp/a b/one.rs"))
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
/// says *what*, and costs the line a row of the file's own space. Which is
/// why it is only ever the caret's line: a file with thirty of these is a
/// file whose shape is the complaints rather than the code.
///
/// Broken deliberately by leaving `show_what_is_wrong` uncalled: the words
/// are nowhere on the page and this goes red.
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

    // Onto the line that has.
    support::press(&mut app, crossterm::event::KeyCode::Down);
    let dump = support::render(&mut app, 60, 16);
    assert!(
        support::text_block(&dump).contains("cannot find value"),
        "the words the server used are not under the line:\n{dump}"
    );

    // And away again -- one key, not two: the caret steps over a complaint
    // rather than into it, because it is not a thing the reader opened.
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

/// A complaint about the file's last line still opens under it.
///
/// The row after the last line is a place a block can hang, which nothing
/// could use before: a hunk that deleted the end of a file hung its removed
/// lines off a line the view never reached, so they could not be opened at
/// all. The alternative was a complaint that opened above its line at the
/// bottom of a file and below it everywhere else.
///
/// Reached by walking there rather than by starting there, in a file
/// taller than the screen, because the three things this needs each only
/// bite once the view has to scroll. Broken deliberately, three ways, each
/// leaving the words nowhere on the page: stopping the view's pass past the
/// end; taking `rows_below` out of `screen_rows_of`, so the last screenful
/// stops one row short of the block; and capping `scroll_into_view`'s tail
/// at nothing, so the block hangs just under the bottom edge with nothing
/// to pull it up.
#[test]
fn what_is_wrong_with_the_last_line_is_opened_under_it() {
    // Long enough that the last line is only reached by scrolling, and no
    // newline at the end so that line really is the last one: with a
    // trailing newline there is an empty line after it and the block hangs
    // off that, which is the ordinary case and not this one.
    let mut file = "fn main() {\n".to_string();
    for _ in 0..40 {
        file.push_str("    let _ = 1;\n");
    }
    file.push_str("    nmae");
    let (_scratch, mut app, path) = editing("trouble-last", &file);
    app.publish_for_test(published(&path, 41, 4, 8, 1));
    for _ in 0..41 {
        support::press(&mut app, crossterm::event::KeyCode::Down);
    }

    let dump = support::render(&mut app, 60, 16);
    let rows: Vec<&str> = support::text_block(&dump)
        .lines()
        .filter(|row| row.contains('|'))
        .collect();
    let said = rows
        .iter()
        .position(|row| row.contains("cannot find value"))
        .unwrap_or_else(|| panic!("the words are not on the page:\n{dump}"));
    let about = rows
        .iter()
        .position(|row| row.contains("nmae"))
        .expect("the line it is about");
    assert!(
        said > about,
        "the complaint about the last line opened above it:\n{dump}"
    );
}
