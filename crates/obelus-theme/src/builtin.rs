//! The themes compiled into the binary.
//!
//! `COLORTERM=truecolor` is the assumption, so these are RGB rather than the
//! sixteen named colours: syntax highlighting needs more distinct slots than
//! sixteen, and a reader that inherits the terminal's palette cannot promise
//! the same file looks the same twice.

use ratatui::style::Color;

use crate::{SyntaxTheme, Theme, tint};

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

/// The page gruvbox-dark is drawn on, and its three change colours, named
/// for the same reason the default's are: the washes are mixed from them.
const GRUVBOX_DARK_PAGE: Color = Color::Rgb(40, 40, 40);
const GRUVBOX_DARK_ADDED: Color = Color::Rgb(184, 187, 38);
const GRUVBOX_DARK_MODIFIED: Color = Color::Rgb(250, 189, 47);
const GRUVBOX_DARK_REMOVED: Color = Color::Rgb(251, 73, 52);

/// gruvbox, dark.
///
/// The roles are coloured the way gruvbox's own editors colour them --
/// red keywords, green strings, yellow types -- and not the way `dark`
/// above does. A reader who picks this by name wants gruvbox, and
/// Obelus's hues in gruvbox paint would be neither.
pub const GRUVBOX_DARK: Theme = Theme {
    background: GRUVBOX_DARK_PAGE,
    foreground: Color::Rgb(235, 219, 178),
    gutter: Color::Rgb(124, 111, 100),
    gutter_current: Color::Rgb(168, 153, 132),
    scrollbar_track: tint(GRUVBOX_DARK_PAGE, Color::Rgb(124, 111, 100), 70),
    status_foreground: Color::Rgb(213, 196, 161),
    status_stale: Color::Rgb(251, 73, 52),
    selected_row_background: Color::Rgb(60, 56, 54),
    raised_background: Color::Rgb(60, 56, 54),
    picker_match_background: tint(GRUVBOX_DARK_PAGE, Color::Rgb(131, 165, 152), 45),
    marked_background: tint(GRUVBOX_DARK_PAGE, Color::Rgb(69, 133, 136), 45),
    selection_background: tint(GRUVBOX_DARK_PAGE, Color::Rgb(131, 165, 152), 30),
    change_added: GRUVBOX_DARK_ADDED,
    change_modified: GRUVBOX_DARK_MODIFIED,
    change_removed: GRUVBOX_DARK_REMOVED,
    change_added_background: tint(GRUVBOX_DARK_PAGE, GRUVBOX_DARK_ADDED, WASH),
    change_modified_background: tint(GRUVBOX_DARK_PAGE, GRUVBOX_DARK_MODIFIED, WASH),
    change_removed_background: tint(GRUVBOX_DARK_PAGE, GRUVBOX_DARK_REMOVED, WASH),
    bracket_background: Color::Rgb(80, 73, 69),
    syntax: SyntaxTheme {
        attribute: Color::Rgb(250, 189, 47),
        boolean: Color::Rgb(211, 134, 155),
        comment: Color::Rgb(146, 131, 116),
        constant: Color::Rgb(211, 134, 155),
        constructor: Color::Rgb(250, 189, 47),
        escape: Color::Rgb(254, 128, 25),
        function: Color::Rgb(142, 192, 124),
        keyword: Color::Rgb(251, 73, 52),
        label: Color::Rgb(254, 128, 25),
        number: Color::Rgb(211, 134, 155),
        operator: Color::Rgb(168, 153, 132),
        property: Color::Rgb(131, 165, 152),
        punctuation: Color::Rgb(168, 153, 132),
        string: Color::Rgb(184, 187, 38),
        type_name: Color::Rgb(250, 189, 47),
        variable: Color::Rgb(235, 219, 178),
        error: GRUVBOX_DARK_REMOVED,
        warning: GRUVBOX_DARK_MODIFIED,
    },
};

