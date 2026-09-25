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
    Weight, fontdb,
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
    /// The faces to try, in order: the reader's, and then whatever this
    /// machine calls its monospaced one.
    ///
    /// The last of them is why a reader who has chosen nothing gets the
    /// face their terminal already draws code in rather than a name
    /// cosmic-text has written into itself -- see [`crate::monospace`].
    families: Vec<String>,
    /// That last one, kept so that changing the reader's list does not
    /// mean asking the platform again.
    otherwise: Option<String>,
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
            families: Vec::new(),
            otherwise: crate::monospace::here(),
        };
        fonts.families = chain(&[], fonts.otherwise.as_deref());
        fonts.measure();
        fonts
    }

    /// Draws in these faces from now on, tried in this order.
    ///
    /// Nothing is refused: a name this machine does not have is stepped
    /// over when it comes to be drawn with, because one settings file is
    /// read on every machine the reader uses.
    pub(crate) fn use_families(&mut self, names: &[String]) {
        let wanted = chain(names, self.otherwise.as_deref());
        if self.families == wanted {
            return;
        }
        self.families = wanted;
        self.shaped.clear();
        self.measure();
    }

    /// What the faces on this machine are called, for the reader to choose
    /// between.
    ///
    /// One name per family rather than one per face: what a reader picks
    /// is `JetBrains Mono`, and the four files behind it are the weights
    /// and the slants of the same face.
    pub(crate) fn here(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .system
            .db()
            .faces()
            .filter_map(|face| face.families.first().map(|(name, _)| name.clone()))
            .collect();
        names.sort_unstable();
        names.dedup();
        names
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
        // The reader's first face, because that is the one a column is
        // counted in: a grid measured on the machine's monospace and drawn
        // in something else would have a column of one width and a letter
        // of another.
        let first = self.families.first().cloned();
        let family = first.as_deref().map_or(Family::Monospace, Family::Name);
        let mut buffer = Buffer::new(&mut self.system, self.metrics);
        let mut measuring = buffer.borrow_with(&mut self.system);
        measuring.set_size(None, None);
        measuring.set_text(
            // A letter rather than a space: a space's advance is the same
            // in a monospaced face and is not in a fallback one, and what
            // is being measured is the face that was actually chosen.
            "M",
            &Attrs::new().family(family),
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
        let families = &self.families;
        self.shaped
            .entry(key)
            .or_insert_with(|| shape(system, metrics, width, families, text, weight, style))
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
    families: &[String],
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
    let mark = text.chars().next().is_some_and(is_a_mark);
    let family = match mark {
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
    // The reader's list, in their order, and then whatever the machine
    // would have chosen. A name that is not here, or is here and does not
    // cover this character, is stepped over: what says so is which face
    // the glyphs actually came from, because cosmic-text will quietly
    // substitute one of its own and a chain that did not notice would
    // stop at the first name every time.
    let (placed, drawn) = match mark {
        true => lay(system, metrics, text, &attrs),
        false => tried(system, metrics, families, text, &attrs),
    };
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

/// The faces to try, in order.
///
/// What the reader asked for, and then what this machine calls its
/// monospaced face -- which is the answer when they asked for nothing, and
/// the one under them when a character is in none of theirs. Not added
/// twice where they have named it themselves.
fn chain(names: &[String], otherwise: Option<&str>) -> Vec<String> {
    let mut chain: Vec<String> = names.to_vec();
    if let Some(last) = otherwise
        && !chain.iter().any(|name| name.eq_ignore_ascii_case(last))
    {
        chain.push(last.to_string());
    }
    chain
}

/// Lays the text out in the first of the reader's faces that draws it.
///
/// "Draws it" is asked of the answer rather than of the font database: a
/// family that is installed but has no glyph for this character is a family
/// that did not draw it, and cosmic-text's own fallback will have put
/// something else there without being asked. So the face each glyph came
/// from is compared with the face that was asked for, and a substitution
/// counts as a miss.
///
/// Where none of them draws it -- or the reader has named none -- the last
/// attempt stands, which is the machine's own answer and is what a reader
/// who has said nothing gets.
fn tried(
    system: &mut FontSystem,
    metrics: Metrics,
    families: &[String],
    text: &str,
    attrs: &Attrs<'_>,
) -> (Vec<Placed>, f32) {
    let mut last = None;
    for family in families {
        let wanted: Vec<fontdb::ID> = system
            .db()
            .faces()
            .filter(|face| {
                face.families
                    .iter()
                    .any(|(name, _)| name.eq_ignore_ascii_case(family))
            })
            .map(|face| face.id)
            .collect();
        if wanted.is_empty() {
            // Not on this machine, which the list the reader built says
            // about itself as well.
            continue;
        }
        let asked = Attrs {
            family: Family::Name(family),
            ..attrs.clone()
        };
        let (placed, drawn, faces) = laid(system, metrics, text, &asked);
        if !faces.is_empty() && faces.iter().all(|face| wanted.contains(face)) {
            return (placed, drawn);
        }
        last = Some((placed, drawn));
    }
    // Nothing of theirs drew it. Whatever the last attempt put there is
    // still a drawing of this character, and where they named nothing at
    // all there is no attempt to keep.
    last.unwrap_or_else(|| lay(system, metrics, text, attrs))
}

/// One laying out, and how wide it came out.
fn lay(
    system: &mut FontSystem,
    metrics: Metrics,
    text: &str,
    attrs: &Attrs<'_>,
) -> (Vec<Placed>, f32) {
    let (placed, drawn, _) = laid(system, metrics, text, attrs);
    (placed, drawn)
}

/// The same, and which faces the glyphs came from.
fn laid(
    system: &mut FontSystem,
    metrics: Metrics,
    text: &str,
    attrs: &Attrs<'_>,
) -> (Vec<Placed>, f32, Vec<fontdb::ID>) {
    let mut buffer = Buffer::new(system, metrics);
    let mut shaping = buffer.borrow_with(system);
    shaping.set_size(None, None);
    shaping.set_text(text, attrs, Shaping::Advanced, None);
    let mut placed = Vec::new();
    let mut faces = Vec::new();
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
            faces.push(glyph.font_id);
        }
    }
    (placed, drawn, faces)
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

#[cfg(test)]
mod chains {
    use super::*;

    /// What this machine calls monospaced is the last thing tried, and a
    /// reader who chose nothing gets it first.
    ///
    /// Which is the whole of the fix: `Family::Monospace` resolves to a
    /// name cosmic-text wrote into itself, and on a machine where that is
    /// not what `fc-match monospace` says, a window with no setting drew
    /// in a face nothing else on the screen was using.
    ///
    /// Deliberate break: leaving the platform's face out makes the first
    /// of these empty, and an empty chain is `Family::Monospace` again.
    #[test]
    fn the_machines_own_face_is_under_the_readers() {
        assert_eq!(chain(&[], Some("JetBrains Mono")), ["JetBrains Mono"]);
        assert_eq!(
            chain(&["Iosevka".to_string()], Some("JetBrains Mono")),
            ["Iosevka", "JetBrains Mono"]
        );
    }

    /// And it is not added twice where the reader named it themselves.
    ///
    /// Deliberate break: pushing it unconditionally puts the same face in
    /// the chain twice, which is a second shaping of every character it
    /// fails to draw.
    #[test]
    fn a_face_the_reader_named_is_not_added_again() {
        assert_eq!(
            chain(&["JetBrains Mono".to_string()], Some("JetBrains Mono")),
            ["JetBrains Mono"]
        );
        // However it is spelled: a family name is a name, and fontconfig
        // and a reader do not have to agree about its case.
        assert_eq!(
            chain(&["jetbrains mono".to_string()], Some("JetBrains Mono")),
            ["jetbrains mono"]
        );
    }

    /// A machine that says nothing leaves the chain to the reader alone.
    ///
    /// Deliberate break: putting an empty name in the chain asks for a
    /// family called nothing, once per character.
    #[test]
    fn a_machine_with_no_answer_adds_nothing() {
        assert!(chain(&[], None).is_empty());
        assert_eq!(chain(&["Iosevka".to_string()], None), ["Iosevka"]);
    }
}
