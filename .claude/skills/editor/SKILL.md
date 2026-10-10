---
name: editor
description: The crowd2x map editor - CurrentMap editing, the TileWindow and PropWindow map windows (also used by the game) and their touch() redraw rule, terrain painting and rectangle instruments (walls, blocks), free prop and lamp placement, animated prop strips, palettes linked to the map by name, the Cursor, saving. Use when changing the editor, its palettes, how terrain or props are drawn on either screen, or adding new placeable content. For the layer table, grid layers and x-ray overlay read map-layers too.
---

# The map editor

The editor edits the open `CurrentMap`, not the screen: a click writes the tile or prop
into the map **and nothing else**. What is on screen is drawn *from* the map, a canvas at a
time, by two windows that follow the camera — `background::TileWindow` for terrain,
`props::PropWindow` for props and lamps — so what is drawn cannot be something the saved file does
not contain. Both run on the game screen too, which is why they are registered here, where
the palettes live. Leaving writes the map back (`F5` saves too, and so does closing the
window — that path never runs `OnExit`). Painting outside the map's dimensions is refused
rather than growing it.

**A window redraws when it is told to, never on `CurrentMap`'s change flag.** An edit that
changed the map calls the window's `touch()`; loading a map needs no signal, because every
load is a change of screen and `reset_map_windows` empties both windows on the way into
either one. The change flag cannot be trusted to mean anything: the brush borrows the map
mutably on every frame it is held, whether or not a cell changed, and a prop window that
listened to it respawned every prop on screen — restarting every animation — 181 times in
a second and a half of dragging. `qa/place_props.json` covers placing and erasing.

**The layers are a table, `editor::LAYERS`**: each row a name, a `Target` — the terrain,
an `ObjectLayer` or a `GridLayer` — and the palette it paints from. `Tool` holds the active
row and a palette index per row; `tab`/`Y` steps through them and `1`-`6` picks one — floor,
props, lamps, ceiling, power, water. A new layer of an existing kind is a row there (and,
for a grid layer, a `GridLayer` variant, below); a new *kind* is a `Target` variant and a
match arm in `edit`. The kinds exist to be different, and new placeable content should
respect the split:

- `editor/background.rs` — one tile per grid cell at a single depth behind everything,
  drawn only for the cells under the canvas: `TileWindow` re-points the sprites of the row
  or column that left the view at the one that arrived, and keeps a free list bounded by
  the view (`pool::park_budget`) for the rest. Drag to paint.
  It is the drawn face of the map's terrain layer. `"block"` and anything named
  `"wall..."` are instruments rather than plain tiles (`Instrument`, in `editor/mod.rs`):
  a drag is a rectangle from corner to corner, held as a ghost preview and committed only
  on release — hollow (the four sides) for a wall, filled for a block. A single click is
  just a one-cell rectangle, so it paints exactly the one tile a plain drag would, which
  is what keeps a script that clicks once still passing.
- `editor/props.rs` — free placement, depth from `characters::depth_for`, so props
  interleave with characters. One click, one prop; erase hits the nearest centre. These
  are the map's `Props` object layer, positioned in whole *pixels* rather than cells.
  `PropWindow` draws the ones whose cell is under the canvas; it culls and does **not**
  pool, since a prop's components depend on its art and recycling one would be an
  archetype move, and props are few (the header of `props.rs` says when to revisit).
  **A prop blocks the cell its centre is in** (below); a new palette entry needs a
  matching `map::PROPS` entry, and two tests fail if the lists stop lining up.
  **A prop's art may be a strip** — `PaletteItem::animated(name, path, frames)`, frames of
  16x16 side by side in one PNG, stepped by `StripAnimation` (`Sprite::rect`, not a
  texture atlas: prop art is drawn by the window and by the cursor's ghost, which swaps
  picture whenever the palette moves, and a rect needs nothing but the image).
  `.used(path, frames)` adds a
  **second** strip for what it looks like while somebody is using it, which only the game
  screen ever shows (`game/props.rs`).
  `PaletteItem::posed(name, path, frames)` is a strip the game picks the frame of (no
  `StripAnimation`): the doors, whose `DoorLeaf` is turned and mirrored to fit its wall at
  spawn (`DoorLeaf::pose`) and set by `game::props::swing_doors` — the `doors` skill.
- **lamps** (`props::LAMP_PALETTE`, the map's `Lamps` object layer) — placed and erased
  exactly like props, drawn by the same `PropWindow`, but at a fixed `LAMP_Z` above every
  character, since a lamp hangs from the ceiling, and **blocking nothing** (`map::PROPS`
  is not asked). A lamp exists to light: each one's `EMITTERS` entry is a test. Erasing on
  one layer never takes the other's object, however near.
- `editor/grids.rs` — the **grid layers**: a value per cell that is not terrain. The
  ceiling is a rectangle instrument, always (left drag roofs it, right drag opens it to the
  sky); power (`"wiring"`, `"power line"`, `"distribution box"`) and water (`"sewer pipe"`)
  are a brush that paints **every cell between this frame's and the last one's** (`stroke`),
  since a wire with a gap in it powers nothing. Palette entry `i` paints value `i + 1`.
  None of them is part of the world anybody walks in, so each is drawn as an **x-ray
  overlay** (`GridOverlay`): one sprite per layer, a texture painted on the CPU at a texel a
  sixteenth of a cell and drawn at `ART_SCALE`, repainted when the view crosses a cell or
  the map is edited (`touch()`, never the change flag). Wiring, lines and pipes are bright
  where live and dim where not, and everything that needs a network has a green or red
  light in its corner on that layer. **Every layer can be seen, on both screens**: the
  layer being edited always, and `o` (or a click of the right stick; the game's `x-ray`
  button) steps `LayerView` through off, each grid layer, and all of them. The editor's HUD
  and the game's stats say how much is connected (`power 12/14  water 3/3`).

**Art and data are linked by name, never by index.** A palette entry is called after the
terrain it paints (`"wall brown"`) or the prop it places (`"bed 1"`), and that name is
what a map file stores — so the palette can be reordered without turning every saved bed
into a toilet. Two tests fail if the two lists stop lining up, and one more
(`every_palette_item_is_one_cell_wide_once_scaled`) fails if a `PaletteItem::new` should
have been `PaletteItem::upscaled`.

Painting is a pointing task, so it goes through a `Cursor` resource either device can
move: the mouse while it is moving, the left stick when it stops. Everything else has a
button on both.
