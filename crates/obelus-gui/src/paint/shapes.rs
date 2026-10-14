//! What a view said a region is -- a rule, a frame, a bar, a key's cap --
//! drawn as itself rather than spelled in the characters a terminal would
//! use.

use obelus_font::{self as font, Fonts, Size};
use obelus_ui::shapes::{About, Side};
use ratatui::style::Modifier;

use super::*;
use crate::grid::{Barred, Capped, Look, Page, Parted, Ruled, Spun, Stroked, Ticked};

/// How a tab being read turns its four corners, in the order `held` in
/// the shader reads them: its own round corner at the top left and the top
/// right, and at the foot on either side the corner that bends the other
/// way, out into the bar -- the same shape a hold makes where the row
/// under it reaches further.
const JOINS_THE_PAGE: u32 = 0b0101_0000;

/// And a tab under the pointer: round at the top, and square at its foot,
/// where there is no corner because the bar carries on under it.
const STANDS_ON_THE_BAR: u32 = 0b1010_0000;

impl Painter {
    /// The tabs along the top, on any bar the page runs up under -- see
    /// `title::tabs` -- as part of the page: first, under everything, so
    /// that a pane whose glass runs up under the bar is over them the way
    /// it is over the rest of the page, and slides down over them coming
    /// in. Seen through its glass they are blurred with everything else,
    /// and nothing on the bar is theirs to press while it is there
    /// (`Strip::cover`).
    ///
    /// In the window's pixels less the grid's origin, which is where every
    /// quad is placed: the bar is above the grid, so its rows come out
    /// negative.
    pub(super) fn tabs_on_the_bar(&mut self, across: f32, margin: [f32; 2], fonts: &mut Fonts) {
        if self.bar_left_out {
            return;
        }
        let dark = crate::title::dark(ground_of(self.ground));
        if let Some(strip) = self.strip.take() {
            self.tab_strip(&strip, across, margin, fonts, dark);
            self.strip = Some(strip);
        }
    }

    /// The three buttons on a bar Obelus draws -- see `title::Bar` -- over
    /// the picture and the page's colour, and over any pane whose glass runs
    /// up under the bar -- see `bar_on_top` in `lay`. A pane covers the
    /// tabs and not these: the window has to be closable whatever is open.
    ///
    /// In the same pixels as the tabs.
    pub(super) fn buttons_on_the_bar(&mut self, across: f32, margin: [f32; 2]) {
        if self.bar_left_out {
            return;
        }
        let dark = crate::title::dark(ground_of(self.ground));
        let Some(bar) = self.bar else {
            return;
        };
        for button in crate::title::Button::ALL {
            let [left, top, wide, tall] =
                crate::title::button_at(button, across, bar.captioned, bar.button);
            let (left, top) = (left - margin[0], top - margin[1]);
            let (behind, ink) = crate::title::inks(&bar, button, dark);
            if let Some(behind) = behind {
                self.block(left, top, wide, tall, behind);
            }
            let mark = match button {
                crate::title::Button::Minimise => 0.0,
                crate::title::Button::Maximise if bar.maximised => 2.0,
                crate::title::Button::Maximise => 1.0,
                crate::title::Button::Close => 3.0,
            };
            // Whole pixels, or every stroke is drawn across two of them.
            self.quads.push(Quad {
                rect: [
                    (left + (wide - bar.mark) / 2.0).round(),
                    (top + (tall - bar.mark) / 2.0).round(),
                    bar.mark,
                    bar.mark,
                ],
                uv: self.atlas.white,
                colour: ink,
                flags: CAPTION,
                radius: mark,
                layer: 0,
                lower: bar.pen,
            });
        }
    }

