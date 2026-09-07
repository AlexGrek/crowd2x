---
name: dev
description: Working knowledge for developing crowd2x - which Bevy version and APIs are in play, how assets and animations are wired, what recent Bevy releases changed, and how to keep the simulation fast with thousands of actors. Use before writing or changing game code, adding assets or animations, reaching for a Bevy API you have not verified, or making architecture and performance decisions.
---

# Developing crowd2x

A 2D pixel-art crowd simulation. The art was imported from an earlier prototype
(a different engine, abandoned); the code is new.

## Stack

- **Bevy 0.19.1**, Rust **edition 2024**, toolchain **1.98.1** (Bevy 0.19 needs >= 1.95).
- `default-features = false, features = ["2d", "png", "jpeg"]`. `2d` is Bevy's curated
  umbrella feature — app, platform, winit, sprite rendering, picking — without pbr, gltf,
  audio, anti-alias or solari. Do not switch to default features to get one type; find
  the right feature.
- `rand 0.10` (already in the tree via Bevy, so it costs nothing).
- `bevy/dynamic_linking` is **not usable**: `bevy_dylib` 0.19.1 was never published.
  Do not re-add a `dev` feature for it.

### Verify APIs against the source, not memory

Bevy 0.19 is newer than most training data and moved a lot of things. The vendored source
is the authority and it is already on disk:

```sh
ls ~/.cargo/registry/src/*/bevy_*-0.19.1/src/
```

Grep it before using an API you have not used in this repo. Things that are *not* where
older docs put them:

| Thing | 0.19 location |
| --- | --- |
| `RenderTarget` | `bevy::camera::RenderTarget` — a **Component**, not a `Camera` field |
| `RenderLayers` | `bevy::camera::visibility::RenderLayers` (const `::layer(n)`) |
| `Extent3d`, `TextureUsages`, `TextureFormat` | `bevy::render::render_resource::*` |
| `RenderAssetUsages` | `bevy::asset::RenderAssetUsages` |
| `WindowResized` | `bevy::window::WindowResized`, read with `MessageReader` |
| `Msaa` | in the prelude, a per-camera **Component** |
| `Anchor` | `bevy_sprite`, a separate Component, no longer a `Sprite` field |

Buffered events are `Message` / `MessageReader` / `MessageWriter` since 0.17; `Event` is
reserved for observers (`On<E>`). `AppExit` is a `Message`.

### States run before startup

`StatesPlugin` does `insert_startup_before(PreStartup, StateTransition)`, so the initial
`OnEnter(..)` fires **before every `Startup` system**. An `OnEnter` system therefore cannot
see anything `Startup` spawns or inserts — the camera and `PixelCanvas` from
`render::setup_pipeline` do not exist yet, and a `Res<PixelCanvas>` parameter would panic.

So an `OnEnter` system must not query for the camera or take `Res<PixelCanvas>`. The menu
and the HUD get away with it because `bevy_ui` needs neither: root nodes with no
`UiTargetCamera` bind to the default UI camera later, once one exists.

`DespawnOnExit(State)` (and `DespawnOnEnter`, `DisableOnExit`, ...) come from
`bevy_state::state_scoped` and are in the prelude; they replace hand-written teardown
systems.

## Layout

```
src/main.rs           app + window setup, plugin registration
src/state.rs          AppState - MainMenu / Maps / Editor / Game
src/render.rs         PixelRenderPlugin - the pixel-perfect pipeline, PixelZoom
src/ui.rs             UiPlugin (UiScale), shared colours and font sizes
src/menu.rs           MainMenuPlugin
src/editor/           the map editor
  mod.rs              EditorPlugin, Tool, palettes, cursor overlay, HUD, draw_map
  background.rs       grid-snapped tile layer
  props.rs            free-placed, Y-sorted object layer
src/browser.rs        BrowserPlugin - the saved-maps screen
src/game.rs           GamePlugin - playing a map: camera, zoom, clamp
src/characters/       humans and dogs
  mod.rs              CharacterPlugin, CELL, depth_for()
  human.rs            layered paperdoll
  dog.rs              animated sprite + facing sheets
src/animation.rs      FrameAnimation, atlas frame stepping
src/debug.rs          screenshot/smoke harness (see the `debugger` skill)
tools/                check_pixel_grid.py
assets/               imported wholesale from an earlier prototype
```

