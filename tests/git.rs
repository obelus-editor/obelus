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

    // The palette offers it, and it opens.
    support::press_control(&mut app, 'p');
    assert!(
        app.picker()
            .expect("the palette")
            .matches()
            .any(|item| item.label == "git.hunk"),
        "not offered on an added line"
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
    // like a key that did nothing while losing the reader's place.
    support::press_alt_key(&mut app, KeyCode::Down);
    assert_eq!(line(&app), 50, "it wrapped around");
    assert_eq!(app.note(), Some("no change below here"));
    // Which is also why the palette does not offer it here.
    support::press_control(&mut app, 'p');
    let offered: Vec<String> = app
        .picker()
        .expect("the palette")
        .matches()
        .map(|item| item.label.clone())
        .collect();
    support::press(&mut app, KeyCode::Esc);
    assert!(
        offered.iter().any(|label| label == "git.previous"),
        "not offered with changes above: {offered:?}"
    );
    assert!(
        !offered.iter().any(|label| label == "git.next"),
        "offered with nothing below: {offered:?}"
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
    assert_eq!(app.note(), Some("no change above here"));

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
            .take(11)
            .all(|row| matches!(cell(row, 2), '\u{2502}' | '\u{2588}')),
        "the bar is not the column before last:\n{dump}"
    );
    assert!(
        rows.iter().take(11).any(|row| cell(row, 2) == '\u{2588}'),
        "the thumb is not on the bar:\n{dump}"
    );

    // The change is fifty lines down a sixty-line file, so it belongs nine
    // rows down an eleven-row column -- and nowhere else, on a screen where
    // nothing visible has changed at all.
    let marked: Vec<usize> = (0..11)
        .filter(|row| cell(&rows[*row], 1) == '\u{2590}')
        .collect();
    assert_eq!(
        marked,
        vec![9],
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
    let marked: Vec<usize> = (0..11)
        .filter(|row| cell(&rows[*row], 1) == '\u{2590}')
        .collect();
    assert_eq!(
        marked,
        vec![3, 4, 5, 6, 7],
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
