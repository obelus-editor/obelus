//! What is open over the file, and leaving it.

use super::*;

impl App {
    /// Which set of key bindings a key is looked up in.
    ///
    /// What the reader is in, rather than what they are doing: a dialog
    /// takes the keys bound in it and no others, so Obelus's own commands
    /// cannot open a second dialog over the first -- `f1` in a
    /// conversation used to put a file list on top of it, which then took
    /// two escapes to leave and gave no way to tell which of the two a key
    /// would reach.
    pub(crate) fn context(&self) -> Context {
        // Being asked which project is a dialog like any other, and takes
        // what `Context::Dialog` binds: leaving, and the four keys that
        // act on what the reader has hold of -- a box they can select in
        // and not paste into is half a box. Everything else in Obelus is
        // about a project, and `Requires::AProject` is what refuses it.
        if self.chooser.is_some() {
            return Context::Dialog;
        }
        // A list whose rows are open files is the list of open files, and
        // that one has a command of its own.
        if self.selected_document().is_some() {
            return Context::Documents;
        }
        if self.is_showing_dialog() {
            return Context::Dialog;
        }
        if self.conversation().is_some() {
            return Context::Chat;
        }
        if self.typing_to_a_program() {
            return Context::Terminal;
        }
        Context::Normal
    }

    /// What is on screen over the file, worked out from what is open.
    ///
    /// The one answer. Everything that used to ask "is a dialog showing" or
    /// "which of these is nearest" asks this instead, and the `match` below
    /// is the single place where which field means which layer is written
    /// down. It is exhaustive, so a view added without an answer here is a
    /// view that does not compile.
    #[must_use]
    pub fn layers(&self) -> layers::Layers {
        layers::Layers::showing(|layer| match layer {
            layers::Layer::Counts => self.counts.is_some(),
            layers::Layer::Settings => self.settings.is_some(),
            layers::Layer::Names => self.names.is_some(),
            layers::Layer::Picker => self.picker.is_some(),
            layers::Layer::Prompt => self.prompt.is_some(),
            layers::Layer::Gone => self.gone,
        })
    }

    /// Clears the room a view is about to take.
    ///
    /// The one thing every opener does, in one place. There were six of
    /// them, each with its own idea: two cleared a list and a question, one
    /// cleared a list and the settings, and three cleared nothing at all --
    /// including the two added most recently, which is the shape of the
    /// problem. Nothing made anybody think about it, so nobody did.
    ///
    /// The rule is not "opening covers": that is false for a question on the
    /// status bar, which is about the thing now behind it, and it would
    /// allow two pages at once. The rule is that a view covers what shares
    /// its room, which is [`Room::covers`] and is declared beside the view
    /// rather than here.
    pub(crate) fn make_room(&mut self, room: layers::Room) {
        for layer in self.layers().nearest_first() {
            if room.covers(layer.room()) {
                self.leave(layer);
            }
        }
    }

    /// Takes one layer down, doing whatever leaving it means.
    ///
    /// The same door escape goes through, so a view cannot be left one way
    /// and not the other: the notes are written down however the reader
    /// leaves them.
    pub(crate) fn leave(&mut self, layer: Layer) {
        match layer {
            // Not left: answered. Escape gives up on the nearest thing,
            // and here there is nothing nearer to give up on and nothing
            // behind it to give up to -- what is behind it is the project
            // that went.
            Layer::Gone => {}
            Layer::Picker => {
                self.picker = None;
                // Back to where they were looking from. The other way out
                // of a list is choosing a row, and that goes somewhere on
                // purpose -- see `App::accept`.
                self.look_back();
                self.history = history_view::Showing::default();
                self.troubling.clear();
                self.conversing = conversations::Conversing::default();
                self.close_calls();
                // What a server offered to do here, which the rows were
                // indexes into. A row is chosen by its position, so offers
                // outliving their list are offers pointing at nothing.
                self.code_actions.clear();
                // Nothing about the agent's question: that is a card in the
                // conversation, not a list, and a list opened over it and
                // closed again -- or one that took the reader to another
                // conversation -- was never the question. Refusing it here
                // answered "no" for a reader who had only looked at F2.
                // A theme previewed but not chosen, and the file a
                // question about leaving took the reader to. The two
                // things a picker changes about the application while it
                // is open, and so the two that have to be put back.
                self.go_back_from_asking();
                if let Some((name, before)) = self.theme_before.take() {
                    self.set_theme(&name, before);
                }
            }
            Layer::Names => self.names = None,
            Layer::Settings => self.settings = None,
            Layer::Counts => self.counts = None,
            Layer::Prompt => self.prompt = None,
        }
    }

    /// Whether something is showing that the reader is *in*.
    ///
    /// A list, the settings, or the counts: each takes the keys itself, each
    /// is left with escape, and each is over whatever is being read rather
    /// than being it. Obelus's own commands do not run from inside one, so
    /// the only way to a second one is to leave the first.
    ///
    /// A conversation is not one of these, and stopped being one when it
    /// became a document: it is *what* is being read, not something over it,
    /// which is why Obelus's own keys work inside one.
    ///
    /// The question on the status bar is not one of these. It is a row
    /// rather than a screen, what it is asking about is still visible
    /// behind it, and it says its own answer to escape.
    #[must_use]
    pub fn is_showing_dialog(&self) -> bool {
        self.layers()
            .furthest_first()
            .any(|layer| matches!(layer.context(), obelus_keymap::Context::Dialog))
    }
}
