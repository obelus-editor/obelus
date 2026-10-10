//! The window's pixels: one texture of glyphs, and a quad for everything.
//!
//! Everything on the screen is a rectangle with a colour and, sometimes, a
//! picture -- a cell's background, a letter, the caret. So there is one
//! pipeline, one buffer of instances, and one texture that every glyph this
//! session has drawn lives in. The order they are written in is the order
//! they are drawn in, which is what puts a letter over its background
//! without a second pass to arrange it.
//!
//! What is *not* here is any idea of what the cells mean. This file is
//! handed a page and a set of faces and turns them into rectangles.
//!
//! The colours go through untouched, which is why the surface is viewed
//! without its sRGB conversion -- a theme's `#1e1e2e` is the colour the
//! reader picked, and a pipeline that corrects it draws a different one.
//!
//! Every pane and every box is glass over everything said before it. Each
//! is a level with a picture of its own -- the screen as it was before it
//! was put there -- so a list opened over the settings shows the settings
//! through it, and the settings still show the page through theirs. There
//! were two places for that, one pane and one box, and the second pane
//! said took the first one's: the settings went solid the moment a
//! setting's choices opened over them (`Level`, `put_over`).

mod atlas;
mod glass;
mod ground;
mod lay;
mod setup;
mod shapes;
mod text;

use std::{collections::HashMap, ops::Range, sync::Arc};

use bytemuck::{Pod, Zeroable};
use cosmic_text::CacheKey;
use obelus_font::CellSize;
use obelus_ui::{image::Palette, shapes::Joined};
use ratatui::{layout::Rect, style::Color};
use winit::window::Window;

use self::setup::{Seen, Whole};
use crate::grid::Behind;

/// How much of the glass is the pane's own colour, before the shader
/// adds to it where what is behind would leave the text nothing to stand
/// against.
///
/// Thin enough that what is under it reads as shapes, thick enough that
/// what is written on it is what is being read.
const TINT: f32 = 0.74;

/// The same, for a box whose glass is over another pane's.
///
/// Thinner, because what it is over has been tinted once already: at the
/// first pane's measure the two together let through a few hundredths of
/// what is behind, and a box of glass that shows nothing is a grey box.
const ON_GLASS_TINT: f32 = 0.55;

/// How far a pane travels on its way in, as a part of its own height.
///
/// A fraction rather than the whole of it. A slab sliding the length of
/// itself is every row of a list arriving from somewhere else, which for
/// the fifth of a second it takes reads as the list scrolling rather than
/// as the pane opening -- and a reader who was about to press a key has to
/// wait to see what they are pressing it on.
const TRAVEL: f32 = 0.18;

/// How far a cap is held off the rows either side of it, as a part of a
/// cell's height.
const INSET: f32 = 0.08;
/// And off the cells either side, as a part of a cell's width.
///
/// The same idea and the same reason: a cap that reached the edge of the
/// blank it was given would touch whatever is in the next cell, and one
/// place -- the welcome screen, where a picture sits against the key --
/// has only that one blank to share. A fraction of a cell rather than a
/// whole one, because the blank *is* the cap's; what this holds off is
/// what is on the other side of it.
const SIDE: f32 = 0.15;
/// How thick the lip under it is, by the same measure.
///
/// What says the key is raised. Thicker than the outline on the other
/// three sides, because a key is lit from above and a real one's bottom
/// edge is the part of it you can see.
const LIP: f32 = 0.12;
/// How round its corners are, as a part of its height.
const ROUNDING: f32 = 0.22;

/// How thick a line between two things is, as a part of a cell's height.
///
/// About what a font's own light rule is, and never less than a pixel: a
/// line that rounds away is two things with nothing between them.
const LINE: f32 = 0.06;

/// How round a frame's corners are, as a part of a cell's width.
///
/// Half of it, which is the curve `╭` draws: the frame is the same shape
/// in a window as in a terminal, drawn rather than spelled.
const FRAME_CORNER: f32 = 0.5;

/// How wide the bar caret is, as a part of a cell.
///
/// Thin enough to stand between two characters rather than on one, which is
/// the whole of what it says.
const BAR: f32 = 0.15;

/// And how thick the line under what is being spelled is.
const UNDERLINE: f32 = 0.06;

/// How big one layer of a glyph texture is, in pixels each way.
///
/// A screenful of code is a few hundred distinct glyphs, and a layer holds
/// about five hundred Chinese characters at the size a doubled screen draws
/// 24-point text in. A window of Chinese can be more than that -- every
/// character is a glyph of its own -- so a texture that is full takes
/// another layer rather than being emptied.
/// Emptying it was what this did once, half way through a frame: the
/// glyphs already placed in that frame were read from where the next ones
/// had just been written, and a line number came out as part of a
/// character until something drew the frame again.
const ATLAS: u32 = 1024;

/// How many layers a glyph texture starts with.
///
/// Two, because one is not an array everywhere: wgpu's GL backend makes a
/// texture of one layer a plain two-dimensional one, and a shader reading
/// that as an array reads nothing from it -- no error, and no text. Seen on
/// Mesa's software GL, where the same texture read on Vulkan was right. The
/// same backend has a second count it does this at -- see `Room::open`.
const LAYERS: u32 = 2;

/// What Obelus draws on.
pub(crate) struct Painter {
    /// Told each time a frame is about to go, which is what lets winit pace
    /// the next one by the compositor rather than by the swapchain.
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    configured: wgpu::SurfaceConfiguration,
    /// The format the surface is *viewed* as, which is the format it was
    /// given with any sRGB conversion taken off. Colours in a theme are
    /// written the way a terminal means them, and a surface that converts
    /// them on the way out draws a different colour from the one the reader
    /// chose.
    view: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    atlas: Atlas,
    /// What each mark is drawn from, until it has been drawn.
    ///
    /// The text rather than the pixels: how many pixels a mark is depends
    /// on how big a cell is, which the reader changes.
    drawings: HashMap<(String, bool), String>,
    /// The colours those drawings are inked in. A theme change is a new
    /// palette, and every mark already drawn is the old theme's.
    palette: Option<Palette>,
    /// What the page is drawn on, which is what the margin round the grid
    /// is painted in -- see `grid::margin`.
    ground: Color,
    /// How wide and how tall the grid is, in pixels: the cells and nothing
    /// else, which is what a glyph is cut back to -- see `clipped`.
    grid: [f32; 2],
    /// How far in from the window's own edges that grid starts -- see
    /// `grid::margin`. Kept because `whole_window` is wanted below
    /// `draw`, where the margin is worked out.
    margin: [f32; 2],
    /// How many pixels at the top of the window belong to the title bar
    /// -- see `title::height`.
    titled: f32,
    /// Which two colours mean the reader has hold of something: a run of
    /// characters, and the row their keys are on -- see
    /// `Drawing::holding`.
    holding: (Color, Color),
    /// The instances of the frame being built, kept so that a screenful of
    /// rectangles is allocated once rather than once a frame.
    quads: Vec<Quad>,
    /// Where things are in it, which the passes that draw it read.
    placed: Placed,
    instances: wgpu::Buffer,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    smooth: wgpu::Sampler,
    /// What each pane's and each box's glass reads, furthest first: each
    /// is a picture of its own, because what one sees through itself is
    /// the glass of everything under it. Made as they are first wanted and
    /// kept: a window that has once had three things open holds three
    /// pictures from then on, though it draws into only as many as are open
    /// -- and the first is always there, because its bindings are the ones
    /// the whole frame is drawn with, nothing else reading a picture.
    levels: Vec<Seen>,
    /// Where a blur's first way is put, for the second to read, and the
    /// bindings that read it. Half the window each way, as the blurred
    /// picture is -- see `halved`.
    scratch: wgpu::TextureView,
    scratch_bindings: wgpu::BindGroup,
    /// And the whole frame as one, which is drawn only while a pane is on
    /// its way in, with the bindings the two quads that put it back on the
    /// screen read it through.
    ///
    /// There only while something is over the page -- see
    /// `picture_while_wanted` -- because a window with nothing open is the
    /// one most often on the screen, and a picture the size of it is the
    /// memory of a whole window kept for a moment that is not coming.
    picture: Option<Whole>,
    /// The screen as it was when a pane went from it, drawn once at that
    /// moment: the page is what Obelus said, and a pane that has gone is
    /// on no page, so this is the one place it is still to be found -- and
    /// what it is drawn leaving out of (`keep`). Made there, and let go by
    /// the first frame with nothing leaving.
    left: Option<Whole>,
    /// Where each pane that went stood in it -- its glass, which is where
    /// it was cut from the screen under it -- and the edge it goes along.
    gone: Vec<([f32; 4], Joined)>,
    /// The same bindings with something else in the backdrop's place, for
    /// the pass that *draws* it: a texture cannot be read and written in
    /// one pass, and what goes in the slot is never sampled there.
    plain_bindings: wgpu::BindGroup,
    /// What goes in a picture's slot where none is read: a pixel, because
    /// a binding cannot be left empty and the atlas is the wrong shape for
    /// the slot.
    nothing: wgpu::TextureView,
}

/// Where things are in the frame being built, which the passes that draw
/// it read.
#[derive(Clone, Debug, Default)]
struct Placed {
    /// Each pane and box, furthest first -- see `Level`.
    levels: Vec<Level>,
    /// And which quad lays the nearest box's picture back over it while the
    /// box is still coming up, where one is, with which level that is: it
    /// reads the same picture the glass does, so it is drawn with the same
    /// bindings.
    covered: Option<(usize, usize)>,
    /// How many are what the screen draws.
    drawn: usize,
    /// And with the pieces a sliding pane is put back in, which is where
    /// the blurs start.
    moved: usize,
    /// Whether the screen is put back together out of a picture of the
    /// frame: a pane arriving, or a band catching up.
    composed: bool,
    /// The panes that went, put back out of the screen they left -- see
    /// `Painter::keep`. Among the pieces where the screen is composed, and
    /// over the frame where it is not.
    going: Option<Range<usize>>,
    /// Where a band under the first pane is catching up: the rows of it
    /// above the pane, which the slide brings in under it, and the quads
    /// that put what is behind the pane back together with the band slid.
    /// See `Painter::sliding_under`.
    under: Option<(Range<usize>, Range<usize>)>,
    /// The glass under each list catching up, where its rows have not
    /// arrived yet, with which level's glass it is. On the screen and not
    /// in the frame, so drawn there among the pieces with that level's
    /// pictures -- see `Painter::catching_up`.
    gaps: Vec<(usize, usize)>,
}

/// Where one pane or box is in the frame being built.
///
/// What is under it, then what is round its glass, then the glass, then
/// what is round it again: the first level's picture is its own `under`,
/// and every level over it is the glass of every level under it with its
/// own `under` on top -- which is the screen as it was before this one was
/// put over it, and so the picture this one's glass reads.
#[derive(Clone, Debug)]
struct Level {
    /// What it was put over. The first level's starts at the first quad,
    /// which is the window's own ground, and is drawn on the screen as
    /// well; every other level's is drawn into its picture and nowhere
    /// else.
    under: Range<usize>,
    /// Which quad is the glass, which is the one drawn with this level's
    /// pictures. Between `under` and it is a box's frame.
    glass: usize,
    /// How many quads straight after it are the same glass again, under a
    /// list catching up -- see `Painter::glass_kept_still`. Drawn with the
    /// level's pictures, and never into a picture another level reads.
    lowered: usize,
    /// Where what is about it ends: a rule's half row under a pane's line.
    end: usize,
    /// Where the glass is, left, top, right and bottom, which is what its
    /// blur covers and what a pane casts its shadow from.
    rect: [f32; 4],
    /// Whether it is a pane rather than a box.
    pane: bool,
    /// The cells it was said over, which is how a pane that goes is found
    /// again among them.
    area: Rect,
    /// The first quad of its blur's pair.
    blur: usize,
}

