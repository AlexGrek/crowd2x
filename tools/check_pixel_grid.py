#!/usr/bin/env python3
"""Verify that a captured frame is a genuine integer upscale.

Every source texel must appear as a solid SCALE x SCALE block of identical
screen pixels. If the renderer filtered the canvas, snapped a sprite to a
half-pixel, or stretched instead of scaling, blocks come out non-uniform and
this reports it.

The block grid is allowed to be phase-shifted: when the canvas is rounded up to
cover a window whose size is not a multiple of SCALE, it is centred and the grid
can start up to SCALE-1 pixels in. All offsets are tried and the best reported.

    python3 tools/check_pixel_grid.py screenshots/shot.png 4

Exits non-zero if the frame is not pixel-perfect, so it can gate a check.
"""

import struct
import sys
import zlib

_CHANNELS = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}


def read_png(path):
    """Decode a non-interlaced 8-bit PNG to (width, height, channels, bytes)."""
    data = open(path, "rb").read()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError(f"{path} is not a PNG")

    idat = b""
    width = height = depth = colour = None
    pos = 8
    while pos < len(data):
        length = struct.unpack(">I", data[pos : pos + 4])[0]
        kind = data[pos + 4 : pos + 8]
        body = data[pos + 8 : pos + 8 + length]
        if kind == b"IHDR":
            width, height, depth, colour = struct.unpack(">IIBB", body[:10])
        elif kind == b"IDAT":
            idat += body
        elif kind == b"IEND":
            break
        pos += 12 + length

    if depth != 8:
        raise ValueError(f"only 8-bit PNGs supported, got {depth}-bit")
    channels = _CHANNELS[colour]
    raw = zlib.decompress(idat)
    stride = width * channels
    out = bytearray(width * height * channels)
    prev = bytearray(stride)
    pos = 0
    for y in range(height):
        filt = raw[pos]
        pos += 1
        line = bytearray(raw[pos : pos + stride])
        pos += stride
        if filt == 1:  # Sub
            for x in range(channels, stride):
                line[x] = (line[x] + line[x - channels]) & 255
        elif filt == 2:  # Up
            for x in range(stride):
                line[x] = (line[x] + prev[x]) & 255
        elif filt == 3:  # Average
            for x in range(stride):
                left = line[x - channels] if x >= channels else 0
                line[x] = (line[x] + ((left + prev[x]) >> 1)) & 255
        elif filt == 4:  # Paeth
            for x in range(stride):
                a = line[x - channels] if x >= channels else 0
                b = prev[x]
                c = prev[x - channels] if x >= channels else 0
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pred = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[x] = (line[x] + pred) & 255
        out[y * stride : (y + 1) * stride] = line
        prev = line
    return width, height, channels, out


def check(path, scale):
    width, height, channels, px = read_png(path)

    def pixel(x, y):
        off = (y * width + x) * channels
        return bytes(px[off : off + channels])

    best = None
    for phase_y in range(scale):
        for phase_x in range(scale):
            bad = 0
            for by in range((height - phase_y) // scale):
                for bx in range((width - phase_x) // scale):
                    x0, y0 = phase_x + bx * scale, phase_y + by * scale
                    base = pixel(x0, y0)
                    if any(
                        pixel(x0 + dx, y0 + dy) != base
                        for dy in range(scale)
                        for dx in range(scale)
                    ):
                        bad += 1
            if best is None or bad < best[0]:
                best = (bad, phase_x, phase_y)
            if bad == 0:
                break
        if best[0] == 0:
            break

    bad, phase_x, phase_y = best
    blocks = ((height - phase_y) // scale) * ((width - phase_x) // scale)
    print(f"{path}: {width}x{height}, {channels} channels")
    print(f"grid phase: x={phase_x} y={phase_y}   blocks checked: {blocks}")
    if bad == 0:
        shifted = " (phase-shifted, expected on odd window sizes)" if (phase_x or phase_y) else ""
        print(f"PIXEL-PERFECT at {scale}x{shifted}")
        return 0
    print(f"NOT pixel-perfect: {bad} of {blocks} blocks are not uniform")
    return 1


if __name__ == "__main__":
    if len(sys.argv) != 3:
        sys.exit(f"usage: {sys.argv[0]} <frame.png> <scale>")
    sys.exit(check(sys.argv[1], int(sys.argv[2])))
