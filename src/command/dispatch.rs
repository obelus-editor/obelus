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
        Command::BufferClose => app.close_current(),
        Command::MarkdownPreview => app.toggle_markdown(),
        Command::ThemeSelect => app.open_theme_picker(),
        Command::CommandPalette => app.open_command_palette(),
        Command::SymbolMenu => app.open_symbol_menu(),
        Command::SymbolOutline => app.open_outline(),
        Command::SymbolDefinition
        | Command::SymbolTypeDefinition
        | Command::SymbolImplementation
        | Command::SymbolReferences => app.ask_about_symbol(command),
        Command::GoLine => app.open_line_prompt(),
        Command::GoBracket => app.go_to_bracket(),
        Command::SelectionCopy => app.copy_selection(),
        Command::GoBack => app.go_back(),
        Command::GoForward => app.go_forward(),
        Command::LogOpen => app.open_log(),
        Command::LspRestart => app.restart_server(),
        Command::LspStop => app.stop_server(),
        Command::Quit => app.request_quit(),
    }
}
