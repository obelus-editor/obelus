//! The conversation with an agent.
//!
//! Four bands: who is being talked to, the transcript, the box a message is
//! written in, and a status row of the conversation's own. Rules between
//! them, because a screen holding four different things has to say where
//! each one stops.
//!
//! Not the editor's own view and not a buffer: there are no line numbers,
//! no file, and the only caret is in the box.
//!
//! The box grows with what is written, up to a few rows, and the transcript
//! gives up the room -- a reader writing a paragraph is looking at the
//! paragraph, and one reading an answer has not started typing.
//!
//! The transcript reads from the bottom, like every other transcript: new
//! rows arrive at the end and the view follows them unless the reader has
//! scrolled up to read something.
//!
//! A header says what a thing *is*; the foot of the transcript says what is
//! happening. The conversation's header carried five states, and they were
//! the wrong five: two said what the screen already said better ("nobody is
//! chosen", beside a header already reading "no agent" and over a transcript
//! already saying where to fix it), one said "not started yet" about an agent
//! that had *failed* to start, and the two that were real -- starting, thinking
//! -- belong where the next thing will appear, because that is where the reader
//! is looking. So the header is the name, and nothing else.
//!
//! What is happening is one row at the foot of the transcript, worked out from
//! the state every frame rather than written into the transcript. A state has
//! no history: the next one replaces it and Ready removes it, and what is not
//! stored cannot be left on screen saying something that has stopped being
//! true. What *went wrong* is the opposite and stays a line in the transcript
//! where it went wrong -- an agent that died, and why, is the thing a reader
//! needs next, and a row that overwrote it would take away the only record.
//! `esc stops it` rides on the row that says something is going, beside the
//! thing it would stop.

use std::path::Path;

use obelus_agent::{Talking, acp};
use obelus_component::{
    card::Card,
    chat::{Chat, DEEPER, Focus, Row, Speaker},
};
use obelus_text::text_width;
use obelus_theme::Theme;
use ratatui::{buffer::Buffer as CellBuffer, layout::Rect, style::Style, widgets::Widget};

use crate::{Screen, fill, put, rule, write, write_within};

/// How far a speaker's mark is from the edge.
///
/// One, which is what the header and the status bar use: a column of glyphs
/// touching the edge of the screen reads as a gutter rather than as who is
/// speaking.
const MARGIN: u16 = 1;

/// How far the words are from the mark.
///
/// Three: a glyph, the column a Nerd Font's glyph bleeds into, and one to
/// read by.
const INDENT: u16 = 3;

/// Who is speaking, for a terminal with no Nerd Font.
///
/// Distinct marks rather than a colour each: colours say it as well, and a
/// reader who has turned the glyphs off has a terminal that may be saying
/// them badly too.
fn mark(speaker: Speaker) -> &'static str {
    match speaker {
        Speaker::Reader => ">",
        Speaker::Agent => "*",
        Speaker::Thought => "~",
        Speaker::Tool => "+",
        Speaker::Note => "!",
        Speaker::Away => "^",
        Speaker::Doing => "\u{2026}",
        // Nothing: a step wears how far along it is instead, in the column
        // a speaker's mark would have been.
        Speaker::Step => " ",
    }
}

/// What a tool call's state says while it is still running.
///
/// The protocol's own word, named because three places in this file have
/// to agree about it: the mark at the front of a row, which turns while a
/// call is in this state; the mark after the title, which stays away while
/// it is; and [`ChatView::state_of`], which says what each of the four
/// states looks like. Three spellings of one string is two chances for the
/// row to say a call is running and still at once.
const UNDER_WAY: &str = "in_progress";

/// The rows that are there whatever is written: the header and two rules.
///
/// The box needs no rule under it -- the screen keeps one between whatever
/// is showing and the status bar, and that one is directly under the box.
const FIXED: u16 = 3;

/// The most rows the box takes, however much is written in it.
///
/// Six. A message longer than that scrolls inside the box, because the
/// transcript is what the reader came for and a box that ate the screen
/// would be a text editor with an agent attached.
const MOST_WRITING: u16 = 6;

/// The bands the conversation is drawn in.
///
/// One function, shared by the view, the scrolling and the keys, so that a
/// page of movement is the page on screen and the caret is in the box the
/// reader can see.
///
/// The status row is not among them. While the conversation is what the
/// screen is showing, Obelus's own status row is the conversation's -- one
/// row at the foot of the screen, which is where a status row goes. A band
/// of its own inside the region would be a second status bar with the real
/// one under it, and the pair reads as one bar two rows tall.
#[derive(Clone, Copy, Debug)]
pub struct Regions {
    /// Who is being talked to, and what they are doing.
    pub header: Rect,
    /// What has been said.
    pub transcript: Rect,
    /// What is being written.
    pub writing: Rect,
}

/// The cells a row of the box has to write in.
///
/// Known from the region alone, which is what lets the number of rows the
/// box needs be worked out before the bands are laid out.
#[must_use]
pub fn writing_width(area: Rect) -> u16 {
    area.width.saturating_sub(MARGIN + INDENT + 1).max(1)
}

/// The cells a row of the transcript has to write in.
///
/// Which is what its rows are wrapped to, and so what decides which row is
/// which -- the keys need it as much as the drawing does, because a cursor
/// in the transcript stands on a row.
#[must_use]
pub fn reading_width(area: Rect) -> u16 {
    area.width.saturating_sub(MARGIN + INDENT + 1).max(1)
}

/// The room a list opened over the conversation has.
///
/// Everything above whatever is at the foot of it, and the rule over that.
/// A compact list draws against the foot of what it is given, and the foot
/// of the whole region is what is being written -- which for the list of
/// commands is the one row that must stay visible, because the list is a
/// list of what is being typed there.
///
/// Through [`bands`], which is the one answer to where that boundary is.
/// This worked the box's rows out for itself, and a card is taller than a
/// box: a list over a conversation waiting on an answer was given rows the
/// card was already drawn in and painted over the top of it, which is the
/// half of a card that says what is being asked.
#[must_use]
pub fn above_writing(area: Rect, chat: &Chat, card: Option<&Card>) -> Rect {
    let writing = bands(area, chat, card).writing;
    Rect {
        height: writing.y.saturating_sub(area.y + 1),
        ..area
    }
}

/// The fewest rows of what has been said the reader is left with.
///
/// A card an agent's question is on takes what it needs from the foot of
/// the region, and what it needs can be most of it: a question with eight
/// answers and room to write your own is a tall thing. This is where it
/// stops -- below this the transcript would be gone, and the question
/// would be on screen with nothing saying what it came out of.
const LEAST_SAID: u16 = 3;

/// How many rows a card may take from the foot of the region.
///
/// More than the box gets, because the two are different promises: the box
/// is a few lines to type in, and a card is a question that has to be
/// readable or it cannot be answered.
const fn most_for_a_card(area: Rect) -> u16 {
    area.height.saturating_sub(FIXED + LEAST_SAID)
}

/// The bands, with whatever is at the foot of the region: the box a message
/// is written in, or the card an agent's question is answered on.
///
/// One function so the view, the caret and the keys cannot disagree about
/// where the boundary is.
#[must_use]
pub fn bands(area: Rect, chat: &Chat, card: Option<&Card>) -> Regions {
    match card {
        Some(card) => bands_for(area, card),
        None => regions(area, chat.writing().rows(writing_width(area)).len()),
    }
}

/// The bands with a card at the foot of them.
///
/// Measured against the width a card's rows have, which is not the width the
/// box has: the box is written in under an icon, and a card is a region of
/// its own with a margin.
#[must_use]
pub fn bands_for(area: Rect, card: &Card) -> Regions {
    let rows = card.rows(super::card::width_of(area));
    regions_capped(area, rows, most_for_a_card(area))
}

/// Where the four bands go, given how many rows the box needs.
#[must_use]
pub fn regions(area: Rect, needed: usize) -> Regions {
    regions_capped(area, needed, MOST_WRITING)
}

/// The blanks between a key and the word for what it does.
const GAP_IN_A_HINT: usize = 2;
/// And between one of those and the next.
///
/// Wider than the gap inside one, for the reason the foot's `BETWEEN` is:
/// the gap between items has to beat the gaps inside one, or the row is a
/// line of tokens with nothing saying which belongs to which.
const GAP_BETWEEN_HINTS: usize = 3;

