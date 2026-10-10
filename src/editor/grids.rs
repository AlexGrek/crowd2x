//! The grid layers — the ceiling over the map, and the wiring and the pipes
//! under it — and the x-ray that shows them.
//!
//! None of them is in the world a unit sees: the ceiling is looked down
//! through, and the networks are under the floor. So every one of them is
//! drawn as an **overlay**, over everything else on the canvas, and only when
//! asked for: the layer being edited always, and any of them on either screen
//! through [`LayerView`] — `o`, or clicking a gamepad's right stick, steps through none,
//! each layer, and all of them.
//!
//! An overlay is **one sprite per layer**, a texture painted on the CPU from
//! the map, a texel per sixteenth of a cell and drawn at [`ART_SCALE`] like
//! every other piece of art — so a wire is two texels wide on the same grid
//! as everything it runs under, and the whole layer is one draw however much
//! of it there is. It is repainted when the view crosses a cell or the map is
//! edited, never on `CurrentMap`'s change flag (see `PropWindow::touch`).
//!
//! What the networks *reach* is painted too, since that is what a player
//! needs to see: wiring, a line or a pipe is bright where it is live and dim
//! where it is not ([`Supply`]), and everything that needs one of them has a
//! light in its corner on that layer — green when it has what it needs, red
//! when it does not.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::{CurrentMap, PaletteItem, Target, Tool};
use crate::characters::{upscale, ART, ART_SCALE};
use crate::map::grid::{power, water};
use crate::map::utilities::{needs, Network, Supply, Utility};
use crate::map::{GridLayer, Map, ObjectLayer, Point};
use crate::render::WORLD_LAYER;
use crate::state::AppState;
use crate::view::{CellRect, VisibleArea};

/// The ceiling has one thing to paint: a roof. Erasing is opening it.
pub const CEILING_PALETTE: &[PaletteItem] = &[PaletteItem::upscaled("ceiling", "ceiling.png")];

/// In [`power`] value order, from 1: entry `i` paints value `i + 1`.
pub const POWER_PALETTE: &[PaletteItem] = &[
    PaletteItem::upscaled("wiring", "wiring.png"),
    PaletteItem::upscaled("power line", "power_line.png"),
    PaletteItem::upscaled("distribution box", "distribution_box.png"),
];

/// In [`water`] value order, from 1.
pub const WATER_PALETTE: &[PaletteItem] = &[PaletteItem::upscaled("sewer pipe", "sewer_pipe.png")];

/// The palette a grid layer is painted from.
pub const fn palette_of(layer: GridLayer) -> &'static [PaletteItem] {
    match layer {
        GridLayer::Ceiling => CEILING_PALETTE,
        GridLayer::Power => POWER_PALETTE,
        GridLayer::Water => WATER_PALETTE,
    }
}

/// The value palette entry `item` paints.
pub fn value_of(item: usize) -> u8 {
    item as u8 + 1
}

/// The ceiling is above every tile and prop, under the lamps hanging below
/// it and the cursor.
const CEILING_Z: f32 = 40.0;
/// What is under the floor is drawn over everything in the world — it is an
/// x-ray — lamps included, and under the editor's cursor and rectangle
/// preview.
const UNDERGROUND_Z: f32 = 60.0;

/// Texels to a cell side: the art grid an overlay is painted on.
const TEXELS: i32 = ART as i32;

/// Which overlays are on, whatever is being edited.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct LayerView {
    /// 0 is none, `1..=COUNT` one [`GridLayer`] each, past that all of them.
    mode: usize,
}

impl LayerView {
    const MODES: usize = GridLayer::COUNT + 2;

    pub fn cycle(&mut self) {
        self.mode = (self.mode + 1) % Self::MODES;
    }

    pub fn shows(&self, layer: GridLayer) -> bool {
        self.mode == GridLayer::COUNT + 1 || self.mode == layer as usize + 1
    }

    /// What the readouts call it: `off`, a layer's name, or `all`.
    pub fn label(&self) -> &'static str {
        match self.mode {
            0 => "off",
            mode if mode <= GridLayer::COUNT => GridLayer::ALL[mode - 1].name(),
            _ => "all",
        }
    }

    /// Show exactly this — `off`, a layer's name or `all` — for a QA script.
    pub fn set(&mut self, label: &str) -> bool {
        match (0..Self::MODES).find(|&mode| LayerView { mode }.label() == label) {
            Some(mode) => {
                self.mode = mode;
                true
            }
            None => false,
        }
    }
}

