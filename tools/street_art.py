#!/usr/bin/env -S uv run --script
"""Draw the street lighting props: a lamp post and a street light controller.

Both are 16x16, seen from the front and a little above like the transformer,
standing on a strip of their own shadow. The controller is a grey cabinet
with a photocell on top: it switches the distribution box under it on while
it is dark (`map::utilities::CONTROLLER`). The lamp's light is not drawn; it
is the lightmap's (`lighting::scene::EMITTERS`, "street lamp").

    uv run tools/street_art.py     # write assets/street_lamp.png and assets/street_light_controller.png
"""

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from human_art import write_png  # noqa: E402

ASSETS = os.path.join(os.path.dirname(__file__), "..", "assets")

LAMP = (
    "................",
    ".....aaaaaa.....",
    "....abbbbbba....",
    "....acccccca....",
    ".....adddda.....",
    ".......ef.......",
    ".......ef.......",
    ".......ef.......",
    ".......ef.......",
    ".......ef.......",
    ".......ef.......",
    ".......ef.......",
    "......aeefa.....",
    ".....aggggga....",
    ".....hhhhhhh....",
    "................",
)

LAMP_COLOURS = {
    "a": (34, 36, 42, 255),  # outline
    "b": (88, 92, 100, 255),  # the hood
    "c": (250, 214, 120, 255),  # sodium glass
    "d": (255, 238, 176, 255),  # the glass's hot underside
    "e": (122, 126, 134, 255),  # the post, lit side
    "f": (72, 74, 82, 255),  # the post, shaded side
    "g": (96, 98, 106, 255),  # the foot
    "h": (52, 52, 58, 255),  # shadow
}

CONTROLLER = (
    "................",
    "................",
    ".......aa.......",
    "......abba......",
    ".......cc.......",
    "...aaaaaaaaaa...",
    "...adddddddda...",
    "...aeeeeeeeea...",
    "...aeffffeeea...",
    "...aeeeeeeeea...",
    "...aeeeeeeeea...",
    "...aeeeeeieea...",
    "...aeeeeeeeea...",
    "...aaaaaaaaaa...",
    "..hhhhhhhhhhhh..",
    "................",
)

CONTROLLER_COLOURS = {
    "a": (36, 40, 44, 255),  # outline
    "b": (214, 232, 244, 255),  # the photocell's dome
    "c": (90, 94, 100, 255),  # its stem
    "d": (150, 160, 158, 255),  # the lid, catching the light
    "e": (112, 124, 120, 255),  # the cabinet
    "f": (232, 200, 64, 255),  # a warning label
    "i": (30, 30, 34, 255),  # the lock
    "h": (52, 52, 58, 255),  # shadow
}


def paint(mask, colours):
    assert len(mask) == 16 and all(len(row) == 16 for row in mask)
    return [[colours[c] if c != "." else (0, 0, 0, 0) for c in row] for row in mask]


def main():
    write_png(os.path.join(ASSETS, "street_lamp.png"), paint(LAMP, LAMP_COLOURS))
    write_png(os.path.join(ASSETS, "street_light_controller.png"), paint(CONTROLLER, CONTROLLER_COLOURS))
    print("wrote assets/street_lamp.png and assets/street_light_controller.png, 16x16")


if __name__ == "__main__":
    main()
