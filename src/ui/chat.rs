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

use ratatui::{buffer::Buffer as CellBuffer, layout::Rect, style::Style, widgets::Widget};

use crate::{
    app::{App, talking::Talking},
    component::chat::{Chat, Speaker},
    icons,
    theme::Theme,
    ui::{fill, put, rule, text_width, write},
};

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
    }
}

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

/// The room a list opened over the conversation has.
///
/// Everything above the box and the rule over it. A compact list draws
/// against the foot of whatever it is given, and the foot of the whole
/// region is what is being written -- which for the list of commands is
/// the one row that must stay visible, because the list is a list of what
/// is being typed there.
#[must_use]
pub fn above_writing(area: Rect, chat: &Chat) -> Rect {
    let rows = chat.writing().rows(writing_width(area)).len();
    let writing = regions(area, rows).writing;
    Rect {
        height: writing.y.saturating_sub(area.y + 1),
        ..area
    }
}

/// Where the four bands go, given how many rows the box needs.
#[must_use]
pub fn regions(area: Rect, needed: usize) -> Regions {
    let row = |y: u16, height: u16| Rect {
        x: area.x,
        y,
        width: area.width,
        height,
    };
    // The header and two rules: three rows that are there whatever is
    // written. The box needs no rule under it -- the screen keeps one
    // between whatever is showing and the status bar, and that one is
    // directly under the box.
    let fixed = 3;
    let writing = u16::try_from(needed)
        .unwrap_or(MOST_WRITING)
        .clamp(1, MOST_WRITING)
        .min(area.height.saturating_sub(fixed).max(1));
    let transcript = area.height.saturating_sub(fixed + writing);
    let top = area.y;
    Regions {
        header: row(top, 1),
        transcript: row(top + 2, transcript),
        writing: row(top + 3 + transcript, writing),
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
    /// Which way of working is on, and how many there are to walk.
    mode: Option<&'a str>,
    /// How many ways of working there are, which is what says whether
    /// there is anything to switch.
    modes: usize,
}

impl<'a> ChatView<'a> {
    /// Borrows what the view needs, or nothing if the conversation is not
    /// what the region is showing.
    #[must_use]
    pub fn new(app: &'a App) -> Option<Self> {
        Some(Self {
            chat: app.chat()?,
            theme: app.theme(),
            state: app.talking(),
            name: app.agent_name(),
            mode: app.agent_mode().map(|mode| mode.name.as_str()),
            modes: app.agent_modes().len(),
        })
    }

    /// Where the terminal should put its caret: in the box, where the
    /// writing is.
    #[must_use]
    pub fn caret(area: Rect, chat: &Chat) -> Option<ratatui::layout::Position> {
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
        let regions = regions(area, rows.len());

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
        self.writing(cells, regions.writing, &rows, plain, dim);
    }
}

impl ChatView<'_> {
    /// What has been said, and the commands being completed over it.
    fn transcript(&self, cells: &mut CellBuffer, area: Rect, plain: Style, dim: Style) {
        let words = area.x + MARGIN + INDENT;
        let rows = self
            .chat
            .rows(area.width.saturating_sub(MARGIN + INDENT + 1));
        if rows.is_empty() {
            write(cells, words, area.y, self.nothing_said(), dim);
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
            let (glyph, style) = self.voice(row.speaker, plain, dim);
            if row.first {
                if icons::enabled() {
                    put(cells, area.x + MARGIN, y, glyph, style);
                } else {
                    write(cells, area.x + MARGIN, y, mark(row.speaker), style);
                }
            }
            let ended = write(cells, words, y, &row.text, style);
            // A tool call's state goes after its title rather than in front
            // of it: the title is what a reader is scanning, and the state
            // changes under them twice.
            if let Some(state) = &row.state {
                self.state_of(cells, ended + 1, y, state, dim);
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
                let _ = match icons::enabled() {
                    true => put(cells, area.x + MARGIN, y, icons::ui::SAY, dim),
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
        // No band. Everywhere else in obelus the status row is a solid
        // strip because it is a different subject from the code above it;
        // here the row above it is the box a message is written in, and the
        // rule between them has already said where one stops. A strip as
        // well would be the only heavy thing on a screen that is otherwise
        // all text.
        let plain = Style::new()
            .bg(self.theme.background)
            .fg(self.theme.foreground);
        fill(cells, area, plain);
        // The mode on the left, because it is a fact about the
        // conversation and the left is where obelus puts those.
        let mode = self.mode.unwrap_or(match self.state {
            Talking::Ready | Talking::Thinking => "no modes",
            _ => "",
        });
        write(
            cells,
            area.x + 1,
            area.y,
            mode,
            plain.fg(self.theme.gutter_current),
        );

        // And how to change it, which is only worth saying when there is
        // more than one to change to.
        if self.modes < 2 {
            return;
        }
        let hint = match icons::enabled() {
            true => format!("{}{}  mode", icons::key::SHIFT, icons::key::TAB),
            false => "shift+tab  mode".to_string(),
        };
        if let Ok(offset) =
            u16::try_from(usize::from(area.width).saturating_sub(text_width(&hint) + 1))
        {
            write(
                cells,
                area.x + offset,
                area.y,
                &hint,
                plain.fg(self.theme.gutter),
            );
        }
    }

    /// Who is being talked to, and what they are doing.
    fn header(&self, cells: &mut CellBuffer, area: Rect, plain: Style, dim: Style) {
        let mut column = area.x + MARGIN;
        if icons::enabled() {
            put(cells, column, area.y, icons::ui::AGENT, dim);
            column += INDENT;
        }
        let name = self.name.unwrap_or("no agent");
        column = write(
            cells,
            column,
            area.y,
            name,
            plain.fg(self.theme.gutter_current),
        );

        let doing = match self.state {
            Talking::Nobody => "nobody is chosen",
            Talking::Idle => "not started yet",
            Talking::Starting => "starting\u{2026}",
            Talking::Ready => "",
            Talking::Thinking => "thinking\u{2026}",
            Talking::Gone => "it has stopped",
        };
        if !doing.is_empty() {
            write(cells, column + 2, area.y, doing, dim);
        }

        // What escape does, which is the one thing that changes: while the
        // agent is working it stops the agent, and otherwise it closes the
        // view.
        let hint = match self.state {
            Talking::Thinking => "esc stops it",
            _ => "esc closes",
        };
        if let Ok(offset) =
            u16::try_from(usize::from(area.width).saturating_sub(text_width(hint) + 1))
            && offset > column + 2
        {
            write(cells, area.x + offset, area.y, hint, dim);
        }
    }

    /// The glyph and colour one speaker's rows are drawn in.
    fn voice(&self, speaker: Speaker, plain: Style, dim: Style) -> (char, Style) {
        match speaker {
            // The reader's own words in the brighter colour: a transcript is
            // read looking for where you asked something.
            Speaker::Reader => (icons::ui::READER, plain.fg(self.theme.gutter_current)),
            Speaker::Agent => (icons::ui::AGENT, plain),
            // Thinking is not the answer, and a transcript that draws them
            // alike is a transcript a reader has to sort out themselves.
            Speaker::Thought => (icons::ui::THOUGHT, dim),
            Speaker::Tool => (icons::ui::TOOL, dim),
            Speaker::Note => (icons::ui::NOTE, dim),
        }
    }

    /// How far a tool call has got.
    fn state_of(&self, cells: &mut CellBuffer, x: u16, y: u16, state: &str, dim: Style) {
        let (glyph, word, colour) = match state {
            "pending" => (icons::ui::WAITING, "waiting", self.theme.gutter),
            "in_progress" => (icons::ui::RUNNING, "running", self.theme.gutter_current),
            "completed" => (icons::ui::DONE, "done", self.theme.gutter),
            "failed" => (icons::ui::BROKEN, "failed", self.theme.syntax.constant),
            other => (icons::ui::WAITING, other, self.theme.gutter),
        };
        let style = dim.fg(colour);
        if icons::enabled() {
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
            Talking::Gone => "it stopped. ask something to start it again",
            Talking::Starting | Talking::Idle => "starting\u{2026}",
            Talking::Ready | Talking::Thinking => "ask it something",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Speaker, mark};

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
