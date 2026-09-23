//! What changed since the last commit.
//!
//! Values in, hunks out: the three cases a reader sees in the margin are
//! decided here, and each of them is a different claim about the file. A
//! full-height bar beside a line says "this line is different"; a mark at a
//! seam says "lines are missing here". Getting the two confused is a margin
//! that lies about which lines the reader is looking at.

mod support;

use std::sync::mpsc::Receiver;

use obelus_app::{app::App, event::Event};
use obelus_buffer::Buffer;
use obelus_git::Changes;
use obelus_text::{coordinates::LineNumber, marker::Marker};

#[test]
fn an_added_run_is_added() {
    let changes = Changes::between("one\ntwo\n", "one\nnew\ntwo\n");
    let hunks = changes.hunks();
    assert_eq!(hunks.len(), 1, "{hunks:?}");
    assert_eq!(hunks[0].marker(), Marker::Added);
    assert_eq!(hunks[0].line, LineNumber::new(1));
    assert_eq!(hunks[0].lines, 1);
    assert!(hunks[0].removed.is_empty(), "nothing was replaced");

    assert_eq!(changes.marker_at(LineNumber::new(1)), Some(Marker::Added));
    assert_eq!(changes.marker_at(LineNumber::new(0)), None);
    assert_eq!(changes.marker_at(LineNumber::new(2)), None);
}

#[test]
fn a_replaced_run_is_modified_and_keeps_what_it_replaced() {
    let changes = Changes::between("one\nold\nthree\n", "one\nnew\nthree\n");
    let hunks = changes.hunks();
    assert_eq!(hunks.len(), 1, "{hunks:?}");
    assert_eq!(hunks[0].marker(), Marker::Modified);
    assert_eq!(hunks[0].line, LineNumber::new(1));
    // The old text, so the hunk can be opened in place without going back
    // to git for it.
    assert_eq!(hunks[0].removed, ["old"]);
}

/// The case with nowhere to put a bar. The lines are gone, so there is no
/// line to mark: what is left is the boundary they were between, and the
/// marker belongs to the line that is now in front of it.
#[test]
fn a_deletion_marks_the_seam_it_left_behind() {
    let changes = Changes::between("one\ngone\nthree\n", "one\nthree\n");
    let hunks = changes.hunks();
    assert_eq!(hunks.len(), 1, "{hunks:?}");
    assert_eq!(hunks[0].marker(), Marker::Removed);
    assert_eq!(hunks[0].lines, 0, "a deletion covers no lines now");
    assert_eq!(hunks[0].removed, ["gone"]);

    // The line the removed text was in front of, which is `three`.
    assert_eq!(hunks[0].line, LineNumber::new(1));
    assert_eq!(changes.marker_at(LineNumber::new(1)), Some(Marker::Removed));
    assert_eq!(
        changes.marker_at(LineNumber::new(0)),
        None,
        "the line above a deletion did not change"
    );

    // And the reader standing on that line can open it: a seam with nothing
    // selectable beside it is a mark that cannot be acted on.
    assert!(changes.hunk_at(LineNumber::new(1)).is_some());
}

/// A deletion at the end of a file has no line in front of it, and still has
/// to be reachable.
#[test]
fn a_deletion_at_the_end_of_the_file_is_still_reachable() {
    let changes = Changes::between("one\ntwo\ngone\n", "one\ntwo\n");
    let hunks = changes.hunks();
    assert_eq!(hunks.len(), 1, "{hunks:?}");
    assert_eq!(hunks[0].marker(), Marker::Removed);
    assert_eq!(hunks[0].removed, ["gone"]);
    // Past the last line of the file as it is now, which is where the
    // removed text was.
    assert_eq!(hunks[0].line, LineNumber::new(2));
}

#[test]
fn the_three_cases_can_all_be_in_one_file() {
    // The screenshot's file: something replaced, something added, something
    // removed, all at once.
    let before = "keep\nold one\nold two\nremoved\nkeep too\n";
    let after = "keep\nnew one\nnew two\nadded\nkeep too\n";
    let changes = Changes::between(before, after);
    assert!(!changes.is_empty());

    let markers: Vec<Option<Marker>> = (0..5)
        .map(|line| changes.marker_at(LineNumber::new(line)))
        .collect();
    assert_eq!(markers[0], None, "an untouched line has no marker");
    assert_eq!(markers[4], None);
    assert!(
        markers[1..4].iter().all(Option::is_some),
        "the changed run is not marked: {markers:?}"
    );
}

#[test]
fn an_unchanged_file_has_nothing_to_say() {
    let same = "one\ntwo\n";
    assert!(Changes::between(same, same).is_empty());
    assert_eq!(
        Changes::between(same, same).marker_at(LineNumber::new(0)),
        None
    );
}

/// Against the repository obelus is being read in, which is the only thing
/// that says the two halves agree: a diff of what git *actually* has against
/// what is on disk.
#[test]
fn the_committed_text_comes_from_git() {
    let path = std::path::PathBuf::from(env!("OBELUS_TREE")).join("crates/obelus-app/src/lib.rs");
    let committed = obelus_git::head_text(&path).expect("src/lib.rs is committed");
    assert!(
        committed.contains("pub mod app;"),
        "that is not obelus's lib.rs"
    );

    let working = std::fs::read_to_string(&path).expect("reading it");
    assert!(
        Changes::between(&committed, &working).is_empty()
            || !Changes::between(&committed, &working).hunks().is_empty(),
        "a diff of a real file either has hunks or has none"
    );

    // A file git has never heard of has no committed text, which is how a
    // new file ends up with no markers rather than with a crash.
    let missing = path.with_file_name("obelus-not-a-file.rs");
    assert_eq!(obelus_git::head_text(&missing), None);
}

/// A repository in a temporary directory, with one file committed.
///
/// Against real git rather than against a fixture: what "changed since the
/// last commit" means has to be what git says it means, and a fixture of
/// hand-written hunks would only ever agree with itself.
struct Repository {
    directory: std::path::PathBuf,
}

