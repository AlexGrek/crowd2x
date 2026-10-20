#!/usr/bin/env -S uv run --script
"""Draw the farm's grain bin and the wheat the farmers grow.

* `grain_bin.png` - the farm's store of wheat, a farmer's workplace: a wooden
  bin, four 16x16 frames side by side (empty, a few sheaves' grain, half
  full, heaped over the rim), chosen by the game from how much it holds
  (`game::props`, `fill_bins`), like the warehouse's scrap bin.
* `wheat_1.png` .. `wheat_4.png` - a crop of wheat in its plot of tilled
  earth at each of its four stages (`sim::fields::Stage`): a seedling, young
  and leafy, in ear and still green, and ripe and gold. The game draws the
  one for the stage the crop has reached (`game::crops`).

Seen from the front and a little above like every prop, lit from the upper
left, on a strip of soft shadow.

    uv run tools/farm_art.py                                # write the assets
    uv run tools/farm_art.py preview screenshots/farm.png   # every frame, enlarged; writes no assets
"""

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from human_art import write_png  # noqa: E402

ASSETS = os.path.join(os.path.dirname(__file__), "..", "assets")

COLOURS = {
    "o": (34, 28, 26, 255),  # outline
    "L": (194, 150, 94, 255),  # wood, lit
    "P": (156, 114, 66, 255),  # wood
    "p": (108, 76, 42, 255),  # wood, shaded
    "k": (52, 38, 28, 255),  # the dark inside the bin
    "Y": (248, 222, 140, 255),  # grain, catching the light
    "G": (224, 186, 88, 255),  # grain
    "g": (178, 138, 52, 255),  # grain, shaded
    "F": (150, 200, 90, 255),  # leaf, lit
    "E": (96, 154, 62, 255),  # leaf
    "e": (60, 106, 42, 255),  # leaf, shaded
    "D": (110, 76, 50, 255),  # tilled earth
    "d": (74, 50, 34, 255),  # a furrow
    "T": (206, 166, 76, 255),  # a ripe stalk
    "t": (156, 118, 50, 255),  # a ripe stalk, shaded
    "h": (22, 22, 26, 82),  # shadow
    "H": (22, 22, 26, 41),  # its soft edge
}


def put(rows, y, x, text):
    """Write `text` into row `y` from column `x`, leaving `.` see-through."""
    for i, c in enumerate(text):
        if c != ".":
            rows[y][x + i] = c


def frame(rows):
    out = ["".join(row) for row in rows]
    assert len(out) == 16 and all(len(row) == 16 for row in out), out
    return out


# --- the grain bin ------------------------------------------------------------

BIN = (
    "................",
    "................",
    "................",
    "................",
    ".oooooooooooooo.",
    ".oLLLLLLLLLLLpo.",
    ".oPkkkkkkkkkkpo.",
    ".oPkkkkkkkkkkpo.",
    ".oPkkkkkkkkkkpo.",
    ".oppppppppppppo.",
    ".oLPPPPPPPPPPpo.",
    ".oPPPPPPPPPPPpo.",
    ".oppppppppppppo.",
    ".oLPPPPPPPPPPpo.",
    ".oooooooooooooo.",
    ".HhhhhhhhhhhhhH.",
)

# Grain in the bottom of it, up to the rim, and heaped over it.
BIN_FILL = [
    [],
    [(8, 3, "gGg..gGGg")],
    [(7, 3, "gGYGgGGYGg"), (8, 3, "GgGGgGGgGG")],
    [
        (2, 6, "oooo"),
        (3, 4, "ooGYYGoo"),
        (4, 2, "ogGGYGGYGGgo"),
        (5, 2, "GGYGGgGGYGGg"),
        (6, 3, "GgGYGGgGYG"),
        (7, 3, "gGYGgGGYGg"),
        (8, 3, "GgGGgGGgGG"),
    ],
]


def bin_frames():
    frames = []
    for fill in BIN_FILL:
        rows = [list(row) for row in BIN]
        for y, x, text in fill:
            put(rows, y, x, text)
        frames.append(frame(rows))
    return frames


