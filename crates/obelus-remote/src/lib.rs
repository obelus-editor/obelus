//! Working on notes from a chat.
//!
//! A reader away from their screen talks to Obelus through a chat they
//! already have on their phone -- Slack first, others after it -- and what
//! they find there is the same conversations their windows hold: one thread
//! a conversation, and a thread they start a conversation begun.
//!
//! **Words are the floor.** Most chats cannot be given a screen of their own,
//! so everything Obelus says there is text and everything it is told is
//! text: a question is a numbered list answered by replying with a number. A
//! platform that can draw buttons may draw them later, and pressing one is the
//! same as replying with its number.
//!
//! **A platform declares; Obelus keeps.** What one has to be told is a list
//! of fields ([`platform`]), and where each is kept -- a secret in the
//! keyring ([`secrets`]), anything else in `[remotes.<platform>]` -- is
//! decided here once, for all of them.

pub mod feishu;
pub mod model;
pub mod platform;
pub mod secrets;
pub mod slack;
mod waiting;

/// What the work done for a chat comes back with.
#[derive(Debug)]
pub enum Event {
    /// What the keyring keeps for one of a platform's secrets.
    Kept {
        /// Which platform it was asked for.
        platform: &'static str,
        /// Which field.
        field: &'static str,
        /// The secret, `None` where none is kept, or why it could not be
        /// asked.
        read: Result<Option<String>, secrets::Trouble>,
    },
    /// One of them kept or forgotten, with how its row now draws it.
    Written {
        /// Which platform it was for.
        platform: &'static str,
        /// Which field.
        field: &'static str,
        /// Its ends where one was kept, `None` where it was forgotten, or
        /// why the keyring would not.
        written: Result<Option<String>, secrets::Trouble>,
    },
    /// Where the connection to a platform has got to.
    Connection {
        /// Where.
        state: State,
        /// Why, where it is something wrong and something said why: the
        /// platform's own words, or what would not answer. Capped, because
        /// it is somebody else's text.
        why: Option<String>,
    },
    /// A platform is being connected to, and this is where to send what is
    /// to be said to it.
    Started {
        /// Which.
        platform: &'static str,
        /// Where to send things; dropped, it disconnects.
        out: tokio::sync::mpsc::UnboundedSender<model::Out>,
    },
    /// The code to pair with has run out.
    PairingOver,
    /// Somebody said something in a group the bot is in.
    Heard {
        /// Their id, which is what they are checked by.
        from: String,
        /// Which group, by the platform's id for it: the room once one is
        /// kept, and any group while there is a code to pair with.
        room: String,
        /// Where in it.
        at: model::Where,
        /// What.
        text: String,
    },
    /// A thread asked for is there.
    Opened {
        /// The number it was asked for with.
        asked: u64,
        /// What the platform calls it.
        thread: String,
        /// Where a reader can be sent to read it, where the platform says.
        link: Option<String>,
    },
    /// A thread asked for did not open: the platform would not, or could
    /// not be reached. Said so that the conversation is not left waiting
    /// on it, and is asked for again on the next connection.
    Unopened {
        /// The number it was asked for with.
        asked: u64,
        /// Whether it was let go of here, waiting too long for a connection,
        /// rather than refused by the platform: the conversation says which,
        /// since only one of them is the platform's doing.
        waited: bool,
    },
    /// What somebody is called.
    Named {
        /// Their id.
        id: String,
        /// The name they go by.
        name: String,
    },
}

/// Where a window stands with the chat it talks to, as one answer.
///
/// One, because the mark on its status row and the rows of the settings
/// page that say what is wrong both read it, and two answers there would be
/// the two drifting apart.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum State {
    /// No chat is set.
    #[default]
    Off,
    /// One is set, and something it has to be told has not been.
    Unready,
    /// Connecting, or trying again.
    Connecting,
    /// Connected.
    Connected,
    /// The platform turned a token down.
    Refused,
    /// The platform took the tokens and would not let the app connect:
    /// something on its side of the app is not set up, which no row of
    /// Obelus's can mend.
    Declined,
    /// The platform could not be reached; Obelus goes on trying.
    Unreachable,
    /// The keyring would not open.
    Locked,
    /// There is no keyring on this machine to keep a token in.
    NoKeyring,
}

impl State {
    /// Whether messages are getting through.
    #[must_use]
    pub const fn connected(self) -> bool {
        matches!(self, Self::Connected)
    }

    /// Whether something is wrong that the reader has to do something about.
    #[must_use]
    pub const fn wrong(self) -> bool {
        // Unready among them: a window is only asked where it stands once
        // the reader has told it to connect, and a field nobody filled in
        // is then a connection that will never be made.
        matches!(
            self,
            Self::Unready
                | Self::Refused
                | Self::Declined
                | Self::Unreachable
                | Self::Locked
                | Self::NoKeyring
        )
    }
}

impl Event {
    /// Where the connection has got to, and why, with the why capped.
    #[must_use]
    pub fn connection(state: State, why: Option<String>) -> Self {
        Self::Connection {
            state,
            why: why.map(|why| capped(&why)),
        }
    }
}

/// How much of a reason is kept: enough for the sentence a platform says
/// why in, and not a page of HTML from a proxy in front of it.
const REASON_AT_MOST: usize = 160;

fn capped(why: &str) -> String {
    let why = why.trim();
    match why.chars().nth(REASON_AT_MOST) {
        None => why.to_string(),
        Some(_) => {
            let kept: String = why.chars().take(REASON_AT_MOST - 1).collect();
            format!("{kept}\u{2026}")
        }
    }
}
