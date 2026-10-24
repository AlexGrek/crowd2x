#!/usr/bin/env -S uv run --script
"""Draw the power plant's props.

* `generator.png` (2 frames) - what makes the district's power: a green-grey
  housing with a round fan grille whose blades turn (a cross, then a
  saltire), a yellow plate with a black bolt, a status lamp blinking green,
  and a band of hazard stripes along its foot.
* `control_panel_idle.png` (2 frames) and `control_panel.png` (2 frames) -
  the panel the power plant workers stand by: two screens over a sloping desk
  of buttons. Idle, the screens are dark but for a standby dot and the
  buttons blink; while somebody stands by it the screens show their traces
  (the game swaps the strips, `game::props`).

Seen from the front and a little above like every prop, lit from the upper
left, on a strip of soft shadow.

    uv run tools/power_plant_art.py                                     # write the assets
    uv run tools/power_plant_art.py preview screenshots/power_plant.png  # every frame, enlarged; writes no assets
"""

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from human_art import write_png  # noqa: E402

ASSETS = os.path.join(os.path.dirname(__file__), "..", "assets")

COLOURS = {
    "o": (34, 28, 26, 255),  # outline
    "L": (170, 186, 176, 255),  # housing, its lit top
    "M": (128, 146, 138, 255),  # housing, its top
    "N": (98, 118, 110, 255),  # housing
    "n": (70, 86, 80, 255),  # housing, shaded edge
    "d": (30, 32, 36, 255),  # behind the grille
    "b": (182, 190, 196, 255),  # a fan blade
    "h": (124, 130, 136, 255),  # the fan's hub
    "Y": (226, 192, 60, 255),  # warning yellow
    "K": (40, 38, 36, 255),  # warning black
    "G": (124, 236, 124, 255),  # a green lamp, lit
    "g": (52, 108, 60, 255),  # a green lamp, dim
    "R": (240, 84, 70, 255),  # a red lamp, lit
    "r": (112, 44, 40, 255),  # a red lamp, dim
    "A": (250, 210, 90, 255),  # an amber lamp, lit
    "a": (120, 96, 40, 255),  # an amber lamp, dim
    "w": (196, 202, 210, 255),  # panel steel, lit
    "S": (152, 158, 166, 255),  # panel steel
    "s": (104, 110, 118, 255),  # panel steel, shaded
    "P": (178, 182, 188, 255),  # the desk's sloping top
    "Q": (112, 118, 126, 255),  # the desk's front
    "c": (28, 70, 62, 255),  # a screen, on
    "e": (120, 230, 170, 255),  # a trace on a screen
    "z": (60, 120, 90, 255),  # a screen's standby dot
    "H": (0, 0, 0, 90),  # shadow, near
    "h": (0, 0, 0, 56),  # shadow, far
}

# --- the generator ------------------------------------------------------------

GENERATOR = (
    "................",
    ".oooooooooooooo.",
    ".oLLLLLLLLLLLMo.",
    ".oMMMMMMMMMMMno.",
    ".oNNoooNNNNNNno.",
    ".oNodddoNNYYKno.",
    ".oodddddoNYKKno.",
    ".ooddhddoNKKYno.",
    ".oodddddoNKYYno.",
    ".oNodddoNNGNNno.",
    ".oNNoooNNNNNNno.",
    ".oYYKKYYKKYYKKo.",
    ".oYKKYYKKYYKKYo.",
    ".oooooooooooooo.",
    ".HhhhhhhhhhhhhH.",
    "................",
)

# The fan's blades, (row, column) round the hub at (7, 5): a cross, then a
# saltire laid flat, so two frames turn it.
BLADES = [
    [(5, 5), (6, 5), (8, 5), (9, 5), (7, 3), (7, 4), (7, 6), (7, 7)],
    [(6, 3), (6, 4), (6, 6), (6, 7), (8, 3), (8, 4), (8, 6), (8, 7)],
]
# The status lamp blinks.
STATUS = ["G", "g"]


def generator_frames():
    frames = []
    for blades, lamp in zip(BLADES, STATUS):
        rows = [list(row) for row in GENERATOR]
        for y, x in blades:
            assert rows[y][x] == "d", (y, x)
            rows[y][x] = "b"
        rows[9][10] = lamp
        frames.append(frame(rows))
    return frames


