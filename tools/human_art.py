#!/usr/bin/env -S uv run --script
"""Draw the human paperdoll's hair and outfit layers from masks and palettes.

Every hairstyle and outfit is a 16x16 mask here, one character a texel, and
every colour is a small palette; a layer is a mask painted in a palette. So a
new colour is one line and every style gets it, and a style is drawn once and
not once per colour by hand. The masks are drawn over `human_base.png`: the
face is columns 6-9 of rows 3-6 (eyes on rows 4-5), the torso rows 8-13, the
feet row 14.

    uv run tools/human_art.py generate          # write assets/human/, print the Rust lists
    uv run tools/human_art.py preview out.png   # every layer on the base body, at 4x

`generate` prints the `HAIR` and `CLOTHES` arrays for `src/characters/human.rs`.
"""

import os
import struct
import sys
import zlib

from check_pixel_grid import read_png

ASSETS = os.path.join(os.path.dirname(__file__), "..", "assets")

OUTLINE = (0, 0, 0, 255)  # the hair's outline, as the imported hair has it
SEAM = (22, 22, 26, 255)  # the body's own outline, for outfits that leave it

# Hair. `o` outline, `d` / `m` / `l` the colour's dark, mid and light.
HAIR_STYLES = {
    "bob": [
        "......oooo......",
        ".....odlmdo.....",
        "....odllmmdo....",
        "....oldlldlo....",
        "...old....dlo...",
        "...odl....ldo...",
        "....ol....lo....",
    ],
    "long": [
        "......oooo......",
        ".....odlmdo.....",
        "....odllmmdo....",
        "....oldlldlo....",
        "...old....dlo...",
        "...odl....ldo...",
        "...odl....ldo...",
        "...od......do...",
    ],
    "very_long": [
        "......oooo......",
        ".....odlmdo.....",
        "....odllmmdo....",
        "...odldlldldo...",
        "...old....dlo...",
        "...odl....ldo...",
        "...odl....ldo...",
        "...odl....ldo...",
        "...odl....ldo...",
        "...odd....ddo...",
        "....oo....oo....",
    ],
    "crop": [
        "................",
        "......oooo......",
        ".....odlmdo.....",
        "....odllmmdo....",
        "....od....do....",
        ".....o....o.....",
    ],
    "swept": [
        ".....ooooo......",
        "....odllmdo.....",
        "...odlllmmdo....",
        "...olllmldlo....",
        "...oll....do....",
        "....ol....o.....",
    ],
    "mohawk": [
        ".......oo.......",
        "......olmo......",
        "......olmo......",
        ".....odlmdo.....",
    ],
    "bun": [
        "......oooo......",
        ".....olmmdo.....",
        "....odoooodo....",
        "....odllmmdo....",
        "....od....do....",
        ".....o....o.....",
    ],
    "pigtails": [
        "......oooo......",
        ".....odlmdo.....",
        "....odllmmdo....",
        "...ooldlldloo...",
        "..olmd....dmlo..",
        "..olmo....omlo..",
        "..odo......odo..",
        "...o........o...",
    ],
    "afro": [
        ".....oooooo.....",
        "....odlmmldo....",
        "...odllmmlldo...",
        "..odlmllmmlmdo..",
        "..odlm....mldo..",
        "..odlm....mldo..",
        "...ooo....ooo...",
    ],
}

# (dark, mid, light). Blonde and blue are the imported hair's own colours.
HAIR_COLOURS = {
    "blonde": ((199, 168, 58), (225, 191, 66), (225, 216, 66)),
    "black": ((28, 26, 34), (48, 44, 56), (74, 68, 84)),
    "brown": ((74, 46, 26), (102, 66, 38), (134, 92, 54)),
    "auburn": ((112, 40, 26), (148, 60, 36), (180, 88, 52)),
    "ginger": ((190, 86, 30), (222, 118, 46), (244, 154, 72)),
    "grey": ((140, 140, 148), (182, 182, 190), (222, 222, 228)),
    "blue": ((7, 7, 139), (18, 18, 153), (12, 12, 224)),
    "pink": ((190, 66, 134), (220, 100, 166), (246, 148, 198)),
    "green": ((30, 116, 58), (46, 154, 78), (90, 198, 108)),
    "purple": ((88, 40, 146), (118, 60, 186), (158, 100, 218)),
}

