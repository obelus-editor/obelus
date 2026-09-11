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
//! So obelus has one switch, [`NERD_FONT`], and everything that draws a glyph
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

/// Whether obelus may draw glyphs from a Nerd Font.
///
/// One switch for the whole program, because whether the font has them is a
/// fact about the reader's terminal rather than about any one view -- and
/// every reader of it has a fallback that reads correctly without one.
///
/// Global state, which is unlike the rest of obelus: the alternative is
/// threading a flag into every function that draws a glyph, including the
/// pure ones that turn a name into a character. It is written once at
/// startup and once per change of the setting, and read while drawing.
static NERD_FONT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Turns the glyphs on or off.
pub fn use_glyphs(on: bool) {
    NERD_FONT.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Whether glyphs are in use.
#[must_use]
pub fn enabled() -> bool {
    NERD_FONT.load(std::sync::atomic::Ordering::Relaxed)
}

/// The glyphs for keys, for whatever shows a binding.
///
/// The win here is the *words*: `pagedown` is eight columns and one glyph,
/// and a column of keys is right-aligned, so those columns come off the
/// width of everything else on the row. The arrow keys are left as arrows --
/// they are already symbols, and they are in every font.
pub mod key {
    /// `ctrl`.
    pub const CONTROL: char = '\u{f0634}';
    /// `alt`.
    pub const ALT: char = '\u{f0635}';
    /// `shift`.
    pub const SHIFT: char = '\u{f0636}';
    /// `enter`.
    pub const ENTER: char = '\u{f0311}';
    /// `esc`.
    pub const ESCAPE: char = '\u{f12b7}';
    /// `backspace`.
    pub const BACKSPACE: char = '\u{f030d}';
    /// `delete`.
    ///
    /// Ordinary Unicode -- "erase to the right" -- because a Nerd Font has
    /// no key glyph for this one, and the symbol on the keycap is exactly
    /// this character.
    pub const DELETE: char = '\u{2326}';
    /// `tab`.
    pub const TAB: char = '\u{f0312}';
    /// `space`.
    pub const SPACE: char = '\u{f1050}';
    /// `pageup`.
    pub const PAGE_UP: char = '\u{f013f}';
    /// `pagedown`.
    pub const PAGE_DOWN: char = '\u{f013c}';
    /// `home`.
    pub const HOME: char = '\u{f0600}';
    /// `end`.
    pub const END: char = '\u{f0601}';

    /// `f1`, and the eleven after it in order.
    ///
    /// One keycap glyph each, which is what a function key deserves: the
    /// twelve of them are the first thing a reader's eye goes to in a list
    /// of keys, and `f10` spelled out is three columns of text among
    /// one-column pictures.
    const FIRST_FUNCTION: u32 = 0xf12ab;

    /// The keycap for a function key, if it is one a keyboard has.
    #[must_use]
    pub fn function(number: u8) -> Option<char> {
        (1..=12)
            .contains(&number)
            .then(|| char::from_u32(FIRST_FUNCTION + u32::from(number) - 1))
            .flatten()
    }
}

/// The glyphs the views use for things that are not files.
pub mod ui {
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
    /// A file that can no longer be read from disk.
    pub const STALE: char = '\u{f0a4b}';
    /// A set of colours.
    pub const THEME: char = '\u{f03d8}';
    /// The reader, in a conversation with an agent.
    pub const READER: char = '\u{f0013}';
    /// The agent answering.
    pub const AGENT: char = '\u{f06a9}';
    /// The agent thinking, which agents send apart from their answer.
    pub const THOUGHT: char = '\u{f07f7}';
    /// The agent using a tool.
    pub const TOOL: char = '\u{f05b7}';
    /// obelus's own remark in a conversation.
    pub const NOTE: char = '\u{f02fd}';
    /// A tool call that has not started.
    pub const WAITING: char = '\u{f01d8}';
    /// One that is running.
    pub const RUNNING: char = '\u{f0772}';
    /// One that finished.
    pub const DONE: char = '\u{f012c}';
    /// One that did not.
    pub const BROKEN: char = '\u{f0159}';
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
        // A kind obelus has not heard of. The name says what it does; the
        // glyph says only that it is an answer.
        _ => '\u{f0450}',
    }
}

