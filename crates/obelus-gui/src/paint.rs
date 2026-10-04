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

use std::{collections::HashMap, ops::Range, sync::Arc, time::Instant};

use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use cosmic_text::{CacheKey, SwashContent};
use obelus_app::app::Caret;
use obelus_ui::{
    image::{Palette, SLOT},
    shapes::{About, Joined, Side},
};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier},
};
use winit::window::Window;

use crate::{
    font::{self, CellSize, Fonts, Size},
    grid::{
        Barred, Behind, Capped, Look, Marked, Page, Parted, Rolled, Ruled, Said, Spelling, Stroked,
        Ticked,
    },
    motion::Moving,
};

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
    /// bindings that read it.
    scratch: wgpu::TextureView,
    scratch_bindings: wgpu::BindGroup,
    /// And the whole frame as one, which is drawn only while a pane is on
    /// its way in.
    picture: wgpu::TextureView,
    /// The bindings that read that one, for the two quads that put it back
    /// on the screen.
    showing_bindings: wgpu::BindGroup,
    /// The same bindings with something else in the backdrop's place, for
    /// the pass that *draws* it: a texture cannot be read and written in
    /// one pass, and what goes in the slot is never sampled there.
    plain_bindings: wgpu::BindGroup,
    /// What goes in a picture's slot where none is read: a pixel, because
    /// a binding cannot be left empty and the atlas is the wrong shape for
    /// the slot.
    nothing: wgpu::TextureView,
}

/// What one pane's glass reads: what is behind the pane, as a picture of
/// the window; the same picture blurred; and the bindings that read both.
///
/// The blur is a picture of its own rather than worked out by the glass,
/// because a blur worked out a pixel at a time is a handful of taps, and a
/// handful of taps over text is a handful of faint copies of it -- a
/// picture that looks enlarged from one a fraction of its size. Blurred
/// one way and then the other, which is what a Gaussian allows and is a
/// few dozen samples a pixel where doing it at once would be several
/// hundred.
struct Seen {
    backdrop: wgpu::TextureView,
    blurred: wgpu::TextureView,
    bindings: wgpu::BindGroup,
}

