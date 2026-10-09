// The canvas quad: draws the off-screen canvas to the window, lit.
//
// Everything is decided per *canvas texel*, never per screen pixel: the
// texel is fetched with textureLoad (no filtering), and the light it gets is
// sampled at that texel's centre. So every screen pixel of one texel gets
// the same colour, and the upscale stays an exact integer one however smooth
// the light is between texels.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput

struct CanvasParams {
    // World position of the canvas centre: the snapped world camera.
    camera: vec2<f32>,
    // Canvas size in texels (= world units).
    canvas_size: vec2<f32>,
    // The sun, or the night: the light under open sky, before any lamp.
    ambient: vec4<f32>,
    // The light under a ceiling the sky does not reach, before any lamp.
    indoor: vec4<f32>,
    // Lightmap size in subtiles; zero when there is no lightmap.
    lightmap_size: vec2<f32>,
    // Subtiles per world unit (5 / 48).
    subtiles_per_unit: f32,
    // 0: draw the canvas as it is (the editor), 1: light it.
    lit: f32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: CanvasParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var canvas: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var lightmap: texture_2d<f32>;

// The lights' RGB, and the sky factor in alpha.
fn light_at(subtile: vec2<f32>) -> vec4<f32> {
    let size = vec2<i32>(params.lightmap_size);
    // Bilinear between subtile centres, by hand: four loads, clamped at the
    // map's edge so the outermost subtiles extend to it.
    let p = subtile - 0.5;
    let base = vec2<i32>(floor(p));
    let f = p - floor(p);
    let lo = clamp(base, vec2<i32>(0), size - 1);
    let hi = clamp(base + 1, vec2<i32>(0), size - 1);
    let a = textureLoad(lightmap, vec2<i32>(lo.x, lo.y), 0);
    let b = textureLoad(lightmap, vec2<i32>(hi.x, lo.y), 0);
    let c = textureLoad(lightmap, vec2<i32>(lo.x, hi.y), 0);
    let d = textureLoad(lightmap, vec2<i32>(hi.x, hi.y), 0);
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let size = params.canvas_size;
    let texel = min(floor(in.uv * size), size - 1.0);
    let albedo = textureLoad(canvas, vec2<i32>(texel), 0);
    if (params.lit == 0.0) {
        return albedo;
    }

    // uv runs down the canvas; world Y runs up.
    let world = params.camera + vec2<f32>(texel.x + 0.5 - size.x * 0.5, size.y * 0.5 - texel.y - 0.5);
    let subtile = world * params.subtiles_per_unit;
    // Off the map is open sky with no lamp in it.
    var baked = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    let inside = all(subtile >= vec2<f32>(0.0)) && all(subtile < params.lightmap_size);
    if (inside) {
        baked = light_at(subtile);
    }
    // As much of the sun as the sky factor lets in, the indoor dark for the
    // rest, and the lamps on top.
    let light = mix(params.indoor.rgb, params.ambient.rgb, baked.a) + baked.rgb;
    // Capped at full brightness: daylight is the canvas exactly as drawn,
    // and a lamp lights the dark rather than bleaching the day.
    return vec4<f32>(albedo.rgb * min(light, vec3<f32>(1.0)), albedo.a);
}
