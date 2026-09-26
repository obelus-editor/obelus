//! What the window is doing to a frame that the frame does not know about.
//!
//! A frame is a grid of cells and says nothing about time: it is what
//! Obelus wants on the screen at this moment, and the moment before it is
//! gone. Everything here is the window's own -- the caret going on and off,
//! the caret on its way from one cell to another -- and the application is
//! never told, which is why `ob` has none of it and nothing here reaches
//! anything shared with it.
//!
//! One rule keeps that safe: **the page is what Obelus said, and motion is
//! only how the window is showing it.** Throw every animation away at any
//! moment -- a resize, a window uncovered, a frame arriving mid-flight --
//! and what is left is the right screen. So nothing here writes to the
//! page, and [`Moving`] is a parameter the painter is handed rather than
//! state it keeps.
//!
//! Two kinds of waiting, and they are not one thing. A blink is *a
//! moment*: it wants the loop woken once, at the flip, and not at all in
//! between. A glide is *a rate*: it wants every frame the screen will draw
//! until it arrives. [`Wake`] is that difference, and [`Motion::wake`]
//! folds what everything here wants into the single answer the event loop
//! takes.
//!
//! Worked out from what is true, every time, rather than switched on where
//! something starts and off in each of the ways it stops. That is the rule
//! the blink was already written to, and the reason it could be the only
//! thing in `obg` that ever woke itself: a deadline that is derived cannot
//! outlive its reason.

use std::time::{Duration, Instant};

use ratatui::layout::Position;

use crate::blink::Blink;

/// How long the caret takes to arrive where the page put it.
///
/// Short, because a caret is not decoration: its shape is Obelus saying
/// what the next key will do, and one still in the air is saying it about
/// a cell the reader is not in. Long enough for the eye to follow it from
/// one line to the next, and no longer.
const GLIDE: Duration = Duration::from_millis(70);

/// How long a pane takes to arrive.
///
/// Longer than the caret's flight, because what is moving is the whole of
/// the screen rather than one cell of it, and shorter than the pause
/// before a reader would wonder whether the key had worked. A pane that
/// took its time would be a key that feels slow, which is the one thing
/// an animation must not make a key feel.
const SLIDE: Duration = Duration::from_millis(190);

/// How long a band takes to catch up with where it has got to.
///
/// Shorter than a pane arriving and longer than the caret's flight. What
/// it is measured against is the next press: a reader holding an arrow key
/// sends one every thirty milliseconds or so, and a catch-up that outlived
/// two of them would be a list that never stops sliding.
const CATCH_UP: Duration = Duration::from_millis(110);

/// When the window wants the loop back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Wake {
    /// At this moment, and not before it.
    At(Instant),
    /// As often as the screen is drawn, which is what something in flight
    /// wants: a glide has no next moment of interest, it has a rate.
    EveryFrame,
}

/// What the painter is told about a frame beyond the cells in it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Moving {
    /// Whether the caret is drawn at this moment.
    pub(crate) caret: bool,
    /// How many rows behind where it has got to a band is drawn, and how
    /// far the page it scrolled off is from there -- or `None` for a band
    /// that is where it belongs.
    ///
    /// Both, because the rows it has not caught up to yet are on no page
    /// but the one it scrolled off, and where in that page they are is
    /// the difference between the two.
    pub(crate) scroll: Option<(f32, f32)>,
    /// How far along a pane is on its way in, from nothing at all to
    /// arrived, or `None` for a pane that is simply there.
    ///
    /// `None` rather than `Some(1.0)` for a pane at rest, because the two
    /// are not the same thing to draw: one is a frame with a pane on it,
    /// and the other is a frame being composed out of two pictures. A
    /// window that took the second path every frame would pay for an
    /// animation nobody is watching.
    pub(crate) pane: Option<f32>,
    /// How far from where the page says it is the caret is drawn, in
    /// cells.
    ///
    /// In cells rather than in pixels because how big a cell is changes
    /// under a flight -- the reader turns the text up, the window is
    /// dragged to a screen of another density -- and a distance in pixels
    /// would put the caret somewhere it never was.
    pub(crate) drift: (f32, f32),
}

