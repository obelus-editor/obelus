//! The cells, on their way from Obelus's loop to the window.
//!
//! Obelus draws into a grid of cells and always has. What a terminal does
//! with that grid is write escape sequences down a pipe; what the window
//! does is put it on a texture. Neither is the application's business, which
//! is why the whole of the difference is a `ratatui::backend::Backend` --
//! this one -- and why nothing above it had to change.
//!
//! The grid itself lives in the window, not here. `ratatui`'s `Terminal`
//! hands a backend only the cells that *differ* from the last frame, so what
//! crosses the channel is a keystroke's worth of change rather than a
//! screenful: the window keeps the cells and applies what arrives. A frame
//! that really did change everything -- a resize, a new theme -- crosses as
//! everything, which is the cost being paid where it is incurred.
//!
//! **A full-width character takes the cells it covers with it, because the
//! diff will not.** It is two columns of the grid and one glyph, and a
//! terminal advanced the two columns itself -- so it has to be told twice
//! here. The second column is a cell `ratatui` has *reset*, and a window
//! draws every cell, so `Look::columns` says how many a cell spans or the
//! default background is painted behind the right half of every Chinese
//! character. And `ratatui`'s diff leaves that second cell out, so what
//! the window still has in it is whatever was there before: a page of
//! Chinese over a page of code showed one Latin letter sitting on the right
//! half of every other character, in the colours it had had. Which is what
//! said where it came from: a glyph carrying a *style* is not a font going
//! wrong, it is a cell nobody repainted. So `Page::covered` blanks the
//! neighbours, with the character's own style, which is what `ratatui`'s
//! own buffer holds there -- the page saying to itself what the diff did
//! not.

use std::sync::{Arc, mpsc::Sender};

use obelus_app::app::Caret;
use obelus_component::layers::Layer;
use obelus_front::Closed;
pub(crate) use obelus_front::{Cells, Marking, Measured};
use obelus_ui::shapes::{About, Bar, Joined, Pair, Side, Stroke};
use ratatui::{
    buffer::Cell,
    layout::{Position, Rect},
    style::Color,
};

/// A picture under the page as the window holds it: which asking it
/// answers, and the pixels or that there are none.
///
/// Only one newer than the last drawn is drawn, so two pictures decoded in
/// the wrong order cannot put the older one back.
pub(crate) type Picture = (u64, Option<Arc<crate::wallpaper::Hung>>);

/// One change to what is on the screen, as the window holds it: what Obelus
/// said, with its picture decoded.
pub(crate) type Update = obelus_front::Update<Picture>;

/// What crosses to the window's thread at once: one update, or every cell a
/// frame changed -- a send for each of those was thousands of them for a
/// page scrolled.
pub(crate) type Sent = Vec<Update>;

/// The window's end of what Obelus says: the window's own channel, and the
/// decoder that turns the bytes of a picture into one.
pub(crate) struct Here {
    frames: Sender<Sent>,
    wake: Arc<dyn Fn() + Send + Sync>,
    decoder: Arc<crate::wallpaper::Decoder>,
}

impl Here {
    /// What says to the window down `frames`, and wakes it with `wake`,
    /// for a window that opened on `screen` -- as large as a picture fitted
    /// to it is ever drawn.
    pub(crate) fn new(
        frames: Sender<Sent>,
        wake: Arc<dyn Fn() + Send + Sync>,
        screen: [u32; 2],
    ) -> Self {
        let decoder = {
            let (frames, wake) = (frames.clone(), Arc::clone(&wake));
            crate::wallpaper::Decoder::new(screen, move |number, hung| {
                if frames.send(vec![Update::Picture((number, hung))]).is_ok() {
                    wake();
                }
            })
        };
        Self {
            frames,
            wake,
            decoder,
        }
    }
}

impl obelus_front::Showing for Here {
    fn show(&self, update: obelus_front::Update) -> Result<(), Closed> {
        self.show_all(vec![update])
    }

    fn show_all(&self, updates: Vec<obelus_front::Update>) -> Result<(), Closed> {
        let mut sent = Vec::with_capacity(updates.len());
        for update in updates {
            match update.apart() {
                Ok(update) => sent.push(update),
                // The page lets go of its picture at once where it is not
                // the one wanted -- see `Decoder::ask` -- and that none is
                // numbered like any picture, so one still decoding cannot
                // arrive after it.
                Err(hanging) => {
                    if let Some(number) = self.decoder.ask(hanging) {
                        sent.push(Update::Picture((number, None)));
                    }
                }
            }
        }
        if sent.is_empty() {
            return Ok(());
        }
        self.frames.send(sent).map_err(|_| Closed)
    }

    fn wake(&self) {
        (self.wake)();
    }
}

/// What a view said about the frame beyond the cells in it.
///
/// One thing rather than three parameters: they arrive together, they are
/// swapped together when the frame ends, and the painter reads them in one
/// pass -- see `obelus_ui::shapes`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Said<'a> {
    /// Where the marks go.
    pub(crate) marked: &'a [Marked],
    /// Which runs of cells are keys in caps.
    pub(crate) capped: &'a [Capped],
    /// And which cells are switches.
    pub(crate) ticked: &'a [Ticked],
    /// And which cells are the mark that turns.
    pub(crate) spun: &'a [Spun],
    /// And which columns are bars, with how the window is showing each.
    pub(crate) barred: &'a [Barred],
    /// And which rows are lines between two things.
    pub(crate) ruled: &'a [Ruled],
    /// And which cells are what an input method is still spelling, with
    /// how many panes and boxes had been put over the page when each was
    /// said -- see `Page::layer`.
    pub(crate) spelled: &'a [(Rect, usize)],
    /// And the mark the light runs across, where one is showing.
    pub(crate) sheened: Option<&'a Sheened>,
    /// And which rows begin something new, with nowhere to say so but the
    /// pixel between two rows.
    pub(crate) parted: &'a [Parted],
    /// And which runs of a one-cell column are marked.
    pub(crate) stroked: &'a [Stroked],
    /// And which cells are the cursor's pair of brackets.
    pub(crate) paired: &'a [Paired],
    /// And the pairs that were, where their light is still going out, each
    /// with how bright it still is.
    pub(crate) fading: &'a [(Paired, f32)],
    /// And which runs of cells are what a search found.
    pub(crate) found: &'a [Finding],
    /// And the plates a pane has been put over -- the halves of a pair and
    /// the runs a search found that the page no longer shows, because a
    /// pane is over them -- for the picture of what that pane was put
    /// over, which is the page with its plates on it. See `pane_under`.
    pub(crate) plated_behind: (&'a [Paired], &'a [Finding]),
    /// And what is under each pane and each box, furthest first: each is
    /// glass over everything before it.
    pub(crate) stack: &'a [Behind],
    /// And the bands that are catching up with where their lists have
    /// got to.
    ///
    /// As many as the screen has: a hover over a file and a list beside a
    /// preview are two bands, and which of them moved is not which of
    /// them was mentioned last.
    pub(crate) bands: &'a [Rolled<'a>],
}

/// A band of rows on its way to where its list has got to.
///
/// Four facts that mean nothing apart: which rows, the page they were on
/// before the move -- the only place the rows that have scrolled off
/// still exist -- how far behind it is being drawn, and how far that page
/// is from there.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Rolled<'a> {
    /// The rows the list is in.
    pub(crate) room: Rect,
    /// Whether the pane is over it rather than the list being the pane's
    /// own.
    pub(crate) under: bool,
    /// The page as it was before the slide began.
    pub(crate) before: &'a Page,
    /// How many rows behind where it has got to it is drawn.
    pub(crate) behind: f32,
    /// And how far the kept page is from there.
    pub(crate) since: f32,
    /// The bar beside it: where it is, and how many rows its mark still
    /// has to come. Rows the bar itself was drawn at, both ends, so there
    /// is nothing between them to round differently.
    pub(crate) bar: Option<(Rect, f32)>,
}

/// A pane that has gone from the page, and the way it goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Going {
    /// The cells it stood on.
    pub(crate) area: Rect,
    /// The edge it was joined to, which it goes back into: a pane leaves
    /// the way it came.
    pub(crate) joined: Joined,
}

/// What is behind the pane on the frame being drawn.
#[derive(Clone, Debug)]
pub(crate) struct Behind {
    /// Which cells it is.
    pub(crate) area: Rect,
    /// Which edge it is joined to.
    pub(crate) joined: Joined,
    /// The pane's own colour.
    pub(crate) ground: Color,
    /// Row-major.
    pub(crate) cells: Vec<Cell>,
    /// How many panes and boxes were put over the page before it, which
    /// is where it is among them -- see `Page::layer`.
    pub(crate) depth: usize,
}

impl Behind {
    /// Whether this is a box put over the page rather than a pane joined
    /// to it.
    pub(crate) fn is_a_box(&self) -> bool {
        self.joined == Joined::Nowhere
    }

    /// What was at this place, as the painter reads a cell.
    pub(crate) fn look(&self, x: u16, y: u16) -> Option<Look<'_>> {
        self.at(x, y).map(|cell| Look {
            text: cell.symbol(),
            foreground: cell.fg,
            background: cell.bg,
            modifier: cell.modifier,
            underline: cell.underline_color,
        })
    }

    /// What was at this place, or nothing where it is outside the region.
    fn at(&self, x: u16, y: u16) -> Option<&Cell> {
        if x < self.area.x || y < self.area.y {
            return None;
        }
        let (along, down) = (x - self.area.x, y - self.area.y);
        if along >= self.area.width || down >= self.area.height {
            return None;
        }
        self.cells
            .get(usize::from(down) * usize::from(self.area.width) + usize::from(along))
    }

    /// Whether this cell is still this one's: nothing said after it has
    /// been put over the cell -- see `Page::layer`.
    fn owns(&self, page: &Page, x: u16, y: u16) -> bool {
        page.layer(x, y) <= self.depth + 1
    }

    /// Whether a box's frame is still there to be drawn: its four corners
    /// still hold the corners the view put in them.
    ///
    /// The corners and not the whole ring, because the ring is asked
    /// again cell by cell -- see [`Behind::ring_holds`] -- and a list
    /// drawn over one side of a hover leaves a frame that is still a
    /// frame everywhere the list is not.
    pub(crate) fn framed(&self, page: &Page) -> bool {
        let area = self.area;
        if self.joined != Joined::Nowhere || area.width < 2 || area.height < 2 {
            return false;
        }
        let (right, bottom) = (area.right() - 1, area.bottom() - 1);
        [
            (area.x, area.y, "\u{256d}"),
            (right, area.y, "\u{256e}"),
            (area.x, bottom, "\u{2570}"),
            (right, bottom, "\u{256f}"),
        ]
        .into_iter()
        .all(|(x, y, corner)| page.look(x, y).text == corner && self.owns(page, x, y))
    }

    /// Whether this cell is part of a box's frame, and still holds it.
    ///
    /// A cell of the ring holding anything but a line is somebody else's
    /// by now, and is drawn as its own cell.
    pub(crate) fn ring_holds(&self, page: &Page, x: u16, y: u16) -> bool {
        let area = self.area;
        let inside = x >= area.x && x < area.right() && y >= area.y && y < area.bottom();
        let ring = x == area.x || x + 1 == area.right() || y == area.y || y + 1 == area.bottom();
        inside && ring && a_line(page.look(x, y).text) && self.owns(page, x, y)
    }

    /// The cells of the ring with a letter under them, which a window puts
    /// back outside the line -- see `Painter::frame`.
    ///
    /// A letter and not a line. What is under the ring may be a rule, or
    /// the frame of another box, and a window draws those as lines of its
    /// own and never from the font: put back here, the glyph was a stub of
    /// a second line beside the first, at the font's height and thickness.
    pub(crate) fn lettered_ring(&self, page: &Page) -> Vec<(u16, u16, Look<'_>)> {
        let area = self.area;
        if area.width == 0 || area.height == 0 {
            return Vec::new();
        }
        let (right, bottom) = (area.right() - 1, area.bottom() - 1);
        // The top and the bottom rows whole, and the two sides between
        // them: a box's ring, once each. A box one row tall is all top.
        let across = [area.y, bottom]
            .into_iter()
            .take(if bottom > area.y { 2 } else { 1 })
            .flat_map(|y| (area.x..=right).map(move |x| (x, y)));
        let down = (area.y + 1..bottom).flat_map(|y| {
            [area.x, right]
                .into_iter()
                .take(if right > area.x { 2 } else { 1 })
                .map(move |x| (x, y))
        });
        across
            .chain(down)
            .filter(|&(x, y)| self.ring_holds(page, x, y))
            .filter_map(|(x, y)| {
                self.look(x, y)
                    .filter(|under| !under.text.trim().is_empty() && !a_line(under.text))
                    .map(|under| (x, y, under))
            })
            .collect()
    }

    /// The cells past the box's right-hand edge that a full-width letter
    /// put back on that edge runs into.
    ///
    /// They are the character's own, and the page holds them blank in its
    /// colours -- see `Page::covered` -- so `backgrounds` would paint them
    /// after `frame` had drawn the letter, over its right half. Left to
    /// `frame` instead, which paints their ground before the letter the
    /// way it does the ring's. Only while they are still blank: one with
    /// anything in it is somebody else's by now.
    pub(crate) fn spilt(&self, page: &Page) -> Vec<(u16, u16)> {
        let past = self.area.right();
        self.lettered_ring(page)
            .into_iter()
            .flat_map(|(x, y, under)| {
                (past.max(x + 1)..x.saturating_add(under.columns()))
                    .filter(move |&x| x < page.columns())
                    .map(move |x| (x, y))
            })
            .filter(|&(x, y)| page.look(x, y).text.trim().is_empty())
            .collect()
    }
}