/// The page gruvbox-light is drawn on, and its three change colours, named
/// for the same reason the default's are: the washes are mixed from them.
const GRUVBOX_LIGHT_PAGE: Color = Color::Rgb(251, 241, 199);
const GRUVBOX_LIGHT_ADDED: Color = Color::Rgb(121, 116, 14);
const GRUVBOX_LIGHT_MODIFIED: Color = Color::Rgb(181, 118, 20);
const GRUVBOX_LIGHT_REMOVED: Color = Color::Rgb(157, 0, 6);

/// gruvbox, light: the same palette with the two ends of it swapped,
/// which is how gruvbox itself is built.
pub const GRUVBOX_LIGHT: Theme = Theme {
    background: GRUVBOX_LIGHT_PAGE,
    foreground: Color::Rgb(60, 56, 54),
    gutter: Color::Rgb(189, 174, 147),
    gutter_current: Color::Rgb(124, 111, 100),
    scrollbar_track: tint(GRUVBOX_LIGHT_PAGE, Color::Rgb(189, 174, 147), 70),
    status_foreground: Color::Rgb(80, 73, 69),
    status_stale: Color::Rgb(157, 0, 6),
    selected_row_background: Color::Rgb(235, 219, 178),
    raised_background: Color::Rgb(235, 219, 178),
    picker_match_background: tint(GRUVBOX_LIGHT_PAGE, Color::Rgb(7, 102, 120), 25),
    marked_background: tint(GRUVBOX_LIGHT_PAGE, Color::Rgb(7, 102, 120), 22),
    selection_background: tint(GRUVBOX_LIGHT_PAGE, Color::Rgb(7, 102, 120), 15),
    change_added: GRUVBOX_LIGHT_ADDED,
    change_modified: GRUVBOX_LIGHT_MODIFIED,
    change_removed: GRUVBOX_LIGHT_REMOVED,
    change_added_background: tint(GRUVBOX_LIGHT_PAGE, GRUVBOX_LIGHT_ADDED, WASH),
    change_modified_background: tint(GRUVBOX_LIGHT_PAGE, GRUVBOX_LIGHT_MODIFIED, WASH),
    change_removed_background: tint(GRUVBOX_LIGHT_PAGE, GRUVBOX_LIGHT_REMOVED, WASH),
    bracket_background: Color::Rgb(213, 196, 161),
    syntax: SyntaxTheme {
        attribute: Color::Rgb(181, 118, 20),
        boolean: Color::Rgb(143, 63, 113),
        comment: Color::Rgb(146, 131, 116),
        constant: Color::Rgb(143, 63, 113),
        constructor: Color::Rgb(181, 118, 20),
        escape: Color::Rgb(175, 58, 3),
        function: Color::Rgb(66, 123, 88),
        keyword: Color::Rgb(157, 0, 6),
        label: Color::Rgb(175, 58, 3),
        number: Color::Rgb(143, 63, 113),
        operator: Color::Rgb(124, 111, 100),
        property: Color::Rgb(7, 102, 120),
        punctuation: Color::Rgb(124, 111, 100),
        string: Color::Rgb(121, 116, 14),
        type_name: Color::Rgb(181, 118, 20),
        variable: Color::Rgb(60, 56, 54),
        error: GRUVBOX_LIGHT_REMOVED,
        warning: GRUVBOX_LIGHT_MODIFIED,
    },
};

/// The page catppuccin-mocha is drawn on, and its three change colours, named
/// for the same reason the default's are: the washes are mixed from them.
const CATPPUCCIN_MOCHA_PAGE: Color = Color::Rgb(30, 30, 46);
const CATPPUCCIN_MOCHA_ADDED: Color = Color::Rgb(166, 227, 161);
const CATPPUCCIN_MOCHA_MODIFIED: Color = Color::Rgb(249, 226, 175);
const CATPPUCCIN_MOCHA_REMOVED: Color = Color::Rgb(243, 139, 168);

