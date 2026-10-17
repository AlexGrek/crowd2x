---
name: savegame
description: Saved games in crowd2x - the .savegame file (a zip of Protobuf files written with prost, one per part of the world), what each file holds, the schema's evolution rules, the bit-exact save/load guarantee and the test that holds it, where every type's save and restore code lives (and the exhaustive-destructure rule that stops new state being forgotten), the endless world in a save (chunks stored verbatim, salvage tallies), the .saves/ store (date-time file names, atomic writes, newest-first listing, quick saves kept to four), the game screen's save and quick save (F5) with the name dialog and background writer, the main menu's Load screen, CROWD2X_SAVES / CROWD2X_LOAD, the golden format fixture, and measured timings. Use when adding state to the simulation, an entity kind, a brain part, a task, a goal executor or the map (it must be saved), when changing the save format or a schema message, when touching saving or loading in the UI, or when a loaded game behaves differently from the one that was saved.
---

# Saved games

A save is `.saves/{date-time}.savegame` (e.g. `.saves/2026-10-17_14-03-22.savegame`): a **zip of
Protobuf files**, one per part of the world. Plain Rust (`src/save/`, no `bevy::`), like
`sim/` and `map/`; the Bevy side is `src/game/saving.rs` (saving) and `src/saves.rs` (the
Load screen).

## The bar: a loaded world is the same world

**A world saved and loaded carries on exactly as it would have**: the same positions, to the
bit, on every tick after. That is the project's determinism rule carried across a reload, and
it is a test, not a hope (`save::tests::a_saved_world_carries_on_exactly_as_if_it_had_never_been_saved`
and `a_district_saved_in_the_middle_of_everything_carries_on_exactly`): two worlds, the
original and its saved-and-loaded copy, run side by side with the same commands, compared every
20 ticks. So everything that decides what happens next is saved, including the id RNG, the
arena's holes and free list, who holds which cell, and **a route search the budget stopped
halfway** (`PathFinder::save`/`restore`: frontier plus reached cells).

What is **not** saved: what a tick starts by overwriting (intent/move/effect buffers,
`Walker::meter`, `route_work`), what only says how memory is used (census lists, occupancy's
wild chunks, reclaim cursors — all rebuilt or re-prepared by the next spawn pass), scratch
(`Route::repair`, thread-local finders), and the log. Derived state is **rebuilt from the
map**, never trusted: features, properties, supply (both ways), `controlled`, fridges' power,
doors' locks. `GameState::restore` (`sim/saving.rs`) builds a world exactly as `GameState::new`
does and then applies the saved state.

## The file

| File | Message (`save::pb`) | What |
| --- | --- | --- |
| `manifest.pb` | `Manifest` | format, game version, name, quick flag, when, map title, tick, elapsed, population. The Load screen reads only this (`archive::read_manifest`). |
| `map.pb` | `SiteMap` | the site, as a map file holds it: terrain by name (palette + indices), grids, objects by name, wasteland seed (`map/save.rs`) |
| `world.pb` | `World` | tick, elapsed, id-RNG state, boxes off, street lights, switches off, fridges, doors, beds, arena layout (`sim/saving.rs`) |
| `chunks/NNNN.pb` | `Chunks` | the endless world, `CHUNKS_PER_FILE` chunks a file: every loaded chunk's **ground stored verbatim** (tiles + heaps as found) and every salvage tally, loaded or not; stamped with `wasteland::GENERATION` |
| `entities/NNNN.pb` | `EntityShard` | `SLOTS_PER_FILE` arena slots a file (holes left out): slot, serial, `holds_cell`, `Kind` oneof (`Human`/`Dog`) |
| `view.pb` | `View` | optional, Bevy side: camera centre, zoom, speed multiplier, paused |

Restored chunks go back into the map's cache; reclaiming one later regenerates it from the seed
(`map::save` test checks restored = regenerated). Bump `wasteland::GENERATION` whenever the
generator's output changes; a save from a newer generator is refused.

