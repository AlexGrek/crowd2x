---
name: pixel-artist
description: Drawing new pixel art for crowd2x and wiring it in - human paperdoll layers (hairstyles, hair colours, dresses, outfits, eyes) drawn from ASCII masks and palettes by tools/human_art.py, plus where every kind of art lives in assets/, how it is named, how it is listed in code (human.rs CLOTHES/HAIR/EYES, editor palettes, item art, dog atlases), the 16x16 / ART_SCALE rule, and how to preview and verify it. Use when asked to draw, add or recolour sprites, hair, clothes, dresses, outfits, characters, props, tiles or items, or to make the crowd look more varied.
---

# Pixel artist

All new art is **16x16 RGBA PNG at its true resolution**; the engine upscales it by
`ART_SCALE` (3). Never store art pre-upscaled, never draw at 48x48 (`renderer` skill has
why). Art is linked to code and map files **by name, never by index**.

## Where art lives and how it is wired

| Art | File | Listed in |
| --- | --- | --- |
| Human body | `assets/human/human_base.png` | `human::BASE` |
| Hair | `assets/human/hair_<style>_<colour>.png` | `human::HAIR` (generated) |
| Clothes | `assets/human/clothes_<style>_<colour>[_<trousers>].png` | `human::CLOTHES` (generated, plus the imported swimsuits) |
| Eyes | `assets/human/eyes_<colour>.png` | `human::EYES` |
| Floors / walls | `assets/<name>.png` | `src/editor/background.rs` palette, `PaletteItem::upscaled("name", "file.png")` |
| Props | `assets/<name>.png` | `src/editor/props.rs` palette; `animated(name, path, frames)` for a strip, `.used(path, frames)` for an in-use look |
| Held items | `assets/<name>.png` (any size) | `src/characters/item.rs` `art()` → `ItemArt::Texture` |
| Dog | `assets/dog_idle*.png`, 4 frames side by side | `src/characters/dog.rs` |

An animated strip is N frames of 16x16 side by side in one PNG (`64x16` for 4 frames).
A palette entry's *name* is what map files store — renaming one breaks saved maps.
`human_bluehair.png` and `human_nude.png` are unused imported composites; don't list them.

## Humans: draw with `tools/human_art.py`, not by hand

Every hair and outfit layer is generated from a mask and a palette, so a new style gets
every colour and a new colour gets every style:

```sh
uv run tools/human_art.py preview screenshots/human_preview.png   # look first, writes no assets
uv run tools/human_art.py generate > target/lists.rs              # write assets/human/*, print Rust lists
```

Then paste the printed `HAIR` and `CLOTHES` arrays into `src/characters/human.rs`
(keep the two `clothes_swimsuit_*` entries at the front of `CLOTHES` and bump its length).
Write the lists to a file inside the repo: on Windows, `uv run` Python cannot see bash's `/tmp`.

### Masks

One string per row, exactly 16 characters, `.` transparent. Rows past the end are empty;
an outfit mask starts with `""` placeholders for the rows above the torso.

- Hair: `o` outline (pure black, as the imported hair has), `d` / `m` / `l` dark, mid,
  light of the hair colour.
- Outfits: `o` the body's seam colour `(22,22,26)`, `d` / `m` / `l` the main cloth,
  `p` / `q` trousers mid and dark, `b` belt, `k` buckle.

### The body you are drawing over (`human_base.png`)

```
rows 0-2   empty - hair only
row  3     top of the head, cols 6-9 (hair covers it as a forehead)
rows 4-5   face, cols 6-9; the eyes are at cols 6 and 9 - NEVER cover these
row  6     chin, cols 6-9
row  7     shoulders/neck, cols 4-11
rows 8-10  torso cols 5-10; arms at cols 4 and 11 (row 9), hands at cols 3 and 12 (row 10)
rows 11-13 hips and legs, cols 5-10 (gap between the legs at cols 7-8)
row  14    feet, cols 6-9
row  15    ground shadow
```

Hair may go wider than the head (cols 2-13) and down over the shoulders (`very_long`
reaches row 10) because the hair layer draws over clothes. Keep a hole at cols 6-9 of
rows 4-6. A skirt can flare to cols 3-12 and close with an `o` hem row; covering row 14
hides the feet (fine for a gown).

### Palettes

`HAIR_COLOURS` and `CLOTH` are `(dark, mid, light)`; `TROUSERS` is `(mid, dark)`.
Keep about 25-40 brightness between steps, light on the left of a strand or fold.
Which combinations exist is data too: `NATURAL` colours get every hairstyle, `DYED`
names the few styles each fantasy colour comes in (that is the weighting — `Look::random`
picks uniformly from the list), `DRESSES` and `SEPARATES` pick the outfit colourways.

### Adding a layer kind that is not hair or clothes

A new slot (a hat, glasses) means a new array in `human.rs`, a new entry in
`slot_paths`, `SLOT_DEPTHS` and `SLOTS`, and the tests in that file — it changes the
paperdoll the body pool recycles, so read the `renderer` and `game-screen` skills first.

## Drawing anything else

There is no generator for props, tiles or items; write the PNG with a short script
(`tools/human_art.py` has a dependency-free `write_png(path, rows)`, and
`check_pixel_grid.read_png` reads one) or a pixel editor. Rules:

- 16x16 (or a strip of 16x16 frames). Opaque pixels only where the object is; a soft
  shadow is `(22,22,26)` at alpha 41 or 82, as the body has.
- A limited palette per sprite (3-5 shades a material) and a dark outline on the
  silhouette, matching the existing art.
- Top-down-ish front view, light from the upper left.

## Verify

1. **Look at it** before wiring it in: `preview` for humans; for other art, scale it up
   (whole-number factor, nearest neighbour) into `screenshots/` and Read the image. Crop
   and zoom further when the sheet is too small to judge.
2. `cargo test characters` — every listed human layer must exist and be 16x16
   (`character_art_is_drawn_at_the_art_resolution`); palette entries have the same check.
3. `uv run tools/art_scale.py report assets` — catches a file that is secretly an upscale.
4. In the game: `debugger` skill, e.g. a district (`CROWD2X_DISTRICT`) with a crowd, and a
   capture zoomed in.

## Gotchas

- `generate` overwrites every generated file; hand edits to a generated PNG are lost. Edit
  the mask or palette instead.
- Removing or renaming a hair or outfit changes which look a seed produces (`Look::random`
  indexes the arrays) — harmless, looks are not saved, but expect units to change clothes.
- `cargo` needs free disk space; `target/` is ~10 GB. If a write fails with "No space left
  on device", tell the user rather than deleting things.
