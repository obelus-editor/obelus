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
    obelus::app::dispatch::dispatch(&mut app, obelus::command::Command::SymbolTroubles);

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
        "uri": support::uri_for(path),
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

/// A narrow window gives up the indent and keeps the frame whole.
///
/// The frame is the thing that says these rows are not the file, and half
/// a frame says it worse than none -- so what a narrow window costs is the
/// words wrapping harder inside it, which is only prose being prose. The
/// indent is the one thing given up, and given up outright rather than
/// shaved: which word the complaint is about is a nicety, and it is not
/// worth every sentence wrapping twice as hard.
///
/// Anything wider than the room is wrapped by the editor like any other
/// text, and a wrapped frame is a rail to a row: the frame taken apart
/// into exactly the shape it was drawn to avoid. So the room decides what
/// is affordable before the words are laid out, rather than the frame
/// being built and clamped afterwards.
///
/// Broken deliberately by shaving the indent rather than dropping it, or
/// by letting the words inside be wider than what is left of the row --
/// which is what a `max` on that width does, however reasonable the number
/// in it looks.
#[test]
fn a_narrow_window_gives_up_the_indent_and_keeps_the_frame_whole() {
    let rows_at = |width: u16| {
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
        // One sentence, so the whole frame fits the screen: what is being
        // measured is its width, and a bottom rail scrolled off the bottom
        // would read as a bottom rail that was never drawn.
        app.publish_for_test(json!({
            "uri": support::uri_for(path),
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

    // Whole at every width, which is four facts: one top rail with both
    // its corners, one bottom rail with both of its, and a pair of side
    // rails on every row of words.
    let whole = |rows: &[String], dump: &str, width: u16| {
        for (corner, opposite, which) in [
            ('\u{250c}', '\u{2510}', "top"),
            ('\u{2514}', '\u{2518}', "bottom"),
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
            .filter(|row| row.contains("cannot") || row.contains("scope"))
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
            .find(|row| row.contains('\u{250c}'))
            .expect("a top rail");
        let byte = row.find('\u{250c}').expect("the corner");
        row[..byte].chars().count()
    };

    let (wide, dump) = rows_at(60);
    whole(&wide, &dump, 60);
    let (narrow, dump) = rows_at(20);
    whole(&narrow, &dump, 20);
    // And one in between, where a shaved indent and a given-up one are
    // different numbers: at the narrowest they both come out at nothing,
    // so a width where they disagree is the only place the rule is
    // actually being read.
    let (between, middle) = rows_at(40);
    whole(&between, &middle, 40);

    // And the indent is what paid for it: at the wider width the frame
    // hangs under the word, at the narrower one it is back at the code's
    // own left edge.
    assert!(
        starts(&wide) > starts(&narrow),
        "the narrow window kept an indent it could not afford:\n{dump}"
    );
    let left = |rows: &[String]| {
        rows.iter()
            .find(|row| row.contains("fn main"))
            // In characters: the fold mark before it is three bytes wide.
            .map(|row| {
                let byte = row.find("fn").expect("the word");
                row[..byte].chars().count()
            })
            .expect("the first line")
    };
    assert_eq!(
        starts(&narrow),
        left(&narrow),
        "the narrowest window kept an indent:\n{dump}"
    );
    assert_eq!(
        starts(&between),
        left(&between),
        "the indent was shaved rather than given up:\n{middle}"
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
    for _ in 0..4 {
        support::press(&mut app, crossterm::event::KeyCode::Down);
    }
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