impl Seen {
    /// Both pictures, the size of the window.
    fn made(binder: &Binder<'_>, format: wgpu::TextureFormat, width: u32, height: u32) -> Self {
        let backdrop = made_to_draw_into(binder.device, format, width, height);
        let blurred = made_to_draw_into(binder.device, format, width, height);
        let bindings = binder.bound(&backdrop, &blurred);
        Self {
            backdrop,
            blurred,
            bindings,
        }
    }
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
    let mut plan = Vec::new();
    let mut at = placed.drawn;
    for &(glass, level) in &placed.gaps {
        plan.push((at..glass, Reads::Nothing));
        plan.push((glass..glass + 1, Reads::Level(level)));
        at = glass + 1;
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
    /// for glass, in pixels, the rectangle it is drawn inside.
    uv: [f32; 4],
    colour: [f32; 4],
    flags: u32,
    /// How far its corners are rounded, in pixels. Read only where
    /// `ROUNDED` is set.
    radius: f32,
    /// Which layer of the atlas `uv` is on: of the letters', or of the
    /// pictures' where the quad is `COLOURFUL`.
    layer: u32,
    /// How much further down than where it stands a glass reads what is
    /// behind it, in pixels. Nothing else reads it -- see
    /// `Painter::glass_kept_still`.
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

/// The face of a held run, which is frosted, as against the rim under
/// it, which is not -- see `grain` in the shader. Past the turns for the
/// same reason the wedge is.
const FROSTED: u32 = 4_194_304;

/// A letter the light on the welcome screen's mark runs across.
///
/// Set on the glyphs rather than on a rectangle over them, because what is
/// lit is the ink: a band drawn over the plate would light the page showing
/// between the letters as well, which is a lamp behind the mark and not a
/// sheen on it.
const SHEENED: u32 = 2048;

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
    /// How many pixels a point is, which is what a hold's grain is
    /// measured in -- see `GRAIN_SIZE` in the shader.
    scale: f32,
    /// The hardware reads a uniform in sixteens; nothing reads these.
    padding: [f32; 3],
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
    /// Takes over the window.
    ///
    /// Asking for an adapter and a device are the only two `await`s in the
    /// whole binary, and they happen once: blocked on rather than given a
    /// runtime, which would be a runtime for two calls.
    pub(crate) fn new(window: Arc<Window>) -> Result<Self> {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(
            // From the environment, so that `WGPU_BACKEND=gl` on a machine
            // whose Vulkan driver is the problem is a thing the reader can
            // try without a rebuild.
            wgpu::InstanceDescriptor::new_without_display_handle_from_env(),
        );
        let surface = instance
            .create_surface(Arc::clone(&window))
            .context("no surface to draw on")?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            // A grid of text is not work for a discrete card, and asking
            // for one on a laptop is asking for the fan and the battery.
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
            apply_limit_buckets: false,
        }))
        .context("no graphics adapter this window can be drawn with")?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("obelus"),
            required_features: wgpu::Features::empty(),
            // The floor every desktop adapter clears, so that what is asked
            // for is what a window needs rather than what this machine
            // happens to have.
            // What this machine offers, not a floor. The floor allows a
            // texture of 2048 pixels, and a window is a texture: a
            // maximised Obelus on a tall screen asked for 1882 by 2052 and
            // the surface refused it -- which is a panic on a resize, not
            // a degraded picture.
            required_limits: adapter.limits(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        }))
        .context("the graphics adapter would not open a device")?;

        let most = device.limits().max_texture_dimension_2d;
        let mut configured = surface
            .get_default_config(
                &adapter,
                size.width.clamp(1, most),
                size.height.clamp(1, most),
            )
            .context("the surface offers no way to be drawn on")?;
        let view = configured.format.remove_srgb_suffix();
        // The surface is drawn through a view that does not convert, and
        // the view has to be declared before the surface is configured.
        configured.view_formats = vec![view];
        let capabilities = surface.get_capabilities(&adapter);
        // Waiting for the screen rather than racing it: Obelus draws when
        // something happened, so there is never a frame to throw away.
        //
        // Except on Wayland, where FIFO is the compositor's to release, and
        // Hyprland on NVIDIA was seen not to: every acquire ran out its
        // second, so a window drew a frame a second for most of a minute
        // with nothing busy on either side -- with the driver's explicit
        // sync turned off as well. Mailbox never waits on that, and the
        // pacing FIFO gave an animation comes from the frame callback
        // instead, which `pre_present_notify` asks winit for. Only there,
        // because elsewhere that call paces nothing and Mailbox would draw
        // an animation as fast as the card can.
        configured.present_mode = if on_wayland(&window)
            && capabilities
                .present_modes
                .contains(&wgpu::PresentMode::Mailbox)
        {
            wgpu::PresentMode::Mailbox
        } else {
            wgpu::PresentMode::AutoVsync
        };
        // Said, because the default is whatever the platform would rather
        // do and on a Wayland compositor that is to honour the alpha
        // channel: a page drawn in a theme's own dark background came out
        // with the wallpaper showing through it. Obelus's window is not a
        // transparent window.
        if capabilities
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::Opaque)
        {
            configured.alpha_mode = wgpu::CompositeAlphaMode::Opaque;
        }
        surface.configure(&device, &configured);
        // What the window turned out to be drawn with, which is the line to
        // read when a colour or a shape is not what the theme says.
        tracing::info!(
            adapter = adapter.get_info().name,
            backend = ?adapter.get_info().backend,
            format = ?configured.format,
            view = ?view,
            alpha = ?configured.alpha_mode,
            offered = ?capabilities.alpha_modes,
            present = ?configured.present_mode,
            presents = ?capabilities.present_modes,
            "drawing with"
        );

        let atlas = Atlas::new(&device, &queue);
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("obelus screen"),
            size: std::mem::size_of::<Screen>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("obelus glyphs"),
            // Nearest, because a glyph is rasterised at the size it is drawn
            // at: there is nothing to interpolate, and interpolating it
            // would be blurring text that was sharp.
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("obelus"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    // The fragment stage reads it too, since the glass:
                    // what it samples is the window, so it has to know how
                    // big the window is.
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        // Smooth, unlike the glyphs': what this samples is a picture being
        // bent, and bending it a pixel at a time is what a staircase is.
        let smooth = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("obelus behind"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let (width, height) = (configured.width, configured.height);
        let binder = Binder {
            device: &device,
            layout: &layout,
            uniforms: &uniforms,
            letters: &atlas.letters.view,
            pictures: &atlas.pictures.view,
            sampler: &sampler,
            smooth: &smooth,
        };
        let nothing = made_to_draw_into(&device, view, 1, 1);
        let levels = vec![Seen::made(&binder, view, width, height)];
        let scratch = made_to_draw_into(&device, view, width, height);
        let scratch_bindings = binder.bound(&scratch, &nothing);
        let picture = made_to_draw_into(&device, view, width, height);
        let showing_bindings = binder.bound(&picture, &nothing);
        let plain_bindings = binder.bound(&nothing, &nothing);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("obelus"),
            source: wgpu::ShaderSource::Wgsl(include_str!("paint.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("obelus"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("obelus"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Quad>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x4,
                        1 => Float32x4,
                        2 => Float32x4,
                        3 => Uint32,
                        4 => Float32,
                        5 => Uint32,
                        6 => Float32,
                    ],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: view,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("obelus quads"),
            size: (std::mem::size_of::<Quad>() * 4096) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Ok(Self {
            window,
            surface,
            device,
            queue,
            configured,
            view,
            pipeline,
            uniforms,
            atlas,
            drawings: HashMap::new(),
            palette: None,
            ground: Color::Reset,
            grid: [0.0, 0.0],
            margin: [0.0, 0.0],
            titled: 0.0,
            holding: (Color::Reset, Color::Reset),
            quads: Vec::new(),
            placed: Placed::default(),
            instances,
            layout,
            sampler,
            smooth,
            levels,
            scratch,
            scratch_bindings,
            picture,
            showing_bindings,
            plain_bindings,
            nothing,
        })
    }

    /// The window changed size, so the surface has to.
    pub(crate) fn resized(&mut self, width: u32, height: u32) {
        // Clamped, because a surface bigger than the largest texture this
        // device can make is not a picture that comes out wrong -- it is a
        // validation error, which is to say a panic while the reader drags
        // a corner.
        let most = self.device.limits().max_texture_dimension_2d;
        if width > most || height > most {
            // Said, because what it does instead of failing is draw a
            // surface smaller than the window and let the compositor
            // stretch it: the picture goes soft, and a reader who cannot
            // see why would have nothing to go on.
            tracing::warn!(
                width,
                height,
                most,
                "the window is larger than this device can draw in one texture"
            );
        }
        self.configured.width = width.clamp(1, most);
        self.configured.height = height.clamp(1, most);
        self.surface.configure(&self.device, &self.configured);
        // Every picture is a picture of the window, so it is the size of
        // the window: one that stayed the old size would be sampled at the
        // wrong place for every pixel of the glass.
        let (width, height) = (self.configured.width, self.configured.height);
        let binder = self.binder();
        let levels = (0..self.levels.len())
            .map(|_| Seen::made(&binder, self.view, width, height))
            .collect();
        let scratch = made_to_draw_into(&self.device, self.view, width, height);
        let scratch_bindings = binder.bound(&scratch, &self.nothing);
        let picture = made_to_draw_into(&self.device, self.view, width, height);
        let showing_bindings = binder.bound(&picture, &self.nothing);
        self.levels = levels;
        (self.scratch, self.scratch_bindings) = (scratch, scratch_bindings);
        (self.picture, self.showing_bindings) = (picture, showing_bindings);
    }

    /// Every bind group again, over the same pictures, because the atlas
    /// they read is a different texture now.
    fn bound_again(&mut self) {
        let binder = self.binder();
        let levels: Vec<wgpu::BindGroup> = self
            .levels
            .iter()
            .map(|seen| binder.bound(&seen.backdrop, &seen.blurred))
            .collect();
        let scratch = binder.bound(&self.scratch, &self.nothing);
        let showing = binder.bound(&self.picture, &self.nothing);
        let plain = binder.bound(&self.nothing, &self.nothing);
        for (seen, bindings) in self.levels.iter_mut().zip(levels) {
            seen.bindings = bindings;
        }
        self.scratch_bindings = scratch;
        self.showing_bindings = showing;
        self.plain_bindings = plain;
    }

    /// What every bind group is made of, but the two pictures it reads.
    fn binder(&self) -> Binder<'_> {
        Binder {
            device: &self.device,
            layout: &self.layout,
            uniforms: &self.uniforms,
            letters: &self.atlas.letters.view,
            pictures: &self.atlas.pictures.view,
            sampler: &self.sampler,
            smooth: &self.smooth,
        }
    }

    /// The text is a different size now, so nothing kept about its glyphs
    /// -- or about the marks, which are drawn to fit a cell -- is about
    /// this size.
    pub(crate) fn forget_the_glyphs(&mut self) {
        self.atlas.empty();
    }

    /// A mark the window may be asked to draw, and what it is drawn from.
    ///
    /// Kept as the drawing rather than turned into pixels here: what size
    /// to draw it at is known at the moment it is drawn, and a reader who
    /// changes the text's size changes it.
    pub(crate) fn carries(&mut self, id: String, focused: bool, svg: String, palette: Palette) {
        if self.palette != Some(palette) {
            // Pixels cannot be recoloured after the fact, so a new theme
            // is every mark drawn again. The same rule the terminal's side
            // of this follows.
            self.drawings.clear();
            self.atlas.forget_the_marks();
            self.palette = Some(palette);
        }
        tracing::debug!(id, focused, "the window carries a mark");
        self.drawings.insert((id, focused), svg);
    }

    /// Draws a page, the marks on it, and whatever is being spelled over
    /// it.
    pub(crate) fn paint(
        &mut self,
        page: &Page,
        fonts: &mut Fonts,
        spelling: Option<&Spelling>,
        moving: Moving,
        said: Said<'_>,
    ) -> Result<()> {
        let cell = fonts.cell();
        // Before anything is placed, which is the one moment starting the
        // glyphs again is safe: nothing from the last frame is read after it.
        if std::mem::take(&mut self.atlas.overflowed) {
            self.atlas.empty();
        }
        self.quads.clear();
        self.placed = Placed::default();
        #[expect(
            clippy::cast_precision_loss,
            reason = "a window is thousands of pixels, not millions"
        )]
        let (across, down) = (self.configured.width as f32, self.configured.height as f32);
        let margin = crate::grid::origin(
            [across, down],
            self.titled,
            [cell.width, cell.height],
            [page.columns(), page.rows()],
        );
        self.grid = [
            f32::from(page.columns()) * cell.width,
            f32::from(page.rows()) * cell.height,
        ];
        self.margin = margin;
        // The whole window, in the page's own ground, under everything.
        //
        // The cells are all one size and the grid is middled in the
        // window, so there is a margin round it that no cell reaches: this
        // is what is in it. Before the pane's own cells, so it is in the
        // picture taken of what is behind one as well -- where it used to
        // be the black the pass clears to.
        let [left, top, wide, tall] = whole_window([across, down], margin);
        self.block(left, top, wide, tall, rgba(self.ground, Ink::Background));
        // Every pane and box, furthest first, each over the glass of those
        // before it. A box is one only while its frame is still there to
        // hold it -- see `Behind::framed` -- which four passes ask about.
        let stack = uncovered(
            said.stack
                .iter()
                .filter(|over| !over.is_a_box() || over.framed(page))
                .collect(),
        );
        // The bands put back from further up the frame, which is all of
        // them unless a pane is arriving -- then the pane is what moves.
        let catching_up = match moving.pane.is_some() && stack.iter().any(|over| !over.is_a_box()) {
            true => &[][..],
            false => said.bands,
        };
        // First of everything, because the first of these is what the
        // first picture draws and it draws the front of the buffer.
        for (at, over) in stack.iter().enumerate() {
            let lower = &stack[..at];
            let start = match at {
                // The window's own ground is under the first one too.
                0 => 0,
                _ => self.quads.len(),
            };
            match (at, over.is_a_box()) {
                (0, false) => self.pane_under(over, said.barred, said.capped, fonts),
                _ => self.seen_under(over, lower, fonts),
            }
            let under = start..self.quads.len();
            let (glass, rect) = match over.is_a_box() {
                true => self.box_glass(page, over, lower, cell),
                false => self.pane_glass(page, over, lower, said.ruled, cell),
            };
            let lowered = self.glass_kept_still(glass, at, &stack, catching_up, cell);
            self.placed.levels.push(Level {
                under,
                glass,
                lowered,
                end: self.quads.len(),
                rect,
                pane: !over.is_a_box(),
                blur: 0,
            });
        }
        // The pane on top, which is the one that arrives: a list opened
        // over the settings comes up over a page that was already there.
        let top = stack.iter().rposition(|over| !over.is_a_box());
        let pane = top.map(|top| self.placed.levels[top].rect);
        // And the first, where it is a pane: a band scrolling under glass
        // is put back into the first picture -- see `sliding_under`.
        let first = stack.first().copied().filter(|over| !over.is_a_box());
        // The shapes the page still holds -- see `Ticked::still_said` and
        // the one beside it, and `Capped`'s a few lines down, which is
        // kept apart only because the caps are wanted as borrows. Asked
        // here rather than where each is drawn, so that the cells
        // `letters` leaves alone are the cells a shape is drawn over.
        //
        // A bar is not among them, and answers the same question finer:
        // where a switch and a mark are one cell and a run of them either
        // is or is not still there, a bar is a column and the thing over
        // it may take the middle of it -- so it is asked row by row, at
        // the two places it matters, and `Barred::runs` is that answer.
        let ticked: Vec<Ticked> = said
            .ticked
            .iter()
            .copied()
            .filter(|tick| tick.still_said(page))
            .collect();
        let marked: Vec<Marked> = said
            .marked
            .iter()
            .filter(|mark| mark.still_said(page))
            .cloned()
            .collect();
        let said = Said {
            ticked: &ticked,
            marked: &marked,
            ..said
        };
        let panes = &stack;
        let framed: Vec<&Behind> = stack
            .iter()
            .copied()
            .filter(|over| over.is_a_box())
            .collect();
        // The caps still about the cells they were said about -- see
        // `Capped::still_said`.
        let capped: Vec<&Capped> = said
            .capped
            .iter()
            .filter(|cap| cap.still_said(page))
            .collect();
        self.backgrounds(page, cell, panes, &framed, &capped);
        // Over the page and under everything written on it: a hold is a
        // ground, and the letters it is behind are the ones the reader is
        // holding.
        self.holdings(page, panes, &framed, &capped, cell);
        self.rules(page, said.ruled, cell);
        // The boundaries with no row to be on, which go where there is no
        // cell: the pixel between one row and the one above it. Under the
        // letters, like a rule, because a line through a letter is a line
        // nobody put there -- and there is nothing on that pixel to be
        // under except the ground.
        self.partings(page, said.parted, panes, cell);
        self.letters(page, fonts, said, &framed, &capped);
        // Over the glyph, the way a terminal draws one: a descender
        // crossing the line is what an underline looks like everywhere
        // else.
        self.underlines(page, cell);
        // Over the text, which it covers: a cap is the shape the cells
        // behind a key are, and it writes the key on itself, smaller than
        // the words beside it.
        self.caps(page, &capped, panes, fonts);
        // Over the letters: a switch replaces the glyph standing in for
        // it, rather than sitting beside one.
        self.ticks(page, said.ticked, panes, &framed, &capped, cell);
        // And so does a bar, for the same reason: what a terminal has for
        // a track is a column of full blocks, and a window has a shape.
        self.bars(page, said.barred, cell);
        // And so does a change mark: half a block per row is what a cell
        // has, and a window has one shape however many rows the hunk
        // covers.
        self.strokes(page, said.stroked, cell);
        // After the text, over cells the view left empty: a view draws its
        // glyph only where a picture could not be drawn.
        self.marks(said.marked, cell);
        // And after all of it, because a shadow falls on what is behind
        // the thing casting it and everything behind these has now been
        // drawn. Before the caret, which is the reader's own place and
        // is never in shadow.
        self.shadows(
            &stack,
            top.filter(|_| moving.pane.is_some()),
            moving.card.unwrap_or(1.0),
            cell,
        );
        // Over the cells and under the caret: the word being spelled is
        // going in at the caret, so the caret belongs at the place in it
        // the input method says.
        self.spelling(page, fonts, spelling);
        // Off for half of every cycle, which is the blink. What is under it
        // is drawn either way, by the pass above.
        if moving.caret {
            self.caret(page, fonts, spelling, moving.drift);
        }

        // A box put over the page travels nowhere, so what it does
        // instead is come up. The cells it was put over are already in a
        // picture of their own -- the one its glass reads -- so laying
        // those back over it, fading out, is the box fading in, and
        // nothing has to be drawn a second time. Over everything of the
        // box's, the caret in it included: a solid caret on a box that is
        // half there is the one part of it that has already arrived.
        let nearest = stack.iter().rposition(|over| over.is_a_box());
        if let (Some(nearest), Some(along)) = (nearest, moving.card) {
            self.placed.covered = Some((self.quads.len(), nearest));
            self.slid(box_of(stack[nearest].area, cell), 0.0, 1.0 - along);
        }

        // Everything the frame says has been said. What is left is
        // putting it back on the screen in two pieces, where a pane is on
        // its way in -- see `paint.wgsl`.
        self.placed.drawn = self.quads.len();
        if let (Some(top), Some(pane), Some(along)) = (top, pane, moving.pane) {
            let height = pane[3] - pane[1];
            // A pane comes from the side it is joined to, which is the
            // only side it could come from without crossing the page.
            let away = match Some(stack[top].joined) {
                Some(Joined::Below) => 1.0,
                _ => -1.0,
            };
            let shift = away * (1.0 - along) * height * TRAVEL;
            self.covering(&[pane]);
            self.slid(pane, shift, along);
            // And the shadow, here rather than in the frame -- see
            // `shadows`. It falls from the edge the pane has *reached*,
            // which is the only edge of it that has moved: the other is
            // the seam it is joined by, and the piece the pane is taken
            // from stops there. And it comes up as the pane does, because
            // a shadow at full strength under a pane that is still half
            // there is a shadow with nothing casting it.
            if let Some(joined) = casting(stack[top].joined) {
                self.shadow(
                    reached(pane, shift),
                    0.0,
                    joined,
                    cell.height * SHADOW_SPREAD,
                    along,
                );
            }
        } else if !said.bands.is_empty() {
            // The pane's cells rather than its glass, which starts half
            // way down the rule over it: the half row above the line,
            // slid, would be filled from inside the list.
            let panes: Vec<[f32; 4]> = stack
                .iter()
                .filter(|over| !over.is_a_box())
                .map(|over| box_of(over.area, cell))
                .collect();
            self.catching_up(said.bands, &panes, &stack, fonts);
        }
        // Last, because none of it is drawn on the screen: the blurs are
        // passes of their own, before any of the above.
        self.placed.moved = self.quads.len();
        for at in 0..self.placed.levels.len() {
            self.placed.levels[at].blur = self.blurs_over(self.placed.levels[at].rect);
        }
        if moving.pane.is_none()
            && let Some(first) = first
        {
            self.sliding_under(page, first, said.bands, fonts);
        }
        // After the last glyph has been placed and before anything is drawn:
        // a glyph that found no room made the texture again, and every
        // binding is still reading the one before.
        if std::mem::take(&mut self.atlas.remade) {
            self.bound_again();
        }
        // And a frame that went without a glyph asks for the one that will
        // have it, rather than waiting for a key the reader has no reason
        // to press.
        if self.atlas.overflowed {
            self.window.request_redraw();
        }

        // Where the light is, in the pixels a fragment knows itself by:
        // the mark's own left edge plus how far along it the clock has
        // brought it, and the grid's origin, because a fragment's `x` is
        // the screen's rather than the grid's.
        let (sheen, glow) = match said.sheened.zip(moving.sheen) {
            Some((mark, along)) => {
                let left = f32::from(mark.area.x) * cell.width;
                let wide = f32::from(mark.area.width) * cell.width;
                (
                    [
                        margin[0] + left + wide * along,
                        (wide * crate::motion::SHEEN_WIDTH).max(1.0),
                        1.0,
                        0.0,
                    ],
                    rgba(mark.to, Ink::Foreground),
                )
            }
            // Nothing carried, so the letters keep the colour they were
            // given: the mark at rest is the mark in one colour.
            None => ([0.0, 1.0, 0.0, 0.0], [0.0; 4]),
        };
        #[expect(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            reason = "a window is thousands of pixels, not millions, and a scale factor is a small number"
        )]
        let screen = Screen {
            size: [self.configured.width as f32, self.configured.height as f32],
            origin: margin,
            sheen,
            glow,
            // Asked every frame rather than kept: a window dragged to
            // another screen changes it.
            scale: self.window.scale_factor() as f32,
            padding: [0.0; 3],
        };
        self.queue
            .write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&screen));

        let wanted = (std::mem::size_of::<Quad>() * self.quads.len().max(1)) as u64;
        if wanted > self.instances.size() {
            // Doubled, so that a screen that keeps growing does not
            // reallocate on every frame of the resize.
            self.instances = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("obelus quads"),
                size: wanted * 2,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        self.queue
            .write_buffer(&self.instances, 0, bytemuck::cast_slice(&self.quads));

        let acquiring = Instant::now();
        let acquired = self.surface.get_current_texture();
        let waited = acquiring.elapsed();
        if waited >= crate::window::SLOW {
            tracing::warn!(?waited, "the surface took this long to give up a frame");
        }
        let frame = match acquired {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            // The surface went out from under the frame -- the window was
            // resized, a monitor changed -- which is not an error to stop
            // for: it is put back, and the frame that was being drawn is
            // drawn again by the redraw the resize itself asks for.
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.configured);
                return Ok(());
            }
            // Nobody can see it: the window is hidden, or the driver did
            // not answer in time. Both are reasons to skip a frame rather
            // than to stop.
            wgpu::CurrentSurfaceTexture::Occluded | wgpu::CurrentSurfaceTexture::Timeout => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                anyhow::bail!("the surface refused to give up a frame to draw into")
            }
        };
        let target = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.view),
            ..Default::default()
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("obelus"),
            });
        let placed = self.placed.clone();
        self.levels_for(placed.levels.len());
        // What is behind the first pane, into a picture of its own. Only the
        // front of the buffer, which is exactly those cells -- and only
        // where there is a pane at all, so a window with nothing over the
        // page does none of this.
        match placed.under.clone() {
            // A band scrolling under the pane: what is behind it is drawn
            // where it stands into the frame's picture, which nothing has
            // drawn into yet, and put back into the backdrop with the band
            // taken from where it has got to -- the same two steps the
            // screen is put back in, so what the glass shows moves with
            // what is round it.
            Some((above, put_back)) => {
                {
                    let mut pass =
                        self.pass(&mut encoder, "obelus behind, standing", &self.picture);
                    pass.set_bind_group(0, &self.plain_bindings, &[]);
                    drawing(&mut pass, 0..placed.behind());
                    drawing(&mut pass, above);
                }
                let mut pass = self.pass(&mut encoder, "obelus behind", &self.levels[0].backdrop);
                pass.set_bind_group(0, &self.showing_bindings, &[]);
                drawing(&mut pass, put_back);
            }
            None if !placed.levels.is_empty() => {
                let mut pass = self.pass(&mut encoder, "obelus behind", &self.levels[0].backdrop);
                self.beneath(&mut pass, 0);
            }
            None => {}
        }
        // And each one over it, in order: a picture of the screen as it was
        // before that one was put over it, which reads the pictures before
        // it and so has to come after them. Reading one while drawing into
        // another, which is two textures and allowed.
        for (at, level) in placed.levels.iter().enumerate() {
            if at > 0 {
                let mut pass = self.pass(
                    &mut encoder,
                    "obelus behind the next",
                    &self.levels[at].backdrop,
                );
                self.beneath(&mut pass, at);
            }
            self.blurring(&mut encoder, level.blur, &self.levels[at]);
        }
        // A pane on its way in: the frame goes into a picture of its own
        // first, and the screen is put together out of it below. Only
        // while one is moving -- an arrived pane is drawn straight to the
        // screen like everything else.
        let composing = placed.moved > placed.drawn;
        if composing {
            // The glass is drawn in here, and what it reads is the
            // backdrop -- which this pass is not writing to.
            let mut pass = self.pass(&mut encoder, "obelus frame", &self.picture);
            self.the_frame(&mut pass);
        }
        {
            // Cleared to black, which is never seen: the first quad of
            // every frame is the whole window in the page's own ground,
            // and the cells are drawn over that.
            let mut pass = self.pass(&mut encoder, "obelus", &target);
            match composing {
                // The page first, and nothing of the pane: the glass is
                // part of the pane and arrives with it. Left standing in
                // place, it was a pane already open with only its words
                // sliding into it -- which is not what the list does.
                true => {
                    // What the pane on top was put over, which is the
                    // picture its glass reads.
                    if let Some(top) = placed.levels.iter().rposition(|level| level.pane) {
                        self.beneath(&mut pass, top);
                    }
                    self.follow(&mut pass, &catching(&placed), &self.showing_bindings);
                }
                false => self.the_frame(&mut pass),
            }
        }
        self.queue.submit([encoder.finish()]);
        // Only where a frame really goes: the callback this asks for comes
        // with a commit, and one asked for with nothing committed is a
        // redraw winit holds back for ever.
        self.window.pre_present_notify();
        let presenting = Instant::now();
        self.queue.present(frame);
        let waited = presenting.elapsed();
        if waited >= crate::window::SLOW {
            tracing::warn!(?waited, "the surface took this long to take a frame");
        }
        Ok(())
    }

    /// The colour behind the text, as few rectangles as it takes.
    ///
    /// A run of cells with the same background is one rectangle: a screen
    /// is mostly the page's own colour, and a quad per cell would be ten
    /// thousand of them to say so.
    fn backgrounds(
        &mut self,
        page: &Page,
        cell: CellSize,
        panes: &[&Behind],
        framed: &[&Behind],
        capped: &[&Capped],
    ) {
        let (width, height) = (cell.width, cell.height);
        for row in 0..page.rows() {
            // A frame's own cells, which `frame` has drawn already: what
            // the cells hold there is a terminal's square corner under a
            // round one.
            let ring: Vec<u16> = (0..page.columns())
                .filter(|&x| framed.iter().any(|card| card.ring_holds(page, x, row)))
                .collect();
            for (start, end, colour) in runs(page, row) {
                // The pane's own colour inside the pane is where the glass
                // is: it is the pane saying nothing there, and drawing it
                // would be painting over what the reader is meant to see
                // through. Anything else it wears -- a selected row, a
                // tab, a rule -- is the pane speaking, and stays.
                let mut holes: Vec<(u16, u16)> = ring.iter().map(|&x| (x, x + 1)).collect();
                holes.extend(
                    panes
                        .iter()
                        .filter(|pane| {
                            colour == pane.ground && row >= pane.area.y && row < pane.area.bottom()
                        })
                        .map(|pane| (pane.area.x, pane.area.right())),
                );
                // And a cap on glass, whose cells are the run a shade off
                // the page that is a terminal's cap: `caps` draws the
                // rounded one, and what is round its corners is the glass.
                // Painted, the square the cells make stood behind the
                // round cap as a ground of its own.
                holes.extend(
                    capped
                        .iter()
                        .filter(|cap| {
                            cap.area.y == row
                                && seen_through(panes, cap.area.x, cap.area.y, cap.page)
                        })
                        .map(|cap| (cap.area.x, cap.area.right())),
                );
                // And what the reader has hold of, which `holdings` draws
                // as a plate: painted here as well, the square the cells
                // make would stand behind the plate's round corners in
                // the full strength of the colour.
                holes.extend(
                    self.holds(page, row, capped)
                        .into_iter()
                        .map(|(from, to, _)| (from, to)),
                );
                for (start, end) in without(start, end, &mut holes) {
                    self.block(
                        f32::from(start) * width,
                        f32::from(row) * height,
                        f32::from(end - start) * width,
                        height,
                        rgba(colour, Ink::Background),
                    );
                }
            }
        }
    }

    /// The runs of one row the reader has hold of.
    ///
    /// A run of cells wearing one of the two colours the theme gives a
    /// hold -- see `Drawing::holding` -- which is how the window finds
    /// one without any view having to say so: a list drawn next month is
    /// drawn like every other list because it paints the row that colour,
    /// which it has to do anyway for the terminal.
    ///
    /// Minus the caps. A key's cap is a run a shade off the page and the
    /// themes Obelus ships give that the same colour as a selected row --
    /// two different promises that a theme is entitled to keep in one
    /// colour. What tells them apart is that a cap has already been said,
    /// so it is already drawn as its own shape.
    fn holds(&self, page: &Page, row: u16, capped: &[&Capped]) -> Vec<(u16, u16, Color)> {
        let (held, chosen) = self.holding;
        let mut found = Vec::new();
        for (start, end, colour) in runs(page, row) {
            if colour != held && colour != chosen {
                continue;
            }
            let mut caps: Vec<(u16, u16)> = capped
                .iter()
                .filter(|cap| cap.area.y == row)
                .map(|cap| (cap.area.x, cap.area.right()))
                .collect();
            found.extend(
                without(start, end, &mut caps)
                    .into_iter()
                    .map(|(from, to)| (from, to, colour)),
            );
        }
        found
    }

    /// What the reader has hold of, drawn as a plate rather than a square.
    ///
    /// The face is carried most of the way from what is under it to the
    /// colour the cells wear, and the colour itself is the rim round it
    /// -- see `HELD`. `backgrounds` leaves the run unpainted for this,
    /// the way it leaves a cap's cells unpainted, so what shows outside
    /// the rounded corners is whatever the hold is standing on: the
    /// page, or a pane's own glass.
    ///
    /// A hold that carries on into the row above or below keeps its
    /// corners square on that side and its face runs to the edge, so a
    /// selection several lines tall is one plate rather than a stack of
    /// them with a seam between each pair.
    fn holdings(
        &mut self,
        page: &Page,
        panes: &[&Behind],
        framed: &[&Behind],
        capped: &[&Capped],
        cell: CellSize,
    ) {
        let rim = (cell.height * HELD_EDGE).round().max(1.0);
        let corner = cell.height * HELD_CORNER;
        let rows: Vec<Vec<(u16, u16, Color)>> = (0..page.rows())
            .map(|row| self.holds(page, row, capped))
            .collect();
        for (row, holds) in rows.iter().enumerate() {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "a row of a grid is inside the window"
            )]
            let at = row as u16;
            // Whether a box with a frame round it is over this cell. A
            // run that stops because something was put over it has not
            // stopped: the row carries on underneath, so that end is not
            // an end -- it is square, and it keeps no rim, the same as a
            // row the hold carries on into.
            let covered = |x: u16| {
                framed.iter().any(|card| {
                    (card.area.left()..card.area.right()).contains(&x)
                        && (card.area.top()..card.area.bottom()).contains(&at)
                })
            };
            let touching = |beside: Option<&Vec<(u16, u16, Color)>>, run: (u16, u16, Color)| {
                beside.is_some_and(|beside| {
                    beside
                        .iter()
                        .any(|&(from, to, colour)| colour == run.2 && from < run.1 && to > run.0)
                })
            };
            for &(start, end, colour) in holds {
                let above = touching(
                    row.checked_sub(1).and_then(|row| rows.get(row)),
                    (start, end, colour),
                );
                let below = touching(rows.get(row + 1), (start, end, colour));
                let cut = (start.checked_sub(1).is_some_and(covered), covered(end));
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "a window is thousands of pixels, not millions"
                )]
                let top = row as f32 * cell.height;
                let left = f32::from(start) * cell.width;
                let wide = f32::from(end - start) * cell.width;
                let ink = rgba(colour, Ink::Background);
                let under = self.under(panes, framed, start, at);
                // The rim, further from what is under it than the colour
                // itself: it is the thing being seen.
                let edge = mixed(ink, away(under, ink), HELD_RIM);
                // Which way each of the four corners turns, which is a
                // fact about the row beside it -- see `Turn`.
                let beside = |up: bool| {
                    let beside = match up {
                        true => row.checked_sub(1).and_then(|row| rows.get(row)),
                        false => rows.get(row + 1),
                    }?;
                    beside
                        .iter()
                        .find(|&&(from, to, theirs)| theirs == colour && from < end && to > start)
                        .copied()
                };
                let turns = Turn::corners(start, end, beside(true), beside(false), cut);
                self.plate([left, top, wide, cell.height], corner, edge, turns, 0);
                // The face, inside the rim where there is one. No inset
                // where the hold carries on: an edge there is a seam
                // across the middle of one thing.
                let (up, down) = (if above { 0.0 } else { rim }, if below { 0.0 } else { rim });
                let (near, far) = (if cut.0 { 0.0 } else { rim }, if cut.1 { 0.0 } else { rim });
                let face = [
                    left + near,
                    top + up,
                    (wide - near - far).max(0.0),
                    (cell.height - up - down).max(0.0),
                ];
                self.plate(
                    face,
                    (corner - rim).max(0.0),
                    mixed(under, ink, HELD),
                    turns,
                    FROSTED,
                );
            }
        }
    }

    /// The face a hold is drawn in at this cell, where the cell is part
    /// of one.
    ///
    /// Asked by anything that would otherwise put a cell's own ground
    /// back over a hold. The cells carry the colour at full strength and
    /// the plate is a shade of it, so a square of the raw colour is a
    /// hole in the plate -- which is the same thing a square of a pane's
    /// ground is in glass, and is guarded against a line above for the
    /// same reason.
    fn held_face(
        &self,
        page: &Page,
        at: ratatui::layout::Rect,
        panes: &[&Behind],
        framed: &[&Behind],
        capped: &[&Capped],
    ) -> Option<[f32; 4]> {
        let (from, _, colour) = self
            .holds(page, at.y, capped)
            .into_iter()
            .find(|&(from, to, _)| (from..to).contains(&at.x))?;
        let under = self.under(panes, framed, from, at.y);
        Some(mixed(under, rgba(colour, Ink::Background), HELD))
    }

    /// The page's own ground, as a colour.
    fn ground_colour(&self) -> [f32; 4] {
        rgba(self.ground, Ink::Background)
    }

    /// What a hold at this cell is standing on: a pane's own ground where
    /// it is inside one, and the page's otherwise.
    ///
    /// Nearest the reader wins, which is the order the cards are in.
    fn under(&self, panes: &[&Behind], framed: &[&Behind], x: u16, y: u16) -> [f32; 4] {
        let inside = |pane: &&&Behind| {
            let area = pane.area;
            (area.left()..area.right()).contains(&x) && (area.top()..area.bottom()).contains(&y)
        };
        let ground = framed
            .iter()
            .rev()
            .find(inside)
            .or_else(|| panes.iter().rev().find(inside))
            .map_or(self.ground, |pane| pane.ground);
        rgba(ground, Ink::Background)
    }

    /// A block of colour with each of its four corners taken off, or not,
    /// or bent the other way -- see `Turn`.
    ///
    /// The quad reaches a radius past the block on each side, because a
    /// corner bent the other way is drawn out there: the shader takes
    /// that much off again to find the block itself.
    fn plate(&mut self, rect: [f32; 4], radius: f32, colour: [f32; 4], turns: u32, frosted: u32) {
        let [left, top, width, height] = rect;
        let radius = radius.max(0.0).min(width.min(height) / 2.0);
        self.quads.push(Quad {
            rect: [left - radius, top, radius.mul_add(2.0, width), height],
            uv: self.atlas.white,
            colour,
            flags: SOLID | ROUNDED | HELD_PLATE | (turns << HELD_TURNS) | frosted,
            radius,
            layer: 0,
            lower: 0.0,
        });
    }

    /// One block of colour, in pixels.
    /// What the page is drawn on, for the margin round the grid.
    ///
    /// Handed over every frame rather than when the application says it:
    /// the painter is built after the application is told who is drawing,
    /// so the first one would have nowhere to land.
    pub(crate) const fn drawn_on(&mut self, ground: Color) {
        self.ground = ground;
    }

    /// Which colours mean the reader has hold of something -- see
    /// `Drawing::holding`. Handed over every frame, like the ground, and
    /// for the same reason.
    pub(crate) const fn holding(&mut self, holding: (Color, Color)) {
        self.holding = holding;
    }

    /// How much of the top of the window is the title bar's. Handed over
    /// every frame, like the ground: the window measures it when it
    /// changes size, and a second copy kept here would be one to forget.
    pub(crate) const fn titled(&mut self, titled: f32) {
        self.titled = titled;
    }

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

    /// The bands drawn behind where their lists have got to.
    ///
    /// The same pieces a pane arrives on, one set per band, and one thing
    /// before them. What a band shows while it catches up is partly on
    /// the frame that has just been drawn -- taken from it lower down,
    /// which is the band showing what it showed a moment ago -- and
    /// partly on no frame at all: the rows the list scrolled *past* are
    /// not on the new page, and the only place they exist is the page it
    /// scrolled off. So those pages are drawn first, where their rows
    /// have got to, and what covers them is the frame everywhere the
    /// bands are not -- which is also what cuts each page back to the
    /// band it belongs to, since a row shifted far enough lands outside
    /// it.
    ///
    /// And less the pane over a band, where one is: a transcript that
    /// scrolls on under a list is the transcript moving, not the list, and
    /// a band that took the cells over it along took the foot of the list
    /// with it -- every line an agent wrote behind a list was the list
    /// coming up again. A band the pane's own view drew is the list, and
    /// moves.
    fn catching_up(
        &mut self,
        bands: &[Rolled<'_>],
        panes: &[[f32; 4]],
        stack: &[&Behind],
        fonts: &mut Fonts,
    ) {
        let cell = fonts.cell();
        for band in bands {
            let room = band.room;
            // A list on glass: the page it scrolled off is on the glass,
            // which stands where it is, so that is drawn first and the
            // pane's own colour in those rows is left to it -- the same
            // hole `backgrounds` leaves.
            let glass = on_glass(band, stack);
            if let Some(level) = glass {
                self.placed.gaps.push((self.quads.len(), level));
                self.quads.push(Quad {
                    uv: box_of(room, cell),
                    ..self.quads[self.placed.levels[level].glass]
                });
            }
            // Where the page it scrolled off has got to, which is further
            // back than the band by however much of the move is already
            // done.
            let offset = (band.behind - band.since) * cell.height;
            for y in room.top()..room.bottom() {
                for x in room.left()..room.right() {
                    let look = band.before.look(x, y);
                    let left = f32::from(x) * cell.width;
                    let top = f32::from(y).mul_add(cell.height, offset);
                    if glass.is_none_or(|level| look.background != stack[level].ground) {
                        self.block(
                            left,
                            top,
                            cell.width,
                            cell.height,
                            rgba(look.background, Ink::Background),
                        );
                    }
                    if !look.text.trim().is_empty() {
                        let ink = rgba(look.foreground, Ink::Foreground);
                        self.glyphs_at((left, top), look, ink, 0, fonts, Size::Cell);
                    }
                }
            }
        }
        let rooms: Vec<[f32; 4]> = bands.iter().map(|band| box_of(band.room, cell)).collect();
        let moving: Vec<[f32; 4]> = bands
            .iter()
            .zip(&rooms)
            .flat_map(|(band, room)| sliding(*room, panes, band.under))
            .collect();
        self.covering(&moving);
        for (at, (band, room)) in bands.iter().zip(&rooms).enumerate() {
            // Less the rooms of the other bands, so that a band with one
            // inside it -- a hover over the file it is about -- does not
            // take the inner one along on its own journey.
            let others: Vec<[f32; 4]> = rooms
                .iter()
                .enumerate()
                .filter(|&(other, _)| other != at)
                .map(|(_, room)| *room)
                .collect();
            for part in sliding(*room, panes, band.under) {
                for piece in tiles(part, &others) {
                    self.slid_piece(piece, *room, band.behind * cell.height, 1.0);
                }
            }

            // And the bar, which is not in the band and does not stand
            // still either: its mark belongs where the band is being
            // *drawn*, which is its own share of the same distance
            // behind.
            //
            // Taken out of the same picture and not redrawn from the
            // page, which is what this did first and what made the column
            // flicker: a bar inside a pane sits on glass, and the cells it
            // is made of carry the pane's own colour -- painted back as
            // cells, that colour goes down opaque over what the reader was
            // seeing through. Out of the picture it is whatever it was,
            // glass included, moved.
            if let Some((bar, to_come)) = band.bar {
                self.slid(
                    box_of(bar, cell),
                    mark_behind(to_come, band.behind, band.since) * cell.height,
                    1.0,
                );
            }
        }
    }

    /// The glass under each list that is catching up, drawn again over its
    /// rows and reading what is behind it from as much further down as the
    /// rows are about to be taken from further up. Hands back how many.
    ///
    /// The rows are put back from higher up the frame -- see `catching_up`
    /// -- and the glass they are on is in the same picture: taken along
    /// with them, what is behind a list slid with the list and jumped back
    /// when it arrived. Read lower by the distance it is taken from, it
    /// stands still on the screen while the rows go over it.
    ///
    /// Straight after the glass it is a copy of, so that the rows are
    /// drawn over it -- the half row `pane_glass` paints under the status
    /// row's rule comes after, and is nowhere near a list's rows.
    fn glass_kept_still(
        &mut self,
        glass: usize,
        level: usize,
        stack: &[&Behind],
        bands: &[Rolled<'_>],
        cell: CellSize,
    ) -> usize {
        let copies = kept_still(&self.quads[glass], level, stack, bands, cell);
        let lowered = copies.len();
        self.quads.splice(glass + 1..glass + 1, copies);
        lowered
    }

    /// What is behind the first pane, with the bands under it slid the
    /// way `catching_up` slides them on the screen.
    ///
    /// The screen keeps the pane still and slides the band round it, and
    /// the glass is a picture of what is behind -- so without this the
    /// band came up to the pane's edge and stopped, and what showed through
    /// the glass jumped to where the band was going.
    ///
    /// The rows of the band above the pane are drawn as well, because a
    /// slide fills the top of the pane from above it. Where it would be
    /// filled from outside the band -- what scrolled past, which is on the
    /// page it scrolled off and is not behind anything -- the band is left
    /// where it is, which through the frost is the same rows.
    fn sliding_under(
        &mut self,
        page: &Page,
        behind: &Behind,
        bands: &[Rolled<'_>],
        fonts: &mut Fonts,
    ) {
        let cell = fonts.cell();
        let pane = box_of(behind.area, cell);
        let under: Vec<(&Rolled<'_>, [f32; 4])> = bands
            .iter()
            .filter_map(|band| Some((band, beneath(box_of(band.room, cell), pane, band.under)?)))
            .collect();
        if under.is_empty() {
            return;
        }
        let start = self.quads.len();
        for (band, _) in &under {
            let room = band.room;
            for y in room.top()..behind.area.top().min(room.bottom()) {
                for x in room.left()..room.right() {
                    let look = page.look(x, y);
                    let (left, top) = (f32::from(x) * cell.width, f32::from(y) * cell.height);
                    self.block(
                        left,
                        top,
                        cell.width,
                        cell.height,
                        rgba(look.background, Ink::Background),
                    );
                    if !look.text.trim().is_empty() {
                        let ink = rgba(look.foreground, Ink::Foreground);
                        self.glyphs_at((left, top), look, ink, 0, fonts, Size::Cell);
                    }
                }
            }
        }
        let above = start..self.quads.len();
        let start = self.quads.len();
        self.covering(&[]);
        for (band, piece) in &under {
            self.slid_piece(
                *piece,
                box_of(band.room, cell),
                band.behind * cell.height,
                1.0,
            );
        }
        self.placed.under = Some((above, start..self.quads.len()));
    }

    /// The frame that has just been drawn, put back on the screen
    /// everywhere the things that are moving are not.
    ///
    /// The frame has been drawn into a picture of its own by then, and
    /// what is moving is taken from higher up in that picture by `slid`.
    /// This is the rest of it, as the few rectangles the rest of it is --
    /// so what shows where a pane has not reached is whatever was drawn
    /// there before, which for a pane is the page and for a band is the
    /// page it scrolled off.
    ///
    /// As rectangles rather than as one quad that leaves a hole, because
    /// there may be several holes: two lists catching up at once are two
    /// rooms to keep clear, and a quad can only be told about one.
    fn covering(&mut self, rooms: &[[f32; 4]]) {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a window is thousands of pixels, not millions"
        )]
        let (right, bottom) = (self.configured.width as f32, self.configured.height as f32);
        let [left, top, wide, tall] = whole_window([right, bottom], self.margin);
        for tile in tiles([left, top, left + wide, top + tall], rooms) {
            let [left, top, far, low] = tile;
            self.quads.push(Quad {
                rect: [left, top, (far - left).max(1.0), (low - top).max(1.0)],
                // Nothing reads it: a piece of the frame is the picture at
                // the place the piece stands, and where it stands is its
                // own rectangle.
                uv: [0.0; 4],
                colour: [0.0, 0.0, 0.0, 1.0],
                flags: FRAME,
                radius: 0.0,
                layer: 0,
                lower: 0.0,
            });
        }
    }

    /// One region of the frame that has just been drawn, put back
    /// somewhere other than where it stands.
    ///
    /// `shift` is how far, in pixels, and what is under it where the
    /// region runs out is whatever was drawn there before -- the page for
    /// a pane that has not arrived, the frame's own copy for a bar whose
    /// mark has moved on.
    fn slid(&mut self, box_: [f32; 4], shift: f32, fade: f32) {
        self.slid_piece(box_, box_, shift, fade);
    }

    /// And a piece of one, which is a region with a hole in it: the piece
    /// is what is drawn and the region is what says where the picture is
    /// taken from and where it runs out.
    ///
    /// A band with another band inside it is the reason -- a hover over
    /// the file it is about. Both catch up on their own, so the outer one
    /// must not take the inner one with it: what it puts back is the
    /// region less the rooms of the bands inside it, and each of those
    /// puts back its own.
    fn slid_piece(&mut self, piece: [f32; 4], room: [f32; 4], shift: f32, fade: f32) {
        let [left, top, far, low] = piece;
        self.quads.push(Quad {
            rect: [left, top, (far - left).max(1.0), (low - top).max(1.0)],
            uv: room,
            colour: [0.0, 0.0, 0.0, fade],
            flags: SLID,
            radius: shift,
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

    /// The line under every cell a view underlined.
    ///
    /// A pass of its own rather than something `letters` does, because an
    /// underline is about the cell and not about the glyph: a span a
    /// server complained about runs over the spaces in it too, and
    /// `letters` skips a cell with nothing in it. A terminal draws these
    /// itself, off the modifier; a window has to be told, which is the
    /// same seam a full-width character crosses.
    fn underlines(&mut self, page: &Page, cell: CellSize) {
        for row in 0..page.rows() {
            for column in 0..page.columns() {
                let look = page.look(column, row);
                let Some(ink) = underline_ink(&look) else {
                    continue;
                };
                self.underline(
                    f32::from(column) * cell.width,
                    f32::from(row) * cell.height,
                    f32::from(look.columns()) * cell.width,
                    cell.height,
                    ink,
                );
            }
        }
    }

    /// One line under one run of cells.
    ///
    /// Two callers and one line: what a server says is wrong with a word,
    /// and what an input method is in the middle of spelling. They are the
    /// same mark, and a window that drew them two thicknesses would be
    /// saying they were two different things.
    fn underline(&mut self, left: f32, top: f32, width: f32, height: f32, ink: [f32; 4]) {
        let thick = (height * UNDERLINE).round().max(1.0);
        self.block(left, top + height - thick, width, thick, ink);
    }

    /// The lines between two things, drawn where `─` would be.
    ///
    /// On the cells' own ground, which `backgrounds` has painted, and in
    /// their own ink, which the view wrote -- the glyph is what is left
    /// out, by `letters`. Cell by cell, because a rule some later view
    /// covered part of is a line only where it was not covered.
    fn rules(&mut self, page: &Page, ruled: &[Ruled], cell: CellSize) {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a window is thousands of pixels, not millions"
        )]
        let right = self.configured.width as f32;
        let line = thickness(cell.height);
        for rule in ruled {
            let y = rule.area.y;
            let top = middle(f32::from(y) * cell.height, cell.height, line);
            for x in rule.area.left()..rule.area.right() {
                let Some((from, to)) = rule.spans(page, x, y) else {
                    continue;
                };
                let left = f32::from(x) * cell.width;
                // The strip past the last whole cell, which a line that
                // reaches the window's edge has to reach as well -- see
                // `backgrounds`.
                let end = match x + 1 == page.columns() && to >= 1.0 {
                    true => right,
                    false => cell.width.mul_add(to, left),
                };
                let start = cell.width.mul_add(from, left);
                self.block(
                    start,
                    top,
                    (end - start).max(1.0),
                    line,
                    rgba(page.look(x, y).foreground, Ink::Foreground),
                );
            }
        }
    }

    /// The lines between one thing and the next where there is no row for
    /// one.
    ///
    /// Drawn at the top edge of the row that begins the new thing, which
    /// is the boundary itself: a grid has no space there and a window has
    /// a pixel. What a terminal does about the same fact is nothing --
    /// see `obelus_ui::shapes::parted`, which is the one thing said on
    /// that channel with no answer of its own in the cells.
    ///
    /// Only where nothing has been put over the row. A `Parted` is said by
    /// a whole-screen view, and what covers one is a pane or a box with a
    /// frame round it -- a line drawn on either would be the page
    /// underneath reaching through.
    fn partings(&mut self, page: &Page, parted: &[Parted], over: &[&Behind], cell: CellSize) {
        // Half a rule's, which on a screen drawn at twice its own pixels
        // is one of the reader's. `thickness` is a rule's own, and a rule
        // *is* the row it is on; a single device pixel is half a pixel of
        // theirs, which on such a screen is a line nobody sees.
        let line = (thickness(cell.height) / 2.0).round().max(1.0);
        for parting in parted {
            if !parting.still_said(over) {
                continue;
            }
            let left = f32::from(parting.area.x) * cell.width;
            let width = f32::from(parting.area.width) * cell.width;
            // Through the middle of the row, which is the blank between
            // two notes: on its top edge the line would hug whatever is
            // above it, and a boundary that belongs to one side of itself
            // is read as that side's underline.
            let top = f32::from(parting.area.y) * cell.height;
            let middle = (top + (cell.height - line) / 2.0).round();
            // The row's own two colours rather than the page's. They are
            // the page's here, because the row is blank and is nowhere
            // the reader can stand -- but a seam is drawn against what it
            // is drawn on, and asking the cell is how that stays true of
            // wherever this is said next.
            let look = page.look(parting.area.x, parting.area.y);
            self.block(
                left,
                middle,
                width.max(1.0),
                line,
                mixed(
                    rgba(look.background, Ink::Background),
                    rgba(look.foreground, Ink::Foreground),
                    SEAM,
                ),
            );
        }
    }

    /// A box's frame, drawn as the shape a terminal spells in `╭─╮`: the
    /// line where the glyphs put it, down the middle of the ring of cells,
    /// and outside it what the box was put over, where a terminal has its
    /// square corners.
    ///
    /// What was put over is the ground of the cells under the ring, and
    /// not their letters: a letter a line cuts in half is not something
    /// anybody put there. Nor that ground where it is glass, which is
    /// already under it -- an opaque strip of it would be the one square
    /// edge left on a round box.
    ///
    /// What goes inside the line is the caller's, the box's own ground or
    /// glass, so what this answers is where that is: the rectangle inside
    /// the line, and how round it is.
    fn frame(
        &mut self,
        page: &Page,
        card: &Behind,
        panes: &[&Behind],
        cell: CellSize,
    ) -> ([f32; 4], f32) {
        let area = card.area;
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                let Some(under) = card.look(x, y) else {
                    continue;
                };
                if card.ring_holds(page, x, y) && !seen_through(panes, x, y, under.background) {
                    let ground = self.under(panes, &[], x, y);
                    self.block(
                        f32::from(x) * cell.width,
                        f32::from(y) * cell.height,
                        cell.width,
                        cell.height,
                        as_held(under.background, self.holding, ground)
                            .unwrap_or_else(|| rgba(under.background, Ink::Background)),
                    );
                }
            }
        }
        let line = thickness(cell.height);
        let [left, top, right, bottom] = outline(area, cell, line);
        let radius = cell.width * FRAME_CORNER;
        self.rounded(
            left,
            top,
            right - left,
            bottom - top,
            radius,
            rgba(page.look(area.x, area.y).foreground, Ink::Foreground),
        );
        (
            [
                left + line,
                top + line,
                (right - left - line * 2.0).max(1.0),
                (bottom - top - line * 2.0).max(1.0),
            ],
            (radius - line).max(0.0),
        )
    }

    /// The switches, drawn as the box a glyph was standing in for.
    ///
    /// After the letters, because what it goes over is that glyph: a
    /// terminal has one cell and one character to say this in, and what
    /// it says there is the whole of the answer -- this is the same
    /// answer drawn rather than spelled.
    ///
    /// Set is a filled box with the mark cut out of it; not is the same
    /// box with its middle taken back out, which leaves an outline. One
    /// shape either way, because a pair that changed shape would put a
    /// jog in a column read straight down -- which is what `tick` says
    /// about the two glyphs, for the same reason.
    /// The bars, as capsules rather than as the blocks a terminal has.
    ///
    /// Nothing is covered over. `letters` is told to leave a bar's cells
    /// alone, the way it leaves a rule's and a cap's, so the blocks a
    /// terminal draws are never put on the screen here at all -- and then
    /// what is behind the capsule is whatever the page already had there.
    ///
    /// Which is the whole of why it is done that way round. This did once
    /// draw the blocks and paint over them with the cell's own background,
    /// and a bar inside a pane sits on *glass*: the cells there carry the
    /// pane's own colour, which `backgrounds` deliberately does not paint
    /// because painting it is covering up what the reader is meant to see
    /// through. The cover put it back, opaque, in one strip down the side
    /// of every list. `catching_up` has the same note for the same reason,
    /// one bug earlier.
    ///
    /// The colours are the cells' own, which is the rule a switch follows
    /// and for the same reason: what a terminal draws the blocks in is
    /// what a window draws the capsules in, and asking the view for them
    /// again would be the same two colours from two places.
    fn bars(&mut self, page: &Page, barred: &[Barred], cell: CellSize) {
        for showing in barred {
            let bar = showing.bar;
            if bar.area.width == 0 || bar.area.height == 0 {
                continue;
            }
            // A pointer on it beats the settling: the reader is reaching
            // for the thing, and a control that went on fading under the
            // hand reaching for it is the one moment it must not.
            // `shown` already carries the pointer's own brightening, which
            // is the louder of the two: a bar under the pointer stays up
            // however long ago it moved.
            let shown = showing.shown.clamp(0.0, 1.0);
            let under = showing.under.clamp(0.0, 1.0);
            let column = bar.area.x;
            let capsule = |width: f32| {
                let width = (cell.width * width).round().max(2.0);
                (
                    f32::from(column) * cell.width + (cell.width - width) / 2.0,
                    width,
                )
            };

            // Only the rows that are still the bar's. A card goes over
            // the page before this pass, so a panel as wide as the editor
            // has its own right-hand edge in this very column -- and the
            // capsule was drawn over it. The same question a rule asks of
            // each of its cells.
            let runs = showing.runs(page);
            if runs.is_empty() {
                continue;
            }

            // A row the mark does not cover, which is where the track's
            // colour is. There may be none -- a mark as long as its track
            // -- and then there is no track to draw either: every pixel of
            // it would be under the mark.
            if let Some(row) = (0..bar.area.height)
                .find(|row| *row < bar.mark || *row >= bar.mark.saturating_add(bar.thumb))
            {
                let look = page.look(column, bar.area.y + row);
                // The track goes out altogether, which the mark may not:
                // what it says is how far the mark can travel, and the
                // column it is in says that much on its own.
                let (left, width) = capsule(BAR_TRACK);
                let ink = mixed(
                    rgba(look.background, Ink::Background),
                    rgba(look.foreground, Ink::Foreground),
                    shown,
                );
                for (top, rows) in &runs {
                    self.rounded(
                        left,
                        f32::from(*top) * cell.height,
                        width,
                        f32::from(*rows) * cell.height,
                        width / 2.0,
                        ink,
                    );
                }
            }

            // Two widths in one: how far up the scrolling has brought it,
            // and then how far the pointer has taken it past that. The
            // second is laid over the first rather than chosen instead of
            // it, so a bar the pointer arrives on while it is still moving
            // widens from where it is and not from where it would have
            // been standing still.
            let moved = BAR_MARK_RESTING + (BAR_MARK - BAR_MARK_RESTING) * shown;
            let wide = moved + (BAR_MARK_UNDER - moved) * under;
            let (left, width) = capsule(wide);
            let top = bar.area.y.saturating_add(bar.mark);
            let look = page.look(column, top);
            let ink = mixed(
                rgba(look.background, Ink::Background),
                rgba(look.foreground, Ink::Foreground),
                BAR_RESTING + (1.0 - BAR_RESTING) * shown,
            );
            // And the mark only where the column is still the bar's, the
            // same as the track: half a mark is where the reader is, and a
            // mark drawn across whatever covered it is not.
            let wanted = top..top.saturating_add(bar.thumb);
            for (run, rows) in &runs {
                let from = (*run).max(wanted.start);
                let to = run.saturating_add(*rows).min(wanted.end);
                if from >= to {
                    continue;
                }
                self.rounded(
                    left,
                    f32::from(from) * cell.height,
                    width,
                    f32::from(to - from) * cell.height,
                    width / 2.0,
                    ink,
                );
            }
        }
    }

    /// The marked runs, as strokes rather than as the half blocks a
    /// terminal has.
    ///
    /// Nothing is covered over, the same as a bar: `letters` is told to
    /// leave these cells alone, so the blocks a terminal draws are never
    /// put on the screen here at all. Which matters for the same reason it
    /// matters there -- a margin inside a pane sits on glass, and a cover
    /// painted in the cell's own ground would be a hole in it.
    ///
    /// The colour is the cell's own, which is the rule a bar and a switch
    /// both follow: what a terminal draws the block in is what a window
    /// draws the stroke in, and asking the view again would be the same
    /// colour from two places.
    fn strokes(&mut self, page: &Page, stroked: &[Stroked], cell: CellSize) {
        for mark in stroked {
            let column = mark.stroke.area.x;
            // Against the edge nearest the text, which is the side the
            // view said -- see `obelus_ui::shapes::Side`.
            let width = (cell.width * STROKE).round().max(2.0);
            let left = match mark.stroke.side {
                Side::Left => f32::from(column) * cell.width,
                Side::Right => f32::from(column + 1) * cell.width - width,
            };
            // Only the rows still this stroke's, and in the runs they are
            // left in: a hunk whose middle rows were drawn over is two
            // strokes, and one bar spanning the hole would be a mark on
            // somebody else's cells.
            for (top, rows) in mark.runs(page) {
                let ink = rgba(page.look(column, top).foreground, Ink::Foreground);
                match mark.stroke.about {
                    // A bar down the rows, with its ends rounded: a hunk of
                    // six lines is one stroke and not six beads, which is
                    // the whole reason the view says it in runs.
                    About::Rows => self.rounded(
                        left,
                        f32::from(top) * cell.height,
                        width,
                        f32::from(rows) * cell.height,
                        width / 2.0,
                        ink,
                    ),
                    // An arrow on the boundary above the row, pointing the
                    // way the stroke leans -- which is at the text, and so
                    // at the place the missing lines were. The rows are
                    // there and the lines between them are not, so the one
                    // thing this must not be is a mark *on* a row.
                    About::Seam => {
                        let reach = (cell.width * SEAM_REACH).round().max(2.0);
                        let height = (reach * SEAM_BASE).round().max(2.0);
                        let (at, way) = match mark.stroke.side {
                            Side::Left => (f32::from(column) * cell.width, -1.0),
                            Side::Right => (f32::from(column + 1) * cell.width - reach, 1.0),
                        };
                        self.wedge(
                            at,
                            f32::from(top).mul_add(cell.height, -(height / 2.0)),
                            reach,
                            height,
                            way,
                            ink,
                        );
                    }
                }
            }
        }
    }

    /// A triangle filling a box, its point in the middle of one short side.
    ///
    /// `way` is which side: positive for the right-hand one, negative for
    /// the left.
    fn wedge(&mut self, left: f32, top: f32, width: f32, height: f32, way: f32, colour: [f32; 4]) {
        self.quads.push(Quad {
            rect: [left, top, width, height],
            uv: self.atlas.white,
            colour,
            flags: SOLID | WEDGE,
            radius: way,
            layer: 0,
            lower: 0.0,
        });
    }

    fn ticks(
        &mut self,
        page: &Page,
        ticked: &[Ticked],
        panes: &[&Behind],
        framed: &[&Behind],
        capped: &[&Capped],
        cell: CellSize,
    ) {
        for tick in ticked {
            let left = f32::from(tick.area.x) * cell.width;
            let top = f32::from(tick.area.y) * cell.height;
            // The cell's own ink and ground, which the view wrote there:
            // what a terminal draws the glyph in is what a window draws
            // the box in. Except on a hold, where the ground on the
            // screen is the plate's face rather than the colour the cell
            // wears -- see `held_face`.
            let look = page.look(tick.area.x, tick.area.y);
            let ground = self
                .held_face(page, tick.area, panes, framed, capped)
                .unwrap_or_else(|| rgba(look.background, Ink::Background));
            let side = (cell.width * BOX).round().max(3.0);
            // A shade above the middle of the cell, which is where the
            // middle of the writing is: letters sit on a baseline with
            // their descenders below it, so a box centred on the cell
            // sits low against the words beside it.
            let (at, over) = (
                left + (cell.width - side) / 2.0,
                (top + (cell.height - side) / 2.0 - cell.height * ABOVE).round(),
            );
            let radius = side * BOX_CORNER;
            self.rounded(
                at,
                over,
                side,
                side,
                radius,
                rgba(look.foreground, Ink::Foreground),
            );
            match tick.on {
                true => self.quads.push(Quad {
                    rect: [at, over, side, side],
                    uv: self.atlas.white,
                    colour: ground,
                    flags: CHECKED,
                    radius: 0.0,
                    layer: 0,
                    lower: 0.0,
                }),
                false => {
                    let edge = (side * BOX_EDGE).round().max(1.0);
                    self.rounded(
                        at + edge,
                        over + edge,
                        side - edge * 2.0,
                        side - edge * 2.0,
                        (radius - edge).max(0.0),
                        ground,
                    );
                }
            }
        }
    }

    /// The caps the keys at the foot of a view are drawn in.
    ///
    /// A terminal's cap is the run of cells behind the key, a shade off
    /// the page, and that is what has already been painted here by
    /// `backgrounds`. What a window can say that a terminal cannot is the
    /// *shape*: so the ground is put back over those cells and the cap is
    /// drawn on it -- which is why this runs after the letters rather
    /// than before them. The key is the one thing on the screen not
    /// written at the size the grid is counted in (see `font::Size`), so
    /// the cells' own glyphs are covered by the cap's ground and the key
    /// is written again on the face, centred in it. Drawn from `keys`
    /// rather than cell by cell, because a word tracked out to the cell
    /// pitch at three quarters the size reads as spaced-out capitals.
    ///
    /// Three rectangles, and the third is what makes it a key rather than
    /// a rounded box: the face is drawn a pixel inside the outline on
    /// three sides and further in at the bottom, so what is left showing
    /// under it is a lip. Which is the whole of the trick a keyboard's own
    /// keys use.
    fn caps(&mut self, page: &Page, capped: &[&Capped], panes: &[&Behind], fonts: &mut Fonts) {
        for cap in capped {
            // What the cells said, put back: the corners this is about to
            // round away are painted in the cap's own ground, and a cap
            // drawn over them would have square shoulders. Unless what is
            // behind the cap is glass, which is already there and is the
            // right thing to show round a corner.
            let put_back = !seen_through(panes, cap.area.x, cap.area.y, cap.page);
            self.cap(cap, |x, y| Some(page.look(x, y)), put_back, fonts);
        }
    }

    /// One cap, out of whichever cells it was said about: the page's, or
    /// the picture of what a pane was put over.
    fn cap<'a>(
        &mut self,
        cap: &Capped,
        look: impl Fn(u16, u16) -> Option<Look<'a>>,
        put_back: bool,
        fonts: &mut Fonts,
    ) {
        let cell = fonts.cell();
        let left = f32::from(cap.area.x) * cell.width;
        let top = f32::from(cap.area.y) * cell.height;
        let width = f32::from(cap.area.width) * cell.width;
        let side = (cell.width * SIDE).round();
        // Never thinner than a pixel: a lip that rounds away is a cap
        // that lies flat, and the inset is what keeps a cap off the
        // rows either side of it.
        let inset = (cell.height * INSET).round().max(1.0);
        let lip = (cell.height * LIP).round().max(1.0);
        let height = (cell.height - inset * 2.0).max(1.0);
        let radius = (height * ROUNDING).min(cell.width);
        if put_back {
            self.block(
                left,
                top,
                width,
                cell.height,
                rgba(cap.page, Ink::Background),
            );
        }
        // Held off the cells either side, which the ground above is
        // not: what was put back is every cell the cap was said
        // about, and what is drawn on it stops short of them.
        let drawn = (width - side * 2.0).max(1.0);
        self.rounded(
            left + side,
            top + inset,
            drawn,
            height,
            radius,
            rgba(cap.edge, Ink::Foreground),
        );
        let face = (height - 1.0 - lip).max(1.0);
        self.rounded(
            left + side + 1.0,
            top + inset + 1.0,
            (drawn - 2.0).max(1.0),
            face,
            (radius - 1.0).max(0.0),
            rgba(cap.cap, Ink::Background),
        );
        self.legend(
            cap,
            look,
            left + width / 2.0,
            top + inset + 1.0 + face / 2.0,
            fonts,
        );
    }

    /// The key itself, written on the face of its cap.
    ///
    /// Middled on the face rather than on the cells: the face is held off
    /// the bottom of the cap by the lip, so a key centred in the row
    /// would sit low in the thing it is in by exactly that much.
    ///
    /// Cell by cell, as everything else that draws text here is, and at a
    /// pitch of its own so that the letters close up rather than keeping
    /// the room the grid gave them. Shaping the key in one run instead
    /// would track it properly and get the *face* wrong: what decides
    /// which family is asked for is whether the text is one of Obelus's
    /// marks, and `Ctrl` is drawn as a mark with a letter after it -- so
    /// one run would send the letter to the symbols font as well.
    ///
    /// The ink comes from the cells, which is where the theme said it --
    /// the cap knows the three colours it is drawn in and not the one the
    /// key is written in.
    fn legend<'a>(
        &mut self,
        cap: &Capped,
        look: impl Fn(u16, u16) -> Option<Look<'a>>,
        middle: f32,
        height: f32,
        fonts: &mut Fonts,
    ) {
        let cell = fonts.cell();
        let pitch = cell.width * font::SMALLER;
        let columns = obelus_text::text_width(&cap.keys).max(1);
        #[expect(
            clippy::cast_precision_loss,
            reason = "a key is a few columns wide, never billions"
        )]
        let left = middle - columns as f32 * pitch / 2.0;
        let top = height - cell.height * font::SMALLER / 2.0;
        let mut column = 0;
        for at in 1..cap.area.width.saturating_sub(1) {
            if column >= columns {
                break;
            }
            let Some(look) = look(cap.area.x.saturating_add(at), cap.area.y) else {
                continue;
            };
            // The right half of a wide character, which its neighbour
            // blanked -- see `Page::covered`.
            if look.text.is_empty() {
                continue;
            }
            if !look.text.trim().is_empty() {
                let ink = rgba(look.foreground, Ink::Foreground);
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "a key is a few columns wide, never billions"
                )]
                let along = left + column as f32 * pitch;
                self.glyphs_at((along, top), look, ink, 0, fonts, Size::Capped);
            }
            column += obelus_text::text_width(look.text).max(1);
        }
    }

    /// What is under a pane, drawn exactly as the page would draw it.
    ///
    /// The first pane's goes in front of everything else in the buffer,
    /// because it is drawn twice: once into a texture of its own, which is
    /// what the glass reads, and once on the screen, where it is what shows
    /// through the pane's rounded corners -- the one place the pane is not.
    fn pane_under(
        &mut self,
        behind: &Behind,
        barred: &[Barred],
        capped: &[Capped],
        fonts: &mut Fonts,
    ) {
        let cell = fonts.cell();
        for y in behind.area.top()..behind.area.bottom() {
            let ground = self.ground_colour();
            let (grounds, lettered) = behind_row(behind, barred, y);
            for (start, end, colour) in grounds {
                // What a pane was put over is the page, so a hold in it
                // is a plate on the page's own ground.
                self.block(
                    f32::from(start) * cell.width,
                    f32::from(y) * cell.height,
                    f32::from(end - start) * cell.width,
                    cell.height,
                    as_held(colour, self.holding, ground)
                        .unwrap_or_else(|| rgba(colour, Ink::Background)),
                );
            }
            for x in lettered {
                let Some(under) = behind.look(x, y) else {
                    continue;
                };
                let ink = rgba(under.foreground, Ink::Foreground);
                self.glyphs_at(
                    (f32::from(x) * cell.width, f32::from(y) * cell.height),
                    under,
                    ink,
                    0,
                    fonts,
                    Size::Cell,
                );
            }
        }
        // And the caps the picture still holds, which the page no longer
        // does: a cap is the shape its cells are, and these cells are seen
        // through the glass -- see `Capped::still_behind`. Put back on
        // their own ground always, since nothing is behind the picture.
        for cap in capped.iter().filter(|cap| cap.still_behind(behind)) {
            self.cap(cap, |x, y| behind.look(x, y), true, fonts);
        }
    }

    /// The glass over a pane: one quad over what is under it. What it does
    /// with what is behind is in `paint.wgsl`: the shape is the same
    /// rounded box a key's cap is, and it is the same function that says
    /// where its edge is.
    ///
    /// Hands back which quad the glass is, which is the one drawn with the
    /// pane's own pictures, and where it is.
    fn pane_glass(
        &mut self,
        page: &Page,
        behind: &Behind,
        lower: &[&Behind],
        ruled: &[Ruled],
        cell: CellSize,
    ) -> (usize, [f32; 4]) {
        // Over another pane's glass where it is over one at all: a list
        // opened over the settings is, a list opened over the code never.
        let on_glass = lower.iter().any(|lower| lower.area.intersects(behind.area));
        let mut tint = rgba(behind.ground, Ink::Background);
        tint[3] = match on_glass {
            true => ON_GLASS_TINT,
            false => TINT,
        };
        let (left, mut top) = (
            f32::from(behind.area.x) * cell.width,
            f32::from(behind.area.y) * cell.height,
        );
        let far = f32::from(behind.area.right()) * cell.width;
        let mut low = f32::from(behind.area.bottom()) * cell.height;
        // A rule along the pane's first or last row is its edge, and the
        // glass starts or stops where the rule's line is, which is the
        // middle of that row. A glass edge on the row's boundary was half a
        // row from the line that said where the pane was.
        //
        // Above the line over the pane is what the rule was drawn over,
        // which is in `behind` and has been drawn already, first of
        // everything. A terminal has to give a rule the whole of its row,
        // because a glyph takes a cell; a line drawn here takes a pixel.
        // Painted in the rule's own ground, that half row was a blank band
        // across whatever the list was opened over -- the welcome screen,
        // a line of the file. Under the line below it is the rule's own
        // row and nothing else, since it is the status row's rule and
        // nothing is under it, so that half is painted: it covers the
        // rule's glyph, which is what `behind` holds there.
        //
        // The line above is inside the glass and the line below is not:
        // the first is the list's own edge and arrives with it, and the
        // second is the status row's, which the list stands on.
        let line = thickness(cell.height);
        let edge = |y: u16| {
            ruled.iter().find(|rule| {
                rule.area.y == y
                    && rule.area.x == behind.area.x
                    && rule.area.width == behind.area.width
                    && rule.still_said(page)
            })
        };
        if edge(behind.area.y).is_some() {
            top = middle(top, cell.height, line);
        }
        let mut under = None;
        if behind.area.height > 1
            && let Some(rule) = edge(behind.area.bottom() - 1)
        {
            let row = f32::from(rule.area.y) * cell.height;
            let at = middle(row, cell.height, line);
            under = Some((
                [at, low - at],
                page.look(rule.area.x, rule.area.y).background,
            ));
            low = at;
        }
        let glass = self.quads.len();
        self.quads.push(Quad {
            rect: [left, top, (far - left).max(1.0), (low - top).max(1.0)],
            uv: [left, top, far, low],
            colour: tint,
            flags: SOLID
                | GLASS
                | match on_glass {
                    true => ON_GLASS,
                    false => 0,
                }
                | match behind.joined {
                    Joined::Above => HANGING,
                    Joined::Below => STANDING,
                    // Nothing under the screen's own bottom edge.
                    Joined::Screen => 0,
                    // Never here: a box's glass is `box_glass`'s.
                    Joined::Nowhere => 0,
                },
            // Unread: a pane has no corners to round -- see `outside` in
            // `paint.wgsl`.
            radius: 0.0,
            layer: 0,
            lower: 0.0,
        });
        if let Some(([over, tall], ground)) = under {
            self.block(
                left,
                over,
                (far - left).max(1.0),
                tall,
                rgba(ground, Ink::Background),
            );
        }
        (glass, [left, top, far, low])
    }

    /// The soft edge outside a pane and outside each box with a frame
    /// round it.
    ///
    /// Not the pane's while it is still arriving, which is drawn where
    /// the frame is put back together instead. The frame is drawn once
    /// and put back in two pieces, the pane's own slid up from where it
    /// set out -- and a shadow is the one part of a pane that falls
    /// *outside* the pane's own room, so it is in the other piece: drawn
    /// here it would stand still at the edge the pane is going to have
    /// while there is nothing under it yet. It travels with the pane
    /// instead, from the edge the pane has reached -- see `Painter::frame`.
    ///
    /// Which is the one thing on this screen a terminal has no answer to
    /// at all. A pane's edge, it draws with a rule; a box's, with a
    /// frame of `╭─╮`. Both say *where* the thing stops and neither says
    /// it is over anything, because a cell is a cell and there is
    /// nowhere for a shadow to go. So this is added rather than drawn
    /// better -- `shapes`'s own test says a thing said there has to be
    /// one a terminal already answers -- and it is added in the front
    /// end, from what the front end already knows: the rectangle it drew
    /// the glass in, and which edge the thing is joined to.
    fn shadows(&mut self, stack: &[&Behind], sliding: Option<usize>, boxes: f32, cell: CellSize) {
        let spread = cell.height * SHADOW_SPREAD;
        for (at, over) in stack.iter().enumerate() {
            // A box has four edges and corners, so it casts all round.
            if over.is_a_box() {
                let line = thickness(cell.height);
                self.shadow(
                    outline(over.area, cell, line),
                    cell.width * FRAME_CORNER,
                    0,
                    spread,
                    boxes,
                );
            } else if sliding != Some(at)
                && casts(stack, at)
                && let Some(joined) = casting(over.joined)
            {
                let glass = self.placed.levels[at].rect;
                self.shadow(glass, 0.0, joined, spread, 1.0);
            }
        }
    }

    /// One of them: the rectangle that casts it, how far it reaches, and
    /// how much of it there is -- which is all of it except while the
    /// thing casting it is still arriving.
    fn shadow(&mut self, box_: [f32; 4], radius: f32, joined: u32, spread: f32, fade: f32) {
        let [left, top, far, low] = box_;
        self.quads.push(Quad {
            rect: [
                left - spread,
                top - spread,
                spread.mul_add(2.0, far - left).max(1.0),
                spread.mul_add(2.0, low - top).max(1.0),
            ],
            uv: box_,
            colour: [0.0, 0.0, 0.0, SHADOW_INK * fade],
            flags: SHADOW | joined,
            radius,
            layer: 0,
            lower: 0.0,
        });
    }

    /// What is under anything over the first pane, or under a box with
    /// a frame round it: the picture its glass reads, which goes into its
    /// own backdrop and never onto the screen -- the cells it was put over,
    /// as the screen had them. Where those were glass further down -- the
    /// card of every key is nearly always over a list or a page of
    /// settings, and a setting's choices are over the settings -- they are
    /// the letters alone, because the picture draws that glass before them
    /// and a ground under the letters would cover it.
    fn seen_under(&mut self, over: &Behind, lower: &[&Behind], fonts: &mut Fonts) {
        let cell = fonts.cell();
        for y in over.area.top()..over.area.bottom() {
            for x in over.area.left()..over.area.right() {
                let Some(under) = over.look(x, y) else {
                    continue;
                };
                let left = f32::from(x) * cell.width;
                let top = f32::from(y) * cell.height;
                if !seen_through(lower, x, y, under.background) {
                    let ground = self.under(lower, &[], x, y);
                    self.block(
                        left,
                        top,
                        cell.width,
                        cell.height,
                        as_held(under.background, self.holding, ground)
                            .unwrap_or_else(|| rgba(under.background, Ink::Background)),
                    );
                }
                if !under.text.trim().is_empty() {
                    let ink = rgba(under.foreground, Ink::Foreground);
                    self.glyphs_at((left, top), under, ink, 0, fonts, Size::Cell);
                }
            }
        }
    }

    /// The frame round a box and the glass inside its line. The cells
    /// inside the ring are drawn after all of this by `backgrounds` and
    /// `letters`, which leave the box's own ground to the glass.
    ///
    /// Hands back which quad the glass is and where it is, the way a
    /// pane's does.
    fn box_glass(
        &mut self,
        page: &Page,
        card: &Behind,
        lower: &[&Behind],
        cell: CellSize,
    ) -> (usize, [f32; 4]) {
        let (inside, radius) = self.frame(page, card, lower, cell);
        // Over a pane where it is over one at all: the card of every key
        // always is, a hover over the code never.
        let on_glass = lower.iter().any(|lower| lower.area.intersects(card.area));
        let mut tint = rgba(card.ground, Ink::Background);
        tint[3] = match on_glass {
            true => ON_GLASS_TINT,
            false => TINT,
        };
        let glass = self.quads.len();
        self.quads.push(Quad {
            rect: inside,
            uv: [
                inside[0],
                inside[1],
                inside[0] + inside[2],
                inside[1] + inside[3],
            ],
            colour: tint,
            // Neither hanging nor standing, which is a box with an edge on
            // every side -- see `outside` in `paint.wgsl`.
            flags: SOLID
                | GLASS
                | match on_glass {
                    true => ON_GLASS,
                    false => 0,
                },
            radius,
            layer: 0,
            lower: 0.0,
        });
        let [left, top, wide, tall] = inside;
        (glass, [left, top, left + wide, top + tall])
    }

    /// The two quads that blur what is behind a pane, one way and then
    /// the other, over the pane's own rectangle and no further: the glass
    /// reads nothing outside it, and what is outside it in the picture is
    /// nothing -- a blur that reached out for it would darken the pane's
    /// edges, which is why the shader holds its samples inside `box_`.
    fn blurs_over(&mut self, box_: [f32; 4]) -> usize {
        let [left, top, far, low] = box_;
        let first = self.quads.len();
        for way in [[1.0, 0.0], [0.0, 1.0]] {
            self.quads.push(Quad {
                rect: [left, top, (far - left).max(1.0), (low - top).max(1.0)],
                uv: box_,
                colour: [way[0], way[1], 0.0, 0.0],
                flags: BLUR,
                radius: 0.0,
                layer: 0,
                lower: 0.0,
            });
        }
        first
    }

    /// What is behind a pane, blurred: across into the scratch picture,
    /// then down out of it into the pane's own.
    fn blurring(&self, encoder: &mut wgpu::CommandEncoder, first: usize, seen: &Seen) {
        for (target, bindings, quad) in [
            (&self.scratch, &seen.bindings, first),
            (&seen.blurred, &self.scratch_bindings, first + 1),
        ] {
            let mut pass = self.pass(encoder, "obelus blur", target);
            pass.set_bind_group(0, bindings, &[]);
            drawing(&mut pass, quad..quad + 1);
        }
    }

    /// A pass that draws into `target`, cleared, with everything set but
    /// what it reads.
    fn pass<'e>(
        &self,
        encoder: &'e mut wgpu::CommandEncoder,
        label: &str,
        target: &wgpu::TextureView,
    ) -> wgpu::RenderPass<'e> {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_vertex_buffer(0, self.instances.slice(..));
        pass
    }

    /// The quads the screen draws, each with whatever it reads -- see
    /// `on_the_screen`.
    fn the_frame(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.follow(pass, &on_the_screen(&self.placed), &self.levels[0].bindings);
    }

    /// What one level was put over -- see `put_over`.
    fn beneath(&self, pass: &mut wgpu::RenderPass<'_>, level: usize) {
        self.follow(
            pass,
            &put_over(&self.placed.levels, level),
            &self.plain_bindings,
        );
    }

    /// Draws a plan: each run of quads with the pictures it reads.
    fn follow(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        plan: &[(Range<usize>, Reads)],
        otherwise: &wgpu::BindGroup,
    ) {
        for (quads, reads) in plan {
            let bindings = match reads {
                Reads::Nothing => otherwise,
                Reads::Level(level) => &self.levels[*level].bindings,
            };
            pass.set_bind_group(0, bindings, &[]);
            drawing(pass, quads.clone());
        }
    }

    /// As many pictures as there are levels to read them.
    fn levels_for(&mut self, wanted: usize) {
        while self.levels.len() < wanted {
            let (width, height) = (self.configured.width, self.configured.height);
            let seen = Seen::made(&self.binder(), self.view, width, height);
            self.levels.push(seen);
        }
    }

    /// The text.
    ///
    /// Less what is drawn rather than spelled: a rule's line and a
    /// frame's are `rules` and `frame`, and the glyph a terminal draws
    /// them in would be a second line half a pixel from the first.
    ///
    /// And less what is written again: a cap's key, which `caps` writes
    /// smaller on its face, and the glyph a switch stands in for, which
    /// `ticks` draws as a box. Both used to be covered by a square of the
    /// cells' ground, which on glass is a square nobody wants -- so they
    /// are left out instead, whatever is behind them.
    fn letters(
        &mut self,
        page: &Page,
        fonts: &mut Fonts,
        said: Said<'_>,
        framed: &[&Behind],
        capped: &[&Capped],
    ) {
        for row in 0..page.rows() {
            for column in 0..page.columns() {
                let look = page.look(column, row);
                if look.text.trim().is_empty() {
                    continue;
                }
                if drawn_as_a_shape(page, &said, framed, capped, column, row) {
                    continue;
                }
                // The mark's own cells rest at one colour and are carried
                // to the other by the light, which is the shader's -- see
                // `SHEENED`. What the cell holds is the terminal's answer
                // to the same question, eight bands of it, and reading
                // that as a base would be the light travelling over a ramp
                // that is already travelling.
                let lit = said.sheened.is_some_and(|mark| mark.holds(column, row));
                let ink = match said.sheened.filter(|_| lit) {
                    Some(mark) => mark.from,
                    None => look.foreground,
                };
                let colour = rgba(ink, Ink::Foreground);
                self.glyphs(column, row, look, colour, u32::from(lit) * SHEENED, fonts);
            }
        }
    }

    /// One cell's glyphs, put where the grid says rather than where their
    /// own advances would have reached.
    fn glyphs(
        &mut self,
        column: u16,
        row: u16,
        look: Look<'_>,
        colour: [f32; 4],
        lit: u32,
        fonts: &mut Fonts,
    ) {
        let cell = fonts.cell();
        self.glyphs_at(
            (f32::from(column) * cell.width, f32::from(row) * cell.height),
            look,
            colour,
            lit,
            fonts,
            Size::Cell,
        );
    }

    /// The same, at a place in pixels rather than at a cell.
    ///
    /// Which the caret needs because a caret on its way is not on a cell
    /// boundary, and what a block carries has to be in the same place the
    /// block is.
    fn glyphs_at(
        &mut self,
        at: (f32, f32),
        look: Look<'_>,
        colour: [f32; 4],
        lit: u32,
        fonts: &mut Fonts,
        size: Size,
    ) {
        let (left, top) = at;
        let cell = fonts.cell();
        // A block is the cell, or a part of it, and is drawn as that rather
        // than asked of the face -- see `pieces`.
        if size == Size::Cell
            && let Some(pieces) = pieces(look.text)
        {
            for piece in pieces {
                let [x, y, wide, tall] = snapped((left, top), cell, *piece);
                self.quads.push(Quad {
                    rect: [x, y, wide, tall],
                    uv: self.atlas.white,
                    colour,
                    // Not `SOLID`, which would leave the light out: the
                    // welcome screen's mark is made of these, and it is
                    // carried by the same flag a letter is.
                    flags: lit,
                    radius: 0.0,
                    layer: 0,
                    lower: 0.0,
                });
            }
            return;
        }
        let bold = look.modifier.contains(Modifier::BOLD);
        let italic = look.modifier.contains(Modifier::ITALIC);
        let placed = fonts.glyphs(look.text, bold, italic, size).to_vec();
        for glyph in placed {
            let Some(spot) = self.atlas.spot(fonts, glyph.key) else {
                continue;
            };
            #[expect(
                clippy::cast_precision_loss,
                reason = "a glyph is offset by pixels, and there are few of them"
            )]
            let (x, y) = (glyph.x as f32, glyph.y as f32);
            // The grid's baseline for writing, and a mark's own for a mark
            // -- see `font::Placed::baseline`.
            let baseline = glyph.baseline.unwrap_or(cell.baseline);
            let Some((rect, uv)) = clipped(
                [
                    left + x + spot.left,
                    top + baseline + y - spot.top,
                    spot.width,
                    spot.height,
                ],
                spot.uv,
                self.grid,
            ) else {
                continue;
            };
            self.quads.push(Quad {
                rect,
                uv,
                colour,
                flags: match spot.colourful {
                    true => COLOURFUL,
                    false => lit,
                },
                radius: 0.0,
                layer: spot.layer,
                lower: 0.0,
            });
        }
    }

    /// The marks on this frame, each drawn into the two cells it was given.
    ///
    /// Rasterised the first time it is asked for at this size and kept in
    /// the same texture the glyphs are in: a mark is a picture of about
    /// sixteen pixels square, which is a large glyph and nothing more.
    /// Made here, at the moment of drawing, rather than by the view,
    /// because how many pixels a mark is depends on how big a cell is --
    /// which the reader changes.
    fn marks(&mut self, marked: &[Marked], cell: crate::font::CellSize) {
        for mark in marked {
            let key = (mark.id.clone(), mark.focused);
            let spot = match self.atlas.mark(&key) {
                Some(spot) => spot,
                None => {
                    let (Some(svg), Some(palette)) = (self.drawings.get(&key), self.palette) else {
                        // Asked for before it was carried, which the view
                        // does not do -- but a frame is not the place to
                        // find out.
                        continue;
                    };
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "a mark is a few dozen pixels each way"
                    )]
                    let pixels = (
                        (cell.width * f32::from(SLOT.width)).round() as u32,
                        (cell.height * f32::from(SLOT.height)).round() as u32,
                    );
                    let paper = match mark.focused {
                        true => palette.selected,
                        false => palette.paper,
                    };
                    let Some(drawn) = obelus_ui::image::raster(svg, pixels, palette.ink, paper)
                    else {
                        // A drawing this machine's renderer would not read.
                        // Remembered as nothing, so it is not tried again
                        // on every frame -- and said, because a card
                        // wearing a glyph where the others wear pictures
                        // has no other explanation.
                        tracing::warn!(id = key.0, "a mark that would not draw");
                        self.atlas.no_mark(key);
                        continue;
                    };
                    let rgba = drawn.to_rgba8();
                    let (width, height) = (rgba.width(), rgba.height());
                    let spot = self.atlas.place(width, height, &rgba, true);
                    self.atlas.marked(key, spot);
                    match spot {
                        Some(spot) => spot,
                        None => continue,
                    }
                }
            };
            self.quads.push(Quad {
                rect: [
                    f32::from(mark.x) * cell.width,
                    f32::from(mark.y) * cell.height,
                    cell.width * f32::from(SLOT.width),
                    cell.height * f32::from(SLOT.height),
                ],
                uv: spot.uv,
                // The picture carries its own colours, and the instance's
                // alpha is the whole of what it adds.
                colour: [1.0, 1.0, 1.0, 1.0],
                flags: COLOURFUL,
                radius: 0.0,
                layer: spot.layer,
                lower: 0.0,
            });
        }
    }

    /// The caret, in the shape that says what the next character will do.
    ///
    /// A bar stands between two characters and says the next one goes
    /// there; a block stands on one and says the next one takes its place.
    /// Which is the mode, and the application is what knows it.
    ///
    /// Only a window can draw it, because a terminal's caret is the
    /// terminal's and Obelus does not own its shape. Which is why the
    /// status row says `Replacing` in a word as well: that half works in
    /// both, and a mode with no sign is a mode the reader is in without
    /// knowing.
    ///
    /// The block is the cell with its colours the other way round, which is
    /// what a terminal does and for the same reason: a block that hid the
    /// character under it would be a caret a reader cannot read past.
    fn caret(
        &mut self,
        page: &Page,
        fonts: &mut Fonts,
        spelling: Option<&Spelling>,
        drift: (f32, f32),
    ) {
        let Some(caret) = page.caret() else {
            return;
        };
        let cell = fonts.cell();
        // Inside the word being spelled, where the input method says it is:
        // a caret left at the start of it would be a caret in the wrong
        // half of what the reader is typing.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a caret is a few columns into what is being spelled"
        )]
        let along = spelling.map_or(0, |spelling| spelling.columns() as u16);
        let x = caret.x.saturating_add(along);
        let look = page.look(caret.x, caret.y);
        let ink = rgba(look.foreground, Ink::Foreground);
        // Where it is drawn, which is where the page put it only once it
        // has got there -- see `motion::Moving::drift`. Everything the
        // caret carries is drawn from these two, so a caret in flight
        // takes the character it is going to cover along with it rather
        // than leaving it behind on the cell.
        let left = (f32::from(x) + drift.0) * cell.width;
        let top = (f32::from(caret.y) + drift.1) * cell.height;
        // Inside a word being spelled the caret is always a bar: what is
        // under it there is the spelling itself, which is being written
        // rather than typed over, and a block would hide the character the
        // reader is in the middle of choosing.
        let shape = match spelling.is_some() {
            true => Caret::Bar,
            false => page.shape(),
        };
        match shape {
            // Narrow, and never thinner than a pixel: a caret that rounds
            // to nothing is a caret nobody can find.
            Caret::Bar => self.block(
                left,
                top,
                (cell.width * BAR).round().max(1.0),
                cell.height,
                ink,
            ),
            Caret::Block => {
                self.block(left, top, cell.width, cell.height, ink);
                let behind = rgba(look.background, Ink::Background);
                // What is under the caret, drawn again in the colour behind
                // it, so that a block does not hide the character it is on.
                if !look.text.trim().is_empty() {
                    self.glyphs_at((left, top), look, behind, 0, fonts, Size::Cell);
                }
            }
        }
    }

    /// What an input method is spelling, drawn where the word will go.
    ///
    /// Over the cells to the right of the caret rather than pushing them
    /// along: the file has not changed, and a window that reflowed the line
    /// for a word that may never be committed would be showing the reader a
    /// file that does not exist. Underlined, which is what says these
    /// characters are not in the file yet.
    ///
    /// In the colours of the place it is being typed into, because that is
    /// the only theme the window has: the cells say what the page's ink and
    /// paper are here.
    fn spelling(&mut self, page: &Page, fonts: &mut Fonts, spelling: Option<&Spelling>) {
        let (Some(spelling), Some(caret)) = (spelling, page.caret()) else {
            return;
        };
        let cell = fonts.cell();
        let look = page.look(caret.x, caret.y);
        let ink = rgba(look.foreground, Ink::Foreground);
        let paper = rgba(look.background, Ink::Background);
        let mut column = caret.x;
        for character in spelling.text.chars() {
            if column >= page.columns() {
                // Off the edge. Clipped rather than wrapped: the row below
                // belongs to the next line of the file.
                break;
            }
            let written = character.to_string();
            #[expect(
                clippy::cast_possible_truncation,
                reason = "the width of one character, which is one or two"
            )]
            let wide = obelus_text::text_width(&written).max(1) as u16;
            let left = f32::from(column) * cell.width;
            let top = f32::from(caret.y) * cell.height;
            let width = f32::from(wide) * cell.width;
            self.block(left, top, width, cell.height, paper);
            let over = Look {
                text: &written,
                foreground: look.foreground,
                background: look.background,
                // Less the underline the cell under it may carry: what is
                // being spelled wears its own, drawn below, and the word
                // it is going into is not the word a server complained
                // about yet.
                modifier: look.modifier.difference(Modifier::UNDERLINED),
                underline: Color::Reset,
            };
            self.glyphs(column, caret.y, over, ink, 0, fonts);
            // The line under it, which is what every input method's inline
            // spelling wears and what tells it apart from the file.
            self.underline(left, top, width, cell.height, ink);
            column = column.saturating_add(wide);
        }
    }
}