/// catppuccin, mocha.
///
/// Its base is the `#1e1e2e` `AGENTS.md` reaches for whenever it wants an
/// example of a theme's page, and the mark in `contrib/desktop` is drawn
/// in its ink. Which is a reason to have it and not a reason it is the
/// default: what Obelus opens on is its own.
pub const CATPPUCCIN_MOCHA: Theme = Theme {
    background: CATPPUCCIN_MOCHA_PAGE,
    foreground: Color::Rgb(205, 214, 244),
    gutter: Color::Rgb(88, 91, 112),
    gutter_current: Color::Rgb(147, 153, 178),
    scrollbar_track: tint(CATPPUCCIN_MOCHA_PAGE, Color::Rgb(88, 91, 112), 70),
    status_foreground: Color::Rgb(186, 194, 222),
    status_stale: Color::Rgb(243, 139, 168),
    selected_row_background: Color::Rgb(49, 50, 68),
    raised_background: Color::Rgb(49, 50, 68),
    picker_match_background: tint(CATPPUCCIN_MOCHA_PAGE, Color::Rgb(137, 180, 250), 45),
    marked_background: tint(CATPPUCCIN_MOCHA_PAGE, Color::Rgb(137, 180, 250), 30),
    selection_background: tint(CATPPUCCIN_MOCHA_PAGE, Color::Rgb(180, 190, 254), 25),
    change_added: CATPPUCCIN_MOCHA_ADDED,
    change_modified: CATPPUCCIN_MOCHA_MODIFIED,
    change_removed: CATPPUCCIN_MOCHA_REMOVED,
    change_added_background: tint(CATPPUCCIN_MOCHA_PAGE, CATPPUCCIN_MOCHA_ADDED, WASH),
    change_modified_background: tint(CATPPUCCIN_MOCHA_PAGE, CATPPUCCIN_MOCHA_MODIFIED, WASH),
    change_removed_background: tint(CATPPUCCIN_MOCHA_PAGE, CATPPUCCIN_MOCHA_REMOVED, WASH),
    bracket_background: Color::Rgb(69, 71, 90),
    syntax: SyntaxTheme {
        attribute: Color::Rgb(249, 226, 175),
        boolean: Color::Rgb(250, 179, 135),
        comment: Color::Rgb(108, 112, 134),
        constant: Color::Rgb(250, 179, 135),
        constructor: Color::Rgb(249, 226, 175),
        escape: Color::Rgb(245, 194, 231),
        function: Color::Rgb(137, 180, 250),
        keyword: Color::Rgb(203, 166, 247),
        label: Color::Rgb(249, 226, 175),
        number: Color::Rgb(250, 179, 135),
        operator: Color::Rgb(137, 220, 235),
        property: Color::Rgb(180, 190, 254),
        punctuation: Color::Rgb(147, 153, 178),
        string: Color::Rgb(166, 227, 161),
        type_name: Color::Rgb(249, 226, 175),
        variable: Color::Rgb(205, 214, 244),
        error: CATPPUCCIN_MOCHA_REMOVED,
        warning: CATPPUCCIN_MOCHA_MODIFIED,
    },
};

/// The page catppuccin-latte is drawn on, and its three change colours, named
/// for the same reason the default's are: the washes are mixed from them.
const CATPPUCCIN_LATTE_PAGE: Color = Color::Rgb(239, 241, 245);
const CATPPUCCIN_LATTE_ADDED: Color = Color::Rgb(64, 160, 43);
const CATPPUCCIN_LATTE_MODIFIED: Color = Color::Rgb(223, 142, 29);
const CATPPUCCIN_LATTE_REMOVED: Color = Color::Rgb(210, 15, 57);

