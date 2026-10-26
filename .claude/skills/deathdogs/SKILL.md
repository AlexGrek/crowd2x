---
name: deathdogs
description: Deathdogs, panic, and panic that spreads by sight in crowd2x - the night's packs that come out of the wasteland at dusk and walk off at dawn, how they never set foot on the site (routes, the move step, the spawn pass), how they prowl, hunt, chase and bite (20-60 health), wounds felt in the victim's own react, death by deathdog bites, how a person outside who sees a deathdog or somebody running from one panics (brain step 0, sim::panics), flees flat out at PANIC_SPEED to the nearest way in (sim::refuge) and shelters, and the deathdog art. Maps every concept to the file that owns it (no code). Use when changing deathdogs, their arrival or departure, bites and wounds, panic, fleeing, sheltering, the ways in, or the deathdog sprite.
---

# Deathdogs and panic

The wasteland round every endless map is dangerous at night. **Inside the site
is safe**: a deathdog never sets foot on it, never bites anybody on it, and
nobody inside reacts to one. Everything here is plain simulation
(`src/sim/`), saved, deterministic, and scans no crowd.

## Where each concept lives

| Concept | Owner |
| --- | --- |
| The kind: `EntityType::Deathdog` (3), `keeps_off_the_site()` | `sim/uid.rs` |
| The entity: `Deathdog { walk, brain, facing, schedule }`, `DEATHDOG_SPEED` (= `PANIC_SPEED / (1 + HURRY)`, 4), `DEATHDOG_BITES` cause | `sim/kinds.rs` |
| Constants (`PACKS` 4, `PACK_SIZE` 3, `SPAWN_DISTANCE` / `DEPART_DISTANCE` 48, `LURK_NEAR` 3 / `LURK_FAR` 24, `BITE_DAMAGE` 20..=60, `BITE_INTERVAL` 1 watched s), `distance_from_site`, `is_dusk`, `arrive` | `sim/deathdogs.rs` |
| Tests (arrivals, keeping out, bites, panic, spread, hunt, routes bounded, departure through the two passes, nobody inside bitten, determinism) | `sim/deathdogs/tests.rs` |
| Save test (mid-chase at night, a wound pending) | `save/tests.rs` `a_chase_saved_at_night_with_a_bite_still_to_be_felt_carries_on_exactly` |

## Comings and goings

- **Arrival**: `process_pass` ends with `comings_and_goings(state, before, now)`
  (`sim/mod.rs`), after the frozen table is let go. On the tick
  `deathdogs::is_dusk` (`Sun::is_dark` turning, about 19:30-20:00),
  `arrive` puts `PACKS` packs on random points of the ring `SPAWN_DISTANCE` out,
  each deathdog on free, passable, open (`Navigation::is_outside`) ground within
  `PACK_SPREAD` of its point and at least `SPAWN_DISTANCE` out. Endless maps
  only. The dice are `GameState`'s saved rng. Logged ("a pack of 3 deathdogs
  came out of the wasteland at x, y").
- **Departure**: `Deathdog::react`, by day and `DEPART_DISTANCE` out, halts and
  returns `Effect::Depart`; `effect::apply` puts it on `Facilities::leaving`,
  which `comings_and_goings` despawns. Bodies an undertaker took up
  (`Effect::TakeBody`) leave the same way — this replaced `bury()` in
  `process_game_state`, which the game (`game/actors.rs`) and QA never called.

## Keeping off the site

Three layers: `Think::step_cost_with(cell, key, kind)` gives `None` for site
cells to a kind that keeps off (routes and detours go round the walls);
`move_step` refuses such a body a step into the site (`Blocked { by: None }`,
mirrored in `testing::World::step`); `spawn_pass` refuses a `Command::Spawn` of
one onto the site, with a log line.

## The deathdog's mind (`Brain::deathdog`)

