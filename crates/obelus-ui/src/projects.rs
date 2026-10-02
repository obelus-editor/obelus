//! What Obelus shows while it is asking which project.
//!
//! **A screen of its own, and not the welcome screen with its keys taken
//! away.** The welcome screen is the way in to a project: a wordmark, and
//! the keys that open a file, search, ask the agent. None of those means
//! anything until there is a project, so this was drawn in its place under
//! the same plate -- which said "welcome" over a question that has to be
//! answered before anybody is welcome anywhere. Here there is no plate and
//! no sheen: the projects, the row that opens another, and what went wrong
//! on the way up. The welcome screen comes after, once there is somewhere
//! to be welcomed into.
//!
//! **What went wrong on the way up is said here too**, under the way in and
//! never in front of it, for the reason the welcome screen says it: this is
//! the first thing a reader started from a launcher sees, and the list of
//! projects not reading is exactly the sort of thing they should be told.
//! Read here and not walked: the arrows and enter belong to the projects,
//! and most of what is in that block is a line of a file -- going to it
//! would open a file with no project to open it in. So no row of it carries
//! the mark that says the keys are there, and the welcome screen, which
//! comes next, is where it is walked.

use obelus_theme::Theme;
use ratatui::{buffer::Buffer as CellBuffer, layout::Rect, style::Style, widgets::Widget};

use crate::{Screen, fill, welcome::WentWrongBlock, write};

/// What the row that opens a project not in the list says.
///
/// A verb, because the row is an action among rows that are places. The
/// ellipsis is what a desktop means by "this opens somewhere to say more",
/// and here what it opens is the box at the foot.
///
/// Two of them, because "another" is a word about a list: on a machine
/// with nothing remembered the row is the whole of the screen, and there
/// is nothing for it to be another of.
const OPEN_A_PROJECT: &str = "Open a project…";
const OPEN_ANOTHER: &str = "Open another…";

/// What is over the projects.
///
/// Said because the list is not the only thing on the screen: the row
/// that opens one is below it, and without a word over the rows the two
/// read as one list in which the last entry happens to be a verb.
const RECENT: &str = "Recent projects";

/// What the list says when the filter has left none of them.
const NO_MATCH: &str = "No project matches";

/// How wide the list of projects is allowed to be.
///
/// A path is most of a row, and the room it is given is what keeps its
/// head on; capped, because a row run across a wide terminal puts the
/// time at the far end of it from the name it is about.
const RECENT_WIDTH: u16 = 64;

/// The least space between a path and the time beside it.
///
/// Two columns, so that the end of one and the start of the other are
/// never read as one word on a row where both reach their room. And the
/// same between a project's name and where it is.
const WHEN_GAP: u16 = 2;

/// The question, and what went wrong under it.
pub struct ProjectsView<'a> {
    choosing: crate::Choosing,
    went_wrong: WentWrongBlock<'a>,
    theme: &'a Theme,
}

impl<'a> ProjectsView<'a> {
    /// Borrows what the view needs, where Obelus is asking.
    #[must_use]
    pub fn new(app: &'a impl Screen) -> Option<Self> {
        Some(Self {
            choosing: app.choosing()?,
            went_wrong: WentWrongBlock::read(app),
            theme: app.theme(),
        })
    }
}

