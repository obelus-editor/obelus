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
    /// Joined to nothing: a box put over the page for a moment. Every
    /// side is an edge, so every corner is rounded, and it arrives from
    /// nowhere because it does not travel at all.
    ///
    /// Its edge is a frame, which is the outermost ring of its cells: a
    /// terminal draws it in `╭─╮` on cells painted the box's own ground,
    /// so a round corner stands on a square one -- the only corner a cell
    /// has. A window draws the shape itself, the line where the glyphs put
    /// it and the glass inside the line, and outside it what the box was
    /// put over: which is in the cells this is said with, and is why a
    /// frame needs saying no more than this.
    Nowhere,
}

/// The bar beside a band, for a front end that can slide it.
///
/// A bar is not part of the band -- it says where the band *is*, and slid
/// with it, it would travel the band's distance instead of its own share
/// of it. But it is not still either: its mark belongs where the band is
/// being drawn rather than where the band is going.
///
/// What says where that is, is the row the mark is *drawn* on, and not a
/// rate. A rate is continuous and the mark is not, so a mark slid at one
/// sets out from a place a row's rounding away from where it was last
/// drawn -- which on screen is a mark that steps back before it goes on.
/// Between two rows it was drawn on there is nothing to disagree about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bar {
    /// The column or columns it takes.
    pub area: Rect,
    /// Which row of them its mark starts on.
    pub mark: u16,
    /// And how many rows the mark covers.
    ///
    /// Said rather than left to be worked out, for the reason `bar_reach`
    /// gives about the mark's row: a front end that divided the height by
    /// the total again would be a second working-out of the same three
    /// numbers, and a mark that is one length in a terminal and another in
    /// a window is the two front ends disagreeing about how much of the
    /// thing is on screen.
    ///
    /// It could be counted off the cells instead -- the mark's rows are
    /// the ones in the brighter of the two colours -- but that is reading
    /// the grid back to find out what it means, which is the mistake the
    /// editor's own note is about.
    pub thumb: u16,
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
    ///
    /// A screenful of cells, copied, on every frame a pane is up. Which
    /// is worth knowing rather than worth avoiding: measured at 66us for
    /// two hundred columns by fifty, against a frame a reader is waiting
    /// sixteen thousand of those for, and twelve frames a second is all
    /// the ticker ever asks for. The obvious saving -- keep the last one
    /// and send nothing where it has not changed -- was measured too, and
    /// comparing the two came to 62us: the same work to find out whether
    /// to do the work. A terminal pays none of it either way, because it
    /// never says who is drawing.
    /// `ground` is the pane's own colour, which is where the glass is:
    /// a cell of the pane wearing it is one that says nothing of its own,
    /// and a cell wearing anything else -- a selected row, a tab, a rule
    /// -- is the pane saying something and stays opaque. The view knows
    /// which; a front end counting colours would be guessing.
    fn behind(&self, area: Rect, joined: Joined, ground: Color, cells: &[Cell]);

    /// This cell is a switch, and it is set or it is not.
    ///
    /// One cell, which is the one the glyph is in: the blank after it
    /// belongs to the glyph and not to the box.
    ///
    /// No colours, unlike a cap's: what the switch is drawn in and what
    /// is behind it are the ink and the ground of that very cell, which
    /// the view has already written there. Said again here they would be
    /// the same two colours from two places, and two places is somewhere
    /// for them to differ.
    ///
    /// A terminal says all of this in a glyph, and says it whether or not
    /// anybody is listening here.
    fn ticked(&self, area: Rect, on: bool);

    /// This row is a line between two things.
    ///
    /// A terminal draws it in `─`, which is a glyph: it sits where the
    /// font put it and on the ground its cell was painted, and a window
    /// with glass behind a pane has nowhere to put the pane's edge but on
    /// a cell's boundary -- half a row from any line a glyph can draw. A
    /// window drawing the line itself puts the two in the same place.
    ///
    /// No colours, for the reason a switch has none: the ink is the one
    /// the view wrote in those cells. What travels is only which cells,
    /// and the cells say whether they still are one -- see `ruled` in the
    /// window.
    fn ruled(&self, area: Rect);

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
    fn scrolled(&self, area: Rect, top: i64, bar: Option<Bar>);

    /// There is a bar here, and this much of the thing is on screen.
    ///
    /// Said by the one function that draws one, so a front end hears about
    /// every bar in Obelus rather than about the two that happen to sit
    /// beside a band something else was already saying had moved.
    ///
    /// No colours, for the reason a switch has none: a bar is a column of
    /// cells the view has already written, and what the track and the mark
    /// are drawn in is the foreground of those very cells, with the page
    /// behind them their background. Said again here they would be the
    /// same colours from two places, and two places is somewhere for them
    /// to differ.
    ///
    /// A terminal has an answer to all of this already -- a full block a
    /// shade off the page with a brighter run in it -- which is the whole
    /// test for whether a thing may be said here.
    fn barred(&self, bar: Bar);

    /// The mark is here, and the light on it runs between these two.
    ///
    /// A terminal has its answer already: it writes the ramp into the
    /// cells' own foreground, eight bands across the columns sliding a
    /// step a tick, and that is every sheen a grid of colours can hold.
    /// What a window has that a grid has not is a band of light narrower
    /// than a column and softer at its edges than a colour can be -- which
    /// is a *shape* the region is, drawn out of the same two colours.
    ///
    /// The colours rather than the cells' own, and this is the one shape
    /// where that is not a second answer: a cell's foreground here is
    /// whichever band of the ramp happens to be over it at this instant.
    /// What the mark rests at and what the light carries it to are the two
    /// ends of that ramp, which are on the screen at some column or other
    /// and nowhere in particular -- so they are read off the theme, where
    /// the view read them, and checked against the extremes of what the
    /// cells hold.
    fn sheened(&self, area: Rect, from: Color, to: Color);
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

/// Tells whoever is drawing that a switch is here.
pub(crate) fn ticked(area: Rect, on: bool) {
    if let Some(shapes) = DRAWING.get() {
        shapes.ticked(area, on);
    }
}

/// Tells whoever is drawing that a row is a line between two things.
pub(crate) fn ruled(area: Rect) {
    if let Some(shapes) = DRAWING.get() {
        shapes.ruled(area);
    }
}

/// Tells whoever is drawing where a band of rows has got to.
pub(crate) fn scrolled(area: Rect, top: i64, bar: Option<Bar>) {
    if let Some(shapes) = DRAWING.get() {
        shapes.scrolled(area, top, bar);
    }
}

/// Tells whoever is drawing that a bar is here.
pub(crate) fn barred(bar: Bar) {
    if let Some(shapes) = DRAWING.get() {
        shapes.barred(bar);
    }
}

/// Tells whoever is drawing that the mark with the light on it is here.
pub(crate) fn sheened(area: Rect, from: Color, to: Color) {
    if let Some(shapes) = DRAWING.get() {
        shapes.sheened(area, from, to);
    }
}

/// Tells whoever is drawing that a cap is here.
pub(crate) fn capped(keys: &str, area: Rect, cap: Color, page: Color, edge: Color) {
    if let Some(shapes) = DRAWING.get() {
        shapes.capped(keys, area, cap, page, edge);
    }
}
