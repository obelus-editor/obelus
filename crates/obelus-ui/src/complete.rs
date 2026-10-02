//! The panel of what could be typed next.
//!
//! Drawn beside the cursor rather than in a region of its own: what it
//! offers is about the word being typed, and a list at the foot of the
//! screen would make the reader look away from the place they are writing.
//! Its left edge is the start of that word, so the labels stand under the
//! letters already typed.
//!
//! A box, because it sits on top of code: the file behind it goes on being
//! a file, and a list of names with no edge would read as lines of it. The
//! same box-drawing characters a table is made of, in the same colour --
//! there is one way to draw an edge in Obelus and this is it.

use obelus_component::completion::{
    Completion, DETAIL_GAP, ICON_COLUMNS, LEAST_DOCUMENTATION, MOST_DOCUMENTATION, MOST_ROWS,
};
use obelus_text::text_width;
use obelus_theme::Theme;
use ratatui::{
    buffer::Buffer as CellBuffer,
    layout::Rect,
    style::{Modifier, Style},
};

use crate::{Marked, Matched, Screen, editor, fill, write_marked};

/// The narrowest a panel gets, whatever its rows want.
///
/// Narrower than this and the labels are all ellipsis: a list nobody can
/// read is worse than a list that overhangs the word it belongs to.
const LEAST_WIDTH: u16 = 24;

/// Where the panel goes, and how its room is divided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Panel {
    /// The whole box, borders included.
    pub area: Rect,
    /// How many rows of candidates it shows.
    pub list: u16,
    /// How many rows of documentation, which is none when there is no room
    /// for enough of it to be worth reading.
    pub documentation: u16,
    /// Whether the box hangs above the cursor's row rather than below it.
    ///
    /// The list is always the half nearest the cursor, so this says which
    /// way round the two halves are.
    pub above: bool,
}

/// Where the panel would go, if there is one to draw.
///
/// A function of the application and the region rather than something the
/// panel remembers: where the cursor is on screen changes with every
/// keystroke, and a remembered rectangle would be a frame behind.
#[must_use]
pub fn layout(app: &impl Screen, editor: Rect) -> Option<Panel> {
    let completion = app.completion()?;
    let buffer = app.current_buffer()?;
    if editor.width == 0 || editor.height == 0 {
        return None;
    }
    let offset = editor::text_offset(
        buffer.text().line_count(),
        editor::changed(app.changes()),
        !buffer.folds().is_empty(),
    );
    let (row, cell) = buffer.cursor_screen_cell(app.text_area())?;
    let cursor_y = editor.y + row;
    let cursor_x = editor.x.saturating_add(offset).saturating_add(cell);

    // The left edge is where the word starts, so the labels line up under
    // what has been typed of them.
    let typed = u16::try_from(text_width(completion.query())).unwrap_or(0);
    let left = cursor_x.saturating_sub(typed).max(editor.x);

    let wanted = u16::try_from(
        completion
            .width()
            .saturating_add(usize::from(crate::PANEL_INSET * 2)),
    )
    .unwrap_or(u16::MAX);
    let width = wanted.clamp(LEAST_WIDTH.min(editor.width), editor.width);
    // Overhanging the right edge moves the whole panel left rather than
    // narrowing it: the list is read down its left edge.
    let x = left.min(editor.right().saturating_sub(width));

    let below = editor.bottom().saturating_sub(cursor_y.saturating_add(1));
    let above = cursor_y.saturating_sub(editor.y);
    let rows = u16::try_from(completion.count())
        .unwrap_or(MOST_ROWS)
        .min(MOST_ROWS);
    // Under the cursor if it fits, over it if it does not, and whichever
    // side is roomier when neither fits: a list that is one row short of
    // its candidates is still a list.
    let (room, hanging) = match (below >= rows + 2, above >= rows + 2) {
        (true, _) => (below, false),
        (false, true) => (above, true),
        (false, false) if above > below => (above, true),
        (false, false) => (below, false),
    };
    if room < 3 {
        return None;
    }
    let list = rows.min(room - 2).max(1);

    // What is left goes to the documentation, and only if enough of it is
    // left: two rows of a paragraph that carries on are less use than the
    // two rows of code they cover.
    let spare = room - 2 - list;
    let documentation = match completion.has_documentation() && spare > 1 {
        true => (spare - 1).min(MOST_DOCUMENTATION),
        false => 0,
    };
    let documentation = match documentation >= LEAST_DOCUMENTATION {
        true => documentation,
        false => 0,
    };

    let height = 2
        + list
        + if documentation > 0 {
            1 + documentation
        } else {
            0
        };
    let y = match hanging {
        true => cursor_y.saturating_sub(height),
        false => cursor_y.saturating_add(1),
    };
    Some(Panel {
        area: Rect {
            x,
            y,
            width,
            height,
        },
        list,
        documentation,
        above: hanging,
    })
}