/// Where each of those keys starts in that run, in cells from its
/// beginning.
///
/// A function of its own rather than a sum kept while drawing, because it
/// is the one place the two shapes of this row have to agree: the run is
/// what a terminal writes, and a cap is said about cells *of* it. A walk
/// that counted the gaps differently from `joined` would draw a cap over
/// the word beside the key.
fn where_the_keys_are(hints: &[(String, &'static str)]) -> Vec<usize> {
    let mut along = 0;
    hints
        .iter()
        .map(|(keys, does)| {
            let at = along;
            along += text_width(keys) + GAP_IN_A_HINT + text_width(does) + GAP_BETWEEN_HINTS;
            at
        })
        .collect()
}

/// A row of keys as one run of text, which is what is written and
/// measured.
fn joined(hints: &[(String, &'static str)]) -> Option<String> {
    match hints.is_empty() {
        true => None,
        false => Some(
            hints
                .iter()
                .map(|(keys, does)| format!("{keys}{}{does}", " ".repeat(GAP_IN_A_HINT)))
                .collect::<Vec<_>>()
                .join(&" ".repeat(GAP_BETWEEN_HINTS)),
        ),
    }
}

/// The same, for a foot of the region with a cap of its own.
#[must_use]
pub fn regions_capped(area: Rect, needed: usize, most: u16) -> Regions {
    let row = |y: u16, height: u16| Rect {
        x: area.x,
        y,
        width: area.width,
        height,
    };
    let fixed = FIXED;
    let most = most.max(1);
    let writing = u16::try_from(needed)
        .unwrap_or(most)
        .clamp(1, most)
        .min(area.height.saturating_sub(fixed).max(1));
    let transcript = area.height.saturating_sub(fixed + writing);
    let top = area.y;
    Regions {
        header: row(top, 1),
        transcript: row(top + 2, transcript),
        writing: row(top + 3 + transcript, writing),
    }
}

/// What a tool call says about where it was working, given what its title
/// has already said.
///
/// The path as a reader writes it -- relative to the project Obelus was opened
/// on -- and the line when the agent named one. And how many other files it
/// named, because a call that touched six of them says so on its one row
/// until the reader opens it.
///
/// Unless the title is already that path. An agent's title for a call is
/// usually the verb and the file -- `Write crates/obelus-reading/src/
/// blocks.rs` -- and Obelus writes the path after it because the path is
/// the affordance: a row that names a file is a row that goes there. Both,
/// and the row said one thing twice and ran off the edge of the screen,
/// taking the mark that says there is more behind it and the count of what
/// it changes with it. The same rule as the one a setting's description
/// follows when it is the setting's name again.
///
/// What the title has *not* said is still worth saying, so a line and the
/// other files are what is left: `line 20  +2` where the title named the
/// file, `src/app.rs:20  +2` where it did not.
fn said_place(title: &str, place: &acp::Place, root: &Path, more: usize) -> String {
    let path = super::relative_to(&place.path, root).display().to_string();
    // As the reader writes it, or as the agent wrote it: a title is the
    // agent's own words and it may have put the whole path in.
    let named = title.contains(&path) || title.contains(&place.path.display().to_string());
    let said = match (named, place.line) {
        (true, None) => String::new(),
        (true, Some(line)) => format!("line {line}"),
        (false, None) => path,
        (false, Some(line)) => format!("{path}:{line}"),
    };
    match (more, said.is_empty()) {
        (0, _) => said,
        (more, true) => format!("+{more}"),
        (more, false) => format!("{said}  +{more}"),
    }
}

/// How full the agent's memory is, and whether that is worth saying
/// loudly.
///
/// A proportion rather than the tokens themselves: what a reader does with
/// this is decide whether to start again, and "94%" answers that where
/// "187432 of 200000" is two numbers to divide first. The cost goes with it
/// when the agent counts one -- not every agent does, and one that does not
/// should not leave a gap where a number was.
///
/// Nothing at all until the agent has said how much room there is. A
/// proportion of nothing is not a number, and a bare token count is the
/// thing this deliberately does not show.
fn used_up(usage: &acp::Usage) -> Option<(String, bool)> {
    if usage.room == 0 {
        return None;
    }
    // Rounded down, so it says 99% until it really is full: a conversation
    // reported as 100% that still has room would send a reader to start
    // another one for nothing.
    let part = usage.used.saturating_mul(100) / usage.room;
    let mut said = format!("{part}%");
    if let Some(cost) = &usage.cost {
        // The code rather than a sign, and after the amount: Obelus knows
        // no currency's sign, and a guessed one is a number about the wrong
        // money.
        said.push_str(&format!("{SEPARATOR}{:.2} {}", cost.amount, cost.currency));
    }
    Some((said, part >= NEARLY_FULL))
}

/// The proportion at which how full the agent is stops being furniture.
const NEARLY_FULL: u64 = 90;

/// How much room is left between what the settings say and what comes
/// after it on the row.
const GAP: usize = 3;

/// The least the settings are worth showing in: a word and the mark that
/// says it was cut.
const LEAST_SETTINGS: usize = 4;

/// What goes between two things the status row says.
const SEPARATOR: &str = " \u{b7} ";

/// How many cells that takes.
const SEPARATOR_WIDTH: usize = 3;

/// The bar a line of a change carries, which is the one an opened hunk
/// carries in a file.
const BAR: char = '\u{2590}';

/// Which mark a row that folds something carries.
fn opens(open: bool) -> String {
    format!(" {}", crate::opens(open))
}

/// What says the row holds more than it had room to draw.
const MORE: &str = "\u{2026}";

/// What one setting says on the row, and the tick in front of it if it is
/// a switch.
fn said(setting: &acp::Setting) -> (String, Option<bool>) {
    match setting.kind {
        // The value, which names itself.
        acp::Kind::Select => (
            setting
                .current_name()
                .unwrap_or(&setting.current)
                .to_string(),
            None,
        ),
        // The name, because "on" is not a thing to be told -- the box in
        // front of it is.
        acp::Kind::Switch => (setting.name.clone(), Some(setting.current == "on")),
    }
}

/// How many cells one setting takes on the row, its tick and all.
fn said_width((word, tick): &(String, Option<bool>)) -> usize {
    text_width(word) + tick.map_or(0, |_| usize::from(crate::TICK_WIDTH))
}

/// The conversation, over the whole editor region.
pub struct ChatView<'a> {
    chat: &'a Chat,
    theme: &'a Theme,
    /// What Obelus is doing about an agent.
    state: Talking,
    /// What to call it.
    name: Option<&'a str>,
    /// Everything about the session the agent lets the reader change, in
    /// the agent's own order.
    settings: &'a [acp::Setting],
    /// Which of the two things on this screen the keys are moving.
    focus: Focus,
    /// The card an agent's question is on, while it is waiting on one.
    card: Option<&'a Card>,
    /// The project Obelus was opened on, for writing the paths an agent names
    /// the way a reader writes them.
    root: &'a Path,
    /// Where the animation has got to, for the row that turns.
    phase: u32,
    /// The branch the agent is changing files on, once it has changed one.
    ///
    /// Not the note it is about, which the header used to carry: the note
    /// is what the reader opened the conversation from, and the first
    /// thing said in it. Where the work is going is not on the page
    /// anywhere, and an agent told to work on a branch of its own is out
    /// of sight of the status row, which says the reader's.
    branch: Option<&'a obelus_git::Head>,
    /// Whether it is about a note that is still there, for the key back to
    /// it.
    about_a_note: bool,
    /// What Obelus has to say, until the next key.
    ///
    /// A conversation has a status row of its own, so it has to carry this
    /// too: without it every command that answers by saying something --
    /// there is no history for this, there was nothing to close -- would
    /// press its key and get silence back.
    note: Option<&'a str>,
    /// And whether it is about something that would not go, which is the
    /// ink it is drawn in -- see `StatusView::wrong_ink`.
    note_is_wrong: bool,
    /// How full the agent's memory of this conversation is, once it has
    /// said.
    usage: Option<&'a acp::Usage>,
}

impl<'a> ChatView<'a> {
    /// Borrows what the view needs, or nothing if the conversation is not
    /// what the region is showing.
    #[must_use]
    pub fn new(app: &'a impl Screen) -> Option<Self> {
        Some(Self {
            chat: app.chat()?,
            theme: app.theme(),
            state: app.talking(),
            name: app.agent_name(),
            settings: app.agent_settings(),
            focus: app.chat()?.focus(),
            card: app.card(),
            root: app.working_directory(),
            phase: app.phase(),
            branch: app.branch_this_conversation_works_on(),
            about_a_note: app.is_about_a_note(),
            note: app.note(),
            note_is_wrong: app.note_is_wrong(),
            usage: app.agent_usage(),
        })
    }

    /// Where in the transcript a point on the screen is, if it is in one
    /// at all.
    ///
    /// A place in what was said rather than the row and column it was
    /// pointed at with, because that is what a selection keeps -- and this
    /// is the only piece of Obelus that knows both, the rows having just
    /// been laid out at the width the screen has.
    ///
    /// A point in a row the reading drew rather than read -- a blank, the
    /// heading over a folded run, the row that says what is happening now
    /// -- is the place just after the last word above it. A drag has to go
    /// somewhere while it crosses one, and the words either side of it are
    /// what the reader is dragging between.
    #[must_use]
    pub fn place_in_transcript(
        area: Rect,
        chat: &Chat,
        card: Option<&Card>,
        x: u16,
        y: u16,
    ) -> Option<obelus_component::chat::Spot> {
        if x < area.x || x >= area.right() {
            return None;
        }
        let at = Self::row_in_transcript(area, chat, card, y)?;
        let rows = chat.rows(reading_width(area));
        let row = rows.get(at)?;
        // Cells to characters here, characters to a place in the words
        // there: a cell is this drawing's own business -- a wide glyph is
        // two of them and an indent is several -- and what a character of a
        // row came from is the row's. Two halves of one seam, so that the
        // pointer and the cursor cross it by the same arithmetic.
        row.spot_at(characters_at(row, x, area))
    }

    /// Which row of the transcript a point on screen is on.
    ///
    /// The row rather than the place in the words: what is under the
    /// pointer is a row, and whether that row is a heading to open or words
    /// to take hold of is a question about the row. One piece of code, so
    /// that the two cannot land on different rows.
    ///
    /// `None` for a point outside the transcript's own band, or past the
    /// end of what has been said.
    #[must_use]
    pub fn row_in_transcript(
        area: Rect,
        chat: &Chat,
        card: Option<&Card>,
        y: u16,
    ) -> Option<usize> {
        let band = bands(area, chat, card).transcript;
        if y < band.y || y >= band.bottom() {
            return None;
        }
        let at = chat.top() + usize::from(y - band.y);
        (at < chat.rows(reading_width(area)).len()).then_some(at)
    }

    /// Where the terminal should put its caret: in the box, or in the
    /// transcript while the reader is reading it.
    #[must_use]
    pub fn caret(
        area: Rect,
        chat: &Chat,
        card: Option<&Card>,
    ) -> Option<ratatui::layout::Position> {
        // In the card, while one is up: it is what covers the box, and
        // what the reader is typing in is the half of it that takes words.
        if let Some(card) = card {
            return super::card::caret(area, card);
        }
        match chat.focus() {
            Focus::Writing => {}
            // In the transcript, where the cursor is. What it is on is a
            // reading rather than something to type in, so what the caret
            // says there is "you are here" and nothing about where words
            // would go -- which is the same thing a caret says in a file
            // Obelus is reading.
            Focus::Transcript(place) => return in_transcript(area, chat, card, place),
            // Nowhere, while the keys are walking the row of settings: a
            // caret left blinking in the box would say that what is typed
            // goes there, and it does not. What says where the keys are
            // going is the selected background on the row, the same as in
            // every list.
            Focus::Settings(_) => return None,
        }
        let width = writing_width(area);
        let rows = chat.writing().rows(width);
        let regions = regions(area, rows.len());
        let (row, cell) = chat.writing().caret(width);
        // A box scrolled to keep the caret in it: what is drawn starts at
        // the same row the caret arithmetic starts at.
        let first = row.saturating_sub(usize::from(regions.writing.height).saturating_sub(1));
        let y = regions.writing.y + u16::try_from(row - first).unwrap_or(0);
        (y < regions.writing.bottom()).then(|| ratatui::layout::Position {
            x: (regions.writing.x + MARGIN + INDENT + cell.get())
                .min(regions.writing.right().saturating_sub(1)),
            y,
        })
    }

    /// Which row and cell of the box a point on screen is.
    ///
    /// The inverse of [`ChatView::caret`], and beside it because they are
    /// one geometry: the box is drawn from these numbers and the caret is
    /// put from them, so where a click lands has to come from them too.
    ///
    /// `None` for a point outside the box -- the transcript, the row of
    /// settings, a card over it. Only the box has a caret in it.
    ///
    /// `carded` rather than the card itself: what matters is that one
    /// covers the box, not which one, and asking for the card would mean
    /// holding it while the box it is over is changed.
    #[must_use]
    pub fn place_at(area: Rect, chat: &Chat, carded: bool, x: u16, y: u16) -> Option<(u16, u16)> {
        // A card covers the box, and what is typed goes into the card.
        if carded {
            return None;
        }
        let width = writing_width(area);
        let rows = chat.writing().rows(width);
        let regions = regions(area, rows.len());
        let box_x = regions.writing.x + MARGIN + INDENT;
        if y < regions.writing.y
            || y >= regions.writing.bottom()
            || x < box_x
            || x >= regions.writing.right()
        {
            return None;
        }
        // The same scrolling the caret is placed under: a box taller than
        // its band shows its last rows, so the row on screen counts from
        // there rather than from the first row of the text.
        let (caret_row, _) = chat.writing().caret(width);
        let first = caret_row.saturating_sub(usize::from(regions.writing.height).saturating_sub(1));
        let row = first + usize::from(y - regions.writing.y);
        Some((u16::try_from(row).unwrap_or(u16::MAX), x - box_x))
    }
}

/// Where the caret goes while the cursor is in the transcript.
///
/// Nothing where the row it is on has been scrolled off the screen: the
/// cursor is still there and comes back when the reader scrolls back, but a
/// caret drawn at the edge of a band it is not in would be pointing at
/// somebody else's row.
fn in_transcript(
    area: Rect,
    chat: &Chat,
    card: Option<&Card>,
    place: obelus_component::chat::Place,
) -> Option<ratatui::layout::Position> {
    let band = bands(area, chat, card).transcript;
    let rows = chat.rows(reading_width(area));
    let row = rows.get(place.row)?;
    let offset = place.row.checked_sub(chat.top())?;
    let y = band.y + u16::try_from(offset).ok()?;
    if y >= band.bottom() {
        return None;
    }
    Some(ratatui::layout::Position {
        x: cell_at(row, place.character, area).min(words_end(area)),
        y,
    })
}

/// The last cell a row's own words can be drawn in.
///
/// Not the last cell of the band. The column past the words belongs to the
/// scrollbar -- [`reading_width`] leaves it out on purpose, so that a
/// transcript saying how much of it there is costs the words nothing -- and
/// a caret put there is a caret nobody can see, sitting under the bar.
///
/// Which is where it went. A row is wrapped to the words' full width, so
/// the place after its last character is one past the last column the words
/// have; on a row that fills the width, that is the scrollbar's column
/// exactly. The reader pressed up, the caret went into the transcript, and
/// as far as they could tell it had gone out.
///
/// So the end of a full row draws on its last character rather than after
/// it. There is no cell after it to draw in, and the reader is at the end
/// of that row either way.
const fn words_end(area: Rect) -> u16 {
    // The scrollbar's column, and then the last one before it.
    area.right().saturating_sub(2)
}

/// Which character of a row a cell of the screen is on.
///
/// The drawing's own half of the seam: a wide glyph is two cells and the
/// mark and indent in front of the words are several, so what column a
/// character of a row is drawn at is known here and nowhere else. Past the
/// end of the row is the row's length, which is the place after its last
/// character.
fn characters_at(row: &Row, x: u16, area: Rect) -> usize {
    let mut column = words_begin(row, area);
    let mut seen = 0usize;
    for span in &row.spans {
        for character in span.text.chars() {
            if x < column + wide(character) {
                return seen;
            }
            column += wide(character);
            seen += 1;
        }
    }
    seen
}

/// The cell a character of a row is drawn at.
///
/// The way back across the seam [`characters_at`] crosses the other way,
/// for putting the terminal's caret where the cursor is. Past the end of
/// the row is the cell after its last character, which is where a caret at
/// the end of a line belongs.
fn cell_at(row: &Row, characters: usize, area: Rect) -> u16 {
    let mut column = words_begin(row, area);
    let mut seen = 0usize;
    for span in &row.spans {
        for character in span.text.chars() {
            if seen == characters {
                return column;
            }
            column += wide(character);
            seen += 1;
        }
    }
    column
}

/// The cell a row's own words start at.
fn words_begin(row: &Row, area: Rect) -> u16 {
    area.x + MARGIN + INDENT + u16::from(row.depth) * DEEPER
}

/// How many cells a character takes, never fewer than one.
fn wide(character: char) -> u16 {
    u16::try_from(text_width(&character.to_string()))
        .unwrap_or(1)
        .max(1)
}

impl Widget for ChatView<'_> {
    fn render(self, area: Rect, cells: &mut CellBuffer) {
        let plain = Style::new()
            .fg(self.theme.foreground)
            .bg(self.theme.background);
        let dim = plain.fg(self.theme.gutter);
        fill(cells, area, plain);
        // Three bands and two rules do not fit in less than that, and a
        // region this small is a terminal nobody is reading in.
        if area.height < 5 || area.width < 20 {
            return;
        }

        let width = writing_width(area);
        // Laid rather than just the strings, because what the reader has
        // hold of is marked on to these rows and the box has to draw it:
        // a selection nothing shows is a key that looks broken.
        let rows = self.chat.writing().laid(width);
        let regions = bands(area, self.chat, self.card);

        self.header(cells, regions.header, plain, dim);
        for y in [regions.header.bottom(), regions.writing.y - 1] {
            rule(
                cells,
                Rect {
                    y,
                    height: 1,
                    ..area
                },
                self.theme,
            );
        }

        self.transcript(cells, regions.transcript, plain, dim);
        // The way back, on the rule over the box, while the reader is not
        // at the end.
        //
        // On the rule because the rule is the one row on this screen that
        // carries nothing: a label there covers no word of the
        // conversation, and it sits exactly on the boundary between what
        // was said and what is being written, which is the boundary the
        // reader has scrolled away from.
        //
        // The key is spelled out rather than taken from the key table,
        // which is where every other hint in Obelus gets its spelling.
        // This one cannot be: `ctrl+end` is on the list of chords the
        // table refuses, because the editor takes it before the table is
        // reached -- so the one key Obelus will not let a reader rebind is
        // the one it has to name here.
        self.the_way_back(cells, regions.writing.y - 1, area);
        // The card where the box would be: while the agent is waiting on
        // an answer there is no message to send, so the row the reader
        // would type it in is the room the question needs.
        match self.card {
            Some(card) => super::card::draw(cells, regions.writing, card, self.theme),
            None => self.writing(cells, regions.writing, &rows, plain, dim),
        }
    }
}

impl ChatView<'_> {
    /// Says how to get back to the end, where the reader has left it.
    ///
    /// And what has arrived since, where the agent has said anything: the
    /// question somebody who scrolled up actually has is whether it has
    /// answered them yet, and the scrollbar beside them can only say how
    /// much there is -- never whether any of it is new.
    fn the_way_back(&self, cells: &mut CellBuffer, y: u16, area: Rect) {
        if self.chat.at_the_end() {
            return;
        }
        let said = match self.chat.said_since() {
            0 => "To the end".to_string(),
            1 => "1 new message".to_string(),
            many => format!("{many} new messages"),
        };
        let label = format!("  {said}  ctrl+end \u{2193}  ");
        let width = text_width(&label);
        let Ok(width) = u16::try_from(width) else {
            return;
        };
        if width >= area.width {
            return;
        }
        // Centred, which is where a thing that belongs to the whole width
        // goes -- and where the eye is already, the box being under it.
        let x = area.x + (area.width - width) / 2;
        write(
            cells,
            x,
            y,
            &label,
            Style::new()
                .fg(self.theme.foreground)
                .bg(self.theme.background),
        );
    }

    /// What has been said, and the commands being completed over it.
    fn transcript(&self, cells: &mut CellBuffer, area: Rect, plain: Style, dim: Style) {
        let words = area.x + MARGIN + INDENT;
        let rows = self.chat.rows(reading_width(area));
        if rows.is_empty() {
            write(cells, words, area.y, self.nothing_said(), dim);
        }
        // The same bar everything else that scrolls has, in the same
        // column: a transcript that scrolled and said nothing about it was
        // the one scrolling thing in Obelus with no answer to "how much of
        // this is there". The column is already spare -- the rows are
        // wrapped to leave it -- so nothing moves to make room.
        let bar = self
            .chat
            .scrollable(area.height)
            .then(|| crate::scrollbar(cells, area, self.chat.top(), rows.len(), self.theme))
            .flatten();
        // And where it has got to, for a front end that can draw it
        // arriving rather than simply being there -- the same thing every
        // other list says, and the bar beside it moves its own share of
        // the same distance.
        if let Ok(top) = i64::try_from(self.chat.top()) {
            crate::shapes::scrolled(
                Rect {
                    width: area.width.saturating_sub(crate::editor::SCROLLBAR_WIDTH),
                    ..area
                },
                top,
                bar,
            );
        }
        let first = self.chat.top().min(rows.len());
        for (offset, row) in rows.iter().skip(first).enumerate() {
            let Ok(offset) = u16::try_from(offset) else {
                break;
            };
            if offset >= area.height {
                break;
            }
            let y = area.y + offset;
            // The members of an opened run are drawn in from their heading,
            // so that a run reads as one thing rather than as a stretch of
            // rows that happen to look alike.
            let words = words + u16::from(row.depth) * DEEPER;
            // The row the cursor is on, lit the way every list in Obelus
            // lights one -- but only where the row does something, because
            // that is what the light promises: what is lit is what enter
            // opens.
            //
            // Where the cursor is is said by the caret instead. The cursor
            // can stand anywhere now, so a light that followed it would be
            // a promise kept on one row in twenty; two marks saying two
            // different things is the honest way round.
            let at = first + usize::from(offset);
            let here =
                row.acts() && matches!(self.focus, Focus::Transcript(place) if place.row == at);
            let (glyph, style) = self.voice(row, plain, dim);
            // What has not gone yet is said in the ink: the reader's own
            // words, dim, until the turn in front of them ends. Not by
            // taking the background away -- that mark says where the keys
            // are and says nothing else.
            //
            // Asked of what the row was said from, not of `row.unsent`:
            // that is on the first row only, where the key that takes it
            // back stands, and a message long enough to wrap was dim for
            // one row and in the reader's colour for the rest.
            let waiting = row.from.is_some_and(|(at, _)| self.chat.waits(at));
            let style = match waiting {
                true => dim,
                false => style,
            };
            // A line of a change is drawn the way an opened hunk is drawn
            // in a file: tinted its whole width, with the marker's own bar
            // against the text. The same two colours, because it is the
            // same thing being said.
            let (style, dim) = match row.marker {
                Some(marker) => {
                    let tint = plain.bg(self.theme.marker_background(marker));
                    (tint, tint.fg(self.theme.gutter))
                }
                None => (style, dim),
            };
            let (style, dim) = match here {
                true => (
                    style.bg(self.theme.selected_row_background),
                    dim.bg(self.theme.selected_row_background),
                ),
                false => (style, dim),
            };
            if here {
                fill(
                    cells,
                    Rect {
                        y,
                        height: 1,
                        // Up to the words' last column and no further: a
                        // fill blanks what it covers, so a row tinted to
                        // the edge of the band rubs out the scrollbar.
                        width: (words_end(area) + 1).saturating_sub(area.x),
                        ..area
                    },
                    style,
                );
            }
            // The tint runs to the edge, as it does behind an opened hunk
            // in a file: a block of colour that stopped where the words
            // stop would be ragged down its right side, and the block is
            // what says these lines are a change rather than a quotation.
            if row.marker.is_some() {
                let from = words.saturating_sub(1);
                fill(
                    cells,
                    Rect {
                        x: from,
                        y,
                        width: (words_end(area) + 1).saturating_sub(from),
                        height: 1,
                    },
                    style,
                );
            }
            if let Some(marker) = row.marker {
                put(
                    cells,
                    words.saturating_sub(1),
                    y,
                    BAR,
                    style.fg(self.theme.marker_colour(marker)),
                );
            }
            // A step of the agent's list for this turn wears how far along
            // it is in front of itself, which is where a tool call's state
            // deliberately does not go. The reasons are the same reason: a
            // reader scans a tool call for its title and the state changes
            // under them, and scans a list of steps for the states, because
            // what they are reading it for is how far along it is.
            if row.speaker == Speaker::Step
                && let Some(state) = &row.state
            {
                let at = area.x + MARGIN + u16::from(row.depth) * DEEPER;
                let (_, said, style) = self.state_said(state, dim);
                write_within(cells, at, y, &said, style, words_end(area) + 1);
            }
            if row.first {
                let at = area.x + MARGIN + u16::from(row.depth) * DEEPER;
                // The row that says something is happening turns, and it
                // turns whether or not glyphs are drawn: a picture of a
                // cog says a tool was used, and only movement says it is
                // still going.
                //
                // Two rows say it. The one at the foot is about the turn,
                // and a tool call that is still running is about one thing
                // in it -- the same claim at two sizes, so they are said
                // with the same mark, on the same frame, in the same
                // colour. A reader watching a call that takes a minute
                // should not have to find the foot of the transcript to
                // learn that it has not stalled, and a still picture of a
                // cog is what a call that stopped would wear too.
                //
                // In the column the kind glyph has, so that nothing moves.
                // A column of its own in front -- which is how a list of
                // open documents and the notes both mark a conversation --
                // would push every tool call's title two cells right of
                // every other row's words, and the left edge of a
                // transcript is one column for everything in it. So the
                // kind goes while the call runs and comes back when it
                // ends: what sort of call it is is written along the row
                // beside it, and which of ten calls is the live one is
                // written nowhere else.
                let turning =
                    row.speaker == Speaker::Doing || row.state.as_deref() == Some(UNDER_WAY);
                if turning {
                    put(
                        cells,
                        at,
                        y,
                        crate::spinning(self.phase),
                        style.fg(self.theme.gutter_current),
                    );
                } else if obelus_icons::enabled() {
                    put(cells, at, y, glyph, style);
                } else {
                    write(cells, at, y, mark(row.speaker), style);
                }
            }
            // The runs, in the colours the markdown said they are, over
            // the style the row is drawn in: the voice's colour is what a
            // run with no opinion of its own keeps, and the background --
            // a selected row, a line of a change -- is the row's either
            // way. Clipped at the edge like every other row here.
            // What the row says about itself goes after its words, and the
            // room for it is taken off them first. It used to be written
            // from wherever the words happened to stop, so words that
            // reached the edge of the screen took all of it with them --
            // and an agent's title for a call is the agent's own text,
            // which for a command it ran is the whole command line. A
            // hundred columns of `grep` left the row with nothing on it
            // saying the call could be opened, or how it went, or how much
            // it changed: the one row about a rewritten file said nothing
            // about the rewriting, and read as a line Obelus had lost the
            // end of.
            let tail = self.tail_of(row, dim, here);
            let kept: usize = tail
                .iter()
                .map(|(gap, said, _)| usize::from(*gap) + text_width(said))
                .sum();
            let stop = (words_end(area) + 1).saturating_sub(u16::try_from(kept).unwrap_or(0));
            // Whether they fit, asked of the words rather than of where the
            // drawing got to: a row that ends exactly at the edge has not
            // lost anything and must not be marked as though it had.
            let wanted: usize = row.spans.iter().map(|span| text_width(&span.text)).sum();
            let clipped = wanted > usize::from(stop.saturating_sub(words));
            let mut ended = crate::reading::write_spans(
                cells,
                words,
                y,
                &row.spans,
                &crate::reading::Drawn {
                    base: style,
                    theme: self.theme,
                    // The one answer to where a row's words end, which
                    // `words_end` gives the caret as well: the column past
                    // them belongs to the scrollbar.
                    stop: match clipped {
                        true => stop.saturating_sub(1),
                        false => stop,
                    },
                    held: row.held.as_ref(),
                },
            );
            // Said where they stop, rather than simply running out: a row
            // that ends mid-word at the edge of the screen reads as the
            // terminal having cut it off, not as there being more.
            if clipped {
                ended = write_within(cells, stop.saturating_sub(1), y, "\u{2026}", dim, stop);
            }
            for (gap, said, style) in tail {
                ended = write_within(cells, ended + gap, y, &said, style, words_end(area) + 1);
            }
            // How to stop it, on the row that says it is going: the one
            // thing escape does here that a reader could not guess, and it
            // belongs beside the thing it would stop.
            if row.speaker == Speaker::Doing && self.state == Talking::Thinking {
                let hint = "Esc stops it";
                if let Ok(offset) =
                    u16::try_from(usize::from(area.width).saturating_sub(text_width(hint) + 1))
                    && area.x + offset > ended + 1
                {
                    write(cells, area.x + offset, y, hint, dim);
                }
            }
        }
    }

    /// The box, with the caret's own row scrolled into it.
    fn writing(
        &self,
        cells: &mut CellBuffer,
        area: Rect,
        rows: &[obelus_component::composer::Laid],
        plain: Style,
        dim: Style,
    ) {
        let height = usize::from(area.height);
        let (caret, _) = self.chat.writing().caret(writing_width(area));
        // The last rows, or the ones the caret is on: a box that is being
        // typed into shows where the typing is.
        let first = caret.saturating_sub(height.saturating_sub(1));
        for (offset, row) in rows.iter().skip(first).take(height).enumerate() {
            let Ok(offset) = u16::try_from(offset) else {
                break;
            };
            let y = area.y + offset;
            if offset == 0 && first == 0 {
                let _ = match obelus_icons::enabled() {
                    true => put(cells, area.x + MARGIN, y, obelus_icons::ui::SAY, dim),
                    false => write(cells, area.x + MARGIN, y, ">", dim),
                };
            }
            let x = area.x + MARGIN + INDENT;
            write(cells, x, y, &row.said, plain);
            // Behind the caret rather than in front of it, in the gutter's
            // ink like the keys at the foot: it is not what was written,
            // and the caret sitting at its start says where typing goes.
            if let Some(said) = self.chat.suggestion() {
                write(cells, x, y, said, plain.fg(self.theme.gutter));
            }
            // And what the reader has hold of, over the top -- the colour
            // the file uses for the same fact, because it is the same
            // fact. Counted in characters of the row, which is what the
            // box counts a hold in.
            if let Some(held) = &row.held {
                let mut column = x;
                let mut buffer = [0u8; 4];
                for (at, character) in row.said.chars().enumerate() {
                    let drawn = character.encode_utf8(&mut buffer);
                    let wide = u16::try_from(text_width(drawn)).unwrap_or(1);
                    if held.contains(&at) {
                        write(
                            cells,
                            column,
                            y,
                            drawn,
                            plain.bg(self.theme.selection_background),
                        );
                    }
                    column += wide;
                }
            }
        }
    }

    /// The status row, which while a conversation is showing is the
    /// conversation's: which way of working the agent is in, and how to
    /// change it.
    ///
    /// Drawn into Obelus's own status region rather than into a row of the
    /// conversation's, so that there is one status bar on the screen and it
    /// is at the foot of it.
    pub fn status(&self, cells: &mut CellBuffer, area: Rect) {
        // The page's own colour, like every other status row: the rule
        // above it has already said the row is a different subject from
        // what is above it.
        let plain = Style::new()
            .bg(self.theme.background)
            .fg(self.theme.foreground);
        fill(cells, area, plain);

        // What a key does here, on the right. Two of them at most, and the
        // way back comes first: a conversation about a note is reached from
        // the notes page, and a way out that nothing says exists is the same
        // gap one level up -- which is why the key was added at all.
        //
        // Measured first, because the room the settings have is what is left
        // of the row.
        let keys = self.status_keys();
        let hint = joined(&keys);
        if let Some(hint) = &hint
            && let Ok(offset) =
                u16::try_from(usize::from(area.width).saturating_sub(text_width(hint) + 1))
        {
            let at = area.x + offset;
            write(cells, at, area.y, hint, plain.fg(self.theme.gutter));
            // And which cells of that run are the key. Nothing is written
            // twice: the run above is the whole of what a terminal draws,
            // and this says what shape a window may draw round part of it.
            for (along, (keys, _)) in where_the_keys_are(&keys).into_iter().zip(&keys) {
                if let Ok(x) = u16::try_from(usize::from(at) + along) {
                    crate::cap_around(
                        x,
                        area.y,
                        keys,
                        text_width(keys),
                        self.theme.background,
                        self.theme.background,
                        self.theme.gutter,
                    );
                }
            }
        }

        // How full the agent's memory is, beside the hints rather than
        // beside the settings. The settings scroll along this row to keep
        // the focused one on screen, and a number that slid about with
        // them would be a number the reader has to find again every time
        // they step one. This end does not move.
        let taken = hint.as_deref().map_or(0, |hint| text_width(hint) + 2);
        // And only where all three fit. The settings say what the session
        // is set to and the keys say what they do; this is a number, and a
        // row narrow enough to have to choose has not lost much by losing
        // it -- where a row that kept it had the settings cut to a letter
        // and the keys pushed off the end.
        let over = usize::from(area.width).saturating_sub(taken + 2);
        let used = self
            .usage
            .and_then(used_up)
            .filter(|(said, _)| over > text_width(said) + GAP + LEAST_SETTINGS);
        if let Some((said, full)) = &used
            && let Ok(offset) =
                u16::try_from(usize::from(area.width).saturating_sub(taken + text_width(said) + 1))
        {
            // Dim like the hints for as long as it is only a number. Once
            // the agent is nearly out of room it is the one thing on this
            // row a reader has to act on -- a conversation to start again,
            // a note to write down before it is forgotten -- so it stops
            // being furniture.
            //
            // A colour rather than a brighter grey: this row already says
            // three things in three greys -- the value a setting is on, the
            // ones it is not, the keys -- and a fourth would be one more
            // shade to tell apart rather than a thing that stands out.
            let ink = match full {
                true => self.theme.status_stale,
                false => self.theme.gutter,
            };
            write(cells, area.x + offset, area.y, said, plain.fg(ink));
        }

        let room = over.saturating_sub(used.as_ref().map_or(0, |(said, _)| text_width(said) + GAP));
        // A note over the settings, for as long as it lasts. The settings
        // are what the session is set to and are still true a moment later;
        // a note is the answer to the key just pressed, and an answer that
        // waits its turn is an answer nobody reads.
        if let Some(note) = self.note {
            // In the red everything that would not go is written in, the
            // same as the status row's: the words are the same in either
            // place and a reader glancing at the row has nothing else to
            // tell a refusal from a report.
            let ink = match self.note_is_wrong {
                true => plain.fg(self
                    .theme
                    .colour_for(Some(obelus_text::kind::SyntaxKind::Error))),
                false => plain,
            };
            write(
                cells,
                area.x + 1,
                area.y,
                &super::truncate_from_right(note, room),
                ink,
            );
            return;
        }
        self.settings(cells, area, self.status_room(area), plain);
    }

    /// The mode, if the agent offers one.
    fn mode(&self) -> Option<&acp::Setting> {
        self.settings
            .iter()
            .find(|setting| setting.category == acp::Category::Mode)
    }

    /// What the session is set to, along the row: every setting the agent
    /// offers, in its own order, each said as shortly as it can be said.
    ///
    /// A list of values rather than of names and values: what a select is on
    /// names itself -- `gpt-5` is plainly a model and `careful` is plainly a
    /// way of working -- so the name would be a label on something already
    /// labelled. A switch is the other way round: `on` says nothing, and the
    /// thing it is about is its name, so that is what is written, behind
    /// the box every switch in Obelus is drawn as.
    fn settings(&self, cells: &mut CellBuffer, area: Rect, room: usize, plain: Style) {
        if self.settings.is_empty() {
            // Only once there is a session: before that the row would be
            // saying that an agent which has not spoken yet has nothing to
            // say about itself.
            if matches!(self.state, Talking::Ready | Talking::Thinking) {
                write(
                    cells,
                    area.x + 1,
                    area.y,
                    "Nothing to change",
                    plain.fg(self.theme.gutter),
                );
            }
            return;
        }

        // What each one says, and what it takes to say it. The focused one
        // carries the arrow Obelus puts on everything with a list behind
        // it, so it is wider than the others by exactly that.
        let chosen = match self.focus {
            Focus::Settings(at) => Some(at.min(self.settings.len() - 1)),
            Focus::Transcript(_) | Focus::Writing => None,
        };
        let words = self.setting_words();
        let first = self.first_setting(&words, room);

        let mut column = area.x + 1;
        // What was cut off the front, which is a setting the reader can
        // still walk back to.
        if first > 0 {
            column = write(cells, column, area.y, MORE, plain.fg(self.theme.gutter));
        }
        let (placed, cut) = Self::settings_placed(&words, first, column, room);
        for (index, x, _) in placed {
            let (word, tick) = &words[index];
            let separated = index > first || first > 0;
            if separated {
                write(
                    cells,
                    x.saturating_sub(u16::try_from(SEPARATOR_WIDTH).unwrap_or(0)),
                    area.y,
                    SEPARATOR,
                    plain.fg(self.theme.gutter),
                );
            }
            let column = x;
            // The focused one wears the background a selected row wears in
            // every list, which while the reader is up here is the only
            // thing on screen saying where the keys are going -- the caret
            // is put away for exactly as long.
            let ground = match Some(index) == chosen {
                true => self.theme.selected_row_background,
                false => self.theme.background,
            };
            let ink = match tick {
                // A switch that is off: there, and plainly not in force --
                // the colour a row nobody can choose is drawn in, under a
                // box that already says which.
                Some(false) => self.theme.gutter,
                Some(true) | None => self.theme.gutter_current,
            };
            let style = plain.fg(ink).bg(ground);
            let column = match tick {
                Some(on) => {
                    // The blank the glyph spills into is the setting's
                    // too, so the focused one's ground runs unbroken.
                    write(cells, column, area.y, "  ", style);
                    crate::ticked(cells, column, area.y, *on, style)
                }
                None => column,
            };
            write(cells, column, area.y, word, style);
        }
        // No room for the next one: the row says so rather than stopping
        // silently, because a reader who cannot see a setting cannot know
        // it is there to walk to.
        if let Some(x) = cut {
            write(cells, x, area.y, MORE, plain.fg(self.theme.gutter));
        }
    }

    /// The settings as words, with the tick in front of each switch.
    ///
    /// The focused one carries the arrow Obelus puts on everything with a
    /// list behind it, so it is wider than the others by exactly that --
    /// which is why the words are made before anything measures them.
    fn setting_words(&self) -> Vec<(String, Option<bool>)> {
        let chosen = self.chosen_setting();
        self.settings
            .iter()
            .enumerate()
            .map(|(index, setting)| {
                let (mut word, tick) = said(setting);
                if Some(index) == chosen && setting.kind == acp::Kind::Select {
                    word.push_str(&opens(false));
                }
                (word, tick)
            })
            .collect()
    }

    /// Which setting the keys are on, if they are up here at all.
    fn chosen_setting(&self) -> Option<usize> {
        match self.focus {
            Focus::Settings(at) => Some(at.min(self.settings.len().saturating_sub(1))),
            Focus::Transcript(_) | Focus::Writing => None,
        }
    }

    /// Which setting the row starts at.
    ///
    /// As near the beginning as having the focused one on screen allows. A
    /// row is a window on a list like any other, and the one thing a window
    /// must not do is hide what the keys are moving.
    fn first_setting(&self, words: &[(String, Option<bool>)], room: usize) -> usize {
        let Some(chosen) = self.chosen_setting() else {
            return 0;
        };
        let mut taken = 0;
        for index in (0..=chosen).rev() {
            taken += said_width(&words[index]) + SEPARATOR_WIDTH;
            if taken > room {
                return index + 1;
            }
        }
        0
    }

    /// What a key does on the status row, on the right.
    ///
    /// The way back comes first: a conversation about a note is reached
    /// from the notes page, and a way out that nothing says exists is the
    /// same gap one level up -- which is why the key was added at all.
    /// Then the mode, which is the rightmost thing on this row and always
    /// has been: a key that moved when a conversation gained a setting
    /// would be a key the reader has to look for.
    ///
    /// Not the key to the other conversations, which was here while it
    /// meant something in a conversation that it meant nowhere else. It
    /// means the same everywhere now, as `f1` does, and no view spends its
    /// row on those.
    ///
    /// Asked before the settings are drawn, because the room they have is
    /// what is left of the row once this is on it.
    fn status_hint(&self) -> Option<String> {
        joined(&self.status_keys())
    }

    /// The same row, kept as the key and what it does rather than as one
    /// run of text.
    ///
    /// Two shapes of one thing, because the row is measured and written as
    /// text and the cap round each key is about *where the key is*: a
    /// front end that draws the shape has to be told which cells of that
    /// run are the key, and a run that had already been joined cannot say.
    fn status_keys(&self) -> Vec<(String, &'static str)> {
        let back = self.about_a_note.then(|| {
            let keys = match obelus_icons::enabled() {
                true => format!("{}t", obelus_icons::key::ALT),
                false => "alt+t".to_string(),
            };
            (keys, "The note")
        });
        let mode = self
            .mode()
            .is_some_and(|mode| mode.values.len() > 1)
            .then(|| {
                let keys = match obelus_icons::enabled() {
                    true => format!("{}{}", obelus_icons::key::SHIFT, obelus_icons::key::TAB),
                    false => "shift+tab".to_string(),
                };
                (keys, "Mode")
            });
        // The arrow, while there is something for it to put in the box:
        // nothing on the box says that grey words can be taken, and they
        // are taken by a key that does nothing on an empty box anywhere
        // else.
        let suggested = self.chat.suggestion().map(|_| {
            let keys = obelus_editing::keymap::KeyChord::new(
                crossterm::event::KeyCode::Right,
                crossterm::event::KeyModifiers::NONE,
            );
            (keys.label(), "Fill it in")
        });
        [suggested, back, mode].into_iter().flatten().collect()
    }

    /// How much of the status row the settings have.
    ///
    /// What is left after the keys at one end and the agent's memory at the
    /// other, both of which stay put. Asked by the drawing and by a press,
    /// because a press has to be measured against the row that is there.
    fn status_room(&self, area: Rect) -> usize {
        let hint = self.status_hint();
        let taken = hint.as_deref().map_or(0, |hint| text_width(hint) + 2);
        let over = usize::from(area.width).saturating_sub(taken + 2);
        let used = self
            .usage
            .and_then(used_up)
            .filter(|(said, _)| over > text_width(said) + GAP + LEAST_SETTINGS);
        over.saturating_sub(used.as_ref().map_or(0, |(said, _)| text_width(said) + GAP))
    }

    /// Which of the agent's settings a point on the status row is on.
    ///
    /// `None` for a point that is not on one of them: the keys at the end
    /// of the row, the memory beside them, the marks that say there are
    /// more in either direction.
    #[must_use]
    pub fn setting_at(&self, area: Rect, x: u16, y: u16) -> Option<usize> {
        if y != area.y || self.settings.is_empty() {
            return None;
        }
        let room = self.status_room(area);
        let words = self.setting_words();
        let first = self.first_setting(&words, room);
        let mut column = area.x + 1;
        if first > 0 {
            column = column.saturating_add(u16::try_from(text_width(MORE)).unwrap_or(0));
        }
        let (placed, _) = Self::settings_placed(&words, first, column, room);
        placed
            .into_iter()
            .find(|(_, at, wide)| x >= *at && x < at.saturating_add(*wide))
            .map(|(index, _, _)| index)
    }

    /// Where each of the agent's settings is drawn along the status row,
    /// and where the mark for "there are more" goes if one did not fit.
    ///
    /// One walk, because the row is a window on a list: the drawing goes
    /// down it to put the words, and a press goes down it to find which
    /// word it landed on. Two walks would be two answers about where a
    /// setting is.
    ///
    /// Each is `(which setting, the cell its word starts at, how wide the
    /// word is)`. The separator before a word is in the cells just behind
    /// it, which is why the walk hands back where the word starts rather
    /// than where its room does.
    pub(crate) fn settings_placed(
        words: &[(String, Option<bool>)],
        first: usize,
        from: u16,
        room: usize,
    ) -> (Vec<(usize, u16, u16)>, Option<u16>) {
        let mut placed = Vec::new();
        let mut column = from;
        let mut left = match first > 0 {
            true => room.saturating_sub(text_width(MORE)),
            false => room,
        };
        for (index, word) in words.iter().enumerate().skip(first) {
            let separated = index > first || first > 0;
            let separator = usize::from(separated) * SEPARATOR_WIDTH;
            let wide = said_width(word);
            if wide + separator > left {
                return (placed, Some(column));
            }
            left -= wide + separator;
            column = column.saturating_add(u16::try_from(separator).unwrap_or(0));
            placed.push((index, column, u16::try_from(wide).unwrap_or(0)));
            column = column.saturating_add(u16::try_from(wide).unwrap_or(0));
        }
        (placed, None)
    }

    /// Who is being talked to, and where its work is going.
    ///
    /// A header says what the thing it names *is*, which for an agent is its
    /// name -- and, once it has changed something, the branch the change
    /// is on. What is *happening* goes at the foot of the transcript,
    /// where the next thing will appear; what went wrong is a line in the
    /// transcript where it went wrong. Five states used to sit here, two of
    /// them saying what the screen already said better and one of them
    /// saying "not started yet" about an agent that had failed to start.
    fn header(&self, cells: &mut CellBuffer, area: Rect, plain: Style, dim: Style) {
        let mut column = area.x + MARGIN;
        if obelus_icons::enabled() {
            put(cells, column, area.y, obelus_icons::ui::AGENT, dim);
            column += INDENT;
        }
        let name = self.name.unwrap_or("No agent");
        column = write(
            cells,
            column,
            area.y,
            name,
            plain.fg(self.theme.gutter_current),
        );
        // The branch, dimmed after it, in the badge the status row gives
        // the reader's own. Dropped whole where it does not fit, the rule
        // that row follows: half a branch name is worse than none.
        let branch = crate::status::branch_badge(self.branch);
        let wide = 2 + text_width(branch.trim_end());
        if branch.is_empty() || usize::from(column) + wide > usize::from(area.x + area.width) {
            return;
        }
        column = write(cells, column, area.y, "  ", dim);
        write(cells, column, area.y, branch.trim_end(), dim);
    }

    /// The glyph and colour one speaker's rows are drawn in.
    fn voice(&self, row: &Row, plain: Style, dim: Style) -> (char, Style) {
        match row.speaker {
            // The reader's own words in the brighter colour: a transcript is
            // read looking for where you asked something.
            Speaker::Reader => (
                obelus_icons::ui::READER,
                plain.fg(self.theme.gutter_current),
            ),
            Speaker::Agent => (obelus_icons::ui::AGENT, plain),
            // Thinking is not the answer, and a transcript that draws them
            // alike is a transcript a reader has to sort out themselves.
            Speaker::Thought => (obelus_icons::ui::THOUGHT, dim),
            // What sort of tool, where the agent said: reading a file and
            // rewriting one are not the same news, and the glyph is where a
            // reader scanning a turn takes that in.
            Speaker::Tool => (obelus_icons::for_tool(&row.kind), dim),
            Speaker::Note => (obelus_icons::ui::NOTE, dim),
            // Somewhere the reader was sent. Dim like a tool call, because
            // it is the same kind of row: a thing under way, with a mark on
            // the end saying whether it still is.
            Speaker::Away => (obelus_icons::ui::AWAY, dim),
            // The glyph a tool call carries while it is running, for the
            // same reason: this is the row that says something is under
            // way.
            Speaker::Doing => (obelus_icons::ui::RUNNING, dim.fg(self.theme.gutter_current)),
            // Its status is its mark, drawn where a speaker's would be, so
            // there is no glyph of its own to give it.
            Speaker::Step => (' ', dim),
        }
    }

    /// What a row says about itself, after its words.
    ///
    /// The gap before each piece, the piece, and the colour it is drawn in.
    /// One list rather than four writes in a row, because the room these
    /// need has to be known *before* the words are drawn and what is drawn
    /// has to be the same thing that was measured. Two answers to "what
    /// goes at the end of this row" is how the end of a row goes missing.
    fn tail_of(&self, row: &Row, dim: Style, standing: bool) -> Vec<(u16, String, Style)> {
        let mut tail = Vec::new();
        // What enter does here, on the row it would do it to: the one row
        // in a transcript whose key hands something back rather than
        // opening it, so a reader has no way to guess it. Only while they
        // are standing on it -- said on every waiting row at once it would
        // be answering somebody who has not asked yet.
        if row.unsent.is_some() && standing {
            tail.push((2, "Enter takes it back".to_string(), dim));
        }
        // Where it said it was working. The path is its own affordance:
        // Obelus opens files, so a row that names one is a row that goes
        // there.
        if let Some((place, more)) = &row.place {
            let said = said_place(&row.text(), place, self.root, *more);
            if !said.is_empty() {
                tail.push((2, said, dim));
            }
        }
        // What says there is more behind this row than it is showing: the
        // same mark a settings row and a card use for the same promise,
        // turned down when what it holds is open.
        if row.folds.is_some() {
            tail.push((1, opens(row.open).to_string(), dim));
        }
        // How much it changes, which is what a reader reads first: the
        // shape of the change before any of its lines.
        if let Some((added, removed)) = row.changed {
            tail.push((2, format!("+{added} \u{2212}{removed}"), dim));
        }
        // A tool call's state goes after its title rather than in front of
        // it: the title is what a reader is scanning, and the state changes
        // under them twice.
        //
        // Every state but the one the front of the row is already saying.
        // Said in both places it was said worse: the still glyph out here
        // is the one a reader's eye lands on -- it sits where the sentence
        // ends -- and a still glyph is what a call that has stopped wears.
        // What is left is a row whose front says whether it is alive and
        // whose end says how it went.
        if let Some(state) = &row.state
            && row.speaker != Speaker::Step
            && state != UNDER_WAY
        {
            tail.push(self.state_said(state, dim));
        }
        tail
    }

    /// How far a tool call has got: the gap before it, what to write, and
    /// the colour.
    fn state_said(&self, state: &str, dim: Style) -> (u16, String, Style) {
        let (glyph, word, colour) = match state {
            "pending" => (obelus_icons::ui::WAITING, "Waiting", self.theme.gutter),
            UNDER_WAY => (
                obelus_icons::ui::RUNNING,
                "Running",
                self.theme.gutter_current,
            ),
            "completed" => (obelus_icons::ui::DONE, "Done", self.theme.gutter),
            // The word the transcript already uses for the turn this call
            // was in, because it is the same fact said about a smaller
            // thing.
            "cancelled" => (obelus_icons::ui::STAYING, "Stopped", self.theme.gutter),
            "failed" => (
                obelus_icons::ui::BROKEN,
                "Failed",
                self.theme.syntax.constant,
            ),
            other => (obelus_icons::ui::WAITING, other, self.theme.gutter),
        };
        let style = dim.fg(colour);
        // A glyph where there are glyphs, the word where there are not:
        // both are written the same way, so the room worked out for one is
        // the room the other takes.
        let said = match obelus_icons::enabled() {
            true => glyph.to_string(),
            false => word.to_string(),
        };
        (1, said, style)
    }

    /// What to say when nothing has been said yet, in words.
    ///
    /// Which is not one message: a reader who has chosen no agent has
    /// something to do about it, and a reader whose agent is starting has
    /// only to wait.
    fn nothing_said(&self) -> &'static str {
        match self.state {
            Talking::Nobody => "No agent is active \u{2014} open the settings and choose one",
            Talking::Gone => "It stopped. Ask something to start it again",
            Talking::Starting => "Starting\u{2026}",
            // Idle belongs here rather than with starting: nothing is on
            // its way -- the frame that asks for a session has not been
            // drawn, or no agent could be started for it -- and
            // "starting..." under a still mark would say something is on
            // its way to a page nothing is coming to.
            Talking::Idle | Talking::Ready | Talking::Thinking => "Ask it something",
        }
    }
}

