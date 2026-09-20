//! Showing a colour that is written down.
//!
//! A server that knows a language knows its colours, and what obelus does
//! with that is paint the characters themselves -- so a reader looking at
//! a stylesheet sees `#3264eb` in blue rather than reading six digits and
//! imagining it.

mod support;

use obelus::{app::App, buffer::Buffer};
use serde_json::json;

fn styling(name: &str, text: &str) -> (support::Scratch, App) {
    let scratch = support::Scratch::new(name);
    let path = scratch.path().join("one.css");
    std::fs::write(&path, text).expect("writing it");
    let mut app = App::new(vec![Buffer::open(&path).expect("opening it")]);
    app.working_directory_for_test(scratch.path().to_path_buf());
    support::lay_out(&mut app, 76, 18);
    (scratch, app)
}

/// A square of the colour itself is drawn in front of the literal, and the
/// literal keeps its own syntax colour.
///
/// The cell is one the file does not contain, so the characters after it
/// are drawn one further along than the text alone would put them -- which
/// is the thing everything else on the line has to agree about.
#[test]
fn a_colour_is_shown_in_front_of_where_it_is_written() {
    let (_scratch, mut app) = styling("colour-shown", "a { color: #3264eb; }\n");
    app.colours_for_test(json!([{
        "range": { "start": { "line": 0, "character": 11 },
                   "end": { "line": 0, "character": 18 } },
        "color": { "red": 0.196, "green": 0.392, "blue": 0.922, "alpha": 1.0 }
    }]));

    let dump = support::render(&mut app, 76, 18);
    let rows: Vec<&str> = support::text_block(&dump).lines().collect();
    let styles: Vec<&str> = support::style_block(&dump).lines().collect();
    let row = rows
        .iter()
        .position(|row| row.contains("#3264eb"))
        .expect("the line");
    let legend = |letter: char| {
        support::legend_block(&dump)
            .lines()
            .find(|entry| entry.starts_with(letter))
            .map(str::to_string)
            .unwrap_or_else(|| panic!("no legend for {letter:?}:\n{dump}"))
    };
    let divider = rows[row].find('|').expect("a divider") + 1;
    let text = &rows[row][divider..];
    let cells = &styles[row][divider..];
    let at = |column: usize| cells.chars().nth(column).expect("a style");
    // Cells, not bytes: the square in front of it is three bytes and one
    // cell, and `str::find` hands back a byte.
    let literal = support::column_of(text, "#3264eb");

    // The cell in front of the literal draws a square of the colour the
    // server named. Its ink rather than its background: a terminal cell is
    // about twice as tall as it is wide, so a filled one is an upright bar
    // and a bar beside a literal reads as a mark on the text rather than
    // as the colour itself.
    assert_eq!(
        text.chars().nth(literal - 1),
        Some('\u{25a0}'),
        "there is no square in front of the literal:\n{dump}"
    );
    assert!(
        legend(at(literal - 1)).contains("fg=#3264eb"),
        "the square is not the colour the server named:\n{dump}"
    );

    // And the literal itself is not painted in it any more: it is code,
    // and it keeps the colour code is written in.
    for offset in 0.."#3264eb".len() {
        assert_ne!(
            at(literal + offset),
            at(literal - 1),
            "the literal is painted as well as shown:\n{dump}"
        );
    }

    // And it took a cell of its own: everything after it is drawn one
    // further along than the file alone would put it.
    assert!(
        text.contains("color: \u{25a0}#3264eb"),
        "the square did not take a cell in front of the literal:\n{dump}"
    );
}

/// An answer about a document that has moved since is not shown: the
/// characters it names are characters of the file as it was.
#[test]
fn a_colour_about_a_file_that_has_changed_is_dropped() {
    let (_scratch, mut app) = styling("colour-stale", "a { color: #3264eb; }\n");
    support::type_text(&mut app, "x");
    app.colours_at_version_for_test(
        json!([{
            "range": { "start": { "line": 0, "character": 11 },
                       "end": { "line": 0, "character": 18 } },
            "color": { "red": 0.1, "green": 0.2, "blue": 0.9, "alpha": 1.0 }
        }]),
        app.current_buffer().expect("a buffer").version() - 1,
    );
    assert!(
        app.colours().is_empty(),
        "an answer about the file as it was is on screen"
    );
}

/// And the question obelus actually sends, read off the wire.
///
/// The one thing a server sees is the method name, and nothing that feeds
/// an answer in by hand can check it: a request spelt wrong is answered by
/// nobody, and from this side that looks exactly like a server with no
/// colours to report.
#[test]
fn the_question_that_goes_out_is_the_one_the_protocol_names() {
    use obelus::{app::dispatch, command::Command, syntax::LanguageId};

    let (_scratch, mut app) = styling("colour-asked", "a { color: #3264eb; }\n");
    let (sender, heard) = obelus::event::channel();
    app.events_for_test(sender);
    // A server that says back whatever it is told, so what obelus writes
    // can be read.
    assert!(
        app.stand_in_server_for_test(LanguageId::Css, "cat"),
        "the echo would not start"
    );
    app.declared_for_test(LanguageId::Css, json!({ "colorProvider": true }));

    // Saving is one of the moments the question is asked: the document has
    // stopped moving, so an answer about it is worth having.
    support::type_text(&mut app, " ");
    dispatch::dispatch(&mut app, Command::FileSave);

    let asked = support::heard_requests(&heard, "textDocument/documentColor", 1);
    assert_eq!(
        asked.len(),
        1,
        "obelus asked nobody where the colours are: {asked:?}"
    );
    assert!(
        asked[0]["params"]["textDocument"]["uri"]
            .as_str()
            .is_some_and(|uri| uri.ends_with("one.css")),
        "the question is about another file: {:?}",
        asked[0]
    );
}
