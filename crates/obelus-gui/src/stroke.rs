//! A stroke on a touchpad, and what is left of it once the reader has done
//! something else.
//!
//! A touchpad's scroll does not end when the fingers leave it: macOS goes
//! on sending the scroll, slowing, for a second or more after -- the coast.
//! A coast is the end of one gesture about one thing. A reader who presses
//! `f1` in the middle of one has moved on from it, and what is left of it
//! used to arrive at whatever was in front by then: the file stopped and
//! the list just opened over it scrolled away under the reader's eyes, by a
//! gesture they made at the file. So a key or a press cuts the stroke, and
//! nothing more of it is heard -- on the thing it was about either, which
//! the reader has just turned away from.
//!
//! Asked here rather than of the application, because only the window can
//! see where a stroke starts and stops (winit's `TouchPhase`); `ob` is told
//! of a notch and nothing about whose it is.
//!
//! winit folds the two halves into one stream: the fingers' own `Started`,
//! `Moved`, `Ended`, and then the coast's `Started`, `Moved`, `Ended` --
//! with nothing to say which `Started` is which. What says it is the time:
//! a coast starts the moment the fingers leave, and fingers come back to
//! the pad a good deal later than that. A mouse's wheel has no phases at
//! all -- every notch is `Moved` -- and so is never in a stroke, and never
//! cut.

use std::time::{Duration, Instant};

use winit::event::TouchPhase;

/// How soon after the fingers leave a `Started` is the coast of what they
/// did rather than a new stroke.
///
/// AppKit starts the coast with the next event after the lift, a frame or
/// two; a reader putting their fingers down again takes many times this.
const COAST: Duration = Duration::from_millis(100);

/// Where a touchpad's stroke is, and whether it has been cut.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum Stroke {
    /// No stroke: a mouse's wheel, or a touchpad at rest.
    #[default]
    Idle,
    /// The fingers are on the pad.
    Touching {
        /// Whether a key or a press came while they were.
        cut: bool,
    },
    /// The fingers have left, and a coast may follow.
    Lifted {
        /// When they left.
        at: Instant,
        /// Whether the stroke was cut before they did.
        cut: bool,
    },
    /// What the fingers did, going on without them.
    Coasting {
        /// Whether a key or a press came in the middle of it.
        cut: bool,
    },
}

impl Stroke {
    /// Takes one scroll's phase, and answers whether the scroll is to be
    /// heard.
    pub(crate) fn heard(&mut self, phase: TouchPhase, now: Instant) -> bool {
        let lifted = |at: Instant| now.saturating_duration_since(at) < COAST;
        let (next, cut) = match (phase, *self) {
            (TouchPhase::Started, Self::Lifted { at, cut }) if lifted(at) => {
                (Self::Coasting { cut }, cut)
            }
            // Fingers, whatever was going on: fingers on the pad stop a
            // coast, and a stroke begun afresh is the reader's again.
            (TouchPhase::Started, _) => (Self::Touching { cut: false }, false),
            (TouchPhase::Ended | TouchPhase::Cancelled, Self::Touching { cut }) => {
                (Self::Lifted { at: now, cut }, cut)
            }
            (TouchPhase::Ended | TouchPhase::Cancelled, Self::Coasting { cut }) => {
                (Self::Idle, cut)
            }
            (TouchPhase::Ended | TouchPhase::Cancelled, _) => (Self::Idle, false),
            // Long enough after a lift that no coast followed: whatever
            // turns now is a wheel, which nothing cuts.
            (TouchPhase::Moved, Self::Lifted { at, .. }) if !lifted(at) => (Self::Idle, false),
            (TouchPhase::Moved, stroke) => (stroke, stroke.is_cut()),
        };
        *self = next;
        !cut
    }

    /// The reader did something else: what is left of the stroke is
    /// nobody's.
    pub(crate) fn cut(&mut self) {
        match self {
            Self::Idle => {}
            Self::Touching { cut } | Self::Lifted { cut, .. } | Self::Coasting { cut } => {
                *cut = true
            }
        }
    }

