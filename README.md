# crowd2x

A 2D pixel-art crowd simulation, built on [Bevy](https://bevyengine.org) 0.19.
The art is imported from an earlier prototype; the code is new.

## Running

```sh
cargo run            # debug, dependencies are optimised so it is playable
cargo run --release
```

Run through `cargo`, not the binary directly — Bevy resolves `assets/` relative to
the manifest under cargo, and relative to the executable otherwise.

`F12` saves a screenshot to `screenshots/` from anywhere.

## Screens

The app starts on a **main menu** — click an entry, or use `up`/`down` and `enter`; `esc`
quits. The map browser creates, plays, edits and manages saved maps. Saved games
load from the main menu; application preferences are under Settings.

### Map editor

Two layers, deliberately built on two different sprite functions because they obey
different rules:

| | Background | Props |
| --- | --- | --- |
| Placement | snapped to the 48px cell grid, one tile per cell | free, at the cursor |
| Depth | a single depth behind everything | `depth_for(y)`, same as characters |
| Painting | drag to fill | one click, one prop |
| Erasing | right-click the cell | right-click the nearest prop |

Because props are sorted by world Y exactly like characters are, a character walking
below a bed draws in front of it and one standing above it draws behind — the editor
does not need to know anything about the simulation for that to work.

| Key | |
| --- | --- |
| `tab`, `1`, `2` | switch layer |
| `q` / `e`, `[` / `]`, wheel | cycle through the layer's palette |
| left mouse | place |
| right mouse | erase |
| `WASD` / arrows | pan the camera |
| `esc` | save and return to the map browser |

Maps save as JSON. Leaving the editor saves changes; a failed save keeps the
editor open with the edits intact. `F5` retries a save.

### The shop and economy

Generated districts include a working store near the warehouse. Four shopkeepers
cover six-hour shifts, checking its central food reserve and minding the counter.
When the reserve reaches 30%, they call courier dispatch. A free steel, food or
store courier fetches one warehouse meal and brings it to the reserve. The store's
own courier works every day and helps the food factory between deliveries.

Every meal or dog dinner taken from an ordinary fridge consumes a small share
of this reserve. When it empties, fridges have water but no food until a delivery
arrives. Maps without a central reserve keep unlimited fridge food. Warehouse
transport is free; ordinary fridge purchases charge the buyer at the configured
price. Shopkeepers and store couriers earn $40 gross per worked shift by default,
paid through the district's normal payroll.

The full reserve has normalized capacity `1`; its initial 40% is `0.4`. Each
warehouse delivery adds `0.01`, supporting about 56 retail meals.

Edit `store.yaml` before starting a new game to change reserve capacity,
delivery size, consumption, starting fullness or the dispatch threshold. `CROWD2X_STORE` selects
another rules file. `economy.yaml` sets named wages, food prices, tax and care;
the in-game economy menu also changes these tariffs for the running world.
Saved games retain their own stock, rules, money, jobs and outstanding deliveries.

Select a worker and open **job** or **brains** to inspect its shift and current
work. **Statistics** includes shop employment and stored food. A waiting dispatch
needs a stocked warehouse meal rack and an available courier on duty. Needs can
interrupt deliveries; excess food at a full reserve stays with the courier and
returns to the warehouse. The full implementation contract is in
[the store documentation](.claude/skills/store/SKILL.md).

## Development

`src/debug.rs` can capture a frame and quit, so rendering changes can be checked
without anyone watching the window:

```sh
CROWD2X_SHOT=/tmp/frame.png cargo run                          # capture, then exit
CROWD2X_SHOT=/tmp/odd.png CROWD2X_WINDOW=1002x602 cargo run    # capture at a given size
CROWD2X_SHOT=/tmp/edit.png CROWD2X_STATE=editor cargo run      # skip the menu
CROWD2X_SPAWN=25 CROWD2X_STATE=game CROWD2X_MAP=<a map> cargo run   # with a crowd on it
CROWD2X_EXIT=3 cargo run                                       # smoke run, no capture

# The pixel grid check needs a frame with no UI in it: bevy_ui draws at window
# resolution over the finished upscale, so on-screen text is not on the grid.
CROWD2X_SHOT=/tmp/bare.png CROWD2X_HIDE_UI=1 cargo run
uv run tools/check_pixel_grid.py /tmp/bare.png 4
```

Two Claude Code skills in `.claude/skills/` document the rest: `debugger` for the
capture workflow, `dev` for the Bevy 0.19 API surface, asset and animation conventions,
and the architecture rules for scaling past a few hundred actors.

## Rendering pipeline

Everything is drawn twice, which is what makes the pixels exact:

1. A **world camera** (`RenderLayers` 0) renders the scene into an off-screen
   `Image` sized `window_physical / PIXEL_SCALE`, at 1 world unit = 1 canvas pixel.
2. An **upscale camera** (`RenderLayers` 1) draws that image to the window as a
   single sprite scaled by `PIXEL_SCALE`, with nearest-neighbour sampling.

Three details keep it honest:

- `ImagePlugin::default_nearest()` — no texture filtering anywhere.
- `with_scale_factor_override(1.0)` on the window makes logical and physical
  pixels identical, so the factor is exactly `PIXEL_SCALE` on HiDPI displays too.
- The world camera's `Transform` is snapped to whole pixels every frame
  (`CameraPan` holds the sub-pixel position), so panning cannot make sprites
  land between texels.

The canvas is rebuilt when the window is resized so the factor never drifts into
a stretch. Sizes that are not a multiple of `PIXEL_SCALE` round up, and the
slightly oversized canvas is centred — the pixel grid shifts by up to
`PIXEL_SCALE - 1` screen pixels but every texel is still an exact NxN block.

Verified: a 1280x720 frame contains 57,600 aligned 4x4 blocks, all uniform;
after resizing to 1002x602 the same holds at a 1-pixel phase offset.

Interface is the exception. The menu and the editor HUD are plain `bevy_ui`, drawn by
the upscale camera *over* the finished canvas, so they are composited at window
resolution and are deliberately not on the pixel grid. `UiScale` is set to
`PIXEL_SCALE`, so UI sizes are still written in canvas pixels — the same units as the
world, and `ui::FONT_BODY` is 6 rather than 24. UI has to live on that camera rather
than in the canvas: `bevy_ui` only computes a cursor position for cameras that target a
window, so `Interaction` would never fire on the world camera, which renders to an image.

## What renders today

Whatever has been painted in the editor, plus the actors the simulation says exist.

- **Humans** (`src/characters/human.rs`) are a layered paperdoll: a 16x16 body
  with eyes, clothes and hair stacked on top as child sprites. New outfits are
  a PNG in `assets/human/` plus one line in the relevant array.
- **Dogs** (`src/characters/dog.rs`) use a 4-frame idle atlas of 16x16 frames, with a
  separate mirrored sheet for facing left rather than `flip_x`, so left-facing art can
  be hand-tuned later.

Art is drawn at 16x16 and the game upscales it by a whole number — `characters::ART` is
16, `characters::CELL` is the 48-canvas-pixel cell it fills, and `ART_SCALE` is the 3
between them. A PNG is never stored pre-upscaled, so the scale stays the game's to choose.
A handful of imported props and two floors are still genuinely drawn at 48x48; they are
art debt to redraw, not a second supported resolution.

Depth is painter's order by world Y (`characters::depth_for`), so characters
lower on screen draw in front. Layer offsets within one character are small
enough that two characters can never interleave.

Nothing in `src/characters/` spawns anything. Who exists is the simulation's answer
(`src/sim/`, plain Rust with no `bevy::` imports), and `src/game/actors.rs` is the bridge
that keeps one sprite alongside each entity. To see a crowd, ask for one:

```sh
CROWD2X_SHOT=/tmp/crowd.png CROWD2X_STATE=game CROWD2X_MAP=<a map> CROWD2X_SPAWN=25 cargo run
```

## Assets

Imported wholesale from an earlier prototype, with the original layout preserved:

| Path | Contents |
| --- | --- |
| `assets/*.png` | 16px sprites and sheets (characters, props, tiles); a few unreduced 48px props |
| `assets/32/` | 32px tile variants — the 2x source the 16px tiles were reduced from |
| `assets/human/` | paperdoll layers (`human_base`, `clothes_*`, `eyes_*`, `hair_*`) |
| `assets/concept/` | 512px AI concept art and the Pixelorama source file — reference only, not loaded |

Tiled map files (`.tmx`, `.tiled-project`) were not imported; they are level data,
not art, and the map format for this project is not decided yet.
