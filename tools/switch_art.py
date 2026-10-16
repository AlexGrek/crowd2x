#!/usr/bin/env -S uv run --script
"""Draw the light switch: a strip of two 16x16 frames, on and off.

A switch plate on the wall above its cell, seen from above: drawn against the
top edge, with a sliver of shadow under it on the floor. The game turns it a
quarter at a time to fit whichever wall it is on (`map::mount`), so it is
only ever drawn for the wall above. Frame 0 is on (the rocker's top pressed
out, its lamp amber), frame 1 off; which one shows is the simulation's
(`sim::light_switch`), so the strip is never played as an animation.

    uv run tools/switch_art.py     # write assets/light_switch.png, 32x16
"""

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from human_art import write_png  # noqa: E402

ASSETS = os.path.join(os.path.dirname(__file__), "..", "assets")

# `u` is the rocker's upper half and `d` its lower one; `i` is the little
# lamp in the plate that says the room's lights are on.
PLATE = (
    ".....aaaaaa.....",
    ".....abbbba.....",
    ".....abuuba.....",
    ".....abddba.....",
    ".....abiiba.....",
    ".....aaaaaa.....",
    "......hhhhh.....",
    "................",
    "................",
    "................",
    "................",
    "................",
    "................",
    "................",
    "................",
    "................",
)

COMMON = {
    "a": (66, 60, 52, 255),  # outline
    "b": (230, 224, 206, 255),  # the plate
    "h": (0, 0, 0, 90),  # shadow on the floor
}

PRESSED_OUT = (252, 250, 242, 255)  # the half of the rocker sticking out, catching the light
PRESSED_IN = (164, 156, 140, 255)  # the half pushed in, in its own shade

ON = dict(COMMON, u=PRESSED_OUT, d=PRESSED_IN, i=(255, 186, 58, 255))
OFF = dict(COMMON, u=PRESSED_IN, d=PRESSED_OUT, i=(120, 112, 100, 255))


def paint(mask, colours):
    assert len(mask) == 16 and all(len(row) == 16 for row in mask)
    return [[colours[c] if c != "." else (0, 0, 0, 0) for c in row] for row in mask]


def main():
    on, off = paint(PLATE, ON), paint(PLATE, OFF)
    write_png(os.path.join(ASSETS, "light_switch.png"), [a + b for a, b in zip(on, off)])
    print("wrote assets/light_switch.png, two 16x16 frames: on, off")


if __name__ == "__main__":
    main()
