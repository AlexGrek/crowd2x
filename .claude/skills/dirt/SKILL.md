---
name: dirt
description: Dirt in crowd2x - the four contaminants (dirt, blood, wet, burns) on bodies and on the ground (sim::dirt, Dirt, DirtLayer, dense on the site and chunked in the wasteland), the exchange a footstep makes and the TRACE that ends a trail, where dirt comes from (work in the earth and the scrap - sowing, tending, harvesting, planting flowers, salvaging; the wasteland's constant puddles; blood from bites and pee from a critical bladder in sim::soiling; outdoor relief), wetness drying by the hour, the cleaners who look over the district every 27 world minutes and mop up public ground (CleanGoal, the Clean task, Effect::Clean, FeatureKind::District), how dirt is drawn (game::dirt), saved and set from QA, and the tests. Maps every concept to the file that owns it. Use when changing dirt, stains, trails, puddles, who gets dirty or how, cleaning or the cleaners, or the dirt dots on screen.
---

# Dirt

Plain simulation (`src/sim/dirt.rs`), saved, deterministic, read by the brain and drawn by
the game. It changes nothing a unit needs; it is what a crowd leaves behind it.

## The model (`src/sim/dirt.rs`)

| Concept | What |
| --- | --- |
| `Dirt` | four fractions, 0 clean to 1 saturated: `dirt` (earth, brown), `blood` (red), `wet` (blue), `burns` (charcoal). `CLEAN`, `earth(x)` (earth only), `components()`, `overall()` (the four summed — what a cleaner asks), `describe()` (`"dirt and blood"`, for a log line), `is_valid()` (finite, 0..=1 — what every setter and a save refuse) |
| `Dirt::exchange` | a footstep: each component moves `EXCHANGE` (a quarter) of the difference between body and ground, conserved — **except a `TRACE` (1/256) or less, which is let go**: too faint to draw (the game draws in sixty-fourths), and without it a trail would thin for ever and a wild chunk walked once could never be reclaimed. A trail of a soiled walker ends 10-25 cells on |
| `Dirt::soil(grime)` | work or a body's accident: each component covers `grime`'s share of what was still clean. Never cleaner for it, never past saturated |
| `DirtLayer` | `Facilities::dirt`, `Think::dirt`, `GameState::dirt()`. Dense on the site (`cells`), a `ChunkGrid` of wild chunks off it (`prepare`d round the crowd in `follow_the_crowd`, `sweep`ed with occupancy; a chunk with a stain is kept). `at`, `set` (refuses invalid; a puddle accepts and does not change), `soil`, `enter` (the move step's exchange), `site_stains()` (every stained site cell, row by row: what a cleaner's look reads), `evaporate` |
| `Body::dirt` | what a unit carries (`dirt()`, `set_dirt`, `dirt_mut()`), every kind — a dog tracks mud too |

**Who writes it**: the move step (`sim::move_step`, `dirt.enter` on a granted step into a
new cell), `sim::soiling::settle` (the world step), `effect::apply` (`Pee`, `Clean`),
`process_pass` (`evaporate` on each whole world hour), the spawn pass (`Input::set_dirt`,
`set_unit_dirt`), and a task finishing a piece of work, on its **own** body only
(`ctx.walk.body_mut().dirt_mut().soil(...)`). Parallel think and react only read it.

## Where it comes from

- **Work in the earth and the scrap.** A finished piece soils whoever did it, and it goes
  where they walk: `SOWING` (0.2 earth), `TENDING` (0.1), `HARVESTING` (0.15), `PLANTING`
  (0.2, a decorator's flower bed), `SALVAGING` (0.15, an order and a collector alike). In the
  task's `ActionState::Finished` arm (`brain/tasks/sow.rs`, `tend.rs`, `harvest.rs`,
  `plant_flowers.rs`, `salvage.rs`); a piece cut short soils nobody. Farmers, decorators and
  collectors bring it in through the gates.
- **Puddles** in the wasteland: `Wasteland::puddle(cell, site)` (`map/wasteland.rs`), a pure
  function of the seed — value noise over `PUDDLE_ABOVE`, only on dirt, stony dirt and scrub,
  never under a heap, never on the site, about 2% of the ground in small patches, in the
  clearing too. **Constant and immutable**: `DirtLayer::at` reads a puddle before storage,
  `enter` exchanges with a copy (the body is soiled, the puddle stays as it was), `set` and
  `soil` leave it, and nothing about one is saved. `0.5..=1.0` earth, in sixty-fourths. No
  `GENERATION` bump: tiles and heaps are unchanged.
- **Bodies** (`sim/soiling.rs`, settled in slot order before last tick's wounds are cleared,
  independent of the brain's effect so neither a fatal bite nor a finished task is lost): a
  bite soils victim and cell with blood (damage / 100); a bladder at 100 off a toilet wets
  body and cell (`PEE`). Outside the district a bursting unit walks 3-6 cells and pees on the
  ground (`RelieveGoal`, `Task::Pee`, `Effect::Pee`), wetting only the ground.
- **Drying**: on each whole world hour `DirtLayer::evaporate` takes `EVAPORATION_PER_HOUR`
  (0.25) wetness off every stored cell, on and off the site. Earth, blood and burns stay.

## The cleaners (`jobs` skill for the job's frame)

`JobKind::Cleaner` (number 17, hired **last**): two cleaners every day, one after the other —
`CLEANER_SHIFTS`, a morning (07-15) and an evening (15-23) for each of the playzone's two
groups (`FIRST_GROUP` Mon, Wed, Fri, Sun; `SECOND_GROUP` Tue, Thu, Sat), so four cleaners
working four days a week or three. `CLEANER_COMMUTE` is 0: there is nowhere to go first.

- **Workplace: the district itself.** `FeatureKind::District` (no prop, `prop()` says
  `"district"`): `Features` puts one at the public passable cell nearest the middle of a site
  with `TOWN_HOMES` (20) homes or more — a generated district; a test room never, so no test
  arrival is ever made a cleaner. It is only what `Workplaces` keeps the four shifts by: a
  cleaner never goes there, and with nothing to clean it walks round.
- **The look** (`brain/goals/clean.rs`, `CleanGoal`): the on-duty cleaner's own brain reads
  the site's dirt layer **every `SCAN_EVERY` (27 world minutes), on the tick it is due,
  whatever it is doing** — `GoalExecutor::keep_up` (brain step 3b) asks the goal in charge
  every tick a task of its is going on, and a look there may cut a walk short for something
  nearer (a cell being mopped is mopped to the end). It keeps the `FOUND` (16) nearest cells
  worth cleaning: `overall()` over `DIRTY` (0.15), nobody's home (`Properties::of`), reachable
  by the map's components (`may_reach`, no route), not lately given up on. An insertion into
  an inline array: no allocation, no scan of the crowd, off duty never.
- **Between looks** it cleans what it found, nearest to where it stands each time, checking
  each cell again on the way (`worth_cleaning`): `move_beside` it (never through a stain it
  could stand beside) and `Task::clean` (`CLEAN_SECONDS`, 5 world minutes, from on it or
  beside it; `Event::Worked`, part credit when cut short; fails once the cell is clean).
  `Achieved` per cell. A walk that fails is a `Seek::setback` (wait for a body, else
  remembered out of reach). With nothing left to clean it **roams**: a public, reachable,
  on-site cell within `ROAM_RADIUS` (12), `WANDER_TRIES` tries (a cheap miss), until the next
  look.
- **The world**: `Effect::Clean { at }` — a cleaner within a step of a site cell with
  anything on it — sets it `CLEAN` and logs `"{uid} cleaned up dirt and blood at x, y"`.
- Saved as `pb::Executor::Clean` (tag 5: the seek, the found cells, when it looked, where it
  roams; refused: more than `FOUND`, a NaN time); the task by name, `"clean"`.
- Uniform `"uniform cleaner"`: a teal coverall, a yellow hi-vis band, yellow rubber gloves
  (`tools/human_art.py`). Wage `cleaner: 40` (`economy.yaml`). Job menu: `keeping the
  district clean`.

## Drawn (`src/game/dirt.rs`)

CPU-generated 16x16 dot textures only for visible stained cells (`levels`: each component in
sixty-fourths), upscaled, over the terrain and under props and bodies. Each channel owns a
quarter of the dots, positions hashed from the cell, so a mixed stain keeps every colour and a
growing one only adds dots. No simulation RNG. A puddle is drawn like any stain.

## Saved (`savegame` skill)

`pb::World` tag 21: every non-clean stored cell, site and wild (`DirtLayer::save` /
`restore(map, ...)`, duplicates and invalid values refused); `pb::Body` tag 9: what a unit
carries. Older saves start clean. Puddles are never stored. The cleaner's goal and task as
above.

## Tests

- `sim/dirt.rs`: exchange conserves; a trace is let go and a trail ends; work soils up to
  saturated and never cleans; a puddle soils whoever walks through it and stays as it was; a
  body tracks a puddle onto the site; save round trips and refusals.
- `map/wasteland.rs`: puddles lie in small patches on dry ground, 1-4% of it, never under a
  heap; in the clearing too, never on the site.
- The task tests (`Rig`): a finished sow, tend, harvest, flower bed or salvage soils by its
  grime; a failed one leaves the worker clean.
- `sim/soiling/tests.rs`: blood from bites, accidents off a toilet, drying.
- `brain/tasks/clean.rs`: mopped only from within a step, asked away when done, fails once
  somebody else cleaned it.
- `brain/goals/clean.rs` (`testing::World`, a street with a house): mops public ground,
  leaves faint trails and homes alone; **looks exactly every 27 minutes and dirt between two
  looks waits for the second** (seen failing with `keep_up` off: 1661 s apart); walks round
  between; off duty neither looks nor cleans; the save round trip; a look keeps the nearest.
- `sim::tests::everybody_in_a_district_moves_into_a_bed_of_their_own_house`: four cleaners,
  the two groups' morning and evening shifts, hired last.
- `game/dirt.rs`: dots stable, distinct, growing with amount. QA `qa/dirt.json` (dots, a
  dog's trail), `qa/bladder_accident.json`, `qa/outdoor_toilet.json`.
