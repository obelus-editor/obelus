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
    coordinates::{ByteOffset, CharColumn, LineNumber, Span},
    git::{Changes, Marker},
    syntax::{brackets, highlight::Highlights},
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

/// The column the change markers take, on the left.
///
/// Reserved whenever obelus has an answer about the file -- that is, when it
/// is in a repository -- and not otherwise. A column that came and went as
/// the file was *edited* would rewrap the text under the reader; one that
/// depends only on which file is open does not.
pub const MARGIN_WIDTH: u16 = 1;

/// The column the change map takes, right of the scrollbar.
///
/// One column, the same width as the margin on the other side, and drawn
/// with the same glyph: the two are one answer at two scales -- what changed
/// on this line, and where else in the file to look. Reserved on the same
/// terms as the margin, so a file obelus knows nothing about spends nothing.
pub const CHANGE_MAP_WIDTH: u16 = 1;

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
    /// The characters selected in the file being read.
    selection: Option<Span>,
    /// What has changed since the last commit, if obelus knows.
    ///
    /// `None` for a file outside a repository and for a preview, and then
    /// the margin takes no column at all.
    changes: Option<&'a Changes>,
    /// The hunk the reader has opened, if any.
    opened: Option<LineNumber>,
}

impl EditorView<'_> {
    /// The bar down the right-hand edge: where in the file this screen is.
    ///
    /// Measured in *lines*, not in visual rows. Counting rows would mean
    /// wrapping every line in the document on every frame, which is the one
    /// thing this program must not do -- and a scrollbar is an indication of
    /// where you are, not a measurement. With wrapping on, a file of very
    /// long lines shows a thumb a little too big; nothing depends on it.
    /// Every change in the file, in one column beside the bar.
    ///
    /// Not only the changes on screen: the margin says what changed *here*,
    /// and this says where else to look. Its rows are lines of the *file*,
    /// the same mapping the bar uses, so a mark is level with the part of
    /// the bar that would bring it into view.
    fn change_map(&self, cells: &mut CellBuffer, area: Rect, buffer: &Buffer) {
        let Some(changes) = self.changes else {
            return;
        };
        let total = buffer.text().line_count();
        for hunk in changes.hunks() {
            let marker = hunk.marker();
            let first = crate::ui::bar_row(hunk.line.get(), total, area.height);
            // At least the row it starts on, so a change of one line is not
            // lost to the arithmetic, and every row a long one covers, so
            // that a rewrite does not read like a one-line fix.
            let last =
                crate::ui::bar_row(hunk.line.get() + hunk.lines.max(1) - 1, total, area.height)
                    .max(first);
            for row in first..=last {
                draw_marker(
                    area.x,
                    area.y + row,
                    marker,
                    self.marker_colour(marker),
                    cells,
                );
            }
        }
    }

    /// The colour a marker is drawn in, wherever it is drawn.
    const fn marker_colour(&self, marker: Marker) -> Color {
        match marker {
            Marker::Added => self.theme.change_added,
            Marker::Modified => self.theme.change_modified,
            Marker::Removed => self.theme.change_removed,
        }
    }

    /// The tint behind a line of an opened hunk.
    const fn marker_background(&self, marker: Marker) -> Color {
        match marker {
            Marker::Added => self.theme.change_added_background,
            Marker::Modified => self.theme.change_modified_background,
            Marker::Removed => self.theme.change_removed_background,
        }
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
            selection: app.current_buffer().and_then(Buffer::selection),
            changes: app.changes(),
            opened: app.opened_hunk(),
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
            selection: None,
            // A preview is about somewhere else in a file the reader is not
            // editing; a margin of change markers beside it would be about a
            // question nobody asked.
            changes: None,
            opened: None,
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
        // The margin, then the gutter, then the text, then the scrollbar.
        // The margin is leftmost because it is about the line as a whole and
        // the line number is about where it is: a mark inside the numbers
        // would read as part of one.
        let margin = if self.changes.is_some() {
            MARGIN_WIDTH.min(area.width)
        } else {
            0
        };
        let gutter = gutter_width(text.line_count()).min(area.width - margin);
        let map = if self.changes.is_some() {
            CHANGE_MAP_WIDTH.min(area.width - margin - gutter)
        } else {
            0
        };
        let bar = SCROLLBAR_WIDTH.min(area.width - margin - gutter - map);
        let width = area.width - margin - gutter - bar - map;
        if width == 0 {
            return;
        }
        if bar > 0 {
            let track = Rect {
                width: area.width - map,
                ..area
            };
            crate::ui::scrollbar(
                cells,
                track,
                buffer.viewport().top.get(),
                text.line_count(),
                self.theme,
            );
        }
        if map > 0 {
            let column = Rect {
                x: area.right() - map,
                width: map,
                ..area
            };
            self.change_map(cells, column, buffer);
        }
        let cursor = buffer.cursor();
        let viewport = buffer.viewport();

        // The bracket pair, worked out once for the frame rather than per
        // row: it is one scan over what is on screen, and every row asks the
        // same question.
        let visible = crate::app::visible_bytes(buffer, area.height);
        let at = text.byte_of_char(text.char_offset(cursor.line, cursor.column));
        let brackets = brackets::pair_at(text, self.highlights, at, visible);

        // The hunk the reader has opened, worked out once: every row asks
        // whether it is one of its lines.
        let opened = self.opened.and_then(|anchor| {
            self.changes
                .and_then(|changes| changes.hunk_at(anchor))
                .map(|hunk| (hunk, self.marker_background(hunk.marker())))
        });

        let mut screen_row = 0u16;
        let mut line = viewport.top;
        let mut skip = viewport.top_row;

        while screen_row < area.height && line.get() < text.line_count() {
            // What this line replaced, if the reader has opened it. Above the
            // line, because that is where it was, and pushing the file down
            // rather than overwriting anything: text that is not in the file
            // must not look like text that is.
            if skip == 0
                && self.opened == Some(line)
                && let Some(changes) = self.changes
                && let Some(hunk) = changes.hunk_at(line)
            {
                for removed in &hunk.removed {
                    if screen_row >= area.height {
                        break;
                    }
                    let y = area.y + screen_row;
                    // Filled first, so the text below is written onto the
                    // tint rather than the tint over the text.
                    fill(
                        cells,
                        Rect {
                            x: area.x + margin,
                            y,
                            width: gutter + width,
                            height: 1,
                        },
                        Style::new()
                            .fg(self.theme.foreground)
                            .bg(self.theme.change_removed_background),
                    );
                    // The bar a line on screen gets, not the boundary mark:
                    // `Marker::Removed`'s top edge exists because deleted
                    // lines have no row of their own, and opening the hunk
                    // is exactly the act of giving them one. The colour
                    // still says they are gone.
                    draw_marker(
                        area.x,
                        y,
                        Marker::Modified,
                        self.theme.change_removed,
                        cells,
                    );
                    // No line number: these lines have no number in this
                    // file, and borrowing the next one's would be a lie
                    // about where they are.
                    // No background of its own: the fill above already put
                    // the tint on this row, and a style that names one paints
                    // over it wherever there is a glyph -- which leaves the
                    // colour showing in the gaps between words and nowhere
                    // else. The ordinary foreground, because the row's colour
                    // is now what says these lines are gone, and red text on
                    // a red row is a line nobody can read.
                    crate::ui::write(
                        cells,
                        area.x + margin + gutter,
                        y,
                        removed,
                        Style::new().fg(self.theme.foreground),
                    );
                    screen_row += 1;
                }
            }

            for (index, wrap) in text.wrap_rows(line, width).into_iter().enumerate() {
                if index < skip {
                    continue;
                }
                if screen_row >= area.height {
                    break;
                }
                let y = area.y + screen_row;

                // Behind the lines of an opened hunk: what kind of change
                // this is, said by the whole row. Not the margin column and
                // not the bar, which have marks of their own to stay legible
                // -- from the line number across to the end of the text, so
                // the block reads as one thing.
                if let Some((hunk, tint)) = opened
                    && hunk.covers(line)
                {
                    fill(
                        cells,
                        Rect {
                            x: area.x + margin,
                            y,
                            width: gutter + width,
                            height: 1,
                        },
                        Style::new().fg(self.theme.foreground).bg(tint),
                    );
                }

                // Only the first row of a wrapped line is numbered. Repeating
                // the number on every row of one long line is how a wrapped
                // view stops being readable.
                // The margin marks the line, whether or not this is the
                // row its number is on: a wrapped line is one line, and a
                // change to it is a change to all of it.
                if margin > 0
                    && let Some(changes) = self.changes
                    && let Some(marker) = changes.marker_at(line)
                {
                    draw_marker(area.x, y, marker, self.marker_colour(marker), cells);
                }

                if index == 0 {
                    draw_line_number(
                        area.x + margin,
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
                    x: area.x + margin + gutter,
                    y,
                    width,
                    row: wrap,
                };
                draw_row(
                    placement,
                    buffer,
                    line,
                    cells,
                    &Painting {
                        highlights: self.highlights,
                        theme: self.theme,
                        marked: self.marked,
                        selection: self.selection,
                        brackets,
                    },
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
/// One cell of margin or map, saying what happened to a line.
///
/// A bar for a line that is there and differs; a mark hugging the top edge
/// for lines that are *not* there. The second is the whole difficulty of
/// showing a deletion in a grid of cells: the removed lines have no row of
/// their own, so what is left is the boundary they were on, and the top
/// edge of the cell below it is that boundary. A full bar there would claim
/// the line changed, and it did not.
fn draw_marker(x: u16, y: u16, marker: Marker, colour: Color, cells: &mut CellBuffer) {
    let glyph = match marker {
        // A line that is there and differs: a bar down its whole height,
        // half a cell wide and against the *right* edge of its cell in both
        // columns. Left of the numbers it then sits beside the text it is
        // about; right of the scrollbar it sits at the edge of the screen.
        // Against the other edge each one floats a cell away from the thing
        // it belongs to.
        Marker::Added | Marker::Modified => '\u{2590}',
        // Lines that are not there: a mark on the boundary they were on,
        // which is the top edge of this cell.
        Marker::Removed => '\u{2594}',
    };
    put(cells, x, y, glyph, Style::new().fg(colour));
}

/// Everything about how a row looks, as against where it goes.
///
/// A struct because the list had grown to the point where the compiler was
/// the only thing keeping the order straight -- and because "where" and
/// "how" really are two groups.
struct Painting<'a> {
    highlights: &'a Highlights,
    theme: &'a Theme,
    /// The run a preview is about.
    marked: Option<Span>,
    /// The characters the reader selected in the file being read.
    selection: Option<Span>,
    /// The bracket under the cursor and its partner.
    brackets: Option<(ByteOffset, ByteOffset)>,
}

fn draw_row(
    placement: Placement,
    buffer: &Buffer,
    line: LineNumber,
    cells: &mut CellBuffer,
    painting: &Painting<'_>,
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
        let colour = painting
            .theme
            .colour_for(painting.highlights.kind_at(glyph.first_byte));

        // A foreground, so the background the fill painted stays — except
        // where the run being marked needs one of its own.
        let mut style = Style::new().fg(colour);
        if painting
            .marked
            .is_some_and(|marked| marked.contains(line, CharColumn::new(column)))
        {
            style = style.bg(painting.theme.marked_background);
        }
        if painting
            .selection
            .is_some_and(|selection| selection.contains(line, CharColumn::new(column)))
        {
            style = style.bg(painting.theme.selection_background);
        }
        // The bracket the cursor is on, and its partner. After the mark, so
        // a symbol a preview is about keeps its own background where the two
        // land on the same cell.
        if painting
            .brackets
            .is_some_and(|(open, close)| glyph.first_byte == open || glyph.first_byte == close)
        {
            style = style.bg(painting.theme.bracket_background);
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
