//! The line counts, drawn.
//!
//! A tab row, a rule, a column of names, and the numbers in fixed columns on
//! the right.
//!
//! There was a share bar beside each name, drawn against the biggest row.
//! The files page is a tree now, and a bar in a tree has no honest scale: the
//! biggest row is a top directory, against which every file is a single cell,
//! and a bar scoped to a row's siblings means a different thing at every
//! level. What it encoded was `Tally::lines`, so that is a column instead,
//! and the twenty-two cells it took go back to the names -- which is what a
//! tree of directories wants most.
//!
//! The columns are fixed so they line up down the screen, and they are given
//! up from the right as the terminal narrows -- the blanks first, then the
//! comments, then the code. What is never given up is the name, the file
//! count and the lines: a directory's lines are the only total it can
//! honestly carry, and the one the rows are ordered by. Its `code` reads
//! zero for a directory of Markdown that is plainly not empty.

use ratatui::{buffer::Buffer as CellBuffer, layout::Rect, style::Style, widgets::Widget};

use crate::{
    app::App,
    component::counts::{Counts, Row},
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
/// How much room the names are never squeezed below.
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
    /// Whether the column of file counts is drawn.
    ///
    /// Refused by a screen too narrow to hold it. Both pages have one now:
    /// the files page grew directories, and how many files are under one is
    /// a question about it.
    pub files: bool,
    /// Whether the code column is drawn.
    pub code: bool,
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
        // the comments, then the code. What is left is the name, the file
        // count and the lines -- the total every row can carry and the one
        // they are ordered by.
        let mut columns = Self {
            files,
            code: true,
            comments: true,
            blanks: true,
            numbers: 0,
            width: if files { FILES_WIDTH } else { 0 } + NUMBER_WIDTH * 4,
        };
        for giving_up in [
            &mut columns.blanks,
            &mut columns.comments,
            &mut columns.code,
        ] {
            if room >= LEAST_NAME + columns.width {
                break;
            }
            *giving_up = false;
            columns.width -= NUMBER_WIDTH;
        }
        if room < LEAST_NAME + columns.width && columns.files {
            columns.files = false;
            columns.width -= FILES_WIDTH;
        }
        columns.numbers = room.saturating_sub(columns.width);
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
}

impl Layout {
    /// The number columns that are drawn, each with its heading and width.
    ///
    /// One list, walked by the header and by every row, so a heading cannot
    /// end up over a different column from the numbers under it.
    fn headings(self) -> Vec<(&'static str, u16)> {
        let mut headings = Vec::with_capacity(5);
        if self.columns.files {
            headings.push(("files", FILES_WIDTH));
        }
        headings.push(("lines", NUMBER_WIDTH));
        if self.columns.code {
            headings.push(("code", NUMBER_WIDTH));
        }
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
        // Both pages have a file count now: a language is written across so
        // many files, and a directory holds so many.
        let layout = Layout {
            columns: Columns::fit(area.width, true),
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
        // A directory's mark goes in the glyph's column rather than beside
        // one. The branch in front of the row already says what it hangs
        // under and a folder glyph would say it a third time -- and this is
        // the half that survives a reader with no Nerd Font, which is the
        // half that has to: folding is discovered by seeing the mark.
        if let Some(open) = row.open {
            put(cells, x, y, crate::ui::opens(open), style);
            x += 2;
        } else if crate::icons::enabled() {
            if let Some(icon) = row.icon {
                put(cells, x, y, icon, style);
            }
            // Two columns, always. The terminal allocates one cell for a
            // private-use codepoint and the font draws two, so the second is
            // what the glyph bleeds into.
            x += 2;
        }

        // The name, clipped where the numbers begin.
        let edge = area.x + layout.columns.numbers.saturating_sub(GAP);
        write(
            cells,
            x,
            y,
            &clipped(&row.name, usize::from(edge.saturating_sub(x))),
            style,
        );

        self.numbers(
            cells,
            Rect {
                x: area.x + layout.columns.numbers,
                y,
                width: layout.columns.width,
                height: 1,
            },
            row.files,
            row.tally,
            layout,
            style,
        );
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
        // One entry per column that is *drawn*, so this list and the headings
        // are the same length by construction. A column the row has nothing
        // for is a blank in that column -- a file has no answer to how many
        // files are under it -- and not a column that closes up: closing one
        // up would slide every number left into the heading beside it.
        let mut written: Vec<Option<String>> = Vec::with_capacity(5);
        if layout.columns.files {
            written.push(files.map(|files| files.to_string()));
        }
        written.push(Some(tally.lines().to_string()));
        if layout.columns.code {
            written.push(Some(tally.code.to_string()));
        }
        if layout.columns.comments {
            written.push(Some(tally.comments.to_string()));
        }
        if layout.columns.blanks {
            written.push(Some(tally.blanks.to_string()));
        }

        let mut x = area.x;
        for ((_, width), contents) in layout.headings().into_iter().zip(written) {
            if let Some(contents) = contents {
                right(cells, x, area.y, width, &contents, style);
            }
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
    /// Broken deliberately by dropping the `room <` checks, so every column
    /// was always drawn: on a narrow screen the numbers then started four
    /// cells in, leaving a column of names with nothing in it, and the last
    /// assertion failed.
    #[test]
    fn a_narrow_screen_gives_up_the_least_interesting_columns_first() {
        // Wide: everything.
        let wide = Columns::fit(100, true);
        assert!(wide.files && wide.code && wide.comments && wide.blanks);

        // Narrower, and still every number: what a screen buys first is the
        // names, which a tree of directories wants more than a fifth column.
        let middling = Columns::fit(60, true);
        assert!(middling.comments && middling.code);

        // Narrow: the blanks, then the comments, then the code, then the
        // file counts.
        assert!(
            !Columns::fit(42, true).blanks,
            "the blanks survived a 42-column screen"
        );
        assert!(
            !Columns::fit(34, true).comments,
            "the comments survived a 34-column screen"
        );
        assert!(
            !Columns::fit(26, true).code,
            "the code column survived a 26-column screen"
        );
        assert!(
            !Columns::fit(20, true).files,
            "the file counts survived a 20-column screen"
        );

        // Whatever goes, the lines column stays -- it is the total every row
        // can carry and the one they are ordered by -- and the names keep
        // their room for as long as the screen has any to give them.
        for width in 20..120u16 {
            let columns = Columns::fit(width, true);
            let room = width - SCROLLBAR_WIDTH;
            assert!(
                columns.width >= NUMBER_WIDTH,
                "at {width} columns the lines column went"
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
