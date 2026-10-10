//! The panes and boxes: their glass, what slides under it while one
//! arrives, and the shadows they cast.

use obelus_font::{Fonts, Size};

use super::{ground::runs_from, setup::halved, *};
use crate::grid::{Barred, Capped, Look, Page, Rolled, Ruled};

impl Painter {
    /// The bands drawn behind where their lists have got to.
    ///
    /// The same pieces a pane arrives on, one set per band, and one thing
    /// before them. What a band shows while it catches up is partly on
    /// the frame that has just been drawn -- taken from it lower down,
    /// which is the band showing what it showed a moment ago -- and
    /// partly on no frame at all: the rows the list scrolled *past* are
    /// not on the new page, and the only place they exist is the page it
    /// scrolled off. So those pages are drawn first, where their rows
    /// have got to, and what covers them is the frame everywhere the
    /// bands are not -- which is also what cuts each page back to the
    /// band it belongs to, since a row shifted far enough lands outside
    /// it.
    ///
    /// And less the pane over a band, where one is: a transcript that
    /// scrolls on under a list is the transcript moving, not the list, and
    /// a band that took the cells over it along took the foot of the list
    /// with it -- every line an agent wrote behind a list was the list
    /// coming up again. A band the pane's own view drew is the list, and
    /// moves.
    pub(super) fn catching_up(
        &mut self,
        bands: &[Rolled<'_>],
        panes: &[[f32; 4]],
        stack: &[&Behind],
        fonts: &mut Fonts,
    ) {
        let cell = fonts.cell();
        for band in bands {
            let room = band.room;
            // A list on glass: the page it scrolled off is on the glass,
            // which stands where it is, so that is drawn first and the
            // pane's own colour in those rows is left to it -- the same
            // hole `backgrounds` leaves.
            let glass = on_glass(band, stack);
            if let Some(level) = glass {
                self.placed.gaps.push((self.quads.len(), level));
                self.quads.push(Quad {
                    uv: box_of(room, cell),
                    ..self.quads[self.placed.levels[level].glass]
                });
            }
            // Where the page it scrolled off has got to, which is further
            // back than the band by however much of the move is already
            // done.
            let offset = (band.behind - band.since) * cell.height;
            for y in room.top()..room.bottom() {
                for x in room.left()..room.right() {
                    let look = band.before.look(x, y);
                    let left = f32::from(x) * cell.width;
                    let top = f32::from(y).mul_add(cell.height, offset);
                    if glass.is_none_or(|level| look.background != stack[level].ground) {
                        self.block(
                            left,
                            top,
                            cell.width,
                            cell.height,
                            rgba(look.background, Ink::Background),
                        );
                    }
                    if !look.text.trim().is_empty() {
                        let ink = rgba(look.foreground, Ink::Foreground);
                        self.glyphs_at((left, top), look, ink, 0, fonts, Size::Cell);
                    }
                }
            }
        }
        let rooms: Vec<[f32; 4]> = bands.iter().map(|band| box_of(band.room, cell)).collect();
        let moving: Vec<[f32; 4]> = bands
            .iter()
            .zip(&rooms)
            .flat_map(|(band, room)| sliding(*room, panes, band.under))
            .collect();
        self.covering(&moving);
        for (at, (band, room)) in bands.iter().zip(&rooms).enumerate() {
            // Less the rooms of the other bands, so that a band with one
            // inside it -- a hover over the file it is about -- does not
            // take the inner one along on its own journey.
            let others: Vec<[f32; 4]> = rooms
                .iter()
                .enumerate()
                .filter(|&(other, _)| other != at)
                .map(|(_, room)| *room)
                .collect();
            for part in sliding(*room, panes, band.under) {
                for piece in tiles(part, &others) {
                    self.slid_piece(piece, *room, band.behind * cell.height, 1.0);
                }
            }

            // And the bar, which is not in the band and does not stand
            // still either: its mark belongs where the band is being
            // *drawn*, which is its own share of the same distance
            // behind.
            //
            // Taken out of the same picture and not redrawn from the
            // page, which is what this did first and what made the column
            // flicker: a bar inside a pane sits on glass, and the cells it
            // is made of carry the pane's own colour -- painted back as
            // cells, that colour goes down opaque over what the reader was
            // seeing through. Out of the picture it is whatever it was,
            // glass included, moved.
            if let Some((bar, to_come)) = band.bar {
                self.slid(
                    box_of(bar, cell),
                    mark_behind(to_come, band.behind, band.since) * cell.height,
                    1.0,
                );
            }
        }
    }

    /// The glass under each list that is catching up, drawn again over its
    /// rows and reading what is behind it from as much further down as the
    /// rows are about to be taken from further up. Hands back how many.
    ///
    /// The rows are put back from higher up the frame -- see `catching_up`
    /// -- and the glass they are on is in the same picture: taken along
    /// with them, what is behind a list slid with the list and jumped back
    /// when it arrived. Read lower by the distance it is taken from, it
    /// stands still on the screen while the rows go over it.
    ///
    /// Straight after the glass it is a copy of, so that the rows are
    /// drawn over it -- the half row `pane_glass` paints under the status
    /// row's rule comes after, and is nowhere near a list's rows.
    pub(super) fn glass_kept_still(
        &mut self,
        glass: usize,
        level: usize,
        stack: &[&Behind],
        bands: &[Rolled<'_>],
        cell: CellSize,
    ) -> usize {
        let copies = kept_still(&self.quads[glass], level, stack, bands, cell);
        let lowered = copies.len();
        self.quads.splice(glass + 1..glass + 1, copies);
        lowered
    }

    /// What is behind the first pane, with the bands under it slid the
    /// way `catching_up` slides them on the screen.
    ///
    /// The screen keeps the pane still and slides the band round it, and
    /// the glass is a picture of what is behind -- so without this the
    /// band came up to the pane's edge and stopped, and what showed through
    /// the glass jumped to where the band was going.
    ///
    /// The rows of the band above the pane are drawn as well, because a
    /// slide fills the top of the pane from above it. Where it would be
    /// filled from outside the band -- what scrolled past, which is on the
    /// page it scrolled off and is not behind anything -- the band is left
    /// where it is, which through the frost is the same rows.
    pub(super) fn sliding_under(
        &mut self,
        page: &Page,
        behind: &Behind,
        bands: &[Rolled<'_>],
        fonts: &mut Fonts,
    ) {
        let cell = fonts.cell();
        let pane = box_of(behind.area, cell);
        let under: Vec<(&Rolled<'_>, [f32; 4])> = bands
            .iter()
            .filter_map(|band| Some((band, beneath(box_of(band.room, cell), pane, band.under)?)))
            .collect();
        if under.is_empty() {
            return;
        }
        let start = self.quads.len();
        for (band, _) in &under {
            let room = band.room;
            for y in room.top()..behind.area.top().min(room.bottom()) {
                for x in room.left()..room.right() {
                    let look = page.look(x, y);
                    let (left, top) = (f32::from(x) * cell.width, f32::from(y) * cell.height);
                    self.block(
                        left,
                        top,
                        cell.width,
                        cell.height,
                        rgba(look.background, Ink::Background),
                    );
                    if !look.text.trim().is_empty() {
                        let ink = rgba(look.foreground, Ink::Foreground);
                        self.glyphs_at((left, top), look, ink, 0, fonts, Size::Cell);
                    }
                }
            }
        }
        let above = start..self.quads.len();
        let start = self.quads.len();
        self.covering(&[]);
        for (band, piece) in &under {
            self.slid_piece(
                *piece,
                box_of(band.room, cell),
                band.behind * cell.height,
                1.0,
            );
        }
        self.placed.under = Some((above, start..self.quads.len()));
    }

    /// The frame that has just been drawn, put back on the screen
    /// everywhere the things that are moving are not.
    ///
    /// The frame has been drawn into a picture of its own by then, and
    /// what is moving is taken from higher up in that picture by `slid`.
    /// This is the rest of it, as the few rectangles the rest of it is --
    /// so what shows where a pane has not reached is whatever was drawn
    /// there before, which for a pane is the page and for a band is the
    /// page it scrolled off.
    ///
    /// As rectangles rather than as one quad that leaves a hole, because
    /// there may be several holes: two lists catching up at once are two
    /// rooms to keep clear, and a quad can only be told about one.
    pub(super) fn covering(&mut self, rooms: &[[f32; 4]]) {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a window is thousands of pixels, not millions"
        )]
        let (right, bottom) = (self.configured.width as f32, self.configured.height as f32);
        let [left, top, wide, tall] = whole_window([right, bottom], self.margin);
        for tile in tiles([left, top, left + wide, top + tall], rooms) {
            let [left, top, far, low] = tile;
            self.quads.push(Quad {
                rect: [left, top, (far - left).max(1.0), (low - top).max(1.0)],
                // Nothing reads it: a piece of the frame is the picture at
                // the place the piece stands, and where it stands is its
                // own rectangle.
                uv: [0.0; 4],
                colour: [0.0, 0.0, 0.0, 1.0],
                flags: FRAME,
                radius: 0.0,
                layer: 0,
                lower: 0.0,
            });
        }
    }

    /// The panes that went, each put back out of the screen it left and on
    /// its way into the edge it came from: the arriving the other way
    /// round, over the same distance, fading as it goes.
    ///
    /// Out of a picture rather than drawn from cells, for the reason a bar
    /// catching up is: a pane is glass, and its cells carry its own colour
    /// where the glass let what was behind show through.
    pub(super) fn going(&mut self, along: Option<f32>) {
        let Some(along) = along else {
            return;
        };
        let start = self.quads.len();
        for at in 0..self.gone.len() {
            let (pane, joined) = self.gone[at];
            let toward = match joined {
                Joined::Below => 1.0,
                _ => -1.0,
            };
            let height = pane[3] - pane[1];
            self.slid(pane, toward * along * height * TRAVEL, 1.0 - along);
        }
        if self.quads.len() > start {
            self.placed.going = Some(start..self.quads.len());
        }
    }

    /// One region of the frame that has just been drawn, put back
    /// somewhere other than where it stands.
    ///
    /// `shift` is how far, in pixels, and what is under it where the
    /// region runs out is whatever was drawn there before -- the page for
    /// a pane that has not arrived, the frame's own copy for a bar whose
    /// mark has moved on.
    pub(super) fn slid(&mut self, box_: [f32; 4], shift: f32, fade: f32) {
        self.slid_piece(box_, box_, shift, fade);
    }

    /// And a piece of one, which is a region with a hole in it: the piece
    /// is what is drawn and the region is what says where the picture is
    /// taken from and where it runs out.
    ///
    /// A band with another band inside it is the reason -- a hover over
    /// the file it is about. Both catch up on their own, so the outer one
    /// must not take the inner one with it: what it puts back is the
    /// region less the rooms of the bands inside it, and each of those
    /// puts back its own.
    fn slid_piece(&mut self, piece: [f32; 4], room: [f32; 4], shift: f32, fade: f32) {
        let [left, top, far, low] = piece;
        self.quads.push(Quad {
            rect: [left, top, (far - left).max(1.0), (low - top).max(1.0)],
            uv: room,
            colour: [0.0, 0.0, 0.0, fade],
            flags: SLID,
            radius: shift,
            layer: 0,
            lower: 0.0,
        });
    }

    /// What is under a pane, drawn exactly as the page would draw it.
    ///
    /// The first pane's goes in front of everything else in the buffer,
    /// because it is drawn twice: once into a texture of its own, which is
    /// what the glass reads, and once on the screen, where it is what shows
    /// through the pane's rounded corners -- the one place the pane is not.
    pub(super) fn pane_under(
        &mut self,
        behind: &Behind,
        barred: &[Barred],
        capped: &[Capped],
        fonts: &mut Fonts,
    ) {
        let cell = fonts.cell();
        for y in behind.area.top()..behind.area.bottom() {
            let ground = self.ground_colour();
            let (grounds, lettered) = behind_row(behind, barred, y);
            for (start, end, colour) in grounds {
                // What a pane was put over is the page, so a hold in it
                // is a plate on the page's own ground.
                self.block(
                    f32::from(start) * cell.width,
                    f32::from(y) * cell.height,
                    f32::from(end - start) * cell.width,
                    cell.height,
                    as_held(colour, self.holding, ground)
                        .unwrap_or_else(|| rgba(colour, Ink::Background)),
                );
            }
            for x in lettered {
                let Some(under) = behind.look(x, y) else {
                    continue;
                };
                let ink = rgba(under.foreground, Ink::Foreground);
                self.glyphs_at(
                    (f32::from(x) * cell.width, f32::from(y) * cell.height),
                    under,
                    ink,
                    0,
                    fonts,
                    Size::Cell,
                );
            }
        }
        // And the caps the picture still holds, which the page no longer
        // does: a cap is the shape its cells are, and these cells are seen
        // through the glass -- see `Capped::still_behind`. Put back on
        // their own ground always, since nothing is behind the picture.
        for cap in capped.iter().filter(|cap| cap.still_behind(behind)) {
            self.cap(cap, |x, y| behind.look(x, y), true, None, fonts);
        }
    }

    /// The glass over a pane: one quad over what is under it. What it does
    /// with what is behind is in `paint.wgsl`: the shape is the same
    /// rounded box a key's cap is, and it is the same function that says
    /// where its edge is.
    ///
    /// Hands back which quad the glass is, which is the one drawn with the
    /// pane's own pictures, and where it is.
    pub(super) fn pane_glass(
        &mut self,
        page: &Page,
        behind: &Behind,
        lower: &[&Behind],
        ruled: &[Ruled],
        cell: CellSize,
    ) -> (usize, [f32; 4]) {
        // Over another pane's glass where it is over one at all: a list
        // opened over the settings is, a list opened over the code never.
        let on_glass = lower.iter().any(|lower| lower.area.intersects(behind.area));
        let mut tint = rgba(behind.ground, Ink::Background);
        tint[3] = match on_glass {
            true => ON_GLASS_TINT,
            false => TINT,
        };
        let (left, mut top) = (
            f32::from(behind.area.x) * cell.width,
            f32::from(behind.area.y) * cell.height,
        );
        let far = f32::from(behind.area.right()) * cell.width;
        let mut low = f32::from(behind.area.bottom()) * cell.height;
        // A rule along the pane's first or last row is its edge, and the
        // glass starts or stops where the rule's line is, which is the
        // middle of that row. A glass edge on the row's boundary was half a
        // row from the line that said where the pane was.
        //
        // Above the line over the pane is what the rule was drawn over,
        // which is in `behind` and has been drawn already, first of
        // everything. A terminal has to give a rule the whole of its row,
        // because a glyph takes a cell; a line drawn here takes a pixel.
        // Painted in the rule's own ground, that half row was a blank band
        // across whatever the list was opened over -- the welcome screen,
        // a line of the file. Under the line below it is the rule's own
        // row and nothing else, since it is the status row's rule and
        // nothing is under it, so that half is painted: it covers the
        // rule's glyph, which is what `behind` holds there.
        //
        // The line above is inside the glass and the line below is not:
        // the first is the list's own edge and arrives with it, and the
        // second is the status row's, which the list stands on.
        let line = thickness(cell.height);
        let edge = |y: u16| {
            ruled.iter().find(|rule| {
                rule.area.y == y
                    && rule.area.x == behind.area.x
                    && rule.area.width == behind.area.width
                    && rule.still_said(page)
            })
        };
        if edge(behind.area.y).is_some() {
            top = middle(top, cell.height, line);
        }
        let mut under = None;
        if behind.area.height > 1
            && let Some(rule) = edge(behind.area.bottom() - 1)
        {
            let row = f32::from(rule.area.y) * cell.height;
            let at = middle(row, cell.height, line);
            under = Some((
                [at, low - at],
                page.look(rule.area.x, rule.area.y).background,
            ));
            low = at;
        }
        let glass = self.quads.len();
        self.quads.push(Quad {
            rect: [left, top, (far - left).max(1.0), (low - top).max(1.0)],
            uv: [left, top, far, low],
            colour: tint,
            flags: SOLID
                | GLASS
                | match on_glass {
                    true => ON_GLASS,
                    false => 0,
                }
                | match behind.joined {
                    Joined::Above => HANGING,
                    Joined::Below => STANDING,
                    // Nothing under the screen's own bottom edge.
                    Joined::Screen => 0,
                    // Never here: a box's glass is `box_glass`'s.
                    Joined::Nowhere => 0,
                },
            // Unread: a pane has no corners to round -- see `outside` in
            // `paint.wgsl`.
            radius: 0.0,
            layer: 0,
            lower: 0.0,
        });
        if let Some(([over, tall], ground)) = under {
            self.block(
                left,
                over,
                (far - left).max(1.0),
                tall,
                rgba(ground, Ink::Background),
            );
        }
        (glass, [left, top, far, low])
    }

    /// The soft edge outside a pane and outside each box with a frame
    /// round it.
    ///
    /// Not the pane's while it is still arriving, which is drawn where
    /// the frame is put back together instead. The frame is drawn once
    /// and put back in two pieces, the pane's own slid up from where it
    /// set out -- and a shadow is the one part of a pane that falls
    /// *outside* the pane's own room, so it is in the other piece: drawn
    /// here it would stand still at the edge the pane is going to have
    /// while there is nothing under it yet. It travels with the pane
    /// instead, from the edge the pane has reached -- see `Painter::frame`.
    ///
    /// Which is the one thing on this screen a terminal has no answer to
    /// at all. A pane's edge, it draws with a rule; a box's, with a
    /// frame of `╭─╮`. Both say *where* the thing stops and neither says
    /// it is over anything, because a cell is a cell and there is
    /// nowhere for a shadow to go. So this is added rather than drawn
    /// better -- `shapes`'s own test says a thing said there has to be
    /// one a terminal already answers -- and it is added in the front
    /// end, from what the front end already knows: the rectangle it drew
    /// the glass in, and which edge the thing is joined to.
    pub(super) fn shadows(
        &mut self,
        stack: &[&Behind],
        sliding: Option<usize>,
        boxes: f32,
        cell: CellSize,
    ) {
        let spread = cell.height * SHADOW_SPREAD;
        for (at, over) in stack.iter().enumerate() {
            // A box has four edges and corners, so it casts all round.
            if over.is_a_box() {
                let line = thickness(cell.height);
                self.shadow(
                    outline(over.area, cell, line),
                    cell.width * FRAME_CORNER,
                    0,
                    spread,
                    boxes,
                );
            } else if sliding != Some(at)
                && casts(stack, at)
                && let Some(joined) = casting(over.joined)
            {
                let glass = self.placed.levels[at].rect;
                self.shadow(glass, 0.0, joined, spread, 1.0);
            }
        }
    }

    /// One of them: the rectangle that casts it, how far it reaches, and
    /// how much of it there is -- which is all of it except while the
    /// thing casting it is still arriving.
    pub(super) fn shadow(
        &mut self,
        box_: [f32; 4],
        radius: f32,
        joined: u32,
        spread: f32,
        fade: f32,
    ) {
        let [left, top, far, low] = box_;
        self.quads.push(Quad {
            rect: [
                left - spread,
                top - spread,
                spread.mul_add(2.0, far - left).max(1.0),
                spread.mul_add(2.0, low - top).max(1.0),
            ],
            uv: box_,
            colour: [0.0, 0.0, 0.0, SHADOW_INK * fade],
            flags: SHADOW | joined,
            radius,
            layer: 0,
            lower: 0.0,
        });
    }

    /// What is under anything over the first pane, or under a box with
    /// a frame round it: the picture its glass reads, which goes into its
    /// own backdrop and never onto the screen -- the cells it was put over,
    /// as the screen had them. Where those were glass further down -- the
    /// card of every key is nearly always over a list or a page of
    /// settings, and a setting's choices are over the settings -- they are
    /// the letters alone, because the picture draws that glass before them
    /// and a ground under the letters would cover it.
    pub(super) fn seen_under(&mut self, over: &Behind, lower: &[&Behind], fonts: &mut Fonts) {
        let cell = fonts.cell();
        for y in over.area.top()..over.area.bottom() {
            for x in over.area.left()..over.area.right() {
                let Some(under) = over.look(x, y) else {
                    continue;
                };
                let left = f32::from(x) * cell.width;
                let top = f32::from(y) * cell.height;
                if !seen_through(lower, x, y, under.background) {
                    let ground = self.under(lower, &[], x, y);
                    self.block(
                        left,
                        top,
                        cell.width,
                        cell.height,
                        as_held(under.background, self.holding, ground)
                            .unwrap_or_else(|| rgba(under.background, Ink::Background)),
                    );
                }
                if !under.text.trim().is_empty() {
                    let ink = rgba(under.foreground, Ink::Foreground);
                    self.glyphs_at((left, top), under, ink, 0, fonts, Size::Cell);
                }
            }
        }
    }

    /// The frame round a box and the glass inside its line. The cells
    /// inside the ring are drawn after all of this by `backgrounds` and
    /// `letters`, which leave the box's own ground to the glass.
    ///
    /// Hands back which quad the glass is and where it is, the way a
    /// pane's does.
    pub(super) fn box_glass(
        &mut self,
        page: &Page,
        card: &Behind,
        lower: &[&Behind],
        cell: CellSize,
    ) -> (usize, [f32; 4]) {
        let (inside, radius) = self.frame(page, card, lower, cell);
        // Over a pane where it is over one at all: the card of every key
        // always is, a hover over the code never.
        let on_glass = lower.iter().any(|lower| lower.area.intersects(card.area));
        let mut tint = rgba(card.ground, Ink::Background);
        tint[3] = match on_glass {
            true => ON_GLASS_TINT,
            false => TINT,
        };
        let glass = self.quads.len();
        self.quads.push(Quad {
            rect: inside,
            uv: [
                inside[0],
                inside[1],
                inside[0] + inside[2],
                inside[1] + inside[3],
            ],
            colour: tint,
            // Neither hanging nor standing, which is a box with an edge on
            // every side -- see `outside` in `paint.wgsl`.
            flags: SOLID
                | GLASS
                | match on_glass {
                    true => ON_GLASS,
                    false => 0,
                },
            radius,
            layer: 0,
            lower: 0.0,
        });
        let [left, top, wide, tall] = inside;
        (glass, [left, top, left + wide, top + tall])
    }

    /// The two quads that blur what is behind a pane, one way and then
    /// the other, over the pane's own rectangle and no further: the glass
    /// reads nothing outside it, and what is outside it in the picture is
    /// nothing -- a blur that reached out for it would darken the pane's
    /// edges, which is why the shader holds its samples inside `box_`.
    ///
    /// Each carries how many of the window's pixels one of the picture it
    /// draws into is, each way: the blur is worked out in the window's
    /// pixels, wherever it is drawn.
    pub(super) fn blurs_over(&mut self, box_: [f32; 4]) -> usize {
        let [left, top, far, low] = box_;
        let (width, height) = (self.configured.width, self.configured.height);
        #[expect(
            clippy::cast_precision_loss,
            reason = "a window is nowhere near 2^24 pixels across"
        )]
        let (across, down) = (
            width as f32 / halved(width).max(1) as f32,
            height as f32 / halved(height).max(1) as f32,
        );
        let first = self.quads.len();
        for way in [[1.0, 0.0], [0.0, 1.0]] {
            self.quads.push(Quad {
                rect: [left, top, (far - left).max(1.0), (low - top).max(1.0)],
                uv: box_,
                colour: [way[0], way[1], across, down],
                flags: BLUR,
                radius: 0.0,
                layer: 0,
                lower: 0.0,
            });
        }
        first
    }
}

