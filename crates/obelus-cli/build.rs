//! Which build this is, for the first line of the log.
//!
//! The whole of it is shared with the window's binary, which writes the same
//! line about a build made the same way: a commit stamped into one of them
//! and not the other would be two answers to "was the fix in the thing you
//! ran". Included rather than copied, because a build script is not a
//! library and there is nowhere else for a Cargo workspace to put one.
include!("../../build/commit.rs");
