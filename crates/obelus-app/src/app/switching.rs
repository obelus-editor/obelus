//! Going from one view to another by the key that names it.
//!
//! A view keeps its keys to itself so that nothing opens *over* it: a list
//! over a list is two things on screen and two escapes back to the file,
//! with nothing saying which of them a key reaches. A key that names another
//! whole-screen view does not do that. It swaps one view for the other, and
//! there is still one thing on screen and one escape back.
//!
//! So `f6` from the files goes to the search, rather than being a key that
//! does nothing until the reader has escaped first. And a key that names a
//! tab of the view already showing walks to that tab instead, with the query
//! kept: `f3` from `f1`'s list is its other tab, not the same list opened
//! again with the words taken away.

use super::*;

impl App {
    /// Whether what the reader is in takes the whole screen.
    ///
    /// The nearest layer only. A list open over the settings is a list, and
    /// a key in it is about the list.
    pub(super) fn in_a_whole_view(&self) -> bool {
        match self.layers().nearest() {
            Some(Layer::Settings | Layer::Counts) => true,
            // A list is a band of the screen by its room, whichever layout it
            // was given, so the layout is what says it is a whole view.
            Some(Layer::Picker) => self
                .picker
                .as_ref()
                .is_some_and(|picker| picker.layout() == PickerLayout::FullArea),
            Some(Layer::Prompt) | None => false,
        }
    }

    /// Goes to the view a command opens, from the one on screen.
    ///
    /// Asked whether it can before anything is closed: a key whose command is
    /// dim does nothing anywhere, and here doing nothing has to include
    /// leaving the reader where they were.
    pub(super) fn switch_view(&mut self, command: Command) {
        if !self.offers(command) {
            return;
        }
        if let (Some(wanted), Some(picker)) = (self.tab_for(command), self.picker.as_ref()) {
            let (now, count) = (picker.tab(), picker.tabs().len());
            self.walk_to_tab(now, wanted, count, |app, onwards| {
                app.picker_key(&stepping(onwards));
            });
            return;
        }
        // Left the way escape leaves each of them, which is what puts back
        // what a view had changed while it was open: a theme it previewed, a
        // question it was asking for an agent, where the file was scrolled
        // to under a preview.
        let showing: Vec<Layer> = self.layers().nearest_first().collect();
        for layer in showing {
            if layer.context() == Context::Dialog {
                self.leave(layer);
            }
        }
        dispatch::dispatch(self, command);
    }

    /// Which tab of the view on screen a command names, if it names one.
    ///
    /// A tab the view has not got is not asked about here: the view leaves
    /// out exactly the tabs whose commands are dim -- the changed files in a
    /// tree with nothing changed, the symbols with no server -- so `offers`
    /// has already said no. And a file's history and a project's are two
    /// views rather than one, so a radius that is not a tab here is the
    /// other view.
    fn tab_for(&self, command: Command) -> Option<usize> {
        let picker = self.picker.as_ref()?;
        if picker.is_listing() {
            let wanted = match command {
                Command::FileOpen => Listing::All,
                Command::FileChanged => Listing::Changed,
                _ => return None,
            };
            return self.listing.iter().position(|shown| *shown == wanted);
        }
        if picker.is_searching() {
            let wanted = match command {
                Command::SearchFile => Scope::File,
                Command::SearchProject => Scope::Project,
                Command::SearchSymbols => Scope::Symbols,
                _ => return None,
            };
            return self.searching.iter().position(|shown| *shown == wanted);
        }
        let wanted = match command {
            Command::HistoryFile => history_view::About::File,
            Command::HistoryProject => history_view::About::Project,
            _ => return None,
        };
        self.history.radii.iter().position(|shown| *shown == wanted)
    }
}
