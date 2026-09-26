//! What Obelus shows when nothing is open.
//!
//! Two layouts. The wide one is a wordmark, the keys as caps, and a footer;
//! the narrow one is the keys and nothing else. A screen too small for even
//! that gets nothing, because a welcome squeezed into wrapping is worse than
//! an empty one.
//!
//! The keys come from the key table rather than from strings here, so a
//! rebound key changes the screen instead of leaving it lying. The working
//! directory is on it because the file picker only ever searches that one
//! project, which is worth knowing before pressing the key that opens it.

use crossterm::event::{KeyCode, KeyModifiers};
use obelus_command::Command;
use obelus_editing::keymap::{KeyChord, Keymap};
use obelus_theme::Theme;
use ratatui::{
    buffer::Buffer as CellBuffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};
use unicode_width::{UnicodeWidthChar as _, UnicodeWidthStr};

use crate::{Screen, put, write};

/// The commands worth naming, with the words this screen says them in.
///
/// Ways *in*, which is what this screen is for: a file, a file that has
/// changed, a search of the project, the agent, everything by name, and the way
/// out. Not `switch-document` -- there is nothing open to switch to on the one
/// screen where this is showing.
///
/// The words are this screen's own, not [`obelus_command::CommandSpec`]'s.
/// A palette row is read once, in a list, with the whole width to say what
/// the command does; these sit two to a line under a plate fifty columns
/// wide, beside a key and a picture of what they open. What is left to say
/// there is which *thing* -- a file, the files that changed, the agent --
/// and the verb is the key. Thirteen columns is what two of them and their
/// caps fit into, which is the other half of why they are short.
const OFFERED: &[(Command, &str)] = &[
    (Command::FileOpen, "Open a file"),
    (Command::FileChanged, "Changed files"),
    (Command::SearchProject, "Search files"),
    (Command::AgentOpen, "Ask the agent"),
    (Command::CommandPalette, "Run a command"),
    (Command::Quit, "Leave Obelus"),
];

/// How many columns of keys the plate carries under it.
const COLUMNS: usize = 2;

/// The gap between those columns.
const GUTTER: u16 = 4;

/// How many rows of screen one row of the grid takes.
///
/// Two: the keys are read one at a time, and a blank between them is what
/// makes a block of six read as six things rather than as a paragraph.
const ROW_HEIGHT: u16 = 2;

/// How wide the block of what went wrong is allowed to be.
///
/// Wider than the plate, which the keys are centred on: those are two
/// short words under a cap, and these are a sentence with a file and a
/// line after it. Capped, because a line of prose run across a wide
/// terminal is a line the eye loses its place in.
const AMISS_WIDTH: u16 = 64;

/// And how many of them are shown at once.
///
/// Six, which is as many as the keys above take. More than that and the
/// block is the screen rather than a note under the way in -- and the list
/// is a list, reachable by name, where there is room for all of them.
///
/// Public because the application walks the list by it: what the keys move
/// by has to be what is on screen, and two answers to how many rows there
/// are is a page key that skips some.
pub const AMISS_ROWS: u16 = 6;

/// The gap between a key and what it opens.
///
/// One, against the four between the columns: the key and the words after
/// it are one thing said twice, and a cell of air between them is enough to
/// keep the keys a column of their own. More than that and each row reads
/// as two things that happen to share a line.
const BESIDE: u16 = 1;

/// The name, on a plate.
///
/// Block elements and box drawing, which every monospace font has -- unlike
/// the private use area the file glyphs come from, so this is the one thing
/// on screen that needs no particular font.
///
/// The letters are drawn at half a row's resolution: a stroke that ends
/// halfway down a cell ends in a half block, so the round ends of the O, the
/// S and the U are curves rather than the square steps a whole cell gives.
///
/// The frame is part of the mark rather than drawn around it, so the sheen
/// runs through it -- the ramp is taken per column across whatever is here,
/// and a frame outside it would be the one still thing on a moving screen.
const WORDMARK: &[&str] = &[
    "╔═════════════════════════════════════════════════╗",
    "║ ▄█████▄ ██████▄ ███████ ██      ██   ██ ▄█████▄ ║",
    "║ ██   ██ ██   ██ ██      ██      ██   ██ ██      ║",
    "║ ██   ██ ██████  ██████  ██      ██   ██ ▀█████▄ ║",
    "║ ██   ██ ██   ██ ██      ██      ██   ██      ██ ║",
    "║ ▀█████▀ ██████▀ ███████ ███████ ▀█████▀ ▀█████▀ ║",
    "╚═════════════════════════════════════════════════╝",
];