/// Whether a cell holds one of the box-drawing lines, which is what a
/// frame and a rule are spelled in.
fn a_line(text: &str) -> bool {
    text.chars()
        .next()
        .is_some_and(|glyph| ('\u{2500}'..='\u{257f}').contains(&glyph))
}

/// Where a switch is in the frame being drawn, and how it stands.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Ticked {
    /// Which cell it is.
    pub(crate) area: Rect,
    /// Whether it is set.
    pub(crate) on: bool,
    /// How many panes and boxes had been put over the page when it was
    /// said -- see `Page::layer`.
    pub(crate) depth: usize,
}

impl Ticked {
    /// Whether the page still says what this switch was said about.
    ///
    /// The same question `Capped::still_said` asks: the settings are drawn
    /// and then the list of a setting's choices is drawn over them, and a
    /// switch further down the page had already been said -- so a box sat
    /// on the list, on a row with nothing to switch, and the letter it
    /// was over was missing as well, because `letters` leaves a switch's
    /// cell to `ticks`.
    ///
    /// And whose the cell is, because a list over the page can hold the
    /// same glyph in the same cell -- the notes and the conversations put
    /// a box in front of a row -- and then the glyph cannot say whose.
    pub(crate) fn still_said(&self, page: &Page) -> bool {
        page.layer(self.area.x, self.area.y) <= self.depth
            && page
                .look(self.area.x, self.area.y)
                .text
                .chars()
                .eq(std::iter::once(obelus_ui::tick(self.on)))
    }
}

/// Where half of the cursor's pair of brackets is in the frame being drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Paired {
    /// Which cell it is.
    pub(crate) area: Rect,
    /// Which pair it is half of.
    pub(crate) pair: Pair,
    /// How many panes and boxes had been put over the page when it was
    /// said -- see `Page::layer`.
    pub(crate) depth: usize,
    /// Which bracket the cell held when the frame that said it arrived,
    /// and the ground it wore, or `None` before then.
    ///
    /// What lets a pair that has gone go on being drawn while its light
    /// goes out: nothing says it any more, so the one thing that can say
    /// the cell is still that bracket is the bracket -- see
    /// [`Self::still_there`] -- and the cell has the page's ground back,
    /// so what the light was made of has to be kept as well.
    pub(crate) held: Option<(char, Color)>,
}

impl Paired {
    /// Whether the page still says what this was said about.
    ///
    /// A bracket in the cell, and nothing put over it since. The same
    /// question a cap asks: a list opened over the file is drawn after
    /// the file in the same frame, and a light left where the file's
    /// bracket was would be shining on a row of the list.
    pub(crate) fn still_said(&self, page: &Page) -> bool {
        self.bracket(page).is_some()
    }

    /// Whether a pair that has gone is still where its light is going
    /// out: the same bracket in the same cell, with nothing over it.
    ///
    /// The page moves on under it. A key that took the caret out of the
    /// pair may have scrolled the file as well, and then the cell holds
    /// something else -- or another bracket, which a light going out on
    /// would be a light on a bracket nobody was in.
    pub(crate) fn still_there(&self, page: &Page) -> bool {
        self.held
            .is_some_and(|(held, _)| self.bracket(page) == Some(held))
    }

    /// Whether this half is in the picture of what a pane was put over:
    /// said before the pane, under it, and a bracket in the cell the pane
    /// was put over -- `Finding::still_behind`, for a pair.
    pub(crate) fn still_behind(&self, behind: &Behind) -> bool {
        self.depth <= behind.depth
            && behind.area.intersection(self.area) == self.area
            && behind
                .look(self.area.x, self.area.y)
                .is_some_and(|look| a_bracket(look.text).is_some())
    }

    /// Writes down what the cell holds, as the frame that said it arrives.
    pub(crate) fn hold(&mut self, page: &Page) {
        self.held = self
            .bracket(page)
            .map(|bracket| (bracket, page.look(self.area.x, self.area.y).background));
    }

    /// The bracket in the cell, where it is one and nothing is over it.
    fn bracket(&self, page: &Page) -> Option<char> {
        let (x, y) = (self.area.x, self.area.y);
        a_bracket(page.look(x, y).text).filter(|_| page.layer(x, y) <= self.depth)
    }
}

/// The bracket a cell holds, where what it holds is one and nothing else --
/// the one judgement of whether a cell is half a pair, for the page and for
/// what a pane was put over alike.
fn a_bracket(text: &str) -> Option<char> {
    let mut text = text.chars();
    let bracket = text
        .next()
        .filter(|&c| obelus_syntax::brackets::opens(c) || obelus_syntax::brackets::closes(c))?;
    text.next().is_none().then_some(bracket)
}

/// Where what a search found is in the frame being drawn: one run of cells
/// on one row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Finding {
    /// Which cells.
    pub(crate) area: Rect,
    /// Which match they are.
    pub(crate) found: obelus_ui::shapes::Found,
    /// How many panes and boxes had been put over the page when it was
    /// said -- see `Page::layer`.
    pub(crate) depth: usize,
}

impl Finding {
    /// Whether the page still says what this was said about: nothing put
    /// over any of its cells since. The question a pair asks, for its
    /// reason -- a list opened over the page is drawn after the page in
    /// the same frame, and a plate left where the match was would be
    /// lying on a row of the list.
    pub(crate) fn still_said(&self, page: &Page) -> bool {
        (self.area.left()..self.area.right()).all(|x| page.layer(x, self.area.y) <= self.depth)
    }

    /// Whether this is in the picture of what a pane was put over: said
    /// before the pane, and all of it under the pane -- the question a cap
    /// asks (`Capped::still_behind`), for a run the page no longer shows.
    pub(crate) fn still_behind(&self, behind: &Behind) -> bool {
        self.depth <= behind.depth && behind.area.intersection(self.area) == self.area
    }

    /// The runs said cell by cell joined into one each: the cells of one
    /// match, side by side on one row. The editor says a match a cell at
    /// a time, because it draws a cell at a time.
    pub(crate) fn joined(mut runs: Vec<Self>) -> Vec<Self> {
        runs.sort_by_key(|run| (run.found, run.area.y, run.area.x));
        let mut joined: Vec<Self> = Vec::with_capacity(runs.len());
        for run in runs {
            match joined.last_mut() {
                Some(last)
                    if last.found == run.found
                        && last.area.y == run.area.y
                        && last.area.right() == run.area.x =>
                {
                    last.area.width += run.area.width;
                }
                _ => joined.push(run),
            }
        }
        joined
    }
}

/// Where the mark that turns is in the frame being drawn.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Spun {
    /// Which cell it is.
    pub(crate) area: Rect,
    /// How many panes and boxes had been put over the page when it was
    /// said -- see `Page::layer`.
    pub(crate) depth: usize,
}

impl Spun {
    /// How far round the cell's own frame of the turn is, or `None` where
    /// the cell holds something else by now -- the question
    /// `Ticked::still_said` asks, answered with the one thing a front end
    /// that has stopped its own clock still wants from the cell.
    ///
    /// Whose the cell is as well, for the reason a switch asks: a frame of
    /// the turn is a braille letter, and a file previewed over the turning
    /// mark may have that letter in that cell.
    pub(crate) fn round(&self, page: &Page) -> Option<f32> {
        if page.layer(self.area.x, self.area.y) > self.depth {
            return None;
        }
        obelus_ui::how_far_round(page.look(self.area.x, self.area.y).text)
    }

    /// Its cell in the picture of what a pane was put over, where that
    /// still holds a frame of the turn.
    ///
    /// The other half, as `Capped::still_behind` is: a transcript a list
    /// was opened over is seen through the glass, and asked only of the
    /// page its mark was the braille the window draws instead of.
    ///
    /// Only a mark said before the pane, as everything in the picture was.
    pub(crate) fn behind<'a>(&self, behind: &'a Behind) -> Option<Look<'a>> {
        if self.depth > behind.depth {
            return None;
        }
        behind
            .look(self.area.x, self.area.y)
            .filter(|look| obelus_ui::how_far_round(look.text).is_some())
    }
}

/// A bar in the frame being drawn, and how the window is showing it.
///
/// The bar is the application's and the two numbers are not: what Obelus
/// said is that a bar is here and this much of it is the mark, and how
/// loudly to draw that is the window's own, the same as a caret's blink.
#[derive(Clone, Debug)]
pub(crate) struct Barred {
    /// What was said about it.
    pub(crate) bar: Bar,
    /// How many panes and boxes had been put over the page when it was
    /// said.
    ///
    /// A list opened over a file puts its own bar in the file's column, in
    /// the same block, so the cells cannot say whose a row is -- and the
    /// file's mark was drawn on the list's rows at the file's place, a
    /// second thumb beside the list's that never moved with it. See
    /// `Page::layer`.
    pub(crate) depth: usize,
    /// From nothing at all to all of it: full while it is moving and for
    /// a moment after, and settled back down once the reader has stopped.
    pub(crate) shown: f32,
    /// And how far the pointer's own brightening has come, which is a
    /// second number because the pointer widens it as well: a reader
    /// reaching for a control is about to take hold of it.
    pub(crate) under: f32,
}

impl Barred {
    /// Whether this row of the column is still the bar's mark.
    ///
    /// The mark is the one part of a bar a terminal draws, a block, and
    /// the track is blank: so a cell holding anything but the block is not
    /// the mark's -- the mark is not on this row, or something has been
    /// written over it. Whether something has been put *over* the bar is
    /// the layer's to say, as it is a rule's and a cap's: a list over the
    /// file writes its own bar's block in the same column.
    ///
    /// The glyph was a bar's only answer once, and it is drawn late: a card
    /// goes over the page before the backgrounds, the letters and this, so
    /// a panel as wide as the editor puts its own right-hand edge in the
    /// scrollbar's column -- and the capsule was painted over it. A
    /// terminal never had the bug, because there the panel simply writes
    /// `\u{2502}` into the cell and the bar is gone.
    fn holds(&self, page: &Page, y: u16) -> bool {
        page.look(self.bar.area.x, y).text == "\u{2588}"
            && page.layer(self.bar.area.x, y) <= self.depth
    }

