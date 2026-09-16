//! What is over the file being read, and in what order.
//!
//! One declaration, read by everything that used to answer it on its own:
//! which view a key is offered to, what order they are drawn in, whether
//! obelus's own keys reach the file underneath, where the caret goes, and
//! what a view covers when it opens.
//!
//! Eight places used to decide this and they did not agree. The counts were
//! left out of the rule that stops typing reaching the file, so a letter
//! pressed over a table of numbers went into a file nobody could see; the
//! notes were left out of the rule that stops the pointer, so a click landed
//! behind them; and the caret was worked out by a chain in a different order
//! from the one the keys walk, so with a question on the status bar and a
//! list over it the caret sat in one and the typing went to the other. None
//! of those were decisions. They were a predicate somebody had to remember
//! to update, and did not.
//!
//! Nothing here draws. A layer says how much room it takes; `ui` reads that
//! and hands it a rectangle.

use crate::keymap::Context;

/// One thing the reader is *in*, over the file being read.
///
/// The variants are the whole list, and no `match` on this may carry a `_`
/// arm. Adding a view should be a compile error in every place that has to
/// answer for it -- what it covers, where it is drawn, whether a key reaches
/// past it -- which is the one thing eight hand-kept predicates could not
/// do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    /// A conversation with an agent.
    ///
    /// Here because it is one today: it takes the editor region, it is left
    /// with escape, and obelus's own keys do not reach past it. It is the
    /// one of these that should not be a layer at all -- a conversation is
    /// somewhere the reader goes back to, which is a document rather than
    /// something over one -- and when it becomes a document this variant
    /// goes, and every place that had to answer for it says so by failing
    /// to compile.
    Chat,
    /// How much code is here.
    Counts,
    /// What the tree means to come back to.
    Notes,
    /// The settings, the reader's or the tree's.
    Settings,
    /// A list with a query over it: the palette, the files, a search, a
    /// setting's choices, a commit's history, what a server offers to do.
    Picker,
    /// A question on the status bar.
    Prompt,
}

/// Every layer there is, furthest from the reader first.
///
/// Drawing walks this forwards and keys walk it backwards, and nothing else
/// may hold an order. Public so that anything which has to answer for all of
/// them -- a test, most of all -- reads the list from here rather than
/// keeping a second copy that can fall behind. The three pages -- the counts, the notes and the
/// settings -- cannot be open together, so their order among themselves is
/// never observed; it is declared anyway, because an order nobody wrote down
/// is an order every reader of the code guesses at.
pub const STACK: [Layer; 6] = [
    Layer::Chat,
    Layer::Counts,
    Layer::Notes,
    Layer::Settings,
    Layer::Picker,
    Layer::Prompt,
];

/// How much of the screen a view takes.
///
/// The view's own declaration, not something read off what it happens to
/// draw. It decides what the view covers when it opens, whether the file
/// behind it can still be pointed at, and which rectangle it is handed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Room {
    /// One row, on the status bar, with what it is asking about still on
    /// screen behind it.
    Row,
    /// A band of the editor region, with the code above and below it still
    /// readable.
    Band,
    /// The whole editor region, and obelus's status row under it.
    Region,
    /// The screen: the status row and the rule above it included. A view
    /// with no file and no cursor in it has nothing for that row to say.
    Screen,
}

impl Room {
    /// Whether a view taking this room covers one taking that room.
    ///
    /// The whole of the rule that opening obeys. A page covers everything:
    /// it *is* the region, and a question on the status bar about something
    /// now behind it is a question nobody can answer. A list covers the
    /// question and the list that was there and nothing else -- a setting's
    /// choices open *over* the settings, which is the one place obelus
    /// stacks two things the reader is in.
    #[must_use]
    pub const fn covers(self, other: Self) -> bool {
        match self {
            Self::Row => matches!(other, Self::Row),
            Self::Band => matches!(other, Self::Row | Self::Band),
            Self::Region | Self::Screen => true,
        }
    }
}

impl Layer {
    /// How much of the screen this one takes.
    #[must_use]
    pub const fn room(self) -> Room {
        match self {
            Self::Counts => Room::Screen,
            Self::Chat | Self::Notes | Self::Settings => Room::Region,
            Self::Picker => Room::Band,
            Self::Prompt => Room::Row,
        }
    }

    /// Which key table applies while this is showing.
    ///
    /// A dialog takes the keys bound in it and no others, so obelus's own
    /// commands cannot open a second one over the first. The question on the
    /// status bar is not one of those: it is a row rather than a screen,
    /// what it is asking about is still visible, and `ctrl+q` still leaves
    /// obelus from inside it.
    ///
    /// Read off the room rather than declared a second time, because it is
    /// the same fact said twice: a view that covers nothing is not a view
    /// the reader is shut inside.
    #[must_use]
    pub const fn context(self) -> Context {
        match self.room() {
            Room::Row => Context::Normal,
            Room::Band | Room::Region | Room::Screen => Context::Dialog,
        }
    }
}

