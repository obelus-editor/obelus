//! The prompt's list.
//!
//! Both layouts occupy whole rows, which is why nothing here has to deal with
//! a double-width glyph cut in half by a vertical edge: a partial-width
//! overlay would leave one cell of a pair showing the code underneath and the
//! other showing the list.

use ratatui::{
    buffer::Buffer as CellBuffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};
use unicode_width::UnicodeWidthChar;

use crate::{
    app::App,
    picker::{Picker, PickerLayout},
    theme::Theme,
    ui::{drop_from_left, text_width},
};

/// The list, above the prompt.
pub struct PickerView<'a> {
    picker: &'a Picker,
    theme: &'a Theme,
}

impl<'a> PickerView<'a> {
    /// Borrows what the view needs, or nothing if no picker is open.
    #[must_use]
    pub fn new(app: &'a App) -> Option<Self> {
        Some(Self {
            picker: app.picker()?,
            theme: app.theme(),
        })
    }

    /// Where the list goes within the editor region.
    ///
    /// A compact list sits on the bottom edge and grows upwards only as far as
    /// it has to, so the code above stays readable.
    #[must_use]
    pub fn region(&self, editor: Rect) -> Rect {
        match self.picker.layout() {
            PickerLayout::FullArea => editor,
            PickerLayout::Compact { .. } => {
                let wanted = self.picker.visible_rows(editor.height);
                Rect {
                    y: editor.y + editor.height - wanted,
                    height: wanted,
                    ..editor
                }
            }
        }
    }
}

impl Widget for PickerView<'_> {
    fn render(self, area: Rect, cells: &mut CellBuffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        // The whole region first. A list with fewer rows than the region
        // would otherwise leave the code showing underneath, which reads as a
        // half-drawn screen rather than as a short list.
        let empty = Style::new()
            .fg(self.theme.foreground)
            .bg(self.theme.background);
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                if let Some(cell) = cells.cell_mut((x, y)) {
                    cell.set_symbol(" ");
                    cell.set_style(empty);
                }
            }
        }

        let selected = self.picker.selected();
        // The window is placed so the selection sits near the middle of it,
        // clamped at both ends of the list. Anchoring the selection to the
        // bottom row instead is simpler and reads badly once paging exists:
        // every move slides the whole list under a cursor that never moves.
        let height = usize::from(area.height);
        let count = self.picker.match_count();
        let first = selected
            .saturating_sub(height / 2)
            .min(count.saturating_sub(height));

        for (row, (index, item)) in self
            .picker
            .matches()
            .enumerate()
            .skip(first)
            .take(height)
            .enumerate()
        {
            let Ok(row) = u16::try_from(row) else { break };
            let y = area.y + row;
            let chosen = index == selected;

            let background = if chosen {
                self.theme.picker_selected_background
            } else {
                self.theme.background
            };
            let style = Style::new().fg(self.theme.foreground).bg(background);
            for x in area.left()..area.right() {
                if let Some(cell) = cells.cell_mut((x, y)) {
                    cell.set_symbol(" ");
                    cell.set_style(style);
                }
            }

            let mut column = 1u16;
            if let Some(icon) = item.icon {
                let mut glyph = String::new();
                glyph.push(icon);
                column = write(cells, area, column, y, &glyph, style, None, 0);
                // One blank column after it, always. The terminal allocates
                // one cell for a private-use codepoint, and the icons in a
                // Nerd Font's non-`Mono` variant are drawn two cells wide, so
                // the glyph bleeds to the right. This is what it bleeds into.
                column = column.saturating_add(1);
            }

            // What the right-aligned text needs, plus a gap, comes out of
            // everything else's room first: it is the one part of a row that
            // never gets cut.
            let trailing = item.trailing.as_deref().unwrap_or_default();
            let reserved = if trailing.is_empty() {
                0
            } else {
                u16::try_from(text_width(trailing) + 2).unwrap_or(u16::MAX)
            };
            let limit = area.width.saturating_sub(1).saturating_sub(reserved);
            let inner = Rect {
                width: limit,
                ..area
            };

            // A path too long for the row loses its head, not its tail: the
            // file name is the part being looked for, and the directories
            // above it are the part already known.
            let room = usize::from(limit.saturating_sub(column));
            let dropped = drop_from_left(&item.label, room);
            if dropped >= item.label.chars().count() {
                // Not even room for the ellipsis.
                continue;
            }
            if dropped > 0 {
                column = write(cells, inner, column, y, "\u{2026}", style, None, 0);
            }
            column = write(
                cells,
                inner,
                column,
                y,
                &item.label,
                style,
                if chosen {
                    Some((self.picker.selected_indices(), self.theme.picker_match))
                } else {
                    None
                },
                dropped,
            );

            if let Some(detail) = &item.detail {
                column = column.saturating_add(2);
                write(
                    cells,
                    inner,
                    column,
                    y,
                    detail,
                    style.fg(self.theme.gutter),
                    None,
                    0,
                );
            }

            if !trailing.is_empty()
                && let Ok(offset) =
                    u16::try_from(usize::from(area.width).saturating_sub(text_width(trailing) + 1))
                && offset >= column
            {
                write(
                    cells,
                    area,
                    offset,
                    y,
                    trailing,
                    style.fg(self.theme.gutter),
                    None,
                    0,
                );
            }
        }
    }
}

/// Writes text at a column, optionally colouring the matched characters.
///
/// `skip` leading characters are not drawn, for text whose head has been
/// truncated away. The matched positions are still counted from the start of
/// the whole text, so a match that fell in the dropped part simply has no
/// character left to colour.
///
/// Returns the column after the text.
#[expect(
    clippy::too_many_arguments,
    reason = "each is a distinct piece of where and how; a struct moves the same list one line up"
)]
fn write(
    cells: &mut CellBuffer,
    area: Rect,
    start: u16,
    y: u16,
    contents: &str,
    style: Style,
    matched: Option<(&[u32], Color)>,
    skip: usize,
) -> u16 {
    let mut column = start;
    for (index, character) in contents.chars().enumerate().skip(skip) {
        let width = u16::try_from(character.width().unwrap_or(0)).unwrap_or(0);
        if column >= area.width {
            break;
        }
        let index = u32::try_from(index).unwrap_or(u32::MAX);
        let style = match matched {
            Some((indices, colour)) if indices.binary_search(&index).is_ok() => style.fg(colour),
            _ => style,
        };
        if let Some(cell) = cells.cell_mut((area.x + column, y)) {
            cell.set_char(character);
            cell.set_style(style);
        }
        for extra in 1..width {
            if let Some(cell) = cells.cell_mut((area.x + column + extra, y)) {
                cell.set_symbol("");
                cell.set_style(style);
            }
        }
        column = column.saturating_add(width.max(1));
    }
    column
}