# Natural colours come in every style, the dyed ones in a few, so a crowd is
# mostly people and only here and there a punk.
NATURAL = ["blonde", "black", "brown", "auburn", "ginger", "grey"]
DYED = {
    "blue": ["bob", "crop", "pigtails"],
    "pink": ["bob", "mohawk", "pigtails"],
    "green": ["mohawk", "crop", "swept"],
    "purple": ["long", "bun", "afro"],
}

# Outfits. `o` the body's seam colour, `d` / `m` / `l` the main colour, `p` /
# `q` the trousers' mid and dark, `b` a belt and `k` its buckle.
OUTFIT_STYLES = {
    "sundress": [
        "", "", "", "", "", "", "", "",
        "......d..d......",
        ".....dmmmmd.....",
        ".....dmlmmd.....",
        "....odmlmmdo....",
        "...odmmlmmmdo...",
        "...oooooooooo...",
    ],
    "gown": [
        "", "", "", "", "", "", "", "",
        ".....dd..dd.....",
        ".....dmmmmd.....",
        ".....dlllld.....",
        "....odmlmmdo....",
        "....odmlmmdo....",
        "...odmmlmmmdo...",
        "...oooooooooo...",
    ],
    "shorts": [
        "", "", "", "", "", "", "", "",
        ".....dm..md.....",
        "....dmmlmmmd....",
        ".....bbkbbb.....",
        ".....pqqqqp.....",
    ],
    "trousers": [
        "", "", "", "", "", "", "", "",
        ".....dm..md.....",
        "....dmmlmmmd....",
        ".....bbkbbb.....",
        ".....pppppp.....",
        "......pqqp......",
        "......pqqp......",
    ],
}

CLOTH = {
    "red": ((150, 30, 40), (190, 44, 52), (222, 80, 80)),
    "yellow": ((196, 150, 30), (230, 190, 50), (250, 222, 110)),
    "green": ((40, 110, 60), (56, 146, 80), (96, 184, 110)),
    "white": ((170, 170, 180), (214, 214, 222), (244, 244, 248)),
    "black": ((30, 30, 36), (48, 48, 58), (76, 76, 90)),
    "navy": ((24, 36, 92), (36, 54, 130), (64, 88, 170)),
    "purple": ((96, 46, 140), (126, 66, 176), (164, 108, 206)),
    "teal": ((20, 110, 120), (30, 148, 156), (80, 192, 196)),
    "orange": ((180, 80, 24), (220, 110, 40), (246, 150, 80)),
}

# (mid, dark)
TROUSERS = {
    "jeans": ((60, 90, 150), (40, 60, 110)),
    "khaki": ((166, 138, 90), (130, 106, 66)),
    "charcoal": ((60, 60, 70), (40, 40, 48)),
    "brown": ((108, 74, 46), (80, 54, 34)),
}
BELT = (40, 30, 24)
BUCKLE = (200, 180, 60)

DRESSES = {
    "sundress": ["red", "yellow", "green", "white", "teal", "orange", "purple"],
    "gown": ["red", "black", "navy", "purple", "white", "green"],
}
# (top, trousers)
SEPARATES = {
    "shorts": [("white", "jeans"), ("red", "khaki"), ("yellow", "jeans"), ("teal", "charcoal")],
    "trousers": [
        ("white", "charcoal"),
        ("black", "jeans"),
        ("green", "khaki"),
        ("navy", "brown"),
        ("orange", "jeans"),
    ],
}


def paint(mask, colours):
    """A 16x16 RGBA image (rows of tuples) of `mask` painted from `colours`."""
    rows = [[(0, 0, 0, 0)] * 16 for _ in range(16)]
    for y, line in enumerate(mask):
        if line and len(line) != 16:
            raise ValueError(f"mask row {y} is {len(line)} wide: {line!r}")
        for x, c in enumerate(line):
            if c != ".":
                rgb = colours[c]
                rows[y][x] = rgb if len(rgb) == 4 else (*rgb, 255)
    return rows


def hair_layers():
    """Every (file name, image) of hair."""
    for style, mask in HAIR_STYLES.items():
        for colour in NATURAL + [c for c, styles in DYED.items() if style in styles]:
            d, m, l = HAIR_COLOURS[colour]
            yield f"hair_{style}_{colour}.png", paint(mask, {"o": OUTLINE, "d": d, "m": m, "l": l})


