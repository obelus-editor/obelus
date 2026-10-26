//! What Obelus says to whatever draws it, and the parts of Obelus that say
//! it: a ratatui backend whose frames are [`Update`]s, and the three things
//! a view tells a front end beside its cells -- what the settings say about
//! the window ([`Telling`]), what a run of cells *is* ([`Marking`]), and the
//! marks it draws itself.
//!
//! **Here and not in the window, because the window may be somewhere
//! else.** `obg` puts these on a channel to its own window; Obelus on a
//! machine at the other end of a connection puts them on the connection, and
//! the window there reads them off it. Both go through [`Showing`], which is
//! the one thing that knows which.
//!
//! **A picture crosses as its bytes, and the window decodes it.** What the
//! application knows is which file and how it is laid; how large the
//! window's screen is, and so how small the picture can be made, is the
//! window's. So [`Telling::hang`] reads the file -- on a thread of its own,
//! one picture after another in the order they were asked for -- and hands
//! on what it read, and decoding is left to whoever draws (`Update::Picture`).

use std::sync::{
    Arc,
    atomic::{AtomicU16, AtomicU32, Ordering},
};

mod input;
pub mod link;
pub mod replay;
pub mod wire;

pub use input::Input;
use obelus_app::app::Caret;
use obelus_component::layers::Layer;
use obelus_ui::{
    image::Palette,
    shapes::{Bar, Joined, Pair, Stroke},
};
use ratatui::{
    backend::{Backend, ClearType, WindowSize},
    buffer::Cell,
    layout::{Position, Rect, Size},
    style::Color,
};

/// Where what Obelus says goes: a window's channel, or a connection to one.
pub trait Showing: Send + Sync {
    /// Puts one update on its way, or says there is nobody to show it to.
    ///
    /// # Errors
    ///
    /// Where whatever drew Obelus has gone.
    fn show(&self, update: Update) -> Result<(), Closed>;

    /// Says that what has been put on its way is to be looked at now.
    ///
    /// Apart from [`Self::show`] because sending alone does not: a window's
    /// loop is asleep in the platform's own wait, and the one way to reach
    /// a thread parked there is to post an event to it.
    fn wake(&self);
}

/// A picture to lay under the page: the file's bytes, and what the theme
/// said about it.
///
/// The theme's whole word, file and all, because which file it is and when
/// it was written is what says whether it is another picture or the same
/// one laid differently -- see the window's decoder.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Hanging {
    /// What the theme said.
    pub wallpaper: obelus_theme::Wallpaper,
    /// What the file held, or nothing where it would not read -- which
    /// decodes to no picture, the way a picture that will not decode does.
    pub bytes: Vec<u8>,
}

