//! Drawing.
//!
//! Nothing in here reads a file, makes a syscall or parses anything. The draw
//! path runs inside `Terminal::draw`, which blocks the main loop on a write to
//! stdout; adding slow work to it is the mistake that actually happens, rather
//! than the write itself being slow.

pub mod editor;
pub mod picker;
pub mod status;
pub mod welcome;

use ratatui::{
    buffer::Buffer as CellBuffer,
    layout::{Position, Rect, Size},
    style::Style,
    widgets::Widget as _,
};
use unicode_width::UnicodeWidthChar as _;

use crate::{app::App, theme::Theme};

/// Where the two regions of the screen are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Regions {
    /// The gutter and the text.
    pub editor: Rect,
    /// The one-line status bar.
    pub status: Rect,
}

/// Splits the screen.
///
/// Called both before drawing, to scroll the cursor into view, and while
/// drawing. One function so the two cannot disagree about where the boundary
/// is.
#[must_use]
pub fn regions(area: Rect) -> Regions {
    let status_height = area.height.min(1);
    let editor_height = area.height - status_height;
    Regions {
        editor: Rect {
            height: editor_height,
            ..area
        },
        status: Rect {
            y: area.y + editor_height,
            height: status_height,
            ..area
        },
    }
}

/// The screen as a `Rect` starting at the origin.
#[must_use]
pub fn area_of(size: Size) -> Rect {
    Rect {
        x: 0,
        y: 0,
        width: size.width,
        height: size.height,
    }
}

/// Where the terminal should put its cursor, if anywhere.
///
/// The terminal's own cursor rather than a painted block, so it takes the
/// shape and the blink the reader configured, and — the reason this matters —
/// goes hollow by itself when the window loses focus. A cell grid cannot
/// express that: the terminal draws an outline over the cell, and an
/// application can only put characters in it.
///
/// While a picker is open the cursor belongs in the prompt, which is also
/// where the keys are going.
#[must_use]
pub fn cursor_position(area: Rect, app: &App) -> Option<Position> {
    let regions = regions(area);

    if let Some(picker) = app.picker() {
        let column = status::prompt_caret(picker);
        return (column < regions.status.width).then(|| Position {
            x: regions.status.x + column,
            y: regions.status.y,
        });
    }

    let buffer = app.current_buffer()?;
    let gutter = editor::gutter_width(buffer.text().line_count());
    if gutter >= regions.editor.width {
        return None;
    }
    let (row, cell) = buffer.cursor_screen_cell(app.text_area())?;
    if row >= regions.editor.height || cell >= regions.editor.width - gutter {
        return None;
    }
    Some(Position {
        x: regions.editor.x + gutter + cell,
        y: regions.editor.y + row,
    })
}

/// Draws one frame into a cell grid.
///
/// Takes the grid rather than a `Frame` so the golden tests can assert on the
/// cells obelus wrote. Going through a `Frame` would mean reading them back
/// from the backend afterwards, and by then ratatui's diff has dropped the
/// cell a wide glyph covers — correctly, since the terminal advances two
/// columns for it, but the record left behind cannot be told apart from a cell
/// nothing painted.
pub fn draw(cells: &mut CellBuffer, area: Rect, app: &App) {
    let regions = regions(area);
    editor::EditorView::new(app).render(regions.editor, cells);
    // Nothing open: the editor region has been painted and is otherwise
    // empty, which is the one moment a reader needs telling what the keys are.
    if app.current_buffer().is_none() {
        welcome::WelcomeView::new(app).render(regions.editor, cells);
    }
    // Over the code, because a compact list is meant to leave the code above
    // it visible.
    if let Some(view) = picker::PickerView::new(app) {
        let region = view.region(regions.editor);
        view.render(region, cells);

        // A compact list sits on top of the code, so it needs an edge: the
        // same rule the preview gets, for the same reason, which is that two
        // different things sharing a screen have to be told apart. A list
        // filling the whole region has no room above it and needs none.
        if region.y > regions.editor.y {
            rule(
                cells,
                Rect {
                    y: region.y - 1,
                    height: 1,
                    ..region
                },
                app.theme(),
            );
        }

        // Below the list, with a rule between them. The preview is drawn by
        // the editor's own view, which is what makes it look like the editor.
        if let Some(preview) = picker::preview_region(app.picker(), regions.editor) {
            rule(
                cells,
                Rect {
                    y: preview.y - 1,
                    height: 1,
                    ..preview
                },
                app.theme(),
            );

            match app.preview() {
                Some((buffer, highlights, marked)) => {
                    editor::EditorView::for_buffer(buffer, highlights, app.theme(), marked)
                        .render(preview, cells);
                }
                // Room set aside and nothing to put in it: a file that has
                // gone, or a row that names no file.
                None => fill(cells, preview, Style::new().bg(app.theme().background)),
            }
        }
    }
    status::StatusView::new(app).render(regions.status, cells);
}

