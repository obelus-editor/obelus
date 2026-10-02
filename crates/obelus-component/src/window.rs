//! Where a list is: which row has the focus, and which rows are on screen.
//!
//! Every list in Obelus had its own answer to this and they did not agree.
//! The rule they should all follow is one sentence -- *move the window by
//! the least that puts the focused row back on screen* -- and it had three
//! implementations, so it had to be fixed three times: once for the
//! pickers, once for the agents' cards, and once for the conversation.
//!
//! Two kinds of list, because there are two: one with a focus, which is
//! what a reader is choosing from, and one without, which is a transcript.
//! The difference is which end the window is parked at when nothing has
//! been chosen, and it is four lines rather than a second type.
//!
//! What this does not know is what a row *is*. Rows can be one line each or
//! as tall as a card, they can be filtered out from under it, and choosing
//! one can mean anything; all of that belongs to whoever owns the rows.

/// A key that moves about a list.
///
/// One table for the six of them, because a list added later should get all
/// six rather than the two somebody remembered: the settings went a while
/// with no paging and no ends, and what was missing was not a decision.
///
/// Which modifiers a view accepts is the view's own business -- a
/// conversation's `home` is the row being typed and its `shift+home` is the
/// transcript -- so this maps the key and nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    /// One row back.
    Up,
    /// One row on.
    Down,
    /// A screenful back.
    PageUp,
    /// A screenful on.
    PageDown,
    /// The first row.
    First,
    /// The last.
    Last,
}

impl Move {
    /// Which movement a key means, if it means one.
    #[must_use]
    pub const fn of(code: crossterm::event::KeyCode) -> Option<Self> {
        use crossterm::event::KeyCode;
        match code {
            KeyCode::Up => Some(Self::Up),
            KeyCode::Down => Some(Self::Down),
            KeyCode::PageUp => Some(Self::PageUp),
            KeyCode::PageDown => Some(Self::PageDown),
            KeyCode::Home => Some(Self::First),
            KeyCode::End => Some(Self::Last),
            _ => None,
        }
    }
}

/// Whether stepping off the end of a list comes back at the other one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wrap {
    /// It does. The other end of a list is faster to reach than to scroll
    /// back through, which is what a single step is for.
    Yes,
    /// It stops. Paging is how a reader gets to the end of a long list, and
    /// a page that wrapped past it would overshoot what they were reaching
    /// for -- and a wheel is rolled without looking.
    No,
}

/// A list's place: how many rows, which has the focus, which is on top.
#[derive(Clone, Copy, Debug, Default)]
pub struct Window {
    /// How many rows there are to be in.
    count: usize,
    /// Which one has the focus. Meaningless while `follow` is set: a
    /// transcript is not being chosen from.
    focus: usize,
    /// Which row is drawn first.
    top: usize,
    /// Whether the window follows the end of the list rather than the
    /// focus.
    follow: bool,
    /// Whether the reader has scrolled away from that end.
    ///
    /// Only meaningful with `follow`: it is what keeps a streaming answer
    /// from dragging the view out from under somebody reading it.
    left_the_end: bool,
    /// Whether the reader has dragged the window away from the focus.
    ///
    /// The one way the window leaves the focus behind on a list that is
    /// chosen from: a bar taken hold of moves what is on screen and not
    /// what is chosen. Without this the next frame's settling would put
    /// the window straight back where the focus is, and the drag would
    /// have moved nothing. Anything that moves the focus puts it back.
    left_the_focus: bool,
}

