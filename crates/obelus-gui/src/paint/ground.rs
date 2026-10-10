//! What is under the letters: each cell's own colour, and the reader's
//! holds with the light round them.

use super::*;
use crate::grid::{Capped, Page};

impl Painter {
    /// The colour behind the text, as few rectangles as it takes.
    ///
    /// A run of cells with the same background is one rectangle: a screen
    /// is mostly the page's own colour, and a quad per cell would be ten
    /// thousand of them to say so.
    pub(super) fn backgrounds(
        &mut self,
        page: &Page,
        cell: CellSize,
        panes: &[&Behind],
        framed: &[&Behind],
        capped: &[&Capped],
    ) {
        let (width, height) = (cell.width, cell.height);
        for row in 0..page.rows() {
            // A frame's own cells, which `frame` has drawn already: what
            // the cells hold there is a terminal's square corner under a
            // round one.
            let ring: Vec<u16> = (0..page.columns())
                .filter(|&x| framed.iter().any(|card| card.ring_holds(page, x, row)))
                .collect();
            for (start, end, colour) in runs(page, row) {
                // The pane's own colour inside the pane is where the glass
                // is: it is the pane saying nothing there, and drawing it
                // would be painting over what the reader is meant to see
                // through. Anything else it wears -- a selected row, a
                // tab, a rule -- is the pane speaking, and stays.
                let mut holes: Vec<(u16, u16)> = ring.iter().map(|&x| (x, x + 1)).collect();
                holes.extend(
                    panes
                        .iter()
                        .filter(|pane| {
                            colour == pane.ground && row >= pane.area.y && row < pane.area.bottom()
                        })
                        .map(|pane| (pane.area.x, pane.area.right())),
                );
                // And a cap on glass, whose cells are the run a shade off
                // the page that is a terminal's cap: `caps` draws the
                // rounded one, and what is round its corners is the glass.
                // Painted, the square the cells make stood behind the
                // round cap as a ground of its own.
                holes.extend(
                    capped
                        .iter()
                        .filter(|cap| {
                            cap.area.y == row
                                && seen_through(panes, cap.area.x, cap.area.y, cap.page)
                        })
                        .map(|cap| (cap.area.x, cap.area.right())),
                );
                // And what the reader has hold of, which `holdings` draws
                // as a plate: painted here as well, the square the cells
                // make would stand behind the plate's round corners in
                // the full strength of the colour.
                holes.extend(
                    self.holds(page, row, capped)
                        .into_iter()
                        .map(|(from, to, _)| (from, to)),
                );
                for (start, end) in without(start, end, &mut holes) {
                    self.block(
                        f32::from(start) * width,
                        f32::from(row) * height,
                        f32::from(end - start) * width,
                        height,
                        rgba(colour, Ink::Background),
                    );
                }
            }
        }
    }

    /// The runs of one row the reader has hold of.
    ///
    /// A run of cells wearing one of the two colours the theme gives a
    /// hold -- see `Drawing::holding` -- which is how the window finds
    /// one without any view having to say so: a list drawn next month is
    /// drawn like every other list because it paints the row that colour,
    /// which it has to do anyway for the terminal.
    ///
    /// Minus the caps. A key's cap is a run a shade off the page and the
    /// themes Obelus ships give that the same colour as a selected row --
    /// two different promises that a theme is entitled to keep in one
    /// colour. What tells them apart is that a cap has already been said,
    /// so it is already drawn as its own shape.
    ///
    /// Except a cap said to be standing *on* the hold, which a row the
    /// reader is on offers its key in: that is part of the plate, and cut
    /// out of it the row was two plates with a square between them.
    fn holds(&self, page: &Page, row: u16, capped: &[&Capped]) -> Vec<(u16, u16, Color)> {
        held_runs(page, row, self.holding, capped)
    }