/// One row of the picture a pane's glass reads: its grounds, then which
/// of its columns carry a letter.
///
/// Two lists and in this order, because that is what they are drawn in --
/// the same order `backgrounds` and `letters` go in on the screen, and for
/// the same reason: a ground drawn after a glyph is a ground *over* it.
///
/// Which is how a page of Chinese read through the glass came out as half
/// of every character. This was a ground and a glyph per cell, a column at
/// a time, and a full-width character is one glyph over two columns whose
/// second is a cell `ratatui` has reset -- so that cell's own ground was
/// painted over the right half of the character before it. As runs the
/// second column belongs to the character, so there is no rectangle there
/// to do it with, and the letters come after every one of them anyway.
pub(super) fn behind_row(
    behind: &Behind,
    barred: &[Barred],
    y: u16,
) -> (Vec<(u16, u16, Color)>, Vec<u16>) {
    let (from, to) = (behind.area.left(), behind.area.right());
    let grounds = runs_from(
        from,
        (from..to).map(|x| {
            behind.look(x, y).map_or((1, Color::Reset), |under| {
                (under.columns(), under.background)
            })
        }),
    );
    let lettered = (from..to)
        .filter(|&x| {
            behind
                .look(x, y)
                .is_some_and(|under| lettered_behind(under, barred, x, y))
        })
        .collect();
    (grounds, lettered)
}

