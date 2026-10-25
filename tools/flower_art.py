#!/usr/bin/env -S uv run --script
"""Native 16px gardening art: six flower silhouettes and their seedlings."""
from pathlib import Path
from human_art import write_png, uniform_layers

ASSETS = Path(__file__).resolve().parent.parent / "assets"
COLORS = [(231, 69, 70), (255, 207, 72), (94, 157, 242), (245, 129, 179), (244, 241, 224), (168, 109, 224)]
SHAPES = [
    [".ppp.", "ppppp", "ppcpp", ".ppp."],
    ["..p..", ".ppp.", "ppcpp", ".ppp.", "..p.."],
    ["p.p.p", ".ppp.", "ppcpp", ".ppp.", "p.p.p"],
    ["p.p.p", "ppppp", ".ppp.", "..p.."],
    ["..p..", ".ppp.", "ppcpp", ".ppp.", "..p.."],
    ["..p..", ".ppp.", ".pcp.", "ppppp", ".p.p."],
]


def bed(variety, blooming):
    pixels = [[(0, 0, 0, 0) for _ in range(16)] for _ in range(16)]

    def put(x, y, rgb, alpha=255):
        pixels[y][x] = (*rgb, alpha)

    for x in range(2, 14):
        put(x, 14, (30, 43, 26), 70)
    for x, top in [(4, 5), (10, 3)]:
        for y in range(top + 3 if blooming else 10, 14):
            put(x, y, (48, 99, 45))
            put(x + 1, y, (88, 149, 62))
        for dx, dy in [(-2, 10), (-1, 11), (2, 11), (1, 12)]:
            put(x + dx, dy, (108, 170, 70))
        if blooming:
            for dy, row in enumerate(SHAPES[variety]):
                for dx, c in enumerate(row):
                    if c != '.':
                        color = (255, 226, 122) if c == 'c' else COLORS[variety]
                        if dy > 2 and c != 'c':
                            color = tuple(max(0, v - 28) for v in color)
                        put(x - 2 + dx, top + dy, color)
    return pixels


def supplies():
    pixels = [[(0, 0, 0, 0) for _ in range(16)] for _ in range(16)]
    def rect(x1, y1, x2, y2, rgb):
        for y in range(y1, y2):
            for x in range(x1, x2):
                pixels[y][x] = (*rgb, 255)
    rect(2, 9, 14, 12, (105, 73, 47))
    rect(2, 9, 14, 10, (182, 137, 84))
    rect(3, 12, 5, 15, (72, 52, 36))
    rect(11, 12, 13, 15, (72, 52, 36))
    for x, rgb in [(3, COLORS[0]), (6, COLORS[1]), (9, COLORS[5])]:
        rect(x, 5, x+3, 9, (236, 221, 178))
        rect(x+1, 6, x+2, 8, rgb)
    rect(13, 4, 14, 9, (132, 99, 61))
    rect(12, 2, 15, 5, (146, 170, 172))
    return pixels


if __name__ == '__main__':
    for variety in range(6):
        for stage in range(2):
            write_png(ASSETS / f"flowers_{variety}_{stage}.png", bed(variety, bool(stage)))
    write_png(ASSETS / 'flower_supplies.png', supplies())
    for name, pixels in uniform_layers():
        if name == 'clothes_uniform_city_decorator.png':
            write_png(ASSETS / 'human' / name, pixels)