impl Repository {
    fn new(name: &str, committed: &str) -> Self {
        let directory =
            std::env::temp_dir().join(format!("obelus-git-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a directory");

        let git = |arguments: &[&str]| {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(&directory)
                .args(arguments)
                // A commit needs an identity, and the machine's own may be
                // unset or may be someone else's.
                .env("GIT_AUTHOR_NAME", "obelus")
                .env("GIT_AUTHOR_EMAIL", "obelus@example.invalid")
                .env("GIT_COMMITTER_NAME", "obelus")
                .env("GIT_COMMITTER_EMAIL", "obelus@example.invalid")
                .output()
                .expect("running git");
            assert!(
                status.status.success(),
                "git {arguments:?} failed: {}",
                String::from_utf8_lossy(&status.stderr)
            );
        };
        git(&["init", "--quiet", "--initial-branch=master"]);
        std::fs::write(directory.join("file.rs"), committed).expect("the file");
        git(&["add", "file.rs"]);
        git(&["commit", "--quiet", "-m", "committed"]);
        Self { directory }
    }

    fn path(&self) -> std::path::PathBuf {
        self.directory.join("file.rs")
    }

    /// The repository's own directory, for the questions that are about the
    /// project rather than about one file in it.
    fn directory(&self) -> std::path::PathBuf {
        self.directory.clone()
    }

    /// Runs git in the repository, for the things a test sets up that
    /// obelus itself never does: a branch, a checkout.
    fn run(&self, arguments: &[&str]) {
        let outcome = std::process::Command::new("git")
            .arg("-C")
            .arg(&self.directory)
            .args(arguments)
            .env("GIT_AUTHOR_NAME", "obelus")
            .env("GIT_AUTHOR_EMAIL", "obelus@example.invalid")
            .env("GIT_COMMITTER_NAME", "obelus")
            .env("GIT_COMMITTER_EMAIL", "obelus@example.invalid")
            .output()
            .expect("running git");
        assert!(outcome.status.success(), "git {arguments:?} failed");
    }

    /// Commits everything in the tree, for a test that changes more than
    /// the one file.
    fn commit_all(&self, message: &str) {
        self.run(&["add", "-A"]);
        self.run(&["commit", "--quiet", "-m", message]);
    }

    fn write(&self, contents: &str) {
        std::fs::write(self.path(), contents).expect("rewriting the file");
    }

    /// Commits whatever the file now holds, for a test that needs a
    /// history rather than a single commit.
    fn commit(&self, message: &str) {
        let git = |arguments: &[&str]| {
            let outcome = std::process::Command::new("git")
                .arg("-C")
                .arg(&self.directory)
                .args(arguments)
                .env("GIT_AUTHOR_NAME", "obelus")
                .env("GIT_AUTHOR_EMAIL", "obelus@example.invalid")
                .env("GIT_COMMITTER_NAME", "obelus")
                .env("GIT_COMMITTER_EMAIL", "obelus@example.invalid")
                .output()
                .expect("running git");
            assert!(outcome.status.success(), "git {arguments:?} failed");
        };
        git(&["add", "file.rs"]);
        git(&["commit", "--quiet", "-m", message]);
    }
}

impl Drop for Repository {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

/// The whole path, against real git: a commit, a working tree that differs
/// three ways, and the three markers that come out.
#[test]
fn a_real_repository_gives_the_three_markers() {
    let repository = Repository::new(
        "markers",
        "fn one() {}\nfn old() {}\nfn gone() {}\nfn last() {}\n",
    );
    repository.write("fn one() {}\nfn new() {}\nfn added() {}\nfn last() {}\n");

    let committed = obelus_git::head_text(&repository.path()).expect("the committed text");
    assert_eq!(
        committed,
        "fn one() {}\nfn old() {}\nfn gone() {}\nfn last() {}\n"
    );

    let working = std::fs::read_to_string(repository.path()).expect("the working tree");
    let changes = Changes::between(&committed, &working);

    // Line one is untouched, lines two and three differ, line four is
    // untouched.
    assert_eq!(changes.marker_at(LineNumber::new(0)), None);
    assert_eq!(
        changes.marker_at(LineNumber::new(1)),
        Some(Marker::Modified)
    );
    assert_eq!(changes.marker_at(LineNumber::new(3)), None);

    // And a file with nothing done to it says nothing, through the same
    // path.
    let untouched = Repository::new("untouched", "fn one() {}\n");
    let committed = obelus_git::head_text(&untouched.path()).expect("the committed text");
    let working = std::fs::read_to_string(untouched.path()).expect("the working tree");
    assert!(Changes::between(&committed, &working).is_empty());
}

/// The margin on screen: one column at the far left, a solid bar for a line
/// that differs and a mark hugging the seam where lines are missing.
#[test]
fn the_margin_marks_the_lines_on_screen() {
    use obelus_app::app::App;
    use obelus_buffer::Buffer;

    // The three cases, kept apart by untouched lines: adjacent to each other
    // git merges a deletion into the modification next to it, which is one
    // hunk and one marker -- correctly, and not what this test is about.
    let repository = Repository::new("margin", "one\ntwo\nkeep\ngone\nkeep too\nfive\n");
    repository.write("one\nchanged\nkeep\nkeep too\nfive\nadded\n");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    support::lay_out(&mut app, 40, 8);
    let dump = support::render(&mut app, 40, 8);
    let rows: Vec<&str> = support::text_block(&dump)
        .lines()
        .filter(|row| !row.is_empty())
        .collect();

    // The first cell of each row, which is the margin.
    let margin: String = rows
        .iter()
        .take(6)
        .map(|row| row.chars().nth(3).unwrap_or(' '))
        .collect();
    assert_eq!(
        margin, " \u{2590} \u{2594} \u{2590}",
        "not the three markers in the right rows:\n{dump}"
    );

    // And they are three different colours, because they are three different
    // claims about the file.
    let letters: Vec<char> = support::style_block(&dump)
        .lines()
        .filter(|row| !row.is_empty())
        .take(6)
        .map(|row| row.chars().nth(3).unwrap_or(' '))
        .collect();
    assert_ne!(letters[1], letters[3], "modified and removed look alike");
    assert_ne!(letters[1], letters[5], "modified and added look alike");
    assert_ne!(letters[3], letters[5], "removed and added look alike");
    assert_ne!(letters[1], letters[0], "an unchanged line is marked");
}

/// A hunk opened in place: what the lines replaced, above them, pushing the
/// file down. What a changed line means is what it replaced, and the two
/// belong next to each other.
#[test]
fn a_hunk_opens_in_place_and_closes_again() {
    use obelus_app::app::App;
    use obelus_buffer::Buffer;
    use obelus_command::Command;

    let repository = Repository::new("open", "one\nold two\nold three\nfour\n");
    repository.write("one\nnew two\nfour\n");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    support::lay_out(&mut app, 40, 8);
    // Onto the changed line.
    support::press(&mut app, crossterm::event::KeyCode::Down);

    let closed = support::render(&mut app, 40, 8);
    assert!(
        !support::text_block(&closed).contains("old two"),
        "the removed text is showing before it was asked for:\n{closed}"
    );

    obelus_app::app::dispatch::dispatch(&mut app, Command::GitHunk);
    let opened = support::render(&mut app, 40, 8);
    let rows: Vec<&str> = support::text_block(&opened)
        .lines()
        .filter(|row| !row.is_empty())
        .collect();

    // Both removed lines, above the line that replaced them.
    let removed = rows
        .iter()
        .position(|row| row.contains("old two"))
        .unwrap_or_else(|| panic!("the removed text is not on screen:\n{opened}"));
    assert!(rows[removed + 1].contains("old three"), "{opened}");
    assert!(
        rows[removed + 2].contains("new two"),
        "the removed lines are not above the line that replaced them:\n{opened}"
    );

    // With no line number of their own: they have none in this file, and
    // borrowing the next one's would be a lie about where they are.
    assert!(
        !rows[removed].contains('2') || !rows[removed].trim_start().starts_with('2'),
        "the removed line took a line number:\n{opened}"
    );
    // And in the removed colour, which is what says they are not the file.
    let styles: Vec<&str> = support::style_block(&opened)
        .lines()
        .filter(|row| !row.is_empty())
        .collect();
    assert_ne!(
        styles[removed].chars().nth(3 + 6),
        styles[removed + 2].chars().nth(3 + 6),
        "the removed line looks like the file:\n{opened}"
    );

    // And with the bar every line on screen gets, not the boundary mark:
    // the top edge is how a deletion is shown when it has no row of its
    // own, and opening the hunk is the act of giving it one.
    let margin = |row: usize| rows[row].chars().nth(3).unwrap_or(' ');
    assert_eq!(
        (margin(removed), margin(removed + 1)),
        ('\u{2590}', '\u{2590}'),
        "the opened lines did not get the bar:\n{opened}"
    );
    // In their own colour, still: they are gone, not merely different.
    assert_ne!(
        styles[removed].chars().nth(3),
        styles[removed + 2].chars().nth(3),
        "the opened lines are marked like the line that replaced them:\n{opened}"
    );

    // Behind them, too: an opened hunk mixes lines that are gone with lines
    // that are there, and the row is what tells them apart once the eye has
    // left the margin.
    // Cell six, which is a character of the text itself -- not the blank
    // gutter. A style that names a background paints over the row's tint
    // wherever there is a glyph, which leaves the colour showing in the gaps
    // between words and nowhere else, and a check on a blank cell cannot
    // tell the two apart.
    let behind = |row: usize| {
        let letter = styles[row].chars().nth(3 + 6).expect("a style cell");
        support::legend_block(&opened)
            .lines()
            .find(|line| line.trim_start().starts_with(letter))
            .and_then(|line| line.split("bg=").nth(1))
            .map(str::to_string)
            .expect("a background")
    };
    assert_eq!(
        behind(removed),
        behind(removed + 1),
        "the two removed lines are on different colours:\n{opened}"
    );
    assert_ne!(
        behind(removed),
        behind(removed + 2),
        "removed and replaced lines are on the same colour:\n{opened}"
    );
    // And on a colour at all: the row above the hunk is untouched file.
    assert_ne!(
        behind(removed),
        behind(removed - 1),
        "the removed lines are on the file's own background:\n{opened}"
    );

    // The same command closes it.
    obelus_app::app::dispatch::dispatch(&mut app, Command::GitHunk);
    assert!(
        !support::text_block(&support::render(&mut app, 40, 8)).contains("old two"),
        "it would not close"
    );
}

/// The caret walks into the lines a hunk replaced, and what it selects
/// there can be copied.
///
/// They are text -- what the file used to say -- and a reader looking at
/// them wants to read them a line at a time and take a copy. They are not
/// *this* file's text, so the cursor stays on the line the block belongs
/// to: everything that asks the file about "here" goes on being answered
/// from a line the file has.
#[test]
fn the_caret_walks_into_what_a_hunk_replaced() {
    use crossterm::event::KeyCode;
    use obelus_app::app::{App, dispatch};
    use obelus_buffer::Buffer;
    use obelus_command::Command;

    let repository = Repository::new("walk-in", "alpha\nbeta\ngamma\nkept\n");
    repository.write("delta\nkept\n");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    support::lay_out(&mut app, 40, 12);
    dispatch::dispatch(&mut app, Command::GitHunk);
    let at = |app: &App| app.current_buffer().expect("a file").in_block();

    // Up from the line that replaced them walks into the block, at its last
    // line: the block sits directly above that line.
    assert_eq!(at(&app), None, "the caret started in the block");
    support::press(&mut app, KeyCode::Up);
    assert_eq!(
        at(&app).map(|(line, _)| line.get()),
        Some(2),
        "up did not walk into the block"
    );
    support::press(&mut app, KeyCode::Up);
    assert_eq!(at(&app).map(|(line, _)| line.get()), Some(1));

    // The caret is drawn where it is, and the status row says where that is
    // -- as a place in the block, because those lines have no number in
    // this file.
    let dump = support::render(&mut app, 40, 12);
    let rows: Vec<&str> = support::text_block(&dump)
        .lines()
        .filter(|row| row.contains('|'))
        .collect();
    let caret_row: usize = support::cursor_line(&dump)
        .split(',')
        .nth(1)
        .expect("the caret's row")
        .parse()
        .expect("a number");
    assert!(
        rows[caret_row].contains("beta"),
        "the caret is not on the line it is on:\n{dump}"
    );
    assert!(
        rows[rows.len() - 1].contains("-2:1"),
        "the status row does not say where the caret is:\n{dump}"
    );

    // The cursor has not moved: those lines are not places in the file.
    assert_eq!(
        app.current_buffer().expect("a file").cursor().line.get(),
        0,
        "the cursor followed the caret out of the file"
    );

    // And along the line as well as down the block: the caret's cell is a
    // cell of the line it is on, which is one the file does not have.
    support::press(&mut app, KeyCode::End);
    let dump = support::render(&mut app, 40, 12);
    let cell = |dump: &str| {
        support::cursor_line(dump)
            .split(',')
            .next()
            .expect("the caret's cell")
            .parse::<usize>()
            .expect("a number")
    };
    let row = support::text_block(&dump)
        .lines()
        .find(|row| row.contains("beta"))
        .expect("the row")
        .to_string();
    // In cells, counted the way the screen counts: the row's own number and
    // bar come first, and the margin's glyph is one cell however many bytes
    // it takes.
    let word = row[..row.find("beta").expect("the word")].chars().count() - "00|".len();
    assert_eq!(
        cell(&dump),
        word + "beta".len(),
        "the caret is not at the end of the line it is on:\n{dump}"
    );
    support::press(&mut app, KeyCode::Home);
    let dump = support::render(&mut app, 40, 12);
    assert_eq!(
        cell(&dump),
        word,
        "home did not take the caret to the start of the line:\n{dump}"
    );

    // Selecting in there and copying takes the removed text, not the file's.
    support::press_shift(&mut app, KeyCode::Down);
    support::press_shift(&mut app, KeyCode::End);
    assert_eq!(
        app.current_buffer().and_then(Buffer::selected_text),
        Some("beta\ngamma".to_string()),
        "the selection did not take the lines it was drawn over"
    );
    // And the command that copies is offered for it, because a reader who
    // can select something can copy it.
    assert!(
        app.offers(Command::SelectionCopy),
        "a selection in the block is not a selection"
    );
    // It is drawn, too: a copy whose extent nobody can see is a guess.
    let dump = support::render(&mut app, 40, 12);
    let styles: Vec<&str> = support::style_block(&dump)
        .lines()
        .filter(|row| row.contains('|'))
        .collect();
    let ink = |row: usize, word: &str| {
        let text = support::text_block(&dump)
            .lines()
            .filter(|row| row.contains('|'))
            .nth(row)
            .expect("a row")
            .to_string();
        let at = text[..text.find(word).expect("the word")].chars().count();
        styles[row].chars().nth(at).expect("a cell")
    };
    assert_ne!(
        ink(1, "beta"),
        ink(0, "alpha"),
        "the selected lines look like the ones around them:\n{dump}"
    );

    // Walking off the bottom of the block leaves it for the line it was
    // drawn above, which is where the cursor has been waiting. The
    // selection took the caret to the block's last line, so one step does
    // it.
    support::press(&mut app, KeyCode::Down);
    assert_eq!(at(&app), None, "down did not walk out of the block");
    assert_eq!(
        app.current_buffer().expect("a file").cursor().line.get(),
        0,
        "leaving the block landed somewhere else"
    );

    // And closing the hunk puts the caret back in the file whatever it was
    // reading.
    support::press(&mut app, KeyCode::Up);
    assert!(at(&app).is_some(), "the caret did not walk back in");
    dispatch::dispatch(&mut app, Command::GitHunk);
    assert_eq!(at(&app), None, "the caret stayed in a block that is gone");
}

/// Walking down through a block comes out below it, on the line it was
/// drawn above.
///
/// The way in from above and the way out at the bottom are not the same
/// place: a reader who walked in from the line above and then kept going
/// used to be put back on that same line, and pressing down again walked
/// straight back into the block -- a hunk the cursor could never get past.
/// And the column is the one the reader was in, through both doors: a
/// vertical move aims for the cell it left, in the block as anywhere else.
#[test]
fn walking_through_a_block_comes_out_the_other_side() {
    use crossterm::event::KeyCode;
    use obelus_app::app::{App, dispatch};
    use obelus_buffer::Buffer;
    use obelus_command::Command;

    // A change in the middle, so there is a line above the block and a line
    // below it. The lines are long enough to have a column worth keeping.
    let repository = Repository::new("through", "first line\nold one\nold two\nlast line\n");
    repository.write("first line\nnew one\nlast line\n");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    support::lay_out(&mut app, 40, 12);
    let cursor = |app: &App| {
        let cursor = app.current_buffer().expect("a file").cursor();
        (cursor.line.get(), cursor.column.get())
    };
    let at = |app: &App| {
        app.current_buffer()
            .expect("a file")
            .in_block()
            .map(|(line, column)| (line.get(), column.get()))
    };

    // Onto the changed line and open what it replaced. Up from there walks
    // into the block -- it is drawn directly above this line -- so three of
    // them walk its two lines and come out on the line above it.
    support::press(&mut app, KeyCode::Down);
    dispatch::dispatch(&mut app, Command::GitHunk);
    support::press(&mut app, KeyCode::Up);
    assert_eq!(
        at(&app).map(|(line, _)| line),
        Some(1),
        "up did not walk in"
    );
    support::press(&mut app, KeyCode::Up);
    support::press(&mut app, KeyCode::Up);
    assert_eq!(at(&app), None, "up did not walk out of the top");
    for _ in 0..4 {
        support::press(&mut app, KeyCode::Right);
    }
    assert_eq!(cursor(&app), (0, 4), "not on the line above the block");

    // Down walks in, keeping the column, and walks the block's lines.
    support::press(&mut app, KeyCode::Down);
    assert_eq!(at(&app), Some((0, 4)), "down did not walk in at the column");
    support::press(&mut app, KeyCode::Down);
    assert_eq!(at(&app), Some((1, 4)));

    // And out the bottom, onto the line the block was drawn above -- not
    // back to the one it was entered from.
    support::press(&mut app, KeyCode::Down);
    assert_eq!(at(&app), None, "down did not walk out of the block");
    assert_eq!(
        cursor(&app),
        (1, 4),
        "walking out of the bottom landed above the block"
    );

    // Which means the reader gets past it: down again is the line after.
    support::press(&mut app, KeyCode::Down);
    assert_eq!(at(&app), None, "down walked back into the block");
    assert_eq!(cursor(&app), (2, 4), "the block cannot be got past");

    // And back up through it the other way, out of the top onto the line
    // above -- at the same column again.
    support::press(&mut app, KeyCode::Up);
    assert_eq!(cursor(&app), (1, 4));
    for _ in 0..3 {
        support::press(&mut app, KeyCode::Up);
    }
    assert_eq!(at(&app), None, "up did not walk out of the block");
    assert_eq!(
        cursor(&app),
        (0, 4),
        "walking out of the top lost the column"
    );
}

/// A hunk that replaced nothing has nothing to walk into.
///
/// Added lines open like every other hunk -- the tint behind them is what
/// says what kind of change they are -- and there are no removed lines
/// above them. The caret has to pass straight over that, or the key that
/// was going to move the reader up a line does nothing instead.
#[test]
fn a_hunk_with_nothing_removed_is_not_walked_into() {
    use crossterm::event::KeyCode;
    use obelus_app::app::{App, dispatch};
    use obelus_buffer::{Block, Buffer};
    use obelus_command::Command;

    let repository = Repository::new("nothing-removed", "one\ntwo\n");
    repository.write("one\nadded\ntwo\n");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    support::lay_out(&mut app, 40, 12);
    support::press(&mut app, KeyCode::Down);
    dispatch::dispatch(&mut app, Command::GitHunk);
    let buffer = app.current_buffer().expect("a file");
    assert_eq!(
        buffer.blocks().first().map(Block::is_empty),
        Some(true),
        "the added hunk did not open"
    );
    assert_eq!(buffer.cursor().line.get(), 1);

    // Up is the line above, not a step into a block with no lines in it.
    support::press(&mut app, KeyCode::Up);
    let buffer = app.current_buffer().expect("a file");
    assert_eq!(buffer.in_block(), None, "the caret walked into nothing");
    assert_eq!(
        buffer.cursor().line.get(),
        0,
        "up was eaten by an empty block"
    );
}

/// A removed line too long for the screen wraps, like every other line.
///
/// The block is a text, so it gets what the file's lines get: it breaks at
/// the same width, the caret walks the rows it breaks into, and the end of
/// a long line is somewhere a reader can stand. Drawn a row each and cut at
/// the edge, the far end of such a line could be neither read nor reached.
#[test]
fn a_removed_line_too_long_for_the_screen_wraps() {
    use crossterm::event::KeyCode;
    use obelus_app::app::{App, dispatch};
    use obelus_buffer::Buffer;
    use obelus_command::Command;

    let long = "alpha beta gamma delta epsilon zeta eta theta iota kappa";
    let repository = Repository::new("wrapped", &format!("{long}\nkept\n"));
    repository.write("short\nkept\n");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    // Wrapping is off by default, and this is a test about wrapping.
    app.configure(
        obelus_config::Config {
            wrap: true,
            ..obelus_config::Config::default()
        },
        Vec::new(),
    );
    // Narrow enough that the removed line needs more than one row.
    support::lay_out(&mut app, 34, 12);
    dispatch::dispatch(&mut app, Command::GitHunk);

    let dump = support::render(&mut app, 34, 12);
    let text = support::text_block(&dump);
    assert!(
        text.contains("alpha beta") && text.contains("kappa"),
        "the long line was cut rather than wrapped:\n{dump}"
    );

    // And the caret walks its rows: up from the file lands on the last row
    // of it, and one more step is still inside the same line.
    support::press(&mut app, KeyCode::Up);
    let at = |app: &App| app.current_buffer().expect("a file").in_block();
    assert_eq!(
        at(&app).map(|(line, _)| line.get()),
        Some(0),
        "up did not walk into the only line there is"
    );
    support::press(&mut app, KeyCode::End);
    let dump = support::render(&mut app, 34, 12);
    let caret = support::cursor_line(&dump).to_string();
    assert_ne!(caret, "none", "the caret left the screen:\n{dump}");
    // The end of the line is on a later row than its start, which is the
    // whole of what wrapping means here.
    support::press(&mut app, KeyCode::Home);
    let home = support::cursor_line(&support::render(&mut app, 34, 12)).to_string();
    assert_ne!(home, caret, "the line has only one row");
}

/// Selecting inside a block leaves the file's own selection alone.
///
/// The anchor belongs to whichever place the caret is in. Armed in the file
/// while the reader is selecting in a block, it would sit on the line the
/// cursor is parked on -- and walking out of the block would leave a
/// selection nobody made, drawn across the file and copied instead of what
/// they were actually selecting.
#[test]
fn selecting_in_a_block_selects_nothing_in_the_file() {
    use crossterm::event::KeyCode;
    use obelus_app::app::{App, dispatch};
    use obelus_buffer::Buffer;
    use obelus_command::Command;

    let repository = Repository::new("anchors", "first\nold one\nold two\nlast\n");
    repository.write("first\nnew one\nlast\n");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    support::lay_out(&mut app, 40, 12);
    support::press(&mut app, KeyCode::Down);
    dispatch::dispatch(&mut app, Command::GitHunk);

    // Out of the block's top, so the cursor is parked on the line above it,
    // then back in and along with shift held.
    for _ in 0..3 {
        support::press(&mut app, KeyCode::Up);
    }
    support::press_shift(&mut app, KeyCode::Down);
    support::press_shift(&mut app, KeyCode::Down);
    assert_eq!(
        app.current_buffer().and_then(Buffer::selected_text),
        Some("old one\n".to_string()),
        "the selection is not the block's first line"
    );

    // And walking out of the bottom -- still holding shift, which is what
    // a reader selecting downward does -- leaves nothing selected: the
    // block's selection went with the block, and the file never had one.
    support::press_shift(&mut app, KeyCode::Down);
    let buffer = app.current_buffer().expect("a file");
    assert_eq!(buffer.in_block(), None, "still in the block");
    assert_eq!(
        buffer.selection(),
        None,
        "walking out of a block left a selection in the file"
    );
    assert_eq!(buffer.selected_text(), None);
}

/// Escape gives up on a selection made inside a block, like any other.
#[test]
fn escape_clears_a_selection_in_a_block() {
    use crossterm::event::KeyCode;
    use obelus_app::app::{App, dispatch};
    use obelus_buffer::Buffer;
    use obelus_command::Command;

    let repository = Repository::new("clearing", "alpha\nbeta\nkept\n");
    repository.write("delta\nkept\n");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    support::lay_out(&mut app, 40, 12);
    dispatch::dispatch(&mut app, Command::GitHunk);
    support::press(&mut app, KeyCode::Up);
    support::press_shift(&mut app, KeyCode::Up);
    assert!(
        app.current_buffer()
            .and_then(Buffer::selected_text)
            .is_some(),
        "nothing was selected to give up on"
    );
    assert!(app.offers(Command::SelectionClear));

    dispatch::dispatch(&mut app, Command::SelectionClear);
    assert_eq!(
        app.current_buffer().and_then(Buffer::selected_text),
        None,
        "escape left the selection in the block"
    );
    assert!(
        !app.offers(Command::SelectionClear),
        "the command is still offered with nothing selected"
    );
}

/// The key that opens a hunk closes it from wherever the reader walked to.
///
/// Walking into the block parks the cursor on the line it is anchored to,
/// and walking in from above leaves it on the line before that -- from
/// neither of which is "the hunk at the cursor" the hunk in front of them.
#[test]
fn the_key_that_opened_a_hunk_closes_it_from_inside() {
    use crossterm::event::KeyCode;
    use obelus_app::app::{App, dispatch};
    use obelus_buffer::Buffer;
    use obelus_command::Command;

    let repository = Repository::new("closing", "first\nold one\nold two\nlast\n");
    repository.write("first\nnew one\nlast\n");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    support::lay_out(&mut app, 40, 12);
    support::press(&mut app, KeyCode::Down);
    dispatch::dispatch(&mut app, Command::GitHunk);

    // Out of the top of the block, onto the line above it: the cursor is
    // now two lines from the hunk it is looking at.
    for _ in 0..3 {
        support::press(&mut app, KeyCode::Up);
    }
    assert_eq!(app.current_buffer().expect("a file").cursor().line.get(), 0);
    assert!(
        !app.current_buffer().expect("a file").blocks().is_empty(),
        "the block closed on its own"
    );
    assert!(
        app.offers(Command::GitHunk),
        "the key that closes it is dim while it is open"
    );
    dispatch::dispatch(&mut app, Command::GitHunk);
    assert!(
        app.current_buffer().expect("a file").blocks().is_empty(),
        "the hunk could not be closed from outside it"
    );
    assert_eq!(app.note(), None, "closing it said something");
}

/// A deletion taller than the screen can be read all the way through.
///
/// The rows an opened hunk draws are rows of the *screen*, so the viewport
/// can be inside them -- which is what the paging keys then walk through.
/// Before that, the block was only ever drawn from its first row, and its
/// first screenful was the whole of what a reader could see: with the cursor
/// on the line that replaced it the arithmetic put that line off the bottom
/// of the screen, so nothing moved at all and the way out was to close the
/// hunk.
#[test]
fn a_deletion_taller_than_the_screen_can_be_read() {
    use crossterm::event::KeyCode;
    use obelus_app::app::{App, dispatch};
    use obelus_buffer::Buffer;
    use obelus_command::Command;

    // Eighty lines replaced by one, on a screen with ten rows of text.
    let mut committed = String::new();
    for line in 0..80 {
        committed.push_str(&format!("gone {line}\n"));
    }
    committed.push_str("kept\n");
    let repository = Repository::new("tall", &committed);
    repository.write("kept\n");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    support::lay_out(&mut app, 30, 12);
    dispatch::dispatch(&mut app, Command::GitHunk);

    let screen = |app: &mut App| support::text_block(&support::render(app, 30, 12)).to_string();

    // The end of what was removed, next to the line that replaced it: the
    // cursor is on that line, and the rows above it are what it replaced.
    let opened = screen(&mut app);
    assert!(
        opened.contains("gone 79") && opened.contains("kept"),
        "not the end of the deletion beside the line that replaced it:\n{opened}"
    );

    // Eight pages up reaches the first line of it, one page at a time --
    // and stops there rather than snapping back to the cursor.
    for _ in 0..8 {
        support::press(&mut app, KeyCode::PageUp);
    }
    let top = screen(&mut app);
    assert!(
        top.contains("gone 0") && top.contains("gone 9"),
        "the top of the deletion is out of reach:\n{top}"
    );
    support::press(&mut app, KeyCode::PageUp);
    let again = screen(&mut app);
    // The rows, not the whole screen: there is nowhere above this to go, so
    // what is drawn stays put -- but the caret walks up to the first row of
    // it, the way it walks to the first line of a file that cannot scroll
    // any further.
    let rows = |screen: &str| {
        screen
            .lines()
            .take_while(|line| !line.contains('\u{2500}'))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(
        rows(&again),
        rows(&top),
        "the top of the deletion is not where it stopped"
    );
    assert_eq!(
        app.current_buffer()
            .expect("a file")
            .in_block()
            .map(|(line, _)| line.get()),
        Some(0),
        "the caret did not walk up to the first row it could reach"
    );

    // And the middle, which is the part that had no way of being seen: a
    // page down from the top lands in it.
    support::press(&mut app, KeyCode::PageDown);
    let middle = screen(&mut app);
    assert!(
        middle.contains("gone 10") && middle.contains("gone 19"),
        "a page down from the top did not land in the middle:\n{middle}"
    );
    assert!(
        !middle.contains("kept"),
        "the file is on screen, so this is not the middle of the block:\n{middle}"
    );

    // The caret went with it, keeping its place on the screen the way it
    // does through any page: it was on the block's first row, and it is on
    // the first row of what is drawn now. Which is what makes the next
    // arrow key carry on from where the reader is looking rather than from
    // the file below.
    let buffer = app.current_buffer().expect("a file");
    assert_eq!(
        buffer.in_block().map(|(line, _)| line.get()),
        Some(10),
        "the caret did not go where the page went"
    );
    // And the cursor stayed on the line the block belongs to. Those lines
    // are not in the file, so nothing that asks the file about "here" may
    // be answered from one.
    assert_eq!(buffer.cursor().line.get(), 0, "the cursor left the file");

    // Up from the block's first line stays there: this block is drawn above
    // the first line of the file, so there is nothing above it to walk on
    // to. Leaving would put the caret on the line the block was drawn
    // above, which is *below* where it was.
    for _ in 0..25 {
        support::press(&mut app, KeyCode::Up);
    }
    let buffer = app.current_buffer().expect("a file");
    assert_eq!(
        buffer.in_block().map(|(line, _)| line.get()),
        Some(0),
        "up walked out of the top of a block with nothing above it"
    );
}

/// The caret comes down with the file the hunk pushed down.
///
/// Opening a hunk puts what its lines replaced above them, which moves every
/// line from there on down the screen. The caret is on one of those lines,
/// so it moves too -- and when it did not, it was drawn among the removed
/// lines while the cursor was somewhere else entirely: five steps down
/// walked out of a five-line hunk while the caret still looked as though it
/// were on the first line of it, and the key that closes the hunk answered
/// "Nothing changed here".
#[test]
fn the_caret_walks_the_lines_of_an_opened_hunk() {
    use obelus_app::app::App;
    use obelus_buffer::Buffer;
    use obelus_command::Command;

    let repository = Repository::new("walk", "one\nold a\nold b\nold c\nold d\nold e\nlast\n");
    repository.write("one\nnew a\nnew b\nnew c\nnew d\nnew e\nlast\n");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    support::lay_out(&mut app, 40, 16);
    // Onto the first changed line, and open it: five removed lines above
    // five changed ones.
    support::press(&mut app, crossterm::event::KeyCode::Down);
    obelus_app::app::dispatch::dispatch(&mut app, Command::GitHunk);

    // The caret is on the row its own line is drawn on, not on one of the
    // rows the removed lines took.
    let on = |dump: &str, text: &str| {
        support::text_block(dump)
            .lines()
            .position(|row| row.contains(text))
            .unwrap_or_else(|| panic!("no {text} on screen:\n{dump}"))
            .saturating_sub(1)
    };
    let caret_row = |dump: &str| {
        support::cursor_line(dump)
            .split(',')
            .nth(1)
            .and_then(|row| row.parse::<usize>().ok())
            .expect("the caret's row")
    };
    let dump = support::render(&mut app, 40, 16);
    assert_eq!(
        caret_row(&dump),
        on(&dump, "new a"),
        "the caret is not on the line it is on:\n{dump}"
    );

    // Down to the last line of the hunk: four steps, four rows.
    for _ in 0..4 {
        support::press(&mut app, crossterm::event::KeyCode::Down);
    }
    let dump = support::render(&mut app, 40, 16);
    assert_eq!(
        caret_row(&dump),
        on(&dump, "new e"),
        "the caret did not walk the lines of the hunk:\n{dump}"
    );

    // And from there the same key closes it, because that is still inside
    // the hunk.
    obelus_app::app::dispatch::dispatch(&mut app, Command::GitHunk);
    assert!(
        app.opened_hunks().is_empty(),
        "the last line of the hunk would not close it"
    );
    let closed = support::render(&mut app, 40, 16);
    assert!(
        !support::text_block(&closed).contains("old a"),
        "it would not close:\n{closed}"
    );
}

/// A hunk scrolled off the top pushes nothing, so the caret moves with the
/// text again.
///
/// The removed lines are drawn when the drawing reaches the line they belong
/// to, so a hunk left open above the top of the screen takes no rows at all
/// -- and a caret shifted down for rows nobody drew would be as wrong as one
/// not shifted for rows that were.
#[test]
fn a_hunk_above_the_screen_does_not_move_the_caret() {
    use obelus_app::app::App;
    use obelus_buffer::Buffer;
    use obelus_command::Command;

    let tail: String = (1..=40).map(|line| format!("keep {line:02}\n")).collect();
    let repository = Repository::new(
        "scrolled",
        &format!("one\nold a\nold b\nold c\nold d\nold e\n{tail}"),
    );
    repository.write(&format!("one\nnew a\nnew b\nnew c\nnew d\nnew e\n{tail}"));

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    support::lay_out(&mut app, 40, 12);
    support::press(&mut app, crossterm::event::KeyCode::Down);
    obelus_app::app::dispatch::dispatch(&mut app, Command::GitHunk);
    // Down the file until the hunk is off the top of the screen.
    for _ in 0..30 {
        support::press(&mut app, crossterm::event::KeyCode::Down);
    }

    let dump = support::render(&mut app, 40, 12);
    assert!(
        !support::text_block(&dump).contains("old a"),
        "the hunk is still on screen, so this proves nothing:\n{dump}"
    );
    let line = app.current_buffer().expect("a buffer").cursor().line.get() + 1;
    let gutter = support::text_block(&dump)
        .lines()
        .position(|row| {
            row.split('|')
                .nth(1)
                .and_then(|drawn| drawn.split_whitespace().next())
                .is_some_and(|number| number == line.to_string())
        })
        .unwrap_or_else(|| panic!("line {line} is not on screen:\n{dump}"))
        .saturating_sub(1);
    let caret = support::cursor_line(&dump)
        .split(',')
        .nth(1)
        .and_then(|row| row.parse::<usize>().ok())
        .expect("the caret's row");
    assert_eq!(
        caret, gutter,
        "the caret moved for rows nothing drew:\n{dump}"
    );
}

/// A run of added lines opens too, though it has nothing to show above
/// itself: opening a hunk is what puts the change type behind its lines, and
/// "which lines exactly are new here" is what the margin's one column is too
/// small to answer.
#[test]
fn added_lines_open_onto_their_own_colour() {
    use obelus_app::app::App;
    use obelus_buffer::Buffer;
    use obelus_command::Command;

    let repository = Repository::new("added", "one\ntwo\n");
    repository.write("one\nnew\ntwo\n");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    support::lay_out(&mut app, 40, 8);
    support::press(&mut app, crossterm::event::KeyCode::Down);

    // The palette lets it be chosen, and it opens.
    support::press_control(&mut app, 'p');
    assert!(
        app.picker()
            .expect("the palette")
            .matches()
            .any(|item| item.label == "show-change" && item.enabled),
        "not available on an added line"
    );
    support::press(&mut app, crossterm::event::KeyCode::Esc);

    let closed = support::render(&mut app, 40, 8);
    obelus_app::app::dispatch::dispatch(&mut app, Command::GitHunk);
    assert_eq!(app.note(), None, "it refused to open");
    let opened = support::render(&mut app, 40, 8);

    // The added line, and only it, is on a background it did not have
    // before: the row from the line number across to the end of the text.
    let background = |dump: &str, row: usize| {
        let letter = support::style_block(dump)
            .lines()
            .filter(|row| !row.is_empty())
            .nth(row)
            .and_then(|row| row.chars().nth(3 + 6))
            .expect("a style cell");
        support::legend_block(dump)
            .lines()
            .find(|line| line.trim_start().starts_with(letter))
            .and_then(|line| line.split("bg=").nth(1))
            .map(str::to_string)
            .expect("a background")
    };
    assert_ne!(
        background(&opened, 1),
        background(&closed, 1),
        "the added line is not on a colour of its own:\n{opened}"
    );
    assert_eq!(
        background(&opened, 0),
        background(&closed, 0),
        "the line above the hunk was tinted too:\n{opened}"
    );
    assert_eq!(
        background(&opened, 2),
        background(&closed, 2),
        "the line below the hunk was tinted too:\n{opened}"
    );

    // And it is not the colour a *modified* line would get: the whole point
    // of the tint is which of the three claims this row is making.
    let repository = Repository::new("changed", "one\ntwo\n");
    repository.write("one\ntwo changed\n");
    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    support::lay_out(&mut app, 40, 8);
    support::press(&mut app, crossterm::event::KeyCode::Down);
    obelus_app::app::dispatch::dispatch(&mut app, Command::GitHunk);
    let modified = support::render(&mut app, 40, 8);
    // Row 1 there is the line it *replaced*, which the open hunk shows
    // above it, so the changed line itself is row 2.
    assert!(
        support::text_block(&modified)
            .lines()
            .nth(3)
            .is_some_and(|row| row.contains("two changed")),
        "the changed line is not where this test thinks:\n{modified}"
    );
    assert_ne!(
        background(&opened, 1),
        background(&modified, 2),
        "new and changed lines open onto the same colour:\n{modified}"
    );

    // The same command closes it again, tint and all.
    obelus_app::app::dispatch::dispatch(&mut app, Command::GitHunk);
    let shut = support::render(&mut app, 40, 8);
    assert_eq!(
        background(&shut, 1),
        background(&closed, 1),
        "it would not close:\n{shut}"
    );
}

/// Stepping between the changes in a file: what the arrows do at the scale
/// of the diff. A reader who has just come back to a file wants the changes,
/// not the lines, and hunting for the next mark in the margin by scrolling
/// is the thing this replaces.
#[test]
fn the_changes_can_be_stepped_through() {
    use crossterm::event::KeyCode;
    use obelus_app::app::App;
    use obelus_buffer::Buffer;
    use obelus_command::Command;

    // Three changes, far enough apart to be three hunks, in a file long
    // enough that they are not all on screen at once.
    let mut committed = String::new();
    for line in 0..60 {
        committed.push_str(&format!("line {line}\n"));
    }
    let repository = Repository::new("steps", &committed);
    let mut working = committed.clone();
    for line in [5, 20, 21, 22, 50] {
        working = working.replace(&format!("line {line}\n"), &format!("changed {line}\n"));
    }
    repository.write(&working);

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    support::lay_out(&mut app, 40, 12);
    let line = |app: &App| app.current_buffer().expect("a file").cursor().line.get();
    assert_eq!(line(&app), 0);

    // Down: each change in turn, by its first line, and the middle one
    // counts once however many lines it has.
    support::press_alt(&mut app, 'n');
    assert_eq!(line(&app), 5, "not the first change");
    support::press_alt(&mut app, 'n');
    assert_eq!(line(&app), 20, "not the second change");
    support::press_alt(&mut app, 'n');
    assert_eq!(line(&app), 50, "the middle change was counted twice");

    // And nothing below the last one: a wrap back to the top would look
    // like a key that did nothing while losing the reader's place. The key
    // does nothing at all, because the command is not offered here -- and
    // says nothing either, which is what every other key that cannot move
    // does.
    support::press_alt(&mut app, 'n');
    assert_eq!(line(&app), 50, "it wrapped around");
    assert_eq!(app.note(), None, "a key that did nothing said so");
    // The palette is where that is answered: the row is there to be found,
    // and it is dim because it cannot be chosen from here.
    support::press_control(&mut app, 'p');
    let rows: Vec<(String, bool)> = app
        .picker()
        .expect("the palette")
        .matches()
        .map(|item| (item.label.clone(), item.enabled))
        .collect();
    support::press(&mut app, KeyCode::Esc);
    let listed = |name: &str| {
        rows.iter()
            .find(|(label, _)| label == name)
            .map(|(_, enabled)| *enabled)
    };
    assert_eq!(
        listed("go-to-previous-change"),
        Some(true),
        "not available with changes above: {rows:?}"
    );
    assert_eq!(
        listed("go-to-next-change"),
        Some(false),
        "available with nothing below: {rows:?}"
    );

    // Up, and from inside a long change: to the top of that change first,
    // then to the one before it.
    support::press_alt(&mut app, 'p');
    assert_eq!(line(&app), 20);
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Down);
    assert_eq!(line(&app), 22, "still inside the middle change");
    support::press_alt(&mut app, 'p');
    assert_eq!(line(&app), 20, "not the top of the change being read");
    support::press_alt(&mut app, 'p');
    assert_eq!(line(&app), 5);
    support::press_alt(&mut app, 'p');
    assert_eq!(line(&app), 5, "it wrapped around");
    assert_eq!(app.note(), None, "a key that did nothing said so");

    // A leap, so the history brings the reader back where they were: to
    // where the last step started, and then to where the one before it did
    // -- including the line the reader had walked to by hand.
    obelus_app::app::dispatch::dispatch(&mut app, Command::GoBack);
    assert_eq!(line(&app), 20, "the step did not record where it left");
    obelus_app::app::dispatch::dispatch(&mut app, Command::GoBack);
    assert_eq!(line(&app), 22, "the step before that recorded nothing");

    // And a change the reader had to leap to arrives in the middle of the
    // screen, not against an edge: what a change means is the code around
    // it, and a hunk on the last row has half of that missing.
    obelus_app::app::dispatch::dispatch(&mut app, Command::GitNext);
    let dump = support::render(&mut app, 40, 12);
    let rows: Vec<&str> = support::text_block(&dump)
        .lines()
        .filter(|row| !row.is_empty())
        .collect();
    let at = rows
        .iter()
        .position(|row| row.contains("changed 50"))
        .unwrap_or_else(|| panic!("the change it went to is not on screen:\n{dump}"));
    assert!(
        at > 0 && at < rows.len() - 2,
        "the change it leapt to is against an edge, at row {at}:\n{dump}"
    );
}

/// The changes in the whole file, in a column of its own inside the bar.
///
/// One column, the same width as the margin on the other side. Its mark
/// leans the other way, away from the bar beside it and towards the text:
/// two thin strokes with a gap read as two things, where a mark against the
/// bar would read as one thick bar. The rows are lines of the file, not
/// rows of the screen: the margin says what changed here, this says where
/// else to look.
///
/// Inside the bar rather than outside it, so that the bar is the last
/// column -- which is where every list in obelus puts its own, and what
/// keeps them in one line when a list opens over a file.
#[test]
fn the_map_beside_the_bar_shows_the_whole_file() {
    use obelus_app::app::App;
    use obelus_buffer::Buffer;

    // Long enough that the change is off screen, which is what this is for.
    let mut committed = String::new();
    for line in 0..60 {
        committed.push_str(&format!("line {line}\n"));
    }
    let repository = Repository::new("map", &committed);
    repository.write(&committed.replace("line 50\n", "changed 50\n"));

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    support::lay_out(&mut app, 30, 12);
    let dump = support::render(&mut app, 30, 12);
    let rows: Vec<Vec<char>> = support::text_block(&dump)
        .lines()
        .filter(|row| !row.is_empty())
        .map(|row| row.chars().collect())
        .collect();
    // The bar is the last column and the map the one before it.
    let cell = |row: &Vec<char>, back: usize| row.get(row.len() - back).copied().unwrap_or(' ');

    assert!(
        rows.iter()
            .take(10)
            .all(|row| matches!(cell(row, 1), '\u{2502}' | '\u{2588}')),
        "the bar is not the last column:\n{dump}"
    );
    assert!(
        rows.iter().take(10).any(|row| cell(row, 1) == '\u{2588}'),
        "the thumb is not on the bar:\n{dump}"
    );

    // The change is fifty lines down a sixty-line file, so it belongs eight
    // rows down a ten-row column -- and nowhere else, on a screen where
    // nothing visible has changed at all.
    let marked: Vec<usize> = (0..10)
        .filter(|row| cell(&rows[*row], 2) == '\u{258c}')
        .collect();
    assert_eq!(
        marked,
        vec![8],
        "the map is not showing the change:\n{dump}"
    );

    // And a change of many lines takes as many rows as it covers of the
    // file: a map that marked only where each hunk *starts* would show a
    // twenty-line rewrite and a one-line fix identically.
    let mut long = committed.clone();
    for line in 20..40 {
        long = long.replace(&format!("line {line}\n"), &format!("changed {line}\n"));
    }
    repository.write(&long);
    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    support::lay_out(&mut app, 30, 12);
    let dump = support::render(&mut app, 30, 12);
    let rows: Vec<Vec<char>> = support::text_block(&dump)
        .lines()
        .filter(|row| !row.is_empty())
        .map(|row| row.chars().collect())
        .collect();
    let marked: Vec<usize> = (0..10)
        .filter(|row| cell(&rows[*row], 2) == '\u{258c}')
        .collect();
    assert_eq!(
        marked,
        vec![3, 4, 5, 6],
        "a twenty-line change did not take the rows it covers:\n{dump}"
    );
}

/// Where obelus draws a run of changes, against where git draws it.
///
/// Over this repository's own history rather than a hand-written pair. A
/// minimal diff still has choices in it -- a block inserted where the lines
/// around it repeat can be written as starting a line or two earlier, and
/// one change can be written as two hunks with a line between them -- and
/// which reading you get is what decides which lines are marked in the
/// margin. That ambiguity does not show up in a sample anybody would write
/// by hand: every five-line case I tried is drawn the same way with or
/// without the tidying. In real code it shows up constantly -- of the
/// file diffs in the last few dozen commits, a quarter land somewhere git
/// does not put them if the diff is used as the algorithm leaves it.
///
/// Over a window of commits rather than a fixed list, and only over the
/// files still on disk: a commit that moved everything contributes nothing
/// comparable, so the window has to be wide enough to see past one.
///
/// Skipped where there is no history to read, which is what a tarball
/// without a `.git` is.
#[test]
fn a_run_of_changes_is_where_git_draws_it() {
    let root = std::path::PathBuf::from(env!("OBELUS_TREE"));
    let git = |arguments: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(arguments)
            .output()
            .expect("running git");
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let commits: Vec<String> = git(&["log", "-60", "--format=%H"])
        .lines()
        .map(str::to_string)
        .collect();
    if commits.len() < 2 {
        return;
    }

    // `@@ -old,count +new,count @@`, as the runs obelus would name.
    let headers = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|line| line.starts_with("@@"))
            .map(|line| {
                let mut parts = line.split(' ').skip(1);
                let before = parts.next().unwrap_or("-0,0").trim_start_matches('-');
                let after = parts.next().unwrap_or("+0,0").trim_start_matches('+');
                let count = |part: &str| {
                    part.split_once(',')
                        .map_or(1, |(_, many)| many.parse().unwrap_or(1))
                };
                let start: usize = after
                    .split(',')
                    .next()
                    .and_then(|start| start.parse().ok())
                    .unwrap_or(0);
                let (added, removed): (usize, usize) = (count(after), count(before));
                // git names the line *before* a pure deletion; obelus marks
                // the line it sits in front of.
                let at = if added == 0 { start + 1 } else { start };
                format!("{at}+{added}-{removed}")
            })
            .collect()
    };