    /// The rows of its mark that are still its own, as runs: where each
    /// starts and how many rows it is.
    ///
    /// A mark cut in half by something drawn over its middle is two
    /// pieces, for the reason a rule covered in the middle is two lines.
    pub(crate) fn runs(&self, page: &Page) -> Vec<(u16, u16)> {
        let mut runs: Vec<(u16, u16)> = Vec::new();
        for y in self.bar.area.top()..self.bar.area.bottom() {
            if !self.holds(page, y) {
                continue;
            }
            match runs.last_mut() {
                Some(run) if run.0 + run.1 == y => run.1 += 1,
                _ => runs.push((y, 1)),
            }
        }
        runs
    }

    /// Whether this cell is the bar's mark and still holds it.
    pub(crate) fn covers(&self, page: &Page, x: u16, y: u16) -> bool {
        x == self.bar.area.x
            && (self.bar.area.top()..self.bar.area.bottom()).contains(&y)
            && self.holds(page, y)
    }
}

/// A row that begins something new, in the frame being drawn.
///
/// A boundary with no row to be on -- see `obelus_ui::shapes::parted`. The
/// one thing a view says that a terminal has no answer to, said because
/// the window cannot work it out: nothing in a cell says which row begins
/// a note.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Parted {
    /// The run the line covers.
    pub(crate) area: Rect,
}

impl Parted {
    /// Whether anything has been put over the row this was said about.
    ///
    /// The same question `Capped::still_said` asks, and it cannot be asked
    /// of the cells here: a line between two rows has no cell of its own
    /// to still hold anything. What it is asked of instead is what was put
    /// over the page -- a pane, or a box with a frame round it -- because
    /// a `Parted` is said by a whole-screen view and those are the two
    /// things that cover one.
    pub(crate) fn still_said(&self, over: &[&Behind]) -> bool {
        !over.iter().any(|behind| {
            let put = behind.area;
            self.area.left() < put.right()
                && put.left() < self.area.right()
                && self.area.top() < put.bottom()
                && put.top() < self.area.bottom()
        })
    }
}

/// The mark the light runs across, in the frame being drawn.
///
/// One at a time: it is the welcome screen's plate, and the welcome screen
/// is what shows when nothing is open.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Sheened {
    /// Which cells it is.
    pub(crate) area: Rect,
    /// What the mark rests at.
    pub(crate) from: Color,
    /// And what the light carries it to as it passes.
    pub(crate) to: Color,
    /// How many panes and boxes had been put over the page when it was
    /// said -- see `Page::layer`.
    pub(crate) depth: usize,
}

impl Sheened {
    /// Whether this cell is one of the mark's.
    pub(crate) fn holds(&self, x: u16, y: u16) -> bool {
        (self.area.left()..self.area.right()).contains(&x)
            && (self.area.top()..self.area.bottom()).contains(&y)
    }

    /// Whether the page still holds the mark this was said about.
    ///
    /// The same question `Capped::still_said` asks, and the welcome screen
    /// is where it is asked most: it is what shows when nothing is open,
    /// so it is under every list a reader opens from there. The cells the
    /// plate was written into then belong to the list, and a light run
    /// across them lights somebody else's letters -- which is how this was
    /// found, with the tail of every file name past the plate's left edge
    /// coming out in the mark's own colour.
    ///
    /// What says a cell is still the mark's is its ink: a colour on the
    /// ramp between the two this was said with, which is what the cells
    /// say about the sheen in their own way -- a step of it per column,
    /// out and back. A cell with nothing in it is one of the gaps in the
    /// letters and says nothing either way -- so it is asked whose the cell
    /// is as well: the welcome screen is under the settings and the counts
    /// too, and a dialog that leaves the plate's rows blank left a light
    /// running at the frame rate under it, over nothing.
    pub(crate) fn still_said(&self, page: &Page) -> bool {
        let (Color::Rgb(from_r, from_g, from_b), Color::Rgb(to_r, to_g, to_b)) =
            (self.from, self.to)
        else {
            // A theme in named colours has no ramp for a cell to be on,
            // and nothing to check one against.
            return false;
        };
        let between = |one: u8, other: u8, at: u8| at >= one.min(other) && at <= one.max(other);
        (self.area.top()..self.area.bottom()).all(|y| {
            (self.area.left()..self.area.right()).all(|x| {
                if page.layer(x, y) > self.depth {
                    return false;
                }
                let look = page.look(x, y);
                if look.text.trim().is_empty() {
                    return true;
                }
                let Color::Rgb(red, green, blue) = look.foreground else {
                    return false;
                };
                between(from_r, to_r, red)
                    && between(from_g, to_g, green)
                    && between(from_b, to_b, blue)
            })
        })
    }
}

/// A row the view drew a line along, in the frame being drawn.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Ruled {
    /// Which cells it is.
    pub(crate) area: Rect,
    /// How many panes and boxes had been put over the page when it was
    /// said -- see `Page::layer`.
    pub(crate) depth: usize,
}

impl Ruled {
    /// How much of this cell the line runs across, where the cell is
    /// still this rule's: from which part of its width to which.
    ///
    /// Asked cell by cell rather than of the whole row, because what a
    /// view drew over inside the same frame may cover part of a line and
    /// leave the rest -- and the cells are the truth, the same as they
    /// are for a cap. A cell holding anything but the rule's glyphs is
    /// somebody else's, and keeps what they wrote in it.
    ///
    /// A tee is where the line meets a frame's side, so it starts or
    /// stops in the middle, where that side's line is.
    pub(crate) fn spans(&self, page: &Page, x: u16, y: u16) -> Option<(f32, f32)> {
        if y != self.area.y
            || x < self.area.x
            || x >= self.area.right()
            || page.layer(x, y) > self.depth
        {
            return None;
        }
        match page.look(x, y).text {
            "\u{2500}" => Some((0.0, 1.0)),
            "\u{251c}" => Some((0.5, 1.0)),
            "\u{2524}" => Some((0.0, 0.5)),
            _ => None,
        }
    }

    /// Whether every cell of the row is still this rule's.
    pub(crate) fn still_said(&self, page: &Page) -> bool {
        (self.area.left()..self.area.right()).all(|x| self.spans(page, x, self.area.y).is_some())
    }
}

/// Where a marked run of rows is in the frame being drawn.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Stroked {
    /// Which rows, which edge of the column, and what it is about.
    pub(crate) stroke: Stroke,
    /// How many panes and boxes had been put over the page when it was
    /// said -- see `Page::layer`.
    pub(crate) depth: usize,
}

impl Stroked {
    /// The glyph a terminal stands this stroke in with.
    ///
    /// Which is the same fact twice over, and deliberately: the side is
    /// what the view *said*, and the glyph is what it *wrote*. The first
    /// is what the shape is drawn from; the second is only what says the
    /// cell is still this stroke's, the way a rule's glyphs are -- and a
    /// stroke that disagrees with the cells is about a screen that is not
    /// on the screen.
    const fn glyph(&self) -> &'static str {
        match (self.stroke.about, self.stroke.side) {
            (About::Seam, _) => "\u{2594}",
            (About::Rows, Side::Left) => "\u{258c}",
            (About::Rows, Side::Right) => "\u{2590}",
        }
    }

    /// Whether this cell is still the stroke's.
    ///
    /// Asked row by row rather than of the run, because a view that drew
    /// and was then drawn over inside the same frame keeps the rows
    /// nobody covered: the editor's margin is drawn to the foot of its
    /// region and a compact list goes over the bottom of it, so the rows
    /// under the list belong to the list.
    pub(crate) fn holds(&self, page: &Page, x: u16, y: u16) -> bool {
        let area = self.stroke.area;
        x == area.x
            && (area.top()..area.bottom()).contains(&y)
            && page.look(x, y).text == self.glyph()
            && page.layer(x, y) <= self.depth
    }

    /// The rows of it that are still its own, as runs: where each starts
    /// and how many rows it is.
    ///
    /// A run cut in half by something drawn over its middle is two
    /// strokes, for the reason a rule covered in the middle is two lines.
    pub(crate) fn runs(&self, page: &Page) -> Vec<(u16, u16)> {
        let area = self.stroke.area;
        let mut runs: Vec<(u16, u16)> = Vec::new();
        for y in area.top()..area.bottom() {
            if !self.holds(page, area.x, y) {
                continue;
            }
            match runs.last_mut() {
                Some(run) if run.0 + run.1 == y => run.1 += 1,
                _ => runs.push((y, 1)),
            }
        }
        runs
    }
}

/// How far in from one edge of the window the grid starts.
///
/// The compositor picks the window's size and the font picks the cell's,
/// so the one is not a whole number of the other and a strip is left over
/// on each axis. It is halved and put outside both ends -- the grid is
/// middled in the window rather than pushed into a corner -- and what is
/// in it is the page's own ground: every cell is the same size, and the
/// one on the edge is not stretched to cover the strip up.
///
/// Floored, so that a cell's corner lands on a whole pixel. Half a pixel
/// down, every glyph in the window is drawn across two rows of them.
pub(crate) fn margin(window: f32, cell: f32, count: u16) -> f32 {
    ((window - f32::from(count) * cell) / 2.0).max(0.0).floor()
}

/// Where the grid's first cell is in the window, across and down.
///
/// Below the title bar, where the window has one of its own -- see
/// `title::height` -- and middled in what is left. One answer, asked by
/// the pointer and the caret as well as the painter: a grid drawn one
/// place and pointed at in another is a click on the row above the one
/// under the pointer.
pub(crate) fn origin(window: [f32; 2], titled: f32, cell: [f32; 2], count: [u16; 2]) -> [f32; 2] {
    [
        margin(window[0], cell[0], count[0]),
        titled + margin(window[1] - titled, cell[1], count[1]),
    ]
}

/// Where a cap is in the frame being drawn, and what it is drawn in.
#[derive(Clone, Debug)]
pub(crate) struct Capped {
    /// The key in it.
    pub(crate) keys: String,
    /// Which cells it is.
    pub(crate) area: Rect,
    /// The ground the key sits on.
    pub(crate) cap: Color,
    /// What is behind it.
    pub(crate) page: Color,
    /// What draws its outline.
    pub(crate) edge: Color,
    /// How many panes and boxes had been put over the page when it was
    /// said -- see `Page::layer`.
    pub(crate) depth: usize,
}

/// Where a mark is, in the frame being drawn.
#[derive(Clone, Debug)]
pub(crate) struct Marked {
    /// Which mark.
    pub(crate) id: String,
    /// In which colouring.
    pub(crate) focused: bool,
    /// The column.
    pub(crate) x: u16,
    /// And the row.
    pub(crate) y: u16,
    /// How many panes and boxes had been put over the page when it was
    /// said -- see `Page::layer`.
    pub(crate) depth: usize,
}

impl Marked {
    /// Whether the cells this mark was drawn at are still nobody's.
    ///
    /// The same question again, asked the other way round: a view that has
    /// a picture drawn writes nothing in the cells it goes in -- that is
    /// what `Images::draw` answering `true` means -- so there is no glyph
    /// of its own to look for, and anything in them at all is somebody
    /// else's writing. A box opened over the agents page had two agents'
    /// marks sitting on its frame.
    ///
    /// And whose the cells are, which the cells cannot say: a page put
    /// over the agents writes blanks where it has nothing to say, and a
    /// blank over a blank is still blank. The page that says the project
    /// has gone had the agents' marks floating on it.
    pub(crate) fn still_said(&self, page: &Page) -> bool {
        let slot = obelus_ui::image::SLOT;
        (self.y..self.y.saturating_add(slot.height)).all(|y| {
            (self.x..self.x.saturating_add(slot.width))
                .all(|x| page.layer(x, y) <= self.depth && page.look(x, y).text.trim().is_empty())
        })
    }
}

