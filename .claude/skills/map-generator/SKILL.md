---
name: map-generator
description: Knowledge base for crowd2x's procedural district generator - the seeded 79x54 district of blocks, lots and streets, the 32 houses with beds, fridges, toilets and resident spawners, the bank, the shops, roofs, lamps, windows, and the power and sewer networks it lays - its build order, invariants, where each step lives, how it is reached (browser, env var, QA fixture) and how it is tested. Maps every concept to the file and function that owns it (no code). Use when changing or extending the generator, adding a building type or furniture to it, debugging a generated map (sealed rooms, jammed crowds, dead fridges, residents without beds), or changing determinism.
---

# Map generator: `map::district`

Everything is in **`src/map/district.rs`** (plain Rust, no Bevy). Output is an ordinary `Map`
plus a `District` record of where things went; nothing downstream knows it was generated.
For the layers it writes see `map-layers`; for the networks it lays, `electrician` and
`plumber`.

## Entry points

| Who | Where | What |
| --- | --- | --- |
| The API | `district::generate(seed) -> District` | `District { seed, map, houses: Vec<House>, bank: Rect, shops: Vec<Rect> }`, `District::residents()` |
| Browser button `district` | `src/browser.rs` (`Action::District`) | fresh seed, name from the name field or `district <seed>`, saved, opened in the game |
| By hand | `src/debug.rs` | `CROWD2X_DISTRICT=<seed>` (with `CROWD2X_STATE=game`) |
| QA fixture | `src/qa/mod.rs` | `given.maps: [{"name": ..., "district": seed}]` |
| Sim side | `sim::GameState::spawn_from_spawners` | spawns a `"human"` per spawner; `give_a_bed` hands each the nearest unowned bed |

## Geometry (constants at the top of the file)

`BLOCKS_X` x `BLOCKS_Y` = 4 x 3 blocks; a block is 2x2 **lots** of `LOT_W` x `LOT_H` = 8 x 7;
`STREET` = 3 cells between and around blocks. `WIDTH` x `HEIGHT` = 79 x 54. Helpers:
`block_origin`, `lots_of` (each `Lot` knows its two street-facing `outer` sides), `Rect`
(`interior`, `on_edge`, `cells`, `contains`), `Side::outward`, `place_in`, `door`, `build`
(walls round, floor inside), `object` (prop centred in a cell), `tile` (name -> id, panics on
an unknown name).

## Build order - `generate` (the order is the determinism)

One `SmallRng` from the seed, drawn in a fixed order. **Steps 1-5 draw from the RNG; 6-8 do
not**, so adding or changing anything in 6-8 never changes a seed's streets, houses or people.

1. **Pick blocks**: the bank's left block (`bank_x`, `bank_y`; it spans two blocks and the
   street between), then one block for the shops.
2. **`bank`**: walls `BANK_WALL`, floor `BANK_FLOOR`, a 2-wide door in the middle of a long
   side (coin flip which), restrooms at both ends behind a `PARTITION` wall (`RESTROOM_FLOOR`,
   `TOILET`s), fridges along the front wall, desk rows of 1-2 `COMPUTER`s with two-cell aisles.
3. **`shops`**: three lots of the shop block, 7x6 each, `SHOP_WALL`, a random
   `SHOP_FLOORS`, a 2-wide door; empty inside; the fourth lot stays paved.
4. **Lots -> houses**: remaining residential lots shuffled, `EMPTY_LOTS` dropped (leaving
   `HOUSES` = 32), re-sorted into street order (so spawner/arrival order reads bottom-left
   first), then **`house`** for each: 1-2 `RESIDENTS`, 7-8 x 6-7 walls (`HOUSE_WALL`,
   `HOUSE_FLOORS`), a 2-wide door on an outer side, and **`furnish`** -> `arrange` /
   `exhaustively`: beds, `FRIDGE`, `TOILET` against the inner walls, spawners beside beds.
