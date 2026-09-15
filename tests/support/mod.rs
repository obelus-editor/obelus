#![allow(
    dead_code,
    reason = "shared by several test binaries, each of which uses part of it"
)]

//! Golden-file support for the rendering tests.
//!
//! What is asserted on is the cell grid: every cell's symbol, foreground and
//! background. Only the symbols would leave highlighting, theming and the
//! status bar's band untested — a widget can write every character correctly
//! and paint none of them, and an assertion on the text cannot tell.
//!
//! The dump also records where the terminal was told to put its cursor. The
//! cursor is the terminal's, not a painted cell, so it is invisible to an
//! assertion on the grid — and it is the terminal drawing it that makes it go
//! hollow when the window loses focus.
//!
//! Regenerate with `UPDATE_FIXTURES=1 cargo test`.
//!
//! In the text block, a cell holding an empty symbol prints as nothing. That
//! is how the second half of a wide glyph is stored, so the glyph before it
//! occupies the two columns the pair really covers and the block stays
//! aligned.

use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use obelus::{app::App, buffer::Buffer, event::Event, ui};
use ratatui::{buffer::Buffer as CellBuffer, layout::Rect, style::Color};

/// Lays the screen out without keeping the result.
///
/// The loop draws before it waits for a key, so by the time any key arrives
/// the geometry is known. A test that presses first would be asking the
/// application to move a cursor through a zero-sized screen — which, with
/// wrapping, is a screen one cell wide.
pub fn lay_out(app: &mut App, width: u16, height: u16) {
    let _ = render(app, width, height);
}

/// Sends a key with no modifiers through the real handler.
///
/// Driving the tests through key handling rather than a test-only mover means
/// the fixtures also pin down which keys move the cursor.
pub fn press(app: &mut App, code: KeyCode) {
    app.handle(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

/// Sends a key with control held.
pub fn press_control(app: &mut App, character: char) {
    app.handle(Event::Key(KeyEvent::new(
        KeyCode::Char(character),
        KeyModifiers::CONTROL,
    )));
}

/// Sends a key with shift held.
pub fn press_shift(app: &mut App, code: KeyCode) {
    app.handle(Event::Key(KeyEvent::new(code, KeyModifiers::SHIFT)));
}

/// Sends a key with control held.
pub fn press_control_key(app: &mut App, code: KeyCode) {
    app.handle(Event::Key(KeyEvent::new(code, KeyModifiers::CONTROL)));
}

/// Sends a function key, which is how the most used commands are reached.
pub fn press_function(app: &mut App, number: u8) {
    app.handle(Event::Key(KeyEvent::new(
        KeyCode::F(number),
        KeyModifiers::NONE,
    )));
}

/// Sends a key with alt held.
pub fn press_alt_key(app: &mut App, code: KeyCode) {
    app.handle(Event::Key(KeyEvent::new(code, KeyModifiers::ALT)));
}

/// Types a string into whatever is listening.
pub fn type_text(app: &mut App, text: &str) {
    for character in text.chars() {
        app.handle(Event::Key(KeyEvent::new(
            KeyCode::Char(character),
            KeyModifiers::NONE,
        )));
    }
}

/// Reads one of the sample source files.
pub fn open_fixture(name: &str) -> Buffer {
    let path = fixtures().join(name);
    Buffer::open(&path).expect("opening a fixture")
}

/// Draws an application at the given size and dumps the cells.
pub fn render(app: &mut App, width: u16, height: u16) -> String {
    let area = Rect::new(0, 0, width, height);
    let mut cells = CellBuffer::empty(area);
    let cursor = app.draw_into(&mut cells, area);
    let mut out = dump(&cells, width, height);
    out.push_str(&match cursor {
        Some(position) => format!("-- cursor --\n{},{}\n", position.x, position.y),
        None => "-- cursor --\nnone\n".to_string(),
    });
    out
}

/// Where the terminal was told to put its cursor, as the dump records it.
pub fn cursor_line(dump: &str) -> &str {
    let marker = "-- cursor --\n";
    dump.find(marker)
        .map_or("", |index| dump[index + marker.len()..].trim_end())
}

/// The text rows of a dump.
pub fn text_block(dump: &str) -> &str {
    section(dump, "-- text --", "-- style --")
}

/// The style rows of a dump.
pub fn style_block(dump: &str) -> &str {
    section(dump, "-- style --", "-- legend --")
}

/// The legend of a dump.
pub fn legend_block(dump: &str) -> &str {
    section(dump, "-- legend --", "-- cursor --")
}

fn section<'a>(dump: &'a str, from: &str, to: &str) -> &'a str {
    let start = dump.find(from).map_or(0, |index| index + from.len());
    let end = dump[start..]
        .find(to)
        .map_or(dump.len(), |index| start + index);
    &dump[start..end]
}

