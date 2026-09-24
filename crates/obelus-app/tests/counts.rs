//! The line counts: two pages over one walk of the tree.

mod support;

use crossterm::event::KeyCode;
use obelus_app::{
    app::{App, dispatch},
    event::Event,
};
use obelus_command::Command;
use obelus_search::counts::{Child, Counted, File, Language, Tally};
use support::press;

/// A tree of known size.
///
/// Made up rather than counted: a golden grid of this repository's own
/// numbers would be a test that fails every time somebody writes a line,
/// which is a test about the calendar rather than about the view.
fn counted() -> Counted {
    let tally = |code, comments, blanks| Tally {
        code,
        comments,
        blanks,
    };
    Counted {
        languages: vec![
            Language {
                name: "Rust",
                files: 94,
                extension: Some("rs"),
                tally: tally(31501, 3638, 3203),
                children: vec![Child {
                    name: "Markdown",
                    files: 90,
                    tally: tally(0, 7036, 1044),
                }],
            },
            Language {
                name: "Plain Text",
                files: 23,
                extension: Some("txt"),
                tally: tally(0, 1051, 0),
                children: Vec::new(),
            },
            Language {
                name: "TOML",
                files: 2,
                extension: Some("toml"),
                tally: tally(63, 76, 12),
                children: Vec::new(),
            },
        ],
        files: vec![
            File {
                path: std::path::PathBuf::from("src/app/mod.rs"),
                language: "Rust",
                tally: tally(742, 192, 60),
            },
            File {
                path: std::path::PathBuf::from("src/log.rs"),
                language: "Rust",
                tally: tally(749, 23, 23),
            },
            File {
                path: std::path::PathBuf::from("Cargo.toml"),
                language: "TOML",
                tally: tally(63, 76, 12),
            },
        ],
        total: tally(31564, 4765, 3215),
    }
}

/// The view, already filled in, on a screen of a known size.
fn open(width: u16, height: u16) -> App {
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(std::path::PathBuf::from("/tmp/obelus"));
    support::lay_out(&mut app, width, height);
    dispatch::dispatch(&mut app, Command::CountLines);
    // Through the event the walking thread sends, which is the path the real
    // answer arrives on.
    app.handle(Event::Counted(Box::new(counted())));
    app
}

/// The whole table: tabs, the languages with what is written inside them,
/// the bars, the columns and the total.
#[test]
fn the_counts_are_a_table_of_languages_biggest_first() {
    let mut app = open(76, 24);
    support::check("counts_76x24", &support::render(&mut app, 76, 24));
}

/// The file page is the same table over the files.
#[test]
fn the_other_page_is_the_files() {
    let mut app = open(76, 24);
    press(&mut app, KeyCode::Tab);
    support::check("counts_files_76x24", &support::render(&mut app, 76, 24));
}

/// The file page, with more files on it than the screen can hold.
fn many_files(width: u16, height: u16) -> App {
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(std::path::PathBuf::from("/tmp/obelus"));
    support::lay_out(&mut app, width, height);
    dispatch::dispatch(&mut app, Command::CountLines);
    let files: Vec<File> = (0..40)
        .map(|at| File {
            path: std::path::PathBuf::from(format!("src/module/file{at:02}.rs")),
            language: "Rust",
            tally: Tally {
                code: 400 - at * 9,
                comments: 20,
                blanks: 10,
            },
        })
        .collect();
    app.handle(Event::Counted(Box::new(Counted {
        languages: Vec::new(),
        files,
        total: Tally {
            code: 8180,
            comments: 800,
            blanks: 400,
        },
    })));
    press(&mut app, KeyCode::Tab);
    app
}

/// A list longer than the screen gets the bar every list here gets, and the
/// rules meet it rather than cutting it in half.
///
/// Broken deliberately by drawing the bar unconditionally: the fixture then
/// held a track on a page with nothing to scroll, which is a control that
/// does not work.
#[test]
fn a_list_longer_than_the_screen_gets_a_scrollbar() {
    let mut app = many_files(76, 14);
    // Down into `src/module`, which is where the forty of them are: a tree
    // that opens closed has one row on it, and one row does not scroll.
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    let dump = support::render(&mut app, 76, 14);
    assert!(
        support::text_block(&dump).contains('\u{2588}'),
        "a list longer than its region drew no thumb:\n{dump}"
    );
    support::check("counts_scrolling_76x14", &dump);
}