| Routine (`brain/routines.rs`) | Goal (`brain/goals/`) | Priority |
| --- | --- | --- |
| `DepartRoutine`, by day | `DepartGoal`: a cell on the ray from the site's middle, `DEPART_DISTANCE + 4` out | 2.0 |
| `HuntRoutine`, `spotted` younger than `SCENT` (10 world min) and outside | `HuntGoal`: `Chase` → `Bite` → `Chase` (resting) ... | 1.0, urgency 1 |
| `ProwlRoutine`, always | `ProwlGoal`: random cells in the `LURK_NEAR..LURK_FAR` band | `BUSY` |

`GoalId::Hunt` 19, `Prowl` 20, `Depart` 21 have `executor: None`, so no person
carries them. Tasks (`brain/tasks/`): **`Chase`** finds its quarry again each
tick within a cell of where it was (two on the first tick), fails if lost or on
the site, succeeds in reach once `rest` ticks are over, and re-routes only when
the quarry drifts `DRIFT` (2) cells from the route's end. Its quarry is the low
32 bits of the uid's body (`Quarry`) so a `Task` stays 20 bytes
(`a_chase_is_no_bigger_than_a_task_was`). Saved as `pb::TaskKind::Chase` (21).
**`Bite`** is an instant `TimedTask` "bite": the human outside in reach (`at`
first, then the eight round it), damage from `tick_rng`, `Effect::Bite`.

## Bites and wounds

`Effect::Bite { at, damage }` → `effect::bite` checks the occupant is a human
outside the site and in reach, files a `Wound` in `Facilities::wounds`
(`sim/wounds.rs`) and logs "{dog} bit {human} for {damage}". The world step
clears last tick's wounds first (`Wounds::begin_tick`). The bitten person reads
its own wounds at the start of `Human::react` (`Think::wounds`):
`Event::Bitten { damage }` (taken by the `Health` process, so Health off means a
bite does nothing), `memory.spotted` becomes the biter, and dying that tick is
`"deathdog bites"`. Saved: `pb::World.wounds` (tag 18).

## Panic

- **Seeing danger** — brain step 0, `brain/alarm.rs::take_in`, on the look's
  own sightings, nearest first. A person *outside the site* who sees a deathdog,
  or a person in `Think::panics`, remembers it in `Memory::spotted`
  (`Spotted { who, at, when }`, inline, `pb::Memory.spotted` tag 4). A deathdog
  marks the nearest person outside (its current quarry first).
- **Who is seen panicking** — `sim/panics.rs`: `move_step` clears it, pushes
  every mover whose `Body::is_panicking()`, sorts it; react binary-searches it.
  Rebuilt every tick, not saved. `testing::World::step` sets its one unit.
- **Fleeing** — `FleeRoutine` (last in `Brain::human()`): `Flee` (18) at
  `FLEEING` 3.0, urgency 1, for `FEAR` (30 world min) after `spotted`.
  `FleeGoal` runs to `WaysIn::nearest` while `refuge::depth < SAFE_DEPTH` (2),
  shelters (`Task::wait(SHELTER)`) once inside. Logs "{uid} panicked".
- **Flat out** — brain step 7 calls `Walker::panic(top == Flee)`; the flag
  (`Body::panicking`, `pb::Body` tag 7) is on only while walking, and
  `Walker::current_speed` is then `PANIC_SPEED` (7) whatever the pace, hurry or
  gait. A hunting deathdog flat out is exactly as fast, so whoever sees one
  coming and has a head start gets away; deathdogs bite the slow to notice.
- **Ways in** — `sim/refuge.rs`: every passable cell on the site's edge (a
  district's gates), each moved up to `REFUGE_DEPTH` (4) cells in. Derived from
  the map in `Facilities::new`, not saved.

## Art

`assets/deathdog_idle.png` / `deathdog_idle_reversed.png`, drawn by
`tools/deathdog_art.py` from the dog's sheets (luminance onto an ash ramp, red
eye pixels). `characters/dog.rs` has a `Breed { Dog, Deathdog }` component;
`DogSheets::image_for(breed, facing)`; deathdogs share the dog pool
(`game/actors.rs::breed_of`). The fur is ash, not black, so the lightmap does not
swallow it at night.
