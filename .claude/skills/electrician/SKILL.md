---
name: electrician
description: Knowledge base for crowd2x's electrical grid - transformers, power lines, distribution boxes, wiring, what needs power (fridges, computers, ceiling and tube lamps), how power is computed, switching boxes on and off in the game, how power reaches the brain, the fridges, the lighting and the x-ray overlay, and how the district generator wires buildings. Maps every concept to the file and function that owns it (no code). Use for any task about electricity, power, lamps going dark, a fridge or computer that "does nothing", distribution boxes, the box panel, wiring fixtures, or adding a new powered thing or power source.
---

# Electrician: the power grid

The grid is one **grid layer** of the map (`GridLayer::Power`) plus props that stand on it.
For the layer framework itself (file format, editor table, overlay mechanics) see the
`map-layers` skill; water is the `plumber` skill; how a district lays it out is
`map-generator`. This skill is the whole of power, end to end.

## The model in one paragraph

A **transformer** (a prop on the street that blocks its cell) puts high voltage into the
**power line** under its own cell. The line runs, cell to 4-neighbour cell, to
**distribution boxes**. A box the line reaches, and that is **switched on**, feeds the
**wiring** around it with low voltage. A **consumer** - fridge, computer, ceiling lamp, tube
lamp - works only when **live wiring (or a live box) is under its own cell**. A line straight
under a fridge does not power it; wiring no box feeds is dead; a box no line reaches is dead.
Cables run under walls and furniture and block nobody. One value per cell, so a line and
wiring never share a cell - they meet only in a box. A switched-off box still passes the
line through to boxes beyond it.

## Vocabulary -> where it lives

