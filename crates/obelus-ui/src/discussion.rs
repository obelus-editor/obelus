//! A pull request or an issue, drawn: as a page of its own, and in the
//! list's preview of it.
//!
//! One piece of code for both, because they are one page. The preview is
//! what choosing the row gives, with no cursor in it; the page is the same
//! rows with the cursor's caret and what it has hold of, and the key that
//! says what enter does on the thing the cursor is on.

use obelus_component::{
    discussion::{DiscussionView, Row},
    page::{Line, Place},
};
use obelus_theme::Theme;
use ratatui::{
    buffer::Buffer as CellBuffer,
    layout::{Position, Rect},
    style::Style,
    widgets::Widget,
};

use crate::{Screen, fill, put, reading, text_width, write};

/// Where the rows of a page in `area` are drawn: one column in from the
/// edge, the way a reading in a list's preview is, so that the page and its
/// preview are the same page.
#[must_use]
pub const fn band(area: Rect) -> Rect {
    reading::in_a_preview(area)
}

/// How wide a page in `area` is laid out: its band, less the scrollbar's
/// column.
///
/// Asked by everything that lays the rows out or walks them -- the view,
/// the caret, the keys, the pointer -- so none of them can be counting a
/// row that another wrapped differently.
#[must_use]
pub const fn reading_width(area: Rect) -> u16 {
    reading::width_in_a_preview(area)
}

/// Draws a page's rows in `area`, from row `top`.
///
/// `cursor` is where the cursor is, on the page and not in a preview: it is
/// what the key at the end of a row that acts is said for.
pub fn draw(
    cells: &mut CellBuffer,
    area: Rect,
    rows: &[Row],
    top: usize,
    cursor: Option<Place>,
    theme: &Theme,
    phase: u32,
) {
    let plain = Style::new().fg(theme.foreground).bg(theme.background);
    let dim = plain.fg(theme.gutter);
    fill(cells, area, plain);
    if area.width == 0 || area.height == 0 {
        return;
    }
    let column = crate::editor::SCROLLBAR_WIDTH.min(area.width);
    let width = area.width - column;
    let bar = (rows.len() > usize::from(area.height))
        .then(|| crate::scrollbar(cells, area, top, rows.len(), theme))
        .flatten();
    if let Ok(top) = i64::try_from(top) {
        crate::shapes::scrolled(Rect { width, ..area }, top, bar);
    }
    let stop = area.x + width;
    let acting = cursor.and_then(|cursor| Row::acting(rows, cursor.row));
    // The first and the last row of the block of code the cursor is in,
    // where each is on screen, and where its words end: the key that copies
    // it is said on the box's own side.
    let mut head = None;
    let mut foot = None;

    for (offset, row) in rows.iter().skip(top).enumerate() {
        let Ok(offset) = u16::try_from(offset) else {
            break;
        };
        if offset >= area.height {
            break;
        }
        let y = area.y + offset;
        let at = top + usize::from(offset);

        if row.row.rule {
            for x in 0..width {
                put(cells, area.x + x, y, '\u{2500}', dim);
            }
            continue;
        }
        let linked: Vec<std::ops::Range<usize>> = row
            .row
            .links
            .iter()
            .filter_map(|(bytes, _)| {
                obelus_component::page::row_held(&row.row.spans, bytes.start, bytes.end)
            })
            .collect();
        let mut ended = reading::write_spans(
            cells,
            area.x,
            y,
            &row.row.spans,
            &reading::Drawn {
                base: plain,
                theme,
                stop,
                held: row.held.as_ref(),
                linked: &linked,
            },
        );
        for ((_, to), characters) in row.row.links.iter().zip(&linked) {
            let text = row.text();
            let from = crate::page::cell_of(&text, area.x, characters.start);
            let to_cell = crate::page::cell_of(&text, area.x, characters.end).min(ended);
            if from < to_cell {
                crate::links::said(y, from..to_cell, to);
                crate::shapes::linked(Rect::new(from, y, to_cell - from, 1));
            }
        }
        // The mark at the head of the row saying something is on its way:
        // the row keeps two blanks there for it.
        if row.waiting {
            put(cells, area.x, y, crate::spinning(phase), dim);
            crate::shapes::spun(area.x, y);
        }
        // Whether what a heading folds is open, after its words, the way a
        // heading in the transcript says it.
        if row.folds.is_some() {
            ended = write(
                cells,
                ended,
                y,
                &format!(" {}", crate::opens(row.open)),
                dim,
            );
        }
        if let Some(on) = acting.as_ref().filter(|_| row.row.code.is_some()) {
            if at == on.start {
                head = Some((y, ended));
            }
            if at + 1 == on.end {
                foot = Some((y, ended));
            }
            continue;
        }
        // What enter does to the heading the cursor is on, after its words,
        // where they leave room.
        if let Some(on) = acting.as_ref().filter(|on| at + 1 == on.end) {
            let does = match rows.get(on.start) {
                Some(first) if first.goes.is_some() => "Opens its page",
                Some(first) if first.opens.is_some() => "Opens the file",
                Some(first) if first.open => "Closes it",
                Some(_) => "Opens it",
                None => continue,
            };
            let keys = [(
                crate::chat::chord(
                    crossterm::event::KeyCode::Enter,
                    crossterm::event::KeyModifiers::NONE,
                ),
                does,
            )];
            if let Some(said) = crate::chat::joined(&keys)
                && let Ok(wide) = u16::try_from(text_width(&said))
                && let Some(x) = stop.checked_sub(wide)
                && x > ended + 2
            {
                write(cells, x, y, &said, dim);
                crate::chat::cap_the_keys(x, y, &keys, theme.background, theme);
            }
        }
    }
    // Along the box's bottom, or its top where the bottom is off the
    // screen -- the transcript's rule, by the transcript's code.
    if let Some(on) =
        acting.filter(|on| rows.get(on.start).is_some_and(|row| row.row.code.is_some()))
        && let Some((y, ended)) = foot.or(head)
    {
        let side = match foot {
            Some(_) => on.end - 1,
            None => on.start,
        };
        crate::page::offer_copy(cells, &rows[side].row.spans, y, ended, dim, theme);
    }
}

