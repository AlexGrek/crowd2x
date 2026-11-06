---
name: training
description: Soldiers and their training ground in crowd2x - the king's pick of a ground 20 to 30 cells past the district's wall (Paperwork::Decrees, Task::decree, training::ground_due, choose_ground, Effect::DecreeGround), sim::training (Training, the lap of 16 waypoints round the middle, next_waypoint), the soldier job (10, JobKind::Soldier, no barracks: bookkeeping cells on the roll) and the commander (JobKind::Commander, command post) - on demand, 08-15 Monday to Saturday, low pay, camouflage - DrillGoal and Task::drill (soldiers walk circles round the commander, who stands in the middle), getting ready where they live, running from deathdogs like anybody outside, the ground drawn (game::training), the find menu, saving and the tests. Maps every concept to the file that owns it. Use when changing soldiers, the commander, the training ground, the drill, or the king's picks beside the outposts.
---

# Soldiers and the training ground

The king picks a **training ground** out in the wasteland; ten **soldiers** walk circles round
its middle, where their **commander** stands, seven hours a day. Nothing is fought yet:
a soldier that sees a deathdog **runs**, as anybody outside does. Plain simulation, saved,
deterministic, a copy of the builders' pattern (`outposts` skill) with no building in it.

## The ground (`src/sim/training.rs`)

