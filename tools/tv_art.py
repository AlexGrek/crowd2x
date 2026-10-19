#!/usr/bin/env -S uv run --script
"""Draw the television: a wooden-cased set on short legs, seen from the front
and a little above like every prop, lit from the upper left.

Two strips of 16x16 frames, like the computer's:

- `tv_idle.png`, 2 frames: the screen dark grey glass, the standby light
  blinking red;
- `tv.png`, 4 frames: the screen on, a picture that changes from frame to
  frame (sky over hills, a face, a ball game, a newsreader), the light green.

The game swaps between them while somebody is watching (`game::props`).

    uv run tools/tv_art.py                                # write assets/tv_idle.png and assets/tv.png
    uv run tools/tv_art.py preview screenshots/tv.png     # both strips on a floor, enlarged; writes no assets
"""

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from human_art import write_png  # noqa: E402

ASSETS = os.path.join(os.path.dirname(__file__), "..", "assets")

COLOURS = {
    "o": (30, 24, 22, 255),     # outline
    "W": (150, 98, 58, 255),    # wood case, lit
    "w": (112, 70, 40, 255),    # wood case, shade
    "k": (52, 48, 50, 255),     # bezel round the glass
    "s": (60, 68, 72, 255),     # dark glass
    "S": (92, 104, 110, 255),   # glint on dark glass
    "R": (226, 54, 46, 255),    # standby light
    "r": (96, 40, 36, 255),     # standby light, off
    "L": (96, 222, 104, 255),   # power light, on
    "n": (176, 170, 160, 255),  # knob
    "H": (0, 0, 0, 90),         # shadow, near
    "h": (0, 0, 0, 56),         # shadow, far
    # The picture.
    "B": (120, 182, 236, 255),  # sky
    "b": (176, 214, 244, 255),  # pale sky, glow
    "G": (92, 168, 76, 255),    # grass
    "g": (60, 124, 56, 255),    # dark grass
    "Y": (246, 222, 112, 255),  # sun, a ball, a lamp
    "F": (232, 184, 150, 255),  # a face
    "f": (176, 124, 96, 255),   # shade of a face
    "D": (54, 48, 60, 255),     # hair, a suit
    "T": (210, 214, 222, 255),  # a white shirt, a desk top
    "C": (48, 92, 168, 255),    # a blue backdrop
}

# The set, with `#` where the screen goes and `@` the light.
CASE = (
    "................",
    "................",
    ".oooooooooooooo.",
    ".oWWWWWWWWWWWWo.",
    ".oWkkkkkkkkkWwo.",
    ".oWk########kwo.",
    ".oWk########kwo.",
    ".oWk########kwo.",
    ".oWk########kwo.",
    ".oWk########kwo.",
    ".oWkkkkkkkkkkwo.",
    ".oWWWWWWWn@nWwo.",
    ".owwwwwwwwwwwwo.",
    "..oooooooooooo..",
    "..ow........wo..",
    ".HhhhhhhhhhhhhH.",
)

# The screen is 8 wide and 5 high, cols 4-11 of rows 5-9.
DARK = (
    "Sssssss.",
    "sSsssss.",
    "ssssssss",
    "ssssssss",
    "ssssssss",
)
PICTURES = (
    (  # sky over hills, the sun up
        "BBBBBBYB",
        "bBBBBBBB",
        "BBBGGBBB",
        "GGGGGGGG",
        "gGGgGGgG",
    ),
    (  # a face, close up
        "CCDDDDCC",
        "CDFFFFDC",
        "CCFDFDCC",
        "CCFFFfCC",
        "CTTffTTC",
    ),
    (  # a ball game on a pitch
        "GGGGGGGG",
        "GgGGGGgG",
        "GGGTGGGG",
        "GGGGGYGG",
        "gGGGGGGg",
    ),
    (  # the news at a desk
        "CCCDDCCC",
        "CCFFFFCC",
        "CCDFFDCC",
        "TTTTTTTT",
        "TTTTTTTT",
    ),
)


def frame(screen, light):
    rows = []
    for y, row in enumerate(CASE):
        line = []
        for x, c in enumerate(row):
            if c == "#":
                c = screen[y - 5][x - 4]
                if c == ".":
                    c = "s"
            elif c == "@":
                c = light
            line.append(COLOURS[c] if c != "." else (0, 0, 0, 0))
        rows.append(line)
    return rows


def strip(frames):
    return [sum((f[y] for f in frames), []) for y in range(16)]


def idle():
    return strip([frame(DARK, "R"), frame(DARK, "r")])


def on():
    return strip([frame(picture, "L") for picture in PICTURES])


def preview(path, scale=8):
    """Both strips, one above the other, on a wooden floor, enlarged."""
    sheets = [idle(), on()]
    width, height = 4 * 16 * scale, 2 * 16 * scale
    floor = [(150, 118, 84), (138, 108, 76)]
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
        preview(sys.argv[2] if len(sys.argv) > 2 else "screenshots/tv_preview.png")
        return
    write_png(os.path.join(ASSETS, "tv_idle.png"), idle())
    write_png(os.path.join(ASSETS, "tv.png"), on())
    print("wrote assets/tv_idle.png (2 frames) and assets/tv.png (4 frames), 16x16 each")


if __name__ == "__main__":
    main()
