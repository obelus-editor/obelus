//! Searching: one view, three scopes.

mod support;

use crossterm::event::KeyCode;
use obelus::{
    app::App,
    command::{Command, dispatch},
    event::Event,
    search::{self, Hit, Scope},
};

/// The three keys open the same view at three tabs. One view because the
/// reader's question is "where is this" and only its radius changes; three
/// keys because the radius they usually mean is worth a key rather than a
/// walk through the tabs.
#[test]
fn each_key_opens_the_same_view_at_its_own_tab() {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 60, 16);

    // A file is open and no language server is running, so those are the
    // two scopes that can answer anything.
    support::press_control(&mut app, 'f');
    let picker = app.picker().expect("the search");
    assert_eq!(
        picker.tabs(),
        ["file", "project"],
        "not the scopes that can answer"
    );
    assert_eq!(picker.tab(), 0, "ctrl+f did not open the file's own tab");
    assert!(picker.is_searching(), "not marked as a search");
    support::press(&mut app, KeyCode::Esc);

    support::press_alt_key(&mut app, KeyCode::Char('f'));
    let picker = app.picker().expect("the search");
    assert_eq!(picker.tab(), 1, "alt+f did not open the project's tab");
    support::press(&mut app, KeyCode::Esc);
}

/// A scope with nothing to answer gets no tab. A tab that says "no file
/// open" whenever it is walked onto is a tab in the way of the ones that
/// work, and the tabs are how a reader moves between the scopes.
#[test]
fn a_scope_with_nothing_to_say_has_no_tab() {
    // Nothing open: the file's own lines are not a scope, and there is no
    // language server either, so the project is all that is left.
    let mut app = App::new(Vec::new());
    support::lay_out(&mut app, 60, 16);
    support::press_alt_key(&mut app, KeyCode::Char('f'));
    let picker = app.picker().expect("the search");
    assert_eq!(picker.tabs(), ["project"], "not just the project");
    assert_eq!(picker.tab(), 0);

    // And the arrows have nowhere to go, which is what one tab means.
    support::press(&mut app, KeyCode::Right);
    assert_eq!(app.picker().expect("the search").tab(), 0);
    support::press(&mut app, KeyCode::Esc);

    // With a file open, its lines are a scope again.
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 60, 16);
    support::press_control(&mut app, 'f');
    assert_eq!(
        app.picker().expect("the search").tabs(),
        ["file", "project"]
    );
}

/// The file's rows are its lines, and the picker narrows them. The rows are
/// lines rather than matches because the picker is already a matcher: a
/// search that filtered the lines itself would be a second, worse one beside
/// it, and would lose the highlighting of what matched.
#[test]
fn the_file_scope_lists_its_lines_and_narrows_to_the_query() {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 60, 16);
    support::press_control(&mut app, 'f');

    // Nothing typed, nothing listed: the reader is looking at the file
    // already, and a list of every line of it says nothing they cannot see.
    let picker = app.picker().expect("the search");
    assert_eq!(picker.match_count(), 0, "the file was listed unasked");
    assert_eq!(picker.nothing_to_show(), Some("type to search this file"));

    support::type_text(&mut app, "e");
    let picker = app.picker().expect("the search");
    assert_eq!(
        picker.row_count(),
        5,
        "not every line of a four-line file, plus the empty last one"
    );
    // Trimmed at the front: the indentation is the same on every row of a
    // block, so matching it finds nothing and showing it spends the width.
    assert!(
        picker
            .matches()
            .any(|item| item.label == "println!(\"{greeting} world\");"),
        "the lines are not the rows"
    );

    support::type_text(&mut app, "et");
    let picker = app.picker().expect("the search");
    assert_eq!(picker.query(), "eet");
    assert_eq!(picker.match_count(), 2, "the query narrowed nothing");

    // Narrowing does not throw away the row the reader had moved to, which
    // is the picker's own rule -- and which is why the file's lines are
    // gathered once on the way in rather than per keystroke.
    support::press(&mut app, KeyCode::Down);
    let chosen = app.picker().expect("the search").selected();
    assert_eq!(chosen, 1, "the second row is not selected");
    support::press(&mut app, KeyCode::Backspace);
    assert_eq!(
        app.picker().expect("the search").selected(),
        chosen,
        "typing put the selection back at the top"
    );
    // Nor does a redraw: the rows are re-gathered when the file changes, not
    // when the screen is painted.
    let _ = support::render(&mut app, 60, 16);
    assert_eq!(
        app.picker().expect("the search").selected(),
        chosen,
        "a redraw put the selection back at the top"
    );

    // Emptied again, the rows go away rather than becoming the file.
    for _ in 0..3 {
        support::press(&mut app, KeyCode::Backspace);
    }
    let picker = app.picker().expect("the search");
    assert_eq!(picker.row_count(), 0, "the file came back as a list");
    assert_eq!(picker.nothing_to_show(), Some("type to search this file"));
    support::type_text(&mut app, "greet");

    // And the row goes where it says: onto that line of that file.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    assert!(app.picker().is_none(), "the search stayed open");
    let cursor = app.current_buffer().expect("a file").cursor();
    assert_eq!(cursor.line.get(), 2, "not the line the row named");
}

