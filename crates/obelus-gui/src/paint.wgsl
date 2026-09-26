// Every quad on the screen: a cell's background, a glyph, the caret.
//
// One pipeline for all of them, because they differ only in where their
// colour comes from -- the instance's own, the atlas, or both -- and a flag
// says which. Drawn in the order they are written into the buffer, which is
// what puts a glyph over the background it sits on.

struct Screen {
    // The window, in real pixels.
    size: vec2<f32>,
    padding: vec2<f32>,
};

@group(0) @binding(0) var<uniform> screen: Screen;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;
// What is behind a pane, drawn into a texture of its own a pass earlier.
// Sampled only by a glass quad; every other quad ignores it.
@group(0) @binding(3) var behind: texture_2d<f32>;
@group(0) @binding(4) var behind_sampler: sampler;

// What the instance buffer holds.
struct Quad {
    // Where it goes, in pixels: left, top, width, height.
    @location(0) rect: vec4<f32>,
    // And where its picture is in the atlas: left, top, right, bottom.
    @location(1) uv: vec4<f32>,
    @location(2) colour: vec4<f32>,
    // 1: a solid colour. 2: a picture with colours of its own.
    // 4: a solid with its corners taken off. 8: glass over what is behind.
    // 16 and 128: square along the top or along the bottom, which is the
    // edge a pane is joined to and no edge at all.
    // 32 and 64: the frame that has just been drawn, put back on the
    // screen in two pieces while a pane slides into it.
    @location(3) flags: u32,
    // How far those corners are taken off, in pixels.
    @location(4) radius: f32,
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
    // back together. Flat, because what it is is a region rather than
    // something measured across the quad.
    @location(6) @interpolate(flat) box: vec4<f32>,
};

@vertex
fn vertex(@builtin(vertex_index) corner: u32, quad: Quad) -> Fragment {
    // A triangle strip: top left, top right, bottom left, bottom right.
    let along = vec2<f32>(f32(corner & 1u), f32(corner >> 1u));
    let pixels = quad.rect.xy + along * quad.rect.zw;
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
    out.box = quad.uv;
    return out;
}

// How far outside a rounded box a point is, in pixels: negative inside it,
// zero on the edge. The standard signed distance to one, which is the only
// way to draw a curve on a quad without an outline to sample.
// `hanging` says the box has no top edge at all.
//
// A pane is not a card floating over the page; it is the page's own region
// taken over, joined to the row above it. A join is not an edge: nothing
// there is rounded, nothing there bends what is behind, and nothing there
// catches the light. All three fall out of the one line that leaves the
// top off the box -- and what clips the pane up there is the quad's own
// bounds, which is where the join is.
fn outside(point: vec2<f32>, half_size: vec2<f32>, radius: f32, joined: f32) -> f32 {
    var side = abs(point) - half_size;
    // `joined` is which way the seam is: -1 above, 1 below, 0 for a shape
    // with edges all round. Measuring against `joined * point.y` leaves
    // that end open, which is the whole of the difference.
    if (joined != 0.0) {
        side.y = joined * point.y - half_size.y;
    }
    let corner = side + vec2<f32>(radius);
    return length(max(corner, vec2<f32>(0.0))) + min(max(corner.x, corner.y), 0.0) - radius;
}

