//! What Obelus shows when nothing is open.
//!
//! Two layouts. The wide one is a wordmark, the keys as caps, and the
//! address of Obelus's website where there is a row for it; the narrow one
//! is the keys and nothing else. A screen too small for even
//! that gets nothing, because a welcome squeezed into wrapping is worse than
//! an empty one.
//!
//! The keys come from the key table rather than from strings here, so a
//! rebound key changes the screen instead of leaving it lying.
//!
//! Which project they are about is *not* here. It is on the status row
//! under this screen, where everything Obelus says about what is being
//! read goes -- a file's path and its mode go there, and which project is
//! the same kind of fact one step out. This doc claimed for a long time
//! that it was on the page, while nothing drew it anywhere.
//!
//! What went wrong on the way up is *not* here either. It is a list put up
//! over this screen, or over the page that asks which project, when Obelus
//! starts -- `App::tell_what_went_wrong` -- and a block drawn here under the
//! keys was a second thing on this screen with keys of its own, which is
//! one more than a way in should have.

use obelus_command::Command;
use obelus_editing::keymap::Keymap;
use obelus_theme::Theme;
use ratatui::{
    buffer::Buffer as CellBuffer,
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
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
    (Command::ConversationSelect, "Ask the agent"),
    (Command::CommandPalette, "Run a command"),
    (Command::Quit, "Leave Obelus"),
];

/// Where Obelus's own website is.
///
/// The one thing on this screen a press does: the keys are the way into a
/// project, and this is the way to what Obelus is for, which somebody
/// looking at an empty screen is the reader most likely to want.
pub const SITE: &str = "https://obelus-editor.github.io/obelus/";

/// The address as the screen says it, and the blanks it is raised in.
///
/// Without the scheme or the last slash: neither tells a reader anything a
/// browser would not add, and both are columns. The blanks are the ones the
/// way back to the end of a conversation is raised in, for the same reason
/// -- what is under the pointer is a box, and words with no air round them
/// are a band.
fn site_label() -> String {
    let said = SITE.trim_start_matches("https://").trim_end_matches('/');
    format!("  {said}  ")
}

/// How many columns of keys the plate carries under it.
const COLUMNS: usize = 2;

/// The gap between those columns.
const GUTTER: u16 = 4;

/// How many rows of screen one row of the grid takes.
///
/// Two: the keys are read one at a time, and a blank between them is what
/// makes a block of six read as six things rather than as a paragraph.
const ROW_HEIGHT: u16 = 2;

/// The gap between a key and what it opens.
///
/// One cell of air, against the four between the columns: the key and the
/// words after it are one thing said twice, and a cell between them is
/// enough to keep the keys a column of their own. More than that and each
/// row reads as two things that happen to share a line.
///
/// And the cap's own blank where there is a cap, which is why this is two
/// in a window. `cap_around` lays its shape over the cell either side of
/// the key -- that is where a cap's blanks would have been -- so one cell
/// there is *all* cap and no air, and the mark after it sits against the
/// cap's edge. The foot has this written down already, beside `BETWEEN`:
/// the gap between two items has to beat the gaps inside one.
///
/// A terminal draws no cap here, so one cell there is one cell. Which is
/// the same question `drawn` asks below, for the same reason: this screen
/// is laid out against what the front end will actually put on it.
fn beside() -> u16 {
    match obelus_config::in_a_window() {
        true => 2,
        false => 1,
    }
}

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

/// What is set into the plate's own edge: the version.
///
/// In the frame rather than on a row of its own: it is a fact about the
/// thing the plate names, and a line under the plate holding one short
/// word is a row of screen spent on punctuation.
///
/// The commit it was built at was beside it and is not any more. What that
/// answered -- "is the fix in the thing I am looking at" -- is a question
/// somebody asks about a build they are chasing, not one the way in has to
/// carry, and the log says it on the first line of every run.
fn label(version: &str) -> String {
    format!("v{version}")
}

/// And a newer one beside it, where there is one.
///
/// Beside the version rather than on a row of its own, for the reason the
/// version is in the edge: it is a fact about the version.
fn news(newer: &str) -> String {
    format!("v{newer} is out")
}

/// The version and the news, as one run of text, and which characters of
/// it are the news.
fn said(version: &str, newer: Option<&str>) -> (String, std::ops::Range<usize>) {
    let version = label(version);
    match newer {
        Some(newer) => {
            let news = news(newer);
            let from = version.chars().count() + " \u{b7} ".chars().count();
            let to = from + news.chars().count();
            (format!("{version} \u{b7} {news}"), from..to)
        }
        None => (version, 0..0),
    }
}