/// Walking down a long list scrolls it, rather than walking off the bottom
/// of it and scrolling afterwards.
///
/// Broken deliberately by adding one to what `list_height` answers, which
/// is the bug itself: the keys were told the list was a row taller than the
/// rows the view draws, and after eight steps the row the keys were on was
/// nowhere in the grid. (Changing `FURNITURE` instead does *not* reproduce
/// it -- that moves the drawing as well, so the two go on agreeing.)
#[test]
fn the_selection_stays_on_screen_as_it_walks_down() {
    let mut app = many_files(76, 14);

    for step in 0..30 {
        press(&mut app, KeyCode::Down);
        let dump = support::render(&mut app, 76, 14);
        let counts = app.counts().expect("the view closed");
        let row = &counts.rows()[counts.window().focus()];
        assert!(
            support::text_block(&dump).contains(&row.name),
            "after {step} steps the row the keys are on is off screen: {}\n{dump}",
            row.name
        );
    }
}

/// Nothing is typed into the counts, so nothing in them carries a caret.
///
/// Broken deliberately by taking the `app.counts()` check out of
/// `ui::cursor_position`: the file behind the view kept its own caret, which
/// blinked in a view the file is not part of, and this failed.
#[test]
fn the_counts_have_no_caret() {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    support::lay_out(&mut app, 76, 24);
    let reading = support::render(&mut app, 76, 24);
    assert!(
        support::cursor_line(&reading).contains(','),
        "the file being read had no caret to lose:\n{reading}"
    );

    dispatch::dispatch(&mut app, Command::CountLines);
    let counting = support::render(&mut app, 76, 24);
    assert_eq!(
        support::cursor_line(&counting).trim(),
        "none",
        "the counts drew a caret:\n{counting}"
    );
}

/// The counts take the screen whole: no status row, and no rule above one.
///
/// Broken deliberately by drawing the view into `regions.editor` and
/// obelus's own status row under it: the file behind the view was named
/// along the foot at a line and column belonging to a cursor that is nowhere
/// on screen, and this failed.
#[test]
fn the_counts_take_the_whole_screen() {
    let mut app = App::new(vec![support::open_fixture("sample.rs")]);
    app.working_directory_for_test(std::path::PathBuf::from("/tmp/obelus"));
    support::lay_out(&mut app, 76, 24);
    let reading = support::render(&mut app, 76, 24);
    assert!(
        support::text_block(&reading).contains("sample.rs"),
        "the file was not on the status row to begin with:\n{reading}"
    );

    dispatch::dispatch(&mut app, Command::CountLines);
    app.handle(Event::Counted(Box::new(counted())));
    let dump = support::render(&mut app, 76, 24);
    let text = support::text_block(&dump);
    assert!(
        !text.contains("sample.rs"),
        "the file behind the view is still on screen:\n{dump}"
    );
    // The last rows are the view's own -- the keys it answers to -- and not
    // the status row of a file it is covering. What is under a rule down
    // there belongs to this view or there is nothing there at all.
    let foot = text.lines().last().unwrap_or_default();
    assert!(
        foot.contains("Keys"),
        "the foot is not this view's:\n{dump}"
    );
}

/// A walk that has not answered yet says so, rather than showing an empty
/// table that reads as a project with nothing in it.
///
/// Broken deliberately by having the view say "Nothing here to count"
/// whatever the state: the two facts became one, and this failed.
#[test]
fn a_tree_still_being_walked_says_it_is_counting() {
    let mut app = App::new(Vec::new());
    support::lay_out(&mut app, 76, 24);
    dispatch::dispatch(&mut app, Command::CountLines);
    let dump = support::render(&mut app, 76, 24);
    assert!(
        support::text_block(&dump).contains("Counting"),
        "an unanswered walk did not say it was counting:\n{dump}"
    );
}

/// Choosing a language leaves the files of that language, and the tab says
/// which language they are.
///
/// Broken deliberately by leaving the narrowing unset when a language is
/// chosen: every file came back and the tab went on reading "Files".
#[test]
fn choosing_a_language_leaves_its_own_files() {
    let mut app = open(76, 24);
    // Past the tree's own row, onto Rust.
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    let dump = support::render(&mut app, 76, 24);
    let text = support::text_block(&dump);

    assert!(text.contains("Rust"), "the tab did not say rust:\n{dump}");
    assert!(
        !text.contains("Cargo.toml"),
        "a file of another language stayed:\n{dump}"
    );

    // And the files themselves are a directory down, named by what they are
    // called rather than by the whole path to them.
    press(&mut app, KeyCode::Enter);
    let dump = support::render(&mut app, 76, 24);
    let text = support::text_block(&dump);
    assert!(text.contains("log.rs"), "a Rust file is missing:\n{dump}");
    assert!(
        !text.contains("src/log.rs"),
        "a row still carries the whole path:\n{dump}"
    );
}