impl Atlas {
    /// Both textures, with nothing in them but the white pixel.
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let mut atlas = Self {
            device: device.clone(),
            queue: queue.clone(),
            // Not sRGB: what is in here is coverage and emoji, and the
            // colours it is multiplied by are the theme's own.
            letters: Layers::new(device, wgpu::TextureFormat::R8Unorm),
            pictures: Layers::new(device, wgpu::TextureFormat::Rgba8Unorm),
            remade: false,
            overflowed: false,
            spots: HashMap::new(),
            marks: HashMap::new(),
            white: [0.0; 4],
        };
        atlas.whiten();
        atlas
    }

    /// Puts the one opaque pixel in.
    ///
    /// First, so that it is there before anything asks and lands on the
    /// first layer, and through the room like everything else so that
    /// nothing is ever put on top of it.
    fn whiten(&mut self) {
        let white = self
            .place(1, 1, &[0xff], false)
            .expect("an empty texture has room for one pixel");
        // Half a pixel in, so that no filtering can reach a neighbour.
        let middle = [
            f32::midpoint(white.uv[0], white.uv[2]),
            f32::midpoint(white.uv[1], white.uv[3]),
        ];
        self.white = [middle[0], middle[1], middle[0], middle[1]];
    }

    /// Everything in it is the wrong size now.
    ///
    /// New textures rather than the old ones with their maps cleared: what
    /// the old size took is room nothing would ever be put in again.
    fn empty(&mut self) {
        self.letters = Layers::new(&self.device, self.letters.format);
        self.pictures = Layers::new(&self.device, self.pictures.format);
        self.remade = true;
        self.spots.clear();
        self.marks.clear();
        self.whiten();
    }

    /// The marks are the wrong colour now, which a theme change makes them.
    fn forget_the_marks(&mut self) {
        self.marks.clear();
    }

    /// Where a mark is, if it has been drawn at this size and in these
    /// colours.
    fn mark(&self, key: &(String, bool)) -> Option<Spot> {
        self.marks.get(key).copied().flatten()
    }

    /// Remembers where one landed.
    fn marked(&mut self, key: (String, bool), spot: Option<Spot>) {
        self.marks.insert(key, spot);
    }

    /// And remembers that one cannot be drawn at all.
    fn no_mark(&mut self, key: (String, bool)) {
        self.marks.insert(key, None);
    }

    /// Puts pixels in, and says where they went.
    ///
    /// The one piece of code that writes to a texture: a glyph and a mark
    /// differ in where their pixels come from and in nothing else. A byte a
    /// pixel where the picture is not `colourful`, and four where it is.
    fn place(&mut self, width: u32, height: u32, pixels: &[u8], colourful: bool) -> Option<Spot> {
        if width == 0 || height == 0 {
            return None;
        }
        let layers = match colourful {
            true => &mut self.pictures,
            false => &mut self.letters,
        };
        let had = layers.room.layers.len();
        let most = self.device.limits().max_texture_array_layers;
        let Some((layer, [x, y])) = layers.room.find(width, height, most) else {
            // Full, where it would fit in a layer -- see `overflowed`. Said
            // once, rather than for every glyph the frame goes without.
            if width <= ATLAS && height <= ATLAS && !self.overflowed {
                tracing::warn!(most, "the glyph texture has as many layers as it can");
                self.overflowed = true;
            }
            return None;
        };
        if layers.room.layers.len() > had {
            layers.grow(&self.device, &self.queue, had);
            self.remade = true;
        }
        let bytes = match colourful {
            true => 4,
            false => 1,
        };
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &layers.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: layer },
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * bytes),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        #[expect(
            clippy::cast_precision_loss,
            reason = "a layer is a thousand pixels across"
        )]
        let uv = [
            x as f32 / ATLAS as f32,
            y as f32 / ATLAS as f32,
            (x + width) as f32 / ATLAS as f32,
            (y + height) as f32 / ATLAS as f32,
        ];
        #[expect(
            clippy::cast_precision_loss,
            reason = "what is put in is a few dozen pixels each way"
        )]
        Some(Spot {
            uv,
            layer,
            width: width as f32,
            height: height as f32,
            left: 0.0,
            top: 0.0,
            colourful,
        })
    }

    /// Where a glyph is, putting it in if this is the first time it has
    /// been asked for.
    fn spot(&mut self, fonts: &mut Fonts, key: CacheKey) -> Option<Spot> {
        if let Some(known) = self.spots.get(&key) {
            return *known;
        }
        let spot = self.rasterise(fonts, key);
        self.spots.insert(key, spot);
        spot
    }

    /// Draws one glyph and finds it a place.
    fn rasterise(&mut self, fonts: &mut Fonts, key: CacheKey) -> Option<Spot> {
        let picture = fonts.picture(key)?;
        let width = picture.placement.width;
        let height = picture.placement.height;
        let colourful = matches!(picture.content, SwashContent::Color);
        let pixels = match picture.content {
            // A letter's coverage, which is the whole of it: the colour
            // comes from the instance. And an emoji's colours, as they are.
            SwashContent::Mask | SwashContent::Color => picture.data.clone(),
            // Three coverages, one per subpixel. Obelus does not draw
            // subpixel text -- it would be wrong on a rotated screen and on
            // every screen that is not RGB -- so the middle one is taken as
            // the coverage.
            SwashContent::SubpixelMask => picture
                .data
                .as_chunks::<4>()
                .0
                .iter()
                .map(|texel| texel[1])
                .collect::<Vec<u8>>(),
        };
        // Where the glyph sits against the pen, which is the one thing a
        // mark has no use for: a mark is put where it was told.
        #[expect(
            clippy::cast_precision_loss,
            reason = "a glyph is offset by a few dozen pixels"
        )]
        let (left, top) = (picture.placement.left as f32, picture.placement.top as f32);
        let spot = self.place(width, height, &pixels, colourful)?;
        Some(Spot { left, top, ..spot })
    }
}