impl Capped {
    /// Whether the page still says what this cap was said about.
    ///
    /// A view that draws and is then drawn over inside the same frame has
    /// already said its caps, and the cells they were about belong to
    /// whatever covered them -- the welcome screen under a list is drawn
    /// and replaced every frame, and its six keys left six empty caps on
    /// the list. The cells are the truth: a cap that disagrees with them
    /// is about a screen that is not the screen.
    ///
    /// The key sits one cell in from the cap's own left edge, which is
    /// where the blank inside a cap is, wherever the cap came from.
    pub(crate) fn still_said(&self, page: &Page) -> bool {
        let area = self.area;
        (area.top()..area.bottom())
            .all(|y| (area.left()..area.right()).all(|x| page.layer(x, y) <= self.depth))
            && self.said_by(|x, y| page.look(x, y).text)
    }

    /// Whether what a pane was put over says what this cap was said about.
    ///
    /// The other half of the same question. A cap drawn over by a list is
    /// not on the page, but it is in the picture of what the list was put
    /// over -- and the glass shows that picture, so the cap is the
    /// picture's to draw. Asked only of [`Self::still_said`]'s caps, the
    /// welcome screen under a list was its keys in plain ink: the cells
    /// were seen through the glass and the shape round them was not.
    ///
    /// Only a cap said before the pane, as everything in the picture was: a
    /// cap said on the pane with the same key in the same place is the
    /// pane's, and drawn there already.
    pub(crate) fn still_behind(&self, behind: &Behind) -> bool {
        self.depth <= behind.depth
            && behind.area.intersection(self.area) == self.area
            && self.said_by(|x, y| behind.look(x, y).map_or("", |look| look.text))
    }

    fn said_by<'a>(&self, text: impl Fn(u16, u16) -> &'a str) -> bool {
        let mut said = String::new();
        for at in 1..self.area.width.saturating_sub(1) {
            said.push_str(text(self.area.x.saturating_add(at), self.area.y));
        }
        said.trim_end() == self.keys
    }
}

/// The cells, as the window has them.
///
/// The window keeps the grid because it is the one that redraws: a frame
/// arrives as the handful of cells that changed, and a redraw with nothing
/// changed at all -- a window uncovered, a monitor waking -- has to be able
/// to draw the same screen again without asking Obelus for it.
#[derive(Clone, Debug)]
pub(crate) struct Page {
    columns: u16,
    rows: u16,
    /// How many columns and rows `cells` holds, which is never fewer than
    /// are on the screen and may be more -- see `resized`.
    kept_columns: u16,
    kept_rows: u16,
    cells: Vec<Cell>,
    caret: Option<Position>,
    shape: Caret,
    whose: Option<Layer>,
    typing: bool,
    /// What the caret is drawn in, and the reset colour until the
    /// application has said -- see `Page::caret_colour`.
    ink: Color,
    /// What the page is drawn on, which is what a cell nobody has written
    /// yet is drawn on too -- see `blank`.
    ground: Color,
    /// Where each pane and box was put over it on the frame it holds, in
    /// the order they were said -- see `layer`.
    put_over: Vec<Rect>,
}

impl Default for Page {
    fn default() -> Self {
        Self {
            columns: 0,
            rows: 0,
            kept_columns: 0,
            kept_rows: 0,
            cells: Vec::new(),
            caret: None,
            // Until the application says otherwise, which it does with the
            // first frame: a bar is what it is anywhere a reader types.
            shape: Caret::Bar,
            whose: None,
            // And nothing is typed into a screen with nothing on it.
            typing: false,
            ink: Color::Reset,
            ground: Color::Reset,
            put_over: Vec::new(),
        }
    }
}

/// One cell, as the painter reads it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Look<'a> {
    /// What is in it, which may be several characters making one mark.
    pub(crate) text: &'a str,
    /// The ink.
    pub(crate) foreground: ratatui::style::Color,
    /// And what is behind it.
    pub(crate) background: ratatui::style::Color,
    /// Bold, italic, and the rest of what a style can say.
    pub(crate) modifier: ratatui::style::Modifier,
    /// What colour the line under it is, where it is underlined.
    ///
    /// Its own colour and not the ink: a server's complaint is underlined
    /// in the colour that kind of trouble is written in, under a word the
    /// syntax has already coloured. `Reset` is a terminal saying "the ink",
    /// which is what an underline nobody coloured gets.
    pub(crate) underline: ratatui::style::Color,
}

impl Look<'_> {
    /// How many columns this cell takes up.
    ///
    /// Two for a full-width character, and the cell after it is one
    /// `ratatui` has reset -- no text and no colours. In a terminal that
    /// cell is never drawn at all, because the terminal itself advances two
    /// columns for a wide glyph; a window draws every cell, so without
    /// asking this it painted the default background behind the second half
    /// of every Chinese character and a line of them came out striped.
    pub(crate) fn columns(&self) -> u16 {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the width of one cell's text, which is one or two"
        )]
        let columns = obelus_text::text_width(self.text) as u16;
        columns.max(1)
    }
}

impl Page {
    /// How many columns there are.
    pub(crate) const fn columns(&self) -> u16 {
        self.columns
    }

    /// And how many rows.
    pub(crate) const fn rows(&self) -> u16 {
        self.rows
    }

    /// Where the caret is, when it is being shown.
    /// What the caret belongs to, and `None` for the document's own.
    ///
    /// Not a cell, and kept here for the reason its shape is: it is a
    /// fact about the frame the cells arrived with.
    pub(crate) const fn whose(&self) -> Option<Layer> {
        self.whose
    }

    pub(crate) const fn caret(&self) -> Option<Position> {
        self.caret
    }

    /// And what shape it is drawn in.
    pub(crate) const fn shape(&self) -> Caret {
        self.shape
    }

    /// Whether a character typed now goes into any text.
    pub(crate) const fn typing(&self) -> bool {
        self.typing
    }

    /// What colour a caret on this cell is drawn in: the theme's, whatever
    /// the cell's own ink is -- and that ink only before the application
    /// has said the theme's, which is the first frame and nothing after it.
    ///
    /// Here rather than in the painter, which is the one place it is
    /// asked, because the painter cannot be run without a device and this
    /// is the half of it that can be wrong.
    pub(crate) fn caret_colour(&self, x: u16, y: u16) -> Color {
        match self.ink {
            Color::Reset => self.look(x, y).foreground,
            ink => ink,
        }
    }

    /// Makes room for a screen this size, keeping each cell at the place it
    /// was, and the room it grew by on the page's own ground.
    ///
    /// Kept rather than emptied, because the window draws before Obelus
    /// has answered: a resize asks for a frame at once, and the screen the
    /// application sends for the new size is a channel and a layout away.
    /// An emptied page drawn in that gap is every cell in the reset
    /// colour, which is near black -- a flash on every resize, and one
    /// after another while a corner is dragged. The old cells at their
    /// old places are what a terminal shows in the same gap; and they are
    /// moved row by row rather than reshaped as one run, which would draw
    /// the old screen slewed. The frame Obelus sends at a new size clears
    /// the page first, so nothing kept here outlives it.
    ///
    /// And what a smaller screen has no room for is kept too, out of
    /// sight. The window can go one size and come back before Obelus draws
    /// at all -- a reader's wiggle of the corner, the text a size bigger
    /// and back -- and then Obelus never saw a new size, so it neither
    /// clears nor says again what it believes is still there: the cells a
    /// shrink threw away stayed blank until something else changed them.
    pub(crate) fn resized(&mut self, columns: u16, rows: u16) {
        self.columns = columns;
        self.rows = rows;
        if columns <= self.kept_columns && rows <= self.kept_rows {
            return;
        }
        let (wide, tall) = (columns.max(self.kept_columns), rows.max(self.kept_rows));
        let mut cells = vec![self.blank(); usize::from(wide) * usize::from(tall)];
        for y in 0..self.kept_rows {
            for x in 0..self.kept_columns {
                cells[usize::from(y) * usize::from(wide) + usize::from(x)] = std::mem::take(
                    &mut self.cells
                        [usize::from(y) * usize::from(self.kept_columns) + usize::from(x)],
                );
            }
        }
        self.kept_columns = wide;
        self.kept_rows = tall;
        self.cells = cells;
    }

    /// Says where each pane and box was put over the frame the page now
    /// holds, in the order they were said.
    pub(crate) fn put_over(&mut self, stack: &[Behind]) {
        self.put_over = stack.iter().map(|over| over.area).collect();
    }

    /// How many of the panes and boxes put over the page are under what
    /// is in this cell: none for the page's own, one for the first pane's,
    /// and so on -- the last one said whose area this cell is in.
    ///
    /// Which is who a cell belongs to, and the one thing its glyph cannot
    /// say. A list opened over a file puts its bar in the file's column in
    /// the same block, so the cells are the same whoever wrote them: the
    /// file's mark was drawn on the list's rows. A shape is said at a
    /// depth -- how many had been put over the page when it was said --
    /// and a cell deeper than that is somebody else's by now, whatever it
    /// holds.
    pub(crate) fn layer(&self, x: u16, y: u16) -> usize {
        self.put_over
            .iter()
            .rposition(|over| over.contains(Position { x, y }))
            .map_or(0, |at| at + 1)
    }

    /// Says what the page is drawn on.
    pub(crate) const fn drawn_on(&mut self, ground: Color) {
        self.ground = ground;
    }

    /// A cell nobody has written: a space on the page's ground.
    ///
    /// Not `Cell::default()`, whose background is the reset colour and is
    /// drawn near black. A frame that clears the page writes back only the
    /// cells it has room for, and while a corner is dragged it has room
    /// for less than the window already does: the rest was a black strip
    /// down the growing edge on every frame of the drag. `ratatui` holds a
    /// reset cell where this holds a blank, and Obelus paints its own
    /// background over every cell it draws, so the two only differ where
    /// nothing has been drawn.
    fn blank(&self) -> Cell {
        let mut blank = Cell::default();
        blank.set_bg(self.ground);
        blank
    }

    /// Blanks the cells a full-width character at this place covers.
    ///
    /// The one thing a frame does not arrive with. A terminal advanced
    /// two columns itself when it drew such a character, so `ratatui` has
    /// nothing to say about the second one and its diff leaves it out --
    /// and a window draws *every* cell, so what was in it is still drawn:
    /// a letter from some frame before, in the colours it had then,
    /// sitting on top of the character that took its place. A page of
    /// Chinese over a page of code showed a word of the code scattered
    /// across it, syntax colouring and all.
    ///
    /// Blanked rather than left, and with the character's own style,
    /// because that is what `ratatui`'s own buffer holds there: what the
    /// diff failed to say, the page says to itself.
    fn covered(&mut self, x: u16, y: u16) {
        let Some(at) = self.at(x, y) else {
            return;
        };
        let (wide, style) = (
            obelus_text::text_width(self.cells[at].symbol()),
            self.cells[at].style(),
        );
        for over in 1..wide {
            let Ok(over) = u16::try_from(over) else {
                return;
            };
            let Some(covered) = x.checked_add(over).and_then(|x| self.at(x, y)) else {
                return;
            };
            self.cells[covered].reset();
            self.cells[covered].set_style(style);
        }
    }

