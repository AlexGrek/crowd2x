# Scripted QA: overview

A condensed tour of the harness and its design decisions. `SKILL.md` is the full reference: schema, every step, diagnosis.

## Scripted QA (`src/qa/`, `qa/*.json`, `tools/qa.py`)

`cargo test` covers the plain-Rust parts and `debug.rs` can photograph a frame, but
neither answers the question that matters for an interface: *if someone presses these
buttons in this order, does the right thing happen?* A QA test is a JSON file that the
real binary replays as input and then checks.

```sh
uv run tools/qa.py                      # every test in qa/, one process each
uv run tools/qa.py qa/create_map.json   # one test
uv run tools/qa.py -v                   # stream the game's log
CROWD2X_QA=qa/create_map.json cargo run   # the same thing by hand
```

The `qa` skill documents the whole thing end to end — schema, every step, fixtures,
screenshots and how to diagnose a failure — and should be read before writing or changing
a test. In short:

Steps come at two levels and a test is expected to mix them:

- **Intent** — `{"press": "create"}`, `{"goto": "maps"}`, `{"name": "office"}`,
  `{"tool": "wall brown"}`, `{"cursor_cell": {"x": 3, "y": 2}}`. Addressed by *label*, so
  they survive a button moving, a palette being reordered, or a list growing a row. Use
  these unless the input itself is what is under test. `press` refuses an ambiguous label
  rather than guessing, which is why walking a file list still uses arrow keys.
- **Input** — `key`, `type`, `pad`, `stick`, `mouse`, `click`, `drag`, `wheel`. `drag`
  presses on one editor cell, moves the cursor to another and lets go there — the only way
  to draw a rectangle, since a `click` holds the cursor still. The actual
  devices, and the only way to test that the devices work: that a gamepad reaches every
  button, that typing lands in the field and not in the palette.

Input is injected where the OS would have put it, not where it is convenient to fake it:
keys press `ButtonInput` *and* send a `KeyboardInput` message (the game reads both); a
gamepad is spawned and connected through `RawGamepadEvent`, so `bevy_input` builds the
real `Gamepad` component; the pointer moves by setting the window's cursor position, the
same field `bevy_ui`'s focus system reads, so a click goes through real hover-and-click.

The simulation gets six steps of its own, all intent-level: `{"spawn": {"kind": "human",
"x": 3, "y": 2}}` puts a command on the same queue the game uses, `{"select": 0}` selects
the unit that arrived first (arrival order, not slot order, because a despawn leaves a hole
the next spawn fills — and because a `Uid` is random and a test cannot know one in
advance), `{"give": {"item": "food", "count": 3}}` stows items in the selected unit on that
same queue (the only way anything is stowed today, since no goal puts things away),
`{"hold": "food"}` puts one in its **hand** (`null` empties it) — a hand is only ever full
for the length of a meal, so a test cannot wait for the brain to oblige, and it should tick
few times afterwards, since a hungry unit eats what it is handed and a thirsty one drinks
it in about a second of watching —
`{"process": {"name": "hunger", "on": false}}` switches one of the selected unit's
biological processes off on that queue too — the only handle a script has on what a unit
*wants*, since stats are rolled at spawn (70 to 100 percent satisfied) and only a process may write one, so watching one
need means stopping the others happening — and `{"tick": 200}` runs
one spawn pass and then exactly that many processing passes, *immediately* — so pending
spawns are applied once however many ticks were asked for. Ticking rather than waiting is
the point — `FixedUpdate` runs at whatever rate the frame allows, so a test that waited a
second would be asserting on however many ticks the machine managed.

**A test can also time itself** (`src/qa/perf.rs`). `{"populate": {"kind": "human",
"count": 1000}}` queues a crowd spread over the cells that can be stood on, `{"measure":
{"name": "1000 humans", "ticks": 200}}` times that many processing passes — the simulation
alone, no renderer — and `{"measure_frames": {"name": "1000 actors", "seconds": 2.0}}`
times whole frames, sprite sync and render included. Every measurement keeps its
distribution (best, median, p95, worst, microseconds per entity), is written to
`qa-perf/<test>.json` pass or fail, and is printed by `tools/qa.py`: **the number is the
deliverable, and the assertions are a floor under it.**

