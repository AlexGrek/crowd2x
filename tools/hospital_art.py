"""Draw the hospital's 16x16 bed, at its true source resolution."""
from pathlib import Path
from human_art import paint, write_png

BED = [
    "................",
    "...ssssssssss...",
    "...swwwwwwwws...",
    "...swwwwwwwws...",
    "...slllllllls...",
    "...sbbbbbbbbs...",
    "...sbbbbbbbbs...",
    "...sbbbbbbbbs...",
    "...sbbbbbbbbs...",
    "...sbbbbbbbbs...",
    "...slllllllls...",
    "...swwwrrwwws...",
    "...swwrrrrwws...",
    "...swwwrrwwws...",
    "...ssssssssss...",
    "...dd......dd...",
]
COLOURS = {
    "s": (157, 180, 187), "w": (235, 245, 242),
    "l": (167, 224, 224), "b": (92, 177, 190),
    "r": (200, 51, 63), "d": (68, 88, 98),
}

if __name__ == "__main__":
    write_png(Path(__file__).resolve().parent.parent / "assets/hospital_bed.png", paint(BED, COLOURS))