/// Escape gives up on the nearest thing: the narrowing first, then the view.
///
/// Broken deliberately by answering `Cancelled` to escape whatever is
/// showing: the first press closed the whole view, and the assertion that it
/// is still open failed.
#[test]
fn escape_gives_up_the_narrowing_before_the_view() {
    let mut app = open(76, 24);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Esc);
    assert!(
        app.counts().is_some(),
        "escape closed the view instead of the narrowing"
    );
    let dump = support::render(&mut app, 76, 24);
    assert!(
        support::text_block(&dump).contains("Plain Text"),
        "the languages did not come back:\n{dump}"
    );

    press(&mut app, KeyCode::Esc);
    assert!(app.counts().is_none(), "escape left the view open");
}

/// A count is somewhere to go: enter on a file opens it.
///
/// Broken deliberately by having the view keep itself open after opening a
/// file: the file was read behind a table nobody had asked to keep, and the
/// assertion that the view has gone failed.
#[test]
fn enter_on_a_file_opens_it() {
    let mut app = App::new(Vec::new());
    // A real tree, because the file has to be there to be read: this one is
    // the repository the tests are run from.
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    app.working_directory_for_test(root);
    support::lay_out(&mut app, 76, 24);
    dispatch::dispatch(&mut app, Command::CountLines);
    app.handle(Event::Counted(Box::new(Counted {
        languages: Vec::new(),
        files: vec![File {
            path: std::path::PathBuf::from("tests/fixtures/sample.rs"),
            language: "Rust",
            tally: Tally {
                code: 4,
                comments: 0,
                blanks: 1,
            },
        }],
        total: Tally {
            code: 4,
            comments: 0,
            blanks: 1,
        },
    })));

    press(&mut app, KeyCode::Tab);
    // `tests`, then `fixtures`, then the file: enter opens a directory and
    // stays on it, so the way down is a step and a press at each level.
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);

    assert!(app.counts().is_none(), "the view stayed over the file");
    let buffer = app.current_buffer().expect("nothing was opened");
    assert!(
        buffer.path().ends_with("tests/fixtures/sample.rs"),
        "opened {} instead",
        buffer.path().display()
    );
}

/// The walk really runs, on a thread, and its answer really arrives.
///
/// The only test that goes through [`obelus_search::counts::spawn_count`]:
/// every other one here hands the view a count, which proves what the view does
/// with one and nothing about where it comes from.
///
/// Broken deliberately by sending nothing from the thread: the wait timed
/// out and this failed rather than hanging, which is why it has a deadline.
#[test]
fn the_tree_is_counted_on_a_thread_and_the_answer_comes_back() {
    let (sender, events) = obelus_app::event::channel();
    // The fixtures: a handful of small files, in more than one language.
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    obelus_search::counts::spawn_count(&root, sender);

    let event = events
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("the walk never answered");
    let Event::Counted(counted) = event else {
        panic!("the walk sent something else");
    };

    assert!(!counted.is_empty(), "the fixtures counted as nothing");
    assert!(
        counted.total.code > 0,
        "a directory with code in it counted as no code"
    );
    assert!(
        counted
            .languages
            .iter()
            .any(|language| language.name == "Rust"),
        "the Rust fixtures were not counted as Rust"
    );
    // Relative to the tree that was counted, which is what the rows show.
    assert!(
        counted
            .files
            .iter()
            .any(|file| file.path == std::path::Path::new("sample.rs")),
        "sample.rs was not among them, or was not written relative to the tree"
    );
}

/// The counts are a dialog: nothing of obelus's own opens over them, and a
/// key bound in a dialog is not one of theirs.
///
/// Broken deliberately by taking the counts out of `App::is_showing_dialog`:
/// the key table was then reached from a view that has one open, `ctrl+p`
/// put the command palette on top of the table, and this failed. (Letting
/// the keys fall through *without* that is not enough to break it, which is
/// the point: what makes a dialog a dialog is that question, not the
/// component refusing keys.)
#[test]
fn nothing_of_obeluss_own_opens_over_the_counts() {
    let mut app = open(76, 24);
    for key in ['o', 'e', 'p'] {
        support::press_control(&mut app, key);
    }
    assert!(app.picker().is_none(), "a list opened over the counts");
    assert!(app.counts().is_some(), "the counts closed themselves");
}