    let mut checked = 0usize;
    for id in &commits {
        let object = gix::ObjectId::from_hex(id.as_bytes()).expect("a commit");
        for row in git(&["show", "--numstat", "--format=", id]).lines() {
            let Some(name) = row.split('\t').nth(2) else {
                continue;
            };
            let path = root.join(name);
            // A file the commit added or deleted has only one side, and
            // one that is not text has no lines: neither is a diff.
            let (Some(before), Some(after)) = (
                obelus_git::history::text_before(&root, object, &path),
                obelus_git::history::text_at(&root, object, &path),
            ) else {
                continue;
            };
            let ours: Vec<String> = obelus_git::change::Changes::between(&before, &after)
                .hunks()
                .iter()
                .map(|hunk| {
                    format!(
                        "{}+{}-{}",
                        hunk.line.get() + 1,
                        hunk.lines,
                        hunk.removed.len()
                    )
                })
                .collect();
            let theirs = headers(&git(&[
                "-c",
                "diff.algorithm=histogram",
                "show",
                "-U0",
                "--format=",
                id,
                "--",
                name,
            ]));
            assert!(
                same_runs(&ours, &theirs, &after),
                "{} {name}\n  obelus {ours:?}\n  git    {theirs:?}",
                &id[..8]
            );
            checked += 1;
        }
    }
    assert!(
        checked > 20,
        "only {checked} diffs were compared, which is not enough of them"
    );
}

/// Whether two accounts of the same change describe the same change.
///
/// The same runs, or runs that differ only in where an insertion among
/// identical lines was anchored. A block put into a list of like blocks --
/// a package added to a lockfile, an arm added to a match -- can be drawn
/// starting at any line of the run it slides through, and every one of
/// those reconstructs the same file. git slides one way and obelus the
/// other; neither is wrong, and a test that insisted would be asserting a
/// thing neither tool promises.
///
/// Everything else is compared exactly, which is the whole point: where a
/// change *is* has no such freedom.
fn same_runs(ours: &[String], theirs: &[String], after: &str) -> bool {
    if ours == theirs {
        return true;
    }
    if ours.len() != theirs.len() {
        return false;
    }
    let lines: Vec<&str> = after.lines().collect();
    ours.iter().zip(theirs).all(|(ours, theirs)| {
        if ours == theirs {
            return true;
        }
        let read = |run: &str| -> Option<(usize, usize, usize)> {
            let (at, rest) = run.split_once('+')?;
            let (added, removed) = rest.split_once('-')?;
            Some((at.parse().ok()?, added.parse().ok()?, removed.parse().ok()?))
        };
        let (Some((ours, added, removed)), Some((theirs, theirs_added, theirs_removed))) =
            (read(ours), read(theirs))
        else {
            return false;
        };
        // Only a pure insertion of the same lines can have slid.
        if (added, removed) != (theirs_added, theirs_removed) || removed != 0 {
            return false;
        }
        // And it slid only if every line it passed over is the same as the
        // one that took its place: that is what makes both drawings of it
        // produce the same file.
        let (from, to) = (ours.min(theirs), ours.max(theirs));
        (from..to).all(|at| {
            // The runs count from one.
            lines.get(at - 1) == lines.get(at - 1 + added)
        })
    })
}

/// Each file is counted against its own committed version./// Each file is
/// counted against its own committed version.
///
/// Walking a tree to a path moves it: gix leaves the tree on the subtree it
/// descended into, so one tree asked twice answers the second question from
/// wherever the first left it. Every file after the first then looked like
/// a file the commit does not have -- which is drawn as a file where every
/// line was just added, on a list whose whole job is to say how much each
/// one changed.
#[test]
fn every_file_is_counted_against_its_own_committed_version() {
    use obelus_git::counted_against_head;

    let repository = Repository::new("counts-many", "one\ntwo\nthree\n");
    // The second one a directory down, so reaching it is a walk rather
    // than a lookup in the root tree.
    let inner = repository.directory().join("inner");
    std::fs::create_dir_all(&inner).expect("a directory");
    let deep = inner.join("deep.rs");
    std::fs::write(&deep, "a\nb\n").expect("a file");
    repository.run(&["add", "inner/deep.rs"]);
    repository.run(&["commit", "--quiet", "-m", "both"]);

    repository.write("one\ntwo\nthree\nfour\n");
    std::fs::write(&deep, "a\nb\nc\nd\n").expect("rewriting it");

    let file = repository.path();
    for order in [
        vec![file.clone(), deep.clone()],
        vec![deep.clone(), file.clone()],
    ] {
        let counts = counted_against_head(&order);
        assert_eq!(
            counts.get(&file).copied(),
            Some((1, 0)),
            "the file asked about in position {:?}: {counts:?}",
            order.iter().position(|path| *path == file)
        );
        assert_eq!(
            counts.get(&deep).copied(),
            Some((2, 0)),
            "the nested file: {counts:?}"
        );
    }

    // A file the commit really does not have is all of it added, which is
    // the answer the bug above made everything look like.
    let fresh = repository.directory().join("fresh.rs");
    std::fs::write(&fresh, "x\ny\n").expect("a new file");
    let counts = counted_against_head(&[file.clone(), fresh.clone()]);
    assert_eq!(counts.get(&fresh).copied(), Some((2, 0)));
    assert_eq!(counts.get(&file).copied(), Some((1, 0)));
}

/// A list of files says which of them have been touched. A project's file
/// list is mostly files nobody has changed, and the few that have been are
/// what a reader is usually looking for.
#[test]
fn a_list_of_files_says_which_have_changed() {
    use obelus_git::{FileStatus, statuses};

    let repository = Repository::new("statuses", "one\n");
    repository.write("one\ntwo\n");
    std::fs::write(repository.directory.join("new.rs"), "fn new() {}\n").expect("a new file");

    let found = statuses(&repository.directory);
    assert_eq!(
        found.get(&repository.path()).cloned(),
        Some(FileStatus::Changed.into()),
        "a tracked file that differs: {found:?}"
    );
    assert_eq!(
        found.get(&repository.directory.join("new.rs")).cloned(),
        Some(FileStatus::New.into()),
        "a file git has never seen: {found:?}"
    );

    // A directory nothing in it is tracked is *not* what is listed: git's
    // own report collapses one into a single line, which is right for a
    // person reading a terminal and wrong for a list of files to open --
    // picking the folder would do nothing, and the files inside it would
    // be the ones nobody could reach.
    let new_module = repository.directory.join("module");
    std::fs::create_dir_all(new_module.join("inner")).expect("a new directory");
    std::fs::write(new_module.join("mod.rs"), "pub mod inner;\n").expect("a file in it");
    std::fs::write(new_module.join("inner").join("deep.rs"), "fn deep() {}\n").expect("another");
    let found = statuses(&repository.directory);
    assert_eq!(
        found.get(&new_module.join("mod.rs")).cloned(),
        Some(FileStatus::New.into()),
        "the file in a new directory is not listed: {found:?}"
    );
    assert_eq!(
        found
            .get(&new_module.join("inner").join("deep.rs"))
            .cloned(),
        Some(FileStatus::New.into()),
        "a file further down is not listed either: {found:?}"
    );
    // And a repository checked out inside the tree, which is the one thing
    // a walk of every file still reports as a directory: git cannot look
    // inside somebody else's repository, so it names the directory. It is
    // not a file either.
    let nested = repository.directory.join("vendored");
    std::fs::create_dir_all(&nested).expect("a directory");
    std::process::Command::new("git")
        .arg("-C")
        .arg(&nested)
        .args(["init", "--quiet", "--initial-branch=master"])
        .output()
        .expect("running git");
    std::fs::write(nested.join("theirs.rs"), "fn theirs() {}\n").expect("a file in it");

    let found = statuses(&repository.directory);
    assert!(
        !found.keys().any(|path| path.is_dir()),
        "a folder is in the list of files: {:?}",
        found.keys().collect::<Vec<_>>()
    );

    // Keyed by absolute path, because git reports paths relative to the
    // repository root and obelus knows files by where they are: a map keyed
    // by one and read with the other silently matches nothing.
    assert!(
        found.keys().all(|path| path.is_absolute()),
        "{:?}",
        found.keys().collect::<Vec<_>>()
    );

    // And a directory that is not a repository has nothing to say, rather
    // than failing.
    assert!(statuses(std::path::Path::new("/")).is_empty());
}

/// Who last changed the line the cursor is on, at the end of that line.
///
/// One line rather than all of them: on every line it is a wall of grey
/// beside the code, and a reader who wants the name for a line can put the
/// cursor on it -- which is where their attention already is.
#[test]
fn the_blame_sits_at_the_end_of_the_cursor_line() {
    use obelus_app::{app::App, event::Event};
    use obelus_buffer::Buffer;
    use obelus_git::Blamed;

    let repository = Repository::new("blame", "short\nshort\n");
    let buffer = Buffer::open(&repository.path()).expect("opening it");
    let path = buffer.path().to_path_buf();
    let mut app = App::new(vec![buffer]);
    support::lay_out(&mut app, 44, 8);

    let bare = support::render(&mut app, 44, 8);
    assert!(
        !support::text_block(&bare).contains("Ada"),
        "a name before anything answered:\n{bare}"
    );

    let long_ago = 1;
    app.handle(Event::Git(obelus_git::Event::Blamed {
        at: None,
        path,
        lines: vec![
            Some(Blamed {
                id: gix::ObjectId::null(gix::hash::Kind::Sha1),
                line: 0,
                who: "Ada".to_string(),
                when: long_ago,
            }),
            None,
        ],
    }));
    let dump = support::render(&mut app, 44, 8);
    let rows: Vec<&str> = support::text_block(&dump)
        .lines()
        .filter(|row| !row.is_empty())
        .collect();
    assert!(
        rows[0].contains("Ada \u{b7} ") && rows[0].contains("ago"),
        "no note on the cursor's line:\n{dump}"
    );
    // Right-aligned rather than hung off the text, which is what keeps it
    // from moving left and right as the cursor goes down the file. Said by
    // widening the screen: the note moves with the right-hand edge and the
    // line's own text does not move at all.
    let note = rows[0].find("Ada").expect("the note");
    let ends = rows[0].find("short").expect("the line") + "short".len();
    assert!(
        note > ends + 2,
        "the note is hung off the text rather than right-aligned:\n{dump}"
    );
    let wider = support::render(&mut app, 48, 8);
    let row = support::text_block(&wider)
        .lines()
        .find(|row| row.contains("Ada"))
        .expect("the note at the wider size")
        .to_string();
    assert_eq!(
        row.find("Ada").expect("the note"),
        note + 4,
        "the note did not move with the right-hand edge:\n{wider}"
    );
    assert_eq!(
        row.find("short").expect("the line"),
        ends - "short".len(),
        "the line moved:\n{wider}"
    );

    // And nowhere else: the second line has a name in the blame and no note
    // on screen, because the cursor is not on it.
    assert!(
        !rows[1].contains("Ada"),
        "a line the cursor is not on has a note:\n{dump}"
    );

    // Moved onto a line no commit accounts for, there is nothing to say --
    // rather than the name from the line above it.
    support::press(&mut app, crossterm::event::KeyCode::Down);
    let moved = support::render(&mut app, 44, 8);
    assert!(
        !support::text_block(&moved).contains("Ada"),
        "a line with no commit borrowed a name:\n{moved}"
    );
}