/// How many colours the ramp across the wordmark is made of.
///
/// Quantized rather than continuous. Forty-odd distinct colours across
/// forty-odd columns look no smoother than eight bands of six, and they turn
/// a golden fixture's legend into forty lines nobody can read.
const RAMP_STEPS: u16 = 8;

/// How many ticks the ramp takes to travel its own length once.
///
/// Sixteen at twelve a second, so a little over a second a cycle: slow enough
/// to read as a sheen moving across the letters rather than as something
/// flashing, and a whole number of steps so the cycle has no seam.
const CYCLE: u32 = RAMP_STEPS as u32 * 2;

/// What is set into the plate's own edge: the version, and which build.
///
/// In the frame rather than on a row of its own: both are facts about the
/// thing the plate names, and a line under the plate holding two short
/// words is a row of screen spent on punctuation.
///
/// The build is beside the version because the version does not answer the
/// question anybody actually has. It has said `0.1.0` since the first
/// commit and will until a release changes it, so "was the fix in the thing
/// I am looking at" has to be answered by the commit -- and the log is not
/// where a reader looks, this screen is.
fn label(built: &str) -> String {
    match built.is_empty() {
        true => concat!(" v", env!("CARGO_PKG_VERSION"), " ").to_string(),
        false => format!(" v{} {built} ", env!("CARGO_PKG_VERSION")),
    }
}

/// The plate's foot with that set into it.
///
/// Composed here rather than written into [`WORDMARK`], because neither is
/// Obelus's to spell: the version comes from the manifest and the build from
/// whatever started Obelus, and a copy in a string here is a copy that goes
/// stale the moment either moves.
///
/// An edge with no room for both falls back to the version, and then to
/// nothing: what the plate is for is the way in, and a frame broken open to
/// fit a commit into it is worse than a frame that does not say one.
fn foot(built: &str) -> String {
    let Some(edge) = WORDMARK.last() else {
        return String::new();
    };
    let width = edge.chars().count();
    let set = [label(built), label("")]
        .into_iter()
        .find(|said| width >= said.chars().count() + 4);
    let Some(set) = set else {
        return (*edge).to_string();
    };
    let taken = set.chars().count();
    let at = (width - taken) / 2;
    edge.chars()
        .take(at)
        .chain(set.chars())
        .chain(edge.chars().skip(at + taken))
        .collect()
}

/// Whether the screen has room for what went wrong under the keys.
///
/// Answered where the height is worked out and carried down, rather than
/// asked again lower: the second answer disagreed with the first.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Amiss {
    /// There is room.
    Shown,
    /// There is not, so the way in gets the screen.
    Left,
}

/// The centred block.
pub struct WelcomeView<'a> {
    keymap: &'a Keymap,
    /// What went wrong on the way up. Empty on almost every start, and
    /// then the block is not there at all.
    went_wrong: Vec<crate::WentWrong>,
    /// Which of them the reader is on, and which are on screen.
    at: usize,
    showing: std::ops::Range<usize>,
    theme: &'a Theme,
    /// Which build this is, for the plate's edge.
    built: &'a str,
    /// How far the ramp has travelled, in ticks.
    ///
    /// Zero unless something is ticking, and nothing ticks once a file is
    /// open or when the session is remote, so this is the only thing that
    /// makes the screen differ between two draws.
    phase: u32,
}