    /// What the reader has hold of, drawn as a plate rather than a square.
    ///
    /// The face is carried most of the way from what is under it to the
    /// colour the cells wear, and the colour itself is the rim round it
    /// -- see `HELD`. `backgrounds` leaves the run unpainted for this,
    /// the way it leaves a cap's cells unpainted, so what shows outside
    /// the rounded corners is whatever the hold is standing on: the
    /// page, or a pane's own glass.
    ///
    /// A hold that carries on into the row above or below keeps its
    /// corners square on that side and its face runs to the edge, so a
    /// selection several lines tall is one plate rather than a stack of
    /// them with a seam between each pair.
    pub(super) fn holdings(
        &mut self,
        page: &Page,
        panes: &[&Behind],
        framed: &[&Behind],
        capped: &[&Capped],
        cell: CellSize,
    ) {
        let rim = (cell.height * HELD_EDGE).round().max(1.0);
        let corner = cell.height * HELD_CORNER;
        // Every light goes under every plate, so the one round a row does
        // not lie over the edge of the row beside it.
        let first = self.quads.len();
        // The runs of each hold, by the colour it wears and the pane it is
        // in, a list per row of the grid: what the light round it is
        // measured from.
        let mut groups: Vec<((Color, Option<usize>), Runs)> = Vec::new();
        let rows: Vec<Vec<(u16, u16, Color)>> = (0..page.rows())
            .map(|row| self.holds(page, row, capped))
            .collect();
        for (row, holds) in rows.iter().enumerate() {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "a row of a grid is inside the window"
            )]
            let at = row as u16;
            // Whether a box with a frame round it is over this cell. A
            // run that stops because something was put over it has not
            // stopped: the row carries on underneath, so that end is not
            // an end -- it is square, and it keeps no rim, the same as a
            // row the hold carries on into.
            let covered = |x: u16| {
                framed.iter().any(|card| {
                    (card.area.left()..card.area.right()).contains(&x)
                        && (card.area.top()..card.area.bottom()).contains(&at)
                })
            };
            let touching = |beside: Option<&Vec<(u16, u16, Color)>>, run: (u16, u16, Color)| {
                beside.is_some_and(|beside| {
                    beside
                        .iter()
                        .any(|&(from, to, colour)| colour == run.2 && from < run.1 && to > run.0)
                })
            };
            for &(start, end, colour) in holds {
                let above = touching(
                    row.checked_sub(1).and_then(|row| rows.get(row)),
                    (start, end, colour),
                );
                let below = touching(rows.get(row + 1), (start, end, colour));
                let cut = (start.checked_sub(1).is_some_and(covered), covered(end));
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "a window is thousands of pixels, not millions"
                )]
                let top = row as f32 * cell.height;
                let left = f32::from(start) * cell.width;
                let wide = f32::from(end - start) * cell.width;
                let ink = rgba(colour, Ink::Background);
                let under = self.under(panes, framed, start, at);
                // The rim, further from what is under it than the colour
                // itself: it is the thing being seen.
                let edge = mixed(ink, away(under, ink), HELD_RIM);
                // Which way each of the four corners turns, which is a
                // fact about the row beside it -- see `Turn`.
                let beside = |row: Option<usize>| -> Vec<(u16, u16)> {
                    row.and_then(|row| rows.get(row))
                        .into_iter()
                        .flatten()
                        .filter(|&&(_, _, theirs)| theirs == colour)
                        .map(|&(from, to, _)| (from, to))
                        .collect()
                };
                let turns = Turn::corners(
                    start,
                    end,
                    &beside(row.checked_sub(1)),
                    &beside(Some(row + 1)),
                    cut,
                );
                let pane = panes.iter().rposition(|pane| {
                    (pane.area.left()..pane.area.right()).contains(&start)
                        && (pane.area.top()..pane.area.bottom()).contains(&at)
                });
                let group = match groups.iter().position(|(key, _)| *key == (colour, pane)) {
                    Some(group) => group,
                    None => {
                        groups.push(((colour, pane), vec![Vec::new(); rows.len()]));
                        groups.len() - 1
                    }
                };
                groups[group].1[row].push((left, left + wide, turns));
                self.plate(
                    [left, top, wide, cell.height],
                    corner,
                    edge,
                    turns << HELD_TURNS,
                );
                // The face, inside the rim where there is one. No inset
                // where the hold carries on: an edge there is a seam
                // across the middle of one thing.
                let (up, down) = (if above { 0.0 } else { rim }, if below { 0.0 } else { rim });
                let (near, far) = (if cut.0 { 0.0 } else { rim }, if cut.1 { 0.0 } else { rim });
                let face = [
                    left + near,
                    top + up,
                    (wide - near - far).max(0.0),
                    (cell.height - up - down).max(0.0),
                ];
                self.plate(
                    face,
                    (corner - rim).max(0.0),
                    mixed(under, ink, HELD),
                    turns << HELD_TURNS,
                );
            }
        }
        let lights = groups
            .iter()
            .flat_map(|((colour, pane), runs)| {
                lights_round(page, panes, *colour, *pane, runs, cell)
            })
            .collect::<Vec<_>>();
        self.quads.splice(first..first, lights);
    }

    /// The face a hold is drawn in at this cell, where the cell is part
    /// of one.
    ///
    /// Asked by anything that would otherwise put a cell's own ground
    /// back over a hold. The cells carry the colour at full strength and
    /// the plate is a shade of it, so a square of the raw colour is a
    /// hole in the plate -- which is the same thing a square of a pane's
    /// ground is in glass, and is guarded against a line above for the
    /// same reason.
    pub(super) fn held_face(
        &self,
        page: &Page,
        at: ratatui::layout::Rect,
        panes: &[&Behind],
        framed: &[&Behind],
        capped: &[&Capped],
    ) -> Option<[f32; 4]> {
        let (from, _, colour) = self
            .holds(page, at.y, capped)
            .into_iter()
            .find(|&(from, to, _)| (from..to).contains(&at.x))?;
        let under = self.under(panes, framed, from, at.y);
        Some(mixed(under, rgba(colour, Ink::Background), HELD))
    }

    /// The page's own ground, as a colour.
    pub(super) fn ground_colour(&self) -> [f32; 4] {
        rgba(self.ground, Ink::Background)
    }

    /// What a hold at this cell is standing on: a pane's own ground where
    /// it is inside one, and the page's otherwise.
    ///
    /// Nearest the reader wins, which is the order the cards are in.
    pub(super) fn under(&self, panes: &[&Behind], framed: &[&Behind], x: u16, y: u16) -> [f32; 4] {
        let inside = |pane: &&&Behind| {
            let area = pane.area;
            (area.left()..area.right()).contains(&x) && (area.top()..area.bottom()).contains(&y)
        };
        let ground = framed
            .iter()
            .rev()
            .find(inside)
            .or_else(|| panes.iter().rev().find(inside))
            .map_or(self.ground, |pane| pane.ground);
        rgba(ground, Ink::Background)
    }

    /// A block of colour with each of its four corners taken off, or not,
    /// or bent the other way -- see `Turn`.
    ///
    /// The quad reaches a radius past the block on each side, because a
    /// corner bent the other way is drawn out there: the shader takes
    /// that much off again to find the block itself.
    fn plate(&mut self, rect: [f32; 4], radius: f32, colour: [f32; 4], flags: u32) {
        let [left, top, width, height] = rect;
        let radius = radius.max(0.0).min(width.min(height) / 2.0);
        self.quads.push(Quad {
            rect: [left - radius, top, radius.mul_add(2.0, width), height],
            uv: self.atlas.white,
            colour,
            flags: SOLID | ROUNDED | HELD_PLATE | flags,
            radius,
            layer: 0,
            lower: 0.0,
        });
    }
}

