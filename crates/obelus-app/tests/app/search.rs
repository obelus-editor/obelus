//! Searching: one view, three scopes.

use crossterm::event::KeyCode;
use obelus_app::{
    app::{App, dispatch},
    event::Event,
};
use obelus_command::Command;
use obelus_search::Hit;

use crate::support;

/// Each key opens a search of its own. The document in front is searched
/// from a list on the status bar, with the document above it as its
/// preview; the project covers the screen, because its rows are in files
/// nobody has open and the pane under them is the only place to read those.
///
/// Broken deliberately by opening the search of the file full-area again
/// in `open_search`: it covered the file it was searching.
#[test]
fn each_key_opens_a_search_of_its_own() {
    use obelus_component::picker::PickerLayout;

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 60, 16);

    support::press_function(&mut app, 5);
    let picker = app.picker().expect("the search");
    assert!(picker.is_searching(), "not marked as a search");
    assert!(
        matches!(picker.layout(), PickerLayout::Compact { .. }),
        "the search of the file covered the file"
    );
    assert!(picker.tabs().is_empty(), "{:?}", picker.tabs());
    support::press(&mut app, KeyCode::Esc);

    support::press_function(&mut app, 6);
    let picker = app.picker().expect("the search");
    assert!(picker.is_searching(), "not marked as a search");
    assert!(
        matches!(picker.layout(), PickerLayout::FullArea),
        "the search of the project did not cover the screen"
    );
    assert!(picker.tabs().is_empty(), "{:?}", picker.tabs());
    support::press(&mut app, KeyCode::Esc);
}

/// With nothing typed yet, the file being read is still on screen, at the
/// place it is being read: the search of it sits on the status bar, and the
/// file above it is its preview.
#[test]
fn an_empty_search_previews_the_file_being_read() {
    let mut app = App::new(vec![support::open_fixture("many_lines.rs")]);
    support::lay_out(&mut app, 60, 30);
    // Somewhere down the file, so the top of it is not the answer to every
    // question about where the reader is.
    for _ in 0..24 {
        support::press(&mut app, KeyCode::Down);
    }
    let reading = support::render(&mut app, 60, 30);
    assert!(
        support::text_block(&reading).contains("LINE_24"),
        "the cursor is not down the file:\n{reading}"
    );

    support::press_function(&mut app, 5);
    let searching = support::render(&mut app, 60, 30);
    let text = support::text_block(&searching);
    assert!(
        text.contains("Type to search this file"),
        "not the empty search:\n{searching}"
    );
    assert!(
        text.contains("LINE_24"),
        "the search is showing nothing where the file was:\n{searching}"
    );

    // And typing scrolls the file to the row the list is on.
    support::type_text(&mut app, "LINE_03");
    let narrowed = support::render(&mut app, 60, 30);
    assert!(
        support::text_block(&narrowed).contains("LINE_03"),
        "the rows did not take the preview over:\n{narrowed}"
    );
}

/// The file's rows are the lines that have the query in them, found the way
/// a search finds things: literally.
///
/// Broken deliberately by listing every line of the file and letting the
/// picker's fuzzy matcher narrow them. Searching `ac` found `abc` -- an
/// answer about what a line resembles, in a view whose question is where a
/// string is.
#[test]
fn the_file_scope_finds_the_query_and_not_something_like_it() {
    let scratch = temporary("file-literal");
    let path = scratch.write("x.rs", "let abc = 1;\nlet nope = 2;\nlet abc_again = 3;\n");
    let mut app = App::new(vec![obelus_buffer::Buffer::open(&path).expect("the file")]);
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 5);

    // Nothing typed, nothing listed: the reader is looking at the file
    // already, and a list of every line of it says nothing they cannot see.
    let picker = app.picker().expect("the search");
    assert_eq!(picker.match_count(), 0, "the file was listed unasked");
    assert_eq!(picker.nothing_to_show(), Some("Type to search this file"));

    // The lines that have it, and only those.
    support::type_text(&mut app, "abc");
    let picker = app.picker().expect("the search");
    assert_eq!(picker.match_count(), 2, "not the two lines with `abc` in");
    assert!(
        picker.matches().all(|item| item.label.contains("abc")),
        "a line without the query is in the list"
    );

    // And `ac` is not in `abc`.
    for _ in 0.."abc".len() {
        support::press(&mut app, KeyCode::Backspace);
    }
    support::type_text(&mut app, "ac");
    let picker = app.picker().expect("the search");
    assert_eq!(
        picker.match_count(),
        0,
        "`ac` found a line that only resembles it"
    );
    assert_eq!(picker.nothing_to_show(), Some("No match in this file"));

    // Emptied again, the rows go away rather than becoming the file.
    for _ in 0.."ac".len() {
        support::press(&mut app, KeyCode::Backspace);
    }
    let picker = app.picker().expect("the search");
    assert_eq!(picker.row_count(), 0, "the file came back as a list");
    assert_eq!(picker.nothing_to_show(), Some("Type to search this file"));
}

/// The search's three switches: a pattern, a whole word, the capitals as
/// typed. The list knows which way each is set.
#[test]
fn the_three_switches_change_what_the_search_finds() {
    let scratch = temporary("switches");
    let path = scratch.write("x.rs", "let abc = 1;\nlet ABC = 2;\nlet abcdef = 3;\n");
    let mut app = App::new(vec![obelus_buffer::Buffer::open(&path).expect("the file")]);
    support::lay_out(&mut app, 74, 16);
    support::press_function(&mut app, 5);
    support::type_text(&mut app, "abc");

    let found = |app: &App| app.picker().expect("the search").match_count();
    // Smart case to start with: a query in lower case matches either.
    assert_eq!(found(&app), 3, "smart case did not match the capitals");

    // Only where it stands as a word: `abcdef` is not one.
    support::press_alt_key(&mut app, KeyCode::Char('w'));
    assert_eq!(found(&app), 2, "the whole-word switch did nothing");
    support::press_alt_key(&mut app, KeyCode::Char('w'));

    // The capitals as typed, which smart case had no way to be told.
    support::press_alt_key(&mut app, KeyCode::Char('c'));
    assert_eq!(found(&app), 2, "the case switch did nothing");
    support::press_alt_key(&mut app, KeyCode::Char('c'));

    // A pattern rather than the text.
    support::press_alt_key(&mut app, KeyCode::Char('r'));
    for _ in 0.."abc".len() {
        support::press(&mut app, KeyCode::Backspace);
    }
    support::type_text(&mut app, "ab.d");
    assert_eq!(found(&app), 1, "the query was not read as a pattern");

    // And the list knows which way each is set, which is what the row the
    // query is typed into says -- the palette lets go of a list on the
    // status bar rather than opening over it.
    assert_eq!(
        app.picker().and_then(|picker| picker.looks_how()),
        Some(obelus_search::Looking {
            regex: true,
            word: false,
            sensitive: false,
        }),
        "the list does not know which way the switches are set"
    );

    // Half a pattern is not an error in front of the reader: it is a
    // question that is not finished, and it matches nothing.
    support::type_text(&mut app, "(");
    let picker = app.picker().expect("the search");
    assert_eq!(picker.match_count(), 0);
    assert_eq!(picker.nothing_to_show(), Some("That is not a pattern"));
}

