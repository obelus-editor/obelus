//! What a view said a region is -- a rule, a frame, a bar, a key's cap --
//! drawn as itself rather than spelled in the characters a terminal would
//! use.

use obelus_font::{self as font, Fonts, Size};
use obelus_ui::shapes::{About, Side};
use ratatui::style::Modifier;

use super::*;
use crate::grid::{Barred, Capped, Look, Page, Parted, Ruled, Spun, Stroked, Ticked};

impl Painter {
    /// The line under every cell a view underlined.
    ///
    /// A pass of its own rather than something `letters` does, because an
    /// underline is about the cell and not about the glyph: a span a
    /// server complained about runs over the spaces in it too, and
    /// `letters` skips a cell with nothing in it. A terminal draws these
    /// itself, off the modifier; a window has to be told, which is the
    /// same seam a full-width character crosses.
    pub(super) fn underlines(&mut self, page: &Page, cell: CellSize) {
        for row in 0..page.rows() {
            for column in 0..page.columns() {
                let look = page.look(column, row);
                let Some(ink) = underline_ink(&look) else {
                    continue;
                };
                self.underline(
                    f32::from(column) * cell.width,
                    f32::from(row) * cell.height,
                    f32::from(look.columns()) * cell.width,
                    cell.height,
                    ink,
                );
            }
        }
    }

    /// One line under one run of cells.
    ///
    /// Two callers and one line: what a server says is wrong with a word,
    /// and what an input method is in the middle of spelling. They are the
    /// same mark, and a window that drew them two thicknesses would be
    /// saying they were two different things.
    pub(super) fn underline(
        &mut self,
        left: f32,
        top: f32,
        width: f32,
        height: f32,
        ink: [f32; 4],
    ) {
        let thick = (height * UNDERLINE).round().max(1.0);
        self.block(left, top + height - thick, width, thick, ink);
    }