/// catppuccin, latte: the light one of the same four, and the same
/// hues named the same things at the other end of the scale.
pub const CATPPUCCIN_LATTE: Theme = Theme {
    background: CATPPUCCIN_LATTE_PAGE,
    foreground: Color::Rgb(76, 79, 105),
    gutter: Color::Rgb(172, 176, 190),
    gutter_current: Color::Rgb(124, 127, 147),
    scrollbar_track: tint(CATPPUCCIN_LATTE_PAGE, Color::Rgb(172, 176, 190), 70),
    status_foreground: Color::Rgb(92, 95, 119),
    status_stale: Color::Rgb(210, 15, 57),
    selected_row_background: Color::Rgb(204, 208, 218),
    raised_background: Color::Rgb(204, 208, 218),
    picker_match_background: tint(CATPPUCCIN_LATTE_PAGE, Color::Rgb(30, 102, 245), 25),
    marked_background: tint(CATPPUCCIN_LATTE_PAGE, Color::Rgb(30, 102, 245), 18),
    selection_background: tint(CATPPUCCIN_LATTE_PAGE, Color::Rgb(114, 135, 253), 18),
    change_added: CATPPUCCIN_LATTE_ADDED,
    change_modified: CATPPUCCIN_LATTE_MODIFIED,
    change_removed: CATPPUCCIN_LATTE_REMOVED,
    change_added_background: tint(CATPPUCCIN_LATTE_PAGE, CATPPUCCIN_LATTE_ADDED, WASH),
    change_modified_background: tint(CATPPUCCIN_LATTE_PAGE, CATPPUCCIN_LATTE_MODIFIED, WASH),
    change_removed_background: tint(CATPPUCCIN_LATTE_PAGE, CATPPUCCIN_LATTE_REMOVED, WASH),
    bracket_background: Color::Rgb(188, 192, 204),
    syntax: SyntaxTheme {
        attribute: Color::Rgb(223, 142, 29),
        boolean: Color::Rgb(254, 100, 11),
        comment: Color::Rgb(156, 160, 176),
        constant: Color::Rgb(254, 100, 11),
        constructor: Color::Rgb(223, 142, 29),
        escape: Color::Rgb(234, 118, 203),
        function: Color::Rgb(30, 102, 245),
        keyword: Color::Rgb(136, 57, 239),
        label: Color::Rgb(223, 142, 29),
        number: Color::Rgb(254, 100, 11),
        operator: Color::Rgb(4, 165, 229),
        property: Color::Rgb(114, 135, 253),
        punctuation: Color::Rgb(124, 127, 147),
        string: Color::Rgb(64, 160, 43),
        type_name: Color::Rgb(223, 142, 29),
        variable: Color::Rgb(76, 79, 105),
        error: CATPPUCCIN_LATTE_REMOVED,
        warning: CATPPUCCIN_LATTE_MODIFIED,
    },
};

/// The page nord is drawn on, and its three change colours, named
/// for the same reason the default's are: the washes are mixed from them.
const NORD_PAGE: Color = Color::Rgb(46, 52, 64);
const NORD_ADDED: Color = Color::Rgb(163, 190, 140);
const NORD_MODIFIED: Color = Color::Rgb(235, 203, 139);
const NORD_REMOVED: Color = Color::Rgb(191, 97, 106);

/// nord, which has no light half and is not given one here.
///
/// Sixteen colours and four of them greys, so some roles share a hue --
/// a constructor, a type and a property are all nord7. Said rather than
/// worked around: inventing a colour to keep them apart would be a nord
/// that is not nord.
pub const NORD: Theme = Theme {
    background: NORD_PAGE,
    foreground: Color::Rgb(216, 222, 233),
    gutter: Color::Rgb(76, 86, 106),
    gutter_current: tint(NORD_PAGE, Color::Rgb(216, 222, 233), 60),
    scrollbar_track: tint(NORD_PAGE, Color::Rgb(76, 86, 106), 70),
    status_foreground: Color::Rgb(229, 233, 240),
    status_stale: Color::Rgb(191, 97, 106),
    selected_row_background: Color::Rgb(59, 66, 82),
    raised_background: Color::Rgb(59, 66, 82),
    picker_match_background: tint(NORD_PAGE, Color::Rgb(136, 192, 208), 45),
    marked_background: tint(NORD_PAGE, Color::Rgb(94, 129, 172), 55),
    selection_background: Color::Rgb(67, 76, 94),
    change_added: NORD_ADDED,
    change_modified: NORD_MODIFIED,
    change_removed: NORD_REMOVED,
    change_added_background: tint(NORD_PAGE, NORD_ADDED, WASH),
    change_modified_background: tint(NORD_PAGE, NORD_MODIFIED, WASH),
    change_removed_background: tint(NORD_PAGE, NORD_REMOVED, WASH),
    bracket_background: Color::Rgb(76, 86, 106),
    syntax: SyntaxTheme {
        attribute: Color::Rgb(208, 135, 112),
        boolean: Color::Rgb(180, 142, 173),
        comment: Color::Rgb(97, 110, 136),
        constant: Color::Rgb(180, 142, 173),
        constructor: Color::Rgb(143, 188, 187),
        escape: Color::Rgb(235, 203, 139),
        function: Color::Rgb(136, 192, 208),
        keyword: Color::Rgb(129, 161, 193),
        label: Color::Rgb(208, 135, 112),
        number: Color::Rgb(180, 142, 173),
        operator: Color::Rgb(129, 161, 193),
        property: Color::Rgb(143, 188, 187),
        punctuation: tint(NORD_PAGE, Color::Rgb(216, 222, 233), 70),
        string: Color::Rgb(163, 190, 140),
        type_name: Color::Rgb(143, 188, 187),
        variable: Color::Rgb(216, 222, 233),
        error: NORD_REMOVED,
        warning: NORD_MODIFIED,
    },
};

