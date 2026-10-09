//! A reading, drawn.
//!
//! The rows come from [`obelus_reading`] already laid out and already
//! carrying what each run *is*; this puts the theme's colours on them and
//! writes the cells. One drawer for every reading, because by the time a
//! reading is rows it is the same thing as any other: runs of text with a
//! look, scrolled by rows, with no cursor in them -- the rows are not the
//! file's lines, and a line number beside a wrapped paragraph would be a
//! number for something that is not there.

use obelus_row::{Ink, Row};
use obelus_theme::Theme;
use ratatui::{
    buffer::Buffer as CellBuffer,
    layout::Rect,
    style::{Modifier, Style},
};

use crate::{fill, put, put_cluster, scrollbar};

/// Where a reading goes in a list's preview: one column in from the edge,
/// where the list's own rows start, so the two read as one page.
#[must_use]
pub const fn in_a_preview(area: Rect) -> Rect {
    let margin = if area.width > 0 { 1 } else { 0 };
    Rect {
        x: area.x + margin,
        width: area.width - margin,
        ..area
    }
}

/// How wide a reading in a list's preview is laid out.
///
/// Asked by whoever lays it out and answered from the same room it is drawn
/// in, so the rows are never laid out a column wider than they are drawn.
#[must_use]
pub const fn width_in_a_preview(area: Rect) -> u16 {
    in_a_preview(area)
        .width
        .saturating_sub(crate::editor::SCROLLBAR_WIDTH)
}

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

    let column = crate::editor::SCROLLBAR_WIDTH.min(area.width);
    let width = area.width - column;
    // One row of the reading per row of the screen, so this is exact.
    let bar = (rows.len() > usize::from(area.height))
        .then(|| scrollbar(cells, area, top, rows.len(), theme))
        .flatten();

    // And where this reading has got to, for a front end that can draw it
    // arriving rather than simply being there. Three readings are one
    // piece of code: a file shown as what it is rather than as its bytes,
    // a hover, and the documentation beside a completion -- so this is
    // said once for all three, and none of them had it before.
    if let Ok(top) = i64::try_from(top) {
        crate::shapes::scrolled(Rect { width, ..area }, top, bar);
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
            &Drawn {
                base: Style::new().fg(theme.foreground).bg(ground),
                theme,
                stop: area.x + width,
                // A reading on its own page is not held: what holds
                // anything is a conversation, and this draws a file's
                // preview and a server's answer as well.
                held: None,
            },
        );
    }
}

/// How a laid-out row's runs are to be drawn.
///
/// Together because they are one answer: what a run with no opinion of its
/// own looks like, what the colours are, where the room stops, and which of
/// it the reader has hold of.
#[derive(Clone, Copy)]
pub struct Drawn<'a> {
    /// The style a run the reading had no opinion about keeps, which
    /// carries the ground and the colour of whoever is speaking.
    pub base: Style,
    /// The colours the inks are drawn in.
    pub theme: &'a Theme,
    /// The column to stop at, so a row inside a list does not draw over
    /// whatever the list is on top of.
    pub stop: u16,
    /// Which of the row's characters are held, if any are.
    pub held: Option<&'a std::ops::Range<usize>>,
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
    spans: &[obelus_row::Span],
    drawn: &Drawn<'_>,
) -> u16 {
    let Drawn {
        base,
        theme,
        stop,
        held,
    } = *drawn;
    // The runs as one string, because a cluster is one cell and a run is a
    // colour, and a colour can change between a heart and its selector
    // without the two being two things. Each cluster is drawn in the
    // colour of the run its first character is in.
    let said: String = spans.iter().map(|span| span.text.as_ref()).collect();
    let mut runs = spans.iter().scan(0usize, |end, span| {
        *end += span.text.chars().count();
        Some((*end, span))
    });
    let mut run = runs.next();
    let mut column = x;
    for cluster in obelus_text::clusters(&said) {
        if column >= stop {
            return column;
        }
        while run.is_some_and(|(end, _)| end <= cluster.first) {
            run = runs.next();
        }
        let Some((_, span)) = run else { break };
        let style = style_of(span.ink, span, theme, base);
        // What the reader has hold of, in the colour every list in
        // Obelus marks a run of itself with: a ground under whatever
        // colour the characters already carry, which is why the ink
        // above is worked out first and only the ground is replaced.
        let style = match held.is_some_and(|held| held.contains(&cluster.first)) {
            true => style.bg(theme.selection_background),
            false => style,
        };
        column = column.saturating_add(put_cluster(cells, column, y, cluster.text, style));
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
fn style_of(ink: Ink, span: &obelus_row::Span, theme: &Theme, base: Style) -> Style {
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
        Ink::Added => style.fg(theme.change_added),
        Ink::Removed => style.fg(theme.change_removed),
    };
    if span.bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    if span.italic {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if span.strikeout {
        style = style.add_modifier(Modifier::CROSSED_OUT);
    }
    style
}

#[cfg(test)]
mod tests {
    use ratatui::{buffer::Buffer as CellBuffer, layout::Rect, style::Style};

    use super::{Drawn, write_spans};

    /// A picture and its selector go in one cell, two wide, even where the
    /// runs change between them -- which is how a conversation is drawn, and
    /// the one row writer that wrote a character at a time.
    ///
    /// Deliberate break: writing a character at a time, which writes the
    /// selector into a cell of its own and puts the `x` a cell further on.
    #[test]
    fn a_picture_and_its_selector_are_one_cell_of_two() {
        let theme = obelus_theme::builtin::by_name("dark").expect("the dark theme");
        let drawn = Drawn {
            base: Style::new(),
            theme,
            stop: 10,
            held: None,
        };
        let spans = [
            obelus_row::Span::new("\u{2764}", obelus_row::Ink::Plain),
            obelus_row::Span::new("\u{fe0f}x", obelus_row::Ink::Code),
        ];
        let mut cells = CellBuffer::empty(Rect::new(0, 0, 10, 1));
        let after = write_spans(&mut cells, 0, 0, &spans, &drawn);
        assert_eq!(after, 3, "the heart and the x are three cells");
        assert_eq!(cells[(0, 0)].symbol(), "\u{2764}\u{fe0f}");
        assert_eq!(cells[(1, 0)].symbol(), "");
        assert_eq!(cells[(2, 0)].symbol(), "x");
    }
}
