//! The tests that set what the whole process reads, in one binary that
//! takes turns.
//!
//! Each of these applies settings or reads a start's -- the glyph switch, the
//! tab width, the remote's secrets -- or asserts on the list of projects
//! that asking which project rewrites. Beside the application's tests they
//! would move those underneath a screen being drawn, which is why they are
//! not in its binary -- but they need not have a binary each, at three
//! hundred and seventy megabytes apiece, when they can share one and take
//! turns. Every test here holds [`turn`] for as long as it runs, so the
//! process is one test's at a time.
//!
//! A test added here takes it too, first thing, or it runs beside whichever
//! test is holding it and sees that test's settings.

#[path = "../support/mod.rs"]
mod support;

mod choosing_a_project;
mod glyphs;
mod headless;
mod remote;
mod reopening;
mod settings;
mod starting;
mod switching_projects;
mod tabs;
mod title_bar;

/// The one turn at the process, held for the whole of a test.
///
/// Taken even where a test that held it panicked: a poisoned lock means one
/// test failed, and failing every other with it hides which.
pub(crate) fn turn() -> std::sync::MutexGuard<'static, ()> {
    static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
    TURN.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