## Evolving the schema (read before touching `src/save/pb/`)

The schema is **Rust structs with `#[derive(prost::Message)]` and explicit tags** — no `.proto`,
no build script, no `protoc`. The rules (also in `save/pb/mod.rs`):

- **A tag is forever.** Never renumber, never reuse; list removed tags as reserved in the docs.
- **A new field must mean "as before" at its default** — that is what an old save reads as.
  Examples in the tree: `Biology::off` lists processes *off*, so a new process comes back on;
  `Talents::restore` defaults a missing talent to 50; `Goals::restore` leaves a new goal fresh.
- **A new file is optional**; missing means defaults. Unknown files and unknown fields are
  skipped (`files_and_fields_from_a_later_build_are_skipped`).
- **Content by name** (terrain, props, items, entity kinds via the oneof); the game's own state
  machines (heading, goal, task result, stage...) by prost enumerations mapped with exhaustive
  `match` both ways (`impl From<GoalId> for pb::GoalId` and back) — never `as`.
- **Refuse, never repair**: unknown name or enum value, out-of-range index, duplicate uid, a
  kind its uid disagrees with → `SaveError` naming the file and what (`error.rs`: `Invalid`
  inside a part, `.in_file(..)` at the top).
- A change that cannot be additive bumps `save::FORMAT` and needs a migration; a newer format
  is refused (`SaveError::NewerFormat`).
- **`src/save/testdata/format1.savegame` is checked in and must keep loading**
  (`a_format_1_save_still_loads_and_still_runs`). Never regenerate it for format 1; a new
  format adds `formatN.savegame` beside it (`write_the_format_1_fixture` shows how, `--ignored`).

## Adding state to the game — it must be saved

Every type with state has a `// --- saving ---` section in its own module (private fields):
`save(&self) -> pb::X` and `restore(..) -> Result<Self, Invalid>`. **Every `save` destructures
its struct exhaustively** (`let Walker { body, route, speed, meter: _, searching: _ } = self;`),
so adding a field breaks the build until it is saved or explicitly skipped. To add state:

1. the field → 2. a new tag in the `pb` message (default = "as before") → 3. `save` and
   `restore` in the owning module → 4. the side-by-side tests cover it if it affects behaviour.

Where the code is:

