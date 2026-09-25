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

use std::sync::{
    Arc,
    atomic::{AtomicU16, AtomicU32, Ordering},
    mpsc::Sender,
};

use obelus_app::app::Caret;
use obelus_ui::image::Palette;
use ratatui::{
    backend::{Backend, ClearType, WindowSize},
    buffer::Cell,
    layout::{Position, Size},
};

/// One change to what is on the screen.
///
/// Ordered, and applied in the order it was sent: a cell written and then
/// written again is two updates, and the second one is the one that counts.
#[derive(Debug)]
pub(crate) enum Update {
    /// A cell, at a place.
    Cell {
        /// The column.
        x: u16,
        /// The row.
        y: u16,
        /// What is in it.
        cell: Box<Cell>,
    },
    /// Everything, gone.
    Cleared,
    /// Where the caret is, or that it has been put away.
    Caret(Option<Position>),
    /// The end of a frame: what came before it is what to draw.
    Frame,
    /// How big the text is, in points.
    ///
    /// A setting, so it arrives the way every other change to what is on
    /// the screen arrives: down this channel, in order, from the thread
    /// that knows what the settings say.
    TextSize(usize),
    /// Which faces to draw in, tried in this order.
    ///
    /// The same journey as the size, for the same reason.
    Fonts(Vec<String>),
    /// What shape the caret is.
    ///
    /// Which is a fact about the frame it arrives with -- it depends on
    /// where the caret ended up -- so it travels with the frame rather
    /// than beside it.
    CaretShape(Caret),
    /// A mark the window may be asked to draw, and what it is drawn from.
    ///
    /// Once per mark and palette: the drawing is a few kilobytes of text,
    /// and what it becomes -- pixels at the size a cell is now -- is the
    /// window's business.
    Mark {
        /// Which agent's mark it is.
        id: String,
        /// Whether this is the one the reader is on, which is drawn on
        /// another colour.
        focused: bool,
        /// The drawing itself.
        svg: String,
        /// The colours to ink it in.
        palette: Palette,
    },
    /// And that mark goes here, in the frame being laid out.
    Marked {
        /// Which mark.
        id: String,
        /// In which of its two colourings.
        focused: bool,
        /// The column its left edge sits at.
        x: u16,
        /// And the row.
        y: u16,
    },
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
}

/// The window's end of what a view says about marks.
///
/// Down the same channel as the cells, because a mark is part of the frame
/// being laid out: one that arrived after the frame it belongs to would be
/// a picture drawn over the screen that replaced it.
#[derive(Clone)]
pub(crate) struct Marking {
    updates: Sender<Update>,
}

impl std::fmt::Debug for Marking {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Marking")
    }
}

impl Marking {
    /// Says where a view's marks go.
    pub(crate) const fn new(updates: Sender<Update>) -> Self {
        Self { updates }
    }
}

impl obelus_ui::image::Marks for Marking {
    fn carries(&self, id: &str, svg: &str, focused: bool, palette: Palette) {
        let _ = self.updates.send(Update::Mark {
            id: id.to_string(),
            focused,
            svg: svg.to_string(),
            palette,
        });
    }

    fn draws(&self, id: &str, focused: bool, x: u16, y: u16) {
        // No wake, the same as the caret's shape: this is said while a
        // frame is being laid out, and the frame's own end wakes the
        // window a moment later.
        let _ = self.updates.send(Update::Marked {
            id: id.to_string(),
            focused,
            x,
            y,
        });
    }
}

/// How the window is told the things only the application knows.
///
/// The other direction on the same wire: how big the text should be, and
/// what shape the caret is. Neither is a cell, so the window takes them out
/// of the queue before the page sees them -- but they go down the same
/// channel as the cells, because they are about the same frame and
/// anything else could arrive in the wrong order.
#[derive(Clone)]
pub(crate) struct Telling {
    updates: Sender<Update>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl std::fmt::Debug for Telling {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Telling")
    }
}

impl Telling {
    /// Says where to send what the application works out about the window.
    pub(crate) fn new(updates: Sender<Update>, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self { updates, wake }
    }
}

impl obelus_app::app::Drawing for Telling {
    fn text_size(&self, points: usize) {
        // A window that has gone is a send that fails, and the application
        // is about to find that out for itself on its next frame.
        if self.updates.send(Update::TextSize(points)).is_ok() {
            (self.wake)();
        }
    }

    fn use_fonts(&self, names: &[String]) {
        if self.updates.send(Update::Fonts(names.to_vec())).is_ok() {
            (self.wake)();
        }
    }

    fn caret_is(&self, caret: Caret) {
        // No wake: this arrives while a frame is being laid out, and the
        // frame's own end wakes the window a moment later. Waking here as
        // well would be a second wake for one screen.
        let _ = self.updates.send(Update::CaretShape(caret));
    }
}

