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
quits. The world keeps rendering behind it. Its one entry opens the **map editor**.

### Map editor

Two layers, deliberately built on two different sprite functions because they obey
different rules:

| | Background | Props |
| --- | --- | --- |
| Placement | snapped to a 48px grid, one tile per cell | free, at the cursor |
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
| `esc` | back to the menu |

The map is not despawned when you leave the editor, so visiting the menu never throws
away your work. There is no save format yet.

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
python3 tools/check_pixel_grid.py /tmp/bare.png 4
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

- **Humans** (`src/characters/human.rs`) are a layered paperdoll: a 48x48 body
  with eyes, clothes and hair stacked on top as child sprites. New outfits are
  a PNG in `assets/human/` plus one line in the relevant array.
- **Dogs** (`src/characters/dog.rs`) use a 4-frame idle atlas, with a separate
  mirrored sheet for facing left rather than `flip_x`, so left-facing art can be
  hand-tuned later.

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
| `assets/*.png` | 48px sprites and sheets (characters, props, tiles) |
| `assets/32/` | 32px tile variants |
| `assets/human/` | paperdoll layers (`human_base`, `clothes_*`, `eyes_*`, `hair_*`) |
| `assets/concept/` | 512px AI concept art and the Pixelorama source file — reference only, not loaded |

Tiled map files (`.tmx`, `.tiled-project`) were not imported; they are level data,
not art, and the map format for this project is not decided yet.
