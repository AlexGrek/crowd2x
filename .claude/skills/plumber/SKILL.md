---
name: plumber
description: Knowledge base for crowd2x's sewer network - the central sewer, pipes under the floor, what needs a drain (toilets), how drainage is computed, how it reaches the brain (a toilet with no drain is not a toilet), the overlay, the editor tool, and how the district generator plumbs buildings. Maps every concept to the file and function that owns it (no code). Use for any task about water, sewers, pipes, toilets that nobody uses, bladder/relief behaviour that depends on plumbing, plumbing fixtures, or adding a new thing that needs water or a new water source.
---

# Plumber: the sewer network

The sewer is one **grid layer** (`GridLayer::Water`) plus the `"sewer"` prop. For the layer
framework (file format, editor table, overlay) see `map-layers`; power is `electrician`;
district layout is `map-generator`.

## The model

A **sewer** (a manhole on the street, a prop that blocks its cell) drains the **pipe** under
its own cell. Pipes connect cell to 4-neighbour cell. A **toilet** works only when a pipe
connected to a sewer runs **under its own cell**. There is one pipe value (no mains/branch
distinction, no valves), pipes cross walls, furniture and power cables freely (different
layer), and **nothing switches water off during play** - drainage is fixed when the map
loads. A toilet with no drain is left out of the feature index, so a unit never walks to it
and a full bladder has nowhere to go (the relieve goal is held off as "blocked").

## Vocabulary -> where it lives

| Concept | Owner | Names to grep |
| --- | --- | --- |
| Cell values: nothing / pipe, written `.` `o` | `src/map/grid.rs` | `GridLayer::Water`, module `water` (`NONE`, `PIPE`) |
| Saved in a map file | `src/map/format.rs` | top-level `"water"` rows |
| Sewer is a source; toilet needs water | `src/map/utilities.rs` | `SOURCES` (`"sewer"`), `CONSUMERS` (`"toilet"`), `Utility::Water`, `Network::Pipes` |
| Sewer blocks its cell | `src/map/props.rs` | `PROPS` entry `"sewer"` |
| Computing drainage | `src/map/utilities.rs` | `Supply::from_map` / `with_boxes_off` - the third flood, from every sewer standing on a pipe; `is_live(Network::Pipes, cell)`, `serves("toilet", cell)` |
| Laying pipe programmatically | `src/map/utilities.rs` | `l_path`, `lay` (pipes overwrite freely - one value), `serve_everything` (sewer cell gets a pipe, then an L to every toilet) |
| Toilet not a destination without a drain | `src/sim/feature.rs` | `Features::from_map(map, supply)`; also drops it from `entered` (`is_enterable`), so its cell is a plain wall to movement |
| Using a toilet | `src/sim/brain/goals/relieve.rs`, `src/sim/brain/tasks/use_toilet.rs` | `RelieveGoal` picks `features.nearest(Toilet)`; `UseToilet` enters the cell (`Access::Entered`) and interacts. No per-tick drain check is needed: drainage never changes during play |
| Bladder | `src/sim/biology/bladder.rs` | `BLADDER_PER_SECOND`, `FILLING_PER_SECOND` - what makes a toilet needed |
| Editor palette | `src/editor/grids.rs` | `WATER_PALETTE`: `"sewer pipe"` (value 1); layer row `"water"` in `editor::LAYERS` (key `6`) |
| Sewer in the editor | `src/editor/props.rs` | props `PALETTE` entry `"sewer"`, art `assets/sewer.png` |
| X-ray drawing | `src/editor/grids.rs` | `paint` water arm via `cable` (4 texels wide), `PIPE_LIVE`/`PIPE_DEAD`, toilet corner light `SERVED`/`UNSERVED` |
| "water a/b" readouts | `src/editor/grids.rs`, `src/game/hud.rs` | `GridOverlay::summary` |
| District plumbing | `src/map/district.rs` | `SEWER` at `(0, HEIGHT-1)`, `connect` lays a pipe main mid-street (skipping buildings) and, per building with a toilet, `service_entry` on `GridLayer::Water` then `lay` from the entry to each toilet |
| Art | `assets/` | `sewer.png`, `sewer_pipe.png` (16x16) |

## Recipes

- **New thing that needs a drain** (sink, shower, washing machine): prop as usual, then one
  `CONSUMERS` entry with `Utility::Water`. Feature indexing, overlay lights, `summary` and the
  district's plumbing (if it places it in a building) follow.
- **A thing needing both power and water** (washing machine): `CONSUMERS` holds one utility
  per name today and `needs` returns one. Make `needs` return a set (or let the name appear
  twice and have `serves` require all), and update `consumers`, `summarise` and the overlay
  lights, which assume one.
- **Water supply** (clean water in, not just sewage out): a new network - see
  `map-layers` "A new network". Today "water" means the sewer only.
- **Shut-off valves**: model on the electrician's distribution box - a new `water` value, a
  `boxes_off`-like set on `GameState`, a command, and a per-tick check in `UseToilet`.

## Tests that cover it

Unit: `map::utilities::tests::a_toilet_drains_through_pipes_to_a_sewer`,
`pipes_and_wiring_run_under_walls`, `serve_everything_serves_everything`,
`sim::feature::tests::a_fridge_with_no_power_and_a_toilet_with_no_drain...`,
`sim::tests::an_enterable_cell_admits_one_entity...` (plumbed with `serve_everything`),
district `everything_that_needs_power_or_water_in_a_district_has_it`.
QA: `qa/power_and_water.json` (lays a pipe from sewer to toilet in the editor, then
`expect_connected {utility: "water"}`), `qa/toilet.json` (a plumbed washroom). Steps:
`expect_grid {layer: "water", value: "o"}`, `expect_connected`, `xray "water"`.

## Traps

- A test or fixture with a toilet and no pipe: nobody ever relieves themselves. Rust:
  `testing::World::new` serves everything; `serve_everything` for a `GameState`; fixtures:
  `uv run tools/wire_map.py`.
- `"pipe"` is an older **decorative prop** (`pipe_vertical.png`); the network's palette entry
  is `"sewer pipe"`. Palette names must be unique across all layers.
- The sewer blocks its cell: put it at the edge, on a wall, or on a street corner.
