//! What changed since the last commit.
//!
//! Values in, hunks out: the three cases a reader sees in the margin are
//! decided here, and each of them is a different claim about the file. A
//! full-height bar beside a line says "this line is different"; a mark at a
//! seam says "lines are missing here". Getting the two confused is a margin
//! that lies about which lines the reader is looking at.

mod support;

use obelus::{
    app::App,
    buffer::Buffer,
    coordinates::LineNumber,
    event::Event,
    git::{Changes, Marker},
};

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
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs");
    let committed = obelus::git::head_text(&path).expect("src/lib.rs is committed");
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
    assert_eq!(obelus::git::head_text(&missing), None);
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
        git(&["init", "--quiet"]);
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

    /// Commits everything in the tree, for a test that changes more than
    /// the one file.
    fn commit_all(&self, message: &str) {
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
        git(&["add", "-A"]);
        git(&["commit", "--quiet", "-m", message]);
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

    let committed = obelus::git::head_text(&repository.path()).expect("the committed text");
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
    let committed = obelus::git::head_text(&untouched.path()).expect("the committed text");
    let working = std::fs::read_to_string(untouched.path()).expect("the working tree");
    assert!(Changes::between(&committed, &working).is_empty());
}

/// The margin on screen: one column at the far left, a solid bar for a line
/// that differs and a mark hugging the seam where lines are missing.
#[test]
fn the_margin_marks_the_lines_on_screen() {
    use obelus::{app::App, buffer::Buffer};

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
    use obelus::{app::App, buffer::Buffer, command::Command};

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

    obelus::command::dispatch::dispatch(&mut app, Command::GitHunk);
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
    obelus::command::dispatch::dispatch(&mut app, Command::GitHunk);
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
    use obelus::{
        app::App,
        buffer::Buffer,
        command::{Command, dispatch},
    };

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
    use obelus::{
        app::App,
        buffer::Buffer,
        command::{Command, dispatch},
    };

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
    use obelus::{
        app::App,
        buffer::{Block, Buffer},
        command::{Command, dispatch},
    };

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
    use obelus::{
        app::App,
        buffer::Buffer,
        command::{Command, dispatch},
    };

    let long = "alpha beta gamma delta epsilon zeta eta theta iota kappa";
    let repository = Repository::new("wrapped", &format!("{long}\nkept\n"));
    repository.write("short\nkept\n");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    // Wrapping is off by default, and this is a test about wrapping.
    app.configure(obelus::config::Config {
        wrap: true,
        ..obelus::config::Config::default()
    });
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
    use obelus::{
        app::App,
        buffer::Buffer,
        command::{Command, dispatch},
    };

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
    use obelus::{
        app::App,
        buffer::Buffer,
        command::{Command, dispatch},
    };

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
    use obelus::{
        app::App,
        buffer::Buffer,
        command::{Command, dispatch},
    };

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
    use obelus::{
        app::App,
        buffer::Buffer,
        command::{Command, dispatch},
    };

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
    assert_eq!(
        screen(&mut app),
        top,
        "the top of the deletion is not where it stopped"
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
    // does through any page: it began nine rows down, on the line that
    // replaced the block, and it is nine rows down the block now. Which is
    // what makes the next arrow key carry on from where the reader is
    // looking rather than from the file below.
    let buffer = app.current_buffer().expect("a file");
    assert_eq!(
        buffer.in_block().map(|(line, _)| line.get()),
        Some(19),
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
/// "nothing changed here".
#[test]
fn the_caret_walks_the_lines_of_an_opened_hunk() {
    use obelus::{app::App, buffer::Buffer, command::Command};

    let repository = Repository::new("walk", "one\nold a\nold b\nold c\nold d\nold e\nlast\n");
    repository.write("one\nnew a\nnew b\nnew c\nnew d\nnew e\nlast\n");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    support::lay_out(&mut app, 40, 16);
    // Onto the first changed line, and open it: five removed lines above
    // five changed ones.
    support::press(&mut app, crossterm::event::KeyCode::Down);
    obelus::command::dispatch::dispatch(&mut app, Command::GitHunk);

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
    obelus::command::dispatch::dispatch(&mut app, Command::GitHunk);
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
    use obelus::{app::App, buffer::Buffer, command::Command};

    let tail: String = (1..=40).map(|line| format!("keep {line:02}\n")).collect();
    let repository = Repository::new(
        "scrolled",
        &format!("one\nold a\nold b\nold c\nold d\nold e\n{tail}"),
    );
    repository.write(&format!("one\nnew a\nnew b\nnew c\nnew d\nnew e\n{tail}"));

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    support::lay_out(&mut app, 40, 12);
    support::press(&mut app, crossterm::event::KeyCode::Down);
    obelus::command::dispatch::dispatch(&mut app, Command::GitHunk);
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
    use obelus::{app::App, buffer::Buffer, command::Command};

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
    obelus::command::dispatch::dispatch(&mut app, Command::GitHunk);
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
    obelus::command::dispatch::dispatch(&mut app, Command::GitHunk);
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
    obelus::command::dispatch::dispatch(&mut app, Command::GitHunk);
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
    use obelus::{app::App, buffer::Buffer, command::Command};

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
    support::press_alt_key(&mut app, KeyCode::Down);
    assert_eq!(line(&app), 5, "not the first change");
    support::press_alt_key(&mut app, KeyCode::Down);
    assert_eq!(line(&app), 20, "not the second change");
    support::press_alt_key(&mut app, KeyCode::Down);
    assert_eq!(line(&app), 50, "the middle change was counted twice");

    // And nothing below the last one: a wrap back to the top would look
    // like a key that did nothing while losing the reader's place. The key
    // does nothing at all, because the command is not offered here -- and
    // says nothing either, which is what every other key that cannot move
    // does.
    support::press_alt_key(&mut app, KeyCode::Down);
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
    support::press_alt_key(&mut app, KeyCode::Up);
    assert_eq!(line(&app), 20);
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Down);
    assert_eq!(line(&app), 22, "still inside the middle change");
    support::press_alt_key(&mut app, KeyCode::Up);
    assert_eq!(line(&app), 20, "not the top of the change being read");
    support::press_alt_key(&mut app, KeyCode::Up);
    assert_eq!(line(&app), 5);
    support::press_alt_key(&mut app, KeyCode::Up);
    assert_eq!(line(&app), 5, "it wrapped around");
    assert_eq!(app.note(), None, "a key that did nothing said so");

    // A leap, so the history brings the reader back where they were: to
    // where the last step started, and then to where the one before it did
    // -- including the line the reader had walked to by hand.
    obelus::command::dispatch::dispatch(&mut app, Command::GoBack);
    assert_eq!(line(&app), 20, "the step did not record where it left");
    obelus::command::dispatch::dispatch(&mut app, Command::GoBack);
    assert_eq!(line(&app), 22, "the step before that recorded nothing");

    // And a change the reader had to leap to arrives in the middle of the
    // screen, not against an edge: what a change means is the code around
    // it, and a hunk on the last row has half of that missing.
    obelus::command::dispatch::dispatch(&mut app, Command::GitNext);
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
    use obelus::{app::App, buffer::Buffer};

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

/// A list of files says which of them have been touched. A project's file
/// list is mostly files nobody has changed, and the few that have been are
/// what a reader is usually looking for.
#[test]
fn a_list_of_files_says_which_have_changed() {
    use obelus::git::{FileStatus, statuses};

    let repository = Repository::new("statuses", "one\n");
    repository.write("one\ntwo\n");
    std::fs::write(repository.directory.join("new.rs"), "fn new() {}\n").expect("a new file");

    let found = statuses(&repository.directory);
    assert_eq!(
        found.get(&repository.path()).copied(),
        Some(FileStatus::Changed),
        "a tracked file that differs: {found:?}"
    );
    assert_eq!(
        found.get(&repository.directory.join("new.rs")).copied(),
        Some(FileStatus::New),
        "a file git has never seen: {found:?}"
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
    use obelus::{app::App, buffer::Buffer, event::Event, git::Blamed};

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
    app.handle(Event::Blamed {
        path,
        lines: vec![
            Some(Blamed {
                who: "Ada".to_string(),
                when: long_ago,
            }),
            None,
        ],
    });
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
    use obelus::{app::App, buffer::Buffer, event::Event, git::Blamed};

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
            who: name.to_string(),
            when: 1,
        })
    };
    app.handle(Event::Blamed {
        path,
        lines: vec![who("Ada"), who("Bob"), who("Cai")],
    });

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
    use obelus::{app::App, buffer::Buffer, event::Event, git::Blamed};

    let long = format!("// {}\n", "x".repeat(60));
    let repository = Repository::new("long", &long);
    let buffer = Buffer::open(&repository.path()).expect("opening it");
    let path = buffer.path().to_path_buf();
    let mut app = App::new(vec![buffer]);
    support::lay_out(&mut app, 44, 8);
    app.handle(Event::Blamed {
        path,
        lines: vec![Some(Blamed {
            who: "Ada".to_string(),
            when: 1,
        })],
    });

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
    use obelus::{app::App, buffer::Buffer, config::Config, event::Event, git::Blamed};

    let repository = Repository::new("off", "short\n");
    let buffer = Buffer::open(&repository.path()).expect("opening it");
    let path = buffer.path().to_path_buf();
    let mut app = App::new(vec![buffer]);
    support::lay_out(&mut app, 44, 8);
    app.handle(Event::Blamed {
        path,
        lines: vec![Some(Blamed {
            who: "Ada".to_string(),
            when: 1,
        })],
    });
    assert!(support::text_block(&support::render(&mut app, 44, 8)).contains("Ada"));

    app.configure(Config {
        blame: false,
        ..Config::default()
    });
    let off = support::render(&mut app, 44, 8);
    assert!(
        !support::text_block(&off).contains("Ada"),
        "the names are still there:\n{off}"
    );

    // And back on without asking again: the answer is still in hand.
    app.configure(Config::default());
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

    let lines = obelus::git::blame::lines_of(&repository.path()).expect("a blame");
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
    assert!(obelus::git::blame::lines_of(&stranger).is_none());
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
    app.handle(Event::FileChanged {
        path: repository.directory.join(".git").join("HEAD"),
    });
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
    use obelus::{
        app::App,
        buffer::Buffer,
        command::{Command, dispatch},
    };

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
    use obelus::{
        app::App,
        buffer::Buffer,
        command::{Command, dispatch},
    };

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
    use obelus::git::history;

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
    use obelus::git::history;

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
    use obelus::git::history;

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
    use obelus::git::{FileStatus, history};

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
            (std::path::PathBuf::from("deep/new.rs"), FileStatus::New),
            (std::path::PathBuf::from("file.rs"), FileStatus::Changed),
        ],
        "not the files it changed, and only the files"
    );
}