| Concept | Owner | Names to grep |
| --- | --- | --- |
| Cell values: nothing / wiring / line / box, written `.` `-` `=` `B` | `src/map/grid.rs` | `GridLayer::Power`, module `power` (`NONE`, `WIRING`, `LINE`, `BOX`), `alphabet` |
| Reading/writing a cell | `src/map/mod.rs` | `Map::grid`, `Map::set_grid`, `grid_cells`, `grid_count` |
| Saved in a map file | `src/map/format.rs` | top-level `"power"` rows, `encode_grid`, `decode_grid`, `MalformedGrid` |
| Transformer is a source; who needs power | `src/map/utilities.rs` | `SOURCES`, `CONSUMERS`, `Utility::Power`, `needs`, `source_of` |
| Transformer blocks its cell | `src/map/props.rs` | `PROPS` entry `"transformer"` |
| Computing what is live | `src/map/utilities.rs` | `Supply::from_map`, `Supply::with_boxes_off`, private `flood` (three fills: line from transformers, wiring from fed+on boxes, pipes) |
| Asking | `src/map/utilities.rs` | `Supply::is_live(Network::Line / Network::Wiring, cell)`, `serves(name, cell)`, `consumers(map)` |
| What a box feeds (for its panel) | `src/map/utilities.rs` | `fed_by(map, box_cell)` |
| Laying cable programmatically | `src/map/utilities.rs` | `l_path`, `lay`, `lay_over_empty`, `serve_everything` |
| Power in the simulation | `src/sim/mod.rs` | `GameState` fields `supply`, `boxes_off`, `supply_generation`; methods `supply`, `supply_generation`, `has_box`, `box_is_on`, `switch_box`; `Command::SwitchBox`, `Input::switch_box` (applied in `spawn_pass`) |
| Unpowered fridge/computer is not a destination | `src/sim/feature.rs` | `Features::from_map(map, supply)` skips what `supply.serves` refuses |
| Fridge compressor | `src/sim/fridge.rs` | `FridgeState::is_powered`, `closed(powered)` (unpowered starts at `AMBIENT`), `Fridges::set_power`, `advance` (unpowered closed fridge drifts to `AMBIENT` at the `COOLING` rate) |
| A unit already on its way | `src/sim/brain/tasks/take_item.rs`, `use_computer.rs` | `TakeItem` refuses an unpowered fridge when the action would start; `UseComputer` fails any tick its computer is not in `features` (screen off mid-go, no fun given) |
| Lamps and screens | `src/lighting/scene.rs` | `Light::needs_power`; `LightScene::from_map(map, supply)` keeps every emitter, enabled only if powered; `apply_supply` re-switches always-on powered lights |
| Lighting following a switch | `src/lighting/mod.rs` | `follow_power` (compares `Lighting::supply_generation` to the sim's), runs before `switch_lights_in_use` |
| Editor palette | `src/editor/grids.rs` | `POWER_PALETTE`: `"wiring"`, `"power line"`, `"distribution box"` (entry i paints value i+1, `value_of`); layer row `"power"` in `editor::LAYERS` (key `5`) |
| Transformer in the editor | `src/editor/props.rs` | props `PALETTE` entry `"transformer"`, art `assets/transformer.png` |
| Painting cable | `src/editor/mod.rs` | `edit` -> `Target::Grid` brush: `grids::stroke` from `BrushStroke.last`, also on release; right button erases |
| X-ray drawing | `src/editor/grids.rs` | `paint` / private `paint_power`, `cable`, `arm`; colours `WIRING_LIVE/DEAD`, `LINE_LIVE/DEAD`, `BOX_BODY/EDGE`, consumer lights `SERVED`/`UNSERVED`; in the game paints from the sim's supply (`sim_generation`) |
| "power a/b" readouts | `src/editor/grids.rs` + `src/game/hud.rs` | `GridOverlay::summary`; editor HUD line `connected`, game stats line `wired` |
| Selecting a box | `src/game/selection.rs` | `SelectedBox` (exclusive with `Selected`; `pick` selects a box only when nobody is in the cell), frame follows it |
| The box's switch | `src/game/boxpanel.rs` | `BoxPanelPlugin`, `BoxAction::{Toggle, Close}`, `BoxReadout::{State, Toggle}`, `feeds`; buttons labelled `switch off` / `switch on` / `close` |
| District wiring | `src/map/district.rs` | `TRANSFORMER` at `(WIDTH-1, 0)`, `connect` (line main mid-street, skipping buildings), `service_entry` (box just inside the wall nearest a main, spur out to it, `LONGEST_SPUR`), then `lay_over_empty` wiring to each consumer |
| Art | `assets/` | `transformer.png`, `wiring.png`, `power_line.png`, `distribution_box.png` (16x16) |

## Data flow of a switch

Player clicks a box cell -> `selection::pick` sets `SelectedBox` -> `boxpanel` builds the
panel -> `switch off` pushes `Command::SwitchBox` onto `SimInput` -> next `spawn_pass` calls
`GameState::switch_box`: updates sorted `boxes_off`, re-floods `Supply::with_boxes_off`,
rebuilds `Features`, `Fridges::set_power`, bumps `supply_generation`, logs `the distribution
box at x, y was switched off` -> `lighting::follow_power` sees the new generation and calls
`apply_supply` (dirty chunks re-bake) -> `GridOverlay` sees it and repaints -> brains whose
plan pointed at a now-dead fridge/computer fail their task and replan from the new index.
Box state is **simulation state**: not saved in the map, reset when the map is replayed.

## Recipes

- **New powered thing** (prop or lamp): add it as a prop/lamp as usual (palette, `map::PROPS`
  for props, `EMITTERS` if it lights), then one entry in `CONSUMERS` with `Utility::Power`.
  Features, overlay lights, lighting `needs_power`, `summary` and district wiring (if the
  generator places it inside a building) follow. If a task *uses* it, re-check power in the
  task (copy `UseComputer`'s per-tick check, or `TakeItem`'s at-start one).
- **New power source** (generator, solar): prop + palette + art, `SOURCES` with
  `Utility::Power`. It feeds the line/box under its cell exactly like a transformer.
- **Capacity, load, outages, breakers**: not modelled. A load limit would live in
  `Supply::with_boxes_off` (count consumers per box via the wiring flood) and need a new
  `GameState` command or rule; keep it in plain Rust in `map/` or `sim/`.
- **Switch from code/tests**: `GameState::switch_box(cell, on)` directly, or
  `Input::switch_box` through a pass; QA `switch_box` / `select_box` + `press`.

## Tests that cover it

Unit: `map::utilities::tests` (`a_fridge_is_powered_through_a_line_a_box_and_wiring...`,
`a_box_switched_off_passes_the_line_on...`, `a_box_feeds_what_is_on_its_wiring...`,
`serve_everything_serves_everything`), `sim::tests::switching_a_box_off_takes_its_fridges_away...`,
`sim::fridge::tests::a_fridge_with_no_power_is_as_warm_as_the_room...`,
`sim::feature::tests::a_fridge_with_no_power_and_a_toilet_with_no_drain...`,
`tasks::take_item::tests::taking_from_a_fridge_with_no_power...`,
`tasks::use_computer::tests::losing_power_mid_session...`,
`goals::eat::tests::a_fridge_with_no_power_feeds_nobody_until_it_is_wired_up`,
`lighting::scene::tests::a_ceiling_lamp_lights_its_room...` (dark, wired, switched),
`editor::grids::tests::live_wiring_is_bright...`, district
`everything_that_needs_power_or_water_in_a_district_has_it`,
`a_power_line_never_runs_inside_a_building_and_wiring_never_outside_one`.
QA: `qa/power_and_water.json` (lay it in the editor, play, switch the box off and on),
`qa/ceiling_and_lamps_editor.json` (wires its lamp). Steps: `expect_connected {utility:
"power"}`, `expect_grid {layer: "power"}`, `expect_lighting {lights_on}`, `xray`, `select_box`,
`switch_box`.

## Traps

- A test map with a fridge/lamp and no wiring has a **dead** one. Rust tests:
  `testing::World::new` serves everything (`World::as_built` does not);
  `utilities::serve_everything` wires a `GameState` map. Fixtures and QA inline maps:
  `uv run tools/wire_map.py <file>`.
- Wiring adjacent to a line does not connect - only through a box.
- An unpowered lamp is still in `LightScene::lights`, disabled - count `enabled`, as
  `expect_lighting {lights_on}` does.
- Computers' lights are switchable (in-use), so `apply_supply` skips them; no power means
  nobody can be at one, which turns it off.
- Gamepad `Select` leaves the game; the x-ray toggle is `o` / right-stick click.