/// Which switches mean anything depends on the search: three of them read
/// a text, and the fourth is about how far an index reaches.
///
/// A search only offers what it can answer. The file and the project are
/// inside this tree by construction, so there is nothing for "Outside" to
/// do; the symbols come from a server that did its own matching and has
/// never heard of our pattern.
#[test]
fn a_search_offers_only_the_switches_it_can_answer() {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 84, 16);
    support::press_function(&mut app, 5);

    let picker = app.picker().expect("the search");
    assert!(
        picker.looks_how().is_some(),
        "the search of the file says reading a text means nothing to it"
    );
    assert!(
        picker.reaches_outside().is_none(),
        "the search of the file offers to reach past a tree it is inside"
    );
    support::press(&mut app, KeyCode::Esc);

    // The project's says so in the palette, which opens over it.
    support::press_function(&mut app, 6);
    let offered = support::usable_here(&mut app);
    assert_eq!(
        &offered[..3],
        ["Regex", "Word", "Case"],
        "the palette offers a key that would do nothing, or not one that would"
    );
    assert!(!offered.contains(&"Outside".to_string()), "{offered:?}");
}

/// A symbol outside the project is left out unless the reader asks for it.
///
/// A server that has indexed a project has indexed what it was built on
/// too, so a search for a common name answered from the whole index is the
/// registry's answer with the reader's own names somewhere in it. It was a
/// law; it is a switch now, because going to a name in a dependency by
/// searching for it is a real thing to want.
#[test]
fn the_symbols_of_the_project_or_of_everything() {
    use serde_json::json;

    let reply = json!([
        { "name": "mine", "kind": 12,
          "location": { "uri": support::fake_uri("/p/src/main.rs"),
                        "range": { "start": { "line": 1, "character": 3 },
                                   "end": { "line": 1, "character": 7 } } } },
        { "name": "theirs", "kind": 12,
          "location": { "uri": support::fake_uri("/home/reader/.cargo/registry/serde/lib.rs"),
                        "range": { "start": { "line": 2, "character": 0 },
                                   "end": { "line": 2, "character": 6 } } } },
    ]);
    let root = support::fake_path("/p");

    let names = |within: Option<&std::path::Path>| -> Vec<String> {
        obelus_lsp::outline::found_in(Ok(reply.clone()), within)
            .iter()
            .map(|found| found.name.clone())
            .collect()
    };
    assert_eq!(names(Some(&root)), ["mine"]);
    assert_eq!(names(None), ["mine", "theirs"]);
}

/// What the project ignores, the search ignores -- unless the reader has asked
/// for the ignored files, and then it does not.
///
/// Broken deliberately by walking with the ignore rules always on: the file
/// list would offer `target/build.log` and the search could not see a word
/// in it, which is two answers about one tree.
#[test]
fn the_search_reaches_what_the_file_list_offers() {
    let scratch = temporary("ignored");
    scratch.write("kept.rs", "the needle is here\n");
    scratch.write("target/build.log", "the needle is here too\n");
    scratch.write(".ignore", "target/\n");

    let found = |ignored: bool, generation: u64| -> Vec<String> {
        let (sender, events) = std::sync::mpsc::channel();
        let latest = obelus_runtime::cancel::Latest::default();
        for _ in 0..generation {
            latest.next();
        }
        obelus_search::spawn_scan(
            scratch.path(),
            &obelus_search::Needle::new("needle", obelus_search::Looking::default()),
            latest.claim(generation),
            ignored,
            false,
            sender,
        );
        let mut names = Vec::new();
        while let Ok(Event::Search(obelus_search::Event::Matches { hits, done, .. })) =
            events.recv()
        {
            names.extend(hits.into_iter().map(|hit| hit.path.display().to_string()));
            if done {
                break;
            }
        }
        names.sort();
        names
    };

    assert_eq!(found(false, 1), vec!["kept.rs"]);
    assert_eq!(
        found(true, 2),
        vec!["kept.rs".to_string(), support::as_shown("target/build.log")]
    );
}

/// The rows are the lines of one version of one file, so a file rewritten
/// while the search is open is listed as it now is. An agent editing the
/// file under the reader is the ordinary case here, not an exotic one.
#[test]
fn the_rows_follow_the_file_when_it_changes() {
    let scratch = temporary("reload");
    let path = scratch.path().join("f.rs");
    std::fs::write(&path, "fn alpha() {}\n").expect("writing");
    let mut app = App::new(vec![obelus_buffer::Buffer::open(&path).expect("opening")]);
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 5);
    support::type_text(&mut app, "n");
    assert_eq!(
        app.picker()
            .expect("the search")
            .matches()
            .next()
            .expect("a row")
            .label,
        "fn alpha() {}"
    );

    std::fs::write(&path, "fn beta() {}\nfn gamma() {}\n").expect("writing");
    dispatch::dispatch(&mut app, Command::FileReload);
    // No keystroke: nothing the reader did changed the file, so the list
    // must not wait for them to touch it before telling the truth.
    let _ = support::render(&mut app, 60, 16);
    let labels: Vec<String> = app
        .picker()
        .expect("the search")
        .matches()
        .map(|item| item.label.clone())
        .collect();
    assert!(
        labels.iter().any(|label| label == "fn beta() {}"),
        "the search is still listing the file as it was: {labels:?}"
    );
    assert!(
        !labels.iter().any(|label| label == "fn alpha() {}"),
        "a line that is gone is still a row: {labels:?}"
    );
}

