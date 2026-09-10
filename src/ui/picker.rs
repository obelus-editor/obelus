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

use crate::{
    app::App,
    component::picker::{Picker, PickerItem, PickerLayout},
    git::FileStatus,
    theme::Theme,
    ui::{drop_from_left, editor::SCROLLBAR_WIDTH, fill, put, text_width},
};

/// How many rows the list keeps for itself.
///
/// Fixed rather than sized to the candidates: a boundary that moved as the
/// query narrowed the list would slide the preview up and down under a reader
/// who is looking at it.
const LIST_ROWS: u16 = 10;

/// The fewest rows worth giving a preview.
///
/// A preview showing three lines has stopped being a preview and become a
/// strip of decoration above the prompt.
const LEAST_PREVIEW_ROWS: u16 = 4;

/// Where the preview goes, if there is room for one.
///
/// Below the list rather than beside it: a terminal is usually wider than one
/// column of paths needs and never taller than it could use, and splitting
/// left and right makes both halves narrow at once.
///
/// The list takes its ten rows and the preview takes the rest, so a taller
/// terminal buys more of the file rather than more file names — which is the
/// way round that matters, since the list is filtered by typing and the
/// preview is not.
///
/// Only for a list whose rows name a file. A palette of commands has nothing
/// to show.
#[must_use]
pub fn preview_region(picker: Option<&Picker>, editor: Rect) -> Option<Rect> {
    let picker = picker?;
    if picker.layout() != PickerLayout::FullArea {
        return None;
    }
    // The list, the rule between them, and enough left to be worth it.
    if editor.height < LIST_ROWS + 1 + LEAST_PREVIEW_ROWS {
        return None;
    }
    Some(Rect {
        y: editor.y + LIST_ROWS + 1,
        height: editor.height - LIST_ROWS - 1,
        ..editor
    })
}

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
    /// it has to, so the code above stays readable. A full-area list gives up
    /// its bottom rows to the preview, and one row between them to the rule
    /// that says they are different things.
    #[must_use]
    pub fn region(&self, editor: Rect) -> Rect {
        match self.picker.layout() {
            PickerLayout::FullArea => match preview_region(Some(self.picker), editor) {
                Some(_) => Rect {
                    height: LIST_ROWS,
                    ..editor
                },
                None => editor,
            },
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
        fill(
            cells,
            area,
            Style::new()
                .fg(self.theme.foreground)
                .bg(self.theme.background),
        );

        // The tabs first, with their own rule under them, and the list below
        // whatever they took. The rule is why a tab row reads as a heading
        // over the list rather than as its first row.
        let tabs = self.picker.tab_rows();
        if tabs > 0 {
            self.tab_row(cells, area);
            crate::ui::rule(
                cells,
                Rect {
                    y: area.y + 1,
                    height: 1,
                    ..area
                },
                self.theme,
            );
        }
        let list = Rect {
            y: area.y + tabs,
            height: area.height.saturating_sub(tabs),
            ..area
        };

        // Nothing to list. A region of background says only that a key did
        // something and nothing came of it; the reason is the whole content
        // of that region. Below the tabs rather than instead of them: an
        // empty tab is the one place a reader most needs to see the others.
        if let Some(reason) = self.picker.nothing_to_show() {
            let style = Style::new().fg(self.theme.gutter).bg(self.theme.background);
            write(cells, list, 1, list.y, reason, style, None, 0);
            return;
        }

        // The same bar the editor has, for the same reason: a window over
        // something longer than itself should say how much longer. The rows
        // give up its column, so a row never draws under it.
        let first = self.picker.first_visible(list.height);
        let rows = Rect {
            width: list.width.saturating_sub(SCROLLBAR_WIDTH),
            ..list
        };
        crate::ui::scrollbar(cells, list, first, self.picker.match_count(), self.theme);

        let selected = self.picker.selected();
        for (row, (index, item)) in self
            .picker
            .matches()
            .enumerate()
            .skip(first)
            .take(usize::from(list.height))
            .enumerate()
        {
            let Ok(row) = u16::try_from(row) else { break };
            self.row(
                cells,
                rows,
                rows.y + row,
                item,
                index == selected,
                self.picker.indices_at(index),
            );
        }
    }
}

