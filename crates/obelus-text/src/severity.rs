//! How bad something said about a piece of a file is.
//!
//! Here rather than with the language server because a server is not the
//! only thing that says it -- Obelus marks the files it cannot read with the
//! same four words -- and the buffer that keeps the marks has no business
//! carrying a protocol client to name them with. The same reason as
//! [`crate::marker`].

use crate::kind::SyntaxKind;

/// How bad a server says something is.
///
/// Four, because the protocol has four and a reader can tell them apart:
/// what stops the build, what is worth reading, and two kinds of remark.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Something that will not compile.
    Error,
    /// Something that will, and should not.
    Warning,
    /// A remark.
    Information,
    /// A suggestion, usually about style.
    Hint,
}

impl Severity {
    /// The colour it is drawn in.
    ///
    /// The file's own colours, as everything in Obelus is: an error is
    /// what an error in a log is, a warning what a warning is. The two
    /// quieter ones take the colour of a comment, which is what they read
    /// as -- something written beside the code rather than about it.
    #[must_use]
    pub const fn kind(self) -> SyntaxKind {
        match self {
            Self::Error => SyntaxKind::Error,
            Self::Warning => SyntaxKind::Warning,
            Self::Information | Self::Hint => SyntaxKind::Comment,
        }
    }

    /// The mark that stands for it where there is no room for a word.
    ///
    /// Ordinary Unicode rather than a Nerd Font glyph: this goes on the
    /// status row, which is on screen the whole time, so it cannot depend
    /// on a font Obelus has not been told about.
    #[must_use]
    pub const fn mark(self) -> char {
        match self {
            Self::Error => '\u{00d7}',
            Self::Warning => '\u{0021}',
            Self::Information | Self::Hint => '\u{00b7}',
        }
    }

    /// What to call it.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Error => "Error",
            Self::Warning => "Warning",
            Self::Information => "Information",
            Self::Hint => "Hint",
        }
    }
}
