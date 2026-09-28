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

use obelus_ui::shapes::Bar;
use ratatui::layout::{Position, Rect};

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

/// How long a bar stays at full strength after its mark last moved.
///
/// Long enough to still be there when the reader's eye arrives: what
/// makes a bar worth lighting up is the question "where am I now", and
/// that question is asked after the scrolling stops rather than during it.
const BAR_HOLD: Duration = Duration::from_millis(650);

/// And how long it takes to settle back afterwards.
///
/// Slower than it came, because a thing that leaves as fast as it arrives
/// reads as a flicker: arriving is news and going is not, so going is the
/// half that may take its time.
const BAR_SETTLE: Duration = Duration::from_millis(400);

/// How long it takes to come up.
///
/// Short, and not nothing. Arriving is the half that is news, so it is the
/// quick one -- but a bar that went from settled to full between two
/// frames is a thing appearing rather than a thing brightening, and what
/// the reader sees then is a flash at the edge of the page while they were
/// looking at the middle of it.
const BAR_RISE: Duration = Duration::from_millis(110);

/// And how long the pointer's own brightening takes, either way.
///
/// The same coming and going, because neither is news: the reader moved a
/// pointer, and what they are owed is that the thing under it answers.
/// Slower than a scroll's rise, so that a pointer crossing the column on
/// its way somewhere else does not set the bar flashing.
const BAR_UNDER: Duration = Duration::from_millis(140);

/// When the window wants the loop back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Wake {
    /// At this moment, and not before it.
    At(Instant),
    /// As often as the screen is drawn, which is what something in flight
    /// wants: a glide has no next moment of interest, it has a rate.
    EveryFrame,
}

/// A bar the window has drawn, and what it was doing.
#[derive(Clone, Copy, Debug)]
struct Seen {
    /// Which column it is, which is what says it is the same bar.
    area: Rect,
    /// Where its mark was when it was last looked at.
    mark: u16,
    /// And when that last changed.
    stirred: Instant,
    /// How strongly it was being drawn at that moment, which is where the
    /// rise sets out from.
    ///
    /// Kept, rather than rising from nothing every time: a bar stirred
    /// half way down its settling would otherwise drop to where it was
    /// not and climb from there, which on screen is a flicker at the one
    /// moment the reader is looking at it. The same reason the caret's
    /// flight sets out from where it is being *drawn*.
    from: f32,
    /// Whether the pointer is on it.
    under: bool,
    /// And the same two again for that, because the pointer is its own
    /// reason to be bright and comes and goes on its own: a reader can put
    /// a pointer on a bar that has been still for a minute, and take it
    /// off one that is moving.
    under_from: f32,
    under_since: Instant,
}

/// The bars, and how long each has had nothing to say.
///
/// Kept by area rather than by an index, because a frame is a fresh list
/// every time and the second bar of one frame is not the second bar of
/// the next -- a preview opens, a list closes, and the rows they are in
/// move. An area that stops arriving is dropped, which is what keeps this
/// the length of what is on the screen.
#[derive(Debug, Default)]
struct Bars {
    seen: Vec<Seen>,
}

