---
name: map-generator
description: Knowledge base for crowd2x's procedural district generator - the seeded 131x100 district of 6x5 blocks, lots and streets (pavement, road, grass parks), the 92 houses with beds, fridges, toilets and resident spawners, the bank, the shops, the mandatory lot buildings (the warehouse and its four scrap bins, on the edge lot nearest the scrap), roofs, lamps, windows, street lamps and their controllers, and the power and sewer networks it lays - its build order, invariants, where each step lives, how it is reached (browser, env var, QA fixture) and how it is tested. Maps every concept to the file and function that owns it (no code). Use when changing or extending the generator, adding a building type or furniture to it, debugging a generated map (sealed rooms, jammed crowds, dead fridges, residents without beds), or changing determinism.
---

# Map generator: `map::district`

Everything is in **`src/map/district.rs`** (plain Rust, no Bevy). Output is an ordinary `Map`
plus a `District` record of where things went; nothing downstream knows it was generated.
For the layers it writes see `map-layers`; for the networks it lays, `electrician` and
`plumber`.

## Entry points

| Who | Where | What |
| --- | --- | --- |
| The API | `district::generate(seed) -> District` | `District { seed, map, houses: Vec<House>, bank: Rect, shops: Vec<Rect>, warehouse: Rect, parks }`, `District::residents()`, `District::buildings()` (every building's walls: houses, shops, warehouse, bank) |
| Browser button `district` | `src/browser.rs` (`Action::District`) | fresh seed, name from the name field or `district <seed>`, saved, opened in the game |
| By hand | `src/debug.rs` | `CROWD2X_DISTRICT=<seed>` (with `CROWD2X_STATE=game`) |
| QA fixture | `src/qa/mod.rs` | `given.maps: [{"name": ..., "district": seed}]` |
| Sim side | `sim::GameState::spawn_from_spawners` | spawns a `"human"` per spawner; `give_a_bed` hands each the nearest unowned bed |

## Geometry (constants at the top of the file)

`BLOCKS_X` x `BLOCKS_Y` = 6 x 5 blocks; a block is 2x2 **lots** of `LOT_W` x `LOT_H` = 8 x 7;
`STREET` = 5 cells between and around blocks: a `PAVEMENT` each side of three cells of `ROAD`,
the main under the middle one, lamps in the road's outer lanes (`CURB` = 1 in from each edge).
`WIDTH` x `HEIGHT` = 131 x 100. **Lots**: 30 blocks less the bank's two and the shops' one
leave `RESIDENTIAL_LOTS` = 108; `LOT_BUILDINGS` (1, the warehouse) take theirs first, `HOUSES`
= 92 of the rest (about 86%), and `EMPTY_LOTS` = `RESIDENTIAL_LOTS - LOT_BUILDINGS - HOUSES`
(15) are parks — all derived, so a geometry change only needs `HOUSES` chosen. Seed 7 has 136
residents (0 has 135, 42 has 136). Helpers:
`block_origin`, `lots_of` (each `Lot` knows its two street-facing `outer` sides, and
`edge_sides()` — those facing the street round the edge, with the wasteland beyond), `Rect`
(`interior`, `on_edge`, `cells`, `contains`), `Side::outward`, `place_in`, `door`, `build`
(walls round, floor inside), `object` (prop centred in a cell), `tile` (name -> id, panics on
an unknown name).

## Build order - `generate` (the order is the determinism)

One `SmallRng` from the seed, drawn in a fixed order. **Steps 1-6 draw from the RNG; 7-10 do
not**, so adding or changing anything in 7-10 never changes a seed's streets, houses or people.
(Changing the geometry — `STREET` going from 3 to 5, the blocks from 4x3 to 6x5 — moves
everything, so every seed's district looks different afterwards; a seed's resident count is
re-read, never assumed.)

0. **The ground** (`pave`, no RNG): the map starts as `ROAD`; every block, the cell round it
   and the map's edge become `PAVEMENT`, so each street reads pavement, road x3, pavement and
   the pavements wrap round the corners. The bank's two blocks and the street between count as
   one block, since the bank stands across it. Walk costs (`map` skill) make the crowd keep to
   the pavement and cross the road straight over.

1. **Pick blocks**: the bank's left block (`bank_x`, `bank_y`; it spans two blocks and the
   street between), then one block for the shops.