/// A search result is a line of code, so it is coloured like one -- in the
/// file's own colours, from the file's own tree. Both scopes: the file being
/// read is already parsed, and a hit from elsewhere in the project is worth
/// parsing that file for, once, for as long as the list is open.
#[test]
fn the_rows_are_coloured_like_the_code_they_are() {
    /// The theme's colour for a kind, as the dump writes colours.
    fn hex(colour: ratatui::style::Color) -> String {
        match colour {
            ratatui::style::Color::Rgb(red, green, blue) => {
                format!("#{red:02x}{green:02x}{blue:02x}")
            }
            other => panic!("the built-in themes are RGB, not {other:?}"),
        }
    }

    /// The foreground of one cell of the row holding `needle`, at the column
    /// where `at` starts in it.
    ///
    /// Cell by cell rather than by looking for the colour anywhere in the
    /// legend: the preview under the list is the whole file in its own
    /// colours, so a legend that holds the keyword colour says nothing about
    /// whether the *row* used it.
    fn cell(dump: &str, needle: &str, at: &str) -> String {
        let row = support::text_block(dump)
            .lines()
            .position(|row| row.contains(needle))
            .unwrap_or_else(|| panic!("no row holding {needle:?}:\n{dump}"));
        let text = support::text_block(dump)
            .lines()
            .nth(row)
            .expect("the row")
            .to_string();
        let column = text
            .find(at)
            .unwrap_or_else(|| panic!("no {at:?} on the row:\n{dump}"));
        let letter = support::style_block(dump)
            .lines()
            .nth(row)
            .and_then(|styles| styles.chars().nth(column))
            .expect("a style cell");
        support::legend_block(dump)
            .lines()
            .find(|line| line.trim_start().starts_with(letter))
            .and_then(|line| line.split_whitespace().nth(1))
            .map(str::to_string)
            .unwrap_or_default()
    }

    // The file scope, from the buffer Obelus already has parsed. The
    // fixture's line is indented with a tab, which the label does not carry:
    // a run counted from the start of the *line* would colour the wrong
    // characters of the row.
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 60, 16);
    let keyword = hex(app.theme().syntax.keyword);
    let string = hex(app.theme().syntax.string);
    support::press_function(&mut app, 5);
    support::type_text(&mut app, "greeting");
    let dump = support::render(&mut app, 60, 16);
    assert_eq!(
        cell(&dump, "let greeting", "let"),
        format!("fg={keyword}"),
        "`let` is not in the keyword colour:\n{dump}"
    );
    assert_eq!(
        cell(&dump, "let greeting", "\""),
        format!("fg={string}"),
        "the string literal is not in the string colour:\n{dump}"
    );

    // And the project scope, whose hits come from files nothing has opened:
    // this one names the fixture by its path under the working directory.
    let mut app = App::new(Vec::new());
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 6);
    support::type_text(&mut app, "greeting");
    app.handle(Event::Search(obelus_search::Event::Matches {
        generation: app.search_generation(),
        hits: vec![Hit {
            path: std::path::PathBuf::from("tests/fixtures/sample.rs"),
            line: 1,
            text: "let greeting = \"\u{4f60}\u{597d}\";".to_string(),
        }],
        done: true,
    }));
    let dump = support::render(&mut app, 60, 16);
    assert_eq!(
        cell(&dump, "let greeting", "let"),
        format!("fg={keyword}"),
        "a hit from a file nothing had opened was not coloured:\n{dump}"
    );
}

/// Only the rows on screen are coloured, so a project search of two
/// thousand lines does not parse two thousand files to draw ten of them.
#[test]
fn only_the_rows_on_screen_cost_anything() {
    // Real files, because colouring a row means parsing the file it is a
    // line of. Every `.rs` file in the tree, which is a few hundred.
    let mut paths = Vec::new();
    let tree = std::path::Path::new(env!("OBELUS_TREE"));
    for directory in [
        "crates/obelus-app/src/app",
        "crates/obelus-ui/src",
        "crates/obelus-lsp/src",
        "crates/obelus-syntax/src",
        "crates/obelus-theme/src",
        "crates/obelus-git/src",
    ] {
        for entry in std::fs::read_dir(tree.join(directory)).expect("reading the tree") {
            let path = entry.expect("an entry").path();
            if path.extension().is_some_and(|kind| kind == "rs") {
                paths.push(path);
            }
        }
    }
    assert!(paths.len() > 30, "not enough files to tell anything apart");

    let mut app = App::new(Vec::new());
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 6);
    support::type_text(&mut app, "use");
    app.handle(Event::Search(obelus_search::Event::Matches {
        generation: app.search_generation(),
        hits: paths
            .iter()
            .map(|path| Hit {
                path: path.clone(),
                line: 0,
                text: "use std::path::Path;".to_string(),
            })
            .collect(),
        done: true,
    }));
    let dump = support::render(&mut app, 60, 16);
    let visible = support::text_block(&dump)
        .lines()
        .filter(|row| row.contains("use std"))
        .count();
    assert!(visible > 0, "no rows on screen:\n{dump}");
    // Sixteen is the screen this test asked for; the list holds hundreds.
    // The bound is the screen rather than the exact row count because the
    // list's region is a row or two taller than the rows with text in them.
    assert!(
        app.files_parsed_for_rows() <= 16,
        "{} files parsed for a sixteen-row screen holding {} rows",
        app.files_parsed_for_rows(),
        paths.len()
    );
    assert!(
        app.files_parsed_for_rows() > 0,
        "no file was parsed, so nothing was coloured"
    );

    // And the files are let go when the list closes: this is a cache for one
    // list's lifetime, not a second set of buffers.
    support::press(&mut app, KeyCode::Esc);
    let _ = support::render(&mut app, 60, 16);
    assert_eq!(
        app.files_parsed_for_rows(),
        0,
        "the parsed files outlived the list"
    );
}

/// The query goes from one search to the next: a reader who does not find
/// it in this file looks in the project without retyping it, and back.
///
/// Broken deliberately by not keeping the search as a list is left
/// (`keep_the_search` out of `leave`), which is the way one search gives
/// way to another: the project's search opened empty.
#[test]
fn the_query_goes_from_one_search_to_the_next() {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 5);
    support::type_text(&mut app, "greeting");

    support::press_function(&mut app, 6);
    let picker = app.picker().expect("the search");
    assert!(
        matches!(
            picker.layout(),
            obelus_component::picker::PickerLayout::FullArea
        ),
        "f6 did not open the search of the project"
    );
    assert_eq!(picker.query(), "greeting", "the query did not come along");

    // Nothing has come back yet, so the list says what it is doing rather
    // than "no match" -- with a query typed and nothing found, "still
    // looking" and "not there" are different facts.
    assert_eq!(picker.nothing_to_show(), Some("Searching\u{2026}"));

    // And back again, onto the file's own lines, still narrowed.
    support::press_function(&mut app, 5);
    let picker = app.picker().expect("the search");
    assert_eq!(picker.query(), "greeting");
    assert_eq!(picker.match_count(), 2, "the file's rows did not come back");
}

