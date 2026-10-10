#!/usr/bin/env -S uv run --script
"""Draw the door leaves: a strip of 16x16 frames, shut to open.

A door is a sliding leaf across the middle of its cell, drawn for a door in a
wall running left to right; the game turns it a quarter for a wall running up
and down, and mirrors it so each leaf of a double door slides into the wall
beside it (`game::props`). Frame 0 is shut, the last one open (nothing left
but the empty doorway); which frame shows is how far open the simulation says
the door is, so the strip is never played as an animation.

    uv run tools/door_art.py            # write assets/door.png and assets/house_door.png
"""

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from human_art import write_png  # noqa: E402

ASSETS = os.path.join(os.path.dirname(__file__), "..", "assets")
SIZE = 16
FRAMES = 6
TOP, BOTTOM = 5, 10  # the leaf's rows, outline included

CLEAR = (0, 0, 0, 0)

# A house door: wooden planks and a brass handle.
WOOD = {
    "outline": (52, 30, 16, 255),
    "light": (176, 120, 68, 255),
    "mid": (140, 92, 50, 255),
    "dark": (104, 66, 34, 255),
    "handle": (226, 192, 84, 255),
}

# Anybody's door: a grey frame round a pane of glass, a steel handle.
GLASS = {
    "outline": (58, 62, 72, 255),
    "frame": (120, 126, 138, 255),
    "glass": (150, 200, 222, 255),
    "shine": (214, 238, 248, 255),
    "handle": (196, 200, 208, 255),
}


def wood(lx, row):
    """The house door's texel at leaf column `lx`, leaf row `row`."""
    if row in (TOP, BOTTOM) or lx in (0, SIZE - 1):
        return WOOD["outline"]
    if lx == 13 and row in (7, 8):
        return WOOD["handle"]
    if lx % 4 == 0:
        return WOOD["dark"]  # the gaps between planks
    return WOOD["light"] if row == TOP + 1 else WOOD["mid"] if row < BOTTOM - 1 else WOOD["dark"]


def glass(lx, row):
    """Anybody's door's texel at leaf column `lx`, leaf row `row`."""
    if row in (TOP, BOTTOM) or lx in (0, SIZE - 1):
        return GLASS["outline"]
    if lx in (1, 14):
        return GLASS["frame"]
    if lx == 12 and row in (7, 8):
        return GLASS["handle"]
    if (lx + row) % 7 == 0 and 2 <= lx <= 10:
        return GLASS["shine"]
    return GLASS["glass"]


def strip(texel):
    """Every frame side by side: frame `k` shows the leaf slid `k` fifths of
    the way into the wall on its left, so the part still showing is its right
    end — handle and all — until there is nothing left."""
    rows = [[CLEAR] * (SIZE * FRAMES) for _ in range(SIZE)]
    for k in range(FRAMES):
        shown = round(SIZE * (FRAMES - 1 - k) / (FRAMES - 1))
        hidden = SIZE - shown
        for row in range(TOP, BOTTOM + 1):
            for x in range(shown):
                rows[row][k * SIZE + x] = texel(x + hidden, row)
    return rows


def main():
    write_png(os.path.join(ASSETS, "house_door.png"), strip(wood))
    write_png(os.path.join(ASSETS, "door.png"), strip(glass))
    print(f"wrote assets/house_door.png and assets/door.png, {FRAMES} frames of {SIZE}x{SIZE}")


if __name__ == "__main__":
    main()
