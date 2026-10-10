//! What a language server is asked, and what is done with its answers.

use super::*;

pub(super) mod changing;
pub(super) mod completing;
pub(super) mod fixing;
pub(super) mod hierarchy;
pub(super) mod hovering;
pub(super) mod naming;
pub(super) mod renaming;
pub(super) mod renaming_files;
pub(super) mod semantics;

/// What the language server has said about the file being read and the
/// cursor in it, and what is waiting on it: the panels beside the caret, the
/// marks laid over the text, and the questions under way.
#[derive(Debug, Default)]
pub(in crate::app) struct State {
    /// What could be typed next, while a server's answer is on screen.
    ///
    /// Beside the cursor rather than in a region of its own, and its own
    /// field rather than a picker, because the reader is typing into the
    /// document the whole time it is up: it takes six keys and the rest go
    /// where they were going.
    pub(in crate::app) completion: Option<Completion>,
    /// What the call the cursor is inside takes, while it is showing.
    pub(in crate::app) signature: Option<obelus_component::signature::Signature>,
    /// The caret has moved under the panel, and this is the wait for it to
    /// stop -- with the column it is waiting on, so that moving again
    /// starts it again rather than letting a stale one fire.
    pub(in crate::app) signature_pause: Option<(crate::event::Pause, CharColumn)>,
    /// What the server says the place under the caret is, while it is up.
    pub(in crate::app) hover: Option<Hover>,
    /// What will come back for a pointer that has stopped moving.
    pub(in crate::app) hover_pause: Option<crate::event::Pause>,
    /// What the server offered to do here, while a list of it is open.
    ///
    /// Code actions, which are the server's offers to change the file --
    /// not [`App::symbol_actions`], which is the menu of questions about
    /// the name under the caret. Two different things were called actions
    /// here, and this is the half that edits.
    pub(in crate::app) code_actions: Vec<obelus_lsp::actions::Action>,
    /// Every use of the name the pointer is resting on, in this file.
    ///
    /// Marked in the text rather than listed: the answer is "these, here",
    /// and a list would take a region of screen to say what a background
    /// says in place.
    pub(in crate::app) uses: Vec<Span>,
    /// The document being changed, and when it last was.
    ///
    /// The same shape as [`Resting`] and for the same reason: there is a
    /// question worth asking once the reader stops, and none worth asking
    /// while they are still going.
    pub(in crate::app) settling: Option<Settling>,
    /// Where the pointer is resting, since when, and whether that rest
    /// has already asked its question.
    ///
    /// A hover on a rest is the one thing in Obelus that happens because a
    /// reader did *nothing*, so the doing-nothing has to be measured: the
    /// same cell, still under the pointer when the next tick lands. The
    /// asking is remembered because a pointer left on a word that has no
    /// answer must ask about it once rather than twelve times a second.
    pub(in crate::app) resting: Option<Resting>,
    /// The holes left by a snippet, while the reader is filling them in.
    ///
    /// Character offsets into the document, moved by every edit. A snippet
    /// is over once the reader has tabbed past the last of them, which is
    /// what gives `tab` back to indenting.
    pub(in crate::app) filling: Option<obelus_lsp::snippet::Filling>,
    /// What each open file's tokens are, as its server last described them.
    ///
    /// Keyed by path rather than by buffer, because a buffer is a slot that
    /// is reused: a closed file's classification would otherwise answer
    /// about whatever is opened into its place. Each carries the document
    /// version it describes and is ignored once the document has moved past
    /// it, so a stale entry is inert rather than wrong.
    pub(in crate::app) tokens: HashMap<PathBuf, obelus_lsp::tokens::Tokens>,
    /// Where the colours are, per file, as a server last said.
    ///
    /// Beside the tokens because it is the same kind of answer: about a
    /// whole file, kept until the file changes, and thrown away rather
    /// than shown stale.
    pub(in crate::app) colours: HashMap<PathBuf, Vec<obelus_lsp::colour::Coloured>>,
    /// What a server would have the reader know, per file, as it last
    /// said.
    pub(in crate::app) hints: HashMap<PathBuf, Vec<obelus_lsp::hint::Hinted>>,
    /// What is drawn in each file that the file does not contain.
    ///
    /// Both answers in one list, because a cell of a line points at one
    /// entry of it and cannot say which of two lists it meant.
    pub(in crate::app) drawn: HashMap<PathBuf, Vec<obelus_ui::Drawn>>,
    /// Which radii the open list of problems is showing, in tab order.
    ///
    /// The same remembering for the same reason: a tab is only there when
    /// it has something to answer with, so its position is not fixed.
    /// Empty when the list showing is not that one.
    pub(in crate::app) troubling: Vec<semantics::Wrong>,
    /// What is wrong with the line the reader is on, where anything is.
    ///
    /// Worked out every frame from the troubles and the caret rather than
    /// remembered -- the same rule the row that says what is happening
    /// follows, so there is no way for a complaint to be left on a line
    /// that no longer has one.
    pub(in crate::app) complaining: Option<Complaint>,
    /// The tree of calls an open list of them is showing.
    pub(in crate::app) calls: Option<hierarchy::Calls>,
    /// A rename of a file, from the question to the act.
    ///
    /// The gap between the two is a round trip: a server that knows the
    /// language knows which other files name this one by where it is, and
    /// Obelus asks before renaming it rather than leaving the reader to
    /// find out from the next build.
    pub(in crate::app) renaming: Option<renaming_files::Renaming>,
}