/// The rows are the lines of one version of one file, so a file rewritten
/// while the search is open is listed as it now is. An agent editing the
/// file under the reader is the ordinary case here, not an exotic one.
#[test]
fn the_rows_follow_the_file_when_it_changes() {
    let root = temporary("reload");
    let path = root.join("f.rs");
    std::fs::write(&path, "fn alpha() {}\n").expect("writing");
    let mut app = App::new(vec![obelus::buffer::Buffer::open(&path).expect("opening")]);
    support::lay_out(&mut app, 60, 16);
    support::press_control(&mut app, 'f');
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

    // The file scope, from the buffer obelus already has parsed. The
    // fixture's line is indented with a tab, which the label does not carry:
    // a run counted from the start of the *line* would colour the wrong
    // characters of the row.
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 60, 16);
    let keyword = hex(app.theme().syntax.keyword);
    let string = hex(app.theme().syntax.string);
    support::press_control(&mut app, 'f');
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
    support::press_alt_key(&mut app, KeyCode::Char('f'));
    support::type_text(&mut app, "greeting");
    app.handle(Event::Matches {
        generation: app.search_generation(),
        hits: vec![Hit {
            path: std::path::PathBuf::from("tests/fixtures/sample.rs"),
            line: 1,
            text: "let greeting = \"\u{4f60}\u{597d}\";".to_string(),
        }],
        done: true,
    });
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
    for directory in [
        "src",
        "src/ui",
        "src/lsp",
        "src/syntax",
        "src/theme",
        "src/git",
    ] {
        for entry in std::fs::read_dir(directory).expect("reading the tree") {
            let path = entry.expect("an entry").path();
            if path.extension().is_some_and(|kind| kind == "rs") {
                paths.push(path);
            }
        }
    }
    assert!(paths.len() > 30, "not enough files to tell anything apart");

    let mut app = App::new(Vec::new());
    support::lay_out(&mut app, 60, 16);
    support::press_alt_key(&mut app, KeyCode::Char('f'));
    support::type_text(&mut app, "use");
    app.handle(Event::Matches {
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
    });
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

