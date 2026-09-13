//! The line counts, drawn.
//!
//! A tab row, a rule, a column of names with a bar against the biggest of
//! them, the numbers in fixed columns on the right, and the whole tree's
//! total under a rule at the foot. The bar is drawn in ink rather than as a
//! row's background: a background here means one thing only, which is that
//! the keys are going to that row.
//!
//! The columns are fixed so they line up down the screen, and they are given
//! up from the right as the terminal narrows -- the blanks first, then the
//! comments, then the bar. What is never given up is the name and the code
//! column, which is the answer to the question the view was opened to ask.

use ratatui::{buffer::Buffer as CellBuffer, layout::Rect, style::Style, widgets::Widget};

use crate::{
    app::App,
    component::counts::{Counts, Go, Row},
    counts::Tally,
    theme::Theme,
    ui::{editor::SCROLLBAR_WIDTH, fill, put, rule, text_width, write},
};

/// How wide the column of file counts is.
const FILES_WIDTH: u16 = 6;
/// And each of the three columns of lines.
///
/// Eight: seven digits and the blank that keeps a column apart from the one
/// before it. A repository with more than a million lines in one language
/// has a number that runs into that blank, which is a row that reads
/// slightly tight rather than a number that is cut.
const NUMBER_WIDTH: u16 = 8;
/// The gap between the name and whatever is right of it.
const GAP: u16 = 2;
/// How wide the share bar is allowed to get.
///
/// Wide enough to read proportions off and no wider: past this it stops
/// being a bar beside a name and becomes the row.
const BAR_WIDTH: u16 = 22;
/// How much room the names need before the bar is worth drawing at all.
const LEAST_NAME: u16 = 16;
/// The rows that are not the list: the tabs, the rule under them, and the
/// row of column names.
///
/// The header counts, because it is not a row of the list: the keys that
/// move about one have to be told the same height the drawing uses, and the
/// two disagreeing by this one row is how the selection got a row below the
/// last one drawn before the view would scroll.
///
/// The tree's own total is not down here either. It is the `All` row at the
/// top of the languages page, which is the row that says what the whole
/// thing is *and* the way to every file in it -- and a foot repeating those
/// numbers would be the same fact drawn twice on one screen.
const FURNITURE: u16 = 3;

/// Where the rows go, inside a region this size.
///
/// One answer, asked by the drawing and by the keys that move about the
/// list: a page is worth what is on screen, and two answers to how much that
/// is would be a page that overshoots by however much they disagreed.
#[must_use]
pub fn list_region(area: Rect) -> Rect {
    Rect {
        y: area.y + FURNITURE,
        height: area.height.saturating_sub(FURNITURE),
        ..area
    }
}

/// How many rows of list a region has room for.
#[must_use]
pub fn list_height(area: Rect) -> u16 {
    list_region(area).height
}

/// Which columns fit, for a region this wide.
///
/// One answer, asked by the header, every row and the total, so the three
/// cannot disagree about where a column starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Columns {
    /// How wide the bar is, or zero for a screen with no room for one.
    pub bar: u16,
    /// Whether the column of file counts is drawn.
    ///
    /// Asked for by the page -- the file page has no use for it -- and
    /// refused by a screen too narrow to hold it.
    pub files: bool,
    /// Whether the comments column is drawn.
    pub comments: bool,
    /// And the blanks.
    pub blanks: bool,
    /// Where the numbers start, measured from the left of the region.
    pub numbers: u16,
    /// How wide the whole of the numbers are.
    pub width: u16,
}

