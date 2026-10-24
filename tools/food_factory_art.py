#!/usr/bin/env -S uv run --script
"""Draw 16px food production props and their four stock poses.

Uses the existing factory's shelving, pallet and hopper geometry. Wheat is
gold grain; meals are cream cartons with green seals. No baked-in upscale.
Run: uv run tools/food_factory_art.py
"""
from pathlib import Path

from human_art import write_png, uniform_layers
import steel_factory_art as steel

ROOT = Path(__file__).resolve().parent.parent / "assets"
COLOURS = {
    **steel.COLOURS,
    "Y": (126, 174, 130, 255), "U": (176, 216, 166, 255), "u": (72, 116, 80, 255),
    "X": (246, 232, 188, 255), "x": (88, 156, 94, 255), "z": (186, 166, 120, 255),
    "R": (218, 170, 62, 255), "y": (248, 214, 112, 255), "r": (160, 110, 38, 255),
    "G": (220, 226, 218, 255), "g": (148, 164, 154, 255),
}


def hopper_frames():
    frames = []
    for fill in (
        [], [(6, 3, "RyryRyryRy")],
        [(5, 2, "yRyryRyryR"), (6, 2, "RyryRyryRy")],
        [(3, 5, "yRRyRy"), (4, 3, "RyryRyryRy"), (5, 2, "yRyryRyryR"), (6, 2, "RyryRyryRy")],
    ):
        rows = [list(row) for row in steel.HOPPER]
        for y, x, text in fill:
            steel.put(rows, y, x, text)
        frames.append(steel.frame(rows))
    return frames


COOKER = (
    "................",
    "................",
    "..oooooooooooo..",
    "..oGGGGGGGGGgo..",
    "..oGkFkGkFkGgo..",
    "..oGGGGGGGGGgo..",
    ".oooooooooooooo.",
    ".oGGGGGGGGGGGgo.",
    ".oGoooGGoooGGgo.",
    ".oGokkoGokkoGgo.",
    ".oGoXxoGoXxoGgo.",
    ".oGoooGGoooGGgo.",
    ".oggggggggggggo.",
    ".oooooooooooooo.",
    "..oo........oo..",
    ".HhhhhhhhhhhhhH.",
)


def write_sheet(name, frames):
    pixels = [[COLOURS[c] if c != "." else (0, 0, 0, 0)
               for frame in frames for c in frame[y]] for y in range(16)]
    write_png(str(ROOT / name), pixels)


if __name__ == "__main__":
    write_sheet("food_cooker.png", [steel.frame(COOKER)])
    working = []
    for indicator in ("Y", "U"):
        rows = [list(row) for row in COOKER]
        steel.put(rows, 4, 4, f"{indicator}F{indicator}G{indicator}F{indicator}")
        working.append(steel.frame(rows))
    write_sheet("food_cooker_working.png", working)
    write_sheet("wheat_hopper.png", hopper_frames())
    write_sheet("meal_pallet.png", steel.pallet_frames())
    write_sheet("meal_rack.png", steel.rack_frames())
    for name, pixels in uniform_layers():
        if name in ("clothes_uniform_food_factory_worker.png", "clothes_uniform_food_courier.png"):
            write_png(str(ROOT / "human" / name), pixels)
