#!/usr/bin/env -S uv run --script
"""Draw the store's props and its staff's uniforms.

* `superfridge.png` - the store's superfridge, the district's food: a wide
  white double-door fridge with a green sign, its glass doors showing the
  shelves; four 16x16 frames side by side (empty, a few meals on the bottom
  shelf, two shelves stocked, every shelf stocked), chosen by the game from
  how much it holds (`game::props`, `fill_bins`), like the warehouse's bins.
* `shop_counter.png` - the counter a shopkeeper minds: a wooden counter with
  a till on it.
* `human/clothes_uniform_shopkeeper.png` and
  `human/clothes_uniform_store_courier.png` - from `human_art.UNIFORMS`;
  only these two are written, so the rest of `assets/human/` is left alone.

Seen from the front and a little above like every prop, lit from the upper
left, on a strip of soft shadow. No baked-in upscale.

    uv run tools/store_art.py                                   # write the assets
    uv run tools/store_art.py preview screenshots/store.png     # every frame, enlarged; writes no assets
"""

import os
import sys

sys.path.insert(0, os.path.dirname(__file__))
from human_art import uniform_layers, write_png  # noqa: E402
import steel_factory_art as steel  # noqa: E402

ASSETS = os.path.join(os.path.dirname(__file__), "..", "assets")

COLOURS = dict(steel.COLOURS)
COLOURS.update(
    {
        "E": (232, 236, 240, 255),  # enamel, lit
        "e": (196, 204, 212, 255),  # enamel
        "n": (150, 158, 168, 255),  # enamel, shaded
        "G": (84, 172, 104, 255),  # the sign, lit
        "g": (52, 126, 74, 255),  # the sign
        "c": (164, 212, 230, 255),  # cold glass
        "C": (206, 236, 246, 255),  # glass, catching the light
        "l": (104, 140, 160, 255),  # a shelf behind the glass
        "M": (246, 232, 188, 255),  # a meal carton, lit
        "m": (198, 178, 128, 255),  # a meal carton, shaded
        "r": (198, 72, 58, 255),  # a carton's red label
        "t": (110, 204, 150, 255),  # the till's screen
        "v": (210, 210, 200, 255),  # a key
    }
)

SUPERFRIDGE = (
    "................",
    ".oooooooooooooo.",
    ".oEEEEEEEEEEEeo.",
    ".oGgGgGgGgGgGgo.",
    ".oooooooooooooo.",
    ".oeCccceeCcccno.",
    ".oeCccceeCcccno.",
    ".oecccceeccccno.",
    ".oelllleellllno.",
    ".oecccckkccccno.",
    ".oecccckkccccno.",
    ".oelllleellllno.",
    ".oecccceeccccno.",
    ".oecccceeccccno.",
    ".oooooooooooooo.",
    ".HhhhhhhhhhhhhH.",
)


def superfridge_base():
    """Glass at columns 3-6 and 9-12, two shelves behind it, the frame
    between and round the doors, a glint on each and the handles."""
    return [list(row) for row in SUPERFRIDGE]


# A shelf's worth of meals behind one door: cartons with a red label.
CARTONS = ("rMrM", "mmmm")


def superfridge_frames():
    """Empty, a few meals on the bottom shelf, the bottom two shelves
    stocked, every shelf stocked."""
    frames = []
    shelves = {0: [], 1: [(12, 3)], 2: [(12, 3), (12, 9), (9, 3), (9, 9)],
               3: [(12, 3), (12, 9), (9, 3), (9, 9), (6, 3), (6, 9)]}
    for step in range(4):
        rows = superfridge_base()
        for place in shelves[step]:
            y, x = place[0], place[1]
            if len(place) == 3:
                steel.put(rows, y, x, place[2])
                steel.put(rows, y + 1, x, "mm")
            else:
                steel.put(rows, y, x, CARTONS[0])
                steel.put(rows, y + 1, x, CARTONS[1])
        frames.append(steel.frame(rows))
    return frames


COUNTER = (
    "................",
    "................",
    "................",
    "........oooooo..",
    "........otttto..",
    ".......oooooooo.",
    ".......okvkvkvo.",
    ".oooooooooooooo.",
    ".oUUUUUUUUUUUuo.",
    ".oPPPPPPPPPPPpo.",
    ".oPopppppppopPo.",
    ".oPoPPPPPPPoPpo.",
    ".oPoPPPPPPPoPpo.",
    ".oPopppppppopPo.",
    ".oooooooooooooo.",
    ".HhhhhhhhhhhhhH.",
)


def counter_frames():
    rows = [list(row) for row in COUNTER]
    return [steel.frame(rows)]


def paint(mask):
    return [[COLOURS[c] if c != "." else (0, 0, 0, 0) for c in row] for row in mask]


def strip(masks):
    frames = [paint(mask) for mask in masks]
    return [sum((f[y] for f in frames), []) for y in range(16)]


SHEETS = {
    "superfridge.png": superfridge_frames,
    "shop_counter.png": counter_frames,
}

UNIFORMS = ("clothes_uniform_shopkeeper.png", "clothes_uniform_store_courier.png")


def preview(path, scale=6):
    """Every sheet, a row each, on the store's red floor, enlarged."""
    floor = (150, 70, 62)
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
        preview(sys.argv[2] if len(sys.argv) > 2 else "screenshots/store_preview.png")
        return
    for name, make in SHEETS.items():
        write_png(os.path.join(ASSETS, name), strip(make()))
        print(f"wrote assets/{name}")
    for name, pixels in uniform_layers():
        if name in UNIFORMS:
            write_png(os.path.join(ASSETS, "human", name), pixels)
            print(f"wrote assets/human/{name}")


if __name__ == "__main__":
    main()
