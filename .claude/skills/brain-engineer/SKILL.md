---
name: brain-engineer
description: Add and change what crowd2x units decide and do - the unit brain in src/sim/brain/ and the body in src/sim/biology/. Covers routines (what a unit wants and how badly), goals and goal executors (how a want is pursued), task executors (one small step like walk, take, consume, use the toilet, wait), actions (pathfinding and timing), features (what a map prop is for, e.g. a fridge is food and water), items, stats/needs such as hunger, thirst or bladder, and biological processes that change stats and can be switched on/off per unit. Use when adding a behaviour, need, goal, routine, task, action, item or prop use; when a unit gets stuck, dithers between goals, stands still, or replans too often; or when writing brain tests.
---

# Engineering the unit brain

Every unit's decisions live in `src/sim/brain/`, run from `GameEntity::react`. It is plain
Rust with **no `bevy::` imports**, and it is tested with `cargo test`, no `App`. [architecture.md](architecture.md)
("The brain") has the architecture. This skill is how to extend it without breaking the three
things it exists to protect: **bounded route work, reusable hot-loop storage,
determinism**.

Code templates for every extension point are in [recipes.md](recipes.md); read it before
writing any of them.

## The layers, and who may do what

| Layer | Answers | Reads | May change | Lives in |
| --- | --- | --- | --- | --- |
| perception | *who is in front of it?* | its heading, `Occupancy`, the terrain, a static cone table; every 7th tick | its own sightings | `brain/perception.rs` |
| attention | *is any of that news?* | the sightings, world time since the last look | its own faces; hands each new one to the brain, which turns a human into `Event::Met` | `brain/attention.rs` |
| routine | *how badly is each goal wanted, and how soon?* | biology (stats, switches), body, `Think` | the priority list and its urgencies only | `brain/routines.rs` |
| goal executor | *how is this want pursued?* | biology, hands, memory, `Think`, last task result | the task queue, memory, its own fields | `brain/goals/` |
| task executor | *how is one small step done?* | everything in `TaskCtx` | the action, hands, events to the body, and its unit's own `Effect` | `brain/tasks/` |
| effect / world step | *what does a task's request outside its own unit become?* | every entity's `Effect` for the tick | world state no single entity owns — a fridge's door, a door's leaf, a piece off a heap of scrap (into the pack), a packful into a scrap bin | `sim/mod.rs` (`Effect`, `world_step`), `sim/fridge.rs`, `sim/door.rs`, `sim/bins.rs` |
| action | *what is the body doing, how long for?* | walker, `dt`, move outcome | its own clock, the walker's route | `brain/action.rs` |
| walker | *how fast?* | its own pace, the urgency of the goal on top (step 7) | its hurry, eased over a watched second | `sim/walker.rs` (`Walker::hurry`) |
| feature | *what is this prop for, and does it work?* | the map's props and its `Supply` — at `GameState::new`, and again whenever a distribution box is switched | nothing | `sim/feature.rs`; power: `electrician` skill, water: `plumber` |
| item | *what can be held, what is it made of?* | — | nothing | `sim/item.rs` |
| inventory | *what is being carried, and does it still fit?* | its own slots | only a task writes it | `sim/inventory.rs` |
| process | *what does a body do by itself?* | its stats, **world** `dt`, events and how fresh each felt | **the only writer of stats** | `sim/biology/` |
| recollection | *how recently was this treat had?* | events, **world** time per run | its own familiarities; never a stat | `sim/biology/recollection.rs` |
| background task | *what upkeep can wait a few ticks?* | the time since its last run, when the unit's own `Schedule` says its `Priority` is due | its own state | `sim/background.rs` |

**Goals decide, tasks act, processes change the body.** A goal never writes the body or hands
(`GoalCtx` gives it read-only references for exactly this reason). Food goes into a hand when
a `TakeItem` *finishes*, not when a goal hears that it did. That is what makes it safe for a
goal to miss a result. A task never writes a stat either: it calls
`biology.handle(Event::Ingested(item))` / `Event::Relieved`, and the processes decide what that
means (the `Stats` setters are private to `biology`, so this is compiler-held).

## The pipeline (every tick, in this order)

