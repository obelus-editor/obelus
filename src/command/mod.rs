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
    /// Open one of the files that have changed since the last commit.
    FileChanged,
    /// Re-read the current file from disk and reparse what changed.
    FileReload,
    /// Choose among the files already open.
    BufferList,
    /// Stop showing the current file.
    BufferClose,
    /// Show this file as rendered markdown, or stop.
    PreviewToggle,
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
    /// Search the file being read.
    SearchFile,
    /// Search every file under the working directory.
    SearchProject,
    /// Search the names the language server knows.
    SearchSymbols,
    /// Go to a line by number.
    GoLine,
    /// Go to the bracket that matches the one under the cursor.
    GoBracket,
    /// Open what changed here, in place, or close it again.
    GitHunk,
    /// Show who last changed each line, or stop.
    /// Go to the change above the cursor.
    GitPrevious,
    /// Go to the change below the cursor.
    GitNext,
    /// Copy the selected text to the system clipboard.
    SelectionCopy,
    /// Stop selecting.
    SelectionClear,
    /// Select the whole file.
    SelectionAll,
    /// Return to where the last jump was made from.
    GoBack,
    /// Undo a jump back.
    GoForward,
    /// Talk to the active agent.
    AgentOpen,
    /// Change how the agent it is talking to is working.
    AgentSettings,
    /// Read the settings file itself.
    ConfigFile,
    /// Open the settings.
    ConfigOpen,
    /// Open the file obelus logs to.
    LogOpen,
    /// Open the file the language servers' side is logged to.
    LogServers,
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

