//! A row of tabs, and the arrows that walk it.

use super::*;

/// A row of tabs, and the arrows that say how to change them.
///
/// The one that is showing gets the selected row's background, which is the
/// same thing that marks a selected row: on any of Obelus's screens, that
/// background means "this is the one you are on". Returns the column after
/// the last tab.
///
/// Only where the row is `in_front` -- see [`in_front`]. Under a list it
/// keeps the ink that says which tab it is, and not the ground that says
/// the keys are here.
///
/// The arrows are not a hint that can go stale: the keys are the arrows, and
/// there is nowhere to rebind them to.
pub fn tabs<Name>(
    cells: &mut CellBuffer,
    area: Rect,
    names: &[Name],
    current: usize,
    theme: &Theme,
    in_front: bool,
) -> u16
where
    Name: AsRef<str>,
{
    let dim = Style::new().fg(theme.gutter).bg(theme.background);
    fill(cells, Rect { height: 1, ..area }, dim);

    // The keys that walk them, drawn where they are walked. The tab
    // arrows rather than the left and right ones: those are the caret's
    // now, and a hint that names the wrong key is worse than none.
    //
    // Measured first, because the room the tabs have is what is left of the
    // row once this is on it.
    let keys = "\u{21e4} \u{21e5}";
    let placed = tabs_placed(area, names, current);
    if let Some(x) = placed.before {
        write(cells, x, area.y, TAB_MORE, dim);
    }
    let mut column = area.x + 1;
    for (index, x, _) in &placed.placed {
        let style = match (*index == current, in_front) {
            (true, true) => Style::new()
                .fg(theme.foreground)
                .bg(theme.selected_row_background),
            (true, false) => dim.fg(theme.foreground),
            (false, _) => dim,
        };
        column = write_marked(
            cells,
            area,
            *x,
            area.y,
            &format!(" {} ", names[*index].as_ref()),
            style,
            &Marked::plain(),
        );
    }
    if let Some(x) = placed.after {
        column = write(cells, x, area.y, TAB_MORE, dim);
    }
    if let Ok(offset) = u16::try_from(usize::from(area.width).saturating_sub(text_width(keys) + 1))
        && area.x + offset > column
    {
        write(cells, area.x + offset, area.y, keys, dim);
    }
    column
}

/// What says the row holds more tabs than it had room to draw.
///
/// The same mark the conversation's own row of settings uses for the same
/// fact, because it is the same fact: a row is a window on a list, and a
/// reader who cannot see a thing has to be told it is there.
const TAB_MORE: &str = "\u{2026}";

/// Where each tab that fits is drawn, and where the marks for the ones that
/// did not go.
///
/// Walked once. The drawing goes down this to put the words and a press
/// goes down it to find which word it landed on, so the two cannot disagree
/// about where a tab is.
pub struct Tabs {
    /// Which tab the row starts at.
    pub first: usize,
    /// `(which tab, the cell its word starts at, how wide the word is)`.
    pub placed: Vec<(usize, u16, u16)>,
    /// Where the mark for the tabs behind the row goes, if any are.
    pub before: Option<u16>,
    /// And for the ones ahead of it.
    pub after: Option<u16>,
}

/// How many cells the tabs have, which is the row less the keys that walk
/// them.
fn tabs_room(area: Rect) -> usize {
    let keys = text_width("\u{21e4} \u{21e5}") + 1;
    usize::from(area.width).saturating_sub(keys + 1)
}

/// Which tab the row starts at, so that the one the reader is on is drawn.
///
/// As near the beginning as that allows: a row is a window on a list like
/// any other, and the one thing a window must not do is hide what the keys
/// are moving.
fn tabs_first<Name>(area: Rect, names: &[Name], current: usize) -> usize
where
    Name: AsRef<str>,
{
    let room = tabs_room(area);
    let wide = |at: usize| text_width(&format!(" {} ", names[at].as_ref()));
    let mut first = 0;
    let mut taken = 0;
    for at in (0..=current.min(names.len().saturating_sub(1))).rev() {
        taken += wide(at);
        // Room for the mark that says there are more behind, once there
        // are: the mark is part of what the row has to fit.
        let marked = usize::from(at > 0) * text_width(TAB_MORE);
        if taken + marked > room {
            first = at + 1;
            break;
        }
    }
    first
}

/// Where the tabs sit along the row.
#[must_use]
pub fn tabs_placed<Name>(area: Rect, names: &[Name], current: usize) -> Tabs
where
    Name: AsRef<str>,
{
    let room = tabs_room(area);
    let first = tabs_first(area, names, current);
    let mut column = area.x + 1;
    let mut left = room;
    let before = (first > 0).then(|| {
        let at = column;
        column = column.saturating_add(u16::try_from(text_width(TAB_MORE)).unwrap_or(0));
        left = left.saturating_sub(text_width(TAB_MORE));
        at
    });
    let mut placed = Vec::new();
    let mut after = None;
    for (index, name) in names.iter().enumerate().skip(first) {
        let wide = text_width(&format!(" {} ", name.as_ref()));
        // The last of them keeps room for the mark saying there are more.
        let marked = usize::from(index + 1 < names.len()) * text_width(TAB_MORE);
        if wide + marked > left {
            after = Some(column);
            break;
        }
        left -= wide;
        placed.push((index, column, u16::try_from(wide).unwrap_or(0)));
        column = column.saturating_add(u16::try_from(wide).unwrap_or(0));
    }
    Tabs {
        first,
        placed,
        before,
        after,
    }
}

/// Which tab a point on the tab row asks for.
///
/// The way back out of the arithmetic [`tabs`] goes in by, down the same
/// one walk. Written beside it because every view that has tabs draws them
/// with that one function, so every view that lets a reader press one asks
/// this.
///
/// A press on a mark saying there are more that way asks for the tab just
/// off the row in that direction, which is what pressing it is for: the
/// row is a window, and the mark is the only thing on it saying the window
/// can move.
///
/// `None` for a point that is not on the row, or is past the tabs -- where
/// the keys that walk them are drawn.
#[must_use]
pub fn tab_at<Name>(area: Rect, names: &[Name], current: usize, x: u16, y: u16) -> Option<usize>
where
    Name: AsRef<str>,
{
    if y != area.y || x < area.x {
        return None;
    }
    let placed = tabs_placed(area, names, current);
    let mark = u16::try_from(text_width(TAB_MORE)).unwrap_or(0);
    if let Some(at) = placed.before
        && x >= at
        && x < at.saturating_add(mark)
    {
        return Some(placed.first.saturating_sub(1));
    }
    if let Some(at) = placed.after
        && x >= at
        && x < at.saturating_add(mark)
    {
        return placed.placed.last().map(|(index, _, _)| index + 1);
    }
    placed
        .placed
        .into_iter()
        .find(|(_, at, wide)| x >= *at && x < at.saturating_add(*wide))
        .map(|(index, _, _)| index)
}