```
0. perception.turn, and on the unit's Priority::Med beat (every 7th tick):
   attention.age (first: it is the cold memory) -> perception.look ->
   attention.take_in -> Event::Met per new human face, and a wanderer may
   ask that face for a chat (social::notice); then, awake, a word with
   somebody close by, perhaps (social::strike_up — small talk)
   (eyes shut instead while Task::Sleep is current)
   -> the nights' loss off what it thinks of everybody (Relations::pass_nights)
   then every tick: social::tend (a chat taken up, agreed or let go) and
   social::small_talk (small talk answered, kept up or over — no goal, no task;
   true on the tick it is had, and the brain tells the body Event::SmallTalk)
1-2. every routine arranges the list (zeroed first, so a routine raises EVERY tick)
3. Goals::settle: the best goal takes charge — unless the goal in charge HOLDS
   (below). top changed? old.deprioritized -> current task abandoned, walk halted
                -> queue cleared -> new.prioritized
3b. otherwise, a task going on: top.keep_up(ctx, current) — what will not
                wait for the task to end (a cleaner's look over the district every
                27 minutes); true abandons the task as a hand-over does, and
                process runs this tick
4. top.process(last)             ONLY if the goal just changed or no task is current
6. current task's executor runs  (pops the queue front if none is current)
7. walk.hurry(urgency of the top goal, dt)  (0 with no route: every walk starts at its pace)
```

This order has consequences:

- A task that ends at tick N is heard by its goal at N+1, and the next task starts at N+1.
  There is **one tick between tasks**.
- A goal that returns `Working` with nothing queued is asked again next tick. That is fine
  for a cheap miss and a disaster for anything that searched.
- **`GoalExecutor::keep_up`** (default `false`) is the one way a goal acts while its task
  runs, asked of the whole crowd every tick: keep it to a counter compared until it is due.
  `CleanGoal` is the only one that keeps up with anything (`dirt` skill).
- When a handover happens the tick after a task ended, the old goal never gets `process` for
  that ending. It gets it in **`deprioritized`** (`ctx.tasks.result()`, `ctx.finished`).
  `EatGoal` logs a meal there because the last bite is what makes hunger let go.

## What it is doing counts: the hold

**The goal in charge holds** (`goal::HOLD` = 0.3, `Goals::settle` → `holds_against`): a rival
takes over only when it is wanted by **more than `HOLD` over** the goal in charge — a tie with
the hold holds. So a meal is not dropped for a drink wanted a little more, a night in bed not
for a dog to feed, a chat not for the TV, and a shift (1.5) is left only for a need past
`CALLED_AWAY` = (`WORKING` + `HOLD`) × 50 = **90** — starving, parched, bursting — or for
danger (`FLEEING` 3.0 beats anything held). `Goals::best` stays the plain argmax (a fresh
choice); only `settle` holds.

- **Only while it is still wanted, not held off, and holds at all** (`GoalId::holds`): a goal
  its routine let go of (priority 0), one on a cooldown (`Blocked`), or one whose priority is
  NaN holds nothing. **The floors never hold** — `Idle`, `Wander`, a dog's `Follow`, a
  deathdog's `Prowl` — they are what a unit does with nothing to do.
- **Breaking in** (`Goals::break_in(goal, at_least)`): a routine whose whole point is to stop
  something going on raises its goal this way, and the hold does not keep it out — it takes
  over the moment it is wanted more at all. `ReadyRoutine` (getting ready beside the bin
  stops the shift it could never outweigh by the hold) and a dog's full bowel
  (`HeelRoutine`, over its nap beside its sleeping owner) are the only ones. A bit per goal,
  cleared with the priorities every tick, saved (`pb::GoalSlot.breaking_in`, tag 6) so a
  loaded brain settles exactly as the saved one would. Never for an ordinary want.
- Tests: `goal::tests` (holds against a little more, a tie holds, the floors hold nothing,
  let go / held off / NaN hold nothing, breaking in, the save), `routines::tests::
  a_shift_under_way_is_left_only_for_a_need_past_ninety`,
  `getting_ready_breaks_in_on_the_shift_it_could_not_outweigh_by_the_hold`.

## Where does my change go?

