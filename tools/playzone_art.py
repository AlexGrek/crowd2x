#!/usr/bin/env -S uv run --script
"""Draw the playzone: an arcade cabinet and the central computer it is run from,
seen from the front and a little above like every prop, lit from the upper left.

Four strips of 16x16 frames, the TV's and the computer's way (the game swaps
between the idle and the used strip while somebody is at the prop,
`game::props`):

- `arcade_cabinet_idle.png`, 2 frames: a navy cabinet under a violet marquee,
  the screen dark glass, the coin light blinking;
- `arcade_cabinet.png`, 4 frames: somebody playing — a shooter, a maze, a race,
  a platformer — the coin light steady green;
- `playzone_computer_idle.png`, 2 frames: the central computer, a monitor over
  a wooden desk with a keyboard and a coin tray, the screen dark, its light
  blinking;
- `playzone_computer.png`, 2 frames: its administrator at it, the list of games
  on the screen with the highlight moving down it.

The administrator's uniform (`clothes_uniform_playzone_administrator.png`) is
drawn by `human_art.py`'s `UNIFORMS` and written here too.

    uv run tools/playzone_art.py                                 # write the four strips and the uniform
    uv run tools/playzone_art.py preview screenshots/playzone.png   # all of them on a floor, enlarged; writes no assets
"""

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from human_art import uniform_layers, write_png  # noqa: E402

ASSETS = os.path.join(os.path.dirname(__file__), "..", "assets")

COLOURS = {
    "o": (30, 24, 22, 255),      # outline
    "B": (52, 60, 128, 255),     # cabinet body, lit
    "b": (34, 40, 92, 255),      # cabinet body, shade
    "M": (150, 60, 190, 255),    # marquee
    "m": (252, 222, 120, 255),   # marquee lettering
    "k": (24, 24, 30, 255),      # bezel round the glass
    "s": (40, 48, 60, 255),      # dark glass
    "S": (84, 96, 114, 255),     # glint on dark glass
    "P": (82, 84, 104, 255),     # the control deck
    "J": (196, 196, 206, 255),   # joystick shaft
    "R": (226, 54, 46, 255),     # joystick ball, an enemy, a car
    "r": (96, 40, 36, 255),      # coin light, off
    "Y": (246, 222, 112, 255),   # button, a coin, a ship
    "G": (96, 222, 104, 255),    # button, grass, coin light on
    "C": (96, 200, 240, 255),    # button, a bullet
    "c": (224, 178, 64, 255),    # coin slot, coin tray
    "H": (0, 0, 0, 90),          # shadow, near
    "h": (0, 0, 0, 56),          # shadow, far
    # The pictures.
    "n": (20, 24, 84, 255),      # night sky, a maze floor
    "W": (240, 240, 250, 255),   # white
    "t": (96, 100, 112, 255),    # a road
    "p": (240, 110, 190, 255),   # a highlight
    # The computer.
    "D": (74, 44, 116, 255),     # monitor case, lit
    "d": (48, 28, 80, 255),      # monitor case, shade
    "w": (112, 70, 40, 255),     # desk, shade
    "F": (150, 98, 58, 255),     # desk, lit
    "L": (96, 222, 104, 255),    # power light, on
    "N": (176, 170, 160, 255),   # knob
}

# The cabinet, with `#` where the screen goes (6 wide, 3 high) and `@` the coin
# light.
CABINET = (
    "...oooooooooo...",
    "...oMMMMMMMMo...",
    "...oMmMmmMmMo...",
    "...oMMMMMMMMo...",
    "...oBBBBBBBBo...",
    "...okkkkkkkko...",
    "...ok######ko...",
    "...ok######ko...",
    "...ok######ko...",
    "...okkkkkkkko...",
    "...oPPPPPPPPo...",
    "...oPRJPYGCPo...",
    "...oBBBBBBBBo...",
    "...oBB@ccBBbo...",
    "...obbbbbbbbo...",
    "..HhhhhhhhhhhH..",
)

# The screen is 6 wide and 3 high, cols 5-10 of rows 6-8.
DARK = (
    "Ssssss",
    "ssssss",
    "ssssss",
)
PICTURES = (
    (  # a shooter: rows of invaders, a ship, a bullet
        "nRnRnR",
        "nnnCnn",
        "nnYnnn",
    ),
    (  # a maze: dots to eat, walls, a ghost
        "YnYnYn",
        "nWWWnR",
        "YnYnYn",
    ),
    (  # a race: a road with a car on it
        "tWttWt",
        "ttRttt",
        "tWttWt",
    ),
    (  # a platformer: a coin, a hero, the ground
        "nnnnYn",
        "nnWWnn",
        "GGGGGG",
    ),
)

