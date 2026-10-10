//! What moves on screen, and the clocks that bring the loop back.

use super::*;

impl App {
    /// Whether anything on screen is moving.
    ///
    /// Several reasons, each said out loud. It was one question with a
    /// `match` on whether the conversation was showing, and that stopped
    /// being true the moment a list of open documents could say an agent is
    /// working in one the reader is not looking at -- a mark that only
    /// turns while you are watching it is a mark that never turns. The
    /// notes say it too, about the conversation a note has.
    ///
    /// Asked every frame from what is true, rather than switched on and off
    /// from the half-dozen places that change any of it, which is how a
    /// ticker outlives its reason.
    pub(super) fn wants_animating(&self, working: bool) -> bool {
        if self.headless {
            return false;
        }
        // Nothing open and no page taking its place: the welcome screen's
        // sheen. A settings page over the welcome is not a welcome screen,
        // so keeping its clock running would redraw a motionless page.
        //
        // And not where a window is drawing, because there the sheen is
        // the window's own and runs on the window's own clock. What this
        // ticker moves is the ramp written into the cells, which is the
        // sheen a terminal can draw and the one thing a window does not
        // read -- so it would be twelve pages a second pushed at a front
        // end that draws the light itself, on the one screen a reader
        // leaves up while they decide what to open. The cells keep the
        // ramp they were last drawn with, which is that sheen at one
        // moment and as true as any other frame of it.
        //
        // Nor while Obelus is asking which project: that screen is not the
        // welcome screen and has no mark to run a sheen across.
        let sheen = self.current.is_none()
            && self.chooser.is_none()
            && !self.layers().filling()
            && !obelus_config::in_a_window();
        // And a drag held against an edge, which is the one of these that
        // is waiting on the reader's hand rather than on something
        // happening by itself. It is here for the same reason as the
        // rest: without a tick it stops, and a selection that stops at the
        // edge of the screen is a selection of what fits on it.
        //
        // The rest are all a mark that turns, which a window turns itself
        // on its own clock -- the same reason the sheen is not ticked
        // there. Except where the reader turned animation off: then the
        // window turns it a frame a tick, the frame this clock writes.
        let window_turns = obelus_config::in_a_window() && self.settled.config.animation;
        sheen || self.dragging.is_some() || (!window_turns && self.turning(working))
    }

    /// Whether a mark that turns is on the screen -- see `wants_animating`.
    fn turning(&self, working: bool) -> bool {
        // An agent at work in the conversation being read.
        working
            // Or in one that is not, while the list that says so is open.
            || (self.selected_document().is_some() && self.anything_working())
            // Or while the notes are, which say the same thing about the
            // conversation a note has: the mark beside a note turns for
            // exactly as long as its agent is at work, and without this
            // it would be woken only by the reader typing -- a mark that
            // moves when you touch it and stands still while the work
            // happens.
            || (self.notes().is_some() && self.anything_working())
            // A row of a tree of calls waiting on a server. The same rule
            // as a conversation's: a mark that turns has to be woken, and
            // a mark that does not turn is a mark saying nothing is
            // happening.
            || self.calls_turning()
            // And the badge on the status row, for as long as this file's
            // server is reading the project. The same rule again, and the
            // reason the badge turns at all: an empty answer while it
            // reads and an empty answer about a symbol with no definition
            // are the same message, and a mark standing still says the
            // second.
            || self.server_busy()
            // And the chat's mark, while the window it talks to connects.
            || self.remote_turning()
            // And a list being matched somewhere else, or still being
            // filled. The same rule once more: the row that says so turns,
            // and a mark drawn once and never again is a mark saying
            // nothing is happening -- which is the one thing this row
            // exists to contradict. Filled as well as matched, because a
            // list waiting on one answer from somewhere else -- the pull
            // requests, from `gh` -- has no batches arriving to redraw it.
            || self
                .picker
                .as_ref()
                .is_some_and(|picker| picker.is_filling().is_some())
            // And a preview whose last part is still being asked for: what
            // has happened on a pull request, which is one answer with
            // nothing arriving before it.
            || self.preview_turns()
            // And an install, while the page of agents is open: a card
            // whose package manager says nothing until it is done has only
            // its mark to say the install is still going.
            || (self.settings().is_some_and(Settings::on_agents)
                && !self.agents.installing.is_empty())
    }

