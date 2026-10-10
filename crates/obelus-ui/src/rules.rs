//! The boundary between two things, and how much of something longer
//! than the screen is above it.

use super::*;

/// The block a bar is drawn with, track and thumb alike.
///
/// One glyph in two colours rather than a line and a block: a bar is a
/// surface with something sliding on it, and it is the *shade* that says
/// which part of it the reader is looking at.
pub(crate) const BAR: char = '\u{2588}';

/// One row of rule, saying that what is above it and what is below it are
/// different things.
///
/// Filled first: the row it goes on held code a moment ago, and a rule drawn
/// over the top of that would have the code showing between its cells.
///
/// It runs the whole width, joining nothing. A rule that closed itself off
/// against whatever was drawn beside it had to decide, per cell, whether
/// that neighbour was a control -- and the only thing it could ask was what
/// glyph the cell held, which a file's own text answers just as well as a
/// scrollbar does. A rule over one of this repository's golden grids grew a
/// tick everywhere the file had a bar under it. What the bar is drawn with
/// is what tells the two apart now: a block is a surface, and a surface does
/// not need a line to meet it.
pub(crate) fn rule(cells: &mut CellBuffer, area: Rect, theme: &Theme) {
    fill(cells, area, Style::new().bg(theme.background));
    for x in area.left()..area.right() {
        put(cells, x, area.y, '\u{2500}', Style::new().fg(theme.gutter));
    }
    shapes::ruled(Rect { height: 1, ..area });
}

/// The line between the two halves of one panel.
///
/// Not [`rule`], which is a boundary between two subjects on the page: this
/// one is the box's own, so it meets the border it crosses -- `├` and `┤`
/// rather than a line that overwrites the sides and leaves two boxes
/// touching. The completion panel's list and its documentation are divided
/// by it, and so are a signature's labels and what the call says about
/// itself; both are one thing with two parts.
pub(crate) fn divider(cells: &mut CellBuffer, area: Rect, y: u16, theme: &Theme) {
    let edge = Style::new().fg(theme.gutter).bg(theme.background);
    put(cells, area.x, y, '\u{251c}', edge);
    for x in area.x + 1..area.right().saturating_sub(1) {
        put(cells, x, y, '\u{2500}', edge);
    }
    put(cells, area.right().saturating_sub(1), y, '\u{2524}', edge);
    shapes::ruled(Rect {
        y,
        height: 1,
        ..area
    });
}

/// A bar down the right-hand edge of a region: where its window sits.
///
/// Shared by the editor and the lists, because it is the same question in
/// both -- how much of this is on screen, and which part -- and two
/// implementations would answer it in two shapes.
///
/// Drawn as a block in two shades: the track a shade off the page and the
/// thumb the brighter one. A line would be a line, and every rule that
/// crossed it would have to work out whether to join.
///
/// `total` is how many rows the whole thing has and `top` which of them is
/// on the first row.
///
/// Called only when there *is* somewhere to scroll -- a track with no thumb
/// on it is a control that does not work, and what is on screen being all
/// there is says itself. Whether there is somewhere is left to the caller
/// because only the caller can answer it: a list of rows fits when it has
/// fewer rows than the screen, while a file of wrapped lines can spill off
/// the bottom with a tenth of the screen's worth of lines in it.
///
/// The column stays reserved either way. Handing it back would change the
/// width of the text -- and with wrapping on, that means every line rewraps
/// when a file turns out to be one row too long.
/// It also says it is here, and hands back what it said. One piece of code
/// draws a bar, so one piece of code knows where one is: the two callers
/// that pass a bar on to [`shapes::scrolled`] were each working the mark
/// out a second time from the same three numbers, which is the one thing
/// `bar_reach` exists to stop happening.
pub(crate) fn scrollbar(
    cells: &mut CellBuffer,
    area: Rect,
    top: usize,
    total: usize,
    theme: &Theme,
) -> Option<shapes::Bar> {
    if area.width == 0 || area.height == 0 {
        return None;
    }
    let height = usize::from(area.height);
    let total = total.max(1);
    let x = area.right().saturating_sub(1);

    let (thumb, _, _) = bar_reach(height, total);
    let start = bar_mark(area.height, top, total);

    for row in 0..area.height {
        let inside = row >= start && usize::from(row) < usize::from(start) + thumb;
        let colour = if inside {
            theme.gutter_current
        } else {
            theme.scrollbar_track
        };
        put(cells, x, area.y + row, BAR, Style::new().fg(colour));
    }

    let bar = shapes::Bar {
        // The one column the cells went in, which is not `area`: a caller
        // hands this the whole list and the bar takes its last column.
        area: Rect {
            x,
            width: 1,
            ..area
        },
        mark: start,
        thumb: u16::try_from(thumb).unwrap_or(area.height),
    };
    shapes::barred(bar);
    bars::said(bar, total);
    Some(bar)
}