impl Columns {
    /// Works out what fits in `width` cells, with `files` saying whether the
    /// page has a column of file counts in it.
    #[must_use]
    pub fn fit(width: u16, files: bool) -> Self {
        let room = width.saturating_sub(SCROLLBAR_WIDTH);
        // Given up from the right, least interesting first: the blanks, then
        // the comments, then how many files a language is in. The code
        // column is never given up -- it is the answer to the question the
        // view was opened to ask -- and neither is the name, which keeps its
        // room for as long as there is any room to keep.
        let mut columns = Self {
            bar: 0,
            files,
            comments: true,
            blanks: true,
            numbers: 0,
            width: if files { FILES_WIDTH } else { 0 } + NUMBER_WIDTH * 3,
        };
        if room < LEAST_NAME + columns.width {
            columns.blanks = false;
            columns.width -= NUMBER_WIDTH;
        }
        if room < LEAST_NAME + columns.width {
            columns.comments = false;
            columns.width -= NUMBER_WIDTH;
        }
        if room < LEAST_NAME + columns.width && columns.files {
            columns.files = false;
            columns.width -= FILES_WIDTH;
        }
        columns.numbers = room.saturating_sub(columns.width);
        // And the bar out of whatever is left once the names have had their
        // share, which on a narrow screen is nothing.
        columns.bar = columns.numbers.saturating_sub(LEAST_NAME).min(BAR_WIDTH);
        columns
    }
}

/// What every row of one frame is drawn against.
///
/// Worked out once when the frame starts: the columns depend on the width,
/// which page is showing and how big the biggest row is, and a row that
/// asked those questions for itself would be a row that could answer them
/// differently from the header above it.
#[derive(Clone, Copy, Debug)]
struct Layout {
    /// Which columns fit.
    columns: Columns,
    /// The biggest row on the page, for the bars to be drawn against.
    widest: usize,
}

impl Layout {
    /// The number columns that are drawn, each with its heading and width.
    ///
    /// One list, walked by the header and by every row, so a heading cannot
    /// end up over a different column from the numbers under it.
    fn headings(self) -> Vec<(&'static str, u16)> {
        let mut headings = Vec::with_capacity(4);
        if self.columns.files {
            headings.push(("files", FILES_WIDTH));
        }
        headings.push(("code", NUMBER_WIDTH));
        if self.columns.comments {
            headings.push(("comments", NUMBER_WIDTH));
        }
        if self.columns.blanks {
            headings.push(("blank", NUMBER_WIDTH));
        }
        headings
    }
}

/// The counts, over the whole editor region.
pub struct CountsView<'a> {
    counts: &'a Counts,
    theme: &'a Theme,
}

impl<'a> CountsView<'a> {
    /// Borrows what the view needs, or nothing if the counts are not open.
    #[must_use]
    pub fn new(app: &'a App) -> Option<Self> {
        Some(Self {
            counts: app.counts()?,
            theme: app.theme(),
        })
    }
}

impl Widget for CountsView<'_> {
    fn render(self, area: Rect, cells: &mut CellBuffer) {
        fill(
            cells,
            area,
            Style::new()
                .fg(self.theme.foreground)
                .bg(self.theme.background),
        );
        if area.height < FURNITURE || area.width < LEAST_NAME {
            return;
        }

        let tabs = self.counts.tabs();
        crate::ui::tabs(cells, area, &tabs, self.counts.tab(), self.theme);
        let under_tabs = Rect {
            y: area.y + 1,
            height: 1,
            ..area
        };
        rule(cells, under_tabs, self.theme);

        // The languages page counts files as well as lines; the file page's
        // rows are the files, and a column of ones would be a column saying
        // the same thing on every row.
        let files = self.counts.page() == crate::component::counts::Page::Languages;
        let layout = Layout {
            columns: Columns::fit(area.width, files),
            // Against the biggest *row*, and the tree's own row is not one
            // of them: it is the sum of the rest, so measuring against it
            // would make every bar on the page a share of a row that is
            // always full.
            widest: self
                .counts
                .rows()
                .iter()
                .filter(|row| !matches!(row.go, Some(Go::Everything)))
                .map(|row| row.tally.lines())
                .max()
                .unwrap_or(0),
        };
        // The header, on the row under the rule: the numbers in a column
        // are not self-describing the way a time or a level is, and three
        // columns of digits with nothing over them is a table the reader has
        // to guess at.
        self.header(
            cells,
            Rect {
                y: area.y + 2,
                height: 1,
                ..area
            },
            layout,
        );

        let rows = self.counts.rows();
        let body = list_region(area);
        if rows.is_empty() {
            crate::ui::nothing(
                cells,
                body,
                if self.counts.is_counting() {
                    "counting…"
                } else {
                    "nothing here to count"
                },
                self.theme,
            );
        } else {
            self.rows(cells, body, layout);
        }
    }
}