/// The window is gone, so there is nowhere for a frame to go.
///
/// Which ends Obelus's loop, by the same route a terminal whose input thread
/// died ends it: the error travels up out of `app::run` and the thread
/// finishes.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Closed;

impl std::fmt::Display for Closed {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("the window is gone")
    }
}

impl std::error::Error for Closed {}

/// How big the screen is, as both sides measure it.
///
/// Written by the window when it is resized and read by Obelus's loop on
/// the next frame, which is a frame away rather than a lock away: a resize
/// arrives as an event too, so the loop reads this immediately afterwards
/// and never at a moment when nothing told it to look.
#[derive(Debug, Default)]
pub(crate) struct Measured {
    columns: AtomicU16,
    rows: AtomicU16,
    /// The window, in pixels, packed as width in the high half and height
    /// in the low one so that the two are read as one fact.
    pixels: AtomicU32,
}

impl Measured {
    /// Says how big the screen has become.
    pub(crate) fn resized(&self, columns: u16, rows: u16, width: u32, height: u32) {
        self.columns.store(columns, Ordering::Relaxed);
        self.rows.store(rows, Ordering::Relaxed);
        // Clamped rather than wrapped: a window wider than 65535 pixels is
        // not a thing to be wrong about quietly.
        let packed = (width.min(u32::from(u16::MAX)) << 16) | height.min(u32::from(u16::MAX));
        self.pixels.store(packed, Ordering::Relaxed);
    }

    /// How many columns and rows there are room for.
    fn size(&self) -> Size {
        Size::new(
            self.columns.load(Ordering::Relaxed),
            self.rows.load(Ordering::Relaxed),
        )
    }

    /// And how many pixels that is.
    fn pixels(&self) -> Size {
        let packed = self.pixels.load(Ordering::Relaxed);
        // Both halves were clamped to a `u16` on the way in.
        Size::new((packed >> 16) as u16, (packed & 0xffff) as u16)
    }
}

/// Where Obelus's frames go.
pub(crate) struct Cells {
    updates: Sender<Update>,
    /// What makes the window look. Sending alone does not: its loop is
    /// asleep in the platform's own wait, and the one way to reach a thread
    /// parked there is to post an event to it.
    wake: Arc<dyn Fn() + Send + Sync>,
    measured: Arc<Measured>,
    caret: Position,
    /// Whether the caret is being shown, so that moving it while it is put
    /// away does not put it back.
    shown: bool,
}

impl std::fmt::Debug for Cells {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Cells")
            .field("caret", &self.caret)
            .field("shown", &self.shown)
            .finish_non_exhaustive()
    }
}

impl Cells {
    /// The backend Obelus's loop draws into.
    pub(crate) fn new(
        updates: Sender<Update>,
        wake: Arc<dyn Fn() + Send + Sync>,
        measured: Arc<Measured>,
    ) -> Self {
        Self {
            updates,
            wake,
            measured,
            caret: Position::ORIGIN,
            shown: false,
        }
    }

    /// Puts one update on its way, or says the window has gone.
    fn send(&self, update: Update) -> Result<(), Closed> {
        self.updates.send(update).map_err(|_| Closed)
    }
}

