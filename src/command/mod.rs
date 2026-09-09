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
    /// Choose a theme.
    ThemeSelect,
    /// Choose a command by name.
    CommandPalette,
    /// Leave obelus.
    Quit,
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
        command: Command::Quit,
        name: "app.quit",
        title: "Leave obelus",
    },
];

impl Command {
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
            Command::ThemeSelect,
            Command::CommandPalette,
            Command::Quit,
        ] {
            assert_eq!(command.spec().command, command);
        }
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