impl Placed {
    /// How many quads are what is behind the first pane, which is drawn
    /// twice: into its picture, and on the screen, where it is what shows
    /// round the pane's edge.
    fn behind(&self) -> usize {
        self.levels.first().map_or(0, |level| level.under.end)
    }
}

/// The stack less every pane a later pane is over the whole of.
///
/// What such a pane was is in the picture of the one over it already --
/// its cells were on the page when that one was said -- so it is no glass
/// of its own. Left in, it was a level nobody could see that still acted
/// like one: the list of an agent's commands, drawn under the settings
/// opened over the conversation, cast its shadow across the settings, put
/// them on the tint of glass over glass, and made them a level whose
/// picture leaves out the bars and caps.
fn uncovered(stack: Vec<&Behind>) -> Vec<&Behind> {
    let covered = |at: usize| {
        !stack[at].is_a_box()
            && stack[at + 1..].iter().any(|later| {
                !later.is_a_box() && later.area.intersection(stack[at].area) == stack[at].area
            })
    };
    (0..stack.len())
        .filter(|&at| !covered(at))
        .map(|at| stack[at])
        .collect()
}

/// Whether a pane casts its shadow: only where nothing said after it is
/// over any of it. A shadow is drawn over everything, so one under a later
/// pane fell across that pane rather than behind it.
fn casts(stack: &[&Behind], at: usize) -> bool {
    !stack[at + 1..]
        .iter()
        .any(|later| later.area.intersects(stack[at].area))
}

/// Which pictures a run of quads is drawn with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reads {
    /// None at all: a quad that is its colour, or a letter.
    Nothing,
    /// One level's, which its glass reads -- and, for the box coming up,
    /// what is laid back over it.
    Level(usize),
    /// The screen a pane went from.
    Left,
}

/// The quads the screen draws, each with whatever it reads.
///
/// Everything but the glass reads nothing, so it is drawn with whatever the
/// first glass reads. Each glass reads its own level's pictures, and what
/// is under each level over the first is that level's picture and not on
/// the screen at all.
fn on_the_screen(placed: &Placed) -> Vec<(Range<usize>, Reads)> {
    let mut plan = Vec::new();
    let mut at = 0;
    for (level, placed) in placed.levels.iter().enumerate() {
        if level > 0 {
            plan.push((at..placed.under.start, Reads::Nothing));
            at = placed.under.end;
        }
        plan.push((at..placed.glass, Reads::Nothing));
        let glass = placed.glass + 1 + placed.lowered;
        plan.push((placed.glass..glass, Reads::Level(level)));
        at = glass;
    }
    match placed.covered {
        // What is under the box, over the box -- the same picture the
        // glass reads, so the same bindings.
        Some((covered, level)) => {
            plan.push((at..covered, Reads::Nothing));
            plan.push((covered..covered + 1, Reads::Level(level)));
            plan.push((covered + 1..placed.drawn, Reads::Nothing));
        }
        None => plan.push((at..placed.drawn, Reads::Nothing)),
    }
    plan.retain(|(quads, _)| !quads.is_empty());
    plan
}

/// What one level was put over, as the screen had it: the picture its
/// glass reads, and what shows under a pane while it is on its way in.
///
/// The first level's own `under`, then the glass of every level before
/// this one, then this one's `under`. Never this level's own pictures,
/// which are what this may be drawing into -- the one thing a pass may not
/// read.
fn put_over(levels: &[Level], level: usize) -> Vec<(Range<usize>, Reads)> {
    let mut plan = vec![(levels[0].under.clone(), Reads::Nothing)];
    if level > 0 {
        for (at, placed) in levels[..level].iter().enumerate() {
            plan.push((placed.under.end..placed.glass, Reads::Nothing));
            plan.push((placed.glass..placed.glass + 1, Reads::Level(at)));
            plan.push((
                placed.glass + 1 + placed.lowered..placed.end,
                Reads::Nothing,
            ));
        }
        plan.push((levels[level].under.clone(), Reads::Nothing));
    }
    plan.retain(|(quads, _)| !quads.is_empty());
    plan
}

/// The pieces the screen is put back together from while something on it
/// is moving, each with whatever it reads: the frame, and the glass where
/// a list's rows have not arrived -- see `Placed::gaps`.
fn catching(placed: &Placed) -> Vec<(Range<usize>, Reads)> {
    let mut reading: Vec<(Range<usize>, Reads)> = placed
        .gaps
        .iter()
        .map(|&(glass, level)| (glass..glass + 1, Reads::Level(level)))
        .chain(placed.going.clone().map(|going| (going, Reads::Left)))
        .collect();
    reading.sort_by_key(|(quads, _)| quads.start);
    let mut plan = Vec::new();
    let mut at = placed.drawn;
    for (quads, reads) in reading {
        plan.push((at..quads.start, Reads::Nothing));
        at = quads.end;
        plan.push((quads, reads));
    }
    plan.push((at..placed.moved, Reads::Nothing));
    plan.retain(|(quads, _)| !quads.is_empty());
    plan
}

/// One rectangle, as the shader reads it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Quad {
    /// Left, top, width, height, in real pixels.
    rect: [f32; 4],
    /// Left, top, right, bottom in the atlas, from zero to one -- and
    /// for glass, in pixels, the rectangle it is drawn inside. For a
    /// light round a hold, where the runs on the rows above and below it
    /// begin and end, in pixels along the grid.
    uv: [f32; 4],
    colour: [f32; 4],
    flags: u32,
    /// How far its corners are rounded, in pixels. Read only where
    /// `ROUNDED` is set -- and for a light round a hold, which has no
    /// corners of its own, where the run on its own row begins.
    radius: f32,
    /// Which layer of the atlas `uv` is on: of the letters', or of the
    /// pictures' where the quad is `COLOURFUL`.
    layer: u32,
    /// How much further down than where it stands a glass reads what is
    /// behind it, in pixels -- see `Painter::glass_kept_still`. And for a
    /// light round a hold, where the run on its own row ends.
    lower: f32,
}

/// A rectangle with no picture: its colour is the whole of it.
const SOLID: u32 = 1;
/// A picture with colours of its own, which is an emoji.
const COLOURFUL: u32 = 2;
/// A solid whose corners are taken off, which is a key's cap.
const ROUNDED: u32 = 4;
/// What is behind a pane, seen through it.
const GLASS: u32 = 8;
/// Joined to the row above it, so there is no edge along the top.
const HANGING: u32 = 16;
/// Or to the row below it, so there is none along the bottom.
const STANDING: u32 = 128;
/// The mark in a switch that is set.
const CHECKED: u32 = 256;
/// Glass over glass, which takes none of the extra colour a pane adds
/// where what is behind is close to its own -- see `paint.wgsl`.
const ON_GLASS: u32 = 512;
/// What is behind a pane, blurred one way -- see `Painter::blurring`.
const BLUR: u32 = 1024;
/// A triangle filling its quad, its point in the middle of one short
/// side: the arrow on the seam a deletion left -- see `paint.wgsl`. Which
/// side it points to is the sign of the quad's `radius`, that field being
/// the one a wedge has nothing else to say with.
///
/// Past the bits the four corners' turns take, which is why it is so far
/// along: a plate says how each of its corners bends in two bits apiece
/// from `HELD_TURNS`, and a flag inside that run would be read as a turn.
const WEDGE: u32 = 2_097_152;

/// A stretch of the light round a hold -- see `lights`, and the shader.
/// Past the turns, which it carries for its own row's run, and past which
/// of the three rows it has a run to measure to.
const LIGHT: u32 = 536_870_912;

/// Where a light says which rows it has a run to measure to, three bits
/// from here: above, below, and its own.
const HELD_NEAR: u32 = 25;

/// A letter the light on the welcome screen's mark runs across.
///
/// Set on the glyphs rather than on a rectangle over them, because what is
/// lit is the ink: a band drawn over the plate would light the page showing
/// between the letters as well, which is a lamp behind the mark and not a
/// sheen on it.
const SHEENED: u32 = 2048;

/// The mark that turns, as an arc whose tail fades into nothing -- see
/// `paint.wgsl`. How far round its head is rides in the quad's `radius`,
/// which a ring has no corners to need. Past the turns, for the wedge's
/// reason.
const TURNING: u32 = 16_777_216;

/// A run the reader has hold of, whose four corners each turn one of
/// three ways -- see `held` in the shader, and `Turn`.
const HELD_PLATE: u32 = 4096;

/// The soft edge outside a pane or a box. Past the turns for the same
/// reason the wedge is: at 8192 it was the low bit of a plate's top left
/// turn, and the shader asks about a shadow first, so a hold whose row
/// above reached further left was drawn as the shadow of nothing.
const SHADOW: u32 = 8_388_608;

/// Where those four turns sit in the flags, two bits each, in the order
/// `Turn::corners` puts them.
const HELD_TURNS: u32 = 13;

/// How far from a row's own ground toward its own ink a seam between two
/// entries is drawn.
///
/// A seventh, which is a faint line in every theme and a rule in none. Out
/// of the row's own two colours rather than out of a name, because a name
/// promises nothing: this was `raised_background` first -- what a theme
/// calls the ground behind a key's cap -- and a theme whose caps sit five
/// levels off its page had a seam nobody could see. What ink and ground
/// are is settled by having to read words in the one against the other, so
/// a part of the way between them is a part of something every theme
/// keeps.
/// How far a held run's face is carried from what is under it toward the
/// colour the cells wear.
///
/// Not quite the whole way. What a hold has to do is be seen and not
/// shout, and the way to do both is to put the strength in the *edge*:
/// an edge is what an eye finds a shape by, and a face a shade off the
/// square a terminal draws reads quieter while being better bounded.
///
/// A shade, and no more than a shade. A third of the way was tried and
/// is the same rule read off the wrong measurement: the hold it was
/// settled on is a *grey row* on a light page -- `#d1d0d0` on `#faf9f9`
/// -- where a third of the way is still a step the eye finds, and the
/// thing it has to serve as well is a *selection on a dark page*, where
/// the whole distance is small. The dark theme's selection is `#312e81`
/// on `#18181b`: a third of that is `#201f3e`, which against the page is
/// a contrast of 1.13 and is nothing at all -- a reader who selected four
/// lines could not see which four. At a sixth off it is the colour the
/// theme chose, which is what `ob` paints those cells and so what the
/// theme was written against, and the rim still carries the shape.
const HELD: f32 = 0.85;

/// How much further than the colour itself its rim goes, away from what
/// is under it.
///
/// The rim is the thing being seen, so it goes a little past the colour
/// the cells wear rather than stopping at it. A theme is entitled to a
/// hold barely off its page -- one here is `#d1d0d0` on `#faf9f9` -- and
/// a rim that stopped at the colour would be a step off its own face.
const HELD_RIM: f32 = 0.25;

/// How far the light round a hold reaches past it, as a share of a row's
/// height.
///
/// A share of the row, the way the rim and the corner are, so a reader who
/// makes the text bigger gets the same light bigger. Less than half a row,
/// so the rows above and below are read through the edge of it rather
/// than under it -- and less than a row is what lets a row's light measure
/// to the rows beside it and no further (`lights`).
const HELD_REACH: f32 = 0.35;

