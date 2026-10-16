---
name: wasteland
description: Knowledge base for crowd2x's endless world - the procedural wasteland round every map (dirt, stones, cracks, scrub, meadow, mud, lakes) as a pure function of a seed, the 32-cell chunks it is cached in and how they are streamed round the camera and round the crowd, the map file's "wasteland" seed, routes, reachability and occupancy off the site, spawning out there, the unclamped camera and its reach, depth and lighting far from the origin, the editor's dimmed wasteland, its tiles and their art. Maps every concept to the file and function that owns it (no code). Use when changing the wasteland generator or its tiles, chunk loading and streaming, anything a unit does off the site, the camera far from the map, or when something breaks only a long way from the origin (sprites missing or under the ground, units stuck in a lake pocket, searches failing out there).
---

# Wasteland: the world past the map's edge

A map is a finite **site** (the cells somebody authored, or the district generator laid out).
Past its edge the world goes on in every direction as bare ground. Nothing is built out
there; it is terrain only. Every map loaded from a file stands in one.

## The generator: `src/map/wasteland.rs`

- **`Wasteland::tile(cell, site)` is the whole of it**: a pure function of seed and cell,
  with no state and no history. Nothing about the ground needs storing; a map file holds the
  seed.
- Value noise at several scales (`value`, `fbm`), built from integer hashing and `f32` adds
  and multiplies only, so a seed is the same world on every machine. No `sin`, no `exp`.
- Three fields: **wet** (lake where highest, mud on the shore), **green** (meadow, scrub
  round it, raised near water), **dry** (cracked dirt). A per-cell hash scatters stones.
  The thresholds are the `*_ABOVE` constants.
- **The site keeps a clearing.** Water fades out near the site and is gone within
  `CLEARING` (12) cells, so whatever leads off a map's edge opens onto dry ground.
- `TILES` lists the seven tiles. `MIN_COST` is the cheapest wasteland step, which
  navigation uses as the outside's minimum.
- Tiles live in `src/map/terrain.rs` (`DIRT` .. `WATER`, named constants by catalogue
  index). **Water is a pane: seen across, never walked.** The art is drawn by
  `tools/wasteland_art.py` into `assets/{dirt,dirt_stones,dirt_cracked,scrub,meadow,mud,water}.png`
  at 16x16 (see `pixel-artist` for the rules).

## Chunks: `src/map/chunk.rs`

- `CHUNK` = 32 cells. `ChunkPos` (`of`, `covering`, `around`), `Chunk` (tiles plus passable
  and see-through bits, built by `Chunk::generate`), and `ChunkGrid<T>`, a flat grid of
  boxed slots: a lookup is an offset, never a hash, and a skipped chunk costs a pointer.
- **Loaded only means cached.** A chunk holds a pure function of seed and position, so
  loading or dropping one never changes a cell. A cell whose chunk is not loaded reads the
  same, only slower (`Map::ground` falls back to the generator). That is what keeps
  loading out of the simulation's results (`sim` test `what_is_loaded_never_changes_what_happens`).
- `Map` (`src/map/mod.rs`): `with_wasteland` / `set_wasteland` (setting one drops every
  chunk), `wasteland`, `is_endless`, `site`, `load_chunk(s)`, `unload_chunks`,
  `loaded_chunks`, `is_chunk_loaded`. A chunk entirely on the site is never loaded.
  `is_passable`, `is_see_through`, `terrain` and `walk_cost` answer for any cell in the world.
- `WORLD_REACH` (50000 cells) is how far from the origin the world is guaranteed to
  behave. Past it, `f32` positions start to give.

## Who loads what

- **The camera**, on both map screens: `editor::background::stream_chunks` loads the
  chunks under the canvas plus `STREAM_AHEAD` (1), and drops those more than `STREAM_KEEP`
  (4) behind. It loads into `CurrentMap` with `bypass_change_detection`, because loading
  is not an edit. The HUD's `where x, y  chunks n` line shows the camera's cell and the count.
- **The crowd**: `GameState::follow_the_crowd_out` (`src/sim/mod.rs`) loads the chunk under
  everybody off the site, and its neighbours, into the simulation's own copy of the map.
  This is spawn-pass work because it allocates. Nothing is unloaded.
- **Occupancy** (`src/sim/occupancy.rs`) keeps off-site claims per chunk (`WildCells`).
  `prepare(cell)` makes them ahead of a move so the move never allocates. `wild_chunks`
  counts them.

## Routes and reachability off the site

