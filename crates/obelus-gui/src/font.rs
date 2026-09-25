//! The faces, the size of a cell, and where a glyph's pixels come from.
//!
//! Two things are decided here and nothing else decides them: how wide a
//! cell is, which is the advance of the chosen monospaced face and the
//! measure every column on the screen is counted in; and which face a
//! character is drawn from.
//!
//! The marks are carried rather than hoped for. `Symbols Nerd Font Mono` is
//! compiled into the binary and asked for by name whenever the character is
//! one of the ones it exists for, which is the other half of why a window is
//! worth having: in a terminal a glyph is there if the reader installed a
//! patched font, and `icons::NERD_FONT` is Obelus guessing. Here it is a
//! fact about the binary.
//!
//! Its own layout is not used. Each cell is shaped by itself and put where
//! the grid says, because the grid is the layout -- asking a text engine to
//! lay out a screenful of cells would be asking it a question that is
//! already answered, and answering it differently.

use std::collections::HashMap;

use cosmic_text::{
    Attrs, Buffer, CacheKey, Family, FontSystem, Metrics, Shaping, Style, SwashCache, SwashImage,
    Weight,
};

/// The marks, carried.
///
/// Symbols only: letters, digits and CJK come from the fonts the machine
/// has, because a face covering those is ten times this one and which face
/// prose is set in is the reader's to choose. The licence and what it
/// covers are beside the file.
const SYMBOLS: &[u8] = include_bytes!("../fonts/SymbolsNerdFontMono-Regular.ttf");

/// What the symbols face is called, once it is loaded.
const SYMBOLS_FAMILY: &str = "Symbols Nerd Font Mono";

/// How tall a line is, as a multiple of the font's size.
///
/// A terminal's row is the font's own line height, which varies by face and
/// is not something a reader chose. This is the ratio every terminal that
/// lets you set one defaults to, and it keeps the grid the same shape
/// whichever face the machine turns out to have.
const LINE_HEIGHT: f32 = 1.2;

/// One glyph, ready to be asked for by picture.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Placed {
    /// What to ask the cache for.
    pub(crate) key: CacheKey,
    /// Where it goes, from the cell's own corner.
    pub(crate) x: i32,
    /// And how far down, from the cell's baseline.
    pub(crate) y: i32,
}

/// How big a cell is, in real pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CellSize {
    /// How wide.
    pub(crate) width: f32,
    /// How tall.
    pub(crate) height: f32,
    /// How far down a cell the baseline sits, which is where a glyph is
    /// drawn from.
    pub(crate) baseline: f32,
}

/// Every face this machine can draw with, and what they measure.
pub(crate) struct Fonts {
    system: FontSystem,
    pictures: SwashCache,
    metrics: Metrics,
    cell: CellSize,
    /// What a cell's text shapes to, kept because a screen is the same few
    /// hundred cell contents over and over: without it every frame reshapes
    /// every cell, and shaping is the expensive half of drawing text.
    shaped: HashMap<(String, Weight, Style), Vec<Placed>>,
}

impl std::fmt::Debug for Fonts {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Fonts")
            .field("metrics", &self.metrics)
            .field("cell", &self.cell)
            .finish_non_exhaustive()
    }
}

impl Fonts {
    /// Loads what this machine has, and the marks Obelus brought with it.
    ///
    /// `size` is in real pixels, which is the size in points multiplied by
    /// whatever the window says the screen's scale is.
    pub(crate) fn new(size: f32) -> Self {
        let mut system = FontSystem::new();
        // Loaded into the same database the system's own faces are in, so
        // that asking for it by name is the ordinary path rather than a
        // second one.
        system.db_mut().load_font_data(SYMBOLS.to_vec());

        let mut fonts = Self {
            system,
            pictures: SwashCache::new(),
            metrics: Metrics::new(size, (size * LINE_HEIGHT).round()),
            // Replaced immediately below, once there is something to
            // measure with.
            cell: CellSize {
                width: size,
                height: size,
                baseline: size,
            },
            shaped: HashMap::new(),
        };
        fonts.measure();
        fonts
    }

