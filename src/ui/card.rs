//! The card an agent's question is answered on.
//!
//! It sits at the foot of the conversation, where the box a message is
//! written in sits: the two never show at once, because while the agent is
//! waiting on an answer there is no message to send.
//!
//! The parts are laid out by one function, which is what keeps the caret in
//! the row the reader is typing in. Everything below the named answers has
//! a size of its own -- what is written grows, the row that sends it does
//! not -- so the answers take what is left, and scroll inside it.

use ratatui::{buffer::Buffer as CellBuffer, layout::Rect, style::Style};

use super::{chat, fill, put, rule, truncate_from_right, write};
use crate::{
    component::card::{Card, On},
    theme::Theme,
};

/// The margin every row of the conversation is drawn in.
const MARGIN: u16 = 1;

/// The row that sends the card.
const SUBMIT: &str = "submit";

/// The cells a row of the card has to write in.
///
/// One function for it, because a card measured against one width and drawn
/// against another lays out a row it then does not draw -- which on screen
/// is a blank row nobody put there.
#[must_use]
pub const fn width_of(area: Rect) -> u16 {
    let width = area.width.saturating_sub(MARGIN * 2);
    if width == 0 { 1 } else { width }
}

/// Where the card's parts go inside the band it was given.
///
/// One function for it, because the caret's row and the row the words are
/// drawn on have to be the same row -- and they are worked out at different
/// times, from different sides of the application.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    /// The prose, with the rule under it.
    pub about: Rect,
    /// The named answers that fit.
    pub choices: Rect,
    /// The row saying whether the reader is writing one of their own.
    pub tick: Option<Rect>,
    /// What they have written.
    pub words: Option<Rect>,
    /// The row that sends it, with the rule over it.
    pub submit: Option<Rect>,
    /// The row saying what is missing, where there is no row that sends it.
    pub complaint: Option<Rect>,
}

/// Lays the card out in the band it was given.
#[must_use]
pub fn layout(card: &Card, area: Rect) -> Layout {
    let width = width_of(area);
    let band = |y: u16, height: u16| Rect {
        x: area.x,
        y,
        width: area.width,
        height,
    };
    // From the ends inwards: the prose at the top and the row that sends
    // the card at the bottom are what they are, what is written takes what
    // it needs, and the answers have the rest. A card given less than it
    // asked for scrolls its answers, because they are the part there can
    // be many of.
    let about = u16::try_from(card.about_rows(width))
        .unwrap_or(0)
        .min(area.height);
    let mut left = area.height - about;
    let submit = match card.has_submit() {
        true => 2.min(left),
        false => 0,
    };
    left -= submit;
    let complaint = match card.has_submit() || card.complaint().is_none() {
        true => 0,
        false => 1.min(left),
    };
    left -= complaint;
    let written = u16::try_from(card.written_rows(width))
        .unwrap_or(0)
        .min(left);
    left -= written;
    let tick = match card.has_tick() {
        true => 1.min(left),
        false => 0,
    };
    left -= tick;
    let top = area.y + about;
    Layout {
        about: band(area.y, about),
        choices: band(top, left),
        tick: (tick > 0).then(|| band(top + left, tick)),
        words: (written > 0).then(|| band(top + left + tick, written)),
        complaint: (complaint > 0).then(|| band(top + left + tick + written, complaint)),
        submit: (submit > 0).then(|| band(top + left + tick + written + complaint, submit)),
    }
}

/// Where the terminal should put its caret: in what is being written, when
/// that is where the keys are going.
#[must_use]
pub fn caret(area: Rect, card: &Card) -> Option<ratatui::layout::Position> {
    let regions = chat::bands_for(area, card);
    let parts = layout(card, regions.writing);
    let words = parts.words?;
    let (row, cell) = card.caret(width_of(words))?;
    // The rows the caret can be on are the rows the box has: what is
    // written scrolls under the caret rather than the caret leaving the
    // card.
    let height = usize::from(words.height).max(1);
    let first = row.saturating_sub(height - 1);
    let y = words.y + u16::try_from(row - first).unwrap_or(0);
    (y < words.bottom()).then(|| ratatui::layout::Position {
        x: (words.x + MARGIN + cell.get()).min(words.right().saturating_sub(1)),
        y,
    })
}

