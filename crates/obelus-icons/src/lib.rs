//! Glyphs from a Nerd Font, and the one switch that turns them off.
//!
//! These live in the Unicode private use area, so they exist only if the
//! reader's terminal font has them.
//!
//! **Whether it has them cannot be detected.** A terminal works out how many
//! columns a character takes from Unicode's width tables, not from the font,
//! so a codepoint the font has no glyph for still advances one column and
//! draws a box. Asking the terminal where the cursor ended up therefore
//! measures its width table and says nothing about the font. There is no
//! in-band query for "does your font have this codepoint", and the terminal's
//! own identity says nothing either: plenty of people run kitty with a plain
//! font and xterm with a patched one. Every tool that draws these ends up
//! asking the reader instead — lazygit's `nerdFontsVersion`, eza's `--icons`,
//! `vim.g.have_nerd_font`.
//!
//! So Obelus has one switch, [`NERD_FONT`], and everything that draws a glyph
//! has a fallback that reads correctly without one. There is nowhere to
//! configure it yet, which makes it the fourth thing wanting a configuration
//! file.
//!
//! Every codepoint here was checked against a patched font's own tables
//! rather than taken from a chart, because a wrong one is indistinguishable
//! from a missing font.
//!
//! One rule holds everywhere: **leave a blank column after a glyph.** A Nerd
//! Font's non-`Mono` variants draw these two cells wide while the terminal
//! allocates one, so the glyph bleeds to the right, and what it bleeds into
//! had better be nothing.

use std::path::Path;