    /// Draws at a different size from now on, which is a new window scale
    /// or a reader changing it.
    pub(crate) fn resize(&mut self, size: f32) {
        self.metrics = Metrics::new(size, (size * LINE_HEIGHT).round());
        self.shaped.clear();
        self.measure();
    }

    /// How big a cell is.
    pub(crate) const fn cell(&self) -> CellSize {
        self.cell
    }

    /// Works out the cell from the face rather than from a guess.
    ///
    /// The advance of a letter in the monospaced face is the width every
    /// column is counted in; rounded, because a grid on fractional pixels
    /// is a grid whose columns do not line up with each other.
    fn measure(&mut self) {
        let mut buffer = Buffer::new(&mut self.system, self.metrics);
        let mut measuring = buffer.borrow_with(&mut self.system);
        measuring.set_size(None, None);
        measuring.set_text(
            // A letter rather than a space: a space's advance is the same
            // in a monospaced face and is not in a fallback one, and what
            // is being measured is the face that was actually chosen.
            "M",
            &Attrs::new().family(Family::Monospace),
            Shaping::Advanced,
            None,
        );
        let mut width = self.metrics.font_size * 0.6;
        let mut baseline = self.metrics.line_height * 0.8;
        for run in measuring.layout_runs() {
            baseline = run.line_y;
            if let Some(glyph) = run.glyphs.first() {
                width = glyph.w;
            }
        }
        self.cell = CellSize {
            width: width.round().max(1.0),
            height: self.metrics.line_height.max(1.0),
            baseline: baseline.round(),
        };
    }

    /// What one cell's text is drawn as, placed in the cells it occupies.
    ///
    /// Shaped once per distinct string and kept: a screenful is a few
    /// hundred different cells however many rows it has.
    pub(crate) fn glyphs(&mut self, text: &str, bold: bool, italic: bool) -> &[Placed] {
        let weight = match bold {
            true => Weight::BOLD,
            false => Weight::NORMAL,
        };
        let style = match italic {
            true => Style::Italic,
            false => Style::Normal,
        };
        let key = (text.to_string(), weight, style);
        // Two disjoint fields, which is why the shaping is a free function:
        // the cache is borrowed for the entry and the font system for the
        // work inside it.
        let system = &mut self.system;
        let metrics = self.metrics;
        let width = self.cell.width;
        self.shaped
            .entry(key)
            .or_insert_with(|| shape(system, metrics, width, text, weight, style))
    }

    /// The pixels of one glyph, or nothing where the face has none.
    pub(crate) fn picture(&mut self, key: CacheKey) -> Option<&SwashImage> {
        self.pictures.get_image(&mut self.system, key).as_ref()
    }
}

