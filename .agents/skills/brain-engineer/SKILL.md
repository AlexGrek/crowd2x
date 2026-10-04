---
name: brain-engineer
description: Add and change what crowd2x units decide and do - the unit brain in src/sim/brain/ and the body in src/sim/biology/. Covers routines (what a unit wants and how badly), goals and goal executors (how a want is pursued), task executors (one small step like walk, take, consume, use the toilet, wait), actions (pathfinding and timing), features (what a map prop is for, e.g. a fridge is food and water), items, stats/needs such as hunger, thirst or bladder, and biological processes that change stats and can be switched on/off per unit. Use when adding a behaviour, need, goal, routine, task, action, item or prop use; when a unit gets stuck, dithers between goals, stands still, or replans too often; or when writing brain tests.
---

# Engineering the unit brain

Every unit's decisions live in `src/sim/brain/`, run from `GameEntity::react`. It is plain
Rust with **no `bevy::` imports**, and it is tested with `cargo test`, no `App`. AGENTS.md
("The brain") has the architecture. This skill is how to extend it without breaking the three
things it exists to protect: **no per-agent per-tick search, no allocation in a tick,
determinism**.

Code templates for every extension point are in [recipes.md](recipes.md); read it before
writing any of them.

## The layers, and who may do what

| Layer | Answers | Reads | May change | Lives in |
| --- | --- | --- | --- | --- |
| routine | *how badly is each goal wanted?* | biology (stats, switches), body, `Think` | the priority list only | `brain/routines.rs` |
| goal executor | *how is this want pursued?* | biology, hands, memory, `Think`, last task result | the task queue, memory, its own fields | `brain/goals/` |
| task executor | *how is one small step done?* | everything in `TaskCtx` | the action, hands, events to the body — the world | `brain/tasks/` |
| action | *what is the body doing, how long for?* | walker, `dt`, move outcome | its own clock, the walker's route | `brain/action.rs` |
| feature | *what is this prop for?* | the map's props, once | nothing | `sim/feature.rs` |
| item | *what can be held, what is it made of?* | — | nothing | `sim/item.rs` |
| inventory | *what is being carried, and does it still fit?* | its own slots | only a task writes it | `sim/inventory.rs` |
| process | *what does a body do by itself?* | its stats, **world** `dt`, events | **the only writer of stats** | `sim/biology/` |

**Goals decide, tasks act, processes change the body.** A goal never writes the body or hands
(`GoalCtx` gives it read-only references for exactly this reason). Food goes into a hand when
a `TakeItem` *finishes*, not when a goal hears that it did. That is what makes it safe for a
goal to miss a result. A task never writes a stat either: it calls
`biology.handle(Event::Ingested(item))` / `Event::Relieved`, and the processes decide what that
means (the `Stats` setters are private to `biology`, so this is compiler-held).

## The pipeline (every tick, in this order)

```
0. perception.observe            stub
1-2. every routine arranges the list (zeroed first, so a routine raises EVERY tick)
3. top changed? old.deprioritized -> current task abandoned, walk halted
                -> queue cleared -> new.prioritized
4. top.process(last)             ONLY if the goal just changed or no task is current
6. current task's executor runs  (pops the queue front if none is current)
```

This order has consequences:

- A task that ends at tick N is heard by its goal at N+1, and the next task starts at N+1.
  There is **one tick between tasks**.
- A goal that returns `Working` with nothing queued is asked again next tick. That is fine
  for a cheap miss and a disaster for anything that searched.
- When a handover happens the tick after a task ended, the old goal never gets `process` for
  that ending. It gets it in **`deprioritized`** (`ctx.tasks.result()`, `ctx.finished`).
  `EatGoal` logs a meal there because the last bite is what makes hunger let go.

## Where does my change go?

| I want units to... | Add |
| --- | --- |
| care about a new need | a **`Need`** for `NeedRoutine` (plus a stat and a **process** if the need is new) |
| care about a time of day, a threat | a **routine** |
| have a body change by itself, or react to what happened to it | a **process** in `sim/biology/` |
| do a new multi-step thing ("sleep in a bed") | a **goal** (`GoalId` variant + its own executor, one per need) |
| do a new kind of single step ("sit", "open door") | a **task executor** |
| show a new kind of body activity (animation) | an **action** variant |
| use a prop on the map | a **feature** entry (+ palette entry if the prop is new) |
| carry a new thing | an **item** variant with what it is made of (`nutrition`, `hydration`, `consume_seconds`, `mass`, `volume`), **and its art** in `characters/item.rs` — a texture or an emoji; `item::art` has no wildcard arm, so it will not compile without one |
| change *when* something is chosen | the routine's priority curve, not the goal |
| change *how* something is done | the goal executor, not the routine |

Never add a Bevy system that decides something, a `Component` holding agent state, or a new
`Intent` variant for a behaviour. `Intent` stays `Idle | Move`.

## Rules that were each a bug, or would be

<rules>

1. **Never search inside `process` or a routine.** Queue `Task::move_to(cell)`. A far route is
   asked for in exactly one place, `Action::walk_to`, once per walk started. If a goal needs to
   know reachability, it finds out from the `MoveTo` failing.

