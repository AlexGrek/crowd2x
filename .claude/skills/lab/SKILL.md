---
name: lab
description: The crowd2x lab - a district (or any map, or a saved game) lived through headless and as fast as the CPU allows (src/lab/, `cargo run --release -- lab`, `uv run tools/lab.py`), sampling the world's numbers every world hour (people, deaths, every store's stock and the pieces that flowed in and out of it, the scrap left to collect, jobs, goals, what workers on shift are at, money, the crowd's stats), watching them with anomaly rules (lab/watches.json - below, above, rises_by, grows_less_than within hours), saving a .savegame snapshot when one fires, and - served on a localhost socket - pausing there for commands that look into the paused world (units, one unit's body and brain, stores, the log, where shifts go, tracing one unit through hours) and let it go again. Also the run's files (metrics.csv/jsonl, the whole log in log.txt) for long-term analysis, and how to resume from a snapshot in the lab or the game. Use for anything about how a world behaves over days or weeks - production that slows or stops, deaths, a need or a job that goes wrong only after a while - for reproducing a long-run bug, and for checking a fix against a long run.
---

# The lab

A QA script (`qa` skill) answers *does this do the right thing when somebody presses these
buttons*. The lab answers *what does a district come to over a few weeks, and where did it go
wrong*. It is plain Rust over `GameState` — no Bevy app, no window, no assets — so a world goes
by at the speed of its ticks: **seed 7 runs about 7000 ticks a second in release, a world day in
6.5 s, four weeks in about three minutes** (the game tops out at x8, a day in a minute and a half).

```sh
export CARGO_TARGET_DIR=D:/crowd2x-target       # this machine: C: is full
uv run tools/lab.py batch --seed 7 --for 28d --keep-going   # through, then a report
uv run tools/lab.py start --seed 7              # serve in the background (builds release)
uv run tools/lab.py run 14d                     # returns at once
uv run tools/lab.py wait                        # until the end, an anomaly or a pause
uv run tools/lab.py help                        # every command
uv run tools/lab.py stop
```