/// One letter is a real question, and the cheapest one there is: it fills
/// the row limit in the first few files and stops. Only the empty query is
/// not a search -- it matches every line of every file, which is the tree
/// rather than an answer.
#[test]
fn one_letter_is_a_search_and_nothing_is_not() {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 6);

    assert_eq!(
        app.picker().expect("the search").nothing_to_show(),
        Some("Type to search every file"),
        "an empty query started a search"
    );

    support::type_text(&mut app, "g");
    assert_eq!(
        app.picker().expect("the search").nothing_to_show(),
        Some("Searching\u{2026}"),
        "one letter did not start a search"
    );

    // And back to nothing typed: whatever was found is cleared, because the
    // rows for "g" are not an answer to no question at all.
    support::press(&mut app, KeyCode::Backspace);
    let picker = app.picker().expect("the search");
    assert_eq!(picker.match_count(), 0);
    assert_eq!(picker.nothing_to_show(), Some("Type to search every file"));
}

/// A scan the reader has typed past stops instead of reading the rest of the
/// tree. Typing a ten-letter word starts ten scans, and nine of them are
/// answering a question nobody is asking any more; a walk cannot be
/// interrupted from outside, so the thread reads the generation itself.
#[test]
fn a_scan_that_has_been_typed_past_stops() {
    let scratch = temporary("cancel");
    for file in 0..40 {
        std::fs::write(
            scratch.path().join(format!("f{file}.rs")),
            "fn needle() {}\n",
        )
        .expect("writing");
    }

    // Stale before it starts, which is the same state a scan reaches when
    // the reader types another letter: the first file it looks at is enough
    // to find that out.
    let latest = obelus_runtime::cancel::Latest::default();
    for _ in 0..9 {
        latest.next();
    }
    let (sender, events) = obelus_app::event::channel();
    obelus_search::spawn_scan(
        scratch.path(),
        &obelus_search::Needle::new("needle", obelus_search::Looking::default()),
        latest.claim(4),
        false,
        false,
        sender,
    );

    // The thread owns the only sender, so its return closes the channel.
    // Nothing at all comes through: not even the batch that says it is
    // finished, because a scan nobody is waiting for has nothing to report.
    match events.recv_timeout(std::time::Duration::from_secs(10)) {
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {}
        Ok(event) => panic!("a stale scan reported something: {event:?}"),
        Err(error) => panic!("a stale scan did not stop: {error}"),
    }

    // While the generation it was started under does run to the end.
    let latest = obelus_runtime::cancel::Latest::default();
    for _ in 0..4 {
        latest.next();
    }
    let (sender, events) = obelus_app::event::channel();
    obelus_search::spawn_scan(
        scratch.path(),
        &obelus_search::Needle::new("needle", obelus_search::Looking::default()),
        latest.claim(4),
        false,
        false,
        sender,
    );
    let mut found = 0;
    loop {
        match events.recv_timeout(std::time::Duration::from_secs(10)) {
            Ok(Event::Search(obelus_search::Event::Matches { hits, done, .. })) => {
                found += hits.len();
                if done {
                    break;
                }
            }
            Ok(_) => {}
            Err(error) => panic!("the scan did not finish: {error}"),
        }
    }
    assert_eq!(found, 40, "the current scan stopped early");
}

/// Rows arriving for a query the reader has already typed past are dropped.
/// A tree takes longer to walk than a word takes to type, so without this
/// the list would fill with answers to something nobody asked.
#[test]
fn matches_for_an_older_query_are_dropped() {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 6);
    support::type_text(&mut app, "greeting");

    // Text the query matches, because the picker filters what the scan
    // hands it: the scan matches a substring and the picker scores the same
    // query against the row, which is what highlights it.
    let hit = Hit {
        path: std::path::PathBuf::from("elsewhere.rs"),
        line: 3,
        text: "let greeting = elsewhere();".to_string(),
    };
    app.handle(Event::Search(obelus_search::Event::Matches {
        generation: app.search_generation() - 1,
        hits: vec![hit.clone()],
        done: true,
    }));
    assert_eq!(
        app.picker().expect("the search").match_count(),
        0,
        "a stale answer landed in the list"
    );

    app.handle(Event::Search(obelus_search::Event::Matches {
        generation: app.search_generation(),
        hits: vec![hit],
        done: false,
    }));
    let picker = app.picker().expect("the search");
    assert_eq!(picker.match_count(), 1, "the current answer was dropped");
    let row = picker.matches().next().expect("a row");
    assert_eq!(row.label, "let greeting = elsewhere();");
    assert_eq!(
        row.trailing.as_deref(),
        Some("elsewhere.rs:4"),
        "a row does not say which file and line it is"
    );
}

/// Rows for a scope the reader has left are dropped, and so are rows that
/// arrive with a different list on screen. A walk keeps running after the
/// reader has moved on -- there is nothing to cancel a thread with -- so the
/// batches have to land only where they were asked for.
#[test]
fn matches_do_not_land_in_a_list_that_did_not_ask() {
    let hit = Hit {
        path: std::path::PathBuf::from("elsewhere.rs"),
        line: 0,
        text: "let greeting = elsewhere();".to_string(),
    };

    // Asked for in the project, and then the reader goes to the search of
    // the file -- which asks the same thing, of the file.
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 6);
    support::type_text(&mut app, "greeting");
    let generation = app.search_generation();
    support::press_function(&mut app, 5);
    app.handle(Event::Search(obelus_search::Event::Matches {
        generation,
        hits: vec![hit.clone()],
        done: false,
    }));
    let labels: Vec<String> = app
        .picker()
        .expect("the search")
        .matches()
        .map(|item| item.label.clone())
        .collect();
    assert!(
        !labels.iter().any(|label| label == &hit.text),
        "a project row landed in the file's own lines: {labels:?}"
    );

    // And with the search closed and another list open in its place.
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 6);
    support::type_text(&mut app, "greeting");
    let generation = app.search_generation();
    support::press(&mut app, KeyCode::Esc);
    support::press_control(&mut app, 'p');
    app.handle(Event::Search(obelus_search::Event::Matches {
        generation,
        hits: vec![hit.clone()],
        done: false,
    }));
    assert!(
        app.picker()
            .expect("the palette")
            .matches()
            .all(|item| item.label != hit.text),
        "a project row landed in the command palette"
    );
}

/// With the walk finished and nothing found, "No match" is finally a true
/// thing to say, and the reason that was standing in for it goes away.
#[test]
fn a_finished_walk_that_found_nothing_says_so() {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 6);
    support::type_text(&mut app, "greeting");

    app.handle(Event::Search(obelus_search::Event::Matches {
        generation: app.search_generation(),
        hits: Vec::new(),
        done: true,
    }));
    assert_eq!(
        app.picker().expect("the search").nothing_to_show(),
        Some("No match in the project")
    );
}

