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
    fn behind(&self, area: Rect, ground: Color, cells: &[Cell]);
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
pub(crate) fn behind(area: Rect, ground: Color, cells: &CellBuffer) {
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
    shapes.behind(room, ground, &under);
}

/// Tells whoever is drawing that a cap is here.
pub(crate) fn capped(keys: &str, area: Rect, cap: Color, page: Color, edge: Color) {
    if let Some(shapes) = DRAWING.get() {
        shapes.capped(keys, area, cap, page, edge);
    }
}
