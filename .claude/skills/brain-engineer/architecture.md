# The brain and the body: architecture

How `src/sim/brain/` and `src/sim/biology/` are built. `SKILL.md` is how to extend them.

## The brain (`sim/brain/`)

What an entity does with all this is decided by a `Brain`, which runs entirely in
`GameEntity::react` — the only `&mut self` hook, after the whole crowd has moved — while
`think` stays what it was: walk the current route, arithmetic, parallel. Seven layers, each
talking only to the one below:

- **perception** (`brain/perception.rs`) — **who is standing in front of it**: a cone 60°
  either side of the way it is walking (`Heading`, one of eight, kept up every tick and kept
  when it stops), plus the cells at its elbows, out to `FAR` (8) cells; within `NEAR` (3) is
  `Range::Near`. Walls hide what is behind them — a Bresenham line over the *terrain*
  (`Map::sight`, a see-through bit per cell kept beside passability), which also refuses to
  squeeze between two walls meeting at a corner — and furniture and people do not. **It never
  scans the crowd**: the cone's cells are a static table per heading (~70 cells, nearest
  first, built once per process) — the offsets packed two bytes each, since every look
  reads them all, and each cell's line-of-sight checks in a parallel array that only an
  occupied cell touches — and a look asks
  `Occupancy` — the spatial index — who stands in each; an empty cell costs one bit of its
  occupied bitmap, and only an occupied one pays for a line of sight. Away from the map's
  edge a cell is one offset from the viewer's own index, with no bounds checks. At most
  `SIGHTINGS` (16), the nearest, handed back on the stack (`Sightings`): a unit keeps only its
  heading and how many it saw near and far, because **a look in a big crowd is mostly cache
  misses** on whatever the unit kept since its last one. A unit looks
  on its own `Priority::Med` beat, every 7th tick, from the same seeded `Schedule` as its
  background tasks, so a crowd's looking is spread over the ticks — `Brain::react` takes the
  tick's `Due` for this. Asleep (`Task::Sleep` current), its eyes are shut. A dog has no
  schedule and never looks.
- **attention** (`brain/attention.rs`) — **which of that is news**. A vision memory of
  `FACES` (32) faces — a 32-bit key folded from the id and a 16-bit quarter-second timer, so
  three cache lines a look — a sighting already in it
  only refreshes it, and one not in it is remembered and handed to the brain as a notice —
  so passing somebody twice is one meeting. A face out of sight for `FORGET_AFTER` (half a
  world hour) is forgotten and is a meeting again; a full memory gives up whoever has been
  out of sight longest. The one reaction so far: a **human** noticed is `Event::Met` to the
  body, which lifts satisfaction (below), and a count (`in mind ... met N` in the brains
  menu). **There is no log line per meeting** — in a crowd it would be most of the log and
  an allocation on most looks. While asleep it runs with nothing seen, so faces fade
  overnight. Its faces are boxed once at spawn: touched every 7th tick, inline they would
  bloat a `Human`, whose size is a test (`a_human_stays_small_enough_to_be_worth_a_thousand_of`).
- **memory** — a `BTreeMap<String, Recall>`, nearly empty: the one thing in it so far is which
  bed is a human's own (`HOME_BED`, written by the spawn pass). A `BTreeMap` so iterating it
  can never be a hash order. What a unit has *done* lately is a different memory, kept by
  the body because a task writes it every meal (`biology::Recollection`, under "Biology").
- **routines** (`NeedRoutine`, `SleepRoutine`, `StayBusyRoutine`) own **priorities and nothing else**.
  The list is zeroed every tick and each routine raises what it cares about
  (`Goals::raise_to` is a max, so two routines cannot undo each other). `NeedRoutine` is
  one routine built from a `Need` — `HUNGER` ("keep fed"), `THIRST` ("keep hydrated"),
  `BLADDER` ("stay comfortable"), `BOREDOM` ("keep entertained") — with hysteresis between a commit and a release
  threshold, every need on the same `/ 50` priority scale, and nothing wanted while the
  process behind the need is switched off. **Sleep is the routine that is not a `Need`**,
  because it reads the clock as well as a stat: by day it commits at 75 tired and lets go at
  10; by night it commits at 30 and *does not let go until morning*, holding a priority
  floor of 0.6 so somebody rested at one o'clock stays in bed — above wandering, below any
  committed hunger, thirst, bladder or boredom, so a sleeper gets up for the toilet and
  comes back. **There is no cap on how many a brain has**:
  `Routine` is an enum over one struct per routine type, `match`-dispatched like `Task`,
  and a brain holds them in a `Box<[Routine]>` built once at spawn — one allocation per
  unit however many routines, state inline and contiguous, no `push` so the order cannot
  change. A per-routine `Box<dyn>` would be dozens of allocations per unit and a pointer
  chase per routine per unit per tick, which is what a human with dozens of routines in a
  crowd of thousands cannot afford.