/// The page tokyo-night is drawn on, and its three change colours, named
/// for the same reason the default's are: the washes are mixed from them.
const TOKYO_NIGHT_PAGE: Color = Color::Rgb(26, 27, 38);
const TOKYO_NIGHT_ADDED: Color = Color::Rgb(158, 206, 106);
const TOKYO_NIGHT_MODIFIED: Color = Color::Rgb(224, 175, 104);
const TOKYO_NIGHT_REMOVED: Color = Color::Rgb(247, 118, 142);

/// tokyo-night, the dark one of its three, which is the one the name
/// is usually taken to mean.
pub const TOKYO_NIGHT: Theme = Theme {
    background: TOKYO_NIGHT_PAGE,
    foreground: Color::Rgb(192, 202, 245),
    gutter: Color::Rgb(59, 66, 97),
    gutter_current: tint(TOKYO_NIGHT_PAGE, Color::Rgb(192, 202, 245), 55),
    scrollbar_track: tint(TOKYO_NIGHT_PAGE, Color::Rgb(59, 66, 97), 70),
    status_foreground: Color::Rgb(169, 177, 214),
    status_stale: Color::Rgb(247, 118, 142),
    selected_row_background: Color::Rgb(41, 46, 66),
    raised_background: Color::Rgb(41, 46, 66),
    picker_match_background: tint(TOKYO_NIGHT_PAGE, Color::Rgb(122, 162, 247), 45),
    marked_background: tint(TOKYO_NIGHT_PAGE, Color::Rgb(122, 162, 247), 30),
    selection_background: tint(TOKYO_NIGHT_PAGE, Color::Rgb(122, 162, 247), 22),
    change_added: TOKYO_NIGHT_ADDED,
    change_modified: TOKYO_NIGHT_MODIFIED,
    change_removed: TOKYO_NIGHT_REMOVED,
    change_added_background: tint(TOKYO_NIGHT_PAGE, TOKYO_NIGHT_ADDED, WASH),
    change_modified_background: tint(TOKYO_NIGHT_PAGE, TOKYO_NIGHT_MODIFIED, WASH),
    change_removed_background: tint(TOKYO_NIGHT_PAGE, TOKYO_NIGHT_REMOVED, WASH),
    bracket_background: Color::Rgb(59, 66, 97),
    syntax: SyntaxTheme {
        attribute: Color::Rgb(224, 175, 104),
        boolean: Color::Rgb(255, 158, 100),
        comment: Color::Rgb(86, 95, 137),
        constant: Color::Rgb(255, 158, 100),
        constructor: Color::Rgb(224, 175, 104),
        escape: Color::Rgb(137, 221, 255),
        function: Color::Rgb(122, 162, 247),
        keyword: Color::Rgb(187, 154, 247),
        label: Color::Rgb(255, 158, 100),
        number: Color::Rgb(255, 158, 100),
        operator: Color::Rgb(137, 221, 255),
        property: Color::Rgb(125, 207, 255),
        punctuation: Color::Rgb(169, 177, 214),
        string: Color::Rgb(158, 206, 106),
        type_name: Color::Rgb(42, 195, 222),
        variable: Color::Rgb(192, 202, 245),
        error: TOKYO_NIGHT_REMOVED,
        warning: TOKYO_NIGHT_MODIFIED,
    },
};