/// `o` or a click of the right stick steps the x-ray along, on either screen.
pub fn cycle_view(keys: Res<ButtonInput<KeyCode>>, gamepads: Query<&Gamepad>, mut view: ResMut<LayerView>) {
    let asked = keys.just_pressed(KeyCode::KeyO) || gamepads.iter().any(|pad| pad.just_pressed(GamepadButton::RightThumb));
    if asked {
        view.cycle();
    }
}

/// The sprites drawing the grid layers, and what the networks reach.
#[derive(Resource, Default)]
pub struct GridOverlay {
    drawn: [Option<Entity>; GridLayer::COUNT],
    shown: [bool; GridLayer::COUNT],
    rect: CellRect,
    /// Set by an edit, so the next run repaints and floods the networks
    /// again.
    dirty: bool,
    supply: Option<Supply>,
    /// On the game screen, the simulation's supply this was painted from —
    /// a box switched there is a repaint here.
    sim_generation: Option<u64>,
    summary: String,
}

impl GridOverlay {
    /// Say a grid layer — or anything the networks connect, a fridge placed
    /// or erased — changed.
    pub fn touch(&mut self) {
        self.dirty = true;
    }

    /// `power 12/14  water 3/3`: how many of the things that need each
    /// utility have it. Empty until the map has been looked at once.
    pub fn summary(&self) -> &str {
        &self.summary
    }
}

/// Paint one cell of a grid layer. Off the map is refused, like painting
/// terrain.
pub fn set(overlay: &mut GridOverlay, map: &mut Map, layer: GridLayer, cell: IVec2, value: u8) {
    let point = Point::new(cell.x, cell.y);
    if map.grid(layer, point) != value && map.set_grid(layer, point, value) {
        overlay.touch();
    }
}

/// Paint a whole stroke from `from` to `to`, every cell between them —
/// along the row, then the column. A brush otherwise paints only the cells
/// the cursor happened to be over on the frames it was sampled, and a gap in
/// a wire is a fridge with no power.
pub fn stroke(overlay: &mut GridOverlay, map: &mut Map, layer: GridLayer, from: IVec2, to: IVec2, value: u8) {
    for point in crate::map::utilities::l_path(Point::new(from.x, from.y), Point::new(to.x, to.y)) {
        set(overlay, map, layer, IVec2::new(point.x, point.y), value);
    }
}

/// Forget the sprites without despawning them — entering a screen, where the
/// ones from last time went with `DespawnOnExit` — and flood the networks
/// again, since a different map may have been opened.
pub fn reset(mut overlay: ResMut<GridOverlay>) {
    overlay.drawn = [None; GridLayer::COUNT];
    overlay.shown = [false; GridLayer::COUNT];
    overlay.rect = CellRect::EMPTY;
    overlay.dirty = true;
    overlay.sim_generation = None;
}