#[cfg(test)]
mod tests {
    /// Break: count the gap between two hints as the gap inside one, and
    /// every cap after the first is drawn a cell or two to the left of the
    /// key it is about.
    #[test]
    fn a_cap_on_the_status_row_is_over_the_key_it_is_about() {
        let hints = [
            ("alt+t".to_string(), "The note"),
            ("ctrl+g".to_string(), "Conversations"),
            ("shift+tab".to_string(), "Mode"),
        ];
        let said = super::joined(&hints).expect("three hints say something");
        for (along, (keys, _)) in super::where_the_keys_are(&hints).into_iter().zip(&hints) {
            assert_eq!(
                said.get(along..along + keys.len()),
                Some(keys.as_str()),
                "{keys} is not at {along} of {said:?}"
            );
        }
    }

    use std::path::{Path, PathBuf};

    use super::{Speaker, mark, said_place};

    /// Where a tool call was, written the way a reader writes a path.
    ///
    /// Relative to the project Obelus was opened on, because that is the part
    /// already known -- and left alone when it is somewhere else, because a
    /// path outside the tree is news. The others it named are counted
    /// rather than listed: the row is one row until the reader opens it.
    #[test]
    fn a_tool_call_says_where_it_was_the_way_a_reader_writes_it() {
        let root = Path::new("/tree");
        let inside = obelus_agent::acp::Place {
            path: PathBuf::from("/tree/src/app.rs"),
            line: Some(20),
        };
        assert_eq!(
            said_place("Read the file", &inside, root, 0),
            "src/app.rs:20"
        );
        assert_eq!(
            said_place("Read the file", &inside, root, 2),
            "src/app.rs:20  +2"
        );

        // No line, and nowhere near the tree.
        let elsewhere = obelus_agent::acp::Place {
            path: PathBuf::from("/etc/hosts"),
            line: None,
        };
        assert_eq!(
            said_place("Read the file", &elsewhere, root, 0),
            "/etc/hosts"
        );
    }