/// The plate's foot with that set into it, and which columns of it are the
/// news.
///
/// Composed here rather than written into [`WORDMARK`], because it is not
/// Obelus's to spell: the version comes from the manifest, and a copy in a
/// string here is a copy that goes stale the moment it moves.
///
/// An edge with no room for it says nothing: what the plate is for is the
/// way in, and a frame broken open to fit a word into it is worse than a
/// frame that does not say one. With no room for the news it says the
/// version alone, which is what it said before there was any.
fn foot(version: &str, newer: Option<&str>) -> (String, std::ops::Range<usize>) {
    let Some(edge) = WORDMARK.last() else {
        return (String::new(), 0..0);
    };
    let width = edge.chars().count();
    let fits = |(said, news): (String, std::ops::Range<usize>)| {
        let set = format!(" {said} ");
        (width >= set.chars().count() + 4).then_some((set, news))
    };
    let set = newer
        .and_then(|newer| fits(said(version, Some(newer))))
        .or_else(|| fits(said(version, None)));
    let Some((set, news)) = set else {
        return ((*edge).to_string(), 0..0);
    };
    let taken = set.chars().count();
    let at = (width - taken) / 2;
    let foot = edge
        .chars()
        .take(at)
        .chain(set.chars())
        .chain(edge.chars().skip(at + taken))
        .collect();
    // One for the blank the run is set in.
    (foot, at + 1 + news.start..at + 1 + news.end)
}

/// The centred block.
pub struct WelcomeView<'a> {
    keymap: &'a Keymap,
    /// Which version this is.
    version: &'a str,
    /// The version of a newer Obelus, where one is out.
    newer: Option<&'a str>,
    theme: &'a Theme,
    /// How far the ramp has travelled, in ticks.
    ///
    /// Zero unless something is ticking, and nothing ticks once a file is
    /// open or when the session is remote, so this is the only thing that
    /// makes the screen differ between two draws.
    phase: u32,
    /// Where the pointer is, for the one thing here a press does.
    pointer: Option<(u16, u16)>,
    /// Whether nothing is over it, which is whether what is under the
    /// pointer may be raised -- see [`crate::in_front`].
    in_front: bool,
}

/// Where the wide layout puts things, worked out once for the drawing and
/// for a press alike.
struct Lavish {
    /// The plate's left edge.
    left: u16,
    /// The plate's top row.
    top: u16,
    /// The plate's width, which the keys are centred on.
    width: u16,
    /// The cells the website's address covers, where there is a row for it.
    site: Option<Rect>,
}

impl<'a> WelcomeView<'a> {
    /// Borrows what the view needs.
    #[must_use]
    pub fn new(app: &'a impl Screen) -> Self {
        Self {
            keymap: app.keymap(),
            version: app.version(),
            newer: app.newer_release(),
            theme: app.theme(),
            phase: app.phase(),
            pointer: app.pointer(),
            in_front: crate::in_front(app, None),
        }
    }
}

/// The cells the website's address covers, while it is on screen.
///
/// Asked by the drawing and by a press alike, so what is raised under the
/// pointer is exactly what a press there opens.
#[must_use]
pub fn site_at(area: Rect, app: &impl Screen) -> Option<Rect> {
    let view = WelcomeView::new(app);
    view.lavish_in(area, &view.hints())?.site
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
        let short = u16::try_from(hints.len() + 2).unwrap_or(u16::MAX);

        if let Some(lavish) = self.lavish_in(area, &hints) {
            self.lavish(cells, &hints, &lavish);
        } else if let Some(narrow) = hint_block_width(&hints)
            && narrow <= area.width
            && short <= area.height
        {
            self.compact(area, cells, &hints, narrow, short);
        }
    }
}