# --- wheat ---------------------------------------------------------------------

SOIL = [
    (12, 3, "oooooooooo"),
    (13, 2, "oDdDDdDDdDDo"),
    (14, 2, "oooooooooooo"),
    (15, 2, "HhhhhhhhhhhH"),
]

SEEDLING = [
    (10, 4, "F...F...F"),
    (11, 4, "E..eE..eE"),
]

YOUNG = [
    (5, 7, "F"),
    (6, 4, "F..E...F"),
    (7, 4, "E.FE..FE"),
    (8, 3, "eE.EE..Ee.F"),
    (9, 3, "eEeEe.eEe.E"),
    (10, 4, "eEe..eEe.e"),
    (11, 4, "eEe..eEe.e"),
]

# Ears on stalks: `A` an ear catching the light, `a` the rest of it, `s` a
# stalk, `l` a leaf, coloured green while it is heading and gold once ripe.
TALL = [
    (1, 4, "A.....A"),
    (2, 3, "AaA...AaA.A"),
    (3, 3, "AaA.A.AaAAaA"),
    (4, 3, "aAa.AaA.aAaA"),
    (5, 4, "s.aAa..s.aA"),
    (6, 4, "s..s...s..s"),
    (7, 3, "ls..s..sl.s"),
    (8, 4, "s.ls..sl.s"),
    (9, 4, "s..s..s..s"),
    (10, 4, "s..s..s..s"),
    (11, 4, "s..s..s..s"),
]
HEADING = {"A": "F", "a": "E", "s": "e", "l": "E"}
RIPE = {"A": "Y", "a": "G", "s": "T", "l": "t"}


def wheat_frame(parts, colours=None):
    rows = [list("................") for _ in range(16)]
    for y, x, text in SOIL + parts:
        if colours:
            text = "".join(colours.get(c, c) for c in text)
        put(rows, y, x, text)
    return frame(rows)


# --- writing ------------------------------------------------------------------


def paint(mask):
    return [[COLOURS[c] if c != "." else (0, 0, 0, 0) for c in row] for row in mask]


def strip(masks):
    frames = [paint(mask) for mask in masks]
    return [sum((f[y] for f in frames), []) for y in range(16)]


SHEETS = {
    "grain_bin.png": bin_frames,
    "wheat_1.png": lambda: [wheat_frame(SEEDLING)],
    "wheat_2.png": lambda: [wheat_frame(YOUNG)],
    "wheat_3.png": lambda: [wheat_frame(TALL, HEADING)],
    "wheat_4.png": lambda: [wheat_frame(TALL, RIPE)],
}


def preview(path, scale=6):
    """Every sheet, a row each, on meadow, enlarged."""
    ground = (96, 128, 64)
    sheets = [make() for make in SHEETS.values()]
    width = (18 * max(len(s) for s in sheets) + 2) * scale
    height = (18 * len(sheets) + 2) * scale
    rows = [[(*ground, 255) for _ in range(width)] for _ in range(height)]
    for r, masks in enumerate(sheets):
        for n, mask in enumerate(masks):
            art = paint(mask)
            ox, oy = (2 + n * 18) * scale, (2 + r * 18) * scale
            for y in range(16):
                for x in range(16):
                    red, green, blue, a = art[y][x]
                    if a == 0:
                        continue
                    t = a / 255
                    for dy in range(scale):
                        for dx in range(scale):
                            px, py = ox + x * scale + dx, oy + y * scale + dy
                            under = rows[py][px]
                            rows[py][px] = tuple(
                                round(c * t + u * (1 - t)) for c, u in zip((red, green, blue), under[:3])
                            ) + (255,)
    write_png(path, rows)
    print(f"wrote {path}")


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "preview":
        preview(sys.argv[2] if len(sys.argv) > 2 else "screenshots/farm_preview.png")
        return
    for name, make in SHEETS.items():
        masks = make()
        write_png(os.path.join(ASSETS, name), strip(masks))
        print(f"wrote assets/{name}, {16 * len(masks)}x16")


if __name__ == "__main__":
    main()
