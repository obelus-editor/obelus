//! What happened to a line.
//!
//! Here rather than with the reading of a repository because the repository
//! is not the only thing that answers the question and it is certainly not
//! the thing that draws the answer. A marker is three words the margin, the
//! colours and a hunk in a conversation all have to agree on, and putting
//! them where the answer comes from would mean the colours could not name a
//! marker without carrying a git implementation to name it with.

/// What a line's marker means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    /// The line is new since the last commit.
    Added,
    /// The line replaced something.
    Modified,
    /// Lines were removed from in front of this one.
    ///
    /// The only one that is about a *boundary* rather than about the line
    /// itself, which is why it is drawn differently: a full-height bar would
    /// claim the line changed, and it did not.
    Removed,
}
