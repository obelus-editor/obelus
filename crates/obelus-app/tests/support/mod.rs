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
use obelus_app::{app::App, event::Event};
use obelus_buffer::Buffer;
use ratatui::{buffer::Buffer as CellBuffer, layout::Rect, style::Color};

/// The URI a language server would name a file by.
///
/// `lsp::client::uri_for`, never a `format!` beside the test. A path is not a
/// URI with a scheme in front of it: a Windows path begins at a drive letter
/// where a URI's path begins with `/`, and writes `\` between its parts where
/// a URI writes `/`. A test that builds its own agrees with itself and with
/// nothing else -- which is how a dozen of these passed for as long as Obelus
/// was only ever run on one platform.
pub(crate) fn uri_for(path: impl AsRef<Path>) -> String {
    obelus_lsp::client::uri_for(path.as_ref())
        .expect("a uri")
        .as_str()
        .to_string()
}

/// A made-up absolute path, written the unix way and read the local way.
///
/// `/nowhere.rs` is an absolute path on one platform and an ordinary relative
/// name on another. `C:\nowhere.rs` is the same fiction told locally.
pub(crate) fn fake_path(unixish: &str) -> PathBuf {
    match cfg!(windows) {
        true => PathBuf::from(format!("C:{}", unixish.replace('/', "\\"))),
        false => PathBuf::from(unixish),
    }
}

/// And the URI a server would name one by.
pub(crate) fn fake_uri(unixish: &str) -> String {
    uri_for(fake_path(unixish))
}

/// Lays the screen out without keeping the result.
///
/// The loop draws before it waits for a key, so by the time any key arrives
/// the geometry is known. A test that presses first would be asking the
/// application to move a cursor through a zero-sized screen — which, with
/// wrapping, is a screen one cell wide.
pub(crate) fn lay_out(app: &mut App, width: u16, height: u16) {
    let _ = render(app, width, height);
}