/// The blame is about the committed file, so the lines a reader has changed
/// have to be mapped out of the way. Without that, one uncommitted line
/// above shifts every name below it by one -- an answer that is confidently
/// wrong rather than absent.
#[test]
fn a_line_the_reader_changed_has_no_name() {
    use obelus_app::{app::App, event::Event};
    use obelus_buffer::Buffer;
    use obelus_git::Blamed;

    // Committed: three lines. Working tree: a fourth inserted in the
    // *middle*, so the lines below it have moved down by one -- and in the
    // middle rather than at the top on purpose, because an insertion at the
    // top maps to a line before the first one and would be caught by the
    // arithmetic whether or not the rule that drops changed lines exists.
    let repository = Repository::new("mapped", "one\ntwo\nthree\n");
    repository.write("one\ninserted\ntwo\nthree\n");
    let buffer = Buffer::open(&repository.path()).expect("opening it");
    let path = buffer.path().to_path_buf();
    let mut app = App::new(vec![buffer]);
    support::lay_out(&mut app, 44, 8);

    let who = |name: &str| {
        Some(Blamed {
            id: gix::ObjectId::null(gix::hash::Kind::Sha1),
            line: 0,
            who: name.to_string(),
            when: 1,
        })
    };
    app.handle(Event::Git(obelus_git::Event::Blamed {
        at: None,
        path,
        lines: vec![who("Ada"), who("Bob"), who("Cai")],
    }));

    // The cursor walks down, because the note is only ever on its line.
    let note = |app: &mut App| {
        let dump = support::render(app, 44, 8);
        support::text_block(&dump)
            .lines()
            .find(|row| row.contains('\u{b7}'))
            .map(str::to_string)
    };

    let first = note(&mut app).unwrap_or_default();
    assert!(
        first.contains("one") && first.contains("Ada"),
        "the first line is not blamed on Ada: {first:?}"
    );
    support::press(&mut app, crossterm::event::KeyCode::Down);
    assert_eq!(
        note(&mut app),
        None,
        "the inserted line took a name from the committed file"
    );
    for (text, name) in [("two", "Bob"), ("three", "Cai")] {
        support::press(&mut app, crossterm::event::KeyCode::Down);
        let row = note(&mut app).unwrap_or_default();
        assert!(
            row.contains(text) && row.contains(name),
            "line {text:?} is not blamed on {name}: {row:?}"
        );
    }
}

/// A long line keeps its own space: the note is dropped rather than written
/// over the code it is about.
#[test]
fn a_line_too_long_for_a_note_keeps_its_code() {
    use obelus_app::{app::App, event::Event};
    use obelus_buffer::Buffer;
    use obelus_git::Blamed;

    let long = format!("// {}\n", "x".repeat(60));
    let repository = Repository::new("long", &long);
    let buffer = Buffer::open(&repository.path()).expect("opening it");
    let path = buffer.path().to_path_buf();
    let mut app = App::new(vec![buffer]);
    support::lay_out(&mut app, 44, 8);
    app.handle(Event::Git(obelus_git::Event::Blamed {
        at: None,
        path,
        lines: vec![Some(Blamed {
            id: gix::ObjectId::null(gix::hash::Kind::Sha1),
            line: 0,
            who: "Ada".to_string(),
            when: 1,
        })],
    }));

    let dump = support::render(&mut app, 44, 8);
    let text = support::text_block(&dump);
    assert!(
        !text.contains("Ada"),
        "the note was written over the line:\n{dump}"
    );
    // Every column the line has on this screen is the line's own: without
    // wrapping the rest of it is off to the right, and nothing has been
    // taken off the end of what is showing to make room for a note.
    let row = text
        .lines()
        .find(|row| row.contains("//"))
        .expect("the line");
    assert!(
        row.trim_end().ends_with('x'),
        "the line lost its last columns to a note about it:\n{dump}"
    );
}

/// The names can be turned off, because on a narrow screen or in a file
/// being read closely they are the noisiest thing obelus draws.
///
/// The setting is what turns them off, and there is no command for it: a
/// switch that should outlive the session is a setting, and one a key owned
/// as well went back on by itself the next time anything on the settings
/// page changed.
#[test]
fn the_names_can_be_turned_off() {
    use obelus_app::{app::App, event::Event};
    use obelus_buffer::Buffer;
    use obelus_config::Config;
    use obelus_git::Blamed;

    let repository = Repository::new("off", "short\n");
    let buffer = Buffer::open(&repository.path()).expect("opening it");
    let path = buffer.path().to_path_buf();
    let mut app = App::new(vec![buffer]);
    support::lay_out(&mut app, 44, 8);
    app.handle(Event::Git(obelus_git::Event::Blamed {
        at: None,
        path,
        lines: vec![Some(Blamed {
            id: gix::ObjectId::null(gix::hash::Kind::Sha1),
            line: 0,
            who: "Ada".to_string(),
            when: 1,
        })],
    }));
    assert!(support::text_block(&support::render(&mut app, 44, 8)).contains("Ada"));

    app.configure(
        Config {
            blame_margin: false,
            ..Config::default()
        },
        Vec::new(),
    );
    let off = support::render(&mut app, 44, 8);
    assert!(
        !support::text_block(&off).contains("Ada"),
        "the names are still there:\n{off}"
    );

    // And back on without asking again: the answer is still in hand.
    app.configure(Config::default(), Vec::new());
    assert!(
        support::text_block(&support::render(&mut app, 44, 8)).contains("Ada"),
        "the names did not come back"
    );
}

/// The blame comes from `gix`, against a repository built for the test: a
/// real walk of a real history, which is the only thing that says the
/// library is being used correctly.
#[test]
fn a_real_repository_gives_a_real_blame() {
    let repository = Repository::new("history", "first\nsecond\n");
    // A second commit that changes only the second line.
    repository.write("first\nchanged\n");
    repository.commit("the second commit");

    let lines = obelus_git::blame::lines_of(&repository.path(), None).expect("a blame");
    assert_eq!(lines.len(), 2, "not one entry per line");
    let who: Vec<Option<String>> = lines
        .iter()
        .map(|line| line.as_ref().map(|blamed| blamed.who.clone()))
        .collect();
    assert_eq!(
        who,
        vec![Some("obelus".to_string()), Some("obelus".to_string())],
        "not the author the commits were made by"
    );

    // The two lines came from two different commits, and the times say so:
    // the second is not older than the first.
    let times: Vec<i64> = lines
        .iter()
        .map(|line| line.as_ref().map_or(0, |blamed| blamed.when))
        .collect();
    assert!(
        times[1] >= times[0],
        "the later commit is dated before the earlier one: {times:?}"
    );

    // And a file git has never seen has no blame at all, which is a
    // different thing from an empty one.
    let stranger = repository.directory.join("unknown.rs");
    std::fs::write(&stranger, "nothing\n").expect("writing");
    assert!(obelus_git::blame::lines_of(&stranger, None).is_none());
}

/// A commit in another window is a commit in this one.
///
/// obelus does not split its own window -- the terminal does -- so several
/// of them on one project is the ordinary way to work, and the reader's own
/// shell is in there too. What has changed in a file is a question about the
/// file *and* about the commit it is being compared with, and obelus only
/// ever asked again when the file changed: the margin went on drawing a diff
/// against a commit that was no longer the one the file is against.
#[test]
fn a_commit_from_somewhere_else_empties_the_margin() {
    let repository = Repository::new("moved", "one\ntwo\nthree\n");
    repository.write("one\ntwo\nthree\nfour\n");
    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("the file")]);
    support::lay_out(&mut app, 40, 10);
    assert!(
        app.changes().is_some_and(|changes| !changes.is_empty()),
        "the added line is not in the margin"
    );

    // Committed from somewhere else, with nothing touching the file: the
    // line is no longer new, and the only thing that says so is git.
    repository.commit("the fourth line");
    app.handle(Event::Watched(obelus_watch::Changed {
        path: repository.directory.join(".git").join("HEAD"),
    }));
    support::lay_out(&mut app, 40, 10);
    assert!(
        app.changes().is_none_or(Changes::is_empty),
        "the margin is still drawing a diff against the old commit"
    );
}

/// Two changes on one screen, which is what a reader comparing them needs.
///
/// One slot meant opening the second closed the first, and the two things
/// a reader most wants side by side are the two they are deciding between.
/// Each key closes the one it is pressed on and leaves the other alone.
#[test]
fn two_hunks_can_be_open_at_once() {
    use crossterm::event::KeyCode;
    use obelus_app::app::{App, dispatch};
    use obelus_buffer::Buffer;
    use obelus_command::Command;

    let repository = Repository::new("two-hunks", "one\nold a\nthree\nfour\nfive\nold b\nseven\n");
    repository.write("one\nnew a\nthree\nfour\nfive\nnew b\nseven\n");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    support::lay_out(&mut app, 40, 16);

    // Down onto the first change and open it, then on to the second.
    support::press(&mut app, KeyCode::Down);
    dispatch::dispatch(&mut app, Command::GitHunk);
    for _ in 0..4 {
        support::press(&mut app, KeyCode::Down);
    }
    dispatch::dispatch(&mut app, Command::GitHunk);

    let dump = support::render(&mut app, 40, 16);
    let text = support::text_block(&dump);
    assert!(
        text.contains("old a") && text.contains("old b"),
        "the second hunk closed the first:\n{text}"
    );
    assert_eq!(app.opened_hunks().len(), 2, "not two blocks open:\n{text}");

    // The key closes the one it is pressed on, and only that one.
    dispatch::dispatch(&mut app, Command::GitHunk);
    let dump = support::render(&mut app, 40, 16);
    let text = support::text_block(&dump);
    assert!(
        text.contains("old a"),
        "closing the second closed the first too:\n{text}"
    );
    assert!(
        !text.contains("old b"),
        "the second would not close:\n{text}"
    );
    assert_eq!(app.opened_hunks().len(), 1, "not one block left");
}

/// A selection made in one block is drawn in that block and nowhere else.
///
/// The blocks share nothing but a shape: a span is a pair of offsets into
/// one text, and drawn against another it marks whichever characters
/// happen to sit at those offsets -- lines nobody chose, in a block the
/// reader is not even in.
#[test]
fn a_selection_stays_in_the_block_it_was_made_in() {
    use crossterm::event::KeyCode;
    use obelus_app::app::{App, dispatch};
    use obelus_buffer::Buffer;
    use obelus_command::Command;

    let repository = Repository::new(
        "two-selections",
        "one\nold a\nthree\nfour\nfive\nold b\nseven\n",
    );
    repository.write("one\nnew a\nthree\nfour\nfive\nnew b\nseven\n");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    support::lay_out(&mut app, 40, 16);
    support::press(&mut app, KeyCode::Down);
    dispatch::dispatch(&mut app, Command::GitHunk);
    for _ in 0..4 {
        support::press(&mut app, KeyCode::Down);
    }
    dispatch::dispatch(&mut app, Command::GitHunk);

    /// The styles of the row holding a piece of text.
    fn styles_of(dump: &str, needle: &str) -> String {
        support::text_block(dump)
            .lines()
            .zip(support::style_block(dump).lines())
            .find(|(row, _)| row.contains(needle))
            .map(|(_, styles)| styles.to_string())
            .expect("a row holding it")
    }

    let before = styles_of(&support::render(&mut app, 40, 16), "old a");

    // Into the second block, and select a line of it.
    support::press(&mut app, KeyCode::Up);
    support::press_shift(&mut app, KeyCode::Right);
    support::press_shift(&mut app, KeyCode::Right);
    let dump = support::render(&mut app, 40, 16);
    assert_ne!(
        styles_of(&dump, "old b"),
        before,
        "nothing was selected anywhere:\n{}",
        support::text_block(&dump)
    );
    assert_eq!(
        styles_of(&dump, "old a"),
        before,
        "the selection was drawn in the other block too:\n{}",
        support::text_block(&dump)
    );
}

/// A history is a walk of the commits, newest first, and asking about one
/// path keeps the commits that changed it.
#[test]
fn a_history_is_the_commits_that_touched_it() {
    use obelus_git::history;

    let repository = Repository::new("history-walk", "one\n");
    repository.write("one\ntwo\n");
    repository.commit("the second");
    // A commit that leaves the file alone: another file changes instead.
    std::fs::write(repository.directory().join("other.rs"), "elsewhere\n").expect("the other");
    repository.commit_all("something else");
    repository.write("one\ntwo\nthree\n");
    repository.commit("the third");

    let root = repository.directory();
    let whole: Vec<String> = history::of(&root, None, 10)
        .iter()
        .map(|commit| commit.subject.clone())
        .collect();
    assert_eq!(
        whole,
        ["the third", "something else", "the second", "committed"],
        "not the project's commits, newest first"
    );

    let file: Vec<String> = history::of(&root, Some(&repository.path()), 10)
        .iter()
        .map(|commit| commit.subject.clone())
        .collect();
    assert_eq!(
        file,
        ["the third", "the second", "committed"],
        "the commit that left this file alone is in its history"
    );
}

/// A limit on the answer and a limit on the walk. A file touched once, a
/// long way back, must not cost a walk of the whole project before a list
/// of it can be drawn.
#[test]
fn a_history_stops_where_it_was_asked_to() {
    use obelus_git::history;

    let repository = Repository::new("history-limit", "one\n");
    for line in 0..6 {
        repository.write(&format!("one\n{line}\n"));
        repository.commit(&format!("number {line}"));
    }
    let root = repository.directory();
    let found = history::of(&root, None, 3);
    assert_eq!(found.len(), 3, "the limit was not kept");
    assert_eq!(found[0].subject, "number 5", "not the newest first");
}

/// What a commit says, split where a reader reads it: the first line is the
/// row, the rest is what they open it to read.
#[test]
fn a_commits_message_is_a_subject_and_a_body() {
    use obelus_git::history;

    let repository = Repository::new("history-message", "one\n");
    repository.write("two\n");
    repository.commit("a subject line\n\nand a body,\nover two lines.\n");

    let root = repository.directory();
    let found = history::of(&root, None, 1);
    assert_eq!(found[0].subject, "a subject line");
    assert_eq!(found[0].body, "and a body,\nover two lines.");
    assert_eq!(found[0].short().len(), 7, "not a short id");
    assert_eq!(found[0].who, "obelus");
}

/// The files a commit changed, which is what a row of the project's history
/// opens into -- and only the files. A directory turns up in the walk
/// because the walk goes through it, and a row nobody can open is a row in
/// the way.
#[test]
fn a_commit_opens_into_the_files_it_changed() {
    use obelus_git::{FileStatus, history};

    let repository = Repository::new("history-files", "one\n");
    std::fs::create_dir_all(repository.directory().join("deep")).expect("a directory");
    std::fs::write(repository.directory().join("deep/new.rs"), "new\n").expect("the new file");
    repository.write("one\ntwo\n");
    repository.commit_all("touching two");

    let root = repository.directory();
    let head = history::of(&root, None, 1);
    let files = history::files_in(&root, head[0].id);
    assert_eq!(
        files,
        [
            history::Touched {
                path: std::path::PathBuf::from("deep/new.rs"),
                status: FileStatus::New,
                was: None,
            },
            history::Touched {
                path: std::path::PathBuf::from("file.rs"),
                status: FileStatus::Changed,
                was: None,
            },
        ],
        "not the files it changed, and only the files"
    );
}

/// A file a commit moved is one file, and says where it came from.
///
/// Git records no rename: it is inferred from what a commit added and what
/// it removed, and without the inference the list shows the file twice --
/// once gone, once arriving under a name a reader has to pair up by eye --
/// and the lines it counts are the whole file taken away and the whole of
/// it put back.
///
/// Edited on the way, because a move with no edit is matched on content
/// alone and would pass with the search for near-misses turned off.
#[test]
fn a_file_a_commit_moved_is_one_row_that_says_where_it_was() {
    use obelus_git::{FileStatus, history};

    let lines: String = (0..40).map(|n| format!("line {n}\n")).collect();
    let repository = Repository::new("history-renamed", &lines);
    std::fs::create_dir_all(repository.directory().join("deep")).expect("a directory");
    std::fs::remove_file(repository.directory().join("file.rs")).expect("taking it away");
    std::fs::write(
        repository.directory().join("deep/moved.rs"),
        format!("{lines}one more line\n"),
    )
    .expect("the file in its new place");
    repository.commit_all("moving it");

    let root = repository.directory();
    let head = history::of(&root, None, 1);
    assert_eq!(
        history::files_in(&root, head[0].id),
        [history::Touched {
            path: std::path::PathBuf::from("deep/moved.rs"),
            status: FileStatus::Changed,
            was: Some(std::path::PathBuf::from("file.rs")),
        }],
        "a move read as a deletion and an unrelated arrival"
    );

    // And the lines it counts are the edit, not the whole file twice.
    let (added, removed) = history::counted_in(&root, head[0].id).expect("a count");
    assert_eq!(
        (added, removed),
        (1, 0),
        "the move was counted as forty lines gone and forty-one arriving"
    );
}

/// A file's history does not stop where the file was given its name.
///
/// The question a reader asks is about the file, and a file that was moved
/// is the same file: stopping at the move answers a question about a *path*
/// and looks like a complete answer to the one that was asked. Git records
/// no rename -- it infers one from what a commit added and removed, and
/// `git log --follow` is that inference -- so this has to make the same
/// one, or obelus's history is obelus's opinion.
///
/// Edited on the way, because a move with no edit is matched on content
/// alone and would pass with the search for near-misses turned off.
#[test]
fn a_files_history_goes_on_under_the_name_it_had_before() {
    use obelus_git::history;

    let lines: String = (0..40).map(|n| format!("line {n}\n")).collect();
    let repository = Repository::new("history-following", &lines);
    repository.write(&format!("{lines}an edit\n"));
    repository.commit("editing it where it was");

    std::fs::create_dir_all(repository.directory().join("deep")).expect("a directory");
    std::fs::remove_file(repository.directory().join("file.rs")).expect("taking it away");
    std::fs::write(
        repository.directory().join("deep/moved.rs"),
        format!("{lines}an edit\nand another\n"),
    )
    .expect("the file in its new place");
    repository.commit_all("moving it");

    let root = repository.directory();
    let found = history::of(&root, Some(&root.join("deep/moved.rs")), 50);
    let subjects: Vec<&str> = found.iter().map(|commit| commit.subject.as_str()).collect();
    assert_eq!(
        subjects,
        ["moving it", "editing it where it was", "committed"],
        "the history stopped where the file was renamed"
    );

    // The move is said on the commit that made it, and on no other: it
    // happened once, and a row that said it about a commit which did not
    // do it would be pointing at the wrong one.
    assert_eq!(
        found
            .iter()
            .map(|commit| commit.was.clone())
            .collect::<Vec<_>>(),
        [Some(std::path::PathBuf::from("file.rs")), None, None],
        "the move is not said where it happened"
    );

    // And every row opens, which is the whole point of walking past the
    // move: the name each commit had the file under is the name that
    // commit can be asked for it by, and it is not the name it has now.
    for commit in &found {
        let at = commit.at.clone().expect("a walk of one file names it");
        assert!(
            history::text_at(&root, commit.id, &root.join(&at)).is_some(),
            "{} offers a row that opens nothing: {}",
            commit.short(),
            at.display()
        );
    }
}

/// The names a file can be opened at are the names that have it.
///
/// Choosing one of these rows opens the file as that name has it, so a
/// name whose commit has not got the file is a row nobody can open -- and
/// on a tree that has just been reorganised that is most of them.
#[test]
fn a_name_without_the_file_is_not_offered_as_a_place_to_read_it() {
    use obelus_git::history;

    let repository = Repository::new("history-refs", "one\n");
    repository.run(&["branch", "before"]);
    std::fs::write(repository.directory().join("later.rs"), "later\n").expect("a second file");
    repository.commit_all("a file the branch never had");
    repository.run(&["tag", "after"]);

    let root = repository.directory();
    let names = |only: Option<&std::path::Path>| -> Vec<String> {
        let mut found: Vec<String> = history::refs_of(&root, only)
            .into_iter()
            .map(|reference| reference.name)
            .collect();
        found.sort();
        found
    };

    assert_eq!(
        names(None),
        ["after", "before", "master"],
        "not every name in the repository"
    );
    assert_eq!(
        names(Some(&root.join("later.rs"))),
        ["after", "master"],
        "a branch from before the file was written is offered as somewhere to read it"
    );
    assert_eq!(
        names(Some(&root.join("file.rs"))),
        ["after", "before", "master"],
        "a file every name has is not offered at all of them"
    );
}