# --- the control panel --------------------------------------------------------

PANEL = (
    "................",
    "................",
    ".oooooooooooooo.",
    ".owwwwwwwwwwwso.",
    ".owddddwwddddso.",
    ".owddddwwddddso.",
    ".owddddwwddddso.",
    ".oSSSSSSSSSSSso.",
    ".oooooooooooooo.",
    ".oPPPPPPPPPPPPo.",
    ".oPPPPPPPPPPPPo.",
    ".oQQQQQQQQQQQQo.",
    ".oQQQQQQQQQQQQo.",
    ".oooooooooooooo.",
    ".HhhhhhhhhhhhhH.",
    "................",
)

# The buttons along the desk, columns 3, 5, ... 11 of row 9, each frame's.
BUTTONS = ["RgArG", "rGaRg"]
SCREENS = ((3, 4), (9, 4))  # each screen's top-left (column, row), 4x3

# What the two screens show while somebody stands by, a frame each.
TRACES = [
    (("ccec", "cece", "eccc"), ("eecc", "cccc", "ecec")),
    (("eccc", "cece", "ccec"), ("ecec", "eecc", "cccc")),
]
# Idle: dark, a standby dot on the first screen every other frame.
STANDBY = [(("dddd", "dddd", "dddz"), ("dddd", "dddd", "dddd")), (("dddd", "dddd", "dddd"),) * 2]


def panel_frames(screens):
    frames = []
    for buttons, shown in zip(BUTTONS, screens):
        rows = [list(row) for row in PANEL]
        for i, lamp in enumerate(buttons):
            rows[9][3 + 2 * i] = lamp
        for (x, y), lines in zip(SCREENS, shown):
            for dy, line in enumerate(lines):
                put(rows, y + dy, x, line)
        frames.append(frame(rows))
    return frames


# --- writing ------------------------------------------------------------------


def put(rows, y, x, text):
    """Write `text` into row `y` from column `x`, leaving `.` see-through."""
    for i, c in enumerate(text):
        if c != ".":
            rows[y][x + i] = c


def frame(rows):
    out = ["".join(row) for row in rows]
    assert len(out) == 16 and all(len(row) == 16 for row in out), out
    return out


def paint(mask):
    return [[COLOURS[c] if c != "." else (0, 0, 0, 0) for c in row] for row in mask]


def strip(masks):
    frames = [paint(mask) for mask in masks]
    return [sum((f[y] for f in frames), []) for y in range(16)]


SHEETS = {
    "generator.png": generator_frames,
    "control_panel_idle.png": lambda: panel_frames(STANDBY),
    "control_panel.png": lambda: panel_frames(TRACES),
}


def preview(path, scale=6):
    """Every sheet, a row each, on the plant's floor, enlarged."""
    floor = (96, 128, 150)
    sheets = [make() for make in SHEETS.values()]
    width = (18 * max(len(s) for s in sheets) + 2) * scale
    height = (18 * len(sheets) + 2) * scale
    rows = [[(*floor, 255) for _ in range(width)] for _ in range(height)]
    for r, masks in enumerate(sheets):
        for n, mask in enumerate(masks):
            art = paint(mask)
            ox, oy = (2 + n * 18) * scale, (2 + r * 18) * scale
            for y in range(16):
                for x in range(16):
                    red, green, blue, a = art[y][x]
                    if a == 0:
                        continue
                    t = a / 255
                    for dy in range(scale):
                        for dx in range(scale):
                            px, py = ox + x * scale + dx, oy + y * scale + dy
                            under = rows[py][px]
                            rows[py][px] = tuple(
                                round(c * t + u * (1 - t)) for c, u in zip((red, green, blue), under[:3])
                            ) + (255,)
    write_png(path, rows)
    print(f"wrote {path}")


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "preview":
        preview(sys.argv[2] if len(sys.argv) > 2 else "screenshots/power_plant_preview.png")
        return
    for name, make in SHEETS.items():
        masks = make()
        write_png(os.path.join(ASSETS, name), strip(masks))
        print(f"wrote assets/{name}, {16 * len(masks)}x16")


if __name__ == "__main__":
    main()