    /// The lines between two things, drawn where `─` would be.
    ///
    /// On the cells' own ground, which `backgrounds` has painted, and in
    /// their own ink, which the view wrote -- the glyph is what is left
    /// out, by `letters`. Cell by cell, because a rule some later view
    /// covered part of is a line only where it was not covered.
    pub(super) fn rules(&mut self, page: &Page, ruled: &[Ruled], cell: CellSize) {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a window is thousands of pixels, not millions"
        )]
        let right = self.configured.width as f32;
        let line = thickness(cell.height);
        for rule in ruled {
            let y = rule.area.y;
            let top = middle(f32::from(y) * cell.height, cell.height, line);
            for x in rule.area.left()..rule.area.right() {
                let Some((from, to)) = rule.spans(page, x, y) else {
                    continue;
                };
                let left = f32::from(x) * cell.width;
                // The strip past the last whole cell, which a line that
                // reaches the window's edge has to reach as well -- see
                // `backgrounds`.
                let end = match x + 1 == page.columns() && to >= 1.0 {
                    true => right,
                    false => cell.width.mul_add(to, left),
                };
                let start = cell.width.mul_add(from, left);
                self.block(
                    start,
                    top,
                    (end - start).max(1.0),
                    line,
                    rgba(page.look(x, y).foreground, Ink::Foreground),
                );
            }
        }
    }

    /// The lines between one thing and the next where there is no row for
    /// one.
    ///
    /// Drawn at the top edge of the row that begins the new thing, which
    /// is the boundary itself: a grid has no space there and a window has
    /// a pixel. What a terminal does about the same fact is nothing --
    /// see `obelus_ui::shapes::parted`, which is the one thing said on
    /// that channel with no answer of its own in the cells.
    ///
    /// Only where nothing has been put over the row. A `Parted` is said by
    /// a whole-screen view, and what covers one is a pane or a box with a
    /// frame round it -- a line drawn on either would be the page
    /// underneath reaching through.
    pub(super) fn partings(
        &mut self,
        page: &Page,
        parted: &[Parted],
        over: &[&Behind],
        cell: CellSize,
    ) {
        // Half a rule's, which on a screen drawn at twice its own pixels
        // is one of the reader's. `thickness` is a rule's own, and a rule
        // *is* the row it is on; a single device pixel is half a pixel of
        // theirs, which on such a screen is a line nobody sees.
        let line = (thickness(cell.height) / 2.0).round().max(1.0);
        for parting in parted {
            if !parting.still_said(over) {
                continue;
            }
            let left = f32::from(parting.area.x) * cell.width;
            let width = f32::from(parting.area.width) * cell.width;
            // Through the middle of the row, which is the blank between
            // two notes: on its top edge the line would hug whatever is
            // above it, and a boundary that belongs to one side of itself
            // is read as that side's underline.
            let top = f32::from(parting.area.y) * cell.height;
            let middle = (top + (cell.height - line) / 2.0).round();
            // The row's own two colours rather than the page's. They are
            // the page's here, because the row is blank and is nowhere
            // the reader can stand -- but a seam is drawn against what it
            // is drawn on, and asking the cell is how that stays true of
            // wherever this is said next.
            let look = page.look(parting.area.x, parting.area.y);
            self.block(
                left,
                middle,
                width.max(1.0),
                line,
                mixed(
                    rgba(look.background, Ink::Background),
                    rgba(look.foreground, Ink::Foreground),
                    SEAM,
                ),
            );
        }
    }

    /// A box's frame, drawn as the shape a terminal spells in `╭─╮`: the
    /// line where the glyphs put it, down the middle of the ring of cells,
    /// and outside it what the box was put over, where a terminal has its
    /// square corners.
    ///
    /// What was put over is the ground of the cells under the ring, and
    /// not their letters: a letter a line cuts in half is not something
    /// anybody put there. Nor that ground where it is glass, which is
    /// already under it -- an opaque strip of it would be the one square
    /// edge left on a round box.
    ///
    /// What goes inside the line is the caller's, the box's own ground or
    /// glass, so what this answers is where that is: the rectangle inside
    /// the line, and how round it is.
    pub(super) fn frame(
        &mut self,
        page: &Page,
        card: &Behind,
        panes: &[&Behind],
        cell: CellSize,
    ) -> ([f32; 4], f32) {
        let area = card.area;
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                let Some(under) = card.look(x, y) else {
                    continue;
                };
                if card.ring_holds(page, x, y) && !seen_through(panes, x, y, under.background) {
                    let ground = self.under(panes, &[], x, y);
                    self.block(
                        f32::from(x) * cell.width,
                        f32::from(y) * cell.height,
                        cell.width,
                        cell.height,
                        as_held(under.background, self.holding, ground)
                            .unwrap_or_else(|| rgba(under.background, Ink::Background)),
                    );
                }
            }
        }
        let line = thickness(cell.height);
        let [left, top, right, bottom] = outline(area, cell, line);
        let radius = cell.width * FRAME_CORNER;
        self.rounded(
            left,
            top,
            right - left,
            bottom - top,
            radius,
            rgba(page.look(area.x, area.y).foreground, Ink::Foreground),
        );
        (
            [
                left + line,
                top + line,
                (right - left - line * 2.0).max(1.0),
                (bottom - top - line * 2.0).max(1.0),
            ],
            (radius - line).max(0.0),
        )
    }

    /// The switches, drawn as the box a glyph was standing in for.
    ///
    /// After the letters, because what it goes over is that glyph: a
    /// terminal has one cell and one character to say this in, and what
    /// it says there is the whole of the answer -- this is the same
    /// answer drawn rather than spelled.
    ///
    /// Set is a filled box with the mark cut out of it; not is the same
    /// box with its middle taken back out, which leaves an outline. One
    /// shape either way, because a pair that changed shape would put a
    /// jog in a column read straight down -- which is what `tick` says
    /// about the two glyphs, for the same reason.
    /// The bars, as capsules rather than as the blocks a terminal has.
    ///
    /// Nothing is covered over. `letters` is told to leave a bar's cells
    /// alone, the way it leaves a rule's and a cap's, so the blocks a
    /// terminal draws are never put on the screen here at all -- and then
    /// what is behind the capsule is whatever the page already had there.
    ///
    /// Which is the whole of why it is done that way round. This did once
    /// draw the blocks and paint over them with the cell's own background,
    /// and a bar inside a pane sits on *glass*: the cells there carry the
    /// pane's own colour, which `backgrounds` deliberately does not paint
    /// because painting it is covering up what the reader is meant to see
    /// through. The cover put it back, opaque, in one strip down the side
    /// of every list. `catching_up` has the same note for the same reason,
    /// one bug earlier.
    ///
    /// The colours are the cells' own, which is the rule a switch follows
    /// and for the same reason: what a terminal draws the blocks in is
    /// what a window draws the capsules in, and asking the view for them
    /// again would be the same two colours from two places.
    pub(super) fn bars(&mut self, page: &Page, barred: &[Barred], cell: CellSize) {
        for showing in barred {
            let bar = showing.bar;
            if bar.area.width == 0 || bar.area.height == 0 {
                continue;
            }
            // A pointer on it beats the settling: the reader is reaching
            // for the thing, and a control that went on fading under the
            // hand reaching for it is the one moment it must not.
            // `shown` already carries the pointer's own brightening, which
            // is the louder of the two: a bar under the pointer stays up
            // however long ago it moved.
            let shown = showing.shown.clamp(0.0, 1.0);
            let under = showing.under.clamp(0.0, 1.0);
            let column = bar.area.x;
            let capsule = |width: f32| {
                let width = (cell.width * width).round().max(2.0);
                (
                    f32::from(column) * cell.width + (cell.width - width) / 2.0,
                    width,
                )
            };

            // Only the rows that are still the bar's. A card goes over
            // the page before this pass, so a panel as wide as the editor
            // has its own right-hand edge in this very column -- and the
            // capsule was drawn over it. The same question a rule asks of
            // each of its cells.
            let runs = showing.runs(page);
            if runs.is_empty() {
                continue;
            }

            // A row the mark does not cover, which is where the track's
            // colour is. There may be none -- a mark as long as its track
            // -- and then there is no track to draw either: every pixel of
            // it would be under the mark.
            if let Some(row) = (0..bar.area.height)
                .find(|row| *row < bar.mark || *row >= bar.mark.saturating_add(bar.thumb))
            {
                let look = page.look(column, bar.area.y + row);
                // The track goes out altogether, which the mark may not:
                // what it says is how far the mark can travel, and the
                // column it is in says that much on its own.
                let (left, width) = capsule(BAR_TRACK);
                let ink = mixed(
                    rgba(look.background, Ink::Background),
                    rgba(look.foreground, Ink::Foreground),
                    shown,
                );
                for (top, rows) in &runs {
                    self.rounded(
                        left,
                        f32::from(*top) * cell.height,
                        width,
                        f32::from(*rows) * cell.height,
                        width / 2.0,
                        ink,
                    );
                }
            }

            // Two widths in one: how far up the scrolling has brought it,
            // and then how far the pointer has taken it past that. The
            // second is laid over the first rather than chosen instead of
            // it, so a bar the pointer arrives on while it is still moving
            // widens from where it is and not from where it would have
            // been standing still.
            let moved = BAR_MARK_RESTING + (BAR_MARK - BAR_MARK_RESTING) * shown;
            let wide = moved + (BAR_MARK_UNDER - moved) * under;
            let (left, width) = capsule(wide);
            let top = bar.area.y.saturating_add(bar.mark);
            let look = page.look(column, top);
            let ink = mixed(
                rgba(look.background, Ink::Background),
                rgba(look.foreground, Ink::Foreground),
                BAR_RESTING + (1.0 - BAR_RESTING) * shown,
            );
            // And the mark only where the column is still the bar's, the
            // same as the track: half a mark is where the reader is, and a
            // mark drawn across whatever covered it is not.
            let wanted = top..top.saturating_add(bar.thumb);
            for (run, rows) in &runs {
                let from = (*run).max(wanted.start);
                let to = run.saturating_add(*rows).min(wanted.end);
                if from >= to {
                    continue;
                }
                self.rounded(
                    left,
                    f32::from(from) * cell.height,
                    width,
                    f32::from(to - from) * cell.height,
                    width / 2.0,
                    ink,
                );
            }
        }
    }

    /// The marked runs, as strokes rather than as the half blocks a
    /// terminal has.
    ///
    /// Nothing is covered over, the same as a bar: `letters` is told to
    /// leave these cells alone, so the blocks a terminal draws are never
    /// put on the screen here at all. Which matters for the same reason it
    /// matters there -- a margin inside a pane sits on glass, and a cover
    /// painted in the cell's own ground would be a hole in it.
    ///
    /// The colour is the cell's own, which is the rule a bar and a switch
    /// both follow: what a terminal draws the block in is what a window
    /// draws the stroke in, and asking the view again would be the same
    /// colour from two places.
    pub(super) fn strokes(&mut self, page: &Page, stroked: &[Stroked], cell: CellSize) {
        for mark in stroked {
            let column = mark.stroke.area.x;
            // Against the edge nearest the text, which is the side the
            // view said -- see `obelus_ui::shapes::Side`.
            let width = (cell.width * STROKE).round().max(2.0);
            let left = match mark.stroke.side {
                Side::Left => f32::from(column) * cell.width,
                Side::Right => f32::from(column + 1) * cell.width - width,
            };
            // Only the rows still this stroke's, and in the runs they are
            // left in: a hunk whose middle rows were drawn over is two
            // strokes, and one bar spanning the hole would be a mark on
            // somebody else's cells.
            for (top, rows) in mark.runs(page) {
                let ink = rgba(page.look(column, top).foreground, Ink::Foreground);
                match mark.stroke.about {
                    // A bar down the rows, with its ends rounded: a hunk of
                    // six lines is one stroke and not six beads, which is
                    // the whole reason the view says it in runs.
                    About::Rows => self.rounded(
                        left,
                        f32::from(top) * cell.height,
                        width,
                        f32::from(rows) * cell.height,
                        width / 2.0,
                        ink,
                    ),
                    // An arrow on the boundary above the row, pointing the
                    // way the stroke leans -- which is at the text, and so
                    // at the place the missing lines were. The rows are
                    // there and the lines between them are not, so the one
                    // thing this must not be is a mark *on* a row.
                    About::Seam => {
                        let reach = (cell.width * SEAM_REACH).round().max(2.0);
                        let height = (reach * SEAM_BASE).round().max(2.0);
                        let (at, way) = match mark.stroke.side {
                            Side::Left => (f32::from(column) * cell.width, -1.0),
                            Side::Right => (f32::from(column + 1) * cell.width - reach, 1.0),
                        };
                        self.wedge(
                            at,
                            f32::from(top).mul_add(cell.height, -(height / 2.0)),
                            reach,
                            height,
                            way,
                            ink,
                        );
                    }
                }
            }
        }
    }

    /// A triangle filling a box, its point in the middle of one short side.
    ///
    /// `way` is which side: positive for the right-hand one, negative for
    /// the left.
    fn wedge(&mut self, left: f32, top: f32, width: f32, height: f32, way: f32, colour: [f32; 4]) {
        self.quads.push(Quad {
            rect: [left, top, width, height],
            uv: self.atlas.white,
            colour,
            flags: SOLID | WEDGE,
            radius: way,
            layer: 0,
            lower: 0.0,
        });
    }

    pub(super) fn ticks(
        &mut self,
        page: &Page,
        ticked: &[Ticked],
        panes: &[&Behind],
        framed: &[&Behind],
        capped: &[&Capped],
        cell: CellSize,
    ) {
        for tick in ticked {
            let left = f32::from(tick.area.x) * cell.width;
            let top = f32::from(tick.area.y) * cell.height;
            // The cell's own ink and ground, which the view wrote there:
            // what a terminal draws the glyph in is what a window draws
            // the box in. Except on a hold, where the ground on the
            // screen is the plate's face rather than the colour the cell
            // wears -- see `held_face`.
            let look = page.look(tick.area.x, tick.area.y);
            let ground = self
                .held_face(page, tick.area, panes, framed, capped)
                .unwrap_or_else(|| rgba(look.background, Ink::Background));
            let side = (cell.width * BOX).round().max(3.0);
            // A shade above the middle of the cell, which is where the
            // middle of the writing is: letters sit on a baseline with
            // their descenders below it, so a box centred on the cell
            // sits low against the words beside it.
            let (at, over) = (
                left + (cell.width - side) / 2.0,
                (top + (cell.height - side) / 2.0 - cell.height * ABOVE).round(),
            );
            let radius = side * BOX_CORNER;
            self.rounded(
                at,
                over,
                side,
                side,
                radius,
                rgba(look.foreground, Ink::Foreground),
            );
            match tick.on {
                true => self.quads.push(Quad {
                    rect: [at, over, side, side],
                    uv: self.atlas.white,
                    colour: ground,
                    flags: CHECKED,
                    radius: 0.0,
                    layer: 0,
                    lower: 0.0,
                }),
                false => {
                    let edge = (side * BOX_EDGE).round().max(1.0);
                    self.rounded(
                        at + edge,
                        over + edge,
                        side - edge * 2.0,
                        side - edge * 2.0,
                        (radius - edge).max(0.0),
                        ground,
                    );
                }
            }
        }
    }

    /// The marks that turn, each an arc going round in the ink of its own
    /// cell.
    ///
    /// Where the window has a clock running, every one is as far round as
    /// that says, so two of them on a screen turn together the way the
    /// braille does. Where it has not, each is as far round as the frame in
    /// its cell -- which still turns, a step a tick, on the application's.
    pub(super) fn turns(&mut self, page: &Page, spun: &[Spun], turn: Option<f32>, cell: CellSize) {
        for mark in spun {
            let Some(round) = turn.or_else(|| mark.round(page)) else {
                continue;
            };
            let look = page.look(mark.area.x, mark.area.y);
            let side = (cell.width.min(cell.height) * RING).round().max(3.0);
            let left = f32::from(mark.area.x) * cell.width;
            let top = f32::from(mark.area.y) * cell.height;
            // Where a switch's box sits, and for its reason: the middle of
            // the writing is a shade above the middle of the cell.
            self.quads.push(Quad {
                rect: [
                    left + (cell.width - side) / 2.0,
                    (top + (cell.height - side) / 2.0 - cell.height * ABOVE).round(),
                    side,
                    side,
                ],
                uv: self.atlas.white,
                colour: rgba(look.foreground, Ink::Foreground),
                flags: TURNING,
                radius: round,
                layer: 0,
                lower: 0.0,
            });
        }
    }

    /// The caps the keys at the foot of a view are drawn in.
    ///
    /// A terminal's cap is the run of cells behind the key, a shade off
    /// the page, and that is what has already been painted here by
    /// `backgrounds`. What a window can say that a terminal cannot is the
    /// *shape*: so the ground is put back over those cells and the cap is
    /// drawn on it -- which is why this runs after the letters rather
    /// than before them. The key is the one thing on the screen not
    /// written at the size the grid is counted in (see `font::Size`), so
    /// the cells' own glyphs are covered by the cap's ground and the key
    /// is written again on the face, centred in it. Drawn from `keys`
    /// rather than cell by cell, because a word tracked out to the cell
    /// pitch at three quarters the size reads as spaced-out capitals.
    ///
    /// Three rectangles, and the third is what makes it a key rather than
    /// a rounded box: the face is drawn a pixel inside the outline on
    /// three sides and further in at the bottom, so what is left showing
    /// under it is a lip. Which is the whole of the trick a keyboard's own
    /// keys use.
    pub(super) fn caps(
        &mut self,
        page: &Page,
        capped: &[&Capped],
        panes: &[&Behind],
        framed: &[&Behind],
        fonts: &mut Fonts,
    ) {
        for cap in capped {
            // A cap on a hold stands on its plate, and a face in the
            // colour the cells wear would be a shade off the plate round
            // it -- see `held_face`.
            let held = self.held_face(page, cap.area, panes, framed, capped);
            // What the cells said, put back: the corners this is about to
            // round away are painted in the cap's own ground, and a cap
            // drawn over them would have square shoulders. Unless what is
            // behind the cap is glass or a plate, which is already there
            // and is the right thing to show round a corner.
            let put_back = held.is_none() && !seen_through(panes, cap.area.x, cap.area.y, cap.page);
            let on = held.filter(|_| cap.cap == cap.page);
            self.cap(cap, |x, y| Some(page.look(x, y)), put_back, on, fonts);
        }
    }

    /// One cap, out of whichever cells it was said about: the page's, or
    /// the picture of what a pane was put over -- and `on`, the face of
    /// the plate it stands on, where it stands on one.
    pub(super) fn cap<'a>(
        &mut self,
        cap: &Capped,
        look: impl Fn(u16, u16) -> Option<Look<'a>>,
        put_back: bool,
        on: Option<[f32; 4]>,
        fonts: &mut Fonts,
    ) {
        let cell = fonts.cell();
        let left = f32::from(cap.area.x) * cell.width;
        let top = f32::from(cap.area.y) * cell.height;
        let width = f32::from(cap.area.width) * cell.width;
        let side = (cell.width * SIDE).round();
        // Never thinner than a pixel: a lip that rounds away is a cap
        // that lies flat, and the inset is what keeps a cap off the
        // rows either side of it.
        let inset = (cell.height * INSET).round().max(1.0);
        let lip = (cell.height * LIP).round().max(1.0);
        let height = (cell.height - inset * 2.0).max(1.0);
        let radius = (height * ROUNDING).min(cell.width);
        if put_back {
            self.block(
                left,
                top,
                width,
                cell.height,
                rgba(cap.page, Ink::Background),
            );
        }
        // Held off the cells either side, which the ground above is
        // not: what was put back is every cell the cap was said
        // about, and what is drawn on it stops short of them.
        let drawn = (width - side * 2.0).max(1.0);
        self.rounded(
            left + side,
            top + inset,
            drawn,
            height,
            radius,
            rgba(cap.edge, Ink::Foreground),
        );
        let face = (height - 1.0 - lip).max(1.0);
        self.rounded(
            left + side + 1.0,
            top + inset + 1.0,
            (drawn - 2.0).max(1.0),
            face,
            (radius - 1.0).max(0.0),
            on.unwrap_or_else(|| rgba(cap.cap, Ink::Background)),
        );
        self.legend(
            cap,
            look,
            left + width / 2.0,
            top + inset + 1.0 + face / 2.0,
            fonts,
        );
    }

    /// The key itself, written on the face of its cap.
    ///
    /// Middled on the face rather than on the cells: the face is held off
    /// the bottom of the cap by the lip, so a key centred in the row
    /// would sit low in the thing it is in by exactly that much.
    ///
    /// Cell by cell, as everything else that draws text here is, and at a
    /// pitch of its own so that the letters close up rather than keeping
    /// the room the grid gave them. Shaping the key in one run instead
    /// would track it properly and get the *face* wrong: what decides
    /// which family is asked for is whether the text is one of Obelus's
    /// marks, and `Ctrl` is drawn as a mark with a letter after it -- so
    /// one run would send the letter to the symbols font as well.
    ///
    /// The ink comes from the cells, which is where the theme said it --
    /// the cap knows the three colours it is drawn in and not the one the
    /// key is written in.
    fn legend<'a>(
        &mut self,
        cap: &Capped,
        look: impl Fn(u16, u16) -> Option<Look<'a>>,
        middle: f32,
        height: f32,
        fonts: &mut Fonts,
    ) {
        let cell = fonts.cell();
        let pitch = cell.width * font::SMALLER;
        let columns = obelus_text::text_width(&cap.keys).max(1);
        #[expect(
            clippy::cast_precision_loss,
            reason = "a key is a few columns wide, never billions"
        )]
        let left = middle - columns as f32 * pitch / 2.0;
        let top = height - cell.height * font::SMALLER / 2.0;
        let mut column = 0;
        for at in 1..cap.area.width.saturating_sub(1) {
            if column >= columns {
                break;
            }
            let Some(look) = look(cap.area.x.saturating_add(at), cap.area.y) else {
                continue;
            };
            // The right half of a wide character, which its neighbour
            // blanked -- see `Page::covered`.
            if look.text.is_empty() {
                continue;
            }
            if !look.text.trim().is_empty() {
                let ink = rgba(look.foreground, Ink::Foreground);
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "a key is a few columns wide, never billions"
                )]
                let along = left + column as f32 * pitch;
                self.glyphs_at((along, top), look, ink, 0, fonts, Size::Capped);
            }
            column += obelus_text::text_width(look.text).max(1);
        }
    }
}

/// What colour to draw the line under a cell in, where there is one.
///
/// Its own colour where the view gave it one and the ink where it did not,
/// which is what a terminal does with an underline nobody coloured -- and
/// the reason it is asked here rather than taken for the foreground is the
/// one view that colours it: a server's complaint is underlined in the
/// colour that kind of trouble is written in, under a word the syntax has
/// already coloured something else. Taking the ink there would draw the
/// mark in the colour of whatever the word happened to be.
pub(super) fn underline_ink(look: &Look<'_>) -> Option<[f32; 4]> {
    if !look.modifier.contains(Modifier::UNDERLINED) {
        return None;
    }
    Some(match look.underline {
        Color::Reset => rgba(look.foreground, Ink::Foreground),
        colour => rgba(colour, Ink::Foreground),
    })
}