| I want units to... | Add |
| --- | --- |
| care about a new need | a **`Need`** for `NeedRoutine` (plus a stat and a **process** if the need is new) |
| react to who or what it sees | a reaction in `Brain::react` step 0 (the `take_in` closure, as `social::notice` asks a new face for a chat); a new kind of thing seen is a new source in `Perception::look`, never a scan of `entities()`; something about the one seen that only its own brain knows goes in a per-tick index the think or move step gathers (`Panics`, `Chats`, `Smalltalks`, `Bonds`), never a read of its entity |
| care about a time of day, a threat | a **routine** (file map and test list: the `add-routine` skill) |
| have a body change by itself, or react to what happened to it | a **process** in `sim/biology/` |
| do a new multi-step thing ("sleep in a bed") | a **goal** (`GoalId` variant + its own executor, one per need) |
| do a new kind of single step ("sit", "open door") | a **task executor** |
| show a new kind of body activity (animation) | an **action** variant |
| use a prop on the map | a **feature** entry (+ palette entry if the prop is new) |
| die of something new | make the **`Health`** process (`biology/health.rs`) hurt for it: `harm` adds a rate per need at its worst (`STARVING`, `PARCHED`, `SPENT`), `cause_of_death` names it. Health at 0 is death — `Human::react` checks it after `Biology::advance` (see "Health and death" below) |
| carry a new thing | an **item** variant with what it is made of (`nutrition`, `hydration`, `consume_seconds`, `mass`, `volume`), **and its art** in `characters/item.rs` — a texture or an emoji; `item::art` has no wildcard arm, so it will not compile without one |
| change *when* something is chosen | the routine's priority curve, not the goal |
| change *how* something is done | the goal executor, not the routine |
| hurry when something is urgent | `goals.hurry(goal, 0..1)` in the routine that raises it, beside `raise_to` — a `Need`'s `urgent_at`, or a rule like `WorkRoutine`'s lateness. Urgency is per goal, read only for the goal on top, and only changes walking speed |

Never add a Bevy system that decides something, a `Component` holding agent state, or a new
`Intent` variant for a behaviour. `Intent` stays `Idle | Move`.

**Whatever you add with state is saved** (`savegame` skill). A goal executor implements
`GoalExecutor::save`/`restore` (no default; reuse `pb::Executor::Seek`/`Fetch`/`Wander` or add
a variant), a task saves itself — `TimedTask` (a name, saved as `pb::TaskKind::Named`, no
schema change) or `SavedTask` — in its own file, an action
variant one in `Action::save`/`restore`, a routine with memory one in `Routine::save`/`restore`,
a process with state a field in `Biology::save`, a memory key nothing (memory is saved whole).
Every `save` destructures its struct exhaustively, so the build points at what was missed; the
side-by-side tests in `save::tests` catch what changes behaviour after a load.

## Danger: step 0's alarm and step 7's panic

After the look in step 0, `brain/alarm.rs::take_in` reads the sightings: a person outside
the site who sees a deathdog, or somebody in `Think::panics`, remembers it in
`Memory::spotted`; a deathdog marks the nearest person outside. Step 7 calls
`walk.panic(top == Flee)`. A wound is read before time passes in `Human::react`
(`Event::Bitten`, the `Health` process); death that tick is `"deathdog bites"`. Details in
the `deathdogs` skill.

## Health and death

`Stats::health` (0-100, born 70-100 like every need) is moved by the **`Health` process**
(`ProcessId::Health` = 6, `biology/health.rs`): it **drains** while a need is at the end of its
range — starving (hunger 100, dead in `HOURS_TO_STARVE` = 24 h from full), parched (thirst 100,
12 h), spent (stamina 0, 48 h), summed when several — **heals** in `HOURS_TO_HEAL` = 24 h while
hunger, thirst and tiredness are all below `WELL` (80), and holds still in between. Switched off
(`Command::SetProcess`), a body cannot die.

**Death** is the kind's: `Human::react`, after time passes, sees health ≤ 0, halts the walk,
marks `Body::dead` (saved, `pb::Body.dead` tag 6) and returns `Effect::Died { cause }`
(`health::cause_of_death`: `thirst`, `starvation`, `exhaustion`). The world step (`sim::died`)
frees its cell (the crowd walks over the body, and perception no longer sees it), its bed and
workplace, dismisses it, logs `"{uid} died of {cause}"` and files it in `sim::Corpses`. From then
on `think_for`/`react_for` skip it like a frozen unit — **for good**: no thinking, moving or
ageing — `current_goal`, `action_progress` and `interacting_with` are `None`, and the debug
fields say `condition: dead`. The game draws it turned 90° (`game-screen`). The undertakers
carry it away (`jobs`, "The undertaker"); `GameState::hire` refuses the dead.

Tests: `biology/health.rs` (heals, starving kills in a day, thirst faster, exhaustion, holds
still between); the sim test `the_dead_are_carried_to_the_crematorium_and_leave_the_world`.