// Which way the nearest edge of a rounded box faces, from a point inside
// it. The gradient of the distance above, worked out rather than sampled:
// a derivative would be the difference between two pixels, and what this
// is for is the direction light bends at an edge.
fn facing(point: vec2<f32>, half_size: vec2<f32>, radius: f32, joined: f32) -> vec2<f32> {
    var side = abs(point) - half_size;
    var way = sign(point);
    if (joined != 0.0) {
        side.y = joined * point.y - half_size.y;
        way.y = joined;
    }
    let corner = side + vec2<f32>(radius);
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
// How wide the softening is, in pixels.
//
// Wide enough that what is written behind the glass stops being letters
// and becomes the texture of a page with writing on it. A frost that left
// the words legible would be two things to read in one place, which is
// worse than either of them alone -- and it is the one thing a reader
// would rather the glass did not do.
const FROST: f32 = 11.0;
// How many places it is sampled from, on two rings and the middle.
const TAPS: i32 = 12;
// Where the light is, which is above and a little to the left -- the one
// direction every raised thing in every interface is lit from.
const LIGHT: vec2<f32> = vec2<f32>(-0.42, -1.0);

@fragment
fn fragment(in: Fragment) -> @location(0) vec4<f32> {
    // A pane on its way in.
    //
    // The frame was drawn once into a texture of its own, and it is put
    // back on the screen in two pieces: everything that is not the pane,
    // where it belongs, and the pane, higher up than it will end. What is
    // under the second piece is the page, drawn before both, which is what
    // shows in the band the pane has not reached yet.
    //
    // Nothing about this reaches the drawing above. A pane that has
    // arrived is not composed at all, and neither is a window with no pane
    // on it: what it costs is one pass, for a fifth of a second.
    if ((in.flags & 32u) != 0u) {
        let at = in.position.xy;
        // The pane's own room is the other piece's.
        if (at.x >= in.box.x && at.x < in.box.z && at.y >= in.box.y && at.y < in.box.w) {
            discard;
        }
        return textureSample(behind, behind_sampler, at / screen.size);
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
    // Glass: what is behind, bent at the edges, tinted, and lit.
    //
    // Not a blur. The blur is the smallest part of it -- three taps, and
    // only so that what is behind stops competing with what is written on
    // top. What says glass is the other two: content pulled sideways in a
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
        let edge = normalize(LIGHT);
        let bevel = min(in.radius * BEVEL, min(in.half_size.x, in.half_size.y));
        // One at the very rim and nothing at all through the middle.
        let rim = clamp(1.0 + distance / max(bevel, 1.0), 0.0, 1.0);
        let facing = facing(in.middle, in.half_size, in.radius, joined);
        let lens = pow(rim, 2.5);

        let uv = in.position.xy / screen.size;
        // Pulled *inward*: at the edge of a slab you see what is further
        // under it, which is what makes a straight line behind the pane
        // bend as it passes the rim.
        let shift = -facing * lens * DEPTH / screen.size;
        // Two rings and the middle, which at this width is enough that a
        // row of text behind comes out as a band rather than as a comb.
        var frosted = textureSample(behind, behind_sampler, uv + shift).rgb * 2.0;

        var weight = 2.0;
        for (var tap = 0; tap < TAPS; tap = tap + 1) {
            let angle = f32(tap) * 0.5236;
            let ring = select(1.0, 0.55, (tap & 1) == 0);
            // Least at the rim and most through the middle. Which is the
            // whole of why the bending above can be seen at all: a frost
            // laid on evenly would smear away the one band where the
            // glass has any thickness to show.
            let width = FROST * (0.42 + 0.58 * (1.0 - lens));
            let step = vec2<f32>(cos(angle), sin(angle)) * width * ring / screen.size;
            frosted += textureSample(behind, behind_sampler, uv + shift + step).rgb;
            weight += 1.0;
        }
        frosted = frosted / weight;

        // More of the pane's own colour where what is behind is close to
        // it in brightness, because that is where what is written on the
        // glass would otherwise have the least to stand out against.
        let grey = vec3<f32>(0.2126, 0.7152, 0.0722);
        let near = 1.0 - clamp(abs(dot(frosted, grey) - dot(in.colour.rgb, grey)) * 3.0, 0.0, 1.0);
        let tint = clamp(in.colour.a + (1.0 - in.colour.a) * near * 0.4, 0.0, 1.0);
        var glass = mix(frosted, in.colour.rgb, tint);

        // The rim, lit from one side and faintly returned on the other,
        // which is a thing with a thickness rather than a painted line.
        let lit = pow(rim, 10.0) * max(dot(facing, edge), 0.0);
        let far = pow(rim, 22.0) * max(dot(facing, -edge), 0.0);
        glass += vec3<f32>(lit * 0.5 + far * 0.22);
        return vec4<f32>(glass, 1.0);
    }
    // Before the plain solid, because a rounded one is a solid as well.
    if ((in.flags & 4u) != 0u) {
        let distance = outside(in.middle, in.half_size, in.radius, 0.0);
        // Softened over the one pixel either side of the edge. Without
        // it a corner is a staircase, which at the size a key's cap is
        // drawn is the whole of what the eye sees.
        let covered = clamp(0.5 - distance, 0.0, 1.0);
        return vec4<f32>(in.colour.rgb, in.colour.a * covered);
    }
    if ((in.flags & 1u) != 0u) {
        return in.colour;
    }
    let texel = textureSample(atlas, atlas_sampler, in.uv);
    if ((in.flags & 2u) != 0u) {
        // An emoji, which carries its own colours and uses the instance's
        // only for how much of it shows.
        return vec4<f32>(texel.rgb, texel.a * in.colour.a);
    }
    // A letter, which is coverage: the ink is the instance's colour and the
    // glyph says how much of each pixel it covers.
    return vec4<f32>(in.colour.rgb, in.colour.a * texel.a);
}