impl Widget for ProjectsView<'_> {
    /// The projects under a heading, newest first, and below them the row
    /// that opens one that is not in the list -- which is always there, so
    /// a reader on a machine Obelus has never run on has exactly one row,
    /// and it is the one that gets them in. Then it is the whole of the
    /// block, with no heading over a list that is not there.
    fn render(self, area: Rect, cells: &mut CellBuffer) {
        let choosing = &self.choosing;
        let headed = headed(choosing);
        // The heading and a blank over the projects, and a blank and the
        // opening row under them; or that row alone.
        let around = match headed {
            true => 4,
            false => 1,
        };
        // Every project, or the one line saying the filter left none.
        let listed = match headed {
            true => u16::try_from(choosing.known.len().max(1)).unwrap_or(u16::MAX),
            false => 0,
        };
        // What went wrong, with a blank over it, where there is room for
        // it beside the opening row and one project: it goes before the
        // way in does. The projects get the rest, because the filter is
        // what reaches the ones below the fold.
        let amiss = match self.went_wrong.height() {
            0 => 0,
            rows => rows + 1,
        };
        let amiss = match around + listed.min(1) + amiss <= area.height {
            true => amiss,
            false => 0,
        };
        let room = listed.min(area.height.saturating_sub(around + amiss));
        let height = (around + room + amiss).min(area.height);
        let width = RECENT_WIDTH.min(area.width);
        let left = area.x + (area.width - width) / 2;
        let top = area.y + (area.height - height) / 2;
        let list = Rect {
            x: left,
            y: top,
            width,
            height: height - amiss,
        };
        self.rows(cells, choosing, list, room);
        // A cell in from either side of the list, so that its heading
        // starts where `Recent projects` does and its tails end where the
        // times do: the block sets its words a cell into its room, and the
        // list two, for the mark behind the row the reader is on.
        if amiss > 0 {
            let under = Rect {
                x: left + 1,
                width: width.saturating_sub(2),
                ..area
            };
            self.went_wrong.draw(cells, under, list.bottom() + 1);
        }
    }
}