    /// Whether an agent is at work in any conversation at all.
    fn anything_working(&self) -> bool {
        let Some(talker) = self.talker.as_ref() else {
            return false;
        };
        self.documents
            .iter()
            .flatten()
            .filter_map(Document::chat)
            .any(|talk| talker.is_thinking(talk.session.as_ref(), talk.requested))
    }

    /// Whether the last frame asked to be woken again.
    ///
    /// Which is the difference between a tree that catches up on its own
    /// and one that waits for the reader to press something else.
    #[must_use]
    pub const fn is_waking(&self) -> bool {
        self.waking
    }

    /// Whether any open document's tree is older than its text.
    fn anything_behind(&self) -> bool {
        self.documents
            .iter()
            .flatten()
            .filter_map(Document::file)
            .any(Buffer::syntax_is_behind)
    }

    /// How long a tree may be behind its text before it is caught up.
    ///
    /// What the animation's tick used to give this by accident, kept at the
    /// same length so that the colours arrive when they always have: long
    /// enough that a burst of keys is one parse, short enough that the
    /// reader is still looking at what they typed.
    const CATCHES_UP_AFTER: std::time::Duration = std::time::Duration::from_millis(80);

    /// Comes back for the trees that owe an answer, unless something
    /// already is.
    ///
    /// Set by the first frame that notices and not put back by the ones
    /// after it -- the same rule the watcher's debouncing follows, and for
    /// the same reason: a deadline that slid would never arrive while the
    /// reader kept typing, which is exactly when a tree is behind.
    ///
    /// Asked from what is true rather than started where a document
    /// changes, which is how the ticker was asked and is what stops a clock
    /// outliving its reason.
    pub(super) fn catch_up_soon(&mut self) {
        if !self.anything_behind() {
            self.syntax_pause = None;
            return;
        }
        if self.syntax_pause.is_some() {
            return;
        }
        self.syntax_pause = self.come_back_in(Self::CATCHES_UP_AFTER, Event::SyntaxSettled);
    }

    /// Works out what every document that owes it means now.
    ///
    /// Everything open rather than what is on screen: a tree left behind on
    /// a document nobody is looking at would keep the ticker awake for the
    /// rest of the session.
    pub(crate) fn settle_syntax(&mut self) {
        // Nothing else has to be told: the text did not move, only what
        // Obelus knows about it, so everything keyed on the version stays
        // keyed on the version it already had.
        for buffer in self
            .documents
            .iter_mut()
            .flatten()
            .filter_map(Document::file_mut)
        {
            buffer.settle_syntax();
        }
    }

    /// A clock to come back with, where there is a loop to come back to.
    ///
    /// The one place that knows a timer needs the loop's channel. `None`
    /// without one, which is a test driving its own events: whether
    /// something is waiting is decided either way, and what a test drives
    /// by hand is the event the clock would have sent.
    ///
    /// Every wait Obelus keeps goes through here -- the notes, a tree
    /// behind its text, a rename's server, a document's standing questions
    /// and the pointer's rest -- so that the answer to "is there anything
    /// to come back from" is written once.
    pub(super) fn come_back_in(
        &self,
        after: std::time::Duration,
        event: Event,
    ) -> Option<crate::event::Pause> {
        self.events
            .clone()
            .map(|events| crate::event::Pause::start(events, after, event))
    }

    /// Starts or stops the ticker, and does nothing where it is already
    /// what it should be.
    ///
    /// A thread waking twelve times a second to redraw a screen with
    /// nothing moving on it is the one cost an animation must not have.
    pub(super) fn animate(&mut self, wanted: bool) {
        self.waking = wanted;
        match (wanted, self.ticker.is_some()) {
            (true, false) => self.ticker = self.events.clone().and_then(Ticker::start),
            (false, true) => self.ticker = None,
            _ => {}
        }
    }
}
