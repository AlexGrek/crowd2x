---
name: qa
description: Write and run scripted QA tests for crowd2x - JSON test definitions that drive the real binary through menus, the map browser, the editor and the game with keyboard, gamepad and mouse input, assert on outcomes, measure how long a crowd takes to simulate and draw, and take screenshots. Use when changing anything a person interacts with (screens, navigation, focus, input, the editor, the game camera and zoom, saving and loading maps), when adding a regression test for an interface bug, when writing or reading a performance test, or when a qa/ test fails and needs diagnosing.
---

# Scripted QA for crowd2x

`cargo test` covers the plain-Rust parts (map, coordinates, navigation maths) and the
`debugger` skill photographs a frame. Neither answers the question that decides whether an
interface works: *if someone presses these buttons in this order, does the right thing
happen?*

A QA test is a JSON file. The real binary replays it as input — window, renderer, states,
plugins and all — checks what happened, and exits with a status.

```sh
cd /Users/vedmedik/dev/crowd2x
python3 tools/qa.py                      # every test in qa/, one process each
python3 tools/qa.py qa/create_map.json   # one test
python3 tools/qa.py -v                   # stream the game's log while it runs
python3 tools/qa.py --release            # optimised build, for a timing worth quoting
CROWD2X_QA=qa/create_map.json cargo run  # by hand; set CROWD2X_MAPS too (see below)
```

Each test takes about 8 seconds — a performance test half a minute. The runner gives every
test its own maps directory (`target/qa/<test>/maps`), its own screenshot directory and its
own `qa-perf/<test>.json`, and forces a 1280x720 window so coordinates are stable.

**Run `tools/qa.py`, not the binary, unless you are debugging one test.** Running the
binary by hand without `CROWD2X_MAPS` points the test at `maps/` — a test that deletes a
map will delete a real one.

## Anatomy of a test

```json
{
  "name": "create a map from the browser",
  "state": "maps",
  "given": { "maps": ["alpha", { "name": "office", "file": "qa/fixtures/walled_room.json" }] },
  "open": "office",
  "steps": [
    { "note": "what these steps are for" },
    { "name": "studio" },
    { "press": "create" },
    { "expect_state": "editor" },
    { "shot": "the new map" }
  ]
}
```

| Field | Meaning | Default |
| --- | --- | --- |
| `name` | What the test is about. Appears in the log. | required |
| `state` | Screen to start on: `menu`, `maps`, `editor`, `game`. | whatever the app starts on |
| `given.maps` | Maps that exist before the test runs. | none |
| `open` | A map to have open when the app starts, in `state`. | the scratch map |
| `steps` | The session. | required |
| `settle` | Seconds before the first step, for startup. | `1.0` |
| `gap` | Seconds between steps — several frames, so a press lands and its consequences settle. | `0.1` |
| `shot_delay` | Seconds either side of a screenshot. | `0.5` |
| `timeout` | Whole-run limit; a stuck test fails instead of hanging. | `60` |
| `vsync` | Whether the window waits for the display. Turn it **off** to measure frames. | `true` |

Unknown fields and unknown step names are **errors**, not ignored: a typo that silently
skipped a step would make a test pass by not testing anything.

## Fixture maps

Three ways to say a map exists before the test starts, in `given.maps`:

```json
"alpha"                                                  // an empty 8x6 map
{ "name": "office", "file": "qa/fixtures/walled_room.json" }  // a map kept on disk
{ "name": "inline", "map": { "version": 1, "size": { ... }, ... } }  // written inline
```

Use a bare name for anything about the *list* (browsing, copying, deleting), a `file` for
a fixture worth looking at or sharing between tests, and an inline `map` when the test
depends on exactly which cells are painted — that keeps the map and the assertions about
it in one place. A file or an inline document is loaded through the real map format, so a
fixture that has drifted fails at startup with the format's own error.

Remember terrain rows are written **bottom-up**: row 0 of the document is y 0.

`"open": "<name>"` starts with that map loaded, which is how to test the editor or the
game without clicking through the browser first. Which screen it opens *in* is `state` —
`editor` or `game`. A script asking for a screen this build does not have stops the run
with `this build has no "credits" screen; it has: menu, maps, editor, game`, so a test
written ahead of a feature says so plainly and starts working when the feature lands.

## Steps

Two levels, and a test is expected to mix them.

### Intent — what the session is doing

Addressed by *label*, so they survive a button moving, a palette being reordered, or a
list growing a row. **Reach for these unless the input itself is what is under test.**

