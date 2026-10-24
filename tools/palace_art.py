#!/usr/bin/env -S uv run --script
"""Draw the palace's throne.

* `throne.png` (16x16) - where the king holds court: a high-backed chair of
  gilt, a crest on top, its back and seat buttoned crimson velvet, gilt arms
  and legs.

Seen from the front and a little above like every prop, lit from the upper
left, on a strip of soft shadow. The king's robes and crown are a uniform, in
`tools/human_art.py`.

    uv run tools/palace_art.py                                 # write the asset
    uv run tools/palace_art.py preview screenshots/palace.png  # enlarged, on the palace's floor; writes no assets
"""

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from human_art import write_png  # noqa: E402

ASSETS = os.path.join(os.path.dirname(__file__), "..", "assets")

COLOURS = {
    "o": (44, 28, 16, 255),  # outline
    "l": (252, 226, 128, 255),  # gilt, lit
    "y": (226, 180, 56, 255),  # gilt
    "Y": (164, 118, 30, 255),  # gilt, shaded
    "v": (178, 30, 44, 255),  # velvet
    "V": (118, 16, 30, 255),  # velvet, shaded
    "b": (238, 196, 84, 255),  # a gilt button in the velvet
    "j": (70, 150, 230, 255),  # the crest's jewel
    "H": (0, 0, 0, 90),  # shadow, near
    "h": (0, 0, 0, 56),  # shadow, far
}

THRONE = (
    "......oloo......",
    ".....oljyYo.....",
    "...ooolyyYooo...",
    "...olyyyyyyYo...",
    "...oyvvvvvvYo...",
    "...oyvbvvbVYo...",
    "...oyvvvvvVYo...",
    "...oyvbvvbVYo...",
    ".oooyvvvvvVYooo.",
    ".olyyyyyyyyyyYo.",
    ".oyYovvvvvVoyYo.",
    ".oyYoVVVVVVoyYo.",
    ".olyyyyyyyyyyYo.",
    ".oYo.oooooo.oYo.",
    ".oYo........oYo.",
    ".HhhhhhhhhhhhhH.",
)


def paint(mask):
    assert len(mask) == 16 and all(len(row) == 16 for row in mask), mask
    return [[COLOURS[c] if c != "." else (0, 0, 0, 0) for c in row] for row in mask]


SHEETS = {"throne.png": THRONE}


def preview(path, scale=8):
    """The throne on the palace's white marble, enlarged."""
    floor = (209, 208, 215)
    art = paint(THRONE)
    size = 20 * scale
    rows = [[(*floor, 255) for _ in range(size)] for _ in range(size)]
    for y in range(16):
        for x in range(16):
            red, green, blue, a = art[y][x]
            if a == 0:
                continue
            t = a / 255
            for dy in range(scale):
                for dx in range(scale):
                    px, py = (2 + x) * scale + dx, (2 + y) * scale + dy
                    under = rows[py][px]
                    rows[py][px] = tuple(round(c * t + u * (1 - t)) for c, u in zip((red, green, blue), under[:3])) + (255,)
    write_png(path, rows)
    print(f"wrote {path}")


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "preview":
        preview(sys.argv[2] if len(sys.argv) > 2 else "screenshots/palace_preview.png")
        return
    for name, mask in SHEETS.items():
        write_png(os.path.join(ASSETS, name), paint(mask))
        print(f"wrote assets/{name}, 16x16")


if __name__ == "__main__":
    main()