## Interface

The menu and the editor HUD are stock `bevy_ui` — `Node` layout, `Button`, `Interaction`.
Build new interface out of those rather than hand-placed sprites or `Text2d`.

Two things about how it is wired here:

- **UI is on the upscale camera, not the world camera.** That is forced, not a
  preference: `bevy_ui`'s focus system only computes a cursor position for cameras whose
  render target is a *window* (`bevy_ui/src/focus.rs`), and the world camera renders to an
  off-screen image, so `Interaction` would never fire there. The upscale camera is the only
  one targeting the window, so root nodes bind to it automatically; it also carries
  `IsDefaultUiCamera` to say so.
- **`UiScale` is `PIXEL_SCALE`.** Every `Val::Px` and font size is therefore in *canvas*
  pixels, the same units the world uses. The canvas is 320x180, so UI sizes are small
  numbers — `ui::FONT_BODY` is 6.

The consequence is that UI is composited at window resolution over the finished upscale,
so it is smooth rather than pixelated and it is **not** part of the pixel grid. Capture
with `CROWD2X_HIDE_UI=1` before running `tools/check_pixel_grid.py` (see the `debugger`
skill).

## Assets

Layout is preserved from the earlier prototype so porting is mechanical:

| Path | Contents |
| --- | --- |
| `assets/*.png` | sprites and strips, each at its true resolution (16px or 48px) |
| `assets/32/` | 32px tile variants |
| `assets/human/` | paperdoll layers: `human_base`, `clothes_*`, `eyes_*`, `hair_*` |
| `assets/concept/` | 512px AI concept art + Pixelorama source — reference only, never loaded |

Rules:

- **Everything is nearest-filtered.** `ImagePlugin::default_nearest()` in `main.rs` is
  load-bearing. Never set a per-image linear sampler.
- **48x48 is the character cell** (`characters::CELL`), but most art is *drawn* at 16x16
  (`characters::ART`) and upscaled by the game. Animation strips are horizontal:
  `dog_idle.png` is 64x16 = 4 frames.
- **Never commit a pre-upscaled PNG.** Store the true resolution and let the game scale
  it by a whole number (`characters::upscale`, X and Y only — Z is depth). A baked-in
  upscale takes the choice away from the game and disguises 16px art as 48px art.
  `python3 tools/art_scale.py report assets` lists any file that is secretly an upscale;
  `shrink <src> <dst>` reduces one and refuses if the reduction would lose a pixel.
  Mixed resolutions are real here: beds and walls genuinely are 48x48.
- **Paths are relative to `assets/`**: `assets.load("human/hair_blue.png")`.
- Tiled maps (`.tmx`) were deliberately not imported — level data, not art, and the map
  format is undecided.

### Adding a paperdoll layer

Drop a 16x16 PNG in `assets/human/` aligned to the same body, then add its path to the
matching array in `src/characters/human.rs` (`CLOTHES`, `EYES`, `HAIR`). That is all —
`Look::random` picks from the arrays and `spawn` stacks them as child sprites with the
`LAYER_*` depth offsets.

### Adding an animation

1. Author a horizontal strip of `ART`-sized (16px) frames.
2. Build a layout: `TextureAtlasLayout::from_grid(UVec2::splat(ART), frames, 1, None, None)`
   — the grid is in source texels, not the on-screen cell size.
3. Spawn with `Sprite::from_atlas_image(image, TextureAtlas { layout, index: 0 })`.
4. Add `FrameAnimation::new(first, last, seconds_per_frame)` from `src/animation.rs`.

