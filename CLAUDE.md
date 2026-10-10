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
format, a seeded district generator, a lit and wired game screen that runs a simulation, and
a scripted QA harness that drives all of it. The simulation is one `GameState` advanced by one
function; every unit runs a brain (routines, goals, tasks, actions) over a body of switchable
biological processes (hunger, thirst, bladder, fun, energy, satisfaction).

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
`CROWD2X_STATE`, `CROWD2X_MAP`, `CROWD2X_DISTRICT`, `CROWD2X_EXIT`, ...) — the `debugger`
skill has the full list. A pure black capture almost always means the frame was grabbed too
early, not that rendering broke.

Controls: `WASD` / arrows pan, `q` / `e` zoom in the game, `+` / `-` game speed, `p` pause,
left click selects, `o` steps the x-ray, `F12` saves a screenshot to `screenshots/`.

## Which skill to load

| The task touches... | Load |
| --- | --- |
| Bevy APIs, assets, animations, performance, architecture decisions | `dev` |
| `src/render.rs`, window setup, zoom, sprites, art files, `src/characters/`, pixel grid | `renderer` (+ `debugger` to look) |
| `src/lighting/`, shaders, lamps, day/night, something lit wrong | `lighting` |
| Screens, menus, buttons, focus/navigation, text entry, the map browser | `interface` |
| The map editor, palettes, map windows, placing terrain/props/lamps | `editor` (+ `map-layers`) |
| The game screen: sprite bridge, culling, speed, camera, selection, panels, HUD | `game-screen` |
| The map data model, passability, catalogues, spawners, the file format | `map` |
| Layers (terrain/object/grid), the `LAYERS` table, x-ray overlay, a new layer or network | `map-layers` |
| Power: transformers, lines, boxes, wiring, powered props | `electrician` |
| Water: sewer, pipes, toilets that nobody uses | `plumber` |
| The seeded district generator | `map-generator` |
| `GameState`, passes, movement, entities, Commands, items, inventory, clock | `simulation` |
| What a unit wants or does: goals, tasks, actions, features, stats, processes | `brain-engineer` |
| Adding or changing a routine, a need's thresholds | `add-routine` (+ `brain-engineer`) |
| Seeing what is actually drawn; capturing frames | `debugger` |
| Writing, running or diagnosing `qa/*.json` tests, perf numbers | `qa` |

## Rules that hold everywhere

- **The simulation is plain Rust with no `bevy::` imports** (`src/map/`, `src/sim/`), testable
  with `cargo test` and no `App`. Bevy is only a renderer and input reader. New simulation
  logic goes in `src/sim/` behind `GameState` and `process_game_state` — add an entity kind,
  an `Intent`, a `Command` or an `Effect`; never a Bevy system that decides something or a
  `Component` holding agent state. An earlier prototype died at ~100 actors from violating
  this (`dev` skill, "Scaling to thousands of actors").
- **No per-agent scan of other agents, no allocation in a tick, determinism** (same seed,
  same world).
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
- **`main` returns `AppExit`**, or a failing QA test exits 0; check a new test fails when it
  should (`qa`).
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
src/game/               the game screen: actors, pool, speed, hud, panels   [game-screen]
src/map/                map model, format, grid layers, Supply              [map, map-layers]
src/map/utilities.rs    power and water networks                            [electrician, plumber]
src/map/district.rs     the district generator                              [map-generator]
src/sim/                GameState, passes, entities, clock, items           [simulation]
src/sim/brain/, biology/  the mind and the body                             [brain-engineer]
src/qa/, qa/, tools/qa.py  scripted QA                                      [qa]
src/debug.rs            env-var driven capture harness                      [debugger]
src/awake.rs            macOS: hold the display awake during a run
tools/check_pixel_grid.py, art_scale.py, wire_map.py   pixel grid, art size, wiring
qa-screenshots/, qa-perf/, qa-observe/, maps/, screenshots/   gitignored output
assets/                 imported wholesale from an earlier prototype
```
