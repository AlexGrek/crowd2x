---
name: map-layers
description: The crowd2x map layer framework - terrain, object layers (props, lamps, spawners) and grid layers (ceiling, power, water), how each is stored, saved in the map file and edited (the editor's LAYERS table, Target kinds, brushes and rectangles, the x-ray GridOverlay and LayerView), plus how to add a new layer or network and how to wire fixtures. Use when adding or changing a layer, the map format, the editor's layer handling or the overlay. For the power grid itself use the electrician skill, for sewers the plumber skill, for the district generator the map-generator skill.
---

# Map layers and the networks under the floor

**Focused skills go deeper on three systems built on this framework:** `electrician` (the power
grid end to end, including switching boxes), `plumber` (the sewer), `map-generator` (the
district). Read the one that matches the task; this skill is the shared framework.

Read this instead of exploring `src/map/`, `src/editor/` and the game's selection code.
Everything here is current as of the commit that added power and water; if a name below no
longer exists, grep for it before trusting the rest.

## The three kinds of layer

| Kind | Map storage | File key | Editor | Blocks? |
| --- | --- | --- | --- | --- |
| **Terrain** | `Map::terrain` - a `TerrainId` per cell, base layer only | `"terrain"` (palette + CSV rows) | `editor/background.rs`, `TileWindow` | by catalogue (`map/terrain.rs`) |
| **Object** (`ObjectLayer`: `Props`, `Spawners`, `Lamps`) | `Map::objects(layer)` - free-placed `Object { at: pixels, kind: name }` | `"objects": {"props": [...], ...}` | `editor/props.rs`, `PropWindow` | props only, by `map::PROPS`; lamps and spawners never |
| **Grid** (`GridLayer`: `Ceiling`, `Power`, `Water`) | `Map::grid(layer, cell) -> u8`, `set_grid`, `grid_cells`, `grid_count` | top-level `"ceiling"`, `"power"`, `"water"` - one string per row, one char per cell | `editor/grids.rs`, `GridOverlay` | never |

Rows are **bottom-up** everywhere (row 0 is y 0). An object's cell is `Object::cell()` (its
centre, floored) - that one cell is what it blocks and what it plugs into.

### Grid layer values (`src/map/grid.rs`)

Each `GridLayer` has an **alphabet**: value `n` is written as `alphabet()[n]`, value 0 is
always `.` (nothing).

| Layer | Alphabet | Values (`grid::<layer>::*`) |
| --- | --- | --- |
| `Ceiling` | `.#` | `OPEN`, `ROOFED` |
| `Power` | `.-=B` | `NONE`, `WIRING` (low voltage), `LINE` (high voltage), `BOX` (distribution box) |
| `Water` | `.o` | `NONE`, `PIPE` |

One value per cell, so wiring and a line cannot share a cell; they meet only in a box.
`is_underground()` is everything but the ceiling (drawn at `UNDERGROUND_Z` 60, the ceiling at 40).

## The editor's layer table (`src/editor/mod.rs`)

`LAYERS: &[LayerDef]` - `{ name, target: Target, palette, hint }`, where
`Target::{Terrain, Objects(ObjectLayer), Grid(GridLayer)}`. `Layer(usize)` indexes it; `Tool`
holds `layer` and `indices: [usize; LAYERS.len()]`. Order = `tab` order = number keys
(`1`-`9`, at most nine rows - a test enforces it). Today: background, props, lamps, ceiling,
power, water.