impl Window {
    /// A window on a list that is chosen from.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            count: 0,
            focus: 0,
            top: 0,
            follow: false,
            left_the_end: false,
            left_the_focus: false,
        }
    }

    /// A window on a transcript, which is read from its end.
    #[must_use]
    pub const fn following() -> Self {
        Self {
            count: 0,
            focus: 0,
            top: 0,
            follow: true,
            left_the_end: false,
            left_the_focus: false,
        }
    }

    /// How many rows the list has.
    #[must_use]
    pub const fn count(&self) -> usize {
        self.count
    }

    /// Says how many rows there are, keeping the focus on one that exists.
    pub fn set_count(&mut self, count: usize) {
        self.count = count;
        self.focus = self.focus.min(count.saturating_sub(1));
        self.top = self.top.min(count.saturating_sub(1));
    }

    /// Which row has the focus.
    #[must_use]
    pub const fn focus(&self) -> usize {
        self.focus
    }

    /// Puts the focus on a row.
    ///
    /// A choice, so a window dragged away from the focus comes back to it
    /// -- even to the row it was on, which is what a list that starts over
    /// at the top means. A caller that says the same focus again once a
    /// frame has to ask whether it moved first, or no drag lasts a frame.
    pub fn set_focus(&mut self, at: usize) {
        self.focus = at.min(self.count.saturating_sub(1));
        self.left_the_focus = false;
    }

    /// Brings a window dragged away from the focus back to it, without
    /// choosing anything: for what the reader typed, which changes what
    /// the rows are and leaves the choice where it was.
    pub const fn back_to_the_focus(&mut self) {
        self.left_the_focus = false;
    }

    /// Which row is drawn first.
    #[must_use]
    pub const fn top(&self) -> usize {
        self.top
    }

    /// Moves the focus by rows, and says where it landed.
    pub fn step(&mut self, by: isize, wrap: Wrap) -> usize {
        self.left_the_focus = false;
        if self.count == 0 {
            self.focus = 0;
            return 0;
        }
        let last = self.count - 1;
        self.focus = match wrap {
            Wrap::Yes if by > 0 && self.focus >= last => 0,
            Wrap::Yes if by < 0 && self.focus == 0 => last,
            _ => self.focus.saturating_add_signed(by).min(last),
        };
        self.focus
    }

    /// Moves the focus by whole screens, stopping at the ends.
    pub fn page(&mut self, pages: isize, height: u16) -> usize {
        let rows = isize::try_from(height.max(1)).unwrap_or(1);
        self.step(pages.saturating_mul(rows), Wrap::No)
    }

    /// To the first row.
    pub fn home(&mut self) {
        self.focus = 0;
        self.top = 0;
        self.left_the_end = true;
        self.left_the_focus = false;
    }

    /// To the last.
    pub fn end(&mut self) {
        self.focus = self.count.saturating_sub(1);
        self.left_the_end = false;
        self.left_the_focus = false;
    }

    /// Moves the focus the way a key said to.
    ///
    /// `wrap` is the policy for a single step: a list of things to choose
    /// from wraps, because the other end is faster to reach than to scroll
    /// back through. Paging never wraps, whatever this says.
    pub fn apply(&mut self, movement: Move, height: u16, wrap: Wrap) {
        match movement {
            Move::Up => {
                self.step(-1, wrap);
            }
            Move::Down => {
                self.step(1, wrap);
            }
            Move::PageUp => {
                self.page(-1, height);
            }
            Move::PageDown => {
                self.page(1, height);
            }
            Move::First => self.home(),
            Move::Last => self.end(),
        }
    }

    /// Whether a following window is at the end it follows.
    ///
    /// Which is the ordinary state of a transcript: it sits at the end
    /// until the reader scrolls up. Worth asking because a key that means
    /// "further down" has somewhere else to go once there is no further
    /// down -- and because the answer is only true after a frame has
    /// settled, which is where [`Window::settle`] works it out.
    #[must_use]
    pub const fn at_the_end(&self) -> bool {
        self.follow && !self.left_the_end
    }

    /// Scrolls the window without moving the focus, for the wheel and for a
    /// transcript's arrows.
    pub fn scroll(&mut self, rows: isize) {
        if rows < 0 {
            self.top = self.top.saturating_sub(usize::try_from(-rows).unwrap_or(1));
            // Leaving the end is the whole of what this says; coming back
            // to it is noticed by `settle`, which is the only thing that
            // knows where the end is.
            self.left_the_end = true;
        } else {
            self.top = self.top.saturating_add(usize::try_from(rows).unwrap_or(1));
        }
    }

    /// Puts this row at the top, for a bar the reader has hold of.
    ///
    /// The window and not the focus, on a list that is chosen from as
    /// well as on a transcript: what is under the pointer is a picture of
    /// where the window is, so it is the window that goes where it is
    /// dragged. The focus stays chosen until a key moves it, and then the
    /// window goes back to it -- the way the wheel leaves the caret in a
    /// file. How far is `settle`'s, which knows how many rows fit.
    pub fn drag_to(&mut self, top: usize) {
        self.top = top;
        match self.follow {
            // Back at the end is noticed by `settle`, as it is for the
            // wheel.
            true => self.left_the_end = true,
            false => self.left_the_focus = true,
        }
    }

    /// Moves the window if it has to, and no further.
    ///
    /// Once a frame, with the rows the list really has: where the window
    /// belongs depends on the geometry, and the geometry is only settled at
    /// that point -- which is also what makes a resize move the window
    /// rather than leave the focus off the screen.
    pub fn settle(&mut self, height: u16) {
        let height = usize::from(height).max(1);
        let last = self.count.saturating_sub(height);
        if self.follow {
            if self.left_the_end {
                self.top = self.top.min(last);
                // Scrolled back down to the end, so it follows again.
                self.left_the_end = self.top < last;
            } else {
                self.top = last;
            }
            return;
        }
        // Never parked past the end: rows going away under the window --
        // a query narrowing the list -- would otherwise leave it showing
        // the last row and a screen of nothing.
        self.top = self.top.min(last);
        if self.left_the_focus {
            return;
        }
        if self.focus < self.top {
            self.top = self.focus;
        } else if self.focus >= self.top + height {
            self.top = self.focus + 1 - height;
        }
    }

    /// The same, for rows that are not all one row tall.
    ///
    /// Counted in rows rather than in items, because a card is as tall as
    /// its description needs: the window walks forward until the focused
    /// item's last row is on screen. An item taller than the whole page
    /// stops with itself at the top, which is the most of it that can be
    /// shown.
    pub fn settle_by_height(&mut self, heights: &[u16], room: u16) {
        self.set_count(heights.len());
        if heights.is_empty() {
            self.top = 0;
            return;
        }
        let room = room.max(1);
        // Never parked past the end, which is `settle`'s rule in rows: a
        // query narrowing the list would otherwise leave its last few items
        // at the top and a screen of nothing under them.
        let mut filled = heights.len() - 1;
        let mut taken = heights[filled];
        while filled > 0 && taken.saturating_add(heights[filled - 1]) <= room {
            filled -= 1;
            taken = taken.saturating_add(heights[filled]);
        }
        if self.left_the_focus {
            self.top = self.top.min(filled);
            return;
        }
        self.top = self.top.min(self.focus).min(filled);
        while self.top < self.focus {
            let taken: u16 = heights[self.top..=self.focus].iter().sum();
            if taken <= room {
                break;
            }
            self.top += 1;
        }
    }

    /// Which items are on screen, for items that are not all one row tall.
    ///
    /// Whole items only: the first one always, and after it as many as fit.
    /// Half an item at the foot is a row that says less than it seems to,
    /// and the next step brings the whole of it on anyway.
    #[must_use]
    pub fn visible_by_height(&self, heights: &[u16], room: u16) -> std::ops::Range<usize> {
        let first = self.top.min(heights.len());
        let mut taken = 0u16;
        let mut last = first;
        for height in &heights[first..] {
            taken = taken.saturating_add(*height);
            if taken > room && last > first {
                break;
            }
            last += 1;
        }
        first..last
    }

    /// Which rows are on screen.
    #[must_use]
    pub fn visible(&self, height: u16) -> std::ops::Range<usize> {
        let height = usize::from(height);
        let first = self.top.min(self.count);
        first..(first + height).min(self.count)
    }

    /// Whether there is more list than there is screen.
    ///
    /// What decides whether a scrollbar is drawn: a bar on a list that fits
    /// is a bar that says nothing, and every list in Obelus follows the
    /// same rule about that.
    #[must_use]
    pub fn scrollable(&self, height: u16) -> bool {
        self.count > usize::from(height)
    }
}

