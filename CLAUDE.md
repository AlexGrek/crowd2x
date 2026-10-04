# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

crowd2x is a 2D pixel-art crowd simulation built on Bevy 0.19. Art was imported
wholesale from an earlier prototype; all code is new.
What exists is the pixel-perfect render pipeline, a menu, a map browser, a two-layer map
editor backed by a saved map format, a game screen that loads a map and runs a simulation
on it, and a scripted QA harness that drives all of it. The simulation is a `GameState`
advanced by one function, and every unit in it runs a brain — routines, goals, tasks,
actions. What those brains do so far is wander, eat and drink at a fridge, use a toilet,
have a go on a computer, and sleep in a bed at night — driven by biological processes
(hunger, thirst, a bladder that a drink fills, fun that drains away, energy that a night in
bed restores, satisfaction that a treat lifts) that can be switched off per unit, and dulled
by a fading memory of what was done lately: the same treat twice in a day is less of one. The computer is also the first prop that
is *watched* being used: its screen is on for as long as somebody is sitting at it. (A bed
does not yet show that it is slept in: it has no second strip of art to swap to.)

## Commands

```sh
cargo run              # debug build; deps are optimized (opt-level 3) so it's playable
cargo run --release
cargo build
cargo test             # the plain-Rust parts: map, coordinates, navigation, scripts
uv run tools/qa.py    # the scripted QA tests in qa/, against the real binary
uv run tools/qa.py --release   # ...optimised, which is the only profile to quote a timing from
```

**Always run through `cargo`, never the raw binary** (`./target/debug/crowd2x`) — Bevy
resolves `assets/` relative to the manifest under cargo, and relative to the executable
otherwise, so a direct run produces a blank window and asset-load errors.

