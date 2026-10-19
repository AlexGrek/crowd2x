---
name: add-content
description: The touch list for adding a kind of thing to crowd2x — a job, a building every district has, an item, a prop a brain uses (a feature, a store), a goal, a task, an effect, a new part of the world's state. Each is a row in a catalogue or a file of its own, and this skill says which, and what follows from it with no edit. Use before adding any of them, alongside the skill that owns the system (jobs, map-generator, brain-engineer, simulation).
---

# Adding a kind of thing

The simulation is built so that **a new kind is one row, or one file**, and everything else —
the save, the statistics menu, the HUD, the job menu, the test harness, the district's roofs,
lamps, windows and power — follows from it. If adding something seems to need an edit in an
unrelated place, the catalogue is missing a column: add the column, do not add the `match` arm.

## The catalogues (`sim::catalogue!`)

`src/sim/catalogue.rs` defines `catalogue!`: an enum and a `Def` row per variant, written
together. It gives `COUNT`, `ALL` (the order listed), `def()` (a `const fn`), `number()` and
`from_number()`. **The number is the save format**: kinds are numbered 0, 1, 2, ... in the order
listed (checked at compile time), a new one goes **on the end** with the next number, and none is
ever removed or renumbered.

| Catalogue | File | A row says |
| --- | --- | --- |
| `ItemKind` / `ItemDef` | `sim/item.rs` | name (saved by it), consume time, nutrition, hydration, taste, mass, volume, **emoji** (how it is drawn in a hand) |
| `FeatureKind` / `FeatureDef` | `sim/feature.rs` | the **prop names** that are one, `Access` (beside, entered, watched), and for a store a `StoreDef`: what it holds, what the log calls it, its `site` (the statistics menu's building), its HUD word |
| `GoalId` / `GoalDef` | `sim/brain/goal.rs` | name, emoji, **executor constructor** — which is what puts the goal in every `Brain::human()`. Ties go to the goal listed first, so a job's goal goes after the needs |
| `JobKind` / `JobDef` | `sim/job.rs` | name, article, workplace `FeatureKind`, goal, commute, shifts, **uniform name**, and the job menu's `now` line (`fn(at_work, pack) -> &str`). `ALL` order is the **hiring order** |

## Adding a job

1. **A `JobKind` row** (`sim/job.rs`).
2. **Its workplace**, if new: a `FeatureKind` row (`sim/feature.rs`) naming the prop. Plus the map
   side of the prop: a `Prop` in `map/props.rs` (blocking or not), a `CONSUMERS` entry in
   `map/utilities.rs` if it needs power or water, a `PALETTE` row in `editor/props.rs` (art), and
   an `EMITTERS` entry in `lighting/scene.rs` if it glows.
3. **Its goal**, if new: a `GoalId` row and `brain/goals/<goal>.rs` (`pub mod` + `pub use` in
   `goals/mod.rs`). A job worked by standing at it (a desk, a post) can reuse `GoalId::Work`.
4. **Its tasks**, if new: see "Adding a task".
5. **Its uniform**: the PNG `assets/human/clothes_uniform_<job name with _>.png`, drawn by
   `tools/human_art.py` (`pixel-artist`). `Clothing::Uniform(JobKind)` and its art path follow from
   the row's `uniform` name.
6. **Somewhere to work**, for a district: a lot building (below) or props in an existing one.

Nothing else: hiring (`JobKind::ALL`), `WorkRoutine`, the save (the kind's number), the statistics
menu (posts per kind), the job menu (`JobDef::now`), the uniform swap and the brains menu all
read the catalogues. Tests: the job's goal through `testing::World`, a district test if it has a
building (`jobs` skill).

## Adding a building every district has (a lot building)

1. **A module** `src/map/district/<building>.rs` with its prop and tile constants, a builder
   `fn(&mut SmallRng, &mut Map, &mut Vec<Object>, Lot) -> Rect`, and `pub const SPEC: LotBuilding`
   (name, how many, which lots it wants, how it ranks a lot — given the heaps of scrap and the lot
   buildings already built (`Siting::the`) — the builder, and every tile and prop it uses), plus
   its own `#[cfg(test)]` checks over `SEEDS` (`super::super::tests::{props_in, SEEDS}`).
2. **`pub mod <building>;`** and **its `SPEC` on the end of `LOT_BUILDINGS`** in
   `src/map/district/mod.rs`.
3. **`HOUSES`** re-chosen if `EMPTY_LOTS` would go negative.

Everything else follows: the lot it takes from the houses, `District::the(name)` /
`all(name)`, `District::buildings()`, its roof, lamp, windows, power and sewer, the street
lights keeping clear of it, the catalogue test of its tiles and props, the door-width and
sealed-off tests. **The order of `LOT_BUILDINGS` is the order of the RNG's draws**: a new one on
the end keeps every seed's bank, shops and lot buildings and moves only its houses and parks;
re-read seed 7's resident count (`map-generator`).

## Adding an item, a feature or a store

- **Item**: an `ItemKind` row. Inventories, saves (by name) and the hand's art follow.
- **Feature** (a prop a brain can use): a `FeatureKind` row with its prop names; the map side of
  the prop as in "Adding a job" step 2. `Features` indexes it, `nearest_in_reach` finds it.
- **Store** (a feature things are kept in): a `FeatureKind` row with a `StoreDef`, and the prop
  posed in `editor/props.rs` with a 4-frame fill strip (`BinFill`, `fill_bins`). `Bins` keeps it,
  `Effect::Deposit` / `Effect::Withdraw` fill and empty it, the statistics menu adds it to its
  `site`, the HUD counts it if it has a `hud` word, QA `expect_stored` takes its prop name.

## Adding a goal

A `GoalId` row (the next number; its executor constructor), `brain/goals/<goal>.rs` with the
`GoalExecutor` (its fields are its saved state: `save`/`restore` to a `pb::Executor`), and the
`pub mod` + `pub use` in `goals/mod.rs`. Update the ranked-list test in `goal.rs`. Something has to
raise it: a routine (`add-routine`) or a job (`JobDef::goal`).

## Adding a task

`brain/tasks/<task>.rs` with: the `Copy` struct; its `TaskExecutor`; its `Task::` constructor in an
`impl Task` block; and how it is saved — **`TimedTask`** (a `NAME` no other task has, `new`,
`cell`, `seconds`) for a task that is a place and a time, saved as `pb::TaskKind::Named` with
**no schema change**; or `SavedTask` by hand. Then `pub mod` + `pub use` in `tasks/mod.rs` and
**its name in the `tasks!` list** there, which writes the `Task` enum and every `match` over it.
Add it to `one_of_each` in `task.rs`'s tests (round-trips every task through a save). A goal that
reads the queue (`Stage::of`) ignores tasks it does not know.

## Adding an effect, or a new part of the world's state

- **Effect** (a change a task asks of the world beyond its unit): a variant in `sim/effect.rs` and
  its arm in `effect::apply` — the one function the world step and `testing::World` both call.
- **World state** (a new index, store, tally — anything beside the map and the crowd): a field of
  `Facilities` (`sim/facilities.rs`), built in `Facilities::new`; if a brain reads it, a field of
  `Think` filled in `Think::new` (`sim/entity.rs`) — no other place builds a `Think`; if it changes
  in play, through an `Effect`. `GameState` and `testing::World` both hold one `Facilities`
  (`World` derefs to it), so both have it. **Saved**: a tag in `src/save/pb/world.rs` and a line in
  `sim/saving.rs` (`savegame` — new state is saved state).

## What is still per-kind, on purpose

- The **map's prop catalogues** — `map/props.rs` (passability), `map/utilities.rs` (power, water),
  `editor/props.rs` (art), `lighting/scene.rs` (light) — are linked by the prop's **name**, one row
  each, in the layer that owns the concern (`map`, `editor`, `lighting` skills).
- **Old save tags**: `pb::TaskKind` tags 3-19 are only ever read (each task's `TimedTask::legacy`);
  never extend them.