    /// A title that is already the path does not have it written after it.
    ///
    /// Which is what an agent's titles are: `Write <path>`, every time. The
    /// row said the path twice and the second copy ran off the edge of the
    /// screen, taking the mark that says there is more behind the row and
    /// the count of what it changes with it -- so the one row that says a
    /// file was rewritten said nothing about how much.
    ///
    /// What the title has not said is still said. Broken deliberately by
    /// answering `false` for `named`, which puts the second copy back.
    #[test]
    fn a_place_the_title_already_names_is_not_said_again() {
        let root = Path::new("/tree");
        let written = obelus_agent::acp::Place {
            path: PathBuf::from("/tree/src/app.rs"),
            line: None,
        };
        let title = "Write src/app.rs";
        assert_eq!(said_place(title, &written, root, 0), "");
        // The other files it touched are news whatever the title says.
        assert_eq!(said_place(title, &written, root, 2), "+2");
        // And so is the line.
        let at = obelus_agent::acp::Place {
            line: Some(20),
            ..written.clone()
        };
        assert_eq!(said_place(title, &at, root, 0), "line 20");
        assert_eq!(said_place(title, &at, root, 2), "line 20  +2");
        // An agent that wrote the whole path in its title has said it too.
        assert_eq!(said_place("Write /tree/src/app.rs", &written, root, 0), "");
    }