## The mind

`Stats::mental_health` (0 broken, 100 well) is moved only by the **`Mind` process**
(`ProcessId::Mind` = 7, the last bit of `Switches`' `u8`; `biology/mind.rs`; off for dogs). It
is slow, in days, and it holds still asleep (`asleep_pace` 0). Its rates are points of 100:

| Pressure | Told by | Rate |
| --- | --- | --- |
| fun under `JOYLESS` (10) | the stat, in `advance` | down, whole to nothing in `HOURS_TO_BREAK_DOWN` (48 h) |
| fun at `CHEERFUL` (50) or over | the stat, in `advance` | up by what it lacks over `MENDING_HOURS` (48 h) an hour: fast from low, slow near whole |
| work done rested | `Event::Worked` (every job's work task) | up `PURPOSE` (1) an hour worked |
| work done past `STRAINED` (75 tired) | `Event::Worked` | down `STRAIN` (3) an hour worked |
| a scare | `Event::Frightened`, from brain step 0: not afraid before `alarm::take_in`, afraid after (`FEAR`), so once a fright, outside only | down `FRIGHT` (8) at once |
| a bite | `Event::Bitten` (also `Health`'s) | down `BITE_SHOCK` (0.3) a point of damage |
| a debt | `Event::Worried { debt, world_seconds }`, step 0's low beat, awake, while `cash < 0` (`Think::economy`) | down, by `debt / DESPAIRING_DEBT` ($250, capped at 1), at worst whole to nothing in `HOURS_TO_DESPAIR` (48 h); half of it balances cheer |
| the air | `Event::Smelled { value, world_seconds }`, the same beat | by `value / smell::MAXIMUM`, at most whole to nothing (or back) in `HOURS_OF_SCENT` (72 h) |

Mending in proportion to the lack is what makes a steady pressure *settle* rather than lose
the race to cheer: with `MENDING_HOURS` = `HOURS_TO_DESPAIR`, a cheerful mind under a debt
settles at 100 less the debt's share of `DESPAIRING_DEBT` ($50 owed, 80; $125, 50). A flat
mending rate (four days, nothing to whole) put a district's average at 94-99 within a week,
with only the evening guards (joyless through a 16-24 watch) and the frightened below 20.
The two beat events carry the world seconds the beat covers (`Due::elapsed(Priority::Low)`,
about 32), so they are rates like the rest. Smell used to be a whole point a beat, about 110 an
hour. It swung a district's average between 10 and 70 in a day and made the stat mean nothing.
Nothing reads mental health to decide anything yet; the unit panel and the lab show it. The
lab samples `mental_health.employed.avg`, `.jobless.avg`, `.in debt.avg` and
`mental_health.low` (under 20). Tests: `biology/mind.rs` (each pressure),
`brain::tests::a_debt_weighs_on_the_mind_through_the_low_beat`,
`deathdogs::tests::a_scare_costs_the_mind_once_a_fright_and_nothing_inside`,
`smell::tests::humans_breathe_scent_into_their_mind_by_its_strength_and_the_time_breathed`.

**Hospital patients** (`sim/hospital.rs`): under `ADMISSION_HEALTH` (90) a human is a patient
(`Medical::recovering`) until it is back to 100. A patient heals **only** by nurses' care in a
hospital bed: `Biology::advance_hospital` hurts and never heals. A district has eight beds
(`map/district/hospital.rs`, both side walls, each kept only if the room stays whole).
`Medical::arrange` (the `get well` routine) raises `Recover` to 1 and suppresses wandering,
play, salvage and every job's goal, so a patient is on sick leave. It suppresses **Sleep only
for a patient with a bed reserved**, because `HospitalRest` is its sleep. A patient waiting for
a bed sleeps as anybody does. Suppressing Sleep for every patient killed every bitten unit that
found the beds full: twelve in a month on seed 7, found by the lab (`lab` skill). Test:
`a_patient_with_no_bed_to_be_had_still_sleeps_rather_than_dying_of_exhaustion`.

## Rules that were each a bug, or would be

<rules>

1. **Never search inside a goal's `process` or a routine.** Queue `Task::move_to(cell)`
   or `Task::move_beside(feature)`. `Action::walk_to_any` starts the far search once per walk;
   `Walker::advance` continues its saved frontier when `FAR_LIMIT` or the tick's shared route
   budget runs out (`architecture.md`, "The route budget"). `BudgetExhausted`
   keeps the task executing, and must never become an unreachable-feature memory. Feature
   approaches try all free permitted sides, then occupied sides, before failing.

2. **Every `process` call ends in one of these:** it queued a task; it returned `Blocked`
   (expensive miss, so the goal is held off 32 ticks, doubling); it returned `Achieved`; or it
   returned `Working` after a *cheap* miss (a few random candidates landed on walls, or on
   cells `Map::navigation().may_reach` says there is no getting to — an O(1) component
   lookup, so ask it before routing to a random pick). Anything else is polled every tick.

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
   `ActionState::Finished`. `TakeItem` rechecks distance, an empty hand, and fridge power
   through completion. Its open-door check alone is a start condition: another unit
   closing the door must not interrupt an item already being taken.

8. **A task does one thing and never walks.** `TakeItem` from two cells away fails; the goal
   should have queued a `MoveTo` first. One task, one action.

9. **Keep routines, goals and task queues allocation-free.** Routing reuses generation-stamped
   search scratch and walker-owned far/detour buffers. Buffer growth and the owned continuation
   of an unusually large search can allocate; warm route reuse must not. Detours replace a
   separate prefix and never copy the untouched far-route tail.
   - `Task` and every task struct must be `Copy`; the queue is a fixed ring of 8, and `push_*`
     returns `false` when full (`#[must_use]` — write `let _ =` only when full cannot happen).
   - Goal and routine fields are inline.
   - A goal or routine may allocate a `Vec` in `debug_fields` only; route buffers belong to the walker.

10. **Determinism.**
    - Randomness comes only from `crate::sim::rng::tick_rng(ctx.body.uid(), ctx.think.tick)`.
    - No `HashMap` iteration (memory is a `BTreeMap`).
    - Routine order is fixed in `Brain::human()`/`dog()`; executors come from `GoalId`'s
      catalogue, in the order listed.
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
| `NeedRoutine(BOREDOM)` → play | `PLAYING = 0.12` however bored (`Need::at_most`), committed at `BORED` (60), released at `AMUSED` (20): over wandering and under a chat, so **fun never interrupts anything** — it is what a unit does with nothing better to do, never what takes it off something. No bedtime or shift cap is needed: everything else is over it. `Stats::boredom` is `fun` turned over — the one need that falls on its own. A chat (`CHAT_FUN` 20) and small talk (`SMALL_TALK_FUN` 2) are fun too (`Event::Chatted` from a `Talk` that ran its time, `Event::SmallTalk`); what fun does to `mental_health` is the `Mind` process's ("The mind", below) |
| `ChatRoutine` → chat | `CHATTING = 0.15` while a chat both sides agreed to (or this side has finished) is in memory: over wandering and a go at something, under everything else. The `relationships` skill |
| a pet dog's `HeelRoutine` → follow / sleep / defecate | `FOLLOWING = 0.2` while it has an owner (over wandering, which is what a stray does); `GoalId::Sleep` at 0.25 while the owner is asleep (`DogSleepGoal`); `SQUATTING = 0.3` once its bowel is `MUST_GO`, **raised to break in** so it gets up from a nap. `Brain::dog`; `GoalId::Follow`, `Defecate` have no catalogue executor. The `pets` skill |
| `KeepRoutine` → dog care | `DOG_CARE = 0.55` while the dog a person keeps is hungry (its bond): over wandering, a chat and an order (0.5), under a night in bed (0.6), every need committed to and a shift. The `pets` skill |
| `NeedRoutine(HUNGER)` → eat | `hunger / 50`: 1.2 at `PECKISH` (60), released at `SATED` (25) |
| `NeedRoutine(THIRST)` → drink | `thirst / 50`: 1.2 at `THIRSTY` (60), released at `QUENCHED` (25) |
| `NeedRoutine(BLADDER)` → relieve | `bladder / 50`: 1.4 at `BURSTING` (70), released at `RELIEVED` (10) |
| `SleepRoutine` → sleep | `tiredness / 50`, committed at `EXHAUSTED` (75) by day and `SLEEPY` (30) at night, released at `RESTED` (10) **and only by day**; at night — **the unit's own night**, `job::is_bedtime(job, clock)`: the clock's 22-06, or the 8 hours ending 3 before a worker's shift (the night watch sleeps 13-21), see `jobs` — the priority has a floor of `SLEEPY / 50` = 0.6, over wandering and under every committed need. The one routine that reads the clock (`Think::clock`) as well as a stat, which is why it is not a `Need`. `Stats::tiredness` is `stamina` turned over. *Which* bed is the goal's business: its own (`HOME_BED` in `Memory`, handed out at spawn to the nearest unowned bed), any free one at random only at `CRITICALLY_TIRED` (90), and **none at `COLLAPSED` (95)**: it sleeps where it stands (a `Sleep` task whose bed is its own cell), goes down mid-walk to a bed (`SleepGoal::keep_up`), and stays down a stretch at a time while past `EXHAUSTED`; logged `slept on the floor at` (`World::floor_sleeps`) and drawn lying with zzz like any sleeper |
| `WorkRoutine` → work / collect | a flat `WORKING = 1.5` on **the job's goal** (`JobKind::goal`: `Work` at a desk or a guard post, `Collect` for a scrap collector) while the job is due (`Job::is_due`: 09:00-17:00 Mon-Fri from 08:30 for the walk in; a guard's watch from two hours before), nothing without a job. Over every need at its commit level (1.2, a bursting bladder 1.4), under any need past 75 on a fresh choice — **but a shift under way holds** (`HOLD`) until a need past `CALLED_AWAY` (90); boredom never comes near; ties a nap at `EXHAUSTED` and loses it, because `GoalId::Work` and `GoalId::Collect` are the last two and ties go to the lower id. Reads the clock and the job, no stat, so no hysteresis. The `jobs` skill |
| `ReadyRoutine` → provision | the last thing on its list: once the needs are met, `GETTING_READY` less four places while the pocket is short of a meal and a drink (`goals::provision`). The `jobs` skill, "Getting ready" |
| `ReadyRoutine` → eat / drink / play / relieve | before a trip out for a job that goes out (`JobDef::goes_out_for`: the collector): `GETTING_READY = 1.6` less 0.01 per place on its list (drink, eat, play, toilet), **raised to break in** (`Goals::break_in`) so the shift in charge does not hold against it, so over work and under any need past 80, for each need it is seeing to until every one is 85% met. Starts beside the bin or back in with a half trip, keeps a list (no need joins it mid-way) and a `ready` latch until it goes out; tells `CollectGoal` through `GoalCtx::ready_to_go_out`; reads the pack through `RoutineCtx::inventory` and held-off goals through `Goals::cooldown`. The `jobs` skill, "Getting ready" |
| `OrdersRoutine` → salvage | `ORDERED = 0.5` while an order is remembered |
| `FleeRoutine` → flee | `FLEEING = 3.0`, urgency 1, over everything, for `FEAR` (30 world min) after `Memory::spotted` (a deathdog seen, somebody seen panicking, a bite). Flat out at `PANIC_SPEED` (step 7, `Walker::panic`). The `deathdogs` skill |
| a deathdog's `DepartRoutine` / `HuntRoutine` / `ProwlRoutine` | 2.0 by day / 1.0 (urgency 1) while its quarry was seen outside in the last 10 world min / `BUSY`. `Brain::deathdog`; `GoalId::Hunt`, `Prowl`, `Depart` have no catalogue executor |
| `Idle` | 0; wins only when nothing is wanted or everything is held off |

**Urgency is a second number, not the priority** (`Goals::urgency`, 0-1): how much of a
hurry the unit walks in while that goal is on top, up to `HURRY` (1.75x) its own pace. A need
past `URGENT` (75) — `(level - 75) / 25`; boredom never. (Urgency is not the hold: a need
past 75 is hurried for, and takes a worker off a shift under way only past 90.) Late for a desk, post or panel
(`GoalId::Work`, more than a cell away once the hours begin): `LATE` 0.5, rising to 1 an hour
late. Getting ready (1.6) and a shift on time (1.5) are wanted badly and are no hurry, which is
why urgency is not read off the priority. "How fast it walks" in [architecture.md](architecture.md).

Put a new need on the same 0–2 scale so that urgency is comparable across needs. Give a
routine **hysteresis** (commit at a high threshold, release at a low one, with a `committed`
field) whenever the stat it reads is changed by the goal it raises, or units will turn round
halfway there.

## Workflow

1. **Decide the layer(s)** with the table above. A new behaviour is usually a stat plus a
   routine plus a goal, reusing existing tasks. A new task or action is rarer.
2. **Write it from [recipes.md](recipes.md)**, in dependency order: stat/process/item/feature →
   action → task → goal → routine (`Need`) → registration (a routine in `Brain::human()`/`dog()`;
   a goal is registered by its `GoalId` row). The touch list for each kind of addition — a job, a
   lot building, an item, a feature, a goal, a task, an effect — is the `add-content` skill.
3. **Write unit tests alongside each piece** (below). Names are full sentences in snake_case;
   tests are inline `#[cfg(test)] mod tests`.
4. **Run `cargo test`.** The two determinism tests in `src/sim/mod.rs` are the gate on any brain
   change. If you added a goal, extend `determinism_survives_brains_and_biology_in_the_parallel_rounds`
   (or a sibling) so the new goal actually runs in it.
5. **Prove a new guard test fails** by breaking what it guards, run it, then restore. Tests in
   this repo have passed vacuously before.
6. **Add a QA test** if the behaviour is visible, copying `qa/eating.json`. **Assert what
   happened, not the tick it happened on**: a test that photographs an exact tick of an
   unrelated need breaks whenever anything else changes what a unit does, and such tests
   are deleted rather than patched (`qa/computer.json` and `qa/lighting.json` went that way
   when jobs arrived):
   - an inline map with the props;
   - `spawn`, `tick`, `expect_log`;
   - a `shot` of the brains menu;
   - `{"process": {"name": "hunger", "on": false}}` on the selected unit to switch the
     other needs off, and `{"key": "p"}` to pause, when the test needs an exact moment
     rather than "something happened eventually";
   - on a map with a computer, the arrival is **hired** to work at it, and from 08:30 its
     shift (1.5) beats most needs: `{"press": "job"}`, `{"press": "dismiss"}` on the selected
     unit takes the job away, as `qa/office_job.json` does.
7. **Run `uv run tools/qa.py`, then `qa/perf_simulation.json`.** Compare the 4000-humans median
   with the previous run on the same machine. The perf test runs only 200 ticks, so a behaviour
   that triggers later is not measured there; add a `measure` step to a test that reaches it if
   the behaviour does real work.
8. **Update `architecture.md` "The brain"** if the change adds a layer member worth naming (a new goal,
   task, feature kind or stat that moves).

## Testing tools that already exist

- **`crate::sim::testing::World`** — one unit, the real think / move-rules / react sequence.
  `World::new(map)`, `step(&mut entity)`, `log_contains(..)`, `lines()`, `meals()`, `drinks()`,
  `reliefs()`, `plays()`, `ctx()`, pub `dt`, `tick`, `occupancy` (claim cells to put bodies in the way).
- **`crate::sim::testing::{needy_human, prop_at}`** — `needy_human(cell, hunger, thirst,
  bladder)` so a test decides which needs press (it also sets fun to the top, so a test about
  hunger is not interrupted by boredom; `Human::set_fun` is how a test about boredom lowers it); `prop_at(&mut map, "toilet", cell)` puts a prop
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
- **Test hooks:** `Human::set_hunger`/`set_thirst`/`set_bladder`/`set_fun`/`set_carried`/`inventory_mut`,
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
| Stands still, goal `idle`, `held off wander …t` | wander was `Blocked`: a target the components call reachable and the route could not find — a pocket far out in the wasteland (`wasteland`), or a detour past the search window. Near the site, unreachable cells are never picked |
| Stands still with a `move` task, `path 0 cells`, in a big crowd that all wanted to go somewhere at once | waiting its turn in the route budget (`architecture.md`). It gets there in the end; a long wait means far searches that big that the block's share takes ticks each |
| Flips between two goals every few ticks | routine lacks hysteresis, or a goal returns `Blocked` and is retried the moment its cooldown ends |
| Walks halfway, turns round | the routine released because the goal's own progress changed the stat; add hysteresis |
| `result` flickers `in progress`, no task | `process` returns `Working` with nothing queued, every tick |
| Frame rate drops only on some maps | a failure path returns `Working` instead of `Blocked`, re-routing every tick; check `ROUTES_ASKED` |
| An effect happens twice or never | it is applied in the goal instead of the task, or lost on handover (use `deprioritized`) |

## Timesheet (steps 4b and 6c)

Brain records shifts after goal selection, including the commute, and unemployed
days as the day turns. After tasks and playzone Operate, a free effect slot emits
Record and clears pending entries; an occupied slot keeps them for a later tick.
It is saved at Brain tag 15. See ../economy/SKILL.md for payroll and accounting.
