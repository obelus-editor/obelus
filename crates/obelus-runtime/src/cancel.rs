//! Telling work that has been started that nobody wants it any more.
//!
//! Obelus starts a walk, a search or a history the moment the reader asks,
//! and the reader asks again before the first one has finished: another
//! tab, another query, a list opened and closed. What it did until now was
//! number each attempt and throw away the answers from the old ones -- the
//! `generation` a batch carries back. The work still ran, to the end, on a
//! thread nobody was waiting for.
//!
//! This is the same number, made readable from the inside. The worker
//! carries the generation it was started under, and the reader's side holds
//! the generation now: one atomic load per batch tells the worker it has
//! been replaced, and it stops.
//!
//! One load per batch and not per entry. A batch is a hundred paths and a
//! load is a few nanoseconds either way -- the point is not the cost, it is
//! that the check happens where the work is already pausing to send.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// Which attempt is the current one.
///
/// Held by the application, and cloned into everything it starts.
#[derive(Clone, Debug, Default)]
pub struct Latest(Arc<AtomicU64>);

impl Latest {
    /// Starts another attempt, and says which one it is.
    ///
    /// Everything started before this is superseded from here on, which is
    /// the point of bumping it *first*: a worker that checks between the
    /// bump and its own start finds itself already replaced and does
    /// nothing, which is the right answer.
    pub fn next(&self) -> u64 {
        self.0.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Which attempt is current.
    #[must_use]
    pub fn now(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }

    /// Whether an answer from this attempt is still wanted.
    #[must_use]
    pub fn is_current(&self, generation: u64) -> bool {
        self.now() == generation
    }

    /// A claim a worker can carry and ask about.
    #[must_use]
    pub fn claim(&self, generation: u64) -> Wanted {
        Wanted {
            latest: Arc::clone(&self.0),
            mine: generation,
        }
    }
}

/// What a worker asks to find out whether to keep going.
#[derive(Clone, Debug)]
pub struct Wanted {
    latest: Arc<AtomicU64>,
    mine: u64,
}

impl Wanted {
    /// Whether anybody is still waiting for this.
    #[must_use]
    pub fn still(&self) -> bool {
        self.latest.load(Ordering::Relaxed) == self.mine
    }

    /// The generation this is about, for putting on what it sends back.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.mine
    }
}