#[cfg(test)]
mod tests {
    use super::{Move, Window, Wrap};

    /// The rule every list follows: the window moves by the least that puts
    /// the focus back on screen, and a step that is not at an edge moves it
    /// not at all.
    #[test]
    fn the_window_moves_only_when_the_focus_leaves_it() {
        let mut window = Window::new();
        window.set_count(20);

        // Down through the ten rows on screen: nothing moves.
        for expected in 1..10 {
            window.step(1, Wrap::No);
            window.settle(10);
            assert_eq!(window.focus(), expected);
            assert_eq!(window.top(), 0, "it scrolled before the last row");
        }
        // One more, and it moves by one row.
        window.step(1, Wrap::No);
        window.settle(10);
        assert_eq!(window.top(), 1);

        // Back up, and it stays where it is until the focus reaches the top
        // of the window. This is the one that was got wrong three times.
        for _ in 0..9 {
            window.step(-1, Wrap::No);
            window.settle(10);
            assert_eq!(window.top(), 1, "going up scrolled from the middle");
        }
        window.step(-1, Wrap::No);
        window.settle(10);
        assert_eq!(window.top(), 0);
    }

    /// A bar dragged moves the window off the focus, and it stays there
    /// until a key moves the focus -- which brings the window back to it.
    ///
    /// Deliberate break: leave `settle` chasing the focus whatever
    /// `left_the_focus` says. The drag is then undone by the next frame and
    /// the window is back at the top.
    #[test]
    fn a_dragged_window_leaves_the_focus_until_a_key_moves_it() {
        let mut window = Window::new();
        window.set_count(50);
        window.settle(10);

        window.drag_to(30);
        window.settle(10);
        assert_eq!(window.top(), 30, "the drag was undone by settling");
        assert_eq!(window.focus(), 0, "dragging the bar chose something");

        // Never past the last screenful, dragged or not.
        window.drag_to(45);
        window.settle(10);
        assert_eq!(window.top(), 40);

        window.step(1, Wrap::No);
        window.settle(10);
        assert_eq!(window.top(), 1, "a key did not bring the focus back");
        // And choosing the row it is already on is a choice too -- a list
        // starting over at the top -- which brings it back as well.
        //
        // Deliberate break: clear `left_the_focus` in `set_focus` only
        // where the focus moves. This stays at the foot of the list.
        window.drag_to(30);
        window.settle(10);
        window.set_focus(1);
        window.settle(10);
        assert_eq!(window.top(), 1, "choosing the same row left it dragged");
    }