/// A file as a commit had it, which is what choosing a row opens.
#[test]
fn a_file_can_be_read_as_a_commit_had_it() {
    use obelus_git::history;

    let repository = Repository::new("history-text", "first\n");
    repository.write("second\n");
    repository.commit("the second");

    let root = repository.directory();
    let found = history::of(&root, None, 2);
    assert_eq!(
        history::text_at(&root, found[0].id, &repository.path()).as_deref(),
        Some("second\n"),
        "not the file as the newest commit had it"
    );
    assert_eq!(
        history::text_at(&root, found[1].id, &repository.path()).as_deref(),
        Some("first\n"),
        "not the file as the older commit had it"
    );
}

/// Nothing to say, said as nothing: not a repository, and a path it has
/// never heard of.
#[test]
fn a_history_with_no_answer_is_empty() {
    use obelus_git::history;

    let outside = support::Scratch::new("nowhere");
    let elsewhere = outside.path().to_path_buf();
    assert!(history::of(&elsewhere, None, 10).is_empty());

    let repository = Repository::new("history-nothing", "one\n");
    let root = repository.directory();
    assert!(
        history::of(&root, Some(&root.join("never.rs")), 10).is_empty(),
        "a path git has never heard of has a history"
    );
    // And one that is not in this repository at all. Being unable to place
    // a path must not quietly widen the question to the whole project: an
    // answer about every commit, to a reader who asked about one file, is
    // the wrong answer told confidently.
    assert!(
        history::of(&root, Some(&elsewhere.join("outside.rs")), 10).is_empty(),
        "a path outside the repository was answered with the whole of it"
    );
}

/// The history is one view at two radii, and the keys land on the tab they
/// name.
#[test]
fn the_history_opens_at_the_radius_its_key_names() {
    use crossterm::event::KeyCode;
    use obelus_app::app::App;
    use obelus_buffer::Buffer;

    let repository = Repository::new("history-view", "one\n");
    repository.write("one\ntwo\n");
    repository.commit("the second");
    std::fs::write(repository.directory().join("other.rs"), "elsewhere\n").expect("the other");
    repository.commit_all("something else");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 70, 16);

    support::press_function(&mut app, 9);

    support::read_history(&mut app, &events);
    let picker = app.picker().expect("the history");
    // Two tabs about the file being read, and the arrow between them stays
    // inside one errand: its own history, and the places it can be seen
    // from. The project is the one view that stops being about this file,
    // and it has a key rather than a tab.
    assert_eq!(picker.tabs(), ["This file", "The refs"]);
    assert_eq!(picker.tab(), 0, "f9 did not open the file's own tab");
    let rows: Vec<String> = picker.matches().map(|item| item.label.clone()).collect();
    assert_eq!(
        rows,
        ["the second", "committed"],
        "not the commits that changed this file"
    );

    // A commit in this tab has nothing to open: it is already about one
    // file, and a mark offering to show which file that is would be a mark
    // repeating the tab's own name.
    assert!(
        app.picker()
            .expect("the history")
            .matches()
            .all(|item| item.marker.is_none()),
        "the file's own tab offers to open a commit"
    );

    // The other tab is a walk away, and walking onto it asks its own
    // question: not which commits changed this file, but which names point
    // at a commit it can be read from.
    support::press(&mut app, KeyCode::Tab);
    let rows: Vec<String> = app
        .picker()
        .expect("the history")
        .matches()
        .map(|item| item.label.clone())
        .collect();
    assert_eq!(rows, ["master"], "the refs tab shows the same commits");

    // The project is its own key, its own view, and its own single tab --
    // reached by leaving this one first, because obelus's commands do not
    // run from inside a list.
    support::press(&mut app, KeyCode::Esc);
    support::press_function(&mut app, 10);
    support::read_history(&mut app, &events);
    let picker = app.picker().expect("the history");
    assert_eq!(picker.tabs(), ["The project"]);
    assert!(
        picker.matches().any(|item| item.opens.is_some()),
        "the project's rows do not say they open"
    );
    let rows: Vec<String> = picker.matches().map(|item| item.label.clone()).collect();
    assert_eq!(
        rows,
        ["something else", "the second", "committed"],
        "the project's view shows only this file's commits"
    );
}

/// A commit in the project's tab is not a file, so it has nothing to open.
/// What it has is the files it changed, and they go under it in place: one
/// list, one selection, one Escape.
#[test]
fn a_commit_opens_its_files_under_it() {
    use crossterm::event::KeyCode;
    use obelus_app::app::App;
    use obelus_buffer::Buffer;

    let repository = Repository::new("history-expand", "one\n");
    std::fs::write(repository.directory().join("other.rs"), "elsewhere\n").expect("the other");
    repository.write("one\ntwo\n");
    repository.commit_all("touching two");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 70, 16);
    support::press_function(&mut app, 10);
    support::read_history(&mut app, &events);

    let rows = |app: &App| -> Vec<String> {
        app.picker()
            .expect("the history")
            .matches()
            .map(|item| item.label.clone())
            .collect()
    };
    assert_eq!(rows(&app), ["touching two", "committed"]);

    support::press(&mut app, KeyCode::Enter);
    assert_eq!(
        rows(&app),
        ["touching two", "file.rs", "other.rs", "committed"],
        "the commit did not open its files under it"
    );
    assert_eq!(
        app.picker().expect("the history").selected(),
        0,
        "the selection left the row the key was pressed on"
    );

    // And the same key closes it again.
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(
        rows(&app),
        ["touching two", "committed"],
        "it would not close"
    );
}

/// A line written down against one version of a file is found again in the
/// next, which is what a note made while reading needs: it was put beside
/// something, and the something has been moving ever since.
#[test]
fn a_line_is_followed_from_the_commit_it_was_noted_in() {
    let then = "one\ntwo\nthree\nfour\n";

    // Two lines put in above it: what was line 3 is line 5.
    let changes = Changes::between(then, "new\nalso\none\ntwo\nthree\nfour\n");
    assert_eq!(
        changes.working_line(LineNumber::new(2)),
        Some(LineNumber::new(4)),
        "the line did not move down with what was added above it"
    );
    // And nothing above the first line moves it.
    assert_eq!(
        Changes::between(then, "one\ntwo\nthree\nfour\nfive\n").working_line(LineNumber::new(2)),
        Some(LineNumber::new(2)),
        "a line moved for a change below it"
    );

    // A line taken out is gone, and saying so beats pointing at whatever
    // took its place.
    let removed = Changes::between(then, "one\nfour\n");
    assert_eq!(
        removed.working_line(LineNumber::new(2)),
        None,
        "a line the file no longer has was found anyway"
    );
    assert_eq!(
        removed.working_line(LineNumber::new(3)),
        Some(LineNumber::new(1)),
        "the line after a deletion did not come up with it"
    );

    // An unchanged file moves nothing.
    assert_eq!(
        Changes::between(then, then).working_line(LineNumber::new(2)),
        Some(LineNumber::new(2))
    );
}

/// It is the other direction of the map the blame walks, so the two agree
/// wherever both have an answer.
#[test]
fn following_a_line_both_ways_comes_back_to_it() {
    let changes = Changes::between(
        "one\ntwo\nthree\nfour\nfive\n",
        "one\nnew\ntwo\nthree\nfive\nsix\n",
    );
    for line in 0..5usize {
        let Some(now) = changes.working_line(LineNumber::new(line)) else {
            continue;
        };
        assert_eq!(
            changes.committed_line(now),
            Some(LineNumber::new(line)),
            "line {line} went out and came back somewhere else"
        );
    }
}

/// A list of changed files is read for which of them to look at first, and
/// how much each moved is most of that answer. At the right-hand end, where a
/// number is read down a column rather than hunted for at the ragged end of a
/// name.
#[test]
fn the_changed_files_say_how_much_each_changed() {
    use obelus_theme::builtin::DARK;

    let repository = Repository::new("changed-counts", "one\ntwo\nthree\n");
    std::fs::write(repository.directory().join("other.rs"), "a\nb\n").expect("the other");
    repository.commit_all("what they were");
    // Two lines replaced and two added; one line taken away; a file git has
    // never heard of, which is all arrival.
    repository.write("one\nTWO\nfour\nfive\nsix\n");
    std::fs::write(repository.directory().join("other.rs"), "a\n").expect("the other");
    std::fs::write(repository.directory().join("new.rs"), "x\ny\nz\n").expect("the new one");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    support::lay_out(&mut app, 56, 16);
    support::press_function(&mut app, 3);

    let dump = support::render(&mut app, 56, 16);
    let text = support::text_block(&dump);
    let row = |name: &str| {
        text.lines()
            .find(|row| row.contains(name))
            .unwrap_or_else(|| panic!("{name} is not in the list:\n{text}"))
            .to_string()
    };
    assert!(
        row("file.rs").contains("+4 \u{2212}2"),
        "{}",
        row("file.rs")
    );
    assert!(row("new.rs").contains("+3 \u{2212}0"), "{}", row("new.rs"));
    assert!(
        row("other.rs").contains("+0 \u{2212}1"),
        "{}",
        row("other.rs")
    );

    // Right-aligned: every row's count ends at the same column, which is what
    // makes them a column rather than three notes.
    let ends: Vec<usize> = ["file.rs", "new.rs", "other.rs"]
        .into_iter()
        .map(|name| row(name).trim_end().len())
        .collect();
    assert!(
        ends.windows(2).all(|pair| pair[0] == pair[1]),
        "the counts do not line up: {ends:?}"
    );

    // And in the two colours the margin marks the same two facts in.
    assert_eq!(
        support::colour_under(&dump, '+'),
        support::spelled(DARK.change_added)
    );
    assert_eq!(
        support::colour_under(&dump, '\u{2212}'),
        support::spelled(DARK.change_removed)
    );
}

/// A commit chosen from the project's history has no file under it, so the
/// count beside it is the whole commit's -- which is what that row is. In a
/// file's history the same number is about the file. Both answer the question
/// their row asks.
#[test]
fn a_commit_alone_says_what_it_did_to_everything() {
    use obelus_theme::builtin::DARK;

    let repository = Repository::new("history-whole", "one\ntwo\nthree\n");
    // A second file, so the count cannot be one file's by accident.
    std::fs::write(repository.directory().join("other.rs"), "a\nb\n").expect("the other");
    // `file.rs`: two lines replaced and one added. `other.rs`: two arriving.
    repository.write("one\nTWO\nfour\nfive\n");
    repository.commit_all("Change two files at once");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 60, 24);
    support::press_function(&mut app, 10);
    support::read_history(&mut app, &events);

    let dump = support::render(&mut app, 60, 24);
    let shown = support::previewed(&dump);
    assert!(
        shown.contains("Change two files at once"),
        "the project's history previewed no message:\n{shown}"
    );
    assert!(
        shown.contains("+5") && shown.contains("\u{2212}2"),
        "not what the commit did to the whole tree:\n{shown}"
    );

    // Same two colours as everywhere else the same two facts are marked.
    assert_eq!(
        support::colour_under(&dump, '+'),
        support::spelled(DARK.change_added)
    );
    assert_eq!(
        support::colour_under(&dump, '\u{2212}'),
        support::spelled(DARK.change_removed)
    );
}

/// Beside who wrote it: what it did to *this* file. The message hangs over
/// one file, and the question there is what this commit did to that -- not
/// what it did to the project, which is a number about a diff the reader is
/// not looking at.
#[test]
fn the_message_says_how_much_the_commit_changed_this_file() {
    use obelus_theme::builtin::DARK;

    let repository = Repository::new("history-counts", "one\ntwo\nthree\n");
    // Two lines gone, three put in their place.
    repository.write("one\nTWO\nfour\nfive\n");
    repository.commit_all("Change it about");
    let (mut app, _events) = previewing_it_at_its_commit(&repository);

    let dump = support::render(&mut app, 64, 24);
    let text = support::previewed(&dump);
    let head = text
        .lines()
        .find(|row| row.contains("just now"))
        .unwrap_or_else(|| panic!("no header row:\n{text}"));
    assert!(
        head.contains('+') && head.contains('\u{2212}'),
        "the header says nothing about what changed:\n{head}"
    );
    assert!(
        head.contains("+3") && head.contains("\u{2212}2"),
        "not what this commit did to this file:\n{head}"
    );

    // In the colours the margin marks the same two facts in: they are the
    // same two facts, and a reader who learnt them beside the code has
    // learnt them here.
    assert_eq!(
        support::colour_under(&dump, '+'),
        support::spelled(DARK.change_added),
        "what arrived is not in the colour an added line wears:\n{dump}"
    );
    assert_eq!(
        support::colour_under(&dump, '\u{2212}'),
        support::spelled(DARK.change_removed),
        "what went is not in the colour a removed line wears:\n{dump}"
    );
}

/// The message is not the file, and two things say so: the line numbers it
/// does not have, and the rule under it. A panel of another colour on top of
/// the code, as tall as somebody's prose, was a third -- and the loudest
/// thing on a screen whose subject is the code underneath.
#[test]
fn a_message_is_drawn_on_the_page_rather_than_on_a_panel() {
    use obelus_theme::builtin::DARK;

    let repository = Repository::new("history-panel", "one\n");
    repository.write("one\ntwo\n");
    // The letters asked about are in the *body*: the list above the preview
    // shows the subject, on a row that wears the selected background, and a
    // glyph looked up across the whole screen would find that one first.
    repository.commit_all("A subject\n\nzyxw, and nothing else spells it.");
    let (mut app, _events) = previewing_it_at_its_commit(&repository);

    let dump = support::render(&mut app, 64, 24);
    let page = support::spelled(DARK.background);
    for glyph in ['z', 'y', 'x'] {
        let row = support::legend_for(&dump, glyph);
        assert!(
            row.ends_with(&format!("bg={page}")),
            "the message sits on a panel: {glyph:?} is drawn {row}"
        );
    }
}

/// The same history, stopped one key short: the row is under the cursor and
/// the preview under the list is showing what pressing it would give.
///
/// Which is where a commit's message is now read. Opening the row gives the
/// file and nothing else, so anything about the message is asked of this.
fn previewing_it_at_its_commit(repository: &Repository) -> (App, Receiver<Event>) {
    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    // Taller than the editor's helper needs: a preview is drawn only where
    // there is room under the list for one, and eighteen rows is not.
    support::lay_out(&mut app, 64, 24);
    support::press_function(&mut app, 9);
    support::read_history(&mut app, &events);
    (app, events)
}

/// A subject too long for the row says so. Running out of row looks exactly
/// like a subject that ends there, and a reader cannot tell a commit called
/// "Stop and ask" from one called "Stop and ask, instead of counting
/// presses" if the row is narrow enough.
#[test]
fn a_subject_too_long_for_the_row_ends_in_an_ellipsis() {
    use obelus_app::app::App;
    use obelus_buffer::Buffer;

    let subject = "Take the whole of a very long summary of what this commit did";
    let repository = Repository::new("history-elide", "one\n");
    repository.write("one\ntwo\n");
    repository.commit_all(subject);

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 10);
    support::read_history(&mut app, &events);

    let dump = support::render(&mut app, 60, 16);
    let text = support::text_block(&dump);
    let row = text
        .lines()
        .find(|line| line.contains("Take the whole"))
        .unwrap_or_else(|| panic!("the commit is not on screen:\n{text}"));

    assert!(
        !row.contains(subject),
        "the row is wide enough to hold it all, so this proves nothing:\n{row}"
    );
    assert!(
        row.contains('\u{2026}'),
        "the subject was cut without saying so:\n{row}"
    );

    // And what is kept is the *beginning* of the subject, up to the mark: a
    // sentence cut from the front would have lost the half that says which
    // commit this is.
    let from = row.find("Take").expect("the subject");
    let cut = row.find('\u{2026}').expect("the mark");
    let kept = &row[from..cut];
    assert!(
        subject.starts_with(kept),
        "what was kept is not the beginning of the subject: {kept:?}"
    );
    assert!(kept.len() > 10, "almost nothing survived the cut: {kept:?}");
}

/// The arrow that says a commit has files behind it is the arrow the gutter
/// and the transcript draw, and it recedes there. A mark is coloured by what
/// it says, and this one says "there is more here", not "look at this".
///
/// It went red once, when the mark a buffer wears for work that is not on
/// disk was given the status row's colour and every mark in every list came
/// with it. The two are not the same fact and must not be the same colour.
#[test]
fn the_arrow_on_a_commit_recedes() {
    use obelus_app::app::App;
    use obelus_buffer::Buffer;
    use obelus_theme::builtin::DARK;

    let repository = Repository::new("history-arrow", "one\n");
    repository.write("one\ntwo\n");
    repository.commit_all("touching two");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 70, 16);
    support::press_function(&mut app, 10);
    support::read_history(&mut app, &events);

    let dump = support::render(&mut app, 70, 16);
    let arrow = support::colour_under(&dump, '\u{25b8}');
    assert_eq!(
        arrow,
        support::spelled(DARK.gutter),
        "the fold arrow is not in the colour the gutter draws it in:\n{dump}"
    );
    assert_ne!(
        arrow,
        support::spelled(DARK.status_stale),
        "the fold arrow is wearing the mark for work that is not on disk"
    );
}

/// Choosing one of a commit's files opens the file as that commit had it --
/// not the file on disk, which is a different document that happens to
/// share a name.
#[test]
fn a_file_of_a_commit_opens_as_that_commit_had_it() {
    use crossterm::event::KeyCode;
    use obelus_app::app::App;
    use obelus_buffer::Buffer;

    let repository = Repository::new("history-open", "first\n");
    repository.write("second\n");
    repository.commit("the second");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 70, 16);
    support::press_function(&mut app, 10);
    support::read_history(&mut app, &events);
    // The older commit, opened, and its one file chosen.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);

    let buffer = app.current_buffer().expect("a file");
    assert_eq!(
        buffer.text().rope().to_string(),
        "first\n",
        "not the file as that commit had it"
    );
    assert!(
        buffer.content().at().is_some(),
        "the buffer does not know it came from a commit"
    );
    assert_eq!(
        buffer.path(),
        repository.path(),
        "the buffer lost the file's own name"
    );
}

/// A subject too long for the row loses its end, not its head.
///
/// The rest of a picker's rows are names -- a path, a symbol -- where the
/// end is what is being looked for and the head is already known. A
/// sentence is the other way round: "Fold away the block the cursor is in"
/// cut to "…the block the cursor is in" has lost the half that says which
/// commit this is.
///
/// Which end the mark is on, then. That there *is* one is
/// `a_subject_too_long_for_the_row_ends_in_an_ellipsis`.
#[test]
fn a_subject_is_cut_at_its_end() {
    use obelus_app::app::App;
    use obelus_buffer::Buffer;

    let repository = Repository::new("history-cut", "one\n");
    repository.write("one\ntwo\n");
    repository.commit("Give the third bank of function keys to git, and move the reading");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 50, 10);
    support::press_function(&mut app, 9);
    support::read_history(&mut app, &events);

    let dump = support::render(&mut app, 50, 10);
    let row = support::text_block(&dump)
        .lines()
        .find(|row| row.contains("Give the third"))
        .expect("the commit's row");
    // Nothing before the subject's first words, which is where the mark
    // would be if this row were cut the way a path is. Not "no mark
    // anywhere" -- there is one now, on the other end, which is the point.
    let head = row.find("Give the third").expect("the subject's opening");
    assert!(
        !row[..head].contains('\u{2026}'),
        "the subject was cut at its head:\n{row}"
    );
    assert!(
        !row.contains("and move the reading"),
        "the row is wide enough for the whole subject, so this proves nothing:\n{row}"
    );
}

/// A commit's version of a file is the file, and the message is not on it.
///
/// It used to hang above the first line, folded, with the reader landing in
/// it: they opened this to find out *why*, so the why came first. What that
/// cost was a page of somebody's prose between a reader and the file they
/// asked for, and the first line's one block slot spoken for. The message
/// is read whole in the preview, a keypress back; what opening the row
/// gives is the file, and the status row still says which commit it is.
///
/// Broken deliberately by hanging the message above the first line again in
/// `read_at_commit`: the file was a page down and the caret was in prose.
#[test]
fn a_commits_version_is_the_file_and_not_the_message() {
    use crossterm::event::KeyCode;
    use obelus_app::app::App;
    use obelus_buffer::Buffer;

    let repository = Repository::new("history-said", "first\n");
    repository.write("second\n");
    repository.commit("A subject worth reading\n\nAnd a body under it.\n");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 10);
    support::read_history(&mut app, &events);
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);

    let dump = support::render(&mut app, 60, 16);
    let text = support::text_block(&dump);
    assert!(
        text.contains("second"),
        "the file itself is not there:\n{text}"
    );
    assert!(
        !text.contains("And a body under it."),
        "the commit's prose is in front of the file:\n{text}"
    );
    // On the file's own first line, not in prose above it: a negative line
    // is what the status row says for a row the file does not have.
    let status = support::text_block(&dump)
        .lines()
        .last()
        .expect("a status row")
        .to_string();
    assert!(
        status.contains("1:1"),
        "the reader did not land on the file's first line:\n{dump}"
    );
    assert!(
        !status.contains("-1:1"),
        "the reader landed in a message that should not be there:\n{dump}"
    );
    // And which commit this is, where the mode and the staleness go.
    let at = app
        .current_buffer()
        .expect("a file")
        .content()
        .short()
        .expect("a commit");
    assert!(
        status.contains(&at),
        "the status row does not say which commit:\n{dump}"
    );
}

