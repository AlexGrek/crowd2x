# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

crowd2x is a 2D pixel-art crowd simulation built on Bevy 0.19. Art was imported
wholesale from an earlier prototype; all code is new.
What exists is the pixel-perfect render pipeline, a menu, a map browser, a two-layer map
editor backed by a saved map format, a game screen that loads a map and runs a simulation
on it, and a scripted QA harness that drives all of it. The simulation is a `GameState`
advanced by one function; what it does so far is wander, which is enough to exercise every
part of a tick and nothing more.

## Commands

```sh
cargo run              # debug build; deps are optimized (opt-level 3) so it's playable
cargo run --release
cargo build
cargo test             # the plain-Rust parts: map, coordinates, navigation, scripts
python3 tools/qa.py    # the scripted QA tests in qa/, against the real binary
python3 tools/qa.py --release   # ...optimised, which is the only profile to quote a timing from
```

**Always run through `cargo`, never the raw binary** (`./target/debug/crowd2x`) — Bevy
resolves `assets/` relative to the manifest under cargo, and relative to the executable
otherwise, so a direct run produces a blank window and asset-load errors.

Controls: `WASD` / arrows pan the camera, `q` / `e` zoom in the game, `+` / `-` change
the game speed and `p` pauses it, `F12` saves a screenshot to `screenshots/` (gitignored).

### Verifying rendering changes without a human watching

`src/debug.rs` drives the app from environment variables so a frame can be captured and
the process exited unattended:

```sh
CROWD2X_SHOT=/tmp/frame.png cargo run                          # capture, then exit
CROWD2X_SHOT=/tmp/odd.png CROWD2X_WINDOW=1002x602 cargo run    # capture at a given window size
CROWD2X_SHOT=/tmp/edit.png CROWD2X_STATE=editor cargo run      # skip the menu
CROWD2X_SHOT=/tmp/play.png CROWD2X_STATE=game CROWD2X_MAP=office CROWD2X_ZOOM=6 cargo run
CROWD2X_SHOT=/tmp/crowd.png CROWD2X_STATE=game CROWD2X_MAP=office CROWD2X_SPAWN=20 cargo run
CROWD2X_EXIT=3 cargo run                                       # smoke run, no capture

# check_pixel_grid needs a UI-free frame — bevy_ui draws over the upscale at
# window resolution, so on-screen text is legitimately not on the pixel grid.
# Its second argument is the zoom, and must match CROWD2X_ZOOM.
CROWD2X_SHOT=/tmp/bare.png CROWD2X_HIDE_UI=1 cargo run
python3 tools/check_pixel_grid.py /tmp/bare.png 4
```

A pure black capture almost always means the frame was grabbed before the first one was
presented, not that rendering broke — raise `CROWD2X_SHOT_DELAY`, and check the display is
not asleep, before debugging.

Three skills go deeper than the summary below and should be read before the work they
cover: `qa` for the scripted test framework, `debugger` for the capture workflow, and
`dev` for Bevy API specifics, asset and animation conventions, and simulation-scaling
architecture.

## Architecture

### Rendering pipeline (`src/render.rs`)

Everything is drawn twice to get exact-integer pixel scaling:

1. A **world camera** (`RenderLayers` 0, `WORLD_LAYER`) renders the scene into an
   off-screen `Image` sized `window_physical / zoom`, at 1 world unit = 1 canvas pixel.
2. An **upscale camera** (`RenderLayers` 1) draws that image to the window as a single
   sprite scaled by `zoom`, nearest-neighbor sampled.

**Zooming is that factor** (`PixelZoom`, 1 to 8, `PIXEL_SCALE` = 4 by default), not a
camera scale: a fractional zoom would give neighbouring texels different widths, which is
the one thing this pipeline exists to prevent. Changing it rebuilds the canvas at the new
resolution exactly as a window resize does, so zooming in shows less world at a larger
size and every sprite stays on whole pixel blocks at either end. `UiScale` stays at
`PIXEL_SCALE`, so the interface keeps its size on screen however far the world is zoomed.

What keeps this exact, and easy to break:

- `ImagePlugin::default_nearest()` in `main.rs` — no texture filtering anywhere.
- `with_scale_factor_override(1.0)` on the window — makes logical/physical pixels
  identical so the upscale factor is exactly the zoom on HiDPI too.
- The world camera's `Transform` is snapped to whole pixels every frame; `CameraPan`
  holds the sub-pixel position for smooth movement. `CameraTarget` is how a screen asks
  for a position before the camera exists — `OnEnter` for the *initial* state runs before
  every `Startup` system.