/// Keep each overlay that should be on painted over the canvas, and the
/// others gone.
#[allow(clippy::too_many_arguments)]
pub fn sync_grid_overlay(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    area: Res<VisibleArea>,
    current: Res<CurrentMap>,
    view: Res<LayerView>,
    tool: Res<Tool>,
    state: Res<State<AppState>>,
    sim: Option<Res<crate::game::actors::Sim>>,
    mut overlay: ResMut<GridOverlay>,
) {
    if !area.ready {
        return;
    }
    // In the game, what is live is the simulation's to say — a box switched
    // off there is dead wiring here — and its map is the one on screen.
    let live = sim.as_deref().filter(|_| *state.get() == AppState::Game).map(|sim| &sim.0);
    if let Some(world) = live
        && overlay.sim_generation != Some(world.supply_generation())
    {
        overlay.sim_generation = Some(world.supply_generation());
        overlay.dirty = true;
    }
    let state = *state.get();
    let editing = |layer: GridLayer| state == AppState::Editor && tool.layer().target() == Target::Grid(layer);
    let wanted = GridLayer::ALL.map(|layer| view.shows(layer) || editing(layer));

    let map = &current.map;
    // Clipped to the map: there is nothing to paint past its edge.
    let size = map.size();
    let rect = CellRect::new(
        area.tiles.min.max(IVec2::ZERO),
        area.tiles.max.min(IVec2::new(size.width - 1, size.height - 1)),
    );
    if rect == overlay.rect && !overlay.dirty && wanted == overlay.shown {
        return;
    }
    if overlay.dirty || overlay.supply.is_none() {
        let supply = match live {
            Some(world) => world.supply().clone(),
            None => Supply::from_map(map),
        };
        overlay.summary = summarise(map, &supply);
        overlay.supply = Some(supply);
    }
    overlay.dirty = false;
    overlay.rect = rect;
    overlay.shown = wanted;

    for layer in GridLayer::ALL {
        if let Some(entity) = overlay.drawn[layer as usize].take() {
            commands.entity(entity).despawn();
        }
        if !wanted[layer as usize] || rect.is_empty() {
            continue;
        }
        let supply = overlay.supply.as_ref().expect("flooded above");
        let (width, height, pixels) = paint(layer, map, supply, rect);
        let image = Image::new(
            Extent3d { width, height, depth_or_array_layers: 1 },
            TextureDimension::D2,
            pixels,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        // The middle of the rectangle, in world pixels: whole, since it is a
        // whole number of cells and a cell is an even number of pixels.
        let cells = (rect.max - rect.min + IVec2::ONE).as_vec2();
        let centre = (rect.min.as_vec2() + cells / 2.0) * crate::map::PIXELS_PER_CELL as f32;
        let z = if layer.is_underground() { UNDERGROUND_Z } else { CEILING_Z };
        let entity = commands
            .spawn((
                Name::new(layer.name()),
                Sprite::from_image(images.add(image)),
                Transform::from_xyz(centre.x, centre.y, z).with_scale(upscale(ART_SCALE)),
                WORLD_LAYER,
                DespawnOnExit(state),
            ))
            .id();
        overlay.drawn[layer as usize] = Some(entity);
    }
}

fn summarise(map: &Map, supply: &Supply) -> String {
    let mut counts = [(0, 0); 2];
    for (_, utility, served) in supply.consumers(map) {
        let count = &mut counts[utility as usize];
        count.0 += served as usize;
        count.1 += 1;
    }
    format!("power {}/{}  water {}/{}", counts[0].0, counts[0].1, counts[1].0, counts[1].1)
}

type Rgba = [u8; 4];

const ROOF: Rgba = [120, 160, 255, 45];
const ROOF_HATCH: Rgba = [170, 200, 255, 130];
const WIRING_LIVE: Rgba = [255, 220, 60, 255];
const WIRING_DEAD: Rgba = [110, 96, 60, 220];
const LINE_LIVE: Rgba = [255, 120, 30, 255];
const LINE_DEAD: Rgba = [110, 70, 45, 220];
const PIPE_LIVE: Rgba = [70, 160, 255, 255];
const PIPE_DEAD: Rgba = [60, 80, 110, 220];
const BOX_BODY: Rgba = [55, 58, 66, 255];
const BOX_EDGE: Rgba = [190, 195, 205, 255];
const SERVED: Rgba = [80, 220, 90, 255];
const UNSERVED: Rgba = [235, 55, 45, 255];
const MARKER_EDGE: Rgba = [20, 20, 24, 255];

/// A texture covering `rect`, a cell to [`TEXELS`] square texels, top row
/// first as an image is: its width, its height and its RGBA bytes.
///
/// Plain arithmetic over the map, so it can be tested without an `App`.
pub fn paint(layer: GridLayer, map: &Map, supply: &Supply, rect: CellRect) -> (u32, u32, Vec<u8>) {
    let cells = rect.max - rect.min + IVec2::ONE;
    let (width, height) = (cells.x * TEXELS, cells.y * TEXELS);
    let mut canvas = Canvas { width, height, pixels: vec![0; (width * height * 4) as usize] };

    for cell in rect.cells() {
        let point = Point::new(cell.x, cell.y);
        let origin = (cell - rect.min) * TEXELS;
        let value = map.grid(layer, point);
        match layer {
            GridLayer::Ceiling if value != 0 => {
                for ty in 0..TEXELS {
                    for tx in 0..TEXELS {
                        let colour = if (tx + ty) % 4 == 0 { ROOF_HATCH } else { ROOF };
                        canvas.put(origin.x + tx, origin.y + ty, colour);
                    }
                }
            }
            GridLayer::Power => paint_power(&mut canvas, map, supply, point, origin, value),
            GridLayer::Water if value == water::PIPE => {
                let colour = if supply.is_live(Network::Pipes, point) { PIPE_LIVE } else { PIPE_DEAD };
                let joins = |next: Point| map.grid(GridLayer::Water, next) == water::PIPE;
                cable(&mut canvas, point, origin, 4, colour, joins);
            }
            _ => {}
        }
    }

    // A light in the corner of everything this layer serves.
    let serves = match layer {
        GridLayer::Power => Some(Utility::Power),
        GridLayer::Water => Some(Utility::Water),
        GridLayer::Ceiling => None,
    };
    if let Some(utility) = serves {
        for object_layer in [ObjectLayer::Props, ObjectLayer::Lamps] {
            for object in map.objects(object_layer) {
                let cell = object.cell();
                if needs(object.kind.as_str()) != Some(utility) || !rect.contains(IVec2::new(cell.x, cell.y)) {
                    continue;
                }
                let origin = (IVec2::new(cell.x, cell.y) - rect.min) * TEXELS;
                let colour = if supply.serves(object.kind.as_str(), cell) { SERVED } else { UNSERVED };
                canvas.fill(origin.x + 11, origin.y + 11, 5, 5, MARKER_EDGE);
                canvas.fill(origin.x + 12, origin.y + 12, 3, 3, colour);
            }
        }
    }

    (width as u32, height as u32, canvas.pixels)
}

fn paint_power(canvas: &mut Canvas, map: &Map, supply: &Supply, point: Point, origin: IVec2, value: u8) {
    let at = |next: Point| map.grid(GridLayer::Power, next);
    match value {
        power::WIRING => {
            let colour = if supply.is_live(Network::Wiring, point) { WIRING_LIVE } else { WIRING_DEAD };
            cable(canvas, point, origin, 2, colour, |next| matches!(at(next), power::WIRING | power::BOX));
        }
        power::LINE => {
            let colour = if supply.is_live(Network::Line, point) { LINE_LIVE } else { LINE_DEAD };
            cable(canvas, point, origin, 4, colour, |next| matches!(at(next), power::LINE | power::BOX));
        }
        power::BOX => {
            // What comes into it, each at its own width, under the box.
            let line = if supply.is_live(Network::Line, point) { LINE_LIVE } else { LINE_DEAD };
            let wiring = if supply.is_live(Network::Wiring, point) { WIRING_LIVE } else { WIRING_DEAD };
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                match at(point.offset(dx, dy)) {
                    power::LINE | power::BOX => arm(canvas, origin, (dx, dy), 4, line),
                    power::WIRING => arm(canvas, origin, (dx, dy), 2, wiring),
                    _ => {}
                }
            }
            canvas.fill(origin.x + 3, origin.y + 3, 10, 10, BOX_EDGE);
            canvas.fill(origin.x + 4, origin.y + 4, 8, 8, BOX_BODY);
            // The switch: green when it is giving power out — the line
            // reaches it and it is switched on — and red when not.
            let live = supply.is_live(Network::Wiring, point);
            canvas.fill(origin.x + 6, origin.y + 6, 4, 4, if live { SERVED } else { UNSERVED });
        }
        _ => {}
    }
}