    /// Without glyphs the mark is all there is to tell one voice from
    /// another, so no two of them can be the same.
    #[test]
    fn every_voice_has_its_own_mark() {
        let marks = [
            mark(Speaker::Reader),
            mark(Speaker::Agent),
            mark(Speaker::Thought),
            mark(Speaker::Tool),
            mark(Speaker::Note),
        ];
        let mut sorted = marks.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            marks.len(),
            "two voices look alike: {marks:?}"
        );
        // One column each, because the words start at a fixed place: a
        // wider mark would push a row out of the column the others are in.
        for mark in marks {
            assert_eq!(mark.chars().count(), 1, "{mark:?} is not one column");
        }
    }
}

#[cfg(test)]
mod caret {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use obelus_component::chat::{Chat, Room, Speaker};
    use ratatui::layout::Rect;

    /// Nothing a row carries is written in the scrollbar's column.
    ///
    /// The transcript leaves its last column to the bar, and the rows are
    /// wrapped to what is left -- but what a row puts down *after* its
    /// words begins past the end of them, and on a row that fills the
    /// width that is the bar's column exactly. The path a call names, the
    /// mark that says it folds, how much it changed and how it went are
    /// all written there, and every one of them used a writer with no
    /// last column at all: it stops where the screen does.
    ///
    /// So the rows painted over the bar, which is drawn before them.
    ///
    /// Broken deliberately by writing the suffixes with `write` again, or
    /// by putting the spans' `stop` back to `area.right()`.
    #[test]
    fn nothing_a_row_carries_is_written_on_the_scrollbar() {
        let area = Rect::new(0, 0, 76, 24);
        let width = super::reading_width(area);
        // Three lengths and both kinds of call, because the marks are put
        // down one after another and only one of them can land on the
        // bar's column at a time: a title one short of the width puts the
        // fold's mark there, two short puts the state there, and a call
        // that is changing something tints its rows to the edge instead.
        for short in 1..=3 {
            for changing in [false, true] {
                // Both, because which marks reach the bar depends on it: a
                // call still going is open, so its change is on the screen
                // and its rows are tinted, and one that is done is closed,
                // so the only row is its title and what follows it.
                for status in ["completed", "in_progress"] {
                    let mut chat = Chat::new();
                    // Long enough to scroll, so the bar is drawn at all: a band
                    // with no bar in it has nothing for a row to paint over,
                    // and a test set up that way cannot tell a bounded writer
                    // from an unbounded one.
                    for line in 0..40 {
                        chat.chunk(Speaker::Agent, &format!("something said, line {line}\n\n"));
                    }
                    chat.tool(
                        &obelus_agent::acp::Call {
                            id: "c1".to_string(),
                            title: "a".repeat(usize::from(width) - short),
                            kind: "execute".to_string(),
                            said: vec!["what the command printed".to_string()],
                            places: Vec::new(),
                            change: changing.then(|| obelus_agent::acp::Change {
                                path: std::path::PathBuf::from("one.rs"),
                                before: Some("before\n".to_string()),
                                after: "after\n".to_string(),
                            }),
                            ran: None,
                        },
                        status,
                    );

                    // Settled before it is drawn, which is the order the loop
                    // does it in: the window learns how many rows there are
                    // here, and one that has not been told says no bar.
                    let band = super::bands(area, &chat, None).transcript;
                    chat.settle(chat.rows(width).len(), band.height);
                    let last = chat.rows(width).len() - 1;
                    assert!(
                        chat.scrollable(band.height),
                        "this conversation does not scroll, so there is no bar to paint over"
                    );

                    let view = super::ChatView {
                        chat: &chat,
                        theme: &obelus_theme::builtin::DARK,
                        state: obelus_agent::Talking::Ready,
                        name: None,
                        settings: &[],
                        // On a row of the call, so the tint behind a selected
                        // row is drawn as well as the marks after it.
                        focus: obelus_component::chat::Focus::Transcript(
                            obelus_component::chat::Place {
                                row: last,
                                character: 0,
                            },
                        ),
                        card: None,
                        root: std::path::Path::new("/"),
                        phase: 0,
                        branch: None,
                        about_a_note: false,
                        note: None,
                        note_is_wrong: false,
                        usage: None,
                    };
                    let mut cells = ratatui::buffer::Buffer::empty(area);
                    ratatui::widgets::Widget::render(view, area, &mut cells);

                    let bar = area.right() - 1;
                    for y in band.y..band.bottom() {
                        // Every row of a band that scrolls carries the bar, in
                        // the thumb's colour or the track's. Anything else in
                        // that column is a row written over it -- a blank
                        // included, which is what the space in front of a
                        // fold's mark leaves, and what a tint leaves behind it.
                        assert_eq!(
                            cells[(bar, y)].symbol(),
                            "\u{2588}",
                            "a {status} title {short} short of the width, {}changing anything: \
                         row {y} wrote {:?} over the scrollbar in column {bar}",
                            if changing { "" } else { "not " },
                            cells[(bar, y)].symbol()
                        );
                    }
                }
            }
        }
    }

