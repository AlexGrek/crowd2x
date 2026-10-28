---
name: jobs
description: Jobs in crowd2x - who works where and when. The Job model (kind, own workplace, shift), the office worker (a computer) and the scrap collector (a scrap bin in the warehouse, scrap from the heaps in the wasteland), both 09:00-17:00 Mon-Fri, and the guard (a guard post by a gate in the district's wall, three 8-hour watches every day, a guard to each), the undertaker (a crematorium, the guards' three watches, carrying the dead to it: BuryGoal, TakeBody, Cremate), the steel factory's steelworker (a furnace: scrap from a hopper smelted into steel on a pallet, SmeltGoal, Withdraw, Smelt) and courier (a scrap hopper: scrap from the warehouse's bins to the hopper and steel from the pallets to the warehouse's rack, HaulGoal), the farmer (a grain bin at the farm, a field of ten crops of wheat sown in the wasteland's good soil, tended once a day for quality, harvested ripe after six days, 09:00-17:00 Mon-Sat: FarmGoal, Sow, Tend, Harvest, sim::fields), the power plant worker (the power plant's control panel, the three watches, standing by and nothing else yet; the Work task's STRETCHES), the playzone administrator (one central computer, four shifts: a day and an evening shift for each of two groups on alternate days; the arcade cabinets work only while one is on duty beside the computer: sim::playzone, Effect::Operate, UseArcade), the cleaner (the district itself as its workplace, no prop; four on two groups' morning and evening shifts, two a day; its brain looks over the district for dirt every 27 minutes and mops it up, roaming with none: CleanGoal, the dirt skill), the stores (Bins: scrap bins, steel racks, scrap hoppers, steel pallets, grain bins; Effect::Deposit / Effect::Withdraw), weekdays on the clock (day 1 is a Monday), commutes, sleeping by one's shift (Shift::is_bedtime), how a workplace - a cell and a shift - is handed out at spawn like a bed in hiring order - desks first, then bins, then posts (Workplaces, give_a_job, JobKind::ALL) - and taken back, Command::Hire / Command::Dismiss, WorkRoutine and its 1.5 priority against the needs, WorkGoal and the Work task (an hour a stretch), CollectGoal (every trip from the bin), getting ready before a trip out (ReadyRoutine: every need 85% met, at the bin or back in with a half trip), salvaging for work and the Deposit task, the Bins store, Event::Worked fatigue (tiring, no fun), saving a job, the desks and the bins, the job menu in the unit panel, and the tests and QA. Maps every concept to the file and function that owns it. Use when adding a job kind, changing hours, schedules, weekends or what work does to a body, when somebody does not go to work, never leaves it or brings no scrap in, or when touching the job menu.
---

# Jobs

A job is **what a unit does for a living, where, and when**. All tiring and no fun:

- the **office worker**: their own computer, worked an hour at a time, nine to five, Monday to
  Friday;
- the **scrap collector**: their own scrap bin in the district's warehouse — out to the heaps
  of scrap in the wasteland, salvage a packful, back to the bin and store it; **seven to six**,
  Monday to Friday (`COLLECTOR_HOURS`: most of a trip is the walk, and both ends are in
  daylight, so a collector is home before the deathdogs come out at dusk);
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
  steel rack. See "The steel works" below;
- the **farmer**: their own grain bin at the district's farm, nine to five **every day but
  Sunday** — a field of ten crops of wheat sown in the good soil out in the wasteland, tended
  every day, harvested when ripe and the wheat stored in the bin. See "The farm" below;
- the **power plant worker**: their own watch (the guards' `WATCHES`, every day) at the
  `"control panel"` of the district's power plant — stood by an hour at a time like a guard's
  post (`WorkGoal`, the `"stand by"` stretch), **and nothing else yet**. The plant's one panel
  takes 3, hired after the farmers. See "The power plant" below;
- the **king**: the `"throne"` in the district's palace, the one there is — court held an
  hour at a time like a guard's post (`WorkGoal`, the `"hold court"` stretch), ten to four
  every day but Sunday, **and nothing else yet**: the major decisions are to be the king's
  brain's, next. In robes and a crown always, and walking slowly (`Bearing`). Hired after the
  power plant workers. See "The king" below;
- the **playzone administrator**: the `"playzone computer"` in the district's playzone — one
  computer, **four** administrators: a day shift (10-16) and an evening shift (16-22), each kept
  by two who take turns by the day (`PLAYZONE_SHIFTS`: group 1 Mon, Wed, Fri, Sun; group 2 Tue,
  Thu, Sat), so every day has two shifts and two administrators on them — worked an hour at a time
  like a desk (`WorkGoal`, the `"run the playzone"` stretch), **and while one is on duty and
  beside the computer the playzone's arcade cabinets work** (`sim::playzone`). See "The
  playzone" below;
- the **cleaner**: **the district itself** (`FeatureKind::District`, no prop, one to a site of
  `TOWN_HOMES` homes or more) — four cleaners on the playzone's two groups, a morning (07-15) and
  an evening (15-23) shift each (`CLEANER_SHIFTS`), so two every day, one at a time, four days a
  week or three. Its own brain looks over the district every 27 world minutes for public ground
  dirtier than 0.15, mops it up nearest first (`CleanGoal`, `Task::clean`, `Effect::Clean`), and
  with nothing found walks round — it has nowhere to wait. Hired **last** (`JobKind::ALL`'s end).
  See "The cleaner" below and the `dirt` skill.

Plain Rust in `src/sim/`, like every other part of the simulation.

## The model (`src/sim/job.rs`)

| Concept | Where | What |
| --- | --- | --- |
| `JobKind` | `job.rs` | a `catalogue!` (`sim/catalogue.rs`): **one row a job**, a `JobDef` — `name`, `article` (for log lines), `workplace` (the `FeatureKind` its workplace is; `workplace_name()` is that feature's prop: `computer`, `scrap bin`, `guard post`), `goal` (`GoalId::Work` for a desk **and a post**, `GoalId::Collect`... — what `WorkRoutine` raises), `commute` (`COMMUTE`, or `GUARD_COMMUTE` = 2 world hours), `shifts` (the shifts a workplace of the kind is worked in, one worker each, in hand-out order: `[OFFICE_HOURS]`, or `WATCHES`), `uniform` (its name), `now` (the job menu's line), `goes_out_for` and `bearing` (below). The methods read the row. Saved as its number (`JobKind::number`) |
| `Bearing` | `job.rs` | how somebody in the job carries themselves, on duty or off: `gait` (a share of its own pace, hurry included: `Walker::set_gait`, set by `Human::hire`/`dismiss`/`restore` from the job, **not saved**), `always_in_uniform` (`GameEntity::worn` puts the uniform on at any hour, not only while due) and `hat` (a layer over the hair with the uniform: `assets/human/hat_<hat>.png`, `characters::human::hat_path`). `WORKER` (1.0, on shift, none) for every job but the king's `ROYAL` (`KING_GAIT` 0.6, always, `"crown"`) |
| `JobKind::ALL` | `job.rs` | every kind **in hiring order** — the order the rows are listed: office, collectors, guards, undertakers, steelworkers, couriers, farmers, power plant workers. `give_a_job` and `Command::Hire` walk it |
| `Shift` | `job.rs` | `start`/`end` hour **on the same day** (end may be 24), `days` bitmask (bit 0 = Monday). `Shift::new` refuses anything else: a watch across midnight is two shifts. `OFFICE_HOURS` = 9, 17, `WEEKDAYS`; `WATCHES` = 8-16, 16-24, 0-8, `EVERY_DAY`; `FARM_HOURS` = 9, 17, `MONDAY_TO_SATURDAY`; `PLAYZONE_SHIFTS` = 10-16 and 16-22, each for `FIRST_GROUP` (`0b101_0101`: Mon, Wed, Fri, Sun) and `SECOND_GROUP` (`0b010_1010`: Tue, Thu, Sat), handed out day shifts first (so two keep it open every day from ten) |
| `Job::is_due(clock)` | `job.rs` | its hours on its days **plus the kind's commute before** (`Shift::is_due_after`): the office sets off at 08:30, a guard two hours before its watch. A shift from midnight is due from the evening before if the next day is worked. (`Shift::is_due` keeps the plain `COMMUTE`.) What `WorkRoutine` and the job menu read |
| `Shift::is_bedtime(clock)` | `job.rs` | **a worker sleeps by its shift**: the 8 hours (`BEDTIME`-`GETTING_UP`'s length) ending `UP_BEFORE_SHIFT` = 3 before the shift starts, every day. Office hours: 22-06, the clock's own night, so nothing changed for them; day watch 21-05; evening watch 05-13; night watch 13-21. `job::is_bedtime(job, clock)` falls back to `Clock::is_night` with no job. `SleepRoutine` and the boredom routine's no-play-at-night cap read it |
| `Shift::is_working_hours`, `hours()`, `days()` | `job.rs` | the hours alone; `09:00-17:00`; `Mon-Fri`, `Mon-Sun` (or `Mon, Wed, Sat`) |
| uniform | `job.rs`, `clothing.rs`, `entity.rs` | `JobKind::uniform()` = **`Clothing::Uniform(kind)`**, named by the row's `uniform` (`"uniform office worker"`: white shirt, navy tie; `"uniform scrap collector"`: orange hi-vis; `"uniform guard"`: navy tunic, brass...). **`GameEntity::worn(clock)`** = the uniform while `Job::is_due` (setting off to the end of the shift), else the unit's own `clothing()`. Derived from the job and the clock, never stored, so **nothing new is saved** and a loaded world dresses the same. Uniforms are `Clothing::uniforms()`, outside the wardrobe `Clothing::ALL`: never rolled at spawn, not on the clothing menu, but `Clothing::from_name` knows them. Art: `assets/human/clothes_uniform_<job>.png`, found by the name (`characters::human::clothing_path`), drawn by `tools/human_art.py` (`pixel-artist`) |
| `Job { kind, workplace, shift }` | `job.rs` | `Copy`. The workplace is **the unit's own** desk, bin or post; the shift is chosen at hire time (the first free one of `kind.shifts()` there). `Job::new(kind, at, shift)`, `Job::office(desk)`, `Job::collector(bin)`, `Job::guard(post, watch)` |
| `Workplaces` | `job.rs` | **(cell, shift start, shift days) → worker**, a `BTreeMap` twin of `Homes`: a desk or bin is one workplace, a post three, a playzone's computer four (two shifts starting at the same hour, told apart by their days). `claim(at, shift, uid)` (only while free), `free_shift(at, shifts)`, `release(uid)`, `worker(at, shift)`, `is_taken(at, shift)`, `len()` counts cell-and-shift pairs. Bookkeeping only — nothing in a tick reads it. `Shift::mask()` is the days as a bitmask. Saved with `start` **and `days`** (`pb::Workplace.days`, tag 4); a save from before says no days and `Workplaces::settle` gives each workplace its worker's shift's days on load (`savegame`) |
| weekday | `clock.rs` | `Weekday` (`short()`, `is_weekend()`), `Clock::weekday()` = `(day - 1) % 7`. **Day 1 is a Monday.** The HUD's `Display` is `day 1 Mon  08:00:00` |
| desk feature | `feature.rs` | `FeatureKind::Workstation` ↔ `"computer"` (beside `Entertainment`, as a fridge is Food and Water), `Access::Beside`. A computer with no power is neither |
| bin feature | `feature.rs` | `FeatureKind::ScrapBin` ↔ `"scrap bin"`, `Access::Beside`, **needs no power** |
| post feature | `feature.rs` | `FeatureKind::GuardPost` ↔ `"guard post"`, `Access::Beside`, needs no power; a feature even set into a wall, which is where a district has them |
| what stores hold | `bins.rs` | `Bins { cells, kinds, stored }`, built from the map's props that are a **store** — a `FeatureKind` whose row has a `StoreDef` (`feature::store_of`) — like `Fridges` (sorted, binary search): `store(cell)` (its `FeatureKind`), `item(cell)`, `stored(cell)`, `total()`, `total_in(kind)`, `count(kind)`, `add(cell, n)`, `take(cell)`, `iter_stores()`. A `Facilities` field, `GameState::bins()`, `Think::bins`. A store never fills |

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
- **The dead are replaced.** `effect::died` dismisses the dead and queues its job on
  `Facilities::vacancies`; at the end of the same processing pass (`comings_and_goings`)
  `GameState::fill_vacancy` hands **that very workplace and shift** to the living, jobless,
  unfrozen human nearest it who may use it (`Properties::may_use`), one not on sick leave
  while there is any such, the lower uid on a tie. Logged `"... was hired as a scrap collector
  at the scrap bin at x, y, 07:00-18:00, in place of {dead}"`, or `"nobody is free to take on
  the {workplace} at x, y that {dead} left"` and the post stands empty. One walk of the crowd a
  death; the list is empty between ticks, so nothing new is saved. Before it, a post emptied by
  a death stayed empty with dozens jobless, and the last steelworker or courier dying stopped
  steel for good (the `lab` skill). Test: `sim::tests::the_job_somebody_dies_in_goes_to_the_nearest_jobless`.

## The brain

| Layer | What | File |
| --- | --- | --- |
| routine | `WorkRoutine` ("go to work"): raises **`job.kind.goal()`** to **`WORKING` = 1.5** while `job.is_due(clock)` — `Work` at a desk or a post, `Collect` for a collector. Stateless decision (no stat, so no hysteresis); `on_shift` is kept for the brains menu and the save | `brain/routines.rs`, `routine.rs` |
| late | `WorkRoutine` also makes `GoalId::Work` **urgent** (`Goals::hurry`) for a worker kept at one place — desk, guard post, control panel — standing more than a cell (`AT_WORK`) from its workplace once the shift's hours have begun (`Shift::is_working_hours`): `LATE` = 0.5 at the start, 1 an hour (`LATE_BY`) late, so it hurries in, and back from a need that took it away. Stateless: a job worked on the move (collector, courier, farmer, steelworker, undertaker) is never late. Urgency only sets walking speed (`brain-engineer`, "How fast it walks") | `brain/routines.rs` |
| getting ready | `ReadyRoutine` ("get ready to go out"), **last in `Brain::human()`**: before every trip out, a collector sees to its needs first — see "Getting ready" below | `brain/routines.rs` |
| goal | `GoalId::Work` (💼) and `GoalId::Collect` (🧲), **the last two `GoalId`s** so every tie goes to the other goal | `brain/goal.rs` |
| office, gate and power plant | `WorkGoal`: walks beside **its own desk** — or post, or control panel: the job's `workplace()` feature is what has to be there — and queues `Task::work_at(job.kind.workplace(), desk, WORK_SECONDS)` — an hour of world a stretch, `Achieved` after each, so the routine decides how many. Logs `"{uid} {done} an hour at the {prop} at x, y"` from the stretch's words: `"worked an hour at the computer at x, y"` / `"kept watch an hour at the guard post at x, y"` / `"stood by an hour at the control panel at x, y"`. No other desk to reroute to: a desk it cannot reach is `Blocked` (held off); an unpowered desk (`!features.has(Workstation, desk)`) is `Blocked` before any walk. `prioritized` forgets a remembered desk that is no longer the job's | `brain/goals/work.rs` |
| office task | `Work { desk, seconds, place }`: from beside it, every tick checks distance, a body, and that the workplace is still a `place` feature (`Workstation`, which needs power; `GuardPost` and `ControlPanel`, which do not). **`STRETCHES`** (`tasks/work.rs`) is every workplace a job is stood at, with what a stretch there is and was: `(Workstation, "work", "worked")`, `(GuardPost, "keep watch", "kept watch")`, `(ControlPanel, "stand by", "stood by")` — the first word is also the name it is saved under. Constructors `Task::work`, `Task::keep_watch`, `Task::work_at(place, ...)`. Described `work at the computer at x, y` / `keep watch at the guard post at x, y` / `stand by at the control panel at x, y`. `Action::interact` (lights the screen through `interacting_with`). Finished → `Event::Worked { world_seconds }`; **interrupted → `Worked` for the part done** (like `Sleep`) | `brain/tasks/work.rs` |
| collecting | `CollectGoal` (state: one `Seek`): **one trip is its unit of work**, `Achieved` when a packful is in the bin, and the routine starts the next. It plans **from what is true now** and never counts: **a trip starts from the bin** — nothing in the pack, in the district, not beside its bin and not ready (`GoalCtx::ready_to_go_out`, the routine's say) → walk beside the bin first (no task after it); room in the pack → the heap it was working while it has anything left, else `nearest_heap`, then `move_beside` + `Task::salvage_for_work`; pack full, or no heap left and something carried → its own bin (`job.workplace`, which just has to be in `features`), `move_beside` + `Task::deposit`; no heap and nothing carried → `Blocked` with `"found no scrap left to collect"` (the hold's doubling spaces those lines). A failed walk is `Seek::setback`: elsewhere for a heap is the next nearest, a bin has none. A failed salvage (the heap emptied under it) is a cheap replan. `prioritized` forgets a target no longer worth it — an emptied heap, or a bin that is not the job's — though `process` re-checks both anyway (mutating the forget out is caught by nothing) | `brain/goals/collect.rs` |
| nearest heap | `collect::nearest_heap`: `Map::heaps(map.navigation_area(), …)` — the site and `NAV_MARGIN` (64) round it, which a `GameState` always has loaded — keeping heaps with a side the unit may stand on that `navigation().may_reach` says it can get to and its memory does not hold out of reach; min by (manhattan **from the bin while in the district**, from where it stands out in the wasteland; cell). No route, no allocation, once per trip decision. From the bin because the warehouse is built nearest the scrap: measured from home, a collector chose heaps on the far side of the district, two hours' walk round by a gate | `brain/goals/collect.rs` |
| salvage for work | `Salvage { heap, seconds, for_work }`: `Task::salvage` (an order, `SalvageGoal`) or `Task::salvage_for_work` (the job). `for_work` reports `Event::Worked` for the piece on finishing **and for the part done when interrupted**; an order does neither | `brain/tasks/salvage.rs` |
| storing | `Deposit { bin, seconds }`, `DEPOSIT_SECONDS` = 5 world minutes: every tick checks within one step, the bin in `features`, scrap stowed. Finished → takes every piece out of the pack (`Inventory::take_out`), writes `Effect::Deposit { at, pieces }`, reports `Event::Worked`; interrupted → `Worked` for the part done, nothing stored | `brain/tasks/deposit.rs` |
| the world | `Effect::Deposit` is granted by `effect::apply` (`effect::deposit`): `Bins::add`, and the line `"{uid} stored N scrap metal in the bin at x, y: M stored"` (the word is the store's `StoreDef::called`). `testing::World` calls the same `apply` (`World::deposits()` counts the lines) | `sim/effect.rs` |
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
into the next shift and delivers it then (the plan starts from a pack with scrap in it). Such a
morning is the one trip not got ready for at the bin: with scrap in the pack it gets ready at
home and walks out from there.

### Getting ready (`ReadyRoutine`, `brain/routines.rs`)

A trip out is far from any fridge or toilet, and a need past `CALLED_AWAY` (90: a shift under
way holds, `goal::HOLD`, against anything less) calls a collector back in for that one need,
then out, then back for the next. So before going out it **gets ready**: every need on
`READY_NEEDS` 85% met (`READY` = 15). Its goals are raised to **break in**
(`Goals::break_in`): the shift in charge beside the bin does not hold against them.

- **Which jobs**: a `JobDef::goes_out_for` column, what a trip brings back — `Some(Scrap)`
  for the collector, `Some(Wheat)` for the farmer (a field is ten cells past the wall),
  `Some(Fish)` for the fisherman, `None` for the rest.
- **When**: shift due, in the district (on the site), room in the pack, and either **beside
  its bin** (the trip's start; `CollectGoal` goes there first) or **back in with a trip half
  done** (called in by a need — it tops everything up at that fridge, instead of in and out
  per need). Not at home, not out in the wasteland.
- **What**: when any need is worse than `READY`, a list taken there and then of every need
  worse than its `ReadyNeed::top_up` **that its pocket will not meet out there**
  (`ReadyNeed::pocket`: a meal stowed answers hunger, a drink thirst — **no stop at a fridge for
  what it carries**; it eats and drinks it where it stands in the wasteland, and the next trip
  restocks it) — `TOPPED_UP` (5) for thirst, hunger and the bladder, so
  a second round does not put a drink after the toilet; `READY` for fun, since a half-hour
  go drains while it lasts and topping it to 95% meant a second go worth less. Raised at
  `GETTING_READY` (1.6) less `IN_TURN` (0.01) per place, in list order **drink, eat, play,
  toilet** — the toilet last because a drink reaches the bladder over half an hour. A need
  leaves the list when topped up; **nothing joins it meanwhile** (no turning round on the
  way to the toilet for hunger); when it empties, a fresh list if anything is worse than
  ready; none → **ready**, and the log says `"{uid} is ready to go out for scrap metal"`.
  A need whose goal is held off (nothing to play on — a computer is entertainment only in a
  home) is not waited for.
- **Provisions, last** (`PROVISIONS_BIT`, the bit after the needs on the same list, so saved with
  it, `pb::RoutineState.pending`): once every need is seen to, a worker whose **pocket** is short
  of `goals::provision::PROVISIONS` — **one meal and one drink**, stowed, the hand not counting —
  raises `GoalId::Provision` at `GETTING_READY` less four places (`IN_TURN` each, after the needs), and is **ready only when the
  last thing is put away**. Like a need, the bit stays on the list wherever the fridge is (the
  worker leaves the bin for it). `ProvisionGoal` (`brain/goals/provision.rs`): walk beside the
  nearest fridge it may use, open it, `Task::take` each thing missing (**a meal costs a dollar,
  a drink nothing**, the usual `Effect::Buy`) and `Task::stow` it, **shutting the fridge before
  the last stow** (nothing may be left to do once the pocket is full, the routine lets the
  worker go that tick). `Blocked` — no fridge, a fridge off, no room — holds it off and the
  routine stops waiting (`goals.cooldown(Provision)`), as for any need nothing can meet. A pocket
  that is already stocked costs nothing: a worker with its meal and drink goes straight out.
  **Why only one of each**: a pocket is the unit's mass limit like anything else, and three
  pieces of scrap are 9 of the 10 kg a person carries — a meal and a drink are exactly the last
  kilo, so a collector still brings home a full packful of three; two of each would cost it a
  piece. (A farmer's twelve sheaves, 12 L, now fit ten: it takes its harvest home a little oftener.)
- **Eaten out there**: `EatGoal` / `DrinkGoal` — **off the site, with one stowed**
  (`goals::pocket_has`) — queue `Task::unpack` (pocket to hand, half a world minute) and the
  usual `Task::consume` instead of walking to a fridge, and log `"ate what it was carrying"` /
  `"drank what it was carrying"`. On the site they walk to the fridge as ever and leave the
  pocket alone, so the pocket is the trip's. What was eaten is restocked at the next trip's
  start (a meal is a dollar again).
- **Ready lasts** until it is off the site or off shift, so the walk to the gate does not
  send it back. `GoalCtx::ready_to_go_out` tells `CollectGoal`: ready → out from wherever
  it is to the heap nearest the bin; not ready → the bin first.
- **Not tiredness**: only a night's sleep helps, and the shift is tuned so a rested worker
  never reaches `EXHAUSTED` on it.
- The job menu's `now` reads `getting ready: drink` (a needed goal in charge while the
  routine has a list); the brains menu shows `ready to go out` and `seeing to`.

**Measured** (2026-10-20, a week Mon-Fri, every resident's needs off but the four collectors',
packfuls stored): seeds 7, 0, 42 and 1-6 — **93 pieces before, 123 after (+32%)**; the same
trip rules without getting ready stored about a third as much as with it. With the
collectors' needs off as well, seed 7 stores 117: the needs still take more than half of a
shift (a drink every 3 hours, a toilet every 2-3, a meal, a go at something, each a walk), and
time out in the wasteland — about a piece an hour there — is what scrap follows. Seed 4
stores nothing either way (not looked into). Fun to 95% instead of 85% was a wash over nine
seeds.

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

A shift under way **holds** (`goal::HOLD` = 0.3, the `brain-engineer` skill): a rival takes it
off only when wanted by more than 1.8. A shift not yet begun is a fresh choice.

| vs | value | who wins |
| --- | --- | --- |
| boredom, however bad | `PLAYING` 0.12 | work, always: fun never interrupts |
| hunger/thirst at commit (60) | 1.2 | work |
| bladder at commit (`BURSTING` 70) | 1.4 | work |
| a need past 75, shift not yet begun | > 1.5 | the need |
| a need past 75, shift under way | 1.5-1.8 | **work** — held; the worker hurries once it goes |
| a need past `CALLED_AWAY` (90) | > 1.8 | the need; work has the worker back afterwards |
| collapsing (`COLLAPSED` 95 tired) | 1.9 | sleep, where it stands |
| nap at `EXHAUSTED` (75), shift not yet begun | 1.5 | **sleep** (tie → lower `GoalId`) |
| night floor / orders / wander | 0.6 / 0.5 / 0.1 | work (a shift never overlaps its worker's own night, `Shift::is_bedtime`) |
| danger (`FLEEING`) | 3.0 | flee, held or not |

**Why 1.1 and not more**: needs are twice as slow as they were (energy: 32 h from rested to
exhausted), so a body up rested at 06:00 is 34.4 tired by 17:00 with no job and 36.9 after 8
hours at 1.1 — the ratio the old, tighter day was tuned to (68.75 and 73.75, just short of
`EXHAUSTED`, where at 1.25 the nap would have walked them out of every afternoon shift).
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
| kind | `JobKind::Undertaker`: workplace `FeatureKind::Crematorium` (`"crematorium"`, beside, no power), goal `GoalId::Bury` (⚰, the last `GoalId`), `COMMUTE`, `WATCHES`, uniform `"uniform undertaker"` (black cassock, white collar, gold cross) | `job.rs`, `feature.rs` |
| routine | `WorkRoutine` raises `Bury` at `WORKING` while the watch is due **and whenever an undertaker has a body in hand** (`RoutineCtx::inventory`'s hand), so a body is never taken home | `brain/routines.rs` |
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
store on the map, each a `FeatureKind` with a `StoreDef` in its row: `ScrapBin` (`"scrap bin"`) and
`SteelRack` (`"steel rack"`), the warehouse's (`site: "warehouse"`), `ScrapHopper`
(`"scrap hopper"`) and `SteelPallet` (`"steel pallet"`), the factory's (`site: "factory"`); each
holds **one item** (`StoreDef::holds`: scrap or `ItemKind::Steel`), never fills, and gives pieces
back one at a time (`Bins::take`). `Think::bins` hands it to every brain, read-only.

| Layer | What | File |
| --- | --- | --- |
| item | `ItemKind::Steel` (`"steel"`, 5 kg, 1 l: two to a pack), `SCRAP_PER_STEEL` = 2, carried as ⛓ | `item.rs` |
| kinds | `JobKind::Steelworker` (workplace `FeatureKind::Furnace`, `"furnace"`, **needs power**; goal `GoalId::Smelt` 🔥) and `JobKind::Courier` (workplace `FeatureKind::ScrapHopper`; goal `GoalId::Haul` 📦), both `OFFICE_HOURS` and `COMMUTE`, hired **after** the undertakers (`JobKind::ALL`), uniforms `"uniform steelworker"` (leather apron) and `"uniform courier"` (green jacket, satchel) | `job.rs`, `feature.rs` |
| the world | `Effect::Deposit { at, pieces }` puts the pieces in whatever store is at `at` (`effect::deposit`, the log line names the item and the store: `"stored 3 scrap metal in the bin at"`, `"stored 1 steel in the steel pallet at"`); `Effect::Withdraw { at }` takes pieces out one at a time while the store has any and the pack has room (`effect::withdraw`, `"took 2 steel from the steel pallet at x, y: N left"`), in slot order | `sim/mod.rs` |
| tasks | `Deposit` now empties the pack of **the store's item** into any store (checks `Think::bins`); `Withdraw { store, seconds }` (`WITHDRAW_SECONDS` = 5 world min) beside a store with stock and room in the pack → `Effect::Withdraw`; `Smelt { furnace, seconds }` (`SMELT_SECONDS` = 30 world min) beside a powered furnace with 2 scrap in the pack → 2 scrap out, 1 steel in, all in the pack, the furnace lit (`Action::interact`). All `Event::Worked`, and for the part done when cut short | `brain/tasks/withdraw.rs`, `smelt.rs`, `deposit.rs` |
| steelworker | `SmeltGoal` (a `Seek`): 2+ scrap in the pack → its own furnace, `Smelt`; steel in the pack → the nearest pallet, `Deposit` (`Achieved`); else the nearest hopper with scrap (`goals::nearest_store`), `Withdraw`; none → `"found no scrap to smelt"`, `Blocked` | `brain/goals/smelt.rs` |
| courier | `HaulGoal` (a `Seek`): steel in the pack → the nearest steel rack; scrap → its own hopper (`Deposit`, `Achieved` either way); empty → the nearer of the nearest stocked pallet and the nearest stocked scrap bin, `Withdraw`; neither → `"found nothing to carry"`, `Blocked`. **The trips alternate of themselves**: unloading scrap at the factory leaves it beside the pallets, unloading steel at the warehouse beside the bins | `brain/goals/haul.rs` |
| menu | `now` (the rows' `JobDef::now`): `smelting steel` at the furnace, `fetching scrap, storing steel`; `carrying steel to the warehouse`, `carrying scrap to the factory`, `fetching a load` (from the pack) | `job.rs` |
| on screen | the hopper, pallet and rack fill like the scrap bin (`BinFill`, `fill_bins`); the furnace burns while worked (`used` strip, `"furnace"` emitter); the HUD's `steel  N stored` (the racks) | `game-screen`, `lighting` |

**In a district** (`map-generator`): the steel factory is a lot building
(`map/district/steel_factory.rs`) on the lot nearest the warehouse, 2 furnaces, 2 hoppers, 2 pallets (so 2 steelworkers and 2 couriers),
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

## The farm

The first link of the food supply chain: wheat, grown out in the wasteland and stored at the
district's farm. Nothing eats it yet.

| Layer | What | File |
| --- | --- | --- |
| soil | **good soil** is a terrain column, `Terrain::fertile` (`meadow` only); a crop goes only in a **plot** (`map::is_plot`: both coordinates even), so no two crops touch. `Map::can_sow(cell)` = a plot of good soil with nothing in the way **and the eight cells round it open** — so a crop never closes a gap between a lake, a heap or a wall, and never shuts anybody in or walls a way off; a map with nothing round it has no sowable plot on its edge. A crop **blocks its cell** like a heap (`Map::plant` / `unplant`, the map's `planted` set; navigation counts it open ground, so it never invalidates) | `map/terrain.rs`, `map/mod.rs` (`map` skill) |
| fields | `sim::fields::Fields` (a `Facilities` field, `Think::fields`): every `Crop { cell, farm, sown, quality, tended }`, sorted by cell. `farm` is **the grain bin it was sown for** — a farmer's workplace — so a field outlives its farmer and the next one hired to the bin keeps it. **Grows by the clock**: `Crop::stage(now)` is `Seedling`, `Young`, `Heading` (in ear), `Ripe`, a stage every `STAGE_DAYS` (2), ripe after `GROWING_DAYS` (6); nothing is stepped. **Tended once a day** (`needs_tending`: not the day sown, not once ripe) for `TEND_QUALITY` (20) up to `BEST` (100); harvested, it yields `1 + quality / 40` sheaves (`Crop::harvest`, 1-3). `PLOTS` = 10 crops a farm | `sim/fields.rs` |
| kinds | `JobKind::Farmer` (workplace `FeatureKind::GrainBin` — a store of `ItemKind::Wheat`, site `"farm"`, HUD `wheat`; goal `GoalId::Farm` 🌱; `FARM_HOURS`; `"uniform farmer"`: denim dungarees over a red shirt), hired after the steel works; `ItemKind::Wheat` (0.5 kg, 1 l: twelve to a pack, 🌾) | `job.rs`, `feature.rs`, `item.rs` |
| tasks | `Sow { plot }` (`SOW_SECONDS` = 10 world min) beside a plot `can_sow` says yes to → `Effect::Sow`; `Tend { crop }` (15 min) beside a crop wanting it → `Effect::Tend`; `Harvest { crop }` (10 min) beside a ripe crop, with room in the pack for all it yields (`Inventory::room_for_many`) → `Effect::Harvest`. All named `TimedTask`s, all `Event::Worked`. The effects carry `when: Clock` | `brain/tasks/sow.rs`, `tend.rs`, `harvest.rs` |
| the world | `effect::apply`: `Sow` sows for the unit's job's workplace, refused where somebody stands in the plot (the occupancy) or lies dead in it (`Corpses`: the dead let their cell go), it is sown already, or the field is full (`"sowed wheat at x, y: 4 of 10 sown"`); `Tend` (`"tended the wheat at x, y: kept at 60%"`); `Harvest` out of the fields, the plot opened, the sheaves into the pack (`"harvested 3 wheat at x, y, kept at 100%"`) | `sim/effect.rs` |
| farmer | `FarmGoal` (a `Seek`), from what is true now: fewer than 10 crops → `nearest_plot` (ring by ring out from the field's crop nearest the bin, or the bin; a free plot nobody stands in or beside, no body lies in, reachable) and `Sow`; a ripe crop → `Harvest`, or the bin first if the pack has no room; a crop not tended today → `Tend`; wheat carried → the bin, `Deposit` (`Achieved`); nothing → `Blocked`, **without a log line** (it is most afternoons). So a crop harvested is sown again before the next is cut. No grain bin → `Blocked` | `brain/goals/farm.rs` |
| menu | `now`: `storing wheat` at the bin, `in the fields` | `job.rs` |
| on screen | crops drawn by stage (`game/crops.rs`), a click on one opens its panel (`game/croppanel.rs`); the grain bin fills like the scrap bin; HUD `wheat  N stored`; the statistics menu's `farm` and `fields` lines | `game-screen` |

**The week**: sown Monday morning, tended Tuesday to Saturday (quality 100), ripe Sunday while
the farmer is off, harvested (30 sheaves a field) and sown again Monday.

**In a district** (`map-generator`): the farm is a lot building (`map/district/farm.rs`) on the
edge lot nearest the good soil — 10 to 12 cells for 58 of 64 seeds — 8 grain bins along its side
walls, so 8 farmers. Seed 7, the farmers' needs and tiredness off, has all 80 crops sown by
14:43 on the Monday, through the gates and just past the wall. Two farmers going for the same
plot is the one race; a farmer passes over a plot somebody stands beside. A one-plot gap kept
between farms' fields was tried and dropped: farmers sowing side by side kept having ten
minutes' work refused when a neighbour's crop went in first.

Tests: `fields.rs` (stages, once a day, yield, harvest only ripe, a farm's ten, save); the
tasks' `Rig` tests; `farm.rs`'s goal tests on a meadow site (`a_week_on_the_farm_...`: Monday
sown, five days tended, Sunday off and ripe, Monday 30 stored and sown again; held off once
tended); `map::district::farm` (one farm of 8 bins on the edge, nearest the soil, enough soil);
`sim::tests::a_district_on_a_monday_sows_its_fields`;
`save::tests::a_farm_saved_mid_week_carries_on_exactly` (`testing::farm`, `testing::skip_to`).
QA `qa/farmer.json`: a meadow site with a grain bin, the job menu (`Mon-Sat`), ten crops by
Monday 14:00, the crop panel, Saturday in ear at 100%, Monday 30 in the bin and the statistics.

## The power plant

The district's power comes from its **power plant** (`map/district/power_plant.rs`, a lot building
on the edge: two `"generator"`s, the power sources, and one `"control panel"` between them;
`electrician`, `map-generator`). The panel is the power plant worker's workplace.

| Layer | What | File |
| --- | --- | --- |
| kind | `JobKind::PowerPlantWorker` (`"power plant worker"`, number 7): workplace `FeatureKind::ControlPanel` (`"control panel"`, beside, **no power**), goal `GoalId::Work`, `COMMUTE`, `WATCHES` (three to a panel), uniform `"uniform power plant worker"` (a teal boiler suit, a yellow bolt patch), hired after the farmers, before the king | `job.rs`, `feature.rs` |
| the work | `WorkGoal` as for a guard: beside its own panel, a `Work` stretch with `place: ControlPanel` — `"stand by"`, logged `"stood by an hour at the control panel at x, y"`. **It does nothing else yet**: the plant powers the district whether anybody is there or not | `brain/goals/work.rs`, `brain/tasks/work.rs` |
| menu | `now`: `on duty` at the panel, `on the way to the power plant` | `job.rs` |
| on screen | the panel's screens light while somebody stands by it (`used` strip, `game::props`); the generators' fans turn | `editor/props.rs`, `tools/power_plant_art.py` |

**Making them do something** (feeding the generators fuel, the plant going dark when nobody
keeps it): a goal of its own instead of `GoalId::Work`, and the supply would have to follow the
simulation — a generator switched like a box (`GameState::switch_box`'s path), never a map edit.

Tests: `job.rs` `a_power_plant_worker_keeps_the_control_panel_round_the_clock_and_is_hired_last`;
`tasks/work.rs` `every_job_stood_at_has_a_stretch_of_its_own` (every `GoalId::Work` job's
workplace in `STRETCHES`, no name twice), `standing_by_at_a_control_panel_needs_no_power`;
`goals/work.rs` `a_power_plant_worker_on_the_day_watch_stands_by_at_its_control_panel`;
`sim::tests::everybody_in_a_district_moves_into_a_bed_of_their_own_house` (the panel's three
watches 0, 8 and 16, inside the plant, hired last); `map::district::power_plant` tests. QA
`qa/power_plant.json`: one room powered by a generator (its lamp `expect_connected` 1 of 1, no
transformer — seen failing with the power rows upside down, a lamp with no power), three
arrivals hired to the panel in watch order, the job menu (`Mon-Sun`, `on duty`), `stand by at
the control panel` on the brains menu, the uniform on the clothing menu, an hour stood by, and
the evening watch on at 16:30 with the day watch `done for the day`.

## The king

The district's **king** — one, since a palace has one throne. What it decides comes next; for
now it is the job's frame: where, when, and how a king carries itself.

| Layer | What | File |
| --- | --- | --- |
| kind | `JobKind::King` (`"king"`, `"the king"`, number 8, **hired last**): workplace `FeatureKind::Throne` (`"throne"`, beside, no power), goal `GoalId::Work`, `KING_COMMUTE` (1.5 world hours: a slow walk to the middle of the district), `[COURT_HOURS]` (10-16 `MONDAY_TO_SATURDAY`: up at 7, in bed by 23, `Shift::is_bedtime`), uniform `"uniform king"` (a royal purple robe to the floor, an ermine cape, front and hem, a gold chain), `bearing: ROYAL` | `job.rs`, `feature.rs` |
| the work | `WorkGoal` as for a post: beside its throne, a `Work` stretch with `place: Throne` — `"hold court"`, logged `"held court an hour at the throne at x, y"`. Like the office it starts on arrival, from setting off at 08:30. Late for court it hurries, at its gait (`WorkRoutine`'s lateness: `GoalId::Work`) | `brain/goals/work.rs`, `brain/tasks/work.rs` |
| bearing | robes and crown on at every hour of every day (`always_in_uniform`), its own clothes back when let go; walks at `KING_GAIT` of its own pace, its hurry too — a starving king walks briskly at best | `job.rs`, `entity.rs`, `kinds.rs` |
| drawn | the crown is the paperdoll's **hat** slot, drawn over the hair (`characters::human`, `SLOTS` = 4, `HAT_SLOT`), in the world and the portrait; `actors::sync_clothing` redraws it with the outfit | `characters/human.rs`, `game/actors.rs`, `game/unitpanel.rs` |
| menu | `now`: `holding court` at the throne, `on the way to the palace` | `job.rs` |
| where | the **palace** (`map/district/palace.rs`), **the whole of the block in the middle of the district** (`palace::BLOCK`; `District::palace`, not a lot building), `wall red` and `floor white`, its door onto the main street, a red carpet from it to the throne in the middle of the far wall (`map-generator`) | |

**The decisions, next**: a goal of the king's own in place of `GoalId::Work` (a new `GoalId` row
and the job row's `goal`), or a routine only a king's brain raises (`WorkRoutine` reads the job;
`RoutineCtx::job` has the kind) — the brain is where it goes, never a Bevy system.

Tests: `job.rs` `the_king_holds_court_at_its_throne_in_its_crown_and_walks_slowly`;
`goals/work.rs` `the_king_holds_court_beside_its_throne`; `kinds.rs`
`a_king_wears_its_robes_always_and_walks_slower_than_it_would` (seen failing with the robes on
shift only, and with no gait set on hire); `save::tests::a_king_saved_on_the_way_to_court_walks_on_at_its_stately_pace`
(seen failing with the gait not set again on load); `characters::human`
`the_kings_robes_come_with_a_crown_on_top_of_the_hair`; `map::district::palace`;
`sim::tests::everybody_in_a_district_moves_into_a_bed_of_their_own_house` (one king, in the
palace, hired last). QA `qa/king.json`: a throne room, the arrival hired as the king, the job
menu (`10:00-16:00`, `Mon-Sat`, `off until 10:00`), `wearing: uniform king` at 08:00, `60% of
its pace` on the debug menu, `hold court at the throne` and `holding court`, a close-up of the
crown over the hair, an hour of court held, `done for the day` at 16:30 still crowned (seen
failing with the robes on shift only).

## The playzone

The district's arcade: **one central computer, six arcade cabinets, four administrators** — and
the cabinets work only while one of them is running the playzone.

| Layer | What | File |
| --- | --- | --- |
| kind | `JobKind::PlayzoneAdministrator` (`"playzone administrator"`, number 12, **hired last**): workplace `FeatureKind::PlayzoneComputer` (`"playzone computer"`, beside, **needs power**), goal `GoalId::Work`, `PLAYZONE_COMMUTE` (2 world hours, like a guard's: the playzone is in the middle of the district and the last hired live up to two hours' walk away), `PLAYZONE_SHIFTS`, uniform `"uniform playzone administrator"` (a purple polo, a neon cyan tie), `now`: `running the playzone` / `on the way to the playzone` | `job.rs`, `feature.rs` |
| the work | `WorkGoal` as for a desk: a `Work` stretch with `place: PlayzoneComputer` — `"run the playzone"`, logged `"ran the playzone an hour at the playzone computer at x, y"` (`STRETCHES`). The stretch starts on arrival, from the commute; running the playzone starts at the shift's hour | `brain/goals/work.rs`, `brain/tasks/work.rs` |
| **running it** | `playzone::runs(job, cell, think)`: its workplace is a playzone computer, **the shift's hours are on** (`Shift::is_working_hours` — the commute is not duty), it stands **beside the computer**, and the computer works (powered). `Brain::react` asks `Effect::Operate { at }` of the world **every tick** that holds, whatever its task — so there is no gap between two hourly stretches, and one who steps away, goes off shift, is let go or dies simply stops saying so | `sim/playzone.rs`, `brain/mod.rs` |
| the world | `Playzones` (a `Facilities` field, `Think::playzones`, `GameState::playzones()`): every cabinet's nearest computer (fixed by the map) and the computers being run as the last tick ended. `world_step` clears it (`begin_tick`) and grants this tick's `Operate`s (`run`); `Playzones::runs(cabinet)` is what a brain asks. `testing::World::step` does the same | `sim/playzone.rs`, `sim/effect.rs`, `sim/mod.rs`, `sim/testing.rs` |
| the cabinets | `FeatureKind::Arcade` (`"arcade cabinet"`, beside, needs power). `PlayGoal` offers one only while `Playzones::runs` (`goals::nearest_in_reach_where` / `choose_nearest_where`), as the nearest of computer, TV and cabinet; `UseArcade` (`ARCADE_SECONDS` = 10 world minutes, a third of a go at a computer, the same `Event::Entertained`) fails any tick the cabinet is not powered **or not run**, so a game the administrator cuts short is no fun at all. Logged `"played the arcade cabinet at x, y"` | `brain/goals/play.rs`, `brain/tasks/use_arcade.rs` |
| where | the **playzone** (`map/district/playzone.rs`), a lot building on the residential lot nearest the district's middle: `wall purple`, `floor colorful`, 6 cabinets along the side walls (every other row), the computer in the middle of the far wall (`map-generator`) | |

**Who gets which shift**: the first arrival with no earlier job is the first group's day
administrator (Mon, Wed, Fri, Sun, 10-16), the next the second group's day administrator (Tue,
Thu, Sat), then the two evening shifts. A playzone with one administrator hired is open from ten
to four on its group's days.

Tests: `job.rs` `a_playzone_is_run_every_day_from_ten_to_ten_by_two_groups_on_alternate_days`
(every day: a day and an evening shift, one administrator on at every quarter hour from ten to ten
and none outside), `four_administrators_share_one_computer_two_shifts_starting_at_the_same_hour`,
`a_workplace_saved_with_no_days_takes_the_days_of_its_workers_shift`; `playzone.rs` (nearest
computer, only a computer can be run, save); `goals/work.rs`
`a_playzone_administrator_runs_the_playzone_from_ten_to_four_and_the_cabinets_work_just_then` (every
tick of the shift, none outside — seen failing with `runs` always false), `an_administrator_off_duty_leaves_the_cabinets_dark`,
`an_administrator_away_from_the_computer_does_not_run_the_playzone`; `goals/play.rs` (plays a
working cabinet; a dark one is no place to go; the administrator leaving ends a game in progress;
the nearest of cabinet and TV, a dark cabinet not in the running); `tasks/use_arcade.rs`;
`sim::tests::a_district_s_playzone_is_run_from_ten_to_ten_and_dark_outside_those_hours` and the
four-administrators arm of `everybody_in_a_district_moves_into_a_bed_of_their_own_house`;
`save::tests::a_playzone_saved_with_games_being_played_carries_on_exactly` (seen failing with
`playzones_run` not restored); `game/unitpanel.rs`; `map::district::playzone`. QA
`qa/playzone.json`: a powered room with a computer and three cabinets (a crate on the box so no
electrician is hired for it), the first arrival hired as the first group's day administrator, the
second let go and bored, the job menu (`10:00-16:00`, `Mon, Wed, Fri, Sun`), the uniform, the
administrator at the computer at ten, a game played by half past two, `done for the day` and the
cabinets dark at half past four, and the resident still bored at nine at night (seen failing with
the cabinets never run).

## The cleaner

The district's dirt (`dirt` skill) is kept down by its cleaners. The frame is a job like the
rest; what it does is the `dirt` skill's.

| Layer | What | File |
| --- | --- | --- |
| kind | `JobKind::Cleaner` (`"cleaner"`, number 17, **hired last**): workplace `FeatureKind::District`, goal `GoalId::Clean` (🧹, number 25), `CLEANER_COMMUTE` = 0 (nowhere to go: it starts where it is), `CLEANER_SHIFTS`, uniform `"uniform cleaner"` (a teal coverall, a yellow hi-vis band, yellow rubber gloves), `now`: `keeping the district clean`, wage 40 | `job.rs`, `economy.yaml` |
| workplace | `FeatureKind::District` has no prop (`prop()` says `"district"`, as the electrician's `DistributionBox` says `"distribution box"`). `Features` puts **one** at the public passable cell nearest the site's middle when the site has `TOWN_HOMES` (20) homes or more — every generated district, never a test room — so its four shifts are four workplaces (`Workplaces`, told apart by start and days as the playzone's are). Logged `"hired as a cleaner at the district at x, y, 07:00-15:00"`. A cleaner never goes there | `feature.rs` |
| shifts | `CLEANER_SHIFTS`: 07-15 `FIRST_GROUP`, 07-15 `SECOND_GROUP`, 15-23 `FIRST_GROUP`, 15-23 `SECOND_GROUP`, handed out mornings first. Every day one morning and one evening cleaner; never two on at once, so no two race for a stain | `job.rs` |
| goal | `CleanGoal`: the look every 27 minutes on the tick it is due (`GoalExecutor::keep_up`, brain step 3b), the nearest found cell mopped next, a walk round with none. **No `Blocked` for an empty district**: it roams, on duty, until the next look | `brain/goals/clean.rs` |
| task, world | `Task::clean` (`CLEAN_SECONDS` 5 world minutes, `Event::Worked`), `Effect::Clean` | `brain/tasks/clean.rs`, `effect.rs` |

Tests: `goals/clean.rs`, `tasks/clean.rs` (`dirt` skill, "Tests");
`sim::tests::everybody_in_a_district_moves_into_a_bed_of_their_own_house` (four cleaners, the
two groups' morning and evening, hired last; seen with the residents to spare).

## Saving (`savegame` skill)

- `pb::Brain.job` (tag 14, `pb::Job`: `JobKind` enum — `OfficeWorker = 0`,
  `ScrapCollector = 1`, `Guard = 2`, `Undertaker = 3` — workplace cell, start, end, days). Absent = unemployed, which is what
  every older save reads as. `Job::save`/`restore` (refuses a broken shift or unknown kind).
- `pb::World.workplaces` (tag 11, `pb::Workplace { desk, worker, start, days }` — a bin or post is a
  "desk" there; `start` (tag 3, optional) is the shift's start hour, absent in an older save =
  office hours, 9; `days` (tag 4, optional) is the shift's days mask, which tells a playzone's two
  day shifts apart, absent in an older save = the days of the shift its worker keeps,
  `Workplaces::settle`), `Workplaces::save`/`restore` (one workplace to one worker, one worker to a
  workplace; a start past 23 or days of none refused), restored after `homes` in `sim/saving.rs`,
  then **cross-checked** against the entities both ways (cell, start and days).
- `pb::World.playzones_run` (tag 16, `repeated Cell`): the playzone computers an administrator
  was running as the tick ended (`Playzones::save`/`restore`; refuses a cell that is no playzone
  computer). **Everything else of a playzone is the map's.** Absent in an older save = every
  playzone unattended until the next tick.
- `pb::World.crops` (tag 14, `pb::Crop { cell, farm, sown, quality, tended }`):
  `Fields::save`/`restore`, which **plants the map again** (a save's chunks keep the ground as
  found) and refuses a crop where none could be sown; `sim/saving.rs` then refuses one sown for
  a farm with no grain bin.
- `pb::World.bins` (tag 12, `pb::Bin { cell, stored }`), `Bins::save`/`restore`, restored after
  the fridges; refuses a cell with no store or a different number of stores. Which kind each
  is, is the map's. Empty in an older save, which is a map with none.
- `WorkRoutine::on_shift` is saved as the routine's `committed`; `WorkGoal`, `CollectGoal`,
  `BuryGoal`, `SmeltGoal`, `HaulGoal` and `FarmGoal` are a `pb::Executor::Seek`; the tasks save
  by name (`TimedTask`: `"work"`, `"keep watch"`, `"stand by"` (the `Work` task's `STRETCHES`), `"deposit"`, `"salvage for work"`,
  `"withdraw"`, `"smelt"`, `"sow"`, `"tend"`, `"harvest"`...), and still read the tags the builds before wrote them under. Goals and jobs are
  saved as their catalogue numbers (`GoalId::Collect` = 9, `JobKind::Guard` = 2...).
- `ReadyRoutine` saves `ready` as its `committed` and its list as `pb::RoutineState.pending`
  (tag 3, a bit per `READY_NEEDS` place; 0 for every other routine, refused on one); a save
  with a bit past the list, or ready with a list, is refused.
- The routines were appended (`WorkRoutine`, then `ReadyRoutine`), so `Brain::restore`
  accepts a saved routine list that is a **prefix** of the brain's (old saves,
  `format1.savegame`); an executor a save does not mention (`CollectGoal` in an older one)
  starts fresh.

## The job menu (`src/game/unitpanel.rs`)

`job` on the unit bar opens `Menu::Job`: a live `Readout::Job` block from
`job_block(entity, clock)` — `job`, `at` (`computer at 12, 4`, `scrap bin at 30, 2`), `hours`,
`days`, `now` — while the job's goal is in charge, what the job's row says
(`JobDef::now(at_work, pack)`: `working`, `on the way to work`, `collecting scrap`, `storing
scrap` while at the bin, `on watch`, `on the way to the gate`...), `getting ready: <goal>` while
a collector sees to a need before a trip (`GameEntity::is_getting_ready`), and otherwise `on a
break: <goal>`, `off until 09:00`, `off until 00:00` for the night watch once its watch is over, `done
for the day`, `weekend`, `day off` —
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
- `bins.rs`: every bin starts empty, sorted; add and total; every store holds its own item; save
  round trip and refusals.
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
  office worker; let go and hired at another bin, it stores in the new one; out from its bin
  unless ready already; and **getting ready** on the yard with a fridge, a TV and a toilet
  (`yard_to_get_ready_in`, every need on): drink, meal, TV and toilet all before the first
  piece, none between it and the bin, set out with every need within 2 of 85%, and the next
  trip got ready for too.
- `brain/routines.rs`: on shift only with a job; a collector's shift is `Collect`, never
  `Work`; ordering against every need; exhausted ties go to sleep; the night watch goes to bed
  at 14:00 rested and is not in bed at 02:00, when the office and the unemployed are. Getting
  ready (`Readiness`): every need past 85% raised in list order over work, toilet last;
  topping up once stopped (fun only to 85%); nothing joins the list; ready lasts the walk out
  and ends out there; starts beside the bin or back in with a half trip, nowhere else; a need
  held off is not waited for; only a collector on shift, only processes running; the save
  round trip and its refusals.
- `game/unitpanel.rs`: `getting ready: drink` on the job menu.
- `save::tests::a_collector_saved_while_it_gets_ready_for_a_trip_carries_on_exactly`.
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

**The `add-content` skill has the whole touch list.** In short:

1. **A `JobKind` row** in `job.rs` — on the end, which is also the end of the hiring order — with
   its workplace feature, goal, commute, shifts (several for a workplace kept round the clock),
   uniform name and `now` line.
2. **A workplace** that is a new prop: a `FeatureKind` row (`sim/feature.rs`) and the prop's map
   rows (passability, power, art); in the district, a lot building with that many of them
   (`map-generator`).
3. If the work is "stand beside a prop for an hour", `GoalId::Work` and **a row in
   `STRETCHES`** (`brain/tasks/work.rs`: the workplace, what a stretch there is and was — a test
   fails without it). If not, **a goal of its own** (a `GoalId` row, its file) and tasks
   (`tasks!`); what it does to the body is an `Event` the processes decide on.
4. **The uniform's PNG** (`tools/human_art.py`).

Hiring, `WorkRoutine`, the save, the uniform, the job menu and the statistics menu need nothing.

## Payroll and the accountant

JobDef.paperwork is Payroll for the office, Books for the accountant, None otherwise.
Office workers process owed accounts at powered desks, five world minutes each.
The accountant (job 13, hired last) uses the powered accountant computer beside the
palace throne, Mon?Fri 09?17, a two-hour commute, and closes finished daily books.
Wages are $60 office/accountant/scrap/farmer, $100 king and $40 all other jobs.
Timesheets, care, tax and the job panel cash readout: see ../economy/SKILL.md.


## Shop staff and shared courier dispatch

See the [store contract](../store/SKILL.md) for the complete supply chain.
`Shopkeeper` (18) uses a `shop counter`, four daily six-hour shifts, and
`ShopGoal`: check the nearest superfridge, call at its low-stock threshold,
then mind the counter for fifteen world minutes. `StoreCourier` (19) uses a
`superfridge`, 08?16 every day, and `HaulGoal` for food-factory transport
between dispatches. Both have named $40 wages and uniforms.

`JobDef.courier` marks steel, food and store couriers as dispatch candidates.
A free courier on duty may be assigned `DeliverGoal` instead of its ordinary
hauling, fetching one warehouse meal for the store. Normal needs interrupt;
the assignment and carried goods survive, and work can finish past shift end.
Deliveries count toward timesheets. Unavailable couriers release their orders;
full reserves keep excess food in the pack, and hauling returns orphaned meals
to a warehouse meal rack even for a steel courier. The job panel describes an
active dispatch as delivering food to the store.