- **goals** — `GoalId` over a fixed array; the one on top is an argmax, not a sort. A
  `GoalExecutor` (`WanderGoal`, `EatGoal`, `DrinkGoal`, `RelieveGoal`, `PlayGoal`, `SleepGoal`) is a `Box` owned for
  the entity's life, so **its fields are its saved state** across being put down and
  picked up. It reads the body, hands and memory, and manages the task queue; it never
  changes the world itself. **One goal per need, each in its own file**, even where two
  plans look alike today (eating and drinking): the processes behind them will not stay
  alike. What they share — `stand_beside`, `PATIENCE`, `WAIT_FOR_A_GAP` — is in
  `goals/mod.rs`. A goal whose hand holds somebody else's item finishes it first, since
  taking needs an empty hand; without that, food taken just before thirst took over makes
  every drink fail forever. `SleepGoal` queues **one hour** in bed and reports `Achieved`; if
  the routine still wants sleep it stays on top and the next hour starts from where the unit
  lies, so how long a night is belongs to the routine, and an interruption costs nothing that
  was slept: a task dropped half done is told so (`TaskExecutor::interrupted`, before the new
  goal takes over) and `Sleep` reports the part of the hour it got. **A sleeping body ages
  slower** (`Brain::is_asleep` → `Biology::advance_asleep`): each process keeps its
  `Process::asleep_pace` — hunger and thirst half, the bladder 0.4, boredom none — because a
  sleeper that got hungry, thirsty and bored at a waking pace was out of bed every hour and,
  with an interrupted hour worth nothing, never rested (`qa/a_day_in_the_office.json` is what
  showed it). **Which bed:** a human has one of its own, handed out at spawn
  (`GameState::give_a_bed`: the nearest bed nobody owns, *only while one is free* — a `Homes`
  registry on `GameState`, written by the spawn pass alone and released on despawn) and
  remembered in its `Memory`. By default that is the one it goes to, and the only one. It takes
  **any other free bed, at random, only when critically tired** (`CRITICALLY_TIRED`, 90), and
  once lying in a bed stays in it. A human that arrives to find every bed owned has none, and
  stays up until it is critical. Nothing in a tick reads `Homes`: the unit remembers.
- **tasks** — a fixed, double-ended inline queue of `Task`, an enum over one executor struct
  per step (`MoveTo`, `TakeItem`, `ConsumeItem`, `UseToilet`, `UseComputer`, `Sleep`, `Wait`,
  `OpenFridge`, `CloseFridge`), `match`-dispatched so
  queueing one allocates nothing. **A task writes its `TaskResult`** (`InProgress`,
  `Executing`, `Failed`, `Success`), checks its preconditions every tick (`TakeItem` only
  from one step away), and applies what finishing means: food goes into a hand when a
  `TakeItem` ends, not when a goal hears that it did. A task never writes a stat — it tells
  the body what happened (`Event::Ingested`, `Event::Relieved`, `Event::Slept`), below.
- **actions** — pathfinding and timing only. `Action::walk_to` is the one place a far route
  is asked for.