impl Bars {
    /// The frame drew these.
    ///
    /// A bar nobody has seen before counts as stirred, so a list that
    /// opens is a bar the reader can see: what is new is news, and the
    /// alternative is a control that fades in from nothing after the
    /// screen it belongs to has arrived.
    fn drawn(&mut self, bars: &[Bar], pointer: Option<(u16, u16)>, now: Instant) {
        for bar in bars {
            // Its own column and the one before it. A bar is drawn thinner
            // than the cell it is in, so a reader aiming at it with a
            // pointer is aiming at something narrower than the thing they
            // are pointing with, and the near misses land next to it.
            let under = pointer.is_some_and(|(x, y)| {
                x + 1 >= bar.area.x && x <= bar.area.x && y >= bar.area.y && y < bar.area.bottom()
            });
            match self.seen.iter_mut().find(|seen| seen.area == bar.area) {
                Some(seen) => {
                    if seen.mark != bar.mark {
                        // From where it is being drawn, not from nothing:
                        // see `Seen::from`.
                        seen.from = Self::risen(seen, now);
                        seen.mark = bar.mark;
                        seen.stirred = now;
                    }
                    if seen.under != under {
                        seen.under_from = Self::pointed(seen, now);
                        seen.under = under;
                        seen.under_since = now;
                    }
                }
                // A bar nobody has seen before is already up: `from` is
                // where the rise sets out from, and setting out from the
                // top is how "there is no rise here" is said. What is new
                // arrives with a whole new screen, and a control that
                // brightened into one that had already arrived would be
                // answering a question nobody had got to yet.
                None => self.seen.push(Seen {
                    area: bar.area,
                    mark: bar.mark,
                    stirred: now,
                    from: 1.0,
                    under,
                    under_from: if under { 1.0 } else { 0.0 },
                    under_since: now,
                }),
            }
        }
        self.seen
            .retain(|seen| bars.iter().any(|bar| bar.area == seen.area));
    }

    /// How strongly a bar is drawn at this moment, from nothing at all to
    /// all of it.
    ///
    /// One at rest is not gone -- the column is reserved either way, and
    /// an empty one is Obelus saying that what is on screen is all there
    /// is, which is a different thing and must not be said by accident.
    /// What settles is how loudly it says where the reader is, not
    /// whether it says it.
    ///
    /// The pointer is the other half of it, and the louder of the two
    /// wins: a bar under the pointer stays up however long ago it moved,
    /// and one that moves under a pointer that is elsewhere comes up all
    /// the same.
    fn shown(&self, area: Rect, now: Instant) -> f32 {
        let Some(seen) = self.seen.iter().find(|seen| seen.area == area) else {
            return 1.0;
        };
        Self::risen(seen, now).max(Self::pointed(seen, now))
    }

    /// How far up the scrolling has brought it: a rise, then the hold,
    /// then the settling back down.
    fn risen(seen: &Seen, now: Instant) -> f32 {
        let since = now.duration_since(seen.stirred);
        if since < BAR_RISE {
            let along = since.as_secs_f32() / BAR_RISE.as_secs_f32();
            // Into the top of it rather than at one speed, which is what
            // makes it read as brightening rather than as switching on.
            return seen.from + (1.0 - seen.from) * (1.0 - (1.0 - along).powi(2));
        }
        let Some(settling) = since.checked_sub(BAR_RISE.saturating_add(BAR_HOLD)) else {
            return 1.0;
        };
        let gone = settling.as_secs_f32() / BAR_SETTLE.as_secs_f32();
        if gone >= 1.0 {
            return 0.0;
        }
        (1.0 - gone).powi(2)
    }

    /// And how far the pointer has: the same ramp either way, with no
    /// hold, because what is holding it up is the pointer still being
    /// there.
    fn pointed(seen: &Seen, now: Instant) -> f32 {
        let to = if seen.under { 1.0 } else { 0.0 };
        let since = now.duration_since(seen.under_since);
        if since >= BAR_UNDER {
            return to;
        }
        let along = since.as_secs_f32() / BAR_UNDER.as_secs_f32();
        seen.under_from + (to - seen.under_from) * (1.0 - (1.0 - along).powi(2))
    }

    /// How far the pointer's brightening has come on one of them.
    fn under(&self, area: Rect, now: Instant) -> f32 {
        self.seen
            .iter()
            .find(|seen| seen.area == area)
            .map_or(0.0, |seen| Self::pointed(seen, now))
    }

    /// And where it would be with no time taken over it at all.
    fn under_now(&self, area: Rect) -> f32 {
        self.seen
            .iter()
            .find(|seen| seen.area == area)
            .map_or(0.0, |seen| if seen.under { 1.0 } else { 0.0 })
    }

