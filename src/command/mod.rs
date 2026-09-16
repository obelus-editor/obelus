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
    /// Write the current file back to disk.
    FileSave,
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
    /// Offer what could be typed where the cursor is.
    SymbolComplete,
    /// What the language server says the place under the caret is.
    SymbolHover,
    /// What the language server offers to do about where the reader is.
    SymbolActions,
    /// Call the symbol under the cursor something else, everywhere.
    SymbolRename,
    /// Everything the language server says is wrong with this file.
    SymbolTroubles,
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
    /// Every commit that changed the file being read.
    HistoryFile,
    /// Every commit in the project.
    HistoryProject,
    /// The commit that wrote the line under the cursor.
    HistoryLine,
    /// Fold the run of lines the cursor is in, or unfold the one it is on.
    Fold,
    /// Fold every run in the file.
    FoldAll,
    /// Unfold everything that is folded.
    UnfoldAll,
    /// Open what changed here, in place, or close it again.
    GitHunk,
    /// Go to the change above the cursor.
    GitPrevious,
    /// Go to the change below the cursor.
    GitNext,
    /// Copy the selected text to the system clipboard.
    SelectionCopy,
    /// Copy the selection and take it out.
    SelectionCut,
    /// Move the line, or the selected lines, up one.
    LineUp,
    /// And down one.
    LineDown,
    /// Comment the line, or the selected lines, out -- or take the comment
    /// off where they all have one.
    CommentToggle,
    /// Put back what was last copied or cut.
    Paste,
    /// Put back what the last change took away.
    Undo,
    /// Do again what undo put back.
    Redo,
    /// Stop selecting.
    SelectionClear,
    /// Widen what is selected: the word, then whatever holds it.
    SelectionWiden,
    /// Select the whole file.
    SelectionAll,
    /// Return to where the last jump was made from.
    GoBack,
    /// Undo a jump back.
    GoForward,
    /// Talk to the active agent.
    AgentOpen,
    /// Count the lines of the tree, by language and by file.
    CountLines,
    /// Show what this tree means to come back to.
    TodoOpen,
    /// Write down something to come back to, here.
    TodoAdd,
    /// Read the settings file itself.
    ConfigFile,
    /// Open the settings.
    ConfigOpen,
    /// Open the settings this tree carries of its own.
    ConfigTree,
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
/// group per part of obelus would be nine of them, which is a worse way to
/// find `open-file` than its name is.
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
    /// There has to be somewhere with a caret in it.
    ///
    /// A file, or a box a reader is typing into -- a list's query, the
    /// settings' filter, a question on the status bar, a note, a message
    /// to an agent. Not the same as a file being open, which is what these
    /// asked for until a box could hold a selection: `ob some-directory`
    /// opens on a list with no file behind it, and copying out of the box
    /// a reader is typing in has nothing to do with whether there is one.
    ACaret,
    /// The open file has to be in a language obelus can parse.
    AKnownLanguage,
    /// The open file has to have a reading, or be showing one.
    APreview,
    /// The cursor has to be on a bracket.
    ABracket,
    /// The project has to be a repository with something in it.
    AHistory,
    /// The document has to have a change in it that can be put back.
    SomethingToUndo,
    /// And one that has been put back and can be made again.
    SomethingToRedo,
    /// The cursor has to be in a run of lines that folds, or on a folded
    /// one.
    AFoldHere,
    /// The file has to have something left to fold.
    ///
    /// Three conditions rather than one about folding, for the reason the
    /// hunks have three: a reader whose file is folded flat should not be
    /// offered a row whose whole answer is "everything already is".
    AFoldableFile,
    /// Something in the file has to be folded.
    SomethingFolded,
    /// The cursor has to be in something that changed since the last
    /// commit.
    AHunk,
    /// The file has to have a change above the cursor.
    AHunkBefore,
    /// The file has to have a change below the cursor.
    ///
    /// Two conditions rather than one about the file, for the same reason
    /// `go-back` and `go-forward` are two: a reader at the last change in a
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
    /// Something in the tree has to have changed since the last commit.
    AChangedFile,
    /// A language server has to have written to its log.
    AServerLog,
    /// The language has to have something to start a line comment with.
    ALineComment,
}

