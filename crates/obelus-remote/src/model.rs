//! What passes between Obelus and a chat platform: words, and where they go.
//!
//! **Words are the floor.** Nothing here is a button, a card or a form: a
//! platform is told to say some text somewhere and tells Obelus what somebody
//! said and where. Which is everything every chat can do, and so the whole of
//! what a new platform has to be taught.

/// Where in a direct message something is said.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Where {
    /// The conversation itself, outside any thread: where the agent that
    /// finds notes and starts the rest is talked to.
    Top,
    /// A thread, by the platform's own name for it -- which nothing here
    /// reads, only keeps and hands back.
    Thread(String),
}

/// Something Obelus asks a platform to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Out {
    /// Say something.
    Say {
        /// Whose direct message, by the platform's id for them.
        to: String,
        /// Where in it.
        at: Where,
        /// What, in a little markdown: emphasis, code, a code block, a link.
        text: String,
        /// Whether to call them by name, which is what makes a phone ring:
        /// a question waiting on them, or a turn that has ended.
        notify: bool,
    },
    /// Start a thread with this as its first message, and say what it is
    /// called once it is there -- see `Event::Opened`.
    Open {
        /// Obelus's own number for the asking, handed back with the answer.
        asked: u64,
        /// Whose direct message.
        to: String,
        /// The first message, which names the conversation the thread is.
        text: String,
    },
    /// Find out what somebody is called, for the list of who may talk.
    Name {
        /// Their id.
        id: String,
    },
}