Share one `TextureAtlasLayout` handle across every sprite using the same grid — see
`dog::DogSheets`. Directional art uses a second sheet (`dog_idle_reversed.png`) rather
than `flip_x`, so left-facing frames can be hand-tuned later; `dog::apply_facing` swaps
`sprite.image` on `Changed<Facing>`.

## The pixel pipeline

World camera (`RenderLayers` 0) renders into an off-screen `Image` sized
`window_physical / PIXEL_SCALE`; upscale camera (`RenderLayers` 1) draws that image as
one sprite scaled by `PIXEL_SCALE`. Three things keep it exact and are easy to break:

- `with_scale_factor_override(1.0)` on the window — logical == physical pixels, so the
  factor is really 4 on HiDPI.
- The world camera `Transform` is snapped to whole pixels every frame; `CameraPan` holds
  the sub-pixel position.
- Nothing in the world may be scaled by a non-integer or placed at a fractional position.

Anything new that renders must carry `WORLD_LAYER`, or it will not appear.
After touching any of this, verify with `tools/check_pixel_grid.py` (see `debugger` skill).

## What recent Bevy versions brought

Useful context for what is available; check the source before relying on any of it.

**0.19 (June 2026) — what we are on**
- **BSN / next-gen scenes**: the `bsn!` macro (`bevy_scene`) for composable, patchable,
  dependency-aware scene definitions. Worth considering for spawning character archetypes
  instead of hand-written `spawn` functions.
- **Resources are Components**: `pub trait Resource: Component {}`. `#[derive(Resource)]`
  implies `Component`. Resources live on dedicated abstract entities.
- More rendering work moved to the GPU; contact shadows (3D, not our concern).
- Official app settings framework.
- Text moved from `cosmic-text` to `parley`, with `EditableText` and better font families.

**0.18 (January 2026)**
- **Cargo feature collections** — the `2d` / `3d` / `ui` scenario features this project uses.
- First-party fly and pan camera controllers (`bevy_camera_controller`).
- Procedural atmosphere / `ScatteringMedium`; UI directional navigation; fullscreen
  materials; font variations.

**0.17 (September 2025)**
- **Events split**: `Message` for buffered data, `Event` reserved for observer triggers.
  This is the single most common source of stale-example breakage.
- Experimental Solari raytracing; Rust hotpatching (Dioxus-powered); headless UI +
  Bevy Feathers; DLSS.

Bevy does not promise compatibility across point releases. On any upgrade, read the
official migration guide and re-grep the vendored source rather than guessing.

## Scaling to thousands of actors

An earlier prototype died at ~100 dogs partly on architecture. Do not repeat it. Its actual mistakes,
all reproducible in Bevy if you are careless:

- `Arc<Mutex<HashMap<Entity, Handle>>>` for carriables, interactives and messaging — one
  global lock serialising every agent.
- `update_human_looks` ran a full `world.query::<(&OfficeWorker, &Transform)>()` **inside
  a loop over every look part**: O(parts x workers) every frame.
- Per-frame A* per agent with no budget or caching.

### Keep the simulation out of Bevy

Put the actual simulation in plain Rust modules with **no `bevy::` imports** — grid,
pathfinding, agent state, decision-making. The Bevy layer stays a thin adapter that reads
sim state and updates `Transform` and `Sprite`.

This is not purism, it buys three concrete things: the sim is testable with `cargo test`
and no `App`; it can be parallelised on its own terms instead of through the ECS
scheduler; and when Bevy makes another breaking release, the breakage is confined to the
adapter. The earlier prototype died partly because its logic was welded to its game engine's API.

Corollary: an actor is not required to be an entity. Entities are for *things that draw*.
Ten thousand agents can live in dense arrays with a few hundred sprites spawned for the
visible ones.

### Data layout

