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

// What the instance buffer holds.
struct Quad {
    // Where it goes, in pixels: left, top, width, height.
    @location(0) rect: vec4<f32>,
    // And where its picture is in the atlas: left, top, right, bottom.
    @location(1) uv: vec4<f32>,
    @location(2) colour: vec4<f32>,
    // 1: a solid colour. 2: a picture with colours of its own.
    // 4: a solid with its corners taken off.
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
    return out;
}

// How far outside a rounded box a point is, in pixels: negative inside it,
// zero on the edge. The standard signed distance to one, which is the only
// way to draw a curve on a quad without an outline to sample.
fn outside(point: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let corner = abs(point) - half_size + vec2<f32>(radius);
    return length(max(corner, vec2<f32>(0.0))) + min(max(corner.x, corner.y), 0.0) - radius;
}

@fragment
fn fragment(in: Fragment) -> @location(0) vec4<f32> {
    // Before the plain solid, because a rounded one is a solid as well.
    if ((in.flags & 4u) != 0u) {
        let distance = outside(in.middle, in.half_size, in.radius);
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