/// The symbols come from a server, and with no server there is nobody to
/// ask -- so there is no tab for them, and the key that would land on it
/// does nothing at all.
#[test]
fn the_symbols_scope_needs_a_server() {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 8);
    assert!(
        app.picker().is_none(),
        "a search opened on a scope with nobody to ask"
    );
    assert_eq!(app.note(), None, "a key that did nothing said so");

    // And the palette shows it dim -- findable, so a reader learns Obelus
    // can do it, and not choosable, because right now it cannot.
    support::press_control(&mut app, 'p');
    let rows: Vec<(String, bool)> = app
        .picker()
        .expect("the palette")
        .matches()
        .map(|item| (item.label.clone(), item.enabled))
        .collect();
    let listed = |name: &str| {
        rows.iter()
            .find(|(label, _)| label == name)
            .map(|(_, enabled)| *enabled)
    };
    assert_eq!(
        listed("search-project"),
        Some(true),
        "the project search needs nothing and cannot be chosen: {rows:?}"
    );
    assert_eq!(
        listed("search-symbols"),
        Some(false),
        "choosable with no server to ask: {rows:?}"
    );
}

/// With nothing open there is nothing to search in a file, and the command
/// run by name says so rather than opening a view whose one useful tab is
/// somewhere else.
#[test]
fn searching_a_file_needs_a_file() {
    let mut app = App::new(Vec::new());
    support::lay_out(&mut app, 60, 16);
    dispatch::dispatch(&mut app, Command::SearchFile);
    assert!(
        app.picker().is_none(),
        "a search opened with nothing to search"
    );
    assert_eq!(app.note(), Some("Nothing here to search"));
}

/// A file shown as its reading is searched as it is drawn, and choosing a
/// row puts the reading's cursor on the match and the match on screen.
///
/// The words asked for are the reading's and not the file's: the source
/// has `**` in the middle of them, so a search of the lines finds nothing.
/// Broken deliberately two ways. Answering `Here::File` for a reading in
/// `searched_here`: the lines were searched and the list was empty. And
/// leaving the reading's view where it was in `go_within`: the cursor
/// stood on a row below the screen, and no caret was drawn on the match.
#[test]
fn a_reading_is_searched_as_it_is_drawn() {
    let path = std::path::Path::new("found.md");
    let mut source: String = (0..40)
        .map(|at| format!("Paragraph {at} says nothing much.\n\n"))
        .collect();
    source.push_str("The **pelican** wings it.\n");
    let mut app = App::new(vec![obelus_buffer::Buffer::from_text(path, &source)]);
    support::lay_out(&mut app, 60, 12);
    dispatch::dispatch(&mut app, Command::PreviewToggle);
    support::lay_out(&mut app, 60, 12);

    support::press_function(&mut app, 5);
    support::type_text(&mut app, "pelican wings");
    let rows: Vec<String> = app
        .picker()
        .expect("the search")
        .matches()
        .map(|item| item.label.clone())
        .collect();
    assert_eq!(
        rows,
        vec!["The pelican wings it.".to_string()],
        "the reading was not what was searched"
    );
    support::press(&mut app, KeyCode::Enter);
    assert!(app.picker().is_none(), "the search is still open");
    assert_eq!(
        support::under_the_caret(&mut app, 60, 12, 7).as_deref(),
        Some("pelican"),
        "the caret is not on the match:\n{}",
        support::render(&mut app, 60, 12)
    );
}

/// The scan itself, against a real tree: what it finds, what it skips, and
/// that it says when it is finished.
#[test]
fn the_scan_finds_lines_and_finishes() {
    let scratch = temporary("scan");
    // A repository, because `.gitignore` is a git file: `ignore` applies it
    // where git would, and a bare directory is not somewhere git would.
    support::git()
        .arg("-C")
        .arg(scratch.path())
        .args(["init", "--quiet"])
        .output()
        .expect("git init");
    std::fs::write(
        scratch.path().join("one.rs"),
        "fn alpha() {}\nfn beta() {}\n",
    )
    .expect("writing");
    std::fs::write(scratch.path().join("two.rs"), "// beta again\n").expect("writing");
    // A NUL is a binary file, and a reader searching for a word is not
    // searching those -- the same judgement opening one makes.
    std::fs::write(
        scratch.path().join("blob.bin"),
        [0, 0xfe, b'b', b'e', b't', b'a'],
    )
    .expect("writing");
    std::fs::write(scratch.path().join(".gitignore"), "ignored/\n").expect("writing");
    std::fs::create_dir(scratch.path().join("ignored")).expect("a directory");
    std::fs::write(
        scratch.path().join("ignored/three.rs"),
        "fn beta_ignored() {}\n",
    )
    .expect("writing");

    let hits = scan(scratch.path(), "beta");
    let mut found: Vec<String> = hits
        .iter()
        .map(|hit| format!("{}:{}:{}", hit.path.display(), hit.line, hit.text))
        .collect();
    found.sort();
    assert_eq!(
        found,
        ["one.rs:1:fn beta() {}", "two.rs:0:// beta again"],
        "not the lines a search of this tree should find"
    );
}

/// Smart case, the same rule the picker's matcher follows: a query in lower
/// case matches either case, and a query with a capital in it means it.
#[test]
fn the_scan_takes_a_capital_seriously() {
    let scratch = temporary("case");
    std::fs::write(
        scratch.path().join("f.rs"),
        "let alpha = 1;\nlet Alpha = 2;\n",
    )
    .expect("writing");

    assert_eq!(
        scan(scratch.path(), "alpha").len(),
        2,
        "lower case did not match both"
    );
    let strict = scan(scratch.path(), "Alpha");
    assert_eq!(strict.len(), 1, "a capital matched either case");
    assert_eq!(strict[0].line, 1);
}

/// A line no row can show is trimmed to something one can, and a file too
/// big to be written by hand is not read at all: a minified bundle is
/// megabytes of one line nobody is searching.
#[test]
fn the_scan_trims_what_a_row_cannot_show() {
    let scratch = temporary("wide");
    let long = format!("let x = \"{}needle\";", "a".repeat(600));
    std::fs::write(scratch.path().join("wide.rs"), format!("{long}\n")).expect("writing");
    std::fs::write(
        scratch.path().join("huge.rs"),
        format!("// needle\n{}", "x".repeat(3 * 1024 * 1024)),
    )
    .expect("writing");

    let hits = scan(scratch.path(), "let x");
    assert_eq!(hits.len(), 1, "the wide line was not found");
    assert!(
        hits[0].text.chars().count() <= 300,
        "a row would be {} characters wide",
        hits[0].text.chars().count()
    );
    assert!(
        scan(scratch.path(), "needle")
            .iter()
            .all(|hit| hit.path != std::path::Path::new("huge.rs")),
        "a three-megabyte file was read"
    );
}

