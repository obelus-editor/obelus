//! Who hears a key, nearest the reader first.
//!
//! **What is in front has the key, and nothing behind it is asked.** The
//! layers were walked nearest first and then, whatever they did not take,
//! the document was asked as well -- the file, the conversation, the notes,
//! and what floats over the file -- each of which had to know for itself
//! that something was in front of it. The file did and the others did not:
//! `f1` over the notes and then `alt+down` moved the note behind the list,
//! and `alt+delete` took it away. So nothing asks that any more. Each thing
//! that can hear a key says what happens to a key it does not want, and
//! there are only two answers: the next thing down has it, or nobody but
//! the key table does.

use super::*;

/// One thing on screen that a key can go to.
///
/// No `match` on this may carry a `_` arm, for the reason [`Layer`] may not:
/// a new one has to say where what it does not take goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hearer {
    /// Something the reader is in, over the document.
    Layer(Layer),
    /// The page asking which project, which is what there is instead of a
    /// document.
    Chooser,
    /// A conversation, being read.
    Chat,
    /// The notes, being read.
    Notes,
    /// A terminal, being read: its program's, while it runs.
    Terminal,
    /// What the server said about a place.
    Hover,
    /// What the call the cursor is inside takes.
    Signature,
    /// What could be typed next.
    Completion,
    /// The holes a snippet left.
    Snippet,
    /// The file being read, or nothing being read at all.
    Editor,
}

impl Hearer {
    /// Whether what this one does not take goes on to the one behind it.
    ///
    /// Only what floats over the file: the reader is typing into the file
    /// under it, and a key it has no use for is a key for the file. What the
    /// reader is *in* keeps what it does not take, and only the key table
    /// hears it after that. Which is the only way anything behind gets a
    /// key, and how `ctrl+q` still leaves Obelus from inside one.
    const fn lets_through(self) -> bool {
        match self {
            Self::Hover | Self::Signature | Self::Completion | Self::Snippet => true,
            Self::Layer(_)
            | Self::Chooser
            | Self::Chat
            | Self::Notes
            | Self::Terminal
            | Self::Editor => false,
        }
    }
}

impl App {
    /// Everything that could hear a key, nearest the reader first.
    ///
    /// The layers in their own order, then what is being read -- one
    /// document, and over a file the things that float over it, each of
    /// which says itself whether it is up.
    fn hearers(&self) -> Vec<Hearer> {
        let mut hearers: Vec<Hearer> = self.layers().nearest_first().map(Hearer::Layer).collect();
        if self.which_project.chooser.is_some() {
            hearers.push(Hearer::Chooser);
        } else if self.conversation().is_some() {
            hearers.push(Hearer::Chat);
        } else if self.notes().is_some() {
            hearers.push(Hearer::Notes);
        } else if self.terminal().is_some() {
            hearers.push(Hearer::Terminal);
        } else {
            // Escape belongs to whichever is nearest, and the hover and the
            // call are never up together. The panel takes the arrows and
            // `enter` before the snippet's `tab` and the file's motions,
            // and the file is under all of them.
            hearers.extend([
                Hearer::Hover,
                Hearer::Signature,
                Hearer::Completion,
                Hearer::Snippet,
                Hearer::Editor,
            ]);
        }
        hearers
    }

    /// Gives a key to whatever is in front, and on down only as far as each
    /// one lets it through. Answers whether anything took it.
    pub(super) fn hand_over(&mut self, key: &KeyEvent) -> bool {
        for hearer in self.hearers() {
            if self.hear(hearer, key) {
                return true;
            }
            if !hearer.lets_through() {
                return false;
            }
        }
        false
    }

    /// Offers a key to one of them.
    fn hear(&mut self, hearer: Hearer, key: &KeyEvent) -> bool {
        match hearer {
            Hearer::Layer(Layer::Prompt) => self.prompt_key(key),
            Hearer::Layer(Layer::Names) => self.names_key(key),
            // The paging keys first, while a preview is on screen: a
            // screenful is what the thing being *read* is moved by, and the
            // list above it is ten rows with its ends a keypress away. With
            // control they page the list, which is the other half of the
            // same swap. Then the keys a file list and a search have that
            // are not about moving around them, which the picker would not
            // know: what they change is where the rows come from, and that
            // is the application's.
            Hearer::Layer(Layer::Picker) => {
                self.page_preview(key)
                    || self.background_key(key)
                    || self.listing_key(key)
                    || self.searching_key(key)
                    || self.picker_key(key)
            }
            Hearer::Layer(Layer::Settings) => self.settings_key(key),
            Hearer::Layer(Layer::Counts) => self.counts_key(key),
            Hearer::Layer(Layer::Gone) => self.the_page_saying_it_has_gone(key),
            Hearer::Chooser => self.choosing_a_project(key),
            Hearer::Chat => self.chat_key(key),
            Hearer::Notes => self.notes_key(key),
            Hearer::Terminal => self.terminal_key(key),
            Hearer::Hover => self.hover_key(key),
            Hearer::Signature => self.signature_key(key),
            Hearer::Completion => self.completion_key(key),
            Hearer::Snippet => self.snippet_key(key),
            Hearer::Editor => self.editor_key(key),
        }
    }
}