impl Backend for Cells {
    type Error = Closed;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Closed>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        for (x, y, cell) in content {
            self.send(Update::Cell {
                x,
                y,
                cell: Box::new(cell.clone()),
            })?;
        }
        Ok(())
    }

    fn hide_cursor(&mut self) -> Result<(), Closed> {
        self.shown = false;
        self.send(Update::Caret(None))
    }

    fn show_cursor(&mut self) -> Result<(), Closed> {
        self.shown = true;
        self.send(Update::Caret(Some(self.caret)))
    }

    fn get_cursor_position(&mut self) -> Result<Position, Closed> {
        Ok(self.caret)
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Closed> {
        self.caret = position.into();
        match self.shown {
            true => self.send(Update::Caret(Some(self.caret))),
            // Where it will be when it comes back, which is what the
            // terminal does with the same call: a caret that is put away is
            // still somewhere.
            false => Ok(()),
        }
    }

    fn clear(&mut self) -> Result<(), Closed> {
        self.send(Update::Cleared)
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Closed> {
        match clear_type {
            ClearType::All => self.clear(),
            // The rest are a terminal's: they clear from the caret to
            // somewhere, which only means anything where the caret is a
            // place a stream of bytes has reached. Obelus draws a whole
            // screen every frame and asks for none of them.
            other => {
                tracing::warn!(?other, "a window clears the whole screen or nothing");
                Ok(())
            }
        }
    }

    fn size(&self) -> Result<Size, Closed> {
        Ok(self.measured.size())
    }

    fn window_size(&mut self) -> Result<WindowSize, Closed> {
        Ok(WindowSize {
            columns_rows: self.measured.size(),
            pixels: self.measured.pixels(),
        })
    }

    fn flush(&mut self) -> Result<(), Closed> {
        self.send(Update::Frame)?;
        (self.wake)();
        Ok(())
    }
}

/// The cells, as the window has them.
///
/// The window keeps the grid because it is the one that redraws: a frame
/// arrives as the handful of cells that changed, and a redraw with nothing
/// changed at all -- a window uncovered, a monitor waking -- has to be able
/// to draw the same screen again without asking Obelus for it.
#[derive(Debug)]
pub(crate) struct Page {
    columns: u16,
    rows: u16,
    cells: Vec<Cell>,
    caret: Option<Position>,
    shape: Caret,
}

impl Default for Page {
    fn default() -> Self {
        Self {
            columns: 0,
            rows: 0,
            cells: Vec::new(),
            caret: None,
            // Until the application says otherwise, which it does with the
            // first frame: a bar is what it is anywhere a reader types.
            shape: Caret::Bar,
        }
    }
}

/// What an input method is spelling, before it becomes a word.
///
/// Drawn by the window over the cells to the right of the caret, which is
/// where the word will go. It is not in the file and not in any cell: an
/// application that was told about it would be an application with
/// half-typed pinyin in a buffer's undo history.
#[derive(Clone, Debug)]
pub(crate) struct Spelling {
    /// What has been typed so far.
    pub(crate) text: String,
    /// How many characters into it the input method's own caret is.
    pub(crate) caret: usize,
}

impl Spelling {
    /// How many columns of the grid it takes up to its caret, which is
    /// where the caret is drawn and where the candidates are pointed.
    pub(crate) fn columns(&self) -> usize {
        obelus_text::text_width(&self.text.chars().take(self.caret).collect::<String>())
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
    pub(crate) const fn caret(&self) -> Option<Position> {
        self.caret
    }

    /// And what shape it is drawn in.
    pub(crate) const fn shape(&self) -> Caret {
        self.shape
    }

    /// Makes room for a screen this size, and empties it.
    ///
    /// Emptied rather than kept: the cells that were here were at other
    /// places, and a grid reshaped around them would draw the old screen
    /// slewed. Obelus redraws the whole of it on the frame after a resize,
    /// which is the frame this is making room for.
    ///
    /// Only when the size really did change, which is the whole of what is
    /// asked here. Measuring happens for reasons that are not a resize --
    /// another font, another size of it -- and those leave the grid the
    /// same shape: the application has nothing new to say, so it sends no
    /// cells, and a page emptied on the way past is a window that goes
    /// blank until the reader presses something. What it needs instead is
    /// exactly what happens: the same cells, drawn again in the new face.
    pub(crate) fn resized(&mut self, columns: u16, rows: u16) {
        if self.columns == columns && self.rows == rows {
            return;
        }
        self.columns = columns;
        self.rows = rows;
        self.cells.clear();
        self.cells
            .resize_with(usize::from(columns) * usize::from(rows), Cell::default);
    }

    /// Takes one update, and says whether it ended a frame.
    pub(crate) fn apply(&mut self, update: Update) -> bool {
        match update {
            Update::Cell { x, y, cell } => {
                if let Some(at) = self.at(x, y) {
                    self.cells[at] = *cell;
                }
                false
            }
            Update::Cleared => {
                for cell in &mut self.cells {
                    *cell = Cell::default();
                }
                false
            }
            Update::Caret(caret) => {
                self.caret = caret;
                false
            }
            Update::CaretShape(shape) => {
                self.shape = shape;
                false
            }
            // Neither is a cell: the window takes both out of the queue
            // before the page is handed anything.
            Update::Mark { id, .. } | Update::Marked { id, .. } => {
                tracing::warn!(id, "a mark reached the page");
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
            };
        };
        let cell = &self.cells[at];
        Look {
            text: cell.symbol(),
            foreground: cell.fg,
            background: cell.bg,
            modifier: cell.modifier,
        }
    }

    /// Where a cell is in the list, or nothing when it is off the grid.
    fn at(&self, x: u16, y: u16) -> Option<usize> {
        (x < self.columns && y < self.rows)
            .then(|| usize::from(y) * usize::from(self.columns) + usize::from(x))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where the caret goes inside a word being spelled is counted in
    /// cells, not in characters.
    ///
    /// Deliberate break: counting the characters before it -- which is
    /// what the input method's own offset is -- puts the caret one column
    /// short for every full-width character typed so far, so the caret
    /// walks backwards through the word as it is spelled.
    #[test]
    fn a_spelling_is_measured_in_cells() {
        let spelling = Spelling {
            text: "\u{4e2d}a".to_string(),
            caret: 2,
        };
        assert_eq!(spelling.columns(), 3);
        let start = Spelling {
            text: "\u{4e2d}a".to_string(),
            caret: 0,
        };
        assert_eq!(start.columns(), 0);
    }
}
