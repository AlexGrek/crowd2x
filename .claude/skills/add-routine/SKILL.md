---
name: add-routine
description: Step-by-step recipe for adding a new routine (a priority arranger such as a need, a time-of-day rule or a constant pressure) to crowd2x unit brains, with the exact files to edit, which existing routine to copy, what to create and what NOT to read. Use when asked to add or change a routine, a need's priority, thresholds, hysteresis or commit/release levels, or to make a unit want something at a certain time or state. For new stats, processes, goals, tasks, features or items use the brain-engineer skill alongside this one.
---

# Adding a routine

A **routine** decides *how badly each goal is wanted* and nothing else. Every tick the brain
zeroes the priority list and each routine raises the goals it cares about (`Goals::raise_to`
is a max, so routines cannot undo each other and their order does not change the result). The
goal on top wins. Routines live entirely in `src/sim/brain/` and are plain Rust: no `bevy::`.

Read only the files in the map below. The rest of the repository is irrelevant to this task.

## 1. Decide what you are adding

| The routine reads... | Add | New file? |
| --- | --- | --- |
| one stat, and commits/releases on it | a `Need` constant, registered with `Routine::need(..)` | no |
| a stat **and** something else (clock, surroundings, memory, another stat) | a new routine struct | no |
| nothing; a constant pressure | a new routine struct, no state | no |
| how a want is *pursued* (route, tasks, which bed/fridge) | **not a routine** — a goal executor, use `brain-engineer` | - |

A routine itself never needs a new module: routine types all live in `routines.rs`. New
files come only from what the routine needs *around* it:

| Needs | Create | Skill |
| --- | --- | --- |
| a new goal to raise | `src/sim/brain/goals/<goal>.rs` | `brain-engineer` (goal recipe) |
| a new stat moved by a process | `src/sim/biology/<process>.rs` | `brain-engineer` (stat/process recipe) |
| a new task or feature | `tasks/<task>.rs`, entry in `sim/feature.rs` | `brain-engineer` |
| a scripted test | `qa/<name>.json` | `qa` |

## 2. The contract (each line was a bug or would be)

- **Priorities only.** A routine never queues a task, moves, searches or writes a stat.
- **Same scale as every need:** `level / NEED_PER_PRIORITY` (50), so 0-100 maps to 0-2, and
  wandering is `BUSY` (0.1). Urgency is only comparable across needs on one scale. A floor or
  special case must be ordered against the committed needs (see the priority table in
  `.claude/skills/brain-engineer/SKILL.md`) and a test must assert that order.
- **Hysteresis.** Commit at a high level, release at a lower one, kept in a `committed` field
  on the routine. Without it a stat hovering at one threshold makes a unit turn round halfway
  to the goal. `&mut self` in `arrange` exists for this.
- **Off means off.** If the routine watches a stat driven by a process, it wants nothing and
  drops `committed` while `Biology::is_running(process)` is false. No body (`biology` is
  `None`, e.g. a dog) means raise nothing and reset.
- **No allocation, no search, no scan of the crowd** in `arrange`; state is inline. `Vec` only
  in `debug_fields`. Randomness only from `crate::sim::rng::tick_rng`. No `HashMap` iteration.
- **Two clocks:** durations a viewer watches are watched seconds; anything about the world's
  hours is world time (`sim/clock.rs`). Times of day come from `Think::clock`, never a counter.
- **Compare with plain `>`/`>=`** so a NaN never wins.

## 3. Full file map

### Always edit (a new routine type)

| File | Change |
| --- | --- |
| `src/sim/brain/routines.rs` | threshold constants (with doc on why each value); the routine struct; its `RoutineExecutor` impl (`name`, `arrange`, `debug_fields`); unit tests in the `tests` module at the bottom |
| `src/sim/brain/routine.rs` | `use` line; a variant on the `Routine` enum; a `const fn` constructor; a new arm in each of `name`, `arrange`, `debug_fields`; the module doc if the rule about "a new need is not a new type" no longer covers it |
| `src/sim/brain/mod.rs` | register it in `Brain::human()` (and `Brain::dog()` only if a dog should have it); the `use routines::{..}` line if constants are imported; the "Layers" table in the module doc; the test `a_human_s_brain_says_what_it_is_doing`, which pins the routine names **in registration order** |