impl CountsView<'_> {
    /// The row of column names.
    fn header(&self, cells: &mut CellBuffer, area: Rect, layout: Layout) {
        let style = Style::new()
            .fg(self.theme.syntax.comment)
            .bg(self.theme.background);
        let mut x = area.x + layout.columns.numbers;
        for (heading, width) in layout.headings() {
            right(cells, x, area.y, width, heading, style);
            x += width;
        }
    }

    /// The rows themselves.
    fn rows(&self, cells: &mut CellBuffer, area: Rect, layout: Layout) {
        let rows = self.counts.rows();
        let window = self.counts.window();

        for (offset, index) in window.visible(area.height).enumerate() {
            if rows.get(index).is_none() {
                break;
            }
            let Ok(offset) = u16::try_from(offset) else {
                break;
            };
            self.row(
                cells,
                Rect {
                    y: area.y + offset,
                    height: 1,
                    ..area
                },
                index,
                layout,
                index == window.focus(),
            );
        }

        // Only where there is somewhere to scroll: a track with no thumb on
        // it is a control that does not work. The column stays reserved
        // either way, so nothing moves sideways when a list grows past the
        // screen.
        if window.scrollable(area.height) {
            crate::ui::scrollbar(cells, area, window.top(), rows.len(), self.theme);
        }
    }

    /// One row, in its own one-row region.
    fn row(&self, cells: &mut CellBuffer, area: Rect, at: usize, layout: Layout, selected: bool) {
        let Some(row) = self.counts.rows().get(at) else {
            return;
        };
        // One mark for "the keys are here", and it says nothing else -- so
        // the bar beside the name is ink rather than a second background.
        let background = if selected {
            self.theme.selected_row_background
        } else {
            self.theme.background
        };
        fill(
            cells,
            Rect {
                width: area.width.saturating_sub(SCROLLBAR_WIDTH),
                ..area
            },
            Style::new().bg(background),
        );

        // Whether a row can be used is said in the ink, never by taking the
        // background away: a language written inside another is a fact about
        // the row above it rather than somewhere to go.
        let ink = if row.go.is_some() {
            self.theme.foreground
        } else {
            self.theme.syntax.comment
        };
        let style = Style::new().fg(ink).bg(background);
        let y = area.y;

        let mut x = area.x + 1;
        for (column, glyph) in branches(self.counts.rows(), at).into_iter().enumerate() {
            let Ok(column) = u16::try_from(column) else {
                break;
            };
            x = area.x + 1 + column * 2;
            x += put(
                cells,
                x,
                y,
                glyph,
                Style::new().fg(self.theme.gutter).bg(background),
            );
            x += 1;
        }

        // The glyph column, kept whether or not this row has a glyph: a row
        // indented under another has to read as further in than it, and a
        // row that skipped this column would put its name back level with
        // the names above it -- which is what the language written inside
        // another did, flush with the languages it is not one of.
        if crate::icons::enabled() {
            if let Some(icon) = row.icon {
                put(cells, x, y, icon, style);
            }
            // Two columns, always. The terminal allocates one cell for a
            // private-use codepoint and the font draws two, so the second is
            // what the glyph bleeds into.
            x += 2;
        }

        // The name, clipped where the bar or the numbers begin.
        let edge = area.x
            + layout
                .columns
                .numbers
                .saturating_sub(layout.columns.bar + GAP);
        write(
            cells,
            x,
            y,
            &clipped(&row.name, usize::from(edge.saturating_sub(x))),
            style,
        );

        // A bar is a share of the page, so two rows do not get one: the
        // child, whose lines are already inside its parent's on the row
        // above, and the tree itself, whose share is all of it.
        let shares = matches!(row.go, Some(Go::Language(_) | Go::File(_)));
        if layout.columns.bar > 0 && shares {
            self.bar(
                cells,
                Rect {
                    x: area.x + layout.columns.numbers - layout.columns.bar - 1,
                    width: layout.columns.bar,
                    y,
                    height: 1,
                },
                row.tally.lines(),
                layout.widest,
                background,
            );
        }

        self.numbers(
            cells,
            Rect {
                x: area.x + layout.columns.numbers,
                y,
                width: layout.columns.width,
                height: 1,
            },
            layout.columns.files.then_some(row.files).flatten(),
            row.tally,
            layout,
            style,
        );
    }

    /// The share bar: how big this row is against the biggest one.
    ///
    /// In the gutter's own colour, which is what obelus draws every other
    /// measure of "how much of this" in.
    fn bar(
        &self,
        cells: &mut CellBuffer,
        area: Rect,
        lines: usize,
        widest: usize,
        background: ratatui::style::Color,
    ) {
        if widest == 0 || lines == 0 {
            return;
        }
        // At least one cell for a row that has any lines at all: a row drawn
        // with no bar reads as a row with nothing in it.
        let filled = (usize::from(area.width) * lines).div_ceil(widest.max(1));
        let filled = u16::try_from(filled)
            .unwrap_or(area.width)
            .clamp(1, area.width);
        for column in 0..filled {
            put(
                cells,
                area.x + column,
                area.y,
                '\u{2588}',
                Style::new().fg(self.theme.gutter).bg(background),
            );
        }
    }

    /// The columns of numbers, from the left edge of the number block.
    fn numbers(
        &self,
        cells: &mut CellBuffer,
        area: Rect,
        files: Option<usize>,
        tally: Tally,
        layout: Layout,
        style: Style,
    ) {
        let written = [
            files.map(|files| files.to_string()),
            Some(tally.code.to_string()),
            layout.columns.comments.then(|| tally.comments.to_string()),
            layout.columns.blanks.then(|| tally.blanks.to_string()),
        ];
        let mut x = area.x;
        for ((_, width), contents) in layout.headings().into_iter().zip(written.iter().flatten()) {
            right(cells, x, area.y, width, contents, style);
            x += width;
        }
    }
}

