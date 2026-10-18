#!/usr/bin/env -S uv run --script
"""Draw the guard post set into the district's wall beside each gate.

One 16x16 frame: a sentry box in olive paint under a red tin roof, a window
across its front with the glass catching the light, a brass badge by the door,
seen from the front and a little above like every prop, lit from the upper
left, standing on a strip of its own soft shadow. It stands in a cell of the
perimeter wall (`map::district`, `GUARD_POST`), so the wall's bricks show round
its edges.

    uv run tools/guard_post_art.py                                    # write assets/guard_post.png
    uv run tools/guard_post_art.py preview screenshots/guard_post.png  # on a strip of wall, enlarged; writes no assets
"""

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from human_art import write_png  # noqa: E402

ASSETS = os.path.join(os.path.dirname(__file__), "..", "assets")

COLOURS = {
    "o": (38, 32, 28, 255),     # outline
    "R": (164, 58, 44, 255),    # roof, lit
    "r": (118, 40, 32, 255),    # roof, shaded edge
    "G": (116, 128, 86, 255),   # olive wall, lit
    "g": (86, 96, 62, 255),     # olive wall, shade
    "k": (56, 50, 44, 255),     # window frame
    "b": (112, 152, 184, 255),  # glass
    "W": (214, 232, 242, 255),  # glint on the glass
    "Y": (226, 192, 84, 255),   # brass badge
    "H": (0, 0, 0, 90),         # shadow, near
    "h": (0, 0, 0, 56),         # shadow, far
}

POST = (
    "................",
    "..oooooooooooo..",
    ".oRRRRRRRRRRRRo.",
    ".oRRRRRRRRRRRro.",
    ".orrrrrrrrrrrro.",
    "..oGGGGGGGGGgo..",
    "..oGkkkkkkkkgo..",
    "..oGkWbbbbbkgo..",
    "..oGkbWbbbbkgo..",
    "..oGkkkkkkkkgo..",
    "..oGGGGGGGGGgo..",
    "..oGGGGGGGYGgo..",
    "..oGGGGGGGGGgo..",
    "..oggggggggggo..",
    "..oooooooooooo..",
    ".HhhhhhhhhhhhhH.",
)


def paint(mask):
    assert len(mask) == 16 and all(len(row) == 16 for row in mask), mask
    return [[COLOURS[c] if c != "." else (0, 0, 0, 0) for c in row] for row in mask]


def preview(path, scale=8):
    """The post in a strip of brick wall, a cell of wall each side, enlarged."""
    brick, mortar = (176, 120, 72), (140, 96, 60)
    width, height = 48 * scale, 16 * scale
    rows = [[(*(mortar if (y // scale) % 4 == 3 or (x // scale) % 8 == 7 else brick), 255) for x in range(width)] for y in range(height)]
    art = paint(POST)
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
        preview(sys.argv[2] if len(sys.argv) > 2 else "screenshots/guard_post_preview.png")
        return
    write_png(os.path.join(ASSETS, "guard_post.png"), paint(POST))
    print("wrote assets/guard_post.png, 16x16")


if __name__ == "__main__":
    main()
