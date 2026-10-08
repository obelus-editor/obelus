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
//! Nor is a mark the same width in both. A terminal draws with the font
//! the reader installed, whose non-`Mono` variants take two columns; a
//! window draws with the `Mono` one carried here, fitted to the one cell it
//! was given. So a measurement in columns has to ask which front end it is
//! for, and the welcome screen's `drawn` does -- a column a cell too wide
//! put its keys a cell left of where the column said, which is a gap nobody
//! could see until a cap was drawn round them.
//!
//! Its own layout is not used. Each cell is shaped by itself and put where
//! the grid says, because the grid is the layout -- asking a text engine to
//! lay out a screenful of cells would be asking it a question that is
//! already answered, and answering it differently.
//!
//! **A key's cap is the one place the grid is not what a cell is measured
//! in.** Everything on the screen is a row of the grid at the grid's size,
//! except the legend in a keycap: a letter the full height of the row
//! reaches both edges of the cap and hangs its descender out of the
//! bottom, which is a cap the key is too big for. So `Size` says which,
//! and it says it three times over -- the font's size, the line's height,
//! and the room a glyph is middled in -- because the legend is smaller,
//! sits in the middle of a line of its own rather than on the row's
//! baseline, and is laid out at a pitch of its own so the letters close
//! up. Each of the three is drawn by a different piece of the frame, so
//! each has a test and a break of its own.
//!
//! And a cell is still shaped a cell at a time in a cap, which is what
//! decides the *shape* of the cache: a map per way of drawing, each
//! holding the strings drawn that way. One map keyed by the way and the
//! text together would have to own the string to ask the question, which
//! is an allocation per cell per frame for a screenful that was already
//! in it.

use std::{collections::HashMap, sync::Arc};

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
pub(crate) const SYMBOLS_FAMILY: &str = "Symbols Nerd Font Mono";

/// The selector that asks for the character before it to be drawn as a
/// picture.
const PICTURE: char = '\u{fe0f}';

/// The selector that asks for the character before it to be drawn as
/// text, which is a reason not to turn to a picture face for it.
const WORDS: char = '\u{fe0e}';

