---
name: map
description: The crowd2x map model (src/map/) - Point/Size addressing, terrain catalogue, object layers (props, spawners, lamps), the PROPS blocking catalogue, derived passability and sight, windows, the Supply of power and water, a summary of the district generator, and the JSON map file format and its no-repair loading rules. Use when changing the map data model, passability, the map format, catalogues of terrain or props, or spawners. For layers in depth read map-layers; for the generator, map-generator.
---

# The map

The first piece of simulation state, so it follows the simulation rule: **plain Rust, no
`bevy::` imports**, testable with `cargo test` and no `App`.

The map is a finite **site** standing in an endless **wasteland** (`map::wasteland`,
`map::chunk`): every cell in the world answers `is_passable`, `terrain` and the rest, and
`navigation()` folds everything joined to the site's surroundings into one outside
(`Navigation::is_outside`). All of that is the `wasteland` skill.

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
- **Walk cost** lives in the catalogue too (`Terrain::cost`, `TerrainId::walk_cost`,
  `Map::walk_cost`): what a route pays to step into a walkable cell. Floors and
  `"pavement"` cost `MIN_WALK_COST` (1), `"grass"` 3, `"road"` 5; anything impassable 0.
  Routes are *cheapest*, not shortest (`sim::path`), which is how the crowd keeps to the
  pavement and crosses the road straight over. It is read straight off the terrain, not
  kept as another derived layer: a prop changes whether a cell is walkable, never what it
  costs. **No walkable tile may cost less than `MIN_WALK_COST`** — the A* heuristic assumes
  it, and `nothing_walkable_costs_less_than_the_cheapest_step` holds the catalogue to it. A
  new ground is `Terrain::ground(name, cost)`, appended to the end of `TERRAIN` (names are
  what files store; the order is not) with art in the editor palette.
- **Good soil** is a column of the catalogue too (`Terrain::fertile`, `TerrainId::is_fertile`;
  `Terrain::soil(name, cost)`): `"meadow"`, and nothing else yet. A crop of wheat
  (`sim::fields`, the `jobs` skill) goes only in a **plot** — `map::is_plot(cell)`, both
  coordinates even, so no two crops touch — of good soil with nothing in the way and **the eight
  cells round it open**: `Map::can_sow`. None of the eight is a plot and nothing in play closes a
  cell, so a crop never closes a one-cell gap (a lake, a heap, a wall beside it) and no field can
  shut a cell in; on a map with nothing round it, the plots on its edge cannot be sown. **A crop blocks its cell**, as a heap of scrap does:
  `Map::plant(cell)` / `unplant(cell)` keep the map's `planted` set (keyed by chunk like the
  salvage tallies, on the site or off it), clear or set the site's passability bit or a loaded
  chunk's (`Chunk::block` / `clear`), and `load_chunk` and the unloaded fallback respect it.
  `Map::is_open_ground` counts a planted cell as open, so **planting never touches
  `Map::navigation`**. Not in a map file, nor in a save's chunks: the simulation's fields plant
  the map again on load. `set_wasteland` forgets what was sown off the site.
- **Props block.** A static `PROPS` catalogue (`map/props.rs`, the same shape as
  `TERRAIN`, linked to the editor palette by name) says whether each prop can be walked
  through: furniture cannot — beds, the fridge, the toilet, the computer, crates, fire — and the two doors (`"door"`, `"house door"`) can, since a door's leaf is simulation state, not the map's (the `doors` skill).
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
  A `"street light controller"` prop standing on a box is the daylight switch of every box
  on that box's circuit (`utilities::controlled_boxes`); the sim opens them by day, so the
  `"street lamp"`s on the circuit are lit at night only (`electrician`).
  A `"light switch"` is a **wall-mounted** prop (`Prop::on_wall`, `wall_mounted: true`,
  `ObjectKind::is_wall_mounted`): passable, on a free floor cell beside a wall, kept on that
  wall's edge of the cell (`map/mount.rs`: `can_mount`, `wall_of`, `mounted_at`, `mount`).
  It needs no cable; off, the `SWITCHED` lamps of its roofed room are not served
  (`Supply::with_lights_off`, rooms in `sim::light_switch`) (`electrician`).
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

**The district generator** (`map/district/`, plain Rust like the rest of `map/`):
`district::generate(seed)` builds a 131x100 district — a 6x5 grid of 2x2-lot blocks with
5-cell streets between and around them (pavement, three cells of road, pavement; yards
paved, the empty lots grass parks) — holding 92
houses of 1-2 residents (a bed each, a fridge, a toilet, and a `"human"` spawner beside
each bed), a bank across two merged blocks (a lobby lined with fridges, desk rows of
computers, two restrooms at each end), three empty shops, and the mandatory lot buildings —
so far the warehouse, four `"scrap bin"`s on the edge lot nearest the scrap. One `SmallRng`, drawn in a
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
a main, a straight run out to it, and wiring or pipes inside to everything that needs them.
Last, street lamps along both sides of every street on a few long circuits, one controller
each (`light_streets`). A
line never runs indoors and wiring never outdoors; both are tested over 64 seeds, as is every
consumer being served. Reached from the browser's `district` button (a fresh
seed, saved and opened in the game), `CROWD2X_DISTRICT=<seed>`, and a QA fixture
`{"name": ..., "district": seed}`. **Known limit:** a brain goes to the nearest fridge or
toilet as the crow flies and has no idea of a crowd, so the district's crowd — drawn to the
bank's computers — converges on one fridge and jams, and 136 residents jam it worse than the
old 53 did (`map-generator`).

`MapStore::save` writes to a unique temporary file in the same directory, syncs and
closes it, then atomically renames it over the destination. A failed write or replacement
preserves the previous save and removes the temporary file when possible. Never truncate
the destination first. This protects replacement, not full power-loss durability of the
directory entry. The editor must handle the returned error before leaving or closing.

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

## In a saved game (`map/save.rs`)

A `.savegame` holds its own copy of the site (`Map::save_site` / `restore_site`, `map.pb`):
everything a map file holds, by name — a terrain palette and indices, grid layers as raw
values checked against each layer's alphabet, objects by name — rebuilt in the same order
`from_json` builds a map, so passability is derived and never trusted. The wasteland round it
is `save_chunks` / `restore_chunks` (`wasteland` skill). `salvaged` is never in a map file but
is in a save. The rest: `savegame` skill.