/// Draws the card into the band it was given.
pub fn draw(cells: &mut CellBuffer, area: Rect, card: &Card, theme: &Theme) {
    let plain = Style::new().fg(theme.foreground).bg(theme.background);
    let dim = plain.fg(theme.gutter);
    fill(cells, area, plain);
    let parts = layout(card, area);

    // What the agent asked, above its answers, with a rule under it: a list
    // of answers with nothing saying what they answer is not a question.
    if let Some(about) = card.what_about()
        && parts.about.height > 0
    {
        let rows = crate::text::wrapped(about, width_of(area));
        for (offset, words) in rows
            .iter()
            .enumerate()
            .take(usize::from(parts.about.height).saturating_sub(1))
        {
            let Ok(offset) = u16::try_from(offset) else {
                break;
            };
            write(cells, area.x + MARGIN, parts.about.y + offset, words, plain);
        }
        rule(
            cells,
            Rect {
                y: parts.about.bottom() - 1,
                height: 1,
                ..area
            },
            theme,
        );
    }

    // The named answers, with the one the reader is on marked the way
    // every list in obelus marks it.
    let shown = card.visible(parts.choices.height);
    for (offset, index) in shown.clone().enumerate() {
        let Ok(offset) = u16::try_from(offset) else {
            break;
        };
        if offset >= parts.choices.height {
            break;
        }
        let Some(choice) = card.choices().get(index) else {
            break;
        };
        let focused = card.on() == On::Choice(index);
        let background = match focused {
            true => theme.selected_row_background,
            false => theme.background,
        };
        let style = plain.bg(background);
        let y = parts.choices.y + offset;
        fill(
            cells,
            Rect {
                y,
                height: 1,
                ..parts.choices
            },
            style,
        );
        let mut x = area.x + MARGIN;
        if card.several() {
            x = write(cells, x, y, tick_of(choice.chosen), style);
        } else if let Some(icon) = choice.icon {
            // A private-use codepoint measures one cell and a Nerd Font's
            // own glyphs are drawn two wide, so the one after it is left
            // blank for the half that bleeds -- the same allowance every
            // list in obelus makes for the same glyphs.
            x += put(cells, x, y, icon, style) + 1;
        }
        let ended = write(cells, x, y, &choice.name, style);
        if let Some(about) = choice.about.as_deref() {
            // Cut with a mark rather than at the edge: a line that stops
            // mid-word where the screen happens to end reads as a line
            // that was written that way.
            let room = area.right().saturating_sub(ended + MARGIN * 2);
            write(
                cells,
                ended + 2,
                y,
                &truncate_from_right(about, usize::from(room)),
                style.fg(theme.gutter),
            );
        }
    }

    // The row that says whether the reader is writing an answer of their
    // own, where answers are ticked: there it is one of the ticks, because
    // a card where one row means something else is a card with two ways of
    // saying yes on it.
    if let Some(row) = parts.tick {
        let focused = card.on() == On::Tick;
        let style = match focused {
            true => plain.bg(theme.selected_row_background),
            false => plain,
        };
        fill(cells, row, style);
        let x = write(
            cells,
            area.x + MARGIN,
            row.y,
            tick_of(card.writing_wanted()),
            style,
        );
        write(
            cells,
            x,
            row.y,
            card.placeholder().unwrap_or_default(),
            style,
        );
    }

    // What they have written, or what the row is for while it is empty.
    if let Some(row) = parts.words {
        let width = width_of(row);
        let written = card.written(width);
        let height = usize::from(row.height).max(1);
        let (at, _) = card
            .caret(width)
            .unwrap_or((0, crate::coordinates::DisplayColumn::new(0)));
        let first = at.saturating_sub(height - 1);
        if card.blank() {
            // The placeholder, which says what the row is for. Dim, because
            // it is not an answer until somebody writes one.
            write(
                cells,
                row.x + MARGIN,
                row.y,
                card.placeholder().unwrap_or_default(),
                dim,
            );
        } else {
            for (offset, words) in written.iter().skip(first).take(height).enumerate() {
                let Ok(offset) = u16::try_from(offset) else {
                    break;
                };
                write(cells, row.x + MARGIN, row.y + offset, words, plain);
            }
        }
    }

    // What the card cannot do yet, where there is no row that sends it to
    // write it on: said in answer to the enter that asked for it.
    if let Some(row) = parts.complaint
        && let Some(complaint) = card.complaint()
    {
        write(cells, area.x + MARGIN, row.y, &complaint, dim);
    }

    // The row that sends the card, where every row above it is a tick
    // rather than an answer. What is missing is written on it rather than
    // said after the fact: obelus draws what cannot be done dim and says
    // why, everywhere else too.
    if let Some(row) = parts.submit {
        rule(cells, Rect { height: 1, ..row }, theme);
        let wanting = card.wanting();
        // Two things, said in two ways, the way every list in obelus says
        // them: the background is where the keys are, and the ink is
        // whether the row can be used. A row that lost its background for
        // being unusable would leave the reader with no way to see where
        // they are -- pressing enter, getting nothing, and nothing on
        // screen saying which row refused.
        let ground = match card.on() == On::Submit {
            true => theme.selected_row_background,
            false => theme.background,
        };
        let ink = match wanting.is_some() {
            true => theme.gutter,
            false => theme.foreground,
        };
        let style = plain.fg(ink).bg(ground);
        let y = row.y + 1;
        if y < row.bottom() {
            fill(
                cells,
                Rect {
                    y,
                    height: 1,
                    ..row
                },
                style,
            );
            let ended = write(cells, area.x + MARGIN, y, SUBMIT, style);
            if let Some(wanting) = wanting {
                write(
                    cells,
                    ended + 1,
                    y,
                    &format!("\u{b7} {wanting}"),
                    style.fg(theme.gutter),
                );
            }
        }
    }
}

