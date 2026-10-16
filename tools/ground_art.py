#!/usr/bin/env -S uv run --script
"""Draw the outdoor ground: pavement, road and grass.

Three 16x16 floor tiles for the district's streets and parks
(`map::terrain`, "pavement", "road", "grass"; `map::district`). Every one
tiles seamlessly — a street is the same tile repeated a hundred times — so
nothing touches an edge that the opposite edge does not continue. Light from
the upper left, like the rest of the art.

    uv run tools/ground_art.py     # write assets/pavement.png, road.png and grass.png
"""

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from human_art import write_png  # noqa: E402

ASSETS = os.path.join(os.path.dirname(__file__), "..", "assets")

# Four square slabs, the seam along the top and the left of each so that the
# tile's right edge meets the next tile's left seam.
PAVEMENT = (
    "aaaaaaaaaaaaaaaa",
    "accccccbaccccccb",
    "acbbbbbdacbbbbbd",
    "acbbbbbdacbbebbd",
    "acbbebbdacbbbbbd",
    "acbbbbbdacbbbbbd",
    "acbbbbbdacbbbbbd",
    "abddddddabdddddd",
    "aaaaaaaaaaaaaaaa",
    "accccccbaccccccb",
    "acbbbbbdacbbbbbd",
    "acbbbbbdacbbbbbd",
    "acbbbbbdacbbbebd",
    "acbebbbdacbbbbbd",
    "acbbbbbdacbbbbbd",
    "abddddddabdddddd",
)

PAVEMENT_COLOURS = {
    "a": (112, 110, 104, 255),  # the seam
    "b": (164, 161, 152, 255),  # the slab
    "c": (186, 183, 174, 255),  # its lit edge
    "d": (140, 137, 129, 255),  # its shaded edge
    "e": (150, 147, 139, 255),  # a fleck in the stone
}

# Asphalt: one dark grey with a sparse grain of lighter and darker stones,
# none of them in a row or a column, so the repeat does not show as a grid.
ROAD = (
    "aaaaaaabaaaaaaaa",
    "aacaaaaaaaaabaaa",
    "aaaaaaaaacaaaaaa",
    "abaaaaaaaaaaaaca",
    "aaaaacaaaaabaaaa",
    "aaaaaaaaaaaaaaaa",
    "aaabaaaaacaaaaaa",
    "aaaaaaaaaaaaabaa",
    "acaaaabaaaaaaaaa",
    "aaaaaaaaaaacaaaa",
    "aaaaaaaaaaaaaaab",
    "aaaabaaacaaaaaaa",
    "aaaaaaaaaaaabaaa",
    "aaaaaaaaaaaaaaaa",
    "aabaaaaacaaaaaca",
    "aaaaaaaaaaabaaaa",
)

ROAD_COLOURS = {
    "a": (62, 63, 68, 255),  # the asphalt
    "b": (82, 83, 88, 255),  # a pale stone
    "c": (48, 49, 54, 255),  # a dark one
}

# A lawn: a mid green, darker patches, and tufts of blades lit on the left.
GRASS = (
    "aaaaaaaaaaaaaaaa",
    "aaacaaaaaaaaaaaa",
    "aaccdaaaaaaabaaa",
    "aaaaaaaaaaaabaaa",
    "aaaaaaaaaaacbdaa",
    "abaaaaaaaaaaaaaa",
    "abaaaaaacaaaaaaa",
    "cbdaaaaccdaaaaaa",
    "aaaaaaaaaaaaaaab",
    "aaaaaaaaaaaaaaab",
    "aaaacaaaaaaaaacb",
    "aaaccdaaaaaaaaaa",
    "aaaaaaaaabaaaaaa",
    "aaaaaaaaabaaaaaa",
    "aaaaaaaacbdaaaaa",
    "aaaaaaaaaaaaaaaa",
)

GRASS_COLOURS = {
    "a": (86, 132, 62, 255),  # the lawn
    "b": (122, 170, 84, 255),  # a lit blade
    "c": (66, 108, 50, 255),  # a shaded one
    "d": (104, 150, 72, 255),  # a blade half in the light
}


def paint(mask, colours):
    assert len(mask) == 16 and all(len(row) == 16 for row in mask), "a tile is 16x16"
    return [[colours[ch] for ch in row] for row in mask]


def main():
    for name, mask, colours in (
        ("pavement.png", PAVEMENT, PAVEMENT_COLOURS),
        ("road.png", ROAD, ROAD_COLOURS),
        ("grass.png", GRASS, GRASS_COLOURS),
    ):
        write_png(os.path.join(ASSETS, name), paint(mask, colours))
    return 0


if __name__ == "__main__":
    sys.exit(main())
