---
name: outposts
description: Outposts and builders in crowd2x - the king's decree from the throne (Paperwork::Decrees, Task::decree, outposts::decree_due, choose_site - 50 to 100 cells from the district's wall, as near as that allows to the biggest reachable base of scrap), the building plan every builder reads (sim::outposts, LAYOUT, 6x7, Tile/Stage/Part, claims one builder a tile), the builder job (on demand, 08-18 every day, BuildersYard beside the warehouse's steel rack, $150 a shift built, a care package a day with nothing to build), BuildGoal and its tasks (claim, fetch, lay, construct, rest), structures off the site (Map::build, Structure, walls as WALL terrain), finished furniture taken into the feature, fridge, store and door indexes (Fixture, Facilities::install), drawing (game::outposts, the flag, building tiles), the statistics and find menus, saving, the lab metrics and the tests. Maps every concept to the file that owns it. Use when changing outposts, builders, the king's decisions, the building plan, or anything built out in the wasteland.
---

# Outposts

The first thing a district's steel is for. **The king decrees an outpost** out in the
wasteland when the warehouse holds the steel for a whole one; **four builders put it up**, a
tile at a time, from a building plan they all read; and once a piece of furniture is built
it **works** — its fridge is somewhere to eat, its beds somewhere to sleep, its doors doors.
Plain simulation (`src/sim/outposts.rs`, the brain, the map), saved, deterministic.

## The plan (`src/sim/outposts.rs`)

| Concept | What |
| --- | --- |
| `LAYOUT` | 6 wide, 7 tall (`WIDTH`, `HEIGHT`), a character a cell, top row first, the doorway in the bottom row: `W` wall (1 steel), `D` door, `R` fridge (1 steel), `S` the local warehouse (1 steel), `B` bed, `.` nothing. **20 walls, 2 doors, a fridge, a store, 2 beds = 26 tiles (`TILES`), 22 steel (`OUTPOST_STEEL`)**, both counted at compile time. Every piece of furniture keeps two free cells beside it, and the inside is joined up whatever is built first |
| `Outpost` | its flag (`center`, the middle, `CENTRE` = 3,3 from its corner, a cell nothing is built in), `flipped` (upside down: the doorway in the top row, when the district is above it — **the doorway always faces the district**), the day `decreed`, and `tiles`, in `LAYOUT`'s order. `footprint()` |
| `Tile` | `cell`, `part`, `stage`, `claim` (the builder on it). `steel_wanted()` (the part's while `Planned`, none once laid), `done()` (0-1) |
| `Part` | `Wall`, `Door`, `Fridge`, `Store`, `Bed`: `steel()`, `name()`, `prop()` (`"door"`, `"fridge"`, `"outpost store"`, `"bed 1"`; a wall none, it is terrain), `structure()` on the map once built (`Wall`, `Obstacle`, a door none), `blocks_while_laid()` (every building tile but a doorway's, so the inside is always got to), `phase()` (the claim order: walls, fridge and store, beds, doors) |
| `Stage` | `Planned`, `Laid { progress }` (world seconds worked of `TILE_WORK` = **2 hours**), `Built` |
| `Outposts` | every outpost decreed, in order, and `fruitless` (the day the king last looked in vain). **One goes up at a time**: `under_way()` is the last while it is unfinished. `complete()`, `wants_a_decree(day)` |
| reads (brains) | `tile(cell)`, `claim_of(uid)`, `has_work_for(uid)` (a tile of its own, or one nobody has), `steel_wanted()` (the steel still to be brought: all the planned tiles', and the unclaimed ones'), `pick(here, steel, away)` (out at the outpost, a tile its pocket can supply first — no walk back for steel — then by phase, the nearest, the lower cell), `stand_cells(tile)` (the cells beside it no tile of the plan is in), `fixtures()` |
| writes (the world step) | `found(map, center, day)` (refused while one is going up or where `is_clear` says no), `found_nowhere(day)`, `claim(cell, uid)` (only while free, one tile a builder), `release(uid)`, `lay(map, cell, uid)` (the builder's own planned tile on clear ground: `Map::build(Obstacle)` unless a door), `construct(map, cell, uid, seconds)` → `Completed { part, fixture, built, outpost_done }` at `TILE_WORK`: the map gets the part's structure (a wall over its building tile), the claim is given up |

**Not a map file's**: the map keeps *that* a cell is built on (`Map::build`, below), this keeps
the rest, and builds the map again on load (`Outposts::restore`), as `sim::fields` replants.

## The king's decision

(The same court, the same half hour, also picks the soldiers' training ground — once, first,
`Effect::DecreeGround`; see the `training` skill.)

- **When**: the king's row has `paperwork: Some(Paperwork::Decrees)` (`sim/job.rs`).
  `WorkGoal::plan` (`brain/goals/work.rs`), at the throne, on duty (court, 10-16 Mon-Sat),
  queues `Task::decree(throne, DECREE_SECONDS)` (30 world minutes) instead of an hour of court
  while **`outposts::decree_due(think)`**: no outpost going up, not looked in vain today, a
  builders' yard on the map, and the warehouse's racks holding `OUTPOST_STEEL`
  (`Bins::total_in(SteelRack)`). Otherwise it holds court as before.
- **The task** (`brain/tasks/decree.rs`) is a `Work` stretch at the throne (its guards and
  tiring, like `CloseBooks`); when it is over, if still due, it works out **`choose_site`**
  and asks `Effect::Decree { at, day }`. The world founds the outpost (important log line
  `the king decreed an outpost at x, y: its flag is up`) or remembers the day
  (`the king found nowhere to build an outpost`). "Remotely": the king never leaves the
  throne.
- **`choose_site(map, outposts)`**, pure, once a decree:
  1. every heap in `site.grown(FROM_WALLS.1 + BASE_REACH)` (132) that is out in the open
     (`Navigation::is_outside`: reachable, not in a lake's pocket), its pieces summed into
     16-cell blocks (`BLOCK`);
  2. **the base** is the block whose 3x3 neighbourhood holds the most (ties: nearer the
     site, then cell); no scrap at all, no site. **From the second outpost on, spread comes
     first**: among blocks with scrap, the one furthest round the district (`bearing` from the
     site's middle, the site taken as a square; `SECTORS` = 12 steps over a half turn) from the
     nearest earlier outpost wins — the opposite side from one, the middle of the widest gap
     from several — and scrap only decides within a step;
  3. ring by ring out from the base's middle (up to `SEARCH`), the first ring with a clear
     site, the nearest in it (manhattan, then cell).
- **`is_clear(map, center, outposts)`**: every footprint cell **50 to 100 cells from the
  site** (`FROM_WALLS`, worked out from the bounds in O(1), `distances`), and the footprint
  and the ring round it `Map::can_build` (open ground off the site: no heap, crop, water or
  anything built), not meadow (the farmers' soil), `is_outside`, and not touching an earlier
  outpost's ring. The world asks it again in `found`.

## The builder (`sim/job.rs`, `brain/`)

| Layer | What | File |
| --- | --- | --- |
| kind | `JobKind::Builder` (21, **hired last**): workplace `FeatureKind::BuildersYard` (33, no prop, `prop()` reads `builders' yard`), goal `GoalId::Build` (32, 🏗), `BUILDER_COMMUTE` = 1 h, `[BUILDER_HOURS]` = **08-18 every day** (ten hours, all daylight; set off at seven, after dawn, home before dusk), `uniform builder` (hi-vis yellow vest, silver bands, grey shirt, tan trousers), `goes_out_for: Some(Steel)` (so `ReadyRoutine` gets it ready before every trip), **`on_demand: true`** — the `JobDef` column every row has, only this one's true | `job.rs` |
| the yard | `Features::with_properties`: **`BUILDERS` (4) public passable cells nearest the first steel rack**, not on its sides, **only on a map with a throne too** (a king to decree, steel to build with) — a district; a test room with a steel works has no builders | `feature.rs` |
| on duty | `goals::build::on_duty(job, uid, think)` = `job.is_due` and, for an on-demand job, `outposts.has_work_for(uid)`. What `WorkRoutine` and `ReadyRoutine` ask. **No tile for it, it is as good as unemployed** — never raised, never on duty (the timesheet is told so), and still a builder, so nobody hires it into another job | `brain/goals/build.rs` |
| routine | `WorkRoutine`: on duty → `Build` at `WORKING`; **resting** (`Memory::is_resting`) → `Build` at **`RESTING` = 0.8**, so any committed need (1.2) is over it past the hold and a meal or a drink out of the pocket, or the bladder, gets seen to; **out of the site with no work for it now** (the shift over, nothing left) → `Build` at `WORKING`, so it **walks back in before dark** | `brain/routines.rs` |
| goal | `BuildGoal` (a `Seek`, `pb::Executor::Seek`), planned from what is true now: no work and out → beside its yard; resting → the rest of the rest (`Task::wait`); no tile → `Task::claim(pick)`; in the district and **too late to go out** (the walk there leaves less than a stretch before `knock_off`) → done for the day; in the district and not ready → its yard first (**every trip starts there**, where `ReadyRoutine` sees to its needs and puts a meal and a drink in its pocket, as a collector's at its bin); its tile wants steel, **or it is setting out with less than a trip's worth** (`trip_steel`) → the nearest stocked steel rack and `Task::fetch`, a piece at a time (`Effect::Fetch`); then `move_to` a stand cell (`stand_cells`, free first, nearest) and `Task::lay` or `Task::construct` for `min(STRETCH 1 h, work left, time to knock_off)`; past `knock_off` out there → back to its yard; a stretch done → logged (`worked on the outpost's wall at x, y: 50% built`), `rest_until(now + REST 15 min)`, `Achieved`. A lay refused (somebody in the cell) → `Seek::wait_again` up to `PATIENCE`, then held off; no steel anywhere → `found no steel to build with`, held off | `brain/goals/build.rs` |
| a trip's steel | `trip_steel`: its tile's piece and, **only while the rack holds more than every tile still on the plan wants** (`Outposts::steel_wanted` → all, unclaimed) and the pack has room for two more, a share of the unclaimed tiles' steel (`÷ BUILDERS`, rounded up), at most **`TRIP_STEEL` = 3**. Out there `pick` takes a tile its pocket supplies first, so the spares are built with no walk back. The rack condition is what keeps it from deadlocking: with exactly the steel an outpost wants (the sim test), nothing is spare, and no claimed tile ever waits on steel in another builder's pocket. Spares left when an outpost is done stay in the pocket for the next | `brain/goals/build.rs` |
| in before dark | **`knock_off(shift_end, home)`** = `min(shift end, IN_BY − home / HOMEWARD_CELLS_AN_HOUR)`: `IN_BY` half an hour before `SUNSET` (19:30), the walk home (manhattan from the work cell to the yard) at the slowest pace anybody has (`HUMAN_SPEED × (1 − PACE_RANGE)`, a tenth off for bends, ~93 cells a world hour). An outpost an hour out leaves the whole shift; seed 7's, three hours out, stops building before four. Seed 7's first month, before this: builders walked back from a 3 h outpost after 18:00 and reached the wall at nine, in the dark | `brain/goals/build.rs` |
| on leave | **A builder on leave gives its tile back** as it stands: off for the day (`Medical::on_leave`: a sick day, a patient), `Brain::step` 6b' asks `Effect::GiveUpTile` (`gave up the outpost's wall at x, y: on leave`); on mental health leave it gives up the whole post (`Effect::LeavePost`, which releases the tile too, and a jobless resident is hired in its place — `jobs` skill). Seed 7's first month, before this: a builder on mental health leave held the last wall (and its steel) for nine days | `brain/mod.rs`, `sim/effect.rs` |
| tasks | `Claim` (`"take on a tile"`, instant → `Effect::Claim`); `Lay` (`"lay a building tile"`, `LAY_SECONDS` 5 world min beside → `Effect::Lay`, `Event::Worked`); `Construct` (`"build"`, the stretch, **`Effect::Construct { at, seconds }` every tick it works** — an interruption loses a tick, never the stretch — and done the tick the tile is built; `Event::Worked` as `Work`); `Decree` (above). All `TimedTask`s, saved by name | `brain/tasks/{claim,lay,construct,decree}.rs` |
| world | `effect::apply`: `Claim` (a builder's), `Lay` (its own claimed tile within a step; refused where somebody stands or a body lies, or the steel is short; **the steel out of the pack all at once**), `Construct` (its own tile within a step; on completion `Facilities::install` the fixture, `built the outpost's wall at x, y: 12 of 26 built`, and `the outpost at x, y is built`). Claims are released on `died`, `GameState::dismiss`, `despawn`, `Effect::Admit` (hospital) and `Effect::GiveUpTile` (on leave) | `sim/effect.rs`, `sim/mod.rs` |
| pay | `economy.yaml` `builder: 150` a shift built. `Timesheet::last_on_duty` (`pb::Timesheet` tag 6): a day turned that an on-demand job was never on duty is **a care day**, as for the jobless | `brain/timesheet.rs`, `brain/mod.rs` |
| menu | `now`: `carrying steel to the outpost`, `getting ready at the yard`, `building the outpost`; `waiting for work` while due with nothing to build (`job_block_for`) | `job.rs`, `game/unitpanel.rs` |

## Structures off the site (`src/map/mod.rs`, `chunk.rs`)

`Map::built: BTreeMap<(ChunkPos, u16), Structure>` (keyed like `planted`), **`Structure::Obstacle`**
(in the way, seen past: a building tile, a fridge, a bed) or **`Structure::Wall`** (`WALL`
terrain: in the way and opaque). `can_build(cell)` (endless map, off the site, passable now);
`build(cell, s)` (refused on the site; over an `Obstacle` allowed, so a building tile becomes
a wall); `structure(cell)`, `built()`. Read by `ground()` (a wall reads `WALL`, so `terrain`
and `walk_cost` follow), `ground_is_passable`, the unloaded `is_see_through`; a loaded chunk's
bits by `Chunk::block` / `Chunk::shade`, and `load_chunk` applies them again
(`block_built`). **`is_open_ground` counts a built cell as open**, so navigation is never
worked out again — the doorway keeps the inside joined. `set_wasteland` forgets them. Not in a
map file, not in a save's chunks (the ground is kept as found).

## Furniture that works (`Fixture`, `Facilities::install`)

A built prop tile is a `Fixture { prop, cell }`. `Facilities::install` puts it into
`Features` (`install`: its kinds, and `entered` for a bed), `Fridges` (`install`: closed, cold,
**`own_power`** — no grid reaches the wasteland, and `set_power` leaves it powered), `Bins`
(`install`: an empty store) and `Doors` (`install`: shut, unlocked). Every `install` is
idempotent. **`GameState::install_fixtures`** puts every outpost's fixtures back wherever an
index is built again or swapped: `refresh_supply` (both daylight supplies) and
`follow_daylight`. So an outpost's fridge is food and water (stocked from the superfridge like
every fridge, a meal at the usual price), its beds are beds (an arrival is given the nearest
bed nobody has, which can be the outpost's), its doors open for whoever passes.

**The local warehouse** is `FeatureKind::OutpostStore` (34, prop `"outpost store"`, holds
scrap, site `"outpost"`, drawn as the scrap bin's fill strip): a working store — deposit,
withdraw, the statistics — but **no job routes scrap through it yet**. That, people living
out there, and anything else an outpost is *for*, come next.

## Living out there: booked beds, safe ground, the fridge

- **Booking a bed** (`Outposts::bookings`, bed → whose; `book`, `release_bed`, `booker`,
  `booked_by`, `bookable_near(here, BOOK_REACH = 30)`, `can_book`, `beds`, `is_bed`). A *built* outpost
  bed nobody has is any local's to book: when `SleepGoal::choose` is picking a bed (see
  `brain/goals/sleep.rs`) and the unit has no booking, the nearest unbooked outpost bed within
  `BOOK_REACH` cells (manhattan) is a candidate **only if it is nearer than the bed at home** (or
  it has none); `plan` queues **`Task::book(bed)`** (`brain/tasks/book.rs`, `"book a bed"`,
  instant → `Effect::Book { bed }`; fails and asks nothing where `can_book` says no) ahead of the
  walk. The world (`effect::book`) grants it while the bed is not in `Facilities::homes` either and
  the booker is alive; logged `booked the outpost bed at x, y`. **It stays the booker's for good**
  (one bed to a person, one person to a bed) until it dies (`effect::died`) or is despawned
  (`GameState::despawn`) — not on dismissal. Each night the sleeper picks the nearer of its home
  bed and its booked one, so a collector far from home sleeps at the outpost and a resident at
  home never does. A critically tired unit's random free-bed pick skips a bed another has booked.
  **Not `Homes`**: homes is one bed from the district's spawn pass; bookings are saved in
  `pb::Outposts.bookings` (tag 3, `pb::Home`), restore refuses a bed that is not a built bed or a
  second booking either way.
- **Deathdogs cannot enter an outpost.** `Map::shelter(footprint)` (a `sheltered` list beside
  `built`, off the site, not in a map file or a save's chunks) is called by `effect::decree` the
  day the flag goes up and by `Outposts::restore`; `Map::is_safe(cell)` = on the site **or**
  sheltered. Everywhere a deathdog was kept off `site().contains` now asks `is_safe`: its route
  (`Think::step_cost_with`), the move step, `Command::Spawn`, `Chase`, `effect::bite` (nobody
  inside is bitten) and `alarm::take_in` (nobody inside panics at one, no prey inside). The whole
  6x7 footprint counts, the doorway included. `set_wasteland` forgets it.
  **Not yet**: `FleeGoal` still runs for the district's gates (`WaysIn`), not the nearest outpost.
- **The fridge is anyone's.** It is in the `Features` and `Fridges` indexes like any other and has
  no property, so whoever is nearest — a builder at its shift, a collector — uses it
  (`nearest_in_reach`); a meal costs the usual price (`Effect::Buy` carries no place, so there is
  no "free here").

## On screen (`src/game/outposts.rs`)

The `crops.rs` pattern: a sprite on every outpost cell on the canvas that shows something,
drawn again only when what it shows (`Look`) changes. **The flag** (`outpost_flag.png`), a
**building tile** (`building_tile.png`, four frames by `done()`: steel laid and posts up,
scaffolding braced, a wall going up, nearly done), and what is built **in the palettes' own
art, by name**: `wall brown` (just over the ground, `WALL_DEPTH`), `fridge`, `bed 1`,
`outpost store` (its fill by `bin_frame`), `door` (posed by the leaf's openness, the leaf's
slide by `DoorLeaf::pose`). Nothing of an outpost is in `CurrentMap`, so this draws it all.
Art: `tools/outpost_art.py` (`generate`, `preview`); the uniform `UNIFORMS["builder"]` in
`tools/human_art.py`. The statistics menu's `outposts` line (`1 built  12 of 26 tiles going
up`, `OutpostCount`), the find menu's `outposts` (the flags as gold squares, the camera to
the newest), and the outpost store under `outpost` (`game-screen`).

## Saving (`savegame`)

`pb::World.outposts` (tag 28, `pb::Outposts { outposts: [pb::Outpost { center, flipped,
decreed, tiles: [pb::OutpostTile { stage: TileStage, progress, claim }] }], fruitless,
bookings: [pb::Home] }`,
absent = none). `Outposts::restore(saved, &mut map)` refuses a wrong tile count, progress
that is no amount of time, a builder on two tiles or on a built one, an unfinished outpost
before the last, and anything the map will not have built where it is — and builds the map
again. **Restored right after `GameState::new`**, so `refresh_supply` installs the fixtures
before the saved fridge, store and door states are put back by cell; then every claim is
cross-checked to be a living builder's. `pb::Memory.rest_until` (tag 11),
`pb::Timesheet.last_on_duty` (tag 6). Catalogue numbers: `JobKind::Builder` 21, `GoalId::Build`
32, `FeatureKind::BuildersYard` 33, `OutpostStore` 34; tasks by name.

## The lab

`outposts.built` (a headline column) and `outposts.tiles` (built tiles of the one going up),
sampled every world hour (`src/lab/metrics.rs`).

## Tests

- `map::tests`: `what_is_built_off_the_site_stands_there_loaded_or_not` (a building tile in
  the way and seen past, a wall over it `WALL` and opaque, dropped and loaded again, navigation
  untouched — seen failing with `block_built` not called), `nothing_is_built_on_the_site_on_a_heap_or_round_a_map_with_nothing_round_it`,
  and `a_new_wasteland_forgets_...`.
- `sim::outposts`: the layout's counts, the doorway facing the district, the band and the
  footprint, `choose_site` over three seeds, one at a time and once a day, claims exclusive
  (seen failing with the claim check gone), steel-free tiles out there, laying and the two
  hours, a doorway never in the way and the fixtures, stand cells, the save and its refusals.
- Bookings and shelter: `outposts` (`a_bed_is_booked_once_...`, `..._only_from_within_reach_...`,
  `bookings_come_back_from_a_save_...`), `tasks/book.rs`, `goals/sleep.rs` (books and sleeps; a
  booked bed is the one it comes back to; not a free bed to another; the outpost against a far
  home bed; nobody at home books), `map::tests::a_sheltered_area_is_as_safe_as_the_site_...`,
  `deathdogs::tests::a_deathdog_keeps_out_of_an_outpost_and_bites_nobody_inside_it` (seen failing
  with `is_safe` the site alone). `testing` helper `outposts::built_outpost_with_beds`.
- tasks (`Rig`): `claim`, `lay`, `construct` (every tick counted; done mid-stretch), `decree`
  (with and without the steel).
- `brain/goals/build.rs` (`testing::World`): a wall from claim to built in two stretches with
  a rest between; no work, no duty; back in after six (seen failing with the rule gone);
  resting it eats what it carries (seen failing at `WORKING`), working it does not; no steel,
  held off; with steel to spare it sets out with `TRIP_STEEL` (seen failing with the share
  gone), with none to spare only its tile's; on leave it gives its tile back (seen failing with
  the effect not asked); `knock_off` in by `IN_BY` at the slowest pace.
- `brain/timesheet.rs`: a building day a shift, an idle day a care day (seen failing with the
  arm gone).
- `sim::tests::the_king_decrees_an_outpost_and_four_builders_raise_it_with_the_steel_it_takes`
  (`testing::outpost_works`: never two on a tile, nobody building in the dark, every piece of
  steel used, walls as terrain, the furniture in the indexes, then a passer-by eats at its
  fridge and sleeps in its bed); the district test counts four builders at the yard, hired
  last.
- `save::tests::an_outpost_saved_half_built_carries_on_exactly` (seen failing with the
  outposts not restored, and with the fixtures not installed).
- `job.rs`, `economy.rs`, `game/statistics.rs`, `game/unitpanel.rs`, `game/outposts.rs` (the
  art). QA `qa/builders.json`.
