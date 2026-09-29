//! What Obelus says about the key just pressed.
//!
//! Half of what a language server does is answer with nothing, and nothing
//! is invisible: without somewhere to say "no definition found" or "still
//! indexing", pressing the key looks like the key not working. That is what
//! a note is for, and it is why there are two kinds of one.
//!
//! **A note says which kind it is, and it says so by which door it came
//! through.** `Saved` and `Not saved` are the same words in the same corner
//! of the same row, and until the ink told them apart a reader glancing at
//! the row read them the same. A flag set beside the words would have been
//! a second thing to remember at a hundred and thirty call sites, and the
//! one that was forgotten would be a refusal drawn as a report -- so the
//! kind is not something a caller may leave out: [`Note`]'s fields are this
//! module's, and the only way to a value is [`Note::said`] or
//! [`Note::wrong`].
//!
//! Two kinds and not three. A server's severities are three deep because a
//! server is telling the reader about their code; this is Obelus telling
//! them what just happened at the key they pressed, and there are two
//! things that can be: it happened, or it would not. What is *happening* --
//! `Renaming to X\u{2026}` -- is the first of those, because the key worked.

/// Something Obelus has to say, and which kind of thing it is.
///
/// A field beside the words rather than two variants holding them, because
/// everything that reads a note wants the words and only the drawing wants
/// the kind: a `match` at every reader to get at the words would put a
/// question nobody asked in front of the one thing they all want.
#[derive(Debug)]
pub(super) struct Note {
    /// The words.
    said: String,
    /// Whether they are about something that would not go.
    wrong: bool,
}

impl Note {
    /// What happened, or what is happening.
    pub(super) fn said(said: impl Into<String>) -> Self {
        Self {
            said: said.into(),
            wrong: false,
        }
    }

    /// What would not go.
    pub(super) fn wrong(said: impl Into<String>) -> Self {
        Self {
            said: said.into(),
            wrong: true,
        }
    }

    /// The words, which is what every reader but the drawing wants.
    pub(super) fn words(&self) -> &str {
        &self.said
    }

    /// And whether they are about something that would not go.
    pub(super) const fn is_wrong(&self) -> bool {
        self.wrong
    }
}