impl Layers {
    /// A texture of `LAYERS` empty layers.
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let texture = layered(device, format, LAYERS);
        Self {
            view: array_of(&texture),
            texture,
            format,
            room: Room::new(LAYERS),
        }
    }

    /// As many layers as the room has opened, with what was in the `had`
    /// before them still where it was.
    ///
    /// A texture cannot be made bigger, so this is a new one with the old
    /// one copied into it -- and every place already handed out, in this
    /// frame or an earlier one, still holds what it held. Submitted at
    /// once, which puts it after every write already made to the old one:
    /// a queue's writes go before the next work handed to it.
    fn grow(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, had: usize) {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a device allows a few hundred layers"
        )]
        let had = had as u32;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a device allows a few hundred layers"
        )]
        let has = self.room.layers.len() as u32;
        let texture = layered(device, self.format, has);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("obelus glyphs"),
        });
        encoder.copy_texture_to_texture(
            self.texture.as_image_copy(),
            texture.as_image_copy(),
            wgpu::Extent3d {
                width: ATLAS,
                height: ATLAS,
                depth_or_array_layers: had,
            },
        );
        queue.submit([encoder.finish()]);
        self.view = array_of(&texture);
        self.texture = texture;
        tracing::debug!(layers = has, format = ?self.format, "a glyph texture took another layer");
    }
}

