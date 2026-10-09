// The sky factor's propagation: how far the sun reaches under a ceiling.
//
// `sky_init` once, then `sky_step` SKY_ITERATIONS times, ping-ponging two
// buffers. Kept line for line with `LightScene::sky_field` / `sky_step` in
// src/lighting/scene.rs, which reproduces it to the bit: only subtraction and
// max, both correctly rounded, and the step costs come from the CPU in the
// uniform rather than as WGSL constants (which are evaluated at a higher
// precision and could round differently).

struct SkyParams {
    size: vec2<u32>,      // lightmap size, subtiles
    map_width: u32,       // cells, for the ceiling bits
    subtiles: u32,        // per cell
    step: f32,            // what an orthogonal step costs
    diagonal: f32,        // and a diagonal one
    _pad: vec2<f32>,
}

@group(0) @binding(0) var<uniform> params: SkyParams;
@group(0) @binding(1) var<storage, read> occlusion: array<u32>;
@group(0) @binding(2) var<storage, read> ceiling: array<u32>;
@group(0) @binding(3) var<storage, read> src: array<f32>;
@group(0) @binding(4) var<storage, read_write> dst: array<f32>;

fn is_opaque(x: i32, y: i32) -> bool {
    if (x < 0 || y < 0 || x >= i32(params.size.x) || y >= i32(params.size.y)) {
        return true;
    }
    let i = u32(y) * params.size.x + u32(x);
    return (occlusion[i / 32u] & (1u << (i % 32u))) != 0u;
}

fn is_roofed(x: i32, y: i32) -> bool {
    let cell = u32(y / i32(params.subtiles)) * params.map_width + u32(x / i32(params.subtiles));
    return (ceiling[cell / 32u] & (1u << (cell % 32u))) != 0u;
}

@compute @workgroup_size(8, 8, 1)
fn sky_init(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= params.size.x || id.y >= params.size.y) {
        return;
    }
    let i = id.y * params.size.x + id.x;
    dst[i] = select(1.0, 0.0, is_roofed(i32(id.x), i32(id.y)));
}

@compute @workgroup_size(8, 8, 1)
fn sky_step(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= params.size.x || id.y >= params.size.y) {
        return;
    }
    let x = i32(id.x);
    let y = i32(id.y);
    let w = i32(params.size.x);
    let i = id.y * params.size.x + id.x;
    if (!is_roofed(x, y)) {
        dst[i] = 1.0;
        return;
    }
    var best = src[i];
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            if (dx == 0 && dy == 0) {
                continue;
            }
            let nx = x + dx;
            let ny = y + dy;
            // Off the map is opaque, so this covers the edge too.
            if (is_opaque(nx, ny)) {
                continue;
            }
            var cost = params.step;
            if (dx != 0 && dy != 0) {
                if (is_opaque(x + dx, y) && is_opaque(x, y + dy)) {
                    continue;
                }
                cost = params.diagonal;
            }
            best = max(best, src[ny * w + nx] - cost);
        }
    }
    dst[i] = max(best, 0.0);
}
