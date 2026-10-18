---
name: jobs
description: Jobs in crowd2x - who works where and when. The Job model (kind, own workplace, shift), the office worker (a computer, 09:00-17:00 Mon-Fri), weekdays on the clock (day 1 is a Monday), how a desk is handed out at spawn like a bed (Workplaces, give_a_job) and taken back, Command::Hire / Command::Dismiss, WorkRoutine and its 1.5 priority against the needs, WorkGoal and the Work task (an hour a stretch), Event::Worked fatigue (tiring, no fun), saving a job and the desks, the job menu in the unit panel, and the tests and QA. Maps every concept to the file and function that owns it. Use when adding a job kind, changing hours, schedules, weekends or what work does to a body, when somebody does not go to work or never leaves it, or when touching the job menu.
---

# Jobs

A job is **what a unit does for a living, where, and when**. The first and only one is the
**office worker**: their own computer, nine to five, Monday to Friday. It tires them and is no
fun. Plain Rust in `src/sim/`, like every other part of the simulation.

## The model (`src/sim/job.rs`)

| Concept | Where | What |
| --- | --- | --- |
| `JobKind` | `job.rs` | `OfficeWorker`: `name()`, `workplace()` (the `FeatureKind` its desk is), `shift()` |
| `Shift` | `job.rs` | `start`/`end` hour **on the same day**, `days` bitmask (bit 0 = Monday). `Shift::new` refuses anything else. `OFFICE_HOURS` = 9, 17, `WEEKDAYS` |
| `Shift::is_due(clock)` | `job.rs` | its hours on its days, **plus `COMMUTE` (half a world hour) before**, so a worker sets off at 08:30 |
| `Shift::is_working_hours`, `hours()`, `days()` | `job.rs` | the hours alone; `09:00-17:00`; `Mon-Fri` (or `Mon, Wed, Sat`) |
| `Job { kind, workplace, shift }` | `job.rs` | `Copy`. The workplace is **the unit's own desk**; the shift is copied in at hire time, so per-person hours are a data change. `Job::office(desk)` |
| `Workplaces` | `job.rs` | desk → worker, a `BTreeMap` twin of `Homes`: `claim` (only while free), `release(uid)`, `worker`, `is_taken`. Bookkeeping only — nothing in a tick reads it |
| weekday | `clock.rs` | `Weekday` (`short()`, `is_weekend()`), `Clock::weekday()` = `(day - 1) % 7`. **Day 1 is a Monday.** The HUD's `Display` is `day 1 Mon  08:00:00` |
| desk feature | `feature.rs` | `FeatureKind::Workstation` ↔ `"computer"` (beside `Entertainment`, as a fridge is Food and Water), `Access::Beside`. A computer with no power is neither |

## Handing jobs out (`src/sim/mod.rs`)