    /// The six keys, from one table.
    #[test]
    fn the_keys_that_move_a_list_are_one_table() {
        use crossterm::event::KeyCode;

        assert_eq!(Move::of(KeyCode::Up), Some(Move::Up));
        assert_eq!(Move::of(KeyCode::Down), Some(Move::Down));
        assert_eq!(Move::of(KeyCode::PageUp), Some(Move::PageUp));
        assert_eq!(Move::of(KeyCode::PageDown), Some(Move::PageDown));
        assert_eq!(Move::of(KeyCode::Home), Some(Move::First));
        assert_eq!(Move::of(KeyCode::End), Some(Move::Last));
        assert_eq!(Move::of(KeyCode::Enter), None);

        let mut window = Window::new();
        window.set_count(100);
        window.apply(Move::Last, 10, Wrap::Yes);
        assert_eq!(window.focus(), 99);
        window.apply(Move::Down, 10, Wrap::Yes);
        assert_eq!(window.focus(), 0, "the end did not wrap");
        window.apply(Move::PageDown, 10, Wrap::No);
        assert_eq!(window.focus(), 10);
        window.apply(Move::PageUp, 10, Wrap::No);
        assert_eq!(window.focus(), 0);
        window.apply(Move::Up, 10, Wrap::No);
        assert_eq!(
            window.focus(),
            0,
            "a step that does not wrap ran off the end"
        );
        window.apply(Move::First, 10, Wrap::Yes);
        assert_eq!(window.focus(), 0);
    }

    #[test]
    fn a_single_step_wraps_and_a_page_does_not() {
        let mut window = Window::new();
        window.set_count(5);
        window.set_focus(4);
        assert_eq!(window.step(1, Wrap::Yes), 0, "the end did not wrap");
        assert_eq!(window.step(-1, Wrap::Yes), 4, "the start did not wrap");

        window.set_focus(4);
        assert_eq!(window.page(1, 10), 4, "a page wrapped past the end");
        window.set_focus(0);
        assert_eq!(window.page(-1, 10), 0, "a page wrapped past the start");
    }

