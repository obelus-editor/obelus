//! Everything obelus knows, the screen it draws, and the loop between them.
//!
//! The crate at the top, and the only one that has heard of all the others:
//! the application's state is one thing, and cutting it into several would
//! mean deciding, for every pair of them, which one owns the answer.
//!
//! What is here besides the state is what only something at the top can
//! write. [`event`] is the one channel the loop reads, and the `From` impls
//! on it are where each worker's own events are said to be the same inbox --
//! written here because this is the only crate that has heard of every
//! worker. [`ui`] draws, which needs the state to draw from.

pub mod app;
pub mod conversation;
pub mod event;
pub mod jump;
pub mod ui;