- **At spawn, after the bed** (`GameState::spawn` → `give_a_bed` → `give_a_job`): the nearest
  workstation that is not taken **and that the arrival may use** with the key its bed gave it
  (`Properties::may_use` — never a computer in somebody else's house). No free desk:
  unemployed. Logs `"{uid} was hired as an office worker at the computer at x, y"`.
- **In the district** the bank's computers (12+) are the desks; houses have none. So the first
  dozen-odd residents in spawner order get jobs, whatever their distance to the bank.
- `GameEntity::hire(job) -> bool` (default `false`: a dog is never hired), `dismiss()`, `job()`;
  `Human` forwards to `Brain::hire`/`dismiss`/`job`. **The job lives on the `Brain`**
  (`brain/mod.rs`, field `job`), handed to routines and goals as `RoutineCtx::job` /
  `GoalCtx::job` (`Option<&Job>`, read-only).
- `Command::Hire(uid)` → `GameState::hire` (the free desk nearest where it **stands**; logs why
  not: no such entity, already employed, no desk free — which is also what a dog gets).
  `Command::Dismiss(uid)` → `GameState::dismiss` (releases the desk, logs
  `"was let go from the computer at x, y"`). `Input::hire`/`dismiss`.
- `despawn` releases the desk too.

## The brain

| Layer | What | File |
| --- | --- | --- |
| routine | `WorkRoutine` ("go to work"): raises `GoalId::Work` to **`WORKING` = 1.5** while `job.shift.is_due(clock)`. Stateless decision (no stat, so no hysteresis); `on_shift` is kept for the brains menu and the save. **Last in `Brain::human()`** | `brain/routines.rs`, `routine.rs` |
| goal | `GoalId::Work` (💼), **the last `GoalId`** so every tie goes to the other goal. `WorkGoal`: walks beside **its own desk** and queues `Task::work(desk, WORK_SECONDS)` — an hour of world a stretch, `Achieved` after each, so the routine decides how many. No other desk to reroute to: a desk it cannot reach is `Blocked` (held off); an unpowered desk (`!features.has(Workstation, desk)`) is `Blocked` before any walk | `brain/goals/work.rs` |
| task | `Work { desk, seconds }`: from beside it, every tick checks distance, a body, and the desk's power. `Action::interact` (lights the screen through `interacting_with`). Finished → `Event::Worked { world_seconds }`; **interrupted → `Worked` for the part done** (like `Sleep`) | `brain/tasks/work.rs` |
| body | `Energy` takes `WORK_FATIGUE_PER_SECOND` on top of the waking drain: a working hour tires like `WORK_TIRES_LIKE` = **1.1** hours awake. `Fun` has no arm: work is no fun. `Satisfaction`, `Bladder` ignore it; `Experience::of` → `None` (not a treat) | `biology/energy.rs`, `mod.rs` |

### Where 1.5 sits

| vs | value | who wins |
| --- | --- | --- |
| hunger/thirst/boredom at commit (60) | 1.2 | work |
| bladder at commit (`BURSTING` 70) | 1.4 | work (they hold it to 75) |
| any need past 75 | > 1.5 | the need; work has the worker back afterwards |
| nap at `EXHAUSTED` (75) | 1.5 | **sleep** (tie → lower `GoalId`) |
| night floor / orders / wander | 0.6 / 0.5 / 0.1 | work (the shift never overlaps night) |

**Why 1.1 and not more**: a body up rested at 06:00 is 68.75 tired by 17:00 with no job at
all. 8 hours at 1.1 adds 5 → 73.75, just short of `EXHAUSTED`; at 1.25 a worker would hit 75
around 16:10 and the nap (which wins the tie) would walk them out of every afternoon shift.
`a_rested_worker_is_worn_out_but_not_exhausted_at_the_end_of_a_shift` pins this.

## Saving (`savegame` skill)

- `pb::Brain.job` (tag 14, `pb::Job`: `JobKind` enum, workplace cell, start, end, days).
  Absent = unemployed, which is what every older save reads as. `Job::save`/`restore`
  (refuses a broken shift or unknown kind).
- `pb::World.workplaces` (tag 11, `pb::Workplace { desk, worker }`), `Workplaces::save`/
  `restore` (one desk to one worker), restored after `homes` in `sim/saving.rs`, then
  **cross-checked** against the entities both ways (a worker's job names its desk; a desk's
  worker is here and works there).
- `WorkRoutine::on_shift` is saved as the routine's `committed`; `WorkGoal` is a
  `pb::Executor::Seek`; the task is `pb::TaskKind::Work(Timed)` (tag 11).
- The routine was appended, so `Brain::restore` accepts a saved routine list that is a
  **prefix** of the brain's (old saves, `format1.savegame`).

## The job menu (`src/game/unitpanel.rs`)

`job` on the unit bar opens `Menu::Job`: a live `Readout::Job` block from
`job_block(entity, clock)` — `job`, `desk`, `hours`, `days`, `now` (`working`, `on the way to
work`, `on a break: <goal>`, `off until 09:00`, `done for the day`, `weekend`, `day off`) — or
`unemployed` (a human) / `no job` (anything else). Humans get a button (`Action::ToggleJob`,
label `Readout::Employ`: `dismiss` / `hire`) that sends `Input::dismiss` / `Input::hire`.

## Tests

- `job.rs`: office hours, the commute, refused shifts, day lists, save round trips, desks.
- `clock.rs`: day 1 Monday, the weekend, the HUD string.
- `biology/energy.rs`, `fun.rs`: an hour at work vs awake; the shift tuning; no fun.
- `brain/tasks/work.rs` (`Rig`): distance, tiring only once over, part credit when cut short,
  walking away, power, never on the desk + screen lit.
- `brain/goals/work.rs` (`World::step_in_time` moves the clock; `World::works()` counts
  `"worked an hour at the computer"`): sets off at 08:30 and works; stops at 17:00 with the
  part-hour still tiring; nobody works at the weekend; no job, no work; dead desk held off;
  unreachable desk (`ROUTES_ASKED`); someone beside the desk; bursting waits, 85 goes and
  comes back.
- `brain/routines.rs`: on shift only with a job; ordering against every need; exhausted ties
  go to sleep.
- `sim/mod.rs`: hired at the nearest free desk until none; a dog is not; leaving frees the
  desk; hire/dismiss commands; district desks all in the bank; the parallel determinism test
  asserts work happened.
- `save::tests::a_district_saved_in_the_middle_of_a_working_day_carries_on_exactly`, and
  `brain::tests::a_brain_saved_before_a_routine_was_added_on_the_end_still_loads`.
- QA `qa/office_job.json`: hired on arrival, the menu, at the desk by 09:10 with the screen
  on, an hour worked, dismissed (screen dark), hired again. `qa/computer.json` and
  `qa/lighting.json` timed a lone unit's go on the computer to the tick and broke the moment
  that unit was hired to it: deleted rather than patched. In
  `qa/a_day_in_the_office.json` two of the six are hired and its observation's goals column
  shows `work 2` through the day (`expect_log` reads only the log panel's last lines, so it
  cannot assert a morning's work at the end of a day).

Each guard above was seen failing under a mutation (routine unregistered, strict restore,
work not tiring, job not saved, priority at 1.0, no part credit, desks given twice, a
seven-day week, no power check).

## Adding a job kind

1. A `JobKind` variant (+ `pb::JobKind`, both `From`s), its `name`, `workplace()` feature and
   `shift()`. A new workplace is a new `FeatureKind` + `FEATURES` entry (`brain-engineer`).
2. `give_a_job` hands out only `OfficeWorker` today: decide who gets which kind.
3. If the work is not "stand beside a prop for an hour", a goal/task of its own; what it does
   to the body is an `Event` the processes decide on.
4. The log line `"hired as an ..."` uses "an" — fix the article for a consonant.