/// What a tick looks like, ticked or not.
const fn tick_of(ticked: bool) -> &'static str {
    match ticked {
        true => "[x] ",
        false => "[ ] ",
    }
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::{Layout, layout, width_of};
    use crate::{
        component::card::{Card, Choice},
        ui::chat,
    };

    /// A card measured against one width and drawn against another lays out
    /// a row it then does not draw.
    ///
    /// Which on screen is a blank row between the answers and the box --
    /// nothing put it there, and nothing would say where it came from. The
    /// two widths are easy to mix up because the conversation has both: the
    /// box a message is written in is indented under an icon, and a card is
    /// a region of its own with a margin.
    #[test]
    fn a_card_is_measured_against_the_width_it_is_drawn_in() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 76,
            height: 22,
        };
        let choices = ["one", "two", "three"]
            .into_iter()
            .map(|name| Choice {
                id: name.to_string(),
                name: name.to_string(),
                about: None,
                icon: None,
                chosen: false,
            })
            .collect();
        let mut card = Card::new(choices, false);
        card.about("which parts should I look at");
        // Long enough to fill a row the card's own width and to spill over
        // one the box's width: the whole point is that the two differ.
        card.writing(
            "Other",
            false,
            Some(&"x".repeat(usize::from(width_of(area)))),
        );

        let band = chat::bands_for(area, &card).writing;
        let Layout {
            about,
            choices: rows,
            tick,
            words,
            complaint,
            submit,
        } = layout(&card, band);
        assert_eq!(
            usize::from(rows.height),
            card.choices().len(),
            "the answers were given room for a row that is not there"
        );
        let laid = about.height
            + rows.height
            + tick.map_or(0, |row| row.height)
            + words.map_or(0, |row| row.height)
            + complaint.map_or(0, |row| row.height)
            + submit.map_or(0, |row| row.height);
        assert_eq!(laid, band.height, "the parts do not fill the card");
    }
}