impl PickerView<'_> {
    /// The row of tabs, and the arrows that say how to change them.
    ///
    /// The one that is showing gets the selected row's background, which is
    /// the same thing that marks the selected row: on this screen, that
    /// background means "this is the one you are on".
    fn tab_row(&self, cells: &mut CellBuffer, area: Rect) {
        let style = Style::new().fg(self.theme.gutter).bg(self.theme.background);
        fill(cells, Rect { height: 1, ..area }, style);

        let mut column = 1u16;
        for (index, name) in self.picker.tabs().iter().enumerate() {
            let style = if index == self.picker.tab() {
                Style::new()
                    .fg(self.theme.foreground)
                    .bg(self.theme.picker_selected_background)
            } else {
                style
            };
            let padded = format!(" {name} ");
            column = write(cells, area, column, area.y, &padded, style, None, 0);
        }

        // How to move between them. Not a hint that can go stale: the keys
        // are the arrows, and there is nowhere to rebind them to.
        let keys = "\u{2190} \u{2192}";
        if let Ok(offset) =
            u16::try_from(usize::from(area.width).saturating_sub(text_width(keys) + 1))
            && offset > column
        {
            write(cells, area, offset, area.y, keys, style, None, 0);
        }
    }

    /// One row: its background, then its icon, label, detail and key.
    fn row(
        &self,
        cells: &mut CellBuffer,
        area: Rect,
        y: u16,
        item: &PickerItem,
        chosen: bool,
        matched: &[u32],
    ) {
        let background = if chosen {
            self.theme.picker_selected_background
        } else {
            self.theme.background
        };
        let style = Style::new().fg(self.theme.foreground).bg(background);
        // A row that names a thing is coloured by what it names, in the same
        // colours the code itself uses: an outline of a file is a list of
        // its own words, and reading it should feel like reading the file.
        // The matched characters still win over this -- why a row is in the
        // list beats what the row is.
        let label_style = match (item.status, item.kind) {
            // What git says wins over what the syntax layer says: a list of
            // a project's files is mostly files nobody has touched, and the
            // few that have been are what a reader is looking for.
            (Some(FileStatus::Changed), _) => style.fg(self.theme.change_modified),
            (Some(FileStatus::New), _) => style.fg(self.theme.change_added),
            (None, Some(kind)) => style.fg(self.theme.syntax.colour(kind)),
            (None, None) => style,
        };
        fill(
            cells,
            Rect {
                y,
                height: 1,
                ..area
            },
            style,
        );

        // Two columns a level, which is enough to see and cheap enough to
        // spend: an outline of deeply nested code otherwise pushes the names
        // off the row it is meant to be showing.
        let mut column = 1u16.saturating_add(item.depth.saturating_mul(2).min(area.width / 3));
        if let Some(icon) = item.icon {
            let mut glyph = String::new();
            glyph.push(icon);
            column = write(cells, area, column, y, &glyph, style, None, 0);
            // One blank column after it, always. The terminal allocates one
            // cell for a private-use codepoint, and the icons in a Nerd
            // Font's non-`Mono` variant are drawn two cells wide, so the
            // glyph bleeds to the right. This is what it bleeds into.
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

        // A path too long for the row loses its head, not its tail: the file
        // name is the part being looked for, and the directories above it are
        // the part already known.
        let dropped = drop_from_left(&item.label, usize::from(limit.saturating_sub(column)));
        if dropped >= item.label.chars().count() {
            // Not even room for the ellipsis.
            return;
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
            label_style,
            (!matched.is_empty()).then_some((matched, self.theme.picker_match)),
            dropped,
        );

        let dim = style.fg(self.theme.gutter);
        if let Some(detail) = &item.detail {
            column = write(
                cells,
                inner,
                column.saturating_add(2),
                y,
                detail,
                dim,
                None,
                0,
            );
        }

        if !trailing.is_empty()
            && let Ok(offset) =
                u16::try_from(usize::from(area.width).saturating_sub(text_width(trailing) + 1))
            && offset >= column
        {
            write(cells, area, offset, y, trailing, dim, None, 0);
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
        if column >= area.width {
            break;
        }
        let index = u32::try_from(index).unwrap_or(u32::MAX);
        let style = match matched {
            Some((indices, colour)) if indices.binary_search(&index).is_ok() => style.fg(colour),
            _ => style,
        };
        column = column.saturating_add(put(cells, area.x + column, y, character, style));
    }
    column
}