/// Whether the picture of what a pane was opened over writes this cell.
///
/// The same question `letters` asks of the page, asked of the picture: a
/// bar is drawn as a shape, and drawn over the pane as well -- `bars` is
/// asked from where the bar was said and not from what the page holds
/// there -- so the block a terminal has for a track is a glyph nobody
/// wanted here, under a capsule and under the tint.
///
/// It was not merely redundant. A full block's raster is taller than its
/// cell, and the glass covers the pane's own rectangle exactly, so the
/// part of the first row's block that reached above the grid came out in
/// the margin round it -- a dark cell's-width line along the top of the
/// window, wherever a list was opened over a file long enough to have a
/// bar.
pub(super) fn lettered_behind(under: Look<'_>, barred: &[Barred], x: u16, y: u16) -> bool {
    !under.text.trim().is_empty()
        && !barred.iter().any(|showing| {
            let area = showing.bar.area;
            (area.left()..area.right()).contains(&x) && (area.top()..area.bottom()).contains(&y)
        })
}

/// How far a bar's mark is drawn from where the frame put it, while the
/// band beside it catches up, in rows.
///
/// The same fraction of the way along as the band, so the two arrive
/// together. `to_come` is measured between two rows the bar was *drawn*
/// at, so the mark neither sets out from nor lands on a row it was never
/// on -- and it is held between them, because a mark that left the row it
/// was on before it set off, or went past the row it is going to, is a
/// mark that steps backwards. Which is the one thing a mark must not do.
pub(super) fn mark_behind(to_come: f32, behind: f32, since: f32) -> f32 {
    if since.abs() <= f32::EPSILON {
        return 0.0;
    }
    to_come * (behind / since).clamp(0.0, 1.0)
}