/// One change to what is on the screen.
///
/// Ordered, and applied in the order it was sent: a cell written and then
/// written again is two updates, and the second one is the one that counts.
///
/// `P` is what a picture under the page is, which is its bytes on the way
/// to whoever draws ([`Hanging`]) and whatever they decode it into after.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Update<P = Option<Hanging>> {
    /// A cell, at a place.
    Cell {
        /// The column.
        x: u16,
        /// The row.
        y: u16,
        /// What is in it.
        #[serde(with = "wire::boxed_cell")]
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
    /// Whether the title bar holds a tab for each open document.
    ///
    /// The same journey again, and the window's alone: it decides how tall
    /// the bar is, and so where the page starts.
    BarTabs(bool),
    /// How much of the picture under the page shows, as a share of what
    /// the theme lets through.
    ///
    /// The same journey again, and apart from the picture: a share is not
    /// a picture to read and decode again.
    PictureStrength(usize),
    /// What the page is drawn on, for the margin round the grid.
    ///
    /// The same journey once more. A terminal never asks: there the cells
    /// are the whole of the screen, and it is only a window that has a
    /// strip outside them to put a colour in.
    Ground(#[serde(with = "wire::ink")] Color),
    /// Which two colours mean the reader has hold of something: a run of
    /// characters, and the row their keys are on.
    ///
    /// The same journey again, and a fact about the theme rather than
    /// about a frame. What the window does with it is find the runs in
    /// the cells and draw them as something better than a square -- see
    /// `paint::holdings`.
    Holding {
        /// Behind the characters the reader is holding.
        #[serde(with = "wire::ink")]
        held: Color,
        /// Behind the row their keys are on.
        #[serde(with = "wire::ink")]
        row: Color,
    },
    /// The picture under the page, or that there is none.
    ///
    /// The same journey as the ground, a moment later: the application
    /// says which file, and it arrives here once a thread has read it.
    Picture(P),
    /// What is open, for the tabs along the title bar -- see
    /// `Drawing::tabs`.
    ///
    /// Down the same wire as the frame it changed with, so the tab that
    /// says what is being read changes with the page that shows it.
    Tabs(Vec<obelus_app::app::Tab>),
    /// The theme is changing, and what is on the screen is in the one it
    /// was in -- see `Drawing::recoloured`.
    ///
    /// The same journey as the ground, and ahead of it: what the window
    /// keeps of the screen going is drawn in the ground it has until then.
    Recoloured,
    /// What colour the caret is drawn in.
    ///
    /// The theme's, said with the ground and at the same moments, and kept
    /// by the page rather than the window because it is the page that is
    /// kept when the theme goes -- see `Drawing::recoloured` -- and the
    /// screen going is in the caret it had.
    CaretInk(#[serde(with = "wire::ink")] Color),
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
        /// Whether a character typed now goes into any text.
        typing: bool,
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
        #[serde(with = "wire::palette")]
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
        #[serde(with = "wire::ink")]
        cap: Color,
        /// What is behind the cap, which shows through its corners.
        #[serde(with = "wire::ink")]
        page: Color,
        /// What draws its outline.
        #[serde(with = "wire::ink")]
        edge: Color,
    },
    /// A column that is a bar, and how much of it is the mark.
    ///
    /// What a terminal draws as a column of full blocks with a brighter
    /// run in it, and a window draws as the shape it is. No colours: they
    /// are the foreground and background of those very cells, which the
    /// view has already written -- see `obelus_ui::shapes`.
    Barred {
        /// Where it is, how much of it is the mark and where the mark
        /// starts.
        bar: Bar,
    },
    /// A run of rows in a one-cell column that is marked.
    ///
    /// What a terminal draws as half a block per row, and a window draws
    /// as one shape however many rows it covers. No colours, for a bar's
    /// reason -- see `obelus_ui::shapes`.
    Stroked {
        /// Which rows, which edge of the column, and what it is about.
        stroke: Stroke,
    },
    /// A cell that is a switch, and which way it is set.
    Ticked {
        /// Which cell it is.
        area: Rect,
        /// Whether it is set.
        on: bool,
    },
    /// A cell that is the mark that turns while something is happening.
    Spun {
        /// Which cell it is.
        area: Rect,
    },
    /// A row that is a line between two things.
    Ruled {
        /// Which cells it is.
        area: Rect,
    },
    /// A row that begins something new, with no row between it and what
    /// it is parted from.
    Parted {
        /// The run the line covers.
        area: Rect,
    },
    /// Half of the pair of brackets the cursor is on or inside.
    Paired {
        /// Which cell it is.
        area: Rect,
        /// Which pair, as numbers to compare -- see
        /// `obelus_ui::shapes::Pair`.
        pair: Pair,
    },
    /// What a search found, in the page that is its preview.
    Found {
        /// Which cells, on one row.
        area: Rect,
        /// Which match, as numbers to compare -- see
        /// `obelus_ui::shapes::Found`.
        found: obelus_ui::shapes::Found,
    },
    /// The words of a link, on one row.
    Linked {
        /// Which cells they are.
        area: Rect,
    },
    /// What an input method is still spelling, a cell or a character at a
    /// time.
    Spelled {
        /// Which cells they are.
        area: Rect,
    },
    /// The mark with the light on it, and the two colours it runs between.
    Sheened {
        /// Which cells it is.
        area: Rect,
        /// What it rests at.
        #[serde(with = "wire::ink")]
        from: Color,
        /// And what the light carries it to.
        #[serde(with = "wire::ink")]
        to: Color,
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
        #[serde(with = "wire::ink")]
        ground: Color,
        /// Row-major, `area.width` to a row.
        #[serde(with = "wire::cells")]
        cells: Vec<Cell>,
    },
}

impl<P> Update<P> {
    /// The same update for a front end whose pictures are something else,
    /// or the picture it is, which is the one update that is not the same.
    ///
    /// Every arm by name, so that a new kind of update is a decision about
    /// whether it is a picture rather than a wildcard that says not.
    ///
    /// # Errors
    ///
    /// The picture, where this is one.
    pub fn apart<Q>(self) -> Result<Update<Q>, P> {
        Ok(match self {
            Self::Picture(picture) => return Err(picture),
            Self::Cell { x, y, cell } => Update::Cell { x, y, cell },
            Self::Cleared => Update::Cleared,
            Self::Caret(said) => Update::Caret(said),
            Self::Frame => Update::Frame,
            Self::TextSize(said) => Update::TextSize(said),
            Self::Fonts(said) => Update::Fonts(said),
            Self::Animates(said) => Update::Animates(said),
            Self::BarTabs(said) => Update::BarTabs(said),
            Self::PictureStrength(said) => Update::PictureStrength(said),
            Self::Ground(said) => Update::Ground(said),
            Self::Holding { held, row } => Update::Holding { held, row },
            Self::Tabs(said) => Update::Tabs(said),
            Self::Recoloured => Update::Recoloured,
            Self::CaretInk(said) => Update::CaretInk(said),
            Self::CaretIs {
                shape,
                whose,
                typing,
            } => Update::CaretIs {
                shape,
                whose,
                typing,
            },
            Self::Mark {
                id,
                focused,
                svg,
                palette,
            } => Update::Mark {
                id,
                focused,
                svg,
                palette,
            },
            Self::Marked { id, focused, x, y } => Update::Marked { id, focused, x, y },
            Self::Capped {
                keys,
                area,
                cap,
                page,
                edge,
            } => Update::Capped {
                keys,
                area,
                cap,
                page,
                edge,
            },
            Self::Barred { bar } => Update::Barred { bar },
            Self::Stroked { stroke } => Update::Stroked { stroke },
            Self::Ticked { area, on } => Update::Ticked { area, on },
            Self::Spun { area } => Update::Spun { area },
            Self::Ruled { area } => Update::Ruled { area },
            Self::Parted { area } => Update::Parted { area },
            Self::Paired { area, pair } => Update::Paired { area, pair },
            Self::Found { area, found } => Update::Found { area, found },
            Self::Linked { area } => Update::Linked { area },
            Self::Spelled { area } => Update::Spelled { area },
            Self::Sheened { area, from, to } => Update::Sheened { area, from, to },
            Self::Scrolled { area, top, bar } => Update::Scrolled { area, top, bar },
            Self::Behind {
                area,
                joined,
                ground,
                cells,
            } => Update::Behind {
                area,
                joined,
                ground,
                cells,
            },
        })
    }
}

/// The window's end of what a view says about marks.
///
/// Down the same channel as the cells, because a mark is part of the frame
/// being laid out: one that arrived after the frame it belongs to would be
/// a picture drawn over the screen that replaced it.
#[derive(Clone)]
pub struct Marking {
    showing: Arc<dyn Showing>,
}

impl std::fmt::Debug for Marking {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Marking")
    }
}

