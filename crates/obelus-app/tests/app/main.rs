//! The application's tests, in one binary.
//!
//! Every file under `tests/` is a binary of its own, linked with everything
//! Obelus is made of: sixty of them were most of a checkout's `target`, and
//! most of the time it took to build the suite. So everything that can share
//! a process shares this one.
//!
//! What cannot is a file beside this directory rather than in it, and says
//! why at its top -- a switch the whole process reads, set by its tests and
//! never set back. The settings apply the glyph switch and the tab width,
//! the remote page keeps its secrets in one directory for the process, and
//! a start applies whatever settings it reads -- the machine's own among
//! them; a test here drawing a screen while one of those ran would draw it
//! wrong. And the page that asks which project keeps one list for the
//! process, which every test that asks rewrites.
//!
//! What they do share has to be taken in turns across files now, not only
//! within one: the clipboard and the link opener are one thing for this
//! whole binary (`support::clipboard_turn`).

#[path = "../support/mod.rs"]
mod support;

mod agent;
mod calls;
mod changing;
mod choosing_a_keymap;
mod colour;
mod complete;
mod counts;
mod dialogs;
mod editing;
mod field;
mod folds;
mod git;
mod hiding;
mod hint;
mod hover;
mod inviting;
mod layers;
mod making_files;
mod markdown;
mod names;
mod opening;
mod picker;
mod pointer;
mod pointing;
mod read_only;
mod reload;
mod renaming_files;
mod rendering;
mod replacing;
mod saying;
mod scrollbars;
mod search;
mod selecting;
mod serving;
mod sessions;
mod switching;
mod syntax;
mod terminal;
mod themes;
mod todo;
mod tools;
mod tree;
mod trouble;
mod watch;
mod worktrees;