/// Compares a dump against its fixture.
pub fn check(name: &str, actual: &str) {
    let path = fixtures().join(format!("{name}.txt"));

    if std::env::var_os("UPDATE_FIXTURES").is_some() {
        fs::write(&path, actual).expect("writing the fixture");
        return;
    }

    let Ok(expected) = fs::read_to_string(&path) else {
        panic!(
            "fixture {name} does not exist yet.\n\
             Review this output, then create it with UPDATE_FIXTURES=1:\n\n{actual}"
        );
    };

    assert!(
        expected == actual,
        "{name} does not match its fixture.\n\
         If the change is intended: UPDATE_FIXTURES=1 cargo test\n\n\
         --- fixture ---\n{expected}\n--- rendered ---\n{actual}"
    );
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// One cell grid, as text.
fn dump(cells: &CellBuffer, width: u16, height: u16) -> String {
    let regions = ui::regions(ui::area_of(ratatui::layout::Size { width, height }));

    let mut out = String::new();
    let _ = writeln!(
        out,
        "screen {width}x{height}  editor {}x{}  status {}x{}",
        regions.editor.width, regions.editor.height, regions.status.width, regions.status.height
    );

    // One letter per distinct style, assigned in the order first seen so the
    // legend reads top-to-bottom like the screen does.
    let mut styles: Vec<(Color, Color)> = Vec::new();

    out.push_str("-- text --\n");
    for y in 0..height {
        let _ = write!(out, "{y:2}|");
        for x in 0..width {
            let Some(cell) = cells.cell((x, y)) else {
                continue;
            };
            out.push_str(cell.symbol());
        }
        out.push('\n');
    }

    out.push_str("-- style --\n");
    for y in 0..height {
        let _ = write!(out, "{y:2}|");
        for x in 0..width {
            let Some(cell) = cells.cell((x, y)) else {
                continue;
            };
            let key = (cell.fg, cell.bg);
            let index = styles
                .iter()
                .position(|entry| *entry == key)
                .unwrap_or_else(|| {
                    styles.push(key);
                    styles.len() - 1
                });
            out.push(label(index));
        }
        out.push('\n');
    }

    out.push_str("-- legend --\n");
    for (index, (foreground, background)) in styles.iter().enumerate() {
        let _ = writeln!(
            out,
            "{} fg={} bg={}",
            label(index),
            colour(*foreground),
            colour(*background)
        );
    }

    out
}

/// A stable one-character name for a style.
fn label(index: usize) -> char {
    const LABELS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    char::from(*LABELS.get(index).unwrap_or(&b'?'))
}

fn colour(colour: Color) -> String {
    match colour {
        Color::Rgb(red, green, blue) => format!("#{red:02x}{green:02x}{blue:02x}"),
        Color::Reset => "reset".to_string(),
        other => format!("{other:?}"),
    }
}

/// A directory of its own for one test, cleaned up when the test passes.
///
/// Named after the test, so what a failure leaves behind says which one it
/// was -- and it is left behind only then. A passing test that kept its
/// scratch directory leaves one per run, and `/tmp` is a ramdisk on the
/// machines this is written on: a suite run a few hundred times over an
/// afternoon was holding most of a gigabyte of nothing.
///
/// The process id is in the name so that two runs at once do not clear each
/// other's ground, which is a failure that looks like a flaky test.
pub struct Scratch {
    path: PathBuf,
    name: String,
}

impl Scratch {
    /// An empty directory, whatever was there before.
    pub fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("obelus-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a scratch directory");
        Self {
            path,
            name: name.to_string(),
        }
    }

    /// The directory itself.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A path inside it, which need not exist.
    pub fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }

    /// Writes a file inside it, making any directories on the way.
    pub fn write(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("a directory");
        }
        std::fs::write(&path, contents).expect("writing a scratch file");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        // A test that is failing is a test whose evidence is worth more
        // than the disk it sits on.
        if std::thread::panicking() {
            eprintln!("left {} behind, for {}", self.path.display(), self.name);
            return;
        }
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// The preview's own rows, out of a rendered screen.
///
/// A screen with a list on it has a rule under the tabs, one between the
/// list and the preview, and one above the status row, so the preview is
/// what lies between the last two.
#[allow(dead_code)]
pub fn previewed(dump: &str) -> String {
    let rows: Vec<&str> = text_block(dump).lines().collect();
    let rules: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.contains('\u{2500}'))
        .map(|(at, _)| at)
        .collect();
    let (from, to) = (rules[rules.len() - 2] + 1, rules[rules.len() - 1]);
    rows[from..to].join("\n")
}

