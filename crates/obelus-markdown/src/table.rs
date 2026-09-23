//! How wide a table's columns are drawn.
//!
//! A table in a terminal has a width it must fit and content that has no
//! idea about it. Giving every column the width of its widest cell is the
//! answer whenever that fits, and the whole of the difficulty is what to do
//! when it does not: something has to give, and which column gives how much
//! is the only decision here.
//!
//! Nothing in this module knows what is *in* a cell. It is given how wide
//! each cell would like to be and answers with how wide each column will be,
//! so that whoever is laying the table out can wrap the cells that no longer
//! fit. A number in and a number out, which is what makes it testable
//! against the answer obelus has been shipping.

/// The narrowest a column may be drawn.
///
/// Three cells: a character, and somewhere for the ellipsis of a word that
/// did not fit on either side of it. Below this a column says nothing at all
/// and the table would be better without it.
const NARROWEST: usize = 3;

/// The narrowest a column is squeezed to before the *first* pass gives up.
///
/// One wider than [`NARROWEST`], so that the pass that trims the roomy
/// columns leaves them still readable and the pass after it -- which is
/// allowed to go all the way down -- is the one that does the damage. Two
/// passes rather than one so that a column of long text gives up its room
/// before a column of short text does.
const COMFORTABLE: usize = 4;

/// How wide each column of a table should be drawn, to fit `room`.
///
/// `cells` is what each row would like, cell by cell; a row with fewer cells
/// than the table has columns is a row that stops early, and its missing
/// cells are not counted against anything. `room` is the whole width the
/// table has, borders included.
///
/// The answer is at most `columns` wide and may be shorter: a width that
/// cannot hold the columns at their narrowest holds as many as it can, and
/// the rest of them are not drawn. A table squeezed to nothing is worse than
/// a table that says it has been cut.
#[must_use]
pub(crate) fn widths(cells: &[Vec<usize>], columns: usize, room: usize) -> Vec<usize> {
    if columns == 0 || room == 0 {
        return Vec::new();
    }
    // One border down each side of every column, shared between neighbours,
    // so a table of `n` columns spends `n + 1` cells on its borders.
    let Some(inside) = room.checked_sub(columns + 1) else {
        return cut_to_fit(room);
    };
    // A column at its narrowest still wants a border of its own, and the
    // table wants one more at the end. Below that, columns have to go.
    if room < columns * (NARROWEST + 1) + 1 {
        return cut_to_fit(room);
    }

    let wanted = wanted(cells, columns);
    let sum: usize = wanted.iter().map(|column| column.width).sum();
    if sum <= inside {
        return wanted.iter().map(|column| column.width).collect();
    }
    // One column takes what there is. There is nothing to share it with and
    // no reason to leave any of it unused.
    if columns == 1 {
        return vec![inside];
    }
    squeezed(&wanted, inside, sum)
}

/// As many columns as the room can hold at their narrowest.
///
/// What is left when a table cannot be drawn at all: the answer is fewer
/// columns rather than narrower ones, because a column of three cells is
/// already the least that says anything.
fn cut_to_fit(room: usize) -> Vec<usize> {
    let held = room.saturating_sub(1) / (NARROWEST + 1);
    vec![NARROWEST; held]
}

/// What one column would like, and what its cells average.
///
/// The average is what tells a column of one long sentence from a column of
/// numbers with one long heading. Both are as wide as their widest cell and
/// only one of them is wasting the room.
#[derive(Clone, Copy, Debug)]
struct Column {
    /// The widest cell in it, and never less than [`NARROWEST`].
    width: usize,
    /// What its cells come to on average, rounded up.
    average: usize,
}

