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
use obelus_component::layers::Layer;
use obelus_ui::{
    image::Palette,
    shapes::{Bar, Joined},
};
use ratatui::{
    backend::{Backend, ClearType, WindowSize},
    buffer::Cell,
    layout::{Position, Rect, Size},
    style::Color,
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
    /// Whether things arrive where they are going, or are simply there.
    ///
    /// The same journey again: a setting the application cannot act on
    /// because there is nothing in a terminal for it to mean.
    Animates(bool),
    /// What shape the caret is, and whose it is.
    ///
    /// Which is a fact about the frame it arrives with -- both depend on
    /// where the caret ended up -- so it travels with the frame rather
    /// than beside it.
    CaretIs {
        /// A bar or a block.
        shape: Caret,
        /// What it belongs to, and `None` for the document's own.
        whose: Option<Layer>,
    },
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
    /// These cells are a key in a cap of its own.
    ///
    /// What a terminal draws as a run of cells a shade off the page, and
    /// a window draws as the shape it is. The colours travel with it
    /// because the window has no theme -- see `obelus_ui::shapes`.
    Capped {
        /// The key in it, which is what says the cap is still about these
        /// cells.
        keys: String,
        /// Which cells it is.
        area: Rect,
        /// The ground the key sits on.
        cap: Color,
        /// What is behind the cap, which shows through its corners.
        page: Color,
        /// What draws its outline.
        edge: Color,
    },
    /// A cell that is a switch, and which way it is set.
    Ticked {
        /// Which cell it is.
        area: Rect,
        /// Whether it is set.
        on: bool,
    },
    /// A band of rows, and which row of its list it starts at.
    Scrolled {
        /// Which cells it is.
        area: Rect,
        /// The row of the list its first row holds.
        top: i64,
        /// The bar beside it, where it has one.
        bar: Option<Bar>,
    },
    /// What is behind a pane, in the frame being laid out.
    ///
    /// The whole region, not a diff: it is said in the moment between the
    /// page being drawn and the pane going over it, and what was there a
    /// frame ago is exactly what it must not be -- see
    /// `obelus_ui::shapes::Shapes::behind`.
    Behind {
        /// Which cells it is.
        area: Rect,
        /// Which edge it is joined to, which is also the side it arrives
        /// from.
        joined: Joined,
        /// The pane's own colour, which is where the glass is.
        ground: Color,
        /// Row-major, `area.width` to a row.
        cells: Vec<Cell>,
    },
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
    /// And what is under the pane, where there is one.
    pub(crate) behind: Option<&'a Behind>,
    /// The band of rows that is catching up, and the page as it was
    /// before it moved -- which is the only place the rows that have
    /// scrolled off still exist.
    pub(crate) band: Option<(Rect, &'a Page)>,
    /// And the bar beside it: where it is, and how many rows its mark
    /// still has to come. Rows the bar itself was drawn at, both ends, so
    /// there is nothing between them to round differently.
    pub(crate) bar: Option<(Rect, f32)>,
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
}

impl Behind {
    /// What was at this place, as the painter reads a cell.
    pub(crate) fn look(&self, x: u16, y: u16) -> Option<Look<'_>> {
        self.at(x, y).map(|cell| Look {
            text: cell.symbol(),
            foreground: cell.fg,
            background: cell.bg,
            modifier: cell.modifier,
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
}

/// Where a switch is in the frame being drawn, and how it stands.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Ticked {
    /// Which cell it is.
    pub(crate) area: Rect,
    /// Whether it is set.
    pub(crate) on: bool,
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
        let mut said = String::new();
        for at in 1..self.area.width.saturating_sub(1) {
            said.push_str(page.look(self.area.x.saturating_add(at), self.area.y).text);
        }
        said.trim_end() == self.keys
    }
}

impl obelus_ui::shapes::Shapes for Marking {
    fn ticked(&self, area: Rect, on: bool) {
        let _ = self.updates.send(Update::Ticked { area, on });
    }

    fn scrolled(&self, area: Rect, top: i64, bar: Option<Bar>) {
        let _ = self.updates.send(Update::Scrolled { area, top, bar });
    }

    fn behind(&self, area: Rect, joined: Joined, ground: Color, cells: &[Cell]) {
        let _ = self.updates.send(Update::Behind {
            area,
            joined,
            ground,
            cells: cells.to_vec(),
        });
    }

    fn capped(&self, keys: &str, area: Rect, cap: Color, page: Color, edge: Color) {
        // No wake, the same as a mark's placement: this is said while a
        // frame is being laid out, and the frame's own end wakes the
        // window a moment later.
        let _ = self.updates.send(Update::Capped {
            keys: keys.to_string(),
            area,
            cap,
            page,
            edge,
        });
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

    fn animates(&self, on: bool) {
        let _ = self.updates.send(Update::Animates(on));
        (self.wake)();
    }

    fn caret_is(&self, caret: Caret, whose: Option<Layer>) {
        // No wake: this arrives while a frame is being laid out, and the
        // frame's own end wakes the window a moment later. Waking here as
        // well would be a second wake for one screen.
        let _ = self.updates.send(Update::CaretIs {
            shape: caret,
            whose,
        });
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
#[derive(Clone, Debug)]
pub(crate) struct Page {
    columns: u16,
    rows: u16,
    cells: Vec<Cell>,
    caret: Option<Position>,
    shape: Caret,
    whose: Option<Layer>,
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
            whose: None,
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
                for cell in &mut self.cells {
                    *cell = Cell::default();
                }
                false
            }
            Update::Caret(caret) => {
                self.caret = caret;
                false
            }
            Update::CaretIs { shape, whose } => {
                self.shape = shape;
                self.whose = whose;
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
            Update::Animates(on) => {
                tracing::warn!(on, "a setting reached the page");
                false
            }
            // Nor are these: they are what a run of cells *is* and what
            // was under one, and the window takes both out of the queue
            // with the marks.
            Update::Capped { area, .. }
            | Update::Ticked { area, .. }
            | Update::Behind { area, .. }
            | Update::Scrolled { area, .. } => {
                tracing::warn!(?area, "a cap reached the page");
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
        };
        assert!(cap.still_said(&page));
        // What a view drawn over inside the same frame leaves behind.
        write(&mut page, "    ");
        assert!(!cap.still_said(&page));
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
}
