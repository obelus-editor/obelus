//! Markdown, drawn.
//!
//! The rows come from [`crate::markdown`] already wrapped and already
//! carrying what each run *is*; this puts the theme's colours on them and
//! writes the cells. A file in this mode has no cursor and no gutter: the
//! rows are not the file's lines, and a line number beside a wrapped
//! paragraph would be a number for something that is not there.

use ratatui::{
    buffer::Buffer as CellBuffer,
    layout::Rect,
    style::{Modifier, Style},
};

use crate::{
    markdown::{Kind, Row},
    theme::Theme,
    ui::{fill, put, scrollbar},
};

/// Draws the rendering, starting `top` rows in.
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
    scrollbar(cells, area, top, rows.len(), theme);

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
            let style = style_of(span.kind, span.bold, span.italic, theme);
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
/// The theme's own colours, so a rendered README belongs to whichever theme
/// is on: a heading takes the colour of a keyword, code the colour of a
/// string, a quote the colour of a comment. That mapping is the same one the
/// markdown *highlighting* uses, so the rendered view and the source view of
/// the same file are recognizably the same file.
fn style_of(kind: Kind, bold: bool, italic: bool, theme: &Theme) -> Style {
    let mut style = Style::new().bg(theme.background);
    style = match kind {
        Kind::Heading(_) => style.fg(theme.syntax.keyword).add_modifier(Modifier::BOLD),
        Kind::Code => style.fg(theme.syntax.string),
        Kind::Quote => style.fg(theme.syntax.comment),
        Kind::Decoration => style.fg(theme.gutter),
        Kind::Text => style.fg(theme.foreground),
    };
    if bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    if italic {
        style = style.add_modifier(Modifier::ITALIC);
    }
    style
}
