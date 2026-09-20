//! The themes compiled into the binary.
//!
//! `COLORTERM=truecolor` is the assumption, so these are RGB rather than the
//! sixteen named colours: syntax highlighting needs more distinct slots than
//! sixteen, and a reader that inherits the terminal's palette cannot promise
//! the same file looks the same twice.

use ratatui::style::Color;

use crate::theme::{SyntaxTheme, Theme, tint};

/// The dark theme's page, named because its three change tints are washes
/// of it.
const DARK_PAGE: Color = Color::Rgb(24, 24, 27);
/// Green, amber, red: what every diff has used since diffs were printed.
const DARK_ADDED: Color = Color::Rgb(34, 197, 94);
const DARK_MODIFIED: Color = Color::Rgb(234, 179, 8);
const DARK_REMOVED: Color = Color::Rgb(239, 68, 68);
/// How much of the mark's colour a wash carries. Low: the code on top of it
/// keeps its own colours, and a whole block of rows at this strength should
/// read as tinted paper rather than as a highlighter.
const WASH: u32 = 18;

/// The default.
pub const DARK: Theme = Theme {
    background: DARK_PAGE,
    foreground: Color::Rgb(228, 228, 231),
    gutter: Color::Rgb(82, 82, 91),
    gutter_current: Color::Rgb(161, 161, 170),
    scrollbar_track: tint(DARK_PAGE, Color::Rgb(82, 82, 91), 70),
    status_foreground: Color::Rgb(212, 212, 216),
    status_stale: Color::Rgb(248, 113, 113),
    selected_row_background: Color::Rgb(39, 39, 42),
    raised_background: Color::Rgb(39, 39, 42),
    picker_match_background: tint(DARK_PAGE, Color::Rgb(96, 165, 250), 45),
    marked_background: Color::Rgb(30, 58, 95),
    selection_background: Color::Rgb(49, 46, 129),
    // Grey rather than a hue: it sits under whatever colour the bracket
    // already has, and a coloured background under a coloured glyph is two
    // hues fighting.
    change_added: DARK_ADDED,
    change_modified: DARK_MODIFIED,
    change_removed: DARK_REMOVED,
    change_added_background: tint(DARK_PAGE, DARK_ADDED, WASH),
    change_modified_background: tint(DARK_PAGE, DARK_MODIFIED, WASH),
    change_removed_background: tint(DARK_PAGE, DARK_REMOVED, WASH),
    bracket_background: Color::Rgb(63, 63, 70),
    syntax: SyntaxTheme {
        attribute: Color::Rgb(251, 146, 60),
        boolean: Color::Rgb(244, 114, 182),
        comment: Color::Rgb(113, 113, 122),
        constant: Color::Rgb(244, 114, 182),
        constructor: Color::Rgb(251, 191, 36),
        escape: Color::Rgb(240, 171, 252),
        function: Color::Rgb(96, 165, 250),
        keyword: Color::Rgb(192, 132, 252),
        label: Color::Rgb(251, 146, 60),
        number: Color::Rgb(244, 114, 182),
        operator: Color::Rgb(161, 161, 170),
        property: Color::Rgb(125, 211, 252),
        punctuation: Color::Rgb(161, 161, 170),
        string: Color::Rgb(163, 230, 53),
        type_name: Color::Rgb(251, 191, 36),
        variable: Color::Rgb(228, 228, 231),
        error: DARK_REMOVED,
        warning: DARK_MODIFIED,
    },
};

/// The light theme's page and its three change colours, for the same reason.
const LIGHT_PAGE: Color = Color::Rgb(250, 250, 250);
const LIGHT_ADDED: Color = Color::Rgb(22, 163, 74);
const LIGHT_MODIFIED: Color = Color::Rgb(180, 130, 6);
const LIGHT_REMOVED: Color = Color::Rgb(220, 38, 38);

/// The same design with the ends of the scale swapped.
pub const LIGHT: Theme = Theme {
    background: LIGHT_PAGE,
    foreground: Color::Rgb(24, 24, 27),
    gutter: Color::Rgb(161, 161, 170),
    gutter_current: Color::Rgb(82, 82, 91),
    scrollbar_track: tint(LIGHT_PAGE, Color::Rgb(161, 161, 170), 70),
    status_foreground: Color::Rgb(39, 39, 42),
    status_stale: Color::Rgb(185, 28, 28),
    selected_row_background: Color::Rgb(228, 228, 231),
    raised_background: Color::Rgb(228, 228, 231),
    picker_match_background: tint(LIGHT_PAGE, Color::Rgb(29, 78, 216), 25),
    marked_background: Color::Rgb(191, 219, 254),
    selection_background: Color::Rgb(224, 231, 255),
    change_added: LIGHT_ADDED,
    change_modified: LIGHT_MODIFIED,
    change_removed: LIGHT_REMOVED,
    change_added_background: tint(LIGHT_PAGE, LIGHT_ADDED, WASH),
    change_modified_background: tint(LIGHT_PAGE, LIGHT_MODIFIED, WASH),
    change_removed_background: tint(LIGHT_PAGE, LIGHT_REMOVED, WASH),
    bracket_background: Color::Rgb(212, 212, 216),
    syntax: SyntaxTheme {
        attribute: Color::Rgb(194, 65, 12),
        boolean: Color::Rgb(190, 24, 93),
        comment: Color::Rgb(113, 113, 122),
        constant: Color::Rgb(190, 24, 93),
        constructor: Color::Rgb(161, 98, 7),
        escape: Color::Rgb(162, 28, 175),
        function: Color::Rgb(29, 78, 216),
        keyword: Color::Rgb(126, 34, 206),
        label: Color::Rgb(194, 65, 12),
        number: Color::Rgb(190, 24, 93),
        operator: Color::Rgb(82, 82, 91),
        property: Color::Rgb(3, 105, 161),
        punctuation: Color::Rgb(82, 82, 91),
        string: Color::Rgb(63, 98, 18),
        type_name: Color::Rgb(161, 98, 7),
        variable: Color::Rgb(24, 24, 27),
        error: LIGHT_REMOVED,
        warning: LIGHT_MODIFIED,
    },
};

/// Every theme compiled in, by the name it answers to, in the order the
/// picker lists them.
///
/// The name is here rather than on [`Theme`] because a theme is a set of
/// colours and a name is not one of them: the module's own rule is that
/// every field has a reader in the renderer, and nothing has ever painted
/// with this. What it is is a label on a shelf, so it lives on the shelf.
pub const ALL: &[(&str, &Theme)] = &[("dark", &DARK), ("light", &LIGHT)];

/// The theme a name names, if it names one.
///
/// The way in from anything written down -- the settings file, and a row of
/// a list whose value is a word rather than a theme.
#[must_use]
pub fn by_name(name: &str) -> Option<&'static Theme> {
    ALL.iter()
        .find(|(called, _)| *called == name)
        .map(|(_, theme)| *theme)
}
