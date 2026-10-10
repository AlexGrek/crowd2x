---
name: renderer
description: The crowd2x pixel-perfect render pipeline and how characters, props and items are drawn - the world camera and off-screen canvas, the integer upscale, PixelZoom, camera snapping, the lit canvas material, WORLD_LAYER, the 16x16 art rule and ART_SCALE, depth_for painter's order, the human paperdoll, dog atlases, item art (textures and emoji) and centring_nudge. Use when touching src/render.rs, window setup in main.rs, src/characters/, src/animation.rs, a sprite Transform, art files, zoom, or anything that could land off the pixel grid.
---

# Rendering and characters

Everything is drawn twice to get exact-integer pixel scaling:

1. A **world camera** (`RenderLayers` 0, `WORLD_LAYER`) renders the scene into an
   off-screen `Image` sized `window_physical / zoom`, at 1 world unit = 1 canvas pixel.
2. An **upscale camera** (`RenderLayers` 1) draws that image to the window as a single
   sprite scaled by `zoom`, nearest-neighbor sampled.

**Zooming is that factor** (`PixelZoom`, 1 to 8, `PIXEL_SCALE` = 4 by default), not a
camera scale: a fractional zoom would give neighbouring texels different widths, which is
the one thing this pipeline exists to prevent. Changing it rebuilds the canvas at the new
resolution exactly as a window resize does, so zooming in shows less world at a larger
size and every sprite stays on whole pixel blocks at either end. `UiScale` stays at
`PIXEL_SCALE`, so the interface keeps its size on screen however far the world is zoomed.

What keeps this exact, and easy to break:

- `ImagePlugin::default_nearest()` in `main.rs` — no texture filtering anywhere.
- `with_scale_factor_override(1.0)` on the window — makes logical/physical pixels
  identical so the upscale factor is exactly the zoom on HiDPI too.
- The world camera's `Transform` is snapped to whole pixels every frame; `CameraPan`
  holds the sub-pixel position for smooth movement. `CameraTarget` is how a screen asks
  for a position before the camera exists — `OnEnter` for the *initial* state runs before
  every `Startup` system.
- The canvas is rebuilt on window resize *and* on a zoom change, so the factor never
  drifts into a stretch.
- The canvas quad is nudged half a pixel when the canvas was rounded up by an odd number
  of screen pixels (`quad_offset`); without it the quad's edge lands between two screen
  pixels and every texel straddles two of them. A window the display cannot honour
  (999x601 arrives as 1000x602) fails the grid check for a reason that is not this —
  ask for even sizes.

**The canvas quad is a lit material, not a sprite** (`CanvasMaterial`,
`assets/shaders/lit_canvas.wgsl`). It fetches each canvas texel with `textureLoad` and
multiplies it by the light at *that texel's* world position — the lightmap sampled
bilinearly, `mix(indoor, ambient, sky) + lamps` — so the light is smooth across texels and still one colour within one,
and the upscale stays exact (checked at 3x and 6x). What lights it is `CanvasLight`, a
resource `src/lighting/` writes on the game screen; its default is unlit, which is every
other screen, drawn exactly as rendered. `light_canvas` runs after the camera is snapped,
since the shader derives world positions from it.

**Anything new that should render must carry `WORLD_LAYER`**, or it won't appear.
Verify with `tools/check_pixel_grid.py` after touching this file, window setup in
`main.rs`, or any sprite `Transform` that could land on a fractional coordinate.


## Characters (`src/characters/`)

- `CELL = 48` — every character occupies a 48x48 cell of the canvas, but the art is
  drawn at `ART = 16` and upscaled by `ART_SCALE` (3) at spawn time.
- **Sprites are never stored pre-upscaled.** A PNG holds the picture at its true
  resolution and the game applies the whole-number scale; baking the scale into the file
  takes that choice away and hides which art is genuinely detailed. Scale X and Y only
  (`characters::upscale`) — Z carries painter's depth. `tools/art_scale.py report assets`
  finds files that are secretly an upscale, `shrink` reduces one (refusing if lossy), and
  three tests fail if character art or a palette entry stops matching its declared size.
- **16x16 is the resolution everything is meant to be at**, and the engine does the
  upscaling. The palette is nearly there: of the tiles only `floor` and `floor diagonal`
  are not, and of the props only the beds, the toilet, the trash can and the fire. Those
  are the imported art that is *genuinely* drawn at 48x48 — `tools/art_scale.py` refuses
  to reduce them because it would be lossy, and it visibly is: the beds lose their frames
  and the toilet stops being recognisable. They are art debt to redraw at 16x16, not a
  second supported resolution, and until then a palette entry has to declare which it is.
  New art is always 16x16 and always `PaletteItem::upscaled`.
- `depth_for(y)` gives painter's-order depth from world Y (lower on screen = drawn in
  front); per-character layer offsets are small enough that one character's layers can
  never interleave with another's.
- **This module spawns nothing of its own.** Who exists is `src/sim/`'s answer, and
  `game/actors.rs` is what turns it into sprites. It used to scatter a demo crowd on
  `Startup`, which left 28 characters off the edge of every map, in every screen, for the
  life of the process.
- **Humans** (`human.rs`): a layered paperdoll — a base body with eyes, clothes, hair as
  child sprites, each a separate PNG in `assets/human/`. Add a look by dropping a 16x16
  PNG in `assets/human/` and adding its path to the matching array (`CLOTHES`, `EYES`,
  `HAIR`); `Look::random` and `spawn` handle the rest. The scale lives on the root, so
  the layers cannot drift apart.
- **Dogs** (`dog.rs`): a 4-frame idle atlas of 16x16 frames — the atlas grid is in source
  texels, not screen pixels (`FrameAnimation`, `src/animation.rs`), with a
  *separate* mirrored sheet for left-facing rather than `flip_x`, so left-facing art can
  be hand-tuned independently (`dog::apply_facing` swaps `sprite.image` on `Changed<Facing>`).
- **Items** (`item.rs`): every `ItemKind` has art — a pixel-art texture or an emoji
  (`ItemArt`) — and `item::art` is a `match` with no wildcard arm, so a new item does not
  compile until it says which. It lives here and not on `ItemKind` because `sim/` does not
  know what anything looks like. A **texture can be any size** and is drawn at `ART_SCALE`
  like every other sprite, so its pixels match its carrier's (which also makes the file's
  size the item's: a 16x16 one is as big as a person). What makes any size safe is
  `centring_nudge`: a sprite is centred on its position, so one an odd number of canvas
  pixels wide has its edges between two pixels and its texels come out uneven — measured, a
  5-texel texture at zoom 4 drew its texels 12/12/16/8/12 screen pixels wide instead of 12 —
  and an odd axis is moved half a pixel. Its size is only known once it has loaded, so
  `game::held` applies it then. An emoji is one codepoint, drawn as `Text2d` in the bundled
  Twemoji font, as the goal label over a unit's head is. No item is drawn from a texture
  yet: food and water are emoji.
