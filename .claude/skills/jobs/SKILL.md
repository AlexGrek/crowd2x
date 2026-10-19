---
name: jobs
description: Jobs in crowd2x - who works where and when. The Job model (kind, own workplace, shift), the office worker (a computer) and the scrap collector (a scrap bin in the warehouse, scrap from the heaps in the wasteland), both 09:00-17:00 Mon-Fri, and the guard (a guard post by a gate in the district's wall, three 8-hour watches every day, a guard to each), the undertaker (a crematorium, the guards' three watches, carrying the dead to it: BuryGoal, TakeBody, Cremate), the steel factory's steelworker (a furnace: scrap from a hopper smelted into steel on a pallet, SmeltGoal, Withdraw, Smelt) and courier (a scrap hopper: scrap from the warehouse's bins to the hopper and steel from the pallets to the warehouse's rack, HaulGoal), the stores (Bins: scrap bins, steel racks, scrap hoppers, steel pallets; Effect::Deposit / Effect::Withdraw), weekdays on the clock (day 1 is a Monday), commutes, sleeping by one's shift (Shift::is_bedtime), how a workplace - a cell and a shift - is handed out at spawn like a bed in hiring order - desks first, then bins, then posts (Workplaces, give_a_job, JobKind::ALL) - and taken back, Command::Hire / Command::Dismiss, WorkRoutine and its 1.5 priority against the needs, WorkGoal and the Work task (an hour a stretch), CollectGoal, salvaging for work and the Deposit task, the Bins store, Event::Worked fatigue (tiring, no fun), saving a job, the desks and the bins, the job menu in the unit panel, and the tests and QA. Maps every concept to the file and function that owns it. Use when adding a job kind, changing hours, schedules, weekends or what work does to a body, when somebody does not go to work, never leaves it or brings no scrap in, or when touching the job menu.
---

# Jobs

A job is **what a unit does for a living, where, and when**. Six so far, all tiring and no
fun:

- the **office worker**: their own computer, worked an hour at a time, nine to five, Monday to
  Friday;
- the **scrap collector**: their own scrap bin in the district's warehouse — out to the heaps
  of scrap in the wasteland, salvage a packful, back to the bin and store it; office hours;
- the **guard**: their own watch at a guard post set into the district's perimeter wall beside
  a gate — stood at an hour at a time like a desk — on one of the three `WATCHES` (08-16,
  16-24, 00-08) **every day of the week**. Three guards to a post keep it round the clock; a
  district's 8 gates take 24 guards;
