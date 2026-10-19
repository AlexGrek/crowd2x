#!/usr/bin/env -S uv run --script
"""Draw the steel factory's props and the warehouse's steel rack.

* `scrap_hopper.png` - the factory's store of scrap metal to smelt: a yellow
  steel hopper on legs, four 16x16 frames side by side (empty, a few pieces,
  half full, heaped), chosen by the game from how much it holds
  (`game::props`, `fill_bins`), like the warehouse's scrap bin. The scrap is
  `tools/scrap_art.py`'s rust and steel.
* `steel_pallet.png` - the factory's store of the steel it made: ingots
  stacked on a wooden pallet, the same four steps.
* `steel_rack.png` - the warehouse's store of steel: a steel shelving unit,
  its three shelves filling with ingots, the same four steps.
* `furnace_idle.png` (2 frames) and `furnace.png` (3 frames) - a brick
  furnace, embers glowing in its mouth while nobody works it and flames while
  a steelworker does (the game swaps the strips, `game::props`).

Seen from the front and a little above like every prop, lit from the upper
left, on a strip of soft shadow.

    uv run tools/steel_factory_art.py                                   # write the assets
    uv run tools/steel_factory_art.py preview screenshots/factory.png   # every frame, enlarged; writes no assets
"""

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from human_art import write_png  # noqa: E402
from scrap_art import COLOURS as SCRAP  # noqa: E402

ASSETS = os.path.join(os.path.dirname(__file__), "..", "assets")

COLOURS = dict(SCRAP)
COLOURS.update(
    {
        "Y": (204, 166, 44, 255),  # hopper paint
        "U": (232, 200, 92, 255),  # hopper paint, lit
        "u": (150, 118, 30, 255),  # hopper paint, shaded
        "X": (214, 222, 230, 255),  # an ingot, lit
        "x": (160, 170, 182, 255),  # an ingot
        "z": (98, 106, 120, 255),  # an ingot, its shaded face
        "P": (156, 114, 66, 255),  # pallet wood
        "p": (108, 76, 42, 255),  # pallet wood, shaded
        "G": (156, 84, 64, 255),  # firebrick, lit
        "g": (118, 60, 48, 255),  # firebrick
        "F": (255, 226, 130, 255),  # flame, white-hot
        "f": (244, 140, 40, 255),  # flame
        "e": (176, 62, 22, 255),  # ember
    }
)


def blank():
    return [list("................") for _ in range(16)]


def put(rows, y, x, text):
    """Write `text` into row `y` from column `x`, leaving `.` see-through."""
    for i, c in enumerate(text):
        if c != ".":
            rows[y][x + i] = c


def frame(rows):
    out = ["".join(row) for row in rows]
    assert len(out) == 16 and all(len(row) == 16 for row in out), out
    return out


# --- the scrap hopper ---------------------------------------------------------

HOPPER = (
    "................",
    "................",
    "................",
    ".oooooooooooooo.",
    ".oUYYYYYYYYYYuo.",
    ".oukkkkkkkkkkuo.",
    ".oukkkkkkkkkkuo.",
    "..ouYYYYYYYYuo..",
    "...ouYYYYYYuo...",
    "....ouYYYYuo....",
    ".....ouuuuo.....",
    ".....ossSSo.....",
    "....os.oo.so....",
    "....os....so....",
    "...ooo....ooo...",
    "..HhhhhhhhhhhH..",
)

# Scrap showing in the mouth of it, and over the rim when heaped.
HOPPER_FILL = [
    [],
    [(6, 3, "oyRo..oSwo")],
    [(5, 2, "oSwoyRyoko"), (6, 2, "RySwSRyRwS")],
    [(2, 4, "oo....oo"), (3, 3, "oWSoooyRo"), (4, 2, "yRyoSoRySo"), (5, 2, "RyRySwSRyR"), (6, 2, "SwSRyRyRwS")],
]


def hopper_frames():
    frames = []
    for fill in HOPPER_FILL:
        rows = [list(row) for row in HOPPER]
        for y, x, text in fill:
            put(rows, y, x, text)
        frames.append(frame(rows))
    return frames


# --- ingots -------------------------------------------------------------------

# One layer of ingots across a pallet: three, two rows tall.
LAYER = ("oXXXxoXXXxoXXxo", "ozzzzozzzzozzzo")


