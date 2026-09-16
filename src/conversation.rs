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

use crate::{
    acp,
    component::{card::Card, chat::Chat, picker::Picker},
};

/// A conversation with an agent, whether or not it is on screen.
///
/// Kept rather than opened: the view is a region the reader shows and hides,
/// and a conversation that started again every time it was closed would be a
/// conversation nobody could leave for a minute.
#[derive(Debug, Default)]
pub struct Conversation {
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