impl Room {
    /// `layers` layers with nothing in them.
    fn new(layers: u32) -> Self {
        let mut room = Self { layers: Vec::new() };
        for _ in 0..layers {
            room.open();
        }
        room
    }

    /// A place for this, opening another layer where none has room -- so
    /// that what is already in the room stays where it is -- and nothing
    /// where no layer could hold it, or the device allows `most` layers
    /// and a step would take more.
    fn find(&mut self, width: u32, height: u32, most: u32) -> Option<(u32, [u32; 2])> {
        if let Some(taken) = self.take(width, height) {
            return Some(taken);
        }
        // Larger than a layer, which no number of layers has room for.
        if width > ATLAS || height > ATLAS {
            return None;
        }
        // Two short of it, because a step past a multiple of six is two.
        if self.layers.len() + 2 > most as usize {
            return None;
        }
        self.open();
        self.take(width, height)
    }

    /// The first place with room for this, and which layer it is on.
    ///
    /// The first layer first, which is where the white pixel goes.
    fn take(&mut self, width: u32, height: u32) -> Option<(u32, [u32; 2])> {
        #[expect(
            clippy::cast_possible_wrap,
            reason = "what is put in is smaller than a layer, which is a thousand pixels"
        )]
        let wanted = etagere::size2(width as i32, height as i32);
        self.layers
            .iter_mut()
            .enumerate()
            .find_map(|(layer, room)| {
                let corner = room.allocate(wanted)?.rectangle.min;
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "a device allows a few hundred layers, and an allocation is never negative"
                )]
                Some((layer as u32, [corner.x as u32, corner.y as u32]))
            })
    }

    /// Another layer, with nothing in it -- or two, where one would make the
    /// count a multiple of six.
    ///
    /// Which wgpu's GL backend takes a square texture of to be a cube map,
    /// and a shader reading a cube map as an array reads nothing from it:
    /// on Mesa's software GL, six layers and twelve read nothing where five
    /// and seven were right. A layer nothing is put in yet is the cheaper
    /// way out than layers that are not square.
    fn open(&mut self) {
        loop {
            #[expect(
                clippy::cast_possible_wrap,
                reason = "a layer is a thousand pixels across"
            )]
            self.layers
                .push(etagere::AtlasAllocator::new(etagere::size2(
                    ATLAS as i32,
                    ATLAS as i32,
                )));
            if !self.layers.len().is_multiple_of(6) {
                return;
            }
        }
    }
}