| Step | Does |
| --- | --- |
| `{"goto": "maps"}` | Go straight to a screen. |
| `{"focus": "beta"}` | Move the highlight to the widget with this label. |
| `{"press": "create"}` | Choose the widget with this label. |
| `{"name": "office"}` | Put a name in the name field, as if typed. |
| `{"tool": "wall brown"}` | Select a palette entry, on whichever layer it lives. |
| `{"cursor_cell": {"x": 3, "y": 2}}` | Put the editor cursor in the middle of a cell. |
| `{"cursor": {"x": 168, "y": 120}}` | ...or at a world position, in pixels. |
| `{"spawn": {"kind": "human", "x": 3, "y": 2}}` | Put an entity in the world: `human` or `dog`. |
| `{"tick": 200}` | Apply pending spawns, then advance exactly this many steps, immediately. |
| `{"note": "..."}` | Say what the next steps are for; goes to the log. |

`press` **refuses an ambiguous label** rather than guessing — every row of the file list
has an `edit` button — so walking a list uses arrow keys and then `press` on something
unique, or `focus` on the row's name first.

It addresses the focusable widgets of the layer that currently owns input, **and** the
buttons a screen drives itself: the game's two corners (`slower`, `faster`, `pause`,
`menu`, and the zoom's `-` and `+`) are deliberately outside the focus system, because
the arrows pan the camera there and `A` zooms, and they are still pressable by name.

`spawn` and `tick` only mean anything on the game screen, and say so rather than passing
quietly if the test is somewhere else. `spawn` pushes onto the same command queue the game
uses, so it is applied by the *next* tick and not by being asked for — a test that spawns
and then asserts without a `tick` in between is asserting on an empty world.

`tick` runs one **spawn pass** and then N **processing passes**, so however many ticks you
ask for, a pending spawn happens exactly once. It ignores the game speed — `tick` means
exactly this many steps of world, and a test that wants to check the speed asserts on
`expect_speed` and on what the game does with its own fixed steps. `{"tick": 0}` does nothing at all, spawn
pass included; the commands stay queued for the game's own fixed-step tick.

**`tick` runs the steps by hand rather than waiting for them.** `FixedUpdate` advances at
whatever rate the frame allows, so `{"wait": 1.0}` would assert on however many ticks this
machine happened to manage — fine on a fast desktop, a flake on a loaded CI box. `{"tick":
200}` is always exactly 200.

### Input — the devices themselves

The only way to test that the devices work: that a gamepad reaches every button, that
typing lands in the field and not in the palette.

| Step | Does |
| --- | --- |
| `{"key": "enter"}` | Tap a key: `enter`, `esc`, `tab`, `backspace`, `up`, `f5`, `a`, `7`, `+`, `-`. |
| `{"hold_key": {"key": "d", "seconds": 0.5}}` | Hold one — panning, dragging. |
| `{"type": "office"}` | Type text, one character at a time. |
| `{"pad": "south"}` | Tap a gamepad button: `south`/`a`, `east`/`b`, `north`, `west`, `dpad_down`, `start`, `l1`, `r1`. |
| `{"hold_pad": {"button": "south", "seconds": 0.3}}` | Hold one. |
| `{"stick": {"side": "left", "x": -1, "y": 0, "seconds": 0.4}}` | Push a stick, then let go. |
| `{"mouse": {"x": 136, "y": 114}}` | Move the pointer, in **canvas** pixels from the top left. |
| `{"click": {"button": "right", "seconds": 0.2}}` | Click; `button` defaults to left. |
| `{"wheel": -3}` | Scroll, in list rows; positive is up. |
| `{"wait": 0.5}` | Do nothing. |

Input is injected where the OS would have put it. Keys press `ButtonInput` *and* send a
`KeyboardInput` message, because the game reads both. A gamepad is spawned and connected
through `RawGamepadEvent`, so `bevy_input` builds the real `Gamepad` component. The
pointer moves by setting the window's cursor position — the same field `bevy_ui`'s focus
system reads — so a click goes through genuine hover-and-click.

### Assertions