/// One row of rule, saying that what is above it and what is below it are
/// different things.
///
/// Filled first: the row it goes on held code a moment ago, and a rule drawn
/// over the top of that would have the code showing between its cells.
pub(crate) fn rule(cells: &mut CellBuffer, area: Rect, theme: &Theme) {
    fill(cells, area, Style::new().bg(theme.background));
    for x in area.left()..area.right() {
        put(cells, x, area.y, '\u{2500}', Style::new().fg(theme.gutter));
    }
}

/// A bar down the right-hand edge of a region: where its window sits.
///
/// Shared by the editor and the lists, because it is the same question in
/// both -- how much of this is on screen, and which part -- and two
/// implementations would answer it in two shapes.
///
/// `total` is how many rows the whole thing has and `top` which of them is
/// on the first row. A `total` that fits leaves the track empty, which is
/// itself an answer: what you see is all there is.
pub(crate) fn scrollbar(
    cells: &mut CellBuffer,
    area: Rect,
    top: usize,
    total: usize,
    theme: &Theme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let height = usize::from(area.height);
    let total = total.max(1);
    let x = area.right().saturating_sub(1);

    // At least one row of thumb, or a long list has a bar with nothing on it.
    let thumb = (height * height / total).clamp(1, height);
    let travel = height.saturating_sub(thumb);
    // Scaled by how far the *top* can travel, not by the total: dividing by
    // the total leaves the thumb short of the bottom exactly when the last
    // row is on screen, which is the one position anyone checks it against.
    let furthest = total.saturating_sub(height).max(1);
    let start = if total <= height {
        0
    } else {
        (top * travel).div_ceil(furthest).min(travel)
    };

    for row in 0..area.height {
        let inside = usize::from(row) >= start && usize::from(row) < start + thumb;
        let (glyph, colour) = if inside {
            ('\u{2588}', theme.gutter_current)
        } else {
            ('\u{2502}', theme.gutter)
        };
        put(cells, x, area.y + row, glyph, Style::new().fg(colour));
    }
}

/// Paints every cell of a region in one style, blanking whatever was there.
pub fn fill(cells: &mut CellBuffer, area: Rect, style: Style) {
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if let Some(cell) = cells.cell_mut((x, y)) {
                cell.set_symbol(" ");
                cell.set_style(style);
            }
        }
    }
}

/// Writes one character, and blanks the cells it covers beyond the first.
///
/// A wide glyph owns the cells after it, and they must hold no symbol at all:
/// the terminal advances two columns for the glyph, so anything left in the
/// second cell shifts the rest of the row. That is the rule in here that
/// breaks silently, which is why there is one copy of it.
///
/// The style is patched onto the cell rather than replacing it, so a caller
/// that only wants to set a foreground can pass one and keep whatever
/// background was painted underneath.
///
/// Returns how many columns were used, never zero: a character the terminal
/// does not advance over still advances this, or a caller stepping through a
/// string would not terminate.
pub fn put(cells: &mut CellBuffer, x: u16, y: u16, character: char, style: Style) -> u16 {
    let width = u16::try_from(character.width().unwrap_or(0)).unwrap_or(0);
    if let Some(cell) = cells.cell_mut((x, y)) {
        cell.set_char(character);
        cell.set_style(style);
    }
    for extra in 1..width {
        if let Some(cell) = cells.cell_mut((x + extra, y)) {
            cell.set_symbol("");
            cell.set_style(style);
        }
    }
    width.max(1)
}

