//! How much code is here: opening the view, and what comes back from the walk.

use obelus_component::counts::{Counts, CountsOutcome};
use obelus_search::counts::Counted;

use super::{
    App,
    layers::{Layer, Room},
};

impl App {
    /// The line counts, while they are showing.
    #[must_use]
    pub const fn counts(&self) -> Option<&Counts> {
        self.counts.as_ref()
    }

    /// Opens the line counts and starts the walk that fills them.
    ///
    /// Nothing else may be open under it: it is the whole editor region
    /// while it is showing, and two views taking the same keys is two views
    /// neither of which can say what a keypress will do.
    pub fn open_counts(&mut self) {
        self.make_room(Room::Screen);
        self.counts = Some(Counts::new());
        // Counted on every opening rather than once and kept. A count is a
        // fact about the tree as it is now, and this is a program several
        // copies of which sit over one repository while an agent rewrites
        // it: a cached answer would be a screen full of numbers about a
        // project that has moved on, with nothing on it saying so.
        if let Some(sender) = self.events.clone() {
            obelus_search::counts::spawn_count(&self.working_directory, sender);
        }
    }

    /// Takes what the walk found.
    ///
    /// Dropped if the view has been closed in the meantime, which is what
    /// every late answer here does: there is nowhere to put it, and the next
    /// opening starts a walk of its own.
    pub(super) fn on_counted(&mut self, counted: Counted) {
        if let Some(counts) = self.counts.as_mut() {
            counts.show(counted);
        }
    }

    /// Offers a key to the counts. `true` if they took it.
    pub(super) fn counts_key(&mut self, key: &crossterm::event::KeyEvent) -> bool {
        // How many rows of list are on screen, which is what a page is worth
        // here. The view's own geometry answers it, from the whole screen --
        // this is the one view with no status row under it -- so the list
        // and the keys that move about it cannot disagree about how far a
        // page goes.
        let Some(counts) = self.counts.as_ref() else {
            return false;
        };
        let rows = crate::ui::counts::list_height(self.screen_area, counts);
        let Some(counts) = self.counts.as_mut() else {
            return false;
        };
        match counts.handle_key(key, rows) {
            CountsOutcome::Consumed => true,
            CountsOutcome::Cancelled => {
                self.leave(Layer::Counts);
                true
            }
            CountsOutcome::Open(path) => {
                // The view goes away, because going somewhere means seeing
                // it -- the same thing choosing a row in any list does.
                self.counts = None;
                self.open(&self.working_directory.join(path));
                true
            }
            CountsOutcome::Ignored => false,
        }
    }
}
