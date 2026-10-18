#!/usr/bin/env -S uv run --script
"""Draw the scrap bin the warehouse's collectors store their scrap in.

One strip of four 16x16 frames, side by side (64x16), chosen by the game from
how much the bin holds (`game::props`, `fill_bins`): empty, a few pieces, half
full, heaped over the rim. A steel skip in old blue paint gone to rust at the
seams, seen from the front and a little above like every prop, lit from the
upper left, standing on a strip of its own soft shadow. The scrap in it is
drawn in `tools/scrap_art.py`'s rust and steel, so what a collector brings in
looks like what it took off the heaps.

    uv run tools/scrap_bin_art.py                                  # write assets/scrap_bin.png
    uv run tools/scrap_bin_art.py preview screenshots/scrap_bin.png  # the four on a floor, enlarged; writes no assets
"""

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from human_art import write_png  # noqa: E402
from scrap_art import COLOURS  # noqa: E402

ASSETS = os.path.join(os.path.dirname(__file__), "..", "assets")

EMPTY = (
    "................",
    "................",
    "................",
    "................",
    ".oooooooooooooo.",
    ".oWWwwwwwwwwwSo.",
    ".owkkkkkkkkkkso.",
    ".owkkkkkkkkkkso.",
    ".owkkkkkkkkkkso.",
    ".oSSSSSSSSSSSso.",
    ".oBBBBBBBBBBBbo.",
    ".oBWBBBBBBBRWbo.",
    ".obBBBBBBBByRbo.",
    ".obbbbbbbbbrbbo.",
    ".oooooooooooooo.",
    ".HhhhhhhhhhhhhH.",
)

# A few pieces on the floor of it, by the front.
FEW = (
    "................",
    "................",
    "................",
    "................",
    ".oooooooooooooo.",
    ".oWWwwwwwwwwwSo.",
    ".owkkkkkkkkkkso.",
    ".owkkkkkkkkkkso.",
    ".owkoyRokkoSwso.",
    ".oSSSSSSSSSSSso.",
    ".oBBBBBBBBBBBbo.",
    ".oBWBBBBBBBRWbo.",
    ".obBBBBBBBByRbo.",
    ".obbbbbbbbbrbbo.",
    ".oooooooooooooo.",
    ".HhhhhhhhhhhhhH.",
)

# Half full: sheets and girders up to the middle of the far wall.
HALF = (
    "................",
    "................",
    "................",
    "................",
    ".oooooooooooooo.",
    ".oWWwwwwwwwwwSo.",
    ".owkkkkkkkkkkso.",
    ".owkoSwoyRyokso.",
    ".owyRySwSRyRwso.",
    ".oSSSSSSSSSSSso.",
    ".oBBBBBBBBBBBbo.",
    ".oBWBBBBBBBRWbo.",
    ".obBBBBBBBByRbo.",
    ".obbbbbbbbbrbbo.",
    ".oooooooooooooo.",
    ".HhhhhhhhhhhhhH.",
)

# Heaped: over the rim, a pipe and a sheet sticking up out of it.
HEAPED = (
    "................",
    "................",
    "......oo........",
    ".....oWSo..oo...",
    ".oooooWSoooyRoo.",
    ".oWWoyRyoSoRySo.",
    ".owoRyRySwSRyso.",
    ".owSwSRyRyRwSso.",
    ".owyRySwSRyRwso.",
    ".oSSSSSSSSSSSso.",
    ".oBBBBBBBBBBBbo.",
    ".oBWBBBBBBBRWbo.",
    ".obBBBBBBBByRbo.",
    ".obbbbbbbbbrbbo.",
    ".oooooooooooooo.",
    ".HhhhhhhhhhhhhH.",
)

FRAMES = [EMPTY, FEW, HALF, HEAPED]


def paint(mask):
    assert len(mask) == 16 and all(len(row) == 16 for row in mask), mask
    return [[COLOURS[c] if c != "." else (0, 0, 0, 0) for c in row] for row in mask]


def strip():
    """The four frames side by side, empty first."""
    frames = [paint(mask) for mask in FRAMES]
    return [sum((frame[y] for frame in frames), []) for y in range(16)]


def preview(path, scale=8):
    """The four side by side on a warehouse floor, a cell apart, enlarged."""
    floor = (150, 146, 136)
    width, height = (18 * len(FRAMES) + 2) * scale, 20 * scale
    rows = [[(*floor, 255) for _ in range(width)] for _ in range(height)]
    for n, mask in enumerate(FRAMES):
        art = paint(mask)
        ox, oy = (2 + n * 18) * scale, 2 * scale
        for y in range(16):
            for x in range(16):
                r, g, b, a = art[y][x]
                if a == 0:
                    continue
                for dy in range(scale):
                    for dx in range(scale):
                        px, py = ox + x * scale + dx, oy + y * scale + dy
                        under = rows[py][px]
                        t = a / 255
                        rows[py][px] = tuple(round(c * t + u * (1 - t)) for c, u in zip((r, g, b), under[:3])) + (255,)
    write_png(path, rows)
    print(f"wrote {path}")


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "preview":
        preview(sys.argv[2] if len(sys.argv) > 2 else "screenshots/scrap_bin_preview.png")
        return
    write_png(os.path.join(ASSETS, "scrap_bin.png"), strip())
    print(f"wrote assets/scrap_bin.png, {16 * len(FRAMES)}x16")


if __name__ == "__main__":
    main()
