//! The status bar: the file on the left, the cursor on the right.

use std::path::Path;

use ratatui::{buffer::Buffer as CellBuffer, layout::Rect, style::Style, widgets::Widget};

use crate::{
    app::App,
    buffer::Buffer,
    picker::Picker,
    theme::Theme,
    ui::{fill, text_width, truncate_from_left, write},
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
        fill(cells, area, style);

        if let Some(picker) = self.picker {
            self.render_prompt(picker, area, cells, style);
        } else if let Some(buffer) = self.buffer {
            self.render_file(buffer, area, cells, style);
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
    /// The file on the left, the cursor position on the right, and a marker
    /// between them when the file can no longer be read.
    fn render_file(&self, buffer: &Buffer, area: Rect, cells: &mut CellBuffer, style: Style) {
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

        write(cells, area.x + 1, area.y, &path, style);

        // Only if it fits before the cursor position. On a screen too narrow
        // for both, the position wins: it is there every frame, and half a
        // word of warning is worse than none.
        let after_path = 1usize.saturating_add(text_width(&path));
        if let Ok(offset) = u16::try_from(after_path)
            && !marker.is_empty()
            && after_path + marker_width <= right_start
        {
            write(
                cells,
                area.x + offset,
                area.y,
                marker,
                style.fg(self.theme.status_stale),
            );
        }

        if let Ok(offset) = u16::try_from(right_start) {
            write(cells, area.x + offset, area.y, &right, style);
        }
    }

    /// The prompt: what has been typed.
    fn render_prompt(&self, picker: &Picker, area: Rect, cells: &mut CellBuffer, style: Style) {
        let prompt = prompt_text(picker);
        write(cells, area.x + 1, area.y, &prompt, style);
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
            write(
                cells,
                area.x + offset,
                area.y,
                &count,
                style.fg(self.theme.gutter),
            );
        }
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
