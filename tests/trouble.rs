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

    // The bar down the complaint's left is the same half-cell as the mark
    // beside the line it hangs from. Two halves of one column is half a
    // cell apart on screen, which is exactly far enough to read as two
    // marks that failed to line up.
    let cells = support::cells_of(&mut app, 60, 16);
    let margin = |y: u16| cells.cell((0, y)).expect("a cell").symbol().to_string();
    assert_eq!(
        margin(1),
        margin(2),
        "the complaint's bar is not on the half its line is marked on:\n{dump}"
    );
    assert_eq!(
        margin(2),
        margin(3),
        "the complaint's own rows are not all marked alike:\n{dump}"
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
        "uri": format!("file://{}", path.display()),
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

/// A file outside a repository is marked too.
///
/// The two columns used to be reserved for one reason -- being in a
/// repository -- and a file that was not in one had nowhere to carry a
/// mark, so the only news that something was wrong was an underline on the
/// line the reader was already looking at. They are reserved for a second
/// reason now: the language having a server in obelus's table.
///
/// On the language rather than on anything anyone has said, and this file
/// is the case that shows why: there is no repository, no server installed
/// in the test's environment and nothing has started -- and the columns are
/// there all the same, because a width that arrived with an answer would
/// rewrap every line under the reader the moment a server spoke.
///
/// Broken deliberately by putting `marks` back to the changes alone: both
/// columns go, and with them everything this asserts.
#[test]
fn a_file_with_no_repository_still_says_where_the_problems_are() {
    let (_scratch, mut app, path) = editing("trouble-nowhere", "fn main() {\n    nmae;\n}\n");
    assert!(
        app.changes().is_none(),
        "the scratch directory turned out to be a repository, so this proves nothing"
    );
    app.publish_for_test(published(&path, 1, 4, 8, 1));

    let cells = support::cells_of(&mut app, 60, 16);
    let dump = support::render(&mut app, 60, 16);
    let at = |x: u16, y: u16| cells.cell((x, y)).expect("a cell").clone();

    // The margin is the first column, and the line with something wrong
    // with it is the second row.
    assert_eq!(
        at(0, 1).symbol(),
        "\u{258c}",
        "the margin does not mark the line, or is not there at all:\n{dump}"
    );
    assert_ne!(
        at(0, 1).fg,
        at(0, 0).fg,
        "the mark is the colour of a cell with nothing in it:\n{dump}"
    );
    // And the map, in the column before the bar.
    let marked = (0..14)
        .filter(|y| at(58, *y).symbol() == "\u{2590}")
        .count();
    assert_eq!(
        marked, 1,
        "the map does not have the one problem on it:\n{dump}"
    );
}

/// The words are framed, and the frame starts under the word they are about.
///
/// A complaint is prose about a line, and prose drawn in the plain colour
/// at the code's own left edge reads as a line of the file written in
/// English. The frame says it is not the file; the indent says which word
/// it is about.
///
/// The count of the others on the line rides the bottom rail rather than
/// sitting inside: what is inside is what the server said, and the frame
/// is obelus's, and so is the arithmetic.
///
/// Broken deliberately by returning the words unframed, by putting the
/// count inside the frame with them, or by dropping the indent: each is a
/// different assertion here and each goes red.
#[test]
fn a_complaint_is_framed_and_its_count_rides_the_rail() {
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
        "uri": format!("file://{}", path.display()),
        "diagnostics": [one(4, 8, 1, "cannot find value `nmae` in this scope"),
                        one(4, 8, 2, "unused something")]
    }));
    support::press(&mut app, crossterm::event::KeyCode::Down);

    let dump = support::render(&mut app, 60, 16);
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

    let about = rows
        .iter()
        .position(|row| row.contains("nmae"))
        .expect("the line it is about");
    let top = rows
        .iter()
        .position(|row| row.contains('\u{250c}'))
        .unwrap_or_else(|| panic!("the words are not framed:\n{dump}"));
    let bottom = rows
        .iter()
        .position(|row| row.contains('\u{2514}'))
        .unwrap_or_else(|| panic!("the frame has no bottom rail:\n{dump}"));
    let said = rows
        .iter()
        .position(|row| row.contains("cannot find value"))
        .expect("the words");
    assert!(
        top == about + 1 && said > top && bottom > said,
        "the frame is not wrapped round the words under the line:\n{dump}"
    );
    assert!(
        rows[said].contains('\u{2502}'),
        "the row the words are on has no rails:\n{dump}"
    );
    // Under the word it is about, not at the code's own left edge.
    assert_eq!(
        column(rows[top], "\u{250c}"),
        column(rows[about], "nmae"),
        "the frame does not start under the word it is about:\n{dump}"
    );
    // On the rail, which is obelus's, and not inside, which is the
    // server's.
    assert!(
        rows[bottom].contains("and 1 more here"),
        "the count is not on the bottom rail:\n{dump}"
    );
    assert!(
        !rows[said].contains("and 1 more here") && !rows[said - 1].contains("more here"),
        "the count is inside the frame with the server's own words:\n{dump}"
    );
}

