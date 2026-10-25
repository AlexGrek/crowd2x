#!/usr/bin/env -S uv run --script
"""Draw the deathdog: the dog's idle strips recoloured — charcoal and ash fur,
a black outline, eyes red as coals.

The dog's sheets are the source (`assets/dog_idle.png`, facing right, and
`assets/dog_idle_reversed.png`, its mirror): every opaque pixel is put on the
deathdog's ramp by how light it was, so the shape, the shading and the four
frames of the tail are the dog's own, and the eye pixels are painted red.
Still 16x16 frames, four to a strip; the game upscales them.

The fur is ash rather than black on purpose: a deathdog comes out at night,
and the lightmap would swallow a black one whole. Check it in a night capture
(`debugger` skill) after changing the ramp.

    uv run tools/deathdog_art.py                                    # write assets/deathdog_idle.png and _reversed
    uv run tools/deathdog_art.py preview screenshots/deathdog.png   # dog and deathdog side by side, enlarged
"""

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from human_art import load, write_png  # noqa: E402

ASSETS = os.path.join(os.path.dirname(__file__), "..", "assets")
FRAME = 16
FRAMES = 4

# How light a dog pixel is -> the deathdog's colour for it, darkest first.
RAMP = [
    (110, (24, 20, 22, 255)),     # the outline
    (150, (66, 60, 62, 255)),     # fur in shade
    (190, (98, 92, 94, 255)),     # fur
    (215, (130, 122, 122, 255)),  # fur, lit
    (256, (166, 156, 152, 255)),  # ash highlight
]
EYE = (232, 40, 30, 255)

# The eye, in a right-facing frame: the dark band under the brow, the end of
# it nearest the snout. The same in every frame; mirrored for the left sheet.
EYE_PIXELS = [(11, 3), (12, 3)]

SHEETS = [
    ("dog_idle.png", "deathdog_idle.png", False),
    ("dog_idle_reversed.png", "deathdog_idle_reversed.png", True),
]


def lightness(pixel):
    r, g, b, _ = pixel
    return 0.299 * r + 0.587 * g + 0.114 * b


def recolour(rows, mirrored):
    out = []
    for row in rows:
        line = []
        for pixel in row:
            if pixel[3] == 0:
                line.append((0, 0, 0, 0))
                continue
            light = lightness(pixel)
            line.append(next(colour for top, colour in RAMP if light < top))
        out.append(line)
    for frame in range(FRAMES):
        for x, y in EYE_PIXELS:
            column = FRAME - 1 - x if mirrored else x
            out[y][frame * FRAME + column] = EYE
    return out


def generate():
    for source, target, mirrored in SHEETS:
        rows = load(os.path.join(ASSETS, source))
        assert len(rows) == FRAME and len(rows[0]) == FRAME * FRAMES, source
        write_png(os.path.join(ASSETS, target), recolour(rows, mirrored))
        print(f"wrote assets/{target}")


def preview(out, scale=8):
    """The dog's first frame and the deathdog's, both ways round, on dark
    ground and on light, enlarged."""
    pictures = []
    for source, _, mirrored in SHEETS:
        rows = load(os.path.join(ASSETS, source))
        pictures.append([row[:FRAME] for row in rows])
        pictures.append([row[:FRAME] for row in recolour(rows, mirrored)])
    grounds = [(28, 30, 40, 255), (150, 140, 120, 255)]
    canvas = []
    for ground in grounds:
        for y in range(FRAME):
            line = []
            for picture in pictures:
                for x in range(FRAME):
                    pixel = picture[y][x]
                    line += [pixel if pixel[3] else ground] * scale
                line += [ground] * scale
            for _ in range(scale):
                canvas.append(line)
    write_png(out, canvas)
    print(f"wrote {out}")


if __name__ == "__main__":
    if len(sys.argv) >= 3 and sys.argv[1] == "preview":
        preview(sys.argv[2])
    else:
        generate()