/// A file as a commit had it, which is what choosing a row opens.
#[test]
fn a_file_can_be_read_as_a_commit_had_it() {
    use obelus::git::history;

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
    use obelus::git::history;

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
    use obelus::{app::App, buffer::Buffer};

    let repository = Repository::new("history-view", "one\n");
    repository.write("one\ntwo\n");
    repository.commit("the second");
    std::fs::write(repository.directory().join("other.rs"), "elsewhere\n").expect("the other");
    repository.commit_all("something else");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    support::lay_out(&mut app, 70, 16);

    support::press_function(&mut app, 9);
    let picker = app.picker().expect("the history");
    assert_eq!(picker.tabs(), ["this file", "the project"]);
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

    // The other tab is a walk away, and walking onto it asks its question.
    support::press(&mut app, KeyCode::Right);
    assert!(
        app.picker()
            .expect("the history")
            .matches()
            .any(|item| item.marker.is_some()),
        "the project's tab does not say its rows open"
    );
    let rows: Vec<String> = app
        .picker()
        .expect("the history")
        .matches()
        .map(|item| item.label.clone())
        .collect();
    assert_eq!(
        rows,
        ["something else", "the second", "committed"],
        "the other tab shows the same commits"
    );
}

/// A commit in the project's tab is not a file, so it has nothing to open.
/// What it has is the files it changed, and they go under it in place: one
/// list, one selection, one Escape.
#[test]
fn a_commit_opens_its_files_under_it() {
    use crossterm::event::KeyCode;
    use obelus::{app::App, buffer::Buffer};

    let repository = Repository::new("history-expand", "one\n");
    std::fs::write(repository.directory().join("other.rs"), "elsewhere\n").expect("the other");
    repository.write("one\ntwo\n");
    repository.commit_all("touching two");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    support::lay_out(&mut app, 70, 16);
    support::press_function(&mut app, 10);

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

/// Choosing one of a commit's files opens the file as that commit had it --
/// not the file on disk, which is a different document that happens to
/// share a name.
#[test]
fn a_file_of_a_commit_opens_as_that_commit_had_it() {
    use crossterm::event::KeyCode;
    use obelus::{app::App, buffer::Buffer};

    let repository = Repository::new("history-open", "first\n");
    repository.write("second\n");
    repository.commit("the second");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    support::lay_out(&mut app, 70, 16);
    support::press_function(&mut app, 10);
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
#[test]
fn a_subject_is_cut_at_its_end() {
    use obelus::{app::App, buffer::Buffer};

    let repository = Repository::new("history-cut", "one\n");
    repository.write("one\ntwo\n");
    repository.commit("Give the third bank of function keys to git, and move the reading");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    support::lay_out(&mut app, 50, 10);
    support::press_function(&mut app, 9);

    let dump = support::render(&mut app, 50, 10);
    let row = support::text_block(&dump)
        .lines()
        .find(|row| row.contains("Give the third"))
        .expect("the commit's row");
    assert!(
        !row.contains('\u{2026}'),
        "the subject was cut at its head:\n{row}"
    );
    assert!(
        !row.contains("and move the reading"),
        "the row is wide enough for the whole subject, so this proves nothing:\n{row}"
    );
}

/// A commit's version of a file carries the commit's message above its
/// first line, and the reader lands in it: they opened this to find out
/// *why* it says what it says, and the file itself is a page down.
#[test]
fn a_commits_version_carries_what_the_commit_said() {
    use crossterm::event::KeyCode;
    use obelus::{app::App, buffer::Buffer};

    let repository = Repository::new("history-said", "first\n");
    repository.write("second\n");
    repository.commit("A subject worth reading\n\nAnd a body under it.\n");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 10);
    support::press(&mut app, KeyCode::Enter);
    support::press(&mut app, KeyCode::Down);
    support::press(&mut app, KeyCode::Enter);

    let dump = support::render(&mut app, 60, 16);
    let text = support::text_block(&dump);
    assert!(
        text.contains("A subject worth reading"),
        "the message is not above the file:\n{text}"
    );
    assert!(
        text.contains("And a body under it."),
        "only the subject came up:\n{text}"
    );
    assert!(
        text.contains("second"),
        "the file itself is not there:\n{text}"
    );
    // The caret is in the message, which has no line numbers of its own,
    // so the status row says where it is and marks that it is not the
    // file's own count.
    let status = support::text_block(&dump)
        .lines()
        .last()
        .expect("a status row")
        .to_string();
    assert!(
        status.contains("-1:1"),
        "the reader did not land in the message:\n{dump}"
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
    use obelus::{app::App, buffer::Buffer};

    let repository = Repository::new("history-apart", "first\n");
    repository.write("second\n");
    repository.commit("the second");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 10);
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
    app.handle(obelus::event::Event::FileChanged {
        path: repository.path(),
    });
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
    use obelus::app::App;

    let repository = Repository::new("history-reopen", "first\n");
    repository.write("second\n");
    repository.commit("the second");

    // Nothing open, so the commit's version is the only buffer there is
    // wearing that name -- otherwise the list finds the right answer for
    // the wrong reason.
    let mut app = App::new(Vec::new());
    app.working_directory_for_test(repository.directory());
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 10);
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
    use obelus::{app::App, coordinates::LineNumber, git::Marker};

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
        obelus::buffer::Buffer::open(&repository.path()).expect("opening it"),
    ]);
    app.working_directory_for_test(repository.directory());
    support::lay_out(&mut app, 60, 16);
    let _ = support::render(&mut app, 60, 16);

    support::press_function(&mut app, 10);
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
    app.handle(obelus::event::Event::Blamed {
        path: repository.path(),
        lines: vec![
            Some(obelus::git::Blamed {
                who: "somebody".to_string(),
                when: 0,
            });
            3
        ],
    });
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
    use obelus::{app::App, buffer::Buffer, git::history};

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
    support::lay_out(&mut app, 60, 24);
    support::press_function(&mut app, 10);

    /// The preview's own rows: a screen with a list on it has a rule under
    /// the tabs, one between the list and the preview, and one above the
    /// status row, so the preview is what lies between the last two.
    fn previewed(dump: &str) -> String {
        let rows: Vec<&str> = support::text_block(dump).lines().collect();
        let rules: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.contains('\u{2500}'))
            .map(|(at, _)| at)
            .collect();
        let (from, to) = (rules[rules.len() - 2] + 1, rules[rules.len() - 1]);
        rows[from..to].join("\n")
    }

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
    // of its own to go to.
    assert!(
        text.lines()
            .next()
            .expect("a first row")
            .contains('\u{2590}'),
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
    use obelus::{app::App, buffer::Buffer};

    let repository = Repository::new("open-files-commit", "first\n");
    repository.write("second\n");
    repository.commit("the second");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    support::lay_out(&mut app, 60, 14);
    support::press_function(&mut app, 10);
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
            .args(["init", "--bare", "--quiet"])
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
        git(&work, &["init", "--quiet"]);
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
    use obelus::{app::App, buffer::Buffer, git::FileStatus};

    let repository = Pushed::new("marked", 2);
    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.work.clone());
    support::lay_out(&mut app, 64, 14);
    support::press_function(&mut app, 10);

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
    use obelus::{app::App, buffer::Buffer};

    let repository = Repository::new("nowhere-to-push", "one\n");
    repository.write("one\ntwo\n");
    repository.commit("the second");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    support::lay_out(&mut app, 64, 14);
    support::press_function(&mut app, 10);
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
    use obelus::{app::App, buffer::Buffer};

    let repository = Repository::new("history-file-tab", "first\n");
    repository.write("second\n");
    repository.commit("the second");
    repository.write("third\n");
    repository.commit("the third");

    let mut app = App::new(vec![Buffer::open(&repository.path()).expect("opening it")]);
    app.working_directory_for_test(repository.directory());
    support::lay_out(&mut app, 60, 16);
    support::press_function(&mut app, 9);
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
