---
name: game-screen
description: The crowd2x game screen (src/game/) - the sim-to-sprite bridge in actors.rs, culling to VisibleArea and VisibleCrowd, the body pool, game speed and pause, camera clamping, held items, selecting a unit and the unit panel, selecting and switching a distribution box, the HUD corners and the spawn menu. Use when changing what the game screen draws or how a player interacts with it, sprite culling, selection, panels, HUD buttons or speed controls.
---

# The game screen

Playing a map: it is loaded, drawn, simulated and looked at. The simulation itself is not
here — it is `src/sim/` (the `simulation` skill) — and `game/actors.rs` is the whole of the bridge.

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

### Selecting somebody (`game/selection.rs`, `game/unitpanel.rs`)

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

### Switching a box (`game/boxpanel.rs`, `GameState::switch_box`)

A click on a cell with nobody in it but a distribution box under it selects the **box**
(`selection::SelectedBox`, beside `Selected`; one at a time, and the green frame goes on
whichever it is), and a panel along the bottom says where it is, whether it is on, what it
feeds (`utilities::fed_by`: `feeds 1 fridge, 1 ceiling lamp`) and has a `switch off` /
`switch on` button. Pressing it sends `Command::SwitchBox` on the usual queue, so it lands
in the next spawn pass, which is allowed to allocate: `GameState::switch_box` re-floods
the `Supply`, rebuilds the `Features` index (a fridge or computer on a dead box is not
somewhere to go) and plugs each fridge in or out (`Fridges::set_power`: an unplugged one
warms toward the room from wherever it was), then bumps `supply_generation`. Two readers
follow that number rather than comparing anything: the lighting
(`lighting::follow_power` → `LightScene::apply_supply`, which switches each powered lamp
through the same dirty-chunk path a computer's screen uses — every light needing power is in
the scene from the start, its power part of `enabled`) and the x-ray, which in the game
paints from the simulation's supply, not the map's. A unit already on its way is stopped by
its task: `TakeItem` refuses an unpowered fridge when it starts, and `UseComputer` checks
every tick, so a box switched off mid-go turns the screen off and the go is worth nothing.
QA: `select_box` and `switch_box` steps, and `press` on the panel's buttons.

### The two corners (`game/hud.rs`)

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
