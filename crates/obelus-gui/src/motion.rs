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

    /// Moves everything on to this moment, and says whether the screen has
    /// to be drawn again for it.
    ///
    /// Both are asked, never one or the other: the blink has its own state
    /// to move on whether or not anything is in flight, and a `||` between
    /// them would stop asking at the first that said yes.
    pub(crate) fn advance(&mut self, now: Instant, caret: bool) -> bool {
        let flew = self.caret.settle(now);
        let blinked = self.blink.advance(now, caret);
        flew || blinked
    }

    /// When the window wants the loop back, or `None` for nothing moving.
    ///
    /// A rate beats a moment: something in flight wants every frame, and a
    /// blink that is also due will be seen on one of them.
    pub(crate) fn wake(&self, now: Instant, caret: bool) -> Option<Wake> {
        match self.caret.moving(now) {
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
