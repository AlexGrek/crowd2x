#!/usr/bin/env -S uv run --script
"""Draw the crematorium, where the undertakers bring the dead.

One 16x16 frame: a squat furnace of grey stone with a chimney at the back
left, a gold cross set over an arched iron door, the fire glowing through its
grate, seen from the front and a little above like every prop, lit from the
upper left, standing on a strip of its own soft shadow
(`map::district`, `CREMATORIUM`).

    uv run tools/crematorium_art.py                                      # write assets/crematorium.png
    uv run tools/crematorium_art.py preview screenshots/crematorium.png  # on a floor, enlarged; writes no assets
"""

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from human_art import write_png  # noqa: E402

ASSETS = os.path.join(os.path.dirname(__file__), "..", "assets")

COLOURS = {
    "o": (34, 30, 32, 255),     # outline
    "C": (92, 88, 90, 255),     # chimney, lit
    "c": (64, 60, 64, 255),     # chimney, shade
    "S": (150, 146, 140, 255),  # stone, lit
    "s": (114, 110, 106, 255),  # stone, shade
    "j": (96, 92, 90, 255),     # mortar joint
    "Y": (226, 192, 84, 255),   # gold cross
    "k": (46, 42, 46, 255),     # iron door
    "F": (255, 168, 60, 255),   # fire, bright
    "f": (214, 84, 34, 255),    # fire, deep
    "H": (0, 0, 0, 90),         # shadow, near
    "h": (0, 0, 0, 56),         # shadow, far
}

CREMATORIUM = (
    "..ooo...........",
    "..oCco..........",
    "..oCco..........",
    "..oCcoooooooo...",
    ".ooSSSSSSSSSSoo.",
    ".oSSSSSSYSSSSso.",
    ".oSjjjjYYYjjjso.",
    ".oSSSSSSYSSSSso.",
    ".oSSSSkkkkSSSso.",
    ".oSjjkfFFfkjjso.",
    ".oSSSkFfFfkSSso.",
    ".oSSSkfFfFkSSso.",
    ".oSjjkkkkkkjjso.",
    ".osssssssssssso.",
    ".oooooooooooooo.",
    ".HhhhhhhhhhhhhH.",
)


def paint(mask):
    assert len(mask) == 16 and all(len(row) == 16 for row in mask), mask
    return [[COLOURS[c] if c != "." else (0, 0, 0, 0) for c in row] for row in mask]


def preview(path, scale=8):
    """The crematorium on a strip of tiled floor, a cell of floor each side, enlarged."""
    light, dark = (226, 210, 150), (200, 184, 126)
    width, height = 48 * scale, 16 * scale
    rows = [[(*(dark if ((x // scale) // 4 + (y // scale) // 4) % 2 else light), 255) for x in range(width)] for y in range(height)]
    art = paint(CREMATORIUM)
    ox = 16 * scale
    for y in range(16):
        for x in range(16):
            r, g, b, a = art[y][x]
            if a == 0:
                continue
            for dy in range(scale):
                for dx in range(scale):
                    px, py = ox + x * scale + dx, y * scale + dy
                    under = rows[py][px]
                    t = a / 255
                    rows[py][px] = tuple(round(c * t + u * (1 - t)) for c, u in zip((r, g, b), under[:3])) + (255,)
    write_png(path, rows)
    print(f"wrote {path}")


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "preview":
        preview(sys.argv[2] if len(sys.argv) > 2 else "screenshots/crematorium_preview.png")
        return
    write_png(os.path.join(ASSETS, "crematorium.png"), paint(CREMATORIUM))
    print("wrote assets/crematorium.png, 16x16")


if __name__ == "__main__":
    main()
