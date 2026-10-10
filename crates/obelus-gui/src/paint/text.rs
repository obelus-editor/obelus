//! The letters, the agents' marks and the caret: everything drawn out of
//! the atlas, or standing where a letter would.

use obelus_app::app::Caret;
use obelus_font::{Fonts, Size};
use obelus_ui::image::SLOT;
use ratatui::style::Modifier;

use super::*;
use crate::grid::{Capped, Look, Marked, Page, Said, Spelling};

impl Painter {
    /// The text.
    ///
    /// Less what is drawn rather than spelled: a rule's line and a
    /// frame's are `rules` and `frame`, and the glyph a terminal draws
    /// them in would be a second line half a pixel from the first.
    ///
    /// And less what is written again: a cap's key, which `caps` writes
    /// smaller on its face, and the glyph a switch stands in for, which
    /// `ticks` draws as a box. Both used to be covered by a square of the
    /// cells' ground, which on glass is a square nobody wants -- so they
    /// are left out instead, whatever is behind them.
    pub(super) fn letters(
        &mut self,
        page: &Page,
        fonts: &mut Fonts,
        said: Said<'_>,
        framed: &[&Behind],
        capped: &[&Capped],
    ) {
        for row in 0..page.rows() {
            for column in 0..page.columns() {
                let look = page.look(column, row);
                if look.text.trim().is_empty() {
                    continue;
                }
                if drawn_as_a_shape(page, &said, framed, capped, column, row) {
                    continue;
                }
                // The mark's own cells rest at one colour and are carried
                // to the other by the light, which is the shader's -- see
                // `SHEENED`. What the cell holds is the terminal's answer
                // to the same question, eight bands of it, and reading
                // that as a base would be the light travelling over a ramp
                // that is already travelling.
                let lit = said.sheened.is_some_and(|mark| mark.holds(column, row));
                let ink = match said.sheened.filter(|_| lit) {
                    Some(mark) => mark.from,
                    None => look.foreground,
                };
                let colour = rgba(ink, Ink::Foreground);
                self.glyphs(column, row, look, colour, u32::from(lit) * SHEENED, fonts);
            }
        }
    }

    /// One cell's glyphs, put where the grid says rather than where their
    /// own advances would have reached.
    fn glyphs(
        &mut self,
        column: u16,
        row: u16,
        look: Look<'_>,
        colour: [f32; 4],
        lit: u32,
        fonts: &mut Fonts,
    ) {
        let cell = fonts.cell();
        self.glyphs_at(
            (f32::from(column) * cell.width, f32::from(row) * cell.height),
            look,
            colour,
            lit,
            fonts,
            Size::Cell,
        );
    }