/// The faces that draw pictures: one per system that ships one, and the one
/// a reader installs for themselves. Whichever this machine has.
const PICTURES: [&str; 4] = [
    "Segoe UI Emoji",
    "Apple Color Emoji",
    "Noto Color Emoji",
    "Twemoji",
];

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
    /// The baseline this glyph's own layout asked for, where that is not
    /// the one the grid is counted in.
    ///
    /// `None` for writing, which goes on the grid's baseline because that
    /// is what a line of text *is*: a letter from a fallback face put on
    /// its own would be a letter stepping out of the line it is in.
    ///
    /// `Some` for a mark, which shares nothing with the letters beside it.
    /// It is a picture, carried in a face of Obelus's own, and it is drawn
    /// at whatever size fits the cell rather than at the size the prose is
    /// -- `Symbols Nerd Font Mono` designs its glyphs a whole cell wide,
    /// so almost every one of them is laid out again smaller. Both of
    /// those move the baseline it wants: at a sixteen-pixel size in a
    /// nineteen-pixel line, the reader's face asks for 15.71, the symbols
    /// face for 14.30, and the same mark shrunk to fit a ten-pixel cell
    /// for 12.50. Drawn on the first of those, every mark in the window
    /// hung three pixels low -- a quarter of a letter's height, and
    /// visibly below where a terminal puts the same key.
    ///
    /// The face says all of this; nothing here measures pixels to find it
    /// out. Sideways the same question was settled the same way: `shape`
    /// puts a mark in the middle of the room it was given.
    pub(crate) baseline: Option<f32>,
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
    ///
    /// A map per way of drawing, rather than one keyed by the way *and* the
    /// text: there are eight of the first and thousands of the second, and
    /// only this shape can be asked with the text borrowed.
    shaped: HashMap<Face, HashMap<String, Vec<Placed>>>,
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
    /// What this machine falls back to after its monospaced face, in its
    /// order -- see [`crate::cascade`]. Kept for the same reason.
    cascade: Vec<String>,
    /// The same, less what is already in `families`: what is tried once
    /// neither those nor a picture face drew the character.
    after: Vec<String>,
    /// The faces CoreText draws rather than swash, by the name CoreText
    /// knows each by -- see [`crate::coretext`]. Empty on every other
    /// platform.
    by_coretext: HashMap<fontdb::ID, crate::coretext::Names>,
    /// What CoreText drew of them, kept the way `pictures` keeps swash's.
    drawn_by_coretext: HashMap<CacheKey, Option<SwashImage>>,
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
        let by_coretext = crate::faces::settle(system.db_mut());
        // Loaded into the same database the system's own faces are in, so
        // that asking for it by name is the ordinary path rather than a
        // second one.
        system.db_mut().load_font_data(SYMBOLS.to_vec());
        // Both asked once the database is whole: each is kept to the names
        // it has.
        let otherwise = crate::monospace::here(system.db());
        let cascade = crate::cascade::here(system.db(), otherwise.as_deref());
        let by_coretext = crate::coretext::names(system.db(), by_coretext, otherwise.as_deref());

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
            otherwise,
            cascade,
            after: Vec::new(),
            by_coretext,
            drawn_by_coretext: HashMap::new(),
        };
        fonts.families = chain(&[], fonts.otherwise.as_deref());
        fonts.after = rest(&fonts.cascade, &fonts.families);
        fonts.measure();
        fonts.say_what_is_a_picture();
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
        self.after = rest(&self.cascade, &wanted);
        self.families = wanted;
        self.shaped.clear();
        self.measure();
        self.say_what_is_a_picture();
    }

    /// Tells the arithmetic which characters written as text are drawn as
    /// pictures here, so that it counts them two cells wide -- the width
    /// `shape` then draws them at.
    ///
    /// The ones none of the reader's faces has and a face that draws
    /// pictures does: those are the ones `shape` turns to a picture face
    /// for, and the ones every other program on the machine draws as the
    /// picture, at the size of one. A character one of theirs has is drawn
    /// in theirs, as the one cell it is.
    ///
    /// Asked of the faces' character maps rather than by laying anything
    /// out: a few hundred characters, once per change of fonts.
    ///
    /// Every `Fonts::new` in this binary's tests writes the table too, which
    /// bends the rule that a test setting a global has a binary of its own:
    /// `obg` has no library for a test binary of its own to link against.
    /// What makes it safe is that every one of them writes the same table --
    /// one machine, the same faces, the same default -- so a test beside
    /// them that measures a picture gets the same answer whichever ran first.
    fn say_what_is_a_picture(&mut self) {
        let faces_of = |system: &FontSystem, names: &[&str]| -> Vec<fontdb::ID> {
            system
                .db()
                .faces()
                .filter(|face| {
                    face.families.iter().any(|(family, _)| {
                        names.iter().any(|name| family.eq_ignore_ascii_case(name))
                    })
                })
                .map(|face| face.id)
                .collect()
        };
        let readers: Vec<&str> = self.families.iter().map(String::as_str).collect();
        let readers = faces_of(&self.system, &readers);
        let pictures = faces_of(&self.system, &PICTURES);
        let mut maps = |faces: &[fontdb::ID]| -> Vec<Arc<cosmic_text::Font>> {
            faces
                .iter()
                .filter_map(|id| self.system.get_font(*id, Weight::NORMAL))
                .collect()
        };
        let (readers, pictures) = (maps(&readers), maps(&pictures));
        let has = |fonts: &[Arc<cosmic_text::Font>], character: char| {
            fonts
                .iter()
                .any(|font| font.as_swash().charmap().map(character) != 0)
        };
        let drawn: Vec<char> = (0..0x2_0000u32)
            .filter_map(char::from_u32)
            .filter(|character| obelus_text::could_be_a_picture(*character))
            .filter(|character| !has(&readers, *character) && has(&pictures, *character))
            .collect();
        tracing::info!(pictures = drawn.len(), "what is drawn as a picture here");
        obelus_text::draw_as_pictures(&drawn);
    }

    /// What this machine calls its monospaced face, where it said.
    pub(crate) fn otherwise(&self) -> Option<&str> {
        self.otherwise.as_deref()
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
    pub(crate) fn glyphs(&mut self, text: &str, bold: bool, italic: bool, size: Size) -> &[Placed] {
        let face = Face {
            weight: match bold {
                true => Weight::BOLD,
                false => Weight::NORMAL,
            },
            style: match italic {
                true => Style::Italic,
                false => Style::Normal,
            },
            size,
        };
        // Smaller in a cap, and on its own line so that it sits in the
        // middle of one rather than on the writing's baseline: what a cap
        // holds is a key, not a word in a sentence.
        let metrics = match size {
            Size::Cell => self.metrics,
            Size::Capped => Metrics::new(
                self.metrics.font_size * SMALLER,
                self.metrics.line_height * SMALLER,
            ),
        };
        // Two disjoint fields, which is why the shaping is a free function:
        // the cache is borrowed for the entry and the font system for the
        // work inside it.
        let system = &mut self.system;
        // A cell of a cap is that much narrower too, because the legend is
        // laid out at its own pitch -- and this is what the glyph is
        // centred in and measured against.
        let width = match size {
            Size::Cell => self.cell.width,
            Size::Capped => self.cell.width * SMALLER,
        };
        let (families, after) = (&self.families, &self.after);
        let shaped = self.shaped.entry(face).or_default();
        // Asked with the text borrowed and copied only on a miss. One flat
        // map keyed by the whole lot would have to own the string to ask
        // the question, which is an allocation per cell per frame -- and a
        // screenful is a few thousand cells that were all in the cache.
        if !shaped.contains_key(text) {
            let placed = shape(system, metrics, width, families, after, text, face);
            shaped.insert(text.to_string(), placed);
        }
        &shaped[text]
    }

    /// The pixels of one glyph, or nothing where the face has none.
    pub(crate) fn picture(&mut self, key: CacheKey) -> Option<&SwashImage> {
        match self.by_coretext.get(&key.font_id) {
            Some(names) => self
                .drawn_by_coretext
                .entry(key)
                .or_insert_with(|| crate::coretext::draw(names.at(key.font_weight), key))
                .as_ref(),
            None => self.pictures.get_image(&mut self.system, key).as_ref(),
        }
    }
}