    /// The caret stays where the words are, not on the scrollbar.
    ///
    /// The transcript leaves its last column to the bar that says how much
    /// of the conversation there is: the rows are wrapped to what is left,
    /// so the bar costs the words nothing. But the place after the last
    /// character of a row that fills that width is one column past the
    /// words -- which is the bar's column exactly.
    ///
    /// So on a long answer the reader pressed up and the caret vanished.
    /// It was drawn, under the bar, in the one column of the transcript
    /// nothing can be seen in.
    ///
    /// Broken deliberately by clamping the caret to the band's last column
    /// rather than the words', which is what it did.
    #[test]
    fn the_caret_stays_off_the_scrollbar() {
        let area = Rect::new(0, 0, 76, 24);
        let width = super::reading_width(area);
        let mut chat = Chat::new();
        // Two rows of one long word, so the first of them is exactly as
        // wide as the words are allowed to be.
        chat.chunk(Speaker::Agent, &"a".repeat(usize::from(width) * 2));
        let room = Room {
            transcript: 20,
            reading: width,
            writing: super::writing_width(area),
        };
        chat.settle(chat.rows(width).len(), room.transcript);
        assert!(
            chat.rows(width)
                .iter()
                .all(|row| row.characters() == usize::from(width)),
            "this answer does not fill the width, so it cannot show the fault"
        );

        // Up from the box, which puts the cursor after the last character
        // of the last row.
        chat.handle_key(
            &KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
            false,
            room,
            &[],
        );
        let caret = super::ChatView::caret(area, &chat, None).expect("a caret in the transcript");
        assert!(
            caret.x < area.right() - 1,
            "the caret is on the scrollbar, in column {} of {}",
            caret.x,
            area.right() - 1
        );
        // And on the last character of the row, which is as near to after
        // it as there is room for.
        assert_eq!(caret.x, area.right() - 2);
    }
}
