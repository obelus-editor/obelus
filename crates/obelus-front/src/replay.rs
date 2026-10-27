//! What is on the screen now, kept beside Obelus so that a window that
//! comes back can be handed all of it.
//!
//! **A window that comes back has nothing.** What Obelus sends is what
//! changed since the last frame, which is right for a window that saw the
//! last frame and useless for one that did not: a window whose connection
//! went and came back, or a new window on an Obelus that has been running
//! for a day. Obelus's loop could be asked to draw everything again, but it
//! is somebody else's loop to interrupt for a fact that is already known --
//! everything it has said is here, and [`Screen::again`] says it again.
//!
//! What it keeps is what a window keeps: every cell as it was last
//! written, the last word on each thing the settings tell a window, every
//! mark it was given to draw, and the shapes the last whole frame said --
//! which a window replaces at every frame, so the last frame's are all of
//! them.

use std::collections::BTreeMap;

use ratatui::{buffer::Cell, layout::Position};

use crate::Update;

/// The screen, as everything said about it so far leaves it.
#[derive(Debug, Default)]
pub struct Screen {
    /// Every cell written since the screen was last cleared, by row and
    /// then column so that they are said again in order -- a grid rather
    /// than a map, because every cell of a frame comes through here and a
    /// page scrolled is all of them.
    cells: Vec<Vec<Option<Cell>>>,
    /// Where the caret is, where anything has said.
    caret: Option<Option<Position>>,
    /// The last word on each thing the settings tell a window, by what it
    /// is about.
    settled: BTreeMap<u8, Update>,
    /// Every mark a window was given to draw, by which mark and colouring.
    marks: BTreeMap<(String, bool), Update>,
    /// The shapes said since the last frame ended.
    shaping: Vec<Update>,
    /// The shapes the last whole frame said.
    shaped: Vec<Update>,
}

impl Screen {
    /// Takes in one thing said about the screen.
    pub fn saw(&mut self, update: &Update) {
        match update {
            Update::Cell { x, y, cell } => {
                let (x, y) = (usize::from(*x), usize::from(*y));
                if self.cells.len() <= y {
                    self.cells.resize_with(y + 1, Vec::new);
                }
                let row = &mut self.cells[y];
                if row.len() <= x {
                    row.resize_with(x + 1, || None);
                }
                row[x] = Some((**cell).clone());
            }
            Update::Cleared => self.cells.clear(),
            Update::Caret(caret) => self.caret = Some(*caret),
            Update::Frame => self.shaped = std::mem::take(&mut self.shaping),
            Update::Mark { id, focused, .. } => {
                self.marks.insert((id.clone(), *focused), update.clone());
            }
            // Not kept: it says that what is on the screen is in a theme
            // that is going, and a window that comes back sees only the
            // one that came.
            Update::Recoloured => {}
            Update::TextSize(_)
            | Update::Fonts(_)
            | Update::Animates(_)
            | Update::BarTabs(_)
            | Update::PictureStrength(_)
            | Update::Ground(_)
            | Update::Holding { .. }
            | Update::Picture(_)
            | Update::Tabs(_)
            | Update::CaretInk(_)
            | Update::CaretIs { .. } => {
                self.settled.insert(about(update), update.clone());
            }
            Update::Marked { .. }
            | Update::Capped { .. }
            | Update::Barred { .. }
            | Update::Stroked { .. }
            | Update::Ticked { .. }
            | Update::Spun { .. }
            | Update::Ruled { .. }
            | Update::Parted { .. }
            | Update::Paired { .. }
            | Update::Found { .. }
            | Update::Linked { .. }
            | Update::Spelled { .. }
            | Update::Sheened { .. }
            | Update::Scrolled { .. }
            | Update::Behind { .. } => self.shaping.push(update.clone()),
        }
    }

