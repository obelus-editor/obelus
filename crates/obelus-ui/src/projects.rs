//! What Obelus shows while it is asking which project.
//!
//! **A page of its own, and not the welcome screen with its keys taken
//! away.** The welcome screen is the way in to a project: a wordmark, and
//! the keys that open a file, search, ask the agent. None of those means
//! anything until there is a project, so this was drawn in its place under
//! the same plate -- which said "welcome" over a question that has to be
//! answered before anybody is welcome anywhere. The welcome screen comes
//! after, once there is somewhere to be welcomed into.
//!
//! **Laid out the way the settings and the counts are**, because it is the
//! same kind of thing: a page that is the whole of the screen, with what
//! it is along the top under a rule, and the keys it answers to along the
//! foot over another. A reader who has learned one of those pages has
//! learned where to look on this one. The rows are the projects, newest
//! first, with the row that opens one not in the list under them -- held
//! there while the projects scroll, because a list of twenty that pushed
//! the way to a twenty-first off the screen would be a page whose one
//! sure answer was out of sight.
//!
//! What went wrong on the way up is not drawn here. It is a list put up
//! over this page when Obelus starts, the way it is put up over the welcome
//! screen -- `App::tell_what_went_wrong` -- and the page has the keys back
//! once the reader has read it and let it go.

use crossterm::event::{KeyCode, KeyModifiers};
use obelus_command::Command;
use obelus_editing::keymap::{KeyChord, Keymap};
use obelus_theme::Theme;
use ratatui::{buffer::Buffer as CellBuffer, layout::Rect, style::Style, widgets::Widget};

use crate::{Hint, Screen, fill, write};

/// What the page is, along the top of it.
///
/// What the reader is about to do rather than what the rows are: the rows
/// are the projects, and the row under them is not one.
const TITLE: &str = "Open a project";

/// What the row that opens a project not in the list says.
///
/// A verb, because the row is an action among rows that are places. The
/// ellipsis is what a desktop means by "this opens somewhere to say more",
/// and here what it opens is the box at the foot.
///
/// Two of them, because "another" is a word about a list: on a machine
/// with nothing remembered the row is the whole of the page, and there is
/// nothing for it to be another of -- and the page's own title already
/// says what it opens.
const OPEN_ANOTHER: &str = "Open another…";
const TYPE_A_PATH: &str = "Type a path…";

/// What the list says when the filter has left none of them.
const NO_MATCH: &str = "No project matches";

/// The least space between a path and the time beside it.
///
/// Two columns, so that the end of one and the start of the other are
/// never read as one word on a row where both reach their room. And the
/// same between a project's name and where it is.
const WHEN_GAP: u16 = 2;

/// The keys the foot says, and which of them do anything now.
///
/// Not the arrows, which every list answers to. What is here is what a
/// reader could not guess -- that the rows are narrowed by typing at them,
/// what enter does on the row they are on -- and the way out, which on any
/// other page is escape and so goes unsaid. Here escape cannot leave:
/// there is nothing nearer than this page to give up on, and nothing
/// behind it to give up to. The one key that leaves is the one that leaves
/// Obelus, and a page with no way off that it names is a trap.
#[must_use]
pub fn hints(choosing: &crate::Choosing, keymap: &Keymap) -> Vec<Hint> {
    let bare = |code| KeyChord::new(code, KeyModifiers::NONE);
    let on_the_opening_row = choosing.at >= choosing.known.len();
    let mut hints = match choosing.naming {
        false => vec![
            Hint::common(
                bare(KeyCode::Enter),
                match on_the_opening_row {
                    true => "Type a path",
                    false => "Open",
                },
            ),
            Hint::common(bare(KeyCode::Char('a')), "To filter")
                .written("type")
                .saying("Type to narrow the list"),
        ],
        // What enter does to a path is what the row at the foot is already
        // saying in its ink, so it is offered only where it would do it:
        // a candidate taken into the box, or a path that is there.
        true => vec![
            Hint::common(
                bare(KeyCode::Enter),
                match choosing.offering {
                    true => "Fill it in",
                    false => "Open",
                },
            )
            .when(choosing.offering || choosing.there),
        ],
    };
    if let Some(chord) = keymap.chord_for(Command::Quit) {
        hints.push(Hint::common(chord, "Leave").saying("Leave Obelus"));
    }
    hints
}

/// Where everything on the page goes.
///
/// One answer, asked by the drawing and by the keys that page: a page is
/// worth what is on screen, and two answers to how many projects that is
/// would be a page key that skips some.
struct Laid {
    /// The row the title is on.
    title: Rect,
    /// The rule under it.
    rule: Rect,
    /// The projects, or the line saying the filter left none.
    list: Rect,
    /// The row that opens a project not in the list.
    opening: Rect,
}