impl<'a> WelcomeView<'a> {
    /// Borrows what the view needs.
    #[must_use]
    pub fn new(app: &'a impl Screen) -> Self {
        Self {
            keymap: app.keymap(),
            went_wrong: app.went_wrong(),
            at: app.went_wrong_at(),
            showing: app.went_wrong_showing(AMISS_ROWS),
            theme: app.theme(),
            built: app.built(),
            phase: app.phase(),
        }
    }
}

/// One key, the picture of what it opens, and what it is called here.
struct Hint {
    /// The glyph for the command, where the font has one.
    icon: Option<char>,
    key: String,
    text: String,
}

impl Widget for WelcomeView<'_> {
    fn render(self, area: Rect, cells: &mut CellBuffer) {
        let hints = self.hints();

        // The plate, a blank, and the keys. Nothing else: what the reader
        // needs here is the way in, and the version is set into the plate's
        // own edge rather than spending a row of its own.
        // The plate, a blank, and the keys two to a line with a blank
        // between the lines -- and none after the last of them.
        let lines = u16::try_from(hints.len().div_ceil(COLUMNS)).unwrap_or(1);
        let keys = (lines * ROW_HEIGHT).saturating_sub(1);
        let amiss = self.amiss_height();
        let tall = u16::try_from(WORDMARK.len()).unwrap_or(u16::MAX) + 1 + keys + amiss;
        let wordmark = width_of(WORDMARK[0]);
        let short = u16::try_from(hints.len() + 2).unwrap_or(u16::MAX);

        if wordmark <= area.width && tall <= area.height {
            self.lavish(area, cells, &hints, wordmark, tall, Amiss::Shown);
        } else if wordmark <= area.width && tall.saturating_sub(amiss) <= area.height {
            // The plate and the keys, and what went wrong left out. Which
            // is the wrong way round if it were a matter of what is worth
            // the room -- but the block is reachable by name and the way
            // in is the only thing this screen is for.
            //
            // Said rather than worked out again: `lavish` deciding for
            // itself whether the block fits is a second answer to a
            // question already asked, and it got it wrong -- the keys and
            // the block are the same height, so "there is room below the
            // plate" was true in exactly the case this branch is for.
            self.lavish(area, cells, &hints, wordmark, tall - amiss, Amiss::Left);
        } else if let Some(narrow) = hint_block_width(&hints)
            && narrow <= area.width
            && short <= area.height
        {
            self.compact(area, cells, &hints, narrow, short);
        }
    }
}