/// How much of the hold's colour the light carries where it starts.
///
/// The colour the cells wear rather than anything lighter: what a light
/// round the hold adds is that the shape has no hard outside, and a colour
/// of its own would be a second thing the theme did not choose.
const HELD_LIGHT: f32 = 0.55;

/// How wide that rim is, as a share of a row's height.
///
/// A hairline. A rule's own thickness is `LINE` and this is half of it
/// again: what is wanted is the thinnest line the screen can draw, which
/// on a screen drawn at twice its own pixels is one of those.
const HELD_EDGE: f32 = 0.03;

/// And how round its corners are, as a share of a row's height.
///
/// Small. Half a row is as round as a row can be and is a pill, which on
/// a run of words reads as a badge -- a thing to press -- rather than as
/// the ground under what the reader is holding.
const HELD_CORNER: f32 = 0.18;

/// How far a shadow reaches past the thing casting it, as a share of a
/// row's height.
///
/// Wide and faint rather than tight and dark: a tight one reads as an
/// outline drawn in grey, which the page already has a rule for. What a
/// shadow is for is the one thing a terminal cannot say at all -- that
/// the thing is over the page rather than part of it.
const SHADOW_SPREAD: f32 = 0.8;

/// And how dark it is where it leaves the edge.
///
/// A tenth, which on a light page is a shade the eye reads without
/// looking at and on a dark one is very little. That asymmetry is not
/// worth correcting: a shadow on something already dark *is* less of a
/// shadow, and a theme's page is the reader's choice.
const SHADOW_INK: f32 = 0.12;

const SEAM: f32 = 0.14;

/// How much of a cell a switch's box takes, across.
///
/// Nearly all of it: what it stands in for is a glyph, and a glyph fills
/// its cell. The little left over is what keeps it off whatever is beside
/// it.
const BOX: f32 = 0.95;
/// How round its corners are, as a part of its side.
const BOX_CORNER: f32 = 0.30;
/// And how thick its outline is, by the same measure.
const BOX_EDGE: f32 = 0.11;
/// How far above the cell's middle it sits, as a part of the cell.
const ABOVE: f32 = 0.05;

/// How much of a cell the ring the turning mark goes round takes, across.
///
/// Less than a switch's box: the braille it stands in for is a few dots in
/// the middle of its cell, and a ring as wide as the cell reads as a
/// button rather than as a mark beside a word.
const RING: f32 = 0.9;

/// How wide a bar's track is drawn, as a part of the cell it sits in.
///
/// A third, which is what leaves the column reading as a margin with
/// something in it rather than as a wall. The cell stays the cell: what
/// the view reserved is a column, and narrowing the ink inside it is the
/// front end saying what that column looks like, not the application
/// giving back a column it told the text it had taken.
const BAR_TRACK: f32 = 0.32;

/// And how wide its mark is.
///
/// Wider than the track, because the mark is the part that is doing the
/// telling and the track is only there to say how far it can go. Two
/// capsules about one centre line, which is the same figure a terminal
/// draws in one column of blocks and two colours.
const BAR_MARK: f32 = 0.5;

/// How wide the mark is once it has settled.
///
/// Thinner, and still there. The column is reserved whether or not there
/// is anywhere to scroll, and an empty one is Obelus saying that what is
/// on screen is all there is -- so a mark that went out altogether would
/// be the window saying that on a file with more of it below.
const BAR_MARK_RESTING: f32 = 0.3;

/// And how wide it is with the pointer on it.
///
/// Thicker than either, because a pointer on a bar is a reader reaching
/// for it: what they are about to do is take hold of the mark, and what
/// they are aiming at should be the size of the thing they get.
const BAR_MARK_UNDER: f32 = 0.78;

/// How much of the way from the page to its own colour a settled mark is
/// drawn.
///
/// Mixed toward the background rather than drawn with an alpha: the page
/// under it is opaque and known, so this is the colour it would be, and
/// nothing here depends on how the pipeline happens to blend.
const BAR_RESTING: f32 = 0.42;
/// The whole window, in the coordinates a quad's rectangle is written in.
///
/// Which are the grid's, not the window's: the vertex shader adds
/// `screen.origin` -- the margin the grid is middled in -- to every
/// rectangle it is handed, so a quad that wants the *window* has to start
/// that far back. Two of them do, the ground under everything and the
/// frame put back while a pane slides, and the second was written
/// `[0.0, 0.0, width, height]`: it went on the screen a margin down and
/// to the right of where it meant to, leaving the strip along the top and
/// down the left at the black the pass clears to, for as long as the
/// slide lasted.
///
/// Which is visible only where the window is not a whole number of cells
/// -- the compositor picks one size and the font the other, so that is
/// most windows and no test.
fn whole_window(window: [f32; 2], margin: [f32; 2]) -> [f32; 4] {
    [-margin[0], -margin[1], window[0], window[1]]
}

/// Whether the window is a Wayland surface.
fn on_wayland(window: &Window) -> bool {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    window
        .window_handle()
        .is_ok_and(|handle| matches!(handle.as_raw(), RawWindowHandle::Wayland(_)))
}

/// How wide a stroke is drawn, as a part of the cell it sits in.
///
/// The same as a bar's track, because the map and the bar are next to each
/// other and what they say is one picture: a stroke a different weight
/// from the bar beside it would read as two columns that happened to line
/// up. A terminal has half a cell for both, which is the only stroke a
/// cell can draw.
const STROKE: f32 = 0.32;

/// How far the arrow on a seam reaches across its cell.
///
/// Its point is on the edge the stroke is against -- beside the text,
/// where a bar would be -- and it reaches back from there, so the whole of
/// it is inside the column the view reserved.
const SEAM_REACH: f32 = 0.46;

/// And how wide its base is, as a part of that reach.
///
/// Of the *reach*, and not of the cell's height, which is what this was
/// first: a cell is about twice as tall as it is wide, so a base measured
/// down the cell came out two and a half times the length and the
/// arrowhead was a spike. What decides the shape of an arrow is the shape
/// of an arrow, and the column it is in is the only thing here with a
/// size of its own.
///
/// Half again, which is an arrowhead; and short enough that the two rows
/// it sits between are still two rows, because what it points at is the
/// boundary and a mark as tall as a row would be a mark on the row.
const SEAM_BASE: f32 = 1.5;

/// The frame that has just been drawn, put back everywhere but the pane.
const FRAME: u32 = 32;
/// And the pane out of it, higher up than it will end.
const SLID: u32 = 64;

/// What the shader needs to know about the window.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Screen {
    size: [f32; 2],
    /// Where the grid starts in it: half of what the cells do not reach --
    /// see `grid::margin`. Added to every quad in the vertex stage, which
    /// is the one place a pixel becomes a place on the screen.
    origin: [f32; 2],
    /// Where the light on the welcome screen's mark is, in real pixels:
    /// its middle, how far its falloff reaches either side, how much of
    /// the glow it carries, and nothing.
    ///
    /// In the uniform rather than on the quads because there is one light
    /// on the screen and a thousand letters under it: said per quad it
    /// would be the same numbers written a thousand times a frame, and a
    /// quad has no room left for them anyway.
    sheen: [f32; 4],
    /// And the colour it carries the mark to.
    glow: [f32; 4],
    /// How far the light round a hold reaches past it and how round the
    /// hold's corners are, in pixels, and nothing.
    ///
    /// In the uniform for the sheen's reason: both are a share of a row,
    /// the same for every hold on the screen, and a light's quad has its
    /// room taken by the three runs it measures to.
    held: [f32; 4],
}

// A field added without its padding is otherwise found by wgpu, at the
// first frame, as a uniform shorter than the shader's.
const _: () = assert!(
    std::mem::size_of::<Screen>().is_multiple_of(16),
    "the uniform is read in sixteens"
);

/// Where every glyph drawn this session is kept.
struct Atlas {
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// Coverage, a byte a pixel: every letter, and the white pixel.
    letters: Layers,
    /// Colour, four bytes a pixel: an emoji, and an agent's mark. Apart
    /// from the letters because a letter is a quarter of the size in a
    /// texture of its own, and there are thousands of letters.
    pictures: Layers,
    /// Whether either texture has been made again since the bindings that
    /// read it were.
    remade: bool,
    /// Whether a glyph found no room with every layer the device allows:
    /// it is missing from this frame, and the next starts both textures
    /// again. Not this one, which has already placed glyphs that would be
    /// read from where the next ones were written.
    overflowed: bool,
    /// Where each glyph landed, or that the face had no picture for it --
    /// which is worth remembering too, or a missing glyph is rasterised
    /// again on every frame that asks for it.
    spots: HashMap<CacheKey, Option<Spot>>,
    /// And where each agent's mark landed, by the same rule.
    ///
    /// A second map rather than a second texture: a mark is a picture of
    /// about sixteen pixels square, which is a large glyph and nothing
    /// more. What makes it a map of its own is that it is thrown away for
    /// a different reason -- a theme, rather than a size.
    marks: HashMap<(String, bool), Option<Spot>>,
    /// One opaque pixel, so that a rectangle with no picture can go through
    /// the same pipeline as one with. On the letters' first layer, which is
    /// the layer a quad that says none reads.
    white: [f32; 4],
}

/// One texture of layers, and where there is room in it.
struct Layers {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    format: wgpu::TextureFormat,
    room: Room,
}

/// Where there is room in a stack of layers.
///
/// Nothing in it is given back until the whole is: a glyph is asked for
/// again on the next frame, and a place handed out has to hold what was
/// put there for as long as anything may still read it -- which, once it
/// is in a frame's quads, is to the end of that frame. Apart from the
/// texture so that what is handed out can be checked without a device.
struct Room {
    layers: Vec<etagere::AtlasAllocator>,
}

/// Where one glyph is, and how it sits against its cell.
#[derive(Clone, Copy, Debug)]
struct Spot {
    uv: [f32; 4],
    /// Which layer of its texture `uv` is on.
    layer: u32,
    width: f32,
    height: f32,
    /// How far right of the pen the picture starts.
    left: f32,
    /// And how far above the baseline its top is.
    top: f32,
    colourful: bool,
}

impl Painter {
    /// One block of colour, in pixels.
    fn block(&mut self, left: f32, top: f32, width: f32, height: f32, colour: [f32; 4]) {
        self.quads.push(Quad {
            rect: [left, top, width, height],
            uv: self.atlas.white,
            colour,
            flags: SOLID,
            radius: 0.0,
            layer: 0,
            lower: 0.0,
        });
    }

    /// One block of colour with its corners taken off.
    fn rounded(
        &mut self,
        left: f32,
        top: f32,
        width: f32,
        height: f32,
        radius: f32,
        colour: [f32; 4],
    ) {
        self.quads.push(Quad {
            rect: [left, top, width, height],
            uv: self.atlas.white,
            colour,
            flags: SOLID | ROUNDED,
            radius: radius.max(0.0).min(width.min(height) / 2.0),
            layer: 0,
            lower: 0.0,
        });
    }
}

fn seen_through(panes: &[&Behind], x: u16, y: u16, colour: Color) -> bool {
    panes.iter().any(|pane| {
        let area = pane.area;
        pane.ground == colour
            && (area.left()..area.right()).contains(&x)
            && (area.top()..area.bottom()).contains(&y)
    })
}