    /// The same, at a place in pixels rather than at a cell.
    ///
    /// Which the caret needs because a caret on its way is not on a cell
    /// boundary, and what a block carries has to be in the same place the
    /// block is.
    pub(super) fn glyphs_at(
        &mut self,
        at: (f32, f32),
        look: Look<'_>,
        colour: [f32; 4],
        lit: u32,
        fonts: &mut Fonts,
        size: Size,
    ) {
        let (left, top) = at;
        let cell = fonts.cell();
        // A block is the cell, or a part of it, and is drawn as that rather
        // than asked of the face -- see `pieces`.
        if size == Size::Cell
            && let Some(pieces) = pieces(look.text)
        {
            for piece in pieces {
                let [x, y, wide, tall] = snapped((left, top), cell, *piece);
                self.quads.push(Quad {
                    rect: [x, y, wide, tall],
                    uv: self.atlas.white,
                    colour,
                    // Not `SOLID`, which would leave the light out: the
                    // welcome screen's mark is made of these, and it is
                    // carried by the same flag a letter is.
                    flags: lit,
                    radius: 0.0,
                    layer: 0,
                    lower: 0.0,
                });
            }
            return;
        }
        let bold = look.modifier.contains(Modifier::BOLD);
        let italic = look.modifier.contains(Modifier::ITALIC);
        let placed = fonts.glyphs(look.text, bold, italic, size).to_vec();
        for glyph in placed {
            let Some(spot) = self.atlas.spot(fonts, glyph.key) else {
                continue;
            };
            #[expect(
                clippy::cast_precision_loss,
                reason = "a glyph is offset by pixels, and there are few of them"
            )]
            let (x, y) = (glyph.x as f32, glyph.y as f32);
            // The grid's baseline for writing, and a mark's own for a mark
            // -- see `font::Placed::baseline`.
            let baseline = glyph.baseline.unwrap_or(cell.baseline);
            let Some((rect, uv)) = clipped(
                [
                    left + x + spot.left,
                    top + baseline + y - spot.top,
                    spot.width,
                    spot.height,
                ],
                spot.uv,
                self.grid,
            ) else {
                continue;
            };
            self.quads.push(Quad {
                rect,
                uv,
                colour,
                flags: match spot.colourful {
                    true => COLOURFUL,
                    false => lit,
                },
                radius: 0.0,
                layer: spot.layer,
                lower: 0.0,
            });
        }
    }

    /// The marks on this frame, each drawn into the two cells it was given.
    ///
    /// Rasterised the first time it is asked for at this size and kept in
    /// the same texture the glyphs are in: a mark is a picture of about
    /// sixteen pixels square, which is a large glyph and nothing more.
    /// Made here, at the moment of drawing, rather than by the view,
    /// because how many pixels a mark is depends on how big a cell is --
    /// which the reader changes.
    pub(super) fn marks(&mut self, marked: &[Marked], cell: obelus_font::CellSize) {
        for mark in marked {
            let key = (mark.id.clone(), mark.focused);
            let spot = match self.atlas.mark(&key) {
                Some(spot) => spot,
                None => {
                    let (Some(svg), Some(palette)) = (self.drawings.get(&key), self.palette) else {
                        // Asked for before it was carried, which the view
                        // does not do -- but a frame is not the place to
                        // find out.
                        continue;
                    };
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "a mark is a few dozen pixels each way"
                    )]
                    let pixels = (
                        (cell.width * f32::from(SLOT.width)).round() as u32,
                        (cell.height * f32::from(SLOT.height)).round() as u32,
                    );
                    let paper = match mark.focused {
                        true => palette.selected,
                        false => palette.paper,
                    };
                    let Some(drawn) = obelus_ui::image::raster(svg, pixels, palette.ink, paper)
                    else {
                        // A drawing this machine's renderer would not read.
                        // Remembered as nothing, so it is not tried again
                        // on every frame -- and said, because a card
                        // wearing a glyph where the others wear pictures
                        // has no other explanation.
                        tracing::warn!(id = key.0, "a mark that would not draw");
                        self.atlas.no_mark(key);
                        continue;
                    };
                    let rgba = drawn.to_rgba8();
                    let (width, height) = (rgba.width(), rgba.height());
                    let spot = self.atlas.place(width, height, &rgba, true);
                    self.atlas.marked(key, spot);
                    match spot {
                        Some(spot) => spot,
                        None => continue,
                    }
                }
            };
            self.quads.push(Quad {
                rect: [
                    f32::from(mark.x) * cell.width,
                    f32::from(mark.y) * cell.height,
                    cell.width * f32::from(SLOT.width),
                    cell.height * f32::from(SLOT.height),
                ],
                uv: spot.uv,
                // The picture carries its own colours, and the instance's
                // alpha is the whole of what it adds.
                colour: [1.0, 1.0, 1.0, 1.0],
                flags: COLOURFUL,
                radius: 0.0,
                layer: spot.layer,
                lower: 0.0,
            });
        }
    }

    /// The caret, in the shape that says what the next character will do.
    ///
    /// A bar stands between two characters and says the next one goes
    /// there; a block stands on one and says the next one takes its place.
    /// Which is the mode, and the application is what knows it.
    ///
    /// Only a window can draw it, because a terminal's caret is the
    /// terminal's and Obelus does not own its shape. Which is why the
    /// status row says `Replacing` in a word as well: that half works in
    /// both, and a mode with no sign is a mode the reader is in without
    /// knowing.
    ///
    /// The block is the cell with its colours the other way round, which is
    /// what a terminal does and for the same reason: a block that hid the
    /// character under it would be a caret a reader cannot read past.
    pub(super) fn caret(
        &mut self,
        page: &Page,
        fonts: &mut Fonts,
        spelling: Option<&Spelling>,
        drift: (f32, f32),
    ) {
        let Some(caret) = page.caret() else {
            return;
        };
        let cell = fonts.cell();
        // Inside the word being spelled, where the input method says it is:
        // a caret left at the start of it would be a caret in the wrong
        // half of what the reader is typing.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a caret is a few columns into what is being spelled"
        )]
        let along = spelling.map_or(0, |spelling| spelling.columns() as u16);
        let x = caret.x.saturating_add(along);
        let look = page.look(caret.x, caret.y);
        let ink = rgba(look.foreground, Ink::Foreground);
        // Where it is drawn, which is where the page put it only once it
        // has got there -- see `motion::Moving::drift`. Everything the
        // caret carries is drawn from these two, so a caret in flight
        // takes the character it is going to cover along with it rather
        // than leaving it behind on the cell.
        let left = (f32::from(x) + drift.0) * cell.width;
        let top = (f32::from(caret.y) + drift.1) * cell.height;
        // Inside a word being spelled the caret is always a bar: what is
        // under it there is the spelling itself, which is being written
        // rather than typed over, and a block would hide the character the
        // reader is in the middle of choosing.
        let shape = match spelling.is_some() {
            true => Caret::Bar,
            false => page.shape(),
        };
        match shape {
            // Narrow, and never thinner than a pixel: a caret that rounds
            // to nothing is a caret nobody can find.
            Caret::Bar => self.block(
                left,
                top,
                (cell.width * BAR).round().max(1.0),
                cell.height,
                ink,
            ),
            Caret::Block => {
                self.block(left, top, cell.width, cell.height, ink);
                let behind = rgba(look.background, Ink::Background);
                // What is under the caret, drawn again in the colour behind
                // it, so that a block does not hide the character it is on.
                if !look.text.trim().is_empty() {
                    self.glyphs_at((left, top), look, behind, 0, fonts, Size::Cell);
                }
            }
        }
    }

    /// What an input method is spelling, drawn where the word will go.
    ///
    /// Over the cells to the right of the caret rather than pushing them
    /// along: the file has not changed, and a window that reflowed the line
    /// for a word that may never be committed would be showing the reader a
    /// file that does not exist. Underlined, which is what says these
    /// characters are not in the file yet.
    ///
    /// In the colours of the place it is being typed into, because that is
    /// the only theme the window has: the cells say what the page's ink and
    /// paper are here.
    pub(super) fn spelling(&mut self, page: &Page, fonts: &mut Fonts, spelling: Option<&Spelling>) {
        let (Some(spelling), Some(caret)) = (spelling, page.caret()) else {
            return;
        };
        let cell = fonts.cell();
        let look = page.look(caret.x, caret.y);
        let ink = rgba(look.foreground, Ink::Foreground);
        let paper = rgba(look.background, Ink::Background);
        let mut column = caret.x;
        for character in spelling.text.chars() {
            if column >= page.columns() {
                // Off the edge. Clipped rather than wrapped: the row below
                // belongs to the next line of the file.
                break;
            }
            let written = character.to_string();
            #[expect(
                clippy::cast_possible_truncation,
                reason = "the width of one character, which is one or two"
            )]
            let wide = obelus_text::text_width(&written).max(1) as u16;
            let left = f32::from(column) * cell.width;
            let top = f32::from(caret.y) * cell.height;
            let width = f32::from(wide) * cell.width;
            self.block(left, top, width, cell.height, paper);
            let over = Look {
                text: &written,
                foreground: look.foreground,
                background: look.background,
                // Less the underline the cell under it may carry: what is
                // being spelled wears its own, drawn below, and the word
                // it is going into is not the word a server complained
                // about yet.
                modifier: look.modifier.difference(Modifier::UNDERLINED),
                underline: Color::Reset,
            };
            self.glyphs(column, caret.y, over, ink, 0, fonts);
            // The line under it, which is what every input method's inline
            // spelling wears and what tells it apart from the file.
            self.underline(left, top, width, cell.height, ink);
            column = column.saturating_add(wide);
        }
    }
}