/// Which row and character of a page a point on the screen is, for a page
/// drawn in `area` with `rows` on it: the band below the last row is the
/// row after it, which a selection can start from.
#[must_use]
pub fn place_at(area: Rect, page: &DiscussionView, rows: &[Row], x: u16, y: u16) -> Option<Place> {
    let band = band(area);
    if x < area.x || x >= area.right() || y < band.y || y >= band.bottom() {
        return None;
    }
    let row = (page.top() + usize::from(y - band.y)).min(rows.len());
    Some(Place {
        row,
        character: rows
            .get(row)
            .map_or(0, |it| crate::page::character_under(&it.text(), band.x, x)),
    })
}

/// Where the caret goes: where the cursor is, while its row is on screen.
#[must_use]
pub fn caret(area: Rect, page: &DiscussionView) -> Option<Position> {
    let band = band(area);
    let rows = page.rows(reading_width(area));
    let place = page.cursor();
    let row = rows.get(place.row)?;
    let below = place.row.checked_sub(page.top())?;
    let y = band.y + u16::try_from(below).ok()?;
    if y >= band.bottom() {
        return None;
    }
    let last = band.x + reading_width(area);
    let x = crate::page::cell_of(&row.text(), band.x, place.character).min(last.saturating_sub(1));
    Some(Position { x, y })
}

/// What the keys do here, for the first tab of the palette opened over it.
///
/// `elsewhere` is whether another Obelus has the conversation about it,
/// which is when the key that opens it does nothing here.
#[must_use]
pub fn hints(page: &DiscussionView, elsewhere: bool) -> Vec<crate::Hint> {
    use crossterm::event::{KeyCode, KeyModifiers};
    let chord = obelus_keymap::KeyChord::new;
    let what = match page.asked() {
        obelus_component::discussion::Asked::PullRequest(_) => {
            "Ask the agent to review this pull request"
        }
        obelus_component::discussion::Asked::Issue(_) => "Ask the agent to answer this issue",
    };
    vec![
        crate::Hint::new(chord(KeyCode::Char('a'), KeyModifiers::ALT), "Talk")
            .about(what)
            .when(!elsewhere),
    ]
}

/// The page, over the whole editor region.
pub struct DiscussionUi<'a> {
    page: &'a DiscussionView,
    theme: &'a Theme,
    phase: u32,
    /// Whether nothing is over it, which is whether it may say what enter
    /// does where the cursor is -- see [`crate::in_front`].
    in_front: bool,
}

impl<'a> DiscussionUi<'a> {
    /// Borrows what the view needs, or nothing if a page is not what the
    /// region is showing.
    #[must_use]
    pub fn new(app: &'a impl Screen) -> Option<Self> {
        Some(Self {
            page: app.shown().discussion()?,
            theme: app.theme(),
            phase: app.phase(),
            in_front: crate::in_front(app, None),
        })
    }
}

impl Widget for DiscussionUi<'_> {
    fn render(self, area: Rect, cells: &mut CellBuffer) {
        fill(
            cells,
            area,
            Style::new()
                .fg(self.theme.foreground)
                .bg(self.theme.background),
        );
        let rows = self.page.rows(reading_width(area));
        draw(
            cells,
            band(area),
            &rows,
            self.page.top(),
            self.in_front.then(|| self.page.cursor()),
            self.theme,
            self.phase,
        );
    }
}
