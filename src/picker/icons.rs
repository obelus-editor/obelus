//! A glyph for a file, from a Nerd Font.
//!
//! These live in the Unicode private use area, so they exist only if the
//! reader's terminal font has them. A machine without a patched font shows a
//! box for every one — which is the first thing about obelus that will
//! genuinely want a configuration file.
//!
//! Every codepoint here was checked against the font actually installed rather
//! than taken from a table, because a wrong one is indistinguishable from a
//! missing font.

use std::path::Path;

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
