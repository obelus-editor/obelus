//! What obelus shows when nothing is open.
//!
//! Two layouts. The wide one is a wordmark, the keys as caps, and a footer;
//! the narrow one is the keys and nothing else. A screen too small for even
//! that gets nothing, because a welcome squeezed into wrapping is worse than
//! an empty one.
//!
//! The keys come from the key table rather than from strings here, so a
//! rebound key changes the screen instead of leaving it lying. The working
//! directory is on it because the file picker only ever searches that one
//! tree, which is worth knowing before pressing the key that opens it.

use std::path::Path;

use ratatui::{
    buffer::Buffer as CellBuffer,
    layout::Rect,
    style::{Color, Style},
    widgets::Widget,
};
use unicode_width::UnicodeWidthStr;

use crate::{
    app::App,
    command::Command,
    keymap::Keymap,
    theme::Theme,
    ui::{put, write},
};

/// The commands worth naming, in the order they are shown.
const OFFERED: &[Command] = &[Command::FileOpen, Command::CommandPalette, Command::Quit];

/// The name, on a plate.
///
/// Block elements and box drawing, which every monospace font has -- unlike
/// the private use area the file glyphs come from, so this is the one thing
/// on screen that needs no particular font.
///
/// The letters are drawn at half a row's resolution: a stroke that ends
/// halfway down a cell ends in a half block, so the round ends of the O, the
/// S and the U are curves rather than the square steps a whole cell gives.
///
/// The frame is part of the mark rather than drawn around it, so the sheen
/// runs through it -- the ramp is taken per column across whatever is here,
/// and a frame outside it would be the one still thing on a moving screen.
const WORDMARK: &[&str] = &[
    "╔═════════════════════════════════════════════════╗",
    "║ ▄█████▄ ██████▄ ███████ ██      ██   ██ ▄█████▄ ║",
    "║ ██   ██ ██   ██ ██      ██      ██   ██ ██      ║",
    "║ ██   ██ ██████  ██████  ██      ██   ██ ▀█████▄ ║",
    "║ ██   ██ ██   ██ ██      ██      ██   ██      ██ ║",
    "║ ▀█████▀ ██████▀ ███████ ███████ ▀█████▀ ▀█████▀ ║",
    "╚═════════════════════════════════════════════════╝",
];

/// How many colours the ramp across the wordmark is made of.
///
/// Quantized rather than continuous. Forty-odd distinct colours across
/// forty-odd columns look no smoother than eight bands of six, and they turn
/// a golden fixture's legend into forty lines nobody can read.
const RAMP_STEPS: u16 = 8;

/// How many ticks the ramp takes to travel its own length once.
///
/// Sixteen at twelve a second, so a little over a second a cycle: slow enough
/// to read as a sheen moving across the letters rather than as something
/// flashing, and a whole number of steps so the cycle has no seam.
const CYCLE: u32 = RAMP_STEPS as u32 * 2;

/// What the name means, and what the thing is for.
const TAGLINE: &str = "read the code you didn't write";

/// The centred block.
pub struct WelcomeView<'a> {
    keymap: &'a Keymap,
    working_directory: &'a Path,
    theme: &'a Theme,
    /// How far the ramp has travelled, in ticks.
    ///
    /// Zero unless something is ticking, and nothing ticks once a file is
    /// open or when the session is remote, so this is the only thing that
    /// makes the screen differ between two draws.
    phase: u32,
}

impl<'a> WelcomeView<'a> {
    /// Borrows what the view needs.
    #[must_use]
    pub fn new(app: &'a App) -> Self {
        Self {
            keymap: app.keymap(),
            working_directory: app.working_directory(),
            theme: app.theme(),
            phase: app.phase(),
        }
    }
}

/// One key and what it does.
struct Hint {
    key: String,
    text: String,
}

impl Widget for WelcomeView<'_> {
    fn render(self, area: Rect, cells: &mut CellBuffer) {
        let hints = self.hints();
        let footer = home_relative(self.working_directory);

        // The wordmark, a blank, the tagline, a blank, a rule, a blank, the
        // hints, a blank, the footer.
        let tall = u16::try_from(WORDMARK.len() + hints.len() + 6).unwrap_or(u16::MAX);
        let wordmark = width_of(WORDMARK[0]);
        let short = u16::try_from(hints.len() + 4).unwrap_or(u16::MAX);

        if wordmark <= area.width && tall <= area.height {
            self.lavish(area, cells, &hints, &footer, wordmark, tall);
        } else if let Some(narrow) = hint_block_width(&hints)
            && narrow <= area.width
            && short <= area.height
        {
            self.compact(area, cells, &hints, &footer, narrow, short);
        }
    }
}

