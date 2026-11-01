#!/usr/bin/env -S uv run --script
"""Draw an outpost's flag and its building tiles (`src/game/outposts.rs`).

    uv run tools/outpost_art.py generate          # write assets/outpost_flag.png, building_tile.png
    uv run tools/outpost_art.py preview out.png   # both on dirt, enlarged

The flag is the king's, put up where he decreed an outpost: a purple pennant
with a gold edge and a gold crown on a wooden pole, its foot in a cairn of
stones. The building tile is four frames, as the work on a tile of the plan
goes (`sim::outposts`): the steel laid out and four posts up; scaffolding
braced across them; a wall going up behind it; the wall nearly done, a plank
across the top. 16x16 each, one character a texel, `.` transparent.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from human_art import paint, write_png  # noqa: E402

ASSETS = os.path.join(os.path.dirname(__file__), "..", "assets")

OUTLINE = (34, 28, 30)

FLAG = [
    "..ow............",
    "..owoooooooo....",
    "..owppppppppo...",
    "..owpplppyypo...",
    "..owpplyyyypo...",
    "..owppyyyyppo...",
    "..owpppppppo....",
    "..owppppppo.....",
    "..owoooooo......",
    "..ow............",
    "..ow............",
    "..ow............",
    "..ow............",
    ".sowss..........",
    "sSSSSSs.........",
    ".hhhhh..........",
]

FLAG_COLOURS = {
    "o": OUTLINE,
    "w": (150, 104, 60),
    "p": (110, 52, 140),
    "l": (150, 92, 180),
    "y": (236, 196, 72),
    "s": (120, 116, 110),
    "S": (164, 160, 150),
    "h": (22, 22, 26, 82),
}

# A tile of the plan going up. `t` timber, `T` its lit side, `x` steel, `X`
# its shine, `b` / `B` brick and its mortar, `k` a plank.
STAGES = [
    [
        "................",
        "................",
        "................",
        "................",
        "..oo........oo..",
        "..tT........tT..",
        "..tT........tT..",
        "..tT........tT..",
        "..tT........tT..",
        "..tT........tT..",
        "..tT.oooooo.tT..",
        "..tT.oXXXXo.tT..",
        "..tT.oxxxxo.tT..",
        "..tTooooooootT..",
        ".otttttttttttto.",
        "..hhhhhhhhhhhh..",
    ],
    [
        "................",
        "................",
        "..oo........oo..",
        "..tT........tT..",
        "..tTkkkkkkkktT..",
        "..tTt......ttT..",
        "..tT.t....t.tT..",
        "..tT..t..t..tT..",
        "..tT...tt...tT..",
        "..tT..t..t..tT..",
        "..tT.t....t.tT..",
        "..tTt......ttT..",
        "..tTkkkkkkkktT..",
        "..tTooooooootT..",
        ".otttttttttttto.",
        "..hhhhhhhhhhhh..",
    ],
    [
        "................",
        "................",
        "..oo........oo..",
        "..tT........tT..",
        "..tTkkkkkkkktT..",
        "..tTt......ttT..",
        "..tT.t....t.tT..",
        "..tT..t..t..tT..",
        "..tTbbbBbbbbtT..",
        "..tTBBBBBBBBtT..",
        "..tTbbbbbBbbtT..",
        "..tTBBBBBBBBtT..",
        "..tTbBbbbbbbtT..",
        "..tTooooooootT..",
        ".otttttttttttto.",
        "..hhhhhhhhhhhh..",
    ],
    [
        "................",
        "................",
        "..oo........oo..",
        "..tTkkkkkkkktT..",
        "..tTbbbbbBbbtT..",
        "..tTBBBBBBBBtT..",
        "..tTbBbbbbbbtT..",
        "..tTBBBBBBBBtT..",
        "..tTbbbBbbbbtT..",
        "..tTBBBBBBBBtT..",
        "..tTbbbbbBbbtT..",
        "..tTBBBBBBBBtT..",
        "..tTbBbbbbbbtT..",
        "..tTooooooootT..",
        ".otttttttttttto.",
        "..hhhhhhhhhhhh..",
    ],
]

STAGE_COLOURS = {
    "o": OUTLINE,
    "t": (120, 82, 46),
    "T": (168, 124, 76),
    "k": (196, 160, 104),
    "x": (128, 134, 146),
    "X": (196, 204, 214),
    "b": (150, 96, 70),
    "B": (104, 70, 56),
    "h": (22, 22, 26, 82),
}


def frames():
    """The flag, and the building tile's strip of four frames side by side."""
    for rows in [FLAG] + STAGES:
        assert len(rows) == 16 and all(len(row) == 16 for row in rows), rows
    flag = paint(FLAG, FLAG_COLOURS)
    stages = [paint(rows, STAGE_COLOURS) for rows in STAGES]
    strip = [sum((stage[y] for stage in stages), []) for y in range(16)]
    return flag, strip


def generate():
    flag, strip = frames()
    write_png(os.path.join(ASSETS, "outpost_flag.png"), flag)
    write_png(os.path.join(ASSETS, "building_tile.png"), strip)


def preview(out, scale=8):
    flag, strip = frames()
    ground = (142, 116, 84, 255)
    sheet = [[ground] * (16 * 6) for _ in range(16)]
    for x0, image in [(0, flag)] + [(16 * (i + 2), [row[16 * i : 16 * (i + 1)] for row in strip]) for i in range(4)]:
        for y in range(16):
            for x in range(16):
                px = image[y][x]
                if px[3]:
                    a = px[3] / 255
                    under = sheet[y][x0 + x]
                    sheet[y][x0 + x] = tuple(round(px[i] * a + under[i] * (1 - a)) for i in range(3)) + (255,)
    big = [[px for px in row for _ in range(scale)] for row in sheet for _ in range(scale)]
    write_png(out, big)


if __name__ == "__main__":
    if sys.argv[1:2] == ["generate"]:
        generate()
    elif sys.argv[1:2] == ["preview"] and len(sys.argv) == 3:
        preview(sys.argv[2])
    else:
        sys.exit(__doc__)