    /// Takes one update, and says whether it ended a frame.
    pub(crate) fn apply(&mut self, update: Update) -> bool {
        match update {
            Update::Cell { x, y, cell } => {
                if let Some(at) = self.at(x, y) {
                    self.cells[at] = *cell;
                    self.covered(x, y);
                }
                false
            }
            Update::Cleared => {
                let blank = self.blank();
                for cell in &mut self.cells {
                    *cell = blank.clone();
                }
                false
            }
            Update::Caret(caret) => {
                self.caret = caret;
                false
            }
            Update::CaretIs {
                shape,
                whose,
                typing,
            } => {
                self.shape = shape;
                self.whose = whose;
                self.typing = typing;
                false
            }
            Update::CaretInk(ink) => {
                self.ink = ink;
                false
            }
            // Neither is a cell: the window takes both out of the queue
            // before the page is handed anything.
            Update::Mark { id, .. } | Update::Marked { id, .. } => {
                tracing::warn!(id, "a mark reached the page");
                false
            }
            // Nor is this: it is a setting, and the window takes it out
            // of the queue with the size and the faces.
            Update::Animates(on) | Update::BarTabs(on) => {
                tracing::warn!(on, "a setting reached the page");
                false
            }
            Update::PictureStrength(share) => {
                tracing::warn!(share, "a setting reached the page");
                false
            }
            // Nor are these: they are what a run of cells *is* and what
            // was under one, and the window takes both out of the queue
            // with the marks.
            Update::Capped { area, .. }
            | Update::Ticked { area, .. }
            | Update::Spun { area }
            | Update::Ruled { area }
            | Update::Linked { area }
            | Update::Spelled { area }
            | Update::Paired { area, .. }
            | Update::Found { area, .. }
            | Update::Sheened { area, .. }
            | Update::Parted { area }
            | Update::Behind { area, .. }
            | Update::Scrolled { area, .. } => {
                tracing::warn!(?area, "a cap reached the page");
                false
            }
            Update::Barred { bar } => {
                tracing::warn!(?bar.area, "a cap reached the page");
                false
            }
            Update::Stroked { stroke } => {
                tracing::warn!(?stroke.area, "a cap reached the page");
                false
            }
            Update::Frame => true,
            // Taken out of the queue before the page is handed anything,
            // because it is not about a cell. A page that reached this
            // would be a window that failed to act on it.
            Update::TextSize(points) => {
                tracing::warn!(points, "a text size reached the page");
                false
            }
            Update::Fonts(names) => {
                tracing::warn!(faces = names.len(), "a list of faces reached the page");
                false
            }
            Update::Ground(_) => {
                tracing::warn!("a page's ground reached the page");
                false
            }
            Update::Holding { .. } => {
                tracing::warn!("a hold's colours reached the page");
                false
            }
            Update::Picture(_) => {
                tracing::warn!("a wallpaper reached the page");
                false
            }
            Update::Recoloured => {
                tracing::warn!("a change of theme reached the page");
                false
            }
            Update::Tabs(_) => {
                tracing::warn!("the tabs reached the page");
                false
            }
        }
    }