impl Marking {
    /// Says where a view's marks go.
    pub fn new(showing: Arc<dyn Showing>) -> Self {
        Self { showing }
    }
}

impl obelus_ui::shapes::Shapes for Marking {
    fn ticked(&self, area: Rect, on: bool) {
        let _ = self.showing.show(Update::Ticked { area, on });
    }

    fn spun(&self, area: Rect) {
        let _ = self.showing.show(Update::Spun { area });
    }

    fn scrolled(&self, area: Rect, top: i64, bar: Option<Bar>) {
        let _ = self.showing.show(Update::Scrolled { area, top, bar });
    }

    fn barred(&self, bar: Bar) {
        let _ = self.showing.show(Update::Barred { bar });
    }

    fn stroked(&self, stroke: Stroke) {
        let _ = self.showing.show(Update::Stroked { stroke });
    }

    fn behind(&self, area: Rect, joined: Joined, ground: Color, cells: &[Cell]) {
        let _ = self.showing.show(Update::Behind {
            area,
            joined,
            ground,
            cells: cells.to_vec(),
        });
    }

    fn parted(&self, area: Rect) {
        let _ = self.showing.show(Update::Parted { area });
    }

    fn sheened(&self, area: Rect, from: Color, to: Color) {
        let _ = self.showing.show(Update::Sheened { area, from, to });
    }