| Step | Checks |
| --- | --- |
| `{"expect_state": "editor"}` | Which screen is up. |
| `{"expect_zoom": 6}` | The zoom, in screen pixels per canvas pixel. |
| `{"expect_speed": "x2"}` | How fast the game is running: `x1`, `x0.25`, or `paused`. |
| `{"expect_focus": "create"}` | The focused widget's label. |
| `{"expect_map": "office"}` / `{"expect_no_map": "office"}` | A saved map exists, or does not. |
| `{"expect_tile": {"map": "office", "x": 3, "y": 2, "terrain": "wall brown"}}` | A cell of the **saved** map. |
| `{"expect_entities": 3}` | How many entities the simulation holds. |
| `{"expect_sprites": 3}` | How many actor sprites actually exist in the world. |
| `{"expect_log": "spawned dog"}` | That the simulation said something containing this. |
| `{"expect_under": {"measure": "1000 humans", "ms": 1.0}}` | A measurement's median sample came in under a budget. |
| `{"expect_scaling": {"from": "100 humans", "to": "1000 humans", "slack": 1.5}}` | Cost per entity did not grow with the crowd. |

A failed assertion stops the test — the steps after it were written for a state the app is
no longer in — and the run exits non-zero.

`expect_entities` and `expect_sprites` are two assertions on purpose. The simulation
holding three entities and the screen showing three actors are separate claims, and the
second is the one that catches a renderer that has quietly stopped keeping up — which is
the failure a screenshot is worst at showing, because a missing sprite looks like an
actor that walked off the edge of the view.

`expect_log` reads the log *panel*'s lines, not the queue. The queue is drained as it is
displayed, so asking it directly would be a race with the system emptying it — and the
panel only keeps the last handful, so assert on something recent.

`expect_tile` reads the file, not the scene, so "I painted a wall" is only true once it has
been saved. The editor saves on leaving and on `f5`.

`expect_zoom` exists because zooming changes the *size of the canvas* rather than the
scale of a camera, so a screenshot cannot be asked how far in it is without counting
texels. The game screen is the only one that zooms, and it resets to `4` on the way out.

`expect_speed` takes the string the readout shows rather than a number and a flag, because
`paused` and `x1` are the same multiplier and a different game. It resets to `x1` on the
way out, like the zoom, so a test that leaves a map paused does not poison the next one.

## Performance tests

The same harness, timing itself. Three steps measure, two assert, and the numbers are
written to `qa-perf/<test>.json` and printed by `tools/qa.py` whether the test passed or
failed — **the measurement is the deliverable**, and the assertions are a floor under it.

| Step | Does |
| --- | --- |
| `{"populate": {"kind": "human", "count": 1000}}` | Queue a whole crowd, spread over the cells that can be stood on. |
| `{"measure": {"name": "1000 humans", "ticks": 200}}` | Time that many processing passes: the simulation alone, no renderer. |
| `{"measure_frames": {"name": "1000 actors", "seconds": 2.0}}` | Time whole frames for that long: sim, sprite sync, UI and render. |

`populate` is the bulk form of `spawn` and takes the same route — the commands go on the
queue the game uses, so they are applied by the next `measure` or `tick`, and the cost of
spawning a crowd is paid by the pass that really does it. `measure` runs one spawn pass
(untimed, since it runs once however many ticks were asked for) and then times each
processing pass separately.

Every measurement keeps the whole distribution — best, median, mean, p95, worst, and
microseconds per entity — because the mean is the one number that hides a stutter.

**The two assertions read different statistics, on purpose.** `expect_under` judges the
**median**: a budget asks what the game typically does. `expect_scaling` divides the
**fastest** samples: interference only ever adds time, so the quickest tick observed is the
one that ran closest to undisturbed, and a ratio of two medians inherits the noise of both.
With medians, three consecutive runs of an unchanged build gave 0.52x, 0.68x and 1.60x, and
the third failed a check the code had not earned.

### Assert on scaling, not on milliseconds

**`expect_scaling` is the assertion to reach for.** How many microseconds a tick takes
depends on the machine, the build profile and what else is running; how that time *grows
with the crowd* barely does. Doubling the entities should about double the work, so a
ratio near or below `1.0` is healthy, and a ratio above it means each entity is getting
more expensive as the crowd grows — which is the shape of the failure this project is
built around, a per-agent scan over every other agent.

**How much slack depends on the cache.** A hundred entities fit in L1 and a thousand do
not, so that pair moves by 1.3-1.8x between runs of an unchanged build and wants a slack
near `3.0`; two crowds that are both past the cache — a thousand against four thousand —
hold much tighter and can be held to `1.5`. Either still catches what matters, because a
quadratic is 10x over the same range. Compare crowds an order of magnitude apart.

`expect_under` is the blunt one: a **wall, not a target**. Set it several times above what
the machine does today so it catches an order of magnitude rather than a loaded laptop,
and put the observed number in a `note` next to it so the next reader knows what it was
set against.

