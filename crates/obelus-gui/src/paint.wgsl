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
    @location(3) flags: u32,
};

struct Fragment {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) colour: vec4<f32>,
    @location(2) @interpolate(flat) flags: u32,
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
    return out;
}

@fragment
fn fragment(in: Fragment) -> @location(0) vec4<f32> {
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
