//! A reading, drawn.
//!
//! The rows come from [`obelus_reading`] already laid out and already
//! carrying what each run *is*; this puts the theme's colours on them and
//! writes the cells. One drawer for every reading, because by the time a
//! reading is rows it is the same thing as any other: runs of text with a
//! look, scrolled by rows, with no gutter beside them -- the rows are not the
//! file's lines, and a line number beside a wrapped paragraph would be a
//! number for something that is not there.
//!
//! A file shown as its reading has a cursor in it all the same, which is
//! [`obelus_component::page`]'s: what is held is drawn here, and where the
//! caret goes and which character a point on the screen is are
//! [`caret`] and [`place_at`], by the arithmetic [`crate::page`] keeps for
//! every page.

use obelus_component::{
    page::{Line, Place},
    reading::{Cursor, Laid},
};
use obelus_row::{Ink, Row};
use obelus_theme::Theme;
use ratatui::{
    buffer::Buffer as CellBuffer,
    layout::{Position, Rect},
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
    // A hover and a completion's documentation are not held: what holds
    // anything is a page with a cursor in it, which is [`draw_held`].
    draw_rows(
        cells,
        area,
        rows.iter().map(|row| (row, None, None)),
        rows.len(),
        top,
        theme,
        ground,
    );
}

/// Draws a file's reading, starting `top` rows in, with what the reader has
/// hold of marked.
///
/// `cursor` is where the cursor is, while nothing is in front of the
/// reading: where it is in a block of code, the key that copies the block
/// is said on the block's own box, the way the transcript and a pull
/// request's page say it.
pub fn draw_held(
    cells: &mut CellBuffer,
    area: Rect,
    rows: &[Laid<'_>],
    top: usize,
    cursor: Option<Place>,
    theme: &Theme,
) {
    draw_rows(
        cells,
        area,
        rows.iter()
            .map(|laid| (laid.row, laid.held(), laid.found())),
        rows.len(),
        top,
        theme,
        theme.background,
    );
    let Some(on) = cursor.and_then(|cursor| Laid::acting(rows, cursor.row)) else {
        return;
    };
    // Along the box's bottom, or its top where the bottom is off the
    // screen -- the transcript's rule, by the transcript's code.
    let height = usize::from(area.height);
    let shown = |row: usize| row >= top && row < top + height;
    let side = match (shown(on.end - 1), shown(on.start)) {
        (true, _) => on.end - 1,
        (false, true) => on.start,
        (false, false) => return,
    };
    let Some(laid) = rows.get(side) else {
        return;
    };
    let Ok(y) = u16::try_from(side - top) else {
        return;
    };
    let stop = area.x + area.width.saturating_sub(crate::editor::SCROLLBAR_WIDTH);
    let wide = u16::try_from(crate::text_width(&laid.text())).unwrap_or(u16::MAX);
    let ended = area.x.saturating_add(wide).min(stop);
    let dim = Style::new().fg(theme.gutter).bg(theme.background);
    crate::page::offer_copy(cells, &laid.row.spans, area.y + y, ended, dim, theme);
}

/// Where the caret goes on a file's reading drawn in `area` from row `top`:
/// where the cursor is, while its row is on screen.
#[must_use]
pub fn caret(area: Rect, rows: &[Row], cursor: &Cursor, top: usize) -> Option<Position> {
    let place = cursor.place();
    let row = rows.get(place.row)?;
    let below = place.row.checked_sub(top)?;
    let y = area.y + u16::try_from(below).ok()?;
    if y >= area.bottom() {
        return None;
    }
    let last = area.x + area.width.saturating_sub(crate::editor::SCROLLBAR_WIDTH);
    let x = crate::page::cell_of(&Laid::of(row).text(), area.x, place.character)
        .min(last.saturating_sub(1));
    Some(Position { x, y })
}

/// Which row and character of a file's reading drawn in `area` from row
/// `top` a point on the screen is: the band below the last row is the row
/// after it, which a selection can start from.
#[must_use]
pub fn place_at(area: Rect, rows: &[Row], top: usize, x: u16, y: u16) -> Option<Place> {
    if x < area.x || x >= area.right() || y < area.y || y >= area.bottom() {
        return None;
    }
    let row = (top + usize::from(y - area.y)).min(rows.len());
    Some(Place {
        row,
        character: rows.get(row).map_or(0, |it| {
            crate::page::character_under(&Laid::of(it).text(), area.x, x)
        }),
    })
}

/// One row to draw, with which of its characters are held and which a
/// search found.
type Marked<'a> = (
    &'a Row,
    Option<&'a std::ops::Range<usize>>,
    Option<&'a std::ops::Range<usize>>,
);