/// Whether a cell wearing this colour is glass rather than a colour: it
/// is inside a pane, and the colour is that pane's own.
///
/// Which is the question every square of colour drawn over a pane has to
/// ask first, because the glass is already there and a square of the
/// pane's colour on it is a hole in it.
/// Whether this cell's picture is some shape the window draws, rather
/// than the character a terminal stands that shape in with.
///
/// One list, because they are one rule, and the rule is not only that the
/// character would look wrong under the shape. A cap, a bar and a rule can
/// all sit on a pane, and a pane is *glass*: `backgrounds` leaves those
/// cells unpainted on purpose, so a shape that had to cover a leftover
/// glyph would have to paint over them -- and painting over glass is
/// covering up the very thing the reader is meant to see through. Nothing
/// covers anything here; the glyph is simply never drawn.
pub(super) fn drawn_as_a_shape(
    page: &Page,
    said: &Said<'_>,
    framed: &[&Behind],
    capped: &[&Capped],
    column: u16,
    row: u16,
) -> bool {
    let within = |area: ratatui::layout::Rect| {
        (area.left()..area.right()).contains(&column) && (area.top()..area.bottom()).contains(&row)
    };
    said.ruled
        .iter()
        .any(|rule| rule.spans(page, column, row).is_some())
        || framed.iter().any(|card| card.ring_holds(page, column, row))
        || capped.iter().any(|cap| within(cap.area))
        || said.ticked.iter().any(|tick| within(tick.area))
        || said.spun.iter().any(|mark| within(mark.area))
        // Asked of the cell and not of the column, the same as a rule's:
        // a panel drawn over a scrollbar leaves cells in that column that
        // are the panel's, and the letters there are its own.
        || said.barred.iter().any(|bar| bar.covers(page, column, row))
        // Per cell rather than per run, and against the glyph, the same as
        // a rule: the margin is drawn to the foot of the editor's region
        // and a compact list goes over the bottom of it, so the rows under
        // the list are the list's and keep what it wrote in them.
        || said
            .stroked
            .iter()
            .any(|mark| mark.holds(page, column, row))
}

