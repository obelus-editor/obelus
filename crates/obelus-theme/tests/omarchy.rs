//! The template shipped for omarchy, rendered the way omarchy renders it.
//!
//! omarchy fills a template with `sed`, one `{{ name }}` at a time, from a
//! theme's `colors.toml` -- so a name the palette does not have is left in
//! the file as it was written, and a colour that does not parse is a colour
//! inherited from the base without a word. Rendered here from two real
//! palettes, copied in rather than read from a desktop that may not be this
//! one.

use obelus_theme::{Theme, builtin, written};
use ratatui::style::Color;

/// omarchy's Midnight, as its `colors.toml` has it.
const MIDNIGHT: &str = r##"
mode = "dark"
accent = "#407e70"
selection = "#333333"
muted = "#1e1e1e"
background = "#000000"
dark_background = "#0d0d0d"
darker_background = "#000000"
lighter_background = "#121212"
foreground = "#EFEFEF"
dark_foreground = "#555555"
light_foreground = "#8a8a8d"
bright_foreground = "#ffffff"
red = "#D35F5F"
yellow = "#FFC107"
orange = "#F59E0B"
green = "#8A9A7B"
cyan = "#88AABB"
blue = "#8A9FBE"
magenta = "#C1A1C1"
brown = "#8a8a8d"
bright_red = "#B91C1C"
bright_yellow = "#F59E0B"
bright_green = "#A5B799"
bright_cyan = "#A2C4D3"
bright_blue = "#A4BBDD"
bright_magenta = "#D9B9D9"
"##;

/// omarchy's Catppuccin Latte: a light one, so `mode` has to say so.
const LATTE: &str = r##"
mode = "light"
accent = "#1e66f5"
selection = "#ccd0da"
muted = "#acb0be"
background = "#eff1f5"
dark_background = "#e3e4e8"
darker_background = "#d7d8dc"
lighter_background = "#dce0e8"
foreground = "#4c4f69"
dark_foreground = "#9ca0b0"
light_foreground = "#5c5f77"
bright_foreground = "#4c4f69"
red = "#d20f39"
yellow = "#df8e1d"
orange = "#d84e2b"
green = "#40a02b"
cyan = "#179299"
blue = "#1e66f5"
magenta = "#ea76cb"
brown = "#6c2715"
bright_red = "#d20f39"
bright_yellow = "#df8e1d"
bright_green = "#40a02b"
bright_cyan = "#179299"
bright_blue = "#1e66f5"
bright_magenta = "#ea76cb"
"##;

/// The template filled from a palette, by literal replacement as omarchy's
/// `sed` does it, and read as Obelus reads the file that comes out.
fn rendered(palette: &str) -> (String, Theme) {
    let template = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../contrib/omarchy/obelus.toml.tpl"),
    )
    .expect("the template shipped beside Obelus");
    let palette: toml::Table = palette.parse().expect("a palette");
    let mut said = template;
    for (name, value) in &palette {
        let value = value.as_str().expect("a palette is words");
        said = said.replace(&format!("{{{{ {name} }}}}"), value);
    }
    let table: toml::Table = said.parse().expect("the rendered template is not toml");
    let theme = written::over(&table);
    (said, theme)
}

fn colour(palette: &str, name: &str) -> Color {
    let palette: toml::Table = palette.parse().expect("a palette");
    let said = palette
        .get(name)
        .and_then(toml::Value::as_str)
        .expect("a colour the palette has");
    written::hex(said).expect("a colour")
}

/// Midnight comes out the way omarchy's own editors draw it: a black page,
/// and the syntax on the hues its helix and VS Code templates give it --
/// keywords magenta, functions blue, types yellow, strings green. A theme
/// on the right page with the wrong hues would read as a different theme
/// beside every other editor on the desktop.
///
/// Broken deliberately by mapping the template's `keyword` to `{{ blue }}`
/// (the keyword assertion fails), and by misspelling `{{ background }}` as
/// `{{ backgruond }}` (the placeholder is left behind, which is caught
/// before the page -- the built-in dark one's grey -- is looked at).
#[test]
fn midnight_is_the_colours_omarchy_gives_its_editors() {
    let (said, theme) = rendered(MIDNIGHT);
    assert!(
        !said.contains("{{"),
        "a placeholder no omarchy palette has: {said}"
    );
    assert_eq!(
        theme.background,
        Color::Rgb(0, 0, 0),
        "the page is not black"
    );
    assert_eq!(theme.foreground, colour(MIDNIGHT, "foreground"));
    assert_eq!(theme.syntax.keyword, colour(MIDNIGHT, "magenta"));
    assert_eq!(theme.syntax.function, colour(MIDNIGHT, "blue"));
    assert_eq!(theme.syntax.type_name, colour(MIDNIGHT, "yellow"));
    assert_eq!(theme.syntax.string, colour(MIDNIGHT, "green"));
    assert_eq!(theme.syntax.variable, colour(MIDNIGHT, "foreground"));
    // What the template leaves out is the dark theme's.
    assert_eq!(
        theme.change_added_background,
        builtin::DARK.change_added_background
    );
}

/// A light palette builds on the light theme, so what the template leaves
/// out is drawn for a light page rather than a dark one.
///
/// Broken deliberately by writing `base = "dark"` in the template in place
/// of `{{ mode }}`: the wash behind an added line is the dark theme's.
#[test]
fn a_light_palette_builds_on_the_light_theme() {
    let (said, theme) = rendered(LATTE);
    assert!(!said.contains("{{"), "a placeholder left behind: {said}");
    assert_eq!(theme.background, colour(LATTE, "background"));
    assert_eq!(
        theme.change_added_background,
        builtin::LIGHT.change_added_background
    );
}