/// The page solarized-dark is drawn on, and its three change colours, named
/// for the same reason the default's are: the washes are mixed from them.
const SOLARIZED_DARK_PAGE: Color = Color::Rgb(0, 43, 54);
const SOLARIZED_DARK_ADDED: Color = Color::Rgb(133, 153, 0);
const SOLARIZED_DARK_MODIFIED: Color = Color::Rgb(181, 137, 0);
const SOLARIZED_DARK_REMOVED: Color = Color::Rgb(220, 50, 47);

/// solarized, dark.
///
/// Its greys are picked by measured lightness rather than by eye, which
/// is the whole of what it is for, and its eight accents are the same
/// eight in both halves -- so the two below differ in their greys and in
/// nothing else. Green keywords and cyan strings are its own.
pub const SOLARIZED_DARK: Theme = Theme {
    background: SOLARIZED_DARK_PAGE,
    foreground: Color::Rgb(131, 148, 150),
    gutter: Color::Rgb(88, 110, 117),
    gutter_current: Color::Rgb(147, 161, 161),
    scrollbar_track: tint(SOLARIZED_DARK_PAGE, Color::Rgb(88, 110, 117), 70),
    status_foreground: Color::Rgb(147, 161, 161),
    status_stale: Color::Rgb(220, 50, 47),
    selected_row_background: Color::Rgb(7, 54, 66),
    raised_background: Color::Rgb(7, 54, 66),
    picker_match_background: tint(SOLARIZED_DARK_PAGE, Color::Rgb(38, 139, 210), 45),
    marked_background: tint(SOLARIZED_DARK_PAGE, Color::Rgb(38, 139, 210), 35),
    selection_background: tint(SOLARIZED_DARK_PAGE, Color::Rgb(38, 139, 210), 25),
    change_added: SOLARIZED_DARK_ADDED,
    change_modified: SOLARIZED_DARK_MODIFIED,
    change_removed: SOLARIZED_DARK_REMOVED,
    change_added_background: tint(SOLARIZED_DARK_PAGE, SOLARIZED_DARK_ADDED, WASH),
    change_modified_background: tint(SOLARIZED_DARK_PAGE, SOLARIZED_DARK_MODIFIED, WASH),
    change_removed_background: tint(SOLARIZED_DARK_PAGE, SOLARIZED_DARK_REMOVED, WASH),
    bracket_background: Color::Rgb(88, 110, 117),
    syntax: SyntaxTheme {
        attribute: Color::Rgb(181, 137, 0),
        boolean: Color::Rgb(211, 54, 130),
        comment: Color::Rgb(88, 110, 117),
        constant: Color::Rgb(211, 54, 130),
        constructor: Color::Rgb(181, 137, 0),
        escape: Color::Rgb(203, 75, 22),
        function: Color::Rgb(38, 139, 210),
        keyword: Color::Rgb(133, 153, 0),
        label: Color::Rgb(203, 75, 22),
        number: Color::Rgb(211, 54, 130),
        operator: Color::Rgb(131, 148, 150),
        property: Color::Rgb(108, 113, 196),
        punctuation: Color::Rgb(131, 148, 150),
        string: Color::Rgb(42, 161, 152),
        type_name: Color::Rgb(181, 137, 0),
        variable: Color::Rgb(131, 148, 150),
        error: SOLARIZED_DARK_REMOVED,
        warning: SOLARIZED_DARK_MODIFIED,
    },
};

