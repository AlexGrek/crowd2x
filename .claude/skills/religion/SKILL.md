---
name: religion
description: Religion in crowd2x - every person's religiosity, 0 to 100 (rolled evenly from its uid, on Body beside obedience, saved as pb::Body tag 11, shown on the unit panel's talents tab), the church (map::district::church, a lot building near the district's middle: five altar sections along the far wall, pews either side of an aisle), the cleric job (JobKind::Cleric 25, FeatureKind::Altar, 08-16 every day, WorkGoal's "lead prayers" stretch, a white robe and gold stole) and how clerics are chosen - ordained, never offered by distance (JobDef::ordained, GameState::ordain): the most religious of the free, after everybody moves in and on every hour a post is empty, and a dead cleric's post to the most religious free - praying (StayBusyRoutine's wants_to_pray, PRAYING 0.13, prayer_gap by religiosity, PRAYED memory, GoalId::Pray 37, PrayGoal, Task::pray beside a FeatureKind::Pew), Event::Prayed mending the Mind by devotion and twice at a service (a cleric leading prayers, Effect::Lead, sim::religion::Services, pb::World tag 34), the art (tools/church_art.py) and the tests. Maps every concept to the file that owns it. Use when changing religiosity, the church, clerics, who becomes one, prayer or what it does to the mind.
---

# Religion

Everybody is **religious from 0 to 100**. The district has a **church**: an altar of five
sections along its far wall and pews before it. The **five most religious free people** —
jobless, when everybody has moved in — become its **clerics**, one to each section, and lead
prayers there through the day. Anybody religious enough walks to the church **now and then
to pray** at a pew, the devout oftener; a prayer mends the mind by how devout one is, **twice
as much at a service**, while a cleric is leading prayers. Plain simulation, saved,
deterministic.

## Religiosity

