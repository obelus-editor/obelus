//! A frame, laid out: every quad in the order it is drawn, which is what
//! puts each thing over what it is over.

use super::{
    glass::{casting, reached},
    *,
};
use crate::{
    font::Fonts,
    grid::{Capped, Marked, Page, Said, Spelling, Spun, Ticked},
    motion::Moving,
};

impl Painter {
    /// Lays a page out as the quads that draw it, and hands them to the
    /// device.
    pub(super) fn lay(
        &mut self,
        page: &Page,
        fonts: &mut Fonts,
        spelling: Option<&Spelling>,
        moving: Moving,
        said: Said<'_>,
    ) {
        let cell = fonts.cell();
        // Before anything is placed, which is the one moment starting the
        // glyphs again is safe: nothing from the last frame is read after it.
        if std::mem::take(&mut self.atlas.overflowed) {
            self.atlas.empty();
        }
        self.quads.clear();
        self.placed = Placed::default();
        #[expect(
            clippy::cast_precision_loss,
            reason = "a window is thousands of pixels, not millions"
        )]
        let (across, down) = (self.configured.width as f32, self.configured.height as f32);
        let margin = crate::grid::origin(
            [across, down],
            self.titled,
            [cell.width, cell.height],
            [page.columns(), page.rows()],
        );
        self.grid = [
            f32::from(page.columns()) * cell.width,
            f32::from(page.rows()) * cell.height,
        ];
        self.margin = margin;
        // The whole window, in the page's own ground, under everything.
        //
        // The cells are all one size and the grid is middled in the
        // window, so there is a margin round it that no cell reaches: this
        // is what is in it. Before the pane's own cells, so it is in the
        // picture taken of what is behind one as well -- where it used to
        // be the black the pass clears to.
        let [left, top, wide, tall] = whole_window([across, down], margin);
        self.block(left, top, wide, tall, rgba(self.ground, Ink::Background));
        // Every pane and box, furthest first, each over the glass of those
        // before it. A box is one only while its frame is still there to
        // hold it -- see `Behind::framed` -- which four passes ask about.
        let stack = uncovered(
            said.stack
                .iter()
                .filter(|over| !over.is_a_box() || over.framed(page))
                .collect(),
        );
        // The pane on top, which is the one that arrives: a list opened
        // over the settings comes up over a page that was already there.
        let top = stack.iter().rposition(|over| !over.is_a_box());
        // The bands put back from further up the frame, which is all of
        // them unless a pane is arriving -- then the pane is what moves.
        // One answer, read by the glass under them and by the moving.
        let catching_up = match (top, moving.pane) {
            (Some(_), Some(_)) => &[][..],
            _ => said.bands,
        };
        // First of everything, because the first of these is what the
        // first picture draws and it draws the front of the buffer.
        for (at, over) in stack.iter().enumerate() {
            let lower = &stack[..at];
            let start = match at {
                // The window's own ground is under the first one too.
                0 => 0,
                _ => self.quads.len(),
            };
            match (at, over.is_a_box()) {
                (0, false) => self.pane_under(over, said.barred, said.capped, fonts),
                _ => self.seen_under(over, lower, fonts),
            }
            let under = start..self.quads.len();
            let (glass, rect) = match over.is_a_box() {
                true => self.box_glass(page, over, lower, cell),
                false => self.pane_glass(page, over, lower, said.ruled, cell),
            };
            let lowered = self.glass_kept_still(glass, at, &stack, catching_up, cell);
            self.placed.levels.push(Level {
                under,
                glass,
                lowered,
                end: self.quads.len(),
                rect,
                pane: !over.is_a_box(),
                area: over.area,
                blur: 0,
            });
        }
        let pane = top.map(|top| self.placed.levels[top].rect);
        // And the first, where it is a pane: a band scrolling under glass
        // is put back into the first picture -- see `sliding_under`.
        let first = stack.first().copied().filter(|over| !over.is_a_box());
        // The shapes the page still holds -- see `Ticked::still_said` and
        // the one beside it, and `Capped`'s a few lines down, which is
        // kept apart only because the caps are wanted as borrows. Asked
        // here rather than where each is drawn, so that the cells
        // `letters` leaves alone are the cells a shape is drawn over.
        //
        // A bar is not among them, and answers the same question finer:
        // where a switch and a mark are one cell and a run of them either
        // is or is not still there, a bar is a column and the thing over
        // it may take the middle of it -- so it is asked row by row, at
        // the two places it matters, and `Barred::runs` is that answer.
        let ticked: Vec<Ticked> = said
            .ticked
            .iter()
            .copied()
            .filter(|tick| tick.still_said(page))
            .collect();
        let marked: Vec<Marked> = said
            .marked
            .iter()
            .filter(|mark| mark.still_said(page))
            .cloned()
            .collect();
        let spun: Vec<Spun> = said
            .spun
            .iter()
            .copied()
            .filter(|mark| mark.round(page).is_some())
            .collect();
        let said = Said {
            ticked: &ticked,
            spun: &spun,
            marked: &marked,
            ..said
        };
        let panes = &stack;
        let framed: Vec<&Behind> = stack
            .iter()
            .copied()
            .filter(|over| over.is_a_box())
            .collect();
        // The caps still about the cells they were said about -- see
        // `Capped::still_said`.
        let capped: Vec<&Capped> = said
            .capped
            .iter()
            .filter(|cap| cap.still_said(page))
            .collect();
        self.backgrounds(page, cell, panes, &framed, &capped);
        // Over the page and under everything written on it: a hold is a
        // ground, and the letters it is behind are the ones the reader is
        // holding.
        self.holdings(page, panes, &framed, &capped, cell);
        self.rules(page, said.ruled, cell);
        // The boundaries with no row to be on, which go where there is no
        // cell: the pixel between one row and the one above it. Under the
        // letters, like a rule, because a line through a letter is a line
        // nobody put there -- and there is nothing on that pixel to be
        // under except the ground.
        self.partings(page, said.parted, panes, cell);
        self.letters(page, fonts, said, &framed, &capped);
        // Over the glyph, the way a terminal draws one: a descender
        // crossing the line is what an underline looks like everywhere
        // else.
        self.underlines(page, cell);
        // Over the text, which it covers: a cap is the shape the cells
        // behind a key are, and it writes the key on itself, smaller than
        // the words beside it.
        self.caps(page, &capped, panes, &framed, fonts);
        // Over the letters: a switch replaces the glyph standing in for
        // it, rather than sitting beside one.
        self.ticks(page, said.ticked, panes, &framed, &capped, cell);
        // And so does the mark that turns: what a terminal turns a frame a
        // tick, a window turns at whatever rate it draws.
        self.turns(page, said.spun, moving.turn, cell);
        // And so does a bar, for the same reason: what a terminal has for
        // a track is a column of full blocks, and a window has a shape.
        self.bars(page, said.barred, cell);
        // And so does a change mark: half a block per row is what a cell
        // has, and a window has one shape however many rows the hunk
        // covers.
        self.strokes(page, said.stroked, cell);
        // After the text, over cells the view left empty: a view draws its
        // glyph only where a picture could not be drawn.
        self.marks(said.marked, cell);
        // And after all of it, because a shadow falls on what is behind
        // the thing casting it and everything behind these has now been
        // drawn. Before the caret, which is the reader's own place and
        // is never in shadow.
        self.shadows(
            &stack,
            top.filter(|_| moving.pane.is_some()),
            moving.card.unwrap_or(1.0),
            cell,
        );
        // Over the cells and under the caret: the word being spelled is
        // going in at the caret, so the caret belongs at the place in it
        // the input method says.
        self.spelling(page, fonts, spelling);
        // Off for half of every cycle, which is the blink. What is under it
        // is drawn either way, by the pass above.
        if moving.caret {
            self.caret(page, fonts, spelling, moving.drift);
        }

        // A box put over the page travels nowhere, so what it does
        // instead is come up. The cells it was put over are already in a
        // picture of their own -- the one its glass reads -- so laying
        // those back over it, fading out, is the box fading in, and
        // nothing has to be drawn a second time. Over everything of the
        // box's, the caret in it included: a solid caret on a box that is
        // half there is the one part of it that has already arrived.
        let nearest = stack.iter().rposition(|over| over.is_a_box());
        if let (Some(nearest), Some(along)) = (nearest, moving.card) {
            self.placed.covered = Some((self.quads.len(), nearest));
            self.slid(box_of(stack[nearest].area, cell), 0.0, 1.0 - along);
        }

        // Everything the frame says has been said. What is left is
        // putting it back on the screen in two pieces, where a pane is on
        // its way in -- see `paint.wgsl`.
        self.placed.drawn = self.quads.len();
        if let (Some(top), Some(pane), Some(along)) = (top, pane, moving.pane) {
            let height = pane[3] - pane[1];
            // A pane comes from the side it is joined to, which is the
            // only side it could come from without crossing the page.
            let away = match Some(stack[top].joined) {
                Some(Joined::Below) => 1.0,
                _ => -1.0,
            };
            let shift = away * (1.0 - along) * height * TRAVEL;
            self.placed.composed = true;
            self.covering(&[pane]);
            // Under the one arriving, which comes over it: the files come
            // down over the palette going.
            self.going(moving.leaving);
            self.slid(pane, shift, along);
            // And the shadow, here rather than in the frame -- see
            // `shadows`. It falls from the edge the pane has *reached*,
            // which is the only edge of it that has moved: the other is
            // the seam it is joined by, and the piece the pane is taken
            // from stops there. And it comes up as the pane does, because
            // a shadow at full strength under a pane that is still half
            // there is a shadow with nothing casting it.
            if let Some(joined) = casting(stack[top].joined) {
                self.shadow(
                    reached(pane, shift),
                    0.0,
                    joined,
                    cell.height * SHADOW_SPREAD,
                    along,
                );
            }
        } else if !catching_up.is_empty() {
            // The pane's cells rather than its glass, which starts half
            // way down the rule over it: the half row above the line,
            // slid, would be filled from inside the list.
            let panes: Vec<[f32; 4]> = stack
                .iter()
                .filter(|over| !over.is_a_box())
                .map(|over| box_of(over.area, cell))
                .collect();
            self.placed.composed = true;
            self.catching_up(catching_up, &panes, &stack, fonts);
            self.going(moving.leaving);
        } else {
            self.going(moving.leaving);
        }
        // Last, because none of it is drawn on the screen: the blurs are
        // passes of their own, before any of the above.
        self.placed.moved = self.quads.len();
        for at in 0..self.placed.levels.len() {
            self.placed.levels[at].blur = self.blurs_over(self.placed.levels[at].rect);
        }
        if moving.pane.is_none()
            && let Some(first) = first
        {
            self.sliding_under(page, first, said.bands, fonts);
        }
        // After the last glyph has been placed and before anything is drawn:
        // a glyph that found no room made the texture again, and every
        // binding is still reading the one before.
        if std::mem::take(&mut self.atlas.remade) {
            self.bound_again();
        }
        // And a frame that went without a glyph asks for the one that will
        // have it, rather than waiting for a key the reader has no reason
        // to press.
        if self.atlas.overflowed {
            self.window.request_redraw();
        }

        // Where the light is, in the pixels a fragment knows itself by:
        // the mark's own left edge plus how far along it the clock has
        // brought it, and the grid's origin, because a fragment's `x` is
        // the screen's rather than the grid's.
        let (sheen, glow) = match said.sheened.zip(moving.sheen) {
            Some((mark, along)) => {
                let left = f32::from(mark.area.x) * cell.width;
                let wide = f32::from(mark.area.width) * cell.width;
                (
                    [
                        margin[0] + left + wide * along,
                        (wide * crate::motion::SHEEN_WIDTH).max(1.0),
                        1.0,
                        0.0,
                    ],
                    rgba(mark.to, Ink::Foreground),
                )
            }
            // Nothing carried, so the letters keep the colour they were
            // given: the mark at rest is the mark in one colour.
            None => ([0.0, 1.0, 0.0, 0.0], [0.0; 4]),
        };
        #[expect(
            clippy::cast_precision_loss,
            reason = "a window is thousands of pixels, not millions"
        )]
        let screen = Screen {
            size: [self.configured.width as f32, self.configured.height as f32],
            origin: margin,
            sheen,
            glow,
            held: [
                (cell.height * HELD_REACH).round(),
                cell.height * HELD_CORNER,
                0.0,
                0.0,
            ],
        };
        self.queue
            .write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&screen));

        let wanted = (std::mem::size_of::<Quad>() * self.quads.len().max(1)) as u64;
        if wanted > self.instances.size() {
            // Doubled, so that a screen that keeps growing does not
            // reallocate on every frame of the resize.
            self.instances = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("obelus quads"),
                size: wanted * 2,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        self.queue
            .write_buffer(&self.instances, 0, bytemuck::cast_slice(&self.quads));

        self.levels_for(self.placed.levels.len());
    }
}