    fn ruled(&self, area: Rect) {
        let _ = self.showing.show(Update::Ruled { area });
    }

    fn linked(&self, area: Rect) {
        let _ = self.showing.show(Update::Linked { area });
    }

    fn spelled(&self, area: Rect) {
        let _ = self.showing.show(Update::Spelled { area });
    }

    fn paired(&self, area: Rect, pair: Pair) {
        let _ = self.showing.show(Update::Paired { area, pair });
    }

    fn found(&self, area: Rect, found: obelus_ui::shapes::Found) {
        let _ = self.showing.show(Update::Found { area, found });
    }

    fn capped(&self, keys: &str, area: Rect, cap: Color, page: Color, edge: Color) {
        // No wake, the same as a mark's placement: this is said while a
        // frame is being laid out, and the frame's own end wakes the
        // window a moment later.
        let _ = self.showing.show(Update::Capped {
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
        let _ = self.showing.show(Update::Mark {
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
        let _ = self.showing.show(Update::Marked {
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
pub struct Telling {
    showing: Arc<dyn Showing>,
    /// What reads the picture under the page, a picture at a time.
    reading: std::sync::mpsc::Sender<Option<obelus_theme::Wallpaper>>,
}

impl std::fmt::Debug for Telling {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Telling")
    }
}

impl Telling {
    /// Says where to send what the application works out about the window.
    #[must_use]
    pub fn new(showing: Arc<dyn Showing>) -> Self {
        let (reading, asked) = std::sync::mpsc::channel::<Option<obelus_theme::Wallpaper>>();
        let shown = Arc::clone(&showing);
        // One thread, in the order the pictures were asked for: the window
        // takes the newest by the order they arrive in, so two read side by
        // side could hand it the older one last. It ends with the last of
        // what asks it, which is the application going.
        let started = std::thread::Builder::new()
            .name("obelus wallpaper reader".to_string())
            .spawn(move || {
                while let Ok(wallpaper) = asked.recv() {
                    let hanging = wallpaper.map(|wallpaper| {
                        let bytes = std::fs::read(&wallpaper.file).unwrap_or_else(|error| {
                            tracing::warn!(%error, file = %wallpaper.file.display(), "a wallpaper that would not read");
                            Vec::new()
                        });
                        Hanging { wallpaper, bytes }
                    });
                    if shown.show(Update::Picture(hanging)).is_err() {
                        return;
                    }
                    shown.wake();
                }
            });
        if let Err(error) = started {
            tracing::warn!(%error, "no thread to read the wallpaper on, so there is none");
        }
        Self { showing, reading }
    }
}

impl obelus_app::app::Drawing for Telling {
    fn text_size(&self, points: usize) {
        // A window that has gone is a send that fails, and the application
        // is about to find that out for itself on its next frame.
        if self.showing.show(Update::TextSize(points)).is_ok() {
            self.showing.wake();
        }
    }

    fn use_fonts(&self, names: &[String]) {
        if self.showing.show(Update::Fonts(names.to_vec())).is_ok() {
            self.showing.wake();
        }
    }

    fn drawn_on(&self, ground: Color) {
        if self.showing.show(Update::Ground(ground)).is_ok() {
            self.showing.wake();
        }
    }

    fn holding(&self, held: Color, row: Color) {
        if self.showing.show(Update::Holding { held, row }).is_ok() {
            self.showing.wake();
        }
    }

    fn animates(&self, on: bool) {
        let _ = self.showing.show(Update::Animates(on));
        self.showing.wake();
    }

    fn bar_tabs(&self, on: bool) {
        let _ = self.showing.show(Update::BarTabs(on));
        self.showing.wake();
    }

    fn picture_strength(&self, share: usize) {
        if self.showing.show(Update::PictureStrength(share)).is_ok() {
            self.showing.wake();
        }
    }

    fn hang(&self, wallpaper: Option<&obelus_theme::Wallpaper>) {
        // Through the reader even for none, which has nothing to read: a
        // none said at once would overtake a picture still being read, and
        // the picture would arrive after it.
        let _ = self.reading.send(wallpaper.cloned());
    }

    fn recoloured(&self) {
        if self.showing.show(Update::Recoloured).is_ok() {
            self.showing.wake();
        }
    }

    fn tabs(&self, tabs: &[obelus_app::app::Tab]) {
        // No wake, the same as the caret's shape: said while a frame is
        // being laid out, whose own end wakes the window.
        let _ = self.showing.show(Update::Tabs(tabs.to_vec()));
    }

    fn caret_in(&self, colour: Color) {
        if self.showing.show(Update::CaretInk(colour)).is_ok() {
            self.showing.wake();
        }
    }

    fn caret_is(&self, caret: Caret, whose: Option<Layer>, typing: bool) {
        // No wake: this arrives while a frame is being laid out, and the
        // frame's own end wakes the window a moment later. Waking here as
        // well would be a second wake for one screen.
        let _ = self.showing.show(Update::CaretIs {
            shape: caret,
            whose,
            typing,
        });
    }
}

/// The window is gone, so there is nowhere for a frame to go.
///
/// Which ends Obelus's loop, by the same route a terminal whose input thread
/// died ends it: the error travels up out of `app::run` and the thread
/// finishes.
#[derive(Clone, Copy, Debug)]
pub struct Closed;

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
pub struct Measured {
    columns: AtomicU16,
    rows: AtomicU16,
    /// The window, in pixels, packed as width in the high half and height
    /// in the low one so that the two are read as one fact.
    pixels: AtomicU32,
}

impl Measured {
    /// Says how big the screen has become.
    pub fn resized(&self, columns: u16, rows: u16, width: u32, height: u32) {
        self.columns.store(columns, Ordering::Relaxed);
        self.rows.store(rows, Ordering::Relaxed);
        // Clamped rather than wrapped: a window wider than 65535 pixels is
        // not a thing to be wrong about quietly.
        let packed = (width.min(u32::from(u16::MAX)) << 16) | height.min(u32::from(u16::MAX));
        self.pixels.store(packed, Ordering::Relaxed);
    }

    /// How many columns and rows there are room for.
    #[must_use]
    pub fn size(&self) -> Size {
        Size::new(
            self.columns.load(Ordering::Relaxed),
            self.rows.load(Ordering::Relaxed),
        )
    }

    /// And how many pixels that is.
    #[must_use]
    pub fn pixels(&self) -> Size {
        let packed = self.pixels.load(Ordering::Relaxed);
        // Both halves were clamped to a `u16` on the way in.
        Size::new((packed >> 16) as u16, (packed & 0xffff) as u16)
    }
}

/// Where Obelus's frames go.
pub struct Cells {
    showing: Arc<dyn Showing>,
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
    #[must_use]
    pub fn new(showing: Arc<dyn Showing>, measured: Arc<Measured>) -> Self {
        Self {
            showing,
            measured,
            caret: Position::ORIGIN,
            shown: false,
        }
    }

    /// Puts one update on its way, or says the window has gone.
    fn send(&self, update: Update) -> Result<(), Closed> {
        self.showing.show(update)
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
        self.showing.wake();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use crossterm::event::KeyEvent;
    use obelus_app::app::Drawing as _;

    use super::*;

    /// What a front end was shown, kept.
    #[derive(Default)]
    struct Kept(Mutex<Vec<Update>>);

    impl Showing for Kept {
        fn show(&self, update: Update) -> Result<(), Closed> {
            self.0.lock().expect("the kept updates").push(update);
            Ok(())
        }

        fn wake(&self) {}
    }

    /// A cell with something in every field that crosses.
    fn styled(symbol: &str) -> Cell {
        let mut cell = Cell::default();
        cell.set_symbol(symbol);
        cell.fg = Color::Rgb(1, 2, 3);
        cell.bg = Color::Indexed(7);
        cell.underline_color = Color::LightMagenta;
        cell.modifier = ratatui::style::Modifier::BOLD | ratatui::style::Modifier::ITALIC;
        cell.diff_option = ratatui::buffer::CellDiffOption::ForcedWidth(
            std::num::NonZeroU16::MIN.saturating_add(1),
        );
        cell
    }

    /// One of every update, as it crosses.
    fn every_update() -> Vec<Update> {
        let area = Rect::new(1, 2, 3, 4);
        let bar = Bar {
            area,
            mark: 1,
            thumb: 2,
        };
        vec![
            Update::Cell {
                x: 3,
                y: 4,
                cell: Box::new(styled("中")),
            },
            Update::Cleared,
            Update::Caret(Some(Position::new(5, 6))),
            Update::Frame,
            Update::TextSize(14),
            Update::Fonts(vec!["JetBrains Mono".to_string()]),
            Update::Animates(true),
            Update::BarTabs(false),
            Update::PictureStrength(60),
            Update::Ground(Color::Rgb(9, 8, 7)),
            Update::Holding {
                held: Color::Blue,
                row: Color::Reset,
            },
            Update::Picture(Some(Hanging {
                wallpaper: obelus_theme::Wallpaper {
                    file: std::path::PathBuf::from("/themes/dusk.jpg"),
                    opacity: 80,
                    fit: obelus_theme::wallpaper::Fit::Cover,
                    anchor: obelus_theme::wallpaper::Anchor::default(),
                    modified: None,
                },
                bytes: vec![0xff, 0xd8],
            })),
            Update::Tabs(vec![obelus_app::app::Tab {
                document: obelus_buffer::DocumentId::new(2),
                name: "main.rs".to_string(),
                icon: Some('r'),
                reading: true,
                unwritten: false,
            }]),
            Update::Recoloured,
            Update::CaretInk(Color::White),
            Update::CaretIs {
                shape: Caret::Bar,
                whose: Some(Layer::Settings),
                typing: true,
            },
            Update::Mark {
                id: "claude".to_string(),
                focused: true,
                svg: "<svg/>".to_string(),
                palette: Palette {
                    ink: Color::Black,
                    paper: Color::Rgb(250, 250, 250),
                    selected: Color::Indexed(4),
                },
            },
            Update::Marked {
                id: "claude".to_string(),
                focused: false,
                x: 7,
                y: 8,
            },
            Update::Capped {
                keys: "ctrl+p".to_string(),
                area,
                cap: Color::DarkGray,
                page: Color::Black,
                edge: Color::Gray,
            },
            Update::Barred { bar },
            Update::Stroked {
                stroke: Stroke {
                    area,
                    side: obelus_ui::shapes::Side::Left,
                    about: obelus_ui::shapes::About::Rows,
                },
            },
            Update::Ticked { area, on: true },
            Update::Spun { area },
            Update::Ruled { area },
            Update::Parted { area },
            Update::Paired {
                area,
                pair: Pair {
                    document: 1,
                    version: 2,
                    open: 3,
                },
            },
            Update::Found {
                area,
                found: obelus_ui::shapes::Found {
                    page: 1,
                    row: 2,
                    start: 3,
                },
            },
            Update::Linked { area },
            Update::Spelled { area },
            Update::Sheened {
                area,
                from: Color::Green,
                to: Color::LightGreen,
            },
            Update::Scrolled {
                area,
                top: -1,
                bar: Some(bar),
            },
            Update::Behind {
                area,
                joined: Joined::Below,
                ground: Color::Rgb(20, 20, 20),
                cells: vec![styled("a"), Cell::default()],
            },
        ]
    }

    /// One of every input, as it crosses.
    fn every_input() -> Vec<Input> {
        use crossterm::event::{KeyCode, KeyModifiers};
        let key = KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL);
        let document = obelus_buffer::DocumentId::new(1);
        vec![
            Input::Key(key),
            Input::Command(key),
            Input::Resize {
                columns: 120,
                rows: 40,
                width: 1440,
                height: 960,
            },
            Input::Fonts {
                here: vec!["Menlo".to_string()],
                otherwise: None,
            },
            Input::Closed,
            Input::Shifted(true),
            Input::Scroll {
                rows: -3,
                at: Some((1, 2)),
            },
            Input::Pointer {
                kind: obelus_app::event::Pointer::Dragged,
                x: 4,
                y: 5,
            },
            Input::Chose(document),
            Input::Shut(document),
            Input::Placed {
                document,
                beside: obelus_buffer::DocumentId::new(3),
                after: true,
            },
            Input::Paste("words".to_string()),
            Input::Spelling("にほ".to_string(), Some(1)),
            Input::Dropped(std::path::PathBuf::from("/tmp/a.png")),
            Input::Summoned(Some("token".to_string())),
        ]
    }

    /// Every update crosses as itself.
    ///
    /// Broken deliberately by writing a colour given in red, green and blue
    /// as `Reset` (in `Ink::from`): the cells came back colourless.
    #[test]
    fn every_update_crosses_as_itself() {
        for update in every_update() {
            let frame = obelus_wire::framed(&update).expect("framed");
            let back: Update = obelus_wire::unframed(&frame[4..]).expect("read");
            assert_eq!(back, update);
        }
    }

    /// Every input crosses as itself.
    ///
    /// Broken deliberately by leaving where the wheel turned out of the
    /// bytes (`serde(skip)` on `Scroll::at`): it arrived over no cell.
    #[test]
    fn every_input_crosses_as_itself() {
        for input in every_input() {
            let frame = obelus_wire::framed(&input).expect("framed");
            let back: Input = obelus_wire::unframed(&frame[4..]).expect("read");
            assert_eq!(back, input);
        }
    }

    /// What crosses, as bytes: what a window and an Obelus of another build
    /// read. The bytes move with the shape, and moving them is the moment
    /// to move `obelus_wire::WIRE_VERSION` too. `UPDATE_FIXTURES=1` writes
    /// them again.
    ///
    /// Broken deliberately by putting `Cleared` before `Cell` in `Update`:
    /// every update's first byte moved.
    #[test]
    fn what_crosses_is_these_bytes() {
        let mut bytes = Vec::new();
        for update in every_update() {
            bytes.extend(obelus_wire::framed(&update).expect("framed"));
        }
        for input in every_input() {
            bytes.extend(obelus_wire::framed(&input).expect("framed"));
        }
        bytes.extend(obelus_wire::framed(&link::Challenge([3; 32])).expect("framed"));
        bytes.extend(
            obelus_wire::framed(&link::Knock {
                signed: [4; 32],
                front: link::Front::Window {
                    screen: [2560, 1440],
                    command_is_control: false,
                },
                size: [120, 40, 1440, 960],
                path: None,
                window: None,
            })
            .expect("framed"),
        );
        for answered in [
            link::Answered::Open {
                window: "w1".to_string(),
                again: false,
            },
            link::Answered::Refused("no".to_string()),
        ] {
            bytes.extend(obelus_wire::framed(&answered).expect("framed"));
        }
        for down in [
            link::Down::Update(Update::Frame),
            link::Down::Copy("c".to_string()),
            link::Down::Paste(1),
            link::Down::Picture(2),
            link::Down::HoldsAPicture(3),
            link::Down::Open("u".to_string()),
            link::Down::Finished,
        ] {
            bytes.extend(obelus_wire::framed(&down).expect("framed"));
        }
        for up in [
            link::Up::Input(Input::Closed),
            link::Up::Pasted {
                number: 1,
                words: None,
            },
            link::Up::Pictured {
                number: 2,
                picture: Some(("image/png".to_string(), vec![1])),
            },
            link::Up::Holds {
                number: 3,
                picture: true,
            },
        ] {
            bytes.extend(obelus_wire::framed(&up).expect("framed"));
        }
        for told in [
            link::ToChild::Open {
                front: link::Front::Terminal {
                    pictures: None,
                    keyboard: false,
                },
                size: [80, 24, 0, 0],
                path: Some("/p".to_string()),
            },
            link::ToChild::Up(link::Up::Input(Input::Closed)),
            link::ToChild::Attached {
                size: [100, 30, 0, 0],
            },
            link::ToChild::Detached,
            link::ToChild::Leave,
        ] {
            bytes.extend(obelus_wire::framed(&told).expect("framed"));
        }
        let written: String = bytes
            .chunks(32)
            .map(|line| {
                let mut hex: String = line.iter().map(|byte| format!("{byte:02x}")).collect();
                hex.push('\n');
                hex
            })
            .collect();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/frames.hex");
        if std::env::var_os("UPDATE_FIXTURES").is_some() {
            std::fs::create_dir_all(path.parent().expect("a directory")).expect("the directory");
            std::fs::write(&path, &written).expect("the bytes");
        }
        let kept = std::fs::read_to_string(&path).expect("the bytes as they were");
        assert_eq!(
            written, kept,
            "what crosses moved: move WIRE_VERSION, and write these again"
        );
    }

    /// A resize takes the size with it, and it is written down at the far
    /// end before the loop is told to look.
    ///
    /// Broken deliberately by `into_event` answering `Event::Resize`
    /// without writing it down: the far end's backend still said nothing.
    #[test]
    fn a_resize_takes_the_size_with_it() {
        let here = Measured::default();
        here.resized(80, 24, 800, 480);
        let input = Input::of(obelus_app::event::Event::Resize, &here).expect("a resize");
        let there = Measured::default();
        let event = input.into_event(&there);
        assert!(matches!(event, obelus_app::event::Event::Resize));
        assert_eq!(there.size(), Size::new(80, 24));
        assert_eq!(there.pixels(), Size::new(800, 480));
    }

    /// A picture under the page is handed on as what its file holds, and
    /// the pictures -- and the none between them -- in the order they were
    /// asked for.
    ///
    /// Broken deliberately by handing on nothing read (`bytes` left
    /// empty): the window had no picture to decode.
    #[test]
    fn a_picture_is_handed_on_as_its_bytes_in_the_order_asked() {
        let directory = std::env::temp_dir().join(format!("obelus-front-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("somewhere to write");
        let picture = |name: &str, bytes: &[u8]| {
            let file = directory.join(name);
            std::fs::write(&file, bytes).expect("the picture");
            obelus_theme::Wallpaper {
                file,
                opacity: 80,
                fit: obelus_theme::wallpaper::Fit::Cover,
                anchor: obelus_theme::wallpaper::Anchor::default(),
                modified: None,
            }
        };
        let first = picture("first.png", b"first");
        let second = picture("second.png", b"second");

        let kept = Arc::new(Kept::default());
        let telling = Telling::new(Arc::clone(&kept) as Arc<dyn Showing>);
        telling.hang(Some(&first));
        telling.hang(None);
        telling.hang(Some(&second));

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let shown = loop {
            let shown: Vec<Option<Vec<u8>>> = kept
                .0
                .lock()
                .expect("the kept updates")
                .iter()
                .filter_map(|update| match update {
                    Update::Picture(hanging) => {
                        Some(hanging.as_ref().map(|hanging| hanging.bytes.clone()))
                    }
                    _ => None,
                })
                .collect();
            if shown.len() == 3 || std::time::Instant::now() > deadline {
                break shown;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        assert_eq!(
            shown,
            vec![Some(b"first".to_vec()), None, Some(b"second".to_vec())]
        );
        let _ = std::fs::remove_dir_all(&directory);
    }
}