/// A command's name and description, for the palette to list and match on.
///
/// **A name is what the command does, verb first, words joined by hyphens**:
/// `open-file`, `go-to-definition`, `select-all`. Not a family and a member
/// (`file.open`, `selection.all`), which read backwards -- a reader reaching
/// for the palette knows what they want to *do* and types that word first,
/// and a list sorted or filtered by such a name buries the verb behind a
/// noun they have to guess. The family a command belongs to is
/// [`Command::group`], which is a tab on the palette and not part of any
/// name.
#[derive(Clone, Copy, Debug)]
pub struct CommandSpec {
    /// The command itself.
    pub command: Command,
    /// The name shown in the palette and matched against the query.
    pub name: &'static str,
    /// One line of description.
    pub title: &'static str,
}

/// Every command, in the order the palette lists them.
pub const ALL: &[CommandSpec] = &[
    CommandSpec {
        command: Command::FileOpen,
        name: "open-file",
        title: "Open a file",
    },
    CommandSpec {
        command: Command::FileChanged,
        name: "open-changed-file",
        title: "Open a file that has changed",
    },
    CommandSpec {
        command: Command::FileSave,
        name: "save-file",
        title: "Write this file back to disk",
    },
    CommandSpec {
        command: Command::FileReload,
        name: "reload-file",
        title: "Re-read this file from disk",
    },
    CommandSpec {
        command: Command::BufferList,
        name: "switch-file",
        title: "Switch to an open file",
    },
    CommandSpec {
        command: Command::BufferClose,
        name: "close-file",
        title: "Close this file",
    },
    CommandSpec {
        command: Command::PreviewToggle,
        name: "toggle-preview",
        title: "Show this file rendered, or stop",
    },
    CommandSpec {
        command: Command::ThemeSelect,
        name: "choose-theme",
        title: "Change the colours",
    },
    CommandSpec {
        command: Command::CommandPalette,
        name: "run-command",
        title: "Run a command by name",
    },
    CommandSpec {
        command: Command::SymbolMenu,
        name: "ask-about-symbol",
        title: "Ask about the symbol under the cursor",
    },
    // No default keys. The menu is the way in, and giving each of these a
    // chord would rebuild the one-key-per-question arrangement the menu
    // exists to replace. Binding one later needs no code: the menu and the
    // palette both read the key table.
    // Not usually reached by name: typing a letter asks by itself. This is
    // the way back for a reader who dismissed the panel and wants it again,
    // which is the one moment no letter is about to be typed.
    CommandSpec {
        command: Command::SymbolComplete,
        name: "complete-here",
        title: "What could be typed here",
    },
    CommandSpec {
        command: Command::SymbolHover,
        name: "describe-symbol",
        title: "What this is",
    },
    CommandSpec {
        command: Command::SymbolActions,
        name: "do-something-here",
        title: "What can be done here",
    },
    CommandSpec {
        command: Command::SymbolRename,
        name: "rename-symbol",
        title: "Rename this, everywhere it is",
    },
    CommandSpec {
        command: Command::SymbolTroubles,
        name: "show-problems",
        title: "What is wrong with this file",
    },
    CommandSpec {
        command: Command::SymbolOutline,
        name: "show-outline",
        title: "Everything this file defines",
    },
    CommandSpec {
        command: Command::SymbolDefinition,
        name: "go-to-definition",
        title: "Go to definition",
    },
    CommandSpec {
        command: Command::SymbolTypeDefinition,
        name: "go-to-type-definition",
        title: "Go to type definition",
    },
    CommandSpec {
        command: Command::SymbolImplementation,
        name: "go-to-implementation",
        title: "Go to implementation",
    },
    CommandSpec {
        command: Command::SymbolReferences,
        name: "find-references",
        title: "Find references",
    },
    CommandSpec {
        command: Command::SearchFile,
        name: "search-file",
        title: "Search this file",
    },
    CommandSpec {
        command: Command::SearchProject,
        name: "search-project",
        title: "Search every file",
    },
    CommandSpec {
        command: Command::SearchSymbols,
        name: "search-symbols",
        title: "Search the project's symbols",
    },
    CommandSpec {
        command: Command::GoLine,
        name: "go-to-line",
        title: "Go to a line by number",
    },
    CommandSpec {
        command: Command::GoBracket,
        name: "go-to-bracket",
        title: "Go to the matching bracket",
    },
    CommandSpec {
        command: Command::HistoryFile,
        name: "show-file-history",
        title: "Every commit that changed this file",
    },
    CommandSpec {
        command: Command::HistoryProject,
        name: "show-project-history",
        title: "Every commit in this project",
    },
    CommandSpec {
        command: Command::HistoryLine,
        name: "show-line-commit",
        title: "Open the commit that wrote this line",
    },
    CommandSpec {
        command: Command::Fold,
        name: "fold",
        title: "Fold what is here, or unfold it",
    },
    CommandSpec {
        command: Command::FoldAll,
        name: "fold-all",
        title: "Fold everything this file offers",
    },
    CommandSpec {
        command: Command::UnfoldAll,
        name: "unfold-all",
        title: "Unfold everything that is folded",
    },
    CommandSpec {
        command: Command::GitHunk,
        name: "show-change",
        title: "Show what changed here",
    },
    CommandSpec {
        command: Command::GitPrevious,
        name: "go-to-previous-change",
        title: "Go to the previous change",
    },
    CommandSpec {
        command: Command::GitNext,
        name: "go-to-next-change",
        title: "Go to the next change",
    },
    CommandSpec {
        command: Command::SelectionCopy,
        name: "copy-selection",
        title: "Copy the selection, or this line",
    },
    CommandSpec {
        command: Command::SelectionWiden,
        name: "widen-selection",
        title: "Widen what is selected",
    },
    CommandSpec {
        command: Command::SelectionAll,
        name: "select-all",
        title: "Select the whole file",
    },
    CommandSpec {
        command: Command::SelectionCut,
        name: "cut-selection",
        title: "Cut the selection, or this line",
    },
    CommandSpec {
        command: Command::LineUp,
        name: "move-line-up",
        title: "Move this line, or the selected ones, up",
    },
    CommandSpec {
        command: Command::LineDown,
        name: "move-line-down",
        title: "Move this line, or the selected ones, down",
    },
    CommandSpec {
        command: Command::CommentToggle,
        name: "toggle-comment",
        title: "Comment this line, or the selected ones, out",
    },
    CommandSpec {
        command: Command::Paste,
        name: "paste",
        title: "Put back what was last copied or cut",
    },
    CommandSpec {
        command: Command::Undo,
        name: "undo",
        title: "Put back what the last change took away",
    },
    CommandSpec {
        command: Command::Redo,
        name: "redo",
        title: "Do again what undo put back",
    },
    CommandSpec {
        command: Command::SelectionClear,
        name: "clear-selection",
        title: "Stop selecting",
    },
    CommandSpec {
        command: Command::GoBack,
        name: "go-back",
        title: "Go back to where you were",
    },
    CommandSpec {
        command: Command::GoForward,
        name: "go-forward",
        title: "Go forward again",
    },
    CommandSpec {
        command: Command::AgentOpen,
        name: "talk-to-agent",
        title: "Talk to the active agent",
    },
    CommandSpec {
        command: Command::CountLines,
        name: "count-lines",
        title: "How much code is here, by language and by file",
    },
    CommandSpec {
        command: Command::TodoOpen,
        name: "todo",
        title: "What this project means to come back to",
    },
    CommandSpec {
        command: Command::TodoAdd,
        name: "todo-add",
        title: "Write down something to come back to, at this line",
    },
    CommandSpec {
        command: Command::ConfigOpen,
        name: "open-settings",
        title: "Change obelus's settings",
    },
    CommandSpec {
        command: Command::ConfigTree,
        // "project" rather than "tree", which is obelus's own word for it
        // everywhere else: a name is what a reader types, and what they
        // will type for the settings a repository carries is the word every
        // other program has taught them.
        name: "open-project-settings",
        title: "Change the settings this project carries",
    },
    CommandSpec {
        command: Command::ConfigFile,
        name: "open-settings-file",
        title: "Open the settings file",
    },
    CommandSpec {
        command: Command::LogOpen,
        name: "open-log",
        title: "Open obelus's own log",
    },
    CommandSpec {
        command: Command::LogServers,
        name: "open-server-log",
        title: "Open the language servers' log",
    },
    CommandSpec {
        command: Command::LspRestart,
        name: "restart-server",
        title: "Restart the language server",
    },
    CommandSpec {
        command: Command::LspStop,
        name: "stop-server",
        title: "Stop the language server",
    },
    CommandSpec {
        command: Command::Quit,
        name: "quit",
        title: "Leave obelus",
    },
];