    /// Rows going away under the window leaves the focus on a row that
    /// exists and the window on the focus.
    #[test]
    fn a_list_that_shrinks_pulls_the_window_back() {
        let mut window = Window::new();
        window.set_count(50);
        window.set_focus(49);
        window.settle(10);
        assert_eq!(window.top(), 40);

        window.set_count(3);
        window.settle(10);
        assert_eq!(window.focus(), 2);
        assert_eq!(window.top(), 0, "the window was left past the end");
        assert!(!window.scrollable(10));
    }

    /// A card is as tall as its description needs, so the window walks rows
    /// rather than items.
    #[test]
    fn variable_heights_are_counted_in_rows() {
        let heights = vec![4u16; 12];
        let mut window = Window::new();
        // Thirteen rows of room is three whole cards.
        window.settle_by_height(&heights, 13);
        assert_eq!(window.top(), 0);

        window.set_focus(2);
        window.settle_by_height(&heights, 13);
        assert_eq!(window.top(), 0, "the third card did not fit after all");

        window.set_focus(3);
        window.settle_by_height(&heights, 13);
        assert_eq!(window.top(), 1, "it did not scroll by one card");

        // And back up: the window stays until the focus reaches its top.
        window.set_focus(2);
        window.settle_by_height(&heights, 13);
        assert_eq!(window.top(), 1);
        window.set_focus(1);
        window.settle_by_height(&heights, 13);
        assert_eq!(window.top(), 1);
        window.set_focus(0);
        window.settle_by_height(&heights, 13);
        assert_eq!(window.top(), 0);
    }

    /// Items of rows that differ going away under the window pull it back,
    /// the way rows of one row each do: a query that leaves three items
    /// shows all three, not the last of them and a screen of nothing.
    ///
    /// Broken by taking the `.min(filled)` off `settle_by_height`: the
    /// window stayed on the last item.
    #[test]
    fn items_of_rows_that_differ_going_away_pull_the_window_back() {
        let mut window = Window::new();
        window.set_count(20);
        window.set_focus(19);
        window.settle_by_height(&[3u16; 20], 10);
        assert_eq!(window.top(), 17, "the focused item is not on screen");

        window.set_count(3);
        window.set_focus(2);
        window.settle_by_height(&[3u16; 3], 10);
        assert_eq!(window.top(), 0, "the window was left past the end");
        assert_eq!(window.visible_by_height(&[3u16; 3], 10), 0..3);
    }

    /// Whole items only, and the first always, however tall it is.
    #[test]
    fn what_is_on_screen_is_the_items_that_fit_whole() {
        let window = Window::new();
        assert_eq!(window.visible_by_height(&[4, 4, 4], 10), 0..2);
        assert_eq!(
            window.visible_by_height(&[12, 1], 10),
            0..1,
            "an item taller than the room was not shown"
        );
        assert_eq!(window.visible_by_height(&[], 10), 0..0);
    }

    /// A transcript is read from its end, and stays where the reader put it
    /// once they have scrolled away from that end.
    #[test]
    fn a_following_window_keeps_the_readers_place() {
        let mut window = Window::following();
        window.set_count(40);
        window.settle(10);
        assert_eq!(window.top(), 30, "it did not start at the end");

        window.scroll(-1);
        window.settle(10);
        assert_eq!(window.top(), 29);

        // Something new arrives, and the view stays put.
        window.set_count(45);
        window.settle(10);
        assert_eq!(window.top(), 29, "it dragged the view to the end");

        // Back down to the end, and it follows again.
        window.scroll(10);
        window.settle(10);
        assert_eq!(window.top(), 35);
        window.set_count(50);
        window.settle(10);
        assert_eq!(window.top(), 40, "it stopped following the end");
    }

    #[test]
    fn an_empty_list_has_no_focus_and_no_rows_on_screen() {
        let mut window = Window::new();
        window.set_count(0);
        window.settle(10);
        assert_eq!(window.focus(), 0);
        assert_eq!(window.visible(10), 0..0);
        assert!(!window.scrollable(10));
        assert_eq!(window.step(1, Wrap::Yes), 0);
    }

    #[test]
    fn what_is_on_screen_is_the_window() {
        let mut window = Window::new();
        window.set_count(100);
        window.set_focus(50);
        window.settle(10);
        assert_eq!(window.visible(10), 41..51);
        assert!(window.scrollable(10));
    }
}