    /// All of it again, as a window with nothing would need to be told it:
    /// what the settings say first, because they decide how the cells are
    /// drawn, then the page from nothing, and the frame's end last.
    #[must_use]
    pub fn again(&self) -> Vec<Update> {
        let mut said: Vec<Update> = self.settled.values().cloned().collect();
        said.extend(self.marks.values().cloned());
        said.push(Update::Cleared);
        for (y, row) in (0_u16..).zip(&self.cells) {
            for (x, cell) in (0_u16..).zip(row) {
                if let Some(cell) = cell {
                    said.push(Update::Cell {
                        x,
                        y,
                        cell: Box::new(cell.clone()),
                    });
                }
            }
        }
        if let Some(caret) = self.caret {
            said.push(Update::Caret(caret));
        }
        said.extend(self.shaped.iter().cloned());
        said.push(Update::Frame);
        said
    }
}

/// Which thing the settings tell a window an update is the last word on.
///
/// In the order they are said again: the caret's shape is said while a
/// frame is laid out, so it goes after what the frame is drawn in.
const fn about(update: &Update) -> u8 {
    match update {
        Update::TextSize(_) => 0,
        Update::Fonts(_) => 1,
        Update::Animates(_) => 2,
        Update::BarTabs(_) => 3,
        Update::PictureStrength(_) => 4,
        Update::Ground(_) => 5,
        Update::Holding { .. } => 6,
        Update::CaretInk(_) => 7,
        Update::Picture(_) => 8,
        Update::Tabs(_) => 9,
        _ => 10,
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{layout::Rect, style::Color};

    use super::*;

    fn cell(symbol: &str) -> Box<Cell> {
        let mut cell = Cell::default();
        cell.set_symbol(symbol);
        Box::new(cell)
    }

    /// What is said again is the screen as it is now: the cells since the
    /// last clear, the last word on each setting, and the shapes of the
    /// last whole frame -- not those of a frame still being laid out, and
    /// not the cells a clear took away.
    ///
    /// Broken deliberately twice. By a clear that keeps the cells
    /// (`Cleared` arm doing nothing): the `x` from before it came back. And
    /// by keeping every shape ever said rather than the last frame's: the
    /// rule from the first frame was drawn again.
    #[test]
    fn what_is_said_again_is_the_screen_as_it_is() {
        let mut screen = Screen::default();
        for update in [
            Update::Cell {
                x: 0,
                y: 0,
                cell: cell("x"),
            },
            Update::Ruled {
                area: Rect::new(0, 1, 4, 1),
            },
            Update::TextSize(13),
            Update::Frame,
            Update::Cleared,
            Update::Cell {
                x: 2,
                y: 0,
                cell: cell("y"),
            },
            Update::TextSize(15),
            Update::Spun {
                area: Rect::new(1, 1, 1, 1),
            },
            Update::Frame,
            Update::Sheened {
                area: Rect::new(3, 3, 1, 1),
                from: Color::Red,
                to: Color::Blue,
            },
        ] {
            screen.saw(&update);
        }
        let again = screen.again();
        let cells: Vec<&str> = again
            .iter()
            .filter_map(|update| match update {
                Update::Cell { cell, .. } => Some(cell.symbol()),
                _ => None,
            })
            .collect();
        assert_eq!(cells, ["y"], "the cells are not the screen's now");
        assert!(again.contains(&Update::TextSize(15)));
        assert!(!again.contains(&Update::TextSize(13)), "an old size");
        assert!(
            again
                .iter()
                .any(|update| matches!(update, Update::Spun { .. }))
        );
        assert!(
            !again
                .iter()
                .any(|update| matches!(update, Update::Ruled { .. })),
            "a shape from a frame that has gone"
        );
        assert!(
            !again
                .iter()
                .any(|update| matches!(update, Update::Sheened { .. })),
            "a shape from a frame not yet ended"
        );
        assert_eq!(again.last(), Some(&Update::Frame));
        let cleared = again.iter().position(|update| *update == Update::Cleared);
        let size = again
            .iter()
            .position(|update| *update == Update::TextSize(15));
        assert!(size < cleared, "the settings are not said before the page");
    }
}
