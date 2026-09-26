//! What a view says about a region, beyond the cells it wrote there.
//!
//! A cell grid is the truth and both front ends draw it. What a window can
//! do with it that a terminal cannot is draw the *shape* a region is:
//! a key's cap is a rounded block with a lip under it, and in a terminal it
//! is a run of cells a shade off the page, because that is the only cap a
//! terminal has.
//!
//! So the view says the same kind of thing it says about a mark -- here is
//! a cap, it is here, and these are its colours -- and whoever is drawing
//! decides what that looks like. Which is the rule the whole of this
//! channel is written to: **the app says what a region *is*, and the front
//! end says what that looks like.** Never the other way round: a view that
//! asked for a radius in pixels would be the application deciding how a
//! window draws, which is the thing `Drawn` and the glyph switch exist to
//! stop.
//!
//! One test decides whether something may be said here: **the terminal has
//! to have an answer of its own already.** A cap does -- it is
//! `raised_background`, and it is drawn whether or not anybody is listening
//! here. Something with no terminal answer is not a fact about the view; it
//! is a drawing instruction, and it belongs in the front end that wants it.
//!
//! Which is why nothing here is ever required: the cells stay complete and
//! correct on their own, and a front end that says nothing gets exactly the
//! screen it gets today.
//!
//! Said once, on the way up, because who is drawing is a fact about the
//! process rather than about a frame -- the same thing
//! `obelus_config::drawn_in_a_window` says, and said for the same reason.
//! A terminal never says it, so [`capped`] is a call that goes nowhere,
//! which is what every test gets too.

use std::sync::{Arc, OnceLock};

use ratatui::{
    buffer::{Buffer as CellBuffer, Cell},
    layout::Rect,
    style::Color,
};

/// Which edge of the region a pane is joined to.
///
/// One fact deciding three things, which is why it is a fact and not three
/// settings. The edge it is joined to is not an edge at all: nothing there
/// is rounded, nothing there bends what is behind and nothing there
/// catches the light, because a join is a seam and not a boundary. The
/// other end *is* an edge, so it keeps its corners. And a pane arrives
/// from the side it is joined to, which is the only side it could come
/// from without crossing the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Joined {
    /// Hanging from the row above it: a page's whole region taken over.
    Above,
    /// Standing on the row below it: a list that leaves the code showing,
    /// which is what a compact one is for.
    Below,
}

/// What a front end that draws its own pixels can be told about a frame.
///
/// `&self` throughout, the same as `image::Marks` and for the same reason:
/// this is said in the middle of a frame being laid out, by whichever view
/// is drawing, and what is on the other end is a channel rather than
/// something to be borrowed mutably.
pub trait Shapes: Send + Sync {
    /// These cells are a key in a cap of its own.
    ///
    /// `keys` is the key itself, which sits one cell in from the area's
    /// left edge. It
    /// travels so that the cap can be *checked*: a view that draws and is
    /// then drawn over within the same frame has already said this, and
    /// the cells it said it about now belong to whatever covered them.
    /// Saying what it is about is what lets the front end notice -- the
    /// cells are the truth, and an annotation that disagrees with them is
    /// about a screen that is not on the screen.
    ///
    /// The colours travel with it because a front end that draws its own
    /// pixels has no theme: `cap` is the ground the key sits on, `page` is
    /// what is behind the cap and has to show through where its corners
    /// are rounded away, and `edge` is what draws its outline -- the same
    /// colour `panel` rounds its frame in, so the two round things Obelus
    /// draws are drawn in one ink.
    fn capped(&self, keys: &str, area: Rect, cap: Color, page: Color, edge: Color);

    /// What is behind a pane: the cells that were there in the moment
    /// before it was drawn over them.
    ///
    /// Said *then* rather than kept from the last frame, because the thing
    /// behind a dialog is one of the few things in Obelus that changes
    /// while nobody is touching it. The file under a list is redrawn every
    /// frame from the buffer it is in, so a reload the watcher noticed, a
    /// commit in another window, an agent's write -- all of them are
    /// already in these cells when the pane goes over them. A backdrop
    /// kept would be the screen as it was, going staler the longer the
    /// dialog stays up, and the one place a reader would notice is the
    /// one this is for.
    ///
    /// Row-major, `area.width` to a row. Nothing has to be done with it:
    /// a front end that draws a pane opaque, the way a terminal does, can
    /// ignore every one of these and be right.
    /// `ground` is the pane's own colour, which is where the glass is:
    /// a cell of the pane wearing it is one that says nothing of its own,
    /// and a cell wearing anything else -- a selected row, a tab, a rule
    /// -- is the pane saying something and stays opaque. The view knows
    /// which; a front end counting colours would be guessing.
    fn behind(&self, area: Rect, joined: Joined, ground: Color, cells: &[Cell]);

    /// This band shows a list of things starting at `top`.
    ///
    /// A number to be *compared*, not read: what a front end does with it
    /// is subtract the one it was given last time, and what that says is
    /// how many rows the band moved. Which is the only way it can be
    /// known -- a band that scrolled and a band whose every row changed
    /// are the same handful of differing cells, and nothing in them says
    /// which happened.
    ///
    /// Absolute rather than a difference, so that a frame drawn twice
    /// says the same thing twice and means it: a difference would have to
    /// be consumed, and a redraw nobody asked for would replay a scroll
    /// that already happened.
    fn scrolled(&self, area: Rect, top: i64);
}

/// Who is drawing, where it is somebody who wants to be told.
static DRAWING: OnceLock<Arc<dyn Shapes>> = OnceLock::new();

/// Says who is drawing.
///
/// Called once by a front end that draws its own pixels, before there is
/// anything on the screen. A second call is ignored rather than refused:
/// there is one window in a process, and a caller that says so twice has
/// said nothing new.
pub fn drawn_by(shapes: Arc<dyn Shapes>) {
    let _ = DRAWING.set(shapes);
}

/// Tells whoever is drawing what is under a pane, out of the frame being
/// laid out.
///
/// Costs a terminal nothing: `DRAWING` is never set there, so the cells
/// are not even walked.
pub(crate) fn behind(area: Rect, joined: Joined, ground: Color, cells: &CellBuffer) {
    let Some(shapes) = DRAWING.get() else {
        return;
    };
    let room = area.intersection(cells.area);
    if room.is_empty() {
        return;
    }
    let mut under = Vec::with_capacity(usize::from(room.width) * usize::from(room.height));
    for y in room.top()..room.bottom() {
        for x in room.left()..room.right() {
            under.push(cells[(x, y)].clone());
        }
    }
    shapes.behind(room, joined, ground, &under);
}

/// Tells whoever is drawing where a band of rows has got to.
pub(crate) fn scrolled(area: Rect, top: i64) {
    if let Some(shapes) = DRAWING.get() {
        shapes.scrolled(area, top);
    }
}

/// Tells whoever is drawing that a cap is here.
pub(crate) fn capped(keys: &str, area: Rect, cap: Color, page: Color, edge: Color) {
    if let Some(shapes) = DRAWING.get() {
        shapes.capped(keys, area, cap, page, edge);
    }
}