- **Navigation** (`src/map/navigation.rs`) works out components cell by cell over
  `Map::navigation_area`: the site plus `NAV_MARGIN` (2 chunks). Every component touching
  that area's edge is folded into one **outside** component, which also holds every cell past it.
  So the two sides of a wall across the site meet round it, and a room walled in stays sealed.
- `Navigation::is_outside(cell)`: whether a cell is open wasteland rather than a pocket a
  lake has walled in. **Ask it before putting a unit out there**: from a pocket nothing is
  reachable, and the search answers `Unreachable` correctly. Past the margin every cell
  counts as outside, so a pocket far out is not detected (the bounded search below then fails).
- **Search** (`src/sim/path.rs`): on an endless map a search is confined to
  `path::window(map, start, goals)`, the bounds of start and goals grown by
  `SEARCH_MARGIN` (one chunk). A route far out is searched in a window of its own. Water
  is never a destination and never floods the world.
- **Perception** reads terrain see-through, not passability: a lake is seen across.
- **Wandering** (`brain::goals::wander::WanderGoal::pick`) asks `may_reach` of every
  random candidate. Without it, a unit in a walled building at a map's edge picked the
  wasteland past its wall most of the time, failed the route, was held off, and mostly
  stood still (`qa/doors.json` caught it; `sim` test
  `a_wanderer_shut_in_at_the_edge_of_the_site_still_walks_its_room`). Any new goal that
  picks random cells near the site should do the same.

## Spawning out there

- The game's spawn menu (`game::hud::spawn_points`) widens a square round the camera until
  it has enough cells. Off the site it takes only passable cells that are `is_outside`;
  on the site, any passable cell.
- The sim tests' `dry_cells` helper does the same. A test that drops somebody on the
  nearest passable cell can land them in a pocket and fail for that reason alone.

## The camera and drawing far from the origin

- **No clamp to the map on an endless map**: `game::clamp_to_reach` stops the camera only
  at `WORLD_REACH`. A map with nothing round it is still clamped to its edges
  (`clamp_to_map`). QA tests that relied on the clamp moving the camera need updated
  coordinates (`qa/culling.json`).
- **Depth.** Painter's order is `characters::depth_for(y) = -y * 0.01`, which is unbounded
  in an endless world. So **every flat layer is an offset from `depth_for` of its own y**,
  never a fixed z: ground `BACKGROUND_DEPTH` (-1000), x-ray `CEILING_DEPTH` (400) and
  `UNDERGROUND_DEPTH` (600), lamps `LAMP_DEPTH` (500), rectangle preview (900) and editor
  cursor (1000). The world camera sees `render::WORLD_DEPTH` (1e6) either side, not Bevy's
  default 1000. With fixed z values, units were drawn under the ground past cell y ≈ 208
  and clipped away entirely past |y| ≈ 2083 (`qa/wasteland.json` photographs both spots).
- **Lighting**: the lightmap covers the site plus `lighting::scene::LIGHT_MARGIN` (8
  cells, more than a street lamp's radius), with `LightScene::origin` at its corner, so
  lamps at the edge light the ground beside them. Past it is open sky with no lamp. The
  canvas shader takes `lightmap_origin` (`lit_canvas.wgsl`).
- **Editor**: the wasteland is drawn dimmed (`background::OFF_SITE_TINT`) and cannot be painted.

## The file format

`"wasteland"` in a map file (`src/map/format.rs`) is a seed, or `false` for nothing round
the site. **Absent means the default seed** (`wasteland::DEFAULT_SEED`, 0), and saving the
default omits the key, so a map made before the wasteland saves back byte for byte. Anything
else is refused with `"wasteland" is …, expected a seed or false`. New maps from the browser
get the default. A generated district's wasteland is seeded by the district's own seed and
added last, so it changes nothing laid out on the site.

## Tests

- `map::wasteland`, `map::chunk`: determinism, the clearing, the grid.
- `map::navigation`: walls met round, sealed rooms, `is_outside`.
- `sim::path`: routes round the site, windows far out, water.
- `sim::occupancy`: off-site claims, `prepare`.
- `sim` (`mod.rs`): a crowd far out loads its ground; loading never changes the outcome; a
  hungry human walks home from the wasteland.
- `lighting::scene`: `an_endless_maps_lightmap_reaches_out_into_the_wasteland`.
- `map::format`: old maps load surrounded; seed or `false` round-trips; junk refused.
- QA: `qa/wasteland.json` (streaming bounds via `expect_chunks`, dogs far out drawn,
  the editor's dimmed edge) and `qa/district_lighting.json` (lamps over the edge at night).