The pipeline runs in the order specified and the order is the design: perception and
attention (when the unit's schedule says it is time to look); every
routine arranges the list; on a change at the top, the old goal's `deprioritized`, the
current task abandoned, the queue cleared, the new goal's `prioritized`; the goal on top's
`process` **only if the goal changed or no task is current**; then the current task's
executor. So a task that ends at tick N is heard by its goal at N+1, which decides — carry
on, retry, replan — before anything else starts: one tick between tasks, the price of the
goal having a say. A goal put down before it heard is handed that result in
`deprioritized`. A goal that returns `Blocked` is held off (`BLOCKED_TICKS`, doubling), and
`Achieved` does not hand over — if the routine still wants it, it starts again.

The routing under `MoveTo` is **two stages of one A\*** (`sim/path.rs`, asked twice with two
different predicates), in `sim/walker.rs`. The *far* stage plans against the terrain only,
once per walk, and its cost is bounded by a count of expansions rather than by hope — an
unreachable goal floods everything it can reach before it knows, so `FAR_LIMIT` turns a
walled-off room from a frame-rate cliff into a bounded miss. The *near* stage is the detour:
something was in the way, so `DETOUR_CELLS` of the plan are thrown out and rerouted against
the crowd as well as the terrain; no local way round fails the walk, and the goal decides
whether to wait for a gap (`WAIT_FOR_A_GAP`, up to `PATIENCE` times) or give up.

The search is 4-connected and movement is not: a walker leaves for the next cell as soon as
it is inside the current one, so the line it walks cuts corners — except the last cell,
which a walk ends in the *middle* of. `path.rs` says why corner cutting can never skip a
cell the search vetted.

**Features** (`sim/feature.rs`) are what props are *for*: a static `FEATURES` catalogue binds
a prop name to a `FeatureKind`, and `GameState::new` indexes the map's props by cell — and
indexes them again whenever a distribution box is switched (`GameState::switch_box`), the one
thing during play that changes which features work.
A name may appear more than once — `"fridge"` is both `Food` and `Water` — `"toilet"` is
`Toilet`, `"computer"` is `Entertainment` and `"bed 1"` to `"bed 6"` are `Bed`. A fridge never
runs out of food or drink, and is used from one of the four cells beside it — it blocks its
own. Adding a use for a prop is an entry there, a palette entry in `editor/props.rs` and a
`map::PROPS` entry (tests fail without them), and a goal that queues the tasks. **A feature
that does not work is not indexed**: `Features::from_map` takes the map's `Supply`, and a
fridge or computer with no power, or a toilet with no drain, is left out — a brain never walks
to one, exactly as if it were not there. If it needs power or water, add it to
`map::utilities::CONSUMERS`. An unpowered fridge is still in `Fridges` (its door opens), but
its compressor does not run: it starts at room temperature and stays there, and one that loses
power warms toward the room from wherever it was (`Fridges::set_power`). Because the index can
change under a goal's plan, a task that uses a powered feature re-checks it (`TakeItem` when it
starts, `UseComputer` every tick). The `map-layers` skill has the whole picture.

**Doors and property** (the `doors` skill has the whole of it): a walker whose next cell is a
door not fully open stands still, and every walk asks for the door ahead
(`TaskCtx::advance_action` → `Effect::Door`), so goals and tasks never mention doors. A
human's `Body::home` is the property its bed stands in, handed over with the bed; routes
plan only through doors it may open (`Think::is_passable_with`), and **any goal that picks a
destination must skip what is in somebody else's property** (`Think::may_use`) —
`nearest_in_reach`, `choose_nearest`, `SleepGoal`'s any-bed pick and `WanderGoal::pick` do.