What reads the table, so you don't have to touch it: `Tool::select` (QA `tool` step - names
must be unique across **all** palettes, tested), `switch_layer`, `describe` (HUD),
`update_cursor` (grid/terrain snap to cell centre, objects to whole pixels),
`every_palette_item_is_one_cell_wide_once_scaled` (checks every palette's PNG sizes).

`edit()` matches on `target`:

- `Terrain` - brush, or a rectangle `Instrument` for `wall*`/`block`.
- `Objects(layer)` - one press places (`props::place`), right press erases nearest
  (`props::erase_nearest`); then `overlay.touch()` since a placed fridge changes what is powered.
- `Grid(layer)` - `Layer::instrument` says: `Ceiling` is a rectangle (`Instrument::Block`,
  committed on release through `grids::set`), the others a **brush** that paints every cell
  between last frame's and this frame's (`grids::stroke`, `BrushStroke.last`) **and the cell it
  is released on** - the QA `drag` step presses, moves and releases in one frame, so a brush
  that only painted while held laid one cell. Palette entry `i` paints `grids::value_of(i)` =
  `i + 1`; right button paints 0.

## The x-ray overlay (`src/editor/grids.rs`)

`GridOverlay` (resource) draws **one sprite per grid layer** for the visible cell rect: an
RGBA `Image` painted on the CPU by `paint(layer, map, supply, rect)` at 16 texels per cell,
scaled by `ART_SCALE`. Repaints when the view crosses a cell, `touch()` is called, the wanted
set changes, or (game) `GameState::supply_generation` changes - never on `CurrentMap`'s change
flag. In the game it paints from **the simulation's** `Supply` (so a switched-off box shows
dead wiring); in the editor it floods `Supply::from_map(&current.map)` itself. Live/dead
colours, the box's centre light (green = feeding wiring), and a green/red corner light on every
consumer of that layer's utility. `summary()` is `power a/b  water c/d`, shown by both HUDs.

Which layers show: `LayerView` (modes off, each layer, all) - `o` / right-stick click
(`cycle_view`), the game's `x-ray` button, QA `{"xray": "power"}`. In the editor the layer
being edited always shows. Registered in `EditorPlugin` for **both** map screens, reset on
entering either (`grids::reset`).

## The networks (`src/map/utilities.rs`, plain Rust)

| | Source (a prop that blocks its cell **and** is a network node there) | Carrier | Consumers (`CONSUMERS`) |
| --- | --- | --- | --- |
| Power | `"transformer"` feeds the `LINE`/`BOX` under it | line -> box -> `WIRING` | `fridge`, `computer`, `ceiling lamp`, `tube lamp` |
| Water | `"sewer"` drains the `PIPE` under it | pipes | `toilet` |

Rules, all through the consumer's **own cell**: powered = live `WIRING` or `BOX` under it
(a `LINE` straight into a fridge does **not** count; wiring with no box is dead). A box is fed
when the line reaches it, and feeds wiring only when switched on. Cables run under walls and
furniture freely.