/// The counts say what their keys do: `alt+f` cannot be guessed, and enter
/// does two different things depending on what the row names.
#[test]
fn the_counts_say_what_their_keys_do() {
    let mut app = open(76, 18);
    press(&mut app, KeyCode::Tab);
    let text = support::text_block(&support::render(&mut app, 76, 18)).to_string();
    for word in ["Open", "Keys"] {
        assert!(text.contains(word), "{word:?} is not at the foot:\n{text}");
    }
    // And not escape, which means the same thing in every view obelus has:
    // a foot is for what *this* one does. It is on the card.
    assert!(
        !text.contains("Leave"),
        "the foot is spending itself on escape:\n{text}"
    );
    support::press_control(&mut app, 'k');
    assert!(
        support::text_block(&support::render(&mut app, 76, 18)).contains("Leave"),
        "and it is not on the card either"
    );
    press(&mut app, KeyCode::Esc);

    // On a file it reads it instead: one key, two acts, and the foot says
    // which one is in front of the reader.
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Down);
    let text = support::text_block(&support::render(&mut app, 76, 18)).to_string();
    assert!(
        text.contains("Read it"),
        "enter does not say what it does:\n{text}"
    );
    // And `ctrl+k` says all of them, at length.
    support::press_control(&mut app, 'k');
    let dump = support::render(&mut app, 76, 18);
    assert!(
        support::text_block(&dump).contains("Read the file this row names"),
        "no card:\n{dump}"
    );
    press(&mut app, KeyCode::Esc);
    assert!(app.counts().is_some(), "escape left the view, not the card");
}

/// A press moves the selection, and a press on a row's fold mark opens it.
///
/// The counted table is drawn over the whole screen, and until now a press
/// anywhere in it did nothing: one line turned the pointer away before every
/// view drawn over a file. The wheel always reached it -- the wheel is its
/// own event -- which is the shape of the omission.
///
/// Moving the selection and nothing else, because a row here is a file to
/// open or a language to look inside, and a mis-aimed press that *chose*
/// one would take the reader off this page. Choosing stays on the keyboard.
///
/// Except the fold mark, which says the row opens. Pressing it does what
/// pressing an arrow means everywhere, and cannot take the reader anywhere:
/// the mark is on the rows that open and on no others.
///
/// Broken deliberately by handing the press back to nothing, which leaves
/// the selection where it was; or by opening on a press anywhere in the
/// row, which turns a press meant to look at a directory into one that
/// walks into it.
#[test]
fn a_press_moves_the_selection_and_the_fold_mark_opens_a_row() {
    let mut app = open(76, 24);
    // The files page, which is the one with anything to fold.
    press(&mut app, KeyCode::Tab);
    let _ = support::render(&mut app, 76, 24);

    let rows: Vec<(Option<bool>, u16)> = app
        .counts()
        .expect("the table")
        .rows()
        .iter()
        .map(|row| (row.open, row.depth))
        .collect();
    let directory = rows
        .iter()
        .position(|(open, _)| *open == Some(false))
        .expect("a row that is shut");
    let other = rows
        .iter()
        .position(|(open, _)| open.is_none())
        .expect("a row that opens nothing");

    let area = app.editor_area_for_test();
    let top = obelus_ui::counts::list_region(area, app.counts().expect("the table")).y;
    let press_at = |app: &mut App, row: usize, x: u16| {
        app.handle(Event::Pointer {
            kind: obelus_app::event::Pointer::Pressed,
            x,
            y: top + u16::try_from(row).expect("a row"),
        });
    };

    // A press on the words of a row moves the selection there and leaves
    // the page alone.
    let before = app.counts().expect("the table").rows().len();
    press_at(&mut app, other, area.x + 30);
    assert_eq!(
        app.counts().expect("the table").window().focus(),
        other,
        "the press did not move the selection"
    );
    assert_eq!(
        app.counts().expect("the table").rows().len(),
        before,
        "the press on the words of a row opened something"
    );

    // And a press on the mark of a row that is shut opens it. One column in
    // and two more for every branch drawn in front of it, which is where
    // the table draws the mark.
    let mark = 1 + rows[directory].1 * 2;
    press_at(&mut app, directory, area.x + mark);
    assert!(
        app.counts().expect("the table").rows().len() > before,
        "the press on the mark did not open the row"
    );
}
