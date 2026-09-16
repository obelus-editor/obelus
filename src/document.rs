//! What the reader can switch between.
//!
//! A file and a conversation with an agent are not the same kind of thing.
//! One has bytes, a syntax tree, a version a language server counts, and
//! every conversion between the coordinate spaces; the other has a
//! transcript that grows at the end and a box under it. `AGENTS.md` puts the
//! rule plainly -- *"reuse stops where the subject differs... share the
//! mechanism, not the meaning"* -- and [`crate::component::chat`] opens by
//! saying it is not a buffer.
//!
//! So this is where the difference is written down, and what they share is
//! only what they really share: a place in the list, an id that names that
//! place, and the fact that escape from anything over them comes back
//! *here*.
//!
//! One variant so far, which is on purpose: the list, the id and the
//! switching move to this shape first, and what goes in beside a file
//! arrives afterwards. If moving them churns a test, the shape was wrong
//! and better to know before there is a second kind of thing riding on it.

use crate::{buffer::Buffer, conversation::Conversation};

/// One of the things the reader can be looking at.
#[derive(Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "a buffer is what a slot in this list usually holds, so it is \
              the one that should not be behind a pointer; the conversation \
              is boxed because it was twice the size and there are a handful \
              of them against a reader's whole tree of files"
)]
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
            Self::Chat(_) => None,
        }
    }

    /// And to change it.
    #[must_use]
    pub const fn file_mut(&mut self) -> Option<&mut Buffer> {
        match self {
            Self::File(buffer) => Some(buffer),
            Self::Chat(_) => None,
        }
    }

    /// The conversation, where this is one.
    #[must_use]
    pub fn chat(&self) -> Option<&Conversation> {
        match self {
            Self::Chat(talk) => Some(talk),
            Self::File(_) => None,
        }
    }

    /// And to change it.
    pub fn chat_mut(&mut self) -> Option<&mut Conversation> {
        match self {
            Self::Chat(talk) => Some(talk),
            Self::File(_) => None,
        }
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
