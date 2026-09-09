//! The status bar: the file on the left, the cursor on the right.

use std::path::Path;

use ratatui::{buffer::Buffer as CellBuffer, layout::Rect, style::Style, widgets::Widget};
use unicode_width::UnicodeWidthChar;

use crate::{
    app::App,
    buffer::Buffer,
    picker::Picker,
    theme::Theme,
    ui::{text_width, truncate_from_left},
};

/// The status region.
pub struct StatusView<'a> {
    buffer: Option<&'a Buffer>,
    /// When a picker is open the row is its prompt instead.
    picker: Option<&'a Picker>,
    theme: &'a Theme,
    working_directory: &'a Path,
}

impl<'a> StatusView<'a> {
    /// Borrows what the view needs from the application.
    #[must_use]
    pub fn new(app: &'a App) -> Self {
        Self {
            buffer: app.current_buffer(),
            picker: app.picker(),
            theme: app.theme(),
            working_directory: app.working_directory(),
        }
    }
}

impl Widget for StatusView<'_> {
    fn render(self, area: Rect, cells: &mut CellBuffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }
        let style = Style::new()
            .bg(self.theme.status_background)
            .fg(self.theme.status_foreground);

        // The whole row, including the padding at both ends and the gap in the
        // middle, so the bar reads as one solid band rather than as coloured
        // text floating on the code's background.
        for x in area.left()..area.right() {
            if let Some(cell) = cells.cell_mut((x, area.y)) {
                cell.set_symbol(" ");
                cell.set_style(style);
            }
        }

        if let Some(picker) = self.picker {
            self.render_prompt(picker, area, cells, style);
            return;
        }

        let Some(buffer) = self.buffer else {
            return;
        };

        // Sits with the path rather than with the cursor position, because it
        // is a fact about the file. In its own colour: the whole point is that
        // it is noticed without being looked for.
        let marker = if buffer.is_stale() { " [stale]" } else { "" };
        let marker_width = text_width(marker);

        let cursor = buffer.cursor();
        // One-based, because that is what every other tool reports. The column
        // counts characters rather than cells: it is the cursor's position in
        // the text, which is also the coordinate the LSP layer will speak in.
        let right = format!("{}:{}", cursor.line.get() + 1, cursor.column.get() + 1);
        let right_width = text_width(&right);

        // One column of padding at each end, at least one between the two
        // halves, and room for the marker, which is never the part that gets
        // dropped.
        let reserved = right_width.saturating_add(3).saturating_add(marker_width);
        let path = relative_to(buffer.path(), self.working_directory)
            .display()
            .to_string();
        let available = usize::from(area.width).saturating_sub(reserved);
        let path = truncate_from_left(&path, available);

        let right_start = usize::from(area.width)
            .saturating_sub(right_width)
            .saturating_sub(1);

        draw(cells, area.x + 1, area.y, &path, style);

        // Only if it fits before the cursor position. On a screen too narrow
        // for both, the position wins: it is there every frame, and half a
        // word of warning is worse than none.
        let after_path = 1usize.saturating_add(text_width(&path));
        if let Ok(offset) = u16::try_from(after_path)
            && !marker.is_empty()
            && after_path + marker_width <= right_start
        {
            draw(
                cells,
                area.x + offset,
                area.y,
                marker,
                style.fg(self.theme.status_stale),
            );
        }

        if let Ok(offset) = u16::try_from(right_start) {
            draw(cells, area.x + offset, area.y, &right, style);
        }
    }
}

/// What the prompt shows.
fn prompt_text(picker: &Picker) -> String {
    format!("> {}", picker.query())
}

/// Which column of the status row the caret belongs in.
///
/// The terminal draws the caret, so this is only where to tell it to put it.
/// Shared with the renderer so the text and the caret cannot disagree.
#[must_use]
pub fn prompt_caret(picker: &Picker) -> u16 {
    let caret = 1usize.saturating_add(text_width(&prompt_text(picker)));
    u16::try_from(caret).unwrap_or(u16::MAX)
}

impl StatusView<'_> {
    /// The prompt: what has been typed.
    fn render_prompt(&self, picker: &Picker, area: Rect, cells: &mut CellBuffer, style: Style) {
        let prompt = prompt_text(picker);
        draw(cells, area.x + 1, area.y, &prompt, style);
        let caret = usize::from(prompt_caret(picker));

        // How much of the list is being shown, on the right, where the cursor
        // position sits the rest of the time.
        let count = format!("{}", picker.match_count());
        let start = usize::from(area.width)
            .saturating_sub(text_width(&count))
            .saturating_sub(1);
        if let Ok(offset) = u16::try_from(start)
            && usize::from(offset) > caret
        {
            draw(
                cells,
                area.x + offset,
                area.y,
                &count,
                style.fg(self.theme.gutter),
            );
        }
    }
}

fn draw(cells: &mut CellBuffer, x: u16, y: u16, contents: &str, style: Style) {
    let mut offset = 0u16;
    for character in contents.chars() {
        let width = u16::try_from(character.width().unwrap_or(0)).unwrap_or(0);
        if let Some(cell) = cells.cell_mut((x + offset, y)) {
            cell.set_char(character);
            cell.set_style(style);
        }
        for extra in 1..width {
            if let Some(cell) = cells.cell_mut((x + offset + extra, y)) {
                cell.set_symbol("");
                cell.set_style(style);
            }
        }
        offset = offset.saturating_add(width.max(1));
    }
}

/// The path as it should be read: relative to the working directory when it
/// lies under it, and unchanged when it does not.
///
/// A reader spends its time inside one tree, and the leading directories of
/// that tree are the part already known.
fn relative_to<'a>(path: &'a Path, root: &Path) -> &'a Path {
    path.strip_prefix(root).unwrap_or(path)
}
