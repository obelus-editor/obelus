//! What Obelus shows when nothing is open.
//!
//! Two layouts. The wide one is a wordmark, the keys as caps, and a footer;
//! the narrow one is the keys and nothing else. A screen too small for even
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
//! **What went wrong on the way up is said here, under the keys and never in
//! front of them.** A mark on a line of a settings file is a mark nobody sees
//! until they open that file, and the reader who has just started Obelus has
//! opened nothing -- so this screen carries the same sentences again, below
//! the way in, because the way in is what this screen is for. It is absent on
//! almost every start, which is the point: a heading over an empty list is a
//! row of screen spent saying nothing happened.
//!
//! The rows are Obelus's own only: what a server says about the code is the
//! code's business and is not something that went wrong starting up. And the
//! block takes the keys, because most of what is on it is about a line of a
//! file the reader wrote and being told without being taken there is half an
//! answer: the arrows walk it, enter goes to the line, and the row the reader
//! is on carries `selected_row_background` like the row of every other list.
//!
//! Nothing without a place, though. A mark is a mark *on* something: a file
//! whose permissions forbid it, a watcher that would not start, a terminal
//! that would not report the wheel -- none of those has a line to draw
//! under. They go on this screen and nowhere else, which is why they are
//! kept apart from the marks rather than faked onto line one of something.

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

use crate::{Screen, fill, put, write};

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

/// How many columns of keys the plate carries under it.
const COLUMNS: usize = 2;

/// The gap between those columns.
const GUTTER: u16 = 4;

/// How many rows of screen one row of the grid takes.
///
/// Two: the keys are read one at a time, and a blank between them is what
/// makes a block of six read as six things rather than as a paragraph.
const ROW_HEIGHT: u16 = 2;

/// What the row that opens a project not in the list says.
///
/// A verb, because the row is an action among rows that are places. The
/// ellipsis is what a desktop means by "this opens somewhere to say more",
/// and here what it opens is the box at the foot.
///
/// Two of them, because "another" is a word about a list: on a machine
/// with nothing remembered the row is the whole of the screen, and there
/// is nothing for it to be another of.
const OPEN_A_PROJECT: &str = "Open a project…";
const OPEN_ANOTHER: &str = "Open another…";

/// What is over the projects.
///
/// Said because the list is not the only thing under the plate: the row
/// that opens one is below it, and without a word over the rows the two
/// read as one list in which the last entry happens to be a verb.
const RECENT: &str = "Recent projects";

/// What the list says when the filter has left none of them.
const NO_MATCH: &str = "No project matches";

/// How wide the list of projects is allowed to be.
///
/// Wider than the plate, as the block of what went wrong is: a path is
/// most of a row, and held to the plate's fifty columns it lost its head
/// on a terminal with seventy empty ones either side of it.
const RECENT_WIDTH: u16 = 64;