Controls: `WASD` / arrows pan the camera, `q` / `e` zoom in the game, `+` / `-` change
the game speed and `p` pauses it, a left click selects the unit under it, `F12` saves a
screenshot to `screenshots/` (gitignored).

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
uv run tools/check_pixel_grid.py /tmp/bare.png 4
```

A pure black capture almost always means the frame was grabbed before the first one was
presented, not that rendering broke — raise `CROWD2X_SHOT_DELAY`, and check the display is
not asleep, before debugging.

Five skills go deeper than the summary below and should be read before the work they
cover: `qa` for the scripted test framework, `debugger` for the capture workflow, `dev` for
Bevy API specifics, asset and animation conventions, and simulation-scaling architecture,
`brain-engineer` for anything a unit decides or its body does (goals, tasks, features, items,
stats, processes), and **`add-routine` for adding or changing a routine** — it names the exact
files to edit, which existing routine to copy, how to test, and which parts of the repository
not to read.

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
- **Items** (`item.rs`): every `ItemKind` has art — a pixel-art texture or an emoji
  (`ItemArt`) — and `item::art` is a `match` with no wildcard arm, so a new item does not
  compile until it says which. It lives here and not on `ItemKind` because `sim/` does not
  know what anything looks like. A **texture can be any size** and is drawn at `ART_SCALE`
  like every other sprite, so its pixels match its carrier's (which also makes the file's
  size the item's: a 16x16 one is as big as a person). What makes any size safe is
  `centring_nudge`: a sprite is centred on its position, so one an odd number of canvas
  pixels wide has its edges between two pixels and its texels come out uneven — measured, a
  5-texel texture at zoom 4 drew its texels 12/12/16/8/12 screen pixels wide instead of 12 —
  and an odd axis is moved half a pixel. Its size is only known once it has loaded, so
  `game::held` applies it then. An emoji is one codepoint, drawn as `Text2d` in the bundled
  Twemoji font, as the goal label over a unit's head is. No item is drawn from a texture
  yet: food and water are emoji.

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
into the map **and nothing else**. What is on screen is drawn *from* the map, a canvas at a
time, by two windows that follow the camera — `background::TileWindow` for terrain,
`props::PropWindow` for props — so what is drawn cannot be something the saved file does
not contain. Both run on the game screen too, which is why they are registered here, where
the palettes live. Leaving writes the map back (`F5` saves too, and so does closing the
window — that path never runs `OnExit`). Painting outside the map's dimensions is refused
rather than growing it.

**A window redraws when it is told to, never on `CurrentMap`'s change flag.** An edit that
changed the map calls the window's `touch()`; loading a map needs no signal, because every
load is a change of screen and `reset_map_windows` empties both windows on the way into
either one. The change flag cannot be trusted to mean anything: the brush borrows the map
mutably on every frame it is held, whether or not a cell changed, and a prop window that
listened to it respawned every prop on screen — restarting every animation — 181 times in
a second and a half of dragging. `qa/place_props.json` covers placing and erasing.

`Tool` holds the active layer and a per-layer palette index. The two layers exist to be
different, and new placeable content should respect that split:

- `editor/background.rs` — one tile per grid cell at a single depth behind everything,
  drawn only for the cells under the canvas: `TileWindow` re-points the sprites of the row
  or column that left the view at the one that arrived, and keeps a free list bounded by
  the view (`pool::park_budget`) for the rest. Drag to paint.
  It is the drawn face of the map's terrain layer. `"block"` and anything named
  `"wall..."` are instruments rather than plain tiles (`Instrument`, in `editor/mod.rs`):
  a drag is a rectangle from corner to corner, held as a ghost preview and committed only
  on release — hollow (the four sides) for a wall, filled for a block. A single click is
  just a one-cell rectangle, so it paints exactly the one tile a plain drag would, which
  is what keeps a script that clicks once still passing.
- `editor/props.rs` — free placement, depth from `characters::depth_for`, so props
  interleave with characters. One click, one prop; erase hits the nearest centre. These
  are the map's `Props` object layer, positioned in whole *pixels* rather than cells.
  `PropWindow` draws the ones whose cell is under the canvas; it culls and does **not**
  pool, since a prop's components depend on its art and recycling one would be an
  archetype move, and props are few (the header of `props.rs` says when to revisit).
  **A prop blocks the cell its centre is in** (below); a new palette entry needs a
  matching `map::PROPS` entry, and two tests fail if the lists stop lining up.
  **A prop's art may be a strip** — `PaletteItem::animated(name, path, frames)`, frames of
  16x16 side by side in one PNG, stepped by `StripAnimation` (`Sprite::rect`, not a
  texture atlas: prop art is drawn by the window and by the cursor's ghost, which swaps
  picture whenever the palette moves, and a rect needs nothing but the image).
  `.used(path, frames)` adds a
  **second** strip for what it looks like while somebody is using it, which only the game
  screen ever shows (`game/props.rs`).

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

- **It does not own the map's art.** Terrain and props are drawn by the editor's two map
  windows (`TileWindow`, `PropWindow`), from the palettes the editor paints with, so one
  catalogue binds a tile's name to its PNG instead of two that can drift apart.
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
- **It only draws what is on the canvas.** `src/view.rs` works out `VisibleArea` once a
  frame, after the camera is snapped: `canvas` (exactly what is rendered), `actors`
  (`canvas` grown by `characters::OVERHANG_*`, so a unit whose bar or goal label reaches
  the canvas counts), `actors_keep` (`actors` grown by `HYSTERESIS`, one cell) and
  `tiles` (the cells under the canvas, padded by one). `actors::collect_visible_crowd`
  resolves it into `VisibleCrowd`, a list of slots, in one pass over the arena, and
  everything that draws something about a unit — bodies, action bars, goal labels, held
  items, props lighting up — reads that list instead of walking the crowd for itself.
  A unit gets a sprite on entering `actors` and keeps it until it leaves `actors_keep`;
  without that band a unit pacing the edge would swap archetypes every frame. A body
  that leaves is **parked**, not despawned (`game/pool.rs`): hidden, `Actor` taken off,
  redressed for whoever needs one next. The pool is bounded by the view
  (`park_budget`: a quarter of the cells on screen, 16 to 256), because Bevy walks every
  sprite entity each frame before it looks at visibility, so a hidden body is cheaper
  than a spawn and is *not* free. The result is that the number of sprite entities is a
  function of the canvas, not of the map or the crowd: twenty thousand humans on a
  256x256 map draw a few dozen. **A culled unit can still be selected**: `Selected` holds
  a `Uid`, picking asks `sim::Occupancy`, and the frame follows the simulated position,
  so none of it depends on a sprite existing.
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

**What a hand holds is drawn in front of whoever holds it** (`game/held.rs`, art from
`characters::item`). A unit whose `Inventory::hand` is full gets one sprite of its own, in
front of every layer of its paperdoll and at the hands, that follows it and is gone the
frame the hand is empty or holds something else. Only the **hand** — what is stowed is put
away and shows nothing, which is the split the inventory is built on. It is a sprite beside
the character, like the action bar and the goal label, and not a child of it: the paperdoll
root carries the 3x upscale, which an emoji would inherit. Its depth is the carrier's own
plus `0.005`, between the selection frame and the action bar, so it stays inside the band
`depth_for` leaves for one character.

Move with `WASD`, the arrows, the d-pad or the left stick; zoom with `q`/`e` or `A`/`B`;
change speed with `+`/`-` or the bumpers and pause with `p` or `Y`; `esc` or `start` goes
back to the browser. `B` is the one departure from the `esc`/`B` means back convention the
rest of the game follows — it is zoom out here, so this screen reads the leave buttons
directly instead of listening for the `Cancelled` it would otherwise fire on every zoom.

#### Selecting somebody (`game/selection.rs`, `game/unitpanel.rs`)

A left click on the map picks whoever is standing in that cell, a green frame
(`selection.png`) says who, and a panel along the bottom shows their portrait and five
menus: `life` (despawn, freeze), `debug`, `stats` (deliberately empty), `brains` (the
goal in charge, the priority list, the task, its result and action — live, like `debug`)
and `items` (the hand, what is stowed, and both totals against both limits — live too,
since a unit picks things up while you are reading). Clicking empty ground, or `close`,
selects nobody and the panel goes with them.

- **Picking is by cell**, not by sprite bounds: the click becomes a world position, the
  position a cell, and the cell is asked of `sim::Occupancy` — one lookup however big the
  crowd, and the same addressing the simulation itself uses. Hit-testing sprites would
  scan every actor and would disagree with the simulation about a walker halfway across a
  boundary.
- **The selection is not simulation state.** `Selected` is a Bevy resource holding a
  `Uid`: who you are looking at changes nothing about the world, and the same seed still
  replays the same. What the panel *does* — despawn, freeze — goes back through
  `sim::Command` on the same queue as everything else, so the screen still has exactly one
  writer of the world.
- **The panel is rebuilt, the readouts are written.** Structure (which portrait, which
  menu) is rebuilt only when the selection or the open menu changes; the debug menu's
  fields and the freeze button's own label are rewritten in place every frame, because
  they follow the simulation rather than the player. That is what makes the debug menu
  live.
- The portrait is built from the same art the world draws with — `human::portrait_layers`
  stacks the paperdoll as UI nodes, a dog gets frame 0 of its idle sheet — so a portrait
  cannot show an outfit its character is not wearing.
- **Every panel on this screen absorbs clicks** (`hud::absorbs_clicks`). A panel is not a
  button, and `bevy_ui` only tracks nodes carrying an `Interaction`, so without it a click
  on the log would reach through and select whoever stood behind it.

Nothing here is `Focusable`, for the reason the corners are not (below), and every button
still answers to `Activated` so a QA script can press it by name.

#### The two corners (`game/hud.rs`)

The screen is laid out as **the view on the left and the world on the right**: `- x4 +`
over the map's name, size, crowd, tick count and the world's clock top left; `slower x1 faster pause spawn
menu` over the log top right, because the log is the readout for exactly those controls.

**Nothing here is `Focusable`,** and that is forced: the arrows pan the camera on this
screen and `A` zooms, so `nav`'s one highlight would be walked around by the camera
controls and pressed by the zoom. Every control has a direct binding on both devices
instead, and the buttons are for the mouse — the one device this screen otherwise has no
use for. They still answer to `Activated`, so a QA script presses them by name like
anything else; the speed pair says `slower`/`faster` rather than `-`/`+` because two
buttons with one label are two buttons a test cannot tell apart.

`spawn` is the exception that proves the rule: it opens a modal (`SpawnMenu`), and a modal
is the one thing on this screen that *is* navigable — full screen, opaque, `GlobalZIndex`,
its own `Scope(SPAWN_MENU)`, so the corner buttons behind it stop taking the pointer and
the highlight cannot be walked out of it. Its buttons are dispatched apart from the corner
controls (`spawn_menu_actions`, not `press`) precisely because `nav` only activates a
widget in the scope that owns input. A kind is placed at the passable cell nearest the
camera — `spawn_point` scans the map once per click rather than spiralling, which is what a
once-per-click cost is allowed to do — and `leave` asks whether the menu is open before
deciding whether `esc` closes it or leaves the game.

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
- **Props block.** A static `PROPS` catalogue (`map/props.rs`, the same shape as
  `TERRAIN`, linked to the editor palette by name) says whether each prop can be walked
  through, and today nothing can — beds, the fridge, the toilet, the computer, crates, fire.
  A blocking
  prop takes **the cell its centre falls in** (`Object::cell`) and no other: passability is
  a whole-cell fact, the rule bodies follow too, so tall art overhanging the cell above does
  not wall it off. A prop name this build does not know blocks, as unknown terrain does.
  Spawners never block. The consequence for the brain is that a feature is normally used
  *from beside it*, never from its own cell (`goals::stand_beside`) — the toilet and the beds are the
  exceptions, used by entering, and "The brain" below says how a cell can stay impassable
  here and still be walked into by exactly one unit.
- **Passability is derived, never authored.** `PassabilityMap` is one bit per cell: a cell
  is passable when its terrain is and no blocking prop stands in it. Kept in sync by
  `Map::set_terrain`, `add_object` and `remove_object` (floor painted under a fridge stays
  blocked; a cell opens only when its last prop goes) and rebuildable in bulk by
  `Map::rebuild_passability` — which is also where combining overlay layers will land when
  there is a second one. It is a separate structure because it is the hottest read in the
  simulation: every path expansion asks about four cells, and here that is four bits from
  one cache line. Off the map answers "impassable", so callers need no bounds check of
  their own.
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
it are four steps:

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
   the tick, with the later slots not moved yet. It may also hand back an `Effect` — a
   request for a change outside itself, so far only `Effect::Fridge { at, open }` — into a
   third slot-indexed buffer, since **writing only to itself** rules out flipping a fridge's
   door directly: that is a fact about the world, read by every entity beside it, not about
   the one entity that opened it.
4. **world** — single-threaded, slots ascending, like move, and for the same reason: two
   units opening and closing one fridge on the same tick is a race, and this is what settles
   it the same way on every run. Every `Effect` is applied (`fridges.set_open`), the slot
   reset to `Effect::None`, and then `Fridges::advance` moves every fridge's temperature on
   by the tick's worth of world time — see `sim::world_step` and "Features" below.

**Passability is at the tile level, on the centre cell**, and it is two layers asked in
order: `map`'s `PassabilityMap` for the terrain and the furniture, then `sim/occupancy.rs`
for the crowd —
`Occupancy` is the dynamic layer the passability map's docs always said belonged
elsewhere. One entity to a cell, `claim` naming the occupant when it refuses. **Movement
therefore never creates an overlap; spawning can** — an entity is placed exactly where it
was asked for, so two can share a cell, and `release` only ever clears a cell whose
occupant is the entity leaving it, which is what keeps that contained.

**Freezing is the one thing done to an entity from outside it.** `Command::Freeze` rides
the same queue as a spawn and is applied in the spawn pass, so a freeze asked for this tick
is in force before anything thinks; the think step then hands a frozen entity an
`Intent::Idle` instead of asking it, and the react step skips it too — its brain would
find nothing running and ask for work every tick, a route searched for nothing each time.
That costs a bool per entity and means a frozen entity is not deciding things nobody will
carry out; no time passes for it, its biology included. It keeps its cell and its goal, so
letting it go again carries on rather than starting over. `GameEntity::debug_fields` is the
other half of that pair — what a kind would tell a debugger about itself, allocating
freely because it is asked about the one entity somebody has selected and never in a tick.

#### What a unit carries (`sim/inventory.rs`, `sim/item.rs`)

An `ItemKind` says **what it is made of** — `nutrition`, `hydration`, and a `mass` in
kilograms and a `volume` in litres — and never what that does to a body or to whoever is
carrying it. `Inventory` is a unit's **hand plus what it has stowed**, inline and `Copy` on
`Human`, and the two slots are counted differently on purpose:

|  | the hand | stowed |
| --- | --- | --- |
| **mass** (kg) | counted | counted |
| **volume** (litres) | **not** counted | counted |

**Weight is carried wherever it is**, so a full hand leaves less that can be stowed;
**space is about packing things away**, and a hand is not a shelf. Both limits genuinely
bind, which is why there are two of them: food is bulky for its weight and water is dense,
so a person runs out of *space* at twelve meals and out of *strength* at twenty glasses
(`Capacity::HUMAN` is 10kg and 12L, per unit, so a bag or a build can differ later).

Two rules hold the rest together:

- **The limits govern stowing and never the hand.** `stow` refuses what will not fit;
  `set_hand` refuses nothing. A hand that could be refused would deadlock a hungry unit
  whose pockets were full, and nothing in the brain could say why —
  `a_human_loaded_to_its_limits_can_still_pick_food_up_and_eat_it` is that guarantee.
- **It is a count per kind, not a list of things.** `ItemKind` is fieldless, so two
  portions of food are the same item in every way the simulation can tell apart: one `u8`
  per kind, no allocation in a tick, and no slot count imposing a third limit nobody asked
  for. An item that needs state of its own is a change to `ItemKind` first.

A task is the only thing that writes it (`TaskCtx::inventory`); a goal only reads it
(`GoalCtx::inventory`), which is what keeps food arriving in a hand when a `TakeItem`
*finishes* rather than when a goal hears that it did. **Nothing stows anything yet** — the
brain takes into a hand and consumes from it, so a unit walks past a fridge with food in
its pockets, and `Command::GiveItem` (a QA script's `give`) is the only way in. A goal that
fetches and puts away is what closes that, and it needs a task that unstows. The hand is
the half a viewer can see — `game/held.rs` draws what is in it — and `Command::Hold` (a QA
script's `hold`) fills or empties it from outside, since the brain only ever fills it for
the length of a meal.

#### What a second is (`sim/clock.rs`)

**A second of watching is two minutes of world** (`TIME_SCALE`), so an hour goes by in
thirty seconds and a day in twelve minutes. `GameState::clock` is that time of day,
derived from `elapsed` rather than counted beside it, and the HUD's left corner shows it.

Which leaves a tick measuring two different things, and the unit a duration is written in
says which:

- **What is watched happening** is on the watched clock: `Think::dt`, the fixed timestep.
  A walk crosses cells at a pace a person can follow, and `WAIT_FOR_A_GAP` is half a
  second of standing aside for somebody. Scaling these would make a crowd teleport — a
  tick is two minutes of world, and nobody should cross a room between two frames.
- **What the world's clock governs** is on `Think::game_dt`, a hundred and twenty times as
  much: the biology, and nothing else so far. Getting hungry takes hours, and hours are
  what that clock counts.

A duration that is *watched* but means something in world time is written as
`watched(15.0 * MINUTE)` — a quarter of an hour of eating, seven and a half seconds of
watching it — so even those say what the world thinks is going on. The game speed
multiplies how many *ticks* happen, so it moves both clocks together and 4x is the same
world watched faster.

`Think` also carries the time of day, `Think::clock` — one reading of the world's clock per
pass, shared by the whole crowd — and `Clock::is_night` (22:00 to 06:00, across midnight) is
the one question a routine asks of it so far.

#### The brain (`sim/brain/`)

What an entity does with all this is decided by a `Brain`, which runs entirely in
`GameEntity::react` — the only `&mut self` hook, after the whole crowd has moved — while
`think` stays what it was: walk the current route, arithmetic, parallel. Six layers, each
talking only to the one below:

- **perception** — a named stub. What it wants is a spatial index on `Think`, never a
  per-unit scan of the crowd.
- **memory** — a `BTreeMap<String, Recall>`, nearly empty: the one thing in it so far is which
  bed is a human's own (`HOME_BED`, written by the spawn pass). A `BTreeMap` so iterating it
  can never be a hash order. What a unit has *done* lately is a different memory, kept by
  the body because a task writes it every meal (`biology::Recollection`, under "Biology").
- **routines** (`NeedRoutine`, `SleepRoutine`, `StayBusyRoutine`) own **priorities and nothing else**.
  The list is zeroed every tick and each routine raises what it cares about
  (`Goals::raise_to` is a max, so two routines cannot undo each other). `NeedRoutine` is
  one routine built from a `Need` — `HUNGER` ("keep fed"), `THIRST` ("keep hydrated"),
  `BLADDER` ("stay comfortable"), `BOREDOM` ("keep entertained") — with hysteresis between a commit and a release
  threshold, every need on the same `/ 50` priority scale, and nothing wanted while the
  process behind the need is switched off. **Sleep is the routine that is not a `Need`**,
  because it reads the clock as well as a stat: by day it commits at 75 tired and lets go at
  10; by night it commits at 30 and *does not let go until morning*, holding a priority
  floor of 0.6 so somebody rested at one o'clock stays in bed — above wandering, below any
  committed hunger, thirst, bladder or boredom, so a sleeper gets up for the toilet and
  comes back. **There is no cap on how many a brain has**:
  `Routine` is an enum over one struct per routine type, `match`-dispatched like `Task`,
  and a brain holds them in a `Box<[Routine]>` built once at spawn — one allocation per
  unit however many routines, state inline and contiguous, no `push` so the order cannot
  change. A per-routine `Box<dyn>` would be dozens of allocations per unit and a pointer
  chase per routine per unit per tick, which is what a human with dozens of routines in a
  crowd of thousands cannot afford.
- **goals** — `GoalId` over a fixed array; the one on top is an argmax, not a sort. A
  `GoalExecutor` (`WanderGoal`, `EatGoal`, `DrinkGoal`, `RelieveGoal`, `PlayGoal`, `SleepGoal`) is a `Box` owned for
  the entity's life, so **its fields are its saved state** across being put down and
  picked up. It reads the body, hands and memory, and manages the task queue; it never
  changes the world itself. **One goal per need, each in its own file**, even where two
  plans look alike today (eating and drinking): the processes behind them will not stay
  alike. What they share — `stand_beside`, `PATIENCE`, `WAIT_FOR_A_GAP` — is in
  `goals/mod.rs`. A goal whose hand holds somebody else's item finishes it first, since
  taking needs an empty hand; without that, food taken just before thirst took over makes
  every drink fail forever. `SleepGoal` queues **one hour** in bed and reports `Achieved`; if
  the routine still wants sleep it stays on top and the next hour starts from where the unit
  lies, so how long a night is belongs to the routine, and an interruption costs the hour in
  progress rather than the night. **Which bed:** a human has one of its own, handed out at spawn
  (`GameState::give_a_bed`: the nearest bed nobody owns, *only while one is free* — a `Homes`
  registry on `GameState`, written by the spawn pass alone and released on despawn) and
  remembered in its `Memory`. By default that is the one it goes to, and the only one. It takes
  **any other free bed, at random, only when critically tired** (`CRITICALLY_TIRED`, 90), and
  once lying in a bed stays in it. A human that arrives to find every bed owned has none, and
  stays up until it is critical. Nothing in a tick reads `Homes`: the unit remembers.
- **tasks** — a fixed, double-ended inline queue of `Task`, an enum over one executor struct
  per step (`MoveTo`, `TakeItem`, `ConsumeItem`, `UseToilet`, `UseComputer`, `Sleep`, `Wait`,
  `OpenFridge`, `CloseFridge`), `match`-dispatched so
  queueing one allocates nothing. **A task writes its `TaskResult`** (`InProgress`,
  `Executing`, `Failed`, `Success`), checks its preconditions every tick (`TakeItem` only
  from one step away), and applies what finishing means: food goes into a hand when a
  `TakeItem` ends, not when a goal hears that it did. A task never writes a stat — it tells
  the body what happened (`Event::Ingested`, `Event::Relieved`, `Event::Slept`), below.
- **actions** — pathfinding and timing only. `Action::walk_to` is the one place a far route
  is asked for.

The pipeline runs in the order specified and the order is the design: perception; every
routine arranges the list; on a change at the top, the old goal's `deprioritized`, the
current task abandoned, the queue cleared, the new goal's `prioritized`; the goal on top's
`process` **only if the goal changed or no task is current**; then the current task's
executor. So a task that ends at tick N is heard by its goal at N+1, which decides — carry
on, retry, replan — before anything else starts: one tick between tasks, the price of the
goal having a say. A goal put down before it heard is handed that result in
`deprioritized`. A goal that returns `Blocked` is held off (`BLOCKED_TICKS`, doubling), and
`Achieved` does not hand over — if the routine still wants it, it starts again.

The routing under `MoveTo` is **two stages of one A\*** (`sim/path.rs`, asked twice with two
different predicates), in `sim/walker.rs`. The *far* stage plans against the terrain only,
once per walk, and its cost is bounded by a count of expansions rather than by hope — an
unreachable goal floods everything it can reach before it knows, so `FAR_LIMIT` turns a
walled-off room from a frame-rate cliff into a bounded miss. The *near* stage is the detour:
something was in the way, so `DETOUR_CELLS` of the plan are thrown out and rerouted against
the crowd as well as the terrain; no local way round fails the walk, and the goal decides
whether to wait for a gap (`WAIT_FOR_A_GAP`, up to `PATIENCE` times) or give up.

The search is 4-connected and movement is not: a walker leaves for the next cell as soon as
it is inside the current one, so the line it walks cuts corners — except the last cell,
which a walk ends in the *middle* of. `path.rs` says why corner cutting can never skip a
cell the search vetted.

**Features** (`sim/feature.rs`) are what props are *for*: a static `FEATURES` catalogue binds
a prop name to a `FeatureKind`, and `GameState::new` indexes the map's props by cell once.
A name may appear more than once — `"fridge"` is both `Food` and `Water` — `"toilet"` is
`Toilet`, `"computer"` is `Entertainment` and `"bed 1"` to `"bed 6"` are `Bed`. A fridge never
runs out of food or drink, and is used from one of the four cells beside it — it blocks its
own. Adding a use for a prop is an entry there, a palette entry in `editor/props.rs` and a
`map::PROPS` entry (tests fail without them), and a goal that queues the tasks.

**A fridge has state of its own: it is open or closed, and it has a temperature**
(`sim/fridge.rs`, `Fridges`, indexed the same way `Features` is — once, at `GameState::new`,
from the map's `"fridge"` props). The room is +24°C and a fridge cannot get colder than
+4°C; open, its temperature climbs toward the room exponentially (`5` world minutes to close
most of the gap) and, closed, the compressor pulls it back down (`30` world minutes) — never
past either end. **A unit opens a fridge before taking anything out of it and closes it
afterwards**: `EatGoal`/`DrinkGoal` queue `OpenFridge -> TakeItem -> CloseFridge` around the
existing take, and `TakeItem` refuses to start against a closed door (checked once, when the
action starts, not every tick — re-checking would fail every other unit mid-take the moment
anybody shut it). If a unit is put down between opening and closing — the bladder taking over,
say — the fridge **stays open and keeps warming** until somebody closes it: its own goal
closes it first if picked back up still standing beside it, and otherwise the next unit's
`OpenFridge` finds the door already open and that unit closes it when it is done. Since a
task may not write outside its own unit, opening and closing are requests — an `Effect`,
`GameState`'s fourth step, `sim::world_step` — not direct writes; see "The simulation" above.

**A prop can show that it is being used.** A computer's screen is on for as long as
somebody is sitting at it, and dark otherwise — `game/props.rs` swaps the two strips of
its palette entry. **In use is usually a fact about the unit, not about the prop**: an
ordinary feature has no state of its own, which is what lets the feature index be built once
and read from every thread, so the question is asked of the crowd instead —
`GameEntity::interacting_with` names the cell whoever is mid-`Action::Interact` is using,
and a prop standing in one of those cells is in use. Only the units on the canvas are asked
(`VisibleCrowd`), since props are culled to the view too and whoever uses one is beside it
or in it, and nothing is asked at all when no prop on screen could light up. One seam is
left at the very edge: a computer at most half on screen, used from the cell beyond it by
somebody not yet drawn, stays dark until the camera moves another half cell. **The fridge is
the exception**: its door is a fact about `Fridges` itself, so an open-door strip would read
`GameState::fridges()` directly rather than asking who is interacting with it — not built
yet, since there is only the one `fridge.png`.

Every `FeatureKind` also has an `Access`: `Beside` (the fridge and the computer, touched
from next to them — a computer is a desk with chairs on four sides, so nobody queues for
one) or `Entered` (the toilet and the beds, used by walking *into* them). An `Entered` cell stays impassable in
`map::PassabilityMap` — nobody routes through it, and an ordinary walk refuses it exactly
like a wall — but `sim::move_step` lets a `Move` straight onto it through to `Occupancy`,
which is the taken/free state: whoever is standing there holds the claim, and the cell is
free again the moment they leave, the same bookkeeping any other cell already gets from
ordinary movement. A task gets there by starting an `Action::enter`, which sets the walker's
route directly rather than asking for one — the one cell it targets is exactly the one a
search would refuse, and by the time it is asked for it is always a single step away.
`UseToilet` is the pattern for a task built on `Entered` access: it starts by entering, and
once `ctx.here()` is the feature's own cell, carries on exactly as a `Beside` task would.
`Sleep` is the second, and is that shape with a bed for a toilet.

When `UseToilet` fails because the cell was already taken, `blocked_by` names the occupant —
the same signal a `MoveTo` refused by a body in the way carries — so `RelieveGoal` treats the
two failures alike: wait it out (`WAIT_FOR_A_GAP`, up to `PATIENCE` times), then give up and
be held off by `BLOCKED_TICKS` like any other goal that found itself impossible.

#### Biology (`sim/biology/`)

What a body does *by itself* — getting hungry, getting thirsty, a bladder filling — is a
**process**, and `Biology` is a unit's `Stats` plus its processes. Plain Rust, inline in
`Human`, `Copy`; a dog has none.

- **One writer.** A stat is changed by a process and nothing else: `Stats`' setters are
  private to `biology`, so the compiler holds it. A task that finishes a meal says
  `Event::Ingested(item)`, and each process decides what that means — hunger falls by the
  item's `nutrition`, thirst by its `hydration`, and the bladder puts that hydration *on its
  way*. So eating something that also fills the bladder is a change to a process, not a hunt
  through every task that consumes anything. An item says what it is made of, not what it
  does to a body.
- **A process** is a struct in its own file implementing `Process`: `advance(stats, dt)` for
  time passing and `handle(event, novelty, stats)` for something happening to the body. It may keep
  state of its own — `Bladder` holds what has been drunk but has not arrived, filling at
  `FILLING_PER_SECOND` on top of the slow `BLADDER_PER_SECOND`. Processes run in `ProcessId`
  order through `&mut dyn` over `Biology`'s own fields: nothing boxed, nothing allocated.
- **Switches.** Every body has its own `Switches`, a bit per `ProcessId`, all on at spawn.
  **Off means the process does not happen in that body at all**: its stats hold still both
  ways, events reach it and do nothing, and `NeedRoutine` stops wanting the need it drives —
  otherwise a human with hunger switched off at 80 would go back to the fridge forever.
  Switched from code (`Biology::set_running`) or from outside the world with
  `Command::SetProcess`, applied in the spawn pass like a freeze. There is no interface for
  it yet.
- **Recent memory fades** (`biology/recollection.rs`, `Recollection`). Eating something with
  a flavour (`ItemKind::taste` above zero, so not water) and a go at something entertaining
  are each an `Experience` with a **familiarity**: one more each time it happens (capped at
  `MOST_FAMILIAR`, 3), halving every `HOURS_TO_HALF_FORGET` (4 world hours) and forgotten
  outright below `IN_RECENT_MEMORY` (0.1), about thirteen hours after one go — "today".
  `Biology::handle` asks it before the event reaches the processes and hands each one the
  **novelty**, `1 / (1 + familiarity)`: a half for something done just now, never below a
  quarter. A process about the mind scales by it — `Fun` gives `AMUSEMENT * novelty`,
  `Satisfaction` a treat's worth times it — and one about the body ignores it, so a second
  meal of the same food fills exactly as much and is less of a treat. It is not a process:
  it changes no stat and cannot be switched off. Fading is a background task at `VeryLow`
  (below), so it runs every 37th tick and not every tick. When the experience was still remembered,
  `handle` returns it, and the task that caused it (`ConsumeItem`, `UseComputer`) logs
  `... had food in recent memory: 70% as good as fresh` (`tasks::report_recalled`).
- **Satisfaction** (`biology/satisfaction.rs`) is how a person's day is going: it drains
  from content to discontented in 48 world hours and is lifted by a treat — a meal by its
  item's `taste` (`TASTY`, 15), a go on the computer by `ENJOYMENT` (20), each times its
  novelty. Nothing reads it to decide anything yet: it is a readout in the debug menu.
- **Background tasks** (`sim/background.rs`) are upkeep that runs **on a cadence, not on every
  tick**: a `BackgroundTask` names its `Priority`, `High`/`Med`/`Low`/`VeryLow`, every
  3rd/7th/17th/37th tick, and `run(elapsed)` is handed the world time since it last ran.
  **Every unit keeps its own time**: a `Human` carries a `Schedule` — its own seed, rolled
  last at spawn, and one countdown per priority that the seed starts at a different point
  of its period — so different units, and different priorities of one unit, do their
  background work on different ticks, and a crowd's cost is spread evenly over the period
  rather than spiking every 37th tick. `Human::react` ticks the schedule once and hands the
  `Due` it returns to `Biology::background`, which runs the body's tasks (so far only
  `Recollection`). Each countdown adds up the world time it was handed, so a run is owed
  exactly what the unit lived through — a first run two ticks after spawning gets two
  ticks — and a frozen unit's timers stop with it. Pick the priority by how stale the thing
  can go: a rate over hours is `VeryLow`; anything a decision reads the tick it changes is
  not background work. The debug menu shows each countdown (`background in`).
- **Adding a process** is a `ProcessId` variant, a struct and file, a field on `Biology`, and
  its slot in `Biology::parts`/`processes` in id order (a test fails if the slots and ids
  drift). Food reaching the bladder later is a new process between `Hunger` and `Bladder`,
  not a change to eating.

**The rates are in world time, and they are a person's**, which is what the time scale
above is for: full to starving in 8 hours, quenched to parched in 5, an untouched bladder
full in 6, fun draining from a great time to thoroughly bored in 10 and stamina from fully
rested to exhausted in 16 (a waking day, undone by 8 hours in bed) — plus `0.5` of a
bladder per point of hydration drunk, arriving over the half hour after the drink. So a
human eats three or four times a day, drinks rather more often, goes to the toilet after a
drink, looks for something to do about once a day and goes to bed at night; at 1x that is a meal
every couple of minutes of watching, and the speed control is for watching a day go by. Every rate is written as
`100.0 / (hours * HOUR)` in the file that owns it, so the number in the source is the
number of hours.

The doing is in world time too, converted at the point it is defined: 2 minutes to take
food out of a fridge and 15 to eat it, 1 to pour a drink and 2 to drink it, 5 on the
toilet, 30 at the computer, an hour at a time in bed (thirty seconds of watching, and eight of
them are a night) — the longest a go at anything lasts, since having a go at something is
what a person does when nothing else is pressing. Eating and drinking commit at 60 and
release at 25, the toilet at 70 and 10, boredom at 60 and 20; one go is worth 60 points of
fun the first time today (less while the last go is in recent memory, below), so a
thoroughly bored person has a second one the way a starving one eats twice.

**Everybody is born 70 to 100 percent satisfied in every need** (`Stats::random`,
`BORN_AT_LEAST_SATISFIED`): fed, watered, comfortable, entertained, rested and content, worn down by
the world from there. Rolling a need anywhere in its range put a quarter of every crowd
past the line where it goes looking for a meal or a bed on its first tick. It also means
the first hunger is hours of world away, which a test that waits for one has to allow.

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
the `Vec` in slot order and hashes nothing. Each insert is stamped with a serial, so `in_spawn_order` can
answer "the third entity to arrive" — slot order cannot, since a despawn leaves a hole the
next spawn fills. Removal leaves a **tombstone** rather than
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
uv run tools/qa.py                      # every test in qa/, one process each
uv run tools/qa.py qa/create_map.json   # one test
uv run tools/qa.py -v                   # stream the game's log
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

The simulation gets six steps of its own, all intent-level: `{"spawn": {"kind": "human",
"x": 3, "y": 2}}` puts a command on the same queue the game uses, `{"select": 0}` selects
the unit that arrived first (arrival order, not slot order, because a despawn leaves a hole
the next spawn fills — and because a `Uid` is random and a test cannot know one in
advance), `{"give": {"item": "food", "count": 3}}` stows items in the selected unit on that
same queue (the only way anything is stowed today, since no goal puts things away),
`{"hold": "food"}` puts one in its **hand** (`null` empties it) — a hand is only ever full
for the length of a meal, so a test cannot wait for the brain to oblige, and it should tick
few times afterwards, since a hungry unit eats what it is handed and a thirsty one drinks
it in about a second of watching —
`{"process": {"name": "hunger", "on": false}}` switches one of the selected unit's
biological processes off on that queue too — the only handle a script has on what a unit
*wants*, since stats are rolled at spawn (70 to 100 percent satisfied) and only a process may write one, so watching one
need means stopping the others happening — and `{"tick": 200}` runs
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
setting are recorded in the report. `qa/perf_simulation.json`, `qa/perf_rendering.json`
and `qa/perf_huge_map.json` (twenty thousand humans on a 256x256 map) are the three that
exist. **`perf_rendering` is marked `"required": false`**: its frame budget fails on a
machine whose display caps every frame at the refresh rate — an empty map took 16ms a frame
here, on `main` as much as after any change — so `tools/qa.py` still runs it and prints its
numbers, lists it as "not required, failed", and does not fail the run for it. Any test can
carry the flag; reserve it for one whose verdict depends on the machine, not the code.

Assertions are about outcomes — `expect_state`, `expect_focus`, `expect_map`,
`expect_no_map`, `expect_tile`, `expect_zoom`, `expect_speed`, `expect_entities`,
`expect_sprites`, `expect_drawn`, `expect_world_sprites`, `expect_held`, `expect_selected`,
`expect_carrying`, `expect_prop_in_use`, `expect_log`, `expect_world_time` — and
`expect_tile` reads the **saved** map,
so "I painted a wall" is only true once the file says so. `expect_zoom` exists because
zooming changes the size of the canvas rather than the scale of a camera, so a screenshot
cannot be asked how far in it is without counting texels. `expect_speed` takes the string
the readout shows (`x1`, `x0.25`, `paused`) rather than a number and a flag, since
`paused` and `x1` are the same multiplier and a different game. `expect_selected` names the kind (`"human"`, or `null` for nobody) rather
than an id, for the same reason `select` takes an index, and `expect_carrying` counts what
the selected unit has **stowed** — what is in its hand is not stowed, which is the
distinction the whole inventory is built on. `expect_entities` and
`expect_sprites` are deliberately two assertions: the simulation having three entities and
the screen showing three actors are different claims, and the second is the one that
catches a renderer that has quietly stopped keeping up.

**Since sprites are culled, `expect_sprites` means "drawn right now"**, not "exists": a unit
off the edge of the view is supposed to have none. A number asserted there is a claim about
where the camera is, so pin it — pause with `{"key": "p"}` and aim with `{"look_at": {"x":
3, "y": 2}}` (cell units, through the same map clamp as panning, taking effect next frame,
so `wait` or `tick` before asserting). Two assertions carry the culling contract without
naming a number:

- `{"expect_drawn": {}}` — every unit on the canvas has a sprite and no sprite draws
  somebody off it or gone. It is checked against a rect rebuilt from the canvas image and
  the camera rather than read off `VisibleArea`, because an assertion phrased in terms of
  the thing under test can only ask whether the renderer agrees with itself. Holds wherever
  the camera is pointing; actors only.
- `{"expect_world_sprites": {"min": 40, "max": 400}}` — everything on the world layer:
  tiles, props, actors, overlays, parked bodies, the selection frame. `max` is the
  structural form of "what is drawn is bounded by the canvas", and the only check that
  sees a pool growing to the size of the crowd, since a parked body carries no `Actor`.
  **The `min` matters as much**: an empty map satisfies every ceiling there is, and a map
  window still holding entities despawned under it — which is what showed a blank map on
  re-entering a screen, until `reset_map_windows` — draws exactly that. The floor caught
  it before the fix did.

`expect_held` is the same split for
what is carried: it counts the sprites drawing an item, per kind, in anybody's hand, so a
hand that is full and a picture that was never drawn — or was drawn for the wrong item —
are told apart. Placement is not something a count can say, so `qa/held_items.json` also
photographs it; pause first, or the unit wanders out of a zoomed-in frame between steps.
`expect_prop_in_use` is that split again for a prop whose picture says whether somebody is
using it — a computer's screen — and it wants the same pause for a sharper reason:
`FixedUpdate` keeps running between steps, so the tick a shot catches would otherwise
depend on how fast the machine is.

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
src/view.rs             VisibleArea - what is on the canvas, and near enough to it to
                        matter; everything that culls reads it
src/ui/                 UiPlugin, shared widgets; nav.rs (focus), keyboard.rs (typing)
src/menu.rs             MainMenuPlugin
src/editor/             EditorPlugin, background.rs + props.rs; TileWindow and
                        PropWindow draw the map a canvas at a time, on both screens
src/browser.rs          BrowserPlugin - the saved-maps screen
src/game/               GamePlugin - playing a map: camera, zoom, clamped to the map
                        actors.rs is the sim-to-sprite bridge; logview.rs shows the log
                        pool.rs parks bodies that left the view, bounded by the view
                        speed.rs is how fast the world runs; hud.rs the two corners
                        selection.rs is who was clicked; unitpanel.rs the bar about them
                        props.rs lights up a prop while somebody is using it
                        held.rs draws what a hand holds, in front of its carrier
src/map/                the map + coordinate system - plain Rust, no bevy
src/sim/                GameState + spawn_pass/process_pass - plain Rust, no bevy
                        uid.rs, entity.rs, kinds.rs, entities.rs (the arena), log.rs
                        clock.rs is what a second is: 1s watched = 2min of world
                        background.rs is upkeep on a cadence: every 3rd, 7th,
                        17th or 37th tick by priority, on each unit's own
                        seeded Schedule
                        occupancy.rs is who stands where: passability's dynamic half
                        walker.rs is the movement action; feature.rs what props are for
                        brain/ is the mind: routine(s), goal(s)/, task(s)/, action
                        feature.rs is what a prop is for; a computer is somewhere
                        to have a go at something, used from beside it
                        fridge.rs is a fridge's own state: its door, its temperature
                        item.rs is what can be held and what it is made of
                        inventory.rs is what a unit carries: a hand that costs
                        mass only, stowage that costs mass and space, and limits
                        biology/ is the body: Stats, and the processes that alone
                        change them (hunger, thirst, bladder, fun, energy,
                        satisfaction), each switchable; recollection.rs is
                        recent memory, fading, which dulls a treat had twice
src/qa/                 scripted QA: script.rs is the JSON schema, mod.rs replays it
                        perf.rs is the measuring half: statistics, budgets, scaling
src/awake.rs            macOS: hold the display awake so a run can be photographed
src/characters/         how a character is drawn: CELL/ART/upscale, depth_for
                        item.rs is what an item looks like: a texture or an emoji, per kind
src/animation.rs        FrameAnimation (atlas frames) and StripAnimation (Sprite::rect)
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
