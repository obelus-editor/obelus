// Every quad on the screen: a cell's background, a glyph, the caret.
//
// One pipeline for all of them, because they differ only in where their
// colour comes from -- the instance's own, the atlas, or both -- and a flag
// says which. Drawn in the order they are written into the buffer, which is
// what puts a glyph over the background it sits on.

struct Screen {
    // The window, in real pixels.
    size: vec2<f32>,
    // Where the grid starts in it. A window is not a whole number of cells
    // across or down, and the strip left over is halved and put outside
    // both ends rather than swallowed by the cells on the edge -- see
    // `grid::margin`. Added here, which is the one place a quad's pixels
    // become a place on the screen.
    origin: vec2<f32>,
    // Where the light on the welcome screen's mark is, in real pixels: its
    // middle, how far its falloff reaches either side, how much of the
    // glow it carries, and nothing. Zero strength is no light at all,
    // which is the mark at rest and every other screen there is.
    sheen: vec4<f32>,
    // And the colour it carries the mark to.
    glow: vec4<f32>,
    // How many pixels a point is.
    scale: f32,
};

@group(0) @binding(0) var<uniform> screen: Screen;
// Every letter's coverage, in the first channel of as many layers as it
// has taken.
@group(0) @binding(1) var atlas: texture_2d_array<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;
// What is behind a pane, drawn into a texture of its own a pass earlier.
// Sampled only by a glass quad; every other quad ignores it.
@group(0) @binding(3) var behind: texture_2d<f32>;
@group(0) @binding(4) var behind_sampler: sampler;
// And the same picture blurred, by the two passes the blur quads are drawn
// in. Read only by a glass quad, beside the sharp one.
@group(0) @binding(5) var blurred: texture_2d<f32>;
// And the pictures with colours of their own, beside the letters: an emoji,
// and an agent's mark.
@group(0) @binding(6) var pictures: texture_2d_array<f32>;

// What the instance buffer holds.
struct Quad {
    // Where it goes, in pixels: left, top, width, height.
    @location(0) rect: vec4<f32>,
    // And where its picture is in the atlas: left, top, right, bottom --
    // or for glass, in pixels, the rectangle it is drawn inside.
    @location(1) uv: vec4<f32>,
    @location(2) colour: vec4<f32>,
    // 1: a solid colour. 2: a picture with colours of its own.
    // 4: a solid with its corners taken off. 8: glass over what is behind.
    // 16 and 128: square along the top or along the bottom, which is the
    // edge a pane is joined to and no edge at all.
    // 32 and 64: the frame that has just been drawn, put back on the
    // screen in two pieces while a pane slides into it. 256: the mark in
    // a switch that is set. 512: glass over another pane's glass. 1024:
    // what is behind a pane, blurred one way. 2048: a letter of the
    // welcome screen's mark, which the light runs across. 2097152: a
    // triangle filling the quad, which is the arrow on the seam a
    // deletion left. 16777216: the mark that turns while something is
    // happening.
    @location(3) flags: u32,
    // How far those corners are taken off, in pixels -- and for the two
    // quads that carry no corners, the one number each of them needs
    // instead: how far a sliding pane has still to come, which way a
    // wedge points, and how far round a turning mark's head is.
    @location(4) radius: f32,
    // Which layer of the atlas its picture is on: the letters', or the
    // pictures' where it has colours of its own.
    @location(5) layer: u32,
    // How much further down than where it stands a glass reads what is
    // behind it, in pixels: a list catching up is taken from further up
    // the frame, and its glass with it -- see `glass_kept_still`.
    @location(6) lower: f32,
};

struct Fragment {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) colour: vec4<f32>,
    @location(2) @interpolate(flat) flags: u32,
    // Where in the quad this pixel is, measured in pixels from its middle.
    // Only a rounded one reads it, and it is what the distance below is
    // worked out from.
    @location(3) middle: vec2<f32>,
    @location(4) @interpolate(flat) half_size: vec2<f32>,
    @location(5) @interpolate(flat) radius: f32,
    // A rectangle in pixels, for the two quads that put a sliding pane
    // back together and for glass, which is drawn inside one. Flat,
    // because what it is is a region rather than something measured
    // across the quad.
    @location(6) @interpolate(flat) box: vec4<f32>,
    @location(7) @interpolate(flat) layer: u32,
    @location(8) @interpolate(flat) lower: f32,
};