/// Everything that assumes a buffer's path is where its bytes came from has
/// to ask first. A commit's version of a file shares a path with the file
/// and is a different document.
#[test]
fn a_commits_version_is_not_the_file_at_that_path() {
    use crossterm::event::KeyCode;
    use obelus_app::app::App;
    use obelus_buffer::Buffer;

    let repository = Repository::new("history-apart", "first\n");
    repository.write("second\n");
    repository.commit("the second");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 10);
    support::read_history(&mut app, &events);
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.current_buffer()
            .expect("a file")
            .text()
            .rope()
            .to_string(),
        "first\n",
        "not the older commit's version"
    );

    // The file changes on disk and is re-read: the commit's version is not
    // touched, because those bytes are what that commit said.
    repository.write("third\n");
    app.handle(obelus_app::event::Event::Watched(obelus_watch::Changed {
        path: repository.path(),
    }));
    assert_eq!(
        app.current_buffer()
            .expect("a file")
            .text()
            .rope()
            .to_string(),
        "first\n",
        "a re-read of the file overwrote a commit's version of it"
    );
}

/// Opening a file gives the file, not a buffer that happens to wear its
/// name. A commit's version shares the path and is a different document.
#[test]
fn opening_a_file_does_not_find_a_commits_version_of_it() {
    use crossterm::event::KeyCode;
    use obelus_app::app::App;

    let repository = Repository::new("history-reopen", "first\n");
    repository.write("second\n");
    repository.commit("the second");

    // Nothing open, so the commit's version is the only buffer there is
    // wearing that name -- otherwise the list finds the right answer for
    // the wrong reason.
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 10);
    support::read_history(&mut app, &events);
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    let buffer = app.current_buffer().expect("a file");
    assert_eq!(buffer.text().rope().to_string(), "first\n");
    assert!(buffer.content().at().is_some(), "not a commit's version");

    app.open_for_test(&repository.path());
    let buffer = app.current_buffer().expect("a file");
    assert!(
        buffer.content().is_file(),
        "opening the file handed back a commit's version of it"
    );
    assert_eq!(buffer.text().rope().to_string(), "second\n");
}

/// The margin beside a commit's version says what *that commit* changed,
/// not how it differs from the file today. The second is a question about a
/// file the reader is not looking at.
#[test]
fn a_commits_version_is_marked_against_the_commit_before_it() {
    use crossterm::event::KeyCode;
    use obelus_app::app::App;
    use obelus_text::{coordinates::LineNumber, marker::Marker};

    let repository = Repository::new("history-margin", "one\ntwo\nthree\n");
    repository.write("one\nCHANGED\nthree\n");
    repository.commit("the middle line");
    // And then a line the reader is not looking at changes, so "against
    // today" and "against the commit before" give different answers.
    repository.write("one\nCHANGED\nLATER\n");
    repository.commit("the last line");

    // The file itself first, and drawn, so that its own diff is worked out
    // and remembered: a commit's version shares this path and starts at the
    // same version number, so a remembered answer keyed on those two would
    // be handed to it.
    let mut app = App::new(vec![
        obelus_buffer::Buffer::open(&repository.path()).expect("opening it"),
    ]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 60, 16);
    let _ = support::render(&mut app, 60, 16);

    support::press_function(&mut app, 10);

    support::read_history(&mut app, &events);
    // The middle commit, and the file as it had it.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    let _ = support::render(&mut app, 60, 16);

    // And no blame beside it. A blame is a walk from `HEAD`, so its lines
    // are the lines of the file as it is now; laid beside a file as it was
    // they would name whoever last touched whatever sits at those numbers
    // today -- a confident answer about the wrong lines. The answer is put
    // in by hand, because it is worked out on a thread and this is not a
    // test about that.
    app.handle(obelus_app::event::Event::Git(obelus_git::Event::Blamed {
        at: None,
        path: repository.path(),
        lines: vec![
            Some(obelus_git::Blamed {
                id: gix::ObjectId::null(gix::hash::Kind::Sha1),
                line: 0,
                who: "somebody".to_string(),
                when: 0,
            });
            3
        ],
    }));
    assert!(
        app.blame().is_none(),
        "a commit's version was blamed as if it were the file"
    );

    let changes = app.changes().expect("what that commit changed");
    assert_eq!(
        changes.marker_at(LineNumber::new(1)),
        Some(Marker::Modified),
        "the line that commit changed is not marked"
    );
    assert_eq!(
        changes.marker_at(LineNumber::new(2)),
        None,
        "a line changed by a later commit is marked against this one"
    );
}

/// A list previews what choosing a row would give, and a history's rows are
/// not files on disk: a commit is a message, and one of its files is that
/// file as the commit had it.
#[test]
fn the_history_previews_what_a_row_would_give() {
    use crossterm::event::KeyCode;
    use obelus_app::app::App;
    use obelus_buffer::Buffer;
    use obelus_git::history;

    let repository = Repository::new("history-preview", "first\n");
    repository.write("second\n");
    // A body longer than the preview has room for, so that starting at the
    // top of the message can be told from starting anywhere else in it.
    let body: String = (0..20).map(|line| format!("Body line {line}.\n")).collect();
    repository.commit(&format!("A subject worth reading\n\n{body}"));

    // The working tree is a third thing, so that previewing the file on
    // disk can be told from previewing the commit's version of it.
    repository.write("uncommitted\n");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 60, 24);
    support::press_function(&mut app, 10);
    support::read_history(&mut app, &events);

    use support::previewed;

    // A commit: what it said, from the first line of it.
    let dump = support::render(&mut app, 60, 24);
    let text = previewed(&dump);
    assert!(
        text.contains("A subject worth reading") && text.contains("Body line 0."),
        "a commit's row does not preview what it said:\n{text}"
    );
    assert!(
        !text.contains("second"),
        "a commit previewed a file it is not:\n{text}"
    );
    // Drawn as a block, so it has no line numbers: a message has no lines
    // of its own to go to. Asked of the gutter rather than of the bar that
    // used to run down the side -- the rule under the message says where it
    // ends now, and a bar beside prose nobody can open said it twice.
    let subject = text
        .lines()
        .find(|row| row.contains("A subject worth reading"))
        .expect("the subject's row");
    // Past the dump's own row number, which is not on the screen.
    let drawn = subject.split_once('|').map_or(subject, |(_, rest)| rest);
    let gutter = &drawn[..drawn.find("A subject").expect("the subject")];
    assert!(
        !gutter.chars().any(|c| c.is_ascii_digit()),
        "the message is numbered as if it were a file:\n{text}"
    );

    // One of its files: the file as that commit had it, message and all --
    // which is what choosing the row opens.
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Down);
    let dump = support::render(&mut app, 60, 24);
    let text = previewed(&dump);
    assert!(
        text.contains("A subject worth reading"),
        "the file's preview lost the message:\n{text}"
    );
    assert!(
        !text.contains("uncommitted"),
        "the preview shows the file on disk rather than the commit's:\n{text}"
    );

    // And it starts at the top of the message, not somewhere in the middle
    // of it: the first row of the preview is the line that names the
    // commit, and the message is taller than the room there is.
    let first = text.lines().next().expect("a first row").to_string();
    let at = history::of(&repository.directory(), None, 1)[0].short();
    assert!(
        first.contains(&at),
        "the preview did not start at the top of the message:\n{first}"
    );
}

/// Two buffers can wear one path -- the file, and the file as some commit
/// had it -- and the list of open files has to say which is which. They
/// differ in what they say, in whether they follow the disk, and in what
/// the margin beside them means, and a reader picking between two identical
/// rows is picking blind.
#[test]
fn the_open_files_say_which_commit_they_came_from() {
    use crossterm::event::KeyCode;
    use obelus_app::app::App;
    use obelus_buffer::Buffer;

    let repository = Repository::new("open-files-commit", "first\n");
    repository.write("second\n");
    repository.commit("the second");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 60, 14);
    support::press_function(&mut app, 10);
    support::read_history(&mut app, &events);
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    let at = app
        .current_buffer()
        .expect("a file")
        .content()
        .short()
        .expect("a commit");

    support::press_function(&mut app, 2);
    let rows: Vec<(String, Option<String>)> = app
        .picker()
        .expect("the open files")
        .matches()
        .map(|item| (item.label.clone(), item.trailing.clone()))
        .collect();
    assert_eq!(rows.len(), 2, "not both of them: {rows:?}");
    assert_eq!(
        rows[0].1, None,
        "the file on disk is marked as if it came from a commit"
    );
    assert_eq!(
        rows[1].1.as_deref(),
        Some(at.as_str()),
        "the commit's version does not say which commit"
    );
    assert_eq!(
        rows[0].0, rows[1].0,
        "they were telling themselves apart some other way, and this test proves nothing"
    );
}

/// A repository with somewhere to push to, and a branch that tracks it.
struct Pushed {
    directory: std::path::PathBuf,
    work: std::path::PathBuf,
}

impl Pushed {
    /// Three commits pushed, then `ahead` more that the remote has never
    /// seen.
    fn new(name: &str, ahead: usize) -> Self {
        let directory =
            std::env::temp_dir().join(format!("obelus-push-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        let (bare, work) = (directory.join("remote.git"), directory.join("work"));
        std::fs::create_dir_all(&work).expect("a directory");
        std::process::Command::new("git")
            .args(["init", "--bare", "--quiet", "--initial-branch=master"])
            .arg(&bare)
            .output()
            .expect("a remote");

        let git = |at: &std::path::Path, arguments: &[&str]| {
            let outcome = std::process::Command::new("git")
                .arg("-C")
                .arg(at)
                .args(arguments)
                .env("GIT_AUTHOR_NAME", "obelus")
                .env("GIT_AUTHOR_EMAIL", "obelus@example.invalid")
                .env("GIT_COMMITTER_NAME", "obelus")
                .env("GIT_COMMITTER_EMAIL", "obelus@example.invalid")
                .output()
                .expect("running git");
            assert!(outcome.status.success(), "git {arguments:?} failed");
        };
        git(&work, &["init", "--quiet", "--initial-branch=master"]);
        for commit in 0..3 {
            std::fs::write(work.join("file.rs"), format!("line {commit}\n")).expect("the file");
            git(&work, &["add", "-A"]);
            git(
                &work,
                &["commit", "--quiet", "-m", &format!("pushed {commit}")],
            );
        }
        git(
            &work,
            &["remote", "add", "origin", bare.to_str().expect("a path")],
        );
        git(&work, &["push", "--quiet", "-u", "origin", "HEAD"]);
        for commit in 0..ahead {
            std::fs::write(work.join("file.rs"), format!("later {commit}\n")).expect("the file");
            git(&work, &["add", "-A"]);
            git(
                &work,
                &["commit", "--quiet", "-m", &format!("ahead {commit}")],
            );
        }
        Self { directory, work }
    }

    fn path(&self) -> std::path::PathBuf {
        self.work.join("file.rs")
    }
}

impl Drop for Pushed {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

/// What the remote has not seen is what is still the reader's to change, and
/// the list says so in the colour a file git has not seen wears.
#[test]
fn the_commits_the_remote_has_not_seen_are_marked() {
    use obelus_app::app::App;
    use obelus_buffer::Buffer;
    use obelus_git::FileStatus;

    let repository = Pushed::new("marked", 2);
    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.work.clone());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 64, 14);
    support::press_function(&mut app, 10);
    support::read_history(&mut app, &events);

    let rows: Vec<(String, Option<FileStatus>)> = app
        .picker()
        .expect("the history")
        .matches()
        .map(|item| (item.label.clone(), item.status))
        .collect();
    assert_eq!(
        rows,
        [
            ("ahead 1".to_string(), Some(FileStatus::New)),
            ("ahead 0".to_string(), Some(FileStatus::New)),
            ("pushed 2".to_string(), None),
            ("pushed 1".to_string(), None),
            ("pushed 0".to_string(), None),
        ],
        "not the two the remote has never seen"
    );
}

/// Nothing is marked where the question does not arise. Every commit is
/// then equally unpushed, and marking all of them says no more than marking
/// none -- and obelus's own repository, which has no remote at all, would
/// otherwise be a wall of one colour.
#[test]
fn a_repository_with_nowhere_to_push_marks_nothing() {
    use obelus_app::app::App;
    use obelus_buffer::Buffer;

    let repository = Repository::new("nowhere-to-push", "one\n");
    repository.write("one\ntwo\n");
    repository.commit("the second");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 64, 14);
    support::press_function(&mut app, 10);
    support::read_history(&mut app, &events);
    assert!(
        app.picker()
            .expect("the history")
            .matches()
            .all(|item| item.status.is_none()),
        "a repository with no remote marked its commits"
    );
}

/// A commit in a file's own history has nothing to open under it: it is
/// already about one file. Choosing it opens that file as that commit had
/// it -- the list of files it changed would be a list with the tab's own
/// name in it.
#[test]
fn a_commit_in_a_files_history_opens_that_file() {
    use crossterm::event::KeyCode;
    use obelus_app::app::App;
    use obelus_buffer::Buffer;

    let repository = Repository::new("history-file-tab", "first\n");
    repository.write("second\n");
    repository.commit("the second");
    repository.write("third\n");
    repository.commit("the third");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 9);
    support::read_history(&mut app, &events);
    assert_eq!(
        app.picker().expect("the history").tab(),
        0,
        "not the file's own tab"
    );
    // No marks in this tab: there is nothing under these rows to open.
    assert!(
        app.picker()
            .expect("the history")
            .matches()
            .all(|item| item.marker.is_none()),
        "a row offered to open something under it"
    );

    // The commit before the last one.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    assert!(app.picker().is_none(), "the list stayed open");

    let buffer = app.current_buffer().expect("a file");
    assert_eq!(
        buffer.text().rope().to_string(),
        "second\n",
        "not the file as that commit had it"
    );
    assert!(
        buffer.content().at().is_some(),
        "the file on disk was opened instead"
    );
}

/// A file whose last change is a long way back still has a history.
///
/// Whether it has one is a question with a walk in it -- every commit has
/// to be asked whether it touched this path -- and the walk is bounded, so
/// asking it to decide whether the tab exists means the tab disappears for
/// exactly the files nobody has edited lately, which are the ones whose
/// history a reader is curious about.
#[test]
fn a_file_nobody_has_touched_lately_still_opens_its_history() {
    use obelus_app::app::App;
    use obelus_buffer::Buffer;

    let repository = Repository::new("history-far-back", "one\n");
    repository.write("one\ntwo\n");
    repository.commit("the one that touched it");
    // And a long run of commits that leave it alone. Longer than the walk
    // will look when it is asked for a single answer.
    for commit in 0..60 {
        std::fs::write(
            repository.directory().join("other.rs"),
            format!("elsewhere {commit}\n"),
        )
        .expect("the other file");
        repository.commit_all(&format!("elsewhere {commit}"));
    }

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 9);
    support::read_history(&mut app, &events);

    let picker = app.picker().expect("the history");
    assert_eq!(picker.tab(), 0, "the file's own tab is not there");
    let rows: Vec<String> = picker.matches().map(|item| item.label.clone()).collect();
    assert_eq!(
        rows,
        ["the one that touched it", "committed"],
        "the commits that touched it were not found"
    );
}

#[test]
fn a_query_keeps_a_log_in_its_own_order() {
    use obelus_app::app::App;
    use obelus_buffer::Buffer;

    let repository = Repository::new("history-order", "one\n");
    // An older subject the matcher scores well -- the query contiguous, at a
    // word boundary -- under a newer one it merely tolerates, where the same
    // letters are scattered through other words. Ranked, the old commit is
    // listed first.
    repository.write("one\ntwo\n");
    repository.commit("Fold a line");
    repository.write("one\ntwo\nthree\n");
    repository.commit("Something else entirely");
    repository.write("one\ntwo\nthree\nfour\n");
    repository.commit("Fix the outline's depth");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 9);
    support::read_history(&mut app, &events);
    support::type_text(&mut app, "fold");

    let picker = app.picker().expect("the history");
    let rows: Vec<String> = picker.matches().map(|item| item.label.clone()).collect();
    assert_eq!(
        rows,
        ["Fix the outline's depth", "Fold a line"],
        "the query ranked the log instead of filtering it"
    );
}

#[test]
fn a_files_history_previews_the_file_at_that_commit() {
    use crossterm::event::KeyCode;
    use obelus_app::app::App;
    use obelus_buffer::Buffer;
    use support::previewed;

    let repository = Repository::new("history-preview-of-a-file", "first\n");
    repository.write("second\n");
    repository.commit("A subject worth reading");
    // The working tree is a third thing, so previewing the file on disk can
    // be told from previewing the commit's version of it.
    repository.write("uncommitted\n");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 60, 24);
    support::press_function(&mut app, 9);
    support::read_history(&mut app, &events);

    // The newest commit: this file as that commit had it, with the message
    // above it -- which is what choosing the row opens.
    let text = previewed(&support::render(&mut app, 60, 24));
    assert!(
        text.contains("A subject worth reading"),
        "a file's history previewed no message:\n{text}"
    );
    assert!(
        text.contains("second"),
        "a file's history previewed the message alone, not the file:\n{text}"
    );
    assert!(
        !text.contains("uncommitted"),
        "the preview shows the file on disk rather than the commit's:\n{text}"
    );

    // The one before it, where the file said something else. The preview
    // follows the row, or it is a preview of the list rather than of the row.
    support::press(&mut app, KeyCode::Down);
    let text = previewed(&support::render(&mut app, 60, 24));
    assert!(
        text.contains("first") && !text.contains("second"),
        "the preview did not follow the row to its own version:\n{text}"
    );
}

#[test]
fn a_version_already_open_previews_where_it_is_open() {
    use crossterm::event::KeyCode;
    use obelus_app::app::App;
    use obelus_buffer::Buffer;
    use support::previewed;

    let repository = Repository::new("history-preview-open", "top\n");
    // Long enough that a place in the middle is nowhere near the top, so
    // previewing where the reader left it can be told from previewing the
    // start of it.
    let lines: String = (0..80).map(|line| format!("line {line}\n")).collect();
    repository.write(&lines);
    repository.commit("A subject worth reading");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 60, 24);

    // Open the newest commit's version and read down into it.
    support::press_function(&mut app, 9);
    support::read_history(&mut app, &events);
    support::press(&mut app, KeyCode::Enter);
    for _ in 0..60 {
        support::press(&mut app, KeyCode::Down);
    }
    // Which lines a screen is showing, by their number in the file.
    fn lines_on(text: &str) -> Vec<usize> {
        text.lines()
            .filter_map(|row| {
                let (_, said) = row.split_once("line ")?;
                said.split_whitespace().next()?.parse().ok()
            })
            .collect()
    }

    let read_at = support::render(&mut app, 60, 24);
    let reading = lines_on(support::text_block(&read_at));
    assert!(
        reading.iter().min().copied().unwrap_or(0) > 10,
        "the version did not open and scroll:\n{read_at}"
    );

    // Ask for the history again. The row for that commit is the version
    // that is open, so it previews where it is being read -- which is well
    // down the file. Asserted as "not the top" rather than as a line
    // number: how far sixty presses reach is not what this is about, and a
    // number here would be a second claim about where the caret starts.
    support::press_function(&mut app, 9);
    support::read_history(&mut app, &events);
    let shown = previewed(&support::render(&mut app, 60, 24));
    let previewing = lines_on(&shown);
    assert!(
        previewing.iter().min().copied().unwrap_or(0) > 10,
        "the preview went back to the top of a version already open:\n{shown}"
    );
}

#[test]
fn a_history_is_not_cut_off_at_a_screenful() {
    use obelus_app::app::App;
    use obelus_buffer::Buffer;

    let repository = Repository::new("history-unbounded", "0\n");
    // More commits than any list obelus used to ask for. A history is as
    // long as the project, and a reader searching one is searching all of
    // it: rows that were never fetched are rows a query cannot match.
    for change in 1..=230 {
        repository.write(&format!("{change}\n"));
        repository.commit(&format!("change {change}"));
    }

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 9);
    support::read_history(&mut app, &events);

    let picker = app.picker().expect("the history");
    assert_eq!(
        picker.match_count(),
        231,
        "the history stopped short of the commits that touched this file"
    );

    // And the oldest of them can be searched for, which is what a bounded
    // list quietly could not do: rows that were never fetched are rows a
    // query cannot match, and the reader is told "no match" either way.
    support::type_text(&mut app, "committed");
    let rows: Vec<String> = app
        .picker()
        .expect("the history")
        .matches()
        .map(|item| item.label.clone())
        .collect();
    assert_eq!(
        rows,
        ["committed"],
        "a commit past the old limit cannot be searched for"
    );
}

#[test]
fn a_list_still_filling_says_so() {
    use obelus_app::app::App;
    use obelus_buffer::Buffer;

    let repository = Repository::new("history-filling", "one\n");
    repository.write("two\n");
    repository.commit("the second");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 60, 16);

    // The key comes back before the walk does -- that is the whole point of
    // the walk being elsewhere -- so the list it opens is one that says it
    // is not finished.
    support::press_function(&mut app, 9);
    assert!(
        app.picker().expect("the history").is_filling().is_some(),
        "a list that is still being read does not say so"
    );
    assert_eq!(
        app.picker().expect("the history").nothing_to_show(),
        Some("reading the history\u{2026}"),
        "an empty list that is still filling reads as an empty history"
    );

    support::read_history(&mut app, &events);
    assert!(
        app.picker().expect("the history").is_filling().is_none(),
        "a finished list still says it is filling"
    );
    assert_eq!(
        app.picker().expect("the history").nothing_to_show(),
        None,
        "a filled list still shows a reason for being empty"
    );
}