    /// The tabs along the bar, and the arrows at its ends while there are
    /// more of them than room -- see `title::tabs`.
    fn tab_strip(
        &mut self,
        strip: &crate::title::Painted,
        across: f32,
        margin: [f32; 2],
        fonts: &mut Fonts,
        dark: bool,
    ) {
        let room = strip.room;
        // The bar a shade darker than the page, across the whole of it, so
        // that the tab being read -- the page's own colour, reaching down
        // to the page -- is a piece of the page standing up into the bar.
        self.block(
            -margin[0],
            -margin[1],
            across,
            room.titled,
            crate::title::bar_shade(dark),
        );
        // A tab starts a little below the bar's top and runs to its foot,
        // which is where the page begins.
        let inset = (room.titled * 0.2).round();
        let (top, foot) = (inset - margin[1], room.titled - margin[1]);
        let tall = foot - top;
        // Where the system writes its own title, measured off its bar at
        // 250%: a little below the line its capitals would be middled on,
        // here in the middle of the tab rather than of the bar.
        let baseline = (top + tall / 2.0 + room.words * 0.39).round();
        let corner = room.corner();
        // What the page looks like where nothing is drawn on it: its colour,
        // or its colour laid over the picture under it.
        let page = match self.laid {
            Some(_) => self.veil(),
            None => self.ground_colour(),
        };
        for (at, tab) in strip.tabs.iter().enumerate() {
            let laid = tab.laid;
            let left = laid.left - margin[0];
            if tab.hovered && !tab.reading {
                // The tab's own shape, the whole of it, in a shade of the
                // bar rather than the page's colour: a tab, lit, standing
                // on the bar's foot with its foot square -- where the one
                // being read bends out into the page. A piece of the page
                // a little of the way in was a second tab half joined to it.
                self.quads.push(Quad {
                    rect: [left - corner, top, laid.width + 2.0 * corner, tall],
                    uv: self.atlas.white,
                    colour: crate::title::under_the_pointer(dark),
                    flags: HELD_PLATE | (STANDS_ON_THE_BAR << HELD_TURNS),
                    radius: corner,
                    layer: 0,
                    lower: 0.0,
                });
            }
            // A seam between two tabs neither of which has a shape of its
            // own drawn: the one being read, or the one under the pointer,
            // says where it ends by its edge, and a line along that edge
            // is the same thing said twice.
            let next = strip.tabs.get(at + 1);
            if !tab.reading
                && !tab.hovered
                && let Some(after) = next
                && !after.reading
                && !after.hovered
            {
                let pen = (room.words / 12.0).round().max(1.0);
                let high = (tall * 0.68).round();
                self.block(
                    (left + laid.width - pen / 2.0).round(),
                    (top + (tall - high) / 2.0).round(),
                    pen,
                    high,
                    crate::title::seam(dark),
                );
            }
        }
        // The tab being read last of the shapes, so that the curves at its
        // foot go over whatever its neighbours drew there. Its top corners
        // round, and the two at its foot bending out into the bar, the way
        // a tab joins what it is the tab of.
        if let Some(laid) = strip
            .tabs
            .iter()
            .find(|tab| tab.reading)
            .map(|tab| tab.laid)
        {
            let left = laid.left - margin[0];
            self.quads.push(Quad {
                // A corner's width past the tab on either side, which is
                // where the curves that bend outwards are drawn.
                rect: [left - corner, top, laid.width + 2.0 * corner, tall],
                uv: self.atlas.white,
                colour: page,
                flags: HELD_PLATE | (JOINS_THE_PAGE << HELD_TURNS),
                radius: corner,
                layer: 0,
                lower: 0.0,
            });
        }
        for tab in &strip.tabs {
            let left = tab.laid.left - margin[0];
            let ink = crate::title::tab_ink(tab.reading, strip.active, dark);
            let end = left + tab.laid.width - room.padding() / 2.0;
            let mut start = (left + room.padding()).round();
            // The mark first, in the name's ink, as the list of what is
            // open draws it before the same name.
            if let Some(icon) = tab.icon {
                let placed = fonts.caption_mark(icon, room.mark()).to_vec();
                self.written(fonts, placed, [start, baseline], ink, end);
                start += room.marked();
            }
            self.caption(fonts, &tab.said, room.words, [start, baseline], ink, end);
        }
        // What a tab cut by either end has past it goes under the bar
        // again: the page as it is there, and the bar's shade over it --
        // which is what was under the tab before it was drawn. Out to the
        // window's own edges: a tab cut by the end can be wider than all
        // the bar past it, and stopping a corner past the room left it
        // standing between the arrow and the buttons, which are drawn
        // after this and over it.
        if let Some((start, end)) = strip.cut {
            let shade = crate::title::bar_shade(dark);
            for [from, to] in [[0.0, start], [end, across]] {
                let rect = [from - margin[0], -margin[1], to - from, room.titled];
                self.ground_back(rect, self.ground, None);
                self.block(rect[0], rect[1], rect[2], rect[3], shade);
            }
        }
        for ([left, width], more, hovered, which) in &strip.arrows {
            let (behind, ink) = crate::title::arrow_inks(*more, *hovered, dark);
            let left = left - margin[0];
            if let Some(behind) = behind {
                self.rounded(left, top, *width, tall - inset / 2.0, corner, behind);
            }
            // A small solid triangle, the way the system's own tabbed
            // windows point along a row: a pointed bracket in the words'
            // face was a letter, and read as one.
            let (wide, high) = ((room.words * 0.45).round(), (room.words * 0.8).round());
            self.quads.push(Quad {
                rect: [
                    (left + (width - wide) / 2.0).round(),
                    (top + (tall - high) / 2.0).round(),
                    wide,
                    high,
                ],
                uv: self.atlas.white,
                colour: ink,
                flags: SOLID | WEDGE,
                // Which way it points: the wedge's own sign.
                radius: match which {
                    crate::title::Spot::Earlier => -1.0,
                    _ => 1.0,
                },
                layer: 0,
                lower: 0.0,
            });
        }
    }

