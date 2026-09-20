//! A theme read from a file.
//!
//! Every colour is optional and whatever is not given comes from a theme
//! that is compiled in. A file that had to name all thirty-seven would be a
//! format nobody writes by hand and a template nobody can generate: the
//! thing generating one -- a desktop that themes every program it has --
//! knows a dozen colours by a semantic name and nothing at all about which
//! of them a comment should be.
//!
//! It is also what keeps old files working. A colour added to [`Theme`]
//! later is a colour every file on disk is silently already answering,
//! because the base answers it.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ratatui::style::Color;

use crate::theme::{SyntaxTheme, Theme, builtin};

/// The directory a layer's themes live in, beside its settings file.
#[must_use]
pub fn beside(settings: &Path) -> Option<PathBuf> {
    Some(settings.parent()?.join("themes"))
}

/// The themes in a directory, by the name each answers to.
///
/// The file's own stem, so `omarchy.toml` is `omarchy`: a name written
/// inside the file would be a name that can disagree with the one the
/// reader typed into their settings.
#[must_use]
pub fn found_in(directory: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut found: Vec<(String, PathBuf)> = entries
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|kind| kind == "toml"))
        .filter_map(|path| {
            let name = path.file_stem()?.to_str()?.to_string();
            Some((name, path))
        })
        .collect();
    // By name, because a directory hands them over in whatever order it
    // holds them and a list that reorders itself between openings cannot be
    // learned.
    found.sort_by(|left, right| left.0.cmp(&right.0));
    found
}

/// Reads one, over whichever built-in theme it says to build on.
pub fn read(path: &Path) -> Result<Theme> {
    let said =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let table: toml::Table = said
        .parse()
        .with_context(|| format!("{} is not toml", path.display()))?;
    Ok(over(&table))
}

/// The theme a table describes, over the one it builds on.
#[must_use]
pub fn over(table: &toml::Table) -> Theme {
    // Named `base` rather than `mode`, because it names a theme rather than
    // a kind of one: what a file inherits is thirty-seven colours, and which
    // thirty-seven is the whole of what this says.
    let base = table
        .get("base")
        .and_then(toml::Value::as_str)
        .and_then(builtin::by_name)
        .unwrap_or(&builtin::DARK);
    let syntax = table.get("syntax").and_then(toml::Value::as_table);
    let of = |key: &str, fallback: Color| colour(table, key, fallback);
    let syn = |key: &str, fallback: Color| match syntax {
        Some(table) => colour(table, key, fallback),
        None => fallback,
    };

    // A field at a time rather than a loop over names, so a colour added to
    // the theme and not read here does not compile. The same rule the
    // settings follow, and the reason both are this long.
    Theme {
        background: of("background", base.background),
        foreground: of("foreground", base.foreground),
        gutter: of("gutter", base.gutter),
        gutter_current: of("gutter_current", base.gutter_current),
        scrollbar_track: of("scrollbar_track", base.scrollbar_track),
        status_foreground: of("status_foreground", base.status_foreground),
        status_stale: of("status_stale", base.status_stale),
        selected_row_background: of("selected_row_background", base.selected_row_background),
        raised_background: of("raised_background", base.raised_background),
        picker_match_background: of("picker_match_background", base.picker_match_background),
        marked_background: of("marked_background", base.marked_background),
        selection_background: of("selection_background", base.selection_background),
        change_added: of("change_added", base.change_added),
        change_modified: of("change_modified", base.change_modified),
        change_removed: of("change_removed", base.change_removed),
        change_added_background: of("change_added_background", base.change_added_background),
        change_modified_background: of(
            "change_modified_background",
            base.change_modified_background,
        ),
        change_removed_background: of("change_removed_background", base.change_removed_background),
        bracket_background: of("bracket_background", base.bracket_background),
        syntax: SyntaxTheme {
            attribute: syn("attribute", base.syntax.attribute),
            boolean: syn("boolean", base.syntax.boolean),
            comment: syn("comment", base.syntax.comment),
            constant: syn("constant", base.syntax.constant),
            constructor: syn("constructor", base.syntax.constructor),
            escape: syn("escape", base.syntax.escape),
            function: syn("function", base.syntax.function),
            keyword: syn("keyword", base.syntax.keyword),
            label: syn("label", base.syntax.label),
            number: syn("number", base.syntax.number),
            operator: syn("operator", base.syntax.operator),
            property: syn("property", base.syntax.property),
            punctuation: syn("punctuation", base.syntax.punctuation),
            string: syn("string", base.syntax.string),
            type_name: syn("type_name", base.syntax.type_name),
            variable: syn("variable", base.syntax.variable),
            error: syn("error", base.syntax.error),
            warning: syn("warning", base.syntax.warning),
        },
    }
}

/// One colour out of a table, or what it inherits.
///
/// A line that will not parse is a line that did nothing, with a word in
/// the log for whoever wrote it: the rest of the file is still colours, and
/// a theme thrown away over one typo is a screen the reader cannot use to
/// find the typo.
fn colour(table: &toml::Table, key: &str, fallback: Color) -> Color {
    let Some(said) = table.get(key).and_then(toml::Value::as_str) else {
        return fallback;
    };
    match hex(said) {
        Some(colour) => colour,
        None => {
            tracing::warn!(key, said, "not a colour, so it was left alone");
            fallback
        }
    }
}

/// `#rrggbb`, which is how every palette anybody has is written down.
///
/// The three-digit form too, because it is what a person types. Nothing
/// else: a named colour would be the terminal's sixteen, and a theme that
/// reached for those could not promise the same file looks the same twice.
#[must_use]
pub fn hex(said: &str) -> Option<Color> {
    let digits = said.strip_prefix('#')?;
    let (red, green, blue) = match digits.len() {
        3 => {
            let mut characters = digits.chars();
            let mut doubled = || {
                let digit = characters.next()?.to_digit(16)?;
                u8::try_from(digit * 16 + digit).ok()
            };
            (doubled()?, doubled()?, doubled()?)
        }
        6 => (
            u8::from_str_radix(&digits[0..2], 16).ok()?,
            u8::from_str_radix(&digits[2..4], 16).ok()?,
            u8::from_str_radix(&digits[4..6], 16).ok()?,
        ),
        _ => return None,
    };
    Some(Color::Rgb(red, green, blue))
}