/// A file in another encoding is searched as the text opening it shows, on
/// the lines it shows it on: a query in Chinese finds the line of a file in
/// GBK, and a file in UTF-16 is found at all.
///
/// Broken by searching the bytes as they are on disk rather than as
/// `obelus_encoding` reads them: neither file is found.
#[test]
fn the_scan_reads_a_file_the_way_opening_it_does() {
    let scratch = temporary("encodings");
    let (gbk, _, _) = encoding_rs::GBK.encode("// 第一行\nfn main() {} // 这是中文注释\n");
    std::fs::write(scratch.path().join("gbk.rs"), &*gbk).expect("writing");
    let wide: Vec<u8> = [0xFF, 0xFE]
        .into_iter()
        .chain(
            "first\nsecond 这是中文注释\n"
                .encode_utf16()
                .flat_map(u16::to_le_bytes),
        )
        .collect();
    std::fs::write(scratch.path().join("wide.txt"), wide).expect("writing");

    let mut found: Vec<String> = scan(scratch.path(), "中文注释")
        .iter()
        .map(|hit| format!("{}:{}:{}", hit.path.display(), hit.line, hit.text))
        .collect();
    found.sort();
    assert_eq!(
        found,
        [
            "gbk.rs:1:fn main() {} // 这是中文注释",
            "wide.txt:1:second 这是中文注释"
        ],
        "not the lines the files say"
    );
}

/// `^` and `$` are a line's ends in the walk, as they are in the search of
/// the file being read -- on every line of a file, and before a `\r\n`.
///
/// Broken by taking `multi_line` off the scan's matcher: they are the
/// file's ends, and the lines in the middle are not found.
#[test]
fn a_pattern_s_anchors_are_each_line_s_ends() {
    let scratch = temporary("anchors");
    std::fs::write(
        scratch.path().join("lf.rs"),
        "let a;\nfn one() {}\nlet b;\nfn two() {}\n",
    )
    .expect("writing");
    std::fs::write(
        scratch.path().join("crlf.rs"),
        "let c;\r\nfn three() {}\r\nlet d\r\n",
    )
    .expect("writing");
    let how = obelus_search::Looking {
        regex: true,
        ..Default::default()
    };
    let lines = |query: &str| {
        let mut found: Vec<String> = scan_with(scratch.path(), query, how)
            .iter()
            .map(|hit| format!("{}:{}", hit.path.display(), hit.line))
            .collect();
        found.sort();
        found
    };
    assert_eq!(
        lines("^fn"),
        ["crlf.rs:1", "lf.rs:1", "lf.rs:3"],
        "a line that starts with fn was missed"
    );
    assert_eq!(
        lines(";$"),
        ["crlf.rs:0", "lf.rs:0", "lf.rs:2"],
        "a line that ends with ; was missed"
    );
}

/// A pattern with a newline in it is not one a line can hold, and says so
/// rather than compiling and finding nothing.
///
/// Broken by building the scan's matcher with no line terminator and
/// without `crlf`, which sets one of its own: it compiles, and the needle is
/// not broken. Taking the terminator away alone breaks nothing.
#[test]
fn a_pattern_no_line_can_hold_is_not_a_pattern() {
    let needle = obelus_search::Needle::new(
        r"a\nb",
        obelus_search::Looking {
            regex: true,
            ..Default::default()
        },
    );
    assert!(needle.is_broken(), "a newline in a pattern was taken");
}

/// Runs a scan to completion and returns everything it found.
fn scan(root: &std::path::Path, query: &str) -> Vec<Hit> {
    scan_with(root, query, obelus_search::Looking::default())
}

/// The same, looking the way the switches say.
fn scan_with(root: &std::path::Path, query: &str, how: obelus_search::Looking) -> Vec<Hit> {
    let latest = obelus_runtime::cancel::Latest::default();
    for _ in 0..7 {
        latest.next();
    }
    let (sender, events) = obelus_app::event::channel();
    obelus_search::spawn_scan(
        root,
        &obelus_search::Needle::new(query, how),
        latest.claim(7),
        false,
        false,
        sender,
    );
    let mut hits = Vec::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        match events.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(Event::Search(obelus_search::Event::Matches {
                generation,
                hits: batch,
                done,
            })) => {
                assert_eq!(generation, 7, "a batch from another search");
                hits.extend(batch);
                if done {
                    return hits;
                }
            }
            Ok(_) => {}
            Err(error) => panic!("the scan never finished: {error}"),
        }
    }
    panic!("the scan never finished")
}

/// A directory of its own for one test, emptied first so a rerun is clean.
fn temporary(name: &str) -> support::Scratch {
    support::Scratch::new(&format!("search-{name}"))
}

/// The preview marks the characters the query matched, and nothing else.
///
/// It used to mark the whole line: a row of the file scope carried the
/// line's own span -- column zero to the end of it -- and the preview
/// marked what the row said it was about. Which is the line, and a reader
/// who typed three letters is looking for those three letters.
///
/// One run, because that is what a search finds: the mark used to be
/// scattered wherever a fuzzy matcher had landed.
#[test]
fn the_preview_marks_what_the_query_matched() {
    /// The columns of the preview's row for `line` that wear the marked
    /// background.
    fn marked(dump: &str, line: &str) -> Vec<usize> {
        // The file is above the list, so the row wanted is the *first* one
        // holding that text: the list holds it too.
        let rows: Vec<usize> = support::text_block(dump)
            .lines()
            .enumerate()
            .filter(|(_, row)| row.contains(line))
            .map(|(at, _)| at)
            .collect();
        let at = rows
            .first()
            .copied()
            .unwrap_or_else(|| panic!("no preview of {line:?}:\n{dump}"));
        let marked = support::legend_block(dump)
            .lines()
            .filter(|entry| entry.contains("bg=#1e3a5f"))
            .map(|entry| entry.trim_start().chars().next().expect("a letter"))
            .collect::<Vec<char>>();
        support::style_block(dump)
            .lines()
            .nth(at)
            .expect("the styles of the row")
            .chars()
            .enumerate()
            .filter(|(_, letter)| marked.contains(letter))
            .map(|(column, _)| column)
            .collect()
    }

    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    // Tall enough for a preview under the list and the foot under that: the
    // search says what its three switches are set to, and those rows come
    // out of the same region.
    support::lay_out(&mut app, 60, 24);
    support::press_function(&mut app, 5);
    // Three letters that are next to each other in one line of the file:
    // `let greeting = "..."`.
    support::type_text(&mut app, "eti");

    let dump = support::render(&mut app, 60, 24);
    let columns = marked(&dump, "let greeting");
    assert!(
        !columns.is_empty(),
        "the query matched nothing in the preview:\n{dump}"
    );
    assert_eq!(
        columns.len(),
        3,
        "not the three characters that matched:\n{dump}"
    );

    // And they are the characters themselves: the row's text at those
    // columns is what was typed.
    let rows: Vec<String> = support::text_block(&dump)
        .lines()
        .filter(|row| row.contains("let greeting"))
        .map(str::to_string)
        .collect();
    let row = rows.first().expect("the file's row").clone();
    let letters: String = columns
        .iter()
        .filter_map(|column| row.chars().nth(*column))
        .collect();
    assert_eq!(letters, "eti", "the marks are not on what matched:\n{dump}");

    // And a query whose characters are not next to each other is not in the
    // line at all: `lgn` is not in `let greeting`, however many of its
    // letters are. A search asks where a string is.
    for _ in 0.."eti".len() {
        support::press(&mut app, KeyCode::Backspace);
    }
    support::type_text(&mut app, "lgn");
    let picker = app.picker().expect("the search");
    assert_eq!(
        picker.match_count(),
        0,
        "a query whose letters are merely present found a line"
    );
}

