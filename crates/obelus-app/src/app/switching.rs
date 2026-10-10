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
//!
//! Which keys those are is `Command::takes_a_view_s_place`: every key that
//! opens a view, and every one that opens a list over the file -- the
//! palette, the menu about the name under the caret, the problems -- because
//! a reader inside a view who wants one should not have to leave it to ask.
//! And a list over the
//! file rather than instead of it -- the palette, a menu, the conversations
//! -- gives way to them the same: it once kept its keys, as somewhere the
//! reader was choosing, and the cost was that `f1` did nothing in the very
//! palette `open-file` is a row of, and `f4`'s list was the one view `f1`
//! could not be reached from. A key naming a view is the reader choosing
//! that instead. What does not give way is what is waiting on the reader --
//! a question, and what went wrong on the way up -- because leaving one by
//! another key is an answer nobody gave (`Picker::gives_way`); nor what is
//! being typed into, a box or a list of names, which a key that goes
//! elsewhere would throw away.
//!
//! **A view that has bound the key itself beats the swap.** `App::handle_key`
//! asked the swap first once, so a key the showing view had taken was
//! answered one level out and its own binding was dead: `f4`, while it
//! opened a conversation from a file and meant *which one* inside one,
//! swapped the conversation for itself. `Keymap::bound_here` is that
//! question -- what this context binds, with no falling back -- and it is
//! the same precedence `Keymap::lookup` already uses. `f4` is the list
//! everywhere now, but the precedence stays for the next view that takes a
//! key of its own.

use super::*;

impl App {
    /// Whether what the reader is in gives way to a key that opens a view.
    ///
    /// The nearest layer only. A list open over the settings is a list, and
    /// a key in it is about the list -- which gives way, and takes the
    /// settings with it.
    pub(super) fn gives_way_to_a_view(&self) -> bool {
        match self.layers().nearest() {
            Some(Layer::Settings | Layer::Counts) => true,
            // Whichever layout it was given: a band over the file is as much
            // somewhere the reader chose to be as a list instead of it.
            Some(Layer::Picker) => self.picker.as_ref().is_some_and(Picker::gives_way),
            // Something being typed, which a key that went elsewhere would
            // throw away. And nothing at all, which is the file's own table.
            Some(Layer::Names) | Some(Layer::Prompt) | None => false,
            // Never asked: the page saying the project has gone takes every
            // key before a swap is looked for, and swaps with nothing.
            Some(Layer::Gone) => false,
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
        // A list that turns out to have nothing in it is a sentence on the
        // status row instead, and the reader is still where they were, with
        // what they had typed: closing the view first answered "nothing
        // wrong with this file" by throwing their search away.
        if !self.would_list(command) {
            dispatch::dispatch(self, command);
            return;
        }
        self.put_away_the_views();
        dispatch::dispatch(self, command);
    }

    /// Whether a command would put something on screen, asked before a view
    /// is put away for it.
    ///
    /// Every view does, and most lists: the palette, the themes, the
    /// conversations are never empty. Three lists answer some questions with
    /// a sentence instead, and those are asked first.
    fn would_list(&mut self, command: Command) -> bool {
        match command {
            Command::SymbolTroubles => self.anything_wrong(),
            Command::SymbolMenu => {
                // What `open_symbol_menu` settles before asking, so the two
                // are asked about the same tree.
                self.settle_syntax();
                self.symbol_actions().is_ok()
            }
            // Asked of a server, and only its answer says. The answer puts
            // the view away itself when it comes back with a list
            // (`on_code_actions`).
            Command::CodeActions => false,
            _ => true,
        }
    }

    /// Leaves what is showing the way escape leaves each of it, which is
    /// what puts back what a view had changed while it was open: a theme it
    /// previewed, a question it was asking for an agent, where the file was
    /// scrolled to under a preview.
    pub(super) fn put_away_the_views(&mut self) {
        let showing: Vec<Layer> = self.layers().nearest_first().collect();
        for layer in showing {
            if layer.context() == Context::Dialog {
                self.leave(layer);
            }
        }
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
        // The key that opened the list showing is the list the reader is
        // already in: they stay where they are, with what they typed, rather
        // than being given the same list again empty.
        if let Some(opener) = picker.opener() {
            return (command == opener).then(|| picker.tab());
        }
        if !self.worktrees.tabs.is_empty() {
            return self.switching_tab_for(command);
        }
        if picker.is_listing() {
            let wanted = match command {
                Command::FileOpen => Listing::All,
                Command::FileChanged => Listing::Changed,
                _ => return None,
            };
            return self.files.listing.iter().position(|shown| *shown == wanted);
        }
        if picker.is_searching() {
            let wanted = match command {
                Command::SearchFile => Scope::File,
                Command::SearchProject => Scope::Project,
                Command::SearchSymbols => Scope::Symbols,
                _ => return None,
            };
            return self
                .search
                .searching
                .iter()
                .position(|shown| *shown == wanted);
        }
        let wanted = match command {
            Command::HistoryFile => history_view::About::File,
            Command::HistoryProject => history_view::About::Project,
            _ => return None,
        };
        self.history.radii.iter().position(|shown| *shown == wanted)
    }
}