- The canvas is rebuilt on window resize *and* on a zoom change, so the factor never
  drifts into a stretch.
- The canvas quad is nudged half a pixel when the canvas was rounded up by an odd number
  of screen pixels (`quad_offset`); without it the quad's edge lands between two screen
  pixels and every texel straddles two of them. A window the display cannot honour
  (999x601 arrives as 1000x602) fails the grid check for a reason that is not this —
  ask for even sizes.

**Anything new that should render must carry `WORLD_LAYER`**, or it won't appear.
Verify with `tools/check_pixel_grid.py` after touching this file, window setup in
`main.rs`, or any sprite `Transform` that could land on a fractional coordinate.

### Characters (`src/characters/`)

- `CELL = 48` — every character occupies a 48x48 cell of the canvas, but the art is
  drawn at `ART = 16` and upscaled by `ART_SCALE` (3) at spawn time.
- **Sprites are never stored pre-upscaled.** A PNG holds the picture at its true
  resolution and the game applies the whole-number scale; baking the scale into the file
  takes that choice away and hides which art is genuinely detailed. Scale X and Y only
  (`characters::upscale`) — Z carries painter's depth. `tools/art_scale.py report assets`
  finds files that are secretly an upscale, `shrink` reduces one (refusing if lossy), and
  three tests fail if character art or a palette entry stops matching its declared size.
- **16x16 is the resolution everything is meant to be at**, and the engine does the
  upscaling. The palette is nearly there: of the tiles only `floor` and `floor diagonal`
  are not, and of the props only the beds, the toilet, the trash can and the fire. Those
  are the imported art that is *genuinely* drawn at 48x48 — `tools/art_scale.py` refuses
  to reduce them because it would be lossy, and it visibly is: the beds lose their frames
  and the toilet stops being recognisable. They are art debt to redraw at 16x16, not a
  second supported resolution, and until then a palette entry has to declare which it is.
  New art is always 16x16 and always `PaletteItem::upscaled`.
- `depth_for(y)` gives painter's-order depth from world Y (lower on screen = drawn in
  front); per-character layer offsets are small enough that one character's layers can
  never interleave with another's.
- **This module spawns nothing of its own.** Who exists is `src/sim/`'s answer, and
  `game/actors.rs` is what turns it into sprites. It used to scatter a demo crowd on
  `Startup`, which left 28 characters off the edge of every map, in every screen, for the
  life of the process.
- **Humans** (`human.rs`): a layered paperdoll — a base body with eyes, clothes, hair as
  child sprites, each a separate PNG in `assets/human/`. Add a look by dropping a 16x16
  PNG in `assets/human/` and adding its path to the matching array (`CLOTHES`, `EYES`,
  `HAIR`); `Look::random` and `spawn` handle the rest. The scale lives on the root, so
  the layers cannot drift apart.
- **Dogs** (`dog.rs`): a 4-frame idle atlas of 16x16 frames — the atlas grid is in source
  texels, not screen pixels (`FrameAnimation`, `src/animation.rs`), with a
  *separate* mirrored sheet for left-facing rather than `flip_x`, so left-facing art can
  be hand-tuned independently (`dog::apply_facing` swaps `sprite.image` on `Changed<Facing>`).

### Screens and interface

`AppState` (`src/state.rs`) is `MainMenu`, `Maps` (the map browser), `Editor` or `Game`.
Screen entities carry `DespawnOnExit(..)` instead of hand-written teardown, which is also
why drawing a map takes the state it is being drawn for: the editor and the game draw the
same map from the same palettes and each takes its own sprites away on the way out.