| Concept | Where | What |
| --- | --- | --- |
| the value | `Body::religiosity` (`sim/entity.rs`) | `u8`, 0-100, on every body (a dog's is rolled too and never read). `set_religiosity` clamps. **Nothing in play changes it**; from outside, `Command::SetReligiosity` (`Input::set_religiosity`, `GameState::set_religiosity`, a person only) and the QA step `{"religiosity": N}` on the selected unit |
| the roll | `religion::religiosity(uid)` (`sim/religion.rs`) | **even**, 0..=100, from a `SmallRng` seeded with the uid xor a salt — like `sound::obedience`: no draw from the world's or the spawn's stream, so every other roll a seed makes is the one it made before religion |
| saved | `pb::Body.religiosity` (tag 11, optional) | absent in an older save: rolled again from the uid, the value it would have had. Over 100 refused |
| shown | `game/unitpanel.rs` `talents_block` | the talents tab's last line, `religiosity  N / 100` |

## The church (`src/map/district/church.rs`)

A lot building (`LotBuilding`, `SPEC`, last in `LOT_BUILDINGS`, so every seed's other lot
buildings, bank and shops are as they were and only its houses and parks moved). One, on the
free lot nearest the district's middle (the playzone, built before it, takes the nearest).
`wall brown` round `floor diagonal`, a plain door in the middle of its street side, nobody's
property. Laid out from the door (`layout`, both ways a lot can face):

- the row inside the door clear;
- **pews** (`PEW`, `"pew"`) in the next row — and two further on in a room six deep — in every
  column but the **aisle**, the two the door opens onto;
- the row in front of the altar clear: the clerics';
- **`ALTARS` = 5** sections (`ALTAR`, `"altar"`) side by side along the far wall.

`LANDMARKS` has it (`map/landmarks.rs`, the find menu). Props: `map/props.rs` (both
blocking), `editor/props.rs` (`altar.png`, `pew.png`, drawn by `tools/church_art.py` — the
altar's cloth runs edge to edge so five read as one, the pew's backrest and seat too).
Neither needs power or water. Tests: `a_district_has_one_church_with_five_altars_and_its_pews`,
`every_altar_and_pew_has_room_beside_it`, `the_plan_keeps_an_aisle_from_the_door_to_the_altar`.

## The clerics

| Concept | Where | What |
| --- | --- | --- |
| the job | `JobKind::Cleric` (25, `sim/job.rs`) | workplace `FeatureKind::Altar` (39, beside), `GoalId::Work`, `GUARD_COMMUTE` (2 h: the church is in the middle and the clerics live anywhere), `[CHURCH_HOURS]` 08-16 **every day**, uniform `"uniform cleric"` (a white robe to the ankles, a gold stole; `tools/human_art.py` `UNIFORMS["cleric"]`), `now`: `leading prayers` / `on the way to church`, a commoner's (`noble: false`), wage $40 (`economy.yaml`) |
| the work | `brain/tasks/work.rs` `STRETCHES` | `(Altar, "lead prayers", "led prayers")`: `"led prayers an hour at the altar at x, y"` |
| **ordained, not hired** | `JobDef::ordained` (`is_ordained`) | the one row with it true. `give_a_job` (an arrival, `Command::Hire`) **never offers it**. Instead: |
| `GameState::ordain` | `sim/mod.rs` | every altar shift standing empty, filled with **the most religious of the free**: living, jobless humans not on mental leave and not frozen, sorted (not on sick leave first, religiosity highest, lower uid), each the first of them its class allows and who may use the altar. Logs `"... was hired as a cleric at the altar at x, y, 08:00-16:00, ordained for its faith (93)"`. Called at the end of `spawn_from_spawners` (everybody in, everybody else hired: the free are what is left) and **on every hour** from `comings_and_goings` — which walks the crowd only while a post is empty |
| a cleric dies or leaves | `fill_vacancy` | the post goes to the most religious free (`faithless = 100 - religiosity` in the key, distance 0), not the nearest |
| leading prayers | `religion::leads(job, at, think)`; `Brain::react` 6b | a cleric whose workplace is an altar, in its shift's hours (not the commute), beside it: `Effect::Lead { at }` every tick, like the playzone's `Operate` |
| `Services` | `sim/religion.rs`, `Facilities::services`, `Think::services` | the map's altars and the ones led as the last tick ended; `world_step` (and `testing::World::step`) calls `begin_tick` before granting the tick's `Lead`s. `is_on(pew)`: an altar led within `SERVICE_REACH` (12, manhattan) |
| saved | `pb::World.services_led` (tag 34) | `Services::save`/`restore` (a cell that is no altar refused). Absent in an older save: no service until the next tick |

Seed 0, 7 and 42: the least religious cleric is over 50 and no free commoner is more
religious than it (`sim::tests::everybody_in_a_district_moves_into_a_bed_of_their_own_house`).

## Praying

| Layer | What | File |
| --- | --- | --- |
| when | `StayBusyRoutine` → `wants_to_pray(ctx, top)`: a human, alive, the `Mind` process running (off means off: a prayer is for the mind), the goal in charge `Wander` (or `Pray` already), a pew on the map, religiosity ≥ `MIN_FAITH` (10), the last prayer (`PRAYED`, a `Recall::Time`) at least `prayer_gap(r)` ago — `PRAYER_GAP_AT_FULL` (12 h) × 100 / r: twice a day at 100, daily at 50, every five days at 10 — and a slot (`PRAYER_SLOT` 30 world min) a hash of the uid picks, one in `PRAYER_ONE_IN` (4). **Never prayed** counts as prayed when the world began less a uid-picked share of the gap, so a new town's faithful go over its first day, not all on its first morning | `brain/routines.rs` |
| how much | `PRAYING` = 0.13: over wandering (0.1), the phone (0.11) and play (0.12); under a chat (0.15) and everything else. Faith never takes anybody off anything; the night's sleep floor (0.6) keeps it to the day | `brain/routines.rs` |
| the goal | `GoalId::Pray` (37, 🙏), `PrayGoal` (a `Seek`, `pb::Executor::Seek`): `choose_nearest(Pew)`, `move_beside`, `Task::pray(pew, PRAYER_SECONDS)`; a failed walk is `Seek::setback` to the next pew; none → `Blocked`. Said → `PRAYED` remembered, `"{uid} prayed at the pew at x, y"` (`, at a service`), `Achieved` — and the same in `deprioritized` if something took over on the tick it ended | `brain/goals/pray.rs` |
| the task | `Pray { pew, seconds }` (`TimedTask`, `"pray"`), `PRAYER_SECONDS` 20 world min: beside the pew, a body, the pew still a feature, every tick; `Action::interact`. Finished → `Event::Prayed { devotion: religiosity / 100, service: think.services.is_on(pew) }`. Cut short, nothing | `brain/tasks/pray.rs` |
| the body | `Mind` (`biology/mind.rs`): `PRAYER_PEACE` (4) × devotion, × `SERVICE_BLESSING` (2) at a service. Bladder, satisfaction and recollection ignore it (`Event::Prayed { .. }` arms); fun has none: a prayer is no fun | `biology/` |

## Tests

`sim/religion.rs` (the roll even and stable, the gap by religiosity, a service near a led
altar and only while led, the save); `brain/tasks/pray.rs` (from across the church fails,
peace by devotion only once said, twice at a service, walking away is nothing);
`brain/goals/pray.rs` (a devout wanderer prays and is mended, the faithless never, no second
prayer inside the gap, nobody with the mind off, a walled-off pew held off with
`ROUTES_ASKED`); `job.rs` `a_cleric_leads_prayers_at_its_altar_every_day_and_is_the_one_job_ordained`;
`sim::tests::the_most_religious_of_the_free_are_ordained_clerics_and_a_dead_ones_post_goes_to_the_next`;
the district hiring test; `save::tests::a_church_saved_in_the_middle_of_a_service_carries_on_exactly`; `game/unitpanel.rs` (the talents tab's line); the church's own. QA `qa/church.json`: a church of a room, three arrivals none hired by distance, the most religious ordained on the hour, the job menu, `lead prayers at the altar`, the uniform, an hour led, and a prayer said within the day.

## What is not here yet

- Religiosity never changes: no prayer deepens it, no bad news shakes it.
- Clerics do nothing at a service but stand at the altar; nobody comes for a service at a time.
- A cleric's altar has no candles lit in the lightmap (`lighting/scene.rs` `EMITTERS`).