### Two things that would otherwise make the numbers lies

- **Vsync caps every frame at the refresh rate.** A game that takes 2ms a frame and one
  that takes 15ms both present every 16.7ms, so `measure_frames` under vsync measures the
  monitor and only notices a regression after the game has already dropped below 60Hz. Set
  `"vsync": false` in any test that measures frames.
- **A debug timing is not a speed.** This crate builds at `opt-level = 1` in debug (its
  dependencies at 3). Debug numbers are fine for *scaling* — the ratio is what matters —
  and useless as an answer to "how fast is it". Run `python3 tools/qa.py --release` before
  quoting one. The profile and the vsync setting are recorded in the report for exactly
  this reason.

### What the two that exist measure

`qa/perf_simulation.json` times ticks from an empty world up to 5000 actors on
`qa/fixtures/plaza.json` (48x32, walls and pillars). `qa/perf_rendering.json` times frames
with up to 1500 actors on screen, and photographs the crowd.

Where they stood on an M3 when they were written:

| | debug | release |
| --- | --- | --- |
| a tick, 4000 wandering humans | 0.04ms | 0.04ms |
| a frame, 1500 drawn actors | 1.1ms | 0.9ms |

Two things worth knowing from that. The tick barely moves between profiles — the wander is
trivial enough that `opt-level = 1` already handles it, which will stop being true as soon
as there is pathfinding, so re-measure rather than assuming. And a frame costs about 0.6ms
before any actor exists at all: at 1500 actors the crowd is a fifth of the frame, and every
one of them has a sprite whether or not it is on the canvas, which is the culling and
pooling gap `game/actors.rs` documents in its own header.

## Screenshots

```json
{ "shot": "the file list" }
```

Named, not pathed: that writes `qa-screenshots/<test>/01-the-file-list.png`, numbered in
the order the shots were taken. A test can take as many as it likes. `tools/qa.py` wipes
each test's directory before the run and reports what came out; the whole directory is
gitignored.

A capture that beats the renderer to the frame comes back as one flat colour rather than
as an error, and how long to wait is not knowable. So the harness inspects what it
captured and retakes it until there is something in it, then gives up and says so. **If
every shot in a run is blank, the window is not being presented** — a sleeping or locked
display, or no display at all. That is an environment problem, not a test failure.
`src/awake.rs` holds a macOS power assertion to stop the display sleeping mid-run, but
nothing in the process can draw to a locked screen.

Read the PNGs afterwards — you can see them.

## Adding a test

1. Write `qa/<what_it_checks>.json`. Name it after the behaviour, not the mechanism.
2. Start from the intent level; drop to input steps for the thing under test.
3. **Make it fail first.** Break an expectation deliberately, run it, and check the run
   reports a failure. A QA test that cannot fail is worse than none, and this suite has
   already passed vacuously once (see below).
4. Put it back, run `python3 tools/qa.py`, and check the screenshots if it takes any.

## When a test fails

The runner prints the failing step, its number and what it expected. `-v` streams the
whole log, including the `note` lines, which is usually enough to see how far it got.

Common causes, in the order worth checking:

- **`no widget on this screen is labelled "x"`** — the app is not on the screen the test
  thinks it is. Add `{"expect_state": ...}` earlier to find out where it went. Pressing a
  map's *name* in the browser plays it; `edit` is the button beside it.
- **`N widgets are labelled "x"`** — the label repeats on every row; use arrow keys and
  `expect_focus`, or `focus` the row by its unique name first.
- **A tile check fails but the screenshot looks right** — the map was not saved. The
  editor saves on leaving and on `f5`.
- **Everything after one step fails** — that step changed the screen and the rest of the
  test was written for the old one.

## Things this suite learned the hard way

Each of these was a real bug the harness found, and each is now a rule:

- **`main` returns `AppExit`.** Discarding it makes every failing test exit 0 and the whole
  suite pass vacuously. This happened.
- **Fixtures are written in `Plugin::build`, not `Startup`.** The browser lists the maps
  directory in `OnEnter`, which for the *starting* screen runs before every `Startup`
  system — see the ordering trap in the `dev` skill.
- **Input belongs to the screen it was made on.** Messages outlive their frame;
  `nav::drop_input_from_the_last_screen` clears them on a transition. Without it one
  `esc` in the editor fell through the browser and quit the game.
- **A text field captures the keyboard.** Otherwise typing `wasd` walks the highlight and
  `enter` both ends the name and presses whatever it was on.
