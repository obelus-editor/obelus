//! The panes and boxes: their glass, what slides under it while one
//! arrives, and the shadows they cast.

use obelus_font::{Fonts, Size};

use super::{ground::runs_from, setup::halved, *};
use crate::grid::{Barred, Look, Page, Rolled, Ruled, Said, Spun};

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
    ///
    /// `panes` is each pane's glass, the first pane's first where it is the
    /// first level: the glass and not its cells, because the half row above
    /// a list's line is the page's, and held still with the list it was a
    /// strip of the page that did not scroll. `first` is the first pane's
    /// cells, which is where the page layer is held apart -- see `standing`.
    pub(super) fn catching_up(
        &mut self,
        bands: &[Rolled<'_>],
        panes: &[[f32; 4]],
        first: Option<[f32; 4]>,
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
            let rows = self.quads.len();
            self.scrolled_off(band, room.left()..room.right(), glass, stack, fonts);
            // Over a picture, what covers these is not opaque: the frame
            // is put back clear where it is the page, so these rows would
            // show through it -- the same line twice inside the band, drawn
            // by the old page and the new, and outside it rows the slide
            // carried past the band's edge, standing on the file above or
            // below the list until it arrived. So they are kept to what
            // they are for: the rows scrolled past, inside the band.
            if self.laid.is_some() {
                let [_, top, _, bottom] = box_of(room, cell);
                let (from, to) = scrolled_past(top, bottom, band.behind * cell.height);
                for quad in &mut self.quads[rows..] {
                    quad.flags |= ONLY_ROWS;
                    quad.radius = from;
                    quad.lower = to;
                }
            }
        }
        let rooms: Vec<[f32; 4]> = bands.iter().map(|band| box_of(band.room, cell)).collect();
        // And each band's bar, over a picture: the frame's copy of it is
        // clear where the column is the page, so the mark standing where it
        // is going showed through the one on its way there -- two marks.
        // Put back below, only where the moving one has run out.
        let hung = self.laid.is_some();
        let moving: Vec<[f32; 4]> = bands
            .iter()
            .zip(&rooms)
            .flat_map(|(band, room)| sliding(*room, panes, band.under))
            .chain(
                bands
                    .iter()
                    .filter(|_| hung)
                    .filter_map(|band| band.bar.map(|(bar, _)| box_of(bar, cell))),
            )
            .collect();
        self.covering(&moving);
        for (at, (band, room)) in bands.iter().zip(&rooms).enumerate() {
            // Less the rooms of the bands inside this one, so that a band with
            // one inside it -- a hover over the file it is about -- does not
            // take the inner one along on its own journey. See `left_out`.
            let others = left_out(&rooms, at);
            // Each piece by the layer its pixels are in: the page under
            // the first pane is held apart, so a row coming out from under
            // the list is the page's -- see `by_source`.
            let shift = band.behind * cell.height;
            let (page, elsewhere) = match band.under {
                true => (
                    first.filter(|pane| beneath(*room, *pane, true).is_some()),
                    &panes[usize::from(first.is_some())..],
                ),
                false => (None, &[][..]),
            };
            for part in sliding(*room, panes, band.under) {
                for piece in tiles(part, &others) {
                    for (piece, from) in by_source(piece, shift, page, elsewhere) {
                        match from {
                            Source::Frame => self.slid_piece(piece, *room, shift, 1.0),
                            Source::Page => self.slid_from_the_page(piece, *room, shift),
                            // Left to the page the band scrolled off, which
                            // is drawn under all of it -- except over a
                            // picture, where it is kept to the rows scrolled
                            // past, and a strip of the picture showed here.
                            // So drawn again for this piece alone.
                            Source::Nowhere if self.laid.is_some() => {
                                let across = band.room.left()..band.room.right();
                                let mut columns = across.filter(|&x| {
                                    f32::from(x) * cell.width < piece[2]
                                        && f32::from(x + 1) * cell.width > piece[0]
                                });
                                let Some(from) = columns.next() else {
                                    continue;
                                };
                                let to = columns.next_back().unwrap_or(from) + 1;
                                let start = self.quads.len();
                                self.scrolled_off(band, from..to, None, stack, fonts);
                                for quad in &mut self.quads[start..] {
                                    quad.flags |= ONLY_ROWS;
                                    quad.radius = piece[1];
                                    quad.lower = piece[3];
                                }
                            }
                            Source::Nowhere => {}
                        }
                    }
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
                let column = box_of(bar, cell);
                let shift = mark_behind(to_come, band.behind, band.since) * cell.height;
                // Where the moving copy runs out, the frame's own -- which
                // `covering` left out over a picture, and put back here only
                // in that strip, for the mark not to be drawn twice.
                if hung {
                    let (from, to) = scrolled_past(column[1], column[3], shift);
                    self.quads.push(Quad {
                        rect: [
                            column[0],
                            column[1],
                            column[2] - column[0],
                            column[3] - column[1],
                        ],
                        uv: [0.0; 4],
                        colour: [0.0, 0.0, 0.0, 1.0],
                        flags: FRAME | ONLY_ROWS,
                        radius: from,
                        lower: to,
                        layer: 0,
                    });
                }
                self.slid(column, shift, 1.0);
            }
        }
    }

    /// The page a band scrolled off, where it has got to -- further back
    /// than the band by however much of the move is already done -- in the
    /// band's rows and the columns `columns` of them.
    fn scrolled_off(
        &mut self,
        band: &Rolled<'_>,
        columns: std::ops::Range<u16>,
        glass: Option<usize>,
        stack: &[&Behind],
        fonts: &mut Fonts,
    ) {
        let cell = fonts.cell();
        let room = band.room;
        let offset = (band.behind - band.since) * cell.height;
        for y in room.top()..room.bottom() {
            for x in columns.clone() {
                let look = band.before.look(x, y);
                let left = f32::from(x) * cell.width;
                let top = f32::from(y).mul_add(cell.height, offset);
                // Nor the page over a picture, which stands where it
                // is under the band -- see `encode`.
                if glass.is_none_or(|level| look.background != stack[level].ground)
                    && !(glass.is_none() && self.on_the_picture(look.background))
                {
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
                    // The page over a picture is what the backdrop is drawn
                    // on before these are put back into it -- see `encode`.
                    if !self.on_the_picture(look.background) {
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
        let above = start..self.quads.len();
        let start = self.quads.len();
        // Less where the band is put back, over a picture: the frame is clear
        // there where it is the page, and the rows standing still showed
        // through the rows sliding -- every line twice, in the frost. Where
        // the slide runs out, what shows is the picture, through the blur.
        let pieces: Vec<[f32; 4]> = match self.laid {
            Some(_) => under.iter().map(|(_, piece)| *piece).collect(),
            None => Vec::new(),
        };
        self.covering(&pieces);
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
    pub(super) fn going(&mut self, along: Option<f32>, cell: CellSize) {
        let Some(along) = along else {
            return;
        };
        let shifts: Vec<([f32; 4], f32, Joined)> = self
            .gone
            .iter()
            .map(|&(pane, joined, _)| {
                let toward = match joined {
                    Joined::Below => 1.0,
                    _ => -1.0,
                };
                (pane, toward * along * (pane[3] - pane[1]) * TRAVEL, joined)
            })
            .collect();
        let start = self.quads.len();
        // Each one's shadow first, from the edge it has got back to and
        // going as it goes: the pieces below are cut to the pane, and a
        // shadow is the one part of a pane outside it, so taken with them
        // it was gone the moment the pane began to go. The way a pane
        // arriving casts from the edge it has reached -- see `lay`.
        for &(pane, shift, joined) in &shifts {
            if let Some(joined) = casting(joined) {
                self.shadow(
                    reached(pane, shift),
                    0.0,
                    joined,
                    cell.height * SHADOW_SPREAD,
                    1.0 - along,
                );
            }
        }
        let shifts: Vec<([f32; 4], f32)> = shifts
            .into_iter()
            .map(|(pane, shift, _)| (pane, shift))
            .collect();
        match self.left_lifted {
            // The glass stands where it was, over what it showed, and only
            // as much of it as the pane still covers: the slab goes and
            // what was behind it does not. Then what was written on it,
            // which goes with the slab.
            //
            // The glass was drawn once, as it stood, so the edge that moves
            // is cut straight rather than rounded and lit -- for the fifth
            // of a second it takes to fade.
            Some(_) => {
                for &(pane, shift) in &shifts {
                    self.slid_piece(reached(pane, shift), pane, 0.0, 1.0 - along);
                }
                let lifted = self.quads.len();
                for &(pane, shift) in &shifts {
                    self.slid(pane, shift, 1.0 - along);
                }
                self.placed.going_lifted = Some(lifted..self.quads.len());
                self.placed.going = Some(start..lifted);
            }
            None => {
                for &(pane, shift) in &shifts {
                    self.slid(pane, shift, 1.0 - along);
                }
                self.placed.going = Some(start..self.quads.len());
            }
        }
        if self
            .placed
            .going
            .as_ref()
            .is_some_and(|going| going.is_empty())
        {
            self.placed.going = None;
            self.placed.going_lifted = None;
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
    pub(super) fn slid_piece(&mut self, piece: [f32; 4], room: [f32; 4], shift: f32, fade: f32) {
        self.slid_from(piece, room, shift, fade, SLID);
    }

    /// The same, out of the page layer's own picture -- see `by_source`.
    fn slid_from_the_page(&mut self, piece: [f32; 4], room: [f32; 4], shift: f32) {
        self.placed.from_the_page.push(self.quads.len());
        self.slid_from(piece, room, shift, 1.0, SLID | UNDER_GLASS);
    }

    /// A piece taken from a picture `shift` pixels further up, as `flags`
    /// say which picture.
    fn slid_from(&mut self, piece: [f32; 4], room: [f32; 4], shift: f32, fade: f32, flags: u32) {
        let [left, top, far, low] = piece;
        self.quads.push(Quad {
            rect: [left, top, (far - left).max(1.0), (low - top).max(1.0)],
            uv: room,
            colour: [0.0, 0.0, 0.0, fade],
            flags,
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
        said: &Said<'_>,
        turn: Option<f32>,
        fonts: &mut Fonts,
    ) {
        let (barred, capped, spun) = (said.barred, said.capped, said.spun);
        let cell = fonts.cell();
        // The plates the page had under the pane -- the cursor's pair, what
        // a search found -- which are what a window draws those cells'
        // grounds as: left out, the page in this picture was a terminal's,
        // a square where the page has a plate, and a row coming out from
        // under the list wore the square until it arrived. Plates and no
        // light, which would reach out of the pane and onto nothing this
        // picture holds.
        let plates = plates_behind(behind, said.plated_behind);
        let corner = (cell.height * HELD_CORNER).min(cell.width / 2.0);
        for y in behind.area.top()..behind.area.bottom() {
            let ground = self.ground_colour();
            let (grounds, lettered) = behind_row(behind, barred, spun, y);
            // Where a plate is, the square under it would stand outside its
            // corners -- the hole `backgrounds` leaves for the page's own.
            let mut holes: Vec<(u16, u16)> = plates
                .iter()
                .filter(|area| area.y == y)
                .map(|area| (area.x, area.right()))
                .collect();
            for (start, end, colour) in grounds {
                // Straight after the picture under the page, which is the
                // page here already -- see `backgrounds`.
                if self.on_the_picture(colour) {
                    continue;
                }
                for (start, end) in super::ground::without(start, end, &mut holes) {
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
            }
            // Under the letters, which are what is on the plate.
            for area in plates.iter().filter(|area| area.y == y) {
                let Some(look) = behind.look(area.x, area.y) else {
                    continue;
                };
                let [r, g, b, _] = super::ground::lifted(
                    ground,
                    rgba(look.background, Ink::Background),
                    PAIR_LIFT,
                );
                self.rounded(
                    f32::from(area.x) * cell.width,
                    f32::from(area.y) * cell.height,
                    f32::from(area.width) * cell.width,
                    cell.height,
                    corner,
                    [r, g, b, 1.0],
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
        // And the marks that turn, for the same reason.
        for mark in spun {
            if let Some(look) = mark.behind(behind) {
                self.turn(mark, look, turn, cell);
            }
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
        let edge = |y: u16| edge_along(page, behind, ruled, y);
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
        // Up to the top of the window, where the pane hangs from the top of
        // the page under a bar in it: the bar is the page's there,
        // and a pane over the whole page with a strip of the page left
        // bare above it is a pane that stopped short. The tabs are under it,
        // with the page, and the buttons Obelus draws are drawn over it --
        // see `bar_on_top` in `lay`.
        if from_the_top(self.titled, behind.area.y, behind.joined) {
            top = -self.margin[1];
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
    /// and put back in pieces, what is on the pane slid in from where it
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
    ///
    /// `the_page_moves` says the page under the panes is catching up, and
    /// then their shadows are not drawn into the frame but with the panes,
    /// over the page once it has been put back -- see `panes_shadows`. A
    /// pane's shadow is the pane's, but it falls outside the pane, on the
    /// page; drawn into the frame it was in the page's picture, and went
    /// along with the rows it fell on while the list casting it stood
    /// still.
    pub(super) fn shadows(
        &mut self,
        stack: &[&Behind],
        sliding: Option<usize>,
        the_page_moves: bool,
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
            } else if sliding != Some(at) && !the_page_moves {
                self.pane_shadow(stack, at, spread);
            }
        }
    }

    /// The panes' shadows over a page that is catching up, drawn once the
    /// page has been put back from the frame -- the layer they belong to is
    /// the panes', which have not moved, so they stand where they are while
    /// the page goes under them. See `shadows`.
    pub(super) fn panes_shadows(&mut self, stack: &[&Behind], cell: CellSize) {
        let spread = cell.height * SHADOW_SPREAD;
        for (at, over) in stack.iter().enumerate() {
            if !over.is_a_box() {
                self.pane_shadow(stack, at, spread);
            }
        }
    }

    /// One pane's shadow, where it casts one.
    fn pane_shadow(&mut self, stack: &[&Behind], at: usize, spread: f32) {
        if casts(stack, at)
            && let Some(joined) = casting(stack[at].joined)
        {
            let glass = self.placed.levels[at].rect;
            self.shadow(glass, 0.0, joined, spread, 1.0);
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
    /// as the screen had them. Where those were glass further down -- a
    /// setting's choices are over the settings -- they are the letters
    /// alone, because the picture draws that glass before them and a ground
    /// under the letters would cover it.
    pub(super) fn seen_under(
        &mut self,
        over: &Behind,
        lower: &[&Behind],
        spun: &[Spun],
        turn: Option<f32>,
        fonts: &mut Fonts,
    ) {
        self.seen_under_rows(
            over,
            lower,
            spun,
            turn,
            fonts,
            over.area.top()..over.area.bottom(),
        );
    }

    /// The same picture, of some of its rows.
    pub(super) fn seen_under_rows(
        &mut self,
        over: &Behind,
        lower: &[&Behind],
        spun: &[Spun],
        turn: Option<f32>,
        fonts: &mut Fonts,
        rows: std::ops::Range<u16>,
    ) {
        let cell = fonts.cell();
        for y in rows.clone() {
            for x in over.area.left()..over.area.right() {
                let Some(under) = over.look(x, y) else {
                    continue;
                };
                let left = f32::from(x) * cell.width;
                let top = f32::from(y) * cell.height;
                if !seen_through(lower, x, y, under.background) {
                    let ground = self.under(lower, &[], x, y);
                    let held = as_held(under.background, self.holding, ground);
                    self.ground_back([left, top, cell.width, cell.height], under.background, held);
                }
                // No bars: a picture this far up draws a bar's block as a
                // letter, the way the page it was taken from did.
                if lettered_behind(under, &[], spun, over.depth, x, y) {
                    let ink = rgba(under.foreground, Ink::Foreground);
                    self.glyphs_at((left, top), under, ink, 0, fonts, Size::Cell);
                }
            }
        }
        for mark in spun.iter().filter(|mark| rows.contains(&mark.area.y)) {
            if let Some(look) = mark.behind(over) {
                self.turn(mark, look, turn, cell);
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
        fonts: &mut Fonts,
    ) -> (usize, [f32; 4]) {
        let (inside, radius) = self.frame(page, card, lower, fonts);
        // Over a pane where it is over one at all, which a hover over the
        // code never is.
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

/// The rule along one row of a pane that is its edge, where there is one
/// and the row still says it.
fn edge_along<'a>(page: &Page, pane: &Behind, ruled: &'a [Ruled], y: u16) -> Option<&'a Ruled> {
    ruled.iter().find(|rule| {
        rule.area.y == y
            && rule.area.x == pane.area.x
            && rule.area.width == pane.area.width
            && rule.still_said(page)
    })
}

/// The row of what a pane was put over that goes on the screen as well
/// as into its picture: the half of it above the pane's line, which the
/// glass leaves bare -- see `pane_glass`.
///
/// Only over another pane. The first one's picture is on the screen whole
/// already (`pane_under`); every other one's is drawn into its picture and
/// nowhere else, so above its line was the screen with the rule's cells
/// in it -- which are a line and a ground. A list opened over the settings
/// cut the setting its rule fell across in half: the lower half under the
/// glass, and nothing at all above it. The palette over the settings did
/// the same, and so did a setting's choices; the palette over the file
/// never did, because there it is the first pane.
pub(super) fn above_its_line(
    page: &Page,
    pane: &Behind,
    ruled: &[Ruled],
    level: usize,
) -> Option<u16> {
    (level > 0 && !pane.is_a_box() && edge_along(page, pane, ruled, pane.area.y).is_some())
        .then_some(pane.area.y)
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
    spun: &[Spun],
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
                .is_some_and(|under| lettered_behind(under, barred, spun, behind.depth, x, y))
        })
        .collect();
    (grounds, lettered)
}

/// Whether the picture of what a pane was opened over writes this cell.
///
/// The same question `letters` asks of the page, asked of the picture: a
/// bar said before the pane is a shape, and the block a terminal has for
/// its mark is a glyph nobody wanted here, under the tint.
///
/// Only a bar said before the pane, which is `depth` -- how many panes and
/// boxes had been put over the page when this one was. A bar said on the
/// pane or over it is not in this picture at all, and the letters under
/// its column are the picture's to draw.
///
/// It was not merely redundant. A full block's raster is taller than its
/// cell, and the glass covers the pane's own rectangle exactly, so the
/// part of the first row's block that reached above the grid came out in
/// the margin round it -- a dark cell's-width line along the top of the
/// window, wherever a list was opened over a file long enough to have a
/// bar.
///
/// And the mark that turns is drawn as an arc over the picture, the way it
/// is over the page, so its braille is not written under it.
pub(super) fn lettered_behind(
    under: Look<'_>,
    barred: &[Barred],
    spun: &[Spun],
    depth: usize,
    x: u16,
    y: u16,
) -> bool {
    !under.text.trim().is_empty()
        && !barred.iter().any(|showing| {
            let area = showing.bar.area;
            showing.depth <= depth
                && (area.left()..area.right()).contains(&x)
                && (area.top()..area.bottom()).contains(&y)
        })
        && !turns_at(under, spun, depth, x, y)
}

/// Whether this cell of a picture is a mark that turns: said to be one
/// before the pane, and still holding a frame of it -- see `Spun::behind`.
fn turns_at(under: Look<'_>, spun: &[Spun], depth: usize, x: u16, y: u16) -> bool {
    obelus_ui::how_far_round(under.text).is_some()
        && spun
            .iter()
            .any(|mark| mark.depth <= depth && mark.area.x == x && mark.area.y == y)
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

/// The rows of a band from `top` to `bottom` that its slide has scrolled
/// past, as the top and bottom of them, in pixels.
///
/// The band is taken from `shift` further up the frame -- see `slid_piece`
/// -- so it has nothing to show in the first `shift` of its rows, or the
/// last where it slides the other way. That strip is what the page it
/// scrolled off is drawn for, and nothing outside it is: the rest of the
/// band is the band, and beyond its edges is somebody else's.
pub(super) fn scrolled_past(top: f32, bottom: f32, shift: f32) -> (f32, f32) {
    if shift >= 0.0 {
        (top, (top + shift).min(bottom))
    } else {
        ((bottom + shift).max(top), bottom)
    }
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

/// The cells of the page's plates a pane was put over -- halves of the
/// cursor's pair, runs a search found -- which the picture of what it was
/// put over draws as plates, under their letters (`Painter::pane_under`).
pub(super) fn plates_behind(
    behind: &Behind,
    (paired, found): (&[crate::grid::Paired], &[crate::grid::Finding]),
) -> Vec<ratatui::layout::Rect> {
    paired
        .iter()
        .filter(|half| half.still_behind(behind))
        .map(|half| half.area)
        .chain(
            found
                .iter()
                .filter(|run| run.still_behind(behind))
                .map(|run| run.area),
        )
        .collect()
}

/// The rooms of the bands that are not the one at `at` and do not hold it:
/// what a band catching up leaves out of what it puts back, because each is
/// put back by its own band.
///
/// Not every other band's. A band inside another -- a list in its pane
/// over the page scrolling under it -- left out the room of the band round
/// it, which is all of its own, so it put nothing back: the list was a
/// blank for as long as the page took to arrive, whenever the two scrolled
/// at once.
///
/// Two bands that cross, neither holding the other, each leave the other's
/// room out, and what they share is put back by neither for the length of
/// the slide -- as it always was. No view lays two scrolling things across
/// each other; a list is inside what it is over or beside it.
pub(super) fn left_out(rooms: &[[f32; 4]], at: usize) -> Vec<[f32; 4]> {
    let Some(&own) = rooms.get(at) else {
        return Vec::new();
    };
    let holds = |room: &[f32; 4]| {
        room[0] <= own[0] && room[1] <= own[1] && room[2] >= own[2] && room[3] >= own[3]
    };
    rooms
        .iter()
        .enumerate()
        .filter(|&(other, room)| other != at && !holds(room))
        .map(|(_, room)| *room)
        .collect()
}

/// Which layer a piece of a band slid into place takes its pixels from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Source {
    /// The frame, which is the page wherever no pane is over it.
    Frame,
    /// The page layer alone, under the first pane -- see `standing`.
    Page,
    /// Nothing: under a pane whose page no picture holds. Left undrawn, so
    /// what shows is the page the band scrolled off, at the place it has
    /// got to -- see `catching_up`.
    Nowhere,
}

/// `piece` cut by where its pixels come from, when it is taken from
/// `shift` pixels further up the frame -- the pixel at `y` is the frame's
/// at `y - shift`, so a source under a pane is a piece as far down from it.
///
/// What the layers are for, kept while the page moves. The frame has the
/// panes composed over the page, so the rows of a band that come from
/// under a pane, taken out of it, were the pane: scrolling back under a
/// list on the status bar, the list's top edge was drawn again above the
/// list and shrank away, and the rows that belonged there arrived only
/// when the slide did. `page` is where the page layer is held apart --
/// under the first pane -- and `elsewhere` the other panes, which hold no
/// page of their own.
pub(super) fn by_source(
    piece: [f32; 4],
    shift: f32,
    page: Option<[f32; 4]>,
    elsewhere: &[[f32; 4]],
) -> Vec<([f32; 4], Source)> {
    let moved = |hole: [f32; 4]| [hole[0], hole[1] + shift, hole[2], hole[3] + shift];
    let holes = page
        .map(|hole| (moved(hole), Source::Page))
        .into_iter()
        .chain(elsewhere.iter().map(|hole| (moved(*hole), Source::Nowhere)));
    let mut found = Vec::new();
    let mut rest = vec![piece];
    for (hole, from) in holes {
        let mut kept = Vec::with_capacity(rest.len());
        for part in rest {
            let inside = [
                part[0].max(hole[0]),
                part[1].max(hole[1]),
                part[2].min(hole[2]),
                part[3].min(hole[3]),
            ];
            if inside[0] < inside[2] && inside[1] < inside[3] {
                found.push((inside, from));
            }
            kept.extend(tiles(part, &[hole]));
        }
        rest = kept;
    }
    found.extend(rest.into_iter().map(|part| (part, Source::Frame)));
    found
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

/// Asked here, and by the window of the tabs -- see `title::from_the_top`.
pub(super) use crate::title::from_the_top;

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