def outfit_layers():
    """Every (file name, image) of clothes."""
    for style, colours in DRESSES.items():
        for colour in colours:
            d, m, l = CLOTH[colour]
            yield f"clothes_{style}_{colour}.png", paint(
                OUTFIT_STYLES[style], {"o": SEAM, "d": d, "m": m, "l": l}
            )
    for style, pairs in SEPARATES.items():
        for top, bottom in pairs:
            d, m, l = CLOTH[top]
            p, q = TROUSERS[bottom]
            yield f"clothes_{style}_{top}_{bottom}.png", paint(
                OUTFIT_STYLES[style],
                {"o": SEAM, "d": d, "m": m, "l": l, "p": p, "q": q, "b": BELT, "k": BUCKLE},
            )


def write_png(path, rows):
    raw = b"".join(b"\x00" + bytes(v for px in row for v in px) for row in rows)
    def chunk(kind, body):
        return (
            struct.pack(">I", len(body)) + kind + body
            + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)
        )
    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(chunk(b"IHDR", struct.pack(">IIBBBBB", len(rows[0]), len(rows), 8, 6, 0, 0, 0)))
        f.write(chunk(b"IDAT", zlib.compress(raw, 9)))
        f.write(chunk(b"IEND", b""))


def load(path):
    w, h, ch, px = read_png(path)
    assert ch == 4, path
    return [[tuple(px[(y * w + x) * 4 : (y * w + x) * 4 + 4]) for x in range(w)] for y in range(h)]


def over(dst, src):
    """Alpha-composite `src` onto `dst` in place."""
    for y, row in enumerate(src):
        for x, (r, g, b, a) in enumerate(row):
            if a == 0:
                continue
            dr, dg, db, da = dst[y][x]
            t = a / 255
            dst[y][x] = (
                round(r * t + dr * (1 - t)),
                round(g * t + dg * (1 - t)),
                round(b * t + db * (1 - t)),
                max(da, a),
            )


def generate():
    human = os.path.join(ASSETS, "human")
    lists = {"HAIR": [], "CLOTHES": []}
    for key, layers in (("HAIR", hair_layers()), ("CLOTHES", outfit_layers())):
        for name, image in layers:
            write_png(os.path.join(human, name), image)
            lists[key].append(f"human/{name}")
    for key, paths in lists.items():
        print(f"pub(super) const {key}: [&str; {len(paths)}] = [")
        for path in paths:
            print(f'    "{path}",')
        print("];")


def preview(out, scale=4):
    """Hair on one row per style and every outfit below, all on the base body."""
    human = os.path.join(ASSETS, "human")
    base = load(os.path.join(human, "human_base.png"))
    eyes = load(os.path.join(human, "eyes_brown.png"))
    hair = list(hair_layers())
    outfits = list(outfit_layers())
    cols = max(len(NATURAL) + 3, 12)
    cells = []
    for style in HAIR_STYLES:
        row = [img for name, img in hair if name.startswith(f"hair_{style}_")]
        cells.append([(None, img) for img in row])
    for i in range(0, len(outfits), cols):
        cells.append([(img, hair[i % len(hair)][1]) for _, img in outfits[i : i + cols]])
    pad = 18
    W, H = cols * pad, len(cells) * pad
    sheet = [[(60, 64, 60, 255)] * W for _ in range(H)]
    for cy, row in enumerate(cells):
        for cx, (cloth, hair_img) in enumerate(row):
            doll = [list(r) for r in base]
            over(doll, eyes)
            if cloth:
                over(doll, cloth)
            over(doll, hair_img)
            for y in range(16):
                for x in range(16):
                    if doll[y][x][3]:
                        over_px = [[doll[y][x]]]
                        cell = [[sheet[cy * pad + 1 + y][cx * pad + 1 + x]]]
                        over(cell, over_px)
                        sheet[cy * pad + 1 + y][cx * pad + 1 + x] = cell[0][0]
    big = [[px for px in row for _ in range(scale)] for row in sheet for _ in range(scale)]
    write_png(out, big)


if __name__ == "__main__":
    if sys.argv[1:2] == ["generate"]:
        generate()
    elif sys.argv[1:2] == ["preview"] and len(sys.argv) == 3:
        preview(sys.argv[2])
    else:
        sys.exit(__doc__)
