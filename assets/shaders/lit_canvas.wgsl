// The canvas quad: draws the off-screen canvas to the window, lit.
//
// Everything is decided per *canvas texel*, never per screen pixel: the
// texel is fetched with textureLoad (no filtering), and the light it gets is
// sampled at that texel's centre. So every screen pixel of one texel gets
// the same colour, and the upscale stays an exact integer one however smooth
// the light is between texels.
//
// The vignette and temporary zoom blink are screen-space effects. Outside
// the blink there is no blur, filtering, or fractional scaling.

#import bevy_sprite::mesh2d_vertex_output::VertexOutput
#import bevy_sprite::mesh2d_view_bindings::view
#import bevy_render::view::frag_coord_to_uv

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
    // World position of the lightmap's bottom-left corner: the origin, or a
    // margin out into the wasteland round an endless map.
    lightmap_origin: vec2<f32>,
    // Subtiles per world unit (5 / 48).
    subtiles_per_unit: f32,
    // 0: draw the canvas as it is (the editor), 1: light it.
    lit: f32,
    // How much of the light the corners lose: 0 is no vignette.
    vignette: f32,
    blink: f32,
    screen_size: vec2<f32>,
}

// Where the vignette starts and ends, as a distance from the middle of the
// view in half-views: 1 is the middle of an edge, sqrt(2) a corner.
const VIGNETTE_INNER: f32 = 0.5;
const VIGNETTE_OUTER: f32 = 1.45;

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

// How much of the light reaches a screen pixel: 1 in the middle of the
// view, 1 - vignette in its corners. Measured across the camera's whole
// viewport, so an ellipse the shape of the window, the same wherever the
// camera is and however the canvas quad is scaled mid-zoom.
fn vignette_at(frag_coord: vec2<f32>) -> f32 {
    let from_middle = frag_coord_to_uv(frag_coord, view.viewport) * 2.0 - 1.0;
    return 1.0 - params.vignette * smoothstep(VIGNETTE_INNER, VIGNETTE_OUTER, length(from_middle));
}

fn canvas_at(uv: vec2<f32>, blurred: bool) -> vec3<f32> {
    let size = params.canvas_size;
    let texel = clamp(floor(uv * size), vec2<f32>(0.0), size - 1.0);
    var albedo = textureLoad(canvas, vec2<i32>(texel), 0).rgb;
    if (blurred) {
        // Explicit bilinear reconstruction only during a blink; the image
        // sampler stays nearest-neighbour for every other rendering path.
        let p = clamp(uv * size - 0.5, vec2<f32>(0.0), size - 1.0);
        let lo = vec2<i32>(floor(p));
        let hi = min(lo + 1, vec2<i32>(size) - 1);
        let f = fract(p);
        let a = textureLoad(canvas, lo, 0).rgb;
        let b = textureLoad(canvas, vec2<i32>(hi.x, lo.y), 0).rgb;
        let c = textureLoad(canvas, vec2<i32>(lo.x, hi.y), 0).rgb;
        let d = textureLoad(canvas, hi, 0).rgb;
        albedo = mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
    }
    if (params.lit == 0.0) {
        return albedo;
    }

    // uv runs down the canvas; world Y runs up.
    let world = params.camera + vec2<f32>(texel.x + 0.5 - size.x * 0.5, size.y * 0.5 - texel.y - 0.5);
    let subtile = (world - params.lightmap_origin) * params.subtiles_per_unit;
    // Off the lightmap is open sky with no lamp in it: the wasteland.
    var baked = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    let inside = all(subtile >= vec2<f32>(0.0)) && all(subtile < params.lightmap_size);
    if (inside) {
        baked = light_at(subtile);
    }
    // As much of the sun as the sky factor lets in, the indoor dark for the
    // rest, and the lamps on top.
    let light = mix(params.indoor.rgb, params.ambient.rgb, baked.a) + baked.rgb;
    // Capped at full brightness: daylight is the canvas exactly as drawn,
    // and a lamp lights the dark rather than bleaching the day. The vignette
    // after the cap, so a lamp at the edge of the view is darkened with it.
    return albedo * min(light, vec3<f32>(1.0));
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let closed = params.blink;
    if (closed >= 1.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let vignette = vignette_at(in.position.xy);
    if (closed <= 0.0) {
        return vec4<f32>(canvas_at(in.uv, false) * vignette, 1.0);
    }

    // Two curved lids meet at the middle: the corners close first, leaving
    // a narrow horizontal opening. A soft edge suggests eyelashes/defocus.
    let p = frag_coord_to_uv(in.position.xy, view.viewport) * 2.0 - 1.0;
    let aperture = (1.0 - closed) * 1.3 - 0.3 * p.x * p.x;
    let opening = 1.0 - smoothstep(aperture - 0.025, aperture + 0.025, abs(p.y));
    if (opening <= 0.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }

    // A small Gaussian kernel defocuses the visible slit as the lids close.
    // Radius is in window pixels, so it feels the same at every zoom level.
    let radius = vec2<f32>(18.0 * closed) / params.screen_size;
    var color = vec3<f32>(0.0);
    for (var y = -1; y <= 1; y += 1) {
        for (var x = -1; x <= 1; x += 1) {
            let weight = select(1.0, 2.0, x == 0) * select(1.0, 2.0, y == 0) / 16.0;
            color += canvas_at(in.uv + vec2<f32>(f32(x), f32(y)) * radius, true) * weight;
        }
    }
    let fade = pow(1.0 - closed, 1.2);
    return vec4<f32>(color * vignette * opening * fade, 1.0);
}