/// The runs of one colour along a row, as few as they can be said in.
///
/// Two things are settled here. A screen is mostly the page's own colour,
/// so a rectangle per cell would be ten thousand of them to say one thing.
/// And a full-width character owns both of its columns: the second one is a
/// cell `ratatui` has reset, with no text and no colours, which a terminal
/// never draws because it advanced two columns itself. A window draws every
/// cell, so reading that cell's own background painted the default colour
/// behind the right half of every Chinese character -- a line of them came
/// out striped.
pub(super) fn runs(page: &Page, row: u16) -> Vec<(u16, u16, Color)> {
    runs_from(
        0,
        (0..page.columns()).map(|column| {
            let look = page.look(column, row);
            (look.columns(), look.background)
        }),
    )
}

/// The same, over a row of cells from wherever they come.
///
/// Two callers and one rule: the page's own row, and the row of a picture
/// of what a pane was opened over. That picture is read through the glass,
/// and it used to be painted a cell at a time -- so a full-width character
/// had the reset cell beside it painted over the right half of its glyph,
/// and a page of Chinese seen through the glass was a page of half
/// characters. Said in one place, the second column belongs to the
/// character in both.
///
/// Each cell as how many columns it takes and what it is drawn on.
pub(super) fn runs_from(
    start: u16,
    cells: impl Iterator<Item = (u16, Color)>,
) -> Vec<(u16, u16, Color)> {
    let mut runs: Vec<(u16, u16, Color)> = Vec::new();
    // How many columns of the character just seen are still to come.
    let mut rest = 0;
    for (along, (columns, background)) in cells.enumerate() {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a row of the grid, which is not thousands of columns"
        )]
        let column = start + along as u16;
        let colour = match rest {
            0 => {
                rest = columns - 1;
                background
            }
            // The rest of a character that was seen already, so there is
            // always a run to take the colour from.
            _ => {
                rest -= 1;
                runs.last().map_or(Color::Reset, |&(_, _, running)| running)
            }
        };
        match runs.last_mut() {
            Some((_, end, running)) if *running == colour && *end == column => *end = column + 1,
            _ => runs.push((column, column + 1, colour)),
        }
    }
    runs
}