impl WelcomeView<'_> {
    /// The plate, and the keys under it.
    fn lavish(
        &self,
        area: Rect,
        cells: &mut CellBuffer,
        hints: &[Hint],
        width: u16,
        height: u16,
        amiss: Amiss,
    ) {
        let left = area.x + (area.width - width) / 2;
        let mut y = area.y + (area.height - height) / 2;

        // A ramp across the letters, in the theme's own accent hues rather
        // than in colours invented here, so it belongs to whichever theme is
        // on. The frame and the version in its foot are in it too: they are
        // part of the mark, and one still thing in a moving one reads as a
        // thing that has stopped.
        let from = self.theme.syntax.keyword;
        let to = self.theme.syntax.function;
        let foot = foot(self.built);
        let last = WORDMARK.len().saturating_sub(1);
        for (at, row) in WORDMARK.iter().enumerate() {
            let row = if at == last { foot.as_str() } else { *row };
            let mut column = 0u16;
            for character in row.chars() {
                if character != ' ' {
                    let step = u32::from(column * RAMP_STEPS / width.max(1));
                    put(
                        cells,
                        left + column,
                        y,
                        character,
                        Style::new().fg(sheen(from, to, step, self.phase)),
                    );
                }
                column = column.saturating_add(1);
            }
            y += 1;
        }

        y += 1;
        self.grid(cells, left, y, width, hints);

        // Under the keys, where the caller said there was room for it.
        if amiss == Amiss::Shown && self.amiss_height() > 0 {
            let keys = (u16::try_from(hints.len().div_ceil(COLUMNS)).unwrap_or(1) * ROW_HEIGHT)
                .saturating_sub(1);
            self.amiss(cells, area, y + keys + 1);
        }
    }

    /// How many rows the block of what went wrong takes, with its heading
    /// and the blank above it.
    fn amiss_height(&self) -> u16 {
        let rows = self.went_wrong.len().min(AMISS_ROWS as usize);
        match rows {
            0 => 0,
            // The heading and a blank above the rows, and a blank and the
            // foot below them: a reader has to be told the key is there
            // before they press it, which is the rule the palette follows
            // for a command it will not run.
            rows => u16::try_from(rows).unwrap_or(0) + 4,
        }
    }

    /// What went wrong on the way up, under a heading of its own.
    ///
    /// The heading is in the theme's warning colour and the rows are not:
    /// a block of coloured prose is a block a reader cannot read, and what
    /// the colour is for is saying which block this is. Where each row is
    /// about a line of a file, that file and line are the row's tail --
    /// worked out first, so a long sentence cannot push it off the screen.
    fn amiss(&self, cells: &mut CellBuffer, area: Rect, top: u16) {
        let width = AMISS_WIDTH.min(area.width);
        let left = area.x + (area.width.saturating_sub(width)) / 2;
        write(
            cells,
            left + 1,
            top,
            "What went wrong starting up",
            Style::new().fg(self.theme.syntax.warning),
        );
        let showing = self.showing.start..self.showing.end.min(self.went_wrong.len());
        for (offset, at) in showing.clone().enumerate() {
            let Some(row) = self.went_wrong.get(at) else {
                continue;
            };
            let Ok(offset) = u16::try_from(offset) else {
                continue;
            };
            self.amiss_row(cells, left, top + 2 + offset, width, row, at == self.at);
        }

        // What the key on the row does, and only where it does anything:
        // a row with nowhere to go is one the reader is told about by the
        // foot going quiet rather than by pressing and getting nothing.
        let goes = self
            .went_wrong
            .get(self.at)
            .is_some_and(|row| row.at.is_some());
        if goes {
            let Ok(under) = u16::try_from(showing.len()) else {
                return;
            };
            let enter = KeyChord::new(KeyCode::Enter, KeyModifiers::NONE).label();
            write(
                cells,
                left + 1,
                top + 3 + under,
                &format!("{enter}  Go to it"),
                Style::new().fg(self.theme.gutter),
            );
        }
    }

    /// One of them: what Obelus says, and where to go.
    fn amiss_row(
        &self,
        cells: &mut CellBuffer,
        left: u16,
        y: u16,
        width: u16,
        row: &crate::WentWrong,
        here: bool,
    ) {
        // The one mark for "the keys are here", the same one every list,
        // page and card in Obelus puts behind the row the reader is on.
        let ink = Style::new().fg(self.theme.foreground);
        let ink = match here {
            true => ink.bg(self.theme.selected_row_background),
            false => ink,
        };
        if here {
            for column in 0..width {
                put(
                    cells,
                    left + column,
                    y,
                    ' ',
                    Style::new().bg(self.theme.selected_row_background),
                );
            }
        }
        let tail = row.at.as_ref().map(|(path, line)| {
            let name = path.file_name().map_or_else(
                || path.display().to_string(),
                |name| name.to_string_lossy().to_string(),
            );
            format!("{name}:{}", line.get() + 1)
        });
        // The tail first, so the sentence takes what is left: somebody
        // else's words may be as long as they like and may not push what
        // the row says about itself off the screen.
        let tail_width = tail.as_deref().map_or(0, str::width);
        let room = usize::from(width)
            .saturating_sub(2)
            .saturating_sub(if tail_width > 0 { tail_width + 2 } else { 0 });
        let said = crate::truncate_from_right(&row.said, room);
        write(cells, left + 1, y, &said, ink);
        if let Some(tail) = tail
            && let Ok(offset) = u16::try_from(usize::from(width).saturating_sub(tail_width + 1))
        {
            let tail_ink = match here {
                true => Style::new()
                    .fg(self.theme.gutter)
                    .bg(self.theme.selected_row_background),
                false => Style::new().fg(self.theme.gutter),
            };
            write(cells, left + offset, y, &tail, tail_ink);
        }
    }

    /// The keys in columns under the plate, centred on it.
    ///
    /// Two to a line, which is what makes the block as wide as the plate
    /// rather than a narrow list against one edge of it. Every column is the
    /// same width and every cell in it starts at the same cell, so a reader
    /// runs down either one.
    fn grid(&self, cells: &mut CellBuffer, left: u16, top: u16, width: u16, hints: &[Hint]) {
        let Some(cell) = cell_width(hints) else {
            return;
        };
        let columns = u16::try_from(COLUMNS).unwrap_or(1);
        let block = cell * columns + GUTTER * (columns - 1);
        let indent = width.saturating_sub(block) / 2;

        for (at, hint) in hints.iter().enumerate() {
            let Ok(column) = u16::try_from(at % COLUMNS) else {
                continue;
            };
            let Ok(row) = u16::try_from(at / COLUMNS) else {
                continue;
            };
            self.hint_row(
                cells,
                left + indent + column * (cell + GUTTER),
                top + row * ROW_HEIGHT,
                keys_width(hints),
                hint,
            );
        }
    }

    /// The keys, and nothing that needs room.
    fn compact(&self, area: Rect, cells: &mut CellBuffer, hints: &[Hint], width: u16, height: u16) {
        let left = area.x + (area.width - width) / 2;
        let mut y = area.y + (area.height - height) / 2;

        write(
            cells,
            left,
            y,
            "Obelus",
            Style::new().fg(self.theme.foreground),
        );
        // The version and the build at the other end of the same row, which
        // is where the plate carries them when there is room for a plate.
        // The widest of them that fits beside the name, and nothing where
        // neither does: this layout is what a screen too small for the
        // plate gets, and the name is the part of it worth the room.
        let room = usize::from(width).saturating_sub("Obelus".width() + 1);
        if let Some(said) = [label(self.built), label("")]
            .into_iter()
            .map(|said| said.trim().to_string())
            .find(|said| said.width() <= room)
            && let Ok(offset) = u16::try_from(usize::from(width).saturating_sub(said.width()))
        {
            write(
                cells,
                left + offset,
                y,
                &said,
                Style::new().fg(self.theme.gutter),
            );
        }
        y += 2;

        let keys = hints.iter().map(|hint| hint.key.width()).max().unwrap_or(0);
        for hint in hints {
            self.hint_row(cells, left, y, keys, hint);
            y += 1;
        }
    }

    /// One cell of the grid: the key, then the picture of what it opens and
    /// what it is called.
    ///
    /// The key first because that is what the reader is here to learn, and
    /// in plain ink rather than on a panel: six panels in a block is six
    /// strips of colour on a screen that is otherwise a wordmark and some
    /// words, and the key does not need to be told apart from prose when it
    /// is a glyph in its own column. A foot answers the other way, and for
    /// the opposite reason -- see `obelus_theme::Theme::raised_background`.
    ///
    /// Which is still the whole of the answer for a terminal, and nothing
    /// below writes a cell it did not write before. A window is told those
    /// cells are a key's cap all the same, and what it draws is the
    /// *shape* -- an outline and the lip under it, which puts no colour on
    /// the screen at all. The two are not the same decision, and only one
    /// of them was ever about panels.
    ///
    /// The picture belongs to the words, not to the key -- it is a picture
    /// of the *thing* -- so it sits against them, and the key column is left
    /// to the keys.
    ///
    /// Every part sits at a fixed offset from the cell's own left edge, and
    /// the offsets come from the widest of each across all the hints, so the
    /// two columns line up with each other and the cells line up down a
    /// column.
    fn hint_row(&self, cells: &mut CellBuffer, left: u16, y: u16, keys: usize, hint: &Hint) {
        let Ok(width) = u16::try_from(keys) else {
            return;
        };
        let inset = u16::try_from(keys.saturating_sub(drawn(&hint.key))).unwrap_or(0);
        write(
            cells,
            left + inset,
            y,
            &hint.key,
            Style::new().fg(self.theme.foreground),
        );
        // Measured with `drawn` rather than by the cells, because this
        // screen counts a glyph as the two columns a font draws it in and
        // a cap that used the other count would stop short of its key.
        crate::cap_around(
            left + inset,
            y,
            &hint.key,
            drawn(&hint.key),
            self.theme.background,
            self.theme.background,
            self.theme.gutter,
        );

        let mut x = left.saturating_add(width).saturating_add(BESIDE);
        if let Some(icon) = hint.icon {
            put(cells, x, y, icon, Style::new().fg(self.theme.gutter));
            // Two, always: the terminal allocates one cell for a private use
            // codepoint and the font draws two.
            x = x.saturating_add(2);
        }
        write(cells, x, y, &hint.text, Style::new().fg(self.theme.gutter));
    }

    fn hints(&self) -> Vec<Hint> {
        OFFERED
            .iter()
            .filter_map(|(command, text)| {
                let chord = self.keymap.chord_for(*command)?;
                Some(Hint {
                    icon: obelus_icons::enabled().then(|| obelus_icons::for_command(*command)),
                    key: chord.label(),
                    text: (*text).to_string(),
                })
            })
            .collect()
    }
}

