//! Where the frame drew its bars, for a pointer that takes hold of one.
//!
//! A bar is drawn by one function, [`crate::scrollbar`], so that function is
//! the one that knows where every bar is -- and a press on one is a
//! question about a bar, not about the list or the file beside it. Asking
//! each view to work out again where it put its bar would be a dozen
//! second answers to a question one piece of code has already answered.
//!
//! What the bar is *of* is not something the bar knows: the editor draws
//! the file and a preview, the picker draws three different lists. So the
//! frame says whose each one is around the call that draws it
//! ([`of`]), and the bar keeps that with what it measured.
//!
//! Scratch for one frame on the drawing thread, and nothing else: [`said`]
//! is only listening inside [`collect`], so a view drawn by a test on its
//! own records nothing and needs nothing.

use std::cell::{Cell, RefCell};

use ratatui::{buffer::Buffer as CellBuffer, layout::Rect};

use crate::shapes::Bar;

/// Which thing on screen a bar is beside.
///
/// One for each thing that scrolls, because each is moved its own way:
/// a file by its viewport, a list by its window, a hover by how far it
/// has been read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Whose {
    /// The file being read, or a reading of it.
    Document,
    /// The notes.
    Notes,
    /// A conversation's transcript.
    Conversation,
    /// The page that asks which project.
    Projects,
    /// The list that is open: a picker.
    Picker,
    /// What could finish the path being named.
    Naming,
    /// The agent's own commands, offered under the box.
    Commands,
    /// What the row a list is on names, shown beside it.
    Preview,
    /// The settings, or the agents on their page.
    Settings,
    /// What this project is made of.
    Counts,
    /// The fonts to choose from.
    Names,
    /// What could be typed next.
    Completion,
    /// And what the one it is on says about itself.
    Documentation,
    /// What a server said about the place under the caret.
    Hover,
}

/// A bar, as the frame drew it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drawn {
    /// What it is beside.
    pub whose: Whose,
    /// Where it is, and where on it the mark was drawn.
    pub bar: Bar,
    /// How many rows the thing it is about has.
    total: usize,
    /// Where each item starts, for things whose items are not a row each.
    ///
    /// Empty where they are. A card is as tall as its description needs,
    /// so a bar measured in rows is a bar whose rows are not the window's
    /// items -- and what the window is moved by is the item.
    starts: Vec<usize>,
}

impl Drawn {
    /// Whether the pointer is on it.
    #[must_use]
    pub const fn under(&self, x: u16, y: u16) -> bool {
        let area = self.bar.area;
        x >= area.x && x < area.x + area.width && y >= area.y && y < area.y + area.height
    }

    /// Where on the mark a press at this row took hold of it.
    ///
    /// On the mark, where it was pressed: the mark moves with the pointer
    /// from there. Off it, in the middle, so the mark comes to the pointer
    /// and the same press goes on to drag it -- one gesture, wherever on
    /// the track it began.
    #[must_use]
    pub fn grip(&self, y: u16) -> u16 {
        let row = y.saturating_sub(self.bar.area.y);
        let mark = self.bar.mark;
        match row >= mark && row < mark + self.bar.thumb {
            true => row - mark,
            false => self.bar.thumb / 2,
        }
    }

    /// Which item belongs at the top for the mark to be under the pointer.
    ///
    /// In the window's items, which for most bars are its rows: see
    /// `starts`.
    #[must_use]
    pub fn top_for(&self, y: u16, grip: u16) -> usize {
        // Signed, because a pointer dragged above the track is still
        // dragging, and what it says is "the top".
        let row = i32::from(y) - i32::from(self.bar.area.y) - i32::from(grip);
        let mark = u16::try_from(row.max(0)).unwrap_or(u16::MAX);
        let top = crate::bar_top(self.bar.area.height, mark, self.total);
        if self.starts.is_empty() {
            return top;
        }
        // The item that row is in: the last to start at or before it.
        self.starts
            .partition_point(|start| *start <= top)
            .saturating_sub(1)
    }
}

thread_local! {
    /// The bars drawn so far this frame, while a frame is listening.
    static SAID: RefCell<Option<Vec<Drawn>>> = const { RefCell::new(None) };
    /// Whose bar the view being drawn would draw.
    static WHOSE: Cell<Option<Whose>> = const { Cell::new(None) };
}