#[test]
fn a_walk_the_reader_moved_off_does_not_fill_the_list_it_left() {
    use crossterm::event::KeyCode;
    use obelus_app::app::App;
    use obelus_buffer::Buffer;

    let repository = Repository::new("history-stale", "one\n");
    // A project with commits this file knows nothing about, so the file's
    // answer landing in the project's list is visible as rows that are
    // there twice.
    repository.write("two\n");
    repository.commit("the file's own");
    std::fs::write(repository.directory().join("other.rs"), "elsewhere\n").expect("the other file");
    repository.commit_all("elsewhere");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 60, 16);

    // Both walks are started before either is heard from, which is what a
    // reader does when they press a key, change their mind and press
    // another.
    support::press_function(&mut app, 9);
    support::press(&mut app, KeyCode::Esc);
    support::press_function(&mut app, 10);

    // Everything both walks sent, so a batch that should be dropped has
    // every chance to land.
    while let Ok(event) = events.recv_timeout(std::time::Duration::from_millis(300)) {
        app.handle(event);
    }

    let rows: Vec<String> = app
        .picker()
        .expect("the history")
        .matches()
        .map(|item| item.label.clone())
        .collect();
    assert_eq!(
        rows,
        ["elsewhere", "the file's own", "committed"],
        "the list the reader left filled the one they moved to"
    );
}

#[test]
fn a_batch_landing_does_not_move_the_reader_off_their_row() {
    use crossterm::event::KeyCode;
    use obelus_app::{app::App, event::Event};
    use obelus_buffer::Buffer;
    use obelus_git::history::Commit;

    let repository = Repository::new("history-while-reading", "one\n");
    repository.write("two\n");
    repository.commit("the second");
    repository.write("three\n");
    repository.commit("the third");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 9);
    support::read_history(&mut app, &events);

    // The reader has chosen a row and is looking at it.
    support::press(&mut app, KeyCode::Down);
    let chosen = app
        .picker()
        .expect("the history")
        .selected_item()
        .expect("a row")
        .label
        .clone();
    assert_eq!(
        chosen, "the second",
        "the row under the reader is not theirs"
    );

    // More of the history arrives, as it does for the two seconds a large
    // project takes to walk.
    app.handle(Event::Git(obelus_git::Event::Logged {
        generation: app.history_walk_for_test(),
        commits: vec![Commit {
            id: gix::ObjectId::null(gix::hash::Kind::Sha1),
            subject: "older still".to_string(),
            body: String::new(),
            who: "somebody".to_string(),
            when: 0,
            at: None,
            was: None,
        }],
        walked: 900,
        done: false,
    }));

    let picker = app.picker().expect("the history");
    assert_eq!(picker.match_count(), 4, "the batch did not reach the list");
    assert_eq!(
        picker.selected_item().expect("a row").label,
        chosen,
        "a batch landing moved the reader off the row they chose"
    );
}

#[test]
fn a_history_on_screen_notices_the_repository_moving() {
    use crossterm::event::KeyCode;
    use obelus_app::{app::App, event::Event};
    use obelus_buffer::Buffer;

    let repository = Repository::new("history-moved", "one\n");
    repository.write("two\n");
    repository.commit("the second");
    repository.write("three\n");
    repository.commit("the third");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 9);
    support::read_history(&mut app, &events);

    // The reader is on a commit, not on a row number.
    support::press(&mut app, KeyCode::Down);
    assert_eq!(
        app.picker()
            .expect("the history")
            .selected_item()
            .expect("a row")
            .label,
        "the second"
    );

    // The index moves without a commit behind it, which is what `git add`
    // does every time it is used.
    let git = repository.directory().join(".git");
    app.handle(Event::Watched(obelus_watch::Changed {
        path: git.join("index"),
    }));
    assert!(
        app.picker().expect("the history").is_filling().is_none(),
        "the list was thrown away and read again for a staged file"
    );

    // And then a commit in another window.
    repository.write("four\n");
    repository.commit("the fourth");
    app.handle(Event::Watched(obelus_watch::Changed {
        path: git.join("HEAD"),
    }));
    support::read_history(&mut app, &events);

    let picker = app.picker().expect("the history");
    let rows: Vec<String> = picker.matches().map(|item| item.label.clone()).collect();
    assert_eq!(
        rows,
        ["the fourth", "the third", "the second", "committed"],
        "the list did not notice the repository moving under it"
    );
    assert_eq!(
        picker.selected_item().expect("a row").label,
        "the second",
        "the reader was left on a row number rather than on their commit"
    );
}

#[test]
fn a_hunk_in_a_commits_version_is_what_that_commit_changed() {
    use crossterm::event::KeyCode;
    use obelus_app::app::App;
    use obelus_buffer::Buffer;
    use obelus_command::Command;

    let repository = Repository::new("hunk-at-a-commit", "one\nold two\nthree\n");
    repository.write("one\nnew two\nthree\n");
    repository.commit("the second");
    // And the working tree is a third thing, so a hunk about the commit can
    // be told from a hunk about the file on disk.
    repository.write("one\nworking two\nthree\n");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 52, 16);
    support::press_function(&mut app, 9);
    support::read_history(&mut app, &events);
    support::press(&mut app, KeyCode::Enter);

    // Onto the line the commit changed, which is the second. One press:
    // the version opens on its own first line, with nothing above it -- the
    // message is read in the preview and hangs over no file.
    support::press(&mut app, KeyCode::Down);
    let dump = support::render(&mut app, 52, 16);
    assert!(
        support::text_block(&dump)
            .lines()
            .next_back()
            .is_some_and(|status| status.contains("2:1")),
        "the caret is not on the line the commit changed:\n{dump}"
    );

    obelus_app::app::dispatch::dispatch(&mut app, Command::GitHunk);
    let opened = support::render(&mut app, 52, 16);
    let rows: Vec<&str> = support::text_block(&opened).lines().collect();

    // What the commit before it had there, above the line that replaced it.
    let removed = rows
        .iter()
        .position(|row| row.contains("old two"))
        .unwrap_or_else(|| panic!("the commit's own change did not open:\n{opened}"));
    assert!(
        rows[removed + 1].contains("new two"),
        "the removed line is not above the line that replaced it:\n{opened}"
    );
    // And not a word about the file on disk: this buffer is a commit's
    // version, and what it is measured against is the commit before it.
    assert!(
        !opened.contains("working two"),
        "the hunk is against the working tree rather than the commit:\n{opened}"
    );
}

/// The first line of a commit's version has its block slot to itself.
///
/// It did not. The message hung there, a line has room for one block, and
/// `alt+d` on the line the commit changed could only say so -- the one line
/// in the file where the key that asks what a line changed from had nothing
/// to give. The message is read in the preview now and hangs over nothing,
/// so the slot is the hunk's.
///
/// Broken deliberately by hanging the message above the first line again in
/// `read_at_commit`: the hunk had nowhere to go and what opened was the
/// message.
#[test]
fn the_first_lines_hunk_opens_in_a_commits_version() {
    use crossterm::event::KeyCode;
    use obelus_app::app::App;
    use obelus_buffer::Buffer;
    use obelus_command::Command;

    // The commit changes the first line, which is the line whose hunk used
    // to have nowhere to open.
    let repository = Repository::new("hunk-and-message", "old one\ntwo\n");
    repository.write("new one\ntwo\n");
    repository.commit("the second");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 100, 12);
    support::press_function(&mut app, 9);
    support::read_history(&mut app, &events);
    support::press(&mut app, KeyCode::Enter);

    // Nothing of the commit's prose is in the way, and nothing of it is on
    // screen: what the reader asked for was the file.
    let dump = support::render(&mut app, 100, 12);
    assert!(
        !support::text_block(&dump).contains("the second"),
        "the message is hanging over the file again:\n{dump}"
    );

    obelus_app::app::dispatch::dispatch(&mut app, Command::GitHunk);
    let dump = support::render(&mut app, 100, 12);
    let rows: Vec<&str> = support::text_block(&dump).lines().collect();
    let removed = rows
        .iter()
        .position(|row| row.contains("old one"))
        .unwrap_or_else(|| panic!("the first line's hunk did not open:\n{dump}"));
    assert!(
        rows[removed + 1].contains("new one"),
        "what was there is not above what replaced it:\n{dump}"
    );
    // And no word about anything being in the way, because nothing is.
    assert!(
        !support::text_block(&dump).contains("message hangs where"),
        "the key still says the message is in its slot:\n{dump}"
    );
}

#[test]
fn a_query_about_a_history_is_a_question_about_commits() {
    use crossterm::event::KeyCode;
    use obelus_app::app::App;
    use obelus_buffer::Buffer;

    let repository = Repository::new("history-nested-query", "one\n");
    // A commit whose subject shares nothing with the file it changed, and a
    // file whose name shares nothing with the commit that changed it.
    std::fs::write(repository.directory().join("kettle.rs"), "boil\n").expect("the other file");
    repository.commit_all("Teach the parser to fold");
    repository.write("two\n");
    repository.commit("Something else entirely");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 10);
    support::read_history(&mut app, &events);

    // Open the commit, so its files are rows of the list too.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    let rows: Vec<String> = app
        .picker()
        .expect("the history")
        .matches()
        .map(|item| item.label.clone())
        .collect();
    assert!(
        rows.iter().any(|row| row.contains("kettle.rs")),
        "the commit did not open its files: {rows:?}"
    );

    // A query is about the commits. The file it changed goes on hanging
    // under it, whether or not its name has anything to do with the words
    // the reader typed.
    support::type_text(&mut app, "fold");
    let rows: Vec<String> = app
        .picker()
        .expect("the history")
        .matches()
        .map(|item| item.label.clone())
        .collect();
    assert_eq!(
        rows,
        ["Teach the parser to fold", "kettle.rs"],
        "a query emptied a matching commit of the files it was opened to show"
    );

    // And a query the commit does not answer takes its files with it: a row
    // about a change with nothing on screen saying which change is not a
    // row anybody can read.
    support::type_text(&mut app, "\u{8}\u{8}\u{8}\u{8}kettle");
    let rows: Vec<String> = app
        .picker()
        .expect("the history")
        .matches()
        .map(|item| item.label.clone())
        .collect();
    assert!(
        rows.is_empty(),
        "a file was left on screen without the commit it belongs to: {rows:?}"
    );
}

#[test]
fn a_blame_is_about_the_version_on_screen() {
    use crossterm::event::KeyCode;
    use obelus_app::app::App;
    use obelus_buffer::Buffer;

    let repository = Repository::new("blame-of-a-version", "one\n");
    repository.write("one\ntwo\n");
    repository.commit("the second");
    repository.write("one\ntwo\nthree\n");
    repository.commit("the third");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 70, 14);

    // A version of the file has a blame of its own -- it used to have none
    // at all, because a blame was a walk from `HEAD` and its lines would
    // have named whoever last touched those numbers today.
    support::press_function(&mut app, 9);
    support::read_history(&mut app, &events);
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    // The walk is asked for while drawing, which is where obelus finds out
    // what is on screen to be asked about.
    support::render(&mut app, 70, 14);
    let at = std::time::Instant::now();
    while app.blame().is_none() && at.elapsed() < std::time::Duration::from_secs(20) {
        match events.recv_timeout(std::time::Duration::from_secs(20)) {
            Ok(event) => app.handle(event),
            Err(_) => break,
        }
    }
    let blame = app.blame().expect("a blame of the version");
    // The version opened is the one before the last, which had two lines.
    assert_eq!(
        blame.len(),
        2,
        "the blame is of a different version than the one on screen"
    );
}

#[test]
fn a_line_opens_the_commit_that_wrote_it() {
    use crossterm::event::KeyCode;
    use obelus_app::app::App;
    use obelus_buffer::Buffer;

    let repository = Repository::new("line-commit", "a\nb\n");
    // Two lines committed above the old ones, so the line a reader points
    // at is not the line it was in the commit that wrote it.
    repository.write("x\ny\na\nb\n");
    repository.commit("Put two at the top");
    // And one more that is not committed at all, so the line is not even
    // where the blame has it: a name laid beside it has to be carried back
    // through the changes first, and so does the line this key opens.
    repository.write("u\nx\ny\na\nb\n");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 70, 14);
    support::render(&mut app, 70, 14);
    let at = std::time::Instant::now();
    while app.blame().is_none() && at.elapsed() < std::time::Duration::from_secs(20) {
        match events.recv_timeout(std::time::Duration::from_secs(20)) {
            Ok(event) => app.handle(event),
            Err(_) => break,
        }
    }
    assert!(app.blame().is_some(), "the blame never arrived");

    // Onto "b": the fifth line on screen, the fourth the last commit has,
    // and the *second* line of the run that came from the commit that
    // wrote it -- where it was the second line of a two-line file.
    for _ in 0..4 {
        support::press(&mut app, KeyCode::Down);
    }
    obelus_app::app::dispatch::dispatch(&mut app, obelus_command::Command::HistoryLine);

    let dump = support::render(&mut app, 70, 14);
    let text = support::text_block(&dump);
    // The commit that wrote it, not the one that pushed it down -- said by
    // what the version holds, since the message no longer hangs over it:
    // that commit's file is the two lines it started as, and the one that
    // pushed it down has four.
    assert!(
        text.contains(" 1 a") && text.contains(" 2 b"),
        "not the file as the commit that wrote the line had it:\n{dump}"
    );
    assert!(
        !text.contains(" 1 x"),
        "the version open is a later one, whose first line that commit never had:\n{dump}"
    );
    // And on that line as that commit had it: the second of two, not the
    // fifth of a file that did not exist yet.
    assert!(
        text.lines()
            .next_back()
            .is_some_and(|status| status.contains("2:1")),
        "the caret did not land on the line as that commit had it:\n{dump}"
    );
}

#[test]
fn the_commit_that_wrote_a_line_is_where_the_walk_stops() {
    use obelus_app::app::{App, dispatch};
    use obelus_buffer::Buffer;
    use obelus_command::Command;

    let repository = Repository::new("line-commit-end", "a\nb\n");
    repository.write("x\ny\na\nb\n");
    repository.commit("Put two at the top");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 70, 14);
    let settle = |app: &mut App| {
        support::render(app, 70, 14);
        let at = std::time::Instant::now();
        while app.blame().is_none() && at.elapsed() < std::time::Duration::from_secs(20) {
            match events.recv_timeout(std::time::Duration::from_secs(20)) {
                Ok(event) => app.handle(event),
                Err(_) => break,
            }
        }
        support::render(app, 70, 14);
    };
    settle(&mut app);

    // Onto "x", which the newest commit wrote.
    dispatch::dispatch(&mut app, Command::HistoryLine);
    settle(&mut app);
    let opened = app.file_count_for_test();

    // The version it opened is the one that wrote that line, so asking
    // again has nowhere to go. It says so, and it does not leave another
    // buffer behind for every press.
    for _ in 0..3 {
        dispatch::dispatch(&mut app, Command::HistoryLine);
    }
    let dump = support::render(&mut app, 70, 14);
    assert!(
        support::text_block(&dump).contains("This commit wrote this line"),
        "pressing on says nothing about why nothing happened:\n{dump}"
    );
    assert_eq!(
        app.file_count_for_test(),
        opened,
        "a buffer was opened for every press that had nowhere to go"
    );
}

#[test]
fn the_refs_tab_opens_this_file_as_a_name_has_it() {
    use crossterm::event::KeyCode;
    use obelus_app::app::App;
    use obelus_buffer::Buffer;

    let repository = Repository::new("refs-tab", "on master\n");
    // A branch with its own version of the file, and no commit of it
    // reachable from where the reader stands: a history of this file cannot
    // list it, because a history is walked from `HEAD`.
    repository.run(&["checkout", "-q", "-b", "side"]);
    repository.write("on the side\n");
    repository.commit("What the side did");
    repository.run(&["checkout", "-q", "master"]);

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 74, 16);
    support::press_function(&mut app, 9);
    support::read_history(&mut app, &events);

    // The file's own history knows nothing of the branch.
    let rows: Vec<String> = app
        .picker()
        .expect("the history")
        .matches()
        .map(|item| item.label.clone())
        .collect();
    assert_eq!(rows, ["committed"], "a walk from HEAD reached the branch");

    // The refs tab does: both names, and the one being read marked. Not in
    // a fixed order -- two branches committed in the same second are two
    // commits of the same age, and the tie goes to the name.
    support::press(&mut app, KeyCode::Tab);
    let picker = app.picker().expect("the history");
    let rows: Vec<(String, bool)> = picker
        .matches()
        .map(|item| (item.label.clone(), item.marker.is_some()))
        .collect();
    let mut names: Vec<&str> = rows.iter().map(|(name, _)| name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["master", "side"], "the refs are not both there");
    assert_eq!(
        rows.iter()
            .filter(|(_, marked)| *marked)
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["master"],
        "the mark is not on the name being read"
    );

    // And choosing one opens this file as that name has it, in a buffer of
    // its own -- without checking anything out.
    let side = rows
        .iter()
        .position(|(name, _)| name == "side")
        .expect("the branch");
    for _ in 0..side {
        support::press(&mut app, KeyCode::Down);
    }
    support::press(&mut app, KeyCode::Enter);
    let dump = support::render(&mut app, 74, 16);
    assert!(
        support::text_block(&dump).contains("on the side"),
        "the branch's version did not open:\n{dump}"
    );
    assert_eq!(
        std::fs::read_to_string(repository.path()).expect("the file on disk"),
        "on master\n",
        "reading a branch changed the working tree"
    );
}

#[test]
fn a_query_for_a_name_finds_the_nearest_name() {
    use crossterm::event::KeyCode;
    use obelus_app::app::App;
    use obelus_buffer::Buffer;

    let repository = Repository::new("refs-query", "one\n");
    // A name that is exactly what a reader would type, made first, and
    // names that merely contain those letters made after it. In a list that
    // kept its own order the newest of those would come first and the one
    // asked for would be last.
    repository.run(&["tag", "v0.1"]);
    repository.write("two\n");
    repository.commit("the second");
    repository.run(&["branch", "cherry-pick-v0.123.x"]);
    repository.run(&["branch", "revert-v0.144.x-something"]);

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 74, 16);
    support::press_function(&mut app, 9);
    support::read_history(&mut app, &events);
    support::press(&mut app, KeyCode::Tab);
    support::type_text(&mut app, "v0.1");

    let rows: Vec<String> = app
        .picker()
        .expect("the history")
        .matches()
        .map(|item| item.label.clone())
        .collect();
    assert_eq!(
        rows.first().map(String::as_str),
        Some("v0.1"),
        "the name typed is not the first name offered: {rows:?}"
    );
}

#[test]
fn the_commit_behind_a_line_can_be_asked_for_with_the_names_off() {
    use obelus_app::app::{App, dispatch};
    use obelus_buffer::Buffer;
    use obelus_command::Command;

    let repository = Repository::new("line-commit-no-names", "a\nb\n");
    repository.write("x\na\nb\n");
    repository.commit("Put one at the top");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    // The margin's names turned off, which is a question about the margin.
    app.configure(
        obelus_config::Config {
            blame_margin: false,
            ..Default::default()
        },
        Vec::new(),
    );
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 74, 14);
    support::render(&mut app, 74, 14);

    // The command is offered: whether this line has a commit behind it is
    // the answer, and a row greyed until an answer nothing will ask for
    // would be a row greyed for ever.
    assert!(
        app.offers(Command::HistoryLine),
        "the command is greyed out for a reader who keeps the names off"
    );

    // One press. Nothing has been asked yet, so this is the asking -- and
    // it says so, which it could not while nothing was being read.
    dispatch::dispatch(&mut app, Command::HistoryLine);
    let dump = support::render(&mut app, 74, 14);
    assert!(
        support::text_block(&dump).contains("still reading who wrote this"),
        "the key said nothing about what it had started:\n{dump}"
    );

    // And the answer finishes it. No second press: a key that has to be
    // pressed twice for the readers who turned the names off is a key that
    // works for the readers who did not.
    let at = std::time::Instant::now();
    while at.elapsed() < std::time::Duration::from_secs(20) {
        match events.recv_timeout(std::time::Duration::from_secs(20)) {
            Ok(event) => app.handle(event),
            Err(_) => break,
        }
        // Asked of the document rather than of the screen: what says the
        // commit opened is that what is being read is a commit's version,
        // and the commit's own words are no longer drawn over the file to
        // be read off it.
        support::render(&mut app, 74, 14);
        if app
            .current_buffer()
            .is_some_and(|buffer| buffer.content().at().is_some())
        {
            return;
        }
    }
    let dump = support::render(&mut app, 74, 14);
    panic!("the commit never opened with the margin's names off:\n{dump}");
}

#[test]
fn the_short_answer_about_a_tree_agrees_with_the_long_one() {
    use obelus_git::{anything_changed, statuses};

    let repository = Repository::new("anything-changed", "one\n");
    let root = repository.directory();
    assert_eq!(
        anything_changed(&root),
        !statuses(&root).is_empty(),
        "a clean tree is two different answers"
    );

    // One file the reader has touched, which is the case the short answer
    // exists for: it stops at the first, where the long one goes on to
    // build a map of every path to be asked its length.
    repository.write("two\n");
    assert!(
        anything_changed(&root),
        "a changed tree says nothing changed"
    );
    assert_eq!(
        anything_changed(&root),
        !statuses(&root).is_empty(),
        "a changed tree is two different answers"
    );

    // And a file git has never seen, which arrives by a different door.
    repository.commit("the second");
    std::fs::write(root.join("unseen.rs"), "new\n").expect("the new file");
    assert_eq!(
        anything_changed(&root),
        !statuses(&root).is_empty(),
        "a tree with something new in it is two different answers"
    );
    assert!(anything_changed(&root), "a new file is nothing changed");
}