/// Where a frame's line is, in pixels: left, top, right, bottom of its
/// outside edge, down the middle of the ring of cells it is drawn in.
fn outline(area: ratatui::layout::Rect, cell: CellSize, line: f32) -> [f32; 4] {
    [
        along(area.x, cell.width, line),
        middle(f32::from(area.y) * cell.height, cell.height, line),
        along(area.right() - 1, cell.width, line) + line,
        middle(
            f32::from(area.bottom() - 1) * cell.height,
            cell.height,
            line,
        ) + line,
    ]
}

/// How thick a line between two things is, in pixels.
fn thickness(cell_height: f32) -> f32 {
    (cell_height * LINE).round().max(1.0)
}

/// Where a line runs across a row, so that it is in the middle of it: the
/// pixel its top edge is on.
///
/// Whole pixels, because a line between two of them is two lines at half
/// the colour.
fn middle(top: f32, cell_height: f32, line: f32) -> f32 {
    (top + (cell_height - line) / 2.0).round()
}

/// And where one runs down a column, the same way.
fn along(column: u16, cell_width: f32, line: f32) -> f32 {
    (f32::from(column) * cell_width + (cell_width - line) / 2.0).round()
}

/// Which half of a cell a colour is for, which is the whole of what `Reset`
/// means.
#[derive(Clone, Copy, Debug)]
enum Ink {
    Foreground,
    Background,
}

/// One colour as far past another as that one is from it.
///
/// What a rim wants: the colour the cells wear, carried on in the
/// direction it already went from the ground, so a hold barely off its
/// page still has an edge to be found by.
/// The face a hold wearing this colour is drawn in, over this ground.
///
/// The cell's question rather than the run's: whoever is painting a cell
/// back knows where it is and only wants to know what colour to put
/// there. Three things put a cell's own ground back and every one of
/// them had to be told -- a switch replacing its glyph, the ring of
/// cells a box's frame is drawn on, and the picture taken of what a pane
/// or a box was put over. The cells carry the hold's colour at full
/// strength, because that is the whole of what a terminal has for it, so
/// painting one back is a square of it standing where a plate is drawn:
/// a switch in a dark box of its own, a bar of grey across the top of a
/// card, a band through the glass.
fn as_held(colour: Color, holding: (Color, Color), ground: [f32; 4]) -> Option<[f32; 4]> {
    let (held, chosen) = holding;
    (colour == held || colour == chosen).then(|| mixed(ground, rgba(colour, Ink::Background), HELD))
}

/// A region of the grid in pixels, which is what everything moving is
/// measured in.
fn box_of(room: Rect, cell: CellSize) -> [f32; 4] {
    [
        f32::from(room.x) * cell.width,
        f32::from(room.y) * cell.height,
        f32::from(room.right()) * cell.width,
        f32::from(room.bottom()) * cell.height,
    ]
}

fn mixed(from: [f32; 4], to: [f32; 4], along: f32) -> [f32; 4] {
    let along = along.clamp(0.0, 1.0);
    let mut out = to;
    for channel in 0..3 {
        out[channel] = from[channel] + (to[channel] - from[channel]) * along;
    }
    out
}

fn rgba(colour: Color, ink: Ink) -> [f32; 4] {
    let (r, g, b) = channels(colour, ink);
    [
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
        1.0,
    ]
}

/// What the page is drawn on, as the three bytes it is: the colour the
/// ground under everything is painted in, for the platform's title bar to
/// be painted in too -- see `title.rs`.
pub(crate) fn ground_of(colour: Color) -> (u8, u8, u8) {
    channels(colour, Ink::Background)
}

fn channels(colour: Color, ink: Ink) -> (u8, u8, u8) {
    match colour {
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Indexed(index) => indexed(index),
        // What the reader's own terminal would have chosen, which here is
        // nobody: a window has no colours of its own, so it has to have an
        // answer. These are the two ends of the default palette below.
        Color::Reset => match ink {
            Ink::Foreground => (0xcc, 0xcc, 0xcc),
            Ink::Background => (0x0c, 0x0c, 0x0c),
        },
        Color::Black => indexed(0),
        Color::Red => indexed(1),
        Color::Green => indexed(2),
        Color::Yellow => indexed(3),
        Color::Blue => indexed(4),
        Color::Magenta => indexed(5),
        Color::Cyan => indexed(6),
        Color::Gray => indexed(7),
        Color::DarkGray => indexed(8),
        Color::LightRed => indexed(9),
        Color::LightGreen => indexed(10),
        Color::LightYellow => indexed(11),
        Color::LightBlue => indexed(12),
        Color::LightMagenta => indexed(13),
        Color::LightCyan => indexed(14),
        Color::White => indexed(15),
    }
}