/// Gives the app a channel, so the walks it starts have somewhere to answer.
///
/// The loop does this at startup; a test drives the app by hand and would
/// otherwise start threads that can never report back.
#[allow(dead_code)]
pub fn drive(app: &mut App) -> std::sync::mpsc::Receiver<obelus::event::Event> {
    let (sender, events) = obelus::event::channel();
    app.events_for_test(sender);
    events
}

/// Runs the history walk the app has started through to its last batch.
///
/// A history arrives in batches while the reader reads, which is the point
/// of it; a test wants the finished list, so it waits for the batch that
/// says there are no more.
#[allow(dead_code)]
pub fn read_history(app: &mut App, events: &std::sync::mpsc::Receiver<obelus::event::Event>) {
    while let Ok(event) = events.recv_timeout(std::time::Duration::from_secs(20)) {
        app.handle(event);
        // The app's own answer, not the batch's: a walk the reader has
        // moved off still sends its last batch, and that batch being
        // dropped is the point. Waiting on the list saying it has stopped
        // filling waits for the walk anybody is actually waiting for.
        if app
            .picker()
            .is_none_or(|picker| picker.is_filling().is_none())
        {
            return;
        }
    }
    panic!("the history never finished arriving");
}

/// The ways out of the question obelus is asking, in the order offered.
///
/// Panics if it is not asking one: a test that walks past a question it did
/// not expect would go on to assert about a screen nobody is looking at.
pub fn ways(app: &App) -> Vec<String> {
    app.picker()
        .expect("a question is being asked")
        .matches()
        .map(|item| item.label.clone())
        .collect()
}

/// Answers the question obelus is asking, by walking to a way out and
/// choosing it.
///
/// Through the arrow keys and enter rather than by reaching for the value,
/// so that what a test answers is what a reader could answer.
pub fn answer(app: &mut App, way: &str) {
    let ways = ways(app);
    let at = ways
        .iter()
        .position(|label| label == way)
        .unwrap_or_else(|| panic!("no way out called {way:?} among {ways:?}"));
    for _ in 0..at {
        press(app, KeyCode::Down);
    }
    press(app, KeyCode::Enter);
}

/// The text block as one line, with runs of blanks collapsed.
///
/// For asserting on a sentence the screen wrapped. A phrase broken across
/// two rows is still the phrase the reader read, and a test that missed it
/// would be testing the width of the screen.
pub fn said(dump: &str) -> String {
    text_block(dump)
        .lines()
        // Past the row number the dump puts in front of every row, which
        // would otherwise land in the middle of a wrapped sentence.
        .filter_map(|line| line.split_once('|'))
        .flat_map(|(_, row)| row.split_whitespace())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The turn to use the clipboard.
///
/// What obelus keeps when no provider can hold a copy is one thing for the
/// whole process, so two tests reading it at once each see what the other
/// put there. Held for as long as the returned guard lives, which is the
/// body of the test that took it.
pub fn clipboard_turn() -> std::sync::MutexGuard<'static, ()> {
    static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
    // A test that failed while holding it poisoned nothing: there is no
    // state behind this, only the taking of turns.
    TURN.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