@vertex
fn vertex(@builtin(vertex_index) corner: u32, quad: Quad) -> Fragment {
    // A triangle strip: top left, top right, bottom left, bottom right.
    let along = vec2<f32>(f32(corner & 1u), f32(corner >> 1u));
    let pixels = quad.rect.xy + along * quad.rect.zw + screen.origin;
    // Pixels to the clip space the hardware draws in, which puts the
    // origin in the middle and points the second axis the other way.
    let clip = vec2<f32>(
        pixels.x / screen.size.x * 2.0 - 1.0,
        1.0 - pixels.y / screen.size.y * 2.0,
    );

    var out: Fragment;
    out.position = vec4<f32>(clip, 0.0, 1.0);
    out.uv = mix(quad.uv.xy, quad.uv.zw, along);
    out.colour = quad.colour;
    out.flags = quad.flags;
    out.half_size = quad.rect.zw * 0.5;
    out.middle = (along - vec2<f32>(0.5)) * quad.rect.zw;
    out.radius = quad.radius;
    out.box = quad.uv + vec4<f32>(screen.origin, screen.origin);
    out.layer = quad.layer;
    out.lower = quad.lower;
    return out;
}

// How far outside a rounded box a point is, in pixels: negative inside it,
// zero on the edge. The standard signed distance to one, which is the only
// way to draw a curve on a quad without an outline to sample.
// `joined` says which way a pane's seam faces: 1 above, -1 below, and 0
// for a shape with edges all round, which is what a key's cap is.
//
// A pane has one edge and no corners. It is not a card floating over the
// page; it is the page's own region taken over, as wide as the window and
// fastened to the row at one end -- so three of its four sides are seams:
// the two the window's own edges run down, and the one it hangs from. A
// seam is not a boundary, and a corner needs two boundaries meeting, so
// there is nowhere on a pane for one. Rounded anyway, what the curve does
// is bite a notch out of the window's edge.
//
// Which leaves the distance to the one edge it has, and `point.x` out of
// it altogether. What clips the other three is the quad's own bounds,
// which is where the seams are.
//
// A box with a frame round it is the other kind: joined to nothing, so
// every side is an edge and every corner is rounded, the same as a cap.
// It is glass all the same, inside the frame's line.
fn outside(point: vec2<f32>, half_size: vec2<f32>, radius: f32, joined: f32) -> f32 {
    if (joined != 0.0) {
        return joined * point.y - half_size.y;
    }
    let corner = abs(point) - half_size + vec2<f32>(radius);
    return length(max(corner, vec2<f32>(0.0))) + min(max(corner.x, corner.y), 0.0) - radius;
}

// How far outside a run the reader has hold of a point is.
//
// A hold is one rectangle per row of it, and the rows are not the same
// width: a selection starts part way along a line and stops part way
// along another. So each of the four corners turns one of three ways,
// and which one is a fact about the row beside it -- 0 where the hold
// stops and the corner is the hold's own, 2 where the row beside it has
// the same edge and there is no corner at all, and 1 where that row
// carries on *past* this one, which is the corner that bends the other
// way. Without the third the steps between the rows are cut square, and
// a selection reads as a stack of plates rather than as one shape.
//
// The bend the other way is a quarter circle whose middle is outside the
// rectangle in x and inside it in y, which is where a step is: the rows
// are a row tall and it is their ends that move.
fn held(point: vec2<f32>, half_size: vec2<f32>, radius: f32, kinds: u32) -> f32 {
    var kind = 0u;
    if (point.x < 0.0) {
        if (point.y < 0.0) { kind = kinds & 3u; } else { kind = (kinds >> 4u) & 3u; }
    } else {
        if (point.y < 0.0) { kind = (kinds >> 2u) & 3u; } else { kind = (kinds >> 6u) & 3u; }
    }
    let at = abs(point);
    let out = at - half_size;
    let box = length(max(out, vec2<f32>(0.0))) + min(max(out.x, out.y), 0.0);
    // The hold carries on past this corner, so there is no corner.
    if (kind == 2u) {
        return box;
    }
    // The hold's own corner.
    if (kind == 0u) {
        let far = out + vec2<f32>(radius);
        return length(max(far, vec2<f32>(0.0))) + min(max(far.x, far.y), 0.0) - radius;
    }
    // And the one that bends the other way, inside the square of the
    // radius that sits just outside the rectangle in x and just inside
    // it in y. Everywhere else this corner is no corner at all.
    let middle = vec2<f32>(half_size.x + radius, half_size.y - radius);
    if (at.x >= half_size.x && at.x <= middle.x && at.y >= middle.y && at.y <= half_size.y) {
        return radius - length(at - middle);
    }
    return box;
}