/// A narrow window gives up the indent, then the frame -- never the fit.
///
/// Anything wider than the room is wrapped by the editor like any other
/// text, and a wrapped frame is a rail to a row: the frame taken apart
/// into exactly the shape it was drawn to avoid. So the room decides what
/// is affordable before the words are laid out, rather than the frame
/// being built and clamped afterwards.
///
/// Broken deliberately by letting the words inside be wider than what is
/// left of the row -- which is what a `max` on that width does, however
/// reasonable the number in it looks.
#[test]
fn a_narrow_window_gives_up_the_frame_before_it_gives_up_the_fit() {
    let framed_at = |width: u16| {
        let scratch = support::Scratch::new(&format!("trouble-narrow-{width}"));
        let path = scratch.path().join("sample.rs");
        std::fs::write(&path, "fn main() {\n    let _ = step_99(point);\n}\n").expect("writing");
        let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
        app.working_directory_for_test(scratch.path().to_path_buf());
        // Wrapped, because it is the wrapping that took the frame apart.
        app.configure(
            obelus::config::Config {
                wrap: true,
                ..obelus::config::Config::default()
            },
            Vec::new(),
        );
        support::lay_out(&mut app, width, 24);
        // One sentence, so that the whole frame fits the screen: what is
        // being measured is its width, and a bottom rail scrolled off the
        // bottom would read as a bottom rail that was never drawn.
        app.publish_for_test(json!({
            "uri": format!("file://{}", path.display()),
            "diagnostics": [{
                "range": { "start": { "line": 1, "character": 12 },
                           "end": { "line": 1, "character": 19 } },
                "severity": 1,
                "source": "rustc",
                "message": "cannot find function `step_99` in this scope"
            }]
        }));
        support::press(&mut app, crossterm::event::KeyCode::Down);
        let dump = support::render(&mut app, width, 24);
        let rows: Vec<String> = support::text_block(&dump)
            .lines()
            .filter(|row| row.contains('|'))
            .map(|row| row[row.find('|').expect("a divider") + 1..].to_string())
            .collect();
        (rows, dump)
    };

    // Narrow, but with room to frame: every rail is whole, on one row.
    let (rows, dump) = framed_at(34);
    let tops: Vec<&String> = rows.iter().filter(|row| row.contains('\u{250c}')).collect();
    assert_eq!(
        tops.len(),
        1,
        "the top rail is on more than one row:\n{dump}"
    );
    assert!(
        tops[0].contains('\u{2510}'),
        "the top rail ran off the row it started on:\n{dump}"
    );
    let bottoms: Vec<&String> = rows.iter().filter(|row| row.contains('\u{2514}')).collect();
    assert_eq!(
        bottoms.len(),
        1,
        "the bottom rail is on more than one row:\n{dump}"
    );
    assert!(
        bottoms[0].contains('\u{2518}'),
        "the bottom rail ran off the row it started on:\n{dump}"
    );
    // Every row of words has both of its side rails, which is the same
    // fact said about the middle of the frame.
    let words: Vec<&String> = rows
        .iter()
        .filter(|row| row.contains("cannot") || row.contains("scope"))
        .collect();
    assert!(!words.is_empty(), "the words are nowhere:\n{dump}");
    for row in words {
        assert_eq!(
            row.matches('\u{2502}').count(),
            2,
            "a row of words is missing a rail:\n{dump}"
        );
    }

    // Too narrow to frame: the words, and nothing pretending to be a
    // frame. Said as "no rail anywhere" rather than "no corner", because a
    // frame that had come apart would leave the rails behind.
    let (rows, dump) = framed_at(20);
    assert!(
        rows.iter().any(|row| row.contains("cannot")),
        "the words went with the frame:\n{dump}"
    );
    assert!(
        !rows.iter().any(|row| row.contains('\u{2502}')
            || row.contains('\u{250c}')
            || row.contains('\u{2514}')),
        "a frame was drawn where there was no room for one:\n{dump}"
    );
}