def pallet_frames():
    """A pallet empty, with one ingot, one layer and a heap of them."""
    base = blank()
    put(base, 12, 1, "oooooooooooooo")
    put(base, 13, 1, "oPPPPPPPPPPPPo")
    put(base, 14, 1, "opoo.opoo.opoo")
    put(base, 15, 1, "HhhhhhhhhhhhhH")
    frames = []
    for step in range(4):
        rows = [row[:] for row in base]
        if step == 1:
            put(rows, 10, 4, "oXXXxo")
            put(rows, 11, 4, "ozzzzo")
        if step >= 2:
            put(rows, 10, 1, LAYER[0])
            put(rows, 11, 1, LAYER[1])
            put(rows, 8, 3, "oXXXxo")
            put(rows, 9, 3, "ozzzzo")
        if step == 3:
            put(rows, 8, 1, LAYER[0])
            put(rows, 9, 1, LAYER[1])
            put(rows, 6, 1, LAYER[0])
            put(rows, 7, 1, LAYER[1])
            put(rows, 4, 4, "oXXXxoXXxo")
            put(rows, 5, 4, "ozzzzozzzo")
        frames.append(frame(rows))
    return frames


# --- the steel rack -----------------------------------------------------------

RACK = (
    "................",
    ".oo..........oo.",
    ".oS..........So.",
    ".oS..........So.",
    ".oWSSSSSSSSSSso.",
    ".oS..........So.",
    ".oS..........So.",
    ".oS..........So.",
    ".oWSSSSSSSSSSso.",
    ".oS..........So.",
    ".oS..........So.",
    ".oS..........So.",
    ".oWSSSSSSSSSSso.",
    ".oS..........So.",
    ".oo..........oo.",
    ".HhhhhhhhhhhhhH.",
)

# Two ingots on a shelf, in the gap between the uprights.
SHELF = ("oXXxooXXxo", "ozzzoozzzo")


def rack_frames():
    """The rack empty, one ingot on the bottom shelf, the bottom shelf full,
    every shelf full."""
    frames = []
    for step in range(4):
        rows = [list(row) for row in RACK]
        if step == 1:
            put(rows, 10, 3, "oXXxo")
            put(rows, 11, 3, "ozzzo")
        if step >= 2:
            put(rows, 10, 3, SHELF[0])
            put(rows, 11, 3, SHELF[1])
        if step == 3:
            for y in (6, 2):
                put(rows, y, 3, SHELF[0])
                put(rows, y + 1, 3, SHELF[1])
        frames.append(frame(rows))
    return frames


# --- the furnace --------------------------------------------------------------

FURNACE = (
    "................",
    "....oooooooo....",
    "....oGggGggo....",
    "..oooooooooooo..",
    "..oGgGGgGGgGgo..",
    "..ogGgGGgGgGgo..",
    "..oGgoooooogGo..",
    "..ogo......ogo..",
    "..oGo......oGo..",
    "..ogo......ogo..",
    "..oGgoooooogGo..",
    "..ogGgGGgGgGgo..",
    "..oGgGGgGGgGgo..",
    "..oooooooooooo..",
    "..HhhhhhhhhhhH..",
    "................",
)

# What its mouth shows: embers while idle, flames while worked.
EMBERS = [("kkkkkk", "kkekkk", "kekkek"), ("kkkkkk", "kkkkek", "keekkk")]
FLAMES = [("fFffFf", "FfFFfF", "eFfFfe"), ("FffFfF", "fFFfFf", "efFfFe"), ("ffFfFf", "FFfFfF", "eFFfFe")]


def furnace_frames(mouths):
    frames = []
    for mouth in mouths:
        rows = [list(row) for row in FURNACE]
        for i, text in enumerate(mouth):
            put(rows, 7 + i, 5, text)
        frames.append(frame(rows))
    return frames


# --- writing ------------------------------------------------------------------


def paint(mask):
    return [[COLOURS[c] if c != "." else (0, 0, 0, 0) for c in row] for row in mask]


def strip(masks):
    frames = [paint(mask) for mask in masks]
    return [sum((f[y] for f in frames), []) for y in range(16)]


SHEETS = {
    "scrap_hopper.png": hopper_frames,
    "steel_pallet.png": pallet_frames,
    "steel_rack.png": rack_frames,
    "furnace_idle.png": lambda: furnace_frames(EMBERS),
    "furnace.png": lambda: furnace_frames(FLAMES),
}


def preview(path, scale=6):
    """Every sheet, a row each, on a factory floor, enlarged."""
    floor = (128, 128, 132)
    sheets = [make() for make in SHEETS.values()]
    width = (18 * max(len(s) for s in sheets) + 2) * scale
    height = (18 * len(sheets) + 2) * scale
    rows = [[(*floor, 255) for _ in range(width)] for _ in range(height)]
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
        preview(sys.argv[2] if len(sys.argv) > 2 else "screenshots/steel_factory_preview.png")
        return
    for name, make in SHEETS.items():
        masks = make()
        write_png(os.path.join(ASSETS, name), strip(masks))
        print(f"wrote assets/{name}, {16 * len(masks)}x16")


if __name__ == "__main__":
    main()