/// The tree lines in front of the row at `at`, one per level above it.
///
/// A row indented under another needs to say *which* other, and one glyph
/// cannot: a corner on every row of a run reads as "the last one" repeated,
/// which is what it said before this. So the run is drawn the way every tree
/// is -- a tee while more of the same level is still to come, a corner on
/// the last of them -- and a level that has more below it carries its own
/// line down past whatever is indented under the row.
fn branches(rows: &[Row], at: usize) -> Vec<char> {
    let Some(row) = rows.get(at) else {
        return Vec::new();
    };
    (1..=row.depth)
        .map(|level| match level == row.depth {
            // The row's own connector.
            true => match last_of_its_level(rows, at, level) {
                true => '\u{2570}',
                false => '\u{251c}',
            },
            // A level above it: its line goes on down the screen only while
            // that level has something left to come.
            false => match last_of_its_level(rows, parent(rows, at, level), level) {
                true => ' ',
                false => '\u{2502}',
            },
        })
        .collect()
}

/// Whether the row at `at` is the last one at `level` under whatever holds
/// it.
///
/// Reading forward: anything shallower ends the run, anything at the same
/// level means the run goes on, and anything deeper belongs to this row.
fn last_of_its_level(rows: &[Row], at: usize, level: u16) -> bool {
    rows.iter()
        .skip(at + 1)
        .find(|row| row.depth <= level)
        .is_none_or(|row| row.depth < level)
}

