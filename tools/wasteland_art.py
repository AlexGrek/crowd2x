#!/usr/bin/env -S uv run --script
"""Draw the wasteland's ground: dirt, stony dirt, cracked dirt, scrub,
meadow, mud and water.

Seven 16x16 floor tiles for the endless earth round every map
(`map::wasteland`, `map::terrain`: "dirt", "dirt stones", "dirt cracked",
"scrub", "meadow", "mud", "water"). Each one is repeated over thousands of
cells, so every one tiles seamlessly — everything here is drawn on a torus:
a blob or a crack running off the right edge comes back in on the left — and
none has a feature in a row or a column, so the repeat does not read as a
grid. Light from the upper left, like the rest of the art; a dusty,
washed-out palette, since the world ended.

Drawn from a fixed seed, so running it again writes the same files.

    uv run tools/wasteland_art.py     # write assets/dirt.png ... water.png
"""

import os
import random
import sys

sys.path.insert(0, os.path.dirname(__file__))
from human_art import write_png  # noqa: E402

ASSETS = os.path.join(os.path.dirname(__file__), "..", "assets")
N = 16


def rgba(r, g, b):
    return (r, g, b, 255)


def smooth_noise(rng, cells=4):
    """Value noise on a `cells` x `cells` lattice that wraps at 16: a soft
    field in [0, 1) with no seam."""
    lattice = [[rng.random() for _ in range(cells)] for _ in range(cells)]
    step = N / cells

    def ease(t):
        return t * t * (3 - 2 * t)

    field = []
    for y in range(N):
        row = []
        for x in range(N):
            gx, gy = int(x // step), int(y // step)
            tx, ty = ease((x % step) / step), ease((y % step) / step)
            a = lattice[gy][gx]
            b = lattice[gy][(gx + 1) % cells]
            c = lattice[(gy + 1) % cells][gx]
            d = lattice[(gy + 1) % cells][(gx + 1) % cells]
            top = a + (b - a) * tx
            bottom = c + (d - c) * tx
            row.append(top + (bottom - top) * ty)
        field.append(row)
    return field


def shaded(field, colours):
    """A field split into bands, one colour each, darkest first."""
    bands = len(colours)
    return [[colours[min(int(v * bands), bands - 1)] for v in row] for row in field]


def put(tile, x, y, colour):
    tile[y % N][x % N] = colour


def speckle(rng, tile, colours, count):
    for _ in range(count):
        put(tile, rng.randrange(N), rng.randrange(N), rng.choice(colours))


# --- dirt -----------------------------------------------------------------

DIRT_LOW = rgba(108, 89, 68)
DIRT = rgba(121, 100, 76)
DIRT_HIGH = rgba(133, 111, 85)
DIRT_DARK = rgba(90, 74, 57)
DIRT_LIGHT = rgba(148, 125, 96)


def dirt(rng):
    tile = shaded(smooth_noise(rng), [DIRT_LOW, DIRT, DIRT, DIRT_HIGH])
    speckle(rng, tile, [DIRT_DARK, DIRT_DARK, DIRT_LIGHT], 14)
    return tile


# --- stony dirt -----------------------------------------------------------

STONE = rgba(139, 134, 123)
STONE_LIT = rgba(168, 163, 150)
STONE_SHADE = rgba(84, 76, 64)


def pebble(tile, x, y, w, h):
    """A small stone: lit along its top and left, a shadow cast down-right."""
    for dy in range(h):
        for dx in range(w):
            lit = dy == 0 or dx == 0
            put(tile, x + dx, y + dy, STONE_LIT if lit and (dx, dy) != (w - 1, h - 1) else STONE)
    for dx in range(1, w + 1):
        put(tile, x + dx, y + h, STONE_SHADE)
    for dy in range(1, h):
        put(tile, x + w, y + dy, STONE_SHADE)


def dirt_stones(rng):
    tile = dirt(rng)
    for x, y, w, h in ((2, 3, 2, 2), (10, 1, 3, 2), (6, 9, 2, 1), (12, 11, 2, 2), (1, 13, 1, 1)):
        pebble(tile, x, y, w, h)
    return tile


# --- cracked dirt ---------------------------------------------------------

DRY_LOW = rgba(128, 107, 80)
DRY = rgba(140, 118, 89)
DRY_HIGH = rgba(150, 128, 97)
CRACK = rgba(86, 69, 52)
CRACK_LIT = rgba(162, 140, 108)

# A network of cracks as polylines on the torus: plates of dried earth.
CRACKS = (
    ((0, 4), (3, 5), (6, 4), (8, 6), (11, 5), (14, 6), (16, 4)),
    ((6, 4), (5, 0)),
    ((6, 16), (6, 13), (8, 11), (8, 6)),
    ((8, 11), (12, 12), (15, 10), (16, 10)),
    ((0, 10), (2, 12), (1, 16)),
    ((12, 12), (13, 16)),
    ((11, 5), (12, 0)),
)


def line(a, b):
    (x0, y0), (x1, y1) = a, b
    steps = max(abs(x1 - x0), abs(y1 - y0))
    return [(round(x0 + (x1 - x0) * i / steps), round(y0 + (y1 - y0) * i / steps)) for i in range(steps + 1)]


def dirt_cracked(rng):
    tile = shaded(smooth_noise(rng), [DRY_LOW, DRY, DRY_HIGH])
    crack = set()
    for path in CRACKS:
        for a, b in zip(path, path[1:]):
            crack.update((x % N, y % N) for x, y in line(a, b))
    # The plate edge below and right of a crack catches the light.
    for x, y in crack:
        for nx, ny in ((x + 1, y), (x, y + 1)):
            if (nx % N, ny % N) not in crack:
                put(tile, nx, ny, CRACK_LIT)
    for x, y in crack:
        put(tile, x, y, CRACK)
    speckle(rng, tile, [DIRT_DARK], 4)
    return tile


# --- scrub ----------------------------------------------------------------

BLADE = rgba(124, 126, 66)
BLADE_DARK = rgba(100, 104, 55)
BLADE_LIT = rgba(152, 150, 86)


def tuft(tile, x, y):
    """A clump of dry blades: two leaning out, one up the middle, lit left."""
    put(tile, x, y, BLADE_DARK)
    put(tile, x + 1, y, BLADE)
    put(tile, x + 2, y, BLADE_DARK)
    put(tile, x, y - 1, BLADE_LIT)
    put(tile, x + 1, y - 1, BLADE)
    put(tile, x + 2, y - 1, BLADE)
    put(tile, x + 1, y - 2, BLADE_LIT)
    put(tile, x - 1, y - 2, BLADE)
    put(tile, x + 3, y - 2, BLADE_DARK)


def scrub(rng):
    tile = dirt(rng)
    for x, y in ((2, 4), (9, 2), (12, 9), (4, 12), (8, 15)):
        tuft(tile, x, y)
    speckle(rng, tile, [BLADE, BLADE_DARK], 6)
    return tile


# --- meadow ---------------------------------------------------------------

GREEN_LOW = rgba(66, 90, 49)
GREEN = rgba(80, 106, 57)
GREEN_HIGH = rgba(94, 121, 64)
GREEN_LIT = rgba(116, 142, 76)
GREEN_DARK = rgba(56, 77, 43)
FLOWER = rgba(170, 160, 92)


def meadow(rng):
    tile = shaded(smooth_noise(rng), [GREEN_LOW, GREEN, GREEN, GREEN_HIGH])
    # Blades: a lit pixel over a dark one, scattered.
    for _ in range(22):
        x, y = rng.randrange(N), rng.randrange(N)
        put(tile, x, y, GREEN_LIT)
        put(tile, x, y + 1, GREEN_DARK)
    for x, y in ((5, 6), (13, 13)):
        put(tile, x, y, FLOWER)
    # A bare spot where the earth shows through.
    for x, y in ((11, 3), (12, 3), (11, 4)):
        put(tile, x, y, DIRT_LOW)
    return tile


# --- mud ------------------------------------------------------------------

MUD_LOW = rgba(72, 58, 45)
MUD = rgba(84, 68, 52)
MUD_HIGH = rgba(95, 78, 60)
SHEEN = rgba(118, 104, 86)
PUDDLE = rgba(70, 82, 80)


def mud(rng):
    tile = shaded(smooth_noise(rng, cells=8), [MUD_LOW, MUD, MUD, MUD_HIGH])
    for x, y in ((3, 3), (4, 3), (11, 8), (12, 8), (13, 8), (6, 13)):
        put(tile, x, y, PUDDLE)
    for x, y in ((3, 2), (11, 7), (6, 12), (14, 1)):
        put(tile, x, y, SHEEN)
    speckle(rng, tile, [MUD_LOW], 6)
    return tile


# --- water ----------------------------------------------------------------

WATER_DEEP = rgba(42, 62, 66)
WATER = rgba(50, 74, 78)
WATER_HIGH = rgba(58, 84, 87)
RIPPLE = rgba(78, 104, 106)
GLINT = rgba(112, 138, 137)


def water(rng):
    tile = shaded(smooth_noise(rng), [WATER_DEEP, WATER, WATER, WATER_HIGH])
    # Short ripples, staggered so no two share a row or line up down a
    # column; each a highlight with its shadow under it.
    for x, y, length in ((1, 2, 4), (9, 5, 3), (4, 9, 3), (12, 12, 4), (7, 14, 2)):
        for dx in range(length):
            put(tile, x + dx, y, RIPPLE)
            put(tile, x + dx, y + 1, WATER_DEEP)
    for x, y in ((2, 2), (10, 5), (13, 12)):
        put(tile, x, y, GLINT)
    return tile


TILES = (
    ("dirt.png", dirt, 1),
    ("dirt_stones.png", dirt_stones, 2),
    ("dirt_cracked.png", dirt_cracked, 3),
    ("scrub.png", scrub, 4),
    ("meadow.png", meadow, 5),
    ("mud.png", mud, 6),
    ("water.png", water, 7),
)


def main():
    for name, draw, seed in TILES:
        tile = draw(random.Random(seed))
        assert len(tile) == N and all(len(row) == N for row in tile), name
        write_png(os.path.join(ASSETS, name), tile)
    return 0


if __name__ == "__main__":
    sys.exit(main())
