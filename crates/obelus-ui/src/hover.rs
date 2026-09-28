//! What a server said about a place, drawn over the place.
//!
//! The same box the completion panel is in, holding what Obelus's own
//! markdown renderer makes of the answer: a hover is a README about one
//! symbol, and a README is a thing Obelus already knows how to draw.

use ratatui::{buffer::Buffer as CellBuffer, layout::Rect};

use crate::{Screen, editor};

/// Where the box goes, if there is one to draw.
///
/// Above the line it is about where there is room, because what is below
/// the cursor is the code being read next and what is above it has already
/// been read.
#[must_use]
pub fn layout(app: &impl Screen, editor: Rect) -> Option<Rect> {
    let hover = app.hover()?;
    let buffer = app.current_buffer()?;
    if editor.width < 8 || editor.height < 4 {
        return None;
    }
    let offset = editor::text_offset(
        buffer.text().line_count(),
        editor::changed(app.changes()),
        !buffer.folds().is_empty(),
    );
    // Over the place the answer is *about*, which is not the caret when
    // the pointer asked: a box about the word under the mouse, drawn
    // wherever the caret happens to be, is a box about somewhere else.
    let (line, column) = hover.at();
    let (row, cell) = buffer.cell_of_place(line, column, app.text_area())?;
    let anchor_y = editor.y + row;
    let anchor_x = editor.x.saturating_add(offset).saturating_add(cell);

    let inside = hover.wanted().max(1);
    let height = inside + 2;
    let above = anchor_y.saturating_sub(editor.y) >= height;
    let y = match above {
        true => anchor_y - height,
        false => anchor_y.saturating_add(1),
    };
    if !above && y.saturating_add(height) > editor.bottom() {
        // Neither side has room for the whole of it: the roomier one, with
        // as much of the answer as fits.
        let below = editor.bottom().saturating_sub(anchor_y + 1);
        let over = anchor_y.saturating_sub(editor.y);
        let (room, hanging) = match over > below {
            true => (over, true),
            false => (below, false),
        };
        if room < 3 {
            return None;
        }
        return Some(Rect {
            x: anchor_x.min(editor.right().saturating_sub(width(editor))),
            y: match hanging {
                true => anchor_y - room,
                false => anchor_y + 1,
            },
            width: width(editor),
            height: room,
        });
    }
    Some(Rect {
        x: anchor_x.min(editor.right().saturating_sub(width(editor))),
        y,
        width: width(editor),
        height,
    })
}

/// How wide the box is.
///
/// Worked out without the answer, so the answer can be laid out to it: the
/// height depends on how many rows the markdown makes, and the markdown
/// cannot be made into rows until there is a width to make them for.
///
/// Wide enough to read a paragraph in and no wider than the region: the
/// answer is prose, and prose in a column of twenty is a column of two
/// words.
pub(crate) fn width(editor: Rect) -> u16 {
    const WANTED: u16 = 64;
    WANTED.min(editor.width).max(editor.width.min(24))
}

/// The cells the markdown itself has: inside the panel, less the column
/// the reading keeps for its scrollbar.
#[must_use]
pub fn room(editor: Rect) -> u16 {
    width(editor)
        .saturating_sub(crate::PANEL_INSET * 2)
        .saturating_sub(editor::SCROLLBAR_WIDTH)
}

/// Draws it.
pub fn draw(cells: &mut CellBuffer, area: Rect, app: &impl Screen) {
    let Some(hover) = app.hover() else {
        return;
    };
    let theme = app.theme();
    if area.width < 3 || area.height < 3 {
        return;
    }
    crate::panel(cells, area, theme);
    crate::reading::draw(
        cells,
        crate::inside(area),
        hover.rows(),
        hover.scrolled(),
        theme,
        theme.background,
    );
}