/// The caret on its way from where it was to where it is.
#[derive(Debug)]
struct Glide {
    /// How far the caret was from its destination when this flight began,
    /// in cells, or `None` for a caret that is already where it belongs.
    ///
    /// Measured from the destination rather than towards it because the
    /// destination is the one end that is a fact: it is on the page, and
    /// the page may say it again at any moment.
    from: Option<(f32, f32)>,
    /// When the flight began.
    started: Instant,
}

impl Glide {
    /// A caret that is where it belongs.
    fn resting() -> Self {
        Self {
            from: None,
            started: Instant::now(),
        }
    }

    /// Whether the caret is still on its way.
    fn moving(&self, now: Instant) -> bool {
        self.from.is_some() && now.duration_since(self.started) < GLIDE
    }

    /// How far from its destination the caret is drawn at this moment.
    fn drift(&self, now: Instant) -> (f32, f32) {
        let Some(from) = self.from else {
            return (0.0, 0.0);
        };
        let gone = now.duration_since(self.started).as_secs_f32() / GLIDE.as_secs_f32();
        if gone >= 1.0 {
            return (0.0, 0.0);
        }
        // What is left of the distance, falling away fastest at the start:
        // a thing that is arriving somewhere slows into it, and a caret
        // that moved at one speed would look like it was being dragged.
        let left = (1.0 - gone).powi(3);
        (from.0 * left, from.1 * left)
    }

    /// The page says the caret is somewhere else now.
    fn moved(&mut self, was: Position, is: Position, now: Instant) {
        let drift = self.drift(now);
        // From where it is being *drawn*, not from where the page last
        // said it was: a caret that moves again mid-flight carries on from
        // the pixel it is at rather than snapping back to the cell it set
        // out from. Which is what makes a held-down arrow key one
        // movement instead of a stutter.
        self.from = Some((
            f32::from(was.x) - f32::from(is.x) + drift.0,
            f32::from(was.y) - f32::from(is.y) + drift.1,
        ));
        self.started = now;
    }

    /// Moves the flight on, and says whether the caret is being drawn
    /// anywhere other than where it was drawn last.
    ///
    /// True on the frame it lands as well, which is the frame that puts it
    /// on its cell: a flight that stopped asking for frames a moment
    /// before arriving would leave the caret a fraction of a cell short
    /// for as long as nothing else happened.
    fn settle(&mut self, now: Instant) -> bool {
        if self.from.is_none() {
            return false;
        }
        if now.duration_since(self.started) >= GLIDE {
            self.from = None;
        }
        true
    }
}

/// A band of rows catching up with where its list has got to.
///
/// The same shape as the caret's flight and for the same reasons: it is
/// measured from where it is going, it carries on from where it is being
/// drawn rather than from where it set out, and it is over when the time
/// is up.
#[derive(Debug)]
struct Scrolling {
    /// How many rows behind it was when this leg began, or `None` for a
    /// band that has caught up.
    from: Option<f32>,
    /// And how far it has come since the page the window is keeping.
    ///
    /// Not the same number, and the difference is what a second press
    /// during a slide makes: `from` is measured from wherever the band
    /// was being *drawn* at that moment, which is nowhere any page shows,
    /// while this is measured from the one page that does. The rows the
    /// list scrolled past are drawn out of that page, so it is this that
    /// says where to put them.
    since: f32,
    /// When this leg began.
    started: Instant,
}

impl Scrolling {
    /// Whether it is still catching up.
    fn moving(&self, now: Instant) -> bool {
        self.from.is_some() && now.duration_since(self.started) < CATCH_UP
    }

    /// How many rows behind it is drawn at this moment.
    fn behind(&self, now: Instant) -> Option<f32> {
        let from = self.from?;
        let gone = now.duration_since(self.started).as_secs_f32() / CATCH_UP.as_secs_f32();
        if gone >= 1.0 {
            return None;
        }
        Some(from * (1.0 - gone).powi(3))
    }