/// Lays out one cell's worth of text, in the cells that cell occupies.
///
/// How many that is comes from [`obelus_text::text_width`], which is the
/// same question the application asked when it decided which column the
/// next character goes in. Asking it any other way here would be a second
/// answer, and the two would disagree about exactly the characters that are
/// hard: a full-width one, an emoji, a mark that combines.
///
/// Within that room the glyph is left the size the face drew it and put in
/// the middle. A Latin face's advance is about three fifths of its size and
/// a CJK face's is the whole of it, so two cells of Latin is wider than one
/// full-width character: the difference is a little air either side, which
/// is what every terminal shows. It used to be handed to cosmic-text's
/// `monospace_width`, which closes that gap by *scaling the glyph up* until
/// its advance is a whole number of cells -- a fifth bigger, drawn edge to
/// edge with its neighbours, so a line of Chinese was visibly a different
/// size from the Latin above it.
///
/// Scaled down only where it does not fit, which is the other direction and
/// not a matter of taste: a glyph wider than the cells it was given is a
/// glyph drawn over the text beside it.
fn shape(
    system: &mut FontSystem,
    metrics: Metrics,
    width: f32,
    text: &str,
    weight: Weight,
    style: Style,
) -> Vec<Placed> {
    // At least one: a cell that measures zero -- a combining mark on its
    // own, a zero-width space -- still has the cell it was written into.
    let columns = obelus_text::text_width(text).max(1);
    #[expect(
        clippy::cast_precision_loss,
        reason = "a cell is one or two columns, never billions"
    )]
    let room = columns as f32 * width;
    let family = match text.chars().next().is_some_and(is_a_mark) {
        // Asked for by name rather than left to the fallback chain. What
        // Obelus's marks are is a private use area code point, and a
        // fallback chain is organised by script: nothing about `\u{e702}`
        // says which face covers it, so a chain walks every face on the
        // machine and settles on whichever answers first -- which is
        // whatever patched font the reader happens to have installed, and
        // that is the guess the window exists to stop making.
        true => Family::Name(SYMBOLS_FAMILY),
        false => Family::Monospace,
    };
    let attrs = Attrs::new().family(family).weight(weight).style(style);
    let (placed, drawn) = lay(system, metrics, text, &attrs);
    // Too wide for its cells, which happens where a fallback face is not a
    // monospaced one at all. Drawn again at the size that fits rather than
    // squeezed afterwards: a bitmap stretched sideways is a blurred letter,
    // and the glyph has not been rasterised yet.
    let (mut placed, drawn) = match drawn > room + 0.5 {
        true => lay(
            system,
            Metrics::new(metrics.font_size * room / drawn, metrics.line_height),
            text,
            &attrs,
        ),
        false => (placed, drawn),
    };
    #[expect(
        clippy::cast_possible_truncation,
        reason = "half the difference between two widths of a cell or two"
    )]
    let middle = ((room - drawn) / 2.0).round() as i32;
    for glyph in &mut placed {
        glyph.x += middle;
    }
    placed
}

/// One laying out, and how wide it came out.
fn lay(
    system: &mut FontSystem,
    metrics: Metrics,
    text: &str,
    attrs: &Attrs<'_>,
) -> (Vec<Placed>, f32) {
    let mut buffer = Buffer::new(system, metrics);
    let mut shaping = buffer.borrow_with(system);
    shaping.set_size(None, None);
    shaping.set_text(text, attrs, Shaping::Advanced, None);
    let mut placed = Vec::new();
    let mut drawn: f32 = 0.0;
    for run in shaping.layout_runs() {
        drawn = drawn.max(run.line_w);
        for glyph in run.glyphs {
            let physical = glyph.physical((0.0, 0.0), 1.0);
            placed.push(Placed {
                key: physical.cache_key,
                x: physical.x,
                y: physical.y,
            });
        }
    }
    (placed, drawn)
}

/// Whether this is one of the marks Obelus carries its own face for.
///
/// The three private use areas, which is where every Nerd Font glyph lives:
/// the basic one and the two whole planes above it. A character outside them
/// is somebody's writing and belongs to the machine's own fonts.
fn is_a_mark(character: char) -> bool {
    matches!(character, '\u{e000}'..='\u{f8ff}' | '\u{f0000}'..='\u{ffffd}' | '\u{100000}'..='\u{10fffd}')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The marks Obelus draws are asked of the face Obelus carries.
    ///
    /// Deliberate break: leaving the private use areas out of `is_a_mark`
    /// -- which is what a fallback chain would do for them -- fails here.
    #[test]
    fn a_mark_is_not_left_to_the_machine() {
        // The mark a Rust file wears, from `obelus-icons`.
        assert!(is_a_mark('\u{e7a8}'));
        // And one of the two planes above the basic area.
        assert!(is_a_mark('\u{f0a0e}'));
        // Letters, digits and CJK are the machine's.
        assert!(!is_a_mark('a'));
        assert!(!is_a_mark('读'));
    }
}