5. Props and spawners are added to the map only now (painting terrain after a prop would
   recompute passability against it).
6. **`roof_and_light`**: ceiling over every building rect (walls included); one `HOUSE_LAMP`
   in the middle of each house and shop; `OFFICE_LAMP`s in the bank every
   `OFFICE_LAMP_SPACING`; lamps only on see-through cells.
7. **`glaze`** (per building): `WINDOW` tiles replace outer-wall cells every `WINDOW_SPACING`,
   never at a corner, beside a door or another window, and only where the cell inside is open
   floor (so not where a partition meets the wall). Windows are impassable but see-through
   (`Terrain::is_see_through`), so light and sight cross them.
8. **`connect`** (power and sewer): `TRANSFORMER` at `(WIDTH-1, 0)`, `SEWER` at
   `(0, HEIGHT-1)`; a `LINE` and a `PIPE` main down the middle cell of every street, skipping
   cells inside any building (the bank sits across a street); each source joined to the ring
   main with `utilities::lay`. Per building, if it holds powered things: `service_entry` picks
   the middle of the inside wall whose straight run outward hits a main soonest without
   crossing another building (`LONGEST_SPUR`), lays that spur as `LINE`, puts a `BOX` at the
   entry and `lay_over_empty` wiring to every consumer inside. The same for toilets with
   `PIPE` and `lay`.

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
| No line indoors, no wiring outdoors | voltages meet only in a box | `a_power_line_never_runs_inside_a_building_and_wiring_never_outside_one` |
| Counts: 32 houses, 3 shops, spawners = residents, beds/fridge/toilet per house, >= 12 computers and >= 2 toilets in the bank | the brief | `a_district_has_what_it_was_asked_for` |
| Every name used exists in the catalogues | no panic in `tile` | `every_tile_and_prop_the_generator_uses_exists` (`TILES`, `BEDS`, `FRIDGE`, ...) |
| Same seed same bytes; different seed different | reproducible | `the_same_seed_builds_the_same_district`, `a_district_survives_being_saved` |

QA: `qa/district.json` (a day observed, numbers recorded rather than asserted - the crowd jams
on the bank's nearest fridge, a known limit), `qa/district_browser.json` (the button),
`qa/district_lighting.json` (GPU vs CPU lightmap on a district). Seed 7 has 53 residents.

## Recipes

- **New furniture in houses**: add it to `arrange` / `Furnishing` (it must keep `ROOM_AROUND`
  and reachability), add its name constant to the `every_tile_and_prop...` list. If it needs
  power or water, `connect` already wires every consumer inside a building - nothing else to do.
- **New building type**: a builder like `shops` drawing from the RNG at a fixed point (this
  **changes every seed after it** - say so in the commit), return its `Rect`, include it in
  the `buildings` list `generate` passes to `glaze` and `connect`, and in the roof/lamp step.
- **A seed-stable addition** (decoration, signage, street furniture): do it after step 5 from
  the built rects without touching the RNG - the `roof_and_light` / `glaze` / `connect` pattern.
- **Bigger districts**: `BLOCKS_X`/`BLOCKS_Y`, `EMPTY_LOTS` (= residential lots - `HOUSES`),
  `LONGEST_SPUR` if lots grow; the transformer/sewer corners follow `WIDTH`/`HEIGHT`.

## Traps

- Anything placed on a street cell blocks it (the transformer and sewer take two corners);
  `nothing_in_a_district_is_sealed_off` starts its BFS at `(0, 0)`, which must stay passable.
- Painting terrain checks props already in the map; that is why props are collected and added
  after the terrain (step 5) - `glaze` runs after them and only changes wall cells.
- The bank spans the street between its two blocks: anything that runs along streets must skip
  building rects.
- A brain picks the nearest feature as the crow flies; crowds converge on the bank's nearest
  fridge. That is a brain limit, not a generator bug.