    /// The list has got somewhere else.
    ///
    /// Says whether this began a fresh slide, which is when the window
    /// has to keep the page it scrolled off: one already under way keeps
    /// the page it started with, because that is the page `since` counts
    /// from.
    ///
    /// `most` is how many rows the band is tall. Further than that from
    /// the page being kept and there is nothing left to draw the gap out
    /// of, so the band gives up and is simply where it has got to -- the
    /// same answer, for the same reason, that a jump gets.
    fn moved(&mut self, rows: f32, most: f32, now: Instant) -> bool {
        // From where it is being drawn, so that a held-down arrow key is
        // one slide and not a stutter -- the caret's rule, on a band.
        let behind = self.behind(now).unwrap_or(0.0);
        // Over because the time is up, whether or not a frame has been
        // along to notice: what decides is the clock, and a band nobody
        // has drawn for a second is not in the middle of anything.
        let fresh = !self.moving(now);
        let since = if fresh { rows } else { self.since + rows };
        if since.abs() > most {
            self.from = None;
            self.since = 0.0;
            return false;
        }
        self.from = Some(rows + behind);
        self.since = since;
        self.started = now;
        fresh
    }

    /// Moves it on, and says whether what is drawn changed.
    fn settle(&mut self, now: Instant) -> bool {
        if self.from.is_none() {
            return false;
        }
        if !self.moving(now) {
            self.from = None;
            self.since = 0.0;
        }
        true
    }
}

/// A pane on its way in from above.
#[derive(Debug)]
struct Sliding {
    /// When it opened, or `None` where there is no pane or it has
    /// arrived.
    opened: Option<Instant>,
}

impl Sliding {
    /// Whether it is still on its way.
    fn moving(&self, now: Instant) -> bool {
        self.opened
            .is_some_and(|opened| now.duration_since(opened) < SLIDE)
    }

    /// How far along it is, or `None` for a pane that is simply there.
    fn along(&self, now: Instant) -> Option<f32> {
        let opened = self.opened?;
        let gone = now.duration_since(opened).as_secs_f32() / SLIDE.as_secs_f32();
        if gone >= 1.0 {
            return None;
        }
        // Slowest into its place, fastest leaving the top: a thing that
        // arrives somewhere settles into it.
        Some(1.0 - (1.0 - gone).powi(3))
    }

    /// Moves it on, and says whether what is drawn changed.
    fn settle(&mut self, now: Instant) -> bool {
        if self.opened.is_none() {
            return false;
        }
        if !self.moving(now) {
            self.opened = None;
        }
        true
    }
}

/// The caret going on and off.
#[derive(Debug)]
struct Blinking {
    /// How it blinks here, or `None` where the system says it should not.
    how: Option<Blink>,
    /// Whether it is being drawn at this moment.
    lit: bool,
    /// When it next goes the other way.
    flip: Instant,
    /// When the reader last did something.
    ///
    /// What blinking is measured from: a caret is solid while somebody is
    /// typing, and a desktop that says when to stop says it in seconds
    /// since the last key.
    stirred: Instant,
}

impl Blinking {
    /// Whether the reader has stopped for long enough that the caret is
    /// left alone.
    fn settled(&self, how: Blink, now: Instant) -> bool {
        how.settles
            .is_some_and(|settles| now.duration_since(self.stirred) >= settles)
    }

    /// The reader did something, so the caret is solid again and the
    /// blinking starts over.
    ///
    /// Which is what every caret does: one that went on blinking through a
    /// paragraph being typed would flicker under the reader's hands, and
    /// one that stayed dark for the half cycle it was in the middle of
    /// would be a keypress with no caret after it.
    fn stirred(&mut self, now: Instant) -> bool {
        self.stirred = now;
        if let Some(how) = self.how {
            self.flip = now + how.every;
        }
        let dark = !self.lit;
        self.lit = true;
        dark
    }