**A fridge has state of its own: it is open or closed, and it has a temperature**
(`sim/fridge.rs`, `Fridges`, indexed the same way `Features` is — once, at `GameState::new`,
from the map's `"fridge"` props). The room is +24°C and a fridge cannot get colder than
+4°C; open, its temperature climbs toward the room exponentially (`5` world minutes to close
most of the gap) and, closed, the compressor pulls it back down (`30` world minutes) — never
past either end. **A unit opens a fridge before taking anything out of it and closes it
afterwards**: `EatGoal`/`DrinkGoal` queue `OpenFridge -> TakeItem -> CloseFridge` around the
existing take, and `TakeItem` refuses to start against a closed door (checked once, when the
action starts, not every tick — re-checking would fail every other unit mid-take the moment
anybody shut it). If a unit is put down between opening and closing — the bladder taking over,
say — the fridge **stays open and keeps warming** until somebody closes it: its own goal
closes it first if picked back up still standing beside it, and otherwise the next unit's
`OpenFridge` finds the door already open and that unit closes it when it is done. Since a
task may not write outside its own unit, opening and closing are requests — an `Effect`,
`GameState`'s fourth step, `sim::world_step` — not direct writes; see the `simulation` skill.

**A prop can show that it is being used.** A computer's screen is on for as long as
somebody is sitting at it, and dark otherwise — `game/props.rs` swaps the two strips of
its palette entry. **In use is usually a fact about the unit, not about the prop**: an
ordinary feature has no state of its own, which is what lets the feature index be built once
and read from every thread, so the question is asked of the crowd instead —
`GameEntity::interacting_with` names the cell whoever is mid-`Action::Interact` is using,
and a prop standing in one of those cells is in use. Only the units on the canvas are asked
(`VisibleCrowd`), since props are culled to the view too and whoever uses one is beside it
or in it, and nothing is asked at all when no prop on screen could light up. One seam is
left at the very edge: a computer at most half on screen, used from the cell beyond it by
somebody not yet drawn, stays dark until the camera moves another half cell. **The fridge is
the exception**: its door is a fact about `Fridges` itself, so an open-door strip would read
`GameState::fridges()` directly rather than asking who is interacting with it — not built
yet, since there is only the one `fridge.png`.

Every `FeatureKind` also has an `Access`: `Beside` (the fridge and the computer, touched
from next to them — a computer is a desk with chairs on four sides, so nobody queues for
one) or `Entered` (the toilet and the beds, used by walking *into* them). An `Entered` cell stays impassable in
`map::PassabilityMap` — nobody routes through it, and an ordinary walk refuses it exactly
like a wall — but `sim::move_step` lets a `Move` straight onto it through to `Occupancy`,
which is the taken/free state: whoever is standing there holds the claim, and the cell is
free again the moment they leave, the same bookkeeping any other cell already gets from
ordinary movement. A task gets there by starting an `Action::enter`, which sets the walker's
route directly rather than asking for one — the one cell it targets is exactly the one a
search would refuse, and by the time it is asked for it is always a single step away.
`UseToilet` is the pattern for a task built on `Entered` access: it starts by entering, and
once `ctx.here()` is the feature's own cell, carries on exactly as a `Beside` task would.
`Sleep` is the second, and is that shape with a bed for a toilet.

When `UseToilet` fails because the cell was already taken, `blocked_by` names the occupant —
the same signal a `MoveTo` refused by a body in the way carries — so `RelieveGoal` treats the
two failures alike: wait it out (`WAIT_FOR_A_GAP`, up to `PATIENCE` times), then give up and
be held off by `BLOCKED_TICKS` like any other goal that found itself impossible.

## Biology (`sim/biology/`)

What a body does *by itself* — getting hungry, getting thirsty, a bladder filling — is a
**process**, and `Biology` is a unit's `Stats` plus its processes. Plain Rust, inline in
`Human`, `Copy`; a dog has none.

- **One writer.** A stat is changed by a process and nothing else: `Stats`' setters are
  private to `biology`, so the compiler holds it. A task that finishes a meal says
  `Event::Ingested(item)`, and each process decides what that means — hunger falls by the
  item's `nutrition`, thirst by its `hydration`, and the bladder puts that hydration *on its
  way*. So eating something that also fills the bladder is a change to a process, not a hunt
  through every task that consumes anything. An item says what it is made of, not what it
  does to a body.
- **A process** is a struct in its own file implementing `Process`: `advance(stats, dt)` for
  time passing and `handle(event, novelty, stats)` for something happening to the body. It may keep
  state of its own — `Bladder` holds what has been drunk but has not arrived, filling at
  `FILLING_PER_SECOND` on top of the slow `BLADDER_PER_SECOND`. Processes run in `ProcessId`
  order through `&mut dyn` over `Biology`'s own fields: nothing boxed, nothing allocated.
- **Switches.** Every body has its own `Switches`, a bit per `ProcessId`, all on at spawn.
  **Off means the process does not happen in that body at all**: its stats hold still both
  ways, events reach it and do nothing, and `NeedRoutine` stops wanting the need it drives —
  otherwise a human with hunger switched off at 80 would go back to the fridge forever.
  Switched from code (`Biology::set_running`) or from outside the world with
  `Command::SetProcess`, applied in the spawn pass like a freeze. There is no interface for
  it yet.
- **Recent memory fades** (`biology/recollection.rs`, `Recollection`). Eating something with
  a flavour (`ItemKind::taste` above zero, so not water), a go at something entertaining and
  meeting somebody (`Experience::Company`) are each an `Experience` with a **familiarity**:
  one more each time it happens (a treat capped at `MOST_FAMILIAR`, 3; **company uncapped**,
  so the `n`th meeting in a row is worth `1/n` and a crowd's joy comes in no faster than it
  is forgotten — a floor under it pinned everybody in a crowd at 100), halving every `HOURS_TO_HALF_FORGET` (4 world hours) and forgotten
  outright below `IN_RECENT_MEMORY` (0.1), about thirteen hours after one go — "today".
  `Biology::handle` asks it before the event reaches the processes and hands each one the
  **novelty**, `1 / (1 + familiarity)`: a half for something done just now, never below a
  quarter for a treat. A process about the mind scales by it — `Fun` gives `AMUSEMENT * novelty`,
  `Satisfaction` a treat's worth times it — and one about the body ignores it, so a second
  meal of the same food fills exactly as much and is less of a treat. It is not a process:
  it changes no stat and cannot be switched off. Fading is a background task at `VeryLow`
  (below), so it runs every 37th tick and not every tick. When the experience was still remembered,
  `handle` returns it, and the task that caused it (`ConsumeItem`, `UseComputer`) logs
  `... had food in recent memory: 70% as good as fresh` (`tasks::report_recalled`).
- **Satisfaction** (`biology/satisfaction.rs`) is how a person's day is going: it drains
  from content to discontented in 48 world hours and is lifted by a treat — a meal by its
  item's `taste` (`TASTY`, 15), a go on the computer by `ENJOYMENT` (20), meeting somebody by
  `GLAD_TO_SEE` (5), each times its novelty — so the thickest crowd lifts it by under 1 an
  hour, less than half of what a day wears off. Nothing reads it to decide anything yet: it
  is a readout in the debug menu.
- **Background tasks** (`sim/background.rs`) are upkeep that runs **on a cadence, not on every
  tick**: a `BackgroundTask` names its `Priority`, `High`/`Med`/`Low`/`VeryLow`, every
  3rd/7th/17th/37th tick, and `run(elapsed)` is handed the world time since it last ran.
  **Every unit keeps its own time**: a `Human` carries a `Schedule` — its own seed, rolled
  last at spawn, and one countdown per priority that the seed starts at a different point
  of its period — so different units, and different priorities of one unit, do their
  background work on different ticks, and a crowd's cost is spread evenly over the period
  rather than spiking every 37th tick. `Human::react` ticks the schedule once and hands the
  `Due` it returns to `Biology::background`, which runs the body's tasks (so far only
  `Recollection`). Each countdown adds up the world time it was handed, so a run is owed
  exactly what the unit lived through — a first run two ticks after spawning gets two
  ticks — and a frozen unit's timers stop with it. Pick the priority by how stale the thing
  can go: a rate over hours is `VeryLow`; anything a decision reads the tick it changes is
  not background work. The debug menu shows each countdown (`background in`).
