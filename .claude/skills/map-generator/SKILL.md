---
name: map-generator
description: Knowledge base for crowd2x's procedural district generator - the seeded 194x104 district of 9x5 blocks, lots and streets (pavement, road, grass parks; the wider main street, gate to gate along the palace), the 124 houses with beds, fridges, toilets and resident spawners, the palace (the whole middle block, the king's throne), the bank, the shops, the mandatory lot buildings (the warehouse and its four scrap bins, on the edge lot nearest the scrap; four public restaurants with fridges and toilets, one to each quarter; the crematorium and its two crematoria, on the edge; the steel factory with its furnaces, scrap hoppers and steel pallets, on the lot nearest the warehouse, and the warehouse's steel rack; the farm and its eight grain bins, on the edge lot nearest the good soil; the power plant, the district's only source of power, with its two generators and its control panel, on the edge), the perimeter wall with its 8 gates and their guard posts, roofs, lamps, windows, street lamps and their controllers, and the power and sewer networks it lays - its build order, invariants, where each step lives, how it is reached (browser, env var, QA fixture) and how it is tested. Maps every concept to the file and function that owns it (no code). Use when changing or extending the generator, adding a building type or furniture to it, debugging a generated map (sealed rooms, jammed crowds, dead fridges, residents without beds), or changing determinism.
---

# Map generator: `map::district`

Everything is in **`src/map/district/`** (plain Rust, no Bevy): `mod.rs` has the geometry, the
streets, the bank, the shops, the houses, the gates and the services, `palace.rs` the palace (a
block of its own, not a lot building), and **each lot building is
a module of its own** — `warehouse.rs`, `restaurant.rs`, `crematorium.rs`, `steel_factory.rs`,
`farm.rs`, `power_plant.rs`, `playzone.rs` —
listed in `LOT_BUILDINGS`. Output is an ordinary `Map` plus a `District` record of where things
went; nothing downstream knows it was generated.
For the layers it writes see `map-layers`; for the networks it lays, `electrician` and
`plumber`.

## Entry points