    /// Moves the blink on, and says whether what is drawn changed.
    fn advance(&mut self, now: Instant, caret: bool) -> bool {
        let (Some(how), true) = (self.how, caret) else {
            return false;
        };
        if self.settled(how, now) {
            // Stopped blinking and left visible, which is what that
            // setting is for: a caret blinking at an empty desk all
            // afternoon is a process that never sleeps.
            let dark = !self.lit;
            self.lit = true;
            return dark;
        }
        if now >= self.flip {
            self.lit = !self.lit;
            self.flip = now + how.every;
            return true;
        }
        false
    }

    /// When the blink wants the loop back, if it does.
    fn wake(&self, now: Instant, caret: bool) -> Option<Wake> {
        let (Some(how), true) = (self.how, caret) else {
            return None;
        };
        (!self.settled(how, now)).then_some(Wake::At(self.flip))
    }
}

/// Everything the window is animating, and when it wants to be woken for
/// it.
#[derive(Debug)]
pub(crate) struct Motion {
    blink: Blinking,
    caret: Glide,
    pane: Sliding,
    band: Scrolling,
}

impl Motion {
    /// Nothing moving, with the caret blinking the way the system says.
    pub(crate) fn new(blink: Option<Blink>) -> Self {
        let now = Instant::now();
        Self {
            blink: Blinking {
                how: blink,
                lit: true,
                flip: now,
                stirred: now,
            },
            caret: Glide::resting(),
            pane: Sliding { opened: None },
            band: Scrolling {
                from: None,
                since: 0.0,
                started: now,
            },
        }
    }

    /// The reader did something. Says whether that alone is a reason to
    /// draw again.
    pub(crate) fn stirred(&mut self, now: Instant) -> bool {
        self.blink.stirred(now)
    }

    /// The page's caret is somewhere else than it was.
    pub(crate) fn caret_moved(
        &mut self,
        was: Option<Position>,
        is: Option<Position>,
        now: Instant,
    ) {
        // A caret that was not on the page does not glide onto it:
        // appearing somewhere is not moving there, and a box opening would
        // have its caret fly in from wherever the last one happened to
        // stand. The same the other way: a caret that has gone has nothing
        // to fly to.
        let (Some(was), Some(is)) = (was, is) else {
            return;
        };
        if was != is {
            self.caret.moved(was, is, now);
        }
    }

    /// A band of rows is showing a different part of its list.
    ///
    /// `rows` is how far it moved: positive where the list went down,
    /// which is the band's content going up. Says whether the window has
    /// to keep the page it scrolled off -- see `Scrolling::moved`.
    pub(crate) fn band_moved(&mut self, rows: f32, most: f32, now: Instant) -> bool {
        self.band.moved(rows, most, now)
    }

    /// A pane opened over the page.
    ///
    /// Only the opening: a pane that is closed leaves nothing to draw, and
    /// one that is still there while another opens over it is a pane that
    /// has not moved.
    pub(crate) fn pane_opened(&mut self, now: Instant) {
        self.pane.opened = Some(now);
    }

    /// And the page is bare again.
    pub(crate) fn pane_shut(&mut self) {
        self.pane.opened = None;
    }

    /// Moves everything on to this moment, and says whether the screen has
    /// to be drawn again for it.
    ///
    /// Both are asked, never one or the other: the blink has its own state
    /// to move on whether or not anything is in flight, and a `||` between
    /// them would stop asking at the first that said yes.
    pub(crate) fn advance(&mut self, now: Instant, caret: bool) -> bool {
        let flew = self.caret.settle(now);
        let caught = self.band.settle(now);
        let slid = self.pane.settle(now);
        let blinked = self.blink.advance(now, caret);
        flew || caught || slid || blinked
    }

    /// When the window wants the loop back, or `None` for nothing moving.
    ///
    /// A rate beats a moment: something in flight wants every frame, and a
    /// blink that is also due will be seen on one of them.
    pub(crate) fn wake(&self, now: Instant, caret: bool) -> Option<Wake> {
        match self.caret.moving(now) || self.pane.moving(now) || self.band.moving(now) {
            true => Some(Wake::EveryFrame),
            false => self.blink.wake(now, caret),
        }
    }