/// Writes a string, returning the column after it.
pub fn write(cells: &mut CellBuffer, x: u16, y: u16, contents: &str, style: Style) -> u16 {
    let mut column = x;
    for character in contents.chars() {
        column = column.saturating_add(put(cells, column, y, character, style));
    }
    column
}

/// How many cells a string occupies.
#[must_use]
pub fn text_width(contents: &str) -> usize {
    contents
        .chars()
        .map(|character| character.width().unwrap_or(0))
        .sum()
}

/// How many leading characters to drop so the rest of `contents` fits in
/// `cells`, with one cell left for the ellipsis that marks the cut.
///
/// From the left, because the end is the part worth reading: the file name in
/// a path, the last component of a symbol. The leading directories are the
/// part already known. Measured in cells rather than characters so a path with
/// wide glyphs in it does not overrun whatever comes after it.
///
/// Zero when it already fits. Everything when there is no room even for the
/// ellipsis, so the caller can draw nothing rather than a lone `…`.
#[must_use]
pub fn drop_from_left(contents: &str, cells: usize) -> usize {
    if text_width(contents) <= cells {
        return 0;
    }
    let total = contents.chars().count();
    if cells <= 1 {
        return total;
    }

    let budget = cells - 1;
    let mut kept = 0usize;
    let mut width = 0usize;
    for character in contents.chars().rev() {
        let character_width = character.width().unwrap_or(0);
        if width + character_width > budget {
            break;
        }
        width += character_width;
        kept += 1;
    }
    total - kept
}

/// `contents` with its head replaced by an ellipsis if it does not fit.
#[must_use]
pub fn truncate_from_left(contents: &str, cells: usize) -> String {
    let dropped = drop_from_left(contents, cells);
    if dropped == 0 {
        return contents.to_string();
    }
    let total = contents.chars().count();
    if dropped >= total {
        return String::new();
    }
    let mut result = String::from('\u{2026}');
    result.extend(contents.chars().skip(dropped));
    result
}

#[cfg(test)]
mod tests {
    use super::{drop_from_left, truncate_from_left};

    #[test]
    fn a_path_that_fits_is_left_alone() {
        assert_eq!(drop_from_left("src/app.rs", 20), 0);
        assert_eq!(truncate_from_left("src/app.rs", 20), "src/app.rs");
    }

    /// The file name survives; the directories above it are what goes.
    #[test]
    fn the_end_survives_the_cut() {
        let truncated = truncate_from_left("a/very/deep/path/to/app.rs", 12);
        assert_eq!(truncated, "\u{2026}h/to/app.rs");
        assert_eq!(truncated.chars().count(), 12);
    }

    /// Cells, not characters: a wide glyph costs two, and counting characters
    /// would overrun whatever is drawn after the text.
    #[test]
    fn wide_glyphs_are_counted_by_the_cells_they_take() {
        // Five wide glyphs are ten cells. Six cells hold the ellipsis and two
        // glyphs; a third would need a seventh cell.
        let glyphs = "\u{4f60}\u{597d}\u{4e16}\u{754c}\u{554a}";
        assert_eq!(truncate_from_left(glyphs, 6), "\u{2026}\u{754c}\u{554a}");
        assert_eq!(super::text_width(&truncate_from_left(glyphs, 6)), 5);
    }

    /// The property the rest of the layout depends on: whatever comes back
    /// fits. Anything wider would be drawn over the row's other columns.
    #[test]
    fn the_result_never_exceeds_the_room_it_was_given() {
        let samples = [
            "src/app.rs",
            "a/very/deep/path/to/somewhere/app.rs",
            "\u{4f60}\u{597d}\u{4e16}\u{754c}\u{554a}/mixed/\u{8def}\u{5f84}.rs",
            "\tindented",
            "",
        ];
        for contents in samples {
            for cells in 0..40usize {
                let width = super::text_width(&truncate_from_left(contents, cells));
                assert!(
                    width <= cells || width == 0,
                    "{contents:?} at {cells} cells came back {width} wide"
                );
            }
        }
    }

    /// No room even for the ellipsis, so the caller can draw nothing rather
    /// than a lone `\u{2026}` that says only that something was hidden.
    #[test]
    fn nothing_fits_in_one_cell() {
        assert_eq!(drop_from_left("src/app.rs", 1), 10);
        assert_eq!(truncate_from_left("src/app.rs", 1), "");
    }
}