// Which way the nearest edge of a rounded box faces, from a point inside
// it. The gradient of the distance above, worked out rather than sampled:
// a derivative would be the difference between two pixels, and what this
// is for is the direction light bends at an edge.
fn facing(point: vec2<f32>, half_size: vec2<f32>, radius: f32, joined: f32) -> vec2<f32> {
    // One edge, so one way to face.
    if (joined != 0.0) {
        return vec2<f32>(0.0, joined);
    }
    let corner = abs(point) - half_size + vec2<f32>(radius);
    let way = sign(point);
    if (max(corner.x, corner.y) > 0.0) {
        return normalize(max(corner, vec2<f32>(0.0))) * way;
    }
    // Deep inside, where the nearest edge is one of the sides.
    if (corner.x > corner.y) {
        return vec2<f32>(way.x, 0.0);
    }
    return vec2<f32>(0.0, way.y);
}

// How thick the bevel is, as a part of the corner's radius. What decides
// how wide the band is that bends what is behind: a slab of glass with a
// rounded edge shows the world undisturbed through its middle and pulled
// about only where it curves away.
const BEVEL: f32 = 1.7;
// How far that band moves what it is bending, in pixels at the very edge.
//
// Small, and the reason is what is behind it. A bevel gathers what is
// under it into a narrower band, and gathering a photograph reads as
// glass while gathering a page of monospaced text reads as a comb: the
// rows come out as teeth. So the bend is enough to see and not enough to
// count.
const DEPTH: f32 = 13.0;
// How wide the softening is: the spread of the Gaussian, in pixels.
//
// Wide enough that what is written behind the glass stops being letters
// and becomes the texture of a page with writing on it. A frost that left
// the words legible would be two things to read in one place, which is
// worse than either of them alone -- and it is the one thing a reader
// would rather the glass did not do.
const FROST: f32 = 5.5;
// How many pairs of pixels either side of the middle it reaches, which is
// three spreads: past that a Gaussian has nothing left to add.
//
// Pairs, because a sample between two pixels is both of them weighted by
// where it falls -- one read of a smooth sampler is two of the blur.
const PAIRS: i32 = 8;
// How much of the sharp picture shows at the very rim, against the blur.
//
// Some, because the rim is where the glass has a thickness to show: the
// bend above is the whole of it, and a bend in something already smeared
// flat is a bend nobody can see.
const RIM_SHARP: f32 = 0.45;
// Where the light is, which is above and a little to the left -- the one
// direction every raised thing in every interface is lit from.
const LIGHT: vec2<f32> = vec2<f32>(-0.42, -1.0);

// How far a hold's grain moves a pixel either way, toward white or toward
// black. Small: what is written on a hold has to be read through it, and a
// grain that can be seen as dots from where the reader sits is noise on
// the words rather than a surface under them.
const GRAIN: f32 = 0.04;
// And how big a grain is, in points, so that it is the same size to the
// eye on a screen drawn at twice its own pixels as on one that is not. A
// pixel each was tried, and on a screen drawn at twice its pixels that is
// half of anything the eye can pick out: a grain that fine is a flat
// colour again.
const GRAIN_SIZE: f32 = 1.5;

// A number between -1 and 1 for a place on the grid, the same every frame
// for the same place. From the place on the grid rather than in the run,
// so a hold that runs across several quads -- a row each, and the rim
// under the face -- is one grain and not a seam where each begins. And on
// the grid rather than the window, because the grid moves in the window by
// a part of a pixel at every step of a resize, and a grain that stayed
// where it was would crawl under a hold that had not moved.
//
// Smoothed between the corners of a square a grain wide, because a grain
// bigger than a pixel that is not smoothed is a square, and a field of
// squares is a mosaic rather than frost.
fn grain(at: vec2<f32>) -> f32 {
    let place = at / max(GRAIN_SIZE * screen.scale, 1.0);
    let corner = floor(place);
    let along = place - corner;
    let eased = along * along * (vec2<f32>(3.0) - 2.0 * along);
    let at_corner = vec2<u32>(corner);
    let top = mix(speck(at_corner), speck(at_corner + vec2<u32>(1u, 0u)), eased.x);
    let low = mix(speck(at_corner + vec2<u32>(0u, 1u)), speck(at_corner + vec2<u32>(1u, 1u)), eased.x);
    return mix(top, low, eased.y);
}