/// What is left of a run of cells once the holes in it are taken out.
///
/// The holes in any order and overlapping as they please: a frame's ring
/// and a pane's glass can both be in one row.
pub(super) fn without(start: u16, end: u16, holes: &mut [(u16, u16)]) -> Vec<(u16, u16)> {
    holes.sort_unstable();
    let mut left = Vec::new();
    let mut from = start;
    for &(hole, after) in holes.iter() {
        if after <= from || hole >= end {
            continue;
        }
        if hole > from {
            left.push((from, hole));
        }
        from = from.max(after);
    }
    if from < end {
        left.push((from, end));
    }
    left
}

/// The runs of one row the reader has hold of -- see `Drawing::holds`,
/// which is this with the colours of a hold handed in.
pub(super) fn held_runs(
    page: &Page,
    row: u16,
    holding: (Color, Color),
    capped: &[&Capped],
) -> Vec<(u16, u16, Color)> {
    let (held, chosen) = holding;
    let mut found = Vec::new();
    for (start, end, colour) in runs(page, row) {
        if colour != held && colour != chosen {
            continue;
        }
        let mut caps: Vec<(u16, u16)> = capped
            .iter()
            .filter(|cap| cap.area.y == row && cap.page != colour)
            .map(|cap| (cap.area.x, cap.area.right()))
            .collect();
        found.extend(
            without(start, end, &mut caps)
                .into_iter()
                .map(|(from, to)| (from, to, colour)),
        );
    }
    found
}

/// A colour some of the way from one to another.
///
/// What a settled bar is drawn in. Mixed rather than given an alpha
/// because the page under it is opaque and already known, so this is the
/// colour it would come out as -- and nothing here then depends on how
/// the pipeline happens to blend, which is a thing that has to be right
/// in the shader as well as here.
/// Which way one corner of a held run turns.
///
/// A hold is one rectangle per row, and the rows are not the same width:
/// a selection starts part way along a line and stops part way along
/// another. What decides a corner is the row beside it -- whether that
/// row stops short of this one, stops level with it, or carries on past
/// it -- and the third is the one that matters, because without it every
/// step between two rows is cut square and the hold reads as a stack of
/// plates rather than as one shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Turn {
    /// The hold's own corner: the row beside it stops short, or there is
    /// no row beside it.
    Corner = 0,
    /// The row beside it carries on past this one, so the boundary bends
    /// the other way to meet it.
    Other = 1,
    /// The row beside it has the same edge, so there is no corner here at
    /// all and nothing to round.
    None = 2,
}

