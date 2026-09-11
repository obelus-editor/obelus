#![deny(private_interfaces, unreachable_pub)]
#![warn(missing_docs)]

//! obelus — a terminal code reader.
//!
//! Reading is the product; editing is incidental. The pieces here are the ones
//! everything later depends on: named commands with the key table as data, the
//! coordinate spaces kept apart by the type system, and a document that owns
//! every conversion between them.

pub mod acp;
pub mod agent;
pub mod app;
pub mod buffer;
pub mod clipboard;
pub mod command;
pub mod component;
pub mod config;
pub mod coordinates;
pub mod event;
pub mod git;
pub mod icons;
pub mod jump;
pub mod keymap;
pub mod logging;
pub mod lsp;
pub mod markdown;
pub mod reading;
pub mod search;
pub mod syntax;
pub mod text;
pub mod theme;
pub mod ui;
pub mod watch;
