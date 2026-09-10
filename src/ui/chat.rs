//! The conversation with an agent.
//!
//! A header saying who is being talked to and what they are doing, and the
//! transcript under it. Not the editor's own view and not a buffer: there
//! are no line numbers, no cursor in the text and no file -- what is here is
//! a conversation.
//!
//! What is being typed is not here either. It goes on the status bar, where
//! every other line obelus asks a reader to type goes: a picker's filter, a
//! question's answer, the settings' filter. One row on the screen is the row
//! you type into, whatever is above it.
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

/// The rows the transcript itself gets.
///
/// One function, shared by the view, the scrolling and the key handler, so
/// that a page of movement is the page that is actually on screen.
#[must_use]
pub fn transcript(area: Rect) -> Rect {
    Rect {
        x: area.x,
        y: area.y + 2,
        width: area.width,
        // The header and the rule under it. What is being typed is on the
        // status bar, which is not this region.
        height: area.height.saturating_sub(2),
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
        if area.height < 4 || area.width < 12 {
            return;
        }

        self.header(cells, area, plain, dim);
        rule(
            cells,
            Rect {
                y: area.y + 1,
                height: 1,
                ..area
            },
            self.theme,
        );

        let region = transcript(area);
        let words = region.x + MARGIN + INDENT;
        let rows = self
            .chat
            .rows(region.width.saturating_sub(MARGIN + INDENT + 1));
        if rows.is_empty() {
            write(cells, region.x + INDENT, region.y, self.nothing_said(), dim);
        }
        let first = self.chat.top().min(rows.len());
        for (offset, row) in rows.iter().skip(first).enumerate() {
            let Ok(offset) = u16::try_from(offset) else {
                break;
            };
            if offset >= region.height {
                break;
            }
            let y = region.y + offset;
            let (glyph, style) = self.voice(row.speaker, plain, dim);
            if row.first {
                if icons::enabled() {
                    put(cells, region.x + MARGIN, y, glyph, style);
                } else {
                    write(cells, region.x + MARGIN, y, mark(row.speaker), style);
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
}

impl ChatView<'_> {
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