// A number between -1 and 1 for one corner of that square. An integer
// hash (PCG), because one made of a sine falls into bands far from the
// origin on hardware with a short sine.
fn speck(corner: vec2<u32>) -> f32 {
    var state = corner.x * 1973u + corner.y * 9277u + 26699u;
    state = state * 747796405u + 2891336453u;
    var word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    word = (word >> 22u) ^ word;
    return f32(word) / 4294967295.0 * 2.0 - 1.0;
}

// How much a Gaussian of the frost's spread weighs a pixel this far out.
fn gauss(far: f32) -> f32 {
    return exp(-far * far / (2.0 * FROST * FROST));
}

// How far a point is from the line between two others.
fn to_line(point: vec2<f32>, one_end: vec2<f32>, other: vec2<f32>) -> f32 {
    let along = point - one_end;
    let line = other - one_end;
    let how_far = clamp(dot(along, line) / dot(line, line), 0.0, 1.0);
    return length(along - line * how_far);
}

// The mark in a switch, as two strokes of a pen: down to the turn, and up
// again further. In the quad's own units from nought to one, so a reader
// who makes the text bigger gets a bigger one drawn the same.
const TURN: vec2<f32> = vec2<f32>(0.43, 0.70);
const STARTS: vec2<f32> = vec2<f32>(0.25, 0.50);
const ENDS: vec2<f32> = vec2<f32>(0.76, 0.31);
// Half the pen's width, and the ends are round because a distance to a
// line is: a mark with cut ends reads as two strokes rather than one.
const NIB: f32 = 0.085;

// The mark that turns: a ring the pen goes round, in the quad's own units
// from nought to one, and how much of the ring is ink behind the head.
// Three quarters, fading to nothing at the tail, so what the eye follows is
// the head and there is no second end for it to catch on.
const RING_AT: f32 = 0.38;
const RING_NIB: f32 = 0.09;
const RING_TAIL: f32 = 0.75;
const WHOLE_TURN: f32 = 6.2831855;

