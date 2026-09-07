#!/usr/bin/env python3
"""Find sprites that are really a smaller image blown up, and shrink them back.

Much of the imported art is an upscale: `doghd.png` is 48x48, but every texel
is a solid 3x3 block, so the real drawing is 16x16. Storing it upscaled costs
nothing in quality but throws away the game's ability to decide the scale, and
makes "one asset pixel" mean different things in different files. This finds
the true resolution and writes the reduced image; the game scales it back up.

    python3 tools/art_scale.py report assets            # scan a tree
    python3 tools/art_scale.py shrink assets/doghd.png assets/dog_small.png

`shrink` refuses to run unless the reduction is lossless, so it can never
silently degrade art that only looks blocky.
"""

import os
import struct
import sys
import zlib

from check_pixel_grid import read_png


def divisors(n):
    """Every divisor of n above 1, largest first."""
    return [d for d in range(n, 1, -1) if n % d == 0]


def native_scale(width, height, channels, px):
    """Largest N where the image is solid NxN blocks - its real pixel size."""
    for n in divisors(min(width, height)):
        if width % n or height % n:
            continue
        if uniform_blocks(width, height, channels, px, n):
            return n
    return 1


def uniform_blocks(width, height, channels, px, n):
    for y0 in range(0, height, n):
        for x0 in range(0, width, n):
            base = px[(y0 * width + x0) * channels :][:channels]
            for dy in range(n):
                row = (y0 + dy) * width
                for dx in range(n):
                    off = (row + x0 + dx) * channels
                    if px[off : off + channels] != base:
                        return False
    return True


def reduce_image(width, height, channels, px, n):
    """Take the top-left texel of every NxN block."""
    out = bytearray((width // n) * (height // n) * channels)
    pos = 0
    for y in range(0, height, n):
        for x in range(0, width, n):
            off = (y * width + x) * channels
            out[pos : pos + channels] = px[off : off + channels]
            pos += channels
    return out


def write_png(path, width, height, channels, px):
    colour = {1: 0, 2: 4, 3: 2, 4: 6}[channels]
    stride = width * channels
    raw = bytearray()
    for y in range(height):
        raw.append(0)  # filter: None
        raw += px[y * stride : (y + 1) * stride]

    def chunk(kind, body):
        return (
            struct.pack(">I", len(body))
            + kind
            + body
            + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)
        )

    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, colour, 0, 0, 0)))
        f.write(chunk(b"IDAT", zlib.compress(bytes(raw), 9)))
        f.write(chunk(b"IEND", b""))


def report(root):
    rows = []
    for dirpath, _, names in os.walk(root):
        for name in sorted(names):
            if not name.endswith(".png"):
                continue
            path = os.path.join(dirpath, name)
            try:
                w, h, c, px = read_png(path)
            except ValueError as e:
                print(f"skip {path}: {e}")
                continue
            rows.append((native_scale(w, h, c, px), w, h, path))

    for scale, w, h, path in sorted(rows, key=lambda r: (-r[0], r[3])):
        note = f"-> {w // scale}x{h // scale}" if scale > 1 else "native"
        print(f"{scale}x  {w:>4}x{h:<4} {note:<12} {path}")
    upscaled = sum(1 for r in rows if r[0] > 1)
    print(f"\n{upscaled} of {len(rows)} images are upscales")


def shrink(src, dst):
    w, h, c, px = read_png(src)
    n = native_scale(w, h, c, px)
    if n == 1:
        sys.exit(f"{src} is already at native resolution")
    small = reduce_image(w, h, c, px, n)
    write_png(dst, w // n, h // n, c, small)
    print(f"{src} {w}x{h} -> {dst} {w // n}x{h // n} (was {n}x)")


if __name__ == "__main__":
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    if len(sys.argv) == 3 and sys.argv[1] == "report":
        report(sys.argv[2])
    elif len(sys.argv) == 4 and sys.argv[1] == "shrink":
        shrink(sys.argv[2], sys.argv[3])
    else:
        sys.exit(f"usage: {sys.argv[0]} report <dir> | shrink <src.png> <dst.png>")
