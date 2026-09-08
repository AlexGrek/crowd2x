#!/usr/bin/env python3
"""Run the scripted QA tests in qa/ against the real game.

Each test is a JSON script (see src/qa/script.rs) that the game replays as
input and then checks. This runs one game process per test, so a crash or a
hang is contained to the test that caused it, and points each one at its own
maps directory so a test that deletes a map cannot delete yours.

Screenshots a test takes land in qa-screenshots/<test>/, wiped before each run
so what is in there is always from the last one. A capture that beat the
renderer to the frame comes out as a single flat colour rather than as an
error, so those are counted and reported instead of being left to be noticed.

A test that measures something (see src/qa/perf.rs) also writes its numbers to
qa-perf/<test>.json, and they are printed here whether it passed or failed -
the measurement is the point of a perf test, and a run that failed a budget is
exactly the one whose numbers are worth reading.

    python3 tools/qa.py                    # every test in qa/
    python3 tools/qa.py qa/create_map.json # just this one
    python3 tools/qa.py -v                 # stream the game's log as it runs
    python3 tools/qa.py --release          # optimised build; the only honest
                                           # profile to quote a timing from

Exits non-zero if any test failed, so it can gate a commit.
"""

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from check_pixel_grid import read_png  # noqa: E402  (needs the path above)

ROOT = Path(__file__).resolve().parent.parent
QA_DIR = ROOT / "qa"
# Each test gets its own maps directory under here, wiped before it runs.
SCRATCH = ROOT / "target" / "qa"
# ...and its own screenshot directory here, which is gitignored and meant to be
# looked at, so it is not buried in target/.
SHOTS = ROOT / "qa-screenshots"
# ...and its measurements here, next to them and gitignored for the same
# reason: a timing is evidence from one machine, not source.
PERF = ROOT / "qa-perf"
# The window size the coordinates in a `mouse` step assume: 320x180 canvas at
# PIXEL_SCALE 4. Forced so a test does not depend on the last window size.
WINDOW = "1280x720"
# Whatever the script's own timeout is, the process gets this much longer to
# start up, compile nothing, and shut down.
GRACE_SECONDS = 60


def is_blank(png: Path) -> bool:
    """Is every pixel the same colour?

    The game already retakes a blank capture until it gets a real frame, so one
    that arrives here blank means the window was never presented at all - the
    display is asleep, or there is nowhere to draw. That is worth saying out
    loud rather than leaving a directory of black PNGs to be puzzled over.
    """
    try:
        width, height, channels, pixels = read_png(str(png))
    except Exception:
        return True
    first = pixels[0:3]
    for y in range(0, height, 3):
        for x in range(0, width, 3):
            i = (y * width + x) * channels
            if pixels[i : i + 3] != first:
                return False
    return True


def report_shots(shots: Path) -> None:
    taken = sorted(shots.glob("*.png"))
    if not taken:
        return
    blank = [png for png in taken if is_blank(png)]
    print(f"     {len(taken)} screenshot(s) in {shots.relative_to(ROOT)}/")
    for png in blank:
        print(f"     WARNING {png.name} is blank: the window was never presented")


def report_perf(perf: Path) -> None:
    """Print what a test measured, if it measured anything.

    Printed on a pass as well as a failure: a perf test's deliverable is the
    number, and a run nobody looks at is a run that only tells you whether the
    budget - which is deliberately loose - was breached.
    """
    if not perf.exists():
        return
    try:
        report = json.loads(perf.read_text())
    except json.JSONDecodeError as error:
        print(f"     cannot read {perf.name}: {error}")
        return

    vsync = "vsync on" if report.get("vsync", True) else "vsync off"
    print(f"     measurements ({report.get('profile', '?')} build, {vsync}):")
    for m in report.get("measurements", []):
        per_entity = m.get("per_entity_us")
        each = f"  {per_entity:8.2f}us each" if per_entity is not None else ""
        print(
            f"       {m['name']:<32} {m['entities']:>6} entities"
            f"  best {m['best_ms']:7.3f}ms  median {m['median_ms']:7.3f}ms"
            f"  p95 {m['p95_ms']:7.3f}ms  worst {m['worst_ms']:7.3f}ms{each}"
        )


def run_one(path: Path, verbose: bool, release: bool) -> bool:
    name = path.stem
    maps = SCRATCH / name / "maps"
    shutil.rmtree(maps.parent, ignore_errors=True)
    maps.mkdir(parents=True)

    shots = SHOTS / name
    shutil.rmtree(shots, ignore_errors=True)

    perf = PERF / f"{name}.json"
    perf.unlink(missing_ok=True)

    try:
        timeout = json.loads(path.read_text()).get("timeout", 60)
    except json.JSONDecodeError as error:
        print(f"FAIL {name}: not valid json: {error}")
        return False

    env = {
        **os.environ,
        "CROWD2X_QA": str(path),
        "CROWD2X_MAPS": str(maps),
        "CROWD2X_QA_SHOTS": str(shots),
        "CROWD2X_QA_PERF": str(perf),
        "CROWD2X_WINDOW": WINDOW,
    }

    print(f"---- {name}", flush=True)
    try:
        # Through cargo, always: bevy resolves assets/ relative to the manifest
        # under cargo and relative to the executable otherwise.
        result = subprocess.run(
            ["cargo", "run", "--quiet"] + (["--release"] if release else []),
            cwd=ROOT,
            env=env,
            timeout=timeout + GRACE_SECONDS,
            capture_output=not verbose,
            text=True,
        )
    except subprocess.TimeoutExpired:
        print(f"FAIL {name}: the game never exited")
        return False

    if result.returncode == 0:
        print(f"PASS {name}")
        report_shots(shots)
        report_perf(perf)
        return True

    print(f"FAIL {name} (exit {result.returncode})")
    report_shots(shots)
    report_perf(perf)
    if not verbose:
        # Only the qa lines: the rest is bevy's startup chatter.
        for line in (result.stderr or "").splitlines():
            if "qa:" in line:
                print(f"     {line.split('crowd2x::qa:')[-1].strip()}")
    return False


def main() -> int:
    args = [a for a in sys.argv[1:] if not a.startswith("-")]
    verbose = "-v" in sys.argv or "--verbose" in sys.argv
    # A timing from a debug build says whether something is scaling badly, and
    # nothing at all about how fast the game is: this crate builds at
    # opt-level 1 there. Quote numbers from --release.
    release = "--release" in sys.argv

    tests = [Path(a) for a in args] if args else sorted(QA_DIR.glob("*.json"))
    if not tests:
        print(f"no tests found in {QA_DIR}")
        return 1

    passed = sum(run_one(path, verbose, release) for path in tests)
    failed = len(tests) - passed
    print(f"\n{passed}/{len(tests)} passed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
