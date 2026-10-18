---
name: jobs
description: Jobs in crowd2x - who works where and when. The Job model (kind, own workplace, shift), the office worker (a computer) and the scrap collector (a scrap bin in the warehouse, scrap from the heaps in the wasteland), both 09:00-17:00 Mon-Fri, weekdays on the clock (day 1 is a Monday), how a workplace is handed out at spawn like a bed in hiring order - desks first, then bins (Workplaces, give_a_job, JobKind::ALL) - and taken back, Command::Hire / Command::Dismiss, WorkRoutine and its 1.5 priority against the needs, WorkGoal and the Work task (an hour a stretch), CollectGoal, salvaging for work and the Deposit task, the Bins store, Event::Worked fatigue (tiring, no fun), saving a job, the desks and the bins, the job menu in the unit panel, and the tests and QA. Maps every concept to the file and function that owns it. Use when adding a job kind, changing hours, schedules, weekends or what work does to a body, when somebody does not go to work, never leaves it or brings no scrap in, or when touching the job menu.
---

# Jobs

A job is **what a unit does for a living, where, and when**. Two so far, both nine to five,
Monday to Friday, both tiring and no fun:

- the **office worker**: their own computer, worked an hour at a time;
- the **scrap collector**: their own scrap bin in the district's warehouse — out to the heaps
  of scrap in the wasteland, salvage a packful, back to the bin and store it.

Plain Rust in `src/sim/`, like every other part of the simulation.

## The model (`src/sim/job.rs`)

| Concept | Where | What |
| --- | --- | --- |
| `JobKind` | `job.rs` | `OfficeWorker`, `ScrapCollector`: `name()`, `with_article()` (for log lines), `workplace()` (the `FeatureKind` its workplace is), `workplace_name()` (`computer`, `scrap bin`), `goal()` (`GoalId::Work`, `GoalId::Collect` — what `WorkRoutine` raises), `shift()` (both `OFFICE_HOURS`) |
| `JobKind::ALL` | `job.rs` | every kind **in hiring order**: office first. `give_a_job` and `Command::Hire` walk it |
| `Shift` | `job.rs` | `start`/`end` hour **on the same day**, `days` bitmask (bit 0 = Monday). `Shift::new` refuses anything else. `OFFICE_HOURS` = 9, 17, `WEEKDAYS` |
| `Shift::is_due(clock)` | `job.rs` | its hours on its days, **plus `COMMUTE` (half a world hour) before**, so a worker sets off at 08:30 |
| `Shift::is_working_hours`, `hours()`, `days()` | `job.rs` | the hours alone; `09:00-17:00`; `Mon-Fri` (or `Mon, Wed, Sat`) |
| `Job { kind, workplace, shift }` | `job.rs` | `Copy`. The workplace is **the unit's own** desk or bin; the shift is copied in at hire time, so per-person hours are a data change. `Job::new(kind, at)`, `Job::office(desk)`, `Job::collector(bin)` |
| `Workplaces` | `job.rs` | workplace → worker, desks and bins alike, a `BTreeMap` twin of `Homes`: `claim` (only while free), `release(uid)`, `worker`, `is_taken`. Bookkeeping only — nothing in a tick reads it |
| weekday | `clock.rs` | `Weekday` (`short()`, `is_weekend()`), `Clock::weekday()` = `(day - 1) % 7`. **Day 1 is a Monday.** The HUD's `Display` is `day 1 Mon  08:00:00` |
| desk feature | `feature.rs` | `FeatureKind::Workstation` ↔ `"computer"` (beside `Entertainment`, as a fridge is Food and Water), `Access::Beside`. A computer with no power is neither |
| bin feature | `feature.rs` | `FeatureKind::ScrapBin` ↔ `"scrap bin"`, `Access::Beside`, **needs no power** |
| what bins hold | `bins.rs` | `Bins { cells, stored }`, built from the map's `"scrap bin"` props like `Fridges` (sorted, binary search): `stored(cell)`, `total()`, `add(cell, n)`, `iter()`, `len()`. A `GameState` field, `GameState::bins()`. A bin never fills, and nothing takes scrap out yet |

## Handing jobs out (`src/sim/mod.rs`)