/// The palette a terminal would have had.
///
/// Obelus's themes name their colours in full, so this is reached only by a
/// theme that asked for one of the sixteen -- and by anything drawn before
/// a theme is loaded. The sixteen are xterm's, and the rest is the cube and
/// the grey ramp every terminal since has agreed on.
fn indexed(index: u8) -> (u8, u8, u8) {
    const BASE: [(u8, u8, u8); 16] = [
        (0x00, 0x00, 0x00),
        (0xcd, 0x00, 0x00),
        (0x00, 0xcd, 0x00),
        (0xcd, 0xcd, 0x00),
        (0x00, 0x00, 0xee),
        (0xcd, 0x00, 0xcd),
        (0x00, 0xcd, 0xcd),
        (0xe5, 0xe5, 0xe5),
        (0x7f, 0x7f, 0x7f),
        (0xff, 0x00, 0x00),
        (0x00, 0xff, 0x00),
        (0xff, 0xff, 0x00),
        (0x5c, 0x5c, 0xff),
        (0xff, 0x00, 0xff),
        (0x00, 0xff, 0xff),
        (0xff, 0xff, 0xff),
    ];
    const STEPS: [u8; 6] = [0x00, 0x5f, 0x87, 0xaf, 0xd7, 0xff];
    match index {
        0..=15 => BASE[index as usize],
        16..=231 => {
            let index = index - 16;
            (
                STEPS[(index / 36) as usize],
                STEPS[(index % 36 / 6) as usize],
                STEPS[(index % 6) as usize],
            )
        }
        232..=255 => {
            let grey = 8 + (index - 232) * 10;
            (grey, grey, grey)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::glass::mark_behind;

    mod stacked {
        use obelus_font::CellSize;
        use ratatui::{layout::Rect, style::Color};

        use super::super::{
            GLASS, HANGING, Level, Placed, Quad, Reads, SOLID, box_of, casts, catching,
            glass::kept_still, on_the_screen, put_over, uncovered,
        };
        use crate::grid::{Behind, Page, Rolled};

        fn pane(area: Rect, joined: obelus_ui::shapes::Joined) -> Behind {
            Behind {
                area,
                joined,
                ground: Color::Reset,
                cells: Vec::new(),
            }
        }

        /// A pane a later pane is over the whole of is no level of its
        /// own, and one it is over part of still is.
        ///
        /// The list of an agent's commands is drawn under every layer, so
        /// the settings opened over a conversation with one up were said
        /// over it -- and it stayed a level: its shadow fell across the
        /// settings and put them on the tint of glass over glass.
        /// Deliberate break: keep every pane in `uncovered`.
        #[test]
        fn a_pane_covered_whole_is_no_level() {
            use obelus_ui::shapes::Joined;
            let commands = pane(Rect::new(0, 9, 76, 11), Joined::Below);
            let settings = pane(Rect::new(0, 0, 76, 24), Joined::Screen);
            let choices = pane(Rect::new(0, 18, 76, 6), Joined::Below);
            let kept = uncovered(vec![&commands, &settings, &choices]);
            let areas: Vec<Rect> = kept.iter().map(|over| over.area).collect();
            assert_eq!(areas, vec![settings.area, choices.area]);
        }

        /// Only a pane nothing is over casts a shadow: it is drawn over
        /// everything, so one under a later pane fell across that pane.
        /// Deliberate break: let every pane cast.
        #[test]
        fn a_pane_something_is_over_casts_nothing() {
            use obelus_ui::shapes::Joined;
            let list = pane(Rect::new(0, 10, 76, 14), Joined::Below);
            let card = pane(Rect::new(20, 12, 30, 6), Joined::Nowhere);
            let stack = [&list, &card];
            assert!(!casts(&stack, 0), "the list cast across the card over it");
            assert!(casts(&stack, 1), "the card on top cast nothing");
        }

        /// Three things open at once -- the settings, a setting's choices
        /// over them, and the card of every key over those -- each with
        /// what it was put over, a frame or a rule round it, and its glass.
        fn three() -> Placed {
            let level = |under: std::ops::Range<usize>, glass, end| Level {
                under,
                glass,
                lowered: 0,
                end,
                rect: [0.0; 4],
                pane: true,
                area: Rect::default(),
                blur: 0,
            };
            Placed {
                levels: vec![
                    level(0..5, 5, 6),
                    level(6..10, 12, 13),
                    level(13..16, 16, 17),
                ],
                drawn: 30,
                ..Placed::default()
            }
        }

        /// Each glass reads its own level's pictures, and what is under a
        /// level over the first is that level's picture, never the screen.
        ///
        /// The pictures were two, and the window kept one pane: a list
        /// opened over the settings took the settings' glass away, and the
        /// page under them stopped showing through. Deliberate break:
        /// draw a glass with the first level's pictures, or leave out the
        /// skip over what is under a level -- the first reads the wrong
        /// picture, and the second lays the settings' cells over their own
        /// glass on the screen.
        #[test]
        fn every_glass_reads_its_own_picture() {
            let placed = three();
            let plan = on_the_screen(&placed);
            for (at, level) in placed.levels.iter().enumerate() {
                assert!(
                    plan.contains(&(level.glass..level.glass + 1, Reads::Level(at))),
                    "level {at}'s glass reads the wrong picture: {plan:?}"
                );
            }
            let drawn: Vec<usize> = plan.iter().flat_map(|(quads, _)| quads.clone()).collect();
            let wanted: Vec<usize> = (0..30)
                .filter(|quad| !(6..10).contains(quad) && !(13..16).contains(quad))
                .collect();
            assert_eq!(drawn, wanted, "the screen drew the wrong quads: {plan:?}");
        }

        /// The glass under a list catching up is drawn again over the list's
        /// rows, the whole pane's shape cut to those rows, reading what is
        /// behind from as far down as the rows are taken from above -- which
        /// `catching_up` does by `behind` rows. Nothing is drawn again for a
        /// band the pane was put over, or for one on no glass at all.
        ///
        /// The glass was in the picture the rows are taken from, so what
        /// was behind the list slid with it and jumped back when it
        /// arrived. Deliberate breaks: `lower: 0.0` and the backdrop slides
        /// with the rows again; the glass's own `uv` and the copy is drawn
        /// over the whole pane, so its tabs and the box typed into read
        /// from below as well; leave out `band.under` in `on_glass` and the
        /// transcript under the list gets glass of its own; leave out the
        /// bar's copy, or read it as far down as the rows, and the column
        /// beside the list slides as the whole list used to.
        #[test]
        fn a_lists_glass_stands_still_while_its_rows_catch_up() {
            use obelus_ui::shapes::Joined;
            let cell = CellSize {
                width: 8.0,
                height: 16.0,
                baseline: 12.0,
            };
            let list = pane(Rect::new(0, 14, 80, 10), Joined::Below);
            let before = Page::default();
            let band = |room, under, behind| Rolled {
                room,
                under,
                before: &before,
                behind,
                since: behind,
                bar: None,
            };
            let rows = Rect::new(0, 16, 79, 6);
            let bar = Rect::new(79, 16, 1, 6);
            let bands = [
                band(rows, false, 2.5),
                // What the pane was put over, catching up as well, and
                // inside the pane the way a transcript is inside the
                // settings.
                band(Rect::new(0, 14, 80, 6), true, 1.0),
                // A list on the page, beside the pane.
                band(Rect::new(0, 0, 80, 4), false, 1.0),
            ];
            let glass = Quad {
                rect: [0.0, 232.0, 640.0, 152.0],
                uv: [0.0, 232.0, 640.0, 384.0],
                colour: [0.1, 0.1, 0.1, 0.74],
                flags: SOLID | GLASS | HANGING,
                radius: 0.0,
                layer: 0,
                lower: 0.0,
            };
            let copies = kept_still(&glass, 0, &[&list], &bands, cell);
            assert_eq!(copies.len(), 1, "{copies:?}");
            // And the bar beside the rows, whose mark is taken from above
            // by its own share of the way: two rows still to come, all of
            // them, since the band is as far behind as it set out.
            let barred = [Rolled {
                bar: Some((bar, 2.0)),
                ..bands[0]
            }];
            let both = kept_still(&glass, 0, &[&list], &barred, cell);
            assert_eq!(both.len(), 2, "{both:?}");
            assert_eq!(both[1].uv, box_of(bar, cell), "cut to the wrong column");
            assert!(
                (both[1].lower - 2.0 * cell.height).abs() < 0.001,
                "the bar's glass reads from {} pixels lower",
                both[1].lower
            );
            let copy = copies[0];
            assert_eq!(copy.uv, box_of(rows, cell), "cut to the wrong rows");
            assert!(
                (copy.lower - 2.5 * cell.height).abs() < 0.001,
                "reads from {} pixels lower",
                copy.lower
            );
            assert_eq!(
                (copy.rect, copy.flags, copy.colour),
                (glass.rect, glass.flags, glass.colour),
                "not the same glass"
            );
            // And a level the list is not on has nothing drawn again: a
            // box over the top of the page, which neither list is inside.
            let hover = pane(Rect::new(0, 0, 40, 6), Joined::Nowhere);
            let copies = kept_still(&glass, 1, &[&list, &hover], &bands, cell);
            assert!(copies.is_empty(), "{copies:?}");
        }

        /// The glass drawn again under a list reads its level's pictures on
        /// the screen, and is in no picture another level reads; and where
        /// the list's rows have not arrived, the glass there reads its level's
        /// pictures among the pieces the screen is put back from.
        ///
        /// Deliberate breaks: `placed.glass + 1` for the end of the glass in
        /// `on_the_screen`, and the copies read the frame's picture as if it
        /// were what is behind; the same in `put_over`, and they are drawn
        /// into the picture of the box over the list; skip the gaps in
        /// `catching`, and the glass in a gap reads the frame.
        ///
        /// What this does not see is where the quads are put, because that
        /// is a `Painter`'s, and a `Painter` wants a device: the copies
        /// spliced in anywhere but straight after their glass, the gap's
        /// glass pushed anywhere but before the old page's rows, or those
        /// rows painted in the pane's colour after all. Each of those is
        /// a list on glass, scrolled in a window, to look at.
        #[test]
        fn the_glass_drawn_again_reads_its_own_level() {
            let level = |under: std::ops::Range<usize>, glass, lowered, end| Level {
                under,
                glass,
                lowered,
                end,
                rect: [0.0; 4],
                pane: true,
                area: Rect::default(),
                blur: 0,
            };
            let placed = Placed {
                levels: vec![level(0..5, 5, 2, 8), level(8..10, 11, 0, 12)],
                drawn: 20,
                moved: 30,
                gaps: vec![(22, 0), (25, 1)],
                ..Placed::default()
            };
            let plan = on_the_screen(&placed);
            assert!(
                plan.contains(&(5..8, Reads::Level(0))),
                "the copies read the wrong picture: {plan:?}"
            );
            let plan = put_over(&placed.levels, 1);
            assert!(
                plan.iter()
                    .all(|(quads, _)| !quads.contains(&6) && !quads.contains(&7)),
                "the copies are in the next level's picture: {plan:?}"
            );
            assert_eq!(
                catching(&placed),
                vec![
                    (20..22, Reads::Nothing),
                    (22..23, Reads::Level(0)),
                    (23..25, Reads::Nothing),
                    (25..26, Reads::Level(1)),
                    (26..30, Reads::Nothing),
                ]
            );
        }

        /// A pane that went is put back out of the screen it went from,
        /// among the pieces of the one it left -- and between them, where a
        /// pane arriving in its place is drawn after it.
        ///
        /// Deliberate break: leave `going` out of `catching`. The pane that
        /// went is then drawn out of the frame's own picture, which is the
        /// files that took its place, so the palette going down is a strip
        /// of the files going down.
        #[test]
        fn a_pane_that_went_is_drawn_out_of_the_screen_it_left() {
            let placed = Placed {
                drawn: 20,
                moved: 30,
                composed: true,
                going: Some(23..25),
                ..Placed::default()
            };
            assert_eq!(
                catching(&placed),
                vec![
                    (20..23, Reads::Nothing),
                    (23..25, Reads::Left),
                    (25..30, Reads::Nothing),
                ]
            );
        }

        /// What a level was put over is the glass of every level under it
        /// and what it was put over -- and never its own picture, which is
        /// the one being drawn into.
        ///
        /// Deliberate break: walk the levels up to and including this one.
        /// The picture then reads itself, which the device refuses.
        #[test]
        fn a_level_s_picture_is_everything_under_it() {
            let placed = three();
            for level in 0..placed.levels.len() {
                let plan = put_over(&placed.levels, level);
                assert!(
                    plan.iter().all(|(_, reads)| match reads {
                        Reads::Nothing => true,
                        Reads::Level(read) => *read < level,
                        Reads::Left => false,
                    }),
                    "level {level} reads its own picture or one over it: {plan:?}"
                );
                for under in 0..level {
                    let glass = placed.levels[under].glass;
                    assert!(
                        plan.contains(&(glass..glass + 1, Reads::Level(under))),
                        "level {level} does not see level {under}'s glass: {plan:?}"
                    );
                }
                assert_eq!(
                    plan.last().map(|(quads, _)| quads.clone()),
                    Some(placed.levels[level].under.clone()),
                    "what level {level} was put over is not on top of its picture: {plan:?}"
                );
            }
        }
    }

    /// A bar's mark stays between the two rows it was drawn on.
    ///
    /// Deliberate break: drop the clamp. A band that is somehow further
    /// behind than it set out -- which a second press during a slide can
    /// arrange -- then sends the mark past the row it started from, and a
    /// mark that goes the wrong way before it goes the right way is the
    /// one thing anybody notices about a bar.
    #[test]
    fn a_mark_stays_between_the_two_rows_it_was_drawn_on() {
        // Three rows to come.
        assert!((mark_behind(3.0, 3.0, 3.0) - 3.0).abs() < 0.001, "sets out");
        assert!(mark_behind(3.0, 0.0, 3.0).abs() < 0.001, "and lands");
        let part = mark_behind(3.0, 1.0, 3.0);
        assert!(part > 0.0 && part < 3.0, "{part}");
        assert!(
            (mark_behind(3.0, 9.0, 3.0) - 3.0).abs() < 0.001,
            "no further"
        );
        assert!(
            mark_behind(3.0, -1.0, 3.0).abs() < 0.001,
            "nor the other way"
        );
        // Nothing has moved, so the mark has nowhere to be but where it is.
        assert!(mark_behind(3.0, 1.0, 0.0).abs() < f32::EPSILON);
    }

    use obelus_ui::shapes::{About, Side};
    use ratatui::{buffer::Cell, style::Modifier};

    use super::{
        glass::{behind_row, beneath, casting, lettered_behind, reached, sliding, tiles},
        ground::{held_runs, runs, without},
        shapes::underline_ink,
        text::{clipped, drawn_as_a_shape, pieces, snapped},
        *,
    };
    use crate::grid::{Barred, Capped, Page, Said, Spun, Stroked, Update};

    /// A block covers the share of its cell its name says, and only the
    /// blocks are drawn this way.
    ///
    /// Deliberate break: give `\u{2583}` the three eighths from the top
    /// rather than the bottom, or `\u{259b}` the lower right quadrant
    /// where it wants the lower left. The first is caught by where it sits,
    /// the second by what it covers.
    #[test]
    fn a_block_covers_what_its_name_says() {
        let area = |glyph: char| -> f32 {
            pieces(&glyph.to_string())
                .expect("a block")
                .iter()
                .map(|[left, top, right, bottom]| (right - left) * (bottom - top))
                .sum()
        };
        // The lower eighths grow up from the foot, the left ones shrink
        // toward the left edge.
        for (step, glyph) in ('\u{2581}'..='\u{2587}').enumerate() {
            let [_, top, _, bottom] = pieces(&glyph.to_string()).expect("a block")[0];
            assert!(
                (bottom - 1.0).abs() < f32::EPSILON,
                "{glyph} is not on the foot"
            );
            #[expect(clippy::cast_precision_loss, reason = "seven of them")]
            let share = (step + 1) as f32 / 8.0;
            assert!(
                (1.0 - top - share).abs() < f32::EPSILON,
                "{glyph} is the wrong height"
            );
        }
        for (step, glyph) in ('\u{2589}'..='\u{258f}').enumerate() {
            #[expect(clippy::cast_precision_loss, reason = "seven of them")]
            let share = (7 - step) as f32 / 8.0;
            assert!(
                (area(glyph) - share).abs() < f32::EPSILON,
                "{glyph} is the wrong width"
            );
        }
        for (glyph, share) in [
            ('\u{2596}', 0.25),
            ('\u{2597}', 0.25),
            ('\u{2598}', 0.25),
            ('\u{259d}', 0.25),
            ('\u{259a}', 0.5),
            ('\u{259e}', 0.5),
            ('\u{2599}', 0.75),
            ('\u{259b}', 0.75),
            ('\u{259c}', 0.75),
            ('\u{259f}', 0.75),
        ] {
            assert!((area(glyph) - share).abs() < f32::EPSILON, "{glyph}");
        }
        // Which three quarters, since the area cannot say: the one with no
        // lower right is the one with the lower left.
        assert!(
            pieces("\u{259b}")
                .expect("a block")
                .contains(&[0.0, 0.5, 0.5, 1.0]),
            "\u{259b} has no lower left"
        );
        // Everything else is the face's: a shade, a letter, and two blocks
        // in one cell, which is not a thing a cell holds.
        assert!(pieces("\u{2592}").is_none());
        assert!(pieces("a").is_none());
        assert!(pieces("\u{2588}\u{2588}").is_none());
    }

    /// A column of full blocks is one unbroken bar, whatever the cell's
    /// height, which is the whole of why they are drawn at all.
    ///
    /// Deliberate break: round the height on its own -- `(cell.height *
    /// (to_y - from_y)).round()` -- rather than both edges where they fall.
    /// A cell 16.8 pixels tall is then drawn 17 tall from a top that was
    /// rounded somewhere else, and within a few rows one block overlaps
    /// the next or stops short of it: the stripe again, a row in five.
    #[test]
    fn a_column_of_blocks_has_no_seams() {
        let cell = obelus_font::CellSize {
            width: 8.4,
            height: 16.8,
            baseline: 13.0,
        };
        let full = pieces("\u{2588}").expect("a block")[0];
        for row in 0..40_u16 {
            let place = |row: u16| (0.0, f32::from(row) * cell.height);
            let [_, top, _, tall] = snapped(place(row), cell, full);
            let [_, next, _, _] = snapped(place(row + 1), cell, full);
            assert!(
                (top + tall - next).abs() < f32::EPSILON,
                "row {row} ends at {} and the next starts at {next}",
                top + tall
            );
        }
        // And across, for the same reason: a cell is not a whole number of
        // pixels wide either.
        for column in 0..40_u16 {
            let place = |column: u16| (f32::from(column) * cell.width, 0.0);
            let [left, _, wide, _] = snapped(place(column), cell, full);
            let [next, _, _, _] = snapped(place(column + 1), cell, full);
            assert!((left + wide - next).abs() < f32::EPSILON, "column {column}");
        }
    }

    /// A light, and which rows it has a run to measure to, are read as
    /// nothing the shader asks about before it gets to them.
    ///
    /// Deliberate break: put `HELD_NEAR` at 24, and a light with a run
    /// above it is drawn as the mark that turns.
    #[test]
    fn a_light_s_flags_are_no_other_flag() {
        let light = LIGHT | 7 << HELD_NEAR;
        for other in [
            TURNING,
            WEDGE,
            CHECKED,
            FRAME,
            SLID,
            BLUR,
            GLASS,
            SHADOW,
            HELD_PLATE,
            255 << HELD_TURNS,
        ] {
            assert_eq!(light & other, 0, "{other:032b}");
        }
        assert_eq!(LIGHT & 7 << HELD_NEAR, 0);
    }

    /// A quad that wants the whole window covers the whole window.
    ///
    /// Checked against what the vertex shader does with a rectangle it is
    /// handed, which is to add the grid's origin to it -- so this is the
    /// shader's half of the bargain written out, not the function's own
    /// answer read back.
    ///
    /// Deliberate break: answer `[0.0, 0.0, window[0], window[1]]`, which
    /// is the window in the window's own coordinates. That is what the
    /// frame put back during a slide was written in, and on a window five
    /// pixels wider than its cells it left a five-pixel black strip down
    /// the left for as long as the scroll took. A margin of nothing is in
    /// the list below because that break passes it: the bug is invisible
    /// on exactly the windows a test would think to try.
    #[test]
    fn a_quad_that_wants_the_whole_window_covers_it() {
        let window = [1882.0, 1012.0];
        for margin in [[0.0, 0.0], [4.0, 0.0], [0.0, 3.0], [4.0, 3.0]] {
            let [left, top, wide, tall] = whole_window(window, margin);
            // What the shader draws it at.
            let (x, y) = (left + margin[0], top + margin[1]);
            assert!(x.abs() < f32::EPSILON, "{margin:?}: a strip down the left");
            assert!(y.abs() < f32::EPSILON, "{margin:?}: a strip along the top");
            assert!(
                (x + wide - window[0]).abs() < f32::EPSILON,
                "{margin:?}: it stops short of the right"
            );
            assert!(
                (y + tall - window[1]).abs() < f32::EPSILON,
                "{margin:?}: it stops short of the bottom"
            );
        }
    }

    /// The frame is put back everywhere the things that are moving are
    /// not, exactly once.
    ///
    /// The pieces are what a pane's gap and a band's gap show through, so
    /// a pixel they miss is a pixel showing whatever happened to be drawn
    /// there before -- the page, or a row of some other band's old page
    /// that landed outside its own room. And a pixel drawn twice is the
    /// picture copied twice for nothing.
    ///
    /// Deliberate break: leave out either strip beside the hole and the
    /// columns level with a band stop being covered -- which is where a
    /// list's own scrollbar is, and where the code beside a compact list
    /// is. Leave out the piece below and a band standing on the status
    /// row takes the status row with it.
    #[test]
    fn the_frame_is_put_back_everywhere_nothing_is_moving() {
        let whole = [0.0, 0.0, 100.0, 60.0];
        let rooms = [[10.0, 10.0, 40.0, 30.0], [60.0, 0.0, 100.0, 20.0]];
        let pieces = tiles(whole, &rooms);
        let inside = |[left, top, far, low]: [f32; 4], x: f32, y: f32| {
            x >= left && x < far && y >= top && y < low
        };
        // Every half-cell of the window, which catches an edge off by
        // one as well as a piece missing altogether.
        let mut over = 0_usize;
        let mut under = 0_usize;
        for step in 0_u16..(200 * 120) {
            let (x, y) = (f32::from(step % 200) * 0.5, f32::from(step / 200) * 0.5);
            let covered = pieces.iter().filter(|piece| inside(**piece, x, y)).count();
            let moving = rooms.iter().any(|room| inside(*room, x, y));
            match moving {
                true => over += usize::from(covered > 0),
                false => {
                    over += covered.saturating_sub(1);
                    under += usize::from(covered == 0);
                }
            }
        }
        assert_eq!(under, 0, "pixels the frame was not put back on");
        assert_eq!(over, 0, "pixels it was put back on twice, or over a hole");
        // And with nothing moving it is the one rectangle it always was.
        assert_eq!(tiles(whole, &[]), vec![whole]);
    }

    /// A band a pane is over moves only where the pane is not, and the
    /// pane's own band moves whole.
    ///
    /// The first is a transcript scrolling on under a list: slid whole, it
    /// took the foot of the list with it, and every line an agent wrote
    /// behind a list was the list arriving again. The second is the list's
    /// own rows, which are what the reader is scrolling. And a full-screen
    /// dialog, which the transcript is entirely inside: it slid whole, and
    /// the settings shuddered on every line.
    ///
    /// Deliberate breaks: `tiles(room, &[])` in `sliding` and nothing is
    /// kept still under the pane; `pane.as_slice()` without the filter
    /// and the list's own band stops moving at all; the old test, whether
    /// the band is inside the pane, in place of `under`, and the
    /// transcript under the full-screen pane slides whole again.
    #[test]
    fn a_band_under_a_pane_moves_only_where_the_pane_is_not() {
        let pane = [0.0, 60.0, 100.0, 100.0];
        let transcript = [0.0, 0.0, 100.0, 90.0];
        let moving = sliding(transcript, &[pane], true);
        assert_eq!(moving, vec![[0.0, 0.0, 100.0, 60.0]], "{moving:?}");
        let list = [0.0, 70.0, 98.0, 90.0];
        assert_eq!(sliding(list, &[pane], false), vec![list]);
        // And with no pane, a band is all of it.
        assert_eq!(sliding(transcript, &[], true), vec![transcript]);
        // And under two, less both: the list of an agent's commands stood
        // first, and the band slid the list over it along with it. The
        // deliberate break was cutting out the first pane alone.
        let commands = [0.0, 40.0, 100.0, 50.0];
        let moving = sliding(transcript, &[commands, pane], true);
        assert_eq!(
            moving,
            vec![[0.0, 0.0, 100.0, 40.0], [0.0, 50.0, 100.0, 60.0]],
            "{moving:?}"
        );
        // A full-screen dialog: the transcript is inside it, and still
        // nothing of it moves on the screen, while the dialog's own list,
        // inside it as well, moves whole.
        let screen = [0.0, 0.0, 100.0, 100.0];
        let moving = sliding(transcript, &[screen], true);
        assert!(moving.is_empty(), "{moving:?}");
        assert_eq!(sliding(list, &[screen], false), vec![list]);
    }

    /// What slides behind the glass is the part of a band the pane is
    /// over, and only for a band the pane is over: the list's own rows are
    /// the pane, and a band beside it has nothing behind it.
    ///
    /// Deliberate breaks: `None` for every band and what is behind the
    /// glass stands still while the band round it slides; the intersection
    /// for every band and the list's own rows are slid into the backdrop
    /// under themselves.
    #[test]
    fn what_slides_behind_the_glass_is_what_the_pane_is_over() {
        let pane = [0.0, 60.0, 100.0, 100.0];
        assert_eq!(
            beneath([0.0, 0.0, 100.0, 90.0], pane, true),
            Some([0.0, 60.0, 100.0, 90.0]),
            "a transcript the list is over"
        );
        assert_eq!(
            beneath([0.0, 70.0, 98.0, 90.0], pane, false),
            None,
            "the list"
        );
        assert_eq!(
            beneath([0.0, 0.0, 100.0, 50.0], pane, true),
            None,
            "above it"
        );
        // And all of a transcript under a full-screen dialog.
        assert_eq!(
            beneath([0.0, 0.0, 100.0, 90.0], [0.0, 0.0, 100.0, 100.0], true),
            Some([0.0, 0.0, 100.0, 90.0]),
            "a transcript the settings are over"
        );
    }

    /// A pane's shadow falls from the edge the pane has reached.
    ///
    /// The frame is drawn once and put back in two pieces, and a shadow
    /// is the one part of a pane outside the pane's own room -- so it
    /// belongs to the piece that slides, at the edge that has moved. The
    /// joined edge is a seam and does not move: the picture the pane is
    /// taken from stops there.
    ///
    /// Deliberate break: answer `pane` itself. The shadow then lies at
    /// the edge the pane is *going* to have, with the page still showing
    /// under it -- which is a band of dark across the file for a fifth of
    /// a second, and is what a compact list looked like before this.
    #[test]
    fn a_pane_casts_from_the_edge_it_has_reached() {
        let pane = [10.0, 100.0, 200.0, 300.0];
        // Standing on the status row: it comes up from below, so it is
        // drawn from `shift` below its top and down to its own foot.
        let standing = reached(pane, 40.0);
        assert!((standing[1] - 140.0).abs() < f32::EPSILON, "{standing:?}");
        assert!((standing[3] - 300.0).abs() < f32::EPSILON, "the seam");
        // And hanging from the top, the other way about.
        let hanging = reached(pane, -40.0);
        assert!((hanging[1] - 100.0).abs() < f32::EPSILON, "the seam");
        assert!((hanging[3] - 260.0).abs() < f32::EPSILON, "{hanging:?}");
        // Arrived, and it is simply the pane.
        assert_eq!(reached(pane, 0.0), pane);
    }

    /// A full-screen pane casts nothing, and a pane joined along one edge
    /// casts from the other one only.
    ///
    /// Deliberate break: answer `Some(0)` for `Joined::Screen`, which is
    /// what it was. The shader reads `0` as a box joined to nothing, and
    /// the shadow falls all round the grid -- a grey frame in the strip
    /// a window leaves over when it is not a whole number of cells.
    #[test]
    fn a_full_screen_pane_casts_no_shadow() {
        assert_eq!(casting(Joined::Screen), None);
        assert_eq!(casting(Joined::Above), Some(HANGING));
        assert_eq!(casting(Joined::Below), Some(STANDING));
    }

    /// A square of colour is a hole in a pane only where it is the pane's
    /// own colour, inside the pane.
    ///
    /// Deliberate break: drop `pane.ground == colour`. A key's cap in the
    /// card of every key then leaves the cells beside its rounded corners
    /// unpainted wherever the cap's page differs from the glass -- and the
    /// selected row's ground inside a list, asked the same question, would
    /// be taken for glass.
    #[test]
    fn only_the_panes_own_colour_is_seen_through() {
        let ground = Color::Rgb(1, 2, 3);
        let pane = Behind {
            area: ratatui::layout::Rect {
                x: 2,
                y: 1,
                width: 3,
                height: 2,
            },
            joined: Joined::Nowhere,
            ground,
            cells: Vec::new(),
        };
        let panes = [&pane];
        assert!(seen_through(&panes, 2, 1, ground), "a corner of it");
        assert!(seen_through(&panes, 4, 2, ground), "the other corner");
        assert!(
            !seen_through(&panes, 3, 1, Color::Rgb(9, 9, 9)),
            "another colour"
        );
        assert!(!seen_through(&panes, 5, 1, ground), "beside it");
        assert!(!seen_through(&panes, 2, 3, ground), "under it");
        assert!(!seen_through(&[], 2, 1, ground), "no pane at all");
    }

    /// The mark that turns is drawn as an arc and not as braille as well,
    /// and only while its cell still holds a frame of the turn.
    ///
    /// Deliberate break: take the `spun` clause out of `drawn_as_a_shape`,
    /// and the braille is drawn under the arc. Or have `Spun::round` answer
    /// for any cell, and the arc goes on turning over whatever a list put
    /// there.
    #[test]
    fn the_mark_that_turns_is_not_drawn_as_letters_as_well() {
        let page = page("ab\u{2819}c");
        let one = |x: u16| Spun {
            area: ratatui::layout::Rect {
                x,
                y: 0,
                width: 1,
                height: 1,
            },
        };
        let spun = [one(2)];
        let said = Said {
            marked: &[],
            capped: &[],
            ticked: &[],
            spun: &spun,
            barred: &[],
            ruled: &[],
            sheened: None,
            parted: &[],
            stroked: &[],
            stack: &[],
            bands: &[],
        };
        assert!(drawn_as_a_shape(&page, &said, &[], &[], 2, 0), "its cell");
        assert!(!drawn_as_a_shape(&page, &said, &[], &[], 1, 0), "beside it");
        assert!(!drawn_as_a_shape(&page, &said, &[], &[], 3, 0), "nor after");
        assert_eq!(one(2).round(&page), Some(0.1), "the second frame");
        assert_eq!(one(1).round(&page), None, "a letter is not the mark");
    }

    /// A bar's cells are the window's to draw, so the letters leave them
    /// alone -- and only while the cells are still the bar's.
    ///
    /// Both halves matter and each passes with the other broken. A bar
    /// whose cells were not left alone puts the block a terminal draws it
    /// with on the screen under the capsule, and the only way to be rid of
    /// it is to paint over the cell -- which is what this did first, and
    /// on a pane those cells are glass, so every list in Obelus got an
    /// opaque strip down its right-hand side where the reader was meant to
    /// see through. A bar that asked only which *column* it was in claims
    /// the cells of whatever was drawn over it: a panel as wide as the
    /// editor has its own right-hand edge in the scrollbar's column.
    ///
    /// Deliberate break: take the `barred` clause out of
    /// `drawn_as_a_shape` for the first, and put `within(bar.bar.area)`
    /// back for the second.
    #[test]
    fn a_bar_is_not_drawn_as_letters_as_well() {
        // The block a terminal draws a bar with, which is what says the
        // cell is still the bar's.
        let page = page("         \u{2588}\u{2502}");
        let bar = Barred {
            bar: obelus_ui::shapes::Bar {
                area: ratatui::layout::Rect {
                    x: 9,
                    y: 0,
                    width: 1,
                    height: 1,
                },
                mark: 0,
                thumb: 1,
            },
            shown: 1.0,
            under: 0.0,
        };
        let barred = [bar];
        let said = Said {
            marked: &[],
            capped: &[],
            ticked: &[],
            spun: &[],
            barred: &barred,
            ruled: &[],
            sheened: None,
            parted: &[],
            stroked: &[],
            stack: &[],
            bands: &[],
        };
        assert!(
            drawn_as_a_shape(&page, &said, &[], &[], 9, 0),
            "the column the bar is in"
        );
        assert!(
            !drawn_as_a_shape(&page, &said, &[], &[], 8, 0),
            "and not the one beside it, which is the file"
        );

        // The same column, one cell along, holding a panel's own edge: the
        // bar was said about it and the cell is not its any more.
        let wider = Barred {
            bar: obelus_ui::shapes::Bar {
                area: ratatui::layout::Rect {
                    x: 10,
                    y: 0,
                    width: 1,
                    height: 1,
                },
                mark: 0,
                thumb: 1,
            },
            shown: 1.0,
            under: 0.0,
        };
        let covered = [wider];
        let said = Said {
            barred: &covered,
            ..said
        };
        assert!(
            !drawn_as_a_shape(&page, &said, &[], &[], 10, 0),
            "a cell a panel took is drawn as the panel's, not left blank"
        );
        assert!(
            wider.runs(&page).is_empty(),
            "and the capsule is not drawn across it either"
        );
    }

    /// And the picture of what a pane was opened over leaves it alone too.
    ///
    /// Deliberate break: drop the `barred` clause from `lettered_behind`.
    /// The block a terminal draws a track with is then written into the
    /// picture behind the list as well -- where `letters` never put it --
    /// and a full block's raster is taller than its cell, so the first
    /// row's reached above the grid and came out in the margin round it: a
    /// dark line a cell wide along the top of the window, wherever a list
    /// was opened over a file long enough to have a bar.
    #[test]
    fn nor_into_the_picture_behind_a_pane() {
        let page = page("\u{2588}a");
        let barred = [Barred {
            bar: obelus_ui::shapes::Bar {
                area: ratatui::layout::Rect {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
                mark: 0,
                thumb: 1,
            },
            shown: 1.0,
            under: 0.0,
        }];
        assert!(
            !lettered_behind(page.look(0, 0), &barred, 0, 0),
            "the block the bar is drawn with"
        );
        assert!(
            lettered_behind(page.look(1, 0), &barred, 1, 0),
            "and the file's own letter beside it, which nothing else draws"
        );
        assert!(
            !lettered_behind(page.look(2, 0), &barred, 2, 0),
            "nor a cell with nothing in it"
        );
    }

    /// A glyph is cut back to the grid, and its picture with it.
    ///
    /// Deliberate break: hand the rectangle back whole. A full block's
    /// raster is taller than its cell, so the one on the first row reaches
    /// above the grid and lands in the margin -- which is the strip no cell
    /// reaches and is painted in the page's own ground, so what came out
    /// was a dark line along the top of the window belonging to no cell.
    ///
    /// Or cut the rectangle and leave the picture alone: the glyph is then
    /// squeezed into what is left of it rather than cut, which on a block
    /// is invisible and on a letter is a letter of the wrong shape.
    #[test]
    fn a_glyph_is_cut_back_to_the_grid() {
        let grid = [100.0, 50.0];
        let whole = [0.0, 0.0, 1.0, 1.0];

        // Three pixels of ten above the grid.
        let (rect, uv) = clipped([0.0, -3.0, 10.0, 10.0], whole, grid).expect("some of it");
        assert!(rect[1].abs() < f32::EPSILON, "starts at the grid");
        assert!(
            (rect[3] - 7.0).abs() < f32::EPSILON,
            "and is that much less"
        );
        assert!((uv[1] - 0.3).abs() < 0.001, "and so does its picture");
        assert!(
            (uv[3] - 1.0).abs() < f32::EPSILON,
            "which ends where it did"
        );

        // Inside it, untouched.
        let inside = [2.0, 2.0, 10.0, 10.0];
        let (rect, uv) = clipped(inside, whole, grid).expect("all of it");
        assert_eq!(rect, inside, "nothing to cut");
        assert_eq!(uv, whole);

        // And off the far edge, which is the same question the other way.
        let (rect, uv) = clipped([96.0, 0.0, 10.0, 10.0], whole, grid).expect("some of it");
        assert!(
            (rect[2] - 4.0).abs() < f32::EPSILON,
            "cut at the right edge"
        );
        assert!((uv[2] - 0.4).abs() < 0.001, "and its picture with it");

        assert!(
            clipped([-20.0, 0.0, 10.0, 10.0], whole, grid).is_none(),
            "none of it is in the grid"
        );
        assert!(
            clipped([0.0, 60.0, 10.0, 10.0], whole, grid).is_none(),
            "nor under it"
        );
    }

    /// A word a server complained about wears its line, in the colour the
    /// complaint is written in rather than the colour the word is.
    ///
    /// A terminal draws this itself off the modifier, so both halves have
    /// to cross the seam for a window to draw anything at all -- and
    /// neither did: nothing in `obg` ever read `Modifier::UNDERLINED`, so
    /// the one mark Obelus puts under a word was missing from the window
    /// entirely.
    ///
    /// Deliberate break, one assertion each. Dropping `underline_color`
    /// from `Page::look` -- or reading `look.foreground` here -- draws the
    /// mark in whatever colour the syntax gave the word. Taking the
    /// `UNDERLINED` check out puts a line under every cell on the screen.
    /// And a cell nobody coloured has to keep the ink, or what an input
    /// method is spelling loses its own line.
    #[test]
    fn a_cell_a_view_underlined_says_so_and_in_which_colour() {
        let word = Color::Rgb(9, 8, 7);
        let trouble = Color::Rgb(0xef, 0x44, 0x44);
        let mut page = Page::default();
        page.resized(4, 1);
        let marked = |x: u16, underline: Color| {
            let mut cell = Cell::default();
            cell.set_symbol("n");
            cell.fg = word;
            cell.modifier = Modifier::UNDERLINED;
            cell.underline_color = underline;
            Update::Cell {
                x,
                y: 0,
                cell: Box::new(cell),
            }
        };
        page.apply(marked(1, trouble));
        page.apply(marked(2, Color::Reset));

        assert!(
            underline_ink(&page.look(0, 0)).is_none(),
            "a line under a cell nobody marked"
        );
        assert_eq!(
            underline_ink(&page.look(1, 0)),
            Some(rgba(trouble, Ink::Foreground)),
            "not the colour the complaint is written in"
        );
        assert_ne!(
            underline_ink(&page.look(1, 0)),
            Some(rgba(word, Ink::Foreground)),
            "the colour the word is written in"
        );
        // Nobody coloured this one, which is a terminal saying "the ink":
        // it is what the line under a spelling wears.
        assert_eq!(
            underline_ink(&page.look(2, 0)),
            Some(rgba(word, Ink::Foreground)),
            "an uncoloured line is not the ink"
        );
    }

    /// What is left of a run once the holes are cut out of it, whatever
    /// order they came in and however they overlap.
    ///
    /// Deliberate break: drop the `sort_unstable`. The glass hole, pushed
    /// after the frame's ring, then comes first, and a frame on the left
    /// of a pane has its ring painted square after all.
    #[test]
    fn a_run_loses_its_holes_in_any_order() {
        let mut holes = vec![(6, 7), (2, 3), (2, 5)];
        assert_eq!(without(0, 10, &mut holes), vec![(0, 2), (5, 6), (7, 10)]);
        assert_eq!(without(3, 6, &mut [(0, 4)]), vec![(4, 6)]);
        assert!(without(3, 6, &mut [(0, 9)]).is_empty(), "all of it");
        assert_eq!(without(3, 6, &mut []), vec![(3, 6)], "none of it");
    }

    /// A cap standing on a hold is part of its plate, and a cap a shade
    /// off the page in the hold's colour is not.
    ///
    /// Deliberate break: drop `cap.page != colour` from `held_runs`. The
    /// row a conversation offers enter on is then two plates, each with a
    /// rounded end and a rim, and a square of the raw colour between them
    /// where the cap is -- which is what the reader saw.
    #[test]
    fn a_cap_on_a_hold_does_not_cut_it_in_two() {
        let page = page("a  Enter  b");
        let held = Color::Rgb(1, 2, 3);
        let holding = (held, Color::Rgb(4, 5, 6));
        let cap = |page: Color| Capped {
            keys: "Enter".to_string(),
            area: ratatui::layout::Rect {
                x: 2,
                y: 0,
                width: 7,
                height: 1,
            },
            cap: held,
            page,
            edge: Color::Rgb(7, 8, 9),
        };
        let on = cap(held);
        assert_eq!(
            held_runs(&page, 0, holding, &[&on]),
            vec![(0, 11, held)],
            "the row the reader is on, with its key on it"
        );
        let off = cap(Color::Rgb(0, 0, 0));
        assert_eq!(
            held_runs(&page, 0, holding, &[&off]),
            vec![(0, 2, held), (9, 11, held)],
            "a key at the foot, whose cap is the hold's colour by chance"
        );
    }

    /// A page with one column of text down it, for asking about a shape
    /// that covers several rows.
    fn column(x: u16, glyphs: &str) -> Page {
        let mut page = Page::default();
        page.resized(
            12,
            u16::try_from(glyphs.chars().count()).expect("a few rows"),
        );
        for (row, character) in glyphs.chars().enumerate() {
            let mut cell = Cell::default();
            cell.set_symbol(&character.to_string());
            cell.fg = Color::Rgb(9, 8, 7);
            page.apply(Update::Cell {
                x,
                y: u16::try_from(row).expect("a few rows"),
                cell: Box::new(cell),
            });
        }
        page
    }

    /// One change mark over those rows of that column.
    fn stroke(x: u16, y: u16, height: u16, side: Side, about: About) -> Stroked {
        Stroked {
            stroke: obelus_ui::shapes::Stroke {
                area: ratatui::layout::Rect {
                    x,
                    y,
                    width: 1,
                    height,
                },
                side,
                about,
            },
        }
    }

    /// A change mark's cells are the window's to draw, so the letters leave
    /// them alone.
    ///
    /// Deliberate break: take the `stroked` clause out of
    /// `drawn_as_a_shape`. The half block a terminal draws the margin with
    /// is then put on the screen under the stroke -- and the only way to be
    /// rid of it is to paint over the cell, which inside a pane is a hole
    /// in the glass. The bar's own note is about the same bug, one column
    /// along.
    #[test]
    fn a_change_mark_is_not_drawn_as_letters_as_well() {
        let page = column(3, "\u{2590}\u{2590}\u{2594}");
        let stroked = [
            stroke(3, 0, 2, Side::Right, About::Rows),
            stroke(3, 2, 1, Side::Right, About::Seam),
        ];
        let said = Said {
            marked: &[],
            capped: &[],
            ticked: &[],
            spun: &[],
            barred: &[],
            ruled: &[],
            stroked: &stroked,
            sheened: None,
            parted: &[],
            stack: &[],
            bands: &[],
        };
        for row in 0..3 {
            assert!(
                drawn_as_a_shape(&page, &said, &[], &[], 3, row),
                "the margin at row {row}"
            );
        }
        assert!(
            !drawn_as_a_shape(&page, &said, &[], &[], 4, 0),
            "and not the column beside it, which is the file"
        );
    }

    /// A run whose middle was drawn over is two strokes, not one bar across
    /// the hole.
    ///
    /// Which is what a compact list over the foot of the editor does: the
    /// margin is drawn to the bottom of its region and the list goes over
    /// it, so the rows under the list are the list's and keep what it wrote
    /// in them.
    ///
    /// Deliberate break: have `runs` return the whole area as one run and
    /// skip `holds`. A four-row hunk with a list over its middle two rows
    /// is then one stroke four rows tall, drawn straight down the list.
    #[test]
    fn a_change_mark_drawn_over_is_what_is_left_of_it() {
        // The list wrote a blank over the second and third rows of it.
        let page = column(3, "\u{2590}  \u{2590}");
        let mark = stroke(3, 0, 4, Side::Right, About::Rows);
        assert_eq!(mark.runs(&page), vec![(0, 1), (3, 1)]);

        // And where nothing covered it, it is the one run the view said.
        let whole = column(3, "\u{2590}\u{2590}\u{2590}\u{2590}");
        assert_eq!(mark.runs(&whole), vec![(0, 4)]);

        // A stroke is told from the column beside it by the glyph, which is
        // the side it leans: the map's mark in the margin's place is not
        // this stroke, whatever the cells say about anything else.
        let leaning = column(3, "\u{258c}\u{258c}\u{258c}\u{258c}");
        assert!(mark.runs(&leaning).is_empty(), "the other half of the cell");
    }

    /// A page with one row of text on it, for asking what would be drawn.
    /// The cells a pane was opened over, as the painter is handed them.
    ///
    /// The same row `page` builds, reset cells and all, in the shape the
    /// picture behind a pane is painted from.
    fn behind(text: &str) -> Behind {
        let page = page(text);
        let area = ratatui::layout::Rect {
            x: 0,
            y: 0,
            width: page.columns(),
            height: 1,
        };
        let cells = (0..page.columns())
            .map(|x| {
                let look = page.look(x, 0);
                let mut cell = Cell::default();
                cell.set_symbol(look.text);
                cell.bg = look.background;
                cell.fg = look.foreground;
                cell
            })
            .collect();
        Behind {
            area,
            joined: obelus_ui::shapes::Joined::Below,
            ground: Color::Reset,
            cells,
        }
    }

    fn page(text: &str) -> Page {
        let mut page = Page::default();
        page.resized(12, 1);
        let mut column = 0;
        for character in text.chars() {
            let written = character.to_string();
            let mut cell = Cell::default();
            cell.set_symbol(&written);
            cell.bg = Color::Rgb(1, 2, 3);
            let wide = obelus_text::text_width(&written).max(1);
            page.apply(Update::Cell {
                x: column,
                y: 0,
                cell: Box::new(cell),
            });
            // What `ratatui` leaves behind the right half of a wide
            // character: a cell with nothing in it and no colours.
            for rest in 1..wide {
                page.apply(Update::Cell {
                    x: column + rest as u16,
                    y: 0,
                    cell: Box::new(Cell::default()),
                });
            }
            column += wide as u16;
        }
        page
    }

    /// A full-width character's colour covers both of its columns.
    ///
    /// Deliberate break: reading each cell's own background -- which is
    /// what a terminal's front end can do, because a terminal draws the
    /// second half itself -- puts a run of the default colour between every
    /// pair of Chinese characters, and this counts them.
    #[test]
    fn a_wide_character_owns_the_colour_of_both_its_cells() {
        let page = page("\u{4e2d}\u{6587}");
        let coloured: Vec<_> = runs(&page, 0)
            .into_iter()
            .filter(|&(_, _, colour)| colour == Color::Rgb(1, 2, 3))
            .collect();
        assert_eq!(coloured, vec![(0, 4, Color::Rgb(1, 2, 3))]);
    }

    /// And the picture the glass reads gives it both of them too.
    ///
    /// The glass shows what the pane was put over, painted into a picture
    /// of its own -- and that was a ground and a glyph per cell, a column
    /// at a time. A full-width character is one glyph over two columns
    /// whose second is a cell `ratatui` has reset, so that cell's ground
    /// was painted straight over the right half of the character: a page
    /// of Chinese read through the glass was half of every character, and
    /// the halving arrived as the pane slid down over it.
    ///
    /// Deliberate break: read each cell's own column count --
    /// `(1, under.background)`, which is the ground-per-cell this replaced
    /// -- and a run begins at the character's second column, which is the
    /// rectangle that did the covering. The order is not asserted here
    /// because it is not this function's to get wrong: it hands back the
    /// grounds and the letters as two lists, and what is left is that no
    /// ground begins inside a character for a letter to be drawn under.
    #[test]
    fn so_does_the_picture_a_panes_glass_reads() {
        let behind = behind("\u{4e2d}\u{6587}");
        let (grounds, lettered) = behind_row(&behind, &[], 0);

        // Both characters under one run of their own colour: nothing of
        // the reset cells is left to be drawn over a glyph.
        let coloured: Vec<_> = grounds
            .iter()
            .filter(|&&(_, _, colour)| colour == Color::Rgb(1, 2, 3))
            .copied()
            .collect();
        assert_eq!(
            coloured,
            vec![(0, 4, Color::Rgb(1, 2, 3))],
            "a ground of its own at a character's second column:\n{grounds:?}"
        );

        // And the letters are the characters themselves, not the cells
        // beside them: a glyph drawn at a reset cell would be a character
        // drawn twice, half a character along.
        assert_eq!(lettered, vec![0, 2], "the wrong columns carry a letter");

        // Which is the property underneath the first assertion, said
        // about the character rather than about the colour: a ground may
        // cover a character, and may not begin part way through one. A
        // rectangle that begins there is a rectangle over half a glyph,
        // whatever colour it is carrying.
        for &(start, _, _) in &grounds {
            for &x in &lettered {
                let wide = behind.look(x, 0).map_or(1, |under| under.columns());
                assert!(
                    start <= x || start >= x + wide,
                    "a ground begins at {start}, part way through the character at {x}"
                );
            }
        }
    }

    /// And the rest of the row is still said in as few runs as it can be.
    ///
    /// Deliberate break: a rectangle per cell -- the obvious way to write
    /// this -- makes it twelve.
    #[test]
    fn a_row_is_as_few_runs_as_it_can_be() {
        assert_eq!(runs(&page("ab"), 0).len(), 2);
    }

    /// The colours a theme names are the colours that are drawn.
    ///
    /// Deliberate break: converting from sRGB to linear on the way through
    /// -- which is what a pipeline drawing to an sRGB surface has to do --
    /// moves every one of these.
    #[test]
    fn a_colour_goes_through_as_it_was_written() {
        let [r, g, b, a] = rgba(Color::Rgb(0x1e, 0x1e, 0x2e), Ink::Background);
        assert!((r - 30.0 / 255.0).abs() < f32::EPSILON);
        assert!((g - 30.0 / 255.0).abs() < f32::EPSILON);
        assert!((b - 46.0 / 255.0).abs() < f32::EPSILON);
        assert!((a - 1.0).abs() < f32::EPSILON);
    }

    /// The cube and the grey ramp are the ones every terminal agrees on.
    ///
    /// Deliberate break: stepping the cube evenly from zero to 255, which
    /// is the obvious thing and is not what a terminal does.
    ///
    /// Which is why the middle of the cube is what this asserts on. The
    /// ends of it were here first and are worth nothing: `21` and `196` are
    /// made of the first and last step, and those two are the same number
    /// in both -- so the even ramp passed, and a test that passes with the
    /// thing it covers broken is not a test.
    #[test]
    fn the_palette_is_the_one_a_terminal_has() {
        assert_eq!(indexed(0), (0x00, 0x00, 0x00));
        assert_eq!(indexed(16), (0x00, 0x00, 0x00));
        // One step along each axis of the cube: 16 + 36 + 2 * 6 + 3.
        assert_eq!(indexed(67), (0x5f, 0x87, 0xaf));
        assert_eq!(indexed(196), (0xff, 0x00, 0x00));
        assert_eq!(indexed(232), (0x08, 0x08, 0x08));
        assert_eq!(indexed(255), (0xee, 0xee, 0xee));
    }
}