API: `Supply::from_map(map)`, `Supply::with_boxes_off(map, &[Point])`,
`Supply::everywhere(size)` (tests whose wiring is beside the point), `is_live(Network::{Line,
Wiring, Pipes}, cell)`, `serves(name, cell)` (true for anything that needs nothing),
`consumers(map)` (object, utility, served), `needs(name)`, `source_of(name)`,
`fed_by(map, box_cell)` (what a box's wiring reaches), `l_path`, `lay`, `lay_over_empty`,
`serve_everything(map, transformer_cell, sewer_cell)` (wire a map the shortest way, for tests).

**Who reads it:**

- `sim::GameState` - `supply`, `boxes_off`, `supply_generation`; `Features::from_map(map,
  &supply)` **leaves out** unserved fridges/computers/toilets (a brain never goes there);
  `Fridges::from_map(map, &supply)` - an unpowered fridge has `is_powered() == false`, starts at
  room temperature and does not cool.
- `lighting::scene::LightScene::from_map(map, &supply)` - every emitter is in the scene;
  one needing power (`Light::needs_power`) is enabled only when powered. `apply_supply`
  re-switches lamps (not computers - their in-use switching handles that).
- Tasks: `TakeItem` refuses an unpowered fridge at start; `UseComputer` fails any tick its
  computer is no longer in `features` (a box switched off mid-go).

## Switching a box (game)

`Command::SwitchBox { at, on }` (`Input::switch_box`) -> spawn pass -> `GameState::switch_box`:
re-flood, rebuild `features`, `fridges.set_power`, bump `supply_generation`, log
`the distribution box at x, y was switched off`. Readers poll the generation:
`lighting::follow_power`, `GridOverlay`. UI: `game/selection.rs` - a click on an empty cell with
a box selects it (`SelectedBox`, exclusive with `Selected`, same frame sprite);
`game/boxpanel.rs` - panel with `switch off`/`switch on` and `close` (pressable by name).
`has_box`, `box_is_on` on `GameState`. Box state is **not saved** in the map.

## The district generator wires itself (`src/map/district.rs`, `connect`)

After roofs and lamps, seed-independent: transformer at `(WIDTH-1, 0)`, sewer at
`(0, HEIGHT-1)`, a `LINE` and a `PIPE` main down the middle of every street (skipping building
rects), and per building `service_entry` - the inside cell in the middle of the wall whose
straight run out reaches a main soonest without crossing another building (`LONGEST_SPUR`) -
holding the `BOX` (power) or pipe entry; then `lay_over_empty` wiring / `lay` pipes to each
consumer inside. Tests over 64 seeds: every consumer served; no line indoors, no wiring outdoors.

## Recipes

**A new thing that needs power or water** (a TV, a sink): its palette entry (`editor/props.rs`)
and `map::PROPS` entry as for any prop, then one line in `utilities::CONSUMERS`. Features,
lighting (if it's in `EMITTERS`), overlay lights and the district (if the generator places it)
follow. If it's a lamp-like light, it's `needs_power` automatically.

**A new source** (a generator, a water tower): `SOURCES` + `map::PROPS` + props palette +
16x16 art. `Supply::from_map` floods from every source of that utility.

**A new grid layer**: a `GridLayer` variant (append - its discriminant is its storage index;
update `ALL`), `name()` (a file key - never rename), `alphabet()` (`.` first), values module;
a palette const in `editor/grids.rs` + an arm in `palette_of`; a row in `LAYERS`; a paint arm in
`grids::paint`. The format, x-ray modes, HUD and QA `expect_grid` pick it up. Old maps load
unchanged (an absent key is all zeros) - `FORMAT_VERSION` stays.

**A new network/utility** (gas): the grid layer above, a `Utility` + `Network` variant, a flood
in `Supply::with_boxes_off` (and a `live` slot), arms in `serves`, `summarise`, the overlay's
consumer lights, and `connect` in the district.

**New art**: 16x16 PNG, `PaletteItem::upscaled`. The six for this work were generated by a
stdlib-only Python PNG writer (char-map rows -> RGBA); copy that approach.

## Wiring maps and fixtures

A fixture with a fridge but no wiring **has no working fridge**. Either:

- `uv run tools/wire_map.py <map.json | qa-script.json>...` - transformer (box under it) and
  sewer on the first/last spare impassable cell, wiring/pipes in L-shapes to every consumer;
  edits text in place (keeps layout and CRLF), skips maps that already have a source.
- In Rust tests: `utilities::serve_everything(&mut map, a, b)` (blocks cells `a` and `b` -
  pick corners/walls), or `testing::World::new` (serves everything) vs `World::as_built`.

## Testing

- Unit: `map::utilities::tests`, `map::grid::tests`, `map::format::tests` (round trip,
  `UnknownGridLayer`), `editor::grids::tests` (`paint` texels, `LayerView`),
  `editor::tests::every_layer_of_the_map_is_in_the_editor_once`, district tests, `sim::tests::
  switching_a_box_off_...`, `tasks::take_item`/`use_computer` power tests, eat goal
  `a_fridge_with_no_power_feeds_nobody_until_it_is_wired_up`, lighting scene test.
- QA: `qa/power_and_water.json` (lay networks in the editor, play, `expect_connected`,
  `expect_lighting`, switch the box via its panel). Steps: `xray`, `expect_grid`,
  `expect_connected`, `select_box`, `switch_box` - see the `qa` skill.

## Traps already hit

- **`tools/qa.py` runs `cargo run` per test**, so editing Rust during a suite run tests a
  mix of builds - and killing a run mid-link can leave a stale incremental build that fails
  to link (`LNK2019`): `cargo clean -p crowd2x`.
- Brushes and the QA `drag` step: see the editor section - paint on release too.
- `"pipe"` is an existing **prop** (decorative); the water palette entry is `"sewer pipe"`.
- Gamepad `Select` already leaves the game screen; the x-ray is the right-stick click.
- An unpowered lamp is still in `LightScene::lights` (disabled); count with `expect_lighting
  {"lights_on": n}`, not the light count.
