//! What the reader can switch between.
//!
//! A file and a conversation with an agent are not the same kind of thing.
//! One has bytes, a syntax tree, a version a language server counts, and
//! every conversion between the coordinate spaces; the other has a
//! transcript that grows at the end and a box under it. `AGENTS.md` puts the
//! rule plainly -- *"reuse stops where the subject differs... share the
//! mechanism, not the meaning"* -- and [`obelus_component::chat`] opens by
//! saying it is not a buffer.
//!
//! So this is where the difference is written down, and what they share is
//! only what they really share: a place in the list, an id that names that
//! place, and the fact that escape from anything over them comes back
//! *here*.
//!
//! It arrived with one variant on purpose: the list, the id and the
//! switching moved to this shape before anything went in beside a file, so
//! that a churned test would have said the shape was wrong while there was
//! still nothing riding on it. Nothing churned, and the conversation went
//! in.

use obelus_buffer::Buffer;
use obelus_component::todo::TodoView;

use crate::conversation::Conversation;

/// One of the things the reader can be looking at.
/// A buffer is what a slot in this list usually holds, so it is the one
/// that should not be behind a pointer. The conversation is boxed because
/// it was twice the size, and there are a handful of them against a
/// reader's whole project of files. `large_enum_variant` had to be silenced
/// for that until the notes went in beside them: three variants of 440,
/// 8 and 240 bytes is a spread clippy does not mind.
#[derive(Debug)]
pub enum Document {
    /// A file, or a file as a commit had it.
    File(Buffer),
    /// A conversation with an agent.
    ///
    /// Not a view over the file behind it, which is what it was: a
    /// conversation is somewhere the reader goes and comes back to, and
    /// something they go back to is a document. Which is also what stops
    /// escape meaning two things -- it leaves whatever is *over* the
    /// document being read, and a conversation is no longer over anything.
    ///
    /// Boxed, because a conversation is twice the size of a buffer and
    /// every slot in the list would be that big: a reader with forty files
    /// open would pay for forty conversations they have not had.
    Chat(Box<Conversation>),
    /// What this project means to come back to.
    ///
    /// A page over the editor until now, which made escape mean two things:
    /// everywhere else it leaves whatever is *over* what is being read, and
    /// there it closed the thing itself. It is somewhere the reader goes
    /// and comes back to, which is what a document is.
    ///
    /// Not boxed. A conversation is twice a buffer and pays for a pointer;
    /// this is 240 bytes against a buffer's 440, so it rides in the space
    /// the list already spends.
    Notes(TodoView),
    /// A program the reader can type to: their shell, or an agent's own
    /// sign-in.
    ///
    /// A document for the reason a conversation is one -- somewhere the
    /// reader goes and comes back to -- and not a file for the reason a
    /// conversation is not one: what is on it is the program's, drawn by it
    /// a screen at a time, and nothing in it is the reader's to edit.
    ///
    /// Boxed: a parser and its screen, against the buffer every other slot
    /// is sized to.
    Terminal(Box<obelus_terminal::Terminal>),
}

impl Document {
    /// The file, where this is one.
    ///
    /// `Option` rather than a panic, and it is the whole reason this type
    /// earns its place: almost everything that reaches a document wants a
    /// file -- the motions, the syntax, the language server, the margin --
    /// and every one of those already had somewhere to go when there is no
    /// file open at all. Answering `None` puts a conversation down the path
    /// that was written for an empty screen, rather than down the path
    /// written for a file, holding something that is not one.
    #[must_use]
    pub const fn file(&self) -> Option<&Buffer> {
        match self {
            Self::File(buffer) => Some(buffer),
            Self::Chat(_) | Self::Notes(_) | Self::Terminal(_) => None,
        }
    }

    /// And to change it.
    #[must_use]
    pub const fn file_mut(&mut self) -> Option<&mut Buffer> {
        match self {
            Self::File(buffer) => Some(buffer),
            Self::Chat(_) | Self::Notes(_) | Self::Terminal(_) => None,
        }
    }

    /// The conversation, where this is one.
    #[must_use]
    pub fn chat(&self) -> Option<&Conversation> {
        match self {
            Self::Chat(talk) => Some(talk),
            Self::File(_) | Self::Notes(_) | Self::Terminal(_) => None,
        }
    }

    /// And to change it.
    pub fn chat_mut(&mut self) -> Option<&mut Conversation> {
        match self {
            Self::Chat(talk) => Some(talk),
            Self::File(_) | Self::Notes(_) | Self::Terminal(_) => None,
        }
    }

    /// The notes, where this is them.
    #[must_use]
    pub const fn notes(&self) -> Option<&TodoView> {
        match self {
            Self::Notes(notes) => Some(notes),
            Self::File(_) | Self::Chat(_) | Self::Terminal(_) => None,
        }
    }

    /// And to change them.
    #[must_use]
    pub const fn notes_mut(&mut self) -> Option<&mut TodoView> {
        match self {
            Self::Notes(notes) => Some(notes),
            Self::File(_) | Self::Chat(_) | Self::Terminal(_) => None,
        }
    }
}

impl Document {
    /// The terminal, where this is one.
    #[must_use]
    pub fn terminal(&self) -> Option<&obelus_terminal::Terminal> {
        match self {
            Self::Terminal(terminal) => Some(terminal),
            Self::File(_) | Self::Chat(_) | Self::Notes(_) => None,
        }
    }

    /// And to type to it.
    pub fn terminal_mut(&mut self) -> Option<&mut obelus_terminal::Terminal> {
        match self {
            Self::Terminal(terminal) => Some(terminal),
            Self::File(_) | Self::Chat(_) | Self::Notes(_) => None,
        }
    }
}

impl From<obelus_terminal::Terminal> for Document {
    fn from(terminal: obelus_terminal::Terminal) -> Self {
        Self::Terminal(Box::new(terminal))
    }
}

impl From<TodoView> for Document {
    fn from(notes: TodoView) -> Self {
        Self::Notes(notes)
    }
}

impl From<Conversation> for Document {
    fn from(talk: Conversation) -> Self {
        Self::Chat(Box::new(talk))
    }
}

impl From<Buffer> for Document {
    fn from(buffer: Buffer) -> Self {
        Self::File(buffer)
    }
}