/// Whether Obelus may draw glyphs from a Nerd Font.
///
/// One switch for the whole program, because whether the font has them is a
/// fact about the reader's terminal rather than about any one view -- and
/// every reader of it has a fallback that reads correctly without one.
///
/// Global state, which is unlike the rest of Obelus: the alternative is
/// threading a flag into every function that draws a glyph, including the
/// pure ones that turn a name into a character. It is written once at
/// startup and once per change of the setting, and read while drawing.
///
/// Off until the settings say otherwise, which is the setting's own default.
static NERD_FONT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Turns the glyphs on or off.
pub fn use_glyphs(on: bool) {
    NERD_FONT.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Whether glyphs are in use.
#[must_use]
pub fn enabled() -> bool {
    NERD_FONT.load(std::sync::atomic::Ordering::Relaxed)
}

/// The glyphs for the modifiers, for whatever shows a binding.
///
/// Only these three. The keys that are words had glyphs too, and a chord
/// of two pictures -- `End` as a bar with an arrow at it, behind the one
/// for control -- was a chord nobody could read; the words are what is on
/// the keyboard. A modifier is printed as its picture on most of them, and
/// is the part of a chord that repeats down a column of keys, so it is
/// where a glyph saves columns and costs nothing to read.
pub mod key {
    /// `ctrl`.
    pub const CONTROL: char = '\u{f0634}';
    /// `alt`.
    pub const ALT: char = '\u{f0635}';
    /// `shift`.
    pub const SHIFT: char = '\u{f0636}';
}

/// The glyphs the views use for things that are not files.
pub mod ui {
    /// A picture the reader put in a message.
    ///
    /// Checked against the installed font's own charset rather than taken
    /// from a chart, like every other codepoint here.
    pub const PICTURE: char = '\u{f03e}';
    /// One commit, as a row of a history.
    pub const COMMIT: char = '\u{f0718}';
    /// A branch, for a name that points at a commit.
    pub const BRANCH: char = '\u{f062c}';
    /// A branch as a remote last had it.
    pub const REMOTE: char = '\u{f02a1}';
    /// A tag.
    pub const TAG: char = '\u{f04fb}';
    /// A prompt. Every picker filters by typing, so all of them get this
    /// one -- a magnifier, because what typing does there is *find*.
    pub const PROMPT: char = '\u{f0349}';
    /// The row a message to an agent is typed on.
    ///
    /// Not the magnifier: typing here is not looking for anything, it is
    /// saying something. A chevron is what a line you are about to send has
    /// in front of it everywhere else.
    pub const SAY: char = '\u{f0142}';
    /// A language server that has answered its handshake.
    pub const SERVER_READY: char = '\u{f0318}';
    /// One that has been started and has not answered yet.
    pub const SERVER_STARTING: char = '\u{f031a}';
    /// One whose process is gone.
    pub const SERVER_GONE: char = '\u{f0319}';
    /// A document with changes that are not on disk.
    pub const UNSAVED: char = '\u{f0766}';
    /// A file that can no longer be read from disk.
    pub const STALE: char = '\u{f0a4b}';
    /// A note to come back to, and one that has been come back to.
    ///
    /// The same box in both, with a mark in the second: a pair that changed
    /// shape -- a square for one and a circle for the other -- would put a
    /// jog in the one column of this page a reader reads straight down.
    pub const TODO: char = '\u{f0131}';
    /// The same box, with the mark in it.
    pub const TODO_DONE: char = '\u{f0132}';
    /// A conversation another Obelus has open, which this one may not
    /// enter.
    ///
    /// A lock, because that is what it is: the claim is a lock the system
    /// holds, and the reader's question about the row is whether the key
    /// will work on it.
    pub const ELSEWHERE: char = '\u{f033e}';
    /// A worktree another Obelus has open in a window, which choosing goes
    /// to rather than opening a second.
    ///
    /// A window, because that is what is there to go to: the lock beside a
    /// conversation is a refusal, and this row is the opposite.
    pub const WINDOW: char = '\u{f08c6}';
    /// A directory, on a row whose children are what it holds.
    ///
    /// The plain folder rather than [`TREE`]: that one is the whole of what
    /// was counted, and a directory inside it is not that.
    pub const DIRECTORY: char = '\u{f07b}';
    /// A set of colours.
    pub const THEME: char = '\u{f03d8}';
    /// The project Obelus is reading, taken as a whole.
    ///
    /// A folder, which is what a tree of files is drawn as everywhere. It
    /// sits in the glyph column of the row the languages hang from, so the
    /// line down to them has something above it.
    pub const TREE: char = '\u{f024b}';
    /// The reader, in a conversation with an agent.
    pub const READER: char = '\u{f0013}';
    /// The agent answering.
    pub const AGENT: char = '\u{f06a9}';
    /// The agent thinking, which agents send apart from their answer.
    pub const THOUGHT: char = '\u{f07f7}';
    /// The agent using a tool.
    pub const TOOL: char = '\u{f05b7}';
    /// Obelus's own remark in a conversation.
    pub const NOTE: char = '\u{f02fd}';
    /// A tool call that has not started.
    pub const WAITING: char = '\u{f01d8}';
    /// One that is running.
    pub const RUNNING: char = '\u{f0772}';
    /// One that finished.
    pub const DONE: char = '\u{f012c}';
    /// One that did not.
    pub const BROKEN: char = '\u{f0159}';
    /// Somewhere on the web the agent wants the reader to go.
    pub const AWAY: char = '\u{f03cc}';
    /// Not going there.
    pub const STAYING: char = '\u{f0156}';
}

/// The glyph for one answer to a permission request.
///
/// By the protocol's own four kinds rather than by the agent's name for the
/// option: the name is the agent's words and can be anything, and what a
/// reader is looking for in that list is which one is the yes.
#[must_use]
pub fn for_permission(kind: &str) -> char {
    match kind {
        "allow_once" => '\u{f012c}',
        "allow_always" => '\u{f05e0}',
        "reject_once" => '\u{f0156}',
        "reject_always" => '\u{f0159}',
        // A kind Obelus has not heard of. The name says what it does; the
        // glyph says only that it is an answer.
        _ => '\u{f0450}',
    }
}

/// A tool call, by the sort of thing the agent said it is doing.
///
/// The protocol's own list of kinds, which is the cheapest signal a turn
/// carries: a reader scanning what an agent did is looking for whether it
/// *changed* anything, and that is a glyph rather than a sentence.
#[must_use]
pub fn for_tool(kind: &str) -> char {
    match kind {
        // A file, and a file with a pencil on it.
        "read" => '\u{f0219}',
        "edit" => '\u{f0cb6}',
        "delete" => '\u{f01b4}',
        "move" => '\u{f0552}',
        // The same magnifier Obelus searches with everywhere else.
        "search" => '\u{f0349}',
        // A terminal, because that is what running something is.
        "execute" => '\u{f018d}',
        // The glyph the transcript draws thinking with: this is the agent
        // doing it as a tool rather than out loud.
        "think" => ui::THOUGHT,
        "fetch" => '\u{f059f}',
        "switch_mode" => '\u{f04e6}',
        // A kind Obelus has not heard of. The title says what it is; the
        // glyph says only that the agent is doing something.
        _ => ui::TOOL,
    }
}

/// The glyph for a command.
///
/// Per command rather than per family, so a glyph can mean what the command
/// means rather than what its neighbourhood does: "go back" and "go forward"
/// are the same neighbourhood and opposite actions, and one picture for both
/// says nothing. Exhaustive with no fallback -- a new command has to be
/// given a picture, which is a line of work rather than a decision, and the
/// alternative is a family guessed from a name that no longer has a family
/// in it.
#[must_use]
pub fn for_command(command: obelus_command::Command) -> char {
    use obelus_command::Command;

    match command {
        // Finding a file is what the picker does; the folder is closed until
        // then.
        Command::FileOpen => '\u{f021e}',
        // A folder with a pencil on it: the files being worked on.
        Command::FileChanged => '\u{f08de}',
        // A page with a plus on it. A *page* and not a folder, which is
        // what the two commands either side of it carry: those choose
        // among files that are there, and this one makes the file.
        Command::FileNew => '\u{f0224}',
        Command::FileReload => '\u{f0450}',
        // A floppy disk, which nobody has seen for twenty years and
        // everybody still reads as save.
        Command::FileSave => '\u{f0193}',
        // A folder with an arrow off it: what this does to a file is put
        // it somewhere, and calling it something else is putting it
        // somewhere with a different name.
        Command::FileRename => '\u{f0770}',
        Command::DocumentList => '\u{f0222}',
        // The project taken as a whole, which is what a worktree is: the
        // same folder the counts hang their languages from.
        Command::WorktreeList => ui::TREE,
        Command::DocumentClose => '\u{f0b98}',
        Command::PreviewToggle => '\u{f0354}',
        Command::ThemeSelect => '\u{f03d8}',
        Command::CommandPalette => '\u{f018d}',
        // The menu is the questions themselves; each question is what it
        // does. A definition is a place to land on, a type is a shape, an
        // implementation is what hangs below the thing, and references are a
        // search.
        Command::SymbolMenu => '\u{f0174}',
        // A lightbulb: what could be typed here is the one thing Obelus
        // offers rather than answers.
        Command::SymbolComplete => '\u{f0335}',
        // `md-auto_fix`: the wand, which is what every editor draws for
        // the things a server offers to do.
        Command::CodeActions => '\u{f0068}',
        // `md-rename_box`: the one command that changes a name.
        Command::SymbolRename => '\u{f0455}',
        // `md-tooltip`: what a thing is, said beside it.
        Command::SymbolHover => '\u{f0523}',
        // `md-function`: the same picture the outline draws for a
        // function, because it is the same subject -- a function and what
        // goes into it. It is not a repeat in any one list: the outline's
        // glyphs and the palette's are two sets, and `search-symbols` has
        // the variant of this one.
        Command::SymbolSignature => '\u{f0295}',
        // `md-alert_circle_outline`: what is wrong with the file.
        Command::SymbolTroubles => '\u{f05d6}',
        // `md-arrow_up` and `md-arrow_down`: plain arrows, where the
        // changes step with chevrons. Two pairs that both mean previous
        // and next need telling apart on a row, and every command here
        // has its own glyph.
        Command::SymbolTroublePrevious => '\u{f005d}',
        Command::SymbolTroubleNext => '\u{f0045}',
        // A list of what is in something, which is what an outline is.
        Command::SymbolOutline => '\u{f0279}',
        Command::SymbolDefinition => '\u{f04fe}',
        Command::SymbolTypeDefinition => '\u{f0169}',
        Command::SymbolImplementation => '\u{f04aa}',
        Command::SymbolReferences => '\u{f13b8}',
        // A tree, which is what the answer is: every other glyph in this
        // family says "a place", and this one says "a shape".
        Command::SymbolCalls => '\u{f0645}',
        // One view at three radii, so the glyphs say *where* rather than
        // repeating "search": the plain magnifier for the file in front of
        // the reader, folders for the tree, and a name in code for what a
        // server knows.
        Command::SearchFile => '\u{f0349}',
        Command::SearchProject => '\u{f0253}',
        Command::SearchSymbols => '\u{f0871}',
        // A number, which is what this one asks for.
        Command::GoLine => '\u{f03a0}',
        Command::GoBracket => '\u{f0172}',
        // The branching lines every git tool uses for itself.
        // A chevron folded down onto itself, which is the shape the mark
        // in the fold column has.
        // A history is a line of commits; the file's is that line with a
        // document on it.
        // A ticked box, which is what the view is a list of.
        Command::TodoOpen => '\u{f0856}',
        Command::TodoAdd => '\u{f0417}',
        Command::HistoryFile => '\u{f0214}',
        Command::HistoryProject => '\u{f02a1}',
        // One commit, which is what a line has.
        Command::HistoryLine => '\u{f0aa0}',
        Command::Fold => '\u{f0374}',
        Command::FoldAll => '\u{f0376}',
        Command::UnfoldAll => '\u{f0377}',
        Command::GitHunk => '\u{f02a2}',
        // Arrows, because these two move the reader: the diff is what they
        // step through, and the git glyph is already on the command that
        // opens one.
        Command::GitPrevious => '\u{f0143}',
        Command::GitNext => '\u{f0140}',
        // Two sheets of paper, which is what copying is everywhere.
        // Scissors, a clipboard with an arrow in, and the two arrows that
        // curl back on themselves.
        Command::SelectionCut => '\u{f0190}',
        Command::Paste => '\u{f0192}',
        // A line with an arrow off the top of it, and one off the bottom:
        // what moves is the line, not the reader.
        Command::LineUp => '\u{f05ce}',
        Command::LineDown => '\u{f05cd}',
        // Two slashes, which is what a comment starts with in most of the
        // languages this table knows.
        Command::CommentToggle => '\u{f0182}',
        // A caret on a line, which is what the mode is about: where the
        // next character goes.
        Command::ReplaceToggle => '\u{f0379}',
        Command::Undo => '\u{f054c}',
        Command::Redo => '\u{f044e}',
        Command::SelectionCopy => '\u{f018f}',
        // The dotted rectangle every program draws for "all of it".
        // `md-arrow_expand_horizontal`: what is selected, made wider.
        Command::SelectionWiden => '\u{f0616}',
        Command::SelectionAll => '\u{f0486}',
        Command::SelectionClear => '\u{f0156}',
        // The hooked arrows every browser uses, which is what the jump list
        // is.
        Command::GoBack => '\u{f17b3}',
        Command::GoForward => '\u{f17b7}',
        Command::ConversationNew => ui::AGENT,
        // `md-forum_outline`: several of them, which is what this is a
        // list of. Not the one [`ui::AGENT`] wears -- two rows with one
        // picture say less than one, and the difference between these two
        // rows is exactly one conversation against all of them.
        Command::ConversationSelect => '\u{f0286}',
        // A bar chart, which is what the view itself draws: a column per
        // language against the biggest one.
        Command::CountLines => '\u{f0128}',
        // Sliders, because a cog is what everything else in this list would
        // fall back to and two rows with the same picture say less than one.
        Command::ConfigOpen => '\u{f062e}',
        // A link, and the link broken: what the pair of them does between
        // this window and the chat. Not the plugs the server's mark is
        // drawn with, which say something else on the same row.
        Command::RemoteConnect => '\u{f0337}',
        Command::RemoteDisconnect => '\u{f0338}',
        // A folder with a cog on it: the same settings, belonging to the
        // tree rather than to the reader -- the folder is what says which.
        Command::ConfigProject => '\u{f0ee5}',
        // A file with a cog on it: the settings themselves, as the file
        // they are kept in.
        Command::ConfigFile => '\u{f107b}',
        Command::LogOpen => '\u{f09ed}',
        // A server on a wire, because that is whose words are in that one
        // and how they arrive: the handshake, the requests, and whatever it
        // wrote to its stderr.
        Command::LogServers => '\u{f048d}',
        Command::LspRestart => '\u{f0709}',
        Command::LspStop => '\u{f04db}',
        // Leaving Obelus, not switching a machine off.
        Command::Quit => '\u{f0206}',
    }
}

/// The glyph for an agent, by the registry's own name for it.
///
/// The registry ships an icon for every agent, but they are monochrome
/// 16×16 SVGs drawn with `currentColor` -- glyphs, in other words, and a
/// terminal cannot be handed one. What it *can* be handed is a codepoint,
/// and a patched font already carries the marks for the agents a reader is
/// most likely to reach for.
///
/// Everything else gets the robot, which says "an agent" and claims nothing
/// about which. A wrong mark would be worse than a general one: these are
/// brands, and a reader scanning for one recognises it or does not.
#[must_use]
pub fn for_agent(id: &str) -> char {
    match id {
        "claude-acp" => '\u{ec82}',
        "codex-acp" => '\u{ec81}',
        "github-copilot-cli" => '\u{ec1e}',
        "gemini" | "antigravity-acp" => '\u{f02ad}',
        _ => '\u{f06a9}',
    }
}

/// The glyph for a kind of thing in the code.
///
/// An outline row and a completion candidate both name something a file
/// defines, so both wear this. Which means the fallback matters: it used to
/// be the *function* glyph, so a variable, a constructor and a string
/// literal were all drawn as functions -- a picture that is not vague but
/// wrong, and a list of them reads as a list of functions.
///
/// The pictures are the conventional ones as far as there are conventions:
/// a type is a box, a constant is locked, a variable is `x`. Every
/// codepoint here was read out of a patched font's own tables rather than
/// taken from a chart, because a wrong one looks exactly like a missing
/// font.
#[must_use]
pub fn for_kind(kind: obelus_text::kind::SyntaxKind) -> char {
    use obelus_text::kind::SyntaxKind;
    match kind {
        // `md-function`.
        SyntaxKind::Function => '\u{f0295}',
        // `md-cube_outline`: a type is a box, which is the one picture
        // every editor draws for one. It used to be `md-code_braces`,
        // which says "some code" and not "a type".
        SyntaxKind::Type => '\u{f01a7}',
        // `md-cube`, filled: what a constructor makes is one of those.
        SyntaxKind::Constructor => '\u{f01a6}',
        // `md-lock`: a value that does not change.
        SyntaxKind::Constant => '\u{f033e}',
        // `md-tag`: a field is a named slot in something.
        SyntaxKind::Property => '\u{f04f9}',
        // `md-variable`.
        SyntaxKind::Variable => '\u{f0ae7}',
        // `md-sitemap`: a module or namespace, which is what an outline
        // uses this kind for.
        SyntaxKind::Keyword => '\u{f04aa}',
        // `md-format_quote_close`.
        SyntaxKind::String => '\u{f027e}',
        // `md-numeric`.
        SyntaxKind::Number => '\u{f03a0}',
        // `md-toggle_switch`: one of two.
        SyntaxKind::Boolean => '\u{f0521}',
        // `md-dots_horizontal`: something with a name, and nothing said
        // about what it is. Honest, where a borrowed picture is not.
        SyntaxKind::Attribute
        | SyntaxKind::Comment
        | SyntaxKind::Escape
        | SyntaxKind::Label
        | SyntaxKind::Operator
        | SyntaxKind::Punctuation
        | SyntaxKind::Error
        | SyntaxKind::Warning => '\u{f01d8}',
    }
}

/// The glyph for how bad a problem is, by the kind it is coloured as.
///
/// A table of its own and not [`for_kind`]'s: that one is for what a
/// *name* is, and an error is not a kind of name. Asked there, every row of
/// a list of problems wore the mark for "something Obelus could not name",
/// which in front of a sentence reads as the sentence having been cut.
///
/// By the kind rather than by a severity, because the severity is a
/// server's word and this crate draws for everything: `Severity::kind` is
/// how a problem says which colour it is, and the picture goes with the
/// colour. The two quieter ones share it as they share the colour.
#[must_use]
pub fn for_problem(kind: obelus_text::kind::SyntaxKind) -> char {
    use obelus_text::kind::SyntaxKind;
    match kind {
        // `md-close_circle`: what stops the thing it is about.
        SyntaxKind::Error => '\u{f0159}',
        // `md-alert`: what goes on, and should not.
        SyntaxKind::Warning => '\u{f0026}',
        // `md-information`: a remark.
        _ => '\u{f02fc}',
    }
}

/// What a file with nothing more specific gets.
const FILE: char = '\u{f15b}';

/// The glyph for a language, looked up by one of its file extensions.
///
/// An extension rather than a name, so there is one table rather than two:
/// what a Rust file looks like is already written down once, and a second
/// list mapping "Rust" to the same glyph is a list that can disagree with
/// it. Whoever knows the language hands over an extension it is written in
/// -- which for the line counts is tokei, the crate that already holds that
/// mapping for two hundred languages.
#[must_use]
pub fn for_extension(extension: &str) -> char {
    by_extension(extension).unwrap_or(FILE)
}

/// The glyph for a path.
#[must_use]
pub fn for_path(path: &Path) -> char {
    // By whole name first: a dotfile like `.gitignore` has no extension as far
    // as `Path` is concerned — the dot makes it all stem.
    if let Some(name) = path.file_name().and_then(|name| name.to_str())
        && let Some(glyph) = by_name(name)
    {
        return glyph;
    }
    path.extension()
        .and_then(|extension| extension.to_str())
        .and_then(by_extension)
        .unwrap_or(FILE)
}

fn by_name(name: &str) -> Option<char> {
    match name {
        ".gitignore" | ".gitattributes" | ".gitmodules" => Some('\u{e702}'),
        "Makefile" | "Dockerfile" | "Containerfile" => Some('\u{e795}'),
        "LICENSE" | "LICENCE" | "COPYING" => Some('\u{f15c}'),
        _ => None,
    }
}

fn by_extension(extension: &str) -> Option<char> {
    match extension {
        "rs" => Some('\u{e7a8}'),
        "go" => Some('\u{e627}'),
        "py" => Some('\u{e73c}'),
        "c" | "h" | "cc" | "cpp" | "hpp" => Some('\u{e7a3}'),
        "ts" | "tsx" => Some('\u{e628}'),
        "js" | "jsx" | "mjs" | "cjs" => Some('\u{e74e}'),
        "css" | "scss" | "sass" => Some('\u{e749}'),
        "html" | "htm" => Some('\u{e60e}'),
        "json" => Some('\u{e60b}'),
        "toml" | "yaml" | "yml" | "ini" | "conf" => Some('\u{e615}'),
        "md" | "markdown" => Some('\u{f48a}'),
        "lock" => Some('\u{f023}'),
        "sh" | "bash" | "zsh" | "fish" => Some('\u{e795}'),
        "rb" | "rake" | "gemspec" | "podspec" | "rbi" | "ru" => Some('\u{e739}'),
        "java" | "jav" => Some('\u{e738}'),
        "cs" | "csx" | "cake" => Some('\u{f031b}'),
        "php" | "php4" | "php5" | "phtml" | "ctp" => Some('\u{e73d}'),
        "scala" | "sbt" | "sc" => Some('\u{e737}'),
        "hs" | "hs-boot" | "hsc" => Some('\u{e777}'),
        "jl" => Some('\u{e624}'),
        "ml" | "mli" => Some('\u{e7a7}'),
        "txt" | "text" => Some('\u{f15c}'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_language_gets_its_own_glyph_and_anything_else_gets_the_generic_one() {
        assert_eq!(for_path(Path::new("src/app.rs")), '\u{e7a8}');
        assert_eq!(for_path(Path::new("Cargo.toml")), '\u{e615}');
        assert_ne!(for_path(Path::new("mystery.qqq")), '\u{e7a8}');
        assert_eq!(for_path(Path::new("mystery.qqq")), FILE);
    }

    /// A language Obelus highlights is a language its rows have a mark for.
    ///
    /// Two tables with no way to check each other: this one is extensions to
    /// glyphs and `LanguageId::for_name` is extensions to grammars, and
    /// nothing enumerates the extensions, so neither can be walked against
    /// the other. They drifted the moment ten languages were added to one of
    /// them -- a Rails project's list drew a row of anonymous file marks
    /// beside a preview that was highlighted perfectly.
    ///
    /// So it is a list by hand, which is the honest shape here: a language
    /// added to `for_name` and not to this list is a language whose mark
    /// nobody thought about, and the list is where somebody has to think.
    ///
    /// Broken deliberately by deleting any one of the rows this names from
    /// `by_extension` -- it then falls to `FILE`, which is the symptom.
    #[test]
    fn every_language_obelus_reads_has_a_mark_of_its_own() {
        for path in [
            "app.rb",
            "Main.java",
            "Program.cs",
            "index.php",
            "Build.scala",
            "Lib.hs",
            "plot.jl",
            "parser.ml",
            "parser.mli",
            "src/app.rs",
            "main.go",
            "setup.py",
            "index.ts",
            "app.jsx",
            "site.css",
        ] {
            assert_ne!(
                for_path(Path::new(path)),
                FILE,
                "{path} has no mark of its own, so its row is anonymous"
            );
        }
    }

    /// A dotfile is all stem as far as `Path` is concerned, so matching only
    /// on the extension would give `.gitignore` the generic glyph.
    #[test]
    fn a_dotfile_is_matched_by_its_whole_name() {
        assert_eq!(for_path(Path::new(".gitignore")), '\u{e702}');
        assert_eq!(for_path(Path::new("some/where/.gitignore")), '\u{e702}');
        assert_eq!(for_path(Path::new("Makefile")), '\u{e795}');
    }

    /// The name wins, so a file called `Makefile` is not a mystery just
    /// because it has no extension, and one called `Dockerfile.old` is.
    #[test]
    fn the_name_is_tried_before_the_extension() {
        assert_eq!(for_path(Path::new("Dockerfile")), '\u{e795}');
        assert_eq!(for_path(Path::new("Dockerfile.old")), FILE);
    }
}

#[cfg(test)]
mod command_tests {
    use obelus_command::Command;

    use super::*;

    /// A glyph per command, and no two commands that mean opposite things
    /// sharing one. "Go back" and "go forward" are the case that made this
    /// worth asserting: neighbours, and opposite actions.
    #[test]
    fn opposite_commands_do_not_share_a_glyph() {
        for (left, right) in [
            (Command::GoBack, Command::GoForward),
            (Command::LspRestart, Command::LspStop),
            (Command::SymbolDefinition, Command::SymbolReferences),
            (Command::FileOpen, Command::FileReload),
        ] {
            assert_ne!(
                for_command(left),
                for_command(right),
                "{} and {} look the same",
                left.name(),
                right.name()
            );
        }
    }

    /// A row that names something is read by its picture first, so two
    /// kinds that mean different things cannot wear the same one -- and
    /// none of them may wear the one that means "no idea what this is".
    #[test]
    fn the_kinds_a_list_names_have_their_own_glyphs() {
        use obelus_text::kind::SyntaxKind;

        let named = [
            SyntaxKind::Function,
            SyntaxKind::Type,
            SyntaxKind::Constructor,
            SyntaxKind::Constant,
            SyntaxKind::Property,
            SyntaxKind::Variable,
            SyntaxKind::Keyword,
            SyntaxKind::String,
            SyntaxKind::Number,
            SyntaxKind::Boolean,
        ];
        let mut seen = std::collections::HashMap::new();
        for kind in named {
            if let Some(earlier) = seen.insert(for_kind(kind), kind) {
                panic!("{earlier:?} and {kind:?} share a glyph");
            }
        }
        let anything = for_kind(SyntaxKind::Operator);
        for kind in named {
            assert_ne!(
                for_kind(kind),
                anything,
                "{kind:?} is drawn as something Obelus could not name"
            );
        }
    }

    /// How bad a problem is, it says in a picture of its own: an error is not
    /// a warning, and neither is the mark for a name Obelus could not tell
    /// the kind of -- which is what every problem wore while they were drawn
    /// out of `for_kind`, and in front of a sentence it reads as a cut.
    ///
    /// Broken deliberately by having `for_problem` answer `for_kind`.
    #[test]
    fn a_problem_says_how_bad_it_is_in_a_picture_of_its_own() {
        use obelus_text::kind::SyntaxKind;

        let error = for_problem(SyntaxKind::Error);
        let warning = for_problem(SyntaxKind::Warning);
        let remark = for_problem(SyntaxKind::Comment);
        assert_ne!(error, warning, "an error looks like a warning");
        assert_ne!(warning, remark, "a warning looks like a remark");
        assert_ne!(error, remark, "an error looks like a remark");
        let anything = for_kind(SyntaxKind::Operator);
        for glyph in [error, warning, remark] {
            assert_ne!(glyph, anything, "a problem wears the mark for a cut");
        }
    }

    /// Every command Obelus has gets a glyph of its own, so the palette is a
    /// column of pictures that mean something rather than one picture
    /// repeated. There is no fallback to find a hole in: the match is over
    /// the commands themselves, so a new one that nobody has drawn a picture
    /// for does not compile.
    #[test]
    fn every_command_has_its_own_glyph() {
        let mut seen = std::collections::HashMap::new();
        for spec in obelus_command::ALL {
            let glyph = for_command(spec.command);
            if let Some(earlier) = seen.insert(glyph, spec.name) {
                panic!("{} and {} share a glyph", earlier, spec.name);
            }
        }
    }
}