/// A cable `width` texels thick through the middle of a cell, with an arm
/// out to every neighbour it `joins`.
fn cable(canvas: &mut Canvas, point: Point, origin: IVec2, width: i32, colour: Rgba, joins: impl Fn(Point) -> bool) {
    let low = TEXELS / 2 - width / 2;
    canvas.fill(origin.x + low, origin.y + low, width, width, colour);
    for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        if joins(point.offset(dx, dy)) {
            arm(canvas, origin, (dx, dy), width, colour);
        }
    }
}

/// From the middle of a cell out to its edge, toward `(dx, dy)`.
fn arm(canvas: &mut Canvas, origin: IVec2, (dx, dy): (i32, i32), width: i32, colour: Rgba) {
    let low = TEXELS / 2 - width / 2;
    let half = TEXELS / 2;
    let (x, y, w, h) = match (dx, dy) {
        (1, _) => (half, low, half, width),
        (-1, _) => (0, low, half + width / 2, width),
        (_, 1) => (low, half, width, half),
        _ => (low, 0, width, half + width / 2),
    };
    canvas.fill(origin.x + x, origin.y + y, w, h, colour);
}

/// RGBA texels, addressed with y up like the world and stored top row first
/// like an image.
struct Canvas {
    width: i32,
    height: i32,
    pixels: Vec<u8>,
}