/// What has to be true before a command is worth offering.
///
/// The palette leaves out anything that cannot do its job right now: a row
/// that silently fails is worse than a row that is not there, and a list of
/// twenty commands of which six do nothing here is a list nobody trusts.
///
/// One variant per *condition*, not per command, so the answers live in one
/// place: [`Command::requires`] says which condition each command is under,
/// and the application turns each condition into a yes or no from its own
/// state. Both matches are exhaustive with no wildcard arm, so a new command
/// has to declare a condition and a new condition has to be answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Requires {
    /// Always available.
    Nothing,
    /// Some file has to be open.
    AFileOpen,
    /// The open file has to be in a language obelus can parse.
    AKnownLanguage,
    /// The open file has to have a reading, or be showing one.
    APreview,
    /// The cursor has to be on a bracket.
    ABracket,
    /// The cursor has to be in something that changed since the last
    /// commit.
    AHunk,
    /// The file has to have a change above the cursor.
    AHunkBefore,
    /// The file has to have a change below the cursor.
    ///
    /// Two conditions rather than one about the file, for the same reason
    /// `go.back` and `go.forward` are two: a reader at the last change in a
    /// file should not be offered a row that answers "no more changes".
    AHunkAfter,
    /// Something has to be selected.
    ASelection,
    /// The history has to have somewhere behind the reader.
    SomewhereBack,
    /// And somewhere in front.
    SomewhereForward,
    /// A language server has to be running for this file.
    ARunningServer,
    /// The running server has to say it answers this question.
    AnAnswer,
    /// The agent being talked to has to offer something to change.
    AnAgentSetting,
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
        command: Command::FileChanged,
        name: "file.changed",
        title: "Open a file that has changed",
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
        command: Command::PreviewToggle,
        name: "preview.toggle",
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
        command: Command::SearchFile,
        name: "search.file",
        title: "Search this file",
    },
    CommandSpec {
        command: Command::SearchProject,
        name: "search.project",
        title: "Search every file",
    },
    CommandSpec {
        command: Command::SearchSymbols,
        name: "search.symbols",
        title: "Search the project's symbols",
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
        command: Command::GitHunk,
        name: "git.hunk",
        title: "Show what changed here",
    },
    CommandSpec {
        command: Command::GitPrevious,
        name: "git.previous",
        title: "Go to the previous change",
    },
    CommandSpec {
        command: Command::GitNext,
        name: "git.next",
        title: "Go to the next change",
    },
    CommandSpec {
        command: Command::SelectionCopy,
        name: "selection.copy",
        title: "Copy the selected text",
    },
    CommandSpec {
        command: Command::SelectionAll,
        name: "selection.all",
        title: "Select the whole file",
    },
    CommandSpec {
        command: Command::SelectionClear,
        name: "selection.clear",
        title: "Stop selecting",
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
        command: Command::AgentOpen,
        name: "agent.open",
        title: "Talk to the active agent",
    },
    CommandSpec {
        command: Command::AgentSettings,
        name: "agent.settings",
        title: "Change how the agent is working",
    },
    CommandSpec {
        command: Command::ConfigOpen,
        name: "config.open",
        title: "Change obelus's settings",
    },
    CommandSpec {
        command: Command::ConfigFile,
        name: "config.file",
        title: "Open the settings file",
    },
    CommandSpec {
        command: Command::LogOpen,
        name: "log.obelus",
        title: "Open obelus's own log",
    },
    CommandSpec {
        command: Command::LogServers,
        name: "log.servers",
        title: "Open the language servers' log",
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
            | Self::FileChanged
            | Self::FileReload
            | Self::BufferList
            | Self::BufferClose
            | Self::PreviewToggle => Group::Files,
            Self::SymbolMenu
            | Self::SymbolOutline
            | Self::SymbolDefinition
            | Self::SymbolTypeDefinition
            | Self::SymbolImplementation
            | Self::SymbolReferences
            | Self::SearchFile
            | Self::SearchProject
            | Self::SearchSymbols
            | Self::GoLine
            | Self::GoBracket
            | Self::GitHunk
            | Self::GitPrevious
            | Self::GitNext
            | Self::SelectionCopy
            | Self::SelectionClear
            | Self::SelectionAll
            | Self::GoBack
            | Self::GoForward => Group::Code,
            Self::LspRestart
            | Self::LspStop
            | Self::AgentOpen
            | Self::AgentSettings
            | Self::ConfigOpen
            | Self::ConfigFile
            | Self::LogOpen
            | Self::LogServers
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
            // Everything that acts on the file being read. With nothing
            // open, each of them is a key that reports why instead of doing
            // something.
            Self::FileReload | Self::BufferClose | Self::BufferList | Self::GoLine => {
                Requires::AFileOpen
            }
            // An outline comes from the syntax tree when no server will
            // answer, so what it needs is a language obelus can parse.
            Self::SymbolOutline => Requires::AKnownLanguage,
            // Both ways: it turns the rendering on for a markdown file and
            // off again for one already showing as markdown.
            Self::PreviewToggle => Requires::APreview,
            Self::GoBracket => Requires::ABracket,
            // One scope needs a file, one needs nothing but the tree, and
            // one needs a server -- but all three open the same view, whose
            // other tabs are a left or a right away. What each key requires
            // is what the tab it lands on can answer.
            Self::SearchFile => Requires::AFileOpen,
            Self::SearchProject => Requires::Nothing,
            Self::SearchSymbols => Requires::ARunningServer,
            // Both ways, and it needs only a file: turning the names off
            // is exactly what a reader with no repository does not need to
            // do, but a file in one that has never been committed still
            // gets an empty blame, which is an answer.
            // Whether anything has changed is a walk of the whole tree with
            // every ignore rule applied, and the palette would pay for it
            // every time it opened. So this row is always choosable and the
            // command says "nothing has changed" when that is the answer --
            // the one place a row is allowed to report why it did nothing,
            // and what buys the exception is the cost of the question.
            Self::FileChanged => Requires::Nothing,
            Self::GitHunk => Requires::AHunk,
            Self::GitPrevious => Requires::AHunkBefore,
            Self::GitNext => Requires::AHunkAfter,
            Self::SelectionCopy | Self::SelectionClear => Requires::ASelection,
            // Not a selection: this is how one is made. A file, though --
            // there is nothing to take all of otherwise.
            Self::SelectionAll => Requires::AFileOpen,
            Self::GoBack => Requires::SomewhereBack,
            Self::GoForward => Requires::SomewhereForward,
            Self::AgentSettings => Requires::AnAgentSetting,
            // `symbol.menu` needs nothing: with no server it is the thing
            // that says why there is none. Nor does `lsp.restart`, which is
            // how a stopped or dead server is started. `file.open`,
            // `theme.select`, `log.open` and the palette itself work with
            // nothing open at all.
            Self::FileOpen
            | Self::ThemeSelect
            | Self::CommandPalette
            | Self::SymbolMenu
            | Self::AgentOpen
            | Self::ConfigOpen
            | Self::ConfigFile
            | Self::LogOpen
            | Self::LogServers
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

/// The command a name names, if it names one.
///
/// The other direction of [`Command::name`], for the config file: what is
/// written down is the name, because a number would break the moment the
/// table was reordered and an enum's spelling is not the reader's business.
#[must_use]
pub fn by_name(name: &str) -> Option<Command> {
    ALL.iter()
        .find(|spec| spec.name == name)
        .map(|spec| spec.command)
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
            Command::FileChanged,
            Command::FileReload,
            Command::BufferList,
            Command::BufferClose,
            Command::PreviewToggle,
            Command::ThemeSelect,
            Command::CommandPalette,
            Command::SymbolMenu,
            Command::SymbolOutline,
            Command::SymbolDefinition,
            Command::SymbolTypeDefinition,
            Command::SymbolImplementation,
            Command::SymbolReferences,
            Command::SearchFile,
            Command::SearchProject,
            Command::SearchSymbols,
            Command::GoLine,
            Command::GoBracket,
            Command::GitHunk,
            Command::GitPrevious,
            Command::GitNext,
            Command::SelectionCopy,
            Command::SelectionClear,
            Command::SelectionAll,
            Command::GoBack,
            Command::GoForward,
            Command::ConfigOpen,
            Command::ConfigFile,
            Command::LogOpen,
            Command::LogServers,
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