Two assertions, and which one to reach for matters. `{"expect_scaling": {"from": "100
humans", "to": "1000 humans", "slack": 1.5}}` checks that the cost *per entity* did not
grow with the crowd, which is what an accidental per-agent scan over every other agent
looks like and is what killed the earlier prototype; it barely depends on the machine.
It divides the *fastest* sample of each measurement, not the median: interference only ever
adds time, and a ratio of two medians inherits the noise of both — with medians, three
consecutive runs of an unchanged build gave 0.52x, 0.68x and 1.60x. A budget judges the
median instead, because that is what the game typically does. How much slack a comparison
needs still depends on the cache — a hundred entities fit in L1 and a thousand do not, so
that pair moves by 1.3-1.8x between runs.
`{"expect_under": {"measure": "...", "ms": 1.0}}` is a wall rather than a target, set well
above what the machine does today, and the observed number belongs in a `note` beside it.
Two things stop the numbers being lies: a test that measures frames must set `"vsync":
false`, or every frame is capped at the refresh rate and a regression only shows once the
game is already below 60Hz; and a debug timing (this crate is `opt-level = 1` there) is
good for a *ratio* and worthless as a speed, which is why the profile and the vsync
setting are recorded in the report. `qa/perf_simulation.json`, `qa/perf_rendering.json`
and `qa/perf_huge_map.json` (twenty thousand humans on a 256x256 map) are the three that
exist. **`perf_rendering` is marked `"required": false`**: its frame budget fails on a
machine whose display caps every frame at the refresh rate — an empty map took 16ms a frame
here, on `main` as much as after any change — so `tools/qa.py` still runs it and prints its
numbers, lists it as "not required, failed", and does not fail the run for it. Any test can
carry the flag; reserve it for one whose verdict depends on the machine, not the code.

**A test can also watch the world go by** (`src/qa/observe.rs`). `{"observe": {"name":
"a day", "ticks": 46080, "every": 1920}}` ticks like `tick` and samples every unit with a body
before, every `every` ticks and after — each stat and the goal on top — into
`qa-observe/<test>/<name>.csv` (a row per unit per sample) and `<name>.json` (the crowd's
averages and goals per sample), which `tools/qa.py` prints as a table; `{"expect_stat":
{"stat": "hunger", "max": 75, "of": "crowd"}}` asserts on a stat of the selected unit or the
crowd's average. `qa/a_day_in_the_office.json` lives through a day in
`qa/fixtures/office.json` (beds, fridges, toilets, computers) and is the example.

Assertions are about outcomes — `expect_state`, `expect_focus`, `expect_map`,
`expect_no_map`, `expect_tile`, `expect_zoom`, `expect_speed`, `expect_entities`,
`expect_sprites`, `expect_drawn`, `expect_world_sprites`, `expect_held`, `expect_selected`,
`expect_carrying`, `expect_prop_in_use`, `expect_log`, `expect_world_time`, `expect_stat`,
`expect_lighting`, `expect_ceiling`, `expect_grid`, `expect_objects`, `expect_connected` —
and `expect_tile`, `expect_ceiling`, `expect_grid` and `expect_objects` read the **saved** map,
so "I painted a wall" is only true once the file says so. `expect_zoom` exists because
zooming changes the size of the canvas rather than the scale of a camera, so a screenshot
cannot be asked how far in it is without counting texels. `expect_speed` takes the string
the readout shows (`x1`, `x0.25`, `paused`) rather than a number and a flag, since
`paused` and `x1` are the same multiplier and a different game. `expect_selected` names the kind (`"human"`, or `null` for nobody) rather
than an id, for the same reason `select` takes an index, and `expect_carrying` counts what
the selected unit has **stowed** — what is in its hand is not stowed, which is the
distinction the whole inventory is built on. `expect_entities` and
`expect_sprites` are deliberately two assertions: the simulation having three entities and
the screen showing three actors are different claims, and the second is the one that
catches a renderer that has quietly stopped keeping up.

