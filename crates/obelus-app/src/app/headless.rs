//! Obelus with nobody at the screen: `ob --headless`, there to be reached
//! from a chat.
//!
//! **The same Obelus, drawn on nothing.** The loop is the loop every front
//! end runs, and the frames it draws go into a buffer nobody reads. A front
//! end of its own was the alternative, and would have been a second answer
//! to every question the loop already asks of itself between frames -- the
//! connection, the threads, the watches -- each of them settled from what is
//! drawn being asked for.
//!
//! **What would wait for the reader stops instead.** A window that cannot
//! connect says so on its status row and waits for somebody to mend it; a
//! process nobody is looking at would wait for ever, and its being there
//! would hold the chat from a window that could have had it. So what the
//! reader has to do something about -- no chat set, one never paired, a
//! token turned down, another window that would not let go, a tree that has
//! gone -- ends the process with the reason, and a chat that cannot be
//! reached for now is tried again, as anywhere. Asked before the loop where
//! it can be ([`App::ready_to_be_reached`]), so the commonest of them is said
//! before anything has started.
//!
//! **Nothing of the reader's is touched.** No file is opened: what an agent
//! writes to an open file goes into it and waits there to be saved, and here
//! nobody would save it. What was open in a window on the tree is neither
//! opened again nor written over, because the record is the last window's
//! to change and this is not a window. And nothing moves on its own: there
//! is no animation for a screen nobody sees.
//!
//! What the status row would have said goes to the log, which is the one
//! thing here anybody reads.

use std::path::PathBuf;

use anyhow::Result;

use super::App;
use crate::{event, startup};

impl App {
    /// Says nobody is at this Obelus's screen.
    ///
    /// Before the settings are read and what was open is opened again,
    /// both of which ask.
    pub fn headless(&mut self) {
        self.headless = true;
    }

    /// Whether nobody is at the screen.
    pub(super) const fn is_headless(&self) -> bool {
        self.headless
    }

    /// Whether everything a headless Obelus needs before it starts is
    /// there, and what is not where it is not.
    ///
    /// A pairing and an install are what cannot be had here: the code to
    /// pair with is drawn on the settings page for the reader to send to
    /// the bot, and an install is pressed for there.
    ///
    /// # Errors
    ///
    /// What is missing, in the words it is said in.
    pub fn ready_to_be_reached(&mut self) -> Result<(), String> {
        let Some(platform) = self.platform() else {
            return Err("No chat is set in the settings to connect to".to_string());
        };
        if self.the_room().is_none() {
            return Err(format!(
                "{} is not paired: pair it from the settings in a window first",
                platform.name
            ));
        }
        // And somebody to talk to: what a chat says to an Obelus with no
        // agent goes nowhere, and a reader on a phone hears nothing back.
        let Some(agent) = self
            .settled
            .config
            .agent
            .clone()
            .filter(|id| !id.is_empty())
        else {
            return Err("No agent is set in the settings to talk to".to_string());
        };
        if self.installed(&agent).is_none() {
            return Err(format!(
                "Nothing is installed as {agent}: install it from the settings in a window first"
            ));
        }
        Ok(())
    }

    /// Ends a headless Obelus with the reason, and says whether it did.
    ///
    /// Nothing where somebody is at the screen, where the same thing is a
    /// line on the status row and the reader's to answer -- which is why a
    /// caller says it there only where this says no: the reason is said
    /// once, on the way out, and the status row's line would be it again.
    pub(super) fn give_up_unseen(&mut self, why: impl Into<String>) -> bool {
        if !self.headless {
            return false;
        }
        // The first reason, which is the one the rest followed from.
        self.stopped_because.get_or_insert_with(|| why.into());
        self.should_quit = true;
        true
    }

    /// Ends a headless Obelus that has nothing left to do, which is not a
    /// failure: the chat gone to the window the reader asked for it in, or
    /// no chat set any more. Cleanly, so that whatever started it to be
    /// restarted on a failure does not start it again to take the chat
    /// back.
    pub(super) fn leave_unseen(&mut self) {
        if self.headless {
            self.should_quit = true;
        }
    }

    /// Why a headless Obelus ended, where it gave up rather than being
    /// told to stop.
    #[must_use]
    pub fn why_it_stopped(&self) -> Option<&str> {
        self.stopped_because.as_deref()
    }
}

/// How big the screen nobody sees is.
///
/// A size at all because what a conversation is laid out at is a width,
/// and a chat is handed what was laid out. An ordinary terminal's.
const UNSEEN: (u16, u16) = (120, 40);

/// Runs Obelus with nobody at the screen, until it is told to stop or has
/// to give up.
///
/// # Errors
///
/// Why it would not start, or why it gave up.
pub fn run_headless(paths: &[PathBuf], built: &'static str) -> Result<()> {
    let mut app = startup::start_headless(paths, built)?;
    tracing::info!("drawn on nothing");
    let (sender, events) = event::channel();
    event::stop_when_told(sender.clone());
    app.start(sender);
    let mut nothing =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(UNSEEN.0, UNSEEN.1))?;
    super::run(&mut nothing, &mut app, events)?;
    match app.why_it_stopped() {
        Some(why) => anyhow::bail!("{why}"),
        None => Ok(()),
    }
}