/// What is on screen, over the file.
///
/// Derived, never stored: a copy of it would be a ninth answer to the
/// question, kept right by hand. Cheap to take -- five `Option::is_some` --
/// so it is taken again wherever it is wanted rather than passed around.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Layers {
    shown: [bool; STACK.len()],
}

impl Layers {
    /// Asks, of each layer there is, whether it is showing.
    ///
    /// A closure rather than an array, so that the order stays in this
    /// module: a caller handed the array would have to know which slot
    /// means which view, and that is a second place holding the order.
    #[must_use]
    pub(super) fn showing(mut open: impl FnMut(Layer) -> bool) -> Self {
        Self {
            shown: STACK.map(&mut open),
        }
    }

    /// Furthest from the reader first, which is the order they are drawn in.
    pub fn furthest_first(self) -> impl DoubleEndedIterator<Item = Layer> {
        STACK
            .into_iter()
            .zip(self.shown)
            .filter_map(|(layer, shown)| shown.then_some(layer))
    }

    /// Nearest the reader first, which is the order a key is offered in.
    pub fn nearest_first(self) -> impl Iterator<Item = Layer> {
        self.furthest_first().rev()
    }

    /// What the reader is in, if anything.
    ///
    /// Where the keys are going and where the caret is, which have to be one
    /// answer: a caret drawn in one view while the typing reaches another is
    /// a screen that lies about what a key will do.
    #[must_use]
    pub fn nearest(self) -> Option<Layer> {
        self.nearest_first().next()
    }

    /// Whether anything at all is over the file.
    #[must_use]
    pub fn any(self) -> bool {
        self.shown.iter().any(|shown| *shown)
    }

    /// Whether anything on screen hides the file being read.
    ///
    /// Not the same question as [`Self::any`]: a row on the status bar
    /// leaves every line of the file visible, and a line the reader can see
    /// is a line they can point at.
    #[must_use]
    pub fn covering(self) -> bool {
        self.furthest_first()
            .any(|layer| !matches!(layer.room(), Room::Row))
    }

    /// Whether this one is among them.
    #[must_use]
    pub fn has(self, layer: Layer) -> bool {
        self.furthest_first().any(|shown| shown == layer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn showing(layers: &[Layer]) -> Layers {
        Layers::showing(|layer| layers.contains(&layer))
    }

    /// The two orders are one order, read from either end.
    #[test]
    fn the_nearest_is_the_last_one_drawn() {
        let both = showing(&[Layer::Settings, Layer::Picker]);
        assert_eq!(
            both.furthest_first().collect::<Vec<_>>(),
            [Layer::Settings, Layer::Picker]
        );
        assert_eq!(
            both.nearest_first().collect::<Vec<_>>(),
            [Layer::Picker, Layer::Settings]
        );
        assert_eq!(both.nearest(), Some(Layer::Picker));
    }

    /// Nothing open is nothing to answer for.
    #[test]
    fn an_empty_stack_is_empty() {
        let none = showing(&[]);
        assert!(!none.any());
        assert!(!none.covering());
        assert_eq!(none.nearest(), None);
        assert_eq!(none.furthest_first().count(), 0);
    }

    /// A question on the status bar is over the file and does not hide it,
    /// which is the distinction the pointer needs and the keys do not.
    #[test]
    fn a_question_does_not_cover_the_file() {
        let asking = showing(&[Layer::Prompt]);
        assert!(asking.any(), "a question is something the reader is in");
        assert!(!asking.covering(), "a question is one row, not a screen");

        let listing = showing(&[Layer::Picker]);
        assert!(listing.covering(), "a list is drawn over the code");
    }

    /// A page covers everything, a list covers a question and another list,
    /// and a question covers nothing but another question.
    #[test]
    fn what_each_room_covers() {
        for room in [Room::Row, Room::Band, Room::Region, Room::Screen] {
            assert!(room.covers(Room::Row), "{room:?} does not cover a row");
            assert!(room.covers(room), "{room:?} does not cover its own kind");
        }
        // The one nesting obelus has: a setting's choices over the settings.
        assert!(!Room::Band.covers(Room::Region));
        assert!(Room::Region.covers(Room::Band));
        // And a question cannot stay up under anything else.
        assert!(!Room::Row.covers(Room::Band));
    }

    /// The key table follows the room, so the two cannot disagree.
    #[test]
    fn only_a_row_leaves_the_global_keys_alone() {
        assert_eq!(Layer::Prompt.context(), Context::Normal);
        for layer in [
            Layer::Chat,
            Layer::Counts,
            Layer::Notes,
            Layer::Settings,
            Layer::Picker,
        ] {
            assert_eq!(layer.context(), Context::Dialog, "{layer:?}");
        }
    }
}