- **Adding a process** is a `ProcessId` variant, a struct and file, a field on `Biology`, and
  its slot in `Biology::parts`/`processes` in id order (a test fails if the slots and ids
  drift). Food reaching the bladder later is a new process between `Hunger` and `Bladder`,
  not a change to eating.

**The rates are in world time, and they are a person's**, which is what the time scale
above is for: full to starving in 8 hours, quenched to parched in 5, an untouched bladder
full in 6, fun draining from a great time to thoroughly bored in 10 and stamina from fully
rested to exhausted in 16 (a waking day, undone by 8 hours in bed) — plus `0.5` of a
bladder per point of hydration drunk, arriving over the half hour after the drink. So a
human eats three or four times a day, drinks rather more often, goes to the toilet after a
drink, looks for something to do about once a day and goes to bed at night; at 1x that is a meal
every couple of minutes of watching, and the speed control is for watching a day go by. Every rate is written as
`100.0 / (hours * HOUR)` in the file that owns it, so the number in the source is the
number of hours.

The doing is in world time too, converted at the point it is defined: 2 minutes to take
food out of a fridge and 15 to eat it, 1 to pour a drink and 2 to drink it, 5 on the
toilet, 30 at the computer, an hour at a time in bed (thirty seconds of watching, and eight of
them are a night) — the longest a go at anything lasts, since having a go at something is
what a person does when nothing else is pressing. Eating and drinking commit at 60 and
release at 25, the toilet at 70 and 10, boredom at 60 and 20; one go is worth 60 points of
fun the first time today (less while the last go is in recent memory, below), so a
thoroughly bored person has a second one the way a starving one eats twice.

**Everybody is born 70 to 100 percent satisfied in every need** (`Stats::random`,
`BORN_AT_LEAST_SATISFIED`): fed, watered, comfortable, entertained, rested and content, worn down by
the world from there. Rolling a need anywhere in its range put a quarter of every crowd
past the line where it goes looking for a meal or a bed on its first tick. It also means
the first hunger is hours of world away, which a test that waits for one has to allow.