impl Command {
    /// Which group the command belongs to.
    ///
    /// Spelled out per command rather than read off the name: a name says
    /// what the command does and nothing about where it belongs, and moving
    /// a command between groups is not a reason to rename it.
    #[must_use]
    pub const fn group(self) -> Group {
        match self {
            Self::FileOpen
            | Self::FileChanged
            | Self::FileReload
            | Self::FileSave
            | Self::BufferList
            | Self::BufferClose
            | Self::PreviewToggle
            // A question about the tree of files, asked before any of them
            // is open: which makes it one of the files rather than one of
            // obelus's own housekeeping.
            | Self::CountLines
            | Self::TodoOpen
            | Self::TodoAdd => Group::Files,
            Self::SymbolMenu
            | Self::SymbolActions
            | Self::SymbolRename
            | Self::SymbolHover
            | Self::SymbolComplete
            | Self::SymbolTroubles
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
            | Self::HistoryFile
            | Self::HistoryProject
            | Self::HistoryLine
            | Self::Fold
            | Self::FoldAll
            | Self::UnfoldAll
            | Self::GitHunk
            | Self::GitPrevious
            | Self::GitNext
            | Self::SelectionCopy
            | Self::SelectionWiden
            | Self::SelectionCut
            | Self::LineUp
            | Self::LineDown
            | Self::CommentToggle
            | Self::Paste
            | Self::Undo
            | Self::Redo
            | Self::SelectionClear
            | Self::SelectionAll
            | Self::GoBack
            | Self::GoForward => Group::Code,
            Self::LspRestart
            | Self::LspStop
            | Self::AgentOpen
            | Self::ConfigOpen
            | Self::ConfigTree
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
    /// what is *not* conditional -- `ask-about-symbol` with no server is the
    /// thing that says why there is none, and `restart-server` with no
    /// server running is how you get one.
    #[must_use]
    pub const fn requires(self) -> Requires {
        match self {
            Self::SymbolDefinition
            | Self::SymbolTypeDefinition
            | Self::SymbolImplementation
            | Self::SymbolReferences => Requires::AnAnswer,
            Self::LspStop => Requires::ARunningServer,
            // Not a running server, for the reason `show-troubles` is not
            // one: "there is no server for this file", "it is not
            // installed", "it is still starting" are answers, and they are
            // answers a reader can act on. A key that does nothing at all
            // is indistinguishable from a key that is broken, and the
            // states these report are mostly the ones that go away by
            // themselves or by installing something.
            Self::SymbolComplete
            | Self::SymbolHover
            | Self::SymbolRename
            | Self::SymbolActions => Requires::AFileOpen,
            // Not a running server: a file with nothing wrong with it is
            // the answer this gives, and it is worth giving.
            Self::SymbolTroubles => Requires::AFileOpen,
            // A note is made *about* a line, so there has to be one.
            Self::TodoAdd => Requires::AFileOpen,
            Self::TodoOpen => Requires::Nothing,
            // Everything that acts on the file being read. With nothing
            // open, each of them is a key that reports why instead of doing
            // something.
            // Save is offered whether or not there is anything to write.
            // A reader who presses it on a file they have not touched has
            // asked a reasonable question, and the answer is that it is
            // already there rather than a key that does nothing.
            Self::FileSave => Requires::AFileOpen,
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
            // A file, and nothing more: the first step is the word under
            // the caret, which every file has and no grammar is needed for.
            Self::SelectionWiden => Requires::AFileOpen,
            // Not `AKnownLanguage`: a file obelus can parse can still have
            // nothing to fold on the line the reader is on, and a key that
            // is offered everywhere and works in places is worse than one
            // that says where it works.
            // Both, because both tabs of the view need the same thing: a
            // repository with a commit in it. Which of the two tabs can
            // answer is settled when the view opens, the way the search
            // settles its own.
            // All three want the same thing: a repository with a commit in
            // it. Whether *this line* has a commit behind it is the answer
            // rather than the question -- it takes a walk to find out, and
            // a row greyed until that walk lands is a row greyed for ever
            // for a reader who keeps the margin's names off, because then
            // nothing starts one. The command says what it found.
            Self::HistoryFile | Self::HistoryProject | Self::HistoryLine => Requires::AHistory,
            Self::Fold => Requires::AFoldHere,
            Self::FoldAll => Requires::AFoldableFile,
            Self::UnfoldAll => Requires::SomethingFolded,
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
            Self::FileChanged => Requires::AChangedFile,
            Self::GitHunk => Requires::AHunk,
            Self::GitPrevious => Requires::AHunkBefore,
            Self::GitNext => Requires::AHunkAfter,
            Self::SelectionClear => Requires::ASelection,
            Self::LineUp | Self::LineDown => Requires::AFileOpen,
            // A language with only block comments has nothing to put in
            // front of a line, and saying so is better than a key that does
            // nothing on CSS and works on Rust.
            Self::CommentToggle => Requires::ALineComment,
            // Not a selection: with nothing selected these are about the
            // line the cursor is on -- or the whole of the box -- which is
            // what a reader means by them far more often than they mean
            // "nothing". And wherever there is a caret rather than
            // wherever there is a file: what holds what they have hold of
            // may be a box.
            Self::SelectionCopy | Self::SelectionCut | Self::Paste => Requires::ACaret,
            // Not "is there anything to paste": obelus's own store knows
            // without being asked, and an external clipboard has to be run
            // to find out. A requirement that cannot be answered cheaply
            // becomes a key that does nothing and a row grey for ever,
            // because `offers` gates both. The command says what it found.
            Self::Undo => Requires::SomethingToUndo,
            Self::Redo => Requires::SomethingToRedo,
            // Not a selection: this is how one is made. A file, though --
            // there is nothing to take all of otherwise.
            Self::SelectionAll => Requires::AFileOpen,
            Self::GoBack => Requires::SomewhereBack,
            Self::GoForward => Requires::SomewhereForward,
            // `ask-about-symbol` needs nothing: with no server it is the thing
            // that says why there is none. Nor does `restart-server`, which is
            // how a stopped or dead server is started. `open-file`,
            // `choose-theme`, `log.open` and the palette itself work with
            // nothing open at all.
            Self::FileOpen
            | Self::ThemeSelect
            | Self::CommandPalette
            | Self::SymbolMenu
            | Self::AgentOpen
            | Self::ConfigOpen
            | Self::ConfigTree
            | Self::ConfigFile
            | Self::LogOpen
            | Self::LspRestart
            // The tree is always there to be counted, and a tree with
            // nothing in it is an answer as well: what it says is that
            // there is nothing here.
            | Self::CountLines
            | Self::Quit => Requires::Nothing,
            // A file of its own that only exists once a server has said
            // something, which on a file in a language obelus has no
            // server for is never. obelus's own log is not this: it is
            // there from the first line it writes, and if it is not, the
            // command saying so is the only way a reader learns that
            // logging failed.
            Self::LogServers => Requires::AServerLog,
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

    /// The name, for logs and the palette.
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
            Command::FileSave,
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

    /// A name is what the command does, verb first, words joined by
    /// hyphens. The shape is what can be checked -- that it is one word of
    /// lowercase letters and hyphens, with no dots and no camel case -- and
    /// it is worth checking, because the one that gets this wrong is the
    /// next command added beside thirty-six that do not.
    #[test]
    fn names_are_verbs_in_lowercase_words() {
        for spec in ALL {
            assert!(
                spec.name
                    .chars()
                    .all(|character| character.is_ascii_lowercase() || character == '-'),
                "{} is not lowercase words joined by hyphens",
                spec.name
            );
            assert!(
                !spec.name.starts_with('-') && !spec.name.ends_with('-'),
                "{} starts or ends with a hyphen",
                spec.name
            );
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