/// Draws a frame and hands back the bars it left on the page.
///
/// Left on it, not drawn: a view drawn over inside the one frame has
/// already drawn its bar, and a press there lands on whatever covered it.
/// So a bar counts only where every row of it is still a bar's -- the
/// question the window asks before drawing one (`Barred::still_said`),
/// and the reason it has to be every row: a list's bar can be in the same
/// column as the file's, and what says the file's is gone is the rows
/// outside the list.
pub(crate) fn collect(cells: &mut CellBuffer, draw: impl FnOnce(&mut CellBuffer)) -> Vec<Drawn> {
    SAID.with(|said| *said.borrow_mut() = Some(Vec::new()));
    draw(cells);
    let drawn = SAID
        .with(|said| said.borrow_mut().take())
        .unwrap_or_default();
    drawn
        .into_iter()
        .filter(|drawn| still_a_bar(cells, drawn.bar.area))
        .collect()
}

/// Whether every cell of a column still holds what a bar is drawn with.
fn still_a_bar(cells: &CellBuffer, area: Rect) -> bool {
    let mut bar = [0; 4];
    let bar: &str = crate::BAR.encode_utf8(&mut bar);
    (area.top()..area.bottom()).all(|y| {
        cells
            .cell((area.x, y))
            .is_some_and(|cell| cell.symbol() == bar)
    })
}

/// Draws something whose bar is this one's.
pub(crate) fn of<T>(whose: Whose, draw: impl FnOnce() -> T) -> T {
    let before = WHOSE.with(|now| now.replace(Some(whose)));
    let drawn = draw();
    WHOSE.with(|now| now.set(before));
    drawn
}

/// A bar was drawn, measuring a thing of `total` rows.
pub(crate) fn said(bar: Bar, total: usize) {
    let Some(whose) = WHOSE.with(Cell::get) else {
        return;
    };
    SAID.with(|said| {
        if let Some(said) = said.borrow_mut().as_mut() {
            said.push(Drawn {
                whose,
                bar,
                total,
                starts: Vec::new(),
            });
        }
    });
}

/// And the bar just drawn measures items as tall as these.
pub(crate) fn in_items(heights: &[u16]) {
    SAID.with(|said| {
        if let Some(last) = said.borrow_mut().as_mut().and_then(|said| said.last_mut()) {
            last.starts = heights
                .iter()
                .scan(0, |start, height| {
                    let this = *start;
                    *start += usize::from(*height);
                    Some(this)
                })
                .collect();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(height: u16, mark: u16, thumb: u16, total: usize) -> Drawn {
        Drawn {
            whose: Whose::Document,
            bar: Bar {
                area: Rect {
                    x: 9,
                    y: 2,
                    width: 1,
                    height,
                },
                mark,
                thumb,
            },
            total,
            starts: Vec::new(),
        }
    }

    /// A press on the mark keeps hold of it where it was pressed, and one
    /// off it brings the mark's middle to the pointer.
    ///
    /// Deliberate break: answer `row - mark` whether or not the row is on
    /// the mark. A press below the mark then grips it by a row it does not
    /// have, and the mark lands short of the pointer.
    #[test]
    fn a_press_on_the_mark_keeps_hold_of_it_where_it_was_pressed() {
        let bar = drawn(10, 3, 4, 100);
        assert_eq!(bar.grip(2 + 4), 1, "on the mark, a row in");
        assert_eq!(bar.grip(2 + 8), 2, "below it, by its middle");
        assert_eq!(bar.grip(2), 2, "above it, by its middle");
    }

    /// The mark lands under the pointer, all the way to both ends.
    ///
    /// Deliberate break: scale the mark by the total over the height in
    /// `bar_top`, rather than by how far the top can travel over how far
    /// the mark can. The foot of the track then asks for a top past the
    /// last screenful -- which happens to be the same number when the
    /// total is a multiple of the height, so this one is not.
    #[test]
    fn the_mark_lands_under_the_pointer() {
        let bar = drawn(10, 0, 1, 57);
        for row in 0..=9u16 {
            let top = bar.top_for(2 + row, 0);
            assert_eq!(crate::bar_mark(10, top, 57), row, "row {row}");
        }
        assert_eq!(bar.top_for(2 + 9, 0), 47, "the last screenful");
        assert_eq!(bar.top_for(0, 0), 0, "dragged off the top");
        assert_eq!(bar.top_for(40, 0), 47, "and off the foot");
    }

    /// Where items are taller than a row, the top is the item the row is in.
    ///
    /// Deliberate break: leave `starts` out of `top_for`. A drag to the
    /// foot then asks for the ninetieth item of a list of thirty.
    #[test]
    fn items_taller_than_a_row_are_moved_by_the_item() {
        let mut bar = drawn(10, 0, 1, 90);
        bar.starts = (0..30).map(|item| item * 3).collect();
        assert_eq!(bar.top_for(2 + 9, 0), 26, "the last screenful, in items");
        assert_eq!(bar.top_for(2, 0), 0);
    }
}
