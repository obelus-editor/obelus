//! The box that says what is wrong with the line the reader is on.
//!
//! Floated over the page, the way everything Obelus puts there for a
//! moment is. It used to be a *block* in the file -- rows hung under the
//! line, pushing the code down -- which is what a hunk's removed lines
//! are, and a complaint is not: those rows are text the file once had and
//! these are somebody else's prose about text it has now. A block also
//! cost the line a row of the file's own space, so reading the code around
//! a mistake meant reading it three rows further apart than it is written.
//!
//! Under the line, and over the code below it. A complaint belongs under
//! the thing it is about -- which is the one thing the block had right and
//! the reason this does not follow the hover's preference for above.
//!
//! Last of the four boxes, and it yields to all three of the others. They
//! are asked for: a reader pressed a key or typed a dot. This one arrives
//! on its own, and something nobody asked for may not cover something they
//! did.

use ratatui::{buffer::Buffer as CellBuffer, layout::Rect};

use crate::{Complained, Screen, editor};

/// The most rows of prose it takes, however much there is to say.
///
/// Somebody else's text may be as long as it likes and may not take the
/// screen: the same cap a card's own prose gets, for the same reason. What
/// is past it is in the list of problems, which is where a reader goes to
/// read one properly.
const MOST_ROWS: u16 = 6;

/// How wide the box is.
///
/// Wide enough to read a sentence in and no wider than the room: a
/// complaint is prose, and prose in a column of twenty is a column of two
/// words. The same answer a hover gives, because it is the same question
/// about the same kind of text.
fn width(editor: Rect) -> u16 {
    const WANTED: u16 = 64;
    WANTED.min(editor.width).max(editor.width.min(24))
}

/// The cells the words themselves have, inside the frame.
#[must_use]
pub fn room(editor: Rect) -> u16 {
    width(editor).saturating_sub(crate::PANEL_INSET * 2)
}

/// What the box says, as rows, and how many rows that is.
///
/// Wrapped here rather than by the application, because the room is the
/// view's to know: the words went into the file as a block while it was
/// one, and a block is rows -- so the width had to be settled before there
/// was anywhere to put them.
fn rows(said: &str, others: usize, room: u16) -> Vec<String> {
    let mut rows = obelus_text::wrapped(said, room.max(1));
    // Counted rather than listed. One complaint is a box and five is the
    // screen, and the list of problems is where they are all said.
    if others > 0 {
        rows.push(format!("and {others} more here"));
    }
    rows.truncate(usize::from(MOST_ROWS));
    rows
}

/// Where the box goes over the document being read, if there is one.
///
/// Behind all three of the boxes a reader asked for. Asked of what is
/// showing rather than remembered, the same as the rest of this: a
/// complaint that had to be taken down when a completion opened would be a
/// second thing to remember at every key that opens one.
#[must_use]
pub fn layout(app: &impl Screen, editor: Rect) -> Option<Rect> {
    if app.completion().is_some() || app.signature().is_some() || app.hover().is_some() {
        return None;
    }
    where_it_goes(
        &app.complaint()?,
        app.current_buffer()?,
        app.changes(),
        app.text_area(),
        editor,
    )
}

/// Where it goes over whatever document it is about.
///
/// Two callers, because it is the same box asked about two places: the
/// file being read, and the file a list of problems is previewing. The
/// second cannot use the first's inputs -- its buffer is the preview's and
/// its line is the row's, not the caret's.
#[must_use]
pub fn where_it_goes(
    complaint: &Complained<'_>,
    buffer: &obelus_buffer::Buffer,
    changes: Option<&obelus_git::Changes>,
    text_area: obelus_buffer::TextArea,
    editor: Rect,
) -> Option<Rect> {
    if editor.width < 8 || editor.height < 4 {
        return None;
    }
    let offset = editor::text_offset(
        buffer.text().line_count(),
        editor::changed(changes),
        !buffer.folds().is_empty(),
    );
    // Under the *line*, at the column the trouble starts at -- which is
    // where the underline is, so the box hangs off the word it is about.
    let (row, cell) = buffer.cell_of_place(complaint.line, complaint.column, text_area)?;
    let anchor_y = editor.y + row;
    let anchor_x = editor.x.saturating_add(offset).saturating_add(cell);

    let wide = width(editor);
    let x = anchor_x.min(editor.right().saturating_sub(wide));
    let height = u16::try_from(rows(complaint.said, complaint.others, room(editor)).len())
        .unwrap_or(MOST_ROWS)
        .saturating_add(2);

    // Under it where there is room, and over it where there is not: a
    // complaint belongs under the line, and a complaint nobody can see
    // belongs nowhere.
    let below = editor.bottom().saturating_sub(anchor_y.saturating_add(1));
    if below >= height {
        return Some(Rect {
            x,
            y: anchor_y.saturating_add(1),
            width: wide,
            height,
        });
    }
    let above = anchor_y.saturating_sub(editor.y);
    if above >= height {
        return Some(Rect {
            x,
            y: anchor_y - height,
            width: wide,
            height,
        });
    }
    // Neither side has room for the whole of it: the roomier one, with as
    // much as fits. Three rows is a frame and one row of words, which is
    // the least that says anything.
    let (room, hanging) = match above > below {
        true => (above, true),
        false => (below, false),
    };
    (room >= 3).then_some(Rect {
        x,
        y: match hanging {
            true => anchor_y - room,
            false => anchor_y.saturating_add(1),
        },
        width: wide,
        height: room,
    })
}

/// Draws the one over the document being read.
pub fn draw(cells: &mut CellBuffer, area: Rect, app: &impl Screen) {
    let Some(complaint) = app.complaint() else {
        return;
    };
    write(cells, area, &complaint, app.theme());
}

/// Draws one, wherever it is about.
pub fn write(
    cells: &mut CellBuffer,
    area: Rect,
    complaint: &Complained<'_>,
    theme: &obelus_theme::Theme,
) {
    if area.width < 3 || area.height < 3 {
        return;
    }
    crate::panel(cells, area, theme);
    let inside = crate::inside(area);
    // In the colour the same trouble underlines the word in: one
    // complaint, one colour, whichever of them the reader's eye lands on
    // first.
    let ink = ratatui::style::Style::new()
        .fg(theme.colour_for(Some(complaint.severity.kind())))
        .bg(theme.background);
    for (at, row) in rows(complaint.said, complaint.others, inside.width)
        .into_iter()
        .take(usize::from(inside.height))
        .enumerate()
    {
        let Ok(at) = u16::try_from(at) else {
            continue;
        };
        crate::write(cells, inside.x, inside.y + at, &row, ink);
    }
}