    /// A line in the face the system writes its own words in, from a place
    /// on its baseline, and nothing of it past `end`.
    fn caption(
        &mut self,
        fonts: &mut Fonts,
        said: &str,
        size: f32,
        [start, baseline]: [f32; 2],
        ink: [f32; 4],
        end: f32,
    ) {
        let placed = fonts.caption(said, size).to_vec();
        self.written(fonts, placed, [start, baseline], ink, end);
    }

    /// Glyphs laid out from a place on their baseline, and none of them
    /// past `end`.
    fn written(
        &mut self,
        fonts: &mut Fonts,
        placed: Vec<font::Placed>,
        [start, baseline]: [f32; 2],
        ink: [f32; 4],
        end: f32,
    ) {
        for glyph in placed {
            let Some(spot) = self.atlas.spot(fonts, glyph.key) else {
                continue;
            };
            #[expect(clippy::cast_precision_loss, reason = "a glyph is a few pixels along")]
            let (x, y) = (glyph.x as f32, glyph.y as f32);
            let left = start + x + spot.left;
            // Whole glyphs or none: a letter cut in half at a tab's edge
            // reads as another letter.
            if left + spot.width > end {
                break;
            }
            self.quads.push(Quad {
                rect: [left, baseline + y - spot.top, spot.width, spot.height],
                uv: spot.uv,
                colour: ink,
                flags: match spot.colourful {
                    true => COLOURFUL,
                    false => 0,
                },
                radius: 0.0,
                layer: spot.layer,
                lower: 0.0,
            });
        }
    }

    /// The line under every cell a view underlined.
    ///
    /// A pass of its own rather than something `letters` does, because an
    /// underline is about the cell and not about the glyph: a span a
    /// server complained about runs over the spaces in it too, and
    /// `letters` skips a cell with nothing in it. A terminal draws these
    /// itself, off the modifier; a window has to be told, which is the
    /// same seam a full-width character crosses.
    ///
    /// Dotted under what an input method is still spelling, where a view
    /// said so and the cell is still that view's: the line a word nobody
    /// has written yet wears, apart from the solid one under a word a
    /// server complained about.
    pub(super) fn underlines(&mut self, page: &Page, cell: CellSize, spelled: &[(Rect, usize)]) {
        for row in 0..page.rows() {
            for column in 0..page.columns() {
                let look = page.look(column, row);
                let Some(ink) = underline_ink(&look) else {
                    continue;
                };
                let (left, top, width) = (
                    f32::from(column) * cell.width,
                    f32::from(row) * cell.height,
                    f32::from(look.columns()) * cell.width,
                );
                match is_spelled(page, spelled, column, row) {
                    true => self.dotted(left, top, width, cell.height, ink),
                    false => self.underline(left, top, width, cell.height, ink),
                }
            }
        }
    }

    /// The same line, as a run of short dashes.
    ///
    /// Counted from the window's left edge rather than from the cell, so
    /// that the dashes under a word are one run across it, and not one
    /// that starts again at every character.
    pub(super) fn dotted(&mut self, left: f32, top: f32, width: f32, height: f32, ink: [f32; 4]) {
        let thick = (height * UNDERLINE).round().max(1.0);
        for (start, end) in dashes(left, width, thick) {
            self.block(start, top + height - thick, end - start, thick, ink);
        }
    }

