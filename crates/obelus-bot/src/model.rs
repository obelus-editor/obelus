//! What passes between Obelus and a chat platform: words, and where they go.
//!
//! **Words, and a question.** A platform is told to say some text somewhere
//! and tells Obelus what somebody said and where. The one thing that is not
//! words is a question ([`Question`]): what it is made of, for the platform
//! to draw as a card, and a press on the card is its answer.
//!
//! Each of them also crosses from one Obelus to another, to the window the
//! chat talks to and back (`relay`), which is why they can be written down.

use serde::{Deserialize, Serialize};

/// Where in the room something was said.
///
/// **A room is a group the reader made and put the bot in** -- a topic group
/// on Feishu, a private channel on Slack -- and it is the only place Obelus
/// hears anything. Every conversation is a thread in it, and every thread
/// in it a conversation.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Where {
    /// A thread, by the platform's own name for it -- which nothing here
    /// reads, only keeps and hands back.
    Thread(String),
    /// A message outside any thread, by the platform's name for it, which
    /// is the thread that hangs off it: a conversation the reader is
    /// beginning -- or, while there is a code to pair with, perhaps the
    /// code.
    Fresh(String),
}

/// Something Obelus asks a platform to do.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Out {
    /// Say something in a thread.
    Say {
        /// Which room, by the platform's id for it.
        room: String,
        /// Which thread in it.
        thread: String,
        /// Whom the thread is with, by the platform's id for them, to call
        /// them by name.
        to: String,
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
        /// Which room.
        room: String,
        /// What the thread is, which is its first message.
        head: Head,
    },
    /// Say what a thread is again, because something about it moved: its
    /// name, its branch, where its turn has got to.
    Retitle {
        /// Which room.
        room: String,
        /// Which thread, as the platform named it.
        thread: String,
        /// What it is now.
        head: Head,
    },
    /// Find out what somebody is called, for the list of who may talk.
    Name {
        /// Their id.
        id: String,
    },
    /// Put a question the agent is waiting on to the reader in a thread,
    /// calling them, as a card answered by `Event::Answered`.
    Ask {
        /// Which room.
        room: String,
        /// Which thread.
        thread: String,
        /// Whom the thread is with.
        to: String,
        /// Obelus's own number for the question, handed back with the
        /// answer and with what became of it.
        asked: u64,
        /// The question.
        question: Question,
    },
    /// What became of a question asked: answered, here or there, or taken
    /// back -- drawn on the card, so that it cannot be answered twice.
    Settle {
        /// Which room.
        room: String,
        /// Which thread.
        thread: String,
        /// Whom the thread is with.
        to: String,
        /// The number it was asked with.
        asked: u64,
        /// What became of it, in a line.
        said: String,
    },
}

impl Out {
    /// What became of a question whose card never went up -- the platform
    /// would not take it, or it went on a connection since let go -- said
    /// in the thread instead.
    #[must_use]
    pub fn in_words(self) -> Self {
        match self {
            Self::Settle {
                room,
                thread,
                to,
                said,
                ..
            } => Self::Say {
                room,
                thread,
                to,
                text: said,
                notify: false,
            },
            out => out,
        }
    }
}

impl Question {
    /// The question in words, for a thread whose platform would not take
    /// its card: what it asks, and that it is answered on the machine,
    /// since nothing in the thread can be.
    #[must_use]
    pub fn in_words(&self) -> String {
        let mut said = format!("\u{2753} {}", self.about);
        for (_, name) in &self.choices {
            said.push_str("\n- ");
            said.push_str(name);
        }
        said.push_str("\nThis could not be put here as a card; answer it on the machine.");
        said
    }
}

/// A question the agent is waiting on, as a chat is given it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Question {
    /// What it is about.
    pub about: String,
    /// The named answers: the agent's id for each, and what it is called.
    pub choices: Vec<(String, String)>,
    /// Whether more than one may be chosen.
    pub several: bool,
    /// Whether one of them has to be.
    pub needed: bool,
    /// The box for the reader's own words, where it has one: what it is
    /// called, and whether the agent needs something in it.
    pub words: Option<(String, bool)>,
}

/// What a thread is: the first message of it, said again whenever any of
/// this moves.
///
/// Its own type rather than a line of text, because where a platform can
/// draw a card it draws one -- the state as the card's colour, the name as
/// its title -- and where it cannot, [`Head::in_words`] is the line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A question no card could carry is said whole, and says where it is
    /// answered: nothing in the thread can answer it.
    ///
    /// Broken deliberately by leaving out the answers: the thread was asked
    /// something with nothing to choose from.
    #[test]
    fn a_question_in_words_says_where_it_is_answered() {
        let question = Question {
            about: "Read the file?".to_string(),
            choices: vec![
                ("once".to_string(), "Allow once".to_string()),
                ("never".to_string(), "Reject".to_string()),
            ],
            several: false,
            needed: true,
            words: None,
        };
        assert_eq!(
            question.in_words(),
            "\u{2753} Read the file?\n- Allow once\n- Reject\nThis could not be put here as a card; answer it on the machine."
        );
    }
}