/// The rows, each with which of its characters are held and which a search
/// found.
fn draw_rows<'a>(
    cells: &mut CellBuffer,
    area: Rect,
    rows: impl Iterator<Item = Marked<'a>>,
    count: usize,
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
    let bar = (count > usize::from(area.height))
        .then(|| scrollbar(cells, area, top, count, theme))
        .flatten();

    // And where this reading has got to, for a front end that can draw it
    // arriving rather than simply being there. Three readings are one
    // piece of code: a file shown as what it is rather than as its bytes,
    // a hover, and the documentation beside a completion -- so this is
    // said once for all three, and none of them had it before.
    if let Ok(top) = i64::try_from(top) {
        crate::shapes::scrolled(Rect { width, ..area }, top, bar);
    }

    for (offset, (row, held, found)) in rows.skip(top).take(usize::from(area.height)).enumerate() {
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
                held,
                found,
                linked: &[],
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
    /// Which of them a search of the page found, on the row the search is
    /// standing on: marked in the colour the file marks what a search of it
    /// found, because the page is that search's preview.
    pub found: Option<&'a std::ops::Range<usize>>,
    /// Which of the row's characters are the words of a link, which are
    /// underlined whether or not the cursor is on them: a reader scanning a
    /// paragraph should not have to walk it to find out what in it goes
    /// somewhere.
    pub linked: &'a [std::ops::Range<usize>],
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
        found,
        linked,
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
        // And what a search found, under what is held: holding is the
        // reader's own act and wins where the two meet.
        let style = match found.is_some_and(|found| found.contains(&cluster.first)) {
            true => style.bg(theme.marked_background),
            false => style,
        };
        let style = match held.is_some_and(|held| held.contains(&cluster.first)) {
            true => style.bg(theme.selection_background),
            false => style,
        };
        let style = match linked.iter().any(|link| link.contains(&cluster.first)) {
            true => style.add_modifier(Modifier::UNDERLINED),
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
        Ink::Syntax(kind) => style.patch(theme.syntax.style(kind)),
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
    use obelus_text::kind::SyntaxKind;
    use ratatui::{
        buffer::Buffer as CellBuffer,
        layout::Rect,
        style::{Modifier, Style},
    };

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
            found: None,
            linked: &[],
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

    /// Code in a reading -- a block in a note, or in what an agent said --
    /// slants what the file slants.
    ///
    /// Deliberate break: taking only the kind's colour, as a reading did
    /// before a kind had a slant (the comment is upright).
    #[test]
    fn a_comment_in_a_reading_is_slanted() {
        let theme = obelus_theme::builtin::by_name("dark").expect("the dark theme");
        let drawn = Drawn {
            base: Style::new(),
            theme,
            stop: 10,
            held: None,
            found: None,
            linked: &[],
        };
        let spans = [
            obelus_row::Span::new("fn", obelus_row::Ink::Syntax(SyntaxKind::Keyword)),
            obelus_row::Span::new(" // x", obelus_row::Ink::Syntax(SyntaxKind::Comment)),
        ];
        let mut cells = CellBuffer::empty(Rect::new(0, 0, 10, 1));
        write_spans(&mut cells, 0, 0, &spans, &drawn);
        let slanted = |x: u16| cells[(x, 0)].modifier.contains(Modifier::ITALIC);
        assert!(slanted(3), "the comment is upright");
        assert!(!slanted(0), "the keyword is slanted");
    }
}
