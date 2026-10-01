//! The paths that could finish the one being typed.
//!
//! A panel over the welcome screen, above the box it belongs to, with its
//! left edge under the part of the path it is offering to finish -- so the
//! names stand under the letters already typed, which is the rule
//! [`crate::complete`] follows for the same reason.
//!
//! **Its own panel, and not that one.** The two do the same job for the
//! reader and are made of different things: a completion from a server
//! carries a kind, a detail and a paragraph of documentation, and the
//! panel divides its room between a list and that paragraph. A path has
//! none of them. Forcing one into the other would mean a `Completion`
//! built out of empty fields and a layout dividing room nothing asks for
//! -- which is the mistake of sharing a meaning rather than a mechanism.
//! What *is* shared is the edge: there is one way to draw a box in Obelus
//! and [`crate::panel`] is it.
//!
//! **A name, not a path.** The directory is already in the box the reader
//! is typing in, and repeating it down twenty rows spends the width on the
//! one part of each row that is the same. A directory keeps its trailing
//! separator, which is how a reader tells one from a file without a column
//! of glyphs saying so.

use ratatui::{buffer::Buffer as CellBuffer, layout::Rect, style::Style};

use crate::{Choosing, fill, write};

/// The most rows it shows at once.
///
/// A directory of two hundred files is a panel that would cover the
/// screen, and what the reader does about a list this long is type
/// another letter rather than scroll it.
const MOST_ROWS: u16 = 8;

/// The narrowest it gets, whatever its rows want.
const LEAST_WIDTH: u16 = 20;

/// How far the names are inset from the panel's edge.
const INSET: u16 = 2;

/// Draws the panel, where there is one to draw.
///
/// `box_at` is the column the box's caret is in, and `region` is the whole
/// screen: the panel hangs above the status row, because that is where the
/// box it belongs to is and a list of what could be typed next belongs
/// beside what is being typed.
pub fn draw(
    cells: &mut CellBuffer,
    region: Rect,
    status: Rect,
    choosing: &Choosing,
    theme: &obelus_theme::Theme,
) {
    if !choosing.naming || choosing.candidates.is_empty() {
        return;
    }
    let rows = u16::try_from(choosing.candidates.len())
        .unwrap_or(MOST_ROWS)
        .min(MOST_ROWS);
    // The widest name, and then the edges and the inset on both sides.
    let widest = choosing
        .candidates
        .iter()
        .take(rows as usize)
        .map(|name| obelus_text::text_width(name))
        .max()
        .unwrap_or(0);
    let wanted = u16::try_from(widest.saturating_add((INSET * 2) as usize)).unwrap_or(u16::MAX);
    let width = wanted.clamp(LEAST_WIDTH.min(region.width), region.width);
    let height = rows + 2;
    // Inside the page and never over the rule at its foot: the rule is
    // the line between the page and the row the box is on, and a panel
    // laid across it reads as a box that has broken out of the page.
    if height > region.height {
        return;
    }
    let y = region.bottom() - height;
    // Under the part of the path it is finishing, and moved left rather
    // than narrowed where that would hang off the edge -- a list is read
    // down its left edge.
    let left =
        segment_column(choosing, status).min(region.right().saturating_sub(width).max(region.x));
    let area = Rect {
        x: left,
        y,
        width,
        height,
    };
    crate::panel(cells, area, theme);
    for (offset, name) in choosing.candidates.iter().take(rows as usize).enumerate() {
        let Ok(offset) = u16::try_from(offset) else {
            continue;
        };
        let row = y + 1 + offset;
        // The one mark for "the keys are here", and only where the reader
        // has walked into the list: until they do, the caret is in the
        // box and the box is what enter acts on, so a row drawn as though
        // it were under them would be a screen saying the wrong thing
        // about what the key does.
        let on = choosing.candidate_at == Some(offset as usize);
        let style = match on {
            true => Style::new().bg(theme.selected_row_background),
            false => Style::new(),
        };
        if on {
            fill(
                cells,
                Rect {
                    x: area.x + 1,
                    y: row,
                    width: area.width.saturating_sub(2),
                    height: 1,
                },
                style,
            );
        }
        write(
            cells,
            area.x + INSET,
            row,
            &crate::truncate_from_right(name, area.width.saturating_sub(INSET * 2) as usize),
            style.fg(theme.foreground),
        );
    }
}

/// Which column the part of the path being finished starts in.
///
/// Everything after the last separator is what the names stand under, so
/// the panel's left edge goes there rather than at the start of the box: a
/// reader typing `~/Work/ob` is choosing among things called `ob...`, and
/// a list left-aligned with the `~` would put them a dozen columns away
/// from the letters they match.
fn segment_column(choosing: &Choosing, status: Rect) -> u16 {
    // Either separator, the same question `Chooser::wants` asks: the
    // panel has to stand under the segment the reader is typing, and on
    // Windows that segment may be after a `/`.
    let before = match choosing.typed.rfind(std::path::is_separator) {
        Some(at) => choosing.typed[..=at].chars().count(),
        None => 0,
    };
    status.x + crate::status::typed_caret(Some("Open"), &choosing.typed, before)
}
