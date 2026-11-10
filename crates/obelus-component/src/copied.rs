//! What a copy or a cut took, for the reader to be told what they got.

/// What a copy or a cut took.
///
/// A kind rather than the word for it, because the sentence the reader is
/// told is the application's and in the reader's language -- `Copied
/// selection` is one sentence in English and another in Chinese, and a
/// noun handed in to finish it would be English in both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Copied {
    /// What the reader had selected.
    Selection,
    /// A whole line, where nothing was selected -- of the file, or of a
    /// one-line box.
    Line,
    /// A block of code, from a transcript or a page.
    Code,
    /// The whole of a message being written to the agent.
    Message,
    /// The whole of a note.
    Note,
    /// The whole of an answer being written to the agent's question.
    Answer,
    /// What a chat platform makes its app from (Slack's manifest).
    Manifest,
    /// Where a file list's row is, against the project.
    Path,
    /// Where a file list's row is, from the root of the machine.
    FullPath,
}
