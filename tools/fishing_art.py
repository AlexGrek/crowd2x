"""Draw four true-resolution 16px frames of the warehouse's fish bin.

Uses the existing prop mask system. Run: python tools/fishing_art.py
"""
from pathlib import Path
from farm_art import BIN, COLOURS, put
from human_art import write_png


def frames():
    colours = dict(COLOURS, f=(110, 169, 178, 255), s=(180, 220, 216, 255),
                   e=(28, 45, 52, 255))
    result = []
    for count in range(4):
        rows = [list(row) for row in BIN]
        # Painted fish on the front of the bin identifies its contents empty.
        put(rows, 11, 5, "fssfe")
        for y in range(count):
            put(rows, 8 - y, 3, "fssfe.fssf")
        result.append([[colours.get(ch, (0, 0, 0, 0)) for ch in row] for row in rows])
    return result


if __name__ == "__main__":
    poses = frames()
    strip = [sum((pose[y] for pose in poses), []) for y in range(16)]
    write_png(Path(__file__).resolve().parents[1] / "assets/fish_bin.png", strip)
