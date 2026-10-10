---
name: map
description: The crowd2x map model (src/map/) - Point/Size addressing, terrain catalogue, object layers (props, spawners, lamps), the PROPS blocking catalogue, derived passability and sight, windows, the Supply of power and water, a summary of the district generator, and the JSON map file format and its no-repair loading rules. Use when changing the map data model, passability, the map format, catalogues of terrain or props, or spawners. For layers in depth read map-layers; for the generator, map-generator.
---

# The map

The first piece of simulation state, so it follows the simulation rule: **plain Rust, no
`bevy::` imports**, testable with `cargo test` and no `App`.

- All addressing is `Point { x: i32, y: i32 }` in whole cells — not `usize` (neighbours
  and deltas go negative constantly) and not `f32` (a cell is a hash key and a path node,
  so it has to compare exactly). Y increases upward like world Y, so the eventual drawing
  adapter is a scale and never a flip. `Size` owns the one row-major `index_of`, so the
  layers and the passability grid cannot disagree about the layout.
- A map is dimensions plus layers: **terrain** layers where every cell of every layer is
  defined (there is no empty cell, only the `VOID` tile — so no consumer handles a hole),
  and sparse **object** layers, `Props`, `Spawners` and `Lamps`. Only the base terrain
  layer exists so far. Beside them, **grid layers** (`map/grid.rs`, `GridLayer`): a byte
  per cell, authored, from a short alphabet that is also how a file writes it, and in
  nobody's way — `Ceiling` (`.#`, read by lighting), and under the floor `Power` (`.-=B`:
  wiring, power line, distribution box) and `Water` (`.o`: a pipe). `Map::grid`/`set_grid`
  are the whole interface (`has_ceiling` is a wrapper). A new map has nothing in any of
  them, which is what every map made before them still is. Adding one is a variant, its
  alphabet, a palette in `editor/grids.rs` and a row in `editor::LAYERS`; the file format
  and the overlay pick it up from there. **A spawner names an entity kind** (`"human"`), and the game screen spawns one
  in its cell when the map is played (`GameState::spawn_from_spawners`, called by
  `game/actors.rs` — `GameState::new` itself still brings nobody). An unknown kind is
  logged and skipped. The editor has no spawner tool; it keeps the ones a map has.
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
  exceptions, used by entering, and `brain-engineer`'s `architecture.md` ("The brain") says how a cell can stay impassable
  here and still be walked into by exactly one unit.
- **Power and water are derived, never authored** (`map/utilities.rs`, `Supply`), from
  the two networks and the props standing on them. A `"transformer"` feeds the **power
  line** in its own cell; a **distribution box** the line reaches feeds the **wiring**; a
  lamp, fridge or computer is powered when live wiring runs under its own cell. A line
  straight into a fridge does not count, and neither does wiring no box feeds. A
  `"sewer"` drains the pipes in its cell, and a toilet with a live pipe under it works.
  The transformer and the sewer are props — they block their cell like any other — *and*
  nodes of their networks in that same cell; a box and a pipe are only in the network.
  `Supply::from_map` is three flood fills over the grid (the map's area, never what stands
  on it), done once by `GameState::new` and again by the editor's overlay on an edit.
  **A box can be switched off during play** (`Supply::with_boxes_off`): it still carries
  the line through, but feeds no wiring. Which boxes are off is simulation state, not the
  map's (a map is saved with every box on) — see "Switching a box" in the `game-screen` skill.
  `Supply::everywhere` serves everything, for a test whose wiring is beside the point
  (`sim::testing::World::new` uses it; `World::as_built` asks the wiring). `CONSUMERS` and
  `SOURCES` are the catalogue, by name. `serve_everything` wires a map the shortest way,
  for tests; `tools/wire_map.py` does the same to a map file or the maps inside a QA script,
  and is what wired the fixtures when the networks arrived.
- **Windows** (`"window"` terrain, `Terrain::pane`) are impassable but see-through: `Map::sight`
  follows `TerrainId::is_see_through`, not passability, so perception and light cross one. The
  lightmap treats a window cell as roofless whatever the ceiling layer says
  (`TerrainId::lets_sun_in`), so the sky propagates in from it. The district generator glazes
  every building's outer wall (`glaze`, last, no seed drawn, so no district changed otherwise).
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

**The district generator** (`map/district.rs`, plain Rust like the rest of `map/`):
`district::generate(seed)` builds a 79x54 district — a grid of 2x2-lot blocks with
3-cell streets between and around them, everything outside a building paved — holding 32
houses of 1-2 residents (a bed each, a fridge, a toilet, and a `"human"` spawner beside
each bed), a bank across two merged blocks (a lobby lined with fridges, desk rows of
computers, two restrooms at each end) and three empty shops. One `SmallRng`, drawn in a
fixed order, so a seed is a district. Three rules keep it livable, and its tests check
them over 64 seeds: **a door is always in a wall facing the lot's street side** (so
nothing is sealed off), **doors are two wide** (one person in and one out of a one-cell
door wait for each other forever), and **house furniture keeps two free cells beside it**
(whoever is in a toilet or bed cannot leave past somebody waiting in its only doorway). A
spawner one step from its own bed and two from any other is what makes the nearest-free-bed
rule hand each resident its own. **Every building is roofed, walls included, and the
streets and yards are open sky**; a house and a shop get one `"ceiling lamp"` in the middle,
the bank a grid of `"tube lamp"`s every `OFFICE_LAMP_SPACING` cells — laid out last, from the
buildings, without drawing on the seed, so adding them changed no district. **Power and water
are laid the same way, after that**: a transformer in the bottom-right corner, a sewer in the
top-left, a power line and a sewer main down the middle of every street (broken where the bank
stands across one), and for each building a box and a pipe entry just inside the wall nearest
a main, a straight run out to it, and wiring or pipes inside to everything that needs them. A
line never runs indoors and wiring never outdoors; both are tested over 64 seeds, as is every
consumer being served. Reached from the browser's `district` button (a fresh
seed, saved and opened in the game), `CROWD2X_DISTRICT=<seed>`, and a QA fixture
`{"name": ..., "district": seed}`. **Known limit:** a brain goes to the nearest fridge or
toilet as the crow flies and has no idea of a crowd, so the district's crowd — drawn to the
bank's computers — converges on one fridge and jams; `qa/district.json` observes a day and
records the numbers instead of asserting on needs.

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
- **A grid layer is a row of characters per map row**, bottom-up like the terrain, at the
  top level under its name (`"ceiling"`, `"power"`, `"water"`), one character of its
  alphabet per cell. A layer with nothing on it is left out — as is an empty `lamps` list —
  so a map saved before it existed saves back byte for byte and `FORMAT_VERSION` stays 1.
  An unknown top-level key is `UnknownGridLayer`, not ignored.
- **Nothing is repaired on load.** A short row, an unknown object layer, a size of zero,
  an object off the map — each is a `MapFormatError`, because padding or dropping produces
  a map that looks right and isn't. `FORMAT_VERSION` is checked first.