/// A glyph's rectangle cut back to the grid, and the part of its picture
/// that is left. `None` where none of it is.
///
/// A glyph's raster is bigger than its cell often enough -- an accent
/// reaches into the row above, a full block fills its own cell and a pixel
/// or two past it -- and inside the grid that is right: a window draws the
/// letters whole where a terminal chops each one at its cell's edge.
///
/// Outside the grid there is no row above. The margin is the strip no cell
/// reaches, and what is in it is the page's own ground and nothing else --
/// so a glyph that overflowed into it was a mark on the window's edge
/// belonging to no cell: the change map's half block along the top of the
/// window, and the block a bar is drawn with beside it.
pub(super) fn clipped(
    rect: [f32; 4],
    uv: [f32; 4],
    grid: [f32; 2],
) -> Option<([f32; 4], [f32; 4])> {
    let [left, top, width, height] = rect;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let (kept_left, kept_top) = (left.max(0.0), top.max(0.0));
    let (kept_right, kept_bottom) = ((left + width).min(grid[0]), (top + height).min(grid[1]));
    if kept_right <= kept_left || kept_bottom <= kept_top {
        return None;
    }
    let [from_u, from_v, to_u, to_v] = uv;
    // Where each edge ended up, as a part of the whole picture: the atlas
    // is read at the same place the pixels are drawn, or the glyph is
    // squeezed into what is left of it rather than cut.
    let across = |at: f32| from_u + (to_u - from_u) * (at - left) / width;
    let down = |at: f32| from_v + (to_v - from_v) * (at - top) / height;
    Some((
        [
            kept_left,
            kept_top,
            kept_right - kept_left,
            kept_bottom - kept_top,
        ],
        [
            across(kept_left),
            down(kept_top),
            across(kept_right),
            down(kept_bottom),
        ],
    ))
}

