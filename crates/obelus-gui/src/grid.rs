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
    /// What the page is drawn on, for the margin round the grid.
    ///
    /// The same journey once more. A terminal never asks: there the cells
    /// are the whole of the screen, and it is only a window that has a
    /// strip outside them to put a colour in.
    Ground(Color),
    /// Which two colours mean the reader has hold of something: a run of
    /// characters, and the row their keys are on.
    ///
    /// The same journey again, and a fact about the theme rather than
    /// about a frame. What the window does with it is find the runs in
    /// the cells and draw them as something better than a square -- see
    /// `paint::holdings`.
    Holding {
        /// Behind the characters the reader is holding.
        held: Color,
        /// Behind the row their keys are on.
        row: Color,
    },
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
    /// A cell that is a switch, and which way it is set.
    Ticked {
        /// Which cell it is.
        area: Rect,
        /// Whether it is set.
        on: bool,
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
    /// The mark with the light on it, and the two colours it runs between.
    Sheened {
        /// Which cells it is.
        area: Rect,
        /// What it rests at.
        from: Color,
        /// And what the light carries it to.
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
    /// And which columns are bars, with how the window is showing each.
    pub(crate) barred: &'a [Barred],
    /// And which rows are lines between two things.
    pub(crate) ruled: &'a [Ruled],
    /// And the mark the light runs across, where one is showing.
    pub(crate) sheened: Option<&'a Sheened>,
    /// And which rows begin something new, with nowhere to say so but the
    /// pixel between two rows.
    pub(crate) parted: &'a [Parted],
    /// And what is under the pane, where there is one.
    pub(crate) behind: Option<&'a Behind>,
    /// And under each box with a frame round it, nearest the reader last:
    /// over that pane where both are up.
    pub(crate) cards: &'a [Behind],
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
        .all(|(x, y, corner)| page.look(x, y).text == corner)
    }

    /// Whether this cell is part of a box's frame, and still holds it.
    ///
    /// A cell of the ring holding anything but a line is somebody else's
    /// by now, and is drawn as its own cell.
    pub(crate) fn ring_holds(&self, page: &Page, x: u16, y: u16) -> bool {
        let area = self.area;
        let inside = x >= area.x && x < area.right() && y >= area.y && y < area.bottom();
        let ring = x == area.x || x + 1 == area.right() || y == area.y || y + 1 == area.bottom();
        inside
            && ring
            && page
                .look(x, y)
                .text
                .chars()
                .next()
                .is_some_and(|glyph| ('\u{2500}'..='\u{257f}').contains(&glyph))
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

impl Ticked {
    /// Whether the page still says what this switch was said about.
    ///
    /// The same question `Capped::still_said` asks: the settings are drawn
    /// and then the list of a setting's choices is drawn over them, and a
    /// switch further down the page had already been said -- so a box sat
    /// on the list, on a row with nothing to switch, and the letter it
    /// was over was missing as well, because `letters` leaves a switch's
    /// cell to `ticks`.
    pub(crate) fn still_said(&self, page: &Page) -> bool {
        page.look(self.area.x, self.area.y)
            .text
            .chars()
            .eq(std::iter::once(obelus_ui::tick(self.on)))
    }
}

/// A bar in the frame being drawn, and how the window is showing it.
///
/// The bar is the application's and the two numbers are not: what Obelus
/// said is that a bar is here and this much of it is the mark, and how
/// loudly to draw that is the window's own, the same as a caret's blink.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Barred {
    /// What was said about it.
    pub(crate) bar: Bar,
    /// From nothing at all to all of it: full while it is moving and for
    /// a moment after, and settled back down once the reader has stopped.
    pub(crate) shown: f32,
    /// And how far the pointer's own brightening has come, which is a
    /// second number because the pointer widens it as well: a reader
    /// reaching for a control is about to take hold of it.
    pub(crate) under: f32,
}

impl Barred {
    /// Whether the page still holds the bar this was said about.
    ///
    /// The same question `Capped::still_said` asks, for the same reason: a
    /// view that draws and is then drawn over inside the one frame has
    /// already said its bar, and a bar is drawn from where it was said
    /// rather than from what the page holds there. So the file's own bar
    /// was drawn down the side of every list opened over it -- full height,
    /// across the list, the preview, the foot and the row that is typed in
    /// -- where the reader could take hold of it and scroll a file they
    /// could not see.
    ///
    /// Every row of it, because that is what tells one bar from another:
    /// the list that covered the file has a bar of its own in the same
    /// column, drawn with the same block, and what says the file's is gone
    /// is the rows outside the list that are not blocks any more.
    pub(crate) fn still_said(&self, page: &Page) -> bool {
        let area = self.bar.area;
        // What `obelus_ui::scrollbar` draws a bar with, which is a surface
        // rather than a line -- see the `BAR` it writes.
        (area.top()..area.bottom()).all(|y| page.look(area.x, y).text == "\u{2588}")
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
    /// letters and says nothing either way.
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
        if y != self.area.y || x < self.area.x || x >= self.area.right() {
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

impl Marked {
    /// Whether the cells this mark was drawn at are still nobody's.
    ///
    /// The same question again, asked the other way round: a view that has
    /// a picture drawn writes nothing in the cells it goes in -- that is
    /// what `Images::draw` answering `true` means -- so there is no glyph
    /// of its own to look for, and anything in them at all is somebody
    /// else's writing. The card of every key, opened over the agents page,
    /// had two agents' marks sitting on its frame.
    pub(crate) fn still_said(&self, page: &Page) -> bool {
        let slot = obelus_ui::image::SLOT;
        (self.y..self.y.saturating_add(slot.height)).all(|y| {
            (self.x..self.x.saturating_add(slot.width))
                .all(|x| page.look(x, y).text.trim().is_empty())
        })
    }
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

    fn barred(&self, bar: Bar) {
        let _ = self.updates.send(Update::Barred { bar });
    }

    fn behind(&self, area: Rect, joined: Joined, ground: Color, cells: &[Cell]) {
        let _ = self.updates.send(Update::Behind {
            area,
            joined,
            ground,
            cells: cells.to_vec(),
        });
    }

    fn parted(&self, area: Rect) {
        let _ = self.updates.send(Update::Parted { area });
    }

    fn sheened(&self, area: Rect, from: Color, to: Color) {
        let _ = self.updates.send(Update::Sheened { area, from, to });
    }

    fn ruled(&self, area: Rect) {
        let _ = self.updates.send(Update::Ruled { area });
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

    fn drawn_on(&self, ground: Color) {
        if self.updates.send(Update::Ground(ground)).is_ok() {
            (self.wake)();
        }
    }

    fn holding(&self, held: Color, row: Color) {
        if self.updates.send(Update::Holding { held, row }).is_ok() {
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
    /// What an input method said, as a spelling -- or nothing, where it
    /// said the spelling is over.
    ///
    /// `caret` is as winit hands it on: a run of bytes, or nothing for a
    /// caret the input method would rather not show. A run whose two ends
    /// meet is a caret; one whose ends differ is text the input method is
    /// highlighting, and the caret is its *far* end, which is where typing
    /// carries on from. fcitx5 says `(0, 6)` for `ni hao` -- the whole of
    /// it, ending where the next letter goes -- and a caret taken from the
    /// near end sat at the front of the pinyin however much was typed.
    ///
    /// In characters rather than bytes: the window counts cells, and a
    /// byte offset into what is being spelled is not one.
    pub(crate) fn new(text: String, caret: Option<(usize, usize)>) -> Option<Self> {
        if text.is_empty() {
            return None;
        }
        let caret = match caret {
            Some((_, end)) => text.get(..end).map_or(0, |before| before.chars().count()),
            None => text.chars().count(),
        };
        Some(Self { text, caret })
    }

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
            | Update::Ruled { area }
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
        };
        assert!(switch(0, true).still_said(&page), "the settings' own");
        assert!(!switch(1, true).still_said(&page), "and one a list covered");
        assert!(
            !switch(0, false).still_said(&page),
            "nor one drawn unset where the page says it is set"
        );
    }

    /// And a mark is drawn only where the cells are still nobody's.
    ///
    /// Deliberate break: answer `true`. The card of every key, opened over
    /// the agents page, then has two agents' marks sitting on its frame.
    /// Or ask about the first cell alone: a mark is two cells wide, and a
    /// card whose edge lands in the second of them is a picture over a
    /// line.
    #[test]
    fn a_mark_is_drawn_only_where_the_cells_are_still_nobody_s() {
        // A row the card's frame reached the third cell of, and a row it
        // did not reach at all.
        let page = written(&["  \u{2502}", "   "]);
        let mark = |x: u16, y: u16| Marked {
            id: "claude".to_string(),
            focused: false,
            x,
            y,
        };
        assert!(mark(0, 1).still_said(&page), "two cells nobody wrote in");
        assert!(mark(0, 0).still_said(&page), "and the two before the line");
        assert!(
            !mark(1, 0).still_said(&page),
            "but not where the line is the second of them"
        );
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
        let page = inked(&[
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
        };
        assert!(
            mark(2).still_said(&page),
            "the plate's own rows, gaps and all"
        );
        assert!(
            !mark(3).still_said(&page),
            "a row of the list is inside the plate and is not the plate"
        );
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

    /// And a bar is a bar only where its cells still say so.
    ///
    /// Deliberate break: answer `true`. The file's own bar is then drawn
    /// down the side of the list opened over it -- past the list, the
    /// preview and the foot, widening under the pointer -- on a list of
    /// nine rows with nothing to scroll.
    #[test]
    fn a_bar_is_a_bar_only_where_its_cells_still_say_so() {
        // A column the list drew two rows of and the file drew all four:
        // the blank and the rule are the list's foot, over the file's bar.
        let page = written(&["\u{2588}", "\u{2588}", " ", "\u{2500}"]);
        let showing = |y: u16, height: u16| Barred {
            bar: Bar {
                area: Rect {
                    x: 0,
                    y,
                    width: 1,
                    height,
                },
                mark: 0,
                thumb: 1,
            },
            shown: 1.0,
            under: 0.0,
        };
        assert!(
            showing(0, 2).still_said(&page),
            "the list's own, drawn last"
        );
        assert!(!showing(0, 4).still_said(&page), "and the file's, under it");
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

    /// The caret in a spelling is at the far end of what the input method
    /// named, which is where the next letter goes.
    ///
    /// Deliberate break: take the near end, `Some((start, _))`, as this
    /// once did. The first case below is what fcitx5 actually sent, read
    /// off the log, and with the near end the caret sat at the front of
    /// the pinyin however much of it had been typed.
    #[test]
    fn a_spelling_carries_on_from_the_far_end_of_what_is_named() {
        let caret = |text: &str, at| Spelling::new(text.to_string(), at).map(|it| it.caret);
        assert_eq!(caret("ni hao", Some((0, 6))), Some(6), "fcitx5's whole run");
        assert_eq!(caret("ni hao", Some((2, 2))), Some(2), "a caret of its own");
        assert_eq!(caret("ni hao", None), Some(6), "a caret not shown");
        // Bytes in, characters out: two characters of Chinese are six bytes.
        assert_eq!(caret("\u{4f60}\u{597d}hao", Some((0, 6))), Some(2));
        assert_eq!(caret("", Some((0, 0))), None, "nothing is being spelled");
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
}