impl Turn {
    /// The four corners of a run, packed two bits each in the order the
    /// shader reads them: top left, top right, bottom left, bottom right.
    ///
    /// `cut` says whether something was put *over* the run at either end
    /// -- a box with a frame round it, sitting on the row. That end is
    /// not an end: the row carries on under the box, so the corners
    /// there are no corners, the same as where the row beside it is
    /// level. Without it a selected row with a card over its middle is
    /// two little plates with four round corners each, one at either end
    /// of the row, which read as badges rather than as the row they are.
    ///
    /// `above` and `below` are every run of the hold on those rows. A row
    /// can be in more than one piece -- a box put over the middle of it --
    /// and which way a corner turns is asked of the piece that reaches
    /// furthest out past that end of this run, not of whichever came first.
    fn corners(
        start: u16,
        end: u16,
        above: &[(u16, u16)],
        below: &[(u16, u16)],
        cut: (bool, bool),
    ) -> u32 {
        let touching = |beside: &[(u16, u16)]| {
            beside
                .iter()
                .filter(|&&(from, to)| from < end && to > start)
                .copied()
                .collect::<Vec<_>>()
        };
        let left = |beside: &[(u16, u16)]| match touching(beside).iter().map(|run| run.0).min() {
            Some(from) if from < start => Self::Other,
            Some(from) if from == start => Self::None,
            _ => Self::Corner,
        };
        let right = |beside: &[(u16, u16)]| match touching(beside).iter().map(|run| run.1).max() {
            Some(to) if to > end => Self::Other,
            Some(to) if to == end => Self::None,
            _ => Self::Corner,
        };
        let cut_to = |side: bool, turn: Self| match side {
            true => Self::None,
            false => turn,
        };
        [
            cut_to(cut.0, left(above)),
            cut_to(cut.1, right(above)),
            cut_to(cut.0, left(below)),
            cut_to(cut.1, right(below)),
        ]
        .into_iter()
        .enumerate()
        .fold(0, |turns, (at, turn)| turns | ((turn as u32) << (at * 2)))
    }
}

/// The light round one hold, as a quad per stretch of a row -- see
/// `lights`.
///
/// Kept to what the hold is in: the pane or box it is drawn on, or the
/// grid, and never on anything put over that. A light past the grid is
/// in the margin, which is the page's own ground and nothing else, and
/// one past a pane's edge is on whatever the pane was put over.
fn lights_round(
    page: &Page,
    panes: &[&Behind],
    colour: Color,
    pane: Option<usize>,
    runs: &[Vec<(f32, f32, u32)>],
    cell: CellSize,
) -> Vec<Quad> {
    let reach = (cell.height * HELD_REACH).round();
    let ink = rgba(colour, Ink::Background);
    let area = pane.map_or_else(
        || ratatui::layout::Rect::new(0, 0, page.columns(), page.rows()),
        |pane| panes[pane].area,
    );
    let room = (
        f32::from(area.left()) * cell.width,
        f32::from(area.right()) * cell.width,
    );
    let over = &panes[pane.map_or(0, |pane| pane + 1)..];
    (area.top()..area.bottom())
        .flat_map(|row| {
            let holes: Vec<(f32, f32)> = over
                .iter()
                .filter(|pane| (pane.area.top()..pane.area.bottom()).contains(&row))
                .map(|pane| {
                    (
                        f32::from(pane.area.left()) * cell.width,
                        f32::from(pane.area.right()) * cell.width,
                    )
                })
                .collect();
            lights(runs, usize::from(row), reach, room, &holes)
                .into_iter()
                .map(move |light| {
                    let (own, turns) = light
                        .own
                        .map_or(((0.0, 0.0), 0), |(from, to, turns)| ((from, to), turns));
                    let ((over_from, over_to), (under_from, under_to)) = (
                        light.above.unwrap_or_default(),
                        light.below.unwrap_or_default(),
                    );
                    let near = u32::from(light.above.is_some())
                        | u32::from(light.below.is_some()) << 1
                        | u32::from(light.own.is_some()) << 2;
                    Quad {
                        rect: [
                            light.from,
                            f32::from(row) * cell.height,
                            light.to - light.from,
                            cell.height,
                        ],
                        uv: [over_from, over_to, under_from, under_to],
                        colour: [ink[0], ink[1], ink[2], ink[3] * HELD_LIGHT],
                        flags: LIGHT | turns << HELD_TURNS | near << HELD_NEAR,
                        radius: own.0,
                        layer: 0,
                        lower: own.1,
                    }
                })
        })
        .collect()
}