impl ProjectsView<'_> {
    /// The heading, the projects and the opening row.
    fn rows(
        &self,
        cells: &mut CellBuffer,
        choosing: &crate::Choosing,
        // Where the rows go, as one thing: four numbers passed beside
        // each other are four numbers that can be passed in the wrong
        // order.
        list: Rect,
        // How many of the projects there is room for.
        room: u16,
    ) {
        let (left, width) = (list.x, list.width);
        let total = choosing.known.len();
        let selected = Style::new().bg(self.theme.selected_row_background);
        // One mark for "the keys are here", and it says nothing else: the
        // same colour behind the row the reader is on that every list,
        // page and card in Obelus puts there.
        let row_style = |cells: &mut CellBuffer, y: u16, on: bool| match on {
            true => {
                fill(
                    cells,
                    Rect {
                        x: left,
                        y,
                        width,
                        height: 1,
                    },
                    selected,
                );
                selected
            }
            false => Style::new(),
        };
        let mut y = list.y;
        let headed = headed(choosing);
        if headed {
            write(
                cells,
                left + 2,
                y,
                RECENT,
                Style::new().fg(self.theme.syntax.function),
            );
            y += 2;
            if total == 0 {
                crate::nothing(
                    cells,
                    Rect {
                        x: left + 1,
                        y,
                        width,
                        height: 1,
                    },
                    NO_MATCH,
                    self.theme,
                );
                y += 1;
            } else {
                // Which of them are on screen: the reader's own row among
                // them, and the ones nearest the opening row while the
                // reader is on that.
                let focus = choosing.at.min(total - 1);
                let room = usize::from(room);
                let first = match room < total {
                    false => 0,
                    true => focus
                        .saturating_sub(room.saturating_sub(1))
                        .min(total - room),
                };
                let names = self.name_width(choosing, width);
                for (at, opened) in choosing.known.iter().enumerate().skip(first).take(room) {
                    if y >= list.bottom() {
                        return;
                    }
                    let style = row_style(cells, y, at == choosing.at);
                    self.row(cells, left, y, width, names, opened, style);
                    y += 1;
                }
            }
            y += 1;
        }
        if y >= list.bottom() {
            return;
        }
        let style = row_style(cells, y, choosing.at >= total);
        let said = match headed {
            true => OPEN_ANOTHER,
            false => OPEN_A_PROJECT,
        };
        write(cells, left + 2, y, said, style.fg(self.theme.foreground));
    }

    /// How wide the column of names is: the widest of them, and never
    /// more than half the row, so that where each one is keeps room to
    /// say it.
    ///
    /// Taken across every project and not only those on screen, so the
    /// column does not move under the reader as the list scrolls.
    fn name_width(&self, choosing: &crate::Choosing, width: u16) -> u16 {
        let widest = choosing
            .known
            .iter()
            .map(|opened| obelus_text::text_width(name_of(&opened.path).1))
            .max()
            .unwrap_or(0);
        u16::try_from(widest)
            .unwrap_or(u16::MAX)
            .min(width.saturating_sub(4) / 2)
    }

    /// One project's row: its name, where it is, and when it was open.
    ///
    /// **The name first, and in the ink.** What tells two projects apart is
    /// the last part of the path, and a column of whole paths put that at
    /// the ragged end of each row, behind a head every row shared. So the
    /// name is a column of its own and where it is follows, dim.
    ///
    /// **The time is given its room before the path gets any.** A row
    /// works out what it says about itself first and the words get what
    /// is left -- so a path long enough to reach the right-hand edge
    /// cannot take the tail with it. And what is cut off a path is its
    /// *head*, for the reason the name comes first: the end is the part
    /// that says which.
    #[allow(clippy::too_many_arguments)]
    fn row(
        &self,
        cells: &mut CellBuffer,
        left: u16,
        y: u16,
        width: u16,
        names: u16,
        opened: &crate::Opened,
        style: Style,
    ) {
        let right = left + width.saturating_sub(2);
        let when = u16::try_from(obelus_text::text_width(&opened.when)).unwrap_or(0);
        let marked = |skip| crate::Marked {
            matched: match opened.matched {
                Some((first, end)) => crate::Matched::Run(first, end),
                None => crate::Matched::Nothing,
            },
            mark: self.theme.picker_match_background,
            syntax: None,
            skip,
        };
        let (start, name) = name_of(&opened.path);

        // The name, counted from the start of the whole path so that what
        // the filter matched lands on the right letters -- the one writer
        // for it, so this list cannot be the newest one that forgot.
        let x = left + 2;
        let ink = style.fg(self.theme.foreground);
        let long = obelus_text::text_width(name) > usize::from(names);
        let room = match long {
            // A cell for the mark that says it was cut.
            true => names.saturating_sub(1),
            false => names,
        };
        let row = |x: u16, width: u16| Rect {
            x,
            y,
            width,
            height: 1,
        };
        let end = crate::write_marked(cells, row(x, room), x, y, &opened.path, ink, &marked(start));
        if long {
            write(cells, end, y, "\u{2026}", ink);
        }

        // Where it is, in what is left between the name and the time.
        let dim = style.fg(self.theme.gutter);
        let from = x + names + WHEN_GAP;
        let stop = match when {
            0 => right,
            _ => right.saturating_sub(when + WHEN_GAP),
        };
        let place: String = opened.path.chars().take(place_length(start)).collect();
        let room = stop.saturating_sub(from);
        // A lone mark saying something was cut says nothing about where.
        if room > 1 {
            let dropped = crate::drop_from_left(&place, usize::from(room));
            let at = match dropped {
                0 => from,
                _ => write(cells, from, y, "\u{2026}", dim),
            };
            crate::write_marked(
                cells,
                row(at, stop - at),
                at,
                y,
                &place,
                dim,
                &marked(dropped),
            );
        }

        if when > 0 {
            write(cells, right.saturating_sub(when), y, &opened.when, dim);
        }
    }
}

/// Whether the projects are under a heading.
///
/// Where there are any, and where a filter has left none: a reader who has
/// narrowed the list to nothing is told so where the list was, rather than
/// having it vanish and the opening row change its words. Not on a machine
/// with nothing remembered, where a heading would be over an empty list.
fn headed(choosing: &crate::Choosing) -> bool {
    !choosing.known.is_empty() || (!choosing.naming && !choosing.typed.is_empty())
}

/// A project's name -- the last part of the path a row shows -- and how
/// many characters of that path come before it.
///
/// Either separator, because a path on Windows may be written with both.
fn name_of(path: &str) -> (usize, &str) {
    match path.rfind(std::path::is_separator) {
        Some(at) if at + 1 < path.len() => (path[..=at].chars().count(), &path[at + 1..]),
        _ => (0, path),
    }
}

/// How many characters of a path say where its project is, given how many
/// come before the name: all of them but the separator, unless that
/// separator is all there is -- a project at the root is somewhere.
fn place_length(start: usize) -> usize {
    match start {
        0 | 1 => start,
        _ => start - 1,
    }
}
