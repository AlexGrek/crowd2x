#!/usr/bin/env -S uv run --script
"""Draw the robots and the robot factory.

* `robot_idle.png` / `robot_idle_reversed.png` - the robot, four 16x16 frames
  in a strip, facing right and its mirror (the same layout as the dog's
  sheets, so the two share one pool): a boxy steel body on two legs, a visor
  with one eye, an antenna with a blinking lamp.
* `charging_dock.png` (1 frame) and `charging_dock_used.png` (2 frames, a
  bar of charge climbing) - the charging station's dock.
* `robot_workbench.png` - the engineers' workbench, a bench with a vice and a
  half-built arm.
* `assembly_bay.png` - the robot factory's bay: a gantry over a floor plate.
* `parts_rack.png` - the factory's steel rack, four frames, empty to heaped
  (a store, `BinFill`).
* `explosion.png` - eight 16x16 frames of a fireball, `spark.png` and
  `smoke.png` single frames: the epic end of a robot (`game::explosions`).
* `human/clothes_uniform_engineer.png` and
  `human/clothes_uniform_robot_courier.png` - the engineer's overalls (the
  robot courier's is only a stub: a robot wears nothing, but every job has a
  uniform file).

    uv run tools/robot_art.py            # write the assets
    uv run tools/robot_art.py preview screenshots/robots.png
"""

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from human_art import write_png, paint  # noqa: E402

ASSETS = os.path.join(os.path.dirname(__file__), "..", "assets")
T = (0, 0, 0, 0)

COLOURS = {
    ".": T,
    "o": (30, 34, 44, 255),      # outline
    "s": (172, 182, 196, 255),   # steel, lit
    "S": (120, 130, 148, 255),   # steel
    "d": (78, 86, 104, 255),     # steel, shaded
    "v": (24, 30, 40, 255),      # visor
    "e": (90, 230, 255, 255),    # eye
    "E": (40, 150, 200, 255),    # eye, dim
    "r": (232, 60, 50, 255),     # lamp, red
    "R": (140, 30, 30, 255),     # lamp, off
    "y": (252, 206, 64, 255),    # yellow
    "Y": (196, 140, 30, 255),    # yellow, shaded
    "g": (96, 220, 110, 255),    # green
    "G": (40, 130, 60, 255),     # green, dark
    "w": (228, 232, 240, 255),   # white
    "b": (96, 62, 36, 255),      # wood
    "B": (148, 100, 60, 255),    # wood, lit
    "H": (0, 0, 0, 90),          # shadow
    "f": (255, 232, 120, 255),   # flame, bright
    "F": (255, 150, 40, 255),    # flame
    "X": (200, 60, 20, 255),     # flame, deep
    "k": (60, 56, 56, 255),      # smoke
    "K": (110, 104, 104, 255),   # smoke, light
}


def rows(mask):
    return [[COLOURS[ch] for ch in row] for row in mask]


def strip(frames):
    """Frames side by side."""
    height = len(frames[0])
    return [sum((frame[y] for frame in frames), []) for y in range(height)]


def mirror(image):
    return [list(reversed(row)) for row in image]


def mirror_strip(image, frames):
    width = len(image[0]) // frames
    out = [[] for _ in image]
    for f in range(frames):
        for y, row in enumerate(image):
            out[y] += list(reversed(row[f * width:(f + 1) * width]))
    return out


