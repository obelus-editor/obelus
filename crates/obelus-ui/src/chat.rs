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

use crate::{Screen, fill, put, rule, write};

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
/// screen is showing, obelus's own status row is the conversation's -- one
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
/// half of a card that says what is being asked. A reader who went to the
/// list of open documents to go and look something up lost the question on
/// the way.
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

/// What a tool call says about where it was working.
///
/// The path as a reader writes it -- relative to the tree obelus was opened
/// on -- and the line when the agent named one. And how many other files it
/// named, because a call that touched six of them says so on its one row
/// until the reader opens it.
fn said_place(place: &acp::Place, root: &Path, more: usize) -> String {
    let path = super::relative_to(&place.path, root).display().to_string();
    let said = match place.line {
        Some(line) => format!("{path}:{line}"),
        None => path,
    };
    match more {
        0 => said,
        more => format!("{said}  +{more}"),
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
        // The code rather than a sign, and after the amount: obelus knows
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

/// What one setting says on the row, and whether it is in force.
fn said(setting: &acp::Setting) -> (String, bool) {
    match setting.kind {
        // The value, which names itself.
        acp::Kind::Select => (
            setting
                .current_name()
                .unwrap_or(&setting.current)
                .to_string(),
            true,
        ),
        // The name, because "on" is not a thing to be told.
        acp::Kind::Switch => (setting.name.clone(), setting.current == "on"),
    }
}

/// The conversation, over the whole editor region.
pub struct ChatView<'a> {
    chat: &'a Chat,
    theme: &'a Theme,
    /// What obelus is doing about an agent.
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
    /// The tree obelus was opened on, for writing the paths an agent names
    /// the way a reader writes them.
    root: &'a Path,
    /// Where the animation has got to, for the row that turns.
    phase: u32,
    /// The note this conversation is about, where it is about one.
    ///
    /// A header earns its row by carrying something, and this is what it
    /// carries: a reader with four conversations open has four screens that
    /// would otherwise differ only in what was said in them. A label reading
    /// "chat" would answer a question nobody asked -- they pressed the key.
    about: Option<String>,
    /// What obelus has to say, until the next key.
    ///
    /// A conversation has a status row of its own, so it has to carry this
    /// too: without it every command that answers by saying something --
    /// there is no history for this, there was nothing to close -- would
    /// press its key and get silence back.
    note: Option<&'a str>,
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
            about: app.what_this_conversation_is_about(),
            note: app.note(),
            usage: app.agent_usage(),
        })
    }

    /// Where in the transcript a point on the screen is, if it is in one
    /// at all.
    ///
    /// A place in what was said rather than the row and column it was
    /// pointed at with, because that is what a selection keeps -- and this
    /// is the only piece of obelus that knows both, the rows having just
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
        use obelus_component::chat::Spot;

        let band = bands(area, chat, card).transcript;
        if y < band.y || y >= band.bottom() || x < area.x || x >= area.right() {
            return None;
        }
        let rows = chat.rows(reading_width(area));
        let at = chat.top() + usize::from(y - band.y);
        let row = rows.get(at)?;
        let (said, source) = row.from?;

        let mut column = area.x + MARGIN + INDENT + u16::from(row.depth) * DEEPER;
        let mut after: Option<Spot> = None;
        for span in &row.spans {
            let bytes = span.from.clone();
            for (offset, character) in span.text.char_indices() {
                let wide = u16::try_from(text_width(&character.to_string()))
                    .unwrap_or(1)
                    .max(1);
                match bytes.as_ref() {
                    Some(bytes) => {
                        let at = bytes.start + offset;
                        if x < column + wide {
                            return Some(Spot { said, source, at });
                        }
                        after = Some(Spot {
                            said,
                            source,
                            at: at + character.len_utf8(),
                        });
                    }
                    // A mark the reading drew. Pointing at one is pointing
                    // between the words around it.
                    None if x < column + wide => return after,
                    None => {}
                }
                column += wide;
            }
        }
        after
    }

    /// Where the terminal should put its caret: in the box, where the
    /// writing is.
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
        // Nowhere, while the keys are walking the row of settings: a caret
        // left blinking in the box would say that what is typed goes
        // there, and it does not. What says where the keys are going is
        // the selected background on the row, the same as in every list.
        if chat.focus() != Focus::Writing {
            return None;
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
        let rows = self.chat.writing().rows(width);
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
        // which is where every other hint in obelus gets its spelling.
        // This one cannot be: `ctrl+end` is on the list of chords the
        // table refuses, because the editor takes it before the table is
        // reached -- so the one key obelus will not let a reader rebind is
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
        let since = self.chat.said_since();
        let said = match since {
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
        // the one scrolling thing in obelus with no answer to "how much of
        // this is there". The column is already spare -- the rows are
        // wrapped to leave it -- so nothing moves to make room.
        if self.chat.scrollable(area.height) {
            crate::scrollbar(cells, area, self.chat.top(), rows.len(), self.theme);
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
            // The row the reader is standing on, marked the way every list
            // in obelus marks one. Only rows that do something are ever
            // stood on, so the mark is the promise: what is lit is what
            // enter opens.
            let here = self.focus == Focus::Transcript(first + usize::from(offset));
            let (glyph, style) = self.voice(row, plain, dim);
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
                        width: area.right().saturating_sub(from),
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
                self.state_of(cells, at, y, state, dim);
            }
            if row.first {
                let at = area.x + MARGIN + u16::from(row.depth) * DEEPER;
                // The row that says something is happening turns, and it
                // turns whether or not glyphs are drawn: a picture of a
                // cog says a tool was used, and only movement says it is
                // still going.
                if row.speaker == Speaker::Doing {
                    put(cells, at, y, crate::spinning(self.phase), style);
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
            let mut ended = crate::reading::write_spans(
                cells,
                words,
                y,
                &row.spans,
                &crate::reading::Drawn {
                    base: style,
                    theme: self.theme,
                    stop: area.right(),
                    held: row.held.as_ref(),
                },
            );
            // Where it said it was working, after the title. The path is
            // its own affordance: obelus opens files, so a row that names
            // one is a row that goes there.
            if let Some((place, more)) = &row.place {
                ended = write(
                    cells,
                    ended + 2,
                    y,
                    &said_place(place, self.root, *more),
                    dim,
                );
            }
            // What says there is more behind this row than it is showing:
            // the same mark a settings row and a card use for the same
            // promise, turned down when what it holds is open.
            if row.folds.is_some() {
                ended = write(cells, ended + 1, y, &opens(row.open), dim);
            }
            // How much it changes, which is what a reader reads first: the
            // shape of the change before any of its lines.
            if let Some((added, removed)) = row.changed {
                ended = write(
                    cells,
                    ended + 2,
                    y,
                    &format!("+{added} \u{2212}{removed}"),
                    dim,
                );
            }
            // A tool call's state goes after its title rather than in front
            // of it: the title is what a reader is scanning, and the state
            // changes under them twice.
            if let Some(state) = &row.state
                && row.speaker != Speaker::Step
            {
                self.state_of(cells, ended + 1, y, state, dim);
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
        rows: &[String],
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
            write(cells, area.x + MARGIN + INDENT, y, row, plain);
        }
    }

    /// The status row, which while a conversation is showing is the
    /// conversation's: which way of working the agent is in, and how to
    /// change it.
    ///
    /// Drawn into obelus's own status region rather than into a row of the
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
        let mut hints = Vec::new();
        if self.about.is_some() {
            hints.push(match obelus_icons::enabled() {
                true => format!("{}t  the note", obelus_icons::key::ALT),
                false => "alt+t  the note".to_string(),
            });
        }
        if self.mode().is_some_and(|mode| mode.values.len() > 1) {
            hints.push(match obelus_icons::enabled() {
                true => format!(
                    "{}{}  mode",
                    obelus_icons::key::SHIFT,
                    obelus_icons::key::TAB
                ),
                false => "shift+tab  mode".to_string(),
            });
        }
        let hint = (!hints.is_empty()).then(|| hints.join("   "));
        if let Some(hint) = &hint
            && let Ok(offset) =
                u16::try_from(usize::from(area.width).saturating_sub(text_width(hint) + 1))
        {
            write(
                cells,
                area.x + offset,
                area.y,
                hint,
                plain.fg(self.theme.gutter),
            );
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
            write(
                cells,
                area.x + 1,
                area.y,
                &super::truncate_from_right(note, room),
                plain,
            );
            return;
        }
        self.settings(cells, area, room, plain);
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
    /// thing it is about is its name, so that is what is written and being
    /// off is said by writing it dim.
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
        // carries the arrow obelus puts on everything with a list behind
        // it, so it is wider than the others by exactly that.
        let chosen = match self.focus {
            Focus::Settings(at) => Some(at.min(self.settings.len() - 1)),
            Focus::Transcript(_) | Focus::Writing => None,
        };
        let words: Vec<(String, bool)> = self
            .settings
            .iter()
            .enumerate()
            .map(|(index, setting)| {
                let (mut word, on) = said(setting);
                if Some(index) == chosen && setting.kind == acp::Kind::Select {
                    word.push_str(&opens(false));
                }
                (word, on)
            })
            .collect();

        // Which one to start at: as near the beginning as having the
        // focused one on screen allows. A row is a window on a list like
        // any other, and the one thing a window must not do is hide what
        // the keys are moving.
        let mut first = 0;
        if let Some(chosen) = chosen {
            let mut taken = 0;
            for index in (0..=chosen).rev() {
                taken += text_width(&words[index].0) + SEPARATOR_WIDTH;
                if taken > room {
                    first = index + 1;
                    break;
                }
            }
        }

        let mut column = area.x + 1;
        let mut left = room;
        // What was cut off the front, which is a setting the reader can
        // still walk back to.
        if first > 0 {
            column = write(cells, column, area.y, MORE, plain.fg(self.theme.gutter));
            left = left.saturating_sub(text_width(MORE));
        }
        for (index, (word, on)) in words.iter().enumerate().skip(first) {
            let separated = index > first || first > 0;
            let wanted = text_width(word) + usize::from(separated) * SEPARATOR_WIDTH;
            // No room for this one: the row says so rather than stopping
            // silently, because a reader who cannot see a setting cannot
            // know it is there to walk to.
            if wanted > left {
                write(cells, column, area.y, MORE, plain.fg(self.theme.gutter));
                return;
            }
            left -= wanted;
            if separated {
                column = write(
                    cells,
                    column,
                    area.y,
                    SEPARATOR,
                    plain.fg(self.theme.gutter),
                );
            }
            // The focused one wears the background a selected row wears in
            // every list, which while the reader is up here is the only
            // thing on screen saying where the keys are going -- the caret
            // is put away for exactly as long.
            let ground = match Some(index) == chosen {
                true => self.theme.selected_row_background,
                false => self.theme.background,
            };
            let ink = match on {
                true => self.theme.gutter_current,
                // A switch that is off: there, and plainly not in force --
                // the colour a row nobody can choose is drawn in.
                false => self.theme.gutter,
            };
            column = write(cells, column, area.y, word, plain.fg(ink).bg(ground));
        }
    }

    /// Who is being talked to, and what about.
    ///
    /// A header says what the thing it names *is*, which for an agent is its
    /// name -- and, once a reader can have four conversations open, which of
    /// them this is. What is *happening* goes at the foot of the transcript,
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
        // The note, dimmed after it, for as much of the row as is left. It
        // is what the reader called this conversation and the agent's name
        // is the same on all of them, so this is the half that tells one
        // screen from another.
        let Some(about) = self.about.as_deref() else {
            return;
        };
        let left = area.x + area.width;
        if column + 2 >= left {
            return;
        }
        column = write(cells, column, area.y, "  ", dim);
        // From the right, because a note's first words are the ones a
        // reader wrote to recognise it by: "wire the counts tree up to the
        // search" cut at the back is still the note they meant.
        let room = usize::from(left - column);
        write(
            cells,
            column,
            area.y,
            &crate::truncate_from_right(about, room),
            dim,
        );
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

    /// How far a tool call has got.
    fn state_of(&self, cells: &mut CellBuffer, x: u16, y: u16, state: &str, dim: Style) {
        let (glyph, word, colour) = match state {
            "pending" => (obelus_icons::ui::WAITING, "Waiting", self.theme.gutter),
            "in_progress" => (
                obelus_icons::ui::RUNNING,
                "Running",
                self.theme.gutter_current,
            ),
            "completed" => (obelus_icons::ui::DONE, "Done", self.theme.gutter),
            "failed" => (
                obelus_icons::ui::BROKEN,
                "Failed",
                self.theme.syntax.constant,
            ),
            other => (obelus_icons::ui::WAITING, other, self.theme.gutter),
        };
        let style = dim.fg(colour);
        if obelus_icons::enabled() {
            put(cells, x, y, glyph, style);
        } else {
            write(cells, x, y, word, style);
        }
    }

    /// What to say when nothing has been said yet, in words.
    ///
    /// Which is not one message: a reader who has chosen no agent has
    /// something to do about it, and a reader whose agent is starting has
    /// only to wait.
    fn nothing_said(&self) -> &'static str {
        match self.state {
            Talking::Nobody => "no agent is active \u{2014} open the settings and choose one",
            Talking::Gone => "It stopped. Ask something to start it again",
            Talking::Starting | Talking::Idle => "starting\u{2026}",
            Talking::Ready | Talking::Thinking => "Ask it something",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{Speaker, mark, said_place};

    /// Where a tool call was, written the way a reader writes a path.
    ///
    /// Relative to the tree obelus was opened on, because that is the part
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
        assert_eq!(said_place(&inside, root, 0), "src/app.rs:20");
        assert_eq!(said_place(&inside, root, 2), "src/app.rs:20  +2");

        // No line, and nowhere near the tree.
        let elsewhere = obelus_agent::acp::Place {
            path: PathBuf::from("/etc/hosts"),
            line: None,
        };
        assert_eq!(said_place(&elsewhere, root, 0), "/etc/hosts");
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