/// What a file looked like when it was committed is read once per file, not
/// once per keystroke.
///
/// Reading it is opening the repository, finding the commit, walking its
/// tree and unpacking the blob -- two thirds of what the margin cost on
/// every key pressed, for an answer that moves only when the repository
/// does.
#[test]
fn the_committed_text_is_not_read_again_for_every_keystroke() {
    use obelus_app::app::App;
    use obelus_buffer::Buffer;

    let repository = Repository::new("committed-once", "one\ntwo\nthree\n");
    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory.clone());
    support::lay_out(&mut app, 60, 12);

    // The margin has something to say, which is what says the diff happened
    // at all.
    support::type_text(&mut app, "x");
    let dump = support::render(&mut app, 60, 12);
    assert!(
        app.changes().is_some_and(|changes| !changes.is_empty()),
        "nothing was compared:\n{dump}"
    );

    // And it goes on saying it with the repository taken away underneath:
    // the committed text is in hand, so nothing has to be read again.
    std::fs::remove_dir_all(repository.directory.join(".git")).expect("taking the repository away");
    support::type_text(&mut app, "y");
    // Drawn, because that is when the margin's diff is worked out again:
    // asking the application without drawing would be reading the answer
    // from before the repository went.
    let after = support::render(&mut app, 60, 12);
    assert!(
        app.changes().is_some_and(|changes| !changes.is_empty()),
        "the committed text was read again rather than kept:\n{after}"
    );
}

/// The list of what is open opens on the one being read, even where two rows
/// say the same thing.
///
/// A file and that file as a commit had it wear one path, which is the label
/// both rows carry. Asking the list for "the row that says this" then finds
/// whichever of them is nearer the top, and the reader is handed a document
/// they are not in -- so the list is pointed at the row itself.
#[test]
fn the_document_list_opens_on_this_version_and_not_the_other() {
    use crossterm::event::KeyCode;
    use obelus_app::app::App;
    use obelus_buffer::Buffer;

    let repository = Repository::new("history-two-rows", "first\n");
    repository.write("second\n");
    repository.commit("the second");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    app.statuses_for_test(std::collections::HashMap::new());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 60, 16);
    // The project's history, a commit, and one of its files: a second
    // document at the same path as the first.
    support::press_function(&mut app, 10);
    support::read_history(&mut app, &events);
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);
    assert!(
        app.current_buffer()
            .is_some_and(|buffer| buffer.content().at().is_some()),
        "not reading a commit's version"
    );

    support::press_function(&mut app, 2);
    let picker = app.picker().expect("the list of what is open");
    let labels: Vec<String> = picker.matches().map(|item| item.label.clone()).collect();
    assert_eq!(
        labels.len(),
        2,
        "not two documents at one path, so this proves nothing: {labels:?}"
    );
    assert_eq!(
        labels[0], labels[1],
        "the rows do not say the same thing, so this proves nothing: {labels:?}"
    );
    // Told apart by the short id, which is the one thing on the row that
    // differs: a commit's version has one and the file on disk has none.
    assert!(
        picker
            .selected_item()
            .is_some_and(|item| item.trailing.is_some()),
        "the list opened on the file rather than on the commit's version"
    );
}

/// A project that asks for CRLF has a margin that says nothing changed.
///
/// git stores `\n` and checks the file out as `\r\n`, so the buffer the
/// reader has and the blob the margin compares it against differ on every
/// single line. Before the diff base was converted the way a checkout
/// converts it, every line of every file in such a project was marked as
/// changed -- a margin that is wrong about everything, which is the same as
/// having no margin.
#[test]
fn a_project_that_asks_for_crlf_has_an_honest_margin() {
    let repository = Repository::new("crlf", "one\r\ntwo\r\n");
    std::fs::write(
        repository.directory().join(".gitattributes"),
        "* text eol=crlf\n",
    )
    .expect("the attributes");
    repository.run(&["rm", "--cached", "--quiet", "file.rs"]);
    std::fs::write(repository.path(), "one\r\ntwo\r\n").expect("the file");
    repository.commit_all("with attributes");

    // git really did store it with `\n`, or this test is about nothing.
    let stored = std::process::Command::new("git")
        .arg("-C")
        .arg(repository.directory())
        .args(["show", "HEAD:file.rs"])
        .output()
        .expect("git show");
    assert_eq!(
        String::from_utf8_lossy(&stored.stdout),
        "one\ntwo\n",
        "git did not normalise the file, so there is nothing to convert"
    );

    let base = obelus_git::head_text(&repository.path()).expect("a diff base");
    assert_eq!(
        base, "one\r\ntwo\r\n",
        "the diff base is not what a checkout would put on disk"
    );
    let changes = obelus_git::Changes::between(
        &base,
        &std::fs::read_to_string(repository.path()).expect("the file"),
    );
    assert!(
        changes.is_empty(),
        "a file nobody has touched is marked as changed: {changes:?}"
    );
}

/// And a repository does not get to run a program because obelus looked at
/// it.
///
/// Converting a blob the way a checkout would is what makes the margin
/// honest, and it is also what runs a `filter.*` driver -- a program named
/// by the repository's own config. `gix::discover` would call a checkout
/// the reader happens to own fully trusted and run it. A code reader that
/// executes a stranger's code because it was pointed at their clone is not
/// a reader, so the trust level is obelus's decision: reduced, which keeps
/// the conversion and refuses the program.
#[test]
fn a_repository_does_not_get_to_run_a_program_because_obelus_read_it() {
    let repository = Repository::new("driver", "hello\n");
    let marker = repository.directory().join("THE-DRIVER-RAN");
    std::fs::write(
        repository.directory().join(".gitattributes"),
        "* filter=evil\n",
    )
    .expect("the attributes");
    repository.commit_all("with a driver");
    repository.run(&[
        "config",
        "filter.evil.smudge",
        &format!("sh -c 'touch {} ; cat'", marker.display()),
    ]);

    let base = obelus_git::head_text(&repository.path()).expect("a diff base");
    assert_eq!(base, "hello\n", "the content did not survive the refusal");
    assert!(
        !marker.exists(),
        "obelus ran a program the repository named, just by reading the file"
    );
}

/// A move git has been told about is one row with both names on it.
///
/// `git status` writes it `R old -> new`, and this list says the same
/// thing. Only a move git itself reports, which is one that has been
/// staged: a file moved in the working tree and not staged is a deletion
/// and an untracked file to git, and pairing those two up would be obelus's
/// inference rather than the tree's state.
#[test]
fn a_move_git_knows_about_is_one_row_with_the_name_it_had() {
    let lines: String = (0..40).map(|n| format!("line {n}\n")).collect();
    let repository = Repository::new("statuses-moved", &lines);
    std::fs::create_dir_all(repository.directory().join("deep")).expect("a directory");
    repository.run(&["mv", "file.rs", "deep/moved.rs"]);
    std::fs::write(
        repository.directory().join("deep/moved.rs"),
        format!("{lines}an edit\n"),
    )
    .expect("editing it where it landed");

    let statuses = obelus_git::statuses(&repository.directory());
    let moved = statuses
        .get(&repository.directory().join("deep/moved.rs"))
        .expect("the file where it is now");
    assert_eq!(
        moved.was,
        Some(std::path::PathBuf::from("file.rs")),
        "a move is not said to be one: {statuses:?}"
    );
    assert!(
        !statuses.contains_key(&repository.path()),
        "a move is two rows, the way it would be without the inference: {statuses:?}"
    );
}

/// The list of what changed says what git says has changed.
///
/// Two of the things git reports are not files to open, and both used to be
/// dropped for it. A file the reader deleted is not there, and a submodule
/// -- recorded as a commit at a path and reported as changed the moment
/// that commit moves -- is a directory. But a list of changes that leaves
/// out changes is a list that disagrees with `git status` while looking
/// complete, and that was the worse answer: what a deleted file opens is
/// what the last commit had, and a submodule is drawn as the row nobody can
/// press.
#[test]
fn what_git_says_has_changed_is_what_the_list_says() {
    let outer = Repository::new("on-disk-outer", "main\n");
    let inner = Repository::new("on-disk-inner", "one\n");
    outer.run(&[
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        "--quiet",
        inner.directory().to_str().expect("a path"),
        "vendor",
    ]);
    let gone = outer.directory().join("gone.rs");
    std::fs::write(&gone, "gone\n").expect("the file");
    outer.commit_all("a submodule and a file to delete");
    assert!(
        outer.directory().join("vendor").is_dir(),
        "no submodule to test with"
    );

    // The submodule's own HEAD moves, the file goes, and one ordinary file
    // changes -- so a list that reported nothing could not pass this.
    let within = outer.directory().join("vendor");
    std::fs::write(within.join("file.rs"), "two\n").expect("the file");
    for arguments in [
        &["add", "-A"][..],
        &["commit", "--quiet", "-m", "moved on"][..],
    ] {
        let outcome = std::process::Command::new("git")
            .arg("-C")
            .arg(&within)
            .args(arguments)
            .env("GIT_AUTHOR_NAME", "obelus")
            .env("GIT_AUTHOR_EMAIL", "obelus@example.invalid")
            .env("GIT_COMMITTER_NAME", "obelus")
            .env("GIT_COMMITTER_EMAIL", "obelus@example.invalid")
            .output()
            .expect("running git");
        assert!(outcome.status.success(), "git {arguments:?} failed");
    }
    std::fs::remove_file(&gone).expect("removing it");
    outer.write("main\nand more\n");

    let statuses = obelus_git::statuses(&outer.directory());
    assert!(
        statuses.contains_key(&outer.path()),
        "the file that really did change is missing: {statuses:?}"
    );
    // A file that is gone is a change to the tree and git reports it as
    // one. It is listed, and what it opens is what the last commit had:
    // there is no other version of it left.
    assert_eq!(
        statuses.get(&gone).map(|standing| standing.status),
        Some(obelus_git::FileStatus::Gone),
        "a file the reader deleted is not among the changes: {statuses:?}"
    );
    // So is a submodule, which git reports as changed the moment the commit
    // it records moves. It is not a file obelus can open, and the row says
    // so -- rather than being left out of a list that claims to be what git
    // says has changed.
    let vendor = statuses
        .get(&outer.directory().join("vendor"))
        .expect("a submodule that moved on is not among the changes");
    assert!(
        vendor.submodule,
        "a submodule is offered as a file to open: {vendor:?}"
    );

    // And once they are staged, which arrives down a different arm: the
    // index against `HEAD` rather than the working tree against the index.
    // A submodule is told apart there by the mode git keeps for one -- a
    // gitlink is neither a file nor a tree -- because the directory on disk
    // looks the same either way.
    outer.run(&["add", "-A"]);
    let staged = obelus_git::statuses(&outer.directory());
    assert!(
        staged
            .get(&outer.directory().join("vendor"))
            .is_some_and(|standing| standing.submodule),
        "a staged submodule is offered as a file to open: {staged:?}"
    );
    assert_eq!(
        staged.get(&gone).map(|standing| standing.status),
        Some(obelus_git::FileStatus::Gone),
        "a staged deletion is not among the changes: {staged:?}"
    );
}

/// And a path that goes away *after* the list was built still says so.
///
/// The row was fine when the list was drawn, so nothing dimmed it, and by
/// the time the reader presses enter the file is gone. The list closes on
/// the way to trying, which is what makes the status row free to say it --
/// and without this, obelus closed the list, opened nothing and said
/// nothing at all.
#[test]
fn a_path_that_went_away_after_the_list_was_built_says_so() {
    use obelus_app::app::App;
    use obelus_buffer::Buffer;

    let repository = Repository::new("open-vanished", "one\n");
    let gone = repository.directory().join("gone.rs");
    std::fs::write(&gone, "gone\n").expect("the file");
    repository.commit_all("two files");
    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    // Not the file being read: one already open is switched to rather than
    // opened, and this is about the path that cannot be read at all.
    std::fs::remove_file(&gone).expect("removing it");

    app.open_for_test(&gone);
    assert_eq!(
        app.note(),
        Some("gone.rs is not there any more"),
        "opening nothing said nothing"
    );

    // And a directory, which is the other way a path in a list names
    // something that is not a file.
    app.open_for_test(&repository.directory().join("src"));
    std::fs::create_dir_all(repository.directory().join("src")).expect("the directory");
    app.open_for_test(&repository.directory().join("src"));
    assert_eq!(
        app.note(),
        Some("src is a directory"),
        "opening a directory said something else"
    );
}

/// The margin and the map are reserved by there being a change, not by
/// there being a repository.
///
/// A file in a clean tree would otherwise spend two columns on an answer
/// of "nothing", which is most files most of the time. What that costs is
/// a cell of sideways shift when a file does change under the reader --
/// and with wrapping on, a rewrap -- paid where there is news worth the
/// column.
///
/// Both of them carry git's news and nothing else. What a server says is
/// wrong is said by the underline under the word, by the complaint framed
/// under the caret's line, by the count on the status row and by the key
/// that walks to the next one; a mark here would be a fourth telling, in a
/// column whose colours it cannot be told apart from.
///
/// Broken deliberately by reserving either of them for a file that is
/// merely *in* a repository.
#[test]
fn the_margin_and_the_map_are_reserved_by_there_being_a_change() {
    use obelus_app::app::App;
    use obelus_buffer::Buffer;

    let committed = "fn main() {\n    let a = 1;\n    let b = 2;\n}\n";
    let repository = Repository::new("reserved", committed);

    let read = |app: &mut App| {
        let cells = support::cells_of(app, 30, 12);
        let dump = support::render(app, 30, 12);
        (cells, dump)
    };
    // Nothing changed yet, so neither column is there and the text starts
    // at the very first cell.
    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    support::lay_out(&mut app, 30, 12);
    let (cells, dump) = read(&mut app);
    let row = |cells: &ratatui::buffer::Buffer, y: u16| {
        (0..30)
            .map(|x| cells.cell((x, y)).expect("a cell").symbol().to_string())
            .collect::<String>()
    };
    // Where the text begins, which is what a reserved column moves.
    let starts = |cells: &ratatui::buffer::Buffer| {
        let row = row(cells, 0);
        row.find("fn").map(|byte| row[..byte].chars().count())
    };
    let clean = starts(&cells).expect("the first line");
    assert!(
        !row(&cells, 0).contains('\u{258c}'),
        "a clean file has something on its map:\n{dump}"
    );

    // Change a line and both arrive: the margin marks it, and the map has
    // it too.
    repository.write("fn main() {\n    let a = 9;\n    let b = 2;\n}\n");
    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    support::lay_out(&mut app, 30, 12);
    let (cells, dump) = read(&mut app);
    assert_eq!(
        cells.cell((0, 1)).expect("a cell").symbol(),
        "\u{2590}",
        "the margin does not mark the changed line:\n{dump}"
    );
    let marked = (0..10)
        .filter(|y| cells.cell((28, *y)).expect("a cell").symbol() == "\u{258c}")
        .count();
    assert_eq!(
        marked, 1,
        "the map does not have the one change on it:\n{dump}"
    );
    // And the map is one column, not two: the cell before it is the text's.
    assert_ne!(
        cells.cell((27, 1)).expect("a cell").symbol(),
        "\u{258c}",
        "the map is two columns wide:\n{dump}"
    );
    // And the margin cost the text exactly the one column it takes.
    assert_eq!(
        starts(&cells).expect("the first line"),
        clean + 1,
        "the margin arrived without costing the text a column, or cost it two:\n{dump}"
    );
}

/// Choosing a row of a file's own history opens that commit's version.
///
/// Not the file as it is now, which is what a row would open if the name it
/// was walked under were thrown away -- and every row older than a move is
/// under a name the working tree has not got.
#[test]
fn a_row_of_a_files_history_opens_that_commits_version() {
    use crossterm::event::KeyCode;
    use obelus_app::app::{App, dispatch};
    use obelus_buffer::Buffer;
    use obelus_command::Command;

    let lines: String = (0..40).map(|n| format!("line {n}\n")).collect();
    let repository = Repository::new("history-opening", &lines);
    repository.write(&format!("{lines}an edit\n"));
    repository.commit("editing it where it was");
    std::fs::create_dir_all(repository.directory().join("deep")).expect("a directory");
    repository.run(&["mv", "file.rs", "deep/moved.rs"]);
    std::fs::write(
        repository.directory().join("deep/moved.rs"),
        format!("{lines}an edit\nand another\n"),
    )
    .expect("the file where it landed");
    repository.run(&["add", "-A"]);
    repository.run(&["commit", "--quiet", "-m", "moving it"]);

    let moved = repository.directory().join("deep/moved.rs");
    let mut app = App::new(vec![Buffer::open(&moved).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 76, 24);
    dispatch::dispatch(&mut app, Command::HistoryFile);
    support::read_history(&mut app, &events);

    // Down once: onto the commit before the move, which is under a name
    // the working tree has not got.
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);

    let buffer = app.current_buffer().expect("a buffer");
    let read = buffer.text().rope().to_string();
    assert_eq!(
        read.lines().count(),
        41,
        "opened something else: {} lines",
        read.lines().count()
    );
    assert!(
        !read.contains("and another"),
        "the row opened the file as it is now, not as that commit had it"
    );
}

/// The preview shows the row's own version, not the file as it is now.
///
/// It asked for the name the file has, and every row older than a move is
/// under the name it had: there was nothing to show, so the pane kept
/// whatever was in it -- which is the newest version, and reads as a
/// history where every commit says the same thing.
#[test]
fn the_preview_of_a_history_row_is_that_commits_version() {
    use crossterm::event::KeyCode;
    use obelus_app::app::{App, dispatch};
    use obelus_buffer::Buffer;
    use obelus_command::Command;

    let lines: String = (0..40).map(|n| format!("line {n}\n")).collect();
    let repository = Repository::new("history-previewing", &lines);
    repository.write(&format!("{lines}an edit\n"));
    repository.commit("editing it where it was");
    std::fs::create_dir_all(repository.directory().join("deep")).expect("a directory");
    repository.run(&["mv", "file.rs", "deep/moved.rs"]);
    std::fs::write(
        repository.directory().join("deep/moved.rs"),
        format!("{lines}an edit\nand another\n"),
    )
    .expect("the file where it landed");
    repository.run(&["add", "-A"]);
    repository.run(&["commit", "--quiet", "-m", "moving it"]);

    let moved = repository.directory().join("deep/moved.rs");
    let mut app = App::new(vec![Buffer::open(&moved).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    let events = support::drive(&mut app);
    support::lay_out(&mut app, 76, 24);
    dispatch::dispatch(&mut app, Command::HistoryFile);
    support::read_history(&mut app, &events);

    let shown = |app: &mut App| -> usize {
        support::lay_out(app, 76, 24);
        app.preview()
            .map(|preview| preview.buffer.text().rope().to_string().lines().count())
            .expect("a preview")
    };

    // Newest first: the move, then the edit before it, then the first
    // commit -- forty-two lines, forty-one, forty.
    assert_eq!(shown(&mut app), 42, "the newest row");
    support::press(&mut app, KeyCode::Down);
    assert_eq!(
        shown(&mut app),
        41,
        "the row before the move previews the file as it is now"
    );
    support::press(&mut app, KeyCode::Down);
    assert_eq!(shown(&mut app), 40, "the oldest row");
}

/// The mark in the change margin opens what the line replaced.
///
/// A click to the left of the words used to mean one thing everywhere: the
/// start of that row. Which is right for the numbers -- pointing left of
/// the words is how a whole line is reached -- and wrong for the column
/// outside them. The change margin is the picture of the key that opens
/// what a line replaced, and a mark like that is clicked everywhere a
/// reader has met one.
///
/// It goes to that line first, because the key asks about the line the
/// caret is on: the reader pointed at a line, so that is the line.
///
/// Broken deliberately by taking the change column back into "the start of
/// that row", which puts the caret there and leaves the hunk shut.
#[test]
fn the_change_margin_opens_what_the_line_replaced() {
    use obelus_app::app::App;
    use obelus_buffer::Buffer;

    let repository = Repository::new("margin-click", "one\nold a\nthree\n");
    repository.write("one\nnew a\nthree\n");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    support::lay_out(&mut app, 40, 16);
    let _ = support::render(&mut app, 40, 16);
    assert!(
        app.opened_hunks().is_empty(),
        "something was open before anybody asked"
    );

    // The margin is the first column of the region, outside the numbers.
    let lines = app.current_buffer().expect("a file").text().line_count();
    assert_eq!(
        obelus_ui::editor::margin_at(0, lines, true, false),
        obelus_ui::editor::Margin::Changes,
        "that is not the column the change margin is drawn in"
    );

    // Clicked on the changed line, which is the second.
    let area = app.editor_area_for_test();
    app.handle(obelus_app::event::Event::Pointer {
        kind: obelus_app::event::Pointer::Pressed,
        x: area.x,
        y: area.y + 1,
    });
    let dump = support::render(&mut app, 40, 16);
    let text = support::text_block(&dump);
    assert!(
        text.contains("old a"),
        "the click on the margin opened nothing:\n{text}"
    );
    assert_eq!(
        app.current_buffer().expect("a file").cursor().line.get(),
        1,
        "the click did not go to the line it was on"
    );

    // And again, it shuts.
    app.handle(obelus_app::event::Event::Pointer {
        kind: obelus_app::event::Pointer::Pressed,
        x: area.x,
        y: area.y + 1,
    });
    assert!(
        app.opened_hunks().is_empty(),
        "the same click did not shut it again"
    );

    // While the numbers beside it still mean the start of that row.
    let numbers = obelus_ui::editor::MARGIN_WIDTH;
    app.handle(obelus_app::event::Event::Pointer {
        kind: obelus_app::event::Pointer::Pressed,
        x: area.x + numbers,
        y: area.y + 1,
    });
    assert!(
        app.opened_hunks().is_empty(),
        "a click on the numbers opened a hunk"
    );
    assert_eq!(
        app.current_buffer().expect("a file").cursor().line.get(),
        1,
        "a click on the numbers did not reach the line"
    );
}