For a plain `Need`: only `routines.rs` (constant + tests) and `brain/mod.rs` (register).

### Edit only if it applies

| Situation | Files |
| --- | --- |
| Raises a **new** goal | `src/sim/brain/goal.rs` (`GoalId` variant, `COUNT`, `ALL`, `name`, `emoji`, and the `the_ranked_list_...` test), `goals/<goal>.rs`, `goals/mod.rs`, executor list in `Brain::human()`; then `cargo build` — exhaustive `match`es on `GoalId`/`Task` name any others |
| Needs data no routine sees today | the value on `Think` (`src/sim/entity.rs`) **or** on `RoutineCtx` (`routine.rs`). Every construction site must be updated: `Think {` in `src/sim/mod.rs` (two, in `process_pass`), `src/sim/testing.rs` (`World::ctx`), `src/sim/walker.rs` (test rig), `src/sim/brain/routines.rs` (two test helpers); `RoutineCtx {` in `src/sim/brain/mod.rs` (`Brain::react`) and the two test helpers in `routines.rs` |
| Needs a helper on time | `src/sim/clock.rs` (a `Clock` method + its tests) |
| Needs something the unit remembers | `src/sim/brain/memory.rs` (a key constant), a setter on `Brain`, and the spawn pass hook in `src/sim/mod.rs` if it is handed out at spawn |
| Watches a new stat | `src/sim/biology/stats.rs` (getter, derived inverse if it drains, `Stats::random` — every need is born 70-100% satisfied), plus `brain-engineer` for the process |
| Needs a process switch name in scripts | nothing in `src/qa/mod.rs` (names resolve from `ProcessId::ALL`); update the doc on `Step::Process` in `src/qa/script.rs` |

### Test support (edit when the routine adds a stat or competes with other needs)

| File | Why |
| --- | --- |
| `src/sim/testing.rs` | `needy_human` pins every need except the one under test so unrelated tests are not interrupted — pin the new stat too; `World` counters (`meals`, `plays`, `sleeps`, ...) count log lines; `World.clock` chooses the time of day |
| `src/sim/kinds.rs` | `#[cfg(test)] Human::set_<stat>` hooks, and the two test fixtures that pin needs to satisfied (search for `set_fun(100.0)`) |
| `src/sim/mod.rs` | the `determinism_survives_...` tests: extend one so the new routine's goal really runs in the parallel rounds, and assert something happened |
| `src/sim/brain/mod.rs` | `dial(goal, level)` (a test-only routine whose priority a test sets) for handover tests that need no real routine |

### Docs and QA (always, when the routine is visible)

`CLAUDE.md` ("The brain" > routines bullet, and the biology/time sections if touched),
`.claude/skills/brain-engineer/SKILL.md` (priority table row), `qa/<name>.json`, and any
existing `qa/*.json` that watches one need in isolation — it must switch the new process off.

### Read-only context (read only the named part)

| File | Part |
| --- | --- |
| `src/sim/brain/goal.rs` | `Goals::raise_to`, `GoalId` |
| `src/sim/entity.rs` | the `Think` struct, and `Body` if position matters |
| `src/sim/biology/mod.rs` | `Biology::stats`, `Biology::is_running`, `ProcessId` |
| `src/sim/biology/stats.rs` | the getters |
| `src/sim/rng.rs` | `tick_rng`, only if the routine is random |

### Do not read

`src/editor/`, `src/ui/`, `src/game/`, `src/map/` (except the `Point` type), `src/render.rs`,
`src/browser.rs`, `src/menu.rs`, `src/debug.rs`, `src/animation.rs`, `src/characters/`,
`src/qa/mod.rs`, `src/sim/path.rs`, `src/sim/walker.rs` (except its `Think {` site),
`src/sim/entities.rs`, `src/sim/occupancy.rs`, `tools/`, `assets/`, and the other skills
except `brain-engineer` and `qa`. None of them change when a routine is added.

