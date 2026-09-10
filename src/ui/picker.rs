//! The prompt's list.
//!
//! Both layouts occupy whole rows, which is why nothing here has to deal with
//! a double-width glyph cut in half by a vertical edge: a partial-width
//! overlay would leave one cell of a pair showing the code underneath and the
//! other showing the list.

use ratatui::{buffer::Buffer as CellBuffer, layout::Rect, style::Style, widgets::Widget};

use crate::{
    app::App,
    component::picker::{Picker, PickerItem, PickerLayout},
    git::FileStatus,
    theme::Theme,
    ui::{Marked, Matched, drop_from_left, editor::SCROLLBAR_WIDTH, fill, text_width},
};

/// How many rows of the list a reader gets to walk.
///
/// Fixed rather than sized to the candidates: a boundary that moved as the
/// query narrowed the list would slide the preview up and down under a reader
/// who is looking at it.
///
/// Rows of the *list*, not of the region it is drawn in: a list with tabs
/// spends its first two rows on them, and a reader who asked for ten rows
/// meant ten rows to walk.
const LIST_ROWS: u16 = 10;

/// How tall the whole list is: its rows, and the tabs over them.
fn list_region_rows(picker: &Picker) -> u16 {
    LIST_ROWS.saturating_add(picker.tab_rows())
}

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
    let rows = list_region_rows(picker);
    if editor.height < rows + 1 + LEAST_PREVIEW_ROWS {
        return None;
    }
    Some(Rect {
        y: editor.y + rows + 1,
        height: editor.height - rows - 1,
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

    /// The same view over a list that is not the one taking keys.
    ///
    /// For a list that follows what is being typed somewhere else -- the
    /// commands an agent takes, against the box a message is written in.
    /// It is drawn by the same code as every other list, because it is the
    /// same thing to a reader: rows, one of them chosen, what matched
    /// marked.
    #[must_use]
    pub const fn over(picker: &'a Picker, theme: &'a Theme) -> Self {
        Self { picker, theme }
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
                    height: list_region_rows(self.picker),
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

impl PickerView<'_> {
    /// The rows of the list, within the region it is drawn in.
    ///
    /// A list with tabs keeps its first two rows for them -- the tabs and
    /// the rule under them -- so what a reader walks is what is left. One
    /// function, shared with the drawing and with everything that has to
    /// know how many rows are on screen: a window settled on a height the
    /// rows do not have scrolls before the last row it drew, and a page
    /// steps further than the reader can see.
    #[must_use]
    pub fn rows_region(&self, region: Rect) -> Rect {
        let tabs = self.picker.tab_rows();
        Rect {
            y: region.y + tabs,
            height: region.height.saturating_sub(tabs),
            ..region
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
        let list = self.rows_region(area);
        let tabs = self.picker.tab_rows();
        if tabs > 0 {
            crate::ui::tabs(
                cells,
                area,
                self.picker.tabs(),
                self.picker.tab(),
                self.theme,
            );
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

        // Nothing to list. A region of background says only that a key did
        // something and nothing came of it; the reason is the whole content
        // of that region. Below the tabs rather than instead of them: an
        // empty tab is the one place a reader most needs to see the others.
        if let Some(reason) = self.picker.nothing_to_show() {
            crate::ui::nothing(cells, list, reason, self.theme);
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
        // Only when the matches do not fit, which is the window's own
        // answer -- the same one every other list asks it for.
        let matched = self.picker.match_count();
        if self.picker.window().scrollable(list.height) {
            crate::ui::scrollbar(cells, list, first, matched, self.theme);
        }

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
        // A row that cannot be chosen is drawn in the colour the gutter uses
        // -- present, and plainly not for now. Its own colours are dropped
        // with it: a dim row with a bright name in it reads as available.
        let style = if item.enabled {
            Style::new().fg(self.theme.foreground).bg(background)
        } else {
            Style::new().fg(self.theme.gutter).bg(background)
        };
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
            column = at(cells, area, column, y, &glyph, style, &Marked::plain());
            // One blank column after it, always. The terminal allocates one
            // cell for a private-use codepoint, and the icons in a Nerd
            // Font's non-`Mono` variant are drawn two cells wide, so the
            // glyph bleeds to the right. This is what it bleeds into.
            column = column.saturating_add(1);
        }

        // What the right-aligned text needs, plus a gap, comes out of
        // everything else's room first -- but never more than half the row.
        // It used to be the one part that never got cut, which was right
        // while it held a key hint or a line number; a search row's trailing
        // is a path, and a path longer than the row left the label with no
        // columns at all: a list of icons with nothing beside them.
        let trailing = item.trailing.as_deref().unwrap_or_default();
        let wanted = if trailing.is_empty() {
            0
        } else {
            u16::try_from(text_width(trailing) + 2).unwrap_or(u16::MAX)
        };
        let reserved = wanted.min(area.width / 2);
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
            column = at(cells, inner, column, y, "\u{2026}", style, &Marked::plain());
        }
        column = at(
            cells,
            inner,
            column,
            y,
            &item.label,
            label_style,
            &Marked {
                matched: Matched::Indices(matched),
                mark: self.theme.picker_match_background,
                syntax: item
                    .colours
                    .as_deref()
                    .filter(|runs| !runs.is_empty())
                    .map(|runs| (runs, self.theme)),
                skip: dropped,
            },
        );

        let dim = style.fg(self.theme.gutter);
        if let Some(detail) = &item.detail {
            column = at(
                cells,
                inner,
                column.saturating_add(2),
                y,
                detail,
                dim,
                &Marked::plain(),
            );
        }

        if !trailing.is_empty() {
            // Cut from the left, like a label: the end of a path is the file
            // name and the line, which is the part that says where to go.
            let room = usize::from(reserved.saturating_sub(2));
            let dropped = drop_from_left(trailing, room);
            let shown = trailing.chars().count().saturating_sub(dropped);
            if shown > 0
                && let Ok(offset) =
                    u16::try_from(usize::from(area.width).saturating_sub(text_width(trailing) + 1))
                        .map(|offset| {
                            offset.max(area.width.saturating_sub(reserved).saturating_add(1))
                        })
                && offset >= column
            {
                if dropped > 0 {
                    at(cells, area, offset, y, "\u{2026}", dim, &Marked::plain());
                    at(
                        cells,
                        area,
                        offset + 1,
                        y,
                        trailing,
                        dim,
                        &Marked {
                            skip: dropped + 1,
                            ..Marked::plain()
                        },
                    );
                } else {
                    at(cells, area, offset, y, trailing, dim, &Marked::plain());
                }
            }
        }
    }
}

/// The shared row writer, for a column counted from the row's own left
/// edge rather than from the screen's.
///
/// Every list in obelus draws its characters through
/// [`crate::ui::write_marked`] -- what marks a match, what colours a line of
/// code, what a truncated head skips. A picker's rows are laid out relative
/// to the row, so this is the one line of arithmetic between the two.
fn at(
    cells: &mut CellBuffer,
    area: Rect,
    column: u16,
    y: u16,
    contents: &str,
    style: Style,
    marked: &Marked<'_>,
) -> u16 {
    crate::ui::write_marked(cells, area, area.x + column, y, contents, style, marked)
        .saturating_sub(area.x)
}