/// What each column would like, from the cells that are there.
///
/// A row that stopped early says nothing about the columns it did not reach:
/// counting its missing cells as empty would drag the average down and make
/// a full column look like a wasteful one.
fn wanted(cells: &[Vec<usize>], columns: usize) -> Vec<Column> {
    (0..columns)
        .map(|at| {
            let seen: Vec<usize> = cells
                .iter()
                .filter_map(|row| row.get(at).copied())
                .collect();
            let width = seen.iter().copied().max().unwrap_or(0).max(NARROWEST);
            let average = match seen.is_empty() {
                true => 0,
                false => up(seen.iter().sum::<usize>(), seen.len()),
            };
            Column { width, average }
        })
        .collect()
}

/// The columns, brought down to what there is room for.
///
/// Twice over, widest first. The first pass takes from the columns that are
/// wider than their own cells usually are -- room that one long cell asked
/// for and the rest of the column is not using -- and stops at
/// [`COMFORTABLE`]. That is the cut nobody notices. If the table still does
/// not fit, the second pass takes what is left proportionally, down to
/// [`NARROWEST`]: by then every column is losing something and the fair
/// thing is for the big ones to lose most.
fn squeezed(wanted: &[Column], inside: usize, sum: usize) -> Vec<usize> {
    let mut over = sum - inside;
    // Which column is which, so the answer can be put back in the table's
    // own order after being worked out in the order of who has the most.
    let mut order: Vec<(usize, Column, usize)> = wanted
        .iter()
        .enumerate()
        .map(|(at, column)| (at, *column, column.width))
        .collect();
    order.sort_by_key(|(_, _, width)| std::cmp::Reverse(*width));

    // What the first pass could give back if it took the whole of what every
    // roomy column is not using -- capped, because a column should not lose
    // more than a few cells to a pass whose point is that it is painless.
    let loose: usize = order
        .iter()
        .filter(|(_, column, width)| *width > COMFORTABLE && *width > column.average + 1)
        .map(|(_, column, width)| (width - column.average).min(COMFORTABLE))
        .sum();
    let taking = loose.min(over);
    if taking > 0 {
        for (_, column, width) in &mut order {
            if column.width <= COMFORTABLE || column.width <= column.average {
                continue;
            }
            let share = up((*width - column.average) * taking, loose)
                .min(over)
                .min(*width - COMFORTABLE);
            *width -= share;
            over -= share;
            if over == 0 {
                break;
            }
        }
    }

    if over > 0 {
        let room_left: usize = order
            .iter()
            .map(|(_, _, width)| width - NARROWEST)
            .sum::<usize>()
            .min(over);
        let before = over;
        for (_, _, width) in &mut order {
            let share = up((*width - NARROWEST) * before, room_left)
                .min(over)
                .min(*width - NARROWEST);
            *width -= share;
            over -= share;
            if over == 0 {
                break;
            }
        }
    }

    order.sort_by_key(|(at, _, _)| *at);
    order.into_iter().map(|(_, _, width)| width).collect()
}