    /// Whether any of them is still on its way anywhere.
    fn moving(&self, now: Instant) -> bool {
        self.seen.iter().any(|seen| {
            now.duration_since(seen.stirred)
                < BAR_RISE.saturating_add(BAR_HOLD).saturating_add(BAR_SETTLE)
                || now.duration_since(seen.under_since) < BAR_UNDER
        })
    }
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
    /// Whether anything here moves at all.
    ///
    /// The reader's, and it is asked at the three doors rather than at
    /// the drawing: what it turns off is things *starting*, so nothing
    /// is kept about a slide that is not happening -- including the page
    /// the window would otherwise copy to draw a band's gap out of.
    ///
    /// The blink is not one of these. A caret blinks because every caret
    /// does, at the rate the system was asked for, and a reader who wants
    /// none says so to their desktop.
    animates: bool,
    blink: Blinking,
    caret: Glide,
    pane: Sliding,
    band: Scrolling,
    bars: Bars,
}

impl Motion {
    /// Nothing moving, with the caret blinking the way the system says.
    pub(crate) fn new(blink: Option<Blink>) -> Self {
        let now = Instant::now();
        Self {
            animates: true,
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
            bars: Bars::default(),
        }
    }

    /// The reader has said whether things arrive or are simply there.
    ///
    /// What is under way when they turn it off is dropped: a slide that
    /// finished itself after the switch would be the setting taking a
    /// moment to mean anything.
    pub(crate) fn animates(&mut self, on: bool) {
        self.animates = on;
        if !on {
            self.caret.from = None;
            self.pane.opened = None;
            self.band.from = None;
            self.band.since = 0.0;
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
        // to fly to -- and either way a flight in the air is over, because
        // what it was carrying is not what is on the page now.
        let (Some(was), Some(is)) = (was, is) else {
            self.caret.from = None;
            return;
        };
        if self.animates && was != is {
            self.caret.moved(was, is, now);
        }
    }

    /// A band of rows is showing a different part of its list.
    ///
    /// `rows` is how far it moved: positive where the list went down,
    /// which is the band's content going up. Says whether the window has
    /// to keep the page it scrolled off -- see `Scrolling::moved`.
    pub(crate) fn band_moved(&mut self, rows: f32, most: f32, now: Instant) -> bool {
        self.animates && self.band.moved(rows, most, now)
    }

    /// A pane opened over the page.
    ///
    /// Only the opening: a pane that is closed leaves nothing to draw, and
    /// one that is still there while another opens over it is a pane that
    /// has not moved.
    pub(crate) fn pane_opened(&mut self, now: Instant) {
        if self.animates {
            self.pane.opened = Some(now);
        }
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
        match self.caret.moving(now)
            || self.pane.moving(now)
            || self.band.moving(now)
            || (self.animates && self.bars.moving(now))
        {
            true => Some(Wake::EveryFrame),
            false => self.blink.wake(now, caret),
        }
    }

    /// The frame drew these bars.
    ///
    /// Told rather than worked out from the page, for the reason the band
    /// is: a bar that scrolled and a bar redrawn where it was are the same
    /// handful of cells, and nothing in them says which happened.
    pub(crate) fn bars_drawn(&mut self, bars: &[Bar], pointer: Option<(u16, u16)>, now: Instant) {
        self.bars.drawn(bars, pointer, now);
    }

    /// How strongly a bar is drawn at this moment.
    ///
    /// All of it where the reader has turned animation off: what that
    /// setting turns off is things *starting*, and a bar that settled
    /// anyway would be the one animation the switch did not reach.
    pub(crate) fn bar_shown(&self, area: Rect, now: Instant) -> f32 {
        match self.animates {
            true => self.bars.shown(area, now),
            false => 1.0,
        }
    }

    /// And how far the pointer's own brightening has come, which is a
    /// second thing because it is drawn differently: what the pointer
    /// changes is the width as well as the colour, and only it does.
    ///
    /// Not gated on `animates`. A reader who wants nothing to move has
    /// still put a pointer on a control and is owed an answer, and what
    /// that switch turns off is time passing on its own -- the pointer is
    /// the reader, moving.
    pub(crate) fn bar_under(&self, area: Rect, now: Instant) -> f32 {
        match self.animates {
            true => self.bars.under(area, now),
            false => self.bars.under_now(area),
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

    /// A bar in the last column of a forty-row region, with its mark
    /// where it is put.
    fn a_bar(x: u16, mark: u16) -> Bar {
        Bar {
            area: Rect {
                x,
                y: 0,
                width: 1,
                height: 40,
            },
            mark,
            thumb: 4,
        }
    }

    /// How long a bar takes to go all the way up and all the way back.
    const BAR_ROUND: Duration = BAR_RISE.saturating_add(BAR_HOLD).saturating_add(BAR_SETTLE);

    /// Break: stir on every frame rather than on the mark moving -- drop
    /// the `seen.mark != bar.mark` guard -- and a bar never settles at
    /// all, because a frame is drawn for a dozen reasons that are not the
    /// reader scrolling.
    #[test]
    fn a_bar_settles_once_its_mark_stops_moving() {
        let base = Instant::now();
        let mut motion = Motion::new(None);
        let bar = a_bar(99, 0);
        motion.bars_drawn(&[bar], None, base);

        // Drawn again where it was, which is not news.
        let soon = base + Duration::from_millis(100);
        motion.bars_drawn(&[bar], None, soon);
        assert_eq!(motion.bar_shown(bar.area, soon), 1.0);

        // Still inside the hold.
        let held = base + BAR_RISE + BAR_HOLD - Duration::from_millis(1);
        assert_eq!(motion.bar_shown(bar.area, held), 1.0);

        // Part way down it, and all the way down after that.
        let halfway = base + BAR_RISE + BAR_HOLD + BAR_SETTLE / 2;
        let some = motion.bar_shown(bar.area, halfway);
        assert!(some > 0.0 && some < 1.0, "part of the way: {some}");
        assert_eq!(motion.bar_shown(bar.area, base + BAR_ROUND), 0.0);
    }

    /// Break: set `from` to nothing when a bar is stirred rather than to
    /// where it is being drawn, and a bar touched part way down its
    /// settling drops to black and climbs out of it -- a flicker at the
    /// one moment the reader is looking straight at it.
    #[test]
    fn a_bar_stirred_part_way_down_comes_up_from_where_it_is() {
        let base = Instant::now();
        let mut motion = Motion::new(None);
        let bar = a_bar(99, 0);
        motion.bars_drawn(&[bar], None, base);

        // Half way through the settling, where it is dim but not out.
        let dim = base + BAR_RISE + BAR_HOLD + BAR_SETTLE / 2;
        let was = motion.bar_shown(bar.area, dim);
        assert!(was > 0.0 && was < 1.0, "part of the way: {was}");

        // And the reader scrolls again. The next moment is brighter than
        // the one before it, all the way up.
        motion.bars_drawn(&[a_bar(99, 9)], None, dim);
        let mut last = was;
        for step in 1..=10 {
            let at = dim + BAR_RISE * step / 10;
            let now = motion.bar_shown(bar.area, at);
            assert!(
                now >= last - f32::EPSILON,
                "went down at {step}: {last} to {now}"
            );
            last = now;
        }
        assert_eq!(last, 1.0, "and arrives");
    }

    /// Break: give a bar nobody has seen before a `from` of nothing, and
    /// every list that opens has a bar brightening into a screen that has
    /// already arrived.
    #[test]
    fn a_bar_that_was_not_there_is_simply_there() {
        let base = Instant::now();
        let mut motion = Motion::new(None);
        let bar = a_bar(99, 0);
        motion.bars_drawn(&[bar], None, base);
        assert_eq!(motion.bar_shown(bar.area, base), 1.0);
    }

    /// Break: keep one `stirred` for all of them rather than one each,
    /// and a reader scrolling a list lights up the preview's bar under
    /// it, which is a control saying something is happening to a thing
    /// nothing is happening to.
    #[test]
    fn a_bar_settles_on_its_own() {
        let base = Instant::now();
        let mut motion = Motion::new(None);
        let (list, preview) = (a_bar(40, 0), a_bar(99, 0));
        motion.bars_drawn(&[list, preview], None, base);

        // Long enough that both would have settled, and then one moves.
        let later = base + BAR_ROUND;
        assert_eq!(motion.bar_shown(list.area, later), 0.0);
        motion.bars_drawn(&[a_bar(40, 7), preview], None, later);

        assert_eq!(motion.bar_shown(list.area, later), 0.0, "setting out");
        let risen = later + BAR_RISE;
        assert_eq!(
            motion.bar_shown(list.area, risen),
            1.0,
            "the one that moved"
        );
        assert_eq!(
            motion.bar_shown(preview.area, risen),
            0.0,
            "and not the one that did not"
        );
    }

    /// Break: drop the `retain`, and every bar the reader has ever had on
    /// screen is still being asked about -- a list opened and closed forty
    /// times is forty entries, and the one that comes back is found at
    /// whatever it was doing when it left.
    #[test]
    fn a_bar_that_stops_being_drawn_is_forgotten() {
        let base = Instant::now();
        let mut motion = Motion::new(None);
        let list = a_bar(40, 0);
        motion.bars_drawn(&[list], None, base);

        let later = base + BAR_ROUND;
        assert_eq!(motion.bar_shown(list.area, later), 0.0);

        // The list closes, and opens again: what comes back is new, and
        // what is new is simply there.
        motion.bars_drawn(&[], None, later);
        motion.bars_drawn(&[list], None, later);
        assert_eq!(motion.bar_shown(list.area, later), 1.0);
    }

    /// Break: take the pointer for a switch rather than for something
    /// that arrives -- return `1.0` from `pointed` the moment `under` is
    /// set -- and a pointer crossing the column on its way somewhere else
    /// flashes the bar.
    #[test]
    fn a_pointer_brightens_a_bar_rather_than_switching_it_on() {
        let base = Instant::now();
        let mut motion = Motion::new(None);
        let bar = a_bar(99, 0);
        motion.bars_drawn(&[bar], None, base);

        // Settled, and then the pointer arrives on its column.
        let still = base + BAR_ROUND;
        assert_eq!(motion.bar_under(bar.area, still), 0.0);
        motion.bars_drawn(&[bar], Some((99, 5)), still);

        assert_eq!(motion.bar_under(bar.area, still), 0.0, "setting out");
        let part = motion.bar_under(bar.area, still + BAR_UNDER / 2);
        assert!(part > 0.0 && part < 1.0, "part of the way: {part}");
        assert_eq!(motion.bar_under(bar.area, still + BAR_UNDER), 1.0);

        // And it holds there, however long ago the bar last moved.
        let ages = still + BAR_UNDER + BAR_ROUND;
        assert_eq!(motion.bar_under(bar.area, ages), 1.0);
        assert_eq!(motion.bar_shown(bar.area, ages), 1.0, "and holds it up");

        // Off again, the same way.
        motion.bars_drawn(&[bar], None, ages);
        let going = motion.bar_under(bar.area, ages + BAR_UNDER / 2);
        assert!(going > 0.0 && going < 1.0, "part of the way back: {going}");
        assert_eq!(motion.bar_under(bar.area, ages + BAR_UNDER), 0.0);
    }

    /// Break: take the bar's own column alone, and a reader aiming at
    /// something a third of a cell wide with a pointer misses it.
    #[test]
    fn a_pointer_beside_a_bar_counts_as_on_it() {
        let base = Instant::now();
        let mut motion = Motion::new(None);
        let bar = a_bar(99, 0);
        motion.bars_drawn(&[bar], Some((98, 5)), base);
        assert_eq!(motion.bar_under(bar.area, base + BAR_UNDER), 1.0);

        // And two columns off is somewhere else.
        let mut elsewhere = Motion::new(None);
        elsewhere.bars_drawn(&[bar], Some((97, 5)), base);
        assert_eq!(elsewhere.bar_under(bar.area, base + BAR_UNDER), 0.0);
    }

    /// Break: ask `Bars` directly in `bar_shown` rather than going through
    /// `animates`, and the one switch a reader has for this is the one
    /// animation it does not reach.
    #[test]
    fn a_bar_does_not_settle_where_nothing_animates() {
        let base = Instant::now();
        let mut motion = Motion::new(None);
        let bar = a_bar(99, 0);
        motion.bars_drawn(&[bar], None, base);
        motion.animates(false);

        let after = base + BAR_ROUND;
        assert_eq!(motion.bar_shown(bar.area, after), 1.0);
        assert_ne!(motion.wake(after, false), Some(Wake::EveryFrame));
    }

    /// Break: gate `bar_under` on `animates` the way `bar_shown` is, and a
    /// reader who wants nothing to move puts a pointer on a control and is
    /// told nothing. What that switch turns off is time passing on its
    /// own; a pointer is the reader, moving.
    #[test]
    fn a_pointer_is_answered_even_where_nothing_animates() {
        let base = Instant::now();
        let mut motion = Motion::new(None);
        let bar = a_bar(99, 0);
        motion.animates(false);
        motion.bars_drawn(&[bar], Some((99, 5)), base);
        assert_eq!(
            motion.bar_under(bar.area, base),
            1.0,
            "at once, and not over time"
        );
    }

    /// Break: leave `bars` out of `wake`, and a bar settles only where
    /// something else happens to be waking the window -- so it hangs at
    /// full strength on a screen nobody is touching and steps down the
    /// moment a key is pressed.
    #[test]
    fn a_settling_bar_asks_for_frames_until_it_is_down() {
        let base = Instant::now();
        let mut motion = Motion::new(None);
        let bar = a_bar(99, 0);
        motion.bars_drawn(&[bar], None, base);

        assert_eq!(motion.wake(base, false), Some(Wake::EveryFrame));
        assert_eq!(
            motion.wake(base + BAR_RISE + BAR_HOLD + BAR_SETTLE / 2, false),
            Some(Wake::EveryFrame)
        );
        assert_ne!(
            motion.wake(base + BAR_ROUND, false),
            Some(Wake::EveryFrame),
            "down, and nothing left to draw"
        );
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

    /// Break: leave `from` alone where one of the two is `None`, and a
    /// flight already in the air carries on after the caret it was about
    /// has been handed to something else -- a list opening mid-flight
    /// leaves a caret walking to a box that is no longer there.
    #[test]
    fn a_caret_that_changed_hands_is_not_still_on_its_way() {
        let base = Instant::now();
        let mut motion = Motion::new(None);
        motion.caret_moved(Some(at(0, 10)), Some(at(0, 0)), base);
        let midway = base + GLIDE / 2;
        assert_ne!(motion.moving(midway).drift, (0.0, 0.0), "on its way");
        // Which is what the window says by leaving out where it was.
        motion.caret_moved(None, Some(at(4, 20)), midway);
        assert_eq!(motion.moving(midway).drift, (0.0, 0.0));
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

    /// Break: leave the check out of any one of the three doors, and a
    /// reader who turned animation off still gets that one -- which is a
    /// switch they watched do nothing to the thing they turned it off
    /// for.
    #[test]
    fn a_reader_who_turned_animation_off_gets_none_of_it() {
        let base = Instant::now();
        let mut motion = Motion::new(None);
        motion.animates(false);

        motion.caret_moved(Some(at(0, 10)), Some(at(0, 0)), base);
        assert_eq!(motion.moving(base).drift, (0.0, 0.0), "the caret");
        motion.pane_opened(base);
        assert_eq!(motion.moving(base).pane, None, "a pane");
        assert!(!motion.band_moved(3.0, 20.0, base), "a band");
        assert_eq!(motion.moving(base).scroll, None);
        assert_eq!(motion.wake(base, true), None, "and nothing to wake for");

        // And what was under way when they turned it off is dropped: one
        // that finished itself afterwards would be the switch taking a
        // moment to mean anything.
        let mut motion = Motion::new(None);
        motion.caret_moved(Some(at(0, 10)), Some(at(0, 0)), base);
        motion.band_moved(3.0, 20.0, base);
        motion.pane_opened(base);
        motion.animates(false);
        assert_eq!(motion.moving(base).drift, (0.0, 0.0));
        assert_eq!(motion.moving(base).scroll, None);
        assert_eq!(motion.moving(base).pane, None);
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