    /// What the painter is to do with this frame.
    pub(crate) fn moving(&self, now: Instant) -> Moving {
        Moving {
            // A caret on its way is drawn whatever half of the blink it is
            // in: one that went dark mid-flight is a caret the eye loses in
            // the middle of following it.
            caret: self.blink.lit || self.caret.moving(now),
            drift: self.caret.drift(now),
            pane: self.pane.along(now),
            scroll: self
                .band
                .behind(now)
                .map(|behind| (behind, self.band.since)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A caret that blinks the way a desktop that has not been touched
    /// says it should.
    fn blinking() -> Blink {
        Blink {
            every: Duration::from_millis(600),
            settles: Some(Duration::from_secs(10)),
        }
    }

    fn at(x: u16, y: u16) -> Position {
        Position::new(x, y)
    }

    /// Break: take a missing `was` for the origin rather than for no
    /// caret at all, and one appearing in a box flies in from the corner
    /// of the screen.
    #[test]
    fn a_caret_that_appears_does_not_glide() {
        let base = Instant::now();
        let mut motion = Motion::new(Some(blinking()));
        motion.caret_moved(None, Some(at(4, 4)), base);
        assert_eq!(motion.moving(base).drift, (0.0, 0.0));
        assert_ne!(motion.wake(base, true), Some(Wake::EveryFrame));
    }

    /// Break: return `(0.0, 0.0)` from `Glide::drift` and the caret is
    /// drawn on its destination from the first frame, which is a jump.
    #[test]
    fn a_caret_that_moves_glides_and_arrives() {
        let base = Instant::now();
        let mut motion = Motion::new(Some(blinking()));
        motion.caret_moved(Some(at(0, 10)), Some(at(0, 0)), base);
        // Ten rows below where it is going, which is where it was.
        assert_eq!(motion.moving(base).drift, (0.0, 10.0));
        let half = motion.moving(base + GLIDE / 2).drift;
        assert!(half.1 > 0.0 && half.1 < 10.0, "{half:?}");
        assert_eq!(motion.moving(base + GLIDE).drift, (0.0, 0.0));
    }

    /// Break: drop the `|| self.caret.moving(now)` from `Motion::moving`
    /// and a caret that sets off in the dark half of the cycle is invisible
    /// for the whole of its flight.
    #[test]
    fn a_caret_in_flight_is_drawn_through_the_dark_half_of_the_blink() {
        let base = Instant::now();
        let mut motion = Motion::new(Some(blinking()));
        motion.stirred(base);
        let dark = base + blinking().every;
        assert!(motion.advance(dark, true));
        assert!(!motion.moving(dark).caret, "the blink should have taken it");
        motion.caret_moved(Some(at(0, 1)), Some(at(0, 2)), dark);
        assert!(motion.moving(dark).caret);
    }

    /// Break: set `from` in `Glide::moved` from the two cells alone,
    /// leaving the drift of the flight in progress out, and a held-down
    /// arrow key snaps the caret back to the cell it set out from on every
    /// repeat.
    #[test]
    fn a_caret_that_moves_again_carries_on_from_where_it_is() {
        let base = Instant::now();
        let mut motion = Motion::new(None);
        motion.caret_moved(Some(at(0, 0)), Some(at(0, 10)), base);
        let midway = base + GLIDE / 2;
        // Which row it is drawn on at that moment: its destination, plus
        // how far from the destination it still is.
        let drawn = 10.0 + motion.moving(midway).drift.1;
        assert!(drawn > 0.0 && drawn < 10.0, "{drawn}");
        motion.caret_moved(Some(at(0, 10)), Some(at(0, 20)), midway);
        let carried = 20.0 + motion.moving(midway).drift.1;
        assert!((carried - drawn).abs() < 0.001, "{drawn} then {carried}");
    }

    /// Break: answer `Wake::At` while something is in flight and the glide
    /// is drawn at whatever rate the blink happens to want, which is twice
    /// a second.
    #[test]
    fn something_in_flight_asks_for_every_frame() {
        let base = Instant::now();
        let mut motion = Motion::new(Some(blinking()));
        motion.stirred(base);
        let flip = base + blinking().every;
        assert_eq!(motion.wake(base, true), Some(Wake::At(flip)));
        motion.caret_moved(Some(at(0, 0)), Some(at(0, 5)), base);
        assert_eq!(motion.wake(base, true), Some(Wake::EveryFrame));
        assert_eq!(motion.wake(base + GLIDE, true), Some(Wake::At(flip)));
    }

    /// Break: drop the `now >= self.flip` arm of `Blinking::advance` and
    /// the caret never goes off at all.
    #[test]
    fn the_caret_goes_off_after_half_a_cycle() {
        let base = Instant::now();
        let mut motion = Motion::new(Some(blinking()));
        motion.stirred(base);
        assert!(motion.moving(base).caret);
        assert!(!motion.advance(base + blinking().every / 2, true));
        assert!(motion.moving(base).caret);
        assert!(motion.advance(base + blinking().every, true));
        assert!(!motion.moving(base).caret);
    }

    /// Break: drop the `settled` arm and a window nobody is at blinks all
    /// afternoon -- which is a process that never sleeps, and the reason
    /// the desktop has that setting at all.
    #[test]
    fn a_caret_the_reader_has_left_is_put_back_on_and_asks_for_nothing() {
        let base = Instant::now();
        let mut motion = Motion::new(Some(blinking()));
        motion.stirred(base);
        assert!(motion.advance(base + blinking().every, true));
        assert!(!motion.moving(base).caret);
        let later = base + Duration::from_secs(11);
        assert!(motion.advance(later, true));
        assert!(motion.moving(later).caret);
        assert_eq!(motion.wake(later, true), None);
    }

    /// Break: stop asking whether the page has a caret and the window
    /// wakes twice a second behind every screen that has none.
    #[test]
    fn a_page_with_no_caret_on_it_asks_for_nothing() {
        let base = Instant::now();
        let mut motion = Motion::new(Some(blinking()));
        motion.stirred(base);
        let flip = base + blinking().every;
        assert_eq!(motion.wake(flip, false), None);
        assert!(!motion.advance(flip, false));
    }

    /// Break: answer `Some(1.0)` from `Sliding::along` once the time is
    /// up rather than `None`, and every frame for the rest of the session
    /// is drawn twice -- once into a picture and once out of it -- for a
    /// pane that arrived a minute ago.
    #[test]
    fn a_pane_arrives_and_then_is_simply_there() {
        let base = Instant::now();
        let mut motion = Motion::new(None);
        assert_eq!(motion.moving(base).pane, None, "nothing has opened");
        motion.pane_opened(base);
        assert_eq!(motion.moving(base).pane, Some(0.0));
        let half = motion.moving(base + SLIDE / 2).pane.expect("on its way");
        assert!(half > 0.0 && half < 1.0, "{half}");
        assert_eq!(motion.moving(base + SLIDE).pane, None);
    }

    /// Break: leave the pane out of `Motion::wake` and a pane slides in at
    /// whatever rate the blink happens to want, which is twice a second.
    #[test]
    fn a_pane_on_its_way_in_asks_for_every_frame() {
        let base = Instant::now();
        let mut motion = Motion::new(None);
        assert_eq!(motion.wake(base, true), None);
        motion.pane_opened(base);
        assert_eq!(motion.wake(base, true), Some(Wake::EveryFrame));
        assert_eq!(motion.wake(base + SLIDE, true), None);
    }

    /// Break: have `pane_shut` leave the moment it opened behind, and a
    /// pane closed while it was still arriving leaves the window composing
    /// a frame out of a pane that is not on it.
    #[test]
    fn a_pane_that_went_away_is_not_still_arriving() {
        let base = Instant::now();
        let mut motion = Motion::new(None);
        motion.pane_opened(base);
        assert!(motion.moving(base).pane.is_some());
        motion.pane_shut();
        assert_eq!(motion.moving(base).pane, None);
        assert_eq!(motion.wake(base, true), None);
    }

    /// Break: answer the distance still to come from `Scrolling::behind`
    /// without the easing, and a list crawls at one speed and stops dead
    /// instead of settling.
    #[test]
    fn a_band_catches_up_with_where_its_list_has_got_to() {
        let base = Instant::now();
        let mut motion = Motion::new(None);
        assert_eq!(motion.moving(base).scroll, None, "nothing has moved");
        assert!(motion.band_moved(3.0, 20.0, base), "a fresh slide");
        assert_eq!(motion.moving(base).scroll, Some((3.0, 3.0)));
        let (behind, since) = motion
            .moving(base + CATCH_UP / 2)
            .scroll
            .expect("still catching up");
        assert_eq!(since, 3.0, "how far the kept page is does not change");
        assert!(behind > 0.0 && behind < 3.0, "{behind}");
        assert_eq!(motion.moving(base + CATCH_UP).scroll, None);
    }

    /// Break: set `from` in `Scrolling::moved` to the rows alone, leaving
    /// out how far behind it already is, and a held-down arrow key snaps
    /// the list back to where it was on every repeat.
    #[test]
    fn a_band_that_moves_again_carries_on_from_where_it_is() {
        let base = Instant::now();
        let mut motion = Motion::new(None);
        motion.band_moved(3.0, 20.0, base);
        let midway = base + CATCH_UP / 2;
        let (behind, _) = motion.moving(midway).scroll.expect("on its way");
        motion.band_moved(3.0, 20.0, midway);
        let (again, _) = motion.moving(midway).scroll.expect("on its way again");
        assert!((again - (3.0 + behind)).abs() < 0.001, "{again} {behind}");
    }

    /// Break: answer `true` from `Scrolling::moved` whatever was already
    /// happening, and the window swaps the page it is drawing the gap out
    /// of in the middle of a slide -- which draws a row twice, once from
    /// each page, for as long as the slide lasts.
    #[test]
    fn a_band_already_sliding_keeps_the_page_it_started_from() {
        let base = Instant::now();
        let mut motion = Motion::new(None);
        assert!(motion.band_moved(2.0, 20.0, base), "nothing was happening");
        let midway = base + CATCH_UP / 2;
        assert!(
            !motion.band_moved(2.0, 20.0, midway),
            "one is already under way"
        );
        let (_, since) = motion.moving(midway).scroll.expect("on its way");
        assert!(
            (since - 4.0).abs() < 0.001,
            "both moves, from one page: {since}"
        );
        // And once it has caught up, the next one starts again.
        assert!(motion.band_moved(1.0, 20.0, midway + CATCH_UP), "caught up");
    }

    /// Break: drop the `since` bound and a slide that has wandered further
    /// than the band is tall draws its gap out of a page that has none of
    /// those rows on it.
    #[test]
    fn a_band_that_has_come_further_than_it_is_tall_is_simply_there() {
        let base = Instant::now();
        let mut motion = Motion::new(None);
        assert!(motion.band_moved(6.0, 10.0, base));
        let midway = base + CATCH_UP / 2;
        assert!(!motion.band_moved(6.0, 10.0, midway), "too far to draw");
        assert_eq!(motion.moving(midway).scroll, None, "it is simply there");
    }

    /// Break: leave the band out of `Motion::wake` and a list slides at
    /// whatever rate the blink happens to want.
    #[test]
    fn a_band_catching_up_asks_for_every_frame() {
        let base = Instant::now();
        let mut motion = Motion::new(None);
        assert_eq!(motion.wake(base, true), None);
        motion.band_moved(2.0, 20.0, base);
        assert_eq!(motion.wake(base, true), Some(Wake::EveryFrame));
        assert_eq!(motion.wake(base + CATCH_UP, true), None);
    }

    /// Break: give a caret the system says should not blink one anyway,
    /// and a reader who turned it off has a window waking twice a second
    /// behind their work.
    #[test]
    fn a_caret_that_does_not_blink_asks_for_nothing() {
        let base = Instant::now();
        let mut motion = Motion::new(None);
        motion.stirred(base);
        assert_eq!(motion.wake(base + Duration::from_secs(1), true), None);
        assert!(motion.moving(base).caret);
    }
}