/// How many cells a run of text is *drawn* in.
///
/// Not [`UnicodeWidthStr::width`], which answers one for a private use
/// codepoint: a Nerd Font draws those two cells wide while the terminal
/// allocates one, which is why everything here leaves a blank column after a
/// glyph. Two columns of keys line up only if the measuring agrees with the
/// drawing, and the width tables cannot -- they know nothing about the font.
fn drawn(contents: &str) -> usize {
    contents
        .chars()
        .map(|character| match character {
            '\u{e000}'..='\u{f8ff}' | '\u{f0000}'..='\u{ffffd}' => 2,
            _ => character.width().unwrap_or(0),
        })
        .sum()
}

/// How wide the column of keys has to be, as drawn.
fn keys_width(hints: &[Hint]) -> usize {
    hints.iter().map(|hint| drawn(&hint.key)).max().unwrap_or(0)
}

/// How wide one cell of the grid is: the key, a gap, the picture, the words.
fn cell_width(hints: &[Hint]) -> Option<u16> {
    let text = hints.iter().map(|hint| drawn(&hint.text)).max()?;
    let icon = usize::from(hints.iter().any(|hint| hint.icon.is_some())) * 2;
    u16::try_from(keys_width(hints) + usize::from(BESIDE) + icon + text).ok()
}