- the **undertaker**: their own watch (the guards' `WATCHES`, every day) at a `"crematorium"`
  in the district's crematorium building — to the nearest of the dead, take the body up, carry
  it back and cremate it. The building's `CREMATORIA` (2) take 6 undertakers, hired **after**
  the guards (`JobKind::ALL`). See "The undertaker" below;
- the **steelworker**: their own furnace in the district's steel factory, office hours — scrap
  metal out of the factory's hoppers, smelted into steel at the furnace, the steel onto the
  factory's pallets;
- the **courier**: their own scrap hopper in the steel factory, office hours — scrap metal from
  the warehouse's bins to the hopper, and steel from the factory's pallets to the warehouse's
  steel rack. See "The steel works" below.

Plain Rust in `src/sim/`, like every other part of the simulation.

## The model (`src/sim/job.rs`)

| Concept | Where | What |
| --- | --- | --- |
| `JobKind` | `job.rs` | `OfficeWorker`, `ScrapCollector`, `Guard`: `name()`, `with_article()` (for log lines), `workplace()` (the `FeatureKind` its workplace is), `workplace_name()` (`computer`, `scrap bin`, `guard post`), `goal()` (`GoalId::Work` for a desk **and a post**, `GoalId::Collect` — what `WorkRoutine` raises), `commute()` (`COMMUTE`, or `GUARD_COMMUTE` = 2 world hours), `shifts()` (the shifts a workplace of the kind is worked in, one worker each, in hand-out order: `[OFFICE_HOURS]`, or `WATCHES`) |
| `JobKind::ALL` | `job.rs` | every kind **in hiring order**: office, collectors, guards. `give_a_job` and `Command::Hire` walk it |
| `Shift` | `job.rs` | `start`/`end` hour **on the same day** (end may be 24), `days` bitmask (bit 0 = Monday). `Shift::new` refuses anything else: a watch across midnight is two shifts. `OFFICE_HOURS` = 9, 17, `WEEKDAYS`; `WATCHES` = 8-16, 16-24, 0-8, `EVERY_DAY` |
| `Job::is_due(clock)` | `job.rs` | its hours on its days **plus the kind's commute before** (`Shift::is_due_after`): the office sets off at 08:30, a guard two hours before its watch. A shift from midnight is due from the evening before if the next day is worked. (`Shift::is_due` keeps the plain `COMMUTE`.) What `WorkRoutine` and the job menu read |
| `Shift::is_bedtime(clock)` | `job.rs` | **a worker sleeps by its shift**: the 8 hours (`BEDTIME`-`GETTING_UP`'s length) ending `UP_BEFORE_SHIFT` = 3 before the shift starts, every day. Office hours: 22-06, the clock's own night, so nothing changed for them; day watch 21-05; evening watch 05-13; night watch 13-21. `job::is_bedtime(job, clock)` falls back to `Clock::is_night` with no job. `SleepRoutine` and the boredom routine's no-play-at-night cap read it |
| `Shift::is_working_hours`, `hours()`, `days()` | `job.rs` | the hours alone; `09:00-17:00`; `Mon-Fri`, `Mon-Sun` (or `Mon, Wed, Sat`) |
| uniform | `job.rs`, `clothing.rs`, `entity.rs` | `JobKind::uniform()`: `Clothing::UniformOfficeWorker` (white shirt, navy tie), `UniformScrapCollector` (orange hi-vis, silver band), `UniformGuard` (navy tunic, brass). **`GameEntity::worn(clock)`** = the uniform while `Job::is_due` (setting off to the end of the shift), else the unit's own `clothing()`. Derived from the job and the clock, never stored, so **nothing new is saved** and a loaded world dresses the same. Uniforms are `Clothing::UNIFORMS`, outside the wardrobe `Clothing::ALL`: never rolled at spawn, not on the clothing menu, but `Clothing::from_name` knows them. Art in `tools/human_art.py` (`pixel-artist`) |
| `Job { kind, workplace, shift }` | `job.rs` | `Copy`. The workplace is **the unit's own** desk, bin or post; the shift is chosen at hire time (the first free one of `kind.shifts()` there). `Job::new(kind, at, shift)`, `Job::office(desk)`, `Job::collector(bin)`, `Job::guard(post, watch)` |
| `Workplaces` | `job.rs` | **(cell, shift start) → worker**, a `BTreeMap` twin of `Homes`: a desk or bin is one workplace, a post three. `claim(at, shift, uid)` (only while free), `free_shift(at, shifts)`, `release(uid)`, `worker(at, shift)`, `is_taken(at, shift)`, `len()` counts cell-and-shift pairs. Bookkeeping only — nothing in a tick reads it |
| weekday | `clock.rs` | `Weekday` (`short()`, `is_weekend()`), `Clock::weekday()` = `(day - 1) % 7`. **Day 1 is a Monday.** The HUD's `Display` is `day 1 Mon  08:00:00` |
| desk feature | `feature.rs` | `FeatureKind::Workstation` ↔ `"computer"` (beside `Entertainment`, as a fridge is Food and Water), `Access::Beside`. A computer with no power is neither |
| bin feature | `feature.rs` | `FeatureKind::ScrapBin` ↔ `"scrap bin"`, `Access::Beside`, **needs no power** |
| post feature | `feature.rs` | `FeatureKind::GuardPost` ↔ `"guard post"`, `Access::Beside`, needs no power; a feature even set into a wall, which is where a district has them |
| what bins hold | `bins.rs` | `Bins { cells, stored }`, built from the map's `"scrap bin"` props like `Fridges` (sorted, binary search): `stored(cell)`, `total()`, `add(cell, n)`, `iter()`, `len()`. A `GameState` field, `GameState::bins()`. A bin never fills, and nothing takes scrap out yet |

## Handing jobs out (`src/sim/mod.rs`)

- **At spawn, after the bed** (`GameState::spawn` → `give_a_bed` → `give_a_job`): for each kind
  in `JobKind::ALL`, the nearest workplace of its feature with a shift free
  (`Workplaces::free_shift(cell, kind.shifts())`) **and that the arrival may use** with the key
  its bed gave it (`Properties::may_use` — never a computer in somebody else's house); **the
  first kind with one wins**, and the job keeps the first free shift there. So every desk is
  given before the first bin, every bin before the first post, whatever the distances; one
  collector to a bin; a post's day, evening and night watch in that order before the next
  post; then unemployed. Logs `"{uid} was hired as an office worker at the computer at x, y,
  09:00-17:00"` / `"... as a scrap collector at the scrap bin at x, y, 09:00-17:00"` /
  `"... as a guard at the guard post at x, y, 16:00-24:00"`.
- **In the district** the bank's computers (12+) are the desks, the warehouse's
  `WAREHOUSE_BINS` (4) are the bins and the `GATES` (8) posts in the perimeter wall take 24
  guards; houses have none. So the first dozen-odd residents in spawner order sit at desks, the
  next four collect scrap and the next 24 keep the gates, wherever they live (seed 7's 136
  have plenty left over).
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
| routine | `WorkRoutine` ("go to work"): raises **`job.kind.goal()`** to **`WORKING` = 1.5** while `job.is_due(clock)` — `Work` at a desk or a post, `Collect` for a collector. Stateless decision (no stat, so no hysteresis); `on_shift` is kept for the brains menu and the save. **Last in `Brain::human()`** | `brain/routines.rs`, `routine.rs` |
| goal | `GoalId::Work` (💼) and `GoalId::Collect` (🧲), **the last two `GoalId`s** so every tie goes to the other goal | `brain/goal.rs` |
| office and gate | `WorkGoal`: walks beside **its own desk** — or post: the job's `workplace()` feature is what has to be there — and queues `Task::work(desk, WORK_SECONDS)`, or `Task::keep_watch(post, WORK_SECONDS)` for a guard — an hour of world a stretch, `Achieved` after each, so the routine decides how many. Logs `"worked an hour at the computer at x, y"` / `"kept watch an hour at the guard post at x, y"`. No other desk to reroute to: a desk it cannot reach is `Blocked` (held off); an unpowered desk (`!features.has(Workstation, desk)`) is `Blocked` before any walk. `prioritized` forgets a remembered desk that is no longer the job's | `brain/goals/work.rs` |
| office task | `Work { desk, seconds, watch }`: from beside it, every tick checks distance, a body, and that the workplace is still a feature (`Workstation`, which needs power, or with `watch` a `GuardPost`, which does not). Described `work at the computer at x, y` / `keep watch at the guard post at x, y`. `Action::interact` (lights the screen through `interacting_with`). Finished → `Event::Worked { world_seconds }`; **interrupted → `Worked` for the part done** (like `Sleep`) | `brain/tasks/work.rs` |
| collecting | `CollectGoal` (state: one `Seek`): **one trip is its unit of work**, `Achieved` when a packful is in the bin, and the routine starts the next. It plans **from what is true now** and never counts: room in the pack → the heap it was working while it has anything left, else `nearest_heap`, then `move_beside` + `Task::salvage_for_work`; pack full, or no heap left and something carried → its own bin (`job.workplace`, which just has to be in `features`), `move_beside` + `Task::deposit`; no heap and nothing carried → `Blocked` with `"found no scrap left to collect"` (the hold's doubling spaces those lines). A failed walk is `Seek::setback`: elsewhere for a heap is the next nearest, a bin has none. A failed salvage (the heap emptied under it) is a cheap replan. `prioritized` forgets a target no longer worth it — an emptied heap, or a bin that is not the job's — though `process` re-checks both anyway (mutating the forget out is caught by nothing) | `brain/goals/collect.rs` |
| nearest heap | `collect::nearest_heap`: `Map::heaps(map.navigation_area(), …)` — the site and `NAV_MARGIN` (64) round it, which a `GameState` always has loaded — keeping heaps with a side the unit may stand on that `navigation().may_reach` says it can get to and its memory does not hold out of reach; min by (manhattan, cell). No route, no allocation, once per trip decision | `brain/goals/collect.rs` |
| salvage for work | `Salvage { heap, seconds, for_work }`: `Task::salvage` (an order, `SalvageGoal`) or `Task::salvage_for_work` (the job). `for_work` reports `Event::Worked` for the piece on finishing **and for the part done when interrupted**; an order does neither | `brain/tasks/salvage.rs` |
| storing | `Deposit { bin, seconds }`, `DEPOSIT_SECONDS` = 5 world minutes: every tick checks within one step, the bin in `features`, scrap stowed. Finished → takes every piece out of the pack (`Inventory::take_out`), writes `Effect::Deposit { at, pieces }`, reports `Event::Worked`; interrupted → `Worked` for the part done, nothing stored | `brain/tasks/deposit.rs` |
| the world | `Effect::Deposit` is applied in `world_step` beside `Effect::Salvage` (`sim::deposit`): `Bins::add`, and the line `"{uid} stored N scrap metal in the bin at x, y: M stored"`. `testing::World` has `bins` and applies it too (`World::deposits()` counts the lines) | `sim/mod.rs`, `sim/testing.rs` |
| body | `Energy` takes `WORK_FATIGUE_PER_SECOND` on top of the waking drain: a working hour tires like `WORK_TIRES_LIKE` = **1.1** hours awake. `Fun` has no arm: work is no fun. `Satisfaction`, `Bladder` ignore it; `Experience::of` → `None` (not a treat). Walking out and back is not work; salvaging and storing are | `biology/energy.rs`, `mod.rs` |

**Sleeping by the shift**: `SleepRoutine`'s night is `job::is_bedtime` — 8 hours ending 3
before the shift — so every worker ends its shift 11 hours after getting up (10 for the day
watch), as the office does, and the 1.1 tuning below holds for every watch. Without it the
night watch, up since 06:00, passed `EXHAUSTED` mid-watch and went home to bed. A guard
coming off the night watch at 08:00 is near 75 and naps before its bedtime at 13:00.

**Guards' commute**: two world hours (`GUARD_COMMUTE`). The last guards hired get the posts
nobody nearer took, up to 127 cells (two hours' walk) from home; with the office's half hour
the evening watch was still walking when the day watch left and a district kept 230 of 256
post-looks through 16:00, which its test (needing 90%) does not pass. A guard who lives near is
early and waits by the post — the one free cell inside it is the day guard's, so the newcomer
waits for it or stands at the post's outside cell through the gate.

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
(office workers and guards included) from Tuesday on — the crowd limit in `map-generator`'s traps. The
district tests that assert scrap is stored switch the collectors' needs off; `qa/scrap_collector.json`
(one collector, a small yard, needs off) stores two packfuls by 15:30.

### Where 1.5 sits

| vs | value | who wins |
| --- | --- | --- |
| hunger/thirst/boredom at commit (60) | 1.2 | work |
| bladder at commit (`BURSTING` 70) | 1.4 | work (they hold it to 75) |
| any need past 75 | > 1.5 | the need; work has the worker back afterwards |
| nap at `EXHAUSTED` (75) | 1.5 | **sleep** (tie → lower `GoalId`) |
| night floor / orders / wander | 0.6 / 0.5 / 0.1 | work (a shift never overlaps its worker's own night, `Shift::is_bedtime`) |

**Why 1.1 and not more**: a body up rested at 06:00 is 68.75 tired by 17:00 with no job at
all. 8 hours at 1.1 adds 5 → 73.75, just short of `EXHAUSTED`; at 1.25 a worker would hit 75
around 16:10 and the nap (which wins the tie) would walk them out of every afternoon shift.
`a_rested_worker_is_worn_out_but_not_exhausted_at_the_end_of_a_shift` pins this. A collector
is tired only while salvaging and storing, so less than an office worker over a shift.

## The undertaker

The dead are the `biology::health` process's doing (`brain-engineer`): a body whose health
reaches 0 marks itself dead in `Human::react` and returns `Effect::Died`; `sim::died` (world
step) frees its cell, bed and workplace, dismisses it and files it in **`sim::Corpses`**
(`corpses.rs`: `(Uid, cell)` sorted by uid; `at(cell)`, `iter()`, `add`, `remove`). `Think::corpses`
hands it to every brain — the list of the dead, never the crowd. **Not saved**: `Corpses::of`
rebuilds it from the dead entities on load.

| Layer | What | File |
| --- | --- | --- |
| kind | `JobKind::Undertaker`: workplace `FeatureKind::Crematorium` (`"crematorium"`, beside, no power), goal `GoalId::Bury` (⚰, the last `GoalId`), `COMMUTE`, `WATCHES`, `Clothing::UniformUndertaker` (black cassock, white collar, gold cross) | `job.rs`, `feature.rs`, `clothing.rs` |
| routine | `WorkRoutine` raises `Bury` at `WORKING` while the watch is due **and whenever an undertaker has a body in hand** (`RoutineCtx::carrying`), so a body is never taken home | `brain/routines.rs` |
| goal | `BuryGoal` (a `Seek`, `pb::Executor::Seek`): body in hand → beside its own crematorium + `Task::cremate`; other item in hand → consume it first; free hand → `nearest_body` (manhattan over `Think::corpses`, a reachable side by `may_reach`, not remembered out of reach) + `Task::take_body`; none → `Blocked`. `Achieved` per body cremated | `brain/goals/bury.rs` |
| tasks | `TakeBody { body, seconds }` (`TAKE_BODY_SECONDS` = 5 world min, within a step, body still in `corpses`, hand free) → `Effect::TakeBody`; `Cremate { crematorium, seconds }` (`CREMATE_SECONDS` = 15 world min, beside, a body in hand) → empties the hand, `Effect::Cremate`. Both `Event::Worked` (and for the part done when cut short) | `brain/tasks/take_body.rs`, `cremate.rs` |
| world | `sim::take_body`: the lower slot wins a body; `ItemKind::Body` (70 kg, only ever in a hand, which is never refused) into the hand, off `Corpses`, onto `GameState::burials`; `bury()` despawns them after the processing pass, so nothing is pending between ticks. Logs `"{uid} took up the body of {dead} at x, y"`, `"{uid} cremated a body in the crematorium at x, y"` | `sim/mod.rs` |
| hands | `EatGoal`/`DrinkGoal::plan` refuse (`Blocked`) rather than "consume" a body in hand | `brain/goals/eat.rs`, `drink.rs` |
| menu | `now`: `seeing to the dead`, `cremating` at the crematorium | `game/unitpanel.rs` |

**Limit: the dead in a locked house stay there.** An undertaker holds its own home's key and
no other (`Body::home`), so `nearest_body` skips a body behind somebody's house door. A
master key would need keys that are not one `PropertyId` per body.

Tests: `corpses.rs`; `take_body.rs` and `cremate.rs` (`Rig`); `bury.rs` (takes up and cremates,
held off with nobody dead, a body in hand cremated after the watch, nobody goes out off watch,
a hungry undertaker does not eat the body); `sim::tests::the_dead_are_carried_to_the_crematorium_and_leave_the_world`.

## The steel works

The scrap the collectors bring in is made into steel. **Stores** (`sim/bins.rs`): `Bins` is every
store on the map — a `Store` each, by prop: `ScrapBin` (`"scrap bin"`, the warehouse's) and
`SteelRack` (`"steel rack"`, the warehouse's), `ScrapHopper` (`"scrap hopper"`) and
`SteelPallet` (`"steel pallet"`, the factory's); each holds **one item** (`Store::item`: scrap or
`ItemKind::Steel`), is a feature of its own kind (`Store::feature`), never fills, and gives
pieces back one at a time (`Bins::take`). `total_in(store)`, `count(store)`, `iter_stores()`.
`Think::bins` hands it to every brain, read-only.

| Layer | What | File |
| --- | --- | --- |
| item | `ItemKind::Steel` (`"steel"`, 5 kg, 1 l: two to a pack), `SCRAP_PER_STEEL` = 2, carried as ⛓ | `item.rs` |
| kinds | `JobKind::Steelworker` (workplace `FeatureKind::Furnace`, `"furnace"`, **needs power**; goal `GoalId::Smelt` 🔥) and `JobKind::Courier` (workplace `FeatureKind::ScrapHopper`; goal `GoalId::Haul` 📦), both `OFFICE_HOURS` and `COMMUTE`, hired **after** the undertakers (`JobKind::ALL`), uniforms `UniformSteelworker` (leather apron) and `UniformCourier` (green jacket, satchel) | `job.rs`, `feature.rs`, `clothing.rs` |
| the world | `Effect::Deposit { at, pieces }` puts the pieces in whatever store is at `at` (`sim::deposit`, the log line names the item and the store: `"stored 3 scrap metal in the bin at"`, `"stored 1 steel in the steel pallet at"`); `Effect::Withdraw { at }` takes pieces out one at a time while the store has any and the pack has room (`sim::withdraw`, `"took 2 steel from the steel pallet at x, y: N left"`), in slot order | `sim/mod.rs` |
| tasks | `Deposit` now empties the pack of **the store's item** into any store (checks `Think::bins`); `Withdraw { store, seconds }` (`WITHDRAW_SECONDS` = 5 world min) beside a store with stock and room in the pack → `Effect::Withdraw`; `Smelt { furnace, seconds }` (`SMELT_SECONDS` = 30 world min) beside a powered furnace with 2 scrap in the pack → 2 scrap out, 1 steel in, all in the pack, the furnace lit (`Action::interact`). All `Event::Worked`, and for the part done when cut short | `brain/tasks/withdraw.rs`, `smelt.rs`, `deposit.rs` |
| steelworker | `SmeltGoal` (a `Seek`): 2+ scrap in the pack → its own furnace, `Smelt`; steel in the pack → the nearest pallet, `Deposit` (`Achieved`); else the nearest hopper with scrap (`goals::nearest_store`), `Withdraw`; none → `"found no scrap to smelt"`, `Blocked` | `brain/goals/smelt.rs` |
| courier | `HaulGoal` (a `Seek`): steel in the pack → the nearest steel rack; scrap → its own hopper (`Deposit`, `Achieved` either way); empty → the nearer of the nearest stocked pallet and the nearest stocked scrap bin, `Withdraw`; neither → `"found nothing to carry"`, `Blocked`. **The trips alternate of themselves**: unloading scrap at the factory leaves it beside the pallets, unloading steel at the warehouse beside the bins | `brain/goals/haul.rs` |
| menu | `now`: `smelting steel` at the furnace, `fetching scrap, storing steel`; `carrying steel to the warehouse`, `carrying scrap to the factory`, `fetching a load` | `game/unitpanel.rs` |
| on screen | the hopper, pallet and rack fill like the scrap bin (`BinFill`, `fill_bins`); the furnace burns while worked (`used` strip, `"furnace"` emitter); the HUD's `steel  N stored` (the racks) | `game-screen`, `lighting` |

**In a district** (`map-generator`): the steel factory is a mandatory building on the lot
nearest the warehouse, 2 furnaces, 2 hoppers, 2 pallets (so 2 steelworkers and 2 couriers),
and the warehouse has one steel rack. Seed 7 with the three trades' needs off has its first
steel on the warehouse's rack by Tuesday 12:30 (the collectors spend Monday bringing the scrap
in). With needs on, the crowd limit in `map-generator`'s traps applies to them as to everybody.

Tests: `bins.rs` (every store holds its own item, gives it back until empty); `withdraw.rs`,
`smelt.rs` (`Rig`, with `Rig::apply_effect` doing the world's half: two ingots fit a pack);
`sim::tests::a_courier_and_a_steelworker_turn_the_warehouse_s_scrap_into_steel_on_its_rack`
(`testing::steel_works`, a 24x17 floor: the log lines of the whole chain, and every piece of
scrap accounted for — in a store, a pack, or two to an ingot; seen failing with the courier
never fetching from a pallet) and `a_district_s_steel_works_bring_steel_into_its_warehouse`
(seed 7, by Tuesday); `save::tests::the_steel_works_saved_mid_shift_carries_on_exactly`. QA
`qa/steel_factory.json`: a small powered room, the courier `give`n 3 scrap, the job menu at
10:00 (`carrying steel to the warehouse`), `expect_stored {"min": 1, "in": "steel rack"}` and
the HUD's `steel  1 stored` by the afternoon, the statistics menu (seen failing asking for 50).

## Saving (`savegame` skill)

- `pb::Brain.job` (tag 14, `pb::Job`: `JobKind` enum — `OfficeWorker = 0`,
  `ScrapCollector = 1`, `Guard = 2`, `Undertaker = 3` — workplace cell, start, end, days). Absent = unemployed, which is what
  every older save reads as. `Job::save`/`restore` (refuses a broken shift or unknown kind).
- `pb::World.workplaces` (tag 11, `pb::Workplace { desk, worker, start }` — a bin or post is a
  "desk" there; `start` (tag 3, optional) is the shift's start hour, absent in an older save =
  office hours, 9), `Workplaces::save`/`restore` (one workplace to one worker, one worker to a
  workplace; a start past 23 refused), restored after `homes` in `sim/saving.rs`, then
  **cross-checked** against the entities both ways (cell and start).
- `pb::World.bins` (tag 12, `pb::Bin { cell, stored }`), `Bins::save`/`restore`, restored after
  the fridges; refuses a cell with no bin or a different number of bins. Empty in an older
  save, which is a map with none.
- `WorkRoutine::on_shift` is saved as the routine's `committed`; `WorkGoal` and `CollectGoal`
  are a `pb::Executor::Seek`; the tasks are `pb::TaskKind::Work(Timed)` (tag 11), a guard's
  `KeepWatch(Timed)` (tag 14, `Work { watch: true }`),
  `Deposit(Timed)` (tag 12) and `SalvageForWork(Timed)` (tag 13) — an order's salvage stays
  `Salvage(Timed)` (tag 10). `GoalId::Collect` = 9.
- The routine was appended, so `Brain::restore` accepts a saved routine list that is a
  **prefix** of the brain's (old saves, `format1.savegame`); an executor a save does not
  mention (`CollectGoal` in an older one) starts fresh.

## The job menu (`src/game/unitpanel.rs`)

`job` on the unit bar opens `Menu::Job`: a live `Readout::Job` block from
`job_block(entity, clock)` — `job`, `at` (`computer at 12, 4`, `scrap bin at 30, 2`), `hours`,
`days`, `now` (`working`, `on the way to work`, `collecting scrap`, `storing scrap` while at
the bin, `on watch`, `on the way to the gate`, `on a break: <goal>`, `off until 09:00`, `off
until 00:00` for the night watch once its watch is over, `done for the day`, `weekend`, `day
off`) —
or `unemployed` (a human) / `no job` (anything else). Humans get a button
(`Action::ToggleJob`, label `Readout::Employ`: `dismiss` / `hire`) that sends `Input::dismiss`
/ `Input::hire`. The HUD's map stats gain `scrap  N stored` on a map with bins, and a bin on
screen shows how full it is (`game-screen`).

## Tests

- `job.rs`: office hours, the commute, refused shifts, day lists, save round trips, desks, the
  collector kind and `ALL`'s order; a post kept by exactly one watch every quarter hour of
  every day, three watches to a post and one free again when its guard goes; the night watch
  due from the evening before (two hours for a guard, half an hour for the office); bedtimes
  per watch (office = the clock's night); workplaces with and without a saved start.
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
  `Work`; ordering against every need; exhausted ties go to sleep; the night watch goes to bed
  at 14:00 rested and is not in bed at 02:00, when the office and the unemployed are.
- `brain/goals/work.rs`: a night-watch guard sets off at 22:00, stands beside its post (no
  power), and logs `kept watch an hour`, never an hour at a computer.
- `feature.rs`: a guard post is a `GuardPost`, beside, no power, even in a wall.
- `sim/mod.rs`: hired at the nearest free desk until none; a desk before a bin however near the
  bins, one collector to a bin, `Hire` in the same order; a dog is not; leaving frees the
  desk; hire/dismiss commands; a district gives every desk in the bank, then its four bins in
  the warehouse, in arrival order; a district Monday stores scrap in the warehouse (the
  collectors' needs off); the parallel determinism test asserts work happened, and
  `determinism_survives_a_crowd_of_scrap_collectors_in_the_parallel_rounds` runs 70 collectors
  and compares what every bin holds; the district test also checks 24 guards, every gate's
  post with watches 0, 8 and 16, hiring order office < collectors < guards;
  `a_guard_post_is_kept_round_the_clock_by_its_three_watches` (three guards with beds, needs
  but tiredness off: somebody at the post at ≥ 90% of 96 quarter-hour looks, each keeping ≥ 5
  hours); `a_district_keeps_its_gates_through_the_change_of_watch` (seed 7, the guards' needs
  off, 10:00-18:00, ≥ 90% of post-looks kept).
- `game/unitpanel.rs`: the guard's menu (post, `16:00-24:00`, `Mon-Sun`, `off until 16:00` on
  a Saturday, `off until 00:00` for the night watch).
- `save::tests::a_district_saved_in_the_middle_of_a_working_day_carries_on_exactly` (saved
  with somebody at a desk **and** somebody keeping watch),
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
  observation's goals column shows `work 2` through the day. `qa/guards.json`: a gatehouse
  laid out like a district gate, three arrivals hired to one post in watch order (`expect_log`
  after each spawn), the menu, `on watch` / `keep watch at the guard post` in the morning, an
  hour kept, the evening watch on and the day watch `done for the day` at 16:30, the night
  watch on at 00:30; the clothing menu reads `wearing: uniform guard (own: ...)` on watch; seen
  failing with guards left out of `JobKind::ALL`.
- `kinds.rs`: `a_worker_wears_its_jobs_uniform_while_its_shift_is_due_and_its_own_clothes_otherwise`
  (office from 08:30 to 17:00 and not on Saturday, changing clothes changes the clothes it goes
  home in, the night watch dressed before midnight, the collector's, out of uniform when let
  go, a dog wears nothing); `clothing.rs`: uniforms are named, found by name and never in the
  wardrobe; `characters/human.rs`: every uniform has its own art on disk.

The guard tests were seen failing with workers keeping the clock's night (the night-watch
routine test and the three-watch sim test), with no commute before a midnight watch (the job
and work-goal tests), and with guards commuting like the office (the district change-of-watch
test). Each guard above was seen failing under a mutation (routine unregistered, strict restore,
work not tiring, job not saved, priority at 1.0, no part credit, desks given twice, a
seven-day week, no power check; and for collecting: the routine raising `Work` for a
collector, the deposit not emptying the pack, bins offered before desks, salvage for work not
tiring, bins not restored).

## Adding a job kind

1. A `JobKind` variant (+ `pb::JobKind`, both `From`s), its `name`, `with_article`,
   `workplace()` feature, `workplace_name`, `goal()`, `commute()`, `uniform()` (a new
   `Clothing::Uniform*` and its art) and `shifts()` (several for a
   workplace kept round the clock), and its place in `JobKind::ALL` (hiring order). A new workplace is a new `FeatureKind` + `FEATURES` entry
   (`brain-engineer`); in the district, a building with that many of them (`map-generator`,
   mandatory lot buildings).
2. If the work is not "stand beside a prop for an hour", a goal of its own (a `GoalId`
   appended, its executor registered last in `Brain::human()`) and tasks; what it does to the
   body is an `Event` the processes decide on.
3. The job menu's `now` line (`job_block`) has an arm per kind.