# A robot facing right; the lamp blinks and the legs step across the frames.
def robot_frame(lamp, step):
    legs = [
        ["...oSo..oSo....", "...oSo..oSo...."],
        ["....oSo.oSo....", "....oSo.oSo...."],
        ["...oSo..oSo....", "...oSo..oSo...."],
        ["....oSoSoo.....", "....ooSdoo....."],
    ][step]
    head = [
        f"..........{'r' if lamp else 'R'}.....",
        "..........o.....",
        "....oooooooo....",
        "...osssssSSdo...",
        "...ovvveeevdo...",
        "...ovvvvvvvdo...",
        "....oooooooo....",
        "...oSSSSSSSdo...",
        "..oSssSSSSSSdo..",
        "..oSsSyyyySSdo..",
        "..oSSSyYYySSdo..",
        "..ooSSSSSSddoo..",
    ]
    frame = [row.ljust(16, ".")[:16] for row in head + [legs[0].ljust(16, ".")[:16], legs[1].ljust(16, ".")[:16]]]
    # two rows of shadow under the feet
    frame += ["....HHHHHHHH....", "................"]
    return rows(frame[:16])


DOCK = (
    "................",
    "....oooooooo....",
    "...oSssssssSo...",
    "...oSvvvvvvSo...",
    "...oSvEEEEvSo...",
    "...oSvvvvvvSo...",
    "...oSSSSSSSSo...",
    "....oSyyyySo....",
    "....oSyYYySo....",
    "....oSSSSSSo....",
    "....oddoodd o...".replace(" ", "."),
    "....oYooooYo....",
    "...oYYYYYYYYo...",
    "..oooooooooooo..",
    "..HHHHHHHHHHHH..",
    "................",
)


def dock_used(level):
    bars = ["g" if i < level else "E" for i in range(4)]
    mask = list(DOCK)
    mask[4] = "...oSv" + "".join(bars) + "vSo..."
    mask[3] = "...oSvvvvvvSo..." if level < 4 else "...oSvggggggSo..."
    return rows(mask)


WORKBENCH = (
    "................",
    "................",
    "......ooo.......",
    ".....oSssSo.....",
    "..oo.oSdSSo.....",
    ".oSso.oSo.oo....",
    ".oSdo..o.oSso...",
    "oooooooooooooooo",
    "BBBBBBBBBBBBBBBB",
    "bbbbbbbbbbbbbbbb",
    "oooooooooooooooo",
    "obo..........obo",
    "obo..........obo",
    "obo..........obo",
    "ooo..........ooo",
    "HHHHHHHHHHHHHHHH",
)

BAY = (
    "................",
    "oooooooooooooooo",
    "oYYYYYYYYYYYYYYo",
    "oyyyyyyyyyyyyyyo",
    "oo............oo",
    "oS............So",
    "oS............So",
    "oS............So",
    "oS............So",
    "oSd..oooooo..dSo",
    "oSd.osssssSo.dSo",
    "oSddoddddddodSdo",
    "oooooooooooooooo",
    "oYoYoYoYoYoYoYoo",
    "oooooooooooooooo",
    "HHHHHHHHHHHHHHHH",
)


def rack_frame(level):
    mask = [
        "................",
        "................",
        "................",
        "................",
        "................",
        "................",
        "................",
        "................",
        "................",
        "................",
        "oooooooooooooooo",
        "oSSSSSSSSSSSSSSo",
        "oSddddddddddddSo",
        "oooooooooooooooo",
        "obo..........obo",
        "HHHHHHHHHHHHHHHH",
    ]
    ingots = ["", "s", "ss", "sss"][min(level, 3)]
    for tier in range(level):
        y = 9 - tier * 3
        row = "o" + ("sSsSsSsSsSsSsS" if tier % 2 == 0 else "SsSsSsSsSsSsSs") + "o"
        mask[y] = row
        mask[y - 1] = "o" + "o" * 14 + "o" if y - 1 >= 0 and mask[y - 1].strip(".") == "" else mask[y - 1]
    del ingots
    return rows(mask)


