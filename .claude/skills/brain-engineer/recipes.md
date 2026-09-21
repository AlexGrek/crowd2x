# Brain recipes

Templates for every extension point, in the order they depend on each other. Paths are
relative to `src/sim/`. Each template follows code that exists; when in doubt, open the named
existing example and match it.

Contents:

- [A stat, and the process that moves it](#a-stat-and-the-process-that-moves-it)
- [An item](#an-item)
- [An inventory](#an-inventory)
- [A feature (what a prop is for)](#a-feature-what-a-prop-is-for)
- [An action](#an-action)
- [A task executor](#a-task-executor)
- [A goal](#a-goal)
- [A routine (a need)](#a-routine-a-need)
- [Registering on a brain](#registering-on-a-brain)
- [Worked example: using the toilet](#worked-example-using-the-toilet)

---

## A stat, and the process that moves it

`biology/`. A `Human` has a `Biology` — its `Stats` plus its processes; a `Dog` has none.
**Only a process writes a stat.** The `Stats` setters are `pub(super)` to `biology`, so a task
cannot take hunger away itself; it reports an `Event` and the processes decide.

Stats that already exist but never change: `health`, `mental_health`, `attention`. **Prefer making one of those move** over adding a new one.

A new stat is, in `biology/stats.rs`: a field; a line in `Stats::random`; a getter; an entry in
`Stats::fields()` (the debug menu reads it); an entry in `Stats::calm()`; a `with_<stat>` test
builder; a `pub(super) fn change_<stat>(&mut self, by: f32)`; and the name in
`fields_are_named_in_declaration_order`.

A new process is its own file. Copy `biology/hunger.rs` for a stateless one and
`biology/bladder.rs` for one with state:

```rust
//! [`Tiredness`]: more tired with time, rested by sleeping.

use crate::sim::clock::HOUR;

use super::{Event, Process, ProcessId, Stats};

/// World hours from rested to exhausted. [A process runs on the world's clock
/// — `Think::game_dt`, two minutes to the watched second — so say what the
/// number means in hours of a person's day, not in seconds of watching.]
pub const HOURS_TO_EXHAUSTED: f32 = 16.0;

/// Stamina lost per world second.
pub const STAMINA_PER_SECOND: f32 = 100.0 / (HOURS_TO_EXHAUSTED * HOUR);

#[derive(Clone, Copy, PartialEq, Debug, Default)]   // Copy: Biology is inline in Human
pub struct Tiredness;

impl Process for Tiredness {
    fn id(&self) -> ProcessId {
        ProcessId::Tiredness
    }

    /// Time passing, in world seconds.
    fn advance(&mut self, stats: &mut Stats, dt: f32) {
        stats.change_stamina(-STAMINA_PER_SECOND * dt);
    }

    /// Something happened to the body. Ignore what does not concern it.
    fn handle(&mut self, event: Event, stats: &mut Stats) {
        if let Event::Slept { seconds } = event {
            stats.change_stamina(seconds * 2.0);
        }
    }
}
```

Then in `biology/mod.rs`:

1. a `ProcessId` variant with the next discriminant, plus `COUNT`, `ALL` and `name()`;
2. `pub mod` and `pub use` for the file;
3. a field on `Biology`, initialised in `Biology::new`;
4. its slot, **in id order**, in both `Biology::parts` and `Biology::processes`
   (`every_process_sits_in_the_slot_of_its_own_id` fails otherwise);
5. an `Event` variant if something new happens to a body (then an arm in every process that
   `match`es events exhaustively, like `Bladder`);
6. update `the_debug_view_says_which_processes_are_running_and_keeps_its_shape`.

A process that reacts to something already happening needs no new event. The bladder hears
`Event::Ingested(item)` and reads `item.hydration()`. Food reaching it later is a new process,
not a change to eating.

Every process can be switched off per body (`Biology::set_running`, `Command::SetProcess`), and
off means **nothing**: no time, no events. Tests for a process call `advance` and `handle` on the
struct directly with `Stats::calm().with_<stat>(..)`, no `World` needed.

A frozen unit's `react` is skipped, so its processes do not run. That is intended.

## An item

`item.rs`. An item says **what it is made of**, not what it does to a body or to whoever
carries it. Add a variant, bump `ItemKind::COUNT` and `ALL` (they size the inventory's count
array), and add an arm in each of `name`, `consume_seconds`, `nutrition`, `hydration`, `mass`
and `volume`:

```rust
pub enum ItemKind { Food, Water, Coffee }

pub const ALL: [ItemKind; ItemKind::COUNT] = [ItemKind::Food, ItemKind::Water, ItemKind::Coffee];
pub const COUNT: usize = 3;

pub const fn hydration(self) -> f32 {
    match self {
        ItemKind::Food => 0.0,
        ItemKind::Water => DRINK,
        ItemKind::Coffee => 30.0,
    }
}

/// Kilograms, counted wherever it is — a hand or stowed.
pub const fn mass(self) -> f32 { /* ... */ }

/// Litres, counted only when stowed.
pub const fn volume(self) -> f32 { /* ... */ }
```

`TakeItem` and `ConsumeItem` already work for any `ItemKind`, and `ConsumeItem` reports
`Event::Ingested(item)`, so the processes pick up the new numbers without further changes.
`consume_seconds` lives on the item so a goal that finds somebody else's item in its hand can
finish it without knowing what it is.

**Mass and volume must both be above zero** (`everything_that_can_be_carried_weighs_and_measures_something`):
a free item is a limit that does not limit.

## An inventory

`inventory.rs`. `Inventory` is a unit's **hand plus what it has stowed**, inline and `Copy` on
`Human`. You almost never change this file to add a behaviour; what matters is which slot your
task touches and which limit counts it:

|  | the hand | stowed |
| --- | --- | --- |
| **mass** | counted | counted |
| **volume** | **not** counted | counted |

- **A task writes it** (`ctx.inventory`, `&mut`); **a goal only reads it**
  (`ctx.inventory`, `&`). `held(ctx)` in `eat.rs`/`drink.rs` is
  `ctx.inventory.and_then(Inventory::hand)`.
- `set_hand` is **never refused**, whatever the limits say — a unit that could not pick food
  up because its pockets were full would starve beside a fridge. `stow` *is* refused, and
  returns `false` (`#[must_use]`); check `room_for` first if you want to plan around it.
- Nothing stows anything yet. A goal that fetches and puts away needs a new task that calls
  `stow`, and one that unstows before consuming — until then the hand is the only slot the
  brain uses, and stowed food is carried past a fridge rather than eaten.
- `Command::GiveItem` (QA's `give`) is how a test puts a unit in a laden state.

## A feature (what a prop is for)

`feature.rs`. A prop's **name** is what a map file stores and what the editor paints. A feature
binds that name to a use, and one name may have several uses (the fridge is `Food` and `Water`).

```rust
pub enum FeatureKind { Food, Water, Toilet, Bed }

pub const FEATURES: &[Feature] = &[
    // ...
    Feature { name: "bed 1", kind: FeatureKind::Bed },
];

pub struct Features {
    // ...
    bed: Vec<Point>,   // one Vec per kind: `nearest` scans only that kind
}

// in Features::from_map
FeatureKind::Bed => features.bed.push(object.cell()),

// in Features::cells
FeatureKind::Bed => &self.bed,
```

Goals then call `ctx.think.features.nearest(FeatureKind::Bed, here)`.

**The prop must be placeable in the editor.**
`every_feature_the_simulation_knows_is_a_prop_the_editor_can_place` fails otherwise.

- Check `src/editor/props.rs` `PALETTE` for the name first; "toilet", the beds, "trash can"
  and the crates already exist.
- A new prop needs a **16×16** PNG in `assets/` and `PaletteItem::upscaled("name", "file.png")`.
  Never draw it pre-upscaled; CLAUDE.md "Characters" says why.
- A new prop also needs a `map::PROPS` entry (`src/map/props.rs`) saying whether it can be
  walked through; `every_palette_prop_is_in_the_map_s_prop_catalogue` fails otherwise.

**Props block the cell their centre is in.** A feature is therefore used from one of the four
cells beside it — `goals::stand_beside` never offers the feature's own cell, and a feature with
no passable side is `Stand::Nowhere` (the goal returns `Blocked`). Use tasks with a
`manhattan_distance(..) <= 1` precondition, as `TakeItem` and `UseToilet` do. A test map that
puts a unit on a prop's cell, or a prop in a one-wide corridor, gets exactly what a real map
would: a wall.

Features are indexed once, in `GameState::new`, and never change. A feature that depletes (a
fridge that runs out) is state that changes during a tick, which is a design change: raise it
before writing it.

## An action

`brain/action.rs`. Add one only for a **new body activity** an animation would be picked by.
Most new tasks reuse `Interact`, `Consume` or `Wait`.

1. A variant, carrying `seconds` and `elapsed` if it is timed.
2. A `const fn` constructor with `elapsed: 0.0`.
3. An arm in `name()` and in `elapsed()`.
4. An arm in `advance()`. A timed action joins the existing
   `Interact | Consume | Wait` or-pattern; anything that moves the body must go through the
   `Walker`.
5. An arm in `describe()`.

Actions decide nothing and check no preconditions. Those belong to the task.

## A task executor

`brain/tasks/<name>.rs`. Copy the shape of `use_toilet.rs` (or `take_item.rs` for one that
touches hands).

```rust
//! [`UseToilet`]: use a toilet, from beside it.

use crate::map::Point;
use crate::sim::biology::Event;

use super::super::action::{Action, ActionState};
use super::super::task::{TaskCtx, TaskExecutor, TaskResult};

#[derive(Clone, Copy, PartialEq, Debug)]   // Copy is required: the queue is inline
pub struct UseToilet {
    pub toilet: Point,
    pub seconds: f32,
}

impl TaskExecutor for UseToilet {
    /// [Preconditions, and what finishing means.]
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        // 1. Preconditions, EVERY tick.
        if ctx.here().manhattan_distance(self.toilet) > 1 || ctx.biology.is_none() {
            return TaskResult::Failed;
        }
        // 2. Nothing running yet: start the action.
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.toilet, self.seconds);
            return TaskResult::Executing;
        }
        // 3. Advance; tell the body only on Finished. Never write a stat.
        match ctx.advance_action() {
            ActionState::Finished => {
                if let Some(biology) = ctx.biology.as_deref_mut() {
                    biology.handle(Event::Relieved);
                }
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }

    fn describe(&self) -> String {
        format!("use the toilet at {}, {}", self.toilet.x, self.toilet.y)
    }
}
```

Register it in three places:

- **`brain/tasks/mod.rs`:** `pub mod use_toilet;` and `pub use use_toilet::UseToilet;`
- **`brain/task.rs`:** add it to `use super::tasks::{..}`, then add
  - a variant `UseToilet(UseToilet)`;
  - a `pub const fn use_toilet(toilet: Point, seconds: f32) -> Task` constructor;
  - an arm in `Task::execute` and in `Task::describe`.
- **Goals** match it as `Some(Task::UseToilet(_))` in `ctx.finished`. Goals with an
  exhaustive `Stage::of` (`EatGoal`, `DrinkGoal`) need an arm for it too.

`TaskCtx` gives you: `think`, `outcome` (this tick's move), `walk`, `action`,
`biology: Option<&mut Biology>`, `inventory: Option<&mut Inventory>`, `here()`,
`advance_action()`. A task that walks is `MoveTo` and nothing else.

Tests use `tasks::rig::Rig`, whose `biology` changes only through events (time does not
advance it), so the effect of a task is exact. Set stats with
`rig.biology.edit(|stats| stats.with_bladder(90.0))`, and hands with `rig.set_hand(..)` /
`rig.hand()`. Minimum set, from `use_toilet.rs`:

- `using_a_toilet_from_across_the_room_fails_without_starting`
- `the_bladder_is_emptied_only_once_the_toilet_has_been_used`
- `walking_away_mid_use_fails_it_and_relieves_nothing`

## A goal

Four parts. **One goal per need, in its own file.** Do not parameterise an existing executor for
a second need, even when the plans match.

**1. `GoalId` in `brain/goal.rs`.** Add a variant with the next discriminant, then update
`COUNT` (a literal), `ALL`, `name()` and `emoji()` (one codepoint).
`the_ranked_list_is_highest_first_with_ties_in_goal_order` lists every goal and needs the new
one. Ties go to the lower discriminant, so declaration order is a tie-break policy.

**2. The executor, `brain/goals/<name>.rs`.** Copy `relieve.rs` for a walk-there-and-use goal,
or `eat.rs`/`drink.rs` for one that takes an item. The shape:

```rust
use super::{stand_beside, Stand, PATIENCE, WAIT_FOR_A_GAP};   // shared helpers, goals/mod.rs

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RelieveGoal {
    /// Decisions only — survive being put down.
    target: Option<Point>,
    retries: u8,
}

impl RelieveGoal {
    /// Queue the rest FROM WHAT IS TRUE NOW, on the back. `false` = cannot.
    fn plan(&mut self, ctx: &mut GoalCtx<'_>) -> bool {
        let Some(toilet) = self.target else { return false };
        match stand_beside(ctx, toilet) {
            Stand::Nowhere => return false,
            Stand::Here => {}
            Stand::At(cell) => { let _ = ctx.tasks.push_back(Task::move_to(cell)); }
        }
        let _ = ctx.tasks.push_back(Task::use_toilet(toilet, TOILET_SECONDS));
        true
    }
}

impl GoalExecutor for RelieveGoal {
    fn prioritized(..)   // queue is empty; if a target is remembered, plan again from the world
    fn deprioritized(..) // the finishing step made the routine let go: log it here
    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        // no body          -> Blocked
        // MoveTo failed    -> blocked_by a body: wait + plan, up to PATIENCE; else Blocked
        // other failure    -> clear, forget, Working (cheap)
        // last step done   -> log, forget, Achieved
        // tasks queued     -> Working
        // nothing queued   -> Features::nearest, or Blocked; then plan
    }
}
```

A goal that **takes an item** must first `Task::consume(other, other.consume_seconds())`
whatever else is in hand (see `EatGoal::plan`). Otherwise `TakeItem` fails forever.

**3. Register it in `brain/goals/mod.rs`:** `pub mod relieve;` and `pub use relieve::RelieveGoal;`

**4. Put it on a brain.** See [Registering on a brain](#registering-on-a-brain).

`GoalCtx` gives you:

- `think` — map, occupancy, log, features, `dt` (watched) and `game_dt()` (world), `tick`;
- `body` (a copy), `perception`;
- `biology: Option<&Biology>` and `inventory: Option<&Inventory>`, both **read-only**;
- `memory: &mut Memory`, `tasks: &mut Tasks`;
- `blocked_by: Option<Uid>`, `finished: Option<Task>`.

Implement `deprioritized` only when a result must not be lost. `EatGoal::deprioritized` logs
a meal whose last bite caused the handover.

## A routine (a need)

`brain/routines.rs`. A need is a `Need` const for the existing `NeedRoutine`, not a new routine
type:

```rust
/// Bladder at which a human decides it is time for the toilet.
pub const BURSTING: f32 = 70.0;
/// Bladder it has to get down to before it stops thinking about it.
pub const RELIEVED: f32 = 10.0;

pub const BLADDER: Need = Need {
    routine: "stay comfortable",   // shown in the brains menu's routine list
    feeling: "bursting",           // `bursting yes` while committed
    stat: Stats::bladder,
    process: ProcessId::Bladder,   // switched off -> not wanted at all
    goal: GoalId::Relieve,
    commit_at: BURSTING,
    release_at: RELIEVED,
};
```

Priority is `stat / 50`, the same scale for every need. Hysteresis (commit high, release low)
is built in. Test it like `bladder_hovering_at_the_threshold_does_not_flip_the_decision`.

Write a new routine type only for something that is not a stat with thresholds (a time of day,
a threat). A routine type is:

- a struct in `brain/routines.rs` implementing `RoutineExecutor` (`name`, `arrange`,
  `debug_fields`). Keep it small: it is stored inline, once per unit, and the largest variant
  sizes every routine slot. Put anything shared by every unit (thresholds, names) in a
  `&'static` descriptor, as `Need` does, and keep only per-unit state in the struct.
- a variant in `Routine` (`brain/routine.rs`), a `const fn` constructor beside
  `Routine::need`, and its arm in `Routine::name`, `arrange` and `debug_fields`.

Such a routine:

- **raises every tick** it wants something, because the list is zeroed before routines run;
- only ever calls `goals.raise_to` and never queues tasks;
- keeps its decision state (hysteresis) in its own fields;
- reads `ctx.biology` (stats and switches), `ctx.body`, `ctx.think`;
- is tested the way `routines.rs` tests do: build a `RoutineCtx` by hand, read
  `goals.priority(..)`, and walk the thresholds up and down.

## Registering on a brain

`brain/mod.rs`:

```rust
pub fn human() -> Brain {
    Brain::new(
        [
            Routine::need(&HUNGER),
            Routine::need(&THIRST),
            Routine::need(&BLADDER),
            Routine::stay_busy(),
        ],
        [
            Box::new(WanderGoal::new()) as Box<dyn GoalExecutor>,
            Box::new(EatGoal::new()),
            Box::new(DrinkGoal::new()),
            Box::new(RelieveGoal::new()),
        ],
    )
}
```

- **A brain takes as many routines as it is given.** They go into a `Box<[Routine]>` at
  construction (the spawn pass, which may allocate), so there is no cap and no spare slots.
  `Brain::new` panics only on two executors for one goal.
- Routine order is fixed at construction, and determinism depends on that.
- Update `a_human_s_brain_says_what_it_is_doing`, which asserts the routine list.
- Watch `a_human_stays_small_enough_to_be_worth_a_thousand_of` (`size_of::<Human>() <= 576`,
  496 today). Routines do not count against it. A goal slot (16 bytes, per `GoalId`, on every
  kind) and a stat do: raise the wall by a cache line on purpose, with a note, never silently.

A new **unit kind** (not just a new behaviour) is:

- an `EntityType` variant in `uid.rs`;
- a struct in `kinds.rs` holding `walk: Walker` and `brain: Brain`, plus what it has
  (a `Biology`? hands?), with `biology`/`biology_mut` overridden if it has a body;
- an arm in `kinds::build`;
- a `Brain::<kind>()` constructor;
- the drawing side in `game/actors.rs` / `characters/`, where the `dev` skill applies.

---

## Worked example: using the toilet

Built, so read the real files. As a checklist for the next need:

1. **Stat + process.** `bladder` existed; `biology/bladder.rs` makes it fill slowly
   (`BLADDER_PER_SECOND`) and quickly after a drink (hydration put `on_its_way`, arriving at
   `FILLING_PER_SECOND`), and empties it on `Event::Relieved`. Tests call the process directly.
2. **Feature.** `FeatureKind::Toilet` ↔ `"toilet"`; the palette entry already existed. Test:
   `a_toilet_on_the_map_is_a_toilet_in_the_index_and_nothing_else`.
3. **Task.** `tasks/use_toilet.rs`, with its three `Rig` tests.
4. **Goal.** `GoalId::Relieve` (🚽) and `goals/relieve.rs`. Tests through `testing::World` with
   `needy_human` and `prop_at`:
   - `a_bursting_human_walks_to_the_toilet_and_is_relieved`;
   - `a_bursting_human_with_no_toilet_in_the_world_goes_back_to_wandering` (cooldown > 0);
   - `a_toilet_nobody_can_reach_is_given_up_on_rather_than_retried_every_tick` (`ROUTES_ASKED`);
   - `drinking_is_what_sends_a_human_to_the_toilet_in_a_minute_and_a_half`, which switches
     processes off to isolate the effect;
   - `a_bursting_thirsty_human_sees_to_the_worse_first_and_loses_neither` (handover).
5. **Routine.** The `BLADDER` need, with a threshold test.
6. **Register** on `Brain::human()`.
7. **Determinism.** `kitchen()` in `sim/mod.rs` tests has toilets, and
   `determinism_survives_brains_and_biology_in_the_parallel_rounds` asserts reliefs > 0.
8. **QA.** `qa/toilet.json`: an inline map with a fridge and a toilet, and `expect_log` on
   `used the toilet at`.
9. **Verify.**
   - `cargo test`;
   - break each new guard once and see its test fail, then restore;
   - `uv run tools/qa.py`;
   - `qa/perf_simulation.json` compared with the last run;
   - open **brains** on a unit and watch `relieve` climb, take over, and hand back.
10. **Docs.** CLAUDE.md "The brain" and "Biology".

---

## Worked example: a go on the computer

Built, and the one to copy when the *prop* should show what is happening. The brain half is
the toilet's checklist with a `Beside` feature; what is new is everything after step 6.

1. **Stat + process.** `fun` existed and nothing moved it; `biology/fun.rs` drains it
   (`FUN_PER_SECOND`, ten world hours) and puts `AMUSEMENT` back on `Event::Entertained`.
   It is the one stat that falls, so `Stats::boredom` (`100 - fun`) is what the routine
   reads — turning it over in the stat rather than in the routine is what keeps every need
   on one scale.
2. **Feature.** `FeatureKind::Entertainment` ↔ `"computer"`, `Access::Beside`, plus the
   `map::PROPS` and `editor::props::PALETTE` entries the cross-checking tests demand.
3. **Task.** `tasks/use_computer.rs` — `Action::interact` and nothing else, since a
   `Beside` feature needs no entering.
4. **Goal.** `GoalId::Play` (🎮) and `goals/play.rs`.
5. **Routine.** The `BOREDOM` need, commit 60, release 20.
6. **Register** on `Brain::human()`, and add the prop to `kitchen()` in `sim/mod.rs` tests
   so `determinism_survives_brains_and_biology_in_the_parallel_rounds` runs the new goal.
7. **Art.** A 16x16 strip per state — `computer_idle.png` (screen dark) and `computer.png`
   (screen on) — declared as one palette entry:
   `PaletteItem::animated("computer", "computer_idle.png", 10).used("computer.png", 11)`.
   `tools/art_scale.py shrink` is what reduces art that arrived pre-upscaled.
8. **The screen.** `game/props.rs` asks every entity `interacting_with()` and lights the
   props standing in those cells. Nothing is added to the simulation for this: a feature
   has no state of its own, and "in use" is a fact about the unit using it.
9. **QA.** `qa/computer.json` — pauses the game so only scripted ticks move the world,
   switches the other three processes off on the selected unit, then photographs the exact
   tick the screen is on and asserts `expect_prop_in_use`.
