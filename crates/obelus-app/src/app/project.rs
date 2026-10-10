//! The project and what is kept about it: which one, what was open, the
//! settings, the notes, and a newer Obelus.

use super::*;

pub(super) mod noting;
pub(super) mod preferences;
pub(super) mod projects;
pub(super) mod releases;
pub(super) mod reopening;

/// The page that asks which project, while it is up: the two boxes, the list of
/// names that could finish what is typed in the second, and what has been read
/// for it.
#[derive(Debug, Default)]
pub(in crate::app) struct Asking {
    /// The question "which project", while nobody has answered it.
    ///
    /// `Some` on a start with nothing to go on: no argument, and a
    /// directory git has never heard of -- a desktop launcher, which
    /// begins the process in the home directory. And once more after the
    /// project has gone and the reader has said so, which leaves the window
    /// where such a start began. It is the whole screen until it is
    /// answered, and `None` otherwise: a reader on a project does not go
    /// back to being asked, and the way to another one is a second Obelus,
    /// which is how Obelus is used anyway.
    pub(in crate::app) chooser: Option<obelus_component::chooser::Chooser>,
    /// What could finish the path being named, while one is.
    ///
    /// Here rather than inside the chooser for the reason the agent's own
    /// commands are here: a list is built out of what the application
    /// knows -- a directory it read -- and `obelus-component` draws and
    /// walks lists rather than looking at disks.
    pub(in crate::app) naming_list: Option<Picker>,
    /// What the last directory read found, and which directory that was.
    ///
    /// Kept apart from the list above, because the list *goes* for two
    /// ordinary reasons -- the reader shut it, or what they have typed
    /// since matches none of it -- and both of those have to be
    /// undoable by typing another letter. Held together they were not:
    /// once the list was gone there was nothing left to make it from, so
    /// escape shut it for good and a letter too many could not be rubbed
    /// out. The disk is read when the *directory* moves and never again.
    pub(in crate::app) naming_read: Option<(PathBuf, Vec<obelus_component::picker::PickerItem>)>,
    /// Whether the reader shut the list on what is in the box now.
    ///
    /// Cleared the moment the box moves, which is the rule
    /// `component::completion` follows: escape takes the panel away, and
    /// typing is a new question rather than the same one asked twice.
    pub(in crate::app) naming_shut: bool,
    /// Whether what is in the path box names something that is there.
    ///
    /// Kept rather than asked for: the row is drawn on every frame and
    /// the answer moves only when the box does, so it is worked out on
    /// the key that moved it. What it is for is the ink -- a reader has
    /// to see that enter will refuse before they press it, which is the
    /// rule the palette follows for a command it will not run.
    pub(in crate::app) named_is_there: bool,
}

/// The notes, as last read -- every window and an agent's tools write them --
/// and the wait for a burst of writes to settle before they are read again.
#[derive(Debug, Default)]
pub(in crate::app) struct Notes {
    /// The project's notes, as Obelus last read them.
    ///
    /// Not the page's copy, which is the reader's and is ahead of the file
    /// while they are typing in it. This one is the file, for the two
    /// things outside that page which have to know what a note says: the
    /// box of the conversation about it, which offers to ask about the
    /// note again once it has been rewritten, and the conversation's
    /// header, which goes by the note until the agent has named it.
    pub(in crate::app) notes_kept: Option<obelus_todo::Todo>,
    /// What will come back for the notes, to write down what was typed.
    ///
    /// Structural changes -- a note added, finished, moved -- are written
    /// the moment they happen and never wait: they are one act each, and
    /// there is nothing to wait for. Typing is not one act, and it used to
    /// be written when the reader left the page. There is no leaving a
    /// document, so a pause is the moment instead.
    pub(in crate::app) notes_pause: Option<crate::event::Pause>,
}