2. **`bank`**: walls `BANK_WALL`, floor `BANK_FLOOR`, a 2-wide door in the middle of a long
   side (coin flip which), restrooms at both ends behind a `PARTITION` wall (`RESTROOM_FLOOR`,
   `TOILET`s), fridges along the front wall, desk rows of 1-2 `COMPUTER`s with two-cell aisles.
3. **`shops`**: three lots of the shop block, 7x6 each, `SHOP_WALL`, a random
   `SHOP_FLOORS`, a 2-wide door; empty inside; returns the fourth lot, which becomes a park.
4. **Mandatory lot buildings**, between shuffling the residential lots and splitting off the
   empty ones: one lot each (`take_lot(lots, wanted, rank)`), so the next building is one more
   entry and one builder. So far the **`warehouse`**: on the lot facing the edge street
   (`Lot::edge_sides`) **nearest a heap of scrap** — `heaps_round(seed)`, the seed's own
   wasteland over the navigation area (site + `NAV_MARGIN`), worked out from
   `map::Wasteland` cell by cell before the map has one; ties to the first in shuffled order;
   no RNG drawn. 7x6 `place_in` the lot, `WAREHOUSE_WALL` (`wall brown`) and
   `WAREHOUSE_FLOOR` (`floor diagonal`), a 2-wide plain `DOOR` (public, nobody's property) on
   an edge-facing side — a coin flip when there are two — kept two cells off the corners
   (`door_clear_of_corners`), and a `SCRAP_BIN` in each of the 4 interior corners
   (`WAREHOUSE_BINS`, which is what caps a district at four scrap collectors, `jobs`). Picking
   the lot nearest the scrap rather than the first edge lot took the walk from the warehouse to
   the nearest heap from a median of 55 cells (max 137) to 21-33 for 63 of 64 seeds.
5. **Lots -> houses**: the rest, `EMPTY_LOTS` split off (leaving `HOUSES` = 92), re-sorted
   into street order (so spawner/arrival order reads bottom-left
   first), then **`house`** for each: 1-2 `RESIDENTS`, 7-8 x 6-7 walls (`HOUSE_WALL`,
   `HOUSE_FLOORS`), a 2-wide door on an outer side, and **`furnish`** -> `arrange` /
   `exhaustively`: beds, `FRIDGE`, `TOILET` against the inner walls, spawners beside beds.
   Then the **parks**: the split-off lots and the shops' fourth, each a whole lot of `GRASS`
   (`District::parks`), no RNG.
6. Props and spawners are added to the map only now (painting terrain after a prop would
   recompute passability against it).
7. **`roof_and_light(map, rooms, bank)`**: ceiling over every building rect (walls
   included); one `HOUSE_LAMP` in the middle of each of the `rooms` — houses, shops and the
   warehouse; `OFFICE_LAMP`s in the bank every
   `OFFICE_LAMP_SPACING`; lamps only on see-through cells.
8. **`glaze`** (per building, the warehouse among them): `WINDOW` tiles replace outer-wall cells every `WINDOW_SPACING`,
   never at a corner, beside a door or another window, and only where the cell inside is open
   floor (so not where a partition meets the wall). Windows are impassable but see-through
   (`Terrain::is_see_through`), so light and sight cross them.
9. **`connect`** (power and sewer; the warehouse's lamp gets a box and wiring like any
   building's): `TRANSFORMER` at `(WIDTH-1, 0)`, `SEWER` at
   `(0, HEIGHT-1)`; a `LINE` and a `PIPE` main down the middle cell of every street, skipping
   cells inside any building (the bank sits across a street); each source joined to the ring
   main with `utilities::lay`. Per building, if it holds powered things: `service_entry` picks
   the middle of the inside wall whose straight run outward hits a main soonest without
   crossing another building (`LONGEST_SPUR`), lays that spur as `LINE`, puts a `BOX` at the
   entry and `lay_over_empty` wiring to every consumer inside. The same for toilets with
   `PIPE` and `lay`.
10. **`light_streets`** (street lamps, no RNG, so seeds are unchanged): each street's two
   **sides** — the road's outer lanes, `CURB` in from each edge, either side of the main and
   off the one-cell pavement. **Rows** are wired end to end
   (`wire_run`): `WIRING` on empty cells, and a **junction** `BOX` wherever a line (a main at
   a crossroads, a house spur) crosses, so only buildings and props cut a row. **Columns** are
   wired a block at a time, starting beside the row below and stopping one cell short of the
   row above, so each joins one row's circuit and never ties two together. `STREET_LAMP`s
   every `STREET_LAMP_SPACING` (8) cells, the far side staggered by half, nudged up to 2 cells,
   none within 2 of another (`no_lamp_near`). Then `place_controllers`: one
   `utilities::CONTROLLER` per circuit, on a wiring cell beside a line nearest the circuit's
   middle, with a `BOX` under it; a circuit with no lamp or no site is taken up again. Posts and
   cabinets keep clear of doors (Chebyshev 1) and stand only where `blocks_nothing` says the
   cells round them still join up, and never on the pavement (`can_stand`), so a lamp post
   never sends walkers out into the road. Each side has one controller per street running
   across (one per circuit).

## Invariants (each is a test over `SEEDS` = 0..64)

| Rule | Why | Test |
| --- | --- | --- |
| A door is in a wall facing a street side of its lot | nothing sealed off | `nothing_in_a_district_is_sealed_off` (BFS from (0,0); every prop has a reachable neighbour) |
| Every doorway is `DOOR_WIDTH` (2) door leaves: `"house door"` on houses (locked to the residents), `"door"` on the bank, its restrooms and the shops | one in, one out never deadlock; see the `doors` skill | `every_doorway_is_a_door_at_least_two_leaves_wide` |
| House furniture keeps `ROOM_AROUND` (2) free cells beside it | an occupant can leave past a waiter | `arrange` refuses otherwise |
| Each spawner is beside its own bed and not beside another | nearest-free-bed gives everyone their own | `every_resident_starts_beside_a_bed_in_their_own_house`; sim `everybody_in_a_district_moves_into_a_bed_of_their_own_house` |
| Buildings roofed, streets open sky | lighting | `every_building_is_roofed_and_the_streets_are_open_sky` |
| Lamps: one per house/shop, a grid (>= 12) in the bank | lighting | `every_house_and_shop_has_a_lamp_and_the_bank_a_grid_of_them` |
| Windows only in outer walls, spaced | lighting | `every_building_has_windows_in_its_outer_walls_and_nowhere_else` |
| Every consumer has power/water | a dead fridge starves a house | `everything_that_needs_power_or_water_in_a_district_has_it` |
| No line indoors; outdoor wiring only on circuits with a controlled box and nothing indoors | voltages meet only in a box; street lights never share a house's circuit | `a_power_line_never_runs_inside_a_building_and_wiring_never_outside_one` |
| Street lamps powered at night only (controlled boxes off = day), everything else all day; >= 30 lamps; 1..=10 controllers | street lights, and not a cabinet per stretch | `the_street_lamps_have_power_at_night_only_and_everything_else_all_day` |
| >= 90% of main cells within a street lamp's radius | the streets are lit | `the_streets_are_lit` |
| Counts: 92 houses, 3 shops, spawners = residents, beds/fridge/toilet per house, >= 12 computers and >= 2 toilets in the bank; one warehouse holding its 4 bins and nothing else, no bin elsewhere, nobody's property, on the edge, on the edge lot to be had nearest the scrap | the brief | `a_district_has_what_it_was_asked_for` |
| The warehouse's door opens onto the edge street (or the yard cell before it), facing out | collectors step out towards the wasteland | `the_warehouse_door_opens_onto_the_street_round_the_edge` |
| Every street off the crossroads reads pavement, road x3, pavement | people keep to the pavement | `every_street_is_a_pavement_each_side_of_three_cells_of_road` |
| Parks are exactly the empty lots and the shops' fourth, whole lots of grass, with nothing built on them | green zones | `the_empty_lots_are_parks_and_nothing_else_is_grass` |
| Nothing on the pavement but the transformer and the sewer (two corners) | a post on a one-cell pavement sends everyone into the road | `nothing_stands_on_the_pavement_but_the_transformer_and_the_sewer` |
| Routes between front doors (seeds 0..8): >= 80% of outdoor cells pavement, no road run longer than a crossing (3) | people prefer pavements | `people_keep_to_the_pavement_and_cross_the_road_straight_over` |
| Every name used exists in the catalogues | no panic in `tile` | `every_tile_and_prop_the_generator_uses_exists` (`TILES`, `BEDS`, `FRIDGE`, ...) |
| Same seed same bytes; different seed different | reproducible | `the_same_seed_builds_the_same_district`, `a_district_survives_being_saved` |

Every invariant test that builds a building list uses `District::buildings()`, so a new
building is in all of them at once.

QA: `qa/district_browser.json` (the button), `qa/district_lighting.json` (GPU vs CPU lightmap
on a district), `qa/perf_district.json` (a tick with 1000 and 4000 wanderers on a district,
where routes cross roads; it tops seed 7's 136 residents up to 1000, so it names the count).
`qa/district.json` observed a day with seed 7's resident count and camera spots hard-coded and
needed re-adjusting with every generator change: deleted when the district grew. The sim
side: `sim::tests::everybody_in_a_district_moves_into_a_bed_of_their_own_house` (beds, every
desk in the bank, then the four bins in the warehouse) and
`a_district_on_a_monday_stores_scrap_in_its_warehouse`.

## Recipes

- **New furniture in houses**: add it to `arrange` / `Furnishing` (it must keep `ROOM_AROUND`
  and reachability), add its name constant to the `every_tile_and_prop...` list. If it needs
  power or water, `connect` already wires every consumer inside a building - nothing else to do.
- **New building type**: a builder like `shops` drawing from the RNG at a fixed point (this
  **changes every seed after it** - say so in the commit), return its `Rect`, include it in
  the `rooms` list `generate` builds (roof, lamp, `glaze`, `connect`, `light_streets` follow)
  and in `District::buildings()`.
- **New mandatory building** (one every district must have, on a lot of its own): `LOT_BUILDINGS`
  + 1, and in the mandatory step a `take_lot` (which lots qualify, how they rank) and a builder
  like `warehouse`; a `District` field; `rooms` and `buildings()`; and `HOUSES` re-chosen if
  `EMPTY_LOTS` would go negative.
- **A seed-stable addition** (decoration, signage, street furniture): do it after step 5 from
  the built rects without touching the RNG - the `roof_and_light` / `glaze` / `connect` pattern.
- **New ground** (gravel, a plaza): a `Terrain::ground(name, cost)` in `map::terrain`, art in
  the editor palette (`tools/ground_art.py`), its name in `TILES`, painted after `pave`.
- **Bigger districts**: `BLOCKS_X`/`BLOCKS_Y` and `HOUSES`; `RESIDENTIAL_LOTS` and `EMPTY_LOTS`
  follow, and `LONGEST_SPUR` if lots grow; the transformer/sewer corners follow
  `WIDTH`/`HEIGHT`. Re-read seed 7's resident count for `qa/perf_district.json`.

## Traps

- Anything placed on a street cell blocks it (the transformer and sewer take two pavement
  corners; street lamps and controllers take road cells in the outer lanes, never pavement);
  `nothing_in_a_district_is_sealed_off` starts its BFS at `(0, 0)`, which must stay passable.
- Painting terrain checks props already in the map; that is why props are collected and added
  after the terrain (step 5) - `glaze` runs after them and only changes wall cells.
- The bank spans the street between its two blocks: anything that runs along streets must skip
  building rects.
- A brain picks the nearest feature as the crow flies; crowds converge on the bank's nearest
  fridge and computers. That is a brain limit, not a generator bug — but at 136 residents it
  breaks the district. Seed 7, needs on, measured over three days (2026-10-18): the 53-resident
  district worked 41, 22 and 13 office hours a day with at most one resident starving and
  parched at midnight; the 136-resident one works 35 on Monday and **none after**, with 19, 27
  and 10 starving and parched at midnight, everybody piled up in the bank's lobby (its fridges,
  restrooms and the computers the bored go to play on) — and its scrap collectors, caught in
  it, store nothing all week. Fixing it is the brain's (crowd-aware choice of fridge, toilet and
  computer, or residents keeping to their own house), or more public places to spread over.