# The central computer, with `#` where the screen goes (6 wide, 4 high) and `@`
# the light.
COMPUTER = (
    "................",
    "..oooooooooooo..",
    "..oDDDDDDDDDDo..",
    "..oDkkkkkkkkDo..",
    "..oDk######kDo..",
    "..oDk######kDo..",
    "..oDk######kDo..",
    "..oDk######kDo..",
    "..oDkkkkkkkkDo..",
    "..oDDDNDD@DDdo..",
    ".oooooooooooooo.",
    ".oFFFFFFFFFFFFo.",
    ".oFkkkkFFcccFFo.",
    ".owwwwwwwwwwwwo.",
    "..ow........wo..",
    ".HhhhhhhhhhhhhH.",
)

# The screen is 6 wide and 4 high, cols 5-10 of rows 4-7.
COMPUTER_DARK = (
    "Ssssss",
    "ssssss",
    "ssssss",
    "ssssss",
)
LISTS = (
    (  # the list of games, the first picked
        "ppppnn",
        "WWWnWn",
        "nnnnnn",
        "WWnWWn",
    ),
    (  # ...the second
        "WWWnWn",
        "ppppnn",
        "nnnnnn",
        "WWnWWn",
    ),
)


def frame(sheet, screen, light, origin):
    """`sheet` with `screen` set into the `#` cells (which start at `origin`,
    row then column of the first) and `light` where the `@` is."""
    top, left = origin
    rows = []
    for y, row in enumerate(sheet):
        line = []
        for x, c in enumerate(row):
            if c == "#":
                c = screen[y - top][x - left]
            elif c == "@":
                c = light
            line.append(COLOURS[c] if c != "." else (0, 0, 0, 0))
        rows.append(line)
    return rows


def strip(frames):
    return [sum((f[y] for f in frames), []) for y in range(16)]


def cabinet_frame(screen, light):
    return frame(CABINET, screen, light, (6, 5))


def computer_frame(screen, light):
    return frame(COMPUTER, screen, light, (4, 5))


def cabinet_idle():
    return strip([cabinet_frame(DARK, "R"), cabinet_frame(DARK, "r")])


def cabinet_on():
    return strip([cabinet_frame(picture, "G") for picture in PICTURES])


def computer_idle():
    return strip([computer_frame(COMPUTER_DARK, "L"), computer_frame(COMPUTER_DARK, "r")])


def computer_on():
    return strip([computer_frame(list_, "L") for list_ in LISTS])


SHEETS = (
    ("arcade_cabinet_idle.png", cabinet_idle),
    ("arcade_cabinet.png", cabinet_on),
    ("playzone_computer_idle.png", computer_idle),
    ("playzone_computer.png", computer_on),
)


def preview(path, scale=8):
    """Every strip, one above the other, on a coloured floor, enlarged."""
    sheets = [make() for _, make in SHEETS]
    width, height = 4 * 16 * scale, len(sheets) * 16 * scale
    floor = [(120, 96, 150), (108, 86, 138)]
    rows = [[(*floor[((y // scale) // 4) % 2], 255) for x in range(width)] for y in range(height)]
    for s, sheet in enumerate(sheets):
        for y in range(16):
            for x in range(len(sheet[y])):
                r, g, b, a = sheet[y][x]
                if a == 0:
                    continue
                t = a / 255
                for dy in range(scale):
                    for dx in range(scale):
                        px, py = x * scale + dx, (s * 16 + y) * scale + dy
                        under = rows[py][px]
                        rows[py][px] = tuple(round(c * t + u * (1 - t)) for c, u in zip((r, g, b), under[:3])) + (255,)
    write_png(path, rows)
    print(f"wrote {path}")


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "preview":
        preview(sys.argv[2] if len(sys.argv) > 2 else "screenshots/playzone_preview.png")
        return
    for name, make in SHEETS:
        write_png(os.path.join(ASSETS, name), make())
    for name, pixels in uniform_layers():
        if name == "clothes_uniform_playzone_administrator.png":
            write_png(os.path.join(ASSETS, "human", name), pixels)
    print("wrote assets/{arcade_cabinet_idle,arcade_cabinet,playzone_computer_idle,playzone_computer}.png "
          "and assets/human/clothes_uniform_playzone_administrator.png")


if __name__ == "__main__":
    main()
