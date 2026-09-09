//! Named actions, and the table that is both the keymap's target and the
//! command palette's contents.
//!
//! A command is an *action* with a name worth typing. Navigation is not a
//! command: arrow keys, `PageUp`/`PageDown` and a picker's selection keys are
//! handled by whichever component owns the state they move, because
//! `:cursor.up` is meaningless to invoke by name and putting every printable
//! character behind a command would be the logical end of that road.
//!
//! The enum grows one variant at a time, as each command's handler is written.
//! A variant that dispatches to nothing would compile, appear in the palette,
//! and do nothing when chosen — a failure that announces itself to nobody.

pub mod dispatch;

/// Everything obelus can be asked to do by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    /// Choose a file under the working directory and open it.
    FileOpen,
    /// Re-read the current file from disk and reparse what changed.
    FileReload,
    /// Choose among the files already open.
    BufferList,
    /// Stop showing the current file.
    BufferClose,
    /// Show this file as rendered markdown, or stop.
    MarkdownPreview,
    /// Choose a theme.
    ThemeSelect,
    /// Choose a command by name.
    CommandPalette,
    /// Ask a language server about the symbol under the cursor.
    SymbolMenu,
    /// Every symbol this file defines, to jump to.
    SymbolOutline,
    /// Where the symbol under the cursor is defined.
    SymbolDefinition,
    /// Where its type is defined.
    SymbolTypeDefinition,
    /// What implements it.
    SymbolImplementation,
    /// Everywhere it is used.
    SymbolReferences,
    /// Go to a line by number.
    GoLine,
    /// Go to the bracket that matches the one under the cursor.
    GoBracket,
    /// Return to where the last jump was made from.
    GoBack,
    /// Undo a jump back.
    GoForward,
    /// Open the file obelus logs to.
    LogOpen,
    /// Stop the language server for this file and start it again.
    LspRestart,
    /// Stop the language server for this file and leave it stopped.
    LspStop,
    /// Leave obelus.
    Quit,
}

/// The part of obelus a command belongs to.
///
/// Three, and deliberately few: the palette shows these as tabs, and every
/// tab is somewhere a reader has to look before deciding to type instead. A
/// group per dotted prefix would be nine of them, which is a worse way to
/// find `file.open` than its name is.
///
/// Split by what the reader is doing, not by what the code touches: reading
/// files, following what the code means, and running obelus itself -- which
/// is where restarting a server and opening the log both belong, being
/// housekeeping rather than reading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    /// Opening and re-reading files, and moving between the open ones.
    Files,
    /// Following the code: what a symbol is, and where you have been.
    Code,
    /// obelus itself, its colours, its log, and its language servers.
    Obelus,
}

impl Group {
    /// Every group, in the order the tabs appear.
    pub const ALL: &'static [Self] = &[Self::Files, Self::Code, Self::Obelus];

    /// The word on the tab.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Files => "files",
            Self::Code => "code",
            Self::Obelus => "obelus",
        }
    }
}

/// What a command needs before it is worth offering.
///
/// Spelled out per command rather than left to a wildcard, so a new command
/// has to say which it is. The alternative is that every new command silently
/// requires nothing, which is right often enough to be a bad default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Requires {
    /// Always available.
    Nothing,
    /// A language server has to be running for this file.
    ARunningServer,
    /// The running server has to say it answers this question.
    AnAnswer,
}

/// A command's name and description, for the palette to list and match on.
#[derive(Clone, Copy, Debug)]
pub struct CommandSpec {
    /// The command itself.
    pub command: Command,
    /// The dotted name shown in the palette and matched against the query.
    pub name: &'static str,
    /// One line of description.
    pub title: &'static str,
}

/// Every command, in the order the palette lists them.
pub const ALL: &[CommandSpec] = &[
    CommandSpec {
        command: Command::FileOpen,
        name: "file.open",
        title: "Open a file",
    },
    CommandSpec {
        command: Command::FileReload,
        name: "file.reload",
        title: "Re-read this file from disk",
    },
    CommandSpec {
        command: Command::BufferList,
        name: "buffer.list",
        title: "Switch to an open file",
    },
    CommandSpec {
        command: Command::BufferClose,
        name: "buffer.close",
        title: "Close this file",
    },
    CommandSpec {
        command: Command::MarkdownPreview,
        name: "markdown.preview",
        title: "Show this file rendered, or stop",
    },
    CommandSpec {
        command: Command::ThemeSelect,
        name: "theme.select",
        title: "Change the colours",
    },
    CommandSpec {
        command: Command::CommandPalette,
        name: "command.palette",
        title: "Run a command by name",
    },
    CommandSpec {
        command: Command::SymbolMenu,
        name: "symbol.menu",
        title: "Ask about the symbol under the cursor",
    },
    // No default keys. The menu is the way in, and giving each of these a
    // chord would rebuild the one-key-per-question arrangement the menu
    // exists to replace. Binding one later needs no code: the menu and the
    // palette both read the key table.
    CommandSpec {
        command: Command::SymbolOutline,
        name: "symbol.outline",
        title: "Everything this file defines",
    },
    CommandSpec {
        command: Command::SymbolDefinition,
        name: "symbol.definition",
        title: "Go to definition",
    },
    CommandSpec {
        command: Command::SymbolTypeDefinition,
        name: "symbol.typeDefinition",
        title: "Go to type definition",
    },
    CommandSpec {
        command: Command::SymbolImplementation,
        name: "symbol.implementation",
        title: "Go to implementation",
    },
    CommandSpec {
        command: Command::SymbolReferences,
        name: "symbol.references",
        title: "Find references",
    },
    CommandSpec {
        command: Command::GoLine,
        name: "go.line",
        title: "Go to a line by number",
    },
    CommandSpec {
        command: Command::GoBracket,
        name: "go.bracket",
        title: "Go to the matching bracket",
    },
    CommandSpec {
        command: Command::GoBack,
        name: "go.back",
        title: "Go back to where you were",
    },
    CommandSpec {
        command: Command::GoForward,
        name: "go.forward",
        title: "Go forward again",
    },
    CommandSpec {
        command: Command::LogOpen,
        name: "log.open",
        title: "Open obelus's own log",
    },
    CommandSpec {
        command: Command::LspRestart,
        name: "lsp.restart",
        title: "Restart the language server",
    },
    CommandSpec {
        command: Command::LspStop,
        name: "lsp.stop",
        title: "Stop the language server",
    },
    CommandSpec {
        command: Command::Quit,
        name: "app.quit",
        title: "Leave obelus",
    },
];