impl WelcomeView<'_> {
    /// The wordmark, the keys as caps, and a footer.
    fn lavish(
        &self,
        area: Rect,
        cells: &mut CellBuffer,
        hints: &[Hint],
        footer: &str,
        width: u16,
        height: u16,
    ) {
        let left = area.x + (area.width - width) / 2;
        let mut y = area.y + (area.height - height) / 2;

        // A ramp across the letters, in the theme's own accent hues rather
        // than in colours invented here, so it belongs to whichever theme is
        // on.
        let from = self.theme.syntax.keyword;
        let to = self.theme.syntax.function;
        for row in WORDMARK {
            let mut column = 0u16;
            for character in row.chars() {
                if character != ' ' {
                    let step = u32::from(column * RAMP_STEPS / width.max(1));
                    put(
                        cells,
                        left + column,
                        y,
                        character,
                        Style::new().fg(sheen(from, to, step, self.phase)),
                    );
                }
                column = column.saturating_add(1);
            }
            y += 1;
        }

        y += 1;
        centred(cells, left, y, width, TAGLINE, self.theme.gutter);
        y += 2;

        // A rule the full width of the block, which is what makes the
        // wordmark read as a heading rather than as decoration.
        for column in 0..width {
            put(
                cells,
                left + column,
                y,
                '\u{2500}',
                Style::new().fg(self.theme.gutter),
            );
        }
        y += 2;

        let keys = hints.iter().map(|hint| hint.key.width()).max().unwrap_or(0);
        for hint in hints {
            self.hint_row(cells, left, y, keys, hint, true);
            y += 1;
        }

        y += 1;
        write(cells, left, y, footer, Style::new().fg(self.theme.gutter));
        let version = concat!("v", env!("CARGO_PKG_VERSION"));
        if let Ok(offset) = u16::try_from(usize::from(width).saturating_sub(version.width())) {
            write(
                cells,
                left + offset,
                y,
                version,
                Style::new().fg(self.theme.gutter),
            );
        }
    }

    /// The keys, and nothing that needs room.
    fn compact(
        &self,
        area: Rect,
        cells: &mut CellBuffer,
        hints: &[Hint],
        footer: &str,
        width: u16,
        height: u16,
    ) {
        let left = area.x + (area.width - width) / 2;
        let mut y = area.y + (area.height - height) / 2;

        write(
            cells,
            left,
            y,
            "obelus",
            Style::new().fg(self.theme.foreground),
        );
        y += 2;

        let keys = hints.iter().map(|hint| hint.key.width()).max().unwrap_or(0);
        for hint in hints {
            self.hint_row(cells, left, y, keys, hint, false);
            y += 1;
        }

        y += 1;
        write(cells, left, y, footer, Style::new().fg(self.theme.gutter));
    }

    /// One key and its description, the keys right-aligned into their column
    /// so the descriptions start together.
    fn hint_row(
        &self,
        cells: &mut CellBuffer,
        left: u16,
        y: u16,
        keys: usize,
        hint: &Hint,
        capped: bool,
    ) {
        let Ok(pad) = u16::try_from(keys - hint.key.width()) else {
            return;
        };
        if capped {
            // A raised panel behind the key, so it reads as something to
            // press rather than as more prose.
            let style = Style::new()
                .fg(self.theme.foreground)
                .bg(self.theme.raised_background);
            let capped = format!(" {} ", hint.key);
            write(cells, left + pad, y, &capped, style);
            let Ok(offset) = u16::try_from(keys + 4) else {
                return;
            };
            write(
                cells,
                left + offset,
                y,
                &hint.text,
                Style::new().fg(self.theme.gutter),
            );
        } else {
            write(
                cells,
                left + pad,
                y,
                &hint.key,
                Style::new().fg(self.theme.foreground),
            );
            let Ok(offset) = u16::try_from(keys + 3) else {
                return;
            };
            write(
                cells,
                left + offset,
                y,
                &hint.text,
                Style::new().fg(self.theme.gutter),
            );
        }
    }

    fn hints(&self) -> Vec<Hint> {
        OFFERED
            .iter()
            .filter_map(|command| {
                let chord = self.keymap.chord_for(*command)?;
                Some(Hint {
                    key: chord.label(),
                    text: command.spec().title.to_lowercase(),
                })
            })
            .collect()
    }
}

/// How wide the compact block has to be.
fn hint_block_width(hints: &[Hint]) -> Option<u16> {
    let keys = hints.iter().map(|hint| hint.key.width()).max()?;
    let widest = hints.iter().map(|hint| hint.text.width()).max()?;
    u16::try_from(keys + 3 + widest).ok()
}

fn width_of(row: &str) -> u16 {
    u16::try_from(row.width()).unwrap_or(u16::MAX)
}

/// A colour `along` of the way from one to another.
/// The colour of one step of the wordmark at one moment.
///
/// The ramp runs from `from` to `to` and back again over [`CYCLE`] steps, and
/// the phase slides the whole thing along. Out and back rather than round,
/// because a ramp that wraps from its last colour straight to its first has a
/// visible seam travelling across the letters -- which reads as a glitch,
/// not as a sheen.
fn sheen(from: Color, to: Color, step: u32, phase: u32) -> Color {
    let at = (step + phase) % CYCLE;
    let half = CYCLE / 2;
    // The way back, mirrored.
    let along = if at < half { at } else { CYCLE - at };
    #[expect(
        clippy::cast_precision_loss,
        reason = "both are below CYCLE, which is sixteen"
    )]
    ramp(from, to, along as f32 / half as f32)
}

fn ramp(from: Color, to: Color, along: f32) -> Color {
    let (Color::Rgb(fr, fg, fb), Color::Rgb(tr, tg, tb)) = (from, to) else {
        // A theme in named colours has nothing to interpolate between.
        return from;
    };
    let mix = |a: u8, b: u8| {
        let a = f32::from(a);
        let b = f32::from(b);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the result of mixing two u8s along [0, 1] is within u8"
        )]
        {
            (a + (b - a) * along.clamp(0.0, 1.0)) as u8
        }
    };
    Color::Rgb(mix(fr, tr), mix(fg, tg), mix(fb, tb))
}

/// The path as a reader would write it, with the home directory as `~`.
fn home_relative(path: &Path) -> String {
    let Some(home) = std::env::var_os("HOME") else {
        return path.display().to_string();
    };
    match path.strip_prefix(Path::new(&home)) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

fn centred(cells: &mut CellBuffer, left: u16, y: u16, width: u16, contents: &str, colour: Color) {
    let Ok(text) = u16::try_from(contents.width()) else {
        return;
    };
    let offset = width.saturating_sub(text) / 2;
    write(cells, left + offset, y, contents, Style::new().fg(colour));
}