`batch` is `cargo run --release -- lab run ...` and `start` is `... lab serve ...` in the
background, with its output in `lab-runs/server.log`. World options: `--seed N` (a district,
7 by default), `--map FILE`, `--load FILE.savegame`. Also `--for`, `--every` (sample cadence,
1h), `--watches FILE` / `--no-watches`, `--out DIR` (`lab-runs/<world>`), `--port` (7071),
and `--keep-going` (batch: don't stop at the first anomaly). Durations are `90m`, `6h`, `3d`,
`2w` of world time or `5000t` ticks. `batch` exits 1 if any watch fired, 0 if none did, and 2
if it could not start.

## What it keeps

| File in the run's directory | What |
| --- | --- |
| `metrics.jsonl` | a sample per line as it is taken: tick, clock, every metric |
| `metrics.csv` | the same, wide, written at the end, at every stop and on `export` |
| `log.txt` | **every** log line, `tick<TAB>clock<TAB>message` (~10 MB a district month). The lab holds only the last 50 000 in memory, and a world reloaded from a snapshot starts with an empty log, so this is the history |
| `anomaly-NN-<watch>.savegame` | the world where a watch fired, paused, with the camera on the site |

Metrics (`src/lab/metrics.rs` has the table) are flat names:
- People: `alive`, `alive.<kind>`, `died`, `lying`, `cremated`, `jobless`.
- Stores: `store.<prop>` (stock now) and `in.<prop>` / `out.<prop>` (pieces ever stored into / taken out of that kind of store, read off the log's `stored N ... at x, y` / `took N ... at x, y` lines).
- Scrap: `scrap.heaps`, `scrap.left`, `scrap.nearest` (in the navigation area, where collectors look; the nearest heap's distance from a scrap bin).
- Fields: `crops`.
- Jobs: `jobs.<job>`.
- Money: `budget`, `cash`, `in debt`.
- Activity: `goal.<goal>`, `asleep`, `outside`, `<job>.on shift`, `<job>.<goal>` (workers whose shift is due).
- Bodies: `<stat>.avg`, `<stat>.min` over the living humans.

`in.*` is the throughput to watch: a store's stock goes up and down as it is filled and
emptied, and a flow only goes up.

## Watches

`lab/watches.json` (the default; compiled in) is a list of rules, each naming one metric and
one condition. A watch fires when its condition **becomes** true and not again until it has
been false, so a stall is one anomaly, not one an hour. Firing saves a snapshot. A batch run
stops there (unless `--keep-going`), and a served run pauses there.

```json
{ "name": "steel stopped reaching the warehouse", "metric": "in.steel rack",
  "grows_less_than": 1, "within_hours": 72, "after_hours": 48 }
```

Conditions: `below`, `above`, `rises_by` (+ `within_hours`), `grows_less_than`
(+ `within_hours`). `after_hours` keeps a watch quiet until the world has been open that long.
Office-hour chains stop over the weekend (Fri 17:00 to Mon 09:00 is 64 h), so a stall window
under 72 h fires every Saturday. Add a watch to a served lab with
`watch add {"name": "somebody is dying", "metric": "health.min", "below": 15}`, and drop one
with `watch rm NAME`.

## Looking into a paused world

| Command | Prints |
| --- | --- |
| `status`, `anomalies`, `watches` | where it stands, what fired (with snapshots), the rules |
| `metrics [PATTERN...] [by=1d] [last=N]` / `values [PATTERN]` | the series as a table / the latest values |
| `stats` | the game's statistics menu |
| `stores [FILTER]` | every store: kind, cell, pieces |
| `units FILTER` | a line each: id, name, job (`*` on shift), `goal / task`, cell, pack. Filters: `all`, `humans`, `dogs`, `dead`, `asleep`, `outside`, `jobless`, `goal:NAME`, `health<20` (any stat), or a job |
| `unit WHO [lines=N]` | everything: `debug_fields`, `brain_fields` (priorities, task, memory, out of reach...), stats, route, its last log lines. WHO: an id, the start of its hex, or part of a name |
| `log [N] [WORD...]` | the last N lines containing every word |
| `shifts [JOB]` / `shifts reset` | where workers' shifts went: a look every 5 world minutes at everyone whose shift is due, by `goal / task` (what `sim::steel_probe` did by hand) |
| `trace WHO DURATION` | lives through DURATION and prints every change of that unit's goal, task or pack, with its log lines between |
| `tick DURATION` | lives through it now (pausing a run first) |
| `save [NAME] [store]` | a snapshot in the run's directory; `store` also writes one to `.saves/` for the game's Load screen |
| `load PATH` | carry on from a snapshot instead |

**A snapshot carries on exactly.** A lab tick is the game's tick: `DT` = 1/64 s, one spawn
pass and one processing pass, the game at x1. Reloading the snapshot before a death replays
the bites to the same second. So:
- Open one in the game: `CROWD2X_LOAD=lab-runs/x/anomaly-01-....savegame cargo run`. It opens paused.
- Resume it in a lab: `start --load`.

**On Windows a serving lab locks the binary**, so `cargo build` cannot replace it. To add a
command or a metric mid-investigation: `save` (or use the anomaly's snapshot), `stop`,
rebuild, then `start --load` the snapshot.

## How to investigate

1. `batch --for 28d --keep-going` and read the daily headline. The table at the end shows
   when a number stalls or a population falls.
2. Serve from the start or from a snapshot. Add a watch at the symptom (`health.min below 15`,
   `in.steel rack grows_less_than ...`), then `run` and `wait` so it pauses at the moment.
3. `units` to find who is involved, `unit` to read its brain, `trace` to watch it, `shifts` to
   see where a job's hours go.
4. For questions across the whole run, read `log.txt` with a few lines of Python. For example:
   the cause and job of every death, or whether each exhaustion death was bitten first.
5. Fix with a unit test that fails first, then rerun the same seed and length and compare.

## Worked example: steel in the warehouse (seed 7, 2026-10-27)

*Slow*: about 27 ingots a week, every week of a month. Everything downstream of the
collectors is lossless: 264 scrap stored, 111 steel smelted, 111 racked. The steelworkers spend
most of the afternoon on `found no scrap to smelt`, and the two of them could smelt about 160 a
week. The cap is collection, about 3 scrap per collector per day (`trace` of a collector):
- about an hour getting ready at a restaurant fridge (`ReadyRoutine`, even with provisions in
  the pocket);
- 50–70 minutes' walk each way, from the warehouse's bins at (186, 96) to heaps round
  (117, 158);
- 20 world minutes a piece salvaging;
- a pack that holds 3.

*Slower*: heaps never grow back (`CollectGoal`). In a month the nearest heap moved from 25
cells from the bins to 145 (`scrap.nearest`), and the scrap left in the navigation area fell
from 805 to 559 pieces (`scrap.left`). At about 240 pieces a month the collectors' ground runs
dry in a couple more months, and steel with it.

*Zero*: posts emptied by death are never filled again, with 77 jobless. Losing the last
steelworker or courier stops steel for good. The month lost one of each: 53 deaths, 40 of
them from deathdog bites (dogs included) and 12 from exhaustion. Every exhaustion death had
been bitten a day or two before:
- under 90 health is a hospital patient, and a patient heals only in a hospital bed (by design);
- a district has four beds;
- a patient had Sleep suppressed whether or not it had a bed, so one waiting for a bed never
  slept. Fixed in `Medical::arrange` (`sim/hospital.rs`): a patient waiting for a bed sleeps
  as anybody does. Test:
  `a_patient_with_no_bed_to_be_had_still_sleeps_rather_than_dying_of_exhaustion`. The same month after the fix: no exhaustion
  deaths (12 before), 23 humans dead instead of 39, both steelworkers and couriers alive at the
  end, and steel unchanged (116 racked vs 111), because collection is still the cap.

Then, by the owner's call: the dead are replaced from the jobless (`GameState::fill_vacancy`),
a collector skips the fridge for a need its pocket meets (`ReadyNeed::pocket`), collectors work
07:00-18:00 (`COLLECTOR_HOURS`) and the hospital has eight beds. The same month again:

| seed 7, 28 days | before | exhaustion fixed | all four |
| --- | --- | --- | --- |
| steel racked a week | 26, 32, 26, 27 | 25, 33, 36, 22 | 43, 59, 61, 25 |
| steel / scrap in the month | 111 / 264 | 116 / 240 | 188 / 393 |
| humans alive at the end (of 178) | 139 | 155 | 158 |
| dead workers replaced | 0 | 0 | 11 |
| nearest heap from the bins at the end | | 145 cells | 278 cells |

The fourth week is the next limit: faster collecting empties the near heaps sooner, and by
then the nearest is 278 cells from the bins (`scrap.nearest`), more than four hours' walk.
Still open: heaps that never grow back, where the warehouse is against the heaps, deathdog
lethality, and two bitten undertakers who died of thirst in the hospital, one after failing to
reach a bed again and again (one in the first month too).

## Code

| File | What |
| --- | --- |
| `src/main.rs` | `crowd2x lab ...` is decided before any Bevy app is built |
| `src/lab/mod.rs` | the command line, `lab run` and its report |
| `src/lab/world.rs` | `Lab`: the `GameState` (built as `game::actors::build_world` builds one, or restored), `step`, the log ring, `log.txt`, flows parsed from store log lines (`flow`), `Shifts`, snapshots, durations |
| `src/lab/metrics.rs` | `sample` (hourly; one walk of the crowd), `Series`, the CSV and the tables |
| `src/lab/watch.rs` | `WatchSpec` / `Watch` / `Anomaly`; `lab/watches.json` |
| `src/lab/inspect.rs` | every command's text |
| `src/lab/serve.rs` | the socket: one command a connection; runs in 40 ms slices between looks; `wait` holds a connection until the run stops |
| `tools/lab.py` | the client: `start` (detached, waits for the port), `batch`, any command, `stop` |

A new metric goes in `metrics::sample`. It runs hourly, so a walk of the crowd is fine; a
walk per tick is not. Use one flat name, and document it in the table at the top of
`metrics.rs`. A new command goes in `serve::execute`, with its text in `inspect`, and in
`HELP`. Store flows depend on the wording of `effect::deposit` / `effect::withdraw`'s log
lines, which the `flow` test pins.