- Structure of arrays indexed by a dense `AgentId(u32)`: `Vec<Ps>`, `Vec<Brain>`,
  `Vec<Intent>` — not `Vec<Agent>` of fat structs, and never `HashMap<Entity, T>` in a
  hot loop. Hashing per agent per frame dwarfs the actual work.
- Stable ids across despawns: generational indices (`slotmap`) or a free list.
- Spatial queries go through a uniform grid of cell buckets (`Vec<Vec<AgentId>>` or a
  CSR-style flat array rebuilt each tick), never an O(n^2) scan. The earlier prototype's
  vision code already scanned a box per agent per frame; at 10k agents that is the whole budget.

### Shape of a tick

The pattern that removes locks entirely, rather than making them cheaper:

1. **Think** — read-only over the previous state, fully parallel, writes only into
   per-thread output buffers. No shared mutation, so no synchronisation at all.
2. **Apply** — single-threaded, drains those buffers in a deterministic order and mutates
   the world. Small and cache-friendly if think did the expensive part.
3. **Swap** — double-buffered state: read from A, write to B, swap. Also gives you
   deterministic simulation and trivial rollback.

Determinism is worth protecting: iterate in id order, not hash order, and keep RNG
per-agent and seeded.

### Parallelism in Bevy specifically

- Bevy runs systems in parallel when their data access is disjoint. **Every `ResMut<T>`
  is an exclusive lock on the schedule**: two systems touching the same `ResMut` can never
  overlap, no matter what is inside it. One big `ResMut<World>`-ish resource serialises
  your whole frame.
- Prefer many narrow queries over few wide ones; disjointness is what buys parallelism.
- `query.par_iter_mut()` for large uniform per-entity work.
- Do not nest rayon inside Bevy's task pool — pick one executor. If the sim core uses
  rayon, run it from a single Bevy system and let Bevy's pool sit idle during it, or use
  `ComputeTaskPool` throughout instead of adding rayon.
- `Commands::spawn_batch` for bulk spawning; per-entity `spawn` in a loop allocates
  command buffer entries one at a time.
- `Changed<T>` filters are cheap and skip work — `dog::apply_facing` already uses one.

### Lock-free toolbox, honestly ranked

Reach for these in this order. The top of the list is almost always the right answer, and
atomics-everywhere is usually slower than a clean double buffer.

1. **Double buffering + per-thread output vectors.** No atomics, no contention, cache
   friendly, deterministic. Default choice.
2. **`crossbeam-channel` / `crossbeam-queue`** (`SegQueue`, `ArrayQueue`) for agent ->
   world intents when producers are genuinely concurrent. Both are already in the
   dependency tree via Bevy, so they cost no compile time.
3. **Plain atomics** (`AtomicU32`) for counters and stats. Keep them off the hot path;
   a contended atomic is a cache-line ping-pong.
4. **`arc-swap`** for read-mostly snapshots — config, a published world snapshot readers
   grab per tick.
5. **`dashmap`** only when a concurrent map is genuinely unavoidable. Sharded arrays
   indexed by id beat it nearly every time.

A `Mutex` around a small, briefly-held, rarely-contended thing is fine. A `Mutex` around
the world state is what killed the earlier prototype.

### Rendering cost

Sprites batch **by texture**. A thousand actors drawn from one atlas is a handful of draw
calls; the same thousand drawn from separate PNGs is a thousand. This matters here right
now: each paperdoll layer currently loads its own PNG, so every human costs up to four
texture switches. Before the crowd grows, pack the character layers into a single atlas
and address them by index.

Also: pool and reuse sprite entities instead of spawning and despawning as agents enter
and leave view, and only spawn sprites for agents actually on the canvas — the canvas is
320x180, so the visible set is small no matter how big the simulation gets.

### Measure before optimising

Add `FrameTimeDiagnosticsPlugin` and look, or build with Bevy's `trace_tracy` feature.
Guessing which of a dozen systems is the problem is exactly how the O(n^2) query in
the earlier prototype survived to the last commit.
