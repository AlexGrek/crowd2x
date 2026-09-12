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
| `HashMap`, `HashSet` | `bevy::platform::collections::*` — not in the prelude |
| Condition combinator | `.and_then(cond)` / `.and_eager(cond)`; plain `.and(..)` is deprecated |

Also: two parameters of one system claiming the same resource — a `Res<T>` in one
`SystemParam` struct and a `ResMut<T>` in another — is a runtime panic (`B0002`), not a
compile error. It shows up the first time that system runs, which for a screen-specific
system can be well into a test run.

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
src/ui/               UiPlugin (UiScale), shared widgets
  nav.rs              Focus/Focusable/Scope - one highlight, three devices
  keyboard.rs         the on-screen keyboard and TextEntry
src/menu.rs           MainMenuPlugin
src/editor/           the map editor
  mod.rs              EditorPlugin, Tool, palettes, cursor overlay, HUD, draw_map
  background.rs       grid-snapped tile layer
  props.rs            free-placed, Y-sorted object layer
src/browser.rs        BrowserPlugin - the saved-maps screen
src/game/             GamePlugin - playing a map
  mod.rs              camera, zoom, clamp, HUD
  actors.rs           the sim-to-sprite bridge: Sim, SimInput, tick, sync
  logview.rs          drains the simulation's log onto the screen
src/map/              the map + coordinates - PLAIN RUST, no bevy
src/sim/              GameState + process_game_state - PLAIN RUST, no bevy
  uid.rs              Uid (type byte + 56 random bits), EntityType
  entity.rs           GameEntity, Body, Think
  kinds.rs            Human, Dog, the wander behaviour
  entities.rs         the dict-over-arena store
  log.rs              bounded lock-free line queue
src/characters/       how humans and dogs are drawn
  mod.rs              CharacterPlugin, CELL, depth_for()
  human.rs            layered paperdoll
  dog.rs              animated sprite + facing sheets
src/animation.rs      FrameAnimation, atlas frame stepping
src/qa/               scripted QA (see the `qa` skill)
src/awake.rs          macOS: hold the display awake for a capture
src/debug.rs          screenshot/smoke harness (see the `debugger` skill)
tools/                check_pixel_grid.py, art_scale.py, qa.py
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
| `assets/*.png` | sprites and strips, each at its true resolution — 16px, bar the exceptions below |
| `assets/32/` | 32px tile variants: 2x upscales, kept as the source the 16px tiles were reduced from |
| `assets/human/` | paperdoll layers: `human_base`, `clothes_*`, `eyes_*`, `hair_*` |
| `assets/concept/` | 512px AI concept art + Pixelorama source — reference only, never loaded |

Rules:

- **Everything is nearest-filtered.** `ImagePlugin::default_nearest()` in `main.rs` is
  load-bearing. Never set a per-image linear sampler.
- **48x48 is the character cell** (`characters::CELL`), but art is *drawn* at 16x16
  (`characters::ART`) and upscaled 3x by the game. Animation strips are horizontal:
  `dog_idle.png` is 64x16 = 4 frames.
- **Never commit a pre-upscaled PNG.** Store the true resolution and let the game scale
  it by a whole number (`characters::upscale`, X and Y only — Z is depth). A baked-in
  upscale takes the choice away from the game and disguises 16px art as 48px art.
  `python3 tools/art_scale.py report assets` lists any file that is secretly an upscale;
  `shrink <src> <dst>` reduces one and refuses if the reduction would lose a pixel.
- **16x16 is the intended resolution for everything**, and the palette is nearly there:
  every tile but `floor` and `floor_diag` is 16x16, and of the props only the beds, the
  toilet, `trash_can48` and `fire_static` are not. Those are the imported art that is
  *genuinely* drawn at 48x48 — reducing them is lossy and mangles them, so they are art
  debt waiting to be redrawn at 16x16, not a resolution the project wants to keep. Until
  then `PaletteItem::new` exists for them and `PaletteItem::upscaled` for everything else;
  new art should always be the latter.
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