/// The query survives a walk between the tabs, which is the whole reason the
/// three scopes are one view: a reader who does not find it in this file
/// looks in the project without retyping it.
#[test]
fn the_query_walks_between_the_tabs() {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 60, 16);
    support::press_control(&mut app, 'f');
    support::type_text(&mut app, "greeting");

    support::press(&mut app, KeyCode::Right);
    let picker = app.picker().expect("the search");
    assert_eq!(
        picker.tabs()[picker.tab()],
        Scope::Project.label(),
        "the right arrow did not reach the project"
    );
    assert_eq!(picker.query(), "greeting", "the query did not come along");

    // Nothing has come back yet, so the list says what it is doing rather
    // than "no match" -- with a query typed and nothing found, "still
    // looking" and "not there" are different facts.
    assert_eq!(picker.nothing_to_show(), Some("searching\u{2026}"));

    // And back again, onto the file's own lines, still narrowed.
    support::press(&mut app, KeyCode::Left);
    let picker = app.picker().expect("the search");
    assert_eq!(
        picker.tabs()[picker.tab()],
        Scope::File.label(),
        "the left arrow did not come back to the file"
    );
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
    support::press_alt_key(&mut app, KeyCode::Char('f'));

    assert_eq!(
        app.picker().expect("the search").nothing_to_show(),
        Some("type to search every file"),
        "an empty query started a search"
    );

    support::type_text(&mut app, "g");
    assert_eq!(
        app.picker().expect("the search").nothing_to_show(),
        Some("searching\u{2026}"),
        "one letter did not start a search"
    );

    // And back to nothing typed: whatever was found is cleared, because the
    // rows for "g" are not an answer to no question at all.
    support::press(&mut app, KeyCode::Backspace);
    let picker = app.picker().expect("the search");
    assert_eq!(picker.match_count(), 0);
    assert_eq!(picker.nothing_to_show(), Some("type to search every file"));
}

/// A scan the reader has typed past stops instead of reading the rest of the
/// tree. Typing a ten-letter word starts ten scans, and nine of them are
/// answering a question nobody is asking any more; a walk cannot be
/// interrupted from outside, so the thread reads the generation itself.
#[test]
fn a_scan_that_has_been_typed_past_stops() {
    let root = temporary("cancel");
    for file in 0..40 {
        std::fs::write(root.join(format!("f{file}.rs")), "fn needle() {}\n").expect("writing");
    }

    // Stale before it starts, which is the same state a scan reaches when
    // the reader types another letter: the first file it looks at is enough
    // to find that out.
    let current = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(9));
    let (sender, events) = obelus::event::channel();
    search::spawn_scan(&root, "needle", 4, &current, sender);

    // The thread owns the only sender, so its return closes the channel.
    // Nothing at all comes through: not even the batch that says it is
    // finished, because a scan nobody is waiting for has nothing to report.
    match events.recv_timeout(std::time::Duration::from_secs(10)) {
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {}
        Ok(event) => panic!("a stale scan reported something: {event:?}"),
        Err(error) => panic!("a stale scan did not stop: {error}"),
    }

    // While the generation it was started under does run to the end.
    let current = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(4));
    let (sender, events) = obelus::event::channel();
    search::spawn_scan(&root, "needle", 4, &current, sender);
    let mut found = 0;
    loop {
        match events.recv_timeout(std::time::Duration::from_secs(10)) {
            Ok(Event::Matches { hits, done, .. }) => {
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
    support::press_alt_key(&mut app, KeyCode::Char('f'));
    support::type_text(&mut app, "greeting");

    // Text the query matches, because the picker filters what the scan
    // hands it: the scan matches a substring and the picker scores the same
    // query against the row, which is what highlights it.
    let hit = Hit {
        path: std::path::PathBuf::from("elsewhere.rs"),
        line: 3,
        text: "let greeting = elsewhere();".to_string(),
    };
    app.handle(Event::Matches {
        generation: app.search_generation() - 1,
        hits: vec![hit.clone()],
        done: true,
    });
    assert_eq!(
        app.picker().expect("the search").match_count(),
        0,
        "a stale answer landed in the list"
    );

    app.handle(Event::Matches {
        generation: app.search_generation(),
        hits: vec![hit],
        done: false,
    });
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

    // Asked for in the project, and then the reader walks back to the file.
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 60, 16);
    support::press_alt_key(&mut app, KeyCode::Char('f'));
    support::type_text(&mut app, "greeting");
    let generation = app.search_generation();
    support::press(&mut app, KeyCode::Left);
    app.handle(Event::Matches {
        generation,
        hits: vec![hit.clone()],
        done: false,
    });
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
    support::press_alt_key(&mut app, KeyCode::Char('f'));
    support::type_text(&mut app, "greeting");
    let generation = app.search_generation();
    support::press(&mut app, KeyCode::Esc);
    support::press_control(&mut app, 'p');
    app.handle(Event::Matches {
        generation,
        hits: vec![hit.clone()],
        done: false,
    });
    assert!(
        app.picker()
            .expect("the palette")
            .matches()
            .all(|item| item.label != hit.text),
        "a project row landed in the command palette"
    );
}

/// With the walk finished and nothing found, "no match" is finally a true
/// thing to say, and the reason that was standing in for it goes away.
#[test]
fn a_finished_walk_that_found_nothing_says_so() {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 60, 16);
    support::press_alt_key(&mut app, KeyCode::Char('f'));
    support::type_text(&mut app, "greeting");

    app.handle(Event::Matches {
        generation: app.search_generation(),
        hits: Vec::new(),
        done: true,
    });
    assert_eq!(
        app.picker().expect("the search").nothing_to_show(),
        Some("no match in the project")
    );
}