/// Choosing a row lands on what the query matched, not on the line.
///
/// The row is a line and the reader typed three letters: the cursor goes to
/// the letters. It used to go to column one, which is the same answer for
/// every row of a list a query has narrowed to one.
#[test]
fn choosing_a_row_lands_on_the_match() {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 60, 20);
    support::press_function(&mut app, 5);
    support::type_text(&mut app, "eti");
    support::press(&mut app, KeyCode::Enter);

    let buffer = app.current_buffer().expect("the file");
    let cursor = buffer.cursor();
    // `    let greeting = "..."`: the second line, and the match is inside
    // the word rather than at the start of the line.
    assert_eq!(cursor.line.get(), 1, "not the line that matched");
    let line = buffer.text().line(cursor.line).to_string();
    let landed: String = line
        .chars()
        .skip(cursor.column.get())
        .take(3)
        .collect::<String>();
    assert_eq!(landed, "eti", "the cursor is not on the match: {line:?}");
}

/// Only a row that *is* a line carries a column of that line.
///
/// The file and project scopes list lines, so the characters a query
/// matched are characters of the code, and both the mark and the cursor go
/// to them. The symbols scope lists *names*: a column of a name means
/// nothing in the file, and using it landed the cursor short of the symbol
/// -- on the `pub` in front of it.
#[test]
fn a_row_that_is_a_name_keeps_the_place_it_was_given() {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 60, 16);

    // No search open: nothing is a line.
    assert!(!app.rows_are_lines_for_test());

    support::press_function(&mut app, 5);
    support::type_text(&mut app, "greet");
    assert!(
        app.rows_are_lines_for_test(),
        "the file scope's rows are the lines it lists"
    );

    // The project scope, which lists lines too.
    support::press(&mut app, KeyCode::Right);
    assert!(
        app.rows_are_lines_for_test(),
        "the project scope's rows are the lines it lists"
    );

    // And with the search closed, nothing again: a list that is not a
    // search lists names or paths.
    support::press(&mut app, KeyCode::Esc);
    assert!(!app.rows_are_lines_for_test());

    // The rule itself, per scope, because the symbols scope needs a server
    // to be reachable through the view -- and it is the one that was wrong.
    use obelus_search::Scope;
    assert!(Scope::File.lists_lines());
    assert!(Scope::Project.lists_lines());
    assert!(
        !Scope::Symbols.lists_lines(),
        "a symbol row is a name, not the line it is on"
    );
}

/// The search opened again is the one left: the same words, held so that
/// typing replaces them, and the same row under the reader.
///
/// Broken deliberately three ways. Not keeping the search as the list is
/// left (`keep_the_search` out of `leave`): it opened empty. Not holding
/// the words (`hold_in_query` out of `open_search`): typing went on the
/// end of them. And not putting the row back: the first row was selected.
#[test]
fn a_file_search_opened_again_is_the_one_left() {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 5);
    support::type_text(&mut app, "greeting");
    support::press(&mut app, KeyCode::Down);
    assert_eq!(app.picker().map(|picker| picker.selected()), Some(1));
    support::press(&mut app, KeyCode::Esc);
    assert!(app.picker().is_none(), "the search did not close");

    support::press_function(&mut app, 5);
    let picker = app.picker().expect("the search");
    assert_eq!(picker.query(), "greeting", "the words were not kept");
    assert_eq!(picker.match_count(), 2, "the rows were not found again");
    assert_eq!(picker.selected(), 1, "the row was not kept");

    support::type_text(&mut app, "main");
    assert_eq!(
        app.picker().map(|picker| picker.query()).as_deref(),
        Some("main"),
        "typing did not replace the words"
    );
}

/// Choosing a row keeps the search too, and the row chosen is the one the
/// search opens on again -- the next match is one key away.
///
/// Broken deliberately by taking `keep_the_search` out of `accept`: the
/// search opened empty.
#[test]
fn a_row_chosen_is_where_the_search_opens_again() {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 5);
    support::type_text(&mut app, "greeting");
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    assert!(app.picker().is_none(), "the search did not close");

    support::press_function(&mut app, 5);
    let picker = app.picker().expect("the search");
    assert_eq!(picker.query(), "greeting");
    assert_eq!(picker.selected(), 1, "the row chosen was not kept");
}

/// The row stood on is a row of one document, and another document's
/// search starts at its top -- with the same words asked of it.
///
/// Broken deliberately by matching what was kept on the scope and the
/// words alone in `open_search`: the second file opened on its second row.
#[test]
fn another_documents_search_starts_at_its_top() {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 5);
    support::type_text(&mut app, "greeting");
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Esc);

    app.open_buffer_for_test(obelus_buffer::Buffer::from_text(
        std::path::Path::new("other.rs"),
        "greeting one\ngreeting two\ngreeting three\n",
    ));
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 5);
    let picker = app.picker().expect("the search");
    assert_eq!(picker.query(), "greeting");
    assert_eq!(picker.match_count(), 3);
    assert_eq!(
        picker.selected(),
        0,
        "another file opened on the first's row"
    );
}