/// The row at `level` that holds the row at `at`.
fn parent(rows: &[Row], at: usize, level: u16) -> usize {
    rows.iter()
        .take(at)
        .rposition(|row| row.depth == level)
        .unwrap_or(at)
}

/// Writes `contents` right-aligned in a column `width` wide.
fn right(cells: &mut CellBuffer, x: u16, y: u16, width: u16, contents: &str, style: Style) {
    let used = u16::try_from(text_width(contents)).unwrap_or(width);
    // A number wider than its column is written from the column's own left
    // edge rather than over the one before it: a count that long is a count
    // worth seeing, and the column beside it has its own row to be read on.
    let indent = width.saturating_sub(used).saturating_sub(1);
    write(cells, x + indent, y, contents, style);
}

/// `contents`, cut to `room` cells.
///
/// From the left, because a path's tail is the part that says which file it
/// is: `src/component/picker/mod.rs` cut at the front is still a file a
/// reader recognises, and cut at the back is three directories.
fn clipped(contents: &str, room: usize) -> String {
    if text_width(contents) <= room {
        return contents.to_string();
    }
    crate::ui::truncate_from_left(contents, room)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The columns are given up from the right as the screen narrows.
    ///
    /// Broken deliberately by dropping the three `room <` checks, so every
    /// column was always drawn: on a narrow screen the numbers then started
    /// four cells in, leaving a column of names with nothing in it, and the
    /// last assertion failed.
    #[test]
    fn a_narrow_screen_gives_up_the_least_interesting_columns_first() {
        // Wide: everything.
        let wide = Columns::fit(100, true);
        assert!(wide.files && wide.comments && wide.blanks);
        assert!(wide.bar > 0, "a wide screen drew no bar");

        // Narrower: the bar is the first thing to go, and the numbers keep
        // their columns.
        let middling = Columns::fit(50, true);
        assert!(middling.comments, "the comments went before the bar did");
        assert!(middling.bar <= wide.bar);

        // Narrow: the blanks, then the comments, then the file counts.
        assert!(
            !Columns::fit(34, true).blanks,
            "the blanks survived a 34-column screen"
        );
        assert!(
            !Columns::fit(26, true).comments,
            "the comments survived a 26-column screen"
        );
        assert!(
            !Columns::fit(20, true).files,
            "the file counts survived a 20-column screen"
        );

        // Whatever goes, the code column stays and the names keep their room
        // for as long as the screen has any to give them.
        for width in 20..120u16 {
            let columns = Columns::fit(width, true);
            let room = width - SCROLLBAR_WIDTH;
            assert!(
                columns.width >= NUMBER_WIDTH,
                "at {width} columns the code column went"
            );
            assert!(
                columns.numbers >= LEAST_NAME.min(room.saturating_sub(NUMBER_WIDTH)),
                "at {width} columns the names were left {} cells",
                columns.numbers
            );
        }
    }

    /// The keys and the drawing are told the same height.
    ///
    /// The header is not a row of the list. Broken deliberately by taking it
    /// out of `FURNITURE`: the list came back a row taller and starting a
    /// row higher, which is this test's own assertions about `y` and
    /// `height`. What that mismatch *did* on screen -- the selection
    /// standing a row below the last one drawn, the view scrolling only
    /// once it had -- is held down by the test of the same name in
    /// `tests/counts.rs`.
    #[test]
    fn the_list_is_what_is_left_under_the_tabs_and_the_headings() {
        let area = Rect::new(0, 0, 80, 24);
        let list = list_region(area);
        assert_eq!(list_height(area), 21);
        assert_eq!(list.height, 21);
        // Under the tabs, the rule and the headings, and ending where the
        // region does.
        assert_eq!(list.y, area.y + 3);
        assert_eq!(list.bottom(), area.bottom());
        // A region with no room for a list at all does not go round.
        assert_eq!(list_height(Rect::new(0, 0, 80, 2)), 0);
    }
}