impl WelcomeView<'_> {
    /// Where the wide layout goes in this room, if it fits at all.
    ///
    /// The plate, a blank, and the keys two to a line with a blank between
    /// the lines -- and none after the last of them. Then a blank and the
    /// website's address, only where there are rows to spare: the keys are
    /// what this screen is for, and a room one row short of the address
    /// gets the plate without it rather than the narrow layout.
    fn lavish_in(&self, area: Rect, hints: &[Hint]) -> Option<Lavish> {
        let lines = u16::try_from(hints.len().div_ceil(COLUMNS)).unwrap_or(1);
        let keys = (lines * ROW_HEIGHT).saturating_sub(1);
        let tall = u16::try_from(WORDMARK.len()).unwrap_or(u16::MAX) + 1 + keys;
        let width = width_of(WORDMARK[0]);
        if width > area.width || tall > area.height {
            return None;
        }
        let label = u16::try_from(site_label().width()).unwrap_or(u16::MAX);
        let linked = tall + 2 <= area.height && label <= area.width;
        let height = match linked {
            true => tall + 2,
            false => tall,
        };
        let top = area.y + (area.height - height) / 2;
        Some(Lavish {
            left: area.x + (area.width - width) / 2,
            top,
            width,
            site: linked.then(|| Rect {
                x: area.x + (area.width - label) / 2,
                y: top + tall + 1,
                width: label,
                height: 1,
            }),
        })
    }

    /// The plate, the keys under it, and the address under them.
    fn lavish(&self, cells: &mut CellBuffer, hints: &[Hint], lavish: &Lavish) {
        // A ramp across the letters, in the theme's own accent hues rather
        // than in colours invented here, so it belongs to whichever theme is
        // on. The frame and the version in its foot are in it too: they are
        // part of the mark, and one still thing in a moving one reads as a
        // thing that has stopped.
        let mut y = self.plate(cells, lavish.left, lavish.top, lavish.width);

        y += 1;
        self.grid(cells, lavish.left, y, lavish.width, hints);
        if let Some(at) = lavish.site {
            self.site(cells, at);
        }
    }

    /// The website's address, underlined, and raised while the pointer is
    /// over it.
    ///
    /// Underlined because it is the one thing on this screen a press opens,
    /// and nothing else would say so before the pointer got there. The line
    /// is under the address and not under the blanks round it, which are
    /// only there for the raising.
    fn site(&self, cells: &mut CellBuffer, at: Rect) {
        let pointed = self.in_front
            && self
                .pointer
                .is_some_and(|(x, y)| at.contains(Position { x, y }));
        let (ink, ground) = match pointed {
            true => (self.theme.foreground, self.theme.raised_background),
            false => (self.theme.gutter, self.theme.background),
        };
        let label = site_label();
        write(cells, at.x, at.y, &label, Style::new().fg(ink).bg(ground));
        let said = label.trim();
        let inset = u16::try_from(label.len() - label.trim_start().len()).unwrap_or(0);
        write(
            cells,
            at.x + inset,
            at.y,
            said,
            Style::new()
                .fg(ink)
                .bg(ground)
                .add_modifier(Modifier::UNDERLINED),
        );
    }

    /// The wordmark, with the version set into its foot.
    ///
    /// Answers the row after it.
    fn plate(&self, cells: &mut CellBuffer, left: u16, top: u16, width: u16) -> u16 {
        // A ramp across the letters, in the theme's own accent hues rather
        // than in colours invented here, so it belongs to whichever theme is
        // on. The frame and the version in its foot are in it too: they are
        // part of the mark, and one still thing in a moving one reads as a
        // thing that has stopped.
        let from = self.theme.syntax.keyword;
        let to = self.theme.syntax.function;
        let (foot, news) = foot(self.version, self.newer);
        let mut y = top;
        // And the same two colours said as a shape, for a front end that
        // can draw a light rather than a ramp -- see `shapes::sheened`.
        // The whole plate, frame and foot and all, because that is what
        // the ramp runs across.
        crate::shapes::sheened(
            Rect {
                x: left,
                y,
                width,
                height: u16::try_from(WORDMARK.len()).unwrap_or(0),
            },
            from,
            to,
        );
        let last = WORDMARK.len().saturating_sub(1);
        for (at, row) in WORDMARK.iter().enumerate() {
            let row = if at == last { foot.as_str() } else { *row };
            let mut column = 0u16;
            for character in row.chars() {
                if character != ' ' {
                    let step = u32::from(column * RAMP_STEPS / width.max(1));
                    // The news in plain ink, still while the mark moves
                    // round it: it is not part of the mark, and a reader
                    // who has come to the plate to read it should not have
                    // to read it through a sheen.
                    let ink = match at == last && news.contains(&usize::from(column)) {
                        true => self.theme.foreground,
                        false => sheen(from, to, step, self.phase),
                    };
                    put(cells, left + column, y, character, Style::new().fg(ink));
                }
                column = column.saturating_add(1);
            }
            y += 1;
        }
        y
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
        // The version at the other end of the same row, which is where the
        // plate carries it when there is room for a plate. Nothing where it
        // does not fit: this layout is what a screen too small for the
        // plate gets, and the name is the part of it worth the room. The
        // news goes first where there is not room for both, for the same
        // reason the plate's edge drops it first.
        let room = usize::from(width).saturating_sub("Obelus".width() + 1);
        let fits = |(said, news): (String, std::ops::Range<usize>)| {
            (said.width() <= room).then_some((said, news))
        };
        if let Some((said, news)) = self
            .newer
            .and_then(|newer| fits(said(self.version, Some(newer))))
            .or_else(|| fits(said(self.version, None)))
            && let Ok(offset) = u16::try_from(usize::from(width).saturating_sub(said.width()))
        {
            for (at, character) in said.chars().enumerate() {
                let ink = match news.contains(&at) {
                    true => self.theme.foreground,
                    false => self.theme.gutter,
                };
                let Ok(at) = u16::try_from(at) else {
                    continue;
                };
                put(
                    cells,
                    left + offset + at,
                    y,
                    character,
                    Style::new().fg(ink),
                );
            }
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

        let mut x = left.saturating_add(width).saturating_add(beside());
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
///
/// Except in a window, where the face is not the reader's guess but the
/// `Mono` one carried in the binary, and a mark in it is fitted to the one
/// cell it was given. Asking for two there is a column a cell too wide,
/// which puts the key a cell left of where the column says and leaves the
/// gap on the other side -- where a cap drawn round it shows it.
fn drawn(contents: &str) -> usize {
    let glyph = match obelus_config::in_a_window() {
        true => 1,
        false => 2,
    };
    contents
        .chars()
        .map(|character| match character {
            '\u{e000}'..='\u{f8ff}' | '\u{f0000}'..='\u{ffffd}' => glyph,
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
    u16::try_from(keys_width(hints) + usize::from(beside()) + icon + text).ok()
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
