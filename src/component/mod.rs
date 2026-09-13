//! Stateful interaction components.
//!
//! Components own the state and key handling behind a piece of the interface.
//! The views in [`crate::ui`] borrow them to draw, but do not define their
//! behaviour.

pub mod card;
pub mod chat;
pub mod composer;
pub mod counts;
pub mod picker;
pub mod prompt;
pub mod settings;
pub mod window;