/// A division that rounds up, because a column short by half a cell is a
/// column that does not fit.
const fn up(what: usize, by: usize) -> usize {
    match by {
        0 => 0,
        by => what.div_ceil(by),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A table that fits is drawn at the width its cells asked for.
    #[test]
    fn a_table_that_fits_is_left_alone() {
        let cells = vec![vec![4, 6, 5], vec![2, 3, 9]];
        assert_eq!(widths(&cells, 3, 40), vec![4, 6, 9]);
    }

    /// And never narrower than a column can say anything in.
    #[test]
    fn a_column_of_nothing_is_still_a_column() {
        assert_eq!(widths(&[vec![0, 1]], 2, 40), vec![NARROWEST, NARROWEST]);
    }

    /// One column takes the room, because there is nothing to share it with.
    #[test]
    fn one_column_takes_what_there_is() {
        assert_eq!(widths(&[vec![80]], 1, 20), vec![18]);
    }

    /// A table that cannot be drawn at all loses columns rather than being
    /// squeezed to nothing: three cells is already the least a column says
    /// anything in.
    #[test]
    fn a_table_with_no_room_keeps_the_columns_it_can() {
        assert_eq!(widths(&[vec![9, 9, 9, 9]], 4, 12), vec![3, 3]);
        assert!(widths(&[vec![9, 9]], 2, 3).is_empty());
    }

    /// What is taken is taken from the column that is wasting it.
    ///
    /// Two columns as wide as each other: one whose cells are all that wide,
    /// one with a single long cell over a column of short ones. The room
    /// comes out of the second.
    #[test]
    fn the_column_wasting_the_room_is_the_one_that_gives_it_up() {
        let cells = vec![vec![20, 20], vec![20, 2], vec![20, 2], vec![20, 2]];
        let got = widths(&cells, 2, 30);
        assert!(
            got[0] > got[1],
            "the column using its width lost more than the one that was not: {got:?}"
        );
        assert_eq!(got.iter().sum::<usize>(), 30 - 3);
    }

    /// Whatever the shape, what comes out fits.
    ///
    /// The property the whole module is for: columns plus borders never come
    /// to more than the room, and no column is narrower than it may be.
    #[test]
    fn what_comes_out_fits_the_room() {
        for columns in 1..6usize {
            for room in 1..60usize {
                for shape in 0..SHAPES {
                    let cells = shaped(columns, shape);
                    let got = widths(&cells, columns, room);
                    let drawn: usize = got.iter().sum::<usize>() + got.len() + 1;
                    assert!(
                        got.is_empty() || drawn <= room,
                        "{columns} columns in {room}: {got:?} draws {drawn}"
                    );
                    assert!(
                        got.iter().all(|width| *width >= NARROWEST),
                        "a column below the narrowest: {got:?}"
                    );
                    assert!(got.len() <= columns);
                }
            }
        }
    }

    /// How many table shapes that walk covers.
    const SHAPES: usize = 10;

    /// One table's cell widths, by shape.
    ///
    /// Spread on purpose: even columns, one long cell over short ones --
    /// which is the whole reason there are two passes -- rows that stop
    /// early, and a column of nothing.
    fn shaped(columns: usize, shape: usize) -> Vec<Vec<usize>> {
        let row = |make: &dyn Fn(usize) -> usize| (0..columns).map(make).collect::<Vec<usize>>();
        match shape {
            // Even columns: nothing for the first pass to find.
            0 => vec![row(&|at| 8 + at), row(&|at| 8 + at), row(&|at| 8 + at)],
            // One long cell in every column, over short ones: all of the
            // squeezing should come out of the first pass.
            1 => vec![row(&|at| 30 + at), row(&|_| 2), row(&|_| 3)],
            // The same in one column only.
            2 => vec![
                (0..columns)
                    .map(|at| if at == 0 { 40 } else { 6 })
                    .collect(),
                row(&|_| 4),
                row(&|_| 5),
            ],
            // Wide everywhere: the first pass finds nothing and the second
            // takes it all.
            3 => vec![row(&|_| 30), row(&|_| 29), row(&|_| 31)],
            // Nothing at all.
            4 => vec![row(&|_| 0), row(&|_| 0)],
            // Rows that stop early, so some columns are never seen.
            5 => vec![
                vec![12; columns.min(1)],
                vec![9; columns],
                vec![4; columns.min(2)],
            ],
            // One row only.
            6 => vec![row(&|at| at * 9 + 1)],
            // Alternating long and short.
            7 => vec![
                (0..columns)
                    .map(|at| if at % 2 == 0 { 25 } else { 3 })
                    .collect(),
                (0..columns)
                    .map(|at| if at % 2 == 0 { 24 } else { 3 })
                    .collect(),
            ],
            // A single enormous cell.
            8 => vec![row(&|at| if at == columns - 1 { 200 } else { 5 })],
            // Ragged.
            _ => (0..4)
                .map(|row| (0..columns).map(|at| (row * 7 + at * 13) % 25).collect())
                .collect(),
        }
    }
}
