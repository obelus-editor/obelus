//! Talking to an agent, over the Agent Client Protocol.
//!
//! An ACP agent is another program on the other end of a pipe: obelus starts
//! it, says who it is, opens a session rooted at the project, and then sends
//! prompts and reads what comes back. The shape is the language server
//! protocol's -- JSON-RPC over stdio, requests numbered and answered out of
//! order -- and so is the reason it needs no async runtime: reading is a
//! thread blocked on a pipe, and what it reads becomes an [`Event`] like
//! every other source in obelus.
//!
//! [`Event`]: crate::event::Event
//!
//! Written here rather than taken from the protocol's own crate. That crate
//! is the reference implementation and it is good, but it is built around
//! `async` traits and brings a runtime with it, and what obelus needs of the
//! protocol is nine methods -- five it calls and four it answers. The wire
//! shapes are checked against the published schema (`initialize`,
//! `session/new`, `session/prompt`, `session/cancel`, `session/update`,
//! `session/request_permission`, `fs/read_text_file`, `fs/write_text_file`),
//! and the tests below encode them.
//!
//! What obelus tells an agent about itself is the shape of the product: it
//! will read a file out (from a buffer, so an agent sees what the reader
//! sees) and it will not write one. A code reader that let an agent write
//! through it would be a code editor with no undo.

pub mod client;
pub mod transport;

pub use client::{Choice, Client, Incoming, Mode, Order, Permission, Update};

/// Which version of the protocol obelus speaks.
///
/// One. Two exists as a draft and the crate that defines it keeps it behind
/// an `unstable` feature; an agent that only speaks two is an agent obelus
/// declines rather than guesses at.
pub const VERSION: u16 = 1;