### The GameState contract

The rule above has an address. The simulation is `src/sim/`, and its whole surface is:

```rust
pub struct GameState {
    pub map: Map, entities: Entities, occupancy: Occupancy, pub log: Log, /* ... */
}

pub fn process_game_state(state: &mut GameState, dt: f32, input: &Input);  // both passes
pub fn spawn_pass(state: &mut GameState, input: &Input);                   // writes the table
pub fn process_pass(state: &mut GameState, dt: f32);                       // must not
```

One super-object, one entry point. Bevy renders it, reads the devices, runs the shaders and
draws the UI; it decides nothing. `src/game/actors.rs` is the entire bridge and is worth
reading before adding to either side.

**Do not** add a Bevy system that decides something, a `Component` holding agent state, or
a second function that advances the world. Add an entity kind (`sim/kinds.rs`), an
`Intent` variant, or a `Command`.

#### Which pass does your code belong in

This is the first question to answer, and mostly the compiler answers it for you.

| | writes the entity table | allocates | parallelisable |
| --- | --- | --- | --- |
| `spawn_pass` | **yes** — the only thing that does | yes | no |
| `process_pass` | **no** | no | that is what it is for |

`process_pass` is handed a `FrozenEntities`: the table with its membership frozen. It
derefs to `&Entities` so every read works unchanged, and it deliberately has **no
`DerefMut`**, so `insert` and `remove` — which need `&mut Entities` — are simply not
reachable. Anything that changes *which* entities exist has to go through a `Command` and
land in the spawn pass. An entity that wants to remove itself returns something the *next*
spawn pass acts on; there is no back door, and that is the feature.

Three things depend on the table holding still for the whole of a tick, which is why this
is a type and not a comment:

- The intent buffer is indexed by `Slot` and sized once, in the spawn pass. An insert
  mid-tick grows the arena past the buffer.
- A `Vec` that grows moves its contents, invalidating every index and reference into it.
- Structural mutation is the one thing that genuinely cannot be parallelised. Two threads
  moving different entities never conflict; two threads pushing to one `Vec` always do.

The payoff is concrete: `process_pass` touches no allocator at all, and it is the pass that
can be handed to a thread pool without changing shape.

#### Why `&mut` rather than `-> GameState`

Returning a fresh state is the clean semantics and the wrong implementation: it copies the
map, the log queue and the id allocator sixty times a second to express a change that
touched only the entities. What the functional signature is actually *for* — a tick being
a pure function of the previous state — is delivered by the pass split instead:

1. **Spawn pass** — spawns and despawns, before anything thinks, so a new entity thinks
   on the tick it arrived.
2. **Processing pass, think** — read-only over entities and map; each returns an `Intent`
   into a slot-indexed buffer.
3. **Processing pass, move** — single-threaded, ascending slot order, drains the buffer.
4. **Processing pass, react** — each entity is handed what became of its own move.

**The intent buffer is the double buffer.** Think reads entities and writes intents; move
reads intents and writes entities. Neither touches the same memory in the same direction,
which is what makes think safe to `par_iter` later, and it clones no entity to get there.
The think loop is written in that shape already — keep it that way.

#### Think proposes, move disposes, react answers

An `Intent` is a **request**, not an outcome. The move step is the single place it is
granted or refused, and it checks two layers in order: the terrain (`Map::is_passable`)
and then the crowd (`sim::Occupancy`, one entity to a cell, keyed on the centre cell). A
refused move is **cancelled outright** — the entity is exactly where it started, not
partway and not slid along the obstacle — and the reason lands in a second slot-indexed
buffer as a `MoveOutcome::Blocked { by: Option<Uid> }`, naming the entity in the way or
`None` for the map itself.

Two rules to keep, because both are load-bearing:

