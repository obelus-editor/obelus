//! How many colours a terminal can draw, and a theme's colours brought down
//! to the 256 a terminal that cannot draw millions has.
//!
//! Themes are written in RGB, and a terminal that does not understand
//! `38;2;r;g;b` does not say so: it reads the numbers as other attributes,
//! and macOS's own terminal before macOS 26 drew the whole of Obelus in one
//! dark red. So a terminal is drawn in RGB where it says it can be, and
//! otherwise in the nearest of the 240 colours every 256-colour terminal
//! agrees about -- never the first sixteen, which are the terminal's own
//! palette and could be anything.
//!
//! Which one is the front end's to find out, because the terminal is the
//! front end's; this is only what the reader asked for, which the settings
//! say and an Obelus on another machine passes on.

use std::sync::atomic::{AtomicU8, Ordering};

use ratatui::style::Color;

/// How many colours to draw in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Depth {
    /// RGB, as the theme says it.
    Millions,
    /// The nearest of the xterm palette's 240 fixed colours.
    Palette,
}

impl Depth {
    /// What a setting's word asks for, or nothing for "whatever the
    /// terminal says" -- `auto`, or a word nothing answers to.
    #[must_use]
    pub fn named(word: &str) -> Option<Self> {
        match word {
            "truecolor" => Some(Self::Millions),
            "256" => Some(Self::Palette),
            _ => None,
        }
    }

    /// `colour`, as it is drawn at this depth.
    #[must_use]
    pub fn of(self, colour: Color) -> Color {
        match (self, colour) {
            (Self::Palette, Color::Rgb(red, green, blue)) => {
                Color::Indexed(nearest(red, green, blue))
            }
            _ => colour,
        }
    }
}

/// What the reader asked for: 0 for nothing, 1 and 2 for a [`Depth`].
static ASKED: AtomicU8 = AtomicU8::new(0);

/// Draws at `depth` from now on, or at whatever the terminal says for
/// `None`.
///
/// A global, for the glyph switch's reason: it is a decision the whole
/// process shares, and the one thing that reads it -- what hands a frame to
/// the terminal -- is not anywhere the settings are.
pub fn ask(depth: Option<Depth>) {
    let stored = match depth {
        None => 0,
        Some(Depth::Millions) => 1,
        Some(Depth::Palette) => 2,
    };
    ASKED.store(stored, Ordering::Relaxed);
}

/// What the reader asked for, if they asked.
#[must_use]
pub fn asked() -> Option<Depth> {
    match ASKED.load(Ordering::Relaxed) {
        1 => Some(Depth::Millions),
        2 => Some(Depth::Palette),
        _ => None,
    }
}

/// The steps of the 6x6x6 cube, which is 16 to 231.
const STEPS: [u8; 6] = [0x00, 0x5f, 0x87, 0xaf, 0xd7, 0xff];

/// The colour of the palette nearest this one: the nearest in the cube, or
/// on the grey ramp, which is 232 to 255 and has the greys the cube's six
/// steps leave out.
fn nearest(red: u8, green: u8, blue: u8) -> u8 {
    let step = |channel: u8| {
        STEPS
            .iter()
            .enumerate()
            .min_by_key(|(_, step)| channel.abs_diff(**step))
            .map_or(0, |(at, _)| at)
    };
    let (r, g, b) = (step(red), step(green), step(blue));
    let cube = (STEPS[r], STEPS[g], STEPS[b]);
    // 8, 18 ... 238: the nearest of them to the channels' mean.
    let mean = (u16::from(red) + u16::from(green) + u16::from(blue)) / 3;
    let grey = u8::try_from(mean.saturating_sub(3) / 10)
        .unwrap_or(23)
        .min(23);
    let level = 8 + 10 * grey;
    let far = |(r, g, b): (u8, u8, u8)| {
        let d = |a: u8, b: u8| u32::from(a.abs_diff(b)).pow(2);
        d(red, r) + d(green, g) + d(blue, b)
    };
    // Sixteen plus a number below 216, which a `u8` holds.
    let in_cube = u8::try_from(16 + 36 * r + 6 * g + b).unwrap_or(16);
    if far((level, level, level)) < far(cube) {
        232 + grey
    } else {
        in_cube
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A colour in the cube is itself, and one between is the step it is
    /// nearer.
    ///
    /// Deliberate break: the cube's index as `36 * b + 6 * g + r`. Pure
    /// red went to 21, which is blue.
    #[test]
    fn a_colour_in_the_cube_is_itself() {
        assert_eq!(nearest(0xff, 0, 0), 196, "pure red");
        assert_eq!(nearest(0, 0xff, 0), 46, "pure green");
        assert_eq!(nearest(0, 0, 0xff), 21, "pure blue");
        assert_eq!(nearest(0x5f, 0x87, 0xaf), 16 + 36 + 12 + 3);
        // Nearer 0x87 than 0x5f.
        assert_eq!(nearest(0x80, 0, 0), 16 + 36 * 2);
    }

    /// A grey is drawn on the ramp, which has twenty-four of them, rather
    /// than the cube's six.
    ///
    /// Deliberate break: the ramp left out, so only the cube answers. The
    /// dark theme's page, 24 24 27, went to 16, which is black.
    #[test]
    fn a_grey_is_drawn_on_the_ramp() {
        assert_eq!(nearest(24, 24, 27), 234, "the dark theme's page");
        assert_eq!(nearest(0x80, 0x80, 0x80), 244);
        assert_eq!(nearest(0xee, 0xee, 0xee), 255);
        // Black and white are the cube's corners, nearer than the ramp's
        // ends.
        assert_eq!(nearest(0, 0, 0), 16);
        assert_eq!(nearest(0xff, 0xff, 0xff), 231);
    }

    /// Only an RGB colour is brought down, and only to the palette: the
    /// terminal's own and the default are left as they are, and millions
    /// is RGB as it was.
    ///
    /// Deliberate break: `of` matching on the colour alone. Millions came
    /// out as an index.
    #[test]
    fn only_rgb_is_brought_down_and_only_to_the_palette() {
        let page = Color::Rgb(24, 24, 27);
        assert_eq!(Depth::Palette.of(page), Color::Indexed(234));
        assert_eq!(Depth::Millions.of(page), page);
        assert_eq!(Depth::Palette.of(Color::Reset), Color::Reset);
        assert_eq!(Depth::Palette.of(Color::Indexed(3)), Color::Indexed(3));
    }

    /// The setting's words, and `auto` as nothing asked.
    ///
    /// Deliberate break: `256` named as millions. The second assertion
    /// failed.
    #[test]
    fn the_settings_words() {
        assert_eq!(Depth::named("truecolor"), Some(Depth::Millions));
        assert_eq!(Depth::named("256"), Some(Depth::Palette));
        assert_eq!(Depth::named("auto"), None);
    }
}