/// Which row is at the top when a bar's mark starts on this row of it.
///
/// [`bar_mark`] the other way round, for a pointer that has hold of the
/// mark: from the same `bar_reach`, so a mark dragged to a row is drawn on
/// that row, and to the foot of the track is the last screenful.
#[must_use]
pub fn bar_top(height: u16, mark: u16, total: usize) -> usize {
    let (_, travel, furthest) = bar_reach(usize::from(height), total.max(1));
    if travel == 0 {
        return 0;
    }
    usize::from(mark).min(travel) * furthest / travel
}

/// How big a bar's thumb is, how far it can travel, and how far the top it
/// is about can.
///
/// One answer, because two places need it now: the bar that draws the
/// thumb, and the front end that has to know how far the thumb moves for
/// each row the thing it is about moves. Two workings-out of the same
/// three numbers would be a mark that slides at one rate and lands at
/// another.
fn bar_reach(height: usize, total: usize) -> (usize, usize, usize) {
    // A bar with no rows has no thumb and nowhere to put one. The drawing
    // used to be the only caller and turned back at its own door; now that
    // two ask, the answer belongs here -- and a view is handed a region of
    // no height often enough that this is not a corner: a terminal one row
    // tall, a list with a question over it, a frame drawn before anything
    // has been laid out.
    if height == 0 {
        return (0, 0, 1);
    }
    // At least one row of thumb, or a long list has a bar with nothing on it.
    let thumb = (height * height / total.max(1)).clamp(1, height);
    let travel = height.saturating_sub(thumb);
    // Scaled by how far the *top* can travel, not by the total: dividing by
    // the total leaves the thumb short of the bottom exactly when the last
    // row is on screen, which is the one position anyone checks it against.
    let furthest = total.saturating_sub(height).max(1);
    (thumb, travel, furthest)
}

/// Which row of the track a bar's mark starts on.
///
/// The one place it is worked out, because two now want it: the bar that
/// draws the mark, and the front end that slides it. And the front end
/// needs *this* number rather than a rate, which is what it had first and
/// what made the mark step backwards. A rate is continuous and this is
/// not -- the mark is drawn on a row -- so sliding at the rate reached a
/// place a row's rounding away from where the mark was last drawn, in
/// whichever direction the rounding fell. Between two of these there is
/// nothing to disagree about: it sets out from where the mark was and
/// arrives where the mark is.
#[must_use]
pub fn bar_mark(height: u16, top: usize, total: usize) -> u16 {
    let rows = usize::from(height);
    if total <= rows {
        return 0;
    }
    let (_, travel, furthest) = bar_reach(rows, total);
    u16::try_from((top * travel).div_ceil(furthest).min(travel)).unwrap_or(0)
}

/// Which row of a bar `area.height` rows tall a line of `total` falls on.
///
/// Shared by the bar and the change map beside it so that a change is level
/// with the part of the bar it belongs to; two roundings would put them a
/// row apart on tall files, which is exactly where anyone would notice.
/// What a row that folds something away carries: turned right for a run
/// that is closed, turned down for one that is open.
///
/// One pair for the three places that fold: a run of lines in a file, a run
/// of tool calls in the transcript, a commit's files in a list. They are
/// the same act -- one row standing in for several, and a key that opens it
/// -- and a reader who learns the mark in one place has learned it.
pub(crate) const FOLDED: char = '\u{25b8}';
pub(crate) const UNFOLDED: char = '\u{25be}';

/// Whichever of the two says how a row stands.
#[must_use]
pub const fn opens(open: bool) -> char {
    match open {
        true => UNFOLDED,
        false => FOLDED,
    }
}

pub(crate) fn bar_row(line: usize, total: usize, height: u16) -> u16 {
    let height = usize::from(height);
    let row = line * height / total.max(1);
    u16::try_from(row.min(height.saturating_sub(1))).unwrap_or(0)
}