/// The project's rows are kept whole, and the search opened again shows
/// them at once on the row it was left on, without walking the project.
///
/// Broken deliberately by not reusing the rows (`reuse` never set): the
/// search opened walking again, with nothing in it and nothing selected.
#[test]
fn a_project_search_opened_again_keeps_its_rows() {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 6);
    support::type_text(&mut app, "needle");
    let hits = (0..3)
        .map(|line| Hit {
            path: std::path::PathBuf::from(format!("found{line}.rs")),
            line,
            text: format!("the needle {line}"),
        })
        .collect();
    app.handle(Event::Search(obelus_search::Event::Matches {
        generation: app.search_generation(),
        hits,
        done: true,
    }));
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Esc);

    let generation = app.search_generation();
    support::press_function(&mut app, 6);
    let picker = app.picker().expect("the search");
    assert_eq!(picker.query(), "needle");
    let rows: Vec<String> = picker.matches().map(|item| item.label.clone()).collect();
    assert_eq!(rows, ["the needle 0", "the needle 1", "the needle 2"]);
    assert_eq!(picker.selected(), 2, "the row was not kept");
    // And whatever was still walking is told it is answering a question
    // nobody is asking, so none of it lands on the rows put back.
    assert!(app.search_generation() > generation);
}

/// The search of a reading sits on the status bar, and the reading above it
/// is its preview: the row the list is on is scrolled into view before
/// anything is chosen, and escape puts the reading back where it was.
///
/// Broken deliberately three ways. Taking `look_within_at_the_selection`
/// out of the frame: the reading stayed at its top while the list stood on
/// a row far below it. Leaving `found` out of the reading's rows
/// (`Cursor::laid`): nothing was marked. And not taking the mark away as
/// the list goes (`forget_the_find` out of `go_within`): it stayed on the
/// reading after the cursor had gone there.
#[test]
fn a_reading_under_its_search_is_the_preview() {
    let path = std::path::Path::new("looked.md");
    let mut source: String = (0..40)
        .map(|at| format!("Paragraph {at} says nothing much.\n\n"))
        .collect();
    source.push_str("The pelican wings it.\n");
    let mut app = App::new(vec![obelus_buffer::Buffer::from_text(path, &source)]);
    support::lay_out(&mut app, 60, 24);
    dispatch::dispatch(&mut app, Command::PreviewToggle);
    support::lay_out(&mut app, 60, 24);

    support::press_function(&mut app, 5);
    support::type_text(&mut app, "pelican");
    let looking = support::text_block(&support::render(&mut app, 60, 24)).to_string();
    // Above the rule the list starts with, which is the reading.
    let above: Vec<&str> = looking
        .lines()
        .take_while(|row| !row.contains('\u{2500}'))
        .collect();
    assert!(
        above
            .iter()
            .any(|row| row.contains("The pelican wings it.")),
        "the reading was not scrolled to the row:\n{looking}"
    );
    // And what was found is marked on it, as the file marks it.
    let dump = support::render(&mut app, 60, 24);
    assert_eq!(
        support::found_on_screen(&dump, "The pelican wings it."),
        "pelican",
        "the match is not marked on the reading:\n{dump}"
    );

    support::press(&mut app, KeyCode::Esc);
    let back = support::text_block(&support::render(&mut app, 60, 24)).to_string();
    assert!(
        back.contains("Paragraph 0 "),
        "escape did not put the reading back:\n{back}"
    );

    // Chosen, the cursor says where the match is, and the mark goes. A frame
    // between, which is what puts the mark there to go.
    support::press_function(&mut app, 5);
    support::lay_out(&mut app, 60, 24);
    support::press(&mut app, KeyCode::Enter);
    let chosen = support::render(&mut app, 60, 24);
    assert!(
        support::text_block(&chosen).contains("The pelican wings it."),
        "choosing it did not go there:\n{chosen}"
    );
    assert_eq!(
        support::found_on_screen(&chosen, "The pelican wings it."),
        "",
        "the mark outlived the search:\n{chosen}"
    );
}

/// The mark on a page follows the query as it is typed: more letters find
/// the same place and more of it, and the mark grows with them -- and goes
/// back with a backspace.
///
/// Broken deliberately by keying what the page was last marked for on where
/// the match starts alone (`looked_at`): typing on kept the first letters'
/// mark, because the match started in the same place.
#[test]
fn a_page_s_mark_follows_the_query_as_it_is_typed() {
    let path = std::path::Path::new("typed.md");
    let source = "First paragraph.\n\nThe pelican wings it.\n";
    let mut app = App::new(vec![obelus_buffer::Buffer::from_text(path, source)]);
    support::lay_out(&mut app, 60, 24);
    dispatch::dispatch(&mut app, Command::PreviewToggle);
    support::lay_out(&mut app, 60, 24);

    support::press_function(&mut app, 5);
    support::type_text(&mut app, "pel");
    let dump = support::render(&mut app, 60, 24);
    assert_eq!(
        support::found_on_screen(&dump, "The pelican"),
        "pel",
        "{dump}"
    );
    support::type_text(&mut app, "ican");
    let dump = support::render(&mut app, 60, 24);
    assert_eq!(
        support::found_on_screen(&dump, "The pelican"),
        "pelican",
        "the mark did not grow with the query:\n{dump}"
    );
    support::press(&mut app, KeyCode::Backspace);
    let dump = support::render(&mut app, 60, 24);
    assert_eq!(
        support::found_on_screen(&dump, "The pelican"),
        "pelica",
        "the mark did not go back with the query:\n{dump}"
    );
}

/// A page laid out again under its search -- here a reading made narrower,
/// which wraps every paragraph above the match into more rows -- is
/// searched again, so the mark and where enter goes are still the match.
///
/// Broken deliberately by taking `follow_the_page` out of the frame: the
/// mark stayed on the row number it had at the old width, which is other
/// words at the new one.
#[test]
fn a_page_laid_out_again_under_its_search_is_searched_again() {
    let path = std::path::Path::new("narrowed.md");
    let mut source: String = (0..3)
        .map(|at| format!("Paragraph {at} says nothing much at all here.\n\n"))
        .collect();
    source.push_str("The pelican wings it.\n");
    let mut app = App::new(vec![obelus_buffer::Buffer::from_text(path, &source)]);
    support::lay_out(&mut app, 60, 24);
    dispatch::dispatch(&mut app, Command::PreviewToggle);
    support::lay_out(&mut app, 60, 24);

    support::press_function(&mut app, 5);
    support::type_text(&mut app, "pelican");
    let wide = support::render(&mut app, 60, 24);
    assert_eq!(
        support::found_on_screen(&wide, "The pelican"),
        "pelican",
        "{wide}"
    );

    support::lay_out(&mut app, 30, 24);
    let narrow = support::render(&mut app, 30, 24);
    assert_eq!(
        support::found_on_screen(&narrow, "The pelican"),
        "pelican",
        "the mark did not follow the page:\n{narrow}"
    );

    support::press(&mut app, KeyCode::Enter);
    assert_eq!(
        support::under_the_caret(&mut app, 30, 24, 7).as_deref(),
        Some("pelican"),
        "enter did not go to the match:\n{}",
        support::render(&mut app, 30, 24)
    );
}
