# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**This file is a router, not a manual.** The architecture lives in skills, one per system.
Do not read the whole codebase or every skill: find the task in the table below, load the
skill(s) it names, and read only the files those skills point to. A skill's description says
when it applies; if two match, load both.

## What this is

crowd2x is a 2D pixel-art crowd simulation built on Bevy 0.19 (art imported from an earlier
prototype; all code is new): a pixel-perfect render pipeline, a menu, a map browser, a
six-layer map editor (floor, props, lamps, ceiling, power grid, sewer pipes), a saved map
format, a seeded district generator, an endless procedural wasteland round every map
(streamed in chunks as the camera and the crowd reach it), a lit and wired game screen that
runs a simulation, saved games (`.saves/*.savegame`, a zip of Protobuf files, loaded from the
main menu), and a scripted QA harness that drives all of it. The simulation is one
`GameState` advanced by one function; every unit runs a brain (routines, goals, tasks,
actions) over a body of switchable biological processes (hunger, thirst, bladder, fun,
energy, satisfaction, health). A need left at its worst — starving, parched, spent — drains
health, and at none a unit dies: it lies where it fell (drawn turned 90°), thinks and moves no
more. Each walks at a pace of its own, a little faster or slower than the next, and breaks
into a run when what it is after is urgent — a need past 75, or late for its desk or post.
Some have a job, nine to five, Monday to Friday (day 1 is a Monday):
an office worker keeps its own desk in the bank; a scrap collector brings scrap metal in from
the heaps in the wasteland and stores it in its own bin in the district's warehouse, and
before every trip out gets ready — eats, drinks, plays and uses the toilet until every need is
85% met — so that no need calls it back in from the far end of a trip. A
district is walled in, with eight gates, and each gate's guard post is kept round the clock by
three guards on eight-hour watches, every day, each sleeping by its watch; undertakers on the
same watches carry the dead to the crematoria in the district's crematorium. Four public
restaurants, one to each quarter of a district, have fridges and toilets anybody may use. A
steel factory beside the warehouse smelts the scrap into steel: its steelworkers smelt scrap
from its hoppers at their furnaces, and its couriers carry scrap from the warehouse to the
factory and the steel back to the warehouse's rack. Every worker wears its job's uniform while
its shift is due. A farm on the edge nearest the wasteland's good soil (meadow) has eight
farmers, nine to five every day but Sunday: each sows a field of ten crops of wheat, one to a
plot, tends every crop once a day for quality, and six days and four growth stages later
harvests it ripe and stores the wheat in its own grain bin. A power plant on the edge powers
the whole district — its two generators are the only source of power — and its control panel
is kept round the clock by three power plant workers on the guards' watches, who just stand
by it for now. A playzone — an arcade — on the residential lot nearest the district's middle has
six arcade cabinets, the quickest fun there is for the bored (a game is a third of a go at a
computer), and one central computer run by four administrators: a day shift (10-16) and an
evening shift (16-22), each kept by two who take turns by the day (Mon, Wed, Fri, Sun and Tue,
Thu, Sat) — **and the cabinets work only while an administrator is on duty and at that
computer**. A palace filling the middle block, on the wide main street that runs gate to
gate, seats the district's one king, who holds court at its
throne ten to four but Sundays, in purple robes and a crown always, walking at a stately pace;
the major decisions are to be the king's. A crop is in the way like a heap of
scrap, and a click on one says how it is growing. The game screen's `statistics` menu counts the
living, the warehouse's, the factory's and the farm's stock, the fields, the workers per job and
the dead.

Cash and a district budget now track wages, taxes, care packages and fridge purchases.
Office workers do the payroll; the palace accountant reports daily and weekly balances.
The economy menu changes the running game?s tariffs, whose defaults are economy.yaml.

## Commands

```sh
cargo run              # debug build; deps are optimized (opt-level 3) so it's playable
cargo run --release
cargo build
cargo test             # the plain-Rust parts: map, coordinates, navigation, scripts
uv run tools/qa.py    # the scripted QA tests in qa/, against the real binary
uv run tools/qa.py --release   # ...optimised, which is the only profile to quote a timing from
```