/// The symbols come from a server, and with no server there is nobody to
/// ask -- so there is no tab for them, and the key that would land on it
/// says why instead.
#[test]
fn the_symbols_scope_needs_a_server() {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 60, 16);
    support::press_alt_key(&mut app, KeyCode::Char('s'));
    assert!(
        app.picker().is_none(),
        "a search opened on a scope with nobody to ask"
    );
    assert_eq!(app.note(), Some("no language server to ask"));

    // And the palette shows it dim -- findable, so a reader learns obelus
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
        listed("search.project"),
        Some(true),
        "the project search needs nothing and cannot be chosen: {rows:?}"
    );
    assert_eq!(
        listed("search.symbols"),
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
    assert_eq!(app.note(), Some("no file open"));
}

/// The scan itself, against a real tree: what it finds, what it skips, and
/// that it says when it is finished.
#[test]
fn the_scan_finds_lines_and_finishes() {
    let root = temporary("scan");
    // A repository, because `.gitignore` is a git file: `ignore` applies it
    // where git would, and a bare directory is not somewhere git would.
    std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["init", "--quiet"])
        .output()
        .expect("git init");
    std::fs::write(root.join("one.rs"), "fn alpha() {}\nfn beta() {}\n").expect("writing");
    std::fs::write(root.join("two.rs"), "// beta again\n").expect("writing");
    // Not UTF-8 is a binary file, and a reader searching for a word is not
    // searching those.
    std::fs::write(root.join("blob.bin"), [0xff, 0xfe, b'b', b'e', b't', b'a']).expect("writing");
    std::fs::write(root.join(".gitignore"), "ignored/\n").expect("writing");
    std::fs::create_dir(root.join("ignored")).expect("a directory");
    std::fs::write(root.join("ignored/three.rs"), "fn beta_ignored() {}\n").expect("writing");

    let hits = scan(&root, "beta");
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
    let root = temporary("case");
    std::fs::write(root.join("f.rs"), "let alpha = 1;\nlet Alpha = 2;\n").expect("writing");

    assert_eq!(
        scan(&root, "alpha").len(),
        2,
        "lower case did not match both"
    );
    let strict = scan(&root, "Alpha");
    assert_eq!(strict.len(), 1, "a capital matched either case");
    assert_eq!(strict[0].line, 1);
}

