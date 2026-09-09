//! The themes compiled into the binary.
//!
//! `COLORTERM=truecolor` is the assumption, so these are RGB rather than the
//! sixteen named colours: syntax highlighting needs more distinct slots than
//! sixteen, and a reader that inherits the terminal's palette cannot promise
//! the same file looks the same twice.

use ratatui::style::Color;

use crate::theme::{SyntaxTheme, Theme};

/// The default.
pub const DARK: Theme = Theme {
    name: "dark",
    background: Color::Rgb(24, 24, 27),
    foreground: Color::Rgb(228, 228, 231),
    gutter: Color::Rgb(82, 82, 91),
    gutter_current: Color::Rgb(161, 161, 170),
    status_background: Color::Rgb(39, 39, 42),
    status_foreground: Color::Rgb(212, 212, 216),
    status_stale: Color::Rgb(248, 113, 113),
    picker_selected_background: Color::Rgb(39, 39, 42),
    picker_match: Color::Rgb(96, 165, 250),
    marked_background: Color::Rgb(30, 58, 95),
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
    },
};

/// The same design with the ends of the scale swapped.
pub const LIGHT: Theme = Theme {
    name: "light",
    background: Color::Rgb(250, 250, 250),
    foreground: Color::Rgb(24, 24, 27),
    gutter: Color::Rgb(161, 161, 170),
    gutter_current: Color::Rgb(82, 82, 91),
    status_background: Color::Rgb(228, 228, 231),
    status_foreground: Color::Rgb(39, 39, 42),
    status_stale: Color::Rgb(185, 28, 28),
    picker_selected_background: Color::Rgb(228, 228, 231),
    picker_match: Color::Rgb(29, 78, 216),
    marked_background: Color::Rgb(191, 219, 254),
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
    },
};

/// Every theme, in the order the picker lists them.
pub const ALL: &[&Theme] = &[&DARK, &LIGHT];