def fireball(i):
    """Frame `i` of 8: a ball of fire swelling, whitening, then breaking up."""
    size = [2, 4, 6, 8, 7, 6, 5, 3][i]
    cx = cy = 8
    frame = [[T for _ in range(16)] for _ in range(16)]
    for y in range(16):
        for x in range(16):
            d = ((x - cx + 0.5) ** 2 + (y - cy + 0.5) ** 2) ** 0.5
            if d <= size:
                heat = d / size
                if i >= 5 and (x * 7 + y * 13 + i) % 3 == 0:
                    continue
                if heat < 0.35:
                    frame[y][x] = COLOURS["f"] if i < 6 else COLOURS["F"]
                elif heat < 0.7:
                    frame[y][x] = COLOURS["F"] if i < 6 else COLOURS["X"]
                else:
                    frame[y][x] = COLOURS["X"] if i < 6 else COLOURS["k"]
    return frame


SPARK = ["................"] * 7 + ["......ff........", ".....fFFf.......", "......ff........"] + ["................"] * 6
SMOKE = [
    "................", "................", "................", "......kk........",
    ".....kKKk.......", "....kKKKKk......", "....kKKKKKk.....", ".....kKKKk......",
    "......kkk.......", "................", "................", "................",
    "................", "................", "................", "................",
]

# The engineer's overalls: a grey boiler suit with yellow piping and a spanner.
ENGINEER = (
    ["", "", "", "", "", "", "", "",
     ".....ddddd......".replace(".....ddddd......", "....dddddddd...."),
     "....dmmmmmmd....", ".....dmyymd.....", ".....dmmmmd.....", ".....dmmmmd.....",
     "....dmmmmmmd....", "....dddddddd...."],
    {"d": (88, 98, 118), "m": (130, 142, 164), "y": (252, 206, 64)},
)
STUB = (
    ["", "", "", "", "", "", "", "", "", "", "", "", "", "", ""],
    {},
)


def preview(out):
    scale = 6
    sheets = []
    right = strip([robot_frame(i % 2 == 0, i) for i in range(4)])
    sheets.append(right)
    sheets.append(strip([dock_used(i) for i in range(4)] + [rows(DOCK)]))
    sheets.append(strip([rows(WORKBENCH), rows(BAY)] + [rack_frame(i) for i in range(4)]))
    sheets.append(strip([fireball(i) for i in range(8)]))
    width = max(len(s[0]) for s in sheets)
    canvas = []
    for sheet in sheets:
        for row in sheet:
            canvas.append(row + [(60, 60, 70, 255)] * (width - len(row)))
    big = []
    for row in canvas:
        wide = [px for px in row for _ in range(scale)]
        for _ in range(scale):
            big.append([px if px[3] else (60, 60, 70, 255) for px in wide])
    write_png(out, big)


def generate():
    right = strip([robot_frame(i % 2 == 0, i) for i in range(4)])
    write_png(os.path.join(ASSETS, "robot_idle.png"), right)
    write_png(os.path.join(ASSETS, "robot_idle_reversed.png"), mirror_strip(right, 4))
    write_png(os.path.join(ASSETS, "charging_dock.png"), rows(DOCK))
    write_png(os.path.join(ASSETS, "charging_dock_used.png"), strip([dock_used(2), dock_used(4)]))
    write_png(os.path.join(ASSETS, "robot_workbench.png"), rows(WORKBENCH))
    write_png(os.path.join(ASSETS, "assembly_bay.png"), rows(BAY))
    write_png(os.path.join(ASSETS, "parts_rack.png"), strip([rack_frame(i) for i in range(4)]))
    write_png(os.path.join(ASSETS, "explosion.png"), strip([fireball(i) for i in range(8)]))
    write_png(os.path.join(ASSETS, "spark.png"), rows(SPARK))
    write_png(os.path.join(ASSETS, "smoke.png"), rows(SMOKE))
    human = os.path.join(ASSETS, "human")
    for job, (mask, colours) in (("engineer", ENGINEER), ("robot_courier", STUB)):
        write_png(os.path.join(human, f"clothes_uniform_{job}.png"), paint(mask, colours))


if __name__ == "__main__":
    if len(sys.argv) > 2 and sys.argv[1] == "preview":
        preview(sys.argv[2])
    else:
        generate()