@fragment
fn fragment(in: Fragment) -> @location(0) vec4<f32> {
    // The mark that turns, its head `radius` of a turn round from the top
    // and going clockwise, which is the way the braille it stands in for
    // goes.
    if ((in.flags & 16777216u) != 0u) {
        let at = in.middle / (in.half_size * 2.0);
        let ring = abs(length(at) - RING_AT);
        // Back into pixels for the softening, the same as the switch.
        let covered = clamp((RING_NIB - ring) * in.half_size.x * 2.0, 0.0, 1.0);
        // How far behind the head this point is, as a part of a turn:
        // clockwise from the top on a screen whose second axis points
        // down is `atan2(x, -y)`.
        let angle = fract(atan2(at.x, -at.y) / WHOLE_TURN);
        let behind = fract(in.radius - angle + 1.0);
        let tail = clamp(1.0 - behind / RING_TAIL, 0.0, 1.0);
        return vec4<f32>(in.colour.rgb, in.colour.a * covered * tail);
    }
    // The arrow on the seam a deletion left: a triangle filling the quad,
    // its point in the middle of one short side. `radius` says which side,
    // because the direction is the one thing a wedge has to carry and the
    // field is going spare.
    //
    // Before the solid, which it is one of, and drawn as coverage rather
    // than clipped: at the size a cell is, a triangle with hard edges is a
    // staircase, and the point is the part the eye is on.
    if ((in.flags & 2097152u) != 0u) {
        let at = in.middle / (in.half_size * 2.0) + vec2<f32>(0.5);
        var along = at.x;
        if (in.radius < 0.0) {
            along = 1.0 - at.x;
        }
        // Half as tall as the base, closing to nothing at the point.
        let half = 0.5 * (1.0 - along);
        let over = abs(at.y - 0.5) - half;
        // Back into pixels, so the edge is a pixel wide whatever size the
        // text is -- the same as the switch's mark below.
        let covered = clamp(0.5 - over * in.half_size.y * 2.0, 0.0, 1.0);
        return vec4<f32>(in.colour.rgb, in.colour.a * covered);
    }
    // The mark in a switch that is set.
    if ((in.flags & 256u) != 0u) {
        let at = in.middle / (in.half_size * 2.0) + vec2<f32>(0.5);
        let stroke = min(to_line(at, STARTS, TURN), to_line(at, TURN, ENDS));
        // Back into pixels for the softening, so the edge is a pixel wide
        // whatever size the box is.
        let covered = clamp((NIB - stroke) * in.half_size.x * 2.0, 0.0, 1.0);
        return vec4<f32>(in.colour.rgb, in.colour.a * covered);
    }
    // A piece of the frame, where something on it is moving.
    //
    // The frame was drawn once into a texture of its own, and it is put
    // back on the screen in pieces: the rectangles the thing that is
    // moving is *not* in, each where it belongs, and then that thing,
    // taken from somewhere else in the same picture. What is under it
    // where it has not reached is whatever was drawn there before -- the
    // page for a pane, and for a band the page it scrolled off.
    //
    // The rectangles are worked out on the way in -- see `tiles` -- so
    // there is nothing to leave out here: a piece is a copy of the
    // picture at the place it stands.
    //
    // Nothing about this reaches the drawing above. A pane that has
    // arrived is not composed at all, and neither is a window with
    // nothing moving on it: what it costs is one pass, for a fifth of a
    // second.
    if ((in.flags & 32u) != 0u) {
        return textureSample(behind, behind_sampler, in.position.xy / screen.size);
    }
    if ((in.flags & 64u) != 0u) {
        let at = in.position.xy;
        let taken_at = vec2<f32>(at.x, at.y - in.radius);
        // Off the end of the pane is the pane not being there yet.
        if (taken_at.y < in.box.y || taken_at.y >= in.box.w) {
            discard;
        }
        let taken = textureSample(behind, behind_sampler, taken_at / screen.size);
        return vec4<f32>(taken.rgb, in.colour.a);
    }
    // What is behind a pane, blurred one way: across on the first pass and
    // down on the second, which together are a Gaussian in both.
    //
    // Every sample held inside the pane, whose rectangle is `box`: what is
    // outside it in the picture is nothing, and a blur that reached for it
    // would darken the glass along every edge.
    if ((in.flags & 1024u) != 0u) {
        let way = in.colour.xy;
        let at = in.position.xy;
        let low = in.box.xy + vec2<f32>(0.5);
        let high = max(in.box.zw - vec2<f32>(0.5), low);
        var sum = textureSampleLevel(behind, behind_sampler, clamp(at, low, high) / screen.size, 0.0).rgb * gauss(0.0);
        var weight = gauss(0.0);
        for (var pair = 1; pair <= PAIRS; pair = pair + 1) {
            let near = f32(pair * 2 - 1);
            let far = f32(pair * 2);
            let both = gauss(near) + gauss(far);
            // Between the two, where a smooth sampler reads each of them
            // in the proportion the Gaussian gives it.
            let out = (near * gauss(near) + far * gauss(far)) / both;
            let ahead = clamp(at + way * out, low, high) / screen.size;
            let behind_it = clamp(at - way * out, low, high) / screen.size;
            sum += (textureSampleLevel(behind, behind_sampler, ahead, 0.0).rgb
                + textureSampleLevel(behind, behind_sampler, behind_it, 0.0).rgb) * both;
            weight += both * 2.0;
        }
        return vec4<f32>(sum / weight, 1.0);
    }
    // Glass: what is behind, bent at the edges, tinted, and lit.
    //
    // Not only a blur. The blur is the smallest part of it -- worked out a
    // pass earlier, and only so that what is behind stops competing with
    // what is written on top. What says glass is the other two: content pulled sideways in a
    // band along the rim, which is what a bevel does to what is behind it,
    // and a bright line along that rim where the light catches it.
    if ((in.flags & 8u) != 0u) {
        var joined = 0.0;
        if ((in.flags & 16u) != 0u) {
            joined = 1.0;
        } else if ((in.flags & 128u) != 0u) {
            joined = -1.0;
        }
        let distance = outside(in.middle, in.half_size, in.radius, joined);
        // Outside the rounded corners the pane is not there at all, and
        // what shows is what was drawn under it.
        if (distance > 0.0) {
            discard;
        }
        // And outside the rectangle it was given, which is all of it for
        // the glass itself and a list's rows for the same glass drawn
        // again under them -- the shape stays the whole pane's.
        if (any(in.position.xy < in.box.xy) || any(in.position.xy >= in.box.zw)) {
            discard;
        }
        let edge = normalize(LIGHT);
        let bevel = min(in.radius * BEVEL, min(in.half_size.x, in.half_size.y));
        // One at the very rim and nothing at all through the middle.
        let rim = clamp(1.0 + distance / max(bevel, 1.0), 0.0, 1.0);
        let facing = facing(in.middle, in.half_size, in.radius, joined);
        let lens = pow(rim, 2.5);

        let uv = (in.position.xy + vec2<f32>(0.0, in.lower)) / screen.size;
        // Pulled *inward*: at the edge of a slab you see what is further
        // under it, which is what makes a straight line behind the pane
        // bend as it passes the rim.
        let shift = -facing * lens * DEPTH / screen.size;
        // Two rings and the middle, which at this width is enough that a
        // row of text behind comes out as a band rather than as a comb.
        //
        // The blurred picture, with the sharp one let back in toward the
        // rim: least blur at the rim and most through the middle, which is
        // the whole of why the bending can be seen at all -- a frost laid
        // on evenly would smear away the one band where the glass has any
        // thickness to show.
        let soft = textureSampleLevel(blurred, behind_sampler, uv + shift, 0.0).rgb;
        let sharp = textureSampleLevel(behind, behind_sampler, uv + shift, 0.0).rgb;
        let frosted = mix(soft, sharp, lens * RIM_SHARP);

        // More of the pane's own colour where what is behind is close to
        // it in brightness, because that is where what is written on the
        // glass would otherwise have the least to stand out against.
        let grey = vec3<f32>(0.2126, 0.7152, 0.0722);
        var near = 1.0 - clamp(abs(dot(frosted, grey) - dot(in.colour.rgb, grey)) * 3.0, 0.0, 1.0);
        // Not over another pane's glass: what is behind is that pane's
        // colour, which is always close to this one's, so this would add
        // the most colour exactly where the other pane had already -- and
        // a box whose glass shows nothing is a grey box.
        if ((in.flags & 512u) != 0u) {
            near = 0.0;
        }
        let tint = clamp(in.colour.a + (1.0 - in.colour.a) * near * 0.4, 0.0, 1.0);
        var glass = mix(frosted, in.colour.rgb, tint);

        // The rim, lit from one side and faintly returned on the other,
        // which is a thing with a thickness rather than a painted line.
        let lit = pow(rim, 10.0) * max(dot(facing, edge), 0.0);
        let far = pow(rim, 22.0) * max(dot(facing, -edge), 0.0);
        glass += vec3<f32>(lit * 0.5 + far * 0.22);
        return vec4<f32>(glass, 1.0);
    }
    // The soft edge outside a pane or a box, which is the one thing a
    // terminal has no answer to at all: that the thing is *over* the
    // page rather than part of it. A terminal says that with a rule, and
    // a rule is a boundary between two subjects -- a different claim.
    //
    // `box` is what casts it and the quad reaches a spread past it on
    // every side, so the spread is what is left over. A pane joined to
    // the page casts from its one free edge, which is the same half
    // plane `outside` gives its glass; a box joined to nothing casts
    // from all four, round its corners.
    if ((in.flags & 8388608u) != 0u) {
        let half = (in.box.zw - in.box.xy) * 0.5;
        let middle = in.position.xy - (in.box.xy + half);
        var joined = 0.0;
        if ((in.flags & 16u) != 0u) {
            joined = 1.0;
        } else if ((in.flags & 128u) != 0u) {
            joined = -1.0;
        }
        let distance = outside(middle, half, in.radius, joined);
        // Nothing under the thing itself: a shadow is what falls on what
        // is behind it, and the glass is already drawn.
        if (distance <= 0.0) {
            discard;
        }
        let spread = max(in.half_size.x - half.x, 1.0);
        let fall = 1.0 - clamp(distance / spread, 0.0, 1.0);
        // Squared, so it leaves the edge dark and is gone before it
        // reaches the end: a shadow that fades in a straight line reads
        // as a band of grey with an edge of its own.
        return vec4<f32>(in.colour.rgb, in.colour.a * fall * fall);
    }
    // A run the reader has hold of, whose corners do not all turn the
    // same way. Before the plain rounded solid, which it also is.
    //
    // Frosted: a grain over the colour, a little lighter and a little
    // darker pixel by pixel, which is what a flat colour does not have and
    // glass does. Not lit and not shaded -- light on a hold read as a key
    // standing up off the page -- and grey, and as much lighter as darker,
    // so that it averages out to the colour the theme chose and moves none
    // of its hue.
    if ((in.flags & 4096u) != 0u) {
        // The quad reaches a radius past the run either side, because a
        // corner that bends the other way is drawn out there.
        let room = in.half_size - vec2<f32>(in.radius, 0.0);
        let distance = held(in.middle, room, in.radius, (in.flags >> 13u) & 255u);
        let covered = clamp(0.5 - distance, 0.0, 1.0);
        // The face only: the rim is the line a reader finds the shape
        // by, and a line that wanders lighter and darker along its length
        // is not a clean one.
        var colour = in.colour.rgb;
        if ((in.flags & 4194304u) != 0u) {
            colour = clamp(colour + vec3<f32>(grain(in.position.xy - screen.origin) * GRAIN), vec3<f32>(0.0), vec3<f32>(1.0));
        }
        return vec4<f32>(colour, in.colour.a * covered);
    }
    // Before the plain solid, because a rounded one is a solid as well.
    if ((in.flags & 4u) != 0u) {
        // A radius per half, so that a plate which the same hold carries
        // on past keeps its corners square on that side and round on the
        // other. 16 and 128 mean here what they mean for glass: joined
        // above, joined below.
        var top_corner = in.radius;
        var low_corner = in.radius;
        if ((in.flags & 16u) != 0u) {
            top_corner = 0.0;
        }
        if ((in.flags & 128u) != 0u) {
            low_corner = 0.0;
        }
        let round = select(low_corner, top_corner, in.middle.y < 0.0);
        let corner = abs(in.middle) - in.half_size + vec2<f32>(round);
        let distance = length(max(corner, vec2<f32>(0.0)))
            + min(max(corner.x, corner.y), 0.0) - round;
        // Softened over the one pixel either side of the edge. Without
        // it a corner is a staircase, which at the size a key's cap is
        // drawn is the whole of what the eye sees.
        let covered = clamp(0.5 - distance, 0.0, 1.0);
        return vec4<f32>(in.colour.rgb, in.colour.a * covered);
    }
    if ((in.flags & 1u) != 0u) {
        return in.colour;
    }
    if ((in.flags & 2u) != 0u) {
        // An emoji, which carries its own colours and uses the instance's
        // only for how much of it shows.
        let texel = textureSample(pictures, atlas_sampler, in.uv, in.layer);
        return vec4<f32>(texel.rgb, texel.a * in.colour.a);
    }
    let coverage = textureSample(atlas, atlas_sampler, in.uv, in.layer).r;
    // A letter, which is coverage: the ink is the instance's colour and the
    // glyph says how much of each pixel it covers.
    var ink = in.colour.rgb;
    // And one of the welcome screen's, which rests at the colour it was
    // given and is carried to the other as the light goes by. Here rather
    // than in the instance's colour because this is the one thing on the
    // screen drawn finer than a cell: a colour per glyph is the eight
    // bands a terminal already has, and what a window has instead is the
    // pixel.
    if ((in.flags & 2048u) != 0u) {
        let away = abs(in.position.x - screen.sheen.x) / max(screen.sheen.y, 1.0);
        let near = clamp(1.0 - away, 0.0, 1.0);
        // Smooth at both ends, so the light has no edge of its own: a
        // falloff straight from nothing to all of it is a triangle, and a
        // triangle travelling across the mark reads as a shape over it
        // rather than as light on it.
        let lit = near * near * (3.0 - 2.0 * near);
        ink = mix(ink, screen.glow.rgb, lit * screen.sheen.z);
    }
    return vec4<f32>(ink, in.colour.a * coverage);
}