| Who | Where | What |
| --- | --- | --- |
| The API | `district::generate(seed) -> District` | `District { seed, map, houses: Vec<House>, bank: Rect, shops: Vec<Rect>, palace: Rect, lot_buildings: Vec<(&str, Rect)>, parks, gates: Vec<Gate> }` (`Gate { leaves: [Point; 2], post, outward }`), `District::residents()`, `District::the(name)` (the one lot building of that name: `district.the(warehouse::NAME)`), `District::all(name)` (every one: the restaurants), `District::buildings()` (every building's walls: houses, shops, the lot buildings in order, bank, palace) |
| Browser button `district` | `src/browser.rs` (`Action::District`) | fresh seed, name from the name field or `district <seed>`, saved, opened in the game |
| By hand | `src/debug.rs` | `CROWD2X_DISTRICT=<seed>` (with `CROWD2X_STATE=game`) |
| QA fixture | `src/qa/mod.rs` | `given.maps: [{"name": ..., "district": seed}]` |
| Sim side | `sim::GameState::spawn_from_spawners` | spawns a `"human"` per spawner; `give_a_bed` hands each the nearest unowned bed |

## Geometry (constants at the top of the file)

`BLOCKS_X` x `BLOCKS_Y` = 9 x 5 blocks (**both odd**, a const assert: the palace needs a middle
block); a block is 2x2 **lots** of `LOT_W` x `LOT_H` = 8 x 7.
**Streets** are a `Street { start, width, pavement }` each — `across(k)` the `k`th running across
(0 round the bottom edge, `BLOCKS_Y` round the top), `up(k)` the `k`th running up; `end()`,
`road()`, `middle()` (where the mains run), `sides()` (the road's two outer lanes, where lamps and
their wiring stand), `across_at(y)`. Every street is `STREET` = 5: a `PAVEMENT` each side of three
cells of `ROAD`; **the main street**, `across(MAIN)` with `MAIN = BLOCKS_Y / 2` (the street below
the middle row, along the palace's front), is `MAIN_STREET` = 9: `MAIN_PAVEMENT` = 2 of pavement
each side of five of road. `block_origin` / `block(bx, by)` and `WIDTH` x `HEIGHT` = 194 x 104
follow from the streets, so **nothing may assume streets evenly spaced** (`k * (BLOCK + STREET)`
is wrong past the main street). **Lots**: 45 blocks less the bank's two, the shops' one and the
palace's one leave `RESIDENTIAL_LOTS` = 164; the lot buildings (`LOT_BUILDINGS`, 9 lots: the
warehouse, `RESTAURANTS` = 4, the crematorium, the steel factory, the farm, the power plant)
take theirs first, `HOUSES` = 124 of the rest (four in five), and
`EMPTY_LOTS` = `RESIDENTIAL_LOTS - lots_built() - HOUSES` (31) are parks — **room for lot
buildings to come** — all derived, so a geometry change only needs `HOUSES` chosen. Seed 7 has
184 residents (re-read after the district grew to 9x5; pinned by
`the_tvs_draw_nothing_from_the_district_...` and named by `qa/perf_district.json`). Helpers:
`block_origin`, `block`, `lots_of` (each `Lot` knows its two street-facing `outer` sides, and
`edge_sides()` — those facing the street round the edge, with the wasteland beyond), `Rect`
(`interior`, `on_edge`, `cells`, `contains`), `Side::outward`, `place_in`, `door`, `build`
(walls round, floor inside), `object` (prop centred in a cell), `tile` (name -> id, panics on
an unknown name).

## Build order - `generate` (the order is the determinism)

One `SmallRng` from the seed, drawn in a fixed order. **Steps 1-6 draw from the RNG; 7-10 do
not**, so adding or changing anything in 7-10 never changes a seed's streets, houses or people.
(Changing the geometry — `STREET` going from 3 to 5, the blocks from 4x3 to 6x5, then to 9x5
with the main street and the palace's block — moves everything, so every seed's district looks
different afterwards; a seed's resident count is re-read, never assumed.)

0. **The ground** (`pave`, no RNG): the map starts as `ROAD`; every block and round it as far
   as the street beside it has pavement (a cell; two on the main street's side) become
   `PAVEMENT`, and **the map's edge becomes the `PERIMETER_WALL`** (`wall brown big`),
   so each street reads pavement, road x3, pavement — the street round the edge wall, road x3,
   pavement; the main street pavement x2, road x5, pavement x2 — and the pavements wrap round
   the corners while the road runs on through the crossroads. The bank's two blocks and the street between count as
   one block, since the bank stands across it. Walk costs (`map` skill) make the crowd keep to
   the pavement and cross the road straight over.

1. **Pick blocks**: the palace's is **always the middle one** (`palace::BLOCK`, no draw); then
   the bank's left block (`bank_x`, `bank_y`; it spans two blocks and the street between), drawn
   from the pairs that leave the palace's block alone, then one block for the shops from the rest.
1b. **`palace::palace`** (no RNG): **the whole middle block** (16x14), `wall red` and `floor
   white`, a plain 2-leaf `DOOR` in the middle of its south wall — onto the main street's
   pavement — laid out with `Room`: the `THRONE` in the middle of the far wall across the hall
   from the door (`THRONES` = 1, the king's workplace, `jobs`), and a 2-wide `CARPET` (`floor
   red`) from the door to it. Lit by a grid of ceiling lamps every `palace::LAMP_SPACING` (5)
   cells (`roof_and_light`'s halls, like the bank's tubes). Not a lot building: `District::palace`,
   not `the(..)`; its tiles and props are `palace::TILES` / `PROPS` for the catalogue test.
2. **`bank`**: walls `BANK_WALL`, floor `BANK_FLOOR`, a 2-wide door in the middle of a long
   side (coin flip which), restrooms at both ends behind a `PARTITION` wall (`RESTROOM_FLOOR`,
   `TOILET`s), fridges along the front wall, desk rows of 1-2 `COMPUTER`s with two-cell aisles.
3. **`shops`**: three lots of the shop block, 7x6 each, `SHOP_WALL`, a random
   `SHOP_FLOORS`, a 2-wide door; empty inside; returns the fourth lot, which becomes a park.
4. **The lot buildings** (`LOT_BUILDINGS`), between shuffling the residential lots and
   splitting off the empty ones. Each is a `LotBuilding` in its own module (`SPEC`): a name,
   how many, which lots it `wanted`, how it `rank`s a lot for its `index`th — given a `Siting`:
   the heaps of scrap round the district (`heaps_round(seed)`, the seed's own wasteland over the
   navigation area, worked out before the map has one), the plots of good soil round it
   (`soil_round(seed)`, the same way: every `map::is_plot` cell off the site whose wasteland tile
   `is_fertile` and whose eight neighbours are open ground off the site, no heap, no lake — what
   `Map::can_sow` says yes to before anything is sown) and the lot buildings `built` so far
   (`Siting::the(name)`) — and a `build`er, `fn(&mut SmallRng, &mut Map, &mut Vec<Object>, Lot)
   -> Rect`. `generate` walks the list: for each, `count` times, `take_lot` (the lowest rank,
   ties to the first in the shuffled order) and build. **The list's order is the RNG's order.**
   - **`warehouse`**: the edge lot (`Lot::edge_sides`) **nearest a heap of scrap**; no RNG
     drawn for the lot. 7x6 `place_in` it, `wall brown` and `floor diagonal`, a 2-wide plain
     `DOOR` (public, nobody's property) on an edge-facing side — a coin flip when there are two
     — kept two cells off the corners (`door_clear_of_corners`), a `SCRAP_BIN` in each of the 4
     interior corners (`WAREHOUSE_BINS`, which caps a district at four scrap collectors, `jobs`)
     and a `STEEL_RACK` in the middle of its long interior wall away from the door (the
     interior is 5x4, so the rack has a free cell either side before the corner bins). Picking
     the lot nearest the scrap took the walk to the nearest heap from a median of 55 cells (max
     137) to 21-33 for 63 of 64 seeds.
   - **`restaurant`**, `RESTAURANTS` = 4, one per quarter: the lot whose middle is nearest
     `(WIDTH * q / 4, HEIGHT * q / 4)` for q in 1, 3. `public_room` (7x6, a plain `DOOR` on a
     random outer side kept two off the corners, a `Room` that lays out cells `across` the
     door's wall and `deep` in from it, whichever side the door is on): `wall red`,
     `tiles yellow`, `FRIDGE`s along the far wall (both corners, and the middle when the width
     is odd) and a `TOILET` against each side wall one row in from the door.
   - **`crematorium`**: the first edge lot left (rank 0), `public_room` with the door on an edge
     side, `wall purple`, `floor white`, a `CREMATORIUM` in each far corner (`CREMATORIA` = 2,
     so 6 undertakers, `jobs`).
   - **`steel_factory`**: the lot whose middle is nearest the warehouse's (the couriers'
     trips), **the whole lot** (8x7), `block` and `floor`, a plain `DOOR` in the middle of the
     lot's first street side, laid out from the door with `Room`: a `SCRAP_HOPPER` against each
     side wall in the row inside the door (the couriers' workplaces, `HOPPERS` = 2), a
     `STEEL_PALLET` against each side wall two rows short of the far wall (`PALLETS` = 2) and a
     `FURNACE` in each far corner (the steelworkers', `FURNACES` = 2; powered, so `connect`
     wires the building). It draws no RNG.
   - **`farm`**: the edge lot **nearest a plot of good soil** (`Siting::soil`) — 10 to 12 cells
     from the lot's middle for 58 of 64 seeds, 33 at most: the meadow just past the wall —
     **the whole lot** like the steel factory, `wall brown` and `wood cracked`, a plain `DOOR`
     in the middle of its first edge side, and laid out with `Room` a `GRAIN_BIN` against each
     side wall in each of the four rows past the one inside the door (`GRAIN_BINS` = 8, the
     farmers' workplaces, `jobs`). Draws no RNG. Every seed has 219 to 2195 plots of good soil in
     the navigation area (a median of 1011), the farmers' 80 crops more than twice over; the walk
     out by a gate to the fields is a median of 41 cells, 77 at most.
   - **`power_plant`**: the first edge lot left (rank 0), **the whole lot** like the steel
     factory, `block` and `tiles blue`, a plain `DOOR` in the middle of the lot's first street
     side, and laid out with `Room` a `GENERATOR` in each far corner (`GENERATORS` = 2, the
     district's **only power source**: there is no transformer) and the `CONTROL_PANEL` between
     them in the middle of the far wall (`CONTROL_PANELS` = 1, the power plant workers'
     workplace, three watches, `jobs`). The corners keep the generators off the cells the plant
     takes its line in at (the middle of a street-facing wall) and off the wiring from there to
     the lamp in the middle — `connect` runs their line round the walls (`feed_in`). Draws no RNG.
   - **`playzone`** (the last of `LOT_BUILDINGS`, after the food factory): the residential lot whose
     middle is **nearest the middle of the district** (rank: manhattan distance to
     `(WIDTH / 2, HEIGHT / 2)`), **the whole lot**, `wall purple` round `floor colorful`, a plain
     `DOOR` in the middle of the lot's first street side, laid out with `Room`: an
     `ARCADE CABINET` (`"arcade cabinet"`, `CABINETS` = 6) against each side wall in every other
     row from the one inside the door to the far wall, so each has a free cell beside it, and
     the `PLAYZONE_COMPUTER` (`"playzone computer"`, `COMPUTERS` = 1, the four administrators'
     workplace, `jobs`) in the middle of the far wall with a free cell either side. Powered
     things all, so `connect` wires the building. The cabinets work only while an administrator
     is at the computer (`sim::playzone`, `jobs`). Draws no RNG; took a lot from the parks
     (`EMPTY_LOTS` one fewer) and **seed 7's resident count did not change** (177). The constant
     is `PLAYZONE_COMPUTER`, not `COMPUTER`: the bank's `COMPUTER` is in the module above it.

   History: adding the restaurants and the crematorium changed every seed's houses (the lots
   and the draws after the warehouse); the steel factory, drawing nothing, moved only which
   lots are houses and parks (seed 7's count did not change); the farm, drawing nothing either,
   took a lot from the parks and moved the houses, and seed 7 went from 137 residents to 143;
   the power plant, drawing nothing, took another (`EMPTY_LOTS` 8 -> 7) and seed 7 went to 139;
   the palace, drawing nothing, took another (`EMPTY_LOTS` 7 -> 6) and seed 7 went to 140. Then
   (2026-10-24) the district grew from 6x5 blocks to 9x5 with the main street, and the palace
   moved off its lot onto the whole middle block: every seed changed; `EMPTY_LOTS` 6 -> 31,
   `HOUSES` 92 -> 124, seed 7 140 -> 184 residents.
   Splitting the buildings into
   modules (2026-10-19) changed no seed: every district of seeds 0..64 was compared byte for
   byte before and after.
5. **Lots -> houses**: the rest, `EMPTY_LOTS` split off (leaving `HOUSES` = 124), re-sorted
   into street order (so spawner/arrival order reads bottom-left
   first), then **`house`** for each: 1-2 `RESIDENTS`, 7-8 x 6-7 walls (`HOUSE_WALL`,
   `HOUSE_FLOORS`), a 2-wide door on an outer side, and **`furnish`** -> `arrange` /
   `exhaustively`: beds, `FRIDGE`, `TOILET` against the inner walls, spawners beside beds.
   Then, in `HOUSES_WITH_A_TV` (half) of them, a **`TV`** (`place_tv`): a wall cell off the
   door, the furniture and the spawners where the house is still `livable` (the reachability
   and `ROOM_AROUND` check `arrange` makes, factored out). Which houses and which cell are
   drawn from a **second stream**, `SmallRng::seed_from_u64(seed ^ TV_STREAM)`, so the TVs
   take nothing from the district's RNG and no seed's houses or people changed. `House::tv`
   says where. A house is one open room, so the TV is watched from anywhere in it within
   three cells (`brain-engineer`); `connect` wires it like the fridge.
   Then the **parks**: the split-off lots and the shops' fourth, each a whole lot of `GRASS`
   (`District::parks`), no RNG.
5b. **`gates`** (no RNG, so no seed changed): `GATES_PER_SIDE` (2) a side, `GATES` = 8, each
   at the end of one of the streets running out to that side — street `i * (blocks + 1) / 3`
   for i = 1, 2: streets 3 and 6 up, 2 and 4 across — and on the west and east walls **the
   main street in place of the nearest of them** (with 5 rows it is one of them already: street
   2), so the main street runs gate to gate. Two `DOOR` leaves (public) on `ROAD` in the
   street's first two road lanes (`Street::sides().0` and the next), and a `GUARD_POST`
   (`"guard post"`, the guards' workplace, `jobs`) **on the wall tile** two cells short of the
   nearer leaf, a cell of wall between (on a 5-cell street that is the far side of its pavement
   line; on the main street its first cell of pavement): its only side on the map is the road
   cell inside, so a guard stands in the district and out of the gateway. Pushed to `props`
   before step 6.
6. Props and spawners are added to the map only now (painting terrain after a prop would
   recompute passability against it).
7. **`roof_and_light(map, rooms, halls)`**: ceiling over every building rect (walls
   included); one `HOUSE_LAMP` in the middle of each of the `rooms` — houses, shops and the
   lot buildings; a grid in each of the `halls` (`(walls, lamp, spacing)`): `OFFICE_LAMP`s in
   the bank every `OFFICE_LAMP_SPACING`, `HOUSE_LAMP`s in the palace every
   `palace::LAMP_SPACING`; lamps only on see-through cells.
8. **`glaze`** (per building, the warehouse among them): `WINDOW` tiles replace outer-wall cells every `WINDOW_SPACING`,
   never at a corner, beside a door or another window, and only where the cell inside is open
   floor (so not where a partition meets the wall). Windows are impassable but see-through
   (`Terrain::is_see_through`), so light and sight cross them.
9. **`connect`** (power and sewer; the warehouse's lamp gets a box and wiring like any
   building's): **no transformer** — the power plant's generators are the source — and `SEWER`
   at `(1, HEIGHT-2)`, the road corner inside the wall, joined to the ring main with
   `utilities::lay`; a `LINE` and a `PIPE` main down the middle cell (`Street::middle`) of every street, skipping
   cells inside any building (the bank sits across a street). Per building, if it holds powered
   things or a power source: `service_entry` picks the middle of the inside wall whose straight
   run outward hits a main soonest without crossing another building (`LONGEST_SPUR`), lays
   that spur as `LINE`; for a source inside (the plant's generators) `feed_in` lays a `LINE`
   from under it into the wall beside it and round the walls (breadth first over the ring) to
   that spur; then a `BOX` at the entry and `lay_over_empty` wiring to every consumer inside.
   The same for toilets with `PIPE` and `lay`.
10. **`light_streets`** (street lamps, no RNG, so seeds are unchanged): each street's two
   **sides** (`Street::sides`) — the road's outer lanes, just in from the pavement (a cell in;
   two on the main street), either side of the main and off the pavement. **Rows** are wired end to end
   (`wire_run`): `WIRING` on empty cells, and a **junction** `BOX` wherever a line (a main at
   a crossroads, a house spur) crosses, so only buildings and props cut a row. **Columns** are
   wired a block at a time, starting beside the row below and stopping one cell short of the
   row above, so each joins one row's circuit and never ties two together. `STREET_LAMP`s
   every `STREET_LAMP_SPACING` (8) cells, the far side staggered by half, nudged up to 2 cells,
   none within 2 of another (`no_lamp_near`). Posts and cabinets keep a cell clear of every door
   **and guard post** (the `doors` list includes `GUARD_POST`: a lamp on a post's one inside
   cell would send its guard out through the gate to stand at it). Then `place_controllers`: one
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
| Lamps: one per house/shop/lot building, a grid (>= 12) in the bank and one (>= 9) in the palace, every cell of its hall in a lamp's reach | lighting | `every_house_and_shop_has_a_lamp_and_the_bank_a_grid_of_them`, `palace::tests::the_palace_is_lit_by_a_grid_of_lamps` |
| Windows only in outer walls, spaced | lighting | `every_building_has_windows_in_its_outer_walls_and_nowhere_else` |
| Every consumer has power/water | a dead fridge starves a house | `everything_that_needs_power_or_water_in_a_district_has_it` |
| No line indoors but under the power plant's generators; outdoor wiring only on circuits with a controlled box and nothing indoors | voltages meet only in a box; street lights never share a house's circuit | `a_power_line_never_runs_inside_a_building_and_wiring_never_outside_one` |
| Street lamps powered at night only (controlled boxes off = day), everything else all day; >= 30 lamps; 1..=10 controllers | street lights, and not a cabinet per stretch | `the_street_lamps_have_power_at_night_only_and_everything_else_all_day` |
| >= 90% of main cells within a street lamp's radius | the streets are lit | `the_streets_are_lit` |
| Counts: 124 houses, 3 shops, spawners = residents, beds/fridge/toilet per house, >= 12 computers and >= 2 toilets in the bank; every lot building as many times as its `count` | the brief | `a_district_has_what_it_was_asked_for` |
| Each lot building where and what it should be — in its own module's tests: one warehouse holding its 4 bins and its steel rack and nothing else, none elsewhere, nobody's, on the edge lot to be had nearest the scrap, its door onto the edge street facing out; 4 restaurants, one in each quarter, >= 2 fridges and 2 toilets and nothing else, nobody's; one crematorium on the edge holding its 2 crematoria and nothing else, none elsewhere, nobody's; one steel factory holding its 2 furnaces, 2 hoppers and 2 pallets and nothing else, none elsewhere, nobody's, on no lot farther from the warehouse than a house or park lot is; one farm of 8 grain bins and nothing else, none elsewhere, nobody's, every bin with a free side, its door onto the edge street, on no edge lot farther from good soil than a house or park on the edge; enough soil (>= 2 fields' worth, the median walk out to it <= 42, the longest <= 80); one power plant on the edge holding its 2 generators and its control panel and nothing else, none elsewhere, nobody's, the panel with a free side; its generators the only power sources, on their line, and with them gone no cell of the district live; one playzone holding its computer and its 6 cabinets and nothing else, none elsewhere, nobody's, on no lot farther from the middle of the district than a house or park lot is, every cabinet with a free cell beside it and the computer with two | the brief; collectors and farmers step out towards the wasteland; the plant powers the district; the arcade is where everybody can reach it | `warehouse.rs`, `restaurant.rs`, `crematorium.rs`, `steel_factory.rs`, `farm.rs`, `power_plant.rs`, `playzone.rs` tests |
| 40-60% of houses have one `TV`, against an inside wall with `ROOM_AROUND` free cells beside it; none anywhere else; seed 7 has 184 residents | TVs to watch at home, seeds unchanged | `about_half_the_houses_have_a_tv_...`, `the_tvs_draw_nothing_from_the_district_...`; sim `in_a_district_the_bored_watch_the_tvs_in_their_own_houses` |
| Every edge cell is the perimeter wall but the gates' leaves (`DOOR` on `ROAD`, opening off the map, reachable inside); `GATES_PER_SIDE` a side | a walled district, ways out | `the_district_is_walled_in_but_for_its_gates` |
| One guard post per gate and none elsewhere, on the wall, two from its gate, its only on-map side a free cell inside, not in the gateway | guards stand inside, out of the way | `every_gate_has_a_guard_post_in_the_wall_with_room_inside_to_stand_at_it` |
| Every street off the crossroads reads pavement, road x3, pavement (the edge streets: wall for the outer pavement); the main street pavement x2, road x5, pavement x2 | people keep to the pavement | `every_street_is_a_pavement_each_side_of_its_road` |
| The main street is wider than every other street, road and pavement both; a gate at each of its ends (west and east); the palace along it | a main street, gate to gate past the palace | `the_main_street_is_the_widest_and_runs_gate_to_gate_along_the_palace` |
| The palace is the whole middle block (within 3 of the map's middle: the main street's extra width is below it), nothing else built on it, its throne and nothing else, none elsewhere, nobody's, the throne with a free side, across from the 2-leaf door onto the main street's pavement, the carpet between | the king's seat in the middle, always | `palace::tests::a_district_has_one_palace_filling_its_middle_block_with_a_throne_in_it` |
| Parks are exactly the empty lots and the shops' fourth, whole lots of grass, with nothing built on them | green zones | `the_empty_lots_are_parks_and_nothing_else_is_grass` |
| Nothing on the pavement but the sewer (a corner) | a post on a one-cell pavement sends everyone into the road | `nothing_stands_on_the_pavement_but_the_sewer` |
| Routes between front doors (seeds 0..8): >= 80% of outdoor cells pavement, no road run longer than a crossing (3; 5 over the main street) | people prefer pavements | `people_keep_to_the_pavement_and_cross_the_road_straight_over` |
| Every name used exists in the catalogues | no panic in `tile` | `every_tile_and_prop_the_generator_uses_exists` (`TILES`, `BEDS`, `FRIDGE`, ...) |
| Same seed same bytes; different seed different | reproducible | `the_same_seed_builds_the_same_district`, `a_district_survives_being_saved` |

Every invariant test that builds a building list uses `District::buildings()`, so a new
building is in all of them at once.

QA: `qa/district_browser.json` (the button), `qa/district_lighting.json` (GPU vs CPU lightmap
on a district), `qa/perf_district.json` (a tick with 1000 and 4000 wanderers on a district,
where routes cross roads; it tops seed 7's 184 residents up to 1000, so it names the count).
`qa/district.json` observed a day with seed 7's resident count and camera spots hard-coded and
needed re-adjusting with every generator change: deleted when the district grew. The sim
side: `sim::tests::everybody_in_a_district_moves_into_a_bed_of_their_own_house` (beds, every
desk in the bank, then the four bins in the warehouse, ... the power plant's control panel's
three watches, and the palace's king last) and
`a_district_on_a_monday_stores_scrap_in_its_warehouse`.

## Recipes

- **New furniture in houses**: add it to `arrange` / `Furnishing` (it must keep `ROOM_AROUND`
  and reachability), add its name constant to the `every_tile_and_prop...` list. If it needs
  power or water, `connect` already wires every consumer inside a building - nothing else to do.
  Adding a piece to `arrange` changes every seed's houses; furniture only *some* houses get
  can follow the TV instead (a stream of its own and `livable`), which changes no seed.
- **New building type**: a builder like `shops` drawing from the RNG at a fixed point (this
  **changes every seed after it** - say so in the commit), return its `Rect`, include it in
  the `rooms` list `generate` builds (roof, lamp, `glaze`, `connect`, `light_streets` follow)
  and in `District::buildings()`. A building of a whole block, sited by the geometry rather than
  the seed, is the palace's pattern (`palace.rs`: its block taken out before the bank's draw, a
  `halls` entry for a grid of lamps, a field on `District`).
- **New lot building** (one every district must have, on a lot of its own): a module
  `src/map/district/<building>.rs` — copy `crematorium.rs` (a `public_room`) or
  `steel_factory.rs` (the whole lot, drawing nothing) — with its constants, builder, `SPEC` and
  its own tests over `SEEDS`; `pub mod` and the `SPEC` **on the end** of `LOT_BUILDINGS` in
  `mod.rs`; `HOUSES` re-chosen if `EMPTY_LOTS` would go negative. Nothing else: the lot, the
  `District` record (`the`/`all`), `buildings()`, roof, lamp, windows, power and the catalogue
  test follow from the list. On the end of the list it keeps every seed's bank, shops and lot
  buildings, and moves only houses and parks; re-read seed 7's resident count. The `add-content`
  skill has the whole touch list (its props, its jobs).
- **A seed-stable addition** (decoration, signage, street furniture): do it after step 5 from
  the built rects without touching the RNG - the `roof_and_light` / `glaze` / `connect` pattern.
- **New ground** (gravel, a plaza): a `Terrain::ground(name, cost)` in `map::terrain`, art in
  the editor palette (`tools/ground_art.py`), its name in `TILES`, painted after `pave`.
- **Bigger districts**: `BLOCKS_X`/`BLOCKS_Y` (**both odd**, for the palace's middle block) and
  `HOUSES`; `RESIDENTIAL_LOTS`, `EMPTY_LOTS`, `WIDTH`/`HEIGHT`, the main street's place (`MAIN`)
  and its gates follow, and `LONGEST_SPUR` if lots grow; the sewer's corner follows
  `WIDTH`/`HEIGHT`. Every street position goes through `across(k)` / `up(k)`. Re-read seed 7's
  resident count for `qa/perf_district.json`, and the farm's measured soil and walk (its test).

## Traps

- Anything placed on a street cell blocks it (the sewer takes a pavement corner; street lamps and controllers take road cells in the outer lanes, never pavement);
  `nothing_in_a_district_is_sealed_off` starts its BFS at `(1, 1)`, the road corner inside the
  wall, which must stay passable.
- The wall is on the site's edge, so the wasteland is only reached through a gate: a unit's
  way out (a collector's to the heaps) goes round by the nearest gate. Cells off the map are
  passable (the wasteland), so a test asking for "the only open side" of something on the edge
  has to keep to `map.size()`.
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
  it, store nothing all week. The 24 guards go the same way (measured 2026-10-18, seeds 7 and
  0): with needs on, 6-8 posts kept on Monday morning, a few after noon, **none from Tuesday**;
  with the guards' needs off every post is kept nearly round the clock for two days. Fixing it is the brain's (crowd-aware choice of fridge, toilet and
  computer, or residents keeping to their own house), or more public places to spread over.

The palace also places one powered accountant computer four cells along the far
wall from its throne, off the carpet. It draws no RNG. The accountant is hired
last at this desk; palace and district supply tests require it to be powered.