- **At spawn, after the bed** (`GameState::spawn` → `give_a_bed` → `give_a_job`): for each kind
  in `JobKind::ALL`, the nearest workplace of its feature that is not taken **and that the
  arrival may use** with the key its bed gave it (`Properties::may_use` — never a computer in
  somebody else's house); **the first kind with one wins**. So every desk is given before the
  first bin, whatever the distances; one collector to a bin; then unemployed. Logs
  `"{uid} was hired as an office worker at the computer at x, y"` /
  `"... as a scrap collector at the scrap bin at x, y"`.
- **In the district** the bank's computers (12+) are the desks and the warehouse's
  `WAREHOUSE_BINS` (4) are the bins; houses have neither. So the first dozen-odd residents in
  spawner order sit at desks and the next four collect scrap, wherever they live.
- `GameEntity::hire(job) -> bool` (default `false`: a dog is never hired), `dismiss()`, `job()`;
  `Human` forwards to `Brain::hire`/`dismiss`/`job`. **The job lives on the `Brain`**
  (`brain/mod.rs`, field `job`), handed to routines and goals as `RoutineCtx::job` /
  `GoalCtx::job` (`Option<&Job>`, read-only).
- `Command::Hire(uid)` → `GameState::hire` (the same order, nearest where it **stands**; logs
  why not: no such entity, already employed, no workplace free — which is also what a dog
  gets). `Command::Dismiss(uid)` → `GameState::dismiss` (releases the workplace, logs
  `"was let go from the computer at x, y"` / `"... from the scrap bin at x, y"`).
  `Input::hire`/`dismiss`.
- `despawn` releases the workplace too.

## The brain

| Layer | What | File |
| --- | --- | --- |
| routine | `WorkRoutine` ("go to work"): raises **`job.kind.goal()`** to **`WORKING` = 1.5** while `job.shift.is_due(clock)` — `Work` at a desk, `Collect` for a collector. Stateless decision (no stat, so no hysteresis); `on_shift` is kept for the brains menu and the save. **Last in `Brain::human()`** | `brain/routines.rs`, `routine.rs` |
| goal | `GoalId::Work` (💼) and `GoalId::Collect` (🧲), **the last two `GoalId`s** so every tie goes to the other goal | `brain/goal.rs` |
| office | `WorkGoal`: walks beside **its own desk** and queues `Task::work(desk, WORK_SECONDS)` — an hour of world a stretch, `Achieved` after each, so the routine decides how many. No other desk to reroute to: a desk it cannot reach is `Blocked` (held off); an unpowered desk (`!features.has(Workstation, desk)`) is `Blocked` before any walk. `prioritized` forgets a remembered desk that is no longer the job's | `brain/goals/work.rs` |
| office task | `Work { desk, seconds }`: from beside it, every tick checks distance, a body, and the desk's power. `Action::interact` (lights the screen through `interacting_with`). Finished → `Event::Worked { world_seconds }`; **interrupted → `Worked` for the part done** (like `Sleep`) | `brain/tasks/work.rs` |
| collecting | `CollectGoal` (state: one `Seek`): **one trip is its unit of work**, `Achieved` when a packful is in the bin, and the routine starts the next. It plans **from what is true now** and never counts: room in the pack → the heap it was working while it has anything left, else `nearest_heap`, then `move_beside` + `Task::salvage_for_work`; pack full, or no heap left and something carried → its own bin (`job.workplace`, which just has to be in `features`), `move_beside` + `Task::deposit`; no heap and nothing carried → `Blocked` with `"found no scrap left to collect"` (the hold's doubling spaces those lines). A failed walk is `Seek::setback`: elsewhere for a heap is the next nearest, a bin has none. A failed salvage (the heap emptied under it) is a cheap replan. `prioritized` forgets a target no longer worth it — an emptied heap, or a bin that is not the job's — though `process` re-checks both anyway (mutating the forget out is caught by nothing) | `brain/goals/collect.rs` |
| nearest heap | `collect::nearest_heap`: `Map::heaps(map.navigation_area(), …)` — the site and `NAV_MARGIN` (64) round it, which a `GameState` always has loaded — keeping heaps with a side the unit may stand on that `navigation().may_reach` says it can get to and its memory does not hold out of reach; min by (manhattan, cell). No route, no allocation, once per trip decision | `brain/goals/collect.rs` |
| salvage for work | `Salvage { heap, seconds, for_work }`: `Task::salvage` (an order, `SalvageGoal`) or `Task::salvage_for_work` (the job). `for_work` reports `Event::Worked` for the piece on finishing **and for the part done when interrupted**; an order does neither | `brain/tasks/salvage.rs` |
| storing | `Deposit { bin, seconds }`, `DEPOSIT_SECONDS` = 5 world minutes: every tick checks within one step, the bin in `features`, scrap stowed. Finished → takes every piece out of the pack (`Inventory::take_out`), writes `Effect::Deposit { at, pieces }`, reports `Event::Worked`; interrupted → `Worked` for the part done, nothing stored | `brain/tasks/deposit.rs` |
| the world | `Effect::Deposit` is applied in `world_step` beside `Effect::Salvage` (`sim::deposit`): `Bins::add`, and the line `"{uid} stored N scrap metal in the bin at x, y: M stored"`. `testing::World` has `bins` and applies it too (`World::deposits()` counts the lines) | `sim/mod.rs`, `sim/testing.rs` |
| body | `Energy` takes `WORK_FATIGUE_PER_SECOND` on top of the waking drain: a working hour tires like `WORK_TIRES_LIKE` = **1.1** hours awake. `Fun` has no arm: work is no fun. `Satisfaction`, `Bladder` ignore it; `Experience::of` → `None` (not a treat). Walking out and back is not work; salvaging and storing are | `biology/energy.rs`, `mod.rs` |

**Overnight**: at 17:00 the routine lets go wherever the collector is; it carries what it has
into the next shift and delivers it then (the plan starts from a pack with scrap in it).

**Heaps do not grow back** (`map::wasteland`; a salvage tally is forever). A district's yards
empty over the days, nearest first, and its collectors are then held off — the current limit.

**How long a trip takes**: a walker covers about 60 cells a world hour, a piece is 20 world
minutes (`SALVAGE_SECONDS`), a packful is 3 pieces. The warehouse stands on the edge lot
nearest the scrap (`map-generator`), 21-33 cells from a heap for 63 of 64 seeds, so a trip
from the warehouse is under two hours; the first trip of the day starts from home, wherever
that is, and a need that takes a collector home from the yards is a long walk through a
district that jams round the bank. **With needs on, a district's collectors store nothing**:
by Monday afternoon the 136-resident district has piled up in the bank and nobody works
(office workers included) from Tuesday on — the crowd limit in `map-generator`'s traps. The
district tests that assert scrap is stored switch the collectors' needs off; `qa/scrap_collector.json`
(one collector, a small yard, needs off) stores two packfuls by 15:30.

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
`a_rested_worker_is_worn_out_but_not_exhausted_at_the_end_of_a_shift` pins this. A collector
is tired only while salvaging and storing, so less than an office worker over a shift.

## Saving (`savegame` skill)

- `pb::Brain.job` (tag 14, `pb::Job`: `JobKind` enum — `OfficeWorker = 0`,
  `ScrapCollector = 1` — workplace cell, start, end, days). Absent = unemployed, which is what
  every older save reads as. `Job::save`/`restore` (refuses a broken shift or unknown kind).
- `pb::World.workplaces` (tag 11, `pb::Workplace { desk, worker }` — a bin is a "desk" there),
  `Workplaces::save`/`restore` (one workplace to one worker), restored after `homes` in
  `sim/saving.rs`, then **cross-checked** against the entities both ways.
- `pb::World.bins` (tag 12, `pb::Bin { cell, stored }`), `Bins::save`/`restore`, restored after
  the fridges; refuses a cell with no bin or a different number of bins. Empty in an older
  save, which is a map with none.
- `WorkRoutine::on_shift` is saved as the routine's `committed`; `WorkGoal` and `CollectGoal`
  are a `pb::Executor::Seek`; the tasks are `pb::TaskKind::Work(Timed)` (tag 11),
  `Deposit(Timed)` (tag 12) and `SalvageForWork(Timed)` (tag 13) — an order's salvage stays
  `Salvage(Timed)` (tag 10). `GoalId::Collect` = 9.
- The routine was appended, so `Brain::restore` accepts a saved routine list that is a
  **prefix** of the brain's (old saves, `format1.savegame`); an executor a save does not
  mention (`CollectGoal` in an older one) starts fresh.

## The job menu (`src/game/unitpanel.rs`)

`job` on the unit bar opens `Menu::Job`: a live `Readout::Job` block from
`job_block(entity, clock)` — `job`, `at` (`computer at 12, 4`, `scrap bin at 30, 2`), `hours`,
`days`, `now` (`working`, `on the way to work`, `collecting scrap`, `storing scrap` while at
the bin, `on a break: <goal>`, `off until 09:00`, `done for the day`, `weekend`, `day off`) —
or `unemployed` (a human) / `no job` (anything else). Humans get a button
(`Action::ToggleJob`, label `Readout::Employ`: `dismiss` / `hire`) that sends `Input::dismiss`
/ `Input::hire`. The HUD's map stats gain `scrap  N stored` on a map with bins, and a bin on
screen shows how full it is (`game-screen`).

## Tests

- `job.rs`: office hours, the commute, refused shifts, day lists, save round trips, desks, the
  collector kind and `ALL`'s order.
- `bins.rs`: every bin starts empty, sorted; add and total; save round trip and refusals.
- `feature.rs`: a bin is a `ScrapBin`, beside, with no power.
- `clock.rs`: day 1 Monday, the weekend, the HUD string.
- `biology/energy.rs`, `fun.rs`: an hour at work vs awake; the shift tuning; no fun.
- `brain/tasks/work.rs` (`Rig`): distance, tiring only once over, part credit when cut short,
  walking away, power, never on the desk + screen lit.
- `brain/tasks/deposit.rs` (`Rig`): from across the room fails; the pack emptied only once it
  is over and the effect asked with the count, tiring and no fun; nothing to store fails; a bin
  that is gone fails with the pack still full; walking away keeps the scrap.
- `brain/tasks/salvage.rs`: salvaging for a living tires (and for the part done when cut
  short); an order does neither.
- `brain/goals/work.rs` (`World::step_in_time` moves the clock; `World::works()` counts
  `"worked an hour at the computer"`): sets off at 08:30 and works; stops at 17:00 with the
  part-hour still tiring; nobody works at the weekend; no job, no work; dead desk held off;
  unreachable desk (`ROUTES_ASKED`); someone beside the desk; bursting waits, 85 goes and
  comes back; let go on the way to one desk and hired at another, it goes to the new one.
- `brain/goals/collect.rs` (the 9x7 seed-0 yard of `qa/scrap.json` with a bin, the navigation
  area loaded; `World::deposits()`): sets off at 08:30, salvages three and stores them; a trip
  tires more than the hours awake; no scrap anywhere is held off; walled in with its bin, the
  heaps are skipped (`ROUTES_ASKED`); a heap remembered out of reach is passed over; a full
  pack goes to the bin without a heap; nobody collects at the weekend, without the job or as an
  office worker; let go and hired at another bin, it stores in the new one.
- `brain/routines.rs`: on shift only with a job; a collector's shift is `Collect`, never
  `Work`; ordering against every need; exhausted ties go to sleep.
- `sim/mod.rs`: hired at the nearest free desk until none; a desk before a bin however near the
  bins, one collector to a bin, `Hire` in the same order; a dog is not; leaving frees the
  desk; hire/dismiss commands; a district gives every desk in the bank, then its four bins in
  the warehouse, in arrival order; a district Monday stores scrap in the warehouse (the
  collectors' needs off); the parallel determinism test asserts work happened, and
  `determinism_survives_a_crowd_of_scrap_collectors_in_the_parallel_rounds` runs 70 collectors
  and compares what every bin holds.
- `save::tests::a_district_saved_in_the_middle_of_a_working_day_carries_on_exactly`,
  `a_district_saved_with_scrap_in_its_bins_and_its_collectors_out_carries_on_exactly`, and
  `brain::tests::a_brain_saved_before_a_routine_was_added_on_the_end_still_loads`.
- QA `qa/office_job.json`: hired on arrival, the menu, at the desk by 09:10 with the screen
  on, an hour worked, dismissed (screen dark), hired again. `qa/scrap_collector.json`:
  `qa/scrap.json`'s yard with a bin and no computer — hired as a collector, the menu,
  `collecting scrap` an hour after the start, and `expect_stored {"min": 3}` by the afternoon
  (an outcome, not a tick); shots of the bin empty and filling. Seen failing with
  `CollectGoal` unregistered. `qa/computer.json` and `qa/lighting.json` timed a lone unit's go
  on the computer to the tick and broke the moment that unit was hired to it: deleted rather
  than patched. In `qa/a_day_in_the_office.json` two of the six are hired and its
  observation's goals column shows `work 2` through the day.

Each guard above was seen failing under a mutation (routine unregistered, strict restore,
work not tiring, job not saved, priority at 1.0, no part credit, desks given twice, a
seven-day week, no power check; and for collecting: the routine raising `Work` for a
collector, the deposit not emptying the pack, bins offered before desks, salvage for work not
tiring, bins not restored).

## Adding a job kind

1. A `JobKind` variant (+ `pb::JobKind`, both `From`s), its `name`, `with_article`,
   `workplace()` feature, `workplace_name`, `goal()` and `shift()`, and its place in
   `JobKind::ALL` (hiring order). A new workplace is a new `FeatureKind` + `FEATURES` entry
   (`brain-engineer`); in the district, a building with that many of them (`map-generator`,
   mandatory lot buildings).
2. If the work is not "stand beside a prop for an hour", a goal of its own (a `GoalId`
   appended, its executor registered last in `Brain::human()`) and tasks; what it does to the
   body is an `Event` the processes decide on.
3. The job menu's `now` line (`job_block`) has an arm per kind.