/// How the page is laid out in `area`.
fn laid(area: Rect, choosing: &crate::Choosing, hints: &[Hint]) -> Laid {
    let page = crate::footed(area, hints);
    let row = |y: u16| Rect {
        y,
        height: 1,
        ..page
    };
    let below = page.y.saturating_add(2).min(page.bottom());
    let room = page.bottom() - below;
    // The projects, or where the filter left none the line saying so; and
    // on a machine with nothing remembered no list at all, so the row that
    // opens one is the first row under the rule.
    let listed = match headed(choosing) {
        true => u16::try_from(choosing.known.len().max(1)).unwrap_or(u16::MAX),
        false => 0,
    };
    let gap = u16::from(listed > 0);
    // The projects get what the opening row and the blank over it leave,
    // and the filter is what reaches the ones below the fold.
    let shown = listed.min(room.saturating_sub(gap + 1));
    let list = Rect {
        y: below,
        height: shown,
        // The last column is the scrollbar's whether or not there is one:
        // handing it back when the list fits would move every time on the
        // page a column whenever a project was forgotten.
        width: page.width.saturating_sub(1),
        ..page
    };
    let opening = Rect {
        width: list.width,
        ..row((below + shown + gap).min(page.bottom().saturating_sub(1)))
    };
    Laid {
        title: row(page.y),
        rule: row(page.y.saturating_add(1)),
        list,
        opening,
    }
}

/// How many projects the page has room for in `area`, which is what the
/// paging keys move by.
#[must_use]
pub fn list_height(area: Rect, choosing: &crate::Choosing, keymap: &Keymap) -> u16 {
    laid(area, choosing, &hints(choosing, keymap)).list.height
}

/// The page.
pub struct ProjectsView<'a> {
    choosing: crate::Choosing,
    keymap: &'a Keymap,
    theme: &'a Theme,
}

impl<'a> ProjectsView<'a> {
    /// Borrows what the view needs, where Obelus is asking.
    #[must_use]
    pub fn new(app: &'a impl Screen) -> Option<Self> {
        Some(Self {
            choosing: app.choosing()?,
            keymap: app.keymap(),
            theme: app.theme(),
        })
    }
}

impl Widget for ProjectsView<'_> {
    fn render(self, area: Rect, cells: &mut CellBuffer) {
        let choosing = &self.choosing;
        let hints = hints(choosing, self.keymap);
        let laid = laid(area, choosing, &hints);

        write(
            cells,
            laid.title.x + 2,
            laid.title.y,
            TITLE,
            Style::new().fg(self.theme.foreground),
        );
        crate::rule(cells, laid.rule, self.theme);
        if headed(choosing) {
            self.rows(cells, choosing, laid.list);
        }
        let on = choosing.at >= choosing.known.len();
        let style = self.row_style(cells, laid.opening, on);
        let said = match headed(choosing) {
            true => OPEN_ANOTHER,
            false => TYPE_A_PATH,
        };
        write(
            cells,
            laid.opening.x + 2,
            laid.opening.y,
            said,
            style.fg(self.theme.foreground),
        );
        crate::foot_without_a_card(cells, area, &hints, self.theme);
    }
}

impl ProjectsView<'_> {
    /// One mark for "the keys are here", and it says nothing else: the
    /// same colour behind the row the reader is on that every list, page
    /// and card in Obelus puts there.
    fn row_style(&self, cells: &mut CellBuffer, row: Rect, on: bool) -> Style {
        match on {
            true => {
                let selected = Style::new().bg(self.theme.selected_row_background);
                fill(cells, row, selected);
                selected
            }
            false => Style::new(),
        }
    }

    /// The projects, from where the window over them is.
    fn rows(&self, cells: &mut CellBuffer, choosing: &crate::Choosing, list: Rect) {
        let total = choosing.known.len();
        if total == 0 {
            crate::nothing(
                cells,
                Rect {
                    x: list.x + 1,
                    height: 1,
                    ..list
                },
                NO_MATCH,
                self.theme,
            );
            return;
        }
        let names = self.name_width(choosing, list.width);
        let first = choosing.top.min(total);
        for (offset, (at, opened)) in choosing
            .known
            .iter()
            .enumerate()
            .skip(first)
            .take(usize::from(list.height))
            .enumerate()
        {
            let Ok(offset) = u16::try_from(offset) else {
                break;
            };
            let row = Rect {
                y: list.y + offset,
                height: 1,
                ..list
            };
            let style = self.row_style(cells, row, at == choosing.at);
            self.row(cells, row, names, opened, style);
        }
        if total > usize::from(list.height) {
            crate::scrollbar(
                cells,
                Rect {
                    width: list.width + 1,
                    ..list
                },
                first,
                total,
                self.theme,
            );
        }
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
    fn row(
        &self,
        cells: &mut CellBuffer,
        line: Rect,
        names: u16,
        opened: &crate::Opened,
        style: Style,
    ) {
        let (left, y) = (line.x, line.y);
        let right = left + line.width.saturating_sub(2);
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

/// Whether the projects are drawn at all.
///
/// Where there are any, and where a filter has left none: a reader who has
/// narrowed the list to nothing is told so where the list was, rather than
/// having it vanish and the opening row change its words. Not on a machine
/// with nothing remembered, where the opening row is the whole of the page.
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