**Two ordering traps live here.** `bevy_state` inserts `StateTransition` *before*
`PreStartup`, so the initial `OnEnter` runs before **every** `Startup` system — an
`OnEnter` system cannot see the camera or `PixelCanvas` that `render::setup_pipeline`
creates, and `Res<PixelCanvas>` there would panic. Anything a screen needs *before* its
`OnEnter` (the QA harness's fixture maps, `CROWD2X_MAP`) therefore has to be done in
`Plugin::build`, not in `Startup`.

The menu and HUD are stock `bevy_ui` (`Node`, `Button`, `Interaction`), which sidesteps
that: root nodes bind to the default UI camera later, once one exists. Build new
interface from `bevy_ui`, not hand-placed sprites.

UI is rendered by the **upscale camera**, not the world camera. That is forced:
`bevy_ui`'s focus system only computes a cursor position for cameras whose render target
is a window, and the world camera renders to an off-screen image, so `Interaction` would
never fire there. `UiScale` is `PIXEL_SCALE`, so all `Val::Px` and font sizes are in
canvas pixels (`ui::FONT_BODY` is 6, not 24).

#### One highlight, three devices (`src/ui/nav.rs`)

Screens do **not** keep a selection index. A widget spawns with `Focusable::new(row, col)`
and the mouse, the keyboard and a gamepad all move the same `Focus`; choosing anything —
click, `enter`, gamepad `A` — arrives as one `Activated(Entity)` message, and `esc`/`B`
as `Cancelled`. Directional movement is worked out from the row/col coordinates
(`nav::step`, unit-tested), so a list of rows with buttons along them navigates the way it
looks. Four rules are load-bearing and were each a bug first:

- **`Scope` is what makes a modal modal.** Focus never leaves the current scope, so an
  open dialog or on-screen keyboard cannot be navigated past or clicked through.
- **`KeyboardCapture`** turns the physical keyboard off as a navigation device while a
  text field owns it — otherwise typing `wasd` walks the highlight, and `enter` both ends
  the name and presses whatever the highlight was on. It is a frame-start snapshot, not a
  live read of the field, or the keypress that *opens* a field is also typed into it.
- **Input belongs to the screen it was made on.** Messages outlive their frame, so
  `nav::drop_input_from_the_last_screen` clears them on a state transition; without it one
  `esc` in the editor falls through the browser and quits the game. Screens run
  `.after(NavSystems)`.
- **Modals need `GlobalZIndex`.** A later-spawned UI root is not reliably on top.

A gamepad cannot type, so `ui/keyboard.rs` is an on-screen keyboard whose keys are
ordinary focusables writing into one `TextEntry` buffer that the physical keyboard also
writes to.

### The map browser (`src/browser.rs`)

The screen that owns saved maps: create with a name, play, edit, duplicate, delete (with
a confirmation, since it is the only button here that destroys work), and a scroll view
for the list. A map's **name** is the button that plays it and `edit` is the one beside
it — playing is what a map is for, so it gets the biggest target in the row. Scrolling
follows the focus (`ui::scroll_to_show`) so a controller can reach the end of a long
list, and the wheel moves it directly for a mouse. `CROWD2X_MAPS` points the store
somewhere else, which is what keeps a QA run from deleting real maps.

### The map editor

The editor edits the open `CurrentMap`, not the screen: a click writes the tile or prop
into the map and *then* spawns a sprite, so what is drawn cannot be something the saved
file does not contain. Entering rebuilds the scene from the map; leaving despawns it and
writes the map back (`F5` saves too, and so does closing the window — that path never runs
`OnExit`). Painting outside the map's dimensions is refused rather than growing it.

`Tool` holds the active layer and a per-layer palette index. The two layers exist to be
different, and new placeable content should respect that split:

- `editor/background.rs` — one tile per grid cell at a single depth behind everything,
  indexed by cell in the `Tiles` resource so painting can replace in place. Drag to paint.
  It is the drawn face of the map's terrain layer.
- `editor/props.rs` — free placement, depth from `characters::depth_for`, so props
  interleave with characters. One click, one prop; erase hits the nearest centre. These
  are the map's `Props` object layer, positioned in whole *pixels* rather than cells.

**Art and data are linked by name, never by index.** A palette entry is called after the
terrain it paints (`"wall brown"`) or the prop it places (`"bed 1"`), and that name is
what a map file stores — so the palette can be reordered without turning every saved bed
into a toilet. Two tests fail if the two lists stop lining up, and one more
(`every_palette_item_is_one_cell_wide_once_scaled`) fails if a `PaletteItem::new` should
have been `PaletteItem::upscaled`.

Painting is a pointing task, so it goes through a `Cursor` resource either device can
move: the mouse while it is moving, the left stick when it stops. Everything else has a
button on both.

### The game (`src/game/`)

Playing a map: it is loaded, drawn, simulated and looked at. The simulation itself is not
here — it is `src/sim/`, below — and `game/actors.rs` is the whole of the bridge.

- **It does not own the map's art.** Terrain and props go through `editor::draw_map`,
  from the palettes the editor paints with, so one catalogue binds a tile's name to its
  PNG instead of two that can drift apart.
- **It does not decide anything.** `actors.rs` builds a `GameState` from the open map,
  ticks it on `FixedUpdate`, and keeps one sprite alongside each entity — spawning for
  ids that appeared, despawning for ids that went, moving the rest to
  `position * TILE`, snapped to the art grid, at `characters::depth_for` so actors
  interleave with props. **A sprite is positioned in whole texels of its own art**
  (`characters::snap_to_texel`) — a sixteenth of a cell, three canvas pixels — not in
  whole canvas pixels. Either is exact for the pipeline, which never stretches a texel;
  the difference is that on the canvas grid a character's pixels sit a third of a texel
  off the grid they are drawn on, and crossing a cell reads as being nudged between
  sub-positions rather than walking.
  Sprites are not yet culled or pooled; the `dev` skill says why that is the next thing.
- **It does not edit anything**, so leaving is instant and there is nothing to save. The
  simulation gets a *clone* of the map, so the editor's copy cannot move under it.
- **It does not zoom the camera** — zooming is `PixelZoom`, above.
- **Speed is how many ticks happen, never how big one is** (`game/speed.rs`). A tick is
  always the fixed timestep, so 4x is four processing passes in one fixed step and a
  pause is none; `GameSpeed` carries the fraction between steps, so 0.5x is every other
  step, and the ladder (0.25 to 8) is powers of two so that stays exact. Scaling `dt`
  instead would change what the simulation *does* rather than only when. The **spawn**
  pass runs every fixed step regardless, pause included, so something spawned into a
  paused world appears standing still instead of looking lost. Pause is a flag beside the
  speed rather than a rung at the bottom of the ladder, so resuming goes back to the
  speed being watched; both reset on the way out, as the zoom does.
- **The camera is kept inside the map** (`clamp_to_map`). A map has edges and nothing
  outside them, so flying off into the void is getting lost rather than navigating; an
  axis with less map than view is centred, since there is nothing there to scroll to.
  Pan speed scales with the zoom so crossing the window always takes the same time.

Move with `WASD`, the arrows, the d-pad or the left stick; zoom with `q`/`e` or `A`/`B`;
change speed with `+`/`-` or the bumpers and pause with `p` or `Y`; `esc` or `start` goes
back to the browser. `B` is the one departure from the `esc`/`B` means back convention the
rest of the game follows — it is zoom out here, so this screen reads the leave buttons
directly instead of listening for the `Cancelled` it would otherwise fire on every zoom.

#### The two corners (`game/hud.rs`)

The screen is laid out as **the view on the left and the world on the right**: `- x4 +`
over the map's name, size, crowd and tick count top left; `slower x1 faster pause menu`
over the log top right, because the log is the readout for exactly those controls.

**Nothing here is `Focusable`,** and that is forced: the arrows pan the camera on this
screen and `A` zooms, so `nav`'s one highlight would be walked around by the camera
controls and pressed by the zoom. Every control has a direct binding on both devices
instead, and the buttons are for the mouse — the one device this screen otherwise has no
use for. They still answer to `Activated`, so a QA script presses them by name like
anything else; the speed pair says `slower`/`faster` rather than `-`/`+` because two
buttons with one label are two buttons a test cannot tell apart.

### The map (`src/map/`)

The first piece of simulation state, so it follows the rule below: **plain Rust, no
`bevy::` imports**, testable with `cargo test` and no `App`.

- All addressing is `Point { x: i32, y: i32 }` in whole cells — not `usize` (neighbours
  and deltas go negative constantly) and not `f32` (a cell is a hash key and a path node,
  so it has to compare exactly). Y increases upward like world Y, so the eventual drawing
  adapter is a scale and never a flip. `Size` owns the one row-major `index_of`, so the
  layers and the passability grid cannot disagree about the layout.
- A map is dimensions plus layers: **terrain** layers where every cell of every layer is
  defined (there is no empty cell, only the `VOID` tile — so no consumer handles a hole),
  and sparse **object** layers, `Props` and `Spawners`. Only the base terrain layer exists
  so far, and both object layers are placeholders — a spawner has deliberately not been
  defined yet.
- A cell holds a `TerrainId` into the static `TERRAIN` catalogue, so a big map is a flat
  array of `u16`. Passability lives in the catalogue, and an id this build does not know
  is impassable rather than a panic.
- **Passability is derived, never authored.** `PassabilityMap` is one bit per cell, kept
  in sync by `Map::set_terrain` and rebuildable in bulk by `Map::rebuild_passability` —
  which is also where combining overlay layers will land when there is a second one. It is
  a separate structure because it is the hottest read in the simulation: every path
  expansion asks about four cells, and here that is four bits from one cache line. Off the
  map answers "impassable", so callers need no bounds check of their own.
- Art is not in here. Which PNG draws a floor belongs to the Bevy adapter; the editor is
  not wired to this format yet and still spawns its own tile entities.

Maps serialise to JSON (`map/format.rs`, `Map::to_json` / `Map::from_json`), and the file
is a different shape from the runtime map on purpose — `format.rs` is the only thing that
knows both, so the in-memory layout stays free to change and the derived passability map
never reaches a file. Three things about the shape:

- **Tiles are stored by name**, through a per-file palette holding only the tiles the map
  uses. A `TerrainId` is an index into a table a later version may reorder; a name is what
  the author meant. Unknown names are a specific load error, not a wrong floor.
- **A terrain row is a CSV string**, so pretty-printed JSON puts one map row on one line
  instead of one cell per line (Tiled encodes layers this way for the same reason). Rows
  run bottom-up: row 0 is y 0, since Y increases upward everywhere else.
- **Nothing is repaired on load.** A short row, an unknown object layer, a size of zero,
  an object off the map — each is a `MapFormatError`, because padding or dropping produces
  a map that looks right and isn't. `FORMAT_VERSION` is checked first.

### The simulation (`src/sim/`)

The game runs here, and **Bevy is only a renderer**: it draws sprites and UI, reads the
devices, runs the shaders, and does not decide anything. Same rule as the map, one level
up and over the state that changes — **plain Rust, no `bevy::` imports**, testable with
`cargo test` and no `App`.

Four things define the shape, and they are the contract:

- **One super-object.** `GameState` is the whole game: the `map`, the `entities`, and a
  `log`. Not a set of resources, not a plugin — one value you can construct in a test.
- **One function.** `process_game_state(&mut GameState, dt, &Input)`. That is the entire
  entry point. The name says `-> GameState`; taking `&mut` is the honest implementation of
  it, because returning a fresh state would copy the map, the log and the id allocator
  sixty times a second to express a change that touches only the entities. The semantics
  that were actually wanted — a tick is a pure function of the previous state — come from
  the pass split, not from the signature.
- **Entities are addressed by `Uid`, never by Bevy's `Entity`.** 64 bits: the top byte is
  the `EntityType`, the low 56 are random and unique. The type in the id means a log line
  or a debugger says *what* something is with no lookup; the randomness means an id can be
  written to a file and still mean the same thing after a reload, which an `Entity` index
  cannot. "Guaranteed unique" is a retry against the live set, not a hope about 56 bits.
- **An actor is not an entity.** Entities are for things that draw. `game/actors.rs`
  spawns sprites *from* the simulation; the simulation has never heard of them.

A tick is **two passes**, and which one may write the entity table is the design:

| | writes the entity table | allocates | parallelisable |
| --- | --- | --- | --- |
| `spawn_pass(&mut GameState, &Input)` | **yes** — the only thing that does | yes | no |
| `process_pass(&mut GameState, dt)` | **no** | no | that is what it is for |

The **spawn pass** drains `Input`'s spawns and despawns. It runs first, so an entity
spawned this tick thinks this tick, and it is the only pass that may resize the table.

The **processing pass** advances every entity and adds or removes none. It is handed a
`FrozenEntities` — the table with its membership frozen, offering no `insert` and no
`remove` — so **that is a guarantee the compiler holds, not a rule to remember.** Inside
it are three steps:

1. **think** — read-only over the entities and the map, each returning an `Intent` into a
   slot-indexed buffer.
2. **move** — single-threaded, slots ascending, draining that buffer. **An entity does not
   decide where it ends up; the world does.** A step is granted only if the destination
   cell is passable terrain *and* unoccupied, and otherwise the move is **cancelled
   outright** — not trimmed, not slid along the obstacle — and what stopped it goes into a
   second slot-indexed buffer as a `MoveOutcome`, carrying the blocking entity's `Uid` when
   the obstacle was one. This is the one step that cannot be parallelised, and that is
   what it is for: two entities stepping into the same empty cell on the same tick is the
   race it exists to settle, and slot order settles it the same way on every run.
3. **react** — the second thinking round, once the whole crowd has moved. Each entity is
   handed its own outcome and revises its plan, writing only to itself. Answering a
   collision inside the move loop would mean reacting to a world that is halfway through
   the tick, with the later slots not moved yet.

**Passability is at the tile level, on the centre cell**, and it is two layers asked in
order: `map`'s `PassabilityMap` for the terrain, then `sim/occupancy.rs` for the crowd —
`Occupancy` is the dynamic layer the passability map's docs always said belonged
elsewhere. One entity to a cell, `claim` naming the occupant when it refuses. **Movement
therefore never creates an overlap; spawning can** — an entity is placed exactly where it
was asked for, so two can share a cell, and `release` only ever clears a cell whose
occupant is the entity leaving it, which is what keeps that contained.

What the wanderers do with all this is: nothing clever. A blocked walker drops its goal
and picks another next tick — no steering, no waiting, no queueing, no path. Each of those
is a change to `Walker::react` and nothing else.

Freezing membership is what makes the rest work. The intent buffer is indexed by slot and
sized once, in the spawn pass; a `Vec` that grows mid-tick moves its contents and
invalidates every index into it. And structural mutation is the one thing that genuinely
cannot be parallelised — two threads moving different entities never conflict, two threads
pushing to one `Vec` always do — so the pass that wants to run across threads is the pass
that is not allowed to change the table. An entity that needs to remove itself does not get
a back door: it returns something the *next* spawn pass acts on.

**The intent buffer is the double buffer.** Think reads entities and writes intents, move
reads intents and writes entities; the two never touch the same memory in the same
direction. That is what makes the read phase safe to parallelise, and it costs no entity
clone. Determinism follows from it plus a per-entity per-tick seeded RNG: same seed and
same input, same world after a thousand ticks, and there is a test that says so.

**Lock-free means there is nothing to lock**, not that there are clever atomics. The one
thing many threads genuinely write is the `log` — a bounded `ArrayQueue<String>` whose
`push` takes `&self`, evicts the oldest when full rather than growing, and counts what it
dropped. The renderer (`game/logview.rs`) is the only reader and it drains.

The store is a **dict over a dense arena**: `get(uid)`, `insert`, `remove`, `iter`, backed
by a `Vec` of slots plus a `HashMap` index. Callers look entities up by id; the tick walks
the `Vec` in slot order and hashes nothing. Removal leaves a **tombstone** rather than
`swap_remove`-ing, because a slot index has to stay stable — the intent buffer is indexed
by it, and reordering the tail on every despawn would make iteration order, and so the
simulation, non-deterministic.

Positions are in **cell units, not pixels**: `(3.5, 2.5)` is the middle of cell `(3, 2)`,
and `center_position()` is `position.floor()`, derived so the two cannot disagree. How
many screen pixels a cell is drawn at is a fact about the art, and `actors.rs` is where it
is applied. For the same reason nothing here knows what a human looks like — it supplies
an `appearance_seed`, and `characters/` decides which PNGs that means.

### Scripted QA (`src/qa/`, `qa/*.json`, `tools/qa.py`)

`cargo test` covers the plain-Rust parts and `debug.rs` can photograph a frame, but
neither answers the question that matters for an interface: *if someone presses these
buttons in this order, does the right thing happen?* A QA test is a JSON file that the
real binary replays as input and then checks.

```sh
python3 tools/qa.py                      # every test in qa/, one process each
python3 tools/qa.py qa/create_map.json   # one test
python3 tools/qa.py -v                   # stream the game's log
CROWD2X_QA=qa/create_map.json cargo run   # the same thing by hand
```

The `qa` skill documents the whole thing end to end — schema, every step, fixtures,
screenshots and how to diagnose a failure — and should be read before writing or changing
a test. In short:

Steps come at two levels and a test is expected to mix them:

- **Intent** — `{"press": "create"}`, `{"goto": "maps"}`, `{"name": "office"}`,
  `{"tool": "wall brown"}`, `{"cursor_cell": {"x": 3, "y": 2}}`. Addressed by *label*, so
  they survive a button moving, a palette being reordered, or a list growing a row. Use
  these unless the input itself is what is under test. `press` refuses an ambiguous label
  rather than guessing, which is why walking a file list still uses arrow keys.
- **Input** — `key`, `type`, `pad`, `stick`, `mouse`, `click`, `wheel`. The actual
  devices, and the only way to test that the devices work: that a gamepad reaches every
  button, that typing lands in the field and not in the palette.

Input is injected where the OS would have put it, not where it is convenient to fake it:
keys press `ButtonInput` *and* send a `KeyboardInput` message (the game reads both); a
gamepad is spawned and connected through `RawGamepadEvent`, so `bevy_input` builds the
real `Gamepad` component; the pointer moves by setting the window's cursor position, the
same field `bevy_ui`'s focus system reads, so a click goes through real hover-and-click.

The simulation gets two steps of its own, both intent-level: `{"spawn": {"kind": "human",
"x": 3, "y": 2}}` puts a command on the same queue the game uses, and `{"tick": 200}` runs
one spawn pass and then exactly that many processing passes, *immediately* — so pending
spawns are applied once however many ticks were asked for. Ticking rather than waiting is
the point — `FixedUpdate` runs at whatever rate the frame allows, so a test that waited a
second would be asserting on however many ticks the machine managed.

**A test can also time itself** (`src/qa/perf.rs`). `{"populate": {"kind": "human",
"count": 1000}}` queues a crowd spread over the cells that can be stood on, `{"measure":
{"name": "1000 humans", "ticks": 200}}` times that many processing passes — the simulation
alone, no renderer — and `{"measure_frames": {"name": "1000 actors", "seconds": 2.0}}`
times whole frames, sprite sync and render included. Every measurement keeps its
distribution (best, median, p95, worst, microseconds per entity), is written to
`qa-perf/<test>.json` pass or fail, and is printed by `tools/qa.py`: **the number is the
deliverable, and the assertions are a floor under it.**

Two assertions, and which one to reach for matters. `{"expect_scaling": {"from": "100
humans", "to": "1000 humans", "slack": 1.5}}` checks that the cost *per entity* did not
grow with the crowd, which is what an accidental per-agent scan over every other agent
looks like and is what killed the earlier prototype; it barely depends on the machine.
It divides the *fastest* sample of each measurement, not the median: interference only ever
adds time, and a ratio of two medians inherits the noise of both — with medians, three
consecutive runs of an unchanged build gave 0.52x, 0.68x and 1.60x. A budget judges the
median instead, because that is what the game typically does. How much slack a comparison
needs still depends on the cache — a hundred entities fit in L1 and a thousand do not, so
that pair moves by 1.3-1.8x between runs.
`{"expect_under": {"measure": "...", "ms": 1.0}}` is a wall rather than a target, set well
above what the machine does today, and the observed number belongs in a `note` beside it.
Two things stop the numbers being lies: a test that measures frames must set `"vsync":
false`, or every frame is capped at the refresh rate and a regression only shows once the
game is already below 60Hz; and a debug timing (this crate is `opt-level = 1` there) is
good for a *ratio* and worthless as a speed, which is why the profile and the vsync
setting are recorded in the report. `qa/perf_simulation.json` and `qa/perf_rendering.json`
are the two that exist.

Assertions are about outcomes — `expect_state`, `expect_focus`, `expect_map`,
`expect_no_map`, `expect_tile`, `expect_zoom`, `expect_speed`, `expect_entities`,
`expect_sprites`, `expect_log` — and `expect_tile` reads the **saved** map,
so "I painted a wall" is only true once the file says so. `expect_zoom` exists because
zooming changes the size of the canvas rather than the scale of a camera, so a screenshot
cannot be asked how far in it is without counting texels. `expect_speed` takes the string
the readout shows (`x1`, `x0.25`, `paused`) rather than a number and a flag, since
`paused` and `x1` are the same multiplier and a different game. `expect_entities` and
`expect_sprites` are deliberately two assertions: the simulation having three entities and
the screen showing three actors are different claims, and the second is the one that
catches a renderer that has quietly stopped keeping up.

**Screenshots are named, not pathed.** `{"shot": "the file list"}` writes
`qa-screenshots/<test>/01-the-file-list.png`, numbered in the order the shots were taken,
so one test can photograph as many moments as it likes without inventing file names.
`tools/qa.py` wipes each test's directory before it runs and reports what came out; the
whole directory is gitignored, since a screenshot is evidence rather than source.

A capture that beats the renderer to the frame comes back as one flat colour instead of
as an error, and *how long* to wait for the first real frame is not knowable — on this
machine it has ranged from under three seconds to never. So the harness looks at what it
captured and retakes it until there is something in it (`is_flat`, then `shot_delay`
between attempts, then it gives up and says so rather than leaving black PNGs to be
puzzled over). If every shot in a run is blank, the window is not being presented at all —
a sleeping display or no display — which is an environment problem and not a test failure.

A test can start from a world rather than an empty one: `given.maps` takes a bare name (an
empty map), `{"name": ..., "file": ...}` (a map kept in `qa/fixtures/`), or
`{"name": ..., "map": {...}}` (the map document written inline, for tests that depend on
exactly which cells are painted). `"open": "<name>"` has that map already loaded when the
app starts, in whichever screen `state` names — `editor` or `game`; a script asking for a
screen this build does not have stops with a message saying so rather than running
against the wrong screen.

Two things to keep true, both learned the hard way here:

- **`main` returns `AppExit`.** Discard it and a failing test still exits 0, and the whole
  suite passes vacuously. Check a new test fails when it should before trusting it.
- **Fixtures (`"given"`) are written in `Plugin::build`,** because the browser lists the
  directory in `OnEnter`, which for the starting screen runs before every `Startup` system.

Point `CROWD2X_MAPS` at a scratch directory when running these by hand, or a test will
delete maps somebody meant to keep; `tools/qa.py` gives each test its own.

### Module layout

```
src/main.rs             app + window setup, plugin registration
src/state.rs            AppState
src/render.rs           PixelRenderPlugin - the pixel-perfect pipeline
src/ui/                 UiPlugin, shared widgets; nav.rs (focus), keyboard.rs (typing)
src/menu.rs             MainMenuPlugin
src/editor/             EditorPlugin, background.rs + props.rs
src/browser.rs          BrowserPlugin - the saved-maps screen
src/game/               GamePlugin - playing a map: camera, zoom, clamped to the map
                        actors.rs is the sim-to-sprite bridge; logview.rs shows the log
                        speed.rs is how fast the world runs; hud.rs the two corners
src/map/                the map + coordinate system - plain Rust, no bevy
src/sim/                GameState + spawn_pass/process_pass - plain Rust, no bevy
                        uid.rs, entity.rs, kinds.rs, entities.rs (the arena), log.rs
                        occupancy.rs is who stands where: passability's dynamic half
src/qa/                 scripted QA: script.rs is the JSON schema, mod.rs replays it
                        perf.rs is the measuring half: statistics, budgets, scaling
src/awake.rs            macOS: hold the display awake so a run can be photographed
src/characters/         how a character is drawn: CELL/ART/upscale, depth_for
src/animation.rs        FrameAnimation, atlas frame stepping
src/debug.rs            screenshot/smoke-run harness, env-var driven
tools/check_pixel_grid.py  verifies a frame is an exact integer upscale
tools/art_scale.py         finds/reduces art that is stored pre-upscaled
tools/qa.py                runs every qa/*.json against the real binary
qa/                     scripted QA tests, one JSON file each; fixtures/ holds map JSON
qa-screenshots/         what those tests photographed (gitignored)
qa-perf/                what the perf tests measured (gitignored: one machine's evidence)
maps/                   saved maps (gitignored; CROWD2X_MAPS points elsewhere)
assets/                 imported wholesale from an earlier prototype (layout preserved; .tmx maps not imported)
```

### Stack notes

- Bevy 0.19, edition 2024, `default-features = false, features = ["2d", "ui", "png",
  "jpeg"]` — Bevy's curated 2D and UI umbrellas, without pbr/gltf/audio/anti-alias/solari.
  Don't switch to default features to pull in one extra type; find the right feature.
- Bevy 0.19 is newer than most training data and relocated many APIs (e.g. `RenderTarget`
  is a Component under `bevy::camera`, not a `Camera` field; buffered events are
  `Message`/`MessageReader`/`MessageWriter`, not `Event`). Verify against the vendored
  source (`~/.cargo/registry/src/*/bevy_*-0.19.1/src/`) rather than memory — the `dev`
  skill has a lookup table of the common gotchas.
- `bevy/dynamic_linking` is not usable on this version (`bevy_dylib` 0.19.1 was never
  published) — don't re-add a `dev` feature for it.

### Designing for a crowd, not a handful of sprites

An earlier prototype died at ~100 actors on architecture — a global-lock resource,
an O(parts x workers) per-frame query, and unbudgeted per-agent pathfinding. The `dev`
skill documents this in depth; the load-bearing rule when adding simulation logic is:
**keep the simulation in plain Rust with no `bevy::` imports** (grid, pathfinding, agent
state, decisions), with Bevy as a thin adapter that reads sim state and updates
`Transform`/`Sprite`. An actor doesn't need to be an entity — entities are for things
that draw.

Concretely, that rule now has an address: **new simulation logic goes in `src/sim/`,
behind `GameState` and `process_game_state`** — see "The simulation" above. Adding a Bevy
system that decides something, a `Component` holding agent state, or a second function
that advances the world is the thing to not do; add an entity kind, an `Intent` variant or
a `Command` instead. Read the `dev` skill's "Scaling to thousands of actors" section, and
its "The GameState contract", before writing anything simulation-shaped.
