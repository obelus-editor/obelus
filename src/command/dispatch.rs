//! Executing a command.
//!
//! The match below has no wildcard arm, and the lint makes adding one an
//! error. A new [`Command`] variant therefore fails to compile until it is
//! handled here.
#![warn(clippy::wildcard_enum_match_arm)]

use crate::{app::App, command::Command};

/// Runs `command` against the application state.
pub fn dispatch(app: &mut App, command: Command) {
    tracing::debug!(command = command.name(), "dispatch");
    match command {
        Command::FileOpen => app.open_file_picker(),
        Command::FileReload => app.reload_current(),
        Command::BufferList => app.open_buffer_picker(),
        Command::ThemeSelect => app.open_theme_picker(),
        Command::CommandPalette => app.open_command_palette(),
        Command::Quit => app.request_quit(),
    }
}