/// The runs of one hold, a list per row of the grid: where each begins
/// and ends in pixels, and the way its corners turn.
type Runs = Vec<Vec<(f32, f32, u32)>>;

/// A stretch of one row of the grid the light round a hold is drawn
/// across, in pixels, and the run nearest it on each of the three rows a
/// light can reach from: its own with the way its corners turn, and the
/// ones above and below.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Light {
    from: f32,
    to: f32,
    own: Option<(f32, f32, u32)>,
    above: Option<(f32, f32)>,
    below: Option<(f32, f32)>,
}

/// Where the light round a hold is drawn on one row, as stretches that
/// never overlap.
///
/// Every pixel round a hold is drawn once, by the row it is on: a light
/// per run, reaching up and down into the rows beside it, lays two lights
/// over every place two of them reach, which is where two rows meet at a
/// corner -- a brighter spot nobody put there. So a row's stretch measures
/// to the rows above and below it as well as to its own, and the nearest
/// of the three is the distance to the whole hold, because the reach is
/// less than a row and nothing two rows off can be nearer.
///
/// And a stretch is cut wherever the nearest run on any of the three rows
/// changes, which is half way between two of them, because the shader is
/// told one run a row: a row in two pieces measured to only one of them
/// leaves a step beside the other dark.
///
/// `room` is what the hold is in, and `holes` is what was put over it on
/// this row; no light is drawn outside the one or inside the other.
fn lights(
    rows: &[Vec<(f32, f32, u32)>],
    row: usize,
    reach: f32,
    room: (f32, f32),
    holes: &[(f32, f32)],
) -> Vec<Light> {
    let nothing = Vec::new();
    let runs = |row: Option<usize>| row.and_then(|row| rows.get(row)).unwrap_or(&nothing);
    let three = [
        runs(row.checked_sub(1)),
        runs(Some(row)),
        runs(Some(row + 1)),
    ];
    let mut cuts = vec![room.0, room.1];
    for runs in three {
        cuts.extend(runs.windows(2).map(|pair| (pair[0].1 + pair[1].0) / 2.0));
    }
    cuts.extend(holes.iter().flat_map(|&(from, to)| [from, to]));
    cuts.retain(|cut| (room.0..=room.1).contains(cut));
    cuts.sort_by(f32::total_cmp);
    cuts.dedup();
    // How far a run is from a place along the row.
    let gap = |run: &(f32, f32, u32), at: f32| (run.0 - at).max(at - run.1).max(0.0);
    cuts.windows(2)
        .filter_map(|pair| {
            let middle = (pair[0] + pair[1]) / 2.0;
            if holes
                .iter()
                .any(|&(from, to)| from <= middle && middle < to)
            {
                return None;
            }
            let [above, own, below] = three.map(|runs| {
                runs.iter()
                    .min_by(|one, other| gap(one, middle).total_cmp(&gap(other, middle)))
                    .copied()
            });
            // No further than the light reaches past the runs it measures
            // to: past that it has nothing left to show.
            let near = [above, own, below].into_iter().flatten();
            let from = near.clone().map(|run| run.0).fold(f32::INFINITY, f32::min) - reach;
            let to = near.map(|run| run.1).fold(f32::NEG_INFINITY, f32::max) + reach;
            let (from, to) = (pair[0].max(from), pair[1].min(to));
            (from < to).then_some(Light {
                from,
                to,
                own,
                above: above.map(|(from, to, _)| (from, to)),
                below: below.map(|(from, to, _)| (from, to)),
            })
        })
        .collect()
}

