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

use ratatui::{layout::Rect, style::Color};

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

/// Tells whoever is drawing that a cap is here.
pub(crate) fn capped(keys: &str, area: Rect, cap: Color, page: Color, edge: Color) {
    if let Some(shapes) = DRAWING.get() {
        shapes.capped(keys, area, cap, page, edge);
    }
}
