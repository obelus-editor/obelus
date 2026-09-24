//! Re-reading a file that changed underneath us.
//!
//! The case this exists for is an agent rewriting the file while it is open,
//! which is the normal case rather than the exceptional one.

mod support;

use std::{fs, path::PathBuf};

use obelus_buffer::{Buffer, TextArea};
use obelus_text::coordinates::{CharColumn, LineNumber};

/// A file only this test touches, named after the test so a failure leaves
/// evidence that says which one.
struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(name: &str, contents: &str) -> Self {
        let path = std::env::temp_dir().join(format!("obelus-{}-{name}.rs", std::process::id()));
        fs::write(&path, contents).expect("writing the scratch file");
        Self { path }
    }

    fn write(&self, contents: &str) {
        fs::write(&self.path, contents).expect("rewriting the scratch file");
    }

    /// Rewrites the way an editor does: a new file, then a rename over the old
    /// one. The inode changes, which is what breaks a watch placed on the file
    /// rather than on its directory.
    fn replace_by_rename(&self, contents: &str) {
        let temporary = self.path.with_extension("tmp");
        fs::write(&temporary, contents).expect("writing the replacement");
        fs::rename(&temporary, &self.path).expect("renaming over the original");
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Room enough that nothing in these files wraps, so the cursor moves by
/// document line and the assertions say what they mean.
const AREA: TextArea = TextArea {
    width: 80,
    height: 10,
    wrap: true,
};

const BEFORE: &str = "fn main() {\n    let a = 1;\n    let b = 2;\n}\n";

#[test]
fn a_reload_picks_up_the_new_contents() {
    let scratch = Scratch::new("contents", BEFORE);
    let mut buffer = Buffer::open(&scratch.path).expect("opening");

    scratch.write("fn main() {\n    let a = 100;\n    let b = 2;\n}\n");
    assert!(buffer.reload().expect("reloading"));
    assert_eq!(buffer.text().line(LineNumber::new(1)), "    let a = 100;");
}

/// A watcher fires for `touch`, for a rename, and for a permission change,
/// none of which alter a byte. Comparing first costs one pass over the rope
/// and saves a reparse, a cursor clamp and a redraw.
#[test]
fn rewriting_the_same_bytes_is_not_a_change() {
    let scratch = Scratch::new("identical", BEFORE);
    let mut buffer = Buffer::open(&scratch.path).expect("opening");

    scratch.write(BEFORE);
    assert!(!buffer.reload().expect("reloading"));

    scratch.replace_by_rename(BEFORE);
    assert!(!buffer.reload().expect("reloading"));
}

/// Reloading through a rename has to work as often as reloading through a
/// write, because that is how editors save.
#[test]
fn a_file_replaced_by_rename_still_reloads() {
    let scratch = Scratch::new("rename", BEFORE);
    let mut buffer = Buffer::open(&scratch.path).expect("opening");

    scratch.replace_by_rename("fn main() {}\n");
    assert!(buffer.reload().expect("reloading"));
    assert_eq!(buffer.text().line(LineNumber::new(0)), "fn main() {}");
}

/// An agent that rewrites a file should not send the reader back to line one.
#[test]
fn a_reload_keeps_the_cursor_where_it_was() {
    let scratch = Scratch::new("cursor", BEFORE);
    let mut buffer = Buffer::open(&scratch.path).expect("opening");
    for _ in 0..2 {
        buffer.move_cursor(obelus_buffer::Motion::Down, AREA);
    }
    for _ in 0..8 {
        buffer.move_cursor(obelus_buffer::Motion::Right, AREA);
    }
    let before = buffer.cursor();
    assert_eq!(before.line, LineNumber::new(2));
    assert_eq!(before.column, CharColumn::new(8));

    scratch.write("fn main() {\n    let a = 1;\n    let b = 22222;\n}\n");
    assert!(buffer.reload().expect("reloading"));

    let after = buffer.cursor();
    assert_eq!(after.line, before.line);
    assert_eq!(after.column, before.column);
}

/// The document can shrink out from under a cursor that was already past the
/// new end. Clamping is the difference between a wrong position and a panic.
#[test]
fn a_reload_clamps_a_cursor_that_no_longer_fits() {
    let scratch = Scratch::new("shrink", BEFORE);
    let mut buffer = Buffer::open(&scratch.path).expect("opening");
    for _ in 0..3 {
        buffer.move_cursor(obelus_buffer::Motion::Down, AREA);
    }
    for _ in 0..20 {
        buffer.move_cursor(obelus_buffer::Motion::Right, AREA);
    }

    scratch.write("fn f() {}\n");
    assert!(buffer.reload().expect("reloading"));

    let cursor = buffer.cursor();
    assert!(cursor.line <= buffer.text().last_line());
    assert!(cursor.column <= buffer.text().line_length(cursor.line));
}

/// A deleted file leaves the buffer showing what it last held. Losing the
/// contents would be worse than showing something a moment out of date.
#[test]
fn a_deleted_file_reports_an_error_and_keeps_the_text() {
    let scratch = Scratch::new("deleted", BEFORE);
    let mut buffer = Buffer::open(&scratch.path).expect("opening");
    fs::remove_file(&scratch.path).expect("deleting");

    assert!(buffer.reload().is_err());
    assert_eq!(buffer.text().line(LineNumber::new(0)), "fn main() {");
}

/// Highlighting has to survive a reload, which is the whole reason the
/// reparse is there.
///
/// The byte checked has to be one where the old tree and the new one disagree.
/// Checking byte zero does not work: the old contents start with a keyword and
/// so do the new ones, so skipping the reparse entirely passes — the stale
/// tree gives the right answer by coincidence. A quote that lands where the old
/// tree had a keyword does not have that problem.
#[test]
fn highlighting_still_works_after_a_reload() {
    use obelus_syntax::highlight::Highlights;
    use obelus_text::{coordinates::ByteOffset, kind::SyntaxKind};

    const AFTER: &str = "const S: &str = \"hello\";\nfn main() {}\n";

    let scratch = Scratch::new("highlight", BEFORE);
    let mut buffer = Buffer::open(&scratch.path).expect("opening");

    let quote = AFTER.find('"').expect("a quote in the new contents");
    // In the old contents that byte is inside `let`, so a stale tree calls it
    // a keyword.
    assert_eq!(&BEFORE[quote..quote + 3], "let");

    scratch.write(AFTER);
    assert!(buffer.reload().expect("reloading"));

    let state = buffer.syntax().expect("a parse");
    let mut highlights = Highlights::default();
    highlights.refresh(
        state,
        buffer.text(),
        ByteOffset::new(0)..buffer.text().byte_length(),
    );

    assert_eq!(
        highlights.kind_at(ByteOffset::new(quote)),
        Some(SyntaxKind::String),
        "the tree is stale: byte {quote} is a quote now and was a keyword before"
    );
}

/// The buffer keeps its contents when the file can no longer be read, so the
/// only way the reader learns about it is the status bar.
///
/// Unreadable rather than deleted: a file that is gone is a fact Obelus can
/// state, and it says "Deleted". `stale` is for the case it cannot -- here,
/// bytes that are not text.
#[test]
fn a_failed_reload_marks_the_buffer_stale() {
    let scratch = Scratch::new("stale-flag", BEFORE);
    let mut buffer = Buffer::open(&scratch.path).expect("opening");
    assert!(!buffer.is_stale(), "a freshly opened file is not stale");

    fs::write(&scratch.path, [0xff, 0xfe, 0xfd]).expect("writing bytes that are not text");
    assert!(buffer.reload().is_err());
    assert!(buffer.is_stale());

    // And it stops being stale the moment the file can be read again, which is
    // what happens when a tool saves by removing and recreating.
    scratch.write("fn back() {}\n");
    assert!(buffer.reload().expect("reloading"));
    assert!(!buffer.is_stale());
    assert_eq!(buffer.text().line(LineNumber::new(0)), "fn back() {}");
}

/// A reload that finds no change still means the file was readable.
#[test]
fn an_unchanged_reload_clears_staleness() {
    let scratch = Scratch::new("stale-cleared", BEFORE);
    let mut buffer = Buffer::open(&scratch.path).expect("opening");

    fs::write(&scratch.path, [0xff, 0xfe, 0xfd]).expect("writing bytes that are not text");
    assert!(buffer.reload().is_err());
    assert!(buffer.is_stale());

    scratch.write(BEFORE);
    assert!(!buffer.reload().expect("reloading"), "same bytes as before");
    assert!(!buffer.is_stale());
}

/// A file that is gone says so in its own word: a reader told their file is
/// "stale" when it has been deleted will go looking for it.
#[test]
fn a_deleted_file_says_it_was_deleted() {
    let scratch = Scratch::new("deleted-word", BEFORE);
    let mut buffer = Buffer::open(&scratch.path).expect("opening");
    fs::remove_file(&scratch.path).expect("deleting");
    assert!(buffer.reload().is_err());

    assert_eq!(buffer.on_disk(), obelus_buffer::Disk::Deleted);
    assert!(
        !buffer.is_stale(),
        "a file Obelus knows the fate of was called stale"
    );

    let mut app = obelus_app::app::App::new(vec![buffer]);
    let dump = support::render(&mut app, 60, 5);
    assert!(
        support::text_block(&dump).contains("Deleted"),
        "the status row does not say the file was deleted:\n{dump}"
    );
}

/// And it stops saying so when the file comes back with what it had.
#[test]
fn a_file_that_comes_back_unchanged_is_not_deleted_any_more() {
    let scratch = Scratch::new("deleted-back", BEFORE);
    let mut buffer = Buffer::open(&scratch.path).expect("opening");
    fs::remove_file(&scratch.path).expect("deleting");
    assert!(buffer.reload().is_err());
    assert_eq!(buffer.on_disk(), obelus_buffer::Disk::Deleted);

    scratch.write(BEFORE);
    assert!(!buffer.reload().expect("reloading"), "same bytes as before");
    assert_eq!(
        buffer.on_disk(),
        obelus_buffer::Disk::Unchanged,
        "a file that came back is still called deleted"
    );
}

/// A file whose path is a temporary one cannot be pinned by a fixture — the
/// process id is in it — so these assert on what has to be there rather than
/// on the whole screen.
fn stale_app(name: &str) -> (Scratch, obelus_app::app::App) {
    // Named per caller: the tests run in parallel, and two of them sharing a
    // scratch file means one recreates the file the other just deleted.
    let scratch = Scratch::new(name, BEFORE);
    let mut buffer = Buffer::open(&scratch.path).expect("opening");
    fs::write(&scratch.path, [0xff, 0xfe, 0xfd]).expect("writing bytes that are not text");
    assert!(buffer.reload().is_err());
    assert!(buffer.is_stale());
    (scratch, obelus_app::app::App::new(vec![buffer]))
}

#[test]
fn the_status_bar_says_when_a_file_can_no_longer_be_read() {
    let (_scratch, mut app) = stale_app("stale-wide");
    let dump = support::render(&mut app, 60, 5);

    assert!(
        support::text_block(&dump).contains("stale"),
        "no marker on screen:\n{dump}"
    );
    assert!(
        support::legend_block(&dump).contains("#f87171"),
        "the marker is not in its own colour:\n{dump}"
    );
}

#[test]
fn a_readable_file_gets_no_marker() {
    let mut app = obelus_app::app::App::new(vec![support::open_fixture("sample.rs")]);
    let dump = support::render(&mut app, 60, 5);
    assert!(!support::text_block(&dump).contains("stale"));
}

/// The marker takes its width out of the path's budget, not out of the cursor
/// position's.
#[test]
fn the_marker_survives_a_narrow_screen_by_shortening_the_path() {
    let (_scratch, mut app) = stale_app("stale-narrow");
    let dump = support::render(&mut app, 30, 5);
    let text = support::text_block(&dump);

    assert!(text.contains("stale"), "the marker was dropped:\n{dump}");
    assert!(text.contains("1:1"), "the position was dropped:\n{dump}");
    assert!(
        text.contains('\u{2026}'),
        "the path was not truncated:\n{dump}"
    );
}

/// Narrower than both will fit. The position is there every frame; half a word
/// of warning is worse than none.
#[test]
fn on_a_screen_too_narrow_for_both_the_position_wins() {
    let (_scratch, mut app) = stale_app("stale-tiny");
    let dump = support::render(&mut app, 12, 5);
    let text = support::text_block(&dump);

    assert!(text.contains("1:1"), "the position was dropped:\n{dump}");
    assert!(!text.contains("[stale"), "half a marker was drawn:\n{dump}");
}
