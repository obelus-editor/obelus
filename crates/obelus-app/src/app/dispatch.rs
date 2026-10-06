//! Executing a command.
//!
//! The match below has no wildcard arm, and the lint makes adding one an
//! error. A new [`Command`] variant therefore fails to compile until it is
//! handled here.
#![warn(clippy::wildcard_enum_match_arm)]

use obelus_command::Command;
use obelus_search::Scope;

use crate::app::App;

/// Runs `command` against the application state.
pub fn dispatch(app: &mut App, command: Command) {
    tracing::debug!(command = command.name(), "dispatch");
    match command {
        Command::FileOpen => app.open_file_picker(),
        Command::FileChanged => app.open_changed_files(),
        Command::FileReload => app.reload_current(),
        Command::FileSave => app.save_current(),
        Command::DocumentList => app.open_document_picker(),
        Command::WorktreeList => app.open_switching(super::worktrees::Tab::Worktrees),
        Command::DocumentClose => app.close_current(),
        Command::FileRename => app.rename_file(),
        Command::FileNew => app.new_file(),
        Command::PreviewToggle => app.toggle_preview(),
        Command::ThemeSelect => app.open_theme_picker(),
        Command::CommandPalette => app.open_command_palette(),
        Command::SymbolMenu => app.open_symbol_menu(),
        Command::SymbolComplete => app.ask_completion(),
        Command::SymbolOutline => app.open_outline(),
        Command::SymbolHover => app.ask_hover(),
        Command::SymbolSignature => app.ask_signature_here(),
        Command::CodeActions => app.ask_code_actions(),
        Command::SymbolRename => app.rename_symbol(),
        Command::SymbolTroubles => app.open_troubles(),
        Command::SymbolDefinition
        | Command::SymbolTypeDefinition
        | Command::SymbolImplementation
        | Command::SymbolReferences
        | Command::SymbolCalls => app.ask_about_symbol(command),
        Command::SearchFile => app.open_search(Scope::File),
        Command::SearchProject => app.open_search(Scope::Project),
        Command::SearchSymbols => app.open_search(Scope::Symbols),
        Command::GoLine => app.open_line_prompt(),
        Command::GoBracket => app.go_to_bracket(),
        Command::HistoryFile => app.open_history(crate::app::About::File),
        Command::HistoryProject => app.open_history(crate::app::About::Project),
        Command::HistoryLine => app.open_line_commit(),
        Command::Fold => app.toggle_fold(),
        Command::FoldAll => app.fold_all(),
        Command::UnfoldAll => app.unfold_all(),
        Command::GitHunk => app.toggle_hunk(),
        Command::SymbolTroublePrevious => app.go_to_previous_trouble(),
        Command::SymbolTroubleNext => app.go_to_next_trouble(),
        Command::GitPrevious => app.go_to_previous_change(),
        Command::GitNext => app.go_to_next_change(),
        Command::SelectionCopy => app.copy_selection(),
        Command::SelectionCut => app.cut_selection(),
        Command::Paste => app.paste(),
        Command::ReplaceToggle => app.toggle_replacing(),
        Command::LineUp => app.move_lines(true),
        Command::LineDown => app.move_lines(false),
        Command::CommentToggle => app.toggle_comment(),
        Command::Undo => app.undo(),
        Command::Redo => app.redo(),
        Command::SelectionClear => app.clear_selection(),
        Command::SelectionAll => app.select_all(),
        Command::SelectionWiden => app.widen_selection(),
        Command::GoBack => app.go_back(),
        Command::GoForward => app.go_forward(),
        Command::ConversationNew => app.new_conversation(),
        Command::ConversationSelect => app.open_conversation_picker(),
        Command::TerminalOpen => app.open_terminal(),
        Command::CountLines => app.open_counts(),
        Command::TodoOpen => app.open_todo(),
        Command::TodoAdd => app.add_todo(),
        Command::ConfigOpen => app.open_settings(),
        Command::RemoteConnect => app.connect_remote(),
        Command::RemoteDisconnect => app.disconnect_remote(),
        Command::ConfigProject => app.open_project_settings(),
        Command::ConfigFile => app.open_config_file(),
        Command::LogOpen => app.open_log(),
        Command::LogServers => app.open_server_log(),
        Command::LspRestart => app.restart_server(),
        Command::LspStop => app.stop_server(),
        Command::Quit => app.request_quit(),
    }
}