/// What of a band's room moves while it catches up, given the pane over
/// the page and whether the band is `under` it.
///
/// All of it for a band the pane's own view drew, which is the list. Only
/// what the pane leaves showing for one the pane was put over: what is
/// drawn there is the pane, and it is not what scrolled.
///
/// Asked and not worked out from where the two are. It was once whether
/// the band was inside the pane, which a full-screen dialog answers yes
/// for everything -- so a transcript scrolling on under the settings or
/// a full list slid the whole dialog with it, a shudder on every line an
/// agent wrote.
pub(super) fn sliding(room: [f32; 4], panes: &[[f32; 4]], under: bool) -> Vec<[f32; 4]> {
    tiles(room, if under { panes } else { &[] })
}

/// The part of a band's room a pane is over, where the band is under the
/// pane rather than the pane's own.
///
/// The other half of `sliding`: what that leaves out on the screen is
/// what slides behind the glass instead.
pub(super) fn beneath(room: [f32; 4], pane: [f32; 4], under: bool) -> Option<[f32; 4]> {
    if sliding(room, &[pane], under) == [room] {
        return None;
    }
    Some([
        room[0].max(pane[0]),
        room[1].max(pane[1]),
        room[2].min(pane[2]),
        room[3].min(pane[3]),
    ])
}