## 4. Pick the file to copy from

Copy the shape, then change the numbers. Do not paste this skill's prose into code.

| Your routine is... | Copy | Where |
| --- | --- | --- |
| a plain stat need | the `Need` constants and `NeedRoutine` | `src/sim/brain/routines.rs` (top half) |
| a stat plus the time of day | `SleepRoutine` and its tests | `src/sim/brain/routines.rs` |
| a constant pressure | `StayBusyRoutine` | `src/sim/brain/routines.rs` |
| the routine layer's own recipe and checklist | the section "A routine (a need)" in | `.claude/skills/brain-engineer/recipes.md` |
| a whole need end to end (stat, process, goal, task, routine, QA) | "Worked example: using the toilet"; "Worked example: a go on the computer" if a prop should show it | `.claude/skills/brain-engineer/recipes.md` |
| tests for a need with hysteresis | `hovering_at_the_threshold` tests | `src/sim/brain/routines.rs` (`tests`) |
| a QA test isolating one need | `qa/computer.json`; for a night, `qa/sleeping.json` | `qa/` |

## 5. Workflow

1. Classify it with section 1. If it needs a stat, process, goal, task, feature or item, do
   that layer first (`brain-engineer`), in dependency order.
2. Write the thresholds as named, documented constants. Choose commit, release and the
   priority curve so that the ordering against every committed need is what you intend.
3. Write the routine, its `Routine` variant/constructor/arms, and register it.
4. Update the pinned routine-names test in `brain/mod.rs` and the pinning helpers in
   `testing.rs`/`kinds.rs` (section 3). `cargo build` then `cargo test`.
5. Write the tests (section 6), extend a determinism test, add or adjust QA.
6. Update the docs listed in section 3.

## 6. How to test

**Unit tests** live in the `tests` module of `routines.rs` and drive `arrange` directly through
its helpers (`priority`, `priority_in`, and the clock helpers `day`/`night`, which build a
`Think` and `RoutineCtx` by hand). Every routine needs, at minimum:

- nothing wanted below the commit level; wanted above `BUSY` at it;
- hysteresis: dipping under commit does not release; dropping to release does;
- off means off: switch the process off with `Biology::set_running` and it wants nothing and
  forgets its commitment; the other needs are unaffected;
- no body: nothing raised;
- one test per extra input (clock, memory, surroundings) showing the same stat decides
  differently;
- the ordering against the needs it competes with, as a comparison of priorities.

**Brain-level tests** run one unit through the real sequence with `testing::World` and
`needy_human`, with other processes off via `Biology::set_running` so only the routine under
test can act. A tick at `World::dt` is 1/64 s; the sim tests' `run()` uses 1/60. An hour of
world is 1920 ticks at 64 Hz (1800 at 60 Hz). Everyone is born 70-100% satisfied, so a test
that waits for a need must wait **hours** of world, or set the stat directly.

**Prove each guard test can fail.** Break what it guards (drop the branch, invert the
comparison, unregister the routine), run it, watch it fail, restore from a backup taken first.
Do the restore in a separate command from the run so a crash cannot skip it. Tests in this
repo have passed vacuously before.

**Commands** (run through `cargo`, never the raw binary):

```
cargo test routines            # the routine unit tests
cargo test                     # everything; the determinism tests are the gate
uv run tools/qa.py qa/<name>.json
uv run tools/qa.py             # whole QA suite; other needs' tests may need the new process off
uv run tools/qa.py --release qa/perf_simulation.json   # a routine runs per unit per tick
```

A new QA test must be checked failing before it is trusted (unregister the routine, expect
the assertion to fail, restore). The `qa` skill documents the steps and fixtures.

## 7. Done when

- `cargo test` passes and each new guard test was seen failing once.
- The routine appears in the brains menu (`routines` list, and its `debug_fields` line).
- Existing QA tests still pass; those that isolate one need switch the new process off.
- The perf run's per-entity cost has not grown with the crowd (`expect_scaling`).
- `CLAUDE.md` and the `brain-engineer` priority table describe the new routine.
