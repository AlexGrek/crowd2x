#!/usr/bin/env -S uv run --script
"""Draw the church's altar and pew.

* `altar.png` (16x16) - a section of the church's altar, where a cleric
  leads prayers: a table under a white cloth with a gold band, a gold cross
  standing on it between two lit candles. The cloth runs from edge to edge,
  so the five sections side by side along the far wall read as one long
  altar.
* `pew.png` (16x16) - a wooden pew: a backrest on two posts over the seat,
  on two legs. Two side by side read as one long bench.

Seen from the front and a little above like every prop, lit from the upper
left, on a strip of soft shadow. The cleric's robes are a uniform, in
`tools/human_art.py`.

    uv run tools/church_art.py                                 # write the assets
    uv run tools/church_art.py preview screenshots/church.png  # enlarged, on the church's floor; writes no assets
"""

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from human_art import write_png  # noqa: E402

ASSETS = os.path.join(os.path.dirname(__file__), "..", "assets")

COLOURS = {
    "o": (44, 28, 16, 255),  # outline
    # the altar's cloth and its band
    "w": (242, 240, 232, 255),  # cloth, lit
    "W": (206, 202, 194, 255),  # cloth, shaded
    "g": (226, 180, 56, 255),  # gold
    "G": (164, 118, 30, 255),  # gold, shaded
    "l": (252, 226, 128, 255),  # gold, lit
    "c": (244, 236, 208, 255),  # candle wax
    "C": (200, 188, 156, 255),  # candle wax, shaded
    "f": (255, 214, 92, 255),  # flame
    "F": (255, 144, 40, 255),  # flame's heart
    # wood: the altar's base and the pew
    "L": (198, 142, 88, 255),  # wood, lit
    "y": (160, 104, 58, 255),  # wood
    "Y": (118, 74, 40, 255),  # wood, shaded
    "D": (84, 52, 28, 255),  # wood, dark
    "H": (0, 0, 0, 90),  # shadow, near
    "h": (0, 0, 0, 56),  # shadow, far
}

ALTAR = (
    "................",
    ".......lg.......",
    ".....llggGG.....",
    "..f....gG....f..",
    "..F....gG....F..",
    "..c....gG....c..",
    "..C...lggG...C..",
    "oooooooooooooooo",
    "wwwwwwwwwwwwwwww",
    "gggggggggggggggg",
    "GGGGGGGGGGGGGGGG",
    "wwwwwwwwwwwwwwww",
    "WWWWWWWWWWWWWWWW",
    "YYYYYYYYYYYYYYYY",
    "DDDDDDDDDDDDDDDD",
    "HHHHHHHHHHHHHHHH",
)

PEW = (
    "................",
    "................",
    "oooooooooooooooo",
    "LLLLLLLLLLLLLLLL",
    "yyyyyyyyyyyyyyyy",
    "YYYYYYYYYYYYYYYY",
    "oooooooooooooooo",
    ".oDo........oDo.",
    "oooooooooooooooo",
    "LLLLLLLLLLLLLLLL",
    "yyyyyyyyyyyyyyyy",
    "oooooooooooooooo",
    ".oDo........oDo.",
    ".oDo........oDo.",
    ".ooo........ooo.",
    ".hhhhhhhhhhhhhh.",
)


def paint(mask):
    assert len(mask) == 16 and all(len(row) == 16 for row in mask), mask
    return [[COLOURS[c] if c != "." else (0, 0, 0, 0) for c in row] for row in mask]


SHEETS = {"altar.png": ALTAR, "pew.png": PEW}


def preview(path, scale=8):
    """Five altar sections along a wall, two rows of pews before them, on the
    church's floor, enlarged."""
    floor = (190, 186, 178)
    cells_w, cells_h = 7, 5
    rows = [[(*floor, 255) for _ in range(cells_w * 16 * scale)] for _ in range(cells_h * 16 * scale)]
    placed = [(1 + i, 0, ALTAR) for i in range(5)] + [(x, y, PEW) for y in (2, 3) for x in (0, 1, 4, 5)]
    for cx, cy, mask in placed:
        art = paint(mask)
        for y in range(16):
            for x in range(16):
                red, green, blue, a = art[y][x]
                if a == 0:
                    continue
                t = a / 255
                for dy in range(scale):
                    for dx in range(scale):
                        px, py = (cx * 16 + x) * scale + dx, (cy * 16 + y) * scale + dy
                        under = rows[py][px]
                        rows[py][px] = tuple(round(c * t + u * (1 - t)) for c, u in zip((red, green, blue), under[:3])) + (255,)
    write_png(path, rows)
    print(f"wrote {path}")


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "preview":
        preview(sys.argv[2] if len(sys.argv) > 2 else "screenshots/church_preview.png")
        return
    for name, mask in SHEETS.items():
        write_png(os.path.join(ASSETS, name), paint(mask))
        print(f"wrote assets/{name}, 16x16")


if __name__ == "__main__":
    main()