/// The page solarized-light is drawn on, and its three change colours, named
/// for the same reason the default's are: the washes are mixed from them.
const SOLARIZED_LIGHT_PAGE: Color = Color::Rgb(253, 246, 227);
const SOLARIZED_LIGHT_ADDED: Color = Color::Rgb(133, 153, 0);
const SOLARIZED_LIGHT_MODIFIED: Color = Color::Rgb(181, 137, 0);
const SOLARIZED_LIGHT_REMOVED: Color = Color::Rgb(220, 50, 47);

/// solarized, light: the same eight accents on the other end of the
/// same grey scale.
pub const SOLARIZED_LIGHT: Theme = Theme {
    background: SOLARIZED_LIGHT_PAGE,
    foreground: Color::Rgb(101, 123, 131),
    gutter: Color::Rgb(147, 161, 161),
    gutter_current: Color::Rgb(88, 110, 117),
    scrollbar_track: tint(SOLARIZED_LIGHT_PAGE, Color::Rgb(147, 161, 161), 70),
    status_foreground: Color::Rgb(88, 110, 117),
    status_stale: Color::Rgb(220, 50, 47),
    selected_row_background: Color::Rgb(238, 232, 213),
    raised_background: Color::Rgb(238, 232, 213),
    picker_match_background: tint(SOLARIZED_LIGHT_PAGE, Color::Rgb(38, 139, 210), 25),
    marked_background: tint(SOLARIZED_LIGHT_PAGE, Color::Rgb(38, 139, 210), 18),
    selection_background: tint(SOLARIZED_LIGHT_PAGE, Color::Rgb(38, 139, 210), 13),
    change_added: SOLARIZED_LIGHT_ADDED,
    change_modified: SOLARIZED_LIGHT_MODIFIED,
    change_removed: SOLARIZED_LIGHT_REMOVED,
    change_added_background: tint(SOLARIZED_LIGHT_PAGE, SOLARIZED_LIGHT_ADDED, WASH),
    change_modified_background: tint(SOLARIZED_LIGHT_PAGE, SOLARIZED_LIGHT_MODIFIED, WASH),
    change_removed_background: tint(SOLARIZED_LIGHT_PAGE, SOLARIZED_LIGHT_REMOVED, WASH),
    bracket_background: tint(SOLARIZED_LIGHT_PAGE, Color::Rgb(147, 161, 161), 45),
    syntax: SyntaxTheme {
        attribute: Color::Rgb(181, 137, 0),
        boolean: Color::Rgb(211, 54, 130),
        comment: Color::Rgb(147, 161, 161),
        constant: Color::Rgb(211, 54, 130),
        constructor: Color::Rgb(181, 137, 0),
        escape: Color::Rgb(203, 75, 22),
        function: Color::Rgb(38, 139, 210),
        keyword: Color::Rgb(133, 153, 0),
        label: Color::Rgb(203, 75, 22),
        number: Color::Rgb(211, 54, 130),
        operator: Color::Rgb(101, 123, 131),
        property: Color::Rgb(108, 113, 196),
        punctuation: Color::Rgb(101, 123, 131),
        string: Color::Rgb(42, 161, 152),
        type_name: Color::Rgb(181, 137, 0),
        variable: Color::Rgb(101, 123, 131),
        error: SOLARIZED_LIGHT_REMOVED,
        warning: SOLARIZED_LIGHT_MODIFIED,
    },
};

/// The page high-contrast is drawn on, and its three change colours, named
/// for the same reason the default's are: the washes are mixed from them.
const HIGH_CONTRAST_PAGE: Color = Color::Rgb(0, 0, 0);
const HIGH_CONTRAST_ADDED: Color = Color::Rgb(0, 215, 95);
const HIGH_CONTRAST_MODIFIED: Color = Color::Rgb(255, 215, 0);
const HIGH_CONTRAST_REMOVED: Color = Color::Rgb(255, 95, 95);

