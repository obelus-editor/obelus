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

pub mod platform;
pub mod secrets;
pub mod slack;
