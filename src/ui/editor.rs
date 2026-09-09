//! The gutter and the text.
//!
//! A hand-written widget rather than a `Paragraph`: the text needs per-cell
//! styling, a tab has to expand to the next tab stop, a wide glyph has to
//! occupy two cells, and a glyph straddling either edge of a horizontally
//! scrolled viewport has to render as blanks. None of that survives going
//! through a widget that takes styled spans.

use ratatui::{
    buffer::Buffer as CellBuffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};

use crate::{
    app::App,
    buffer::Buffer,
    coordinates::{CharColumn, LineNumber, Span},
    syntax::highlight::Highlights,
    text::WrapRow,
    theme::Theme,
    ui::{fill, put},
};

/// The narrowest the gutter is allowed to be.
///
/// Enough for four digits and the separating space. A short file would
/// otherwise get a two-column gutter, which is correct and looks starved, and
/// the width would then change from file to file.
const MINIMUM_GUTTER_WIDTH: u16 = 5;

/// The column the scrollbar takes, on the right.
///
/// Always reserved, even for a file that fits: a column that came and went
/// would rewrap the text as files were opened, and an empty track is itself
/// an answer -- it says that what is on screen is all there is.
pub const SCROLLBAR_WIDTH: u16 = 1;

/// How many cells the gutter takes for a document with this many lines.
///
/// Enough digits for the largest line number, plus one column of separation,
/// never narrower than [`MINIMUM_GUTTER_WIDTH`]. No upper bound: showing a
/// wrong line number is worse than spending a column, and a file with more
/// than five digits of lines is rare rather than impossible.
#[must_use]
pub fn gutter_width(line_count: usize) -> u16 {
    let digits = line_count.max(1).ilog10() + 1;
    u16::try_from(digits)
        .unwrap_or(u16::MAX)
        .saturating_add(1)
        .max(MINIMUM_GUTTER_WIDTH)
}

/// The editor region.
pub struct EditorView<'a> {
    buffer: Option<&'a Buffer>,
    highlights: &'a Highlights,
    theme: &'a Theme,
    /// A run of characters to mark, for a preview of somewhere in particular.
    marked: Option<Span>,
}

impl EditorView<'_> {
    /// The bar down the right-hand edge: where in the file this screen is.
    ///
    /// Measured in *lines*, not in visual rows. Counting rows would mean
    /// wrapping every line in the document on every frame, which is the one
    /// thing this program must not do -- and a scrollbar is an indication of
    /// where you are, not a measurement. With wrapping on, a file of very
    /// long lines shows a thumb a little too big; nothing depends on it.
    fn scrollbar(&self, cells: &mut CellBuffer, area: Rect, buffer: &Buffer) {
        crate::ui::scrollbar(
            cells,
            area,
            buffer.viewport().top.get(),
            buffer.text().line_count(),
            self.theme,
        );
    }
}

impl<'a> EditorView<'a> {
    /// Borrows what the view needs from the application.
    #[must_use]
    pub fn new(app: &'a App) -> Self {
        Self {
            buffer: app.current_buffer(),
            highlights: app.highlights(),
            theme: app.theme(),
            marked: None,
        }
    }

    /// Draws a document that is not the one being read.
    ///
    /// What makes a preview look like the editor is that it *is* the editor:
    /// the same gutter, the same highlighting, the same wrapping. A second
    /// drawing path would be a second set of those decisions, and they would
    /// drift.
    /// `marked` is the run of characters the preview is about — the symbol a
    /// language server named. A list of references is read by looking at that
    /// symbol in each one, and a preview that says only which line leaves the
    /// reader finding it again on every row.
    #[must_use]
    pub const fn for_buffer(
        buffer: &'a Buffer,
        highlights: &'a Highlights,
        theme: &'a Theme,
        marked: Option<Span>,
    ) -> Self {
        Self {
            buffer: Some(buffer),
            highlights,
            theme,
            marked,
        }
    }
}

