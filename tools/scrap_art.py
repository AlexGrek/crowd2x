#!/usr/bin/env -S uv run --script
"""Draw the heaps of scrap metal that lie about the wasteland.

Five heaps, one per picture a heap can be drawn as (`map::wasteland`,
`SCRAP_VARIANTS`): rusted sheets with a pipe through them, a car door, a
pile of girders, a barrel on its side, a tyre and some gears. Each is 16x16,
seen from the front and a little above like every prop, lit from the upper
left, standing on a strip of its own soft shadow, and drawn in rust and old
steel so it reads against the brown dirt it lies on.

    uv run tools/scrap_art.py                              # write assets/scrap_1.png .. scrap_5.png
    uv run tools/scrap_art.py preview screenshots/scrap.png  # the five on dirt, enlarged; writes no assets
"""

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from human_art import write_png  # noqa: E402

ASSETS = os.path.join(os.path.dirname(__file__), "..", "assets")

COLOURS = {
    "o": (34, 28, 26, 255),  # outline
    "r": (104, 48, 28, 255),  # rust, dark
    "R": (146, 70, 36, 255),  # rust
    "y": (188, 102, 50, 255),  # rust, catching the light
    "s": (66, 72, 80, 255),  # steel, dark
    "S": (108, 116, 124, 255),  # steel
    "w": (156, 164, 170, 255),  # steel, lit
    "W": (206, 212, 214, 255),  # a bright edge
    "b": (58, 84, 104, 255),  # old paint, shaded
    "B": (88, 120, 142, 255),  # old paint
    "k": (38, 38, 42, 255),  # rubber, a hole
    "K": (66, 66, 72, 255),  # rubber, lit
    "h": (22, 22, 26, 82),  # shadow, as the body's
    "H": (22, 22, 26, 41),  # its soft edge
}

# Rusted corrugated sheets, a pipe stuck through the pile.
SHEETS = (
    "................",
    "................",
    "................",
    "...........oo...",
    "..........oWSo..",
    ".........owSo...",
    "...ooo...owSo...",
    "..oyRyoooowSoo..",
    "..oyRyRyRyoSRyo.",
    ".oyRyRyRyRyRyRo.",
    ".oRrRrRrRrRrRro.",
    ".ooooooooooooooo",
    "oSwSwRySwSwSwSo.",
    "osSsSrRsSsSsSso.",
    "ooooooooooooooo.",
    ".HhhhhhhhhhhhhH.",
)

# A car door lying askew: blue paint gone to rust, the window a hole.
DOOR = (
    "................",
    "................",
    "....oooooooo....",
    "...oBBBBBBBBo...",
    "...oBoooooobo...",
    "...oBokkkkobo...",
    "...oBokkkkobo...",
    "...oBoooooobo...",
    "...oBBBBBBRbo...",
    "...oBBWBRRybo...",
    "...obBBbRyrbo...",
    "...obbbbbRRbo.o.",
    "...oooooooooooSo",
    "..oSwSwSwSwSwSo.",
    "..HhhhhhhhhhhhH.",
    "................",
)

# Girders and a pipe in a heap, crossed every which way.
GIRDERS = (
    "................",
    "................",
    "..oo............",
    ".oyRo......oo...",
    "..oyRo....owso..",
    "...oyRo..owso...",
    "....oyRoowso....",
    ".....oyRwso.....",
    "..ooooowsRoooo..",
    ".oyyyyysoRyyyyo.",
    ".oRRRRRoooRRRRo.",
    ".orrrrooSwoorro.",
    ".oooooSwSwSoooo.",
    "..oooooooooooo..",
    "..HhhhhhhhhhhH..",
    "................",
)

# An oil drum on its side, its end rusted through, bolts spilled by it.
BARREL = (
    "................",
    "................",
    "................",
    "................",
    "...ooooooooo....",
    "..oyRyyRyyRoo...",
    ".oyRRRRRRRRoSo..",
    ".oSwwwwwwwwoWSo.",
    ".oRRRRRRRRRoSko.",
    ".orrrrrrrrrosSo.",
    ".oSssssssssoso..",
    "..orrRrrRrroo...",
    "...ooooooooo.oo.",
    "............oWo.",
    "..HhhhhhhhhhhH..",
    "................",
)

# A tyre on a heap of rust, a cog beside it.
TYRE = (
    "................",
    "................",
    "................",
    "......ooooo.....",
    ".....oKKKKko....",
    "....oKkooookko..",
    "....oKo.....ko..",
    "....oKo.....ko..",
    ".o.ooKkooookkoo.",
    "oSwSooKkkkkkoyo.",
    ".wkwo.ooooooRro.",
    "oSwSoyRyRyRyRro.",
    ".o.oorrrrrrrro..",
    "...oooooooooo...",
    "..HhhhhhhhhhhH..",
    "................",
)

HEAPS = [SHEETS, DOOR, GIRDERS, BARREL, TYRE]


def paint(mask):
    assert len(mask) == 16 and all(len(row) == 16 for row in mask), mask
    return [[COLOURS[c] if c != "." else (0, 0, 0, 0) for c in row] for row in mask]


def preview(path, scale=8):
    """The five side by side on a flat dirt ground, a cell apart, enlarged."""
    dirt = (121, 100, 76)
    width, height = (16 * 6) * scale, 24 * scale
    rows = [[(*dirt, 255) for _ in range(width)] for _ in range(height)]
    for n, mask in enumerate(HEAPS):
        art = paint(mask)
        ox, oy = (8 + n * 18) * scale // 1, 4 * scale
        for y in range(16):
            for x in range(16):
                r, g, b, a = art[y][x]
                if a == 0:
                    continue
                for dy in range(scale):
                    for dx in range(scale):
                        px, py = ox + x * scale + dx, oy + y * scale + dy
                        if px >= width:
                            continue
                        under = rows[py][px]
                        t = a / 255
                        rows[py][px] = tuple(round(c * t + u * (1 - t)) for c, u in zip((r, g, b), under[:3])) + (255,)
    write_png(path, rows)
    print(f"wrote {path}")


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "preview":
        preview(sys.argv[2] if len(sys.argv) > 2 else "screenshots/scrap_preview.png")
        return
    for n, mask in enumerate(HEAPS, start=1):
        write_png(os.path.join(ASSETS, f"scrap_{n}.png"), paint(mask))
    print(f"wrote assets/scrap_1.png .. scrap_{len(HEAPS)}.png, 16x16")


if __name__ == "__main__":
    main()