    fn is_cut(self) -> bool {
        match self {
            Self::Idle => false,
            Self::Touching { cut } | Self::Lifted { cut, .. } | Self::Coasting { cut } => cut,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame's worth of time, the gap between one scroll and the next.
    const FRAME: Duration = Duration::from_millis(16);

    /// Plays the phases a frame apart from `start`, and says which were
    /// heard.
    fn play(stroke: &mut Stroke, start: Instant, phases: &[TouchPhase]) -> Vec<bool> {
        phases
            .iter()
            .zip(0u32..)
            .map(|(&phase, frame)| stroke.heard(phase, start + FRAME * frame))
            .collect()
    }

    use TouchPhase::{Ended, Moved, Started};

    /// A key in the middle of a coast silences the rest of it.
    ///
    /// Deliberate break: `cut` doing nothing -- which is what the window did
    /// before this -- hears every scroll after the key, and the list a key
    /// opened is scrolled by the coast of a gesture made at the file.
    #[test]
    fn a_key_in_the_middle_of_a_coast_silences_the_rest_of_it() {
        let mut stroke = Stroke::default();
        let now = Instant::now();
        assert_eq!(
            play(
                &mut stroke,
                now,
                &[Started, Moved, Moved, Ended, Started, Moved]
            ),
            [true; 6]
        );
        stroke.cut();
        let later = now + FRAME * 6;
        assert_eq!(play(&mut stroke, later, &[Moved, Moved, Ended]), [false; 3]);
    }

    /// A key while the fingers are still moving silences the coast that
    /// follows their lift, which is the same stroke.
    ///
    /// Deliberate break: taking every `Started` as a new stroke, which is
    /// what winit's phases alone say, hears the coast again.
    #[test]
    fn the_coast_after_a_cut_touch_is_cut_too() {
        let mut stroke = Stroke::default();
        let now = Instant::now();
        play(&mut stroke, now, &[Started, Moved]);
        stroke.cut();
        let later = now + FRAME * 2;
        assert_eq!(
            play(&mut stroke, later, &[Moved, Ended, Started, Moved, Ended]),
            [false; 5]
        );
    }

    /// Fingers put back on the pad after a cut are a stroke of the
    /// reader's again, whether the coast had finished or not.
    ///
    /// Deliberate break: carrying `cut` into every `Started` leaves the
    /// list the key opened deaf to the reader's own next stroke.
    #[test]
    fn fingers_put_back_are_heard() {
        let mut stroke = Stroke::default();
        let now = Instant::now();
        play(&mut stroke, now, &[Started, Moved, Ended, Started, Moved]);
        stroke.cut();
        // Into the coast, which fingers on the pad stop.
        let later = now + FRAME * 10;
        assert_eq!(play(&mut stroke, later, &[Started, Moved]), [true; 2]);
        // And after a touch that was cut and lifted with no coast at all.
        stroke.cut();
        let lifted = later + FRAME * 2;
        assert!(!stroke.heard(Ended, lifted));
        let back = lifted + Duration::from_millis(400);
        assert_eq!(play(&mut stroke, back, &[Started, Moved]), [true; 2]);
    }

    /// A mouse's wheel is never in a stroke, so a key cuts nothing of it.
    ///
    /// Deliberate break: cutting from `Idle` too makes every wheel after
    /// the first key deaf for good, since a wheel never says `Started`.
    #[test]
    fn a_wheel_is_never_cut() {
        let mut stroke = Stroke::default();
        stroke.cut();
        assert_eq!(
            play(&mut stroke, Instant::now(), &[Moved, Moved]),
            [true; 2]
        );
        // Nor a wheel turned a while after a cut touchpad's last stroke.
        let now = Instant::now();
        play(&mut stroke, now, &[Started, Moved]);
        stroke.cut();
        stroke.heard(Ended, now + FRAME * 2);
        assert!(stroke.heard(Moved, now + Duration::from_secs(1)));
    }
}
