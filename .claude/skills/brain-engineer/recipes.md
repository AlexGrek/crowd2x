# Brain recipes

Templates for every extension point, in the order they depend on each other. Paths are
relative to `src/sim/`. Each template follows code that exists; when in doubt, open the named
existing example and match it.

Contents:

- [A stat that changes](#a-stat-that-changes)
- [An item](#an-item)
- [A feature (what a prop is for)](#a-feature-what-a-prop-is-for)
- [An action](#an-action)
- [A task executor](#a-task-executor)
- [A goal](#a-goal)
- [A routine](#a-routine)
- [Registering on a brain](#registering-on-a-brain)
- [Worked example: using the toilet](#worked-example-using-the-toilet)

---

## A stat that changes

Example: `hunger` in `stats.rs`. A `Human` has `Stats`; a `Dog` has none.

Stats that already exist but never change: `health`, `stamina`, `fun`, `bladder`,
`mental_health`, `attention`. **Prefer making one of those move** over adding a new one.

A new stat takes seven edits in `stats.rs`:

1. a field;
2. a line in `Stats::random`;
3. a getter;
4. an entry in `Stats::fields()`, which the debug menu reads;
5. an entry in `Stats::calm()`;
6. the stat-name list in its test;
7. the change itself, with a rate const beside `HUNGER_PER_SECOND`:

```rust
/// Bladder filled per second. [Say what the number means in play time.]
pub const BLADDER_PER_SECOND: f32 = 0.8;

impl Stats {
    /// Time passing. Extend the existing method rather than adding a second
    /// per-tick call — `Human::react` calls this once, before the brain runs.
    pub fn metabolise(&mut self, dt: f32) {
        self.hunger = (self.hunger + HUNGER_PER_SECOND * dt).clamp(0.0, 100.0);
        self.bladder = (self.bladder + BLADDER_PER_SECOND * dt).clamp(0.0, 100.0);
    }

    /// A need met. Called by a **task executor**, never by a goal.
    pub fn relieve(&mut self) {
        self.bladder = 0.0;
    }
}
```

Test hook for goal tests, in `kinds.rs` beside `set_hunger`:

```rust
#[cfg(test)]
pub(crate) fn set_bladder(&mut self, bladder: f32) {
    self.stats = self.stats.with_bladder(bladder);   // add `with_bladder` beside `with_hunger`
}
```

A frozen unit's `react` is skipped, so its stats do not move. That is intended.

## An item

`item.rs`. One hand, one item: `carried: Option<ItemKind>` on `Human`. Add a variant, its
`name`, and its effect in `consume`:

```rust
pub enum ItemKind { Food, Water }

pub const fn name(self) -> &'static str {
    match self { ItemKind::Food => "food", ItemKind::Water => "water" }
}

pub fn consume(self, stats: &mut Stats) {
    match self {
        ItemKind::Food => stats.eat(MEAL),
        ItemKind::Water => stats.drink(DRINK),
    }
}
```

`TakeItem` and `ConsumeItem` already work for any `ItemKind`, so no new task is needed.

## A feature (what a prop is for)

`feature.rs`. A prop's **name** is what a map file stores and what the editor paints. A feature
binds that name to a use.

```rust
pub enum FeatureKind { Food, Toilet }

pub const FEATURES: &[Feature] = &[
    Feature { name: "fridge", kind: FeatureKind::Food },
    Feature { name: "toilet", kind: FeatureKind::Toilet },
];

pub struct Features {
    food: Vec<Point>,
    toilet: Vec<Point>,   // one Vec per kind: `nearest` scans only that kind
}

// in Features::from_map
Some(FeatureKind::Toilet) => features.toilet.push(object.cell()),

// in Features::cells
FeatureKind::Toilet => &self.toilet,
```

Goals then call `ctx.think.features.nearest(FeatureKind::Toilet, here)`.

**The prop must be placeable in the editor.**
`every_feature_the_simulation_knows_is_a_prop_the_editor_can_place` fails otherwise.

- Check `src/editor/props.rs` `PALETTE` for the name first; "toilet", the beds, "trash can"
  and the crates already exist.
- A new prop needs a **16×16** PNG in `assets/` and `PaletteItem::upscaled("name", "file.png")`.
  Never draw it pre-upscaled; CLAUDE.md "Characters" says why.

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

`brain/tasks/<name>.rs`. Copy the shape of `take_item.rs`.

```rust
//! [`UseToilet`]: use a toilet, from beside it.

use crate::map::Point;

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
        if ctx.here().manhattan_distance(self.toilet) > 1 || ctx.stats.is_none() {
            return TaskResult::Failed;
        }
        // 2. Nothing running yet: start the action.
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.toilet, self.seconds);
            return TaskResult::Executing;
        }
        // 3. Advance; apply the effect only on Finished.
        match ctx.advance_action() {
            ActionState::Finished => {
                if let Some(stats) = ctx.stats.as_deref_mut() {
                    stats.relieve();
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
- **Goals** match it as `Some(Task::UseToilet(_))` in `ctx.finished`.

`TaskCtx` gives you: `think`, `outcome` (this tick's move), `walk`, `action`,
`stats: Option<&mut Stats>`, `carried: Option<&mut Option<ItemKind>>`, `here()`,
`advance_action()`. A task that walks is `MoveTo` and nothing else.

Tests use `tasks::rig::Rig`. A minimum set:

```rust
#[cfg(test)]
mod tests {
    use super::super::rig::Rig;
    use super::super::super::task::Task;
    use super::*;
    use crate::map::{Map, Size, FLOOR};

    #[test]
    fn using_a_toilet_from_across_the_room_fails_without_starting() {
        let mut rig = Rig::new(Map::new(Size::new(9, 9), FLOOR), Point::new(1, 1));
        let mut task = Task::use_toilet(Point::new(6, 6), 1.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert!(rig.action.is_none());
    }

    #[test]
    fn the_need_is_met_only_once_the_action_is_done() {
        let mut rig = Rig::new(Map::new(Size::new(9, 9), FLOOR), Point::new(5, 6));
        let mut task = Task::use_toilet(Point::new(6, 6), 0.5);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        // [assert the stat has not changed yet]
        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        // [assert it has now]
    }
}
```

## A goal

Four parts.

**1. `GoalId` in `brain/goal.rs`.** Add a variant with the next discriminant, then update all
three of `COUNT` (a literal), `ALL` and `name()`.
`the_ranked_list_is_highest_first_with_ties_in_goal_order` lists every goal and needs the new
one. Ties go to the lower discriminant, so declaration order is a tie-break policy.

**2. The executor, `brain/goals/<name>.rs`.** Copy the shape of `eat.rs`.

```rust
use crate::map::Point;
use crate::sim::feature::FeatureKind;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::{stand_beside, Stand, PATIENCE, WAIT_FOR_A_GAP};

pub const USE_SECONDS: f32 = 3.0;

/// [What this goal pursues.]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RelieveGoal {
    /// Decisions only — survive being put down.
    target: Option<Point>,
    retries: u8,
}

impl RelieveGoal {
    pub fn new() -> RelieveGoal { RelieveGoal::default() }

    /// Queue the rest FROM WHAT IS TRUE NOW, on the back. `false` = cannot.
    fn plan(&mut self, ctx: &mut GoalCtx<'_>) -> bool {
        let Some(toilet) = self.target else { return false };
        match stand_beside(ctx, toilet) {
            Stand::Nowhere => return false,
            Stand::Here => {}
            Stand::At(cell) => { let _ = ctx.tasks.push_back(Task::move_to(cell)); }
        }
        let _ = ctx.tasks.push_back(Task::use_toilet(toilet, USE_SECONDS));
        true
    }

    fn forget(&mut self) { *self = RelieveGoal::default(); }
}

impl GoalExecutor for RelieveGoal {
    fn goal(&self) -> GoalId { GoalId::Relieve }

    /// Queue is empty; rebuild from the world.
    fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        if self.target.is_some() {
            self.retries = 0;
            if !self.plan(ctx) { self.forget(); }
        }
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        if ctx.stats.is_none() {
            return GoalProgress::Blocked;           // a kind without the need
        }
        match (last, ctx.finished) {
            (TaskResult::Failed, Some(Task::MoveTo(_))) => {
                ctx.tasks.clear();
                if ctx.blocked_by.is_some() && self.retries < PATIENCE {
                    self.retries += 1;                // a body: wait, go again
                    let _ = ctx.tasks.push_back(Task::wait(WAIT_FOR_A_GAP));
                    if self.plan(ctx) { return GoalProgress::Working; }
                }
                self.forget();
                return GoalProgress::Blocked;         // no way there: expensive miss
            }
            (TaskResult::Failed, _) => {              // a step went wrong: replan next tick
                ctx.tasks.clear();
                self.forget();
                return GoalProgress::Working;
            }
            (TaskResult::Success, Some(Task::UseToilet(_))) => {
                self.forget();
                return GoalProgress::Achieved;
            }
            _ => {}
        }
        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }
        let here = ctx.body.center_position();
        let Some(toilet) = ctx.think.features.nearest(FeatureKind::Toilet, here) else {
            return GoalProgress::Blocked;             // none on the map
        };
        self.target = Some(toilet);
        if self.plan(ctx) { GoalProgress::Working } else { self.forget(); GoalProgress::Blocked }
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![("toilet", self.target.map_or("none".into(), |c| format!("{}, {}", c.x, c.y)))]
    }
}
```

`stand_beside` and `Stand` are currently private to `eat.rs`. The first second user moves them
to `brain/goals/mod.rs` as `pub(super)`; do not copy them.

**3. Register it in `brain/goals/mod.rs`:** `pub mod relieve;` and `pub use relieve::RelieveGoal;`

**4. Put it on a brain.** See [Registering on a brain](#registering-on-a-brain).

`GoalCtx` gives you:

- `think` — map, occupancy, log, features, `dt`, `tick`;
- `body` (a copy), `perception`;
- `stats: Option<&Stats>` and `carried: Option<&Option<ItemKind>>`, both **read-only**;
- `memory: &mut Memory`, `tasks: &mut Tasks`;
- `blocked_by: Option<Uid>`, `finished: Option<Task>`.

Implement `deprioritized` only when a result must not be lost. `EatGoal::deprioritized` logs
a meal whose last bite caused the handover.

## A routine

`brain/routines.rs`. Copy `KeepFedRoutine`.

```rust
/// Bladder at which the unit decides to go.
pub const BURSTING: f32 = 70.0;
/// Bladder it has to get down to before it stops thinking about it.
pub const RELIEVED: f32 = 10.0;

/// Maps bladder onto how badly [`GoalId::Relieve`] is wanted, with hysteresis.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct KeepComfortableRoutine {
    committed: bool,
}

impl Routine for KeepComfortableRoutine {
    fn name(&self) -> &'static str { "keep comfortable" }

    fn arrange(&mut self, ctx: &RoutineCtx<'_>, goals: &mut Goals) {
        let Some(stats) = ctx.stats else { self.committed = false; return };
        let bladder = stats.bladder();
        if bladder >= BURSTING { self.committed = true; }
        else if bladder <= RELIEVED { self.committed = false; }
        if self.committed {
            goals.raise_to(GoalId::Relieve, bladder / 50.0);   // same 0-2 scale as hunger
        }
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![("bursting", if self.committed { "yes" } else { "no" }.to_string())]
    }
}
```

A routine:

- **raises every tick** it wants something, because the list is zeroed before routines run;
- only ever calls `goals.raise_to` and never queues tasks;
- keeps its decision state (hysteresis) in its own fields;
- is tested the way `routines.rs` tests do: build a `RoutineCtx` by hand, read
  `goals.priority(..)`, and walk the thresholds up and down.

## Registering on a brain

`brain/mod.rs`:

```rust
pub fn human() -> Brain {
    Brain::new(
        [
            Box::new(KeepFedRoutine::new()) as Box<dyn Routine>,
            Box::new(KeepComfortableRoutine::default()),
            Box::new(StayBusyRoutine),
        ],
        [
            Box::new(WanderGoal::new()) as Box<dyn GoalExecutor>,
            Box::new(EatGoal::new()),
            Box::new(RelieveGoal::new()),
        ],
    )
}
```

- `MAX_ROUTINES` is 4, and `Brain::new` panics past it or on two executors for one goal. Raise
  the constant rather than working round it; it sizes an inline array.
- Routine order is fixed at construction, and determinism depends on that.
- Update `a_human_s_brain_says_what_it_is_doing`, which asserts the routine list.
- Watch `a_human_stays_small_enough_to_be_worth_a_thousand_of` (`size_of::<Human>() <= 512`)
  when adding goal fields.

A new **unit kind** (not just a new behaviour) is:

- an `EntityType` variant in `uid.rs`;
- a struct in `kinds.rs` holding `walk: Walker` and `brain: Brain`, plus what it has
  (stats? hands?);
- an arm in `kinds::build`;
- a `Brain::<kind>()` constructor;
- the drawing side in `game/actors.rs` / `characters/`, where the `dev` skill applies.

---

## Worked example: using the toilet

The whole of a new behaviour, as a checklist. The sections above hold the code.

1. **Stat.** `bladder` exists; make it rise in `Stats::metabolise` and add `Stats::relieve`,
   `with_bladder`, `Human::set_bladder`. Test: `bladder_fills_with_time_and_never_past_bursting`.
2. **Feature.** `FeatureKind::Toilet` ↔ `"toilet"`. The palette entry already exists. Test:
   extend `a_fridge_on_the_map_is_food_in_the_index_and_a_bed_is_nothing`, or add a sibling.
3. **Task.** `UseToilet` with the two `Rig` tests above, plus
   `walking_away_mid_use_fails_it`.
4. **Goal.**
   - `GoalId::Relieve` (update `COUNT`, `ALL`, `name`, the ranked-list test);
   - `RelieveGoal`;
   - move `stand_beside`/`Stand` into `goals/mod.rs`.

   Tests through `testing::World`, each with a toilet prop at pixel coords:
   - `a_bursting_human_walks_to_the_toilet_and_is_relieved`;
   - `a_bursting_human_with_no_toilet_goes_back_to_wandering` (cooldown > 0);
   - `a_toilet_nobody_can_reach_is_given_up_on_rather_than_retried_every_tick` (`ROUTES_ASKED`).
5. **Routine.** `KeepComfortableRoutine` with a threshold test like
   `hunger_hovering_at_the_threshold_does_not_flip_the_decision`.
6. **Register** on `Brain::human()`. Handover test: a human both hungry and bursting does the
   more urgent one first, then the other, and neither is lost.
7. **Determinism.** Add a toilet to `kitchen()` in `sim/mod.rs` tests so both needs run
   across the parallel threshold.
8. **QA.** Copy `qa/eating.json` to `qa/toilet.json`, with an inline map holding a toilet, and
   `expect_log` on a line the goal writes on `Achieved`.
9. **Verify.**
   - `cargo test`;
   - break the gate or the back-off once to see the route-count test fail, then restore it;
   - `uv run tools/qa.py`;
   - `qa/perf_simulation.json` compared with the last run;
   - open **brains** on a unit and watch `relieve` climb, take over, and hand back.
10. **Docs.** Add the new goal, task and feature kind to CLAUDE.md "The brain".