/// The copies `Painter::glass_kept_still` lays over one level's glass: one
/// per band on it, cut to the band's rows and reading what is behind from
/// as far down as `catching_up` takes the rows from above -- and one per
/// bar beside a band, which is taken from above by its own distance.
///
/// The bar is not quite still. Where its mark has moved on, the screen
/// shows the frame's own copy of the column, which is this glass read
/// lower too: a strip as tall as the mark's distance, at the end it left,
/// for as long as it takes to get there. The rest of the column stands.
pub(super) fn kept_still(
    glass: &Quad,
    level: usize,
    stack: &[&Behind],
    bands: &[Rolled<'_>],
    cell: CellSize,
) -> Vec<Quad> {
    let copy = |room: Rect, lower: f32| Quad {
        uv: box_of(room, cell),
        lower,
        ..*glass
    };
    bands
        .iter()
        .filter(|band| on_glass(band, stack) == Some(level))
        .flat_map(|band| {
            let bar = band.bar.map(|(bar, to_come)| {
                let lower = mark_behind(to_come, band.behind, band.since);
                copy(bar, lower * cell.height)
            });
            std::iter::once(copy(band.room, band.behind * cell.height)).chain(bar)
        })
        .collect()
}

/// Which level's glass a band's rows are drawn on: the nearest pane or box
/// round it, where the list is that one's own. One the pane was put over
/// slides behind the glass instead -- see `Painter::sliding_under`.
fn on_glass(band: &Rolled<'_>, stack: &[&Behind]) -> Option<usize> {
    if band.under {
        return None;
    }
    stack
        .iter()
        .rposition(|over| over.area.intersection(band.room) == band.room)
}

/// What is left of the frame once the rooms that are moving are taken
/// out of it.
///
/// Rectangles, left top right bottom, and they do not overlap: each is
/// drawn as a copy of the picture the frame was drawn into, and a pixel
/// copied twice is a pixel drawn twice for nothing.
pub(super) fn tiles(whole: [f32; 4], rooms: &[[f32; 4]]) -> Vec<[f32; 4]> {
    let mut left = vec![whole];
    for room in rooms {
        let mut kept = Vec::with_capacity(left.len() + 3);
        for piece in left {
            cut(piece, *room, &mut kept);
        }
        left = kept;
    }
    left
}

/// One rectangle less another, as up to four: what is above the hole,
/// what is below it, and the two strips beside it.
///
/// The strips are cut to the hole's own rows, so that they do not overlap
/// the pieces above and below.
fn cut(piece: [f32; 4], room: [f32; 4], into: &mut Vec<[f32; 4]>) {
    let [left, top, far, low] = piece;
    if room[0] >= far || room[2] <= left || room[1] >= low || room[3] <= top {
        into.push(piece);
        return;
    }
    if room[1] > top {
        into.push([left, top, far, room[1]]);
    }
    if room[3] < low {
        into.push([left, room[3], far, low]);
    }
    let (over, under) = (room[1].max(top), room[3].min(low));
    if room[0] > left {
        into.push([left, over, room[0], under]);
    }
    if room[2] < far {
        into.push([room[2], over, far, under]);
    }
}

/// How much of a pane a slide has let through, which is the room its
/// shadow falls from.
///
/// A pane on its way in is taken from `shift` pixels further up or down
/// its own picture and stops where that picture does -- so the edge it is
/// joined by stays where it is, at the seam, and the free edge is the one
/// that has moved. Which is the edge a shadow falls from, so this is the
/// only part of it that the sliding changes.
pub(super) fn reached(pane: [f32; 4], shift: f32) -> [f32; 4] {
    [
        pane[0],
        pane[1] + shift.max(0.0),
        pane[2],
        pane[3] + shift.min(0.0),
    ]
}

/// Which half plane a pane casts its shadow into, which is the same one
/// its glass is cut to -- and `None` where it casts none at all.
///
/// A pane is joined to the page along one edge and casts from the other:
/// a list standing on the status row throws its shadow up over the file,
/// and one hanging from the top throws it down.
///
/// A full-screen pane has no free edge to cast from, and `0` is not the
/// way to say so: to the shader it is a box joined to nothing, which
/// casts on all four sides. Joined as `Above` it was a grey band across
/// the page's last row -- its own shadow, falling on itself -- and as `0`
/// it was a grey frame in the strip round the grid, which is there
/// whenever the window is not a whole number of cells.
pub(super) fn casting(joined: Joined) -> Option<u32> {
    match joined {
        Joined::Above => Some(HANGING),
        Joined::Below => Some(STANDING),
        Joined::Screen => None,
        // A box casts all round -- see `shadows`.
        Joined::Nowhere => Some(0),
    }
}
