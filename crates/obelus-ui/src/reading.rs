//! A reading, drawn.
//!
//! The rows come from [`obelus_reading`] already laid out and already
//! carrying what each run *is*; this puts the theme's colours on them and
//! writes the cells. One drawer for every reading, because by the time a
//! reading is rows it is the same thing as any other: runs of text with a
//! look, scrolled by rows, with no cursor in them -- the rows are not the
//! file's lines, and a line number beside a wrapped paragraph would be a
//! number for something that is not there.

use obelus_reading::{Ink, Row};
use obelus_theme::Theme;
use ratatui::{
    buffer::Buffer as CellBuffer,
    layout::Rect,
    style::{Modifier, Style},
};

use crate::{fill, put, scrollbar};

/// Draws the reading, starting `top` rows in.
///
/// `ground` because the same reading is drawn in two places: a markdown
/// file previewed in the editor sits on the page, and a hover or a
/// completion's documentation sits on a panel's raised one. Filling with
/// the page's own colour painted a panel's ground back out from under it.
pub fn draw(
    cells: &mut CellBuffer,
    area: Rect,
    rows: &[Row],
    top: usize,
    theme: &Theme,
    ground: ratatui::style::Color,
) {
    fill(cells, area, Style::new().fg(theme.foreground).bg(ground));
    if area.width == 0 || area.height == 0 {
        return;
    }

    let bar = crate::editor::SCROLLBAR_WIDTH.min(area.width);
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

        write_spans(
            cells,
            area.x,
            y,
            &row.spans,
            Style::new().fg(theme.foreground).bg(ground),
            theme,
            area.x + width,
        );
    }
}

/// Writes a laid-out row's runs, and says the column they ended in.
///
/// The one place an ink becomes a colour on a screen. Two callers: a
/// reading drawn on a page of its own, and a conversation -- where what an
/// agent said is markdown by the protocol's own word for it, and the row it
/// goes on already has a speaker's mark in front of it, an indent, and
/// sometimes a background saying the reader is standing on it.
///
/// So the look is put *over* a base rather than built from nothing. The
/// base carries the row's ground and the colour of whatever is speaking,
/// and a run the markdown had no opinion about -- [`Ink::Plain`], which is
/// most of any sentence -- is left in it. A reading of its own passes the
/// page's foreground and gets back what it always had.
pub fn write_spans(
    cells: &mut CellBuffer,
    x: u16,
    y: u16,
    spans: &[obelus_reading::Span],
    base: Style,
    theme: &Theme,
    stop: u16,
) -> u16 {
    let mut column = x;
    for span in spans {
        let style = style_of(span.ink, span.bold, span.italic, theme, base);
        for character in span.text.chars() {
            if column >= stop {
                return column;
            }
            column = column.saturating_add(put(cells, column, y, character, style));
        }
    }
    column
}

/// The look of one run.
///
/// The theme's own colours, so a reading belongs to whichever theme is on: a
/// heading takes the colour of a keyword, code the colour of a string, a
/// quote or a timestamp the colour of a comment. That mapping is the same
/// one the *highlighting* uses, so the reading and the bytes of one file are
/// recognizably the same file.
fn style_of(ink: Ink, bold: bool, italic: bool, theme: &Theme, base: Style) -> Style {
    let style = base;
    let mut style = match ink {
        // Left as it came: a run markdown had no opinion about is most of
        // any sentence, and what colour that is belongs to whoever is
        // drawing the row.
        Ink::Plain => style,
        Ink::Heading(_) => style.fg(theme.syntax.keyword).add_modifier(Modifier::BOLD),
        Ink::Code => style.fg(theme.syntax.string),
        Ink::Syntax(kind) => style.fg(theme.syntax.colour(kind)),
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
