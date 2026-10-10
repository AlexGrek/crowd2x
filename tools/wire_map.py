#!/usr/bin/env -S uv run --script
"""Wire up a map: power to everything that needs it, a drain to every toilet.

A fridge, a computer or a lamp with no power, and a toilet with no drain, do
not work (`src/map/utilities.rs`), so a map made before the networks existed
— or a test fixture whose wiring is beside the point — has to be connected
to be played. This does it the shortest way, the same one
`map::utilities::serve_everything` takes in Rust: a transformer with a
distribution box under it and a sewer, each on a spare impassable cell (a
wall or void with nothing standing in it, so no walkable cell is taken), then
wiring and pipes run from them to each thing in an L, along the row first.

    uv run tools/wire_map.py qa/fixtures/office.json      # a map file
    uv run tools/wire_map.py qa/eating.json               # every map inside a QA script

Either is edited as text — the transformer and sewer put at the head of the
props list, the power and water rows before the objects — so the file's own
layout and line endings are left alone, and a map with no props list is
skipped. A map that already has a transformer or a sewer is left alone too,
so running it twice is harmless.
"""

import json
import sys

# The impassable tiles of `map::TERRAIN`.
BLOCKED = {"void", "wall brown", "wall brown big", "wall purple", "wall red", "block"}
POWERED = {"fridge", "computer", "ceiling lamp", "tube lamp"}
DRAINED = {"toilet"}
SOURCES = {"transformer", "sewer"}
CELL = 48


def cell_of(obj):
    return (obj["at"]["x"] // CELL, obj["at"]["y"] // CELL)


def centre(cell):
    return {"x": cell[0] * CELL + CELL // 2, "y": cell[1] * CELL + CELL // 2}


def l_path(a, b):
    (x0, y0), (x1, y1) = a, b
    dx = (x1 > x0) - (x1 < x0)
    dy = (y1 > y0) - (y1 < y0)
    for i in range(abs(x1 - x0) + 1):
        yield (x0 + i * dx, y0)
    for i in range(1, abs(y1 - y0) + 1):
        yield (x1, y0 + i * dy)


def plan(doc):
    """What to add to one map document: (props to add, power rows, water rows),
    or None when there is nothing to do."""
    objects = doc["objects"]
    everything = [o for layer in objects.values() for o in layer]
    if any(o["kind"] in SOURCES for o in everything):
        return None
    consumers = [o for name in ("props", "lamps") for o in objects.get(name, [])]
    powered = [cell_of(o) for o in consumers if o["kind"] in POWERED]
    drained = [cell_of(o) for o in consumers if o["kind"] in DRAINED]
    if not powered and not drained:
        return None

    w, h = doc["size"]["width"], doc["size"]["height"]
    palette = doc["terrain"]["palette"]
    rows = doc["terrain"]["layers"][0]["rows"]
    tile = lambda x, y: palette[int(rows[y].split(",")[x])]
    taken = {cell_of(o) for o in objects.get("props", [])}
    spare = [(x, y) for y in range(h) for x in range(w) if tile(x, y) in BLOCKED and (x, y) not in taken]
    if len(spare) < 2:
        sys.exit(f"no two spare impassable cells for a transformer and a sewer in a {w}x{h} map")
    transformer, sewer = spare[0], spare[-1]

    def lay(source, start_value, value, targets):
        grid = [["."] * w for _ in range(h)]
        grid[source[1]][source[0]] = start_value
        for target in targets:
            for x, y in l_path(source, target):
                if grid[y][x] == ".":
                    grid[y][x] = value
        return ["".join(row) for row in grid]

    props, grids = [], {}
    if powered:
        props.append({"at": centre(transformer), "kind": "transformer"})
        grids["power"] = lay(transformer, "B", "-", powered)
    if drained:
        props.append({"at": centre(sewer), "kind": "sewer"})
        grids["water"] = lay(sewer, "o", "o", drained)
    return props, grids


def splice(text, map_at, doc):
    """Wire the map document `doc` whose text starts at `map_at` in `text`:
    its grid rows put before its `"objects"`, its sources at the head of its
    props list, in the indentation the text already uses. Returns the new
    text, or None when there was nothing to do."""
    added = plan(doc)
    if added is None:
        return None
    props, grids = added
    at = text.index('"objects"', map_at)
    entry = lambda p: f'{{ "at": {{ "x": {p["at"]["x"]}, "y": {p["at"]["y"]} }}, "kind": "{p["kind"]}" }}'
    if "\n" not in text[:at]:
        # A file on one line stays on one line.
        block = "".join(f'"{key}": [' + ", ".join(f'"{row}"' for row in rows) + "], " for key, rows in grids.items())
        text = text[:at] + block + text[at:]
        bracket = text.index("[", text.index('"props"', at + len(block))) + 1
        empty = text[bracket:].lstrip().startswith("]")
        return text[:bracket] + ", ".join(entry(p) for p in props) + ("" if empty else ", ") + text[bracket:]
    line_start = text.rindex("\n", 0, at) + 1
    indent = text[line_start:at]
    inner = indent + "  "
    block = "".join(
        f'"{key}": [\n' + ",\n".join(f'{inner}"{row}"' for row in rows) + f"\n{indent}],\n{indent}"
        for key, rows in grids.items()
    )
    text = text[:at] + block + text[at:]
    at += len(block)
    bracket = text.index("[", text.index('"props"', at)) + 1
    empty = text[bracket:].lstrip().startswith("]")
    entries = ",\n".join(
        f'{inner}  {{ "at": {{ "x": {p["at"]["x"]}, "y": {p["at"]["y"]} }}, "kind": "{p["kind"]}" }}' for p in props
    )
    return text[:bracket] + "\n" + entries + ("\n" + inner if empty else ",") + text[bracket:]


def wire(path):
    with open(path, encoding="utf-8", newline="") as f:
        raw = f.read()
    crlf = "\r\n" in raw
    text = raw.replace("\r\n", "\n")
    doc = json.loads(text)
    changed = False
    if "steps" in doc:
        # Every inline map of a QA script, in the order they are listed.
        search_from = 0
        for given in doc.get("given", {}).get("maps", []):
            if not isinstance(given, dict) or "map" not in given:
                continue
            map_at = text.index('"map"', search_from)
            wired = splice(text, map_at, given["map"])
            if wired is not None:
                text, changed = wired, True
            search_from = text.index('"objects"', map_at) + 1
    elif "props" in doc.get("objects", {}):
        wired = splice(text, 0, doc)
        if wired is not None:
            text, changed = wired, True
    if changed:
        json.loads(text)  # still JSON
        with open(path, "w", encoding="utf-8", newline="") as f:
            f.write(text.replace("\n", "\r\n") if crlf else text)
    return changed


def main():
    for path in sys.argv[1:]:
        changed = wire(path)
        print(f"{path}: {'wired' if changed else 'nothing to do'}")


if __name__ == "__main__":
    main()
