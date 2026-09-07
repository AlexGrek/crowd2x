---
name: debugger
description: Capture and inspect real frames from the running crowd2x game. Use whenever a change affects what is drawn - sprites, layering, the pixel pipeline, animation, camera - and you need to see the actual output instead of assuming the build succeeding means it looks right. Also covers smoke-running the app and verifying pixel-perfect upscaling.
---

# Looking at what crowd2x actually renders

`src/debug.rs` (`DebugPlugin`) can pull a frame out of the running game and quit.
The window still opens — this is not headless — but nothing needs a human watching it,
so it works fine from a tool call.

## Capture one frame

```sh
cd /Users/vedmedik/dev/crowd2x
CROWD2X_SHOT=<scratchpad>/frame.png cargo run
```

Runs the game, waits 1.5s for assets to load, writes the PNG, exits 0.
Takes about 6 seconds. Then `Read` the PNG — you can see it.

**Always use `cargo run`, never `./target/debug/crowd2x`.** Bevy resolves `assets/`
relative to the manifest under cargo and relative to the executable otherwise; running
the binary directly produces a window full of nothing and a wall of `Path not found`.

## Environment variables

| Variable | Meaning | Default |
| --- | --- | --- |
| `CROWD2X_SHOT` | Write one screenshot here, then quit. Enables scripted mode. | off |
| `CROWD2X_SHOT_DELAY` | Seconds to run before capturing. Raise it if new assets are slow. | `3.0` |
| `CROWD2X_WINDOW` | Force the window to `WxH` before capturing. | window default |
| `CROWD2X_EXIT` | Quit after N seconds even with no capture. | off |
| `CROWD2X_STATE` | Boot straight into `menu` or `editor`. | `menu` |
| `CROWD2X_HIDE_UI` | Hide all `bevy_ui`, leaving only the canvas. | off |

The app starts on the main menu, so **a capture of the editor needs
`CROWD2X_STATE=editor`** — otherwise you are looking at the menu over the world.

`F12` in a normal `cargo run` saves to `screenshots/shot-<epoch-millis>.png`
(gitignored). That is for the human, not for you.

## Patterns

**Did my rendering change work?**
```sh
CROWD2X_SHOT=<scratchpad>/after.png cargo run
```
Read the PNG. Capture a `before.png` first if the change is subtle.

**Does it survive an awkward window size?** Sizes that are not multiples of
`PIXEL_SCALE` exercise the canvas-rounding and resize path:
```sh
CROWD2X_SHOT=<scratchpad>/odd.png CROWD2X_WINDOW=1002x602 CROWD2X_SHOT_DELAY=2 cargo run
```

**Does it still run at all?** No capture, no window fiddling, just start and stop:
```sh
CROWD2X_EXIT=3 cargo run
```
Exit code 0 and no `ERROR` lines means the schedule ran clean. Grep the output for
`ERROR` — missing assets and failed asset loads only ever show up there, never as a
build failure.

**Is the upscale still exact?** Capture with the UI hidden first — `bevy_ui` is drawn at
window resolution *on top of* the finished upscale, so any text on screen makes the check
report non-uniform blocks that have nothing to do with the pipeline:
```sh
CROWD2X_SHOT=<scratchpad>/frame.png CROWD2X_HIDE_UI=1 cargo run
python3 tools/check_pixel_grid.py <scratchpad>/frame.png 4
```
Checks that every source texel is a solid 4x4 block of identical pixels. Exits non-zero
if not, so it can gate a change. A reported phase offset like `x=3 y=3` is fine and
expected when the window is not a multiple of 4 — the canvas is centred, so the grid
starts a few pixels in. What matters is `PIXEL-PERFECT`, not the phase.

Run this after touching anything in `src/render.rs`, the window setup in `src/main.rs`,
or any sprite `Transform` that could land on a fractional coordinate.

## Reading the result

Sanity checks worth making on a captured frame:

- Characters visible at all? If the canvas is the flat clear colour, assets failed to
  load (check `ERROR` lines) or nothing is on `WORLD_LAYER`.
- Paperdoll layers aligned? Hair, eyes and clothes are separate 48x48 sprites that must
  sit exactly on the body.
- Edges crisp? Blurred edges mean something reintroduced filtering — most likely a lost
  `ImagePlugin::default_nearest()` or a sprite scaled by a non-integer.
- Layering right? Characters lower on screen draw in front; a character's own layers
  must never interleave with another's.

## Gotchas

- **A pure black PNG means you captured too early**, not that rendering is broken. The
  first frame does not reach the screen for a couple of seconds on a cold start, and the
  capture is silent about it — no error, just black. `CROWD2X_SHOT_DELAY=6` distinguishes
  a real blank frame from a slow one before you go looking for a rendering bug.
- Capture happens once per run. For two states, run twice.
- The delay is wall-clock from app start, not frames. On a cold `cargo build` the build
  time does not count against it.
- `save_to_disk` writes synchronously, then the app exits, so the file is always complete
  when the command returns.
- Do not leave debug systems in `main.rs`. Everything belongs in `src/debug.rs` behind
  the env vars, which is exactly why it was made permanent.