    /// What is in one cell.
    ///
    /// Outside the grid is empty rather than a panic: a frame drawn against
    /// the size the window had a moment ago is the ordinary case during a
    /// resize, and the frame after it is the right one.
    pub(crate) fn look(&self, x: u16, y: u16) -> Look<'_> {
        let Some(at) = self.at(x, y) else {
            return Look {
                text: " ",
                foreground: ratatui::style::Color::Reset,
                background: ratatui::style::Color::Reset,
                modifier: ratatui::style::Modifier::empty(),
                underline: ratatui::style::Color::Reset,
            };
        };
        let cell = &self.cells[at];
        Look {
            text: cell.symbol(),
            foreground: cell.fg,
            background: cell.bg,
            modifier: cell.modifier,
            underline: cell.underline_color,
        }
    }

    /// Where a cell is in the list, or nothing when it is off the grid.
    fn at(&self, x: u16, y: u16) -> Option<usize> {
        (x < self.columns && y < self.rows)
            .then(|| usize::from(y) * usize::from(self.kept_columns) + usize::from(x))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Updates shown together cross to the window's thread as one send.
    ///
    /// Broken deliberately by sending each on its own (a `send` for each in
    /// `Here::show_all`): a frame of cells was as many sends.
    #[test]
    fn updates_shown_together_cross_as_one() {
        use obelus_front::Showing as _;

        let (updates, frames) = std::sync::mpsc::channel();
        let here = Here::new(updates, Arc::new(|| {}), [1920, 1080]);
        let mut cell = Cell::default();
        cell.set_symbol("x");
        here.show_all(
            (0..50)
                .map(|x| obelus_front::Update::Cell {
                    x,
                    y: 0,
                    cell: Box::new(cell.clone()),
                })
                .collect(),
        )
        .expect("shown");
        let sent: Vec<Sent> = std::iter::from_fn(|| frames.try_recv().ok()).collect();
        assert_eq!(sent.len(), 1, "not one send");
        assert_eq!(sent[0].len(), 50);
    }

    /// Rows of text written into a page, one character to a cell.
    fn written(rows: &[&str]) -> Page {
        let mut page = Page::default();
        let width = rows
            .iter()
            .map(|row| row.chars().count())
            .max()
            .unwrap_or(0);
        page.resized(
            u16::try_from(width).expect("a short row"),
            u16::try_from(rows.len()).expect("a few rows"),
        );
        for (y, row) in rows.iter().enumerate() {
            for (x, character) in row.chars().enumerate() {
                let mut cell = Cell::default();
                cell.set_symbol(&character.to_string());
                page.apply(Update::Cell {
                    x: u16::try_from(x).expect("a short row"),
                    y: u16::try_from(y).expect("a few rows"),
                    cell: Box::new(cell),
                });
            }
        }
        page
    }

    /// A resize keeps every cell at the place it was until Obelus's own
    /// frame for the new size arrives, and the room it grew by is the
    /// page's ground rather than the reset colour.
    ///
    /// Deliberate breaks: emptying the cells, as `resized` once did, leaves
    /// `a` gone and the page drawn near black until the next frame; one
    /// `resize_with` over the old run without emptying it puts `c` on the
    /// first row, because it was the third cell; and `Cell::default()` for
    /// the new room leaves its background `Reset`.
    #[test]
    fn a_resize_keeps_the_cells_where_they_were() {
        let ground = Color::Rgb(1, 2, 3);
        let mut page = written(&["ab", "cd"]);
        page.drawn_on(ground);
        page.resized(3, 3);
        assert_eq!(page.look(0, 0).text, "a");
        assert_eq!(page.look(1, 0).text, "b");
        assert_eq!(page.look(0, 1).text, "c");
        assert_eq!(page.look(1, 1).text, "d");
        for (x, y) in [(2, 0), (2, 1), (0, 2), (2, 2)] {
            assert_eq!(page.look(x, y).text, " ");
            assert_eq!(page.look(x, y).background, ground);
        }
        page.resized(1, 2);
        assert_eq!(page.look(0, 0).text, "a");
        assert_eq!(page.look(0, 1).text, "c");
    }

    /// The caret is drawn in the colour the theme said, whatever the ink of
    /// the cell it is on -- and a frame clearing the page does not take
    /// that colour with it, because it is the theme's and not the frame's.
    ///
    /// Deliberate breaks: the arm for it not keeping the colour, and
    /// `caret_colour` answering the cell's ink as the painter once did --
    /// both draw it in the keyword's purple. The last
    /// assertion is a guard against a `Cleared` taught to reset everything
    /// the page holds, which no break of the code as it is can show.
    #[test]
    fn the_caret_is_in_the_colour_it_was_told() {
        let keyword = Color::Rgb(192, 132, 252);
        let mut page = Page::default();
        page.resized(1, 1);
        let mut cell = Cell::default();
        cell.set_symbol("f").set_fg(keyword);
        page.apply(Update::Cell {
            x: 0,
            y: 0,
            cell: Box::new(cell),
        });
        assert_eq!(
            page.caret_colour(0, 0),
            keyword,
            "before the theme's is said, the cell's ink is all there is"
        );
        let rosewater = Color::Rgb(245, 224, 220);
        page.apply(Update::CaretInk(rosewater));
        assert_eq!(page.caret_colour(0, 0), rosewater, "the keyword's colour");
        page.apply(Update::Cleared);
        assert_eq!(page.caret_colour(0, 0), rosewater, "a frame took it away");
    }

    /// A window that goes smaller and back before Obelus draws has every
    /// cell it had, because Obelus saw no new size and says nothing again.
    ///
    /// Deliberate break: dropping what is off the screen on a shrink, as
    /// the first version of keeping the cells did -- `cd` comes back blank.
    #[test]
    fn a_page_made_smaller_and_back_has_what_it_had() {
        let mut page = written(&["abcd"]);
        page.resized(2, 1);
        assert_eq!(page.look(2, 0).text, " ");
        page.resized(4, 1);
        assert_eq!(page.look(2, 0).text, "c");
        assert_eq!(page.look(3, 0).text, "d");
    }

    /// The first frame `ratatui` draws at a new size takes away every cell
    /// the page kept from the old one, including those it has nothing to
    /// say about.
    ///
    /// What keeping the cells across a resize stands on: `ratatui` diffs
    /// against the last frame, so a cell it leaves blank at the new size
    /// is sent only if it clears first. Deliberate break: `Cells::clear`
    /// returning `Ok(())` without sending `Cleared` -- `bcd` is then still
    /// on the page after a frame that drew only `x`.
    #[test]
    fn the_first_frame_at_a_new_size_takes_the_kept_cells_away() {
        let (updates, frames) = std::sync::mpsc::channel();
        let measured = Arc::new(Measured::default());
        measured.resized(4, 1, 40, 10);
        let here = Arc::new(Here::new(updates, Arc::new(|| {}), [1920, 1080]));
        let cells = Cells::new(here, Arc::clone(&measured));
        let mut terminal = ratatui::Terminal::new(cells).expect("a terminal");
        let mut page = Page::default();
        page.resized(4, 1);
        let draw = |terminal: &mut ratatui::Terminal<Cells>, said: &'static str| {
            terminal
                .draw(|frame| {
                    frame
                        .buffer_mut()
                        .set_string(0, 0, said, ratatui::style::Style::new());
                })
                .expect("a frame");
        };
        draw(&mut terminal, "abcd");
        for update in std::iter::from_fn(|| frames.try_recv().ok()).flatten() {
            page.apply(update);
        }
        assert_eq!(page.look(1, 0).text, "b");
        measured.resized(6, 1, 60, 10);
        page.resized(6, 1);
        draw(&mut terminal, "x");
        for update in std::iter::from_fn(|| frames.try_recv().ok()).flatten() {
            page.apply(update);
        }
        assert_eq!(page.look(0, 0).text, "x");
        for x in 1..6 {
            assert_eq!(page.look(x, 0).text, " ", "column {x}");
        }
    }

    /// A page cleared is cleared onto its ground, so the cells a frame
    /// does not write back are not drawn near black.
    ///
    /// Deliberate break: `Cell::default()` in the `Cleared` arm of
    /// `apply`, as it was. Both cells are then `Reset`, which is the strip
    /// down the growing edge of a window being dragged bigger.
    #[test]
    fn a_page_is_cleared_onto_its_ground() {
        let ground = Color::Rgb(1, 2, 3);
        let mut page = written(&["ab"]);
        page.drawn_on(ground);
        page.apply(Update::Cleared);
        for x in 0..2 {
            assert_eq!(page.look(x, 0).text, " ");
            assert_eq!(page.look(x, 0).background, ground);
        }
    }

    /// A rule is a line only where its cells still say `─`, and a tee
    /// stops the line in the middle, where the side it meets is.
    ///
    /// Deliberate break: answer `Some((0.0, 1.0))` from `Ruled::spans`
    /// for every cell in the row. The letter a later view wrote over the
    /// rule is then left out by `letters` and drawn over by a line, and
    /// the line at a frame's side pokes half a cell out of the frame.
    #[test]
    fn a_rule_is_a_line_only_where_its_cells_still_say_so() {
        let page = written(&["\u{251c}\u{2500}x\u{2500}\u{2524}"]);
        let rule = Ruled {
            area: Rect {
                x: 0,
                y: 0,
                width: 5,
                height: 1,
            },
            depth: 0,
        };
        assert_eq!(rule.spans(&page, 0, 0), Some((0.5, 1.0)), "a tee");
        assert_eq!(rule.spans(&page, 1, 0), Some((0.0, 1.0)));
        assert_eq!(rule.spans(&page, 2, 0), None, "written over");
        assert_eq!(rule.spans(&page, 4, 0), Some((0.0, 0.5)), "the other tee");
        assert!(!rule.still_said(&page), "one cell is not the rule's");
    }

    /// And a switch is a switch only where its cell still says so.
    ///
    /// Deliberate break: answer `true`. A setting's switch is then drawn
    /// on the list of another setting's choices opened over it -- a box
    /// on a row with nothing to switch, and the list's own letter missing
    /// under it, because `letters` leaves a switch's cell to `ticks`.
    #[test]
    fn a_switch_is_a_switch_only_where_its_cell_still_says_so() {
        let set = obelus_ui::tick(true).to_string();
        let page = written(&[&set, "x"]);
        let switch = |y: u16, on: bool| Ticked {
            area: Rect {
                x: 0,
                y,
                width: 1,
                height: 1,
            },
            on,
            depth: 0,
        };
        assert!(switch(0, true).still_said(&page), "the settings' own");
        assert!(!switch(1, true).still_said(&page), "and one a list covered");
        assert!(
            !switch(0, false).still_said(&page),
            "nor one drawn unset where the page says it is set"
        );
    }

    /// And not under something put over the page that has the same glyph
    /// in the same cell, which the cell alone cannot tell apart.
    ///
    /// Deliberate break: take `page.layer` out of `Ticked::still_said`, or
    /// out of `Spun::round`. The switch under the list, or the turn under a
    /// file previewed over it, is still drawn -- over the list's own box,
    /// or turning over the file's letter.
    #[test]
    fn a_switch_or_a_turn_under_the_same_glyph_is_not_drawn() {
        let set = obelus_ui::tick(true).to_string();
        let mut page = written(&[&set, "\u{2819}"]);
        let cell = |y: u16| Rect {
            x: 0,
            y,
            width: 1,
            height: 1,
        };
        let switch = |depth: usize| Ticked {
            area: cell(0),
            on: true,
            depth,
        };
        let turn = |depth: usize| Spun {
            area: cell(1),
            depth,
        };
        assert!(switch(0).still_said(&page), "a switch with nothing over it");
        assert!(
            turn(0).round(&page).is_some(),
            "a turn with nothing over it"
        );
        page.put_over(&[pane(Rect::new(0, 0, 1, 2))]);
        assert!(!switch(0).still_said(&page), "a switch under the list");
        assert!(turn(0).round(&page).is_none(), "a turn under the preview");
        assert!(switch(1).still_said(&page), "a switch said on the list");
        assert!(turn(1).round(&page).is_some(), "a turn said on the list");
    }

    /// And a mark is drawn only where the cells are still nobody's.
    ///
    /// Deliberate break: answer `true`. A box opened over the agents page
    /// then has two agents' marks sitting on its frame. Or ask about the
    /// first cell alone: a mark is two cells wide, and a box whose edge
    /// lands in the second of them is a picture over a line.
    #[test]
    fn a_mark_is_drawn_only_where_the_cells_are_still_nobody_s() {
        // A row a box's frame reached the third cell of, and a row it
        // did not reach at all.
        let page = written(&["  \u{2502}", "   "]);
        let mark = |x: u16, y: u16| Marked {
            id: "claude".to_string(),
            focused: false,
            x,
            y,
            depth: 0,
        };
        assert!(mark(0, 1).still_said(&page), "two cells nobody wrote in");
        assert!(mark(0, 0).still_said(&page), "and the two before the line");
        assert!(
            !mark(1, 0).still_said(&page),
            "but not where the line is the second of them"
        );
    }

    /// And not under a page put over it that left its cells blank.
    ///
    /// Deliberate break: take `page.layer` out of `still_said`. The cells
    /// are blank under the page as they were before it, so the mark is
    /// drawn on the page that says the project has gone.
    #[test]
    fn a_mark_under_a_page_with_nothing_in_its_cells_is_not_drawn() {
        let mut page = written(&["   ", "   "]);
        let mark = |depth: usize| Marked {
            id: "claude".to_string(),
            focused: false,
            x: 0,
            y: 1,
            depth,
        };
        assert!(mark(0).still_said(&page), "with nothing over it");
        page.put_over(&[pane(Rect::new(0, 0, 3, 2))]);
        assert!(!mark(0).still_said(&page), "under the page put over it");
        assert!(mark(1).still_said(&page), "and one said on that page");
    }

    /// And a line between two things is drawn only where nothing has been
    /// put over them.
    ///
    /// It is the one shape that cannot be asked of the cells: a line in
    /// the pixel between two rows has no cell of its own to still hold
    /// anything. What it is asked of is what covers the page -- and the
    /// page it is said by is a whole-screen view, so what covers one is a
    /// pane or a box with a frame round it.
    ///
    /// Deliberate break: answer `true`. The notes' own lines are then
    /// drawn across the list a reader opened over them, at the rows the
    /// notes happened to be on.
    #[test]
    fn a_line_between_two_things_is_not_drawn_under_a_pane() {
        let parting = |y: u16| Parted {
            area: Rect {
                x: 0,
                y,
                width: 20,
                height: 1,
            },
        };
        let pane = Behind {
            area: Rect {
                x: 0,
                y: 4,
                width: 20,
                height: 6,
            },
            joined: Joined::Above,
            ground: Color::Rgb(9, 9, 9),
            cells: Vec::new(),
            depth: 0,
        };
        let over = [&pane];

        assert!(parting(2).still_said(&over), "above what was put over it");
        assert!(!parting(4).still_said(&over), "the pane's own first row");
        assert!(!parting(9).still_said(&over), "and its last");
        assert!(parting(10).still_said(&over), "below it again");
        assert!(parting(4).still_said(&[]), "with nothing over the page");
    }

    /// And the mark is the mark only where the page still holds it.
    ///
    /// The welcome screen is what shows when nothing is open, so it is
    /// under every list a reader opens from there, and the light is the
    /// one shape that does not stop at a letter -- it runs across a
    /// rectangle. Asked of the area alone it lit the list: every file
    /// name whose tail reached past the plate's left edge came out in the
    /// mark's own colour, which is how this was found.
    ///
    /// Deliberate break: answer `true`. The second assertion goes, which
    /// is the list's own row inside the plate's rectangle.
    #[test]
    fn a_mark_a_list_was_opened_over_is_not_lit() {
        let from = Color::Rgb(0, 0, 0);
        let to = Color::Rgb(100, 100, 100);
        // Two rows of the plate, in colours along the ramp between them,
        // and a third the list wrote in its own ink.
        let mut page = inked(&[
            ("\u{2588}\u{2588}\u{2588}", Color::Rgb(0, 0, 0)),
            ("\u{2588} \u{2588}", Color::Rgb(60, 60, 60)),
            ("abc", Color::Rgb(220, 30, 30)),
        ]);
        let mark = |height: u16| Sheened {
            area: Rect {
                x: 0,
                y: 0,
                width: 3,
                height,
            },
            from,
            to,
            depth: 0,
        };
        assert!(
            mark(2).still_said(&page),
            "the plate's own rows, gaps and all"
        );
        assert!(
            !mark(3).still_said(&page),
            "a row of the list is inside the plate and is not the plate"
        );
        // And a dialog over the whole of it, whatever is in the cells: the
        // gaps in the plate are blank, and blank is the dialog's here.
        // Deliberate break: take `page.layer` out of `still_said`, and the
        // plate's two rows are still lit under it.
        page.put_over(&[pane(Rect::new(0, 0, 3, 3))]);
        assert!(!mark(2).still_said(&page), "the plate under a dialog");
    }

    /// Rows of text, each in one colour.
    fn inked(rows: &[(&str, Color)]) -> Page {
        let mut page = Page::default();
        let width = rows
            .iter()
            .map(|(row, _)| row.chars().count())
            .max()
            .unwrap_or(0);
        page.resized(
            u16::try_from(width).expect("a short row"),
            u16::try_from(rows.len()).expect("a few rows"),
        );
        for (y, (row, ink)) in rows.iter().enumerate() {
            for (x, character) in row.chars().enumerate() {
                let mut cell = Cell::default();
                cell.set_symbol(&character.to_string());
                cell.fg = *ink;
                page.apply(Update::Cell {
                    x: u16::try_from(x).expect("a short row"),
                    y: u16::try_from(y).expect("a few rows"),
                    cell: Box::new(cell),
                });
            }
        }
        page
    }

    /// And a bar is a bar only where its cells still say so, and where
    /// nothing put over the page after it was said is.
    ///
    /// Row by row, which is what a column needs and a cell does not: the
    /// thing drawn over a mark may take the middle of it, so what is left
    /// is runs rather than an answer about the whole. The file's own bar
    /// under a list keeps none of the rows the list wrote its own bar
    /// into: the block there is the list's, and the file's mark drawn on
    /// it was a second thumb that stayed where the file was. The track is
    /// blank, so it is never among the runs -- and a blank over it says
    /// nothing either way.
    ///
    /// Deliberate break: have `holds` answer `true`. The file's own bar is
    /// then drawn down the side of the list opened over it -- past the
    /// list, the preview and the foot, widening under the pointer -- on a
    /// list of nine rows with nothing to scroll. And take the layer out of
    /// it: the file's bar keeps the list's two rows.
    ///
    /// The depths and the page's layers are written here as the window
    /// would set them, and that setting is not under test: it happens where
    /// a window takes a frame, which stamps each shape with how many panes
    /// and boxes were said before it and hands the page the stack at the
    /// frame's end -- dropping either passes every test here. What was
    /// checked by hand is that it fails in the window: every bar stamped
    /// at depth nothing, and a list's own bar is the terminal's block.
    #[test]
    fn a_bar_is_a_bar_only_where_its_cells_still_say_so() {
        let area = |y: u16, height: u16| Rect {
            x: 0,
            y,
            width: 1,
            height,
        };
        let showing = |y: u16, height: u16, thumb: u16, depth: usize| Barred {
            bar: Bar {
                area: area(y, height),
                mark: 0,
                thumb,
            },
            depth,
            shown: 1.0,
            under: 0.0,
        };
        // The file's bar alone: its mark on the first row, the track blank.
        let page = written(&["\u{2588}", " ", " ", " "]);
        assert_eq!(
            showing(0, 4, 1, 0).runs(&page),
            vec![(0, 1)],
            "the mark's row, and none of the track's"
        );
        // A list over the file, which drew a bar of two rows' mark in the
        // same column and its foot under it.
        let mut page = written(&["\u{2588}", "\u{2588}", " ", "\u{2500}"]);
        page.put_over(&[pane(area(0, 4))]);
        assert_eq!(
            showing(0, 2, 2, 1).runs(&page),
            vec![(0, 2)],
            "the list's own, said over the list"
        );
        assert_eq!(
            showing(0, 4, 1, 0).runs(&page),
            Vec::new(),
            "and the file's under the list, whose rows those are"
        );
        // A pane put over somewhere else takes nothing from it.
        let mut page = written(&["\u{2588}", " ", " ", " "]);
        page.put_over(&[pane(Rect { x: 1, ..area(0, 4) })]);
        assert_eq!(
            showing(0, 4, 1, 0).runs(&page),
            vec![(0, 1)],
            "a pane beside the column took rows from it"
        );
    }

    /// A rule under a pane is not a line where the pane is, though the
    /// pane wrote the same glyph in the same row.
    ///
    /// The status row's rule and a list's own line can be one row of `─`
    /// written twice, so the cells cannot say whose the row is: the one
    /// said before the pane is the page's, and the pane's cells are not.
    ///
    /// Deliberate break: take `page.layer` out of `Ruled::spans`, and the
    /// page's rule is drawn across the pane's row as well as the pane's.
    #[test]
    fn a_rule_under_a_pane_is_not_the_panes() {
        let mut page = written(&["\u{2500}\u{2500}\u{2500}\u{2500}"]);
        page.put_over(&[pane(Rect::new(2, 0, 2, 1))]);
        let rule = |depth| Ruled {
            area: Rect::new(0, 0, 4, 1),
            depth,
        };
        assert_eq!(rule(0).spans(&page, 1, 0), Some((0.0, 1.0)), "beside it");
        assert_eq!(rule(0).spans(&page, 2, 0), None, "the pane's cell");
        assert!(!rule(0).still_said(&page), "and so not all of it");
        assert_eq!(
            rule(1).spans(&page, 2, 0),
            Some((0.0, 1.0)),
            "the pane's own"
        );
    }

    /// Half a pair of brackets is still the pair's only where the cell
    /// holds a bracket and nothing has been put over it.
    ///
    /// A list opened over the file is drawn after it in the same frame,
    /// and its rows can hold a bracket at the very cell the file's was in
    /// -- so neither the glyph nor the layer alone says whose the cell is.
    ///
    /// Deliberate break: take `page.layer` out of `Paired::still_said`, and
    /// the light shines on the pane's `(`; take the bracket out, and it
    /// shines on a letter the frame wrote over the file's bracket.
    #[test]
    fn half_a_pair_under_a_pane_is_not_the_pairs() {
        let mut page = written(&["(a)(b)"]);
        page.put_over(&[pane(Rect::new(3, 0, 3, 1))]);
        let half = |x, depth| Paired {
            area: Rect::new(x, 0, 1, 1),
            pair: Pair {
                document: 0,
                version: 0,
                open: 0,
            },
            depth,
            held: None,
        };
        assert!(half(0, 0).still_said(&page), "the file's own bracket");
        assert!(!half(1, 0).still_said(&page), "a letter is no bracket");
        assert!(!half(3, 0).still_said(&page), "the pane's cell");
        assert!(half(3, 1).still_said(&page), "said by the pane itself");
    }

    /// A pair that has gone is drawn going out only while its cell is the
    /// bracket it was, in the ground it wore then.
    ///
    /// The key that took the caret out may have scrolled the file too, and
    /// then the cell holds another character -- or another bracket.
    ///
    /// Deliberate break: answer `still_said` in `still_there`, and the
    /// light goes out on the `]` that scrolled into the `(`'s cell.
    #[test]
    fn a_pair_going_out_is_where_its_bracket_still_is() {
        let mut page = written(&["(a)"]);
        let mut half = Paired {
            area: Rect::new(0, 0, 1, 1),
            pair: Pair {
                document: 0,
                version: 0,
                open: 0,
            },
            depth: 0,
            held: None,
        };
        assert!(!half.still_there(&page), "nothing held yet");
        half.hold(&page);
        assert_eq!(half.held.map(|(bracket, _)| bracket), Some('('));
        assert!(half.still_there(&page), "the same bracket");
        // Scrolled: another bracket in the cell.
        let mut cell = Cell::default();
        cell.set_symbol("]");
        page.apply(Update::Cell {
            x: 0,
            y: 0,
            cell: Box::new(cell),
        });
        assert!(half.still_said(&page), "it is a bracket");
        assert!(!half.still_there(&page), "but not that one");
    }

    /// A change mark under a pane keeps only the rows the pane is not on.
    ///
    /// Deliberate break: take `page.layer` out of `Stroked::holds`, and
    /// the file's margin runs on down the side of a list put over it.
    #[test]
    fn a_stroke_under_a_pane_keeps_the_rows_beside_it() {
        let mut page = written(&["\u{2590}", "\u{2590}", "\u{2590}", "\u{2590}"]);
        page.put_over(&[pane(Rect::new(0, 2, 1, 2))]);
        let stroke = |area, depth| Stroked {
            stroke: Stroke {
                area,
                side: Side::Right,
                about: About::Rows,
            },
            depth,
        };
        let file = stroke(Rect::new(0, 0, 1, 4), 0);
        assert_eq!(file.runs(&page), vec![(0, 2)], "above the pane");
        let list = stroke(Rect::new(0, 2, 1, 2), 1);
        assert_eq!(list.runs(&page), vec![(2, 2)], "and the pane's own");
    }

    /// A cap under a pane is not drawn on the pane, though the pane wrote
    /// the same key in the same cells.
    ///
    /// Deliberate break: take the layers out of `Capped::still_said`, and
    /// the welcome screen's cap is drawn over a list's own `f1`.
    #[test]
    fn a_cap_under_a_pane_is_not_the_panes() {
        let mut page = written(&[" f1 "]);
        let cap = |depth| Capped {
            keys: "f1".to_string(),
            area: Rect::new(0, 0, 4, 1),
            cap: Color::Reset,
            page: Color::Reset,
            edge: Color::Reset,
            depth,
        };
        assert!(cap(0).still_said(&page), "nothing over it");
        page.put_over(&[pane(Rect::new(0, 0, 4, 1))]);
        assert!(!cap(0).still_said(&page), "under the pane");
        assert!(cap(1).still_said(&page), "the pane's own");
    }

    /// A box's frame is not the frame where something said after it was
    /// put over, though that wrote a line in the same cell.
    ///
    /// Deliberate break: take `owns` out of `Behind::ring_holds`, and a
    /// list over one side of a hover leaves the hover's frame drawn down
    /// the list's own line; out of `framed`, and a hover whose corner a
    /// list covered is still a box.
    #[test]
    fn a_frame_under_a_pane_is_not_the_panes() {
        let mut page = written(&[
            "\u{256d}\u{2500}\u{2500}\u{256e}",
            "\u{2502}  \u{2502}",
            "\u{2570}\u{2500}\u{2500}\u{256f}",
        ]);
        let frame = a_box(4, 3);
        let list = Behind {
            depth: 1,
            ..pane(Rect::new(2, 0, 2, 3))
        };
        page.put_over(&[frame.clone(), list]);
        assert!(frame.ring_holds(&page, 1, 0), "beside the list");
        assert!(!frame.ring_holds(&page, 2, 0), "the list's cell");
        assert!(!frame.framed(&page), "a corner is the list's");
    }

    /// A pane put over the page, of which only where it is matters here.
    fn pane(area: Rect) -> Behind {
        Behind {
            area,
            joined: Joined::Above,
            ground: Color::Reset,
            cells: Vec::new(),
            depth: 0,
        }
    }

    /// A box with a frame, as the view says one: a pane joined to nothing,
    /// of which only where it is matters here.
    fn a_box(width: u16, height: u16) -> Behind {
        Behind {
            area: Rect {
                x: 0,
                y: 0,
                width,
                height,
            },
            joined: Joined::Nowhere,
            ground: Color::Reset,
            cells: Vec::new(),
            depth: 0,
        }
    }

    /// A frame's ring is the frame's only where it still holds a line.
    ///
    /// Deliberate break: drop the glyph check from `Behind::ring_holds`. The
    /// cell a list drew over one side of a hover is then taken for the
    /// frame, its letter is left out and its ground is not painted.
    #[test]
    fn a_frame_leaves_a_cell_drawn_over_to_what_is_in_it() {
        let page = written(&[
            "\u{256d}\u{2500}\u{2500}\u{256e}",
            "x ab\u{2502}",
            "\u{2570}\u{2500}\u{2500}\u{256f}",
        ]);
        let frame = a_box(4, 3);
        assert!(frame.framed(&page), "the four corners are there");
        assert!(frame.ring_holds(&page, 1, 0), "a line of the ring");
        assert!(!frame.ring_holds(&page, 0, 1), "written over");
        assert!(!frame.ring_holds(&page, 1, 1), "inside the box");
    }

    /// What a box was put over keeps its letters under the ring, and only
    /// its letters and only under the ring.
    ///
    /// Which cells, and nothing about how they are drawn: that `frame`
    /// draws them, and before its line, is the painter's, and nothing here
    /// can see it.
    ///
    /// Deliberate breaks: answer an empty list from `Behind::lettered_ring`,
    /// which is what `frame` did before -- the ground put back and no
    /// letter, a blank half cell all round a hover. Drop `ring_holds` from
    /// it, and under the cell a list drew over goes the letter that was
    /// there before the list. Drop `a_line`, and the rule under the
    /// ring is drawn from the font beside the frame's own line. Leave the
    /// sides out of the walk round the ring, and the code beside a hover
    /// loses its letter on every row but the first and the last.
    #[test]
    fn a_frame_puts_back_the_letters_under_its_ring() {
        let page = written(&[
            "\u{256d}\u{2500}\u{2500}\u{256e}",
            "x a\u{2502}",
            "\u{2502} b\u{2502}",
            "\u{2570}\u{2500}\u{2500}\u{256f}",
        ]);
        let frame = Behind {
            cells: "fn main\u{2500}q  r    "
                .chars()
                .map(|character| {
                    let mut cell = Cell::default();
                    cell.set_symbol(&character.to_string());
                    cell
                })
                .collect(),
            ..a_box(4, 4)
        };
        let mut cells: Vec<(u16, u16, &str)> = frame
            .lettered_ring(&page)
            .into_iter()
            .map(|(x, y, under)| (x, y, under.text))
            .collect();
        cells.sort_by_key(|&(x, y, _)| (y, x));
        assert_eq!(
            cells,
            [
                (0, 0, "f"),
                (1, 0, "n"),
                (3, 0, "m"),
                (0, 2, "q"),
                (3, 2, "r")
            ],
            "the space in `fn m`, the cell written over, the rule under the \
             right-hand side, the inside of the box and the row of spaces are \
             not"
        );
    }

    /// A full-width letter put back on a box's right-hand edge keeps the
    /// cell it runs into past the edge, while that cell is still blank.
    ///
    /// Deliberate breaks: answer an empty list from `Behind::spilt`, and
    /// `backgrounds` paints that cell after the letter is drawn -- what was
    /// left of the character was the sliver between the line and the edge
    /// of its cell. Drop `past.max`, and one down the left-hand edge claims
    /// the blank cell inside the box beside it, a square of ground on the
    /// glass. Drop the blank check, and the cell a list drew a letter in
    /// loses its ground.
    #[test]
    fn a_wide_letter_on_a_box_s_edge_keeps_the_cell_past_it() {
        let page = written(&[
            "\u{256d}\u{2500}\u{2500}\u{256e} ",
            "\u{2502}  \u{2502} ",
            "\u{2570}\u{2500}\u{2500}\u{256f}x",
        ]);
        // What ratatui leaves after a full-width character is a blank
        // cell, which is what the box's own picture holds there too.
        let frame = Behind {
            cells: [
                "\u{4e2d}", "", "\u{6587}", "", "\u{5b57}", "", "", "\u{5b57}", "", "", "",
                "\u{5b57}",
            ]
            .into_iter()
            .map(|symbol| {
                let mut cell = Cell::default();
                cell.set_symbol(if symbol.is_empty() { " " } else { symbol });
                cell
            })
            .collect(),
            ..a_box(4, 3)
        };
        assert_eq!(
            frame.spilt(&page),
            [(4, 1)],
            "the runs into the ring along the top, into the box from the \
             left and into a cell with a letter in it are not"
        );
    }

    /// A frame with a corner drawn over is not drawn at all.
    ///
    /// Deliberate break: answer `true` from `Behind::framed`. The
    /// hover a list covered the top of then has its round box drawn under
    /// the list, as a line crossing somebody else's rows.
    #[test]
    fn a_frame_missing_a_corner_is_not_drawn() {
        let page = written(&["xx\u{256e}", "\u{2570}\u{2500}\u{256f}"]);
        assert!(!a_box(3, 2).framed(&page));
    }

    /// A cap is about cells, and the cells are the truth.
    ///
    /// Deliberate break: answer `true` from `Capped::still_said` whatever
    /// the page holds, and the welcome screen -- drawn every frame under
    /// the list that covers it, and saying its caps before it is covered
    /// -- leaves six empty caps sitting on the list.
    #[test]
    fn a_cap_whose_cells_were_drawn_over_is_not_drawn() {
        let mut page = Page::default();
        page.resized(10, 1);
        fn write(page: &mut Page, said: &str) {
            for (at, character) in said.chars().enumerate() {
                let x = u16::try_from(at).expect("a short run");
                let mut cell = Cell::default();
                cell.set_symbol(&character.to_string());
                page.apply(Update::Cell {
                    x,
                    y: 0,
                    cell: Box::new(cell),
                });
            }
        }
        write(&mut page, " f1 ");
        let cap = Capped {
            keys: "f1".to_string(),
            area: Rect {
                x: 0,
                y: 0,
                width: 4,
                height: 1,
            },
            cap: Color::Reset,
            page: Color::Reset,
            edge: Color::Reset,
            depth: 0,
        };
        assert!(cap.still_said(&page));
        // What a view drawn over inside the same frame leaves behind.
        write(&mut page, "    ");
        assert!(!cap.still_said(&page));
    }

    /// And a cap drawn over by a pane is still in the picture of what the
    /// pane was put over, which is what the glass shows.
    ///
    /// Deliberate break: answer `false` from `Capped::still_behind`, and
    /// the welcome screen under a list is its keys in plain ink with no cap
    /// round any of them -- the thing a reader saw.
    #[test]
    fn a_cap_under_a_pane_is_the_pictures_to_draw() {
        let picture = |said: &str, area: Rect| Behind {
            area,
            joined: Joined::Below,
            ground: Color::Reset,
            cells: said
                .chars()
                .map(|character| {
                    let mut cell = Cell::default();
                    cell.set_symbol(&character.to_string());
                    cell
                })
                .collect(),
            depth: 0,
        };
        let cap = Capped {
            keys: "f1".to_string(),
            area: Rect {
                x: 2,
                y: 1,
                width: 4,
                height: 1,
            },
            cap: Color::Reset,
            page: Color::Reset,
            edge: Color::Reset,
            depth: 0,
        };
        let row = Rect {
            x: 0,
            y: 1,
            width: 8,
            height: 1,
        };
        assert!(cap.still_behind(&picture("   f1   ", row)));
        assert!(!cap.still_behind(&picture("   f2   ", row)), "another key");
        // A cap partly in the picture is partly through the glass and
        // partly on the page, which is neither's to draw -- even where the
        // key itself is all inside and only the cap's blank is not.
        //
        // Deliberate break: drop the `intersection` from `still_behind`,
        // and this one is drawn.
        let short = Rect { width: 5, ..row };
        assert!(
            !cap.still_behind(&picture("   f1", short)),
            "its blank outside"
        );
        // And a cap said on the pane, over the same key in the picture.
        //
        // Deliberate break: drop `self.depth <= behind.depth`, and this one
        // is drawn twice: on the pane, and under it in the picture.
        let on_the_pane = Capped { depth: 1, ..cap };
        assert!(
            !on_the_pane.still_behind(&picture("   f1   ", row)),
            "a cap said after the pane"
        );
    }

    /// A full-width character takes the cells it covers with it.
    ///
    /// Deliberate break: leave `covered` out of the cell arm. A letter
    /// from some frame before then stays under the right half of every
    /// full-width character -- in the colours it had then, which is how
    /// this was found: a page of Chinese with a word of the code that had
    /// been there scattered over it, syntax colouring and all.
    #[test]
    fn a_full_width_character_takes_the_cells_it_covers_with_it() {
        let mut page = Page::default();
        page.resized(6, 1);
        fn put(page: &mut Page, x: u16, said: &str) {
            let mut cell = Cell::default();
            cell.set_symbol(said);
            page.apply(Update::Cell {
                x,
                y: 0,
                cell: Box::new(cell),
            });
        }
        // A word of code, as an earlier frame left it.
        for (at, letter) in "shell".chars().enumerate() {
            let x = u16::try_from(at).expect("a short word");
            put(&mut page, x, &letter.to_string());
        }
        // And a character twice as wide over its first cell, which is all
        // the diff has to say about it.
        put(&mut page, 0, "\u{4e2d}");

        assert_eq!(page.look(0, 0).text, "\u{4e2d}");
        assert!(
            page.look(1, 0).text.trim().is_empty(),
            "the h is still under it: {:?}",
            page.look(1, 0).text
        );
        // And no further: what it does not cover is not its to take.
        assert_eq!(page.look(2, 0).text, "e");
    }

    /// What is under the pane is read at the place it was taken from.
    ///
    /// Deliberate break: count the rows by the region's *left edge* rather
    /// than by its width -- which is the same number whenever a pane
    /// starts at column zero, and every pane in Obelus does but one. The
    /// glass would show the page shifted sideways by however far in the
    /// pane begins, and only on the one pane that is not full width.
    #[test]
    fn what_is_under_a_pane_is_read_where_it_was_taken_from() {
        let area = Rect {
            x: 3,
            y: 1,
            width: 2,
            height: 2,
        };
        let cells: Vec<Cell> = ["a", "b", "c", "d"]
            .into_iter()
            .map(|symbol| {
                let mut cell = Cell::default();
                cell.set_symbol(symbol);
                cell
            })
            .collect();
        let behind = Behind {
            area,
            joined: Joined::Above,
            ground: Color::Reset,
            cells,
            depth: 0,
        };
        assert_eq!(behind.look(3, 1).map(|look| look.text), Some("a"));
        assert_eq!(behind.look(4, 1).map(|look| look.text), Some("b"));
        assert_eq!(behind.look(3, 2).map(|look| look.text), Some("c"));
        assert_eq!(behind.look(4, 2).map(|look| look.text), Some("d"));
        // Outside it in every direction, because a pane's region is not
        // the screen and the painter walks what it is given.
        assert!(behind.look(2, 1).is_none());
        assert!(behind.look(5, 1).is_none());
        assert!(behind.look(3, 0).is_none());
        assert!(behind.look(3, 3).is_none());
    }

    /// The strip the cells do not reach is halved, and both halves are
    /// outside the grid.
    ///
    /// Deliberate break: drop the `/ 2.0`. The margin then swallows the
    /// whole strip, the grid's far edge lands a strip short of the
    /// window's, and the first assertion here fails by exactly the amount
    /// the last one says must be left over on the other side.
    #[test]
    fn the_strip_the_grid_does_not_reach_is_halved() {
        // Nineteen pixels over, which is what a compositor's height and a
        // font's line height do to each other.
        let over = 19.0_f32;
        let (cell, count) = (36.0_f32, 20_u16);
        let window = f32::from(count) * cell + over;
        let before = super::margin(window, cell, count);
        assert_eq!(before, 9.0, "the strip was not halved");
        let after = window - (before + f32::from(count) * cell);
        assert_eq!(after, 10.0, "what is left over is the other half");
    }

    /// A margin is a whole number of pixels, and never negative.
    ///
    /// Deliberate break: take the `.floor()` off and the first assertion
    /// gets 9.5, which is a grid half a pixel down the window and a glyph
    /// drawn across two rows of pixels. Take the `.max(0.0)` off and the
    /// last one goes negative, which is a grid drawn off the top of a
    /// window whose page is momentarily wider than it is -- the ordinary
    /// case one frame into a resize.
    #[test]
    fn a_margin_is_whole_pixels_and_never_negative() {
        assert_eq!(super::margin(379.0, 36.0, 10), 9.0);
        assert_eq!(super::margin(360.0, 36.0, 10), 0.0, "nothing is left over");
        assert_eq!(super::margin(100.0, 36.0, 10), 0.0, "the page is too big");
    }

    /// The grid starts under the title bar and is middled in what is left
    /// below it, not in the whole window.
    ///
    /// Deliberate break: middle it in `window[1]` and add the title bar
    /// after. The grid then starts half a title bar too far down, and its
    /// last row runs off the foot of the window by as much.
    #[test]
    fn the_grid_is_middled_under_the_title_bar() {
        // Twenty rows of 36 in a window 56 pixels taller: 48 of title bar
        // and 8 left over, which is 4 above the grid and 4 below it.
        let window = [400.0, 20.0 * 36.0 + 56.0];
        let [across, down] = super::origin(window, 48.0, [20.0, 36.0], [20, 20]);
        assert_eq!(across, 0.0);
        assert_eq!(down, 52.0);
        assert_eq!(window[1] - (down + 20.0 * 36.0), 4.0, "the strip below");
        // And with no title bar it is only the margin.
        let window = [400.0, 20.0 * 36.0 + 8.0];
        assert_eq!(super::origin(window, 0.0, [20.0, 36.0], [20, 20])[1], 4.0);
    }

    /// The cells of one match on one row, said a cell at a time by the
    /// editor, are one run -- and another match beside it on the same row,
    /// or the part of a match that wrapped, are runs of their own.
    ///
    /// Deliberate break: join cells side by side whatever match they are,
    /// and the second match becomes the end of the first.
    #[test]
    fn the_cells_of_one_match_are_one_run() {
        let found = |row, start| obelus_ui::shapes::Found {
            page: 1,
            row,
            start,
        };
        let cell = |x, y, which| Finding {
            area: Rect::new(x, y, 1, 1),
            found: which,
            depth: 0,
        };
        let one = found(0, 2);
        let other = found(0, 6);
        let runs = Finding::joined(vec![
            cell(3, 0, one),
            cell(2, 0, one),
            cell(4, 0, one),
            cell(5, 0, other),
            cell(6, 0, other),
            cell(0, 1, other),
        ]);
        let areas: Vec<Rect> = runs.iter().map(|run| run.area).collect();
        assert_eq!(
            areas,
            [
                Rect::new(2, 0, 3, 1),
                Rect::new(5, 0, 2, 1),
                Rect::new(0, 1, 1, 1)
            ],
            "{runs:?}"
        );
    }

    /// A plate a pane was put over is in the picture of what it was put
    /// over: said before the pane and all of it under the pane -- and for
    /// half a pair, a bracket in the cell the pane was put over.
    ///
    /// Deliberate breaks: leave out the depth, and a run the pane's own
    /// list said -- a match in a row of the list -- is drawn as the page's
    /// under it; leave out the bracket, and a pair moved off since is drawn
    /// on whatever the pane was put over in its place.
    #[test]
    fn a_plate_a_pane_was_put_over_is_in_its_picture() {
        let mut cells = vec![Cell::default(); 10 * 3];
        cells[10 + 4].set_symbol("(");
        let behind = Behind {
            area: Rect::new(0, 5, 10, 3),
            joined: Joined::Below,
            ground: Color::Reset,
            cells,
            depth: 1,
        };
        let found = obelus_ui::shapes::Found {
            page: 1,
            row: 6,
            start: 2,
        };
        let run = |area, depth| Finding { area, found, depth };
        assert!(run(Rect::new(2, 6, 3, 1), 0).still_behind(&behind));
        assert!(
            !run(Rect::new(2, 6, 3, 1), 2).still_behind(&behind),
            "the pane's own"
        );
        assert!(
            !run(Rect::new(8, 6, 4, 1), 0).still_behind(&behind),
            "half out from under it"
        );

        let half = |x| Paired {
            area: Rect::new(x, 6, 1, 1),
            pair: Pair {
                document: 1,
                version: 1,
                open: 0,
            },
            depth: 0,
            held: None,
        };
        assert!(half(4).still_behind(&behind), "the bracket");
        assert!(!half(5).still_behind(&behind), "no bracket there");
    }
}