- **The move step stays sequential.** It writes `Occupancy`, so two entities heading for
  one empty cell on the same tick is the race it exists to settle; ascending slot order
  settles it identically on every run, which is where the determinism test would break
  first. Do not `par_iter` this one.
- **Nothing decides a position anywhere else.** A kind that wants to refuse a step should
  not return an `Intent` it has pre-vetted against the world — think may run in parallel
  and cannot see the crowd mid-tick. Aim, and let the move step say no.

One entity skips all of this: a **frozen** one. `Command::Freeze` arrives on the same queue
as a spawn and is applied in the spawn pass, and the think step then hands that entity an
`Intent::Idle` rather than asking it what it wants — a decision nothing would carry out is
work with nowhere to go. It is a debugging handle (the unit panel's `life` menu), it keeps
the entity's cell and goal, and it lives on `Body` because "stop moving" means the same
thing for everything that has a position.

React is the second thinking round, after the *whole* crowd has moved: an entity revises
its plan, writing only to itself (`GameEntity::react`). Reacting inside the move loop
would mean answering a world that is halfway through the tick. Today a blocked `Walker`
just drops its goal — steering, waiting, queueing and pathfinding are each a change to
`Walker::react` and nowhere else.

`Occupancy` is exactly the dynamic layer `map::PassabilityMap`'s docs reserve a space for,
and it lives in `sim/` because who is standing where is simulation state. Movement never
creates an overlap; **spawning can**, because an entity is placed where it was asked for —
so `release` only clears a cell whose occupant is the entity leaving it.

#### Determinism is a test, not an aspiration

`the_same_seed_and_the_same_input_give_the_same_world` runs two worlds for a thousand ticks
and compares every position. It holds because the per-entity RNG is seeded from
`mix(uid ^ mix(tick))` rather than shared, ids are minted from a seeded `SmallRng`, and
iteration is in slot order. Anything that reaches for `rand::rng()` inside the tick breaks
it, and the breakage is a bug that reproduces once and never again.

#### `Uid`, and why not `Entity`

64 bits: top byte `EntityType`, low 56 random and unique-by-retry. Bevy's `Entity` is an
index into a `World` — it cannot be written to a save file, quoted in a log, or compared
across a reload, and most actors will not have one at all once sprites are culled to the
canvas. `Uid::kind()` reads the type straight off the id, so a log line says what something
is with no lookup into a state that may already have dropped it. An unknown top byte is
`None`, not a panic, for the same reason an unknown `TerrainId` is.

#### The store is a dict over an arena

`Entities` presents `get(uid)` / `insert` / `remove` / `iter` over a `Vec<Option<Box<dyn
GameEntity>>>` plus a `HashMap<Uid, Slot>` index. Look-up by id goes through the map; the
tick walks the `Vec` and hashes nothing. Removal **tombstones** rather than
`swap_remove`-ing: the intent buffer is indexed by slot, and reordering the tail on a
despawn would both misfile intents and make iteration order depend on history.

This is the compromise position between the dict the game wants to talk to and the SoA the
next section asks for. When entity counts make `Box<dyn GameEntity>` the bottleneck — it
will be pointer-chasing and vtable dispatch, not the `HashMap` — the move is to columns
inside `Entities` with the same public API, and callers do not change.

#### Driving the passes separately

`process_game_state` is the two passes in order. A caller that wants a world built once and
then run calls `spawn_pass` once and `process_pass` N times, which applies pending spawns
exactly once however many ticks were asked for. Going through `process_game_state` for that
needs the commands handed to the first tick and an empty `Input` to every one after it,
which is the sort of thing that is wrong the second time somebody writes it.

Two callers do exactly that, and for the same reason. The QA harness's `tick` step runs
exactly the number of processing passes it was asked for, whatever the game speed is.
And `game::actors::tick_sim` runs **one spawn pass every fixed step and `GameSpeed::steps()`
processing passes** — so speed is how many ticks happen and never how big one is, a pause
is zero of them, and something spawned into a paused world still appears. Scaling `dt`
instead would change what the simulation does rather than only when it does it.

#### One `ResMut<Sim>`

`game::actors::tick_sim` is the only system in the game that takes the world mutably. Every
`ResMut<T>` is an exclusive lock on the schedule, so a second writer serialises the frame
around it for nothing. Things that want to *ask* take `ResMut<SimInput>` and push a
`Command`. (The QA harness's `tick` step is the deliberate exception; it lives in a
different schedule, so the two never contend.)

#### Cell units in, pixels out

`position` is `(f32, f32)` in **cells** — `(3.5, 2.5)` is the middle of cell `(3, 2)` — and
`center_position()` is `position.floor()`, derived so they cannot disagree. `CELL = 48` is a
fact about the art and stays in `actors.rs`, which multiplies and rounds. Same line for
appearance: the simulation hands out an `appearance_seed`, and `characters/` decides which
PNGs that is.

### Data layout

- Structure of arrays indexed by a dense `AgentId(u32)`: `Vec<Ps>`, `Vec<Brain>`,
  `Vec<Intent>` — not `Vec<Agent>` of fat structs, and never `HashMap<Entity, T>` in a
  hot loop. Hashing per agent per frame dwarfs the actual work.
- Stable ids across despawns: generational indices (`slotmap`) or a free list.
- Spatial queries go through a uniform grid of cell buckets (`Vec<Vec<AgentId>>` or a
  CSR-style flat array rebuilt each tick), never an O(n^2) scan. The earlier prototype's
  vision code already scanned a box per agent per frame; at 10k agents that is the whole budget.

### Shape of a tick

`src/sim/` already does this; what follows is why, and what to preserve when changing it.

The pattern that removes locks entirely, rather than making them cheaper:

1. **Think** — read-only over the previous state, fully parallel, writes only into
   per-thread output buffers. No shared mutation, so no synchronisation at all.
2. **Apply** — single-threaded, drains those buffers in a deterministic order and mutates
   the world. Small and cache-friendly if think did the expensive part.
3. **Swap** — double-buffered state: read from A, write to B, swap. Also gives you
   deterministic simulation and trivial rollback. In `src/sim/` the *intent buffer* plays
   this role: there is no separate copy of the world, because think and the move step
   (which is what "apply" is called there) already touch disjoint memory in opposite
   directions.

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
   dependency tree via Bevy, so they cost no compile time. `sim/log.rs` is the working
   example: `ArrayQueue::force_push` takes `&self`, is bounded, and evicts the oldest
   rather than growing — pick bounded-and-lossy over unbounded whenever the reader is a
   frame that might not run.
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

**Neither is done yet.** `game/actors.rs` spawns one sprite per entity, for every entity,
and despawns it when the entity goes. That is fine for the hand-built worlds the game
screen has today and is the first thing to fix before the crowd arrives; the module says
so in its own docs, so the gap is recorded rather than forgotten.

### Measure before optimising

**There is a harness for this**: `qa/perf_simulation.json` times processing passes from an
empty world up to 5000 actors, and `qa/perf_rendering.json` times whole frames with up to
1500 of them on screen. Both write their distributions to `qa-perf/<test>.json`, and both
assert on *scaling* — cost per entity must not grow with the crowd — rather than on
milliseconds, because that is the check the earlier prototype's O(parts x workers) loop
would have failed and a wall-clock budget would not. Add a `measure` step to them before
optimising anything here, and read the `qa` skill's "Performance tests" section first:
frame numbers need `"vsync": false`, and a debug timing (`opt-level = 1`) is a ratio, not a
speed — `python3 tools/qa.py --release` for a number worth quoting.

Beyond that: add `FrameTimeDiagnosticsPlugin` and look, or build with Bevy's `trace_tracy`
feature. Guessing which of a dozen systems is the problem is exactly how the O(n^2) query in
the earlier prototype survived to the last commit.