/// Sends a key with no modifiers through the real handler.
///
/// Driving the tests through key handling rather than a test-only mover means
/// the fixtures also pin down which keys move the cursor.
pub(crate) fn press(app: &mut App, code: KeyCode) {
    app.handle(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

/// Sends a key with control held.
pub(crate) fn press_control(app: &mut App, character: char) {
    app.handle(Event::Key(KeyEvent::new(
        KeyCode::Char(character),
        KeyModifiers::CONTROL,
    )));
}

/// Sends a key with shift held.
pub(crate) fn press_shift(app: &mut App, code: KeyCode) {
    app.handle(Event::Key(KeyEvent::new(code, KeyModifiers::SHIFT)));
}

/// Sends a key with control held.
pub(crate) fn press_control_key(app: &mut App, code: KeyCode) {
    app.handle(Event::Key(KeyEvent::new(code, KeyModifiers::CONTROL)));
}

/// Sends a function key, which is how the most used commands are reached.
pub(crate) fn press_function(app: &mut App, number: u8) {
    app.handle(Event::Key(KeyEvent::new(
        KeyCode::F(number),
        KeyModifiers::NONE,
    )));
}

/// Sends a key with alt held.
pub(crate) fn press_alt_key(app: &mut App, code: KeyCode) {
    app.handle(Event::Key(KeyEvent::new(code, KeyModifiers::ALT)));
}

/// Types a string into whatever is listening.
pub(crate) fn type_text(app: &mut App, text: &str) {
    for character in text.chars() {
        app.handle(Event::Key(KeyEvent::new(
            KeyCode::Char(character),
            KeyModifiers::NONE,
        )));
    }
}

/// Draws a frame and hands back the cells themselves.
///
/// The dump names a style by its two colours, which is what almost every
/// test is about; a modifier -- bold, italic, an underline -- is not in it
/// and cannot be without renaming every style in every golden file. A test
/// about one asks the cells.
#[must_use]
pub(crate) fn cells_of(app: &mut App, width: u16, height: u16) -> CellBuffer {
    let area = Rect::new(0, 0, width, height);
    let mut cells = CellBuffer::empty(area);
    app.draw_into(&mut cells, area);
    cells
}

/// Reads one of the sample source files.
pub(crate) fn open_fixture(name: &str) -> Buffer {
    let path = fixtures().join(name);
    Buffer::open(&path).expect("opening a fixture")
}

/// Draws an application at the given size and dumps the cells.
pub(crate) fn render(app: &mut App, width: u16, height: u16) -> String {
    state_of_its_own();
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
pub(crate) fn cursor_line(dump: &str) -> &str {
    let marker = "-- cursor --\n";
    dump.find(marker)
        .map_or("", |index| dump[index + marker.len()..].trim_end())
}

/// The text rows of a dump.
pub(crate) fn text_block(dump: &str) -> &str {
    section(dump, "-- text --", "-- style --")
}

/// The style rows of a dump.
pub(crate) fn style_block(dump: &str) -> &str {
    section(dump, "-- style --", "-- legend --")
}

/// The legend of a dump.
pub(crate) fn legend_block(dump: &str) -> &str {
    section(dump, "-- legend --", "-- cursor --")
}

/// What some words on screen are drawn in: the legend's line for the cell
/// their first character is in.
///
/// Read out of the dump's own legend, so that a test compares two of these
/// rather than naming a colour -- what it asks is whether a row is marked
/// the way it was a moment ago, not which grey the mark is.
pub(crate) fn drawn_in(dump: &str, needle: &str) -> String {
    let split = |row: &str| -> Option<(u16, String)> {
        let (number, rest) = row.split_once('|')?;
        Some((number.trim().parse().ok()?, rest.to_string()))
    };
    let (at, column) = text_block(dump)
        .lines()
        .filter_map(split)
        .find_map(|(at, text)| {
            text.find(needle)
                .map(|index| (at, text[..index].chars().count()))
        })
        .unwrap_or_else(|| panic!("no {needle:?} on screen:\n{dump}"));
    let letter = style_block(dump)
        .lines()
        .filter_map(split)
        .find(|(row, _)| *row == at)
        .and_then(|(_, marks)| marks.chars().nth(column))
        .unwrap_or_else(|| panic!("no style under {needle:?}:\n{dump}"));
    legend_block(dump)
        .lines()
        .find(|line| line.starts_with(&format!("{letter} ")))
        .map(|line| line[2..].to_string())
        .unwrap_or_else(|| panic!("no legend for {letter}:\n{dump}"))
}

fn section<'a>(dump: &'a str, from: &str, to: &str) -> &'a str {
    let start = dump.find(from).map_or(0, |index| index + from.len());
    let end = dump[start..]
        .find(to)
        .map_or(dump.len(), |index| start + index);
    &dump[start..end]
}

/// Compares a dump against its fixture.
pub(crate) fn check(name: &str, actual: &str) {
    let path = fixtures().join(format!("{name}.txt"));
    let actual = &one_spelling(actual);

    if std::env::var_os("UPDATE_FIXTURES").is_some() {
        fs::write(&path, actual).expect("writing the fixture");
        return;
    }

    let Ok(expected) = fs::read_to_string(&path).map(|text| one_spelling(&text)) else {
        panic!(
            "fixture {name} does not exist yet.\n\
             Review this output, then create it with UPDATE_FIXTURES=1:\n\n{actual}"
        );
    };

    assert!(
        &expected == actual,
        "{name} does not match its fixture.\n\
         If the change is intended: UPDATE_FIXTURES=1 cargo test\n\n\
         --- fixture ---\n{expected}\n--- rendered ---\n{actual}"
    );
}

/// A dump with every separator in a path written the one way.
///
/// Obelus draws a path the way this platform writes one, so a row that reads
/// `src/main.rs` here reads `src\main.rs` there. A fixture is one file and
/// both platforms are read against it, so both sides come through here --
/// which also means a fixture regenerated on either is the same file, and a
/// suite run on a machine nobody has used before does not rewrite thirty of
/// them.
///
/// Safe because nothing Obelus draws carries a backslash of its own: no
/// fixture in this directory holds one. A test that needed to tell the two
/// apart would have to say so some other way, and none does.
fn one_spelling(dump: &str) -> String {
    dump.replace('\\', "/")
}

/// A path written the unix way, spelled the way this platform spells one.
///
/// The other side of [`one_spelling`], for the tests that read a row rather
/// than a whole dump: they are written once and run on both, so what they
/// hold is the unix spelling and this makes it local at the moment of the
/// comparison.
pub(crate) fn as_shown(path: &str) -> String {
    match cfg!(windows) {
        true => path.replace('/', "\\"),
        false => path.to_string(),
    }
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// One cell grid, as text.
fn dump(cells: &CellBuffer, width: u16, height: u16) -> String {
    let regions = obelus_ui::regions(obelus_ui::area_of(ratatui::layout::Size { width, height }));

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

/// Which row and column of a dump the first `glyph` is at.
///
/// Shared by everything that asks about one cell: the colour it is drawn in,
/// the whole style behind it. In characters rather than cells, because what
/// it is for is reaching into the style block, which is one letter per cell
/// the same way the text block is one character per cell.
fn cell_of(dump: &str, glyph: char) -> (usize, usize) {
    text_block(dump)
        .lines()
        .filter(|line| !line.is_empty())
        .enumerate()
        .find_map(|(row, line)| {
            let cells = line.split_once('|').map_or(line, |(_, rest)| rest);
            cells
                .chars()
                .position(|cell| cell == glyph)
                .map(|column| (row, column))
        })
        .unwrap_or_else(|| panic!("nothing on screen draws {glyph:?}:\n{}", text_block(dump)))
}

/// Which cell of a dump's row `needle` starts in.
///
/// Cells, which is what a caret and a column are counted in, and what none
/// of the obvious answers gives: `str::find` hands back a *byte*, and `□` is
/// three bytes, one character and one cell, while `\u{4f60}` is three bytes,
/// one character and *two* cells. A test that reached for `find` and
/// compared it with a caret was right only for rows made of ASCII.
///
/// The row's own `NN|` prefix is not counted, the way the dump's style and
/// legend blocks line up against the text without it.
#[must_use]
pub(crate) fn column_of(row: &str, needle: &str) -> usize {
    let cells = row.split_once('|').map_or(row, |(_, rest)| rest);
    let at = cells
        .find(needle)
        .unwrap_or_else(|| panic!("no {needle:?} on {cells:?}"));
    obelus_text::text_width(&cells[..at])
}

/// The corner a panel Obelus floats over the page is drawn with.
///
/// Rounded, and spelled here rather than in each test: what a test is about
/// is *where* a panel is, and a test that went looking for a square corner
/// found the markdown fence inside a hover instead -- which is square
/// because it belongs to the document, not to Obelus.
pub(crate) const PANEL_CORNER: char = '\u{256d}';

/// The first glyph after `word` on a row, blanks skipped.
///
/// For the marks a row wears beside its words: the box after a switch's
/// name, and whatever else is drawn against a label rather than written
/// into it. Read off the row rather than spelled here, so a test says the
/// mark *changed* rather than deciding what a mark looks like -- which
/// would be asking the thing under test what it expects.
#[must_use]
pub(crate) fn glyph_after(row: &str, word: &str) -> char {
    let at = row
        .find(word)
        .unwrap_or_else(|| panic!("no {word:?} on {row:?}"));
    row[at + word.len()..]
        .chars()
        .find(|glyph| !glyph.is_whitespace())
        .unwrap_or_else(|| panic!("nothing after {word:?} on {row:?}"))
}

/// The same, on the other side of it.
///
/// The row's own `NN|` prefix is not a glyph: a mark in the first column
/// would otherwise come back as the bar that separates the dump's numbers
/// from its cells.
#[must_use]
pub(crate) fn glyph_before(row: &str, word: &str) -> char {
    let at = row
        .find(word)
        .unwrap_or_else(|| panic!("no {word:?} on {row:?}"));
    row[..at]
        .chars()
        .rev()
        .find(|glyph| !glyph.is_whitespace() && *glyph != '|')
        .unwrap_or_else(|| panic!("nothing in front of {word:?} on {row:?}"))
}

/// The legend entry for the first cell drawing `glyph`: `fg=… bg=…`.
///
/// The whole entry, for a test about a background: [`colour_under`] answers
/// about the foreground, and a row's ground is the other half of what it is
/// drawn in.
#[must_use]
pub(crate) fn legend_for(dump: &str, glyph: char) -> String {
    let (row, column) = cell_of(dump, glyph);
    let letter = style_block(dump)
        .lines()
        .filter(|line| !line.is_empty())
        .nth(row)
        .and_then(|line| {
            let cells = line.split_once('|').map_or(line, |(_, rest)| rest);
            cells.chars().nth(column)
        })
        .expect("the style row under the text row");

    legend_block(dump)
        .lines()
        .find(|line| line.starts_with(&format!("{letter} ")))
        .expect("the legend entry for the style")
        .to_string()
}

/// What a style letter of a dump's style block stands for.
///
/// The letter rather than the character drawn there, which is the other
/// way round from [`legend_for`] and the one a test wants when it is
/// comparing two cells: the letters are what the style block is made of,
/// and looking one up by finding the same letter in the *text* finds
/// whatever unrelated cell happens to draw it.
#[must_use]
pub(crate) fn legend_of(dump: &str, letter: char) -> String {
    legend_block(dump)
        .lines()
        .find(|line| line.starts_with(&format!("{letter} ")))
        .unwrap_or_else(|| panic!("no legend for the style {letter:?}:\n{dump}"))
        .to_string()
}

/// A theme's colour, spelled the way the legend spells it.
///
/// So that a test names the colour it means -- `DARK.gutter` -- rather than
/// the six hex digits that colour happens to be today.
#[must_use]
pub(crate) fn spelled(value: Color) -> String {
    colour(value)
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
/// Once per name per run, checked: two tests in one binary run at the same
/// time, so a name asked for twice is two tests clearing each other's ground
/// -- which shows up as a test that fails one run in three and nothing to
/// see in either of them.
///
/// The process id is in the name so that two runs at once do not clear each
/// other's ground, which is a failure that looks like a flaky test.
pub(crate) struct Scratch {
    path: PathBuf,
    name: String,
}

/// Keeps this run's state out of the reader's own.
///
/// The notes, the table of which conversation is about which note and the
/// claims saying which are open all live in Obelus's state directory, so a
/// test that did not say this would write into the reader's and leave it
/// there -- which the suite did, for as long as the conversations have been
/// kept there. One directory for the binary: every test names its project
/// after its own scratch, so they do not meet inside it.
///
/// A value rather than `XDG_STATE_HOME`, which is what the first test that
/// needed it reached for: setting the environment while other tests are
/// running is the thing the language made unsafe, and this is wanted by
/// tests that run at the same time.
///
/// Called from both doors a test comes in by -- making a scratch directory
/// and laying the screen out -- because a test that does neither touches
/// nothing of Obelus's either.
pub(crate) fn state_of_its_own() {
    obelus_logging::state_directory_for_test(
        std::env::temp_dir().join(format!("obelus-state-{}", std::process::id())),
    );
}

/// Makes the directory a project's notes are written into.
///
/// They are kept in Obelus's state directory now rather than beside the
/// project, so a test that writes a file of them has to ask where that is
/// -- and make the directory, which Obelus itself makes on its way past.
pub(crate) fn make_room_for_notes(root: &std::path::Path) {
    let path = obelus_git::todo::path(root).expect("a tree that is there");
    let directory = path.parent().expect("the notes are in a directory");
    std::fs::create_dir_all(directory).expect("the directory");
}

impl Scratch {
    /// An empty directory, whatever was there before.
    pub(crate) fn new(name: &str) -> Self {
        // Once per name per run. Two tests that asked for the same one would
        // each clear the other's ground -- they run at the same time in the
        // same process, so the pid in the path does not keep them apart --
        // and what that looks like from outside is a test that fails one run
        // in three for no reason anybody can see. It costs an afternoon; the
        // panic costs a moment and says which two.
        static TAKEN: std::sync::Mutex<Option<std::collections::HashSet<String>>> =
            std::sync::Mutex::new(None);
        let mut taken = TAKEN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(
            taken
                .get_or_insert_with(std::collections::HashSet::new)
                .insert(name.to_string()),
            "two tests asked for the scratch directory {name:?}, and they would clear each other's"
        );
        drop(taken);
        state_of_its_own();

        let path = std::env::temp_dir().join(format!("obelus-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("a scratch directory");
        Self {
            path,
            name: name.to_string(),
        }
    }

    /// The directory itself.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// A path inside it, which need not exist.
    pub(crate) fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }

    /// Writes a file inside it, making any directories on the way.
    pub(crate) fn write(&self, name: &str, contents: &str) -> PathBuf {
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
/// list and the preview, and one above the status row. So the preview is
/// what lies between the *second* rule and the last -- not between the
/// last two, which is what this said while the preview had no rule of its
/// own. A commit's message carries one under it now, and counting from the
/// end took that for the preview's top edge: everything the assertions
/// were about was above it, and the slice came back holding the file
/// alone.
#[allow(dead_code)]
pub(crate) fn previewed(dump: &str) -> String {
    let rows: Vec<&str> = text_block(dump).lines().collect();
    let rules: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.contains('\u{2500}'))
        .map(|(at, _)| at)
        .collect();
    assert!(
        rules.len() >= 3,
        "not a screen with a preview on it: rules at {rules:?}"
    );
    let (from, to) = (rules[1] + 1, rules[rules.len() - 1]);
    rows[from..to].join("\n")
}

/// Gives the app a channel, so the walks it starts have somewhere to answer.
///
/// The loop does this at startup; a test drives the app by hand and would
/// otherwise start threads that can never report back.
#[allow(dead_code)]
pub(crate) fn drive(app: &mut App) -> std::sync::mpsc::Receiver<obelus_app::event::Event> {
    let (sender, events) = obelus_app::event::channel();
    app.events_for_test(sender);
    events
}

/// Runs the history walk the app has started through to its last batch.
///
/// A history arrives in batches while the reader reads, which is the point
/// of it; a test wants the finished list, so it waits for the batch that
/// says there are no more.
#[allow(dead_code)]
pub(crate) fn read_history(
    app: &mut App,
    events: &std::sync::mpsc::Receiver<obelus_app::event::Event>,
) {
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

/// The ways out of the question Obelus is asking, in the order offered.
///
/// Panics if it is not asking one: a test that walks past a question it did
/// not expect would go on to assert about a screen nobody is looking at.
pub(crate) fn ways(app: &App) -> Vec<String> {
    app.picker()
        .expect("a question is being asked")
        .matches()
        .map(|item| item.label.clone())
        .collect()
}

/// Answers the question Obelus is asking, by walking to a way out and
/// choosing it.
///
/// Through the arrow keys and enter rather than by reaching for the value,
/// so that what a test answers is what a reader could answer.
pub(crate) fn answer(app: &mut App, way: &str) {
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
pub(crate) fn said(dump: &str) -> String {
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
/// What Obelus keeps when no provider can hold a copy is one thing for the
/// whole process, so two tests reading it at once each see what the other
/// put there. Held for as long as the returned guard lives, which is the
/// body of the test that took it.
///
/// Every test that *writes* the clipboard has to take it, not only the ones
/// that read one back: `use_provider_for_test` empties what is kept, so a
/// test merely asking for a provider wipes the copy another had just made.
/// And the asking is sticky -- a test that does not ask gets whichever
/// provider the last one asked for, which makes what it does depend on the
/// order the threads happened to run in.
///
/// One mutex per test binary, because the state is per process: a file whose
/// tests all take it is safe from the others whatever they do.
pub(crate) fn clipboard_turn() -> std::sync::MutexGuard<'static, ()> {
    static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
    // A test that failed while holding it poisoned nothing: there is no
    // state behind this, only the taking of turns.
    TURN.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// What the first cell drawing `glyph` is painted in, as the legend spells
/// colours: `#rrggbb`.
///
/// A mark is one cell and its whole job is the colour it is in, so a test of
/// one is a test of that cell's foreground. Found by the glyph rather than by
/// a column, because the column a mark lands in moves with the indent, the
/// icon and the width, and none of those are what is being asserted.
#[must_use]
pub(crate) fn colour_under(dump: &str, glyph: char) -> String {
    let (row, column) = cell_of(dump, glyph);
    let letter = style_block(dump)
        .lines()
        .filter(|line| !line.is_empty())
        .nth(row)
        .and_then(|line| {
            let cells = line.split_once('|').map_or(line, |(_, rest)| rest);
            cells.chars().nth(column)
        })
        .expect("the style row under the text row");

    legend_block(dump)
        .lines()
        .find(|line| line.starts_with(&format!("{letter} ")))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|field| field.strip_prefix("fg="))
        .expect("the legend entry for the style")
        .to_string()
}

/// Sends a letter with alt held, which is the family Obelus's own commands
/// live in.
pub(crate) fn press_alt(app: &mut App, character: char) {
    app.handle(Event::Key(KeyEvent::new(
        KeyCode::Char(character),
        KeyModifiers::ALT,
    )));
}

/// Where a run of text is on screen, as a row and a column.
///
/// Counted in characters rather than in bytes: `str::find` answers in
/// bytes, and a row with a glyph in front of it -- the mark a list draws
/// before its query -- is four bytes and one cell wide, so the two
/// disagree by three wherever it matters.
#[must_use]
pub(crate) fn place_of(app: &mut App, needle: &str) -> (u16, u16) {
    let dump = render(app, 76, 18);
    let rows: Vec<String> = text_block(&dump)
        .lines()
        .filter_map(|row| row.split_once('|').map(|(_, cells)| cells.to_string()))
        .collect();
    let (row, column) = rows
        .iter()
        .enumerate()
        .find_map(|(y, row)| {
            row.find(needle)
                .map(|byte| (y, row[..byte].chars().count()))
        })
        .unwrap_or_else(|| panic!("{needle:?} is not on screen:\n{dump}"));
    (
        u16::try_from(row).unwrap_or(u16::MAX),
        u16::try_from(column).unwrap_or(u16::MAX),
    )
}

/// Whether an event the application started a clock for ever arrives.
///
/// Generously longer than any pause Obelus keeps: what is being asked is
/// whether anything comes back at all, and a machine under load is not a
/// failure. Everything else on the channel is walked past -- a clock is not
/// the only thing that sends.
pub(crate) fn waited_for(
    heard: &std::sync::mpsc::Receiver<obelus_app::event::Event>,
    which: impl Fn(&obelus_app::event::Event) -> bool,
) -> bool {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while let Some(left) = until.checked_duration_since(std::time::Instant::now()) {
        match heard.recv_timeout(left) {
            Ok(event) if which(&event) => return true,
            Ok(_) => {}
            Err(_) => break,
        }
    }
    false
}

/// The messages of a kind that Obelus wrote, as the echo gave them back.
///
/// Waits for `want` of them and then stops waiting, so a test that
/// expects none pays a moment and a test that expects two does not: there
/// is no event to wait for when the point is that none is coming.
pub(crate) fn heard_requests(
    heard: &std::sync::mpsc::Receiver<obelus_app::event::Event>,
    method: &str,
    want: usize,
) -> Vec<serde_json::Value> {
    let mut seen = Vec::new();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while seen.len() < want.max(1) {
        let Some(left) = until.checked_duration_since(std::time::Instant::now()) else {
            break;
        };
        match heard.recv_timeout(left) {
            Ok(obelus_app::event::Event::Lsp(obelus_lsp::Message { message, .. }))
                if message.get("method").and_then(serde_json::Value::as_str) == Some(method) =>
            {
                seen.push(message);
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    seen
}
