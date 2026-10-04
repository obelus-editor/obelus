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
    /// A thread somebody has just started themselves, in the room the
    /// conversations are in, by the platform's name for it: a conversation
    /// that has not begun yet. Only heard, never said to.
    Fresh(String),
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
    /// Start a thread with this head as its first message, and say what it
    /// is called once it is there -- see `Event::Opened`.
    Open {
        /// Obelus's own number for the asking, handed back with the answer.
        asked: u64,
        /// Whose direct message.
        to: String,
        /// What the thread is, which is its first message.
        head: Head,
    },
    /// Say what a thread is again, because something about it moved: its
    /// name, its branch, where its turn has got to.
    Retitle {
        /// Whose direct message.
        to: String,
        /// Which thread, as the platform named it.
        thread: String,
        /// What it is now.
        head: Head,
    },
    /// Where the threads with somebody go: the room kept for them, or one
    /// to be made -- see `Event::Roomed`. Said once a connection, before
    /// any thread is asked for, so that every thread goes there.
    ///
    /// A platform with no room of its own to offer keeps the threads in the
    /// direct message and answers nothing.
    Room {
        /// Whose.
        to: String,
        /// The room kept for them, where there is one.
        room: Option<String>,
    },
    /// Find out what somebody is called, for the list of who may talk.
    Name {
        /// Their id.
        id: String,
    },
}

/// What a thread is: the first message of it, said again whenever any of
/// this moves.
///
/// Its own type rather than a line of text, because where a platform can
/// draw a card it draws one -- the state as the card's colour, the name as
/// its title -- and where it cannot, [`Head::in_words`] is the line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Head {
    /// What the conversation is called.
    pub title: String,
    /// Where it is: the project, and the branch where the agent is working
    /// on one.
    pub place: String,
    /// Where its turn has got to, once it has had one.
    pub state: Option<Turning>,
}

/// Where a conversation's turn has got to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Turning {
    /// The agent is working.
    Working,
    /// The agent is waiting on the reader.
    Waiting,
    /// The turn is over.
    Done,
    /// The conversation was closed.
    Closed,
}

impl Turning {
    /// The mark in front of the title.
    #[must_use]
    pub const fn mark(self) -> &'static str {
        match self {
            Self::Working => "\u{23f3}",
            Self::Waiting => "\u{2753}",
            Self::Done => "\u{2705}",
            Self::Closed => "\u{23f9}",
        }
    }
}

impl Head {
    /// The title with its mark, where it has one.
    #[must_use]
    pub fn titled(&self) -> String {
        match self.state {
            Some(state) => format!("{} {}", state.mark(), self.title),
            None => self.title.clone(),
        }
    }

    /// The head as a line of markdown, for a platform that can only edit
    /// text.
    #[must_use]
    pub fn in_words(&self) -> String {
        format!("**{}**\n{}", self.titled(), self.place)
    }
}