/// A line no row can show is trimmed to something one can, and a file too
/// big to be written by hand is not read at all: a minified bundle is
/// megabytes of one line nobody is searching.
#[test]
fn the_scan_trims_what_a_row_cannot_show() {
    let root = temporary("wide");
    let long = format!("let x = \"{}needle\";", "a".repeat(600));
    std::fs::write(root.join("wide.rs"), format!("{long}\n")).expect("writing");
    std::fs::write(
        root.join("huge.rs"),
        format!("// needle\n{}", "x".repeat(3 * 1024 * 1024)),
    )
    .expect("writing");

    let hits = scan(&root, "let x");
    assert_eq!(hits.len(), 1, "the wide line was not found");
    assert!(
        hits[0].text.chars().count() <= 300,
        "a row would be {} characters wide",
        hits[0].text.chars().count()
    );
    assert!(
        scan(&root, "needle")
            .iter()
            .all(|hit| hit.path != std::path::Path::new("huge.rs")),
        "a three-megabyte file was read"
    );
}

/// Runs a scan to completion and returns everything it found.
fn scan(root: &std::path::Path, query: &str) -> Vec<Hit> {
    let current = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(7));
    let (sender, events) = obelus::event::channel();
    search::spawn_scan(root, query, 7, &current, sender);
    let mut hits = Vec::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        match events.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(Event::Matches {
                generation,
                hits: batch,
                done,
            }) => {
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
fn temporary(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("obelus-search-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("a directory");
    root
}

/// The preview marks the characters the query matched, and nothing else.
///
/// It used to mark the whole line: a row of the file scope carried the
/// line's own span -- column zero to the end of it -- and the preview
/// marked what the row said it was about. Which is the line, and a reader
/// who typed three letters is looking for those three letters.
#[test]
fn the_preview_marks_what_the_query_matched() {
    /// The columns of the preview's row for `line` that wear the marked
    /// background.
    fn marked(dump: &str, line: &str) -> Vec<usize> {
        // The preview is under the list, so the row wanted is the *last*
        // one holding that text: the list holds it too.
        let rows: Vec<usize> = support::text_block(dump)
            .lines()
            .enumerate()
            .filter(|(_, row)| row.contains(line))
            .map(|(at, _)| at)
            .collect();
        let at = rows
            .last()
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
    support::lay_out(&mut app, 60, 20);
    support::press_control(&mut app, 'f');
    // Three letters that are next to each other in one line of the file:
    // `let greeting = "..."`.
    support::type_text(&mut app, "eti");

    let dump = support::render(&mut app, 60, 20);
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
    let row = rows.last().expect("the preview's row").clone();
    let letters: String = columns
        .iter()
        .filter_map(|column| row.chars().nth(*column))
        .collect();
    assert_eq!(letters, "eti", "the marks are not on what matched:\n{dump}");

    // And a query whose characters are *not* next to each other is marked
    // where they are, rather than as one run from the first to the last: a
    // fuzzy match is scattered by nature.
    for _ in 0.."eti".len() {
        support::press(&mut app, KeyCode::Backspace);
    }
    support::type_text(&mut app, "lgn");
    let dump = support::render(&mut app, 60, 20);
    let columns = marked(&dump, "let greeting");
    assert_eq!(
        columns.len(),
        3,
        "a scattered match was marked as one run:\n{dump}"
    );
    let rows: Vec<String> = support::text_block(&dump)
        .lines()
        .filter(|row| row.contains("let greeting"))
        .map(str::to_string)
        .collect();
    let row = rows.last().expect("the preview's row").clone();
    let letters: String = columns
        .iter()
        .filter_map(|column| row.chars().nth(*column))
        .collect();
    assert_eq!(letters, "lgn", "the marks are not on what matched:\n{dump}");
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
    support::press_control(&mut app, 'f');
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

    support::press_control(&mut app, 'f');
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
    use obelus::search::Scope;
    assert!(Scope::File.lists_lines());
    assert!(Scope::Project.lists_lines());
    assert!(
        !Scope::Symbols.lists_lines(),
        "a symbol row is a name, not the line it is on"
    );
}