/// One way of drawing a cell's text: which face, at which of the two sizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Face {
    weight: Weight,
    style: Style,
    size: Size,
}

/// How big the text in a key's cap is, against the writing beside it.
///
/// A keycap's legend is smaller than the prose around it on every keyboard
/// there is, and for the same reason here: a letter drawn the full height
/// of the row fills the cap to its edges and its descender hangs out of the
/// bottom, which is a cap the key is too big for.
pub(crate) const SMALLER: f32 = 0.74;

/// How big a cell's text is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Size {
    /// The size the row is written in.
    Cell,
    /// Smaller, on a line of its own: what goes in a key's cap.
    Capped,
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
    after: &[String],
    text: &str,
    face: Face,
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
    let attrs = Attrs::new()
        .family(family)
        .weight(face.weight)
        .style(face.style);
    // The reader's list, in their order, and then whatever the machine
    // would have chosen. A name that is not here, or is here and does not
    // cover this character, is stepped over: what says so is which face
    // the glyphs actually came from, because cosmic-text will quietly
    // substitute one of its own and a chain that did not notice would
    // stop at the first name every time.
    //
    // A character asked to be drawn as a picture is asked of the faces that
    // draw pictures instead: a monospaced face with a `❤` in it has a line
    // drawing of one, and the selector after it is the writer saying that
    // is not what they meant. Not left to the fallback chain either, which
    // tries every monospaced face first and steps over any face with
    // `Emoji` in its name to do it.
    let asked: Vec<String>;
    let families = match text.contains(PICTURE) {
        true => {
            asked = pictures();
            &asked
        }
        false => families,
    };
    let (placed, drawn, used) = match mark {
        true => {
            let (placed, drawn) = lay(system, metrics, text, &attrs);
            (placed, drawn, None)
        }
        false => match tried(system, metrics, families, text, &attrs) {
            // In none of the reader's faces: the faces that draw pictures
            // before the fallback chain, which is what every other program
            // on the machine does -- an input method's `❤` comes without a
            // selector, and everywhere else the reader puts one it is red.
            // Only where one of them has it, so a character they do not
            // have still goes to the chain, and from the reader's attempt:
            // the chain prefers a face like the one it was asked for, and
            // asked for a picture face it puts Chinese in a proportional one.
            (placed, drawn, None) if !text.contains(PICTURE) && !text.contains(WORDS) => {
                match tried(system, metrics, &pictures(), text, &attrs) {
                    found @ (_, _, Some(_)) => found,
                    _ => (placed, drawn, None),
                }
            }
            found => found,
        },
    };
    // Still in none of them: what this machine falls back to, before
    // cosmic-text's own table of what it thinks the machine has. From the
    // attempt before it where the machine has nothing either, for the same
    // reason as above.
    let (placed, drawn, used) = match (mark, used) {
        (false, None) => match tried(system, metrics, after, text, &attrs) {
            found @ (_, _, Some(_)) => found,
            _ => (placed, drawn, None),
        },
        (_, used) => (placed, drawn, used),
    };
    // Too wide for its cells, which happens where a fallback face is not a
    // monospaced one at all. Drawn again at the size that fits rather than
    // squeezed afterwards: a bitmap stretched sideways is a blurred letter,
    // and the glyph has not been rasterised yet. In the face that drew it,
    // or what fits is some other face's drawing of the character.
    let (mut placed, drawn) = match drawn > room + 0.5 {
        true => lay(
            system,
            Metrics::new(metrics.font_size * room / drawn, metrics.line_height),
            text,
            &Attrs {
                family: used.as_deref().map_or(attrs.family, Family::Name),
                ..attrs.clone()
            },
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
        // Writing goes on the baseline the grid is counted in, whichever
        // face it came from: that is what keeps a line a line. A mark goes
        // on the one its own face asked for -- see `Placed::baseline` --
        // and so does a cap's legend, which is on a line of its own and
        // would otherwise sit on the writing's baseline with its head in
        // the middle of the cap.
        if !mark && face.size == Size::Cell {
            glyph.baseline = None;
        }
    }
    placed
}

