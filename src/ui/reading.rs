//! A reading, drawn.
//!
//! The rows come from [`crate::reading`] already laid out and already
//! carrying what each run *is*; this puts the theme's colours on them and
//! writes the cells. One drawer for every reading, because by the time a
//! reading is rows it is the same thing as any other: runs of text with a
//! look, scrolled by rows, with no cursor in them -- the rows are not the
//! file's lines, and a line number beside a wrapped paragraph would be a
//! number for something that is not there.

use ratatui::{
    buffer::Buffer as CellBuffer,
    layout::Rect,
    style::{Modifier, Style},
};

use crate::{
    reading::{Ink, Row},
    theme::Theme,
    ui::{fill, put, scrollbar},
};

/// Draws the reading, starting `top` rows in.
pub fn draw(cells: &mut CellBuffer, area: Rect, rows: &[Row], top: usize, theme: &Theme) {
    fill(
        cells,
        area,
        Style::new().fg(theme.foreground).bg(theme.background),
    );
    if area.width == 0 || area.height == 0 {
        return;
    }

    let bar = crate::ui::editor::SCROLLBAR_WIDTH.min(area.width);
    let width = area.width - bar;
    // One row of the reading per row of the screen, so this is exact.
    if rows.len() > usize::from(area.height) {
        scrollbar(cells, area, top, rows.len(), theme);
    }

    for (offset, row) in rows
        .iter()
        .skip(top)
        .take(usize::from(area.height))
        .enumerate()
    {
        let Ok(offset) = u16::try_from(offset) else {
            break;
        };
        let y = area.y + offset;

        if row.rule {
            for x in 0..width {
                put(
                    cells,
                    area.x + x,
                    y,
                    '\u{2500}',
                    Style::new().fg(theme.gutter),
                );
            }
            continue;
        }

        let mut column = 0u16;
        for span in &row.spans {
            let style = style_of(span.ink, span.bold, span.italic, theme);
            for character in span.text.chars() {
                if column >= width {
                    break;
                }
                column = column.saturating_add(put(cells, area.x + column, y, character, style));
            }
        }
    }
}

/// The look of one run.
///
/// The theme's own colours, so a reading belongs to whichever theme is on: a
/// heading takes the colour of a keyword, code the colour of a string, a
/// quote or a timestamp the colour of a comment. That mapping is the same
/// one the *highlighting* uses, so the reading and the bytes of one file are
/// recognizably the same file.
fn style_of(ink: Ink, bold: bool, italic: bool, theme: &Theme) -> Style {
    let mut style = Style::new().bg(theme.background);
    style = match ink {
        Ink::Plain => style.fg(theme.foreground),
        Ink::Heading(_) => style.fg(theme.syntax.keyword).add_modifier(Modifier::BOLD),
        Ink::Code => style.fg(theme.syntax.string),
        Ink::Aside => style.fg(theme.syntax.comment),
        Ink::Mark => style.fg(theme.gutter),
        Ink::Name => style.fg(theme.syntax.type_name),
        Ink::Key => style.fg(theme.syntax.property),
        Ink::Wrong => style.fg(theme.syntax.error),
        Ink::Doubtful => style.fg(theme.syntax.warning),
    };
    if bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    if italic {
        style = style.add_modifier(Modifier::ITALIC);
    }
    style
}