/// Obelus's own, for a screen in sunlight or an eye that wants the
/// separation.
///
/// Black paper and white ink, and every accent picked far enough up the
/// lightness scale to stand on it. The comment is the one that matters:
/// every other theme dims comments towards the page, and a theme for
/// somebody who cannot read a dim grey must not do that -- so here a
/// comment is grey and still bright, and what tells it from the code is
/// the absence of a hue rather than the absence of light.
pub const HIGH_CONTRAST: Theme = Theme {
    background: HIGH_CONTRAST_PAGE,
    foreground: Color::Rgb(255, 255, 255),
    gutter: Color::Rgb(138, 138, 138),
    gutter_current: Color::Rgb(255, 255, 255),
    scrollbar_track: tint(HIGH_CONTRAST_PAGE, Color::Rgb(138, 138, 138), 70),
    status_foreground: Color::Rgb(255, 255, 255),
    status_stale: Color::Rgb(255, 95, 95),
    selected_row_background: Color::Rgb(42, 42, 42),
    raised_background: Color::Rgb(42, 42, 42),
    picker_match_background: tint(HIGH_CONTRAST_PAGE, Color::Rgb(95, 175, 255), 45),
    marked_background: tint(HIGH_CONTRAST_PAGE, Color::Rgb(95, 175, 255), 35),
    selection_background: tint(HIGH_CONTRAST_PAGE, Color::Rgb(95, 175, 255), 28),
    change_added: HIGH_CONTRAST_ADDED,
    change_modified: HIGH_CONTRAST_MODIFIED,
    change_removed: HIGH_CONTRAST_REMOVED,
    change_added_background: tint(HIGH_CONTRAST_PAGE, HIGH_CONTRAST_ADDED, WASH),
    change_modified_background: tint(HIGH_CONTRAST_PAGE, HIGH_CONTRAST_MODIFIED, WASH),
    change_removed_background: tint(HIGH_CONTRAST_PAGE, HIGH_CONTRAST_REMOVED, WASH),
    bracket_background: Color::Rgb(74, 74, 74),
    syntax: SyntaxTheme {
        attribute: Color::Rgb(255, 158, 61),
        boolean: Color::Rgb(255, 122, 198),
        comment: Color::Rgb(168, 168, 168),
        constant: Color::Rgb(255, 122, 198),
        constructor: Color::Rgb(255, 215, 95),
        escape: Color::Rgb(255, 158, 240),
        function: Color::Rgb(95, 175, 255),
        keyword: Color::Rgb(215, 138, 255),
        label: Color::Rgb(255, 158, 61),
        number: Color::Rgb(255, 122, 198),
        operator: Color::Rgb(208, 208, 208),
        property: Color::Rgb(95, 215, 255),
        punctuation: Color::Rgb(208, 208, 208),
        string: Color::Rgb(135, 255, 95),
        type_name: Color::Rgb(255, 215, 95),
        variable: Color::Rgb(255, 255, 255),
        error: HIGH_CONTRAST_REMOVED,
        warning: HIGH_CONTRAST_MODIFIED,
    },
};

/// Every theme compiled in, by the name it answers to, in the order the
/// picker lists them.
///
/// The name is here rather than on [`Theme`] because a theme is a set of
/// colours and a name is not one of them: the module's own rule is that
/// every field has a reader in the renderer, and nothing has ever painted
/// with this. What it is is a label on a shelf, so it lives on the shelf.
///
/// Obelus's own two first, because one of them is what it opens on; then
/// the ports, each pair together; then the one for a screen nobody can
/// dim. A reader looking for a name they already know finds it by reading
/// down, and a reader looking for *a* theme meets Obelus's before anybody
/// else's.
pub const ALL: &[(&str, &Theme)] = &[
    ("dark", &DARK),
    ("light", &LIGHT),
    ("gruvbox-dark", &GRUVBOX_DARK),
    ("gruvbox-light", &GRUVBOX_LIGHT),
    ("catppuccin-mocha", &CATPPUCCIN_MOCHA),
    ("catppuccin-latte", &CATPPUCCIN_LATTE),
    ("nord", &NORD),
    ("tokyo-night", &TOKYO_NIGHT),
    ("solarized-dark", &SOLARIZED_DARK),
    ("solarized-light", &SOLARIZED_LIGHT),
    ("high-contrast", &HIGH_CONTRAST),
];

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
