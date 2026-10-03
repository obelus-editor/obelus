//! Working on notes from a chat.
//!
//! A reader away from their screen talks to Obelus through a chat they
//! already have on their phone -- Slack first, others after it -- and what
//! they find there is the same conversations their windows hold: one thread
//! a conversation, and at the top a conversation of its own with the agent
//! that finds notes and starts the rest.
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

pub mod model;
pub mod platform;
pub mod secrets;
pub mod slack;

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
    Connection(State),
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
    /// Somebody said something.
    Heard {
        /// Their id, which is what they are checked by.
        from: String,
        /// Where they said it.
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
    /// What somebody is called.
    Named {
        /// Their id.
        id: String,
        /// The name they go by.
        name: String,
    },
}

/// Where this machine stands with the chat it is set to, as one answer.
///
/// One, because there is one place it is said on the settings page and one
/// mark on the status row, and two answers there would be the two drifting
/// apart.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum State {
    /// No chat is set.
    #[default]
    Off,
    /// One is set, and something it has to be told has not been.
    Unready,
    /// Connecting, or trying again.
    Connecting,
    /// Connected, by this window.
    Connected,
    /// Connected, by another Obelus on this machine, which passes on what is
    /// this window's.
    Through,
    /// The platform turned a token down.
    Refused,
    /// The platform could not be reached; Obelus goes on trying.
    Unreachable,
    /// The keyring would not open.
    Locked,
    /// There is no keyring on this machine to keep a token in.
    NoKeyring,
}

impl State {
    /// What it is, in words, for a platform called `name`.
    #[must_use]
    pub fn words(self, name: &str) -> String {
        match self {
            Self::Off => "Off".to_string(),
            Self::Unready => "Not set up".to_string(),
            Self::Connecting => "Connecting".to_string(),
            Self::Connected => "Connected".to_string(),
            Self::Through => "Through another window".to_string(),
            Self::Refused => format!("{name} refused a token"),
            Self::Unreachable => format!("Cannot reach {name}"),
            Self::Locked => "The keyring is locked".to_string(),
            Self::NoKeyring => "No keyring on this machine".to_string(),
        }
    }

    /// Whether messages are getting through, by this window or another.
    #[must_use]
    pub const fn connected(self) -> bool {
        matches!(self, Self::Connected | Self::Through)
    }

    /// Whether something is wrong that the reader has to do something about.
    #[must_use]
    pub const fn wrong(self) -> bool {
        matches!(
            self,
            Self::Refused | Self::Unreachable | Self::Locked | Self::NoKeyring
        )
    }
}