/// The least space between a path and the time beside it.
///
/// Two columns, so that the end of one and the start of the other are
/// never read as one word on a row where both reach their room. And the
/// same between a project's name and where it is.
const WHEN_GAP: u16 = 2;

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
fn label() -> &'static str {
    concat!("v", env!("CARGO_PKG_VERSION"))
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
fn said(newer: Option<&str>) -> (String, std::ops::Range<usize>) {
    let version = label();
    match newer {
        Some(newer) => {
            let news = news(newer);
            let from = version.chars().count() + " \u{b7} ".chars().count();
            let to = from + news.chars().count();
            (format!("{version} \u{b7} {news}"), from..to)
        }
        None => (version.to_string(), 0..0),
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
fn foot(newer: Option<&str>) -> (String, std::ops::Range<usize>) {
    let Some(edge) = WORDMARK.last() else {
        return (String::new(), 0..0);
    };
    let width = edge.chars().count();
    let fits = |(said, news): (String, std::ops::Range<usize>)| {
        let set = format!(" {said} ");
        (width >= set.chars().count() + 4).then_some((set, news))
    };
    let set = newer
        .and_then(|newer| fits(said(Some(newer))))
        .or_else(|| fits(said(None)));
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
    /// The version of a newer Obelus, where one is out.
    newer: Option<&'a str>,
    theme: &'a Theme,
    /// What is being asked, where Obelus has no project yet. The keys
    /// are not drawn then: none of them can do anything until this is
    /// answered, and a screen offering them would be offering nothing.
    choosing: Option<crate::Choosing>,
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
            newer: app.newer_release(),
            choosing: app.choosing(),
            theme: app.theme(),
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
        // Nothing else, where there is no project: every key the welcome
        // screen offers is about one, and the question has to be answered
        // before any of them means anything.
        if self.choosing.is_some() {
            self.asking(area, cells);
            return;
        }
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
        let y = area.y + (area.height - height) / 2;

        // A ramp across the letters, in the theme's own accent hues rather
        // than in colours invented here, so it belongs to whichever theme is
        // on. The frame and the version in its foot are in it too: they are
        // part of the mark, and one still thing in a moving one reads as a
        // thing that has stopped.
        let mut y = self.plate(cells, left, y, width);

        y += 1;
        self.grid(cells, left, y, width, hints);

        // Under the keys, where the caller said there was room for it.
        if amiss == Amiss::Shown && self.amiss_height() > 0 {
            let keys = (u16::try_from(hints.len().div_ceil(COLUMNS)).unwrap_or(1) * ROW_HEIGHT)
                .saturating_sub(1);
            self.amiss(cells, area, y + keys + 1);
        }
    }

    /// The wordmark, with the version set into its foot.
    ///
    /// Its own piece because two things sit under it now -- the keys on an
    /// ordinary start, the projects on one with nothing to go on -- and a
    /// mark drawn twice is a mark that drifts.
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
        let (foot, news) = foot(self.newer);
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

    /// The plate, and under it the projects this reader has had open.
    ///
    /// The projects under a heading, newest first, and below them the row
    /// that opens one that is not in the list -- which is always there, so
    /// a reader on a machine Obelus has never run on has exactly one row,
    /// and it is the one that gets them in. Then it is the whole of the
    /// block, with no heading over a list that is not there.
    fn asking(&self, area: Rect, cells: &mut CellBuffer) {
        let Some(choosing) = &self.choosing else {
            return;
        };
        let plate = width_of(WORDMARK[0]);
        let plate_height = u16::try_from(WORDMARK.len()).unwrap_or(0);
        let headed = headed(choosing);
        // The heading and a blank over the projects, and a blank and the
        // opening row under them; or that row alone.
        let around = match headed {
            true => 4,
            false => 1,
        };
        // Every project, or the one line saying the filter left none.
        let listed = match headed {
            true => u16::try_from(choosing.known.len().max(1)).unwrap_or(u16::MAX),
            false => 0,
        };
        let least = around + listed.min(1);
        let width = RECENT_WIDTH.max(plate).min(area.width);
        let left = area.x + (area.width - width) / 2;
        let (top, room) = if plate <= area.width && plate_height + 1 + least <= area.height {
            let room = listed.min(area.height - plate_height - 1 - around);
            let height = plate_height + 1 + around + room;
            let y = area.y + (area.height - height) / 2;
            let plate_left = area.x + (area.width - plate) / 2;
            (self.plate(cells, plate_left, y, plate) + 1, room)
        } else {
            // No room for the mark, so the rows are the whole of it: what
            // this screen is for is getting in, and the wordmark is the
            // part that can go.
            let room = listed.min(area.height.saturating_sub(around));
            let height = (around + room).min(area.height);
            (area.y + (area.height - height) / 2, room)
        };
        self.rows(
            cells,
            choosing,
            Rect {
                x: left,
                y: top,
                width,
                height: area.bottom().saturating_sub(top),
            },
            room,
        );
    }

    /// The heading, the projects and the opening row.
    fn rows(
        &self,
        cells: &mut CellBuffer,
        choosing: &crate::Choosing,
        // Where the rows go, as one thing: four numbers passed beside
        // each other are four numbers that can be passed in the wrong
        // order.
        list: Rect,
        // How many of the projects there is room for.
        room: u16,
    ) {
        let (left, width) = (list.x, list.width);
        let total = choosing.known.len();
        let selected = Style::new().bg(self.theme.selected_row_background);
        // One mark for "the keys are here", and it says nothing else: the
        // same colour behind the row the reader is on that every list,
        // page and card in Obelus puts there.
        let row_style = |cells: &mut CellBuffer, y: u16, on: bool| match on {
            true => {
                fill(
                    cells,
                    Rect {
                        x: left,
                        y,
                        width,
                        height: 1,
                    },
                    selected,
                );
                selected
            }
            false => Style::new(),
        };
        let mut y = list.y;
        let headed = headed(choosing);
        if headed {
            write(
                cells,
                left + 2,
                y,
                RECENT,
                Style::new().fg(self.theme.syntax.function),
            );
            y += 2;
            if total == 0 {
                crate::nothing(
                    cells,
                    Rect {
                        x: left + 1,
                        y,
                        width,
                        height: 1,
                    },
                    NO_MATCH,
                    self.theme,
                );
                y += 1;
            } else {
                // Which of them are on screen: the reader's own row among
                // them, and the ones nearest the opening row while the
                // reader is on that.
                let focus = choosing.at.min(total - 1);
                let room = usize::from(room);
                let first = match room < total {
                    false => 0,
                    true => focus
                        .saturating_sub(room.saturating_sub(1))
                        .min(total - room),
                };
                let names = self.name_width(choosing, width);
                for (at, opened) in choosing.known.iter().enumerate().skip(first).take(room) {
                    if y >= list.bottom() {
                        return;
                    }
                    let style = row_style(cells, y, at == choosing.at);
                    self.row(cells, left, y, width, names, opened, style);
                    y += 1;
                }
            }
            y += 1;
        }
        if y >= list.bottom() {
            return;
        }
        let style = row_style(cells, y, choosing.at >= total);
        let said = match headed {
            true => OPEN_ANOTHER,
            false => OPEN_A_PROJECT,
        };
        write(cells, left + 2, y, said, style.fg(self.theme.foreground));
    }

    /// How wide the column of names is: the widest of them, and never
    /// more than half the row, so that where each one is keeps room to
    /// say it.
    ///
    /// Taken across every project and not only those on screen, so the
    /// column does not move under the reader as the list scrolls.
    fn name_width(&self, choosing: &crate::Choosing, width: u16) -> u16 {
        let widest = choosing
            .known
            .iter()
            .map(|opened| obelus_text::text_width(name_of(&opened.path).1))
            .max()
            .unwrap_or(0);
        u16::try_from(widest)
            .unwrap_or(u16::MAX)
            .min(width.saturating_sub(4) / 2)
    }

    /// One project's row: its name, where it is, and when it was open.
    ///
    /// **The name first, and in the ink.** What tells two projects apart is
    /// the last part of the path, and a column of whole paths put that at
    /// the ragged end of each row, behind a head every row shared. So the
    /// name is a column of its own and where it is follows, dim.
    ///
    /// **The time is given its room before the path gets any.** A row
    /// works out what it says about itself first and the words get what
    /// is left -- so a path long enough to reach the right-hand edge
    /// cannot take the tail with it. And what is cut off a path is its
    /// *head*, for the reason the name comes first: the end is the part
    /// that says which.
    #[allow(clippy::too_many_arguments)]
    fn row(
        &self,
        cells: &mut CellBuffer,
        left: u16,
        y: u16,
        width: u16,
        names: u16,
        opened: &crate::Opened,
        style: Style,
    ) {
        let right = left + width.saturating_sub(2);
        let when = u16::try_from(obelus_text::text_width(&opened.when)).unwrap_or(0);
        let marked = |skip| crate::Marked {
            matched: match opened.matched {
                Some((first, end)) => crate::Matched::Run(first, end),
                None => crate::Matched::Nothing,
            },
            mark: self.theme.picker_match_background,
            syntax: None,
            skip,
        };
        let (start, name) = name_of(&opened.path);

        // The name, counted from the start of the whole path so that what
        // the filter matched lands on the right letters -- the one writer
        // for it, so this list cannot be the newest one that forgot.
        let x = left + 2;
        let ink = style.fg(self.theme.foreground);
        let long = obelus_text::text_width(name) > usize::from(names);
        let room = match long {
            // A cell for the mark that says it was cut.
            true => names.saturating_sub(1),
            false => names,
        };
        let row = |x: u16, width: u16| Rect {
            x,
            y,
            width,
            height: 1,
        };
        let end = crate::write_marked(cells, row(x, room), x, y, &opened.path, ink, &marked(start));
        if long {
            write(cells, end, y, "\u{2026}", ink);
        }

        // Where it is, in what is left between the name and the time.
        let dim = style.fg(self.theme.gutter);
        let from = x + names + WHEN_GAP;
        let stop = match when {
            0 => right,
            _ => right.saturating_sub(when + WHEN_GAP),
        };
        let place: String = opened.path.chars().take(place_length(start)).collect();
        let room = stop.saturating_sub(from);
        // A lone mark saying something was cut says nothing about where.
        if room > 1 {
            let dropped = crate::drop_from_left(&place, usize::from(room));
            let at = match dropped {
                0 => from,
                _ => write(cells, from, y, "\u{2026}", dim),
            };
            crate::write_marked(
                cells,
                row(at, stop - at),
                at,
                y,
                &place,
                dim,
                &marked(dropped),
            );
        }

        if when > 0 {
            write(cells, right.saturating_sub(when), y, &opened.when, dim);
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
            .and_then(|newer| fits(said(Some(newer))))
            .or_else(|| fits(said(None)))
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

/// Whether the projects are under a heading.
///
/// Where there are any, and where a filter has left none: a reader who has
/// narrowed the list to nothing is told so where the list was, rather than
/// having it vanish and the opening row change its words. Not on a machine
/// with nothing remembered, where a heading would be over an empty list.
fn headed(choosing: &crate::Choosing) -> bool {
    !choosing.known.is_empty() || (!choosing.naming && !choosing.typed.is_empty())
}

/// A project's name -- the last part of the path a row shows -- and how
/// many characters of that path come before it.
///
/// Either separator, because a path on Windows may be written with both.
fn name_of(path: &str) -> (usize, &str) {
    match path.rfind(std::path::is_separator) {
        Some(at) if at + 1 < path.len() => (path[..=at].chars().count(), &path[at + 1..]),
        _ => (0, path),
    }
}

/// How many characters of a path say where its project is, given how many
/// come before the name: all of them but the separator, unless that
/// separator is all there is -- a project at the root is somewhere.
fn place_length(start: usize) -> usize {
    match start {
        0 | 1 => start,
        _ => start - 1,
    }
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
