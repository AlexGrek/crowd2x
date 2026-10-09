// The lightmap bake: one invocation per subtile of a dirty chunk.
//
// Kept line for line with `LightScene::evaluate` in src/lighting/scene.rs,
// which is the reference this is checked against. Change both or neither.

struct Params {
    size: vec2<u32>,      // lightmap size, subtiles
    chunks_x: u32,
    chunk: u32,           // chunk side, subtiles
}

struct Light {
    pos: vec2<f32>,
    radius: f32,
    enabled: f32,
    color: vec4<f32>,
}

// Crossing times closer than this are one corner crossing; see `CORNER`
// in scene.rs.
const CORNER: f32 = 1e-5;

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> occlusion: array<u32>;
@group(0) @binding(2) var<storage, read> lights: array<Light>;
@group(0) @binding(3) var<storage, read> chunk_offsets: array<u32>;
@group(0) @binding(4) var<storage, read> chunk_lights: array<u32>;
@group(0) @binding(5) var<storage, read> dirty: array<u32>;
// The propagated sky, from `sky.wgsl`: squared into alpha.
@group(0) @binding(6) var<storage, read> sky: array<f32>;
@group(0) @binding(7) var lightmap: texture_storage_2d<rgba16float, write>;

fn is_opaque(x: i32, y: i32) -> bool {
    if (x < 0 || y < 0 || x >= i32(params.size.x) || y >= i32(params.size.y)) {
        return true;
    }
    let i = u32(y) * params.size.x + u32(x);
    return (occlusion[i / 32u] & (1u << (i % 32u))) != 0u;
}

// Amanatides-Woo over the subtile grid, from the light (src) to the
// target subtile's centre (dst). Neither end's own subtile is tested. A
// corner crossing steps diagonally and is blocked only between two opaque
// subtiles.
fn visible(src: vec2<f32>, tx: i32, ty: i32) -> bool {
    let dst = vec2<f32>(f32(tx) + 0.5, f32(ty) + 0.5);
    var cx = i32(floor(src.x));
    var cy = i32(floor(src.y));
    let d = dst - src;
    let step_x = select(-1, 1, d.x > 0.0);
    let step_y = select(-1, 1, d.y > 0.0);
    let inv_x = select(1e30, 1.0 / abs(d.x), d.x != 0.0);
    let inv_y = select(1e30, 1.0 / abs(d.y), d.y != 0.0);
    let first_x = select(src.x - f32(cx), f32(cx) + 1.0 - src.x, d.x > 0.0);
    let first_y = select(src.y - f32(cy), f32(cy) + 1.0 - src.y, d.y > 0.0);
    var t_x = first_x * inv_x;
    var t_y = first_y * inv_y;
    let steps = abs(tx - cx) + abs(ty - cy);
    for (var i = 0; i < steps; i++) {
        if (abs(t_x - t_y) < CORNER) {
            if (is_opaque(cx + step_x, cy) && is_opaque(cx, cy + step_y)) {
                return false;
            }
            cx += step_x;
            cy += step_y;
            t_x += inv_x;
            t_y += inv_y;
        } else if (t_x < t_y) {
            cx += step_x;
            t_x += inv_x;
        } else {
            cy += step_y;
            t_y += inv_y;
        }
        if (cx == tx && cy == ty) {
            return true;
        }
        if (is_opaque(cx, cy)) {
            return false;
        }
    }
    return true;
}

@compute @workgroup_size(8, 8, 1)
fn bake(
    @builtin(workgroup_id) group: vec3<u32>,
    @builtin(local_invocation_id) local: vec3<u32>,
) {
    let chunk = dirty[group.z];
    let origin = vec2<u32>(chunk % params.chunks_x, chunk / params.chunks_x) * params.chunk;
    let texel = origin + group.xy * 8u + local.xy;
    if (texel.x >= params.size.x || texel.y >= params.size.y) {
        return;
    }

    let centre = vec2<f32>(texel) + 0.5;
    var sum = vec3<f32>(0.0);
    for (var k = chunk_offsets[chunk]; k < chunk_offsets[chunk + 1u]; k++) {
        let light = lights[chunk_lights[k]];
        if (light.enabled == 0.0) {
            continue;
        }
        let delta = centre - light.pos;
        let d2 = dot(delta, delta);
        let r2 = light.radius * light.radius;
        if (d2 >= r2) {
            continue;
        }
        if (!visible(light.pos, i32(texel.x), i32(texel.y))) {
            continue;
        }
        let f = 1.0 - d2 / r2;
        sum += light.color.rgb * (f * f);
    }
    let s = sky[texel.y * params.size.x + texel.x];
    textureStore(lightmap, vec2<i32>(texel), vec4<f32>(sum, s * s));
}
