#!/usr/bin/env -S uv run --script
"""Draw the soldiers' training ground's marks (`src/game/training.rs`).

    uv run tools/training_art.py generate          # write assets/drill_mark.png, drill_centre.png
    uv run tools/training_art.py preview out.png   # both on dirt, enlarged

The ground is marked on the dirt the king picked: a patch of trampled earth
with a chalk dot on each of the sixteen waypoints of a soldier's lap
(`drill_mark.png`), and a bigger one with a chalk cross in the middle, where
the commander stands (`drill_centre.png`). Flat on the ground, under whoever
walks over them. 16x16 each, one character a texel, `.` transparent.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from human_art import paint, write_png  # noqa: E402

ASSETS = os.path.join(os.path.dirname(__file__), "..", "assets")

COLOURS = {
    "d": (92, 74, 52, 150),   # the dark rim of the trampled patch
    "e": (150, 124, 88, 190),  # packed earth
    "E": (176, 150, 108, 190),  # its lighter middle
    "c": (236, 232, 218, 255),  # chalk
}

MARK = [
    "................",
    "................",
    "................",
    "................",
    "................",
    "................",
    ".....dddddd.....",
    "....deEEEEed....",
    "...deEEccEEed...",
    "...deEEccEEed...",
    "....deEEEEed....",
    ".....dddddd.....",
    "................",
    "................",
    "................",
    "................",
]

CENTRE = [
    "................",
    "................",
    "....dddddddd....",
    "...deEEEEEEed...",
    "..deEEEEEEEEed..",
    "..deEcEEEEcEed..",
    "..deEEcEEcEEed..",
    "..deEEEccEEEed..",
    "..deEEEccEEEed..",
    "..deEEcEEcEEed..",
    "..deEcEEEEcEed..",
    "..deEEEEEEEEed..",
    "...deEEEEEEed...",
    "....dddddddd....",
    "................",
    "................",
]


def layers():
    return {"drill_mark.png": paint(MARK, COLOURS), "drill_centre.png": paint(CENTRE, COLOURS)}


def generate():
    for name, image in layers().items():
        write_png(os.path.join(ASSETS, name), image)
        print("wrote", name)


def preview(out, scale=8):
    dirt = (122, 98, 70, 255)
    images = list(layers().values())
    pad = 18
    sheet = [[dirt] * (pad * len(images) + 2) for _ in range(pad)]
    for n, image in enumerate(images):
        for y, row in enumerate(image):
            for x, (r, g, b, a) in enumerate(row):
                t = a / 255
                sheet[1 + y][1 + n * pad + x] = tuple(round(c * t + d * (1 - t)) for c, d in zip((r, g, b), dirt[:3])) + (255,)
    big = [[px for px in row for _ in range(scale)] for row in sheet for _ in range(scale)]
    write_png(out, big)


if __name__ == "__main__":
    if sys.argv[1:2] == ["generate"]:
        generate()
    elif sys.argv[1:2] == ["preview"] and len(sys.argv) == 3:
        preview(sys.argv[2])
    else:
        sys.exit(__doc__)
