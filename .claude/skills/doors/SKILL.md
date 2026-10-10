---
name: doors
description: Doors and ownership in crowd2x - door props ("door", "house door"), the Doors leaf state that opens over time while a unit waits, locked house doors, properties (the room behind a house door) and who owns them (the key handed out with a bed), how routes, feature choice and wandering respect them, how leaves are drawn and posed, the district's two-leaf doorways, and the tests. Use when touching doors, locks, property or bed ownership, a unit stuck at or walking through somebody's house, door art or animation, or adding a new kind of door or lock.
---

# Doors and ownership

Plain Rust in `src/sim/door.rs` and `src/sim/property.rs`; drawn by `editor/props.rs` +
`game/props.rs`. Everything here follows the fridge's shape (`sim/fridge.rs`): the map says
*where*, the simulation holds the *state*, a unit *asks* through an `Effect`.

## The pieces

| Concept | Where | What |
| --- | --- | --- |
| Door props | `map/props.rs` `PROPS` | `"door"` (anybody) and `"house door"` (owners only), both `Prop::walkable`: the map lets a route through. `sim::door::DOORS`/`is_door` name them. |
| Leaf state | `sim/door.rs` `Doors` | per door: `openness` 0..1, `held` countdown, `lock: Option<PropertyId>`. Sorted cells + a bitset of locked cells (what A* asks). Built once in `GameState::new`. |
| Timing | `door::OPENING`, `CLOSING` (a world minute each), `HELD_OPEN` (two) | **watched** durations (`watched(..)`), advanced by the watched `dt`. |
| Property | `sim/property.rs` `Properties` | a `u16` per cell. Derived: for each house door, flood its neighbours over terrain-passable non-door cells; a side enclosed within `LARGEST_PROPERTY` (1024) cells is the room, the smaller wins; door cells join it. Ids in door-cell order. A house door enclosing nothing locks nothing. |
| Ownership | `GameState::give_a_bed` | the bed handed to an arrival brings `properties.of(bed)` as `Body::home` (the key) via `GameEntity::set_home(bed, property)`. Logged as `... and the key to property N`. Bed ownership itself is `sim/homes.rs` + `HOME_BED` in memory (older). |

## One tick of a door

1. **think** — `Walker::think`: next path cell is a door that `is_shut` (not fully open) →
   `Intent::Idle`. The unit stands where it is.
2. **move** — `move_step` refuses a shut door (`Blocked { by: None }`) as a backstop.
3. **react** — `TaskCtx::advance_action` (every walk goes through it): while walking and
   `Walker::door_ahead` names the next cell as a door — open or not — it sets
   `Effect::Door { at, key: body.home() }`. Asking for an open door is what holds it open.
4. **world** — `world_step`: `Doors::want(at, key)` (refused if locked to another key) sets
   `held = HELD_OPEN`; then `Doors::advance(dt, occupancy)`: held → opens, else shuts —
   **never while the door cell is occupied**.

Not logged per opening on purpose: a district opens doors all day and it flushed the log
panel (`qa/district.json` broke on it).

## Who may go where

- **Routes**: both A* stages use `Think::is_passable_with(cell, key)` = terrain passable
  && `Doors::lets_through(cell, key)`. A stranger has no route into a house at all.
- **Choosing a feature**: `goals::nearest_in_reach` / `choose_nearest` and `SleepGoal`'s
  critical any-bed pick filter by `Think::may_use(cell, key)` (`Properties::may_use`), so
  nobody plans a meal from a stranger's fridge (that would be a failed walk and a flood).
- **Wandering**: `WanderGoal::pick` skips cells in somebody else's property.
- Anything new that picks a destination must do the same, or strangers will flood-search
  for houses they cannot enter.

## Drawing

- Art: `tools/door_art.py` writes `assets/door.png` (glass) and `assets/house_door.png`
  (wood): 6 frames of 16x16, frame 0 shut, last frame open, a leaf in a left-right wall
  sliding left.
- Palette: `PaletteItem::posed(name, path, 6)` — a strip whose frame the game picks; never
  gets a `StripAnimation`.
- `editor::props::DoorLeaf::pose(map, cell, doors)` at spawn: **vertical** when above and
  below are wall/door (quarter turn, `FRAC_PI_2`, pixel-grid safe); **mirrored** (`flip_x`)
  when the art's slide side is the partner leaf, so each leaf of a double door slides into
  its own wall.
- `game::props::swing_doors`: frame = `round(openness * (frames-1))`, written only on change.

## The district

`map/district.rs`: every doorway is `DOOR_WIDTH` (2) leaves — houses get `"house door"`,
the bank's front door, its four restroom doorways (widened from 1 to 2; no RNG change, so
seeds keep their streets and people) and the shops get `"door"`.
Test: `every_doorway_is_a_door_at_least_two_leaves_wide` (no one-cell gap between walls
inside any building, every outer-wall opening is a door of the building's kind, 2 leaves a
house, 10 in the bank).

## Tests

- `sim/door.rs`, `sim/property.rs` unit tests (timing, never shutting on a body, locks,
  room detection, numbering).
- `sim/mod.rs`: `a_unit_stands_still_at_a_shut_door_...`, `a_door_nobody_is_going_through_shuts_behind_them`,
  `the_owner_of_a_house_lets_themselves_in_to_eat`,
  `a_stranger_never_goes_into_somebody_else_s_house_nor_eats_from_their_fridge`,
  `a_route_goes_through_a_house_door_only_for_somebody_holding_its_key`,
  `in_a_district_everybody_holds_the_key_to_their_own_house_and_stays_out_of_the_others`
  (1500 ticks, nobody in a foreign property), `determinism_survives_a_crowd_going_through_doors`.
- QA `qa/doors.json` with the `expect_door {x, y, min, max}` step: an alcove whose only way
  out is a door, half open at tick 18, open at 58, shut again by 258.

## Known limits

- A closed door does not block light or sight (props never do; `lighting`, `perception`).
- A non-owner spawned *inside* a house (a QA `spawn`, a bedless extra resident) cannot get
  out: locks apply both ways.
- A* may route along a two-leaf doorway (into one leaf, then the other), waiting for both.
- Dogs own nothing, so they never enter houses.
