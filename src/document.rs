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

use crate::buffer::Buffer;

/// One of the things the reader can be looking at.
#[derive(Debug)]
pub enum Document {
    /// A file, or a file as a commit had it.
    File(Buffer),
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
        }
    }

    /// And to change it.
    #[must_use]
    pub const fn file_mut(&mut self) -> Option<&mut Buffer> {
        match self {
            Self::File(buffer) => Some(buffer),
        }
    }
}

impl From<Buffer> for Document {
    fn from(buffer: Buffer) -> Self {
        Self::File(buffer)
    }
}