| Concept | What |
| --- | --- |
| `Training` | `Facilities::training`, `Think::training`, `GameState::training()`: the middle of the ground (`center()`, `None` until picked), the day it was decreed, the last day the king found nowhere (`fruitless`). `wants_a_ground(day)`: none yet and not looked in vain today. `found(map, center, outposts, day)` (refused while there is one or where `is_clear` says no), `found_nowhere(day)`. **Picked once, for good.** |
| the lap | `ring(center)` — `LAP` = 16 waypoints on a circle of `RADIUS` = 4 cells, clockwise on screen (`LAP_OFFSETS`, integers, no float rounding); `footprint(center)` — the square of `FOOTPRINT_RADIUS` = 5 round it (the circle and a cell). `next_index` / `next_waypoint(center, here, slot, soldiers)`: the waypoint after the one `here` is nearest, or — with the lap more than 2 cells off — its own `slot` of the `SOLDIERS`, so they come on spread round the lap. **The lap keeps no state**: where a soldier is on it is where it stands |
| the decision | `ground_due(think)`: no ground, not looked in vain today, and a roll of soldiers on the map. The king's `Paperwork::Decrees` covers both picks: `WorkGoal::plan` queues `Task::decree` while `outposts::decree_due || training::ground_due`; at the end of its half hour `Decree` asks `Effect::DecreeGround { at, day }` **first** if the ground is due (it costs nothing and the soldiers wait on it), else `Effect::Decree` for an outpost |
| `choose_ground(map, outposts, near)` | the clear spot (`is_clear`) nearest, manhattan, to `near` (the commander's cell on the roll), the lower cell on a tie. Pure; one scan of the band round the site, once a game |
| `is_clear(map, center, outposts)` | every cell of the footprint `FROM_WALLS` = **20 to 30** cells from the site (`distances`, as the outposts'), `Map::can_build`, not good soil, `Navigation::is_outside` (no lake pocket), clear of every outpost's footprint. The world asks again in `found` |
| the world | `effect::decree_ground`: from the king alone; logs `the king picked a training ground for the soldiers at x, y` / `the king found nowhere to train the soldiers` (important) |
| saving | `pb::World.training` (tag 33, `pb::Training { center, decreed, fruitless }`), restored right after the outposts; `Training::restore` refuses a ground no ground could be at. Absent in an older save: none yet |

## The jobs (`src/sim/job.rs`)

| | `Soldier` (23) | `Commander` (24) |
| --- | --- | --- |
| workplace | `FeatureKind::Ranks` (37) — **`training::SOLDIERS` = 10** cells on the roll | `FeatureKind::Command` (38) — one cell |
| goal | `GoalId::Drill` (36, 🎖) | the same |
| shift | `SOLDIER_HOURS` = **08-15 Monday to Saturday** (seven hours; Sunday off) | the same |
| commute | `SOLDIER_COMMUTE` = **3 hours** (up at five: they live anywhere in the district, the ground is 20-30 cells past the wall) | the same |
| `on_demand` | **true**: on duty only once the king has picked a ground (`goals::build::has_work`); without one it is as good as unemployed (a care day, as a builder's with nothing to build), still a soldier | the same |
| `goes_out_for` | `Some(ItemKind::Food)` — **gets ready, wherever it is, before every trip out** (`ReadyRoutine`, which for a drilled job does not need a start place: needs to 85%, a meal and a drink in the pocket, eaten where it stands out there), `"ready to go out for food"` | the same |
| wage | **$30** (`economy.yaml`, the lowest), a shift worked | $50 |
| uniform | `"uniform soldier"`: **camouflage** (olive, dark green, tan and brown blotches), black boots | `"uniform commander"`: the same camouflage with gold shoulders and a gold belt |

**There is no barracks and nothing is built for them.** The workplaces are bookkeeping cells, as
the cleaner's district is: `Features::with_properties` takes the **public, open (no roof)
passable ground nearest the throne** (more than two cells from it): the nearest is the
commander's (`Command`), the next ten the soldiers' (`Ranks`). Nobody goes there to work; it is
only the cell a soldier is sent back in to from the wasteland. **Only in a town** (`TOWN_HOMES` homes or more) **with a throne** — a test room has
no soldiers, as it has no cleaners. Hired after the propagandist, soldiers before the commander
(`JobKind::ALL`'s last two).

## The drill (`DrillGoal`, `Task::drill`)

`brain/goals/drill.rs` — a `Seek` (saved as `pb::Executor::Seek`), planned from what is true now:

1. **Off duty** (`goals::build::on_duty`: shift due and a ground picked) **and out in the
   wasteland**: back inside, to its cell on the roll, before dark (`WorkRoutine` keeps the goal
   wanted until it is in: the on-demand rule shared with the builder). Inside, nothing (`Blocked`).
2. In the district: **too late to go out** (the walk leaves under `SHORTEST_DRILL` = half an hour
   of the shift) → nothing today. Not `ready_to_go_out` → it waits where it stands
   while `ReadyRoutine` takes it to the nearest fridge and toilet, then goes straight out.
3. **The commander**: `Task::drill(center, STAND_SECONDS)` — walk to the middle and stand an
   hour; logs `{uid} stood an hour at the drill at x, y`, `Achieved`.
4. **A soldier**: `Task::drill(waypoint, 0)` one waypoint at a time (`next_index`, skipping a
   waypoint somebody stands on, up to `LOOK_AHEAD` = 3); logs `{uid} drilled a lap round the
   commander` when it reaches waypoint 0. A failed march: a body in the way → a moment's wait,
   then the next waypoint worked out again; no way → held off.

`brain/tasks/drill.rs`: `Drill { cell, seconds }` (a `TimedTask`, saved by name `"drill"`): walks
to `cell` (the route asked once) and stands `seconds` there; **every tick is `Event::Worked`** —
seven hours of marching tire like a shift. The job menu's `now`: `drilling` / `commanding the drill`
; `waiting for work` while due with no ground.

**Not fighting.** `FleeRoutine` (3.0) is over everything: a soldier that sees a deathdog (or somebody
running from one) panics and runs for the way in at a deathdog's speed (`deathdogs`). The
drill runs by day (08-15, home before dusk) so it rarely happens.

## On screen (`src/game/training.rs`)

`TrainingPlugin`: once the ground is picked (or a game with one is loaded) sprites are put on
the sixteen waypoints (`drill_mark.png`, a chalk dot on trampled earth) and the middle
(`drill_centre.png`, a chalk cross), flat on the ground (`DEPTH` -990 over the tile, under every
walker). Art: `tools/training_art.py` (`generate`, `preview`). The find menu's `training ground`
(`Search::TrainingGround`: the middle as a gold square, the camera off to it), and a soldier or
commander under the job finds; the statistics menu's workers per job lists both automatically.

## Tests

- `sim::training`: the lap's sixteen cells on a circle, `next_waypoint`, `choose_ground`
  (20-30 from the wall, nearest the roll), once and a day in vain, the save and its refusal.
- `tasks/drill.rs` (`Rig`): marching walks and tires, standing lasts and tires, no way fails.
- `goals/drill.rs` (`testing::World`, `parade`): a soldier visits every waypoint and logs laps;
  the commander stands in the middle most of seven hours; no ground picked → no drill; out when
  the shift ends → comes back in; Sunday.
- `sim::tests::a_district_s_king_picks_a_training_ground_and_the_soldiers_drill_on_it_the_same_monday`
  (seed 7: the ground picked, in the band, the ten and their commander on it); the district
  hiring test counts ten soldiers and one commander, the commander hired last.
- `deathdogs::tests::a_soldier_on_duty_who_sees_a_deathdog_runs_from_it_and_does_not_fight`
  (on duty, it wants the drill; with a deathdog in sight, `Flee`).
- `save::tests::a_district_saved_with_its_soldiers_drilling_carries_on_exactly`.
- `job.rs` (`soldiers_drill_seven_hours_a_day_but_sundays...`), `economy.rs` (the wages),
  `brain/goal.rs` (the ranked list), `game/training.rs` (the art).

## Limits

- Nothing is fought and nothing is trained *for*: the drill is its own end. Strength, ranks, a
  garrison for the outposts come next.
- A soldier gets a care day (as a builder with nothing to build) on a Sunday and on any day with
  no ground: `Timesheet`'s on-demand rule counts days not on duty, not days not due.
- The ground is picked nearest the commander's post (the palace street), which is not
  necessarily the shortest way out of a gate: three hours' commute covers the worst.