    /// One line under one run of cells.
    ///
    /// One line, one thickness, for every underline a view wrote: what a
    /// server says is wrong with a word, a link, a heading. What an input
    /// method is still spelling is the one drawn otherwise, dotted -- see
    /// `dotted` -- and in this same thickness, so the two differ in the one
    /// way that says what each is.
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
    /// What was put over is the cells under the ring, their letters as
    /// well as their ground, and the line goes over the half of each that
    /// is inside it. Which is what a compact list does with the row its
    /// rule takes. A terminal gives the ring the whole of its cells, because
    /// a glyph takes a cell; a line here takes a few pixels, and with only
    /// the ground put back a hover over the code had a blank half cell all
    /// round it, a letter gone from each row it crossed.
    ///
    /// Not that ground where it is glass, which is already under it -- an
    /// opaque strip of it would be the one square edge left on a round box.
    ///
    /// What goes inside the line is the caller's, the box's own ground or
    /// glass, so what this answers is where that is: the rectangle inside
    /// the line, and how round it is.
    pub(super) fn frame(
        &mut self,
        page: &Page,
        card: &Behind,
        panes: &[&Behind],
        fonts: &mut Fonts,
    ) -> ([f32; 4], f32) {
        let cell = fonts.cell();
        let area = card.area;
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                let Some(under) = card.look(x, y) else {
                    continue;
                };
                if card.ring_holds(page, x, y) && !seen_through(panes, x, y, under.background) {
                    let ground = self.under(panes, &[], x, y);
                    let held = as_held(under.background, self.holding, ground);
                    self.ground_back(
                        [
                            f32::from(x) * cell.width,
                            f32::from(y) * cell.height,
                            cell.width,
                            cell.height,
                        ],
                        under.background,
                        held,
                    );
                }
            }
        }
        // And the cells a full-width letter on the right-hand edge runs
        // into, which `backgrounds` leaves to this -- see `Behind::spilt`.
        for (x, y) in card.spilt(page) {
            let look = page.look(x, y);
            if !seen_through(panes, x, y, look.background) {
                let ground = self.under(panes, &[], x, y);
                let held = as_held(look.background, self.holding, ground);
                self.ground_back(
                    [
                        f32::from(x) * cell.width,
                        f32::from(y) * cell.height,
                        cell.width,
                        cell.height,
                    ],
                    look.background,
                    held,
                );
            }
        }
        // Under the line, which is drawn next and covers what of each
        // letter is inside it. And after every ground of the ring, for the
        // reason `behind_row` gives: a full-width character along the top
        // runs into the next cell, whose ground would cover half of it.
        for (x, y, under) in card.lettered_ring(page) {
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
    /// The bars' marks, as capsules rather than as the blocks a terminal has.
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
    /// what a window draws the capsule in, and asking the view for it
    /// again would be the same colour from two places.
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
            // The mark only where the column is still the bar's: half a mark is
            // where the reader is, and a mark drawn across whatever
            // covered it is not.
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
            let held = self.held_face(page, tick.area, panes, framed, capped);
            let ground = held.unwrap_or_else(|| rgba(look.background, Ink::Background));
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
                // The hollow is the cell's ground seen through the outline,
                // which over a picture is the picture with the page on it.
                false => {
                    let edge = (side * BOX_EDGE).round().max(1.0);
                    self.rounded_back(
                        [at + edge, over + edge, side - edge * 2.0, side - edge * 2.0],
                        (radius - edge).max(0.0),
                        look.background,
                        held,
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
            self.turn(mark, page.look(mark.area.x, mark.area.y), turn, cell);
        }
    }

    /// One of them, from what its cell holds -- on the page, or in the
    /// picture of what a pane was put over.
    pub(super) fn turn(&mut self, mark: &Spun, look: Look<'_>, turn: Option<f32>, cell: CellSize) {
        let Some(round) = turn.or_else(|| obelus_ui::how_far_round(look.text)) else {
            return;
        };
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
        // As the page looks there, which over a picture is the picture with
        // the page laid on it: a block of the page's own colour was a dark
        // square behind every cap at the foot.
        if put_back {
            self.ground_back([left, top, width, cell.height], cap.page, None);
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

/// Where the dashes of a dotted line `thick` pixels thick fall between
/// `left` and `left + width`, each from where to where.
///
/// A dash twice as long as the line is thick, and a gap as long, counted
/// from the window's left edge -- see `Painter::dotted`.
pub(super) fn dashes(left: f32, width: f32, thick: f32) -> Vec<(f32, f32)> {
    let dash = thick * 2.0;
    let period = dash * 2.0;
    let mut from = (left / period).floor() * period;
    let mut dashes = Vec::new();
    while from < left + width {
        let (start, end) = (from.max(left), (from + dash).min(left + width));
        if end > start {
            dashes.push((start, end));
        }
        from += period;
    }
    dashes
}

/// Whether this cell is what an input method is still spelling: a view
/// said so, and nothing has been put over the cell since -- see
/// `Page::layer`.
pub(super) fn is_spelled(page: &Page, spelled: &[(Rect, usize)], x: u16, y: u16) -> bool {
    spelled.iter().any(|&(area, depth)| {
        area.contains(ratatui::layout::Position { x, y }) && page.layer(x, y) <= depth
    })
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