**Always run through `cargo`, never the raw binary** (`./target/debug/crowd2x`) — Bevy
resolves `assets/` relative to the manifest under cargo, and relative to the executable
otherwise, so a direct run produces a blank window and asset-load errors.

Unattended runs and screenshots are driven by `CROWD2X_*` env vars (`CROWD2X_SHOT`,
`CROWD2X_STATE`, `CROWD2X_MAP`, `CROWD2X_DISTRICT`, `CROWD2X_LOAD`, `CROWD2X_EXIT`, ...) — the `debugger`
skill has the full list. A pure black capture almost always means the frame was grabbed too
early, not that rendering broke.

Controls: `WASD` / arrows pan, `q` / `e` zoom in the game, `+` / `-` game speed, `p` pause,
left click selects, `o` steps the x-ray, `F5` quick saves the game, `F12` saves a screenshot
to `screenshots/`.

## Which skill to load

| The task touches... | Load |
| --- | --- |
| Bevy APIs, assets, animations, performance, architecture decisions, rust-analyzer and the `LSP` tool | `dev` |
| `src/render.rs`, window setup, zoom, sprites, art files, `src/characters/`, pixel grid | `renderer` (+ `debugger` to look) |
| `src/lighting/`, shaders, lamps, day/night, the sun (`src/sim/sun.rs`), something lit wrong | `lighting` |
| Screens, menus, buttons, focus/navigation, text entry, the map browser | `interface` |
| The map editor, palettes, map windows, placing terrain/props/lamps | `editor` (+ `map-layers`) |
| The game screen: sprite bridge, culling, speed, camera, selection, panels, HUD, the statistics menu, crops drawn and the crop panel | `game-screen` |
| The map data model, passability, catalogues, spawners, the file format | `map` |
| Layers (terrain/object/grid), the `LAYERS` table, x-ray overlay, a new layer or network | `map-layers` |
| Power: transformers, the power plant's generators, lines, boxes, wiring, powered props, street lights and their controllers, light switches | `electrician` |
| Water: sewer, pipes, toilets that nobody uses | `plumber` |
| Doors, locks, property and bed ownership, who may enter a house | `doors` (+ `brain-engineer`) |
| The seeded district generator, its perimeter wall, gates and guard posts | `map-generator` |
| The endless world past a map's edge: the wasteland generator, chunks, streaming and reclaiming, routes, occupancy and the camera off the site, its tiles | `wasteland` (+ `map`) |
| `GameState`, passes, movement, entities, the census, the route budget, Commands, items, inventory, clock | `simulation` |
| **Adding a kind of thing**: a job, a building every district has, an item, a feature or store, a goal, a task, an effect, a new part of the world's state — the touch list | `add-content` (+ the owning skill) |
| What a unit wants or does: goals, tasks, actions, features, stats, processes; how fast it walks (its pace, urgency, hurrying) | `brain-engineer` |
| Adding or changing a routine, a need's thresholds | `add-routine` (+ `brain-engineer`) |
| Jobs: who works where and when, shifts, watches, weekdays, commutes, getting ready before a trip out, sleeping by one's shift, desks, scrap bins, guard posts and crematoria (`Workplaces`, `Bins`), the steel factory (steelworkers, couriers, stores of scrap and steel), the farm (farmers, fields of wheat, sowing, tending, harvesting, grain bins), the power plant's workers and control panel, the playzone's administrators (four shifts at one computer, two groups on alternate days) and the arcade cabinets that work only while one is at it, the king and the palace's throne (a job's `Bearing`: gait, robes always, a crown), hiring order, hire/dismiss, the work routine, collecting scrap, undertakers and the dead (`Corpses`), the job menu | `jobs` (+ `brain-engineer`) |
| Health and death: the `Health` process, dying, the dead (`Body::dead`, `Effect::Died`) | `brain-engineer` (+ `simulation`, `jobs`) |
| Drawing new art: hair, dresses, outfits, work uniforms, sprites, tiles; where art files go | `pixel-artist` (+ `renderer`) |
| Seeing what is actually drawn; capturing frames | `debugger` |
| Writing, running or diagnosing `qa/*.json` tests, perf numbers | `qa` |
| Money, budget, wages, taxes, care, fridge prices, timesheets, payroll, accountant, economy.yaml or economy menu | `economy` (+ `jobs`, `savegame` as needed) |
| Saving and loading: the `.savegame` format, its Protobuf schema, the Load screen, save / quick save; **any new simulation, entity, brain or map state (it must be saved)** | `savegame` |

## Rules that hold everywhere

- **The simulation is plain Rust with no `bevy::` imports** (`src/map/`, `src/sim/`), testable
  with `cargo test` and no `App`. Bevy is only a renderer and input reader. New simulation
  logic goes in `src/sim/` behind `GameState` and `process_game_state` — add an entity kind,
  an `Intent`, a `Command` or an `Effect`; never a Bevy system that decides something or a
  `Component` holding agent state. An earlier prototype died at ~100 actors from violating
  this (`dev` skill, "Scaling to thousands of actors").
- **No per-agent scan of other agents; keep hot loops allocation-free; preserve determinism**
  (same seed, same world). Routing reuses scratch and route buffers, with allocation only
  when those buffers grow or an exhausted search retains its continuation (`brain-engineer`).
- **Anything that should render carries `WORLD_LAYER`**, and nothing may land on a fractional
  pixel; verify with `tools/check_pixel_grid.py` after touching rendering (`renderer`).
- **Art is 16x16 and never stored pre-upscaled**; the engine scales it (`renderer`).
- **Art and data are linked by name, never by index** — palettes, map files and catalogues
  (`map`, `editor`).
- **`OnEnter` for the initial state runs before every `Startup` system**; anything a screen
  needs before it goes in `Plugin::build` (`interface`).
- **Bevy 0.19 is newer than most training data.** Verify APIs against the vendored source
  (`~/.cargo/registry/src/*/bevy_*-0.19.1/src/`), not memory (`dev` has the gotchas). Features
  are `default-features = false, ["2d", "ui", "png", "jpeg"]` — find the right feature rather
  than switching to defaults. `bevy/dynamic_linking` does not work on this version.
- **Find Rust code with the `LSP` tool, not text search.** rust-analyzer runs behind it (the
  `rust-analyzer-lsp` plugin in `.claude/settings.json`): a symbol's definition, its
  references and implementations, the type under a position, a file's symbols, a symbol by
  name across the workspace, who calls a function. Use it before Grep or reading a file whole
  — this is how to read only what a skill points to. Go-to-definition on a Bevy item lands in
  the vendored 0.19.1 source, which is how to verify an API. Grep stays the tool for what is
  not a Rust symbol: strings, comments, art and prop names, JSON, skills. After every `.rs`
  edit read the diagnostics it reports and fix what the edit caused before moving on; they
  are a hint and `cargo build` / `cargo test` is still the check. If the tool errors (no
  server, as in a cloud session), fall back to Grep and say so (`dev`).
- **`main` returns `AppExit`**, or a failing QA test exits 0; check a new test fails when it
  should (`qa`).
- **New state is saved state.** A field added to the simulation, an entity kind, a brain part,
  a task or the map needs a tag in `src/save/pb/` and a `save`/`restore` beside it; every
  `save` destructures its struct exhaustively so the build says so (`savegame`). A loaded
  world must carry on bit for bit as the saved one would have, and a test checks it.
- **Keep the skills current.** A change to a system updates the skill that owns it in the same
  piece of work; a new system gets a skill and a row in the table above. CLAUDE.md stays a
  router — put detail in the skill, not here.

## Module layout (owning skill in brackets)

```
src/main.rs, render.rs  app/window setup; the pixel-perfect pipeline       [renderer]
src/characters/, animation.rs  how characters and items are drawn           [renderer]
src/lighting/           the lightmap; shaders in assets/shaders/            [lighting]
src/view.rs             VisibleArea - what is on the canvas                 [game-screen]
src/state.rs, ui/, menu.rs, browser.rs  screens, focus, keyboard, browser   [interface]
src/editor/             the editor, LAYERS, map windows, grid overlay       [editor, map-layers]
src/game/               the game screen: actors, pool, speed, hud, panels, crops  [game-screen]
src/map/                map model, format, grid layers, Supply              [map, map-layers]
src/map/utilities.rs    power and water networks                            [electrician, plumber]
src/map/mount.rs, src/sim/light_switch.rs  wall-mounted props; light switches and their rooms  [electrician]
src/map/district/       the district generator; a module per lot building  [map-generator]
src/map/wasteland.rs, chunk.rs  the endless ground round every map; chunks    [wasteland]
src/sim/                GameState, passes, entities, clock, items           [simulation]
src/sim/catalogue.rs    catalogue!: a kind enum and a row per kind          [add-content]
src/sim/facilities.rs, effect.rs  the world's parts; Effect and its apply   [simulation]
src/sim/census.rs, occupancy.rs  who is in which chunk; who stands in which cell  [simulation, wasteland]
src/sim/brain/, biology/  the mind and the body                             [brain-engineer]
src/sim/door.rs, property.rs  doors, locks, who owns which room              [doors]
src/sim/job.rs          jobs, shifts, watches, who works which workplace    [jobs]
src/sim/bins.rs         the stores: scrap bins, steel racks, hoppers, pallets, grain bins [jobs]
src/sim/fields.rs       the crops of wheat: sown, growing, tended, harvested  [jobs, map]
src/sim/corpses.rs      who lies dead where, for the undertakers            [jobs, simulation]
src/sim/playzone.rs     which arcade cabinets work: an administrator at the computer [jobs, simulation]
src/sim/economy.rs      cash, budget, tariffs, accounts and daily books      [economy]
src/game/economy.rs     live tariff menu and YAML loading                    [economy, game-screen]
src/sim/statistics.rs   the world in numbers; the dead counted (Tally)      [game-screen, simulation]
src/save/               the .savegame: Protobuf schema (pb/), zip, store    [savegame]
src/sim/saving.rs, src/map/save.rs  the world and the map into a save and back  [savegame]
src/game/saving.rs, src/saves.rs  save / quick save; the Load screen       [savegame, interface]
src/qa/, qa/, tools/qa.py  scripted QA                                      [qa]
src/debug.rs            env-var driven capture harness                      [debugger]
src/awake.rs            macOS: hold the display awake during a run
tools/check_pixel_grid.py, art_scale.py, wire_map.py   pixel grid, art size, wiring
tools/human_art.py      draws the paperdoll's hair and outfits     [pixel-artist]
tools/door_art.py       draws the door leaves                               [doors]
tools/street_art.py     draws the street lamp and its controller            [electrician]
tools/switch_art.py     draws the light switch (on and off)                 [electrician]
tools/ground_art.py     draws the pavement, road and grass tiles            [pixel-artist]
tools/wasteland_art.py  draws the wasteland's dirt, scrub, meadow, mud, water [wasteland]
tools/scrap_bin_art.py  draws the warehouse's scrap bin, empty to heaped     [pixel-artist]
tools/guard_post_art.py draws the guard post in the district's wall         [pixel-artist]
tools/crematorium_art.py draws the crematorium the undertakers bring the dead to [pixel-artist]
tools/tv_art.py         draws the TV, dark and showing a picture               [pixel-artist]
tools/steel_factory_art.py draws the furnace, scrap hopper, steel pallet and rack [pixel-artist]
tools/farm_art.py       draws the grain bin and the wheat at its four stages     [pixel-artist]
tools/power_plant_art.py draws the generator and the control panel              [pixel-artist]
tools/palace_art.py     draws the palace's throne                               [pixel-artist]
tools/playzone_art.py   draws the arcade cabinet, the playzone's computer and the administrator's uniform [pixel-artist]
qa-screenshots/, qa-perf/, qa-observe/, maps/, screenshots/, .saves/   gitignored output
assets/                 imported wholesale from an earlier prototype
```