/// A texture of `layers` layers of glyphs.
fn layered(device: &wgpu::Device, format: wgpu::TextureFormat, layers: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("obelus glyphs"),
        size: wgpu::Extent3d {
            width: ATLAS,
            height: ATLAS,
            depth_or_array_layers: layers,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        // A source as well, because a texture that has to grow is copied
        // into the larger one.
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

/// The whole of a texture, as the array the shader reads it as.
fn array_of(texture: &wgpu::Texture) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    })
}

/// The runs of one colour along a row, as few as they can be said in.
///
/// Two things are settled here. A screen is mostly the page's own colour,
/// so a rectangle per cell would be ten thousand of them to say one thing.
/// And a full-width character owns both of its columns: the second one is a
/// cell `ratatui` has reset, with no text and no colours, which a terminal
/// never draws because it advanced two columns itself. A window draws every
/// cell, so reading that cell's own background painted the default colour
/// behind the right half of every Chinese character -- a line of them came
/// out striped.
fn runs(page: &Page, row: u16) -> Vec<(u16, u16, Color)> {
    runs_from(
        0,
        (0..page.columns()).map(|column| {
            let look = page.look(column, row);
            (look.columns(), look.background)
        }),
    )
}

/// One row of the picture a pane's glass reads: its grounds, then which
/// of its columns carry a letter.
///
/// Two lists and in this order, because that is what they are drawn in --
/// the same order `backgrounds` and `letters` go in on the screen, and for
/// the same reason: a ground drawn after a glyph is a ground *over* it.
///
/// Which is how a page of Chinese read through the glass came out as half
/// of every character. This was a ground and a glyph per cell, a column at
/// a time, and a full-width character is one glyph over two columns whose
/// second is a cell `ratatui` has reset -- so that cell's own ground was
/// painted over the right half of the character before it. As runs the
/// second column belongs to the character, so there is no rectangle there
/// to do it with, and the letters come after every one of them anyway.
fn behind_row(behind: &Behind, barred: &[Barred], y: u16) -> (Vec<(u16, u16, Color)>, Vec<u16>) {
    let (from, to) = (behind.area.left(), behind.area.right());
    let grounds = runs_from(
        from,
        (from..to).map(|x| {
            behind.look(x, y).map_or((1, Color::Reset), |under| {
                (under.columns(), under.background)
            })
        }),
    );
    let lettered = (from..to)
        .filter(|&x| {
            behind
                .look(x, y)
                .is_some_and(|under| lettered_behind(under, barred, x, y))
        })
        .collect();
    (grounds, lettered)
}