/// What part of its cell a block element covers, as rectangles of it:
/// left, top, right and bottom, from nothing to the whole cell.
///
/// Drawn rather than asked of the face, because a face draws `█` from its
/// own ascent to its own descent and the cell is a line height Obelus
/// chose (`font::LINE_HEIGHT`). Where the face is the shorter -- Menlo is,
/// by a pixel or two -- every row of blocks stands a hairline off the next,
/// and the welcome screen's mark is striped across; where it is the taller
/// the glyph is cut back to the cell and nobody sees the difference. Every
/// terminal that draws these itself does it for this reason, which is why
/// `ob` never showed it.
///
/// The shades are left to the face: they are a texture, not a region, and
/// a face's own is what a reader's terminal would show.
pub(super) fn pieces(text: &str) -> Option<&'static [[f32; 4]]> {
    const HALF: f32 = 0.5;
    const EIGHTH: f32 = 0.125;
    let mut chars = text.chars();
    let (Some(glyph), None) = (chars.next(), chars.next()) else {
        return None;
    };
    Some(match glyph {
        '\u{2580}' => &[[0.0, 0.0, 1.0, HALF]],
        // A lower one to seven eighths.
        '\u{2581}' => &[[0.0, 1.0 - EIGHTH, 1.0, 1.0]],
        '\u{2582}' => &[[0.0, 1.0 - 2.0 * EIGHTH, 1.0, 1.0]],
        '\u{2583}' => &[[0.0, 1.0 - 3.0 * EIGHTH, 1.0, 1.0]],
        '\u{2584}' => &[[0.0, HALF, 1.0, 1.0]],
        '\u{2585}' => &[[0.0, 1.0 - 5.0 * EIGHTH, 1.0, 1.0]],
        '\u{2586}' => &[[0.0, 1.0 - 6.0 * EIGHTH, 1.0, 1.0]],
        '\u{2587}' => &[[0.0, 1.0 - 7.0 * EIGHTH, 1.0, 1.0]],
        '\u{2588}' => &[[0.0, 0.0, 1.0, 1.0]],
        // A left seven eighths down to one.
        '\u{2589}' => &[[0.0, 0.0, 7.0 * EIGHTH, 1.0]],
        '\u{258a}' => &[[0.0, 0.0, 6.0 * EIGHTH, 1.0]],
        '\u{258b}' => &[[0.0, 0.0, 5.0 * EIGHTH, 1.0]],
        '\u{258c}' => &[[0.0, 0.0, HALF, 1.0]],
        '\u{258d}' => &[[0.0, 0.0, 3.0 * EIGHTH, 1.0]],
        '\u{258e}' => &[[0.0, 0.0, 2.0 * EIGHTH, 1.0]],
        '\u{258f}' => &[[0.0, 0.0, EIGHTH, 1.0]],
        '\u{2590}' => &[[HALF, 0.0, 1.0, 1.0]],
        '\u{2594}' => &[[0.0, 0.0, 1.0, EIGHTH]],
        '\u{2595}' => &[[1.0 - EIGHTH, 0.0, 1.0, 1.0]],
        // The quadrants, by which of the four they fill.
        '\u{2596}' => &[[0.0, HALF, HALF, 1.0]],
        '\u{2597}' => &[[HALF, HALF, 1.0, 1.0]],
        '\u{2598}' => &[[0.0, 0.0, HALF, HALF]],
        '\u{2599}' => &[[0.0, 0.0, HALF, 1.0], [HALF, HALF, 1.0, 1.0]],
        '\u{259a}' => &[[0.0, 0.0, HALF, HALF], [HALF, HALF, 1.0, 1.0]],
        '\u{259b}' => &[[0.0, 0.0, 1.0, HALF], [0.0, HALF, HALF, 1.0]],
        '\u{259c}' => &[[0.0, 0.0, 1.0, HALF], [HALF, HALF, 1.0, 1.0]],
        '\u{259d}' => &[[HALF, 0.0, 1.0, HALF]],
        '\u{259e}' => &[[HALF, 0.0, 1.0, HALF], [0.0, HALF, HALF, 1.0]],
        '\u{259f}' => &[[HALF, 0.0, 1.0, HALF], [0.0, HALF, 1.0, 1.0]],
        _ => return None,
    })
}

/// One of those rectangles in pixels, from the corner of its cell.
///
/// Each edge is rounded where it falls rather than the size being rounded
/// on its own: a cell is not a whole number of pixels tall, so a block
/// whose height was rounded starts where the one above it ended only by
/// luck, and the stripe `pieces` is there to take away comes back as a
/// pixel of overlap or of gap every few rows.
pub(super) fn snapped(at: (f32, f32), cell: obelus_font::CellSize, piece: [f32; 4]) -> [f32; 4] {
    let [from_x, from_y, to_x, to_y] = piece;
    let left = cell.width.mul_add(from_x, at.0).round();
    let top = cell.height.mul_add(from_y, at.1).round();
    let right = cell.width.mul_add(to_x, at.0).round();
    let bottom = cell.height.mul_add(to_y, at.1).round();
    [left, top, (right - left).max(1.0), (bottom - top).max(1.0)]
}
