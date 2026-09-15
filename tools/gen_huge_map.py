"""Writes qa/fixtures/huge.json: a large map for load testing.

A grid of rooms separated by brown walls, each wall with a doorway, and a
fridge in every room - so a hungry crowd has to path through doors across a
map far bigger than any hand-made one.

    uv run tools/gen_huge_map.py [size] [room]
"""
import json
import sys

SIZE = int(sys.argv[1]) if len(sys.argv) > 1 else 256
ROOM = int(sys.argv[2]) if len(sys.argv) > 2 else 16
CELL_PX = 48
FLOOR, WALL = 1, 2

grid = [[FLOOR] * SIZE for _ in range(SIZE)]
for y in range(SIZE):
    for x in range(SIZE):
        edge = x in (0, SIZE - 1) or y in (0, SIZE - 1)
        on_wall = x % ROOM == 0 or y % ROOM == 0
        door = (x % ROOM == 0 and y % ROOM in (ROOM // 2, ROOM // 2 + 1)) or (
            y % ROOM == 0 and x % ROOM in (ROOM // 2, ROOM // 2 + 1)
        )
        if edge or (on_wall and not door):
            grid[y][x] = WALL

props = []
for ry in range(0, SIZE - ROOM + 1, ROOM):
    for rx in range(0, SIZE - ROOM + 1, ROOM):
        cx, cy = rx + 2, ry + 2
        props.append({"at": {"x": cx * CELL_PX + CELL_PX // 2, "y": cy * CELL_PX + CELL_PX // 2}, "kind": "fridge"})

doc = {
    "version": 1,
    "size": {"width": SIZE, "height": SIZE},
    "terrain": {
        "palette": ["void", "floor", "wall brown"],
        "layers": [{"rows": [",".join(map(str, row)) for row in grid]}],
    },
    "objects": {"props": props, "spawners": []},
}
with open("qa/fixtures/huge.json", "w") as f:
    json.dump(doc, f, indent=2)
print(f"qa/fixtures/huge.json: {SIZE}x{SIZE}, {len(props)} fridges")