/// The same, over a row of cells from wherever they come.
///
/// Two callers and one rule: the page's own row, and the row of a picture
/// of what a pane was opened over. That picture is read through the glass,
/// and it used to be painted a cell at a time -- so a full-width character
/// had the reset cell beside it painted over the right half of its glyph,
/// and a page of Chinese seen through the glass was a page of half
/// characters. Said in one place, the second column belongs to the
/// character in both.
///
/// Each cell as how many columns it takes and what it is drawn on.
fn runs_from(start: u16, cells: impl Iterator<Item = (u16, Color)>) -> Vec<(u16, u16, Color)> {
    let mut runs: Vec<(u16, u16, Color)> = Vec::new();
    // How many columns of the character just seen are still to come.
    let mut rest = 0;
    for (along, (columns, background)) in cells.enumerate() {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a row of the grid, which is not thousands of columns"
        )]
        let column = start + along as u16;
        let colour = match rest {
            0 => {
                rest = columns - 1;
                background
            }
            // The rest of a character that was seen already, so there is
            // always a run to take the colour from.
            _ => {
                rest -= 1;
                runs.last().map_or(Color::Reset, |&(_, _, running)| running)
            }
        };
        match runs.last_mut() {
            Some((_, end, running)) if *running == colour && *end == column => *end = column + 1,
            _ => runs.push((column, column + 1, colour)),
        }
    }
    runs
}

/// What is left of a run of cells once the holes in it are taken out.
///
/// The holes in any order and overlapping as they please: a frame's ring
/// and a pane's glass can both be in one row.
fn without(start: u16, end: u16, holes: &mut [(u16, u16)]) -> Vec<(u16, u16)> {
    holes.sort_unstable();
    let mut left = Vec::new();
    let mut from = start;
    for &(hole, after) in holes.iter() {
        if after <= from || hole >= end {
            continue;
        }
        if hole > from {
            left.push((from, hole));
        }
        from = from.max(after);
    }
    if from < end {
        left.push((from, end));
    }
    left
}

/// A run of quads, drawn; nothing where the run is empty.
fn drawing(pass: &mut wgpu::RenderPass<'_>, quads: std::ops::Range<usize>) {
    if quads.start < quads.end {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a frame is thousands of quads, not billions"
        )]
        pass.draw(0..4, quads.start as u32..quads.end as u32);
    }
}

/// Whether a cell wearing this colour is glass rather than a colour: it
/// is inside a pane, and the colour is that pane's own.
///
/// Which is the question every square of colour drawn over a pane has to
/// ask first, because the glass is already there and a square of the
/// pane's colour on it is a hole in it.
/// Whether this cell's picture is some shape the window draws, rather
/// than the character a terminal stands that shape in with.
///
/// One list, because they are one rule, and the rule is not only that the
/// character would look wrong under the shape. A cap, a bar and a rule can
/// all sit on a pane, and a pane is *glass*: `backgrounds` leaves those
/// cells unpainted on purpose, so a shape that had to cover a leftover
/// glyph would have to paint over them -- and painting over glass is
/// covering up the very thing the reader is meant to see through. Nothing
/// covers anything here; the glyph is simply never drawn.
fn drawn_as_a_shape(
    page: &Page,
    said: &Said<'_>,
    framed: &[&Behind],
    capped: &[&Capped],
    column: u16,
    row: u16,
) -> bool {
    let within = |area: ratatui::layout::Rect| {
        (area.left()..area.right()).contains(&column) && (area.top()..area.bottom()).contains(&row)
    };
    said.ruled
        .iter()
        .any(|rule| rule.spans(page, column, row).is_some())
        || framed.iter().any(|card| card.ring_holds(page, column, row))
        || capped.iter().any(|cap| within(cap.area))
        || said.ticked.iter().any(|tick| within(tick.area))
        // Asked of the cell and not of the column, the same as a rule's:
        // a panel drawn over a scrollbar leaves cells in that column that
        // are the panel's, and the letters there are its own.
        || said.barred.iter().any(|bar| bar.covers(page, column, row))
        // Per cell rather than per run, and against the glyph, the same as
        // a rule: the margin is drawn to the foot of the editor's region
        // and a compact list goes over the bottom of it, so the rows under
        // the list are the list's and keep what it wrote in them.
        || said
            .stroked
            .iter()
            .any(|mark| mark.holds(page, column, row))
}

/// Whether the picture of what a pane was opened over writes this cell.
///
/// The same question `letters` asks of the page, asked of the picture: a
/// bar is drawn as a shape, and drawn over the pane as well -- `bars` is
/// asked from where the bar was said and not from what the page holds
/// there -- so the block a terminal has for a track is a glyph nobody
/// wanted here, under a capsule and under the tint.
///
/// It was not merely redundant. A full block's raster is taller than its
/// cell, and the glass covers the pane's own rectangle exactly, so the
/// part of the first row's block that reached above the grid came out in
/// the margin round it -- a dark cell's-width line along the top of the
/// window, wherever a list was opened over a file long enough to have a
/// bar.
fn lettered_behind(under: Look<'_>, barred: &[Barred], x: u16, y: u16) -> bool {
    !under.text.trim().is_empty()
        && !barred.iter().any(|showing| {
            let area = showing.bar.area;
            (area.left()..area.right()).contains(&x) && (area.top()..area.bottom()).contains(&y)
        })
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

/// A glyph's rectangle cut back to the grid, and the part of its picture
/// that is left. `None` where none of it is.
///
/// A glyph's raster is bigger than its cell often enough -- an accent
/// reaches into the row above, a full block fills its own cell and a pixel
/// or two past it -- and inside the grid that is right: a window draws the
/// letters whole where a terminal chops each one at its cell's edge.
///
/// Outside the grid there is no row above. The margin is the strip no cell
/// reaches, and what is in it is the page's own ground and nothing else --
/// so a glyph that overflowed into it was a mark on the window's edge
/// belonging to no cell: the change map's half block along the top of the
/// window, and the block a bar is drawn with beside it.
fn clipped(rect: [f32; 4], uv: [f32; 4], grid: [f32; 2]) -> Option<([f32; 4], [f32; 4])> {
    let [left, top, width, height] = rect;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let (kept_left, kept_top) = (left.max(0.0), top.max(0.0));
    let (kept_right, kept_bottom) = ((left + width).min(grid[0]), (top + height).min(grid[1]));
    if kept_right <= kept_left || kept_bottom <= kept_top {
        return None;
    }
    let [from_u, from_v, to_u, to_v] = uv;
    // Where each edge ended up, as a part of the whole picture: the atlas
    // is read at the same place the pixels are drawn, or the glyph is
    // squeezed into what is left of it rather than cut.
    let across = |at: f32| from_u + (to_u - from_u) * (at - left) / width;
    let down = |at: f32| from_v + (to_v - from_v) * (at - top) / height;
    Some((
        [
            kept_left,
            kept_top,
            kept_right - kept_left,
            kept_bottom - kept_top,
        ],
        [
            across(kept_left),
            down(kept_top),
            across(kept_right),
            down(kept_bottom),
        ],
    ))
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

/// What colour to draw the line under a cell in, where there is one.
///
/// Its own colour where the view gave it one and the ink where it did not,
/// which is what a terminal does with an underline nobody coloured -- and
/// the reason it is asked here rather than taken for the foreground is the
/// one view that colours it: a server's complaint is underlined in the
/// colour that kind of trouble is written in, under a word the syntax has
/// already coloured something else. Taking the ink there would draw the
/// mark in the colour of whatever the word happened to be.
fn underline_ink(look: &Look<'_>) -> Option<[f32; 4]> {
    if !look.modifier.contains(Modifier::UNDERLINED) {
        return None;
    }
    Some(match look.underline {
        Color::Reset => rgba(look.foreground, Ink::Foreground),
        colour => rgba(colour, Ink::Foreground),
    })
}

/// Which half of a cell a colour is for, which is the whole of what `Reset`
/// means.
#[derive(Clone, Copy, Debug)]
enum Ink {
    Foreground,
    Background,
}

/// A colour as the hardware wants it.
///
/// Straight through, with no conversion: the surface is viewed without its
/// sRGB conversion for exactly this reason. A theme's `#1e1e2e` is the
/// colour the reader picked, and a pipeline that corrects it draws a
/// different one.
/// A texture the size of the window, to draw a frame into and read back.
fn made_to_draw_into(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("obelus behind"),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

/// Everything the shader is handed but the two pictures a bind group
/// reads, in one place because the pictures are made again whenever the
/// window changes size and the rest goes with them.
///
/// Seven things that are the same in every one of them, kept together so
/// that what differs between bind groups is all that is written at each.
struct Binder<'a> {
    device: &'a wgpu::Device,
    layout: &'a wgpu::BindGroupLayout,
    uniforms: &'a wgpu::Buffer,
    letters: &'a wgpu::TextureView,
    pictures: &'a wgpu::TextureView,
    sampler: &'a wgpu::Sampler,
    smooth: &'a wgpu::Sampler,
}

impl Binder<'_> {
    /// A bind group reading `backdrop` as what is behind a pane and
    /// `blurred` as the same, blurred.
    fn bound(&self, backdrop: &wgpu::TextureView, blurred: &wgpu::TextureView) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("obelus"),
            layout: self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(self.letters),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(backdrop),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(self.smooth),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(blurred),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(self.pictures),
                },
            ],
        })
    }
}

/// How far a bar's mark is drawn from where the frame put it, while the
/// band beside it catches up, in rows.
///
/// The same fraction of the way along as the band, so the two arrive
/// together. `to_come` is measured between two rows the bar was *drawn*
/// at, so the mark neither sets out from nor lands on a row it was never
/// on -- and it is held between them, because a mark that left the row it
/// was on before it set off, or went past the row it is going to, is a
/// mark that steps backwards. Which is the one thing a mark must not do.
fn mark_behind(to_come: f32, behind: f32, since: f32) -> f32 {
    if since.abs() <= f32::EPSILON {
        return 0.0;
    }
    to_come * (behind / since).clamp(0.0, 1.0)
}

/// A colour some of the way from one to another.
///
/// What a settled bar is drawn in. Mixed rather than given an alpha
/// because the page under it is opaque and already known, so this is the
/// colour it would come out as -- and nothing here then depends on how
/// the pipeline happens to blend, which is a thing that has to be right
/// in the shader as well as here.
/// Which way one corner of a held run turns.
///
/// A hold is one rectangle per row, and the rows are not the same width:
/// a selection starts part way along a line and stops part way along
/// another. What decides a corner is the row beside it -- whether that
/// row stops short of this one, stops level with it, or carries on past
/// it -- and the third is the one that matters, because without it every
/// step between two rows is cut square and the hold reads as a stack of
/// plates rather than as one shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Turn {
    /// The hold's own corner: the row beside it stops short, or there is
    /// no row beside it.
    Corner = 0,
    /// The row beside it carries on past this one, so the boundary bends
    /// the other way to meet it.
    Other = 1,
    /// The row beside it has the same edge, so there is no corner here at
    /// all and nothing to round.
    None = 2,
}

