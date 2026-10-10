---
name: lighting
description: The crowd2x lightmap - day/night ambient and indoor light, the RGB lightmap at five subtiles a cell baked by compute shaders, emitters (ceiling and tube lamps, fire, computers in use), occlusion, the sky field propagated through doorways, dirty chunks, the GPU/CPU reference check and its measured cost. Use when touching src/lighting/, assets/shaders/lighting.wgsl, sky.wgsl or lit_canvas.wgsl, adding a light source, or when something is lit wrong, too dark or a light does not switch.
---

# Lighting

An RGB lightmap at **five subtiles a cell** (`scene::SUBTILES`), computed on the GPU by a
compute shader (`assets/shaders/lighting.wgsl`), **only where something changed**. Three
parts, each a step further from the simulation:

- **`scene.rs` — plain Rust, no `bevy::`.** What the lightmap is computed *from*: one
  occlusion bit per subtile (from `Map::sight`, so walls block light and furniture does
  not), one ceiling bit per **cell** (`Map::has_ceiling`), the lights (`EMITTERS`, a
  catalogue keyed by object name like `PROPS`, read from the `Props` *and* `Lamps` layers —
  `"ceiling lamp"`, `"tube lamp"` and `"fire"` always on, `"computer"` only while somebody
  is using it — and a lamp or a computer with no power is not in the scene at all:
  `LightScene::from_map` takes the map's `Supply`), and a CSR index from each
  `CHUNK` (40 subtiles, 8 cells) to the lights that can reach it. Switching a light dirties
  the chunks under its radius and no others. It also holds **the CPU reference,
  `LightScene::evaluate`**, which the shader is kept line for line with and checked against.
  The `ambient` curve (the sun: white by day, so a street by day looks exactly as drawn;
  dusk; a dark blue night from 21:00 to 05:00) and `indoor` (what is left under a roof the
  sky does not reach: a little of the sun and a floor, dark but legible) live here too.
- **`mod.rs` — the main world.** Builds a `Lighting` from the simulated map on the game
  screen, switches computer lights by asking `Occupancy` about each computer's own cell and
  the four beside it — *not* the on-screen crowd `game/props.rs` asks, since a screen's
  light reaches past its picture and the lightmap must not depend on the camera — and
  hands each frame's dirty chunks over in a `LightUpload` that the extract system *takes*,
  so each change crosses to the render world once. A frame with no change uploads and
  dispatches nothing.
- **`gpu.rs` — the render world.** Buffers, the sky's propagation passes (once per scene,
  below), and one bake dispatch of `(5, 5, dirty chunks)` workgroups, all in one compute
  pass in `RenderGraph`'s `Render` set **before `camera_driver`** — so a frame is
  drawn with what was baked for it, and so its GPU timestamp is recorded (in `Begin` it
  races `begin_diagnostics_frame` and the span is dropped). Bevy 0.19 has no render graph
  of nodes: a global compute pass is a system in the `RenderGraph` *schedule* taking
  `RenderContext`. Dirty chunks wait here until the pipeline has compiled.

The algorithm is **direct visibility**: a texel is lit by a light when the line between
them crosses no opaque subtile (Amanatides-Woo over the subtile grid; the texel's own subtile
may be opaque, which is how a wall's face is lit one subtile deep). **A line through a
subtile corner** (crossing times within `CORNER`) steps diagonally and is stopped only
between two opaque subtiles: lights on prop centres make exact corners common, and leaving
the tie to rounding made the CPU and the GPU pick different sides of a lone wall corner.
Attenuated by
`(1 - d²/r²)²`, summed in `f32` and stored as `rgba16float` so overlapping lights do not
saturate before they are drawn. The cap at full brightness is in the canvas shader.

**The sky is the lightmap's alpha** — how much of the sun reaches a texel, independent of
the time of day, so the sun moves all day without a re-bake and the canvas multiplies it in.
1 under open sky; under a ceiling it is **propagated over the grid** (`assets/shaders/sky.wgsl`,
reference `LightScene::sky_field`): from every roofless subtile it flows through anything
not opaque in eight directions, losing `SKY_STEP` (1/30) a step — √2 that diagonally, never
between two opaque subtiles meeting at a corner — so it comes in by a doorway, turns
corners softly and is gone six cells in; alpha is that field squared, for a soft shoulder.
It is `SKY_ITERATIONS` (30) ping-pong passes over the whole map, run once when a scene is
first baked, and reproduced to the bit on the CPU (only subtraction and `max`, both
correctly rounded; the step costs reach the shader in a uniform, not as WGSL constants,
which are evaluated at higher precision). **Rays were tried first and discarded**: sixteen
of them miss a doorway two cells away between two rays and a room comes out in streaks.
The ceiling is never drawn in the game — the view is from above — only lit by.

**How it is checked:** `LightProbe` reads the lightmap back once the scene's latest
`generation` has been baked (`Lighting::is_baked`) and compares every texel with the
reference — all four channels, sky included — `expect_lighting` in a QA script,
`CROWD2X_LIGHT_CHECK=1` by hand. It agrees to within f16 rounding (worst 0.001) on every map
tried, 1.6M texels included, and a shader deliberately broken fails it. **Measured**
(`CROWD2X_LIGHT_STRESS=1` re-bakes everything — and re-propagates the sky — every frame and
logs GPU timestamps), on a 256x256 map (1280x1280 texels, 1024 chunks), RTX 5070 Ti: a
full bake with 2000 fires is **0.38 ms of GPU** and ~4 µs of CPU; with half the map roofed
and 128 lamps more, bake plus sky is **1.15 ms** — the sky ~0.8 ms of it, paid once per
scene (`qa/perf_lighting.json`, `qa/perf_lighting_roofed.json`).

Not done yet: lights are never added or moved during play (that would rebuild the chunk
index — `rebuild_index` — which is built for it; the ceiling cannot change during play
either, which would re-propagate the sky), and a moving
light would want a dynamic overlay rather than chunk re-bakes. The z dimension of one
dispatch caps a frame at 65535 dirty chunks, a 4096x4096-cell map's worth.