fn away(from: [f32; 4], to: [f32; 4]) -> [f32; 4] {
    let mut out = to;
    for channel in 0..3 {
        out[channel] = (to[channel] * 2.0 - from[channel]).clamp(0.0, 1.0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row of the light round a hold, in cells a pixel wide.
    fn lit(rows: &[&[(f32, f32)]], row: usize, holes: &[(f32, f32)]) -> Vec<Light> {
        let rows: Vec<Vec<(f32, f32, u32)>> = rows
            .iter()
            .map(|runs| runs.iter().map(|&(from, to)| (from, to, 0)).collect())
            .collect();
        lights(&rows, row, 3.0, (0.0, 100.0), holes)
    }

    /// Where two rows of a hold meet at no more than a corner, the place
    /// round that corner is lit by one row, measuring to the other.
    ///
    /// Selecting down from the middle of a line leaves the line from
    /// there to its end and the next one up to there, which touch at a
    /// point. A light per run reaching into the rows beside it drew both
    /// lights round that point, and two lights over one place is a brighter
    /// place.
    ///
    /// Deliberate break: give each row's stretch only its own row's run,
    /// and the row above's stretch beside the corner has nothing below it
    /// to measure to.
    #[test]
    fn rows_that_meet_at_a_corner_are_lit_once_round_it() {
        let rows: [&[(f32, f32)]; 2] = [&[(40.0, 90.0)], &[(10.0, 40.0)]];
        let upper = lit(&rows, 0, &[]);
        let beside = upper
            .iter()
            .find(|light| light.from < 39.0 && light.to > 39.0)
            .expect("lit beside the corner, on the upper row");
        assert_eq!(beside.below, Some((10.0, 40.0)), "{upper:?}");
        // And nothing reaches out of its own row: a stretch is a row tall,
        // and never over another stretch of it.
        for row in 0..3 {
            let lit = lit(&rows, row, &[]);
            for pair in lit.windows(2) {
                assert!(pair[0].to <= pair[1].from, "row {row}: {lit:?}");
            }
        }
    }

    /// A row lights the step where the row beside it carries on past it,
    /// all the way along.
    ///
    /// Deliberate break: keep each stretch to its own run and a reach
    /// either side, which is what drew the step lit for a reach and then
    /// cut square.
    #[test]
    fn the_step_where_the_row_above_carries_on_is_lit_along_it() {
        let rows: [&[(f32, f32)]; 2] = [&[(0.0, 80.0)], &[(0.0, 10.0)]];
        let lower = lit(&rows, 1, &[]);
        let to = lower.iter().map(|light| light.to).fold(0.0, f32::max);
        assert!((to - 83.0).abs() < f32::EPSILON, "{lower:?}");
    }

    /// A row in two pieces is measured to by the piece nearest each place.
    ///
    /// Deliberate break: measure each row to its first run, and the step
    /// under the right-hand piece is measured to the left-hand one, which
    /// is too far away to light it.
    #[test]
    fn a_row_in_two_pieces_is_measured_to_the_nearer() {
        let rows: [&[(f32, f32)]; 2] = [&[(0.0, 30.0), (60.0, 90.0)], &[(0.0, 85.0)]];
        let lower = lit(&rows, 1, &[]);
        let under = lower
            .iter()
            .find(|light| light.from <= 87.0 && light.to > 87.0)
            .expect("lit under the end of the right-hand piece");
        assert_eq!(under.above, Some((60.0, 90.0)), "{lower:?}");
    }

    /// No light outside what the hold is in, nor on what was put over it.
    ///
    /// Deliberate break: leave the room out of the cuts and keep every
    /// stretch, and the first reaches into the margin left of the grid;
    /// leave the holes out and a stretch is drawn across the box.
    #[test]
    fn a_light_stays_in_its_room_and_off_what_is_over_it() {
        let rows: [&[(f32, f32)]; 1] = [&[(0.0, 50.0)]];
        let lit = lit(&rows, 0, &[(20.0, 30.0)]);
        assert!(lit.iter().all(|light| light.from >= 0.0), "{lit:?}");
        assert!(
            lit.iter()
                .all(|light| light.to <= 20.0 || light.from >= 30.0),
            "{lit:?}"
        );
    }

    /// No turn a plate can carry is read as a shadow.
    ///
    /// Asked of what `Turn::corners` packs rather than of the bits, so a
    /// shadow anywhere a real turn lands fails it. Deliberate break: put
    /// `SHADOW` back at 8192, and a hold under a row reaching further
    /// left -- `Other` at its top left -- is a shadow.
    #[test]
    fn a_shadow_is_none_of_the_turns() {
        let row = |from: u16, to: u16| Some((from, to));
        for (above, below) in [
            (row(0, 20), None),
            (None, row(0, 20)),
            (row(5, 20), row(5, 20)),
        ] {
            for cut in [(false, false), (true, false), (false, true)] {
                let turns = Turn::corners(5, 10, above.as_slice(), below.as_slice(), cut);
                assert_eq!((turns << HELD_TURNS) & SHADOW, 0, "{turns:08b}");
            }
        }
        assert_eq!(SHADOW & (LIGHT | WEDGE), 0);
    }

    /// Which way a hold's corner turns is a fact about the row beside it.
    ///
    /// A hold is one rectangle per row and the rows are not the same
    /// width, so the corners where they step are not the hold's own
    /// corners: the boundary there bends the other way, round into the
    /// row that carries on. Without that, every step is cut square and a
    /// selection several lines tall reads as a stack of plates.
    ///
    /// Deliberate break: answer `Corner` in place of `Other` and the
    /// steps come out square again -- which is what the first of these
    /// drew, and what a reader looking at a selection notices first.
    /// Answer `Corner` in place of `None` and a flush edge grows two
    /// notches where there is no corner at all.
    #[test]
    fn a_corner_turns_the_other_way_where_the_row_beside_it_carries_on() {
        let turns = |above: Option<(u16, u16)>, below: Option<(u16, u16)>| {
            Turn::corners(10, 20, above.as_slice(), below.as_slice(), (false, false))
        };
        // Top left, top right, bottom left, bottom right.
        let at = |turns: u32, corner: u32| (turns >> (corner * 2)) & 3;

        let alone = turns(None, None);
        for corner in 0..4 {
            assert_eq!(
                at(alone, corner),
                Turn::Corner as u32,
                "corner {corner} of a row with nothing beside it"
            );
        }

        // The row above has the same ends, so along the top there is no
        // corner to round at all.
        let flush = turns(Some((10, 20)), None);
        assert_eq!(at(flush, 0), Turn::None as u32, "top left, level");
        assert_eq!(at(flush, 1), Turn::None as u32, "top right, level");
        assert_eq!(at(flush, 2), Turn::Corner as u32, "and nothing below it");

        // It carries on past this row at both ends.
        let wider = turns(Some((0, 30)), None);
        assert_eq!(at(wider, 0), Turn::Other as u32, "top left, above is wider");
        assert_eq!(
            at(wider, 1),
            Turn::Other as u32,
            "top right, above is wider"
        );

        // And one that stops short leaves this row its own corners.
        let narrower = turns(Some((12, 18)), None);
        assert_eq!(at(narrower, 0), Turn::Corner as u32, "above stops short");
        assert_eq!(at(narrower, 1), Turn::Corner as u32, "at both ends");

        // The row below is asked the same question of the other two.
        let step = turns(None, Some((0, 20)));
        assert_eq!(
            at(step, 2),
            Turn::Other as u32,
            "bottom left, below runs on"
        );
        assert_eq!(at(step, 3), Turn::None as u32, "bottom right, level");

        // And where something was put over the run, the end it stops at
        // is not an end: the row carries on under the box.
        let under_a_box = Turn::corners(10, 20, &[], &[], (true, false));
        assert_eq!(at(under_a_box, 0), Turn::None as u32, "top left, cut");
        assert_eq!(at(under_a_box, 2), Turn::None as u32, "bottom left, cut");
        assert_eq!(
            at(under_a_box, 1),
            Turn::Corner as u32,
            "and the other end is still the hold's own"
        );

        // A row above in two pieces, a box over the middle of it: each end
        // turns for the piece that reaches past it, not for the first.
        // Deliberate break: ask only the first piece, and the top right
        // corner is the hold's own, under a row that carries on past it.
        let pieces = Turn::corners(10, 20, &[(0, 12), (18, 30)], &[], (false, false));
        assert_eq!(at(pieces, 0), Turn::Other as u32, "top left, in two");
        assert_eq!(at(pieces, 1), Turn::Other as u32, "top right, in two");
    }
}