| State | Saved by |
| --- | --- |
| `GameState` scalars, fixtures, arena, restore order | `sim/saving.rs` (`save_world`, `save_entities` — parallel, `restore`, `RestoredEntity`) |
| an entity kind | `GameEntity::save` (**required**, no default) + `kinds::restore` (twin of `kinds::build`, checks the uid's kind) |
| a goal executor | `GoalExecutor::save`/`restore` (**required**); state is a `pb::Executor` variant (`Wander`, `Fetch`, `Seek`) |
| a routine | `Routine::save`/`restore` (`brain/routine.rs`) — matched by name and position |
| a task / action | `Task::save`/`restore` (`brain/task.rs`), `Action::save`/`restore` |
| brain parts | `brain/{mod,goal,perception,attention,memory,routines}.rs`, `goals/mod.rs` (`Seek`) |
| body | `walker.rs`, `path.rs` (`Path`, `Destinations`, `PathFinder`), `entity.rs` (`Body`), `biology/*`, `background.rs`, `identity.rs`, `inventory.rs`, `talents.rs` |
| world fixtures | `fridge.rs`, `door.rs`, `homes.rs`; light switches and boxes in `sim/saving.rs` |
| id RNG | `sim/rng.rs` — `WorldRng` (= xoshiro256++, the stream `SmallRng` always was), `state_of`/`from_state` |
| map | `map/save.rs` (`save_site`/`restore_site`, `save_chunks`/`restore_chunks`), `Chunk::from_parts` |

A new **kind** needs a `pb::Kind` variant; a new **task** a `pb::TaskKind` variant; a new
**executor** a `pb::Executor` variant if none fits. Brains are rebuilt by kind (`Brain::human()`)
and filled in, so a routine list that changes order or names refuses old saves — add routines
at the end, or teach `Brain::restore` to match by name.

## Fast

`save::snapshot` encodes on the caller's thread (the game's main thread, from `Res<Sim>`),
shards in parallel with rayon. `SaveFiles::to_zip` compresses every entry in parallel at
Deflate level 1 (zip 9's `ZipFileBuilder`, added with `add_prepared_file`) on a **dedicated
pool of half the cores** so the simulation's global pool is not starved. Loading inflates and
decodes shards in parallel. Measured, release, this machine (2026-10-17,
`cargo test --release saving_twenty_thousand -- --ignored --nocapture`):

| | raw | zipped | encode (main thread) | compress | unzip + load |
| --- | --- | --- | --- | --- | --- |
| 20000 humans, 256×256 | 13.5 MB | 4.7 MB | 22.6 ms | 7.8 ms | 34 ms |
| district 7, 53 units (debug build, QA) | | 34 KB | 2 ms | 2 ms | |

## The store (`save/store.rs`)

- `.saves/` (relative, like `maps/`), or `CROWD2X_SAVES`. `tools/qa.py` points it at a scratch dir.
- File name = local time `YYYY-MM-DD_HH-MM-SS` (`Moment`), `-2`, `-3`... on a clash; the
  player's name is in the manifest. Written whole or not at all (`map::write_atomic`).
- `list()` newest first by the manifest's time; an unreadable or newer-format file is listed
  with its error (so it can be deleted), not hidden.
- `prune_quick(QUICK_SAVES_KEPT = 4)`: quick saves told by the manifest flag, never the name.

## In the game (`game/saving.rs`)

- HUD, top right, a row under the world controls: **`save`** and **`quick save`**
  (`SaveControl`; QA can `press` them). **F5** quick saves.
- `save` opens the on-screen keyboard (scope 2) titled "save the game as", pre-filled with
  "<map> <hh-mm>", **world paused behind it**; enter/`done` saves, esc/B cancels; the speed it
  had comes back. The game's own key and pad bindings are gated by `ui::nav::not_typing` so
  typing a name does not pan, zoom or pause.
- Encoding happens in the system; a `save writer` thread zips, writes and prunes; when it
  finishes the log says `saved "name": N units, K KB, in X ms (a encoding, b compressing and writing) to path`.

## Loading (`src/saves.rs`, `AppState::Saves`)

- Main menu → **Load** → list: name (the button that loads it), map, when, world day/time,
  population, `delete` (confirmation starts on cancel). Esc → menu. Loading is **only** here.
- Reading happens on a `save reader` thread ("loading..."), then `open_game` hands the game
  screen `PendingWorld` (taken by `actors::build_world` instead of a fresh world),
  `CurrentMap` (the save's own map — the ground drawn — so the original map file need not
  exist) and `LoadedView` (applied by `saving::look_where_it_was` after `aim_camera_at_map`).
- `CROWD2X_LOAD=<path|newest>` does the same at startup and boots into the game
  (`debugger` skill).

## Tests

- `save::tests` — side-by-side continuation (crowd with unfinished searches + despawn/spawn,
  saved twice; busy district with sleepers, salvage, frozen, stacked, box/switch off), re-save
  gives identical files, every refusal, unknown files/fields skipped, the store (names,
  listing, quick pruning), the golden fixture, the `--ignored` timing.
- `map::save::tests` — site round trip, restored chunks = regenerated, refusals.
- `sim::rng` — `WorldRng` = `SmallRng`; state round trip.
- QA `qa/save_and_load.json` — name dialog (pauses, esc cancels), F5 and the button keep four
  quick saves, menu → Load → loads where it was, delete asks first. Step `expect_saves`.

Mutation-checked when written: breaking `PathFinder::restore` or `Schedule::restore` fails the
side-by-side tests.