impl Turn {
    /// The four corners of a run, packed two bits each in the order the
    /// shader reads them: top left, top right, bottom left, bottom right.
    ///
    /// `cut` says whether something was put *over* the run at either end
    /// -- a box with a frame round it, sitting on the row. That end is
    /// not an end: the row carries on under the box, so the corners
    /// there are no corners, the same as where the row beside it is
    /// level. Without it a selected row with a card over its middle is
    /// two little plates with four round corners each, one at either end
    /// of the row, which read as badges rather than as the row they are.
    fn corners(
        start: u16,
        end: u16,
        above: Option<(u16, u16, Color)>,
        below: Option<(u16, u16, Color)>,
        cut: (bool, bool),
    ) -> u32 {
        let left = |beside: Option<(u16, u16, Color)>| match beside {
            Some((from, _, _)) if from < start => Self::Other,
            Some((from, _, _)) if from == start => Self::None,
            _ => Self::Corner,
        };
        let right = |beside: Option<(u16, u16, Color)>| match beside {
            Some((_, to, _)) if to > end => Self::Other,
            Some((_, to, _)) if to == end => Self::None,
            _ => Self::Corner,
        };
        let cut_to = |side: bool, turn: Self| match side {
            true => Self::None,
            false => turn,
        };
        [
            cut_to(cut.0, left(above)),
            cut_to(cut.1, right(above)),
            cut_to(cut.0, left(below)),
            cut_to(cut.1, right(below)),
        ]
        .into_iter()
        .enumerate()
        .fold(0, |turns, (at, turn)| turns | ((turn as u32) << (at * 2)))
    }
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

fn away(from: [f32; 4], to: [f32; 4]) -> [f32; 4] {
    let mut out = to;
    for channel in 0..3 {
        out[channel] = (to[channel] * 2.0 - from[channel]).clamp(0.0, 1.0);
    }
    out
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

/// What of a band's room moves while it catches up, given the pane over
/// the page and whether the band is `under` it.
///
/// All of it for a band the pane's own view drew, which is the list. Only
/// what the pane leaves showing for one the pane was put over: what is
/// drawn there is the pane, and it is not what scrolled.
///
/// Asked and not worked out from where the two are. It was once whether
/// the band was inside the pane, which a full-screen dialog answers yes
/// for everything -- so a transcript scrolling on under the settings or
/// a full list slid the whole dialog with it, a shudder on every line an
/// agent wrote.
fn sliding(room: [f32; 4], panes: &[[f32; 4]], under: bool) -> Vec<[f32; 4]> {
    tiles(room, if under { panes } else { &[] })
}

/// The part of a band's room a pane is over, where the band is under the
/// pane rather than the pane's own.
///
/// The other half of `sliding`: what that leaves out on the screen is
/// what slides behind the glass instead.
fn beneath(room: [f32; 4], pane: [f32; 4], under: bool) -> Option<[f32; 4]> {
    if sliding(room, &[pane], under) == [room] {
        return None;
    }
    Some([
        room[0].max(pane[0]),
        room[1].max(pane[1]),
        room[2].min(pane[2]),
        room[3].min(pane[3]),
    ])
}

/// The copies `Painter::glass_kept_still` lays over one level's glass: one
/// per band on it, cut to the band's rows and reading what is behind from
/// as far down as `catching_up` takes the rows from above.
fn kept_still(
    glass: &Quad,
    level: usize,
    stack: &[&Behind],
    bands: &[Rolled<'_>],
    cell: CellSize,
) -> Vec<Quad> {
    bands
        .iter()
        .filter(|band| on_glass(band, stack) == Some(level))
        .map(|band| Quad {
            uv: box_of(band.room, cell),
            lower: band.behind * cell.height,
            ..*glass
        })
        .collect()
}

/// Which level's glass a band's rows are drawn on: the nearest pane or box
/// round it, where the list is that one's own. One the pane was put over
/// slides behind the glass instead -- see `Painter::sliding_under`.
fn on_glass(band: &Rolled<'_>, stack: &[&Behind]) -> Option<usize> {
    if band.under {
        return None;
    }
    stack
        .iter()
        .rposition(|over| over.area.intersection(band.room) == band.room)
}

/// What is left of the frame once the rooms that are moving are taken
/// out of it.
///
/// Rectangles, left top right bottom, and they do not overlap: each is
/// drawn as a copy of the picture the frame was drawn into, and a pixel
/// copied twice is a pixel drawn twice for nothing.
fn tiles(whole: [f32; 4], rooms: &[[f32; 4]]) -> Vec<[f32; 4]> {
    let mut left = vec![whole];
    for room in rooms {
        let mut kept = Vec::with_capacity(left.len() + 3);
        for piece in left {
            cut(piece, *room, &mut kept);
        }
        left = kept;
    }
    left
}

/// One rectangle less another, as up to four: what is above the hole,
/// what is below it, and the two strips beside it.
///
/// The strips are cut to the hole's own rows, so that they do not overlap
/// the pieces above and below.
fn cut(piece: [f32; 4], room: [f32; 4], into: &mut Vec<[f32; 4]>) {
    let [left, top, far, low] = piece;
    if room[0] >= far || room[2] <= left || room[1] >= low || room[3] <= top {
        into.push(piece);
        return;
    }
    if room[1] > top {
        into.push([left, top, far, room[1]]);
    }
    if room[3] < low {
        into.push([left, room[3], far, low]);
    }
    let (over, under) = (room[1].max(top), room[3].min(low));
    if room[0] > left {
        into.push([left, over, room[0], under]);
    }
    if room[2] < far {
        into.push([room[2], over, far, under]);
    }
}

/// How much of a pane a slide has let through, which is the room its
/// shadow falls from.
///
/// A pane on its way in is taken from `shift` pixels further up or down
/// its own picture and stops where that picture does -- so the edge it is
/// joined by stays where it is, at the seam, and the free edge is the one
/// that has moved. Which is the edge a shadow falls from, so this is the
/// only part of it that the sliding changes.
fn reached(pane: [f32; 4], shift: f32) -> [f32; 4] {
    [
        pane[0],
        pane[1] + shift.max(0.0),
        pane[2],
        pane[3] + shift.min(0.0),
    ]
}

/// Which half plane a pane casts its shadow into, which is the same one
/// its glass is cut to -- and `None` where it casts none at all.
///
/// A pane is joined to the page along one edge and casts from the other:
/// a list standing on the status row throws its shadow up over the file,
/// and one hanging from the top throws it down.
///
/// A full-screen pane has no free edge to cast from, and `0` is not the
/// way to say so: to the shader it is a box joined to nothing, which
/// casts on all four sides. Joined as `Above` it was a grey band across
/// the page's last row -- its own shadow, falling on itself -- and as `0`
/// it was a grey frame in the strip round the grid, which is there
/// whenever the window is not a whole number of cells.
fn casting(joined: Joined) -> Option<u32> {
    match joined {
        Joined::Above => Some(HANGING),
        Joined::Below => Some(STANDING),
        Joined::Screen => None,
        // A box casts all round -- see `shadows`.
        Joined::Nowhere => Some(0),
    }
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

/// What part of its cell a block element covers, as rectangles of it:
/// left, top, right and bottom, from nothing to the whole cell.
///
/// Drawn rather than asked of the face, because a face draws `█` from its
/// own ascent to its own descent and the cell is a line height Obelus
/// chose (`font::LINE_HEIGHT`). Where the face is the shorter -- Menlo is,
/// by a pixel or two -- every row of blocks stands a hairline off the next,
/// and the welcome screen's mark is striped across; where it is the taller
/// the glyph is cut back to the cell and nobody sees the difference. Every
/// terminal that draws these itself does it for this reason, which is why
/// `ob` never showed it.
///
/// The shades are left to the face: they are a texture, not a region, and
/// a face's own is what a reader's terminal would show.
fn pieces(text: &str) -> Option<&'static [[f32; 4]]> {
    const HALF: f32 = 0.5;
    const EIGHTH: f32 = 0.125;
    let mut chars = text.chars();
    let (Some(glyph), None) = (chars.next(), chars.next()) else {
        return None;
    };
    Some(match glyph {
        '\u{2580}' => &[[0.0, 0.0, 1.0, HALF]],
        // A lower one to seven eighths.
        '\u{2581}' => &[[0.0, 1.0 - EIGHTH, 1.0, 1.0]],
        '\u{2582}' => &[[0.0, 1.0 - 2.0 * EIGHTH, 1.0, 1.0]],
        '\u{2583}' => &[[0.0, 1.0 - 3.0 * EIGHTH, 1.0, 1.0]],
        '\u{2584}' => &[[0.0, HALF, 1.0, 1.0]],
        '\u{2585}' => &[[0.0, 1.0 - 5.0 * EIGHTH, 1.0, 1.0]],
        '\u{2586}' => &[[0.0, 1.0 - 6.0 * EIGHTH, 1.0, 1.0]],
        '\u{2587}' => &[[0.0, 1.0 - 7.0 * EIGHTH, 1.0, 1.0]],
        '\u{2588}' => &[[0.0, 0.0, 1.0, 1.0]],
        // A left seven eighths down to one.
        '\u{2589}' => &[[0.0, 0.0, 7.0 * EIGHTH, 1.0]],
        '\u{258a}' => &[[0.0, 0.0, 6.0 * EIGHTH, 1.0]],
        '\u{258b}' => &[[0.0, 0.0, 5.0 * EIGHTH, 1.0]],
        '\u{258c}' => &[[0.0, 0.0, HALF, 1.0]],
        '\u{258d}' => &[[0.0, 0.0, 3.0 * EIGHTH, 1.0]],
        '\u{258e}' => &[[0.0, 0.0, 2.0 * EIGHTH, 1.0]],
        '\u{258f}' => &[[0.0, 0.0, EIGHTH, 1.0]],
        '\u{2590}' => &[[HALF, 0.0, 1.0, 1.0]],
        '\u{2594}' => &[[0.0, 0.0, 1.0, EIGHTH]],
        '\u{2595}' => &[[1.0 - EIGHTH, 0.0, 1.0, 1.0]],
        // The quadrants, by which of the four they fill.
        '\u{2596}' => &[[0.0, HALF, HALF, 1.0]],
        '\u{2597}' => &[[HALF, HALF, 1.0, 1.0]],
        '\u{2598}' => &[[0.0, 0.0, HALF, HALF]],
        '\u{2599}' => &[[0.0, 0.0, HALF, 1.0], [HALF, HALF, 1.0, 1.0]],
        '\u{259a}' => &[[0.0, 0.0, HALF, HALF], [HALF, HALF, 1.0, 1.0]],
        '\u{259b}' => &[[0.0, 0.0, 1.0, HALF], [0.0, HALF, HALF, 1.0]],
        '\u{259c}' => &[[0.0, 0.0, 1.0, HALF], [HALF, HALF, 1.0, 1.0]],
        '\u{259d}' => &[[HALF, 0.0, 1.0, HALF]],
        '\u{259e}' => &[[HALF, 0.0, 1.0, HALF], [0.0, HALF, HALF, 1.0]],
        '\u{259f}' => &[[HALF, 0.0, 1.0, HALF], [0.0, HALF, 1.0, 1.0]],
        _ => return None,
    })
}

/// One of those rectangles in pixels, from the corner of its cell.
///
/// Each edge is rounded where it falls rather than the size being rounded
/// on its own: a cell is not a whole number of pixels tall, so a block
/// whose height was rounded starts where the one above it ended only by
/// luck, and the stripe `pieces` is there to take away comes back as a
/// pixel of overlap or of gap every few rows.
fn snapped(at: (f32, f32), cell: crate::font::CellSize, piece: [f32; 4]) -> [f32; 4] {
    let [from_x, from_y, to_x, to_y] = piece;
    let left = cell.width.mul_add(from_x, at.0).round();
    let top = cell.height.mul_add(from_y, at.1).round();
    let right = cell.width.mul_add(to_x, at.0).round();
    let bottom = cell.height.mul_add(to_y, at.1).round();
    [left, top, (right - left).max(1.0), (bottom - top).max(1.0)]
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
    use super::mark_behind;

    mod stacked {
        use ratatui::{layout::Rect, style::Color};

        use super::super::{
            GLASS, HANGING, Level, Placed, Quad, Reads, SOLID, box_of, casts, catching, kept_still,
            on_the_screen, put_over, uncovered,
        };
        use crate::{
            font::CellSize,
            grid::{Behind, Page, Rolled},
        };

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
        /// transcript under the list gets glass of its own.
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
        #[test]
        fn the_glass_drawn_again_reads_its_own_level() {
            let level = |under: std::ops::Range<usize>, glass, lowered, end| Level {
                under,
                glass,
                lowered,
                end,
                rect: [0.0; 4],
                pane: true,
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

    use ratatui::buffer::Cell;

    use super::*;
    use crate::grid::Update;

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
        let cell = crate::font::CellSize {
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

    /// A face says it is frosted in a bit the shader does not read as
    /// one of the four turns, nor as the wedge.
    ///
    /// Deliberate break: put `FROSTED` at `1 << 14`, inside the turns,
    /// and every face is read as a plate whose top right corner bends.
    #[test]
    fn frosted_is_none_of_the_turns() {
        assert_eq!(FROSTED & (255 << HELD_TURNS), 0);
        assert_eq!(FROSTED & WEDGE, 0);
    }

    /// No turn a plate can carry is read as a shadow.
    ///
    /// Asked of what `Turn::corners` packs rather than of the bits, so a
    /// shadow anywhere a real turn lands fails it. Deliberate break: put
    /// `SHADOW` back at 8192, and a hold under a row reaching further
    /// left -- `Other` at its top left -- is a shadow.
    #[test]
    fn a_shadow_is_none_of_the_turns() {
        let row = |from: u16, to: u16| Some((from, to, Color::Reset));
        for (above, below) in [
            (row(0, 20), None),
            (None, row(0, 20)),
            (row(5, 20), row(5, 20)),
        ] {
            for cut in [(false, false), (true, false), (false, true)] {
                let turns = Turn::corners(5, 10, above, below, cut);
                assert_eq!((turns << HELD_TURNS) & SHADOW, 0, "{turns:08b}");
            }
        }
        assert_eq!(SHADOW & (FROSTED | WEDGE), 0);
    }

    /// Which way a hold's corner turns is a fact about the row beside it.
    ///
    /// A hold is one rectangle per row and the rows are not the same
    /// width, so the corners where they step are not the hold's own
    /// corners: the boundary there bends the other way, round into the
    /// row that carries on. Without that, every step is cut square and a
    /// selection several lines tall reads as a stack of plates.
    ///
    /// Deliberate break: answer `Corner` in place of `Other` and the
    /// steps come out square again -- which is what the first of these
    /// drew, and what a reader looking at a selection notices first.
    /// Answer `Corner` in place of `None` and a flush edge grows two
    /// notches where there is no corner at all.
    #[test]
    fn a_corner_turns_the_other_way_where_the_row_beside_it_carries_on() {
        let ink = Color::Rgb(1, 2, 3);
        let turns = |above: Option<(u16, u16)>, below: Option<(u16, u16)>| {
            Turn::corners(
                10,
                20,
                above.map(|(from, to)| (from, to, ink)),
                below.map(|(from, to)| (from, to, ink)),
                (false, false),
            )
        };
        // Top left, top right, bottom left, bottom right.
        let at = |turns: u32, corner: u32| (turns >> (corner * 2)) & 3;

        let alone = turns(None, None);
        for corner in 0..4 {
            assert_eq!(
                at(alone, corner),
                Turn::Corner as u32,
                "corner {corner} of a row with nothing beside it"
            );
        }

        // The row above has the same ends, so along the top there is no
        // corner to round at all.
        let flush = turns(Some((10, 20)), None);
        assert_eq!(at(flush, 0), Turn::None as u32, "top left, level");
        assert_eq!(at(flush, 1), Turn::None as u32, "top right, level");
        assert_eq!(at(flush, 2), Turn::Corner as u32, "and nothing below it");

        // It carries on past this row at both ends.
        let wider = turns(Some((0, 30)), None);
        assert_eq!(at(wider, 0), Turn::Other as u32, "top left, above is wider");
        assert_eq!(
            at(wider, 1),
            Turn::Other as u32,
            "top right, above is wider"
        );

        // And one that stops short leaves this row its own corners.
        let narrower = turns(Some((12, 18)), None);
        assert_eq!(at(narrower, 0), Turn::Corner as u32, "above stops short");
        assert_eq!(at(narrower, 1), Turn::Corner as u32, "at both ends");

        // The row below is asked the same question of the other two.
        let step = turns(None, Some((0, 20)));
        assert_eq!(
            at(step, 2),
            Turn::Other as u32,
            "bottom left, below runs on"
        );
        assert_eq!(at(step, 3), Turn::None as u32, "bottom right, level");

        // And where something was put over the run, the end it stops at
        // is not an end: the row carries on under the box.
        let under_a_box = Turn::corners(10, 20, None, None, (true, false));
        assert_eq!(at(under_a_box, 0), Turn::None as u32, "top left, cut");
        assert_eq!(at(under_a_box, 2), Turn::None as u32, "bottom left, cut");
        assert_eq!(
            at(under_a_box, 1),
            Turn::Corner as u32,
            "and the other end is still the hold's own"
        );
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

    /// A place handed out holds what was put there for as long as the room
    /// does: a full layer opens another, and nothing already in it moves.
    ///
    /// The atlas was emptied when it filled, half way through a frame, and
    /// the glyphs that frame had already placed were read from where the
    /// next ones had just been written: on a doubled screen a page of
    /// Chinese filled it, and a line number came out as a piece of a
    /// character until something drew the frame again.
    ///
    /// Through `find`, which is what `Atlas::place` asks. Deliberate break:
    /// make `find` clear every layer and take again where none has room,
    /// which is what the atlas did -- the next glyph lands on the white
    /// pixel.
    #[test]
    fn a_full_layer_opens_another_and_keeps_what_it_holds() {
        let mut room = Room::new(LAYERS);
        let (layer, [x, y]) = room.find(1, 1, 2048).expect("room for the white pixel");
        assert_eq!(
            layer, 0,
            "the white pixel is on the layer a quad reads by default"
        );
        let mut handed = vec![(layer, [x, y, 1, 1])];
        // Large, so that a few hundred of them fill more than the first two
        // layers.
        let (wide, tall) = (100, 100);
        for _ in 0..400 {
            let (layer, [x, y]) = room.find(wide, tall, 2048).expect("room on some layer");
            handed.push((layer, [x, y, wide, tall]));
        }
        for (at, (layer, [x, y, w, h])) in handed.iter().enumerate() {
            for (other, [ox, oy, ow, oh]) in &handed[..at] {
                let apart = layer != other
                    || x + w <= *ox
                    || ox + ow <= *x
                    || y + h <= *oy
                    || oy + oh <= *y;
                assert!(apart, "place {at} is on top of one handed out before it");
            }
        }
        assert!(
            handed.iter().any(|(layer, _)| *layer >= LAYERS),
            "enough was put in to need another layer"
        );
    }

    /// No count of layers a glyph texture is made with is a multiple of six,
    /// which wgpu's GL backend makes a cube map of -- and a shader reading
    /// that as an array reads nothing, so every letter goes.
    ///
    /// Deliberate break: let `open` push one layer and stop.
    #[test]
    fn a_glyph_texture_is_never_six_layers() {
        let mut room = Room::new(LAYERS);
        let mut counts = vec![room.layers.len()];
        for _ in 0..2000 {
            room.find(100, 100, 2048).expect("room on some layer");
            counts.push(room.layers.len());
        }
        assert!(
            counts.last().is_some_and(|&last| last > 12),
            "enough was put in to pass two multiples of six"
        );
        assert!(
            counts.iter().all(|count| !count.is_multiple_of(6)),
            "a texture was made of {counts:?} layers"
        );
    }

    /// A room at the device's limit says so rather than opening a layer the
    /// device cannot make.
    ///
    /// Deliberate break: drop the check against `most` in `find`.
    #[test]
    fn a_room_stops_at_the_layers_a_device_allows() {
        let mut room = Room::new(LAYERS);
        let mut placed = 0;
        while room.find(512, 512, 4).is_some() {
            placed += 1;
            assert!(placed < 100, "the room kept opening layers");
        }
        assert!(
            room.layers.len() <= 4,
            "{} layers on a device of four",
            room.layers.len()
        );
    }
}