/// Draws the panel.
pub fn draw(cells: &mut CellBuffer, panel: Panel, app: &impl Screen) {
    let Some(completion) = app.completion() else {
        return;
    };
    let theme = app.theme();
    let area = panel.area;
    if area.width < 2 || area.height < 3 {
        return;
    }
    crate::panel(cells, area, theme);
    let room = crate::inside(area);

    // The two halves, in the order they are drawn on screen. The list is
    // always the half against the cursor.
    let documentation = (panel.documentation > 0).then(|| Rect {
        y: match panel.above {
            true => room.y,
            false => room.bottom() - panel.documentation,
        },
        height: panel.documentation,
        ..room
    });
    let list = Rect {
        y: match (panel.above, documentation) {
            (true, Some(_)) => room.bottom() - panel.list,
            _ => room.y,
        },
        height: panel.list,
        ..room
    };
    // The line between the halves, which is the box's own: one thing with
    // two parts rather than two boxes touching.
    if documentation.is_some() {
        let y = match panel.above {
            true => room.y + panel.documentation,
            false => room.y + panel.list,
        };
        crate::divider(cells, area, y, theme);
    }

    rows(cells, list, completion, theme);
    if let Some(area) = documentation {
        // The reading draws its own bar in the column it keeps for one,
        // which is the column the rows were laid out without.
        crate::bars::of(crate::bars::Whose::Documentation, || {
            crate::reading::draw(
                cells,
                area,
                completion.documentation_rows(),
                completion.scrolled(),
                theme,
                theme.background,
            );
        });
    }
}

/// Which candidate a point on screen is on.
///
/// The list is one of the panel's two halves and its rows are one apiece,
/// so this is the panel's own arithmetic read back: which half the list is
/// depends on whether the panel sits above the caret and whether there is
/// documentation beside it, and both are the panel's answers.
///
/// `None` for a point outside the list, or past the last candidate.
#[must_use]
pub fn row_at(panel: Panel, completion: &Completion, x: u16, y: u16) -> Option<usize> {
    let room = crate::inside(panel.area);
    let documented = panel.documentation > 0;
    let list = Rect {
        y: match (panel.above, documented) {
            (true, true) => room.bottom() - panel.list,
            _ => room.y,
        },
        height: panel.list,
        ..room
    };
    if y < list.y || y >= list.y + list.height || x < list.x || x >= list.right() {
        return None;
    }
    let at = completion.top() + usize::from(y - list.y);
    (at < completion.count()).then_some(at)
}

/// The candidates, one to a row.
fn rows(cells: &mut CellBuffer, area: Rect, completion: &Completion, theme: &Theme) {
    let scrolling = completion.count() > usize::from(area.height);
    // The bar goes inside the box, so the row it shares is a cell narrower.
    let room = Rect {
        width: area.width.saturating_sub(u16::from(scrolling)),
        ..area
    };
    for (row, candidate) in completion.visible(area.height) {
        let Ok(offset) = u16::try_from(row - completion.top()) else {
            break;
        };
        if offset >= area.height {
            break;
        }
        let y = area.y + offset;
        let selected = completion.selected() == row;
        let background = match selected {
            true => theme.selected_row_background,
            false => theme.background,
        };
        fill(
            cells,
            Rect {
                y,
                height: 1,
                ..area
            },
            Style::new().bg(background),
        );

        // What the candidate is, in the colour the code gives that kind of
        // thing: the same rule the outline follows, so a function in the
        // list is the colour a function is in the file.
        let colour = candidate
            .kind
            .map_or(theme.foreground, |kind| theme.syntax.colour(kind));
        let mut x = room.x;
        if let Some(icon) = candidate.icon
            && obelus_icons::enabled()
        {
            let mut glyph = String::new();
            glyph.push(icon);
            crate::write(cells, x, y, &glyph, Style::new().fg(colour).bg(background));
            // A blank column after it, always: a Nerd Font's glyphs are
            // drawn two cells wide in a terminal that allocated one.
            x = x.saturating_add(u16::try_from(ICON_COLUMNS).unwrap_or(2));
        }

        let marked = Marked::matched(
            Matched::Indices(completion.indices_at(row)),
            theme.picker_match_background,
        );
        let label = crate::truncate_from_right(
            &candidate.label,
            usize::from(room.right().saturating_sub(x)),
        );
        write_marked(
            cells,
            room,
            x,
            y,
            &label,
            Style::new().fg(colour).bg(background),
            &marked,
        );

        // The signature, in the colour a comment is: it is what the
        // candidate is rather than what it is called, and a reader scans
        // the names first.
        let Some(detail) = candidate.detail.as_deref() else {
            continue;
        };
        // In a column of its own, so the types read down the list.
        let column = u16::try_from(completion.labels() + DETAIL_GAP).unwrap_or(u16::MAX);
        let x = room.x.saturating_add(column);
        let room_left = usize::from(room.right().saturating_sub(x));
        if room_left == 0 {
            continue;
        }
        let detail = crate::truncate_from_right(detail, room_left);
        crate::write(
            cells,
            x,
            y,
            &detail,
            Style::new()
                .fg(theme.syntax.comment)
                .bg(background)
                .add_modifier(Modifier::ITALIC),
        );
    }
    let bar = scrolling
        .then(|| crate::scrollbar(cells, area, completion.top(), completion.count(), theme))
        .flatten();
    // And where the list has got to, for a front end that can draw it
    // arriving rather than simply being there.
    if let Ok(top) = i64::try_from(completion.top()) {
        crate::shapes::scrolled(
            Rect {
                width: area.width.saturating_sub(crate::editor::SCROLLBAR_WIDTH),
                ..area
            },
            top,
            bar,
        );
    }
}