impl Command {
    /// Which group the command belongs to.
    ///
    /// Spelled out per command rather than taken from the name's prefix:
    /// `go.back` and `symbol.definition` share a group and no prefix, and a
    /// prefix that had to be renamed to move a command between groups would
    /// be a name chosen for the wrong reason.
    #[must_use]
    pub const fn group(self) -> Group {
        match self {
            Self::FileOpen
            | Self::FileReload
            | Self::BufferList
            | Self::BufferClose
            | Self::MarkdownPreview => Group::Files,
            Self::SymbolMenu
            | Self::SymbolOutline
            | Self::SymbolDefinition
            | Self::SymbolTypeDefinition
            | Self::SymbolImplementation
            | Self::SymbolReferences
            | Self::GoLine
            | Self::GoBracket
            | Self::GoBack
            | Self::GoForward => Group::Code,
            Self::LspRestart
            | Self::LspStop
            | Self::LogOpen
            | Self::ThemeSelect
            | Self::CommandPalette
            | Self::Quit => Group::Obelus,
        }
    }

    /// What has to be true for the command to be worth offering.
    ///
    /// The palette leaves out anything that cannot do its job right now: a
    /// row that silently fails is worse than a row that is not there. Note
    /// what is *not* conditional -- `symbol.menu` with no server is the thing
    /// that says why there is none, and `lsp.restart` with no server
    /// running is how you get one.
    #[must_use]
    pub const fn requires(self) -> Requires {
        match self {
            Self::SymbolDefinition
            | Self::SymbolTypeDefinition
            | Self::SymbolImplementation
            | Self::SymbolReferences => Requires::AnAnswer,
            Self::LspStop => Requires::ARunningServer,
            Self::FileOpen
            | Self::FileReload
            | Self::BufferList
            | Self::BufferClose
            | Self::MarkdownPreview
            | Self::ThemeSelect
            | Self::CommandPalette
            | Self::SymbolMenu
            | Self::SymbolOutline
            | Self::GoLine
            | Self::GoBracket
            | Self::GoBack
            | Self::GoForward
            | Self::LogOpen
            | Self::LspRestart
            | Self::Quit => Requires::Nothing,
        }
    }

    /// This command's entry in [`ALL`].
    ///
    /// # Panics
    ///
    /// Panics if the command is missing from [`ALL`], which the test below
    /// rules out.
    #[must_use]
    pub fn spec(self) -> &'static CommandSpec {
        ALL.iter()
            .find(|spec| spec.command == self)
            .expect("every command has an entry in ALL")
    }

    /// The dotted name, for logs and the palette.
    #[must_use]
    pub fn name(self) -> &'static str {
        self.spec().name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `spec` panics for a command left out of `ALL`, and the palette would
    /// simply not list it. Both are silent, so enumerate the variants here.
    #[test]
    fn every_command_is_in_the_table() {
        for command in [
            Command::FileOpen,
            Command::FileReload,
            Command::BufferList,
            Command::BufferClose,
            Command::MarkdownPreview,
            Command::ThemeSelect,
            Command::CommandPalette,
            Command::SymbolMenu,
            Command::SymbolOutline,
            Command::SymbolDefinition,
            Command::SymbolTypeDefinition,
            Command::SymbolImplementation,
            Command::SymbolReferences,
            Command::GoLine,
            Command::GoBracket,
            Command::GoBack,
            Command::GoForward,
            Command::LogOpen,
            Command::LspRestart,
            Command::LspStop,
            Command::Quit,
        ] {
            assert_eq!(command.spec().command, command);
        }
    }

    /// Where the housekeeping goes. Restarting a language server and opening
    /// the log are not *reading*, and neither of them is worth a tab: they
    /// belong with obelus's own settings, which is where a reader looks when
    /// the tool rather than the code is the problem.
    #[test]
    fn housekeeping_is_grouped_with_obelus_itself() {
        assert_eq!(Command::LogOpen.group(), Group::Obelus);
        assert_eq!(Command::LspRestart.group(), Group::Obelus);
        assert_eq!(Command::LspStop.group(), Group::Obelus);
        assert_eq!(Command::ThemeSelect.group(), Group::Obelus);

        // And the two that are reading.
        assert_eq!(Command::BufferClose.group(), Group::Files);
        assert_eq!(Command::SymbolOutline.group(), Group::Code);
        assert_eq!(Command::GoBack.group(), Group::Code);

        // Few enough to walk. Every tab is somewhere a reader has to look
        // before deciding to type the name instead, so the count is the
        // point and not an accident.
        assert!(
            Group::ALL.len() <= 3,
            "the tabs have multiplied: {:?}",
            Group::ALL
        );
    }

    #[test]
    fn names_are_unique() {
        for (index, spec) in ALL.iter().enumerate() {
            assert!(
                !ALL[..index].iter().any(|earlier| earlier.name == spec.name),
                "two commands are both named {}",
                spec.name
            );
        }
    }
}
