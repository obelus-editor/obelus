//! One conversation with an agent: what was said, and what it is waiting on.
//!
//! Five things that were five fields on the application, and are one thing:
//! the transcript and the box, the agent's own commands while a list of them
//! is following what is being typed, the form it asked the reader to fill
//! in, the card that form is answered on, and the channel an answer goes
//! back through.
//!
//! They were five because there was one conversation. The moment there are
//! two, four of them are wrong as singletons and one is wrong in a way that
//! loses work: a form the agent is waiting on arrives with a live channel at
//! the other end, and a second conversation putting its own there would
//! leave the first agent waiting for an answer nobody can give it any more.
//!
//! Nothing here talks to an agent. What arrives from one is folded in by
//! [`crate::app`], which is the half that knows there is a process.

use obelus_agent::acp;
use obelus_component::{card::Card, chat::Chat, picker::Picker};
use obelus_git::todo::NoteId;

/// What a conversation is about.
///
/// A note, mostly: a note is something the reader wrote down to come back
/// to, and a conversation is something they come back to, so the two are
/// the same shape and pairing them costs nothing. The other kind is the one
/// started with the key, about nothing in particular.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Topic {
    /// Nothing in particular: opened with the key rather than from a note.
    #[default]
    Loose,
    /// One of the tree's notes, by the name that outlives its position.
    Note(NoteId),
}

/// A conversation with an agent, whether or not it is on screen.
///
/// Kept rather than opened: a reader moves between what is open all day, and
/// a conversation that started again every time they went to a file would be
/// a conversation nobody could leave for a minute.
#[derive(Debug, Default)]
pub struct Conversation {
    /// What it is about.
    pub topic: Topic,
    /// Which conversation on the agent this is, once it has opened one.
    ///
    /// `None` until then, which is a real state and not a gap: opening the
    /// view starts the process, and a reader types faster than node starts.
    pub session: Option<acp::SessionId>,
    /// The one it asked the agent to take up again, while it waits.
    ///
    /// Only for a conversation being reopened, where obelus already knows
    /// the name because it wrote it down. It is not put straight into
    /// `session`, because until the agent has answered the conversation is
    /// still starting and the screen should say so -- and it is kept apart
    /// from "no session yet" because two conversations opening at once would
    /// otherwise be told apart by nothing, and could take each other's.
    pub asked_for: Option<acp::SessionId>,
    /// What the note said when obelus last told the agent about it.
    ///
    /// What the agent has been *told*, rather than what obelus has to say:
    /// the second is worked out from the first every time the reader sends
    /// something, by asking what the note says now. So there is one rule --
    /// tell it what it does not know -- and no flag anybody has to clear.
    ///
    /// `None` for a conversation the agent has been told nothing about,
    /// which is a fresh one, and for one about nothing in particular. Read
    /// from beside the note for a conversation being picked up where it was
    /// left: the agent kept every word of that one, and what told it the
    /// first time is among them.
    pub told: Option<String>,
    /// Whether the agent has been told who it is talking to.
    ///
    /// A bit rather than a fingerprint, which is the difference between
    /// this and [`Self::told`]: what it carries never changes, so there is
    /// nothing to compare it against and no such thing as saying it again
    /// because it is out of date. Said once per conversation, and written
    /// down beside the note like `told` so that picking one up again does
    /// not repeat it.
    pub introduced: bool,
    /// What the reader said while the agent was working, in the order they
    /// said it.
    ///
    /// The protocol takes one prompt turn at a time -- `session/cancel`
    /// names a session and not a turn, and the answer to `session/prompt`
    /// says "the turn is over" with nothing on it saying which turn -- so a
    /// second prompt sent into a running one leaves obelus unable to tell
    /// the two apart. It could not: the first answer home put the
    /// conversation back to resting while the other turn worked on,
    /// unmarked, with no spinner and no key that would stop it.
    ///
    /// So what the reader types goes here instead, and leaves when the turn
    /// ends. Kept on the conversation rather than beside the connection,
    /// because it is one reader's words to one agent and because it has to
    /// be drawn: what is held back is shown over the box.
    pub waiting: std::collections::VecDeque<String>,
    /// Whether what is waiting stays waiting.
    ///
    /// Set when the reader stops the turn themselves. Sending their words
    /// the moment the thing they just stopped comes to a halt is obelus
    /// speaking for them straight after they said not to; the next thing
    /// they send starts it moving again.
    pub held_back: bool,
    /// What was said, and what is being typed.
    pub chat: Chat,
    /// The agent's own commands, while one is being typed in the box.
    ///
    /// Its own list rather than the application's picker, because it does
    /// not take the keys: it follows what is being typed and the box keeps
    /// them.
    pub slash: Option<Picker>,
    /// The form the agent asked the reader to fill in, while one is open.
    pub asking: Option<Asking>,
    /// The card whatever the agent asked is answered on.
    ///
    /// One field for both kinds of question it can ask -- a form's field and
    /// a request for permission -- because on screen they are the same
    /// thing: what it wants to know, what the answers are, and room to say
    /// one in your own words where it will take those.
    pub card: Option<Card>,
    /// The permission request waiting on the reader: the channel its answer
    /// goes back through.
    pub permission: Option<acp::Answer<Option<String>>>,
    /// Somewhere the agent wants the reader to go, while they have not
    /// said whether they will.
    pub going: Option<Going>,
}

/// A place on the web the agent wants the reader to go: to sign in
/// somewhere, to authorise something.
///
/// Not a form, and not a question in the sense the card usually puts:
/// there is nothing to fill in, and the answer is "I went" or "I will
/// not". Held apart from [`Asking`] for that reason -- one field would be
/// two different things with a `match` on which, and every reader of it
/// would have to do the matching.
#[derive(Debug)]
pub struct Going {
    /// What the agent said it is for, in its own words.
    pub message: String,
    /// Where. Checked before it reached here: `http` or `https`, with a
    /// host, and nothing a launcher would read as two arguments.
    pub url: String,
    /// The agent's own name for the question, which is how it later says
    /// the far end happened.
    pub id: String,
    /// Where the answer goes. `true` once the reader has been sent, which
    /// is what the agent asked for -- it watches the far end itself.
    pub answer: acp::Answer<bool>,
}

/// A form an agent asked the reader to fill in.
///
/// One field at a time, in the order the agent listed them: a list where the
/// answer is one of a few, the box where it is words. What has been answered
/// is kept here until the last field is, because the protocol takes the
/// whole form as one answer.
#[derive(Debug)]
pub struct Asking {
    /// What the agent said the form is about, in its own words.
    ///
    /// Kept because it belongs above whichever question is showing rather
    /// than in a line of its own: a form is one question with several parts,
    /// and saying what it is about twice is saying it once too often.
    pub message: String,
    /// The fields nobody has answered yet, the next one first.
    pub left: std::collections::VecDeque<acp::Field>,
    /// What has been answered, in the order it was.
    pub given: Vec<(String, acp::Reply)>,
    /// Where the answers go when the last one is in.
    pub answer: acp::Answer<Option<Vec<(String, acp::Reply)>>>,
}