/// [`PICTURES`], in the shape [`tried`] takes a chain in.
fn pictures() -> Vec<String> {
    PICTURES.iter().map(ToString::to_string).collect()
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

/// What the machine falls back to, less what is tried before it.
fn rest(cascade: &[String], families: &[String]) -> Vec<String> {
    cascade
        .iter()
        .filter(|name| {
            !families
                .iter()
                .any(|family| family.eq_ignore_ascii_case(name))
        })
        .cloned()
        .collect()
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
///
/// And which of them drew it, where one did.
fn tried(
    system: &mut FontSystem,
    metrics: Metrics,
    families: &[String],
    text: &str,
    attrs: &Attrs<'_>,
) -> (Vec<Placed>, f32, Option<String>) {
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
            return (placed, drawn, Some(family.clone()));
        }
        last = Some((placed, drawn));
    }
    // Nothing of theirs drew it. Whatever the last attempt put there is
    // still a drawing of this character, and where they named nothing at
    // all there is no attempt to keep.
    let (placed, drawn) = last.unwrap_or_else(|| lay(system, metrics, text, attrs));
    (placed, drawn, None)
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
                // What this layout asked for, at the face and the size it
                // actually used. Whether it is the one to draw on is
                // `shape`'s to say -- it is the only thing here that knows
                // whether this cell is a mark or writing.
                baseline: Some(run.line_y),
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

    /// A key in a cap is smaller than the row it sits in, is on a line of
    /// its own, and is told from the row by the cache.
    ///
    /// Three things, because `Size::Capped` is three lines in three
    /// places and each of them passes with the other two broken. The
    /// numbers are the machine's; what is asserted is the relationships.
    ///
    /// Deliberate breaks, one per assertion:
    ///
    /// * The cache, keyed with `Size::Cell` whatever was asked for: the second
    ///   ask hands back the run the first was given, so a cap is drawn at the
    ///   row's size -- and, worse, a row after a cap at the cap's.
    /// * The metrics, left at `self.metrics`: the legend goes on the row's
    ///   baseline with its head in the middle of the cap. It is still *small*,
    ///   because the room below shrinks it to fit -- which is exactly why this
    ///   needs an assertion of its own.
    /// * The room, left at `self.cell.width`: the glyph is middled in the cell
    ///   the grid gave it rather than in the pitch the legend is laid out at,
    ///   so a key of two characters is drawn a quarter of a cell right of where
    ///   `legend` put it.
    #[test]
    fn a_key_in_a_cap_is_smaller_than_the_row_it_is_in_and_on_its_own_line() {
        fn asked(fonts: &mut Fonts, size: Size) -> Vec<Placed> {
            fonts.glyphs("F1", false, false, size).to_vec()
        }
        fn sizes(placed: &[Placed]) -> Vec<f32> {
            placed
                .iter()
                .map(|glyph| f32::from_bits(glyph.key.font_size_bits))
                .collect()
        }
        let mut fonts = Fonts::new(16.0);
        let grid = fonts.cell().baseline;
        let row = asked(&mut fonts, Size::Cell);
        let cap = asked(&mut fonts, Size::Capped);
        assert!(!row.is_empty(), "nothing was laid out");
        assert_eq!(row.len(), cap.len(), "the same text laid out differently");

        // Asked for again, after the cap has been: a cache that had lost
        // the size would answer this one with the cap's run.
        assert_eq!(
            sizes(&row),
            sizes(&asked(&mut fonts, Size::Cell)),
            "a key drawn in a cap changed how the same word is drawn in a row"
        );
        for (row, cap) in sizes(&row).iter().zip(sizes(&cap)) {
            assert!(
                cap < *row,
                "a key was asked for at {cap}, the row it is in at {row}"
            );
        }

        // A line of its own, which is what `legend` middles in the cap:
        // writing has none and goes on the grid's -- see
        // `Placed::baseline`. Being merely *above* the grid's proves
        // nothing, because a run laid out in any face is: what says the
        // line shrank with the letters is that it shrank by the same
        // fraction. Loosely, since where in a line a face puts its
        // baseline is the face's own business.
        for glyph in &cap {
            let baseline = glyph.baseline.expect("a key is on a line of its own");
            let wanted = grid * SMALLER;
            assert!(
                (baseline - wanted).abs() < grid * 0.1,
                "a key's line sits at {baseline}, a line {SMALLER} of the grid's at {wanted}"
            );
        }

        // Middled in the pitch the legend is laid out at. Both runs are
        // monospaced text in the room it was measured for, so both start
        // hard against the left of it; a legend middled in the grid's
        // cells instead would be pushed right by what it did not fill.
        let (row, cap) = (row[0].x, cap[0].x);
        assert!(
            cap <= row,
            "a key starts {cap} into its room, the row it is in {row}"
        );
    }

    /// A heart is drawn in a face that draws pictures, and in colour --
    /// asked to be a picture or not, since an input method's comes without
    /// the selector and is red everywhere else on the machine. A character
    /// one of the reader's faces has, or one no picture face has, is not.
    ///
    /// On this machine's faces, so on one with none of `PICTURES` there is
    /// nothing to assert and it says so rather than passing quietly.
    ///
    /// Deliberate breaks: leaving `families` as the reader's whatever the
    /// text holds draws the first heart in whatever monospaced face has a
    /// line drawing of one -- MS Gothic on Windows -- and in one colour;
    /// taking out the turn to the picture faces after the reader's does the
    /// same to the second; turning to them first, ahead of the reader's,
    /// draws the `#` as a picture; and not telling the arithmetic which
    /// characters are pictures leaves the second heart one cell wide, drawn
    /// at a little over half the size of the face beside it.
    #[test]
    fn a_heart_is_drawn_in_colour_with_or_without_its_selector() {
        let mut fonts = Fonts::new(16.0);
        let has_pictures = fonts.here().iter().any(|name| {
            PICTURES
                .iter()
                .any(|picture| name.eq_ignore_ascii_case(picture))
        });
        if !has_pictures {
            eprintln!("no face that draws pictures on this machine; nothing to check");
            return;
        }
        // Which face the first glyph came from, whether that is a picture
        // face, and whether what it drew has colours of its own.
        let drawn_in = |fonts: &mut Fonts, text: &str| {
            let first = *fonts
                .glyphs(text, false, false, Size::Cell)
                .first()
                .expect("it was laid out");
            let names = fonts
                .system
                .db()
                .face(first.key.font_id)
                .expect("the face it was laid out in")
                .families
                .clone();
            let picture = names.iter().any(|(name, _)| {
                PICTURES
                    .iter()
                    .any(|picture| name.eq_ignore_ascii_case(picture))
            });
            let colour = fonts
                .picture(first.key)
                .is_some_and(|image| matches!(image.content, cosmic_text::SwashContent::Color));
            (picture, colour, names)
        };
        // As big as any other picture: given the two cells a picture is
        // given, and shrunk only as far as every picture is to fit two.
        // Given one, it is half the size of the face beside it.
        let size = |fonts: &mut Fonts, text: &str| {
            f32::from_bits(
                fonts.glyphs(text, false, false, Size::Cell)[0]
                    .key
                    .font_size_bits,
            )
        };
        let face = size(&mut fonts, "\u{1f600}");
        let as_a_picture = |fonts: &mut Fonts, heart: &str| {
            let (_, colour, _) = drawn_in(fonts, heart);
            assert!(colour, "{heart:?} was drawn in one colour");
            let drawn = size(fonts, heart);
            assert!(
                (drawn - face).abs() < 0.01,
                "{heart:?} was drawn at {drawn}, a face at {face}"
            );
        };

        // Asked for as a picture, it is one on any machine that has them.
        let (picture, _, names) = drawn_in(&mut fonts, "\u{2764}\u{fe0f}");
        assert!(
            picture,
            "a heart asked to be a picture was drawn in {names:?}"
        );
        as_a_picture(&mut fonts, "\u{2764}\u{fe0f}");

        // Written as text, it is a picture where none of this machine's
        // faces has a heart of its own -- and wherever it is drawn as one it
        // is counted as two cells, and wherever it is not, as one. Which of
        // the two depends on the machine; that the drawing and the counting
        // agree does not.
        let (picture, _, names) = drawn_in(&mut fonts, "\u{2764}");
        assert_eq!(
            picture,
            obelus_text::text_width("\u{2764}") == 2,
            "a heart drawn in {names:?} is counted {} cells wide",
            obelus_text::text_width("\u{2764}")
        );
        if picture {
            as_a_picture(&mut fonts, "\u{2764}");
        } else {
            eprintln!("this machine's own face has a heart, and it is drawn in that");
        }
        // Every picture face has a `#`, for the keycap; the reader's has
        // one too, and theirs is the one drawn.
        let (picture, _, names) = drawn_in(&mut fonts, "#");
        assert!(!picture, "a # was drawn as a picture, in {names:?}");
        // And writing no picture face has is the chain's, as it was.
        let (picture, _, names) = drawn_in(&mut fonts, "\u{8bfb}");
        assert!(!picture, "a Chinese character was drawn in {names:?}");
    }

    /// A mark is drawn on the baseline its own face asked for; writing is
    /// drawn on the one the grid is counted in.
    ///
    /// Which is the difference between a picture and a line of text. The
    /// marks are carried in a face of Obelus's own and are designed a whole
    /// cell wide, so almost every one is laid out again smaller to fit --
    /// and both the face and the size move the baseline it wants. Drawn on
    /// the reader's instead, every mark in the window hung low: a quarter
    /// of a letter's height at the size this measures, which is where
    /// `ctrl`'s keycap sat visibly below where a terminal puts it.
    ///
    /// The numbers are not asserted -- they are the faces' own and change
    /// with whatever this machine has -- but the *relationship* is: a
    /// mark's baseline is its own and is above the grid's, and writing has
    /// none of its own to be put on.
    ///
    /// Deliberate break: clearing `baseline` for marks as well as for
    /// writing puts them all back on the grid's, and the assertion below
    /// that a mark has one of its own fails.
    #[test]
    fn a_mark_is_drawn_on_the_baseline_its_own_face_asked_for() {
        let mut fonts = Fonts::new(16.0);
        let cell = fonts.cell();
        // `md-apple_keyboard_control`, which is what `ctrl` is drawn as.
        let mark: Vec<Placed> = fonts.glyphs("\u{f0634}", false, false, Size::Cell).to_vec();
        let letter: Vec<Placed> = fonts.glyphs("k", false, false, Size::Cell).to_vec();
        assert!(
            !mark.is_empty() && !letter.is_empty(),
            "nothing was laid out"
        );
        assert!(
            letter.iter().all(|glyph| glyph.baseline.is_none()),
            "a letter was given a baseline of its own, which takes it out of its line"
        );
        for glyph in &mark {
            let own = glyph.baseline.expect("a mark is drawn on its own baseline");
            assert!(
                own < cell.baseline,
                "the mark's face asks for {own} and the grid is counted at {}, so drawing it \
                 on the grid's would not have moved it",
                cell.baseline
            );
        }
    }

    /// A character in none of the reader's faces, the machine's monospaced
    /// one or a picture face is drawn in what this machine falls back to,
    /// before cosmic-text's own guess at it.
    ///
    /// The fallback is made to differ from that guess: what the guess drew
    /// `中` in is found first, and the fallback is set to some other face
    /// this machine has that draws it, so the face it came out in says
    /// which of the two answered. On a machine with one face for it there
    /// is nothing to tell apart, and it says so.
    ///
    /// Deliberate break: leaving out the turn to `after` in `shape` draws
    /// it in the guess's face.
    #[test]
    fn what_none_of_the_faces_has_is_asked_of_what_the_machine_falls_back_to() {
        const HAN: &str = "\u{4e2d}";
        let mut fonts = Fonts::new(16.0);
        fonts.cascade.clear();
        fonts.use_families(&[]);
        fonts.after.clear();
        let family_of = |fonts: &mut Fonts| {
            let first = *fonts
                .glyphs(HAN, false, false, Size::Cell)
                .first()
                .expect("it was laid out");
            fonts
                .system
                .db()
                .face(first.key.font_id)
                .and_then(|face| face.families.first().map(|(name, _)| name.clone()))
                .expect("the face it was laid out in")
        };
        let guessed = family_of(&mut fonts);
        let faces: Vec<(fontdb::ID, String)> = fonts
            .system
            .db()
            .faces()
            .filter_map(|face| Some((face.id, face.families.first()?.0.clone())))
            .filter(|(_, name)| !name.eq_ignore_ascii_case(&guessed) && !name.starts_with('.'))
            .collect();
        let other = faces.into_iter().find_map(|(id, name)| {
            let font = fonts.system.get_font(id, Weight::NORMAL)?;
            (font.as_swash().charmap().map('\u{4e2d}') != 0).then_some(name)
        });
        let Some(other) = other else {
            eprintln!("one face for {HAN} on this machine; nothing to tell apart");
            return;
        };
        fonts.cascade = vec![other.clone()];
        fonts.shaped.clear();
        fonts.use_families(&["Nobody's Face".to_string()]);
        assert_eq!(family_of(&mut fonts), other, "the guess said {guessed}");
    }

    /// A character in a face only CoreText can draw is drawn, by CoreText:
    /// `中` in `PingFang SC`, which is the face macOS draws Chinese in.
    ///
    /// On a machine with no such face there is nothing to draw and it says
    /// so.
    ///
    /// Deliberate break: `picture` asking swash whatever the face draws the
    /// character as nothing, which is what a window did with every Chinese
    /// character once `PingFang SC` was found.
    #[test]
    fn a_face_only_coretext_can_draw_is_drawn_by_coretext() {
        let mut fonts = Fonts::new(32.0);
        if fonts.by_coretext.is_empty() {
            eprintln!("no face only CoreText draws on this machine; nothing to check");
            return;
        }
        let key = fonts.glyphs("\u{4e2d}", false, false, Size::Cell)[0].key;
        assert!(
            fonts.by_coretext.contains_key(&key.font_id),
            "{:?} drew it",
            fonts
                .system
                .db()
                .face(key.font_id)
                .map(|face| face.families.clone())
        );
        let picture = fonts.picture(key).expect("it has pixels");
        assert!(picture.data.iter().any(|coverage| *coverage > 0));
    }

    /// And the right way up, and on the line: `上` has its long stroke at
    /// the foot, so the bottom of its picture is the inkiest, and its top is
    /// above the baseline by most of the size it was drawn at.
    ///
    /// Deliberate breaks: reading the bitmap's rows from the bottom puts the
    /// long stroke in the first rows; and counting `top` from the bitmap's
    /// bottom edge rather than its top hangs the character below the line.
    #[test]
    fn what_coretext_draws_is_the_right_way_up_and_on_the_line() {
        let mut fonts = Fonts::new(32.0);
        let key = fonts.glyphs("\u{4e0a}", false, false, Size::Cell)[0].key;
        if !fonts.by_coretext.contains_key(&key.font_id) {
            eprintln!("this machine does not draw it with CoreText; nothing to check");
            return;
        }
        let picture = fonts.picture(key).expect("it has pixels").clone();
        let width = picture.placement.width as usize;
        let rows = picture.placement.height as usize;
        let ink = |row: usize| -> u32 {
            picture.data[row * width..(row + 1) * width]
                .iter()
                .map(|coverage| u32::from(*coverage))
                .sum()
        };
        let upper = (0..rows / 4).map(ink).max();
        let lower = (rows * 3 / 4..rows).map(ink).max();
        assert!(
            lower > upper,
            "the long stroke is at the top: {upper:?} above, {lower:?} below"
        );
        let top = picture.placement.top;
        assert!(
            (20..=34).contains(&top),
            "drawn at 32, it stands {top} above the line"
        );
    }

    /// Drawn in the face CoreText itself would draw after the monospaced
    /// one, at the weight the text is: lighter than the one face of it the
    /// font database knows, which for `PingFang SC` is Medium, and heavier
    /// again in bold.
    ///
    /// Measured in ink, which is what the reader saw: Chinese beside Menlo
    /// that read as bold.
    ///
    /// Deliberate breaks: drawing every weight in the face's own name puts
    /// as much ink in the plain `中` as in the Medium one; and drawing bold
    /// in the plain name puts no more in the bold.
    #[test]
    fn a_face_coretext_draws_is_drawn_at_the_weight_the_text_is() {
        let mut fonts = Fonts::new(32.0);
        let key = fonts.glyphs("\u{4e2d}", false, false, Size::Cell)[0].key;
        let Some(names) = fonts.by_coretext.get(&key.font_id).cloned() else {
            eprintln!("this machine does not draw it with CoreText; nothing to check");
            return;
        };
        let own = fonts
            .system
            .db()
            .face(key.font_id)
            .expect("the face it was shaped in")
            .post_script_name
            .clone();
        let ink = |name: &str, weight: Weight| -> u32 {
            let key = CacheKey {
                font_weight: weight,
                ..key
            };
            crate::coretext::draw(name, key)
                .expect("it has pixels")
                .data
                .iter()
                .map(|coverage| u32::from(*coverage))
                .sum()
        };
        let (plain, bold) = (
            ink(names.at(Weight::NORMAL), Weight::NORMAL),
            ink(names.at(Weight::BOLD), Weight::BOLD),
        );
        let medium = ink(&own, Weight::NORMAL);
        assert!(plain < medium, "{names:?}: plain {plain}, {own} {medium}");
        assert!(bold > plain, "{names:?}: plain {plain}, bold {bold}");
    }

    /// Every face CoreText has is one the reader can choose, which is the
    /// whole of why it is asked: `PingFang SC` was the one missing.
    ///
    /// Asked of the families CoreText names, less its own, whose names
    /// start with a dot and are not names anybody can ask for.
    ///
    /// Including the ones only CoreText can draw, which `PingFang SC` is --
    /// see [`crate::coretext`].
    ///
    /// Deliberate break: `settle` adding nothing leaves the faces fontdb's
    /// walk does not visit out of the list.
    #[test]
    #[cfg(target_os = "macos")]
    fn every_face_coretext_has_can_be_chosen() {
        use objc2_core_foundation::{CFArray, CFRetained, CFString};

        let fonts = Fonts::new(16.0);
        let here: std::collections::HashSet<String> = fonts
            .here()
            .iter()
            .map(|name| name.to_lowercase())
            .collect();
        // SAFETY: takes nothing, and hands back an array the caller owns.
        let names = unsafe { objc2_core_text::CTFontManagerCopyAvailableFontFamilyNames() };
        // SAFETY: an array of strings is what it is documented to return.
        let names = unsafe { CFRetained::cast_unchecked::<CFArray<CFString>>(names) };
        let missing: Vec<String> = names
            .iter()
            .map(|name| name.to_string())
            .filter(|name| !name.starts_with('.') && !here.contains(&name.to_lowercase()))
            .collect();
        assert!(
            missing.is_empty(),
            "CoreText has these and Obelus does not: {missing:?}"
        );
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

    /// What is already tried is not fallen back to again, however it is
    /// spelled, and the rest keep the machine's order.
    ///
    /// Deliberate break: returning the cascade whole tries `Menlo` a second
    /// time for every character it has not got.
    #[test]
    fn what_is_tried_already_is_not_fallen_back_to() {
        let cascade = ["Menlo", "PingFang SC", "Apple Color Emoji"].map(String::from);
        assert_eq!(
            rest(&cascade, &["menlo".to_string()]),
            ["PingFang SC", "Apple Color Emoji"]
        );
        assert_eq!(rest(&cascade, &[]), cascade);
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
