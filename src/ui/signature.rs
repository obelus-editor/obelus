//! What the call the cursor is inside takes.
//!
//! One line in a box, over the cursor's line rather than under it: what is
//! below the cursor is the code being written, and the argument list
//! belongs with the call above. The same box as the completion panel,
//! because it is the same kind of thing -- a server's answer, put where
//! the reader is looking.

use ratatui::{
    buffer::Buffer as CellBuffer,
    layout::Rect,
    style::{Modifier, Style},
};

use crate::{
    app::App,
    ui::{editor, truncate_from_right, write},
};

/// Where the line goes, if there is one to draw.
#[must_use]
pub fn layout(app: &App, editor: Rect) -> Option<Rect> {
    let signature = app.signature()?;
    let buffer = app.current_buffer()?;
    if editor.width < 4 || editor.height < 2 {
        return None;
    }
    let offset = editor::text_offset(
        buffer.text().line_count(),
        editor::marks(app.changes().is_some(), buffer.language()),
        !buffer.folds().is_empty(),
    );
    let (row, cell) = buffer.cursor_screen_cell(app.text_area())?;
    let cursor_y = editor.y + row;
    let cursor_x = editor.x.saturating_add(offset).saturating_add(cell);

    // Three rows: the box and the line in it. Above the cursor where there
    // is room, and below where there is not.
    let above = cursor_y.saturating_sub(editor.y) >= 3;
    let y = match above {
        true => cursor_y - 3,
        false => cursor_y + 1,
    };
    if !above && y + 3 > editor.bottom() {
        return None;
    }
    let width = u16::try_from(
        crate::ui::text_width(&signature.label) + usize::from(crate::ui::PANEL_INSET * 2),
    )
    .unwrap_or(u16::MAX)
    .min(editor.width);
    let x = cursor_x.min(editor.right().saturating_sub(width));
    Some(Rect {
        x,
        y,
        width,
        height: 3,
    })
}

/// Draws it.
pub fn draw(cells: &mut CellBuffer, area: Rect, app: &App) {
    let Some(signature) = app.signature() else {
        return;
    };
    let theme = app.theme();
    crate::ui::panel(cells, area, theme);

    let inside = crate::ui::inside(area);
    let room = usize::from(inside.width);
    let label = truncate_from_right(&signature.label, room);
    let x = inside.x;
    let y = area.y + 1;
    write(
        cells,
        x,
        y,
        &label,
        Style::new().fg(theme.syntax.comment).bg(theme.background),
    );

    // The argument being typed, over the top: the rest of the line is
    // there to be read past, and this is the part the reader is on.
    let Some((from, to)) = signature.active else {
        return;
    };
    let shown: Vec<char> = label.chars().collect();
    if from >= shown.len() {
        return;
    }
    let part: String = shown[from..to.min(shown.len())].iter().collect();
    let before: String = shown[..from].iter().collect();
    let Ok(offset) = u16::try_from(crate::ui::text_width(&before)) else {
        return;
    };
    write(
        cells,
        x + offset,
        y,
        &part,
        Style::new()
            .fg(theme.foreground)
            .bg(theme.background)
            .add_modifier(Modifier::BOLD),
    );
}