impl Canvas {
    fn put(&mut self, x: i32, y: i32, colour: Rgba) {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return;
        }
        let row = self.height - 1 - y;
        let at = ((row * self.width + x) * 4) as usize;
        self.pixels[at..at + 4].copy_from_slice(&colour);
    }

    fn fill(&mut self, x: i32, y: i32, w: i32, h: i32, colour: Rgba) {
        for ty in y..y + h {
            for tx in x..x + w {
                self.put(tx, ty, colour);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Size, FLOOR};

    fn texel(pixels: &[u8], width: u32, height: u32, x: i32, y: i32) -> Rgba {
        let row = height as i32 - 1 - y;
        let at = ((row * width as i32 + x) * 4) as usize;
        pixels[at..at + 4].try_into().unwrap()
    }

    /// Every entry of every grid palette paints a value its layer has.
    #[test]
    fn every_grid_palette_entry_paints_a_value_of_its_layer() {
        for layer in GridLayer::ALL {
            let palette = palette_of(layer);
            assert_eq!(palette.len(), layer.alphabet().len() - 1, "{layer:?}: one entry per value but nothing");
            for item in 0..palette.len() {
                assert!(layer.char_of(value_of(item)).is_some(), "{layer:?} {item}");
            }
        }
        assert_eq!(value_of(2), power::BOX, "the box is the third power entry");
    }

    #[test]
    fn live_wiring_is_bright_dead_wiring_dim_and_a_fridge_says_which_it_has() {
        let mut map = Map::new(Size::new(4, 1), FLOOR);
        crate::sim::testing::prop_at(&mut map, "fridge", Point::new(3, 0));
        for x in 1..4 {
            map.set_grid(GridLayer::Power, Point::new(x, 0), power::WIRING);
        }
        let rect = CellRect::new(IVec2::ZERO, IVec2::new(3, 0));

        let dead = Supply::from_map(&map);
        let (w, h, pixels) = paint(GridLayer::Power, &map, &dead, rect);
        assert_eq!((w, h), (64, 16));
        assert_eq!(texel(&pixels, w, h, 24, 8), WIRING_DEAD, "the middle of cell 1");
        assert_eq!(texel(&pixels, w, h, 3 * 16 + 13, 13), UNSERVED);
        assert_eq!(texel(&pixels, w, h, 2, 2), [0; 4], "nothing under cell 0");

        let live = Supply::everywhere(map.size());
        let (w, h, pixels) = paint(GridLayer::Power, &map, &live, rect);
        assert_eq!(texel(&pixels, w, h, 24, 8), WIRING_LIVE);
        assert_eq!(texel(&pixels, w, h, 31, 8), WIRING_LIVE, "an arm across to cell 2");
        assert_eq!(texel(&pixels, w, h, 3 * 16 + 13, 13), SERVED);
    }

    #[test]
    fn the_view_steps_through_none_each_layer_and_all() {
        let mut view = LayerView::default();
        let mut labels = Vec::new();
        for _ in 0..LayerView::MODES {
            labels.push(view.label());
            view.cycle();
        }
        assert_eq!(labels, ["off", "ceiling", "power", "water", "all"]);
        assert_eq!(view.label(), "off", "and round again");
        assert!(view.set("power") && view.shows(GridLayer::Power) && !view.shows(GridLayer::Water));
        assert!(view.set("all") && GridLayer::ALL.into_iter().all(|layer| view.shows(layer)));
        assert!(!view.set("gas"));
    }
}