/// The glyph for a command.
///
/// By name, so a glyph can mean what the command means rather than what its
/// family does: "go back" and "go forward" are the same family and opposite
/// actions, and one picture for both says nothing. The family is the
/// fallback, so a command added to a known family gets something sensible
/// before anyone thinks about it.
#[must_use]
pub fn for_command(name: &str) -> char {
    match name {
        // Finding a file is what the picker does; the folder is closed until
        // then.
        "file.open" => '\u{f021e}',
        // A folder with a pencil on it: the files being worked on.
        "file.changed" => '\u{f08de}',
        "file.reload" => '\u{f0450}',
        "buffer.list" => '\u{f0222}',
        "buffer.close" => '\u{f0b98}',
        "markdown.preview" => '\u{f0354}',
        "theme.select" => '\u{f03d8}',
        "command.palette" => '\u{f018d}',
        // The menu is the questions themselves; each question is what it
        // does. A definition is a place to land on, a type is a shape, an
        // implementation is what hangs below the thing, and references are a
        // search.
        "symbol.menu" => '\u{f0174}',
        // A list of what is in something, which is what an outline is.
        "symbol.outline" => '\u{f0279}',
        "symbol.definition" => '\u{f04fe}',
        "symbol.typeDefinition" => '\u{f0169}',
        "symbol.implementation" => '\u{f04aa}',
        "symbol.references" => '\u{f13b8}',
        // The hooked arrows every browser uses, which is what the jump list
        // is.
        // A number, which is what this one asks for.
        // The branching lines every git tool uses for itself.
        // One view at three radii, so the glyphs say *where* rather than
        // repeating "search": the plain magnifier for the file in front of
        // the reader, folders for the tree, and a name in code for what a
        // server knows.
        "search.file" => '\u{f0349}',
        "search.project" => '\u{f0253}',
        "search.symbols" => '\u{f0871}',
        "git.hunk" => '\u{f02a2}',
        // A person, because that is the question this one answers.
        "git.blame" => '\u{f0013}',
        // Arrows, because these two move the reader: the diff is what they
        // step through, and the git glyph is already on the command that
        // opens one.
        "git.previous" => '\u{f0143}',
        "git.next" => '\u{f0140}',
        "go.line" => '\u{f03a0}',
        "go.bracket" => '\u{f0172}',
        "selection.clear" => '\u{f0156}',
        "go.back" => '\u{f17b3}',
        "go.forward" => '\u{f17b7}',
        // Sliders, because a cog is the fallback for a command in no
        // family obelus knows and two rows with the same picture say less
        // than one.
        "config.open" => '\u{f062e}',
        // A file with a cog on it: the settings themselves, as the file
        // they are kept in.
        "config.file" => '\u{f107b}',
        "agent.open" => ui::AGENT,
        // A conversation with a cog on it: this changes how the agent is
        // working, not how obelus is, and the sliders above already mean
        // that.
        "agent.settings" => '\u{f06f1}',
        "log.open" => '\u{f09ed}',
        "lsp.restart" => '\u{f0709}',
        "lsp.stop" => '\u{f04db}',
        // Leaving obelus, not switching a machine off.
        "app.quit" => '\u{f0206}',
        other => by_family(other),
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

/// The glyph for a kind of symbol, for an outline row.
///
/// Only the kinds an outline can hold: a list of what a file defines has
/// functions, types, constants and fields in it, and nothing else.
#[must_use]
pub fn for_kind(kind: crate::theme::SyntaxKind) -> char {
    use crate::theme::SyntaxKind;
    match kind {
        SyntaxKind::Function => '\u{f0295}',
        SyntaxKind::Type => '\u{f0169}',
        SyntaxKind::Constant => '\u{f04fe}',
        SyntaxKind::Property => '\u{f0219}',
        // A module or namespace, which is what the outline uses this for.
        SyntaxKind::Keyword => '\u{f04aa}',
        _ => '\u{f0295}',
    }
}

/// What a command whose name is not listed above gets.
fn by_family(name: &str) -> char {
    match name.split_once('.') {
        Some(("file", _)) => '\u{f0219}',
        Some(("buffer", _)) => '\u{f0222}',
        Some(("theme", _)) => '\u{f03d8}',
        Some(("command", _)) => '\u{f018d}',
        Some(("symbol", _)) => '\u{f0295}',
        Some(("go", _)) => '\u{f02da}',
        Some(("log", _)) => '\u{f09ed}',
        Some(("lsp", _)) => '\u{f048b}',
        Some(("git", _)) => '\u{f02a2}',
        // Something in no family obelus knows. A cog says "a thing obelus
        // does" without claiming to know which.
        Some(("app", _)) | None | Some(_) => '\u{f0493}',
    }
}

/// What a file with nothing more specific gets.
const FILE: char = '\u{f15b}';

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
    use super::*;

    /// A glyph per command, and no two commands that mean opposite things
    /// sharing one. "Go back" and "go forward" are the case that made this
    /// worth asserting: same family, opposite actions.
    #[test]
    fn opposite_commands_do_not_share_a_glyph() {
        for (left, right) in [
            ("go.back", "go.forward"),
            ("lsp.restart", "lsp.stop"),
            ("symbol.definition", "symbol.references"),
            ("file.open", "file.reload"),
        ] {
            assert_ne!(
                for_command(left),
                for_command(right),
                "{left} and {right} look the same"
            );
        }
    }

    /// Every command obelus has gets a glyph of its own, so the palette is a
    /// column of pictures that mean something rather than one picture
    /// repeated. A new command falls back to its family, which is a glyph
    /// that is right about the neighbourhood.
    #[test]
    fn every_command_has_its_own_glyph_and_a_new_one_has_a_family() {
        let mut seen = std::collections::HashMap::new();
        for spec in crate::command::ALL {
            let glyph = for_command(spec.name);
            if let Some(earlier) = seen.insert(glyph, spec.name) {
                panic!("{} and {} share a glyph", earlier, spec.name);
            }
        }

        assert_eq!(for_command("symbol.hover"), by_family("symbol.hover"));
        assert_ne!(for_command("nonsense"), for_command("symbol.hover"));
    }
}