**Since sprites are culled, `expect_sprites` means "drawn right now"**, not "exists": a unit
off the edge of the view is supposed to have none. A number asserted there is a claim about
where the camera is, so pin it — pause with `{"key": "p"}` and aim with `{"look_at": {"x":
3, "y": 2}}` (cell units, through the same map clamp as panning, taking effect next frame,
so `wait` or `tick` before asserting). Two assertions carry the culling contract without
naming a number:

- `{"expect_drawn": {}}` — every unit on the canvas has a sprite and no sprite draws
  somebody off it or gone. It is checked against a rect rebuilt from the canvas image and
  the camera rather than read off `VisibleArea`, because an assertion phrased in terms of
  the thing under test can only ask whether the renderer agrees with itself. Holds wherever
  the camera is pointing; actors only.
- `{"expect_world_sprites": {"min": 40, "max": 400}}` — everything on the world layer:
  tiles, props, actors, overlays, parked bodies, the selection frame. `max` is the
  structural form of "what is drawn is bounded by the canvas", and the only check that
  sees a pool growing to the size of the crowd, since a parked body carries no `Actor`.
  **The `min` matters as much**: an empty map satisfies every ceiling there is, and a map
  window still holding entities despawned under it — which is what showed a blank map on
  re-entering a screen, until `reset_map_windows` — draws exactly that. The floor caught
  it before the fix did.

`expect_held` is the same split for
what is carried: it counts the sprites drawing an item, per kind, in anybody's hand, so a
hand that is full and a picture that was never drawn — or was drawn for the wrong item —
are told apart. Placement is not something a count can say, so `qa/held_items.json` also
photographs it; pause first, or the unit wanders out of a zoomed-in frame between steps.
`expect_prop_in_use` is that split again for a prop whose picture says whether somebody is
using it — a computer's screen — and it wants the same pause for a sharper reason:
`FixedUpdate` keeps running between steps, so the tick a shot catches would otherwise
depend on how fast the machine is.

**Screenshots are named, not pathed.** `{"shot": "the file list"}` writes
`qa-screenshots/<test>/01-the-file-list.png`, numbered in the order the shots were taken,
so one test can photograph as many moments as it likes without inventing file names.
`tools/qa.py` wipes each test's directory before it runs and reports what came out; the
whole directory is gitignored, since a screenshot is evidence rather than source.

A capture that beats the renderer to the frame comes back as one flat colour instead of
as an error, and *how long* to wait for the first real frame is not knowable — on this
machine it has ranged from under three seconds to never. So the harness looks at what it
captured and retakes it until there is something in it (`is_flat`, then `shot_delay`
between attempts, then it gives up and says so rather than leaving black PNGs to be
puzzled over). If every shot in a run is blank, the window is not being presented at all —
a sleeping display or no display — which is an environment problem and not a test failure.

A test can start from a world rather than an empty one: `given.maps` takes a bare name (an
empty map), `{"name": ..., "file": ...}` (a map kept in `qa/fixtures/`), or
`{"name": ..., "map": {...}}` (the map document written inline, for tests that depend on
exactly which cells are painted), or `{"name": ..., "district": 7}` (a generated district). `"open": "<name>"` has that map already loaded when the
app starts, in whichever screen `state` names — `editor` or `game`; a script asking for a
screen this build does not have stops with a message saying so rather than running
against the wrong screen.

Two things to keep true, both learned the hard way here:

- **`main` returns `AppExit`.** Discard it and a failing test still exits 0, and the whole
  suite passes vacuously. Check a new test fails when it should before trusting it.
- **Fixtures (`"given"`) are written in `Plugin::build`,** because the browser lists the
  directory in `OnEnter`, which for the starting screen runs before every `Startup` system.

Point `CROWD2X_MAPS` at a scratch directory when running these by hand, or a test will
delete maps somebody meant to keep; `tools/qa.py` gives each test its own.