/// How wide the compact block has to be, which is one cell.
fn hint_block_width(hints: &[Hint]) -> Option<u16> {
    cell_width(hints)
}

fn width_of(row: &str) -> u16 {
    u16::try_from(row.width()).unwrap_or(u16::MAX)
}

/// The colour of one step of the wordmark at one moment.
///
/// The ramp runs from `from` to `to` and back again over [`CYCLE`] steps, and
/// the phase slides the whole thing along. Out and back rather than round,
/// because a ramp that wraps from its last colour straight to its first has a
/// visible seam travelling across the letters -- which reads as a glitch,
/// not as a sheen.
fn sheen(from: Color, to: Color, step: u32, phase: u32) -> Color {
    let at = (step + phase) % CYCLE;
    let half = CYCLE / 2;
    // The way back, mirrored.
    let along = if at < half { at } else { CYCLE - at };
    #[expect(
        clippy::cast_precision_loss,
        reason = "both are below CYCLE, which is sixteen"
    )]
    ramp(from, to, along as f32 / half as f32)
}

fn ramp(from: Color, to: Color, along: f32) -> Color {
    let (Color::Rgb(fr, fg, fb), Color::Rgb(tr, tg, tb)) = (from, to) else {
        // A theme in named colours has nothing to interpolate between.
        return from;
    };
    let mix = |a: u8, b: u8| {
        let a = f32::from(a);
        let b = f32::from(b);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the result of mixing two u8s along [0, 1] is within u8"
        )]
        {
            (a + (b - a) * along.clamp(0.0, 1.0)) as u8
        }
    };
    Color::Rgb(mix(fr, tr), mix(fg, tg), mix(fb, tb))
}