2. **Every `process` call ends in one of these:** it queued a task; it returned `Blocked`
   (expensive miss, so the goal is held off 32 ticks, doubling); it returned `Achieved`; or it
   returned `Working` after a *cheap* miss (a few random candidates landed on walls). Anything
   else is polled every tick.

3. **Tell the two failures of a walk apart with `ctx.blocked_by`.**
   - `Some(uid)`: a body was in the way, and bodies move. Queue `Task::wait(WAIT_FOR_A_GAP)`,
     then the same walk again, up to `PATIENCE` retries, counted in a field.
   - `None`: there is no way there. Return `Blocked`.

4. **Resume from the world, not from a remembered stage.** In `prioritized`, rebuild the queue
   from what is true now: what is in hand, where the unit stands. Keep only *decisions* in
   fields (which fridge, which target cell). A half-walked route is stale; a chosen cell is not.

5. **`prioritized` sees an empty queue; `deprioritized` still sees its own tasks** and the
   unread result. Use `deprioritized` only for results that must not be lost (logging, a
   one-off effect the goal owns).

6. **`Achieved` does not hand over.** If the routine still wants the goal, it stays on top and
   `process` runs again next tick with `InProgress`. Make that restart correct ("second meal").

7. **Task preconditions are checked every tick, not once.** A task must fail if the world
   stopped allowing it mid-action (walked away, hand emptied). Effects are applied only on
   `ActionState::Finished`.

8. **A task does one thing and never walks.** `TakeItem` from two cells away fails; the goal
   should have queued a `MoveTo` first. One task, one action.

9. **No allocation in a tick.**
   - `Task` and every task struct must be `Copy`; the queue is a fixed ring of 8, and `push_*`
     returns `false` when full (`#[must_use]` — write `let _ =` only when full cannot happen).
   - Goal and routine fields are inline.
   - A `Vec` is allowed in `debug_fields` only.

10. **Determinism.**
    - Randomness comes only from `crate::sim::rng::tick_rng(ctx.body.uid(), ctx.think.tick)`.
    - No `HashMap` iteration (memory is a `BTreeMap`).
    - Routine and executor order is fixed in `Brain::human()`/`dog()`.
    - Compare priorities with plain `>` (NaN never wins).

11. **Nothing scans the crowd.** A goal may read `ctx.think.occupancy` for a handful of named
    cells (`stand_beside` reads five). "Nearest X" comes from an index built once
    (`Features::nearest`), never from walking `entities()`.

12. **Rate-limit log lines from per-tick paths** on `ctx.think.tick.is_multiple_of(64)`, as
    `WanderGoal::report_stuck` does. One line per unit per tick floods the bounded log.

13. **A kind without the needs or hands a goal requires**: either don't register the executor
    on that kind's brain, or return `Blocked` when `ctx.biology` / `ctx.inventory` is `None`
    (`EatGoal` does).

14. **One goal per need, in its own file.** Do not fold two needs into one parameterised
    executor even when their plans match today (eating and drinking did); the processes
    behind them diverge. Share helpers (`stand_beside`) in `goals/mod.rs`, not plans.

15. **A goal that takes an item must cope with a hand already holding another one.** Taking
    needs an empty hand, so queue `Task::consume(other, other.consume_seconds())` first
    (`EatGoal`/`DrinkGoal::plan`). Otherwise an item taken just before another goal took over
    fails every `TakeItem` forever, and nothing in charge wants to use it.

16. **Off means off.** A `Need` names its `ProcessId`; `NeedRoutine` wants nothing while that
    process is switched off in the body. A routine that read the stat alone would chase a
    number that can no longer move.

17. **There are two clocks, and a duration says which it is on** (`sim/clock.rs`). A second
    of watching is two minutes of world.
    - A **process** is handed `Think::game_dt` — world seconds. Write its rate as
      `100.0 / (hours * HOUR)`, because a body's needs are in hours.
    - Everything **watched** — a walk, an action being stood through, `WAIT_FOR_A_GAP` — is
      measured in `Think::dt`, watched seconds, or a crowd teleports. A duration there that
      means something in world time is written `watched(15.0 * MINUTE)` and converted at the
      point it is defined.

</rules>

## Priority scale

Priorities are unitless `f32`s compared only against each other.

| Source | Value |
| --- | --- |
| `StayBusyRoutine` (wander) | `BUSY = 0.1`, the floor everything should beat when it matters |
| `NeedRoutine(HUNGER)` → eat | `hunger / 50`: 1.2 at `PECKISH` (60), released at `SATED` (25) |
| `NeedRoutine(THIRST)` → drink | `thirst / 50`: 1.2 at `THIRSTY` (60), released at `QUENCHED` (25) |
| `NeedRoutine(BLADDER)` → relieve | `bladder / 50`: 1.4 at `BURSTING` (70), released at `RELIEVED` (10) |
| `Idle` | 0; wins only when nothing is wanted or everything is held off |

Put a new need on the same 0–2 scale so that urgency is comparable across needs. Give a
routine **hysteresis** (commit at a high threshold, release at a low one, with a `committed`
field) whenever the stat it reads is changed by the goal it raises, or units will turn round
halfway there.