impl Widget for EditorView<'_> {
    fn render(self, area: Rect, cells: &mut CellBuffer) {
        fill(
            cells,
            area,
            Style::new()
                .fg(self.theme.foreground)
                .bg(self.theme.background),
        );

        let Some(buffer) = self.buffer else {
            return;
        };

        let text = buffer.text();
        let gutter = gutter_width(text.line_count()).min(area.width);
        let bar = SCROLLBAR_WIDTH.min(area.width - gutter);
        let width = area.width - gutter - bar;
        if width == 0 {
            return;
        }
        if bar > 0 {
            self.scrollbar(cells, area, buffer);
        }
        let cursor = buffer.cursor();
        let viewport = buffer.viewport();

        let mut screen_row = 0u16;
        let mut line = viewport.top;
        let mut skip = viewport.top_row;

        while screen_row < area.height && line.get() < text.line_count() {
            for (index, wrap) in text.wrap_rows(line, width).into_iter().enumerate() {
                if index < skip {
                    continue;
                }
                if screen_row >= area.height {
                    break;
                }
                let y = area.y + screen_row;

                // Only the first row of a wrapped line is numbered. Repeating
                // the number on every row of one long line is how a wrapped
                // view stops being readable.
                if index == 0 {
                    draw_line_number(
                        area.x,
                        y,
                        gutter,
                        line,
                        cells,
                        if line == cursor.line {
                            self.theme.gutter_current
                        } else {
                            self.theme.gutter
                        },
                    );
                }

                let placement = Placement {
                    x: area.x + gutter,
                    y,
                    width,
                    row: wrap,
                };
                draw_row(
                    placement,
                    buffer,
                    line,
                    cells,
                    self.highlights,
                    self.theme,
                    self.marked,
                );
                screen_row += 1;
            }
            skip = 0;
            line = line.saturating_add(1);
        }
    }
}

/// Writes a right-aligned line number, one-based, with a trailing space.
fn draw_line_number(
    x: u16,
    y: u16,
    width: u16,
    line: LineNumber,
    cells: &mut CellBuffer,
    colour: Color,
) {
    if width == 0 {
        return;
    }
    let label = (line.get() + 1).to_string();
    let digits = u16::try_from(label.len()).unwrap_or(u16::MAX);
    // The separating space is the last column, so the number is right-aligned
    // in the ones before it.
    let padding = width.saturating_sub(1).saturating_sub(digits);
    for (index, character) in label.chars().enumerate() {
        let index = u16::try_from(index).unwrap_or(u16::MAX);
        let Some(offset) = padding.checked_add(index) else {
            break;
        };
        if offset >= width {
            break;
        }
        put(cells, x + offset, y, character, Style::new().fg(colour));
    }
}

/// Where one visual row of text goes.
#[derive(Clone, Copy)]
struct Placement {
    /// The first column of the text area.
    x: u16,
    /// The screen row.
    y: u16,
    /// How many columns the text area has.
    width: u16,
    /// Which slice of the line this row shows.
    row: WrapRow,
}

/// Writes one visual row of a line.
///
/// No clipping and no scroll offset: with wrapping, the row is by construction
/// exactly the characters that fit, so every glyph on it is fully on screen.
/// The one case that needed care — a two-cell glyph cut in half by an edge —
/// is gone, because the wrapping refuses to put one there.
fn draw_row(
    placement: Placement,
    buffer: &Buffer,
    line: LineNumber,
    cells: &mut CellBuffer,
    highlights: &Highlights,
    theme: &Theme,
    marked: Option<Span>,
) {
    let Placement { x, y, width, row } = placement;
    let text = buffer.text();
    let start = usize::from(text.display_column(line, row.first).get());
    let indent = usize::from(row.indent);

    for (column, glyph) in text.glyphs(line).enumerate() {
        if glyph.first_cell < start {
            continue;
        }
        if glyph.first_cell >= usize::from(text.display_column(line, row.end).get()) {
            break;
        }
        let Ok(offset) = u16::try_from(indent + glyph.first_cell - start) else {
            break;
        };
        if offset >= width {
            break;
        }
        let colour = theme.colour_for(highlights.kind_at(glyph.first_byte));

        // A foreground, so the background the fill painted stays — except
        // where the run being marked needs one of its own.
        let mut style = Style::new().fg(colour);
        if marked.is_some_and(|marked| marked.contains(line, CharColumn::new(column))) {
            style = style.bg(theme.marked_background);
        }

        // A tab is blanks by definition.
        if glyph.character == '\t' {
            for cell in 0..glyph.cells.min(usize::from(width - offset)) {
                let Ok(cell) = u16::try_from(cell) else { break };
                put(cells, x + offset + cell, y, ' ', style);
            }
            continue;
        }

        put(cells, x + offset, y, glyph.character, style);
    }
}

#[cfg(test)]
mod tests {
    use super::{MINIMUM_GUTTER_WIDTH, gutter_width};

    #[test]
    fn short_files_get_the_minimum() {
        for lines in [1, 9, 10, 999] {
            assert_eq!(gutter_width(lines), MINIMUM_GUTTER_WIDTH);
        }
    }

    #[test]
    fn the_gutter_grows_once_the_numbers_no_longer_fit() {
        // Five digits plus the separating space is the first width past the
        // minimum, and nothing caps it after that.
        assert_eq!(gutter_width(9_999), 5);
        assert_eq!(gutter_width(10_000), 6);
        assert_eq!(gutter_width(99_999), 6);
        assert_eq!(gutter_width(100_000), 7);
        assert_eq!(gutter_width(1_000_000), 8);
    }
}
