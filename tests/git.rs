//! What changed since the last commit.
//!
//! Values in, hunks out: the three cases a reader sees in the margin are
//! decided here, and each of them is a different claim about the file. A
//! full-height bar beside a line says "this line is different"; a mark at a
//! seam says "lines are missing here". Getting the two confused is a margin
//! that lies about which lines the reader is looking at.

mod support;

use obelus::{
    coordinates::LineNumber,
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
        app.opened_hunk().is_none(),
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

/// The changes in the whole file, in a column of its own right of the bar.
///
/// One column, the same width as the margin on the far side and drawn with
/// the same glyph, so the right edge is a thin bar with a thin stroke beside
/// it rather than one wide block with no obvious position in it. The rows are
/// lines of the file, not rows of the screen: the margin says what changed
/// here, this says where else to look.
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
    // The map is the last column and the bar the one before it.
    let cell = |row: &Vec<char>, back: usize| row.get(row.len() - back).copied().unwrap_or(' ');

    assert!(
        rows.iter()
            .take(10)
            .all(|row| matches!(cell(row, 2), '\u{2502}' | '\u{2588}')),
        "the bar is not the column before last:\n{dump}"
    );
    assert!(
        rows.iter().take(10).any(|row| cell(row, 2) == '\u{2588}'),
        "the thumb is not on the bar:\n{dump}"
    );

    // The change is fifty lines down a sixty-line file, so it belongs eight
    // rows down a ten-row column -- and nowhere else, on a screen where
    // nothing visible has changed at all.
    let marked: Vec<usize> = (0..10)
        .filter(|row| cell(&rows[*row], 1) == '\u{2590}')
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
        .filter(|row| cell(&rows[*row], 1) == '\u{2590}')
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