## Workflow

1. **Decide the layer(s)** with the table above. A new behaviour is usually a stat plus a
   routine plus a goal, reusing existing tasks. A new task or action is rarer.
2. **Write it from [recipes.md](recipes.md)**, in dependency order: stat/process/item/feature →
   action → task → goal → routine (`Need`) → registration in `Brain::human()`/`dog()`.
3. **Write unit tests alongside each piece** (below). Names are full sentences in snake_case;
   tests are inline `#[cfg(test)] mod tests`.
4. **Run `cargo test`.** The two determinism tests in `src/sim/mod.rs` are the gate on any brain
   change. If you added a goal, extend `determinism_survives_brains_and_biology_in_the_parallel_rounds`
   (or a sibling) so the new goal actually runs in it.
5. **Prove a new guard test fails** by breaking what it guards, run it, then restore. Tests in
   this repo have passed vacuously before.
6. **Add a QA test** if the behaviour is visible, copying `qa/eating.json`:
   - an inline map with the props;
   - `spawn`, `tick`, `expect_log`;
   - a `shot` of the brains menu.
7. **Run `uv run tools/qa.py`, then `qa/perf_simulation.json`.** Compare the 4000-humans median
   with the previous run on the same machine. The perf test runs only 200 ticks, so a behaviour
   that triggers later is not measured there; add a `measure` step to a test that reaches it if
   the behaviour does real work.
8. **Update AGENTS.md "The brain"** if the change adds a layer member worth naming (a new goal,
   task, feature kind or stat that moves).

## Testing tools that already exist

- **`crate::sim::testing::World`** — one unit, the real think / move-rules / react sequence.
  `World::new(map)`, `step(&mut entity)`, `log_contains(..)`, `lines()`, `meals()`, `drinks()`,
  `reliefs()`, `ctx()`, pub `dt`, `tick`, `occupancy` (claim cells to put bodies in the way).
- **`crate::sim::testing::{needy_human, prop_at}`** — `needy_human(cell, hunger, thirst,
  bladder)` so a test decides which needs press; `prop_at(&mut map, "toilet", cell)` puts a prop
  in the middle of a cell (object positions are **pixels**, and this does the conversion).
- **Switching a process off in a test:** `human.biology_mut().unwrap().set_running(ProcessId::Hunger, false)`
  isolates one need from the others without a hook — the same switch `Command::SetProcess` uses.
- **`crate::sim::brain::tasks::rig::Rig`** — a task executor with no brain:
  `Rig::new(map, cell)`, pub `walk`/`action`/`biology`/`inventory` (with `hand()`/`set_hand(..)`
  for the slot the item tasks use), `tick(&mut task)`,
  `run(&mut task, max_ticks)`. Its biology does not advance with time, only with events, so a
  task's effect on a stat is exact.
- **Brain test doubles in `brain/mod.rs` tests:**
  - `Puppet` — a walker plus a brain, driven by `World::step`;
  - `dial(goal, level)` — a `Routine::Dial` (test-only variant, struct in `routines.rs`) whose
    priority a test turns up and down;
  - `Recorder` / `Listener` — executors that journal every call.
- **Test hooks:** `Human::set_hunger`/`set_thirst`/`set_bladder`/`set_carried`/`inventory_mut`,
  `Biology::edit(|stats| stats.with_thirst(..))`, and `Stats::calm().with_hunger(..)`.
  Add `#[cfg(test)] pub(crate)` hooks the same way rather than widening visibility.
- **`crate::sim::walker::ROUTES_ASKED`** — a thread-local count of far routes. Assert it stays
  far below the tick count for any goal that can fail (see
  `a_goal_that_keeps_failing_does_not_ask_for_a_route_every_tick`,
  `a_fridge_nobody_can_reach_is_given_up_on_rather_than_retried_every_tick`).

Every behaviour should have at least:

- a happy-path test through `World`;
- a "the thing it needs is missing" test (it goes back to wandering, with the goal on cooldown);
- a "the thing it needs is unreachable" test with `ROUTES_ASKED`;
- if it competes with another goal, a handover test.

## Debugging a misbehaving unit

Select the unit in-game and open **brains**. It shows, live: the goal on top, the ranked
priorities, `held off` cooldowns, the current task, its result, the action, and every routine's
and the top executor's `debug_fields`.

| Symptom | Likely cause |
| --- | --- |
| Stands still, goal `idle`, `held off wander …t` | wander was `Blocked`; its target cells are unreachable |
| Flips between two goals every few ticks | routine lacks hysteresis, or a goal returns `Blocked` and is retried the moment its cooldown ends |
| Walks halfway, turns round | the routine released because the goal's own progress changed the stat; add hysteresis |
| `result` flickers `in progress`, no task | `process` returns `Working` with nothing queued, every tick |
| Frame rate drops only on some maps | a failure path returns `Working` instead of `Blocked`, re-routing every tick; check `ROUTES_ASKED` |
| An effect happens twice or never | it is applied in the goal instead of the task, or lost on handover (use `deprioritized`) |
