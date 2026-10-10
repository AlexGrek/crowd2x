//! The props layer: free-standing objects — and the lamps layer, which is
//! drawn the same way.
//!
//! A lamp is an object like a prop — free-placed in whole pixels, stored by
//! its palette name — but on the map's `Lamps` layer, from [`LAMP_PALETTE`],
//! and drawn at [`LAMP_Z`] rather than by depth: it hangs from the ceiling,
//! so everybody walks under it. What it is for is light
//! (`crate::lighting::scene::EMITTERS`, by the same name).
//!
//! A prop is built by a deliberately different sprite function from a
//! background tile. It is not bound to a cell, several can overlap, and its
//! depth comes from [`characters::depth_for`] — the same rule characters use.
//! That is the whole point of splitting the layers: a character walking below a
//! bed draws in front of it, and one standing above it draws behind, without
//! the editor having to know anything about the simulation.
//!
//! Props are the map's `Props` object layer, which is why an [`Object`] is
//! positioned in whole pixels rather than in cells: rounding a bed to the grid
//! would both move it and throw away the depth ordering that makes this layer
//! worth having. Like terrain, a prop is stored by the name of its palette
//! entry, so re-ordering the palette cannot turn every saved bed into a
//! toilet.

use std::collections::HashMap;

use bevy::prelude::*;

use super::{CurrentMap, PaletteItem};
use crate::animation::StripAnimation;
use crate::characters::{depth_for, upscale, CELL};
use crate::map::{Map, Object, ObjectKind, ObjectLayer, Point};
use crate::render::WORLD_LAYER;
use crate::state::AppState;
use crate::view::{CellRect, VisibleArea};

/// How close the cursor has to be to a prop's centre to delete it.
const ERASE_RADIUS: f32 = CELL as f32 / 2.0;

/// A lamp is above everything in the world — it is on the ceiling — and
/// under the editor's cursor and rectangle preview.
const LAMP_Z: f32 = 50.0;

/// How fast a prop's art cycles, for the ones that move. Fast enough that a
/// screen reads as flickering rather than as stepping through pictures.
const SECONDS_PER_FRAME: f32 = 0.12;

pub const PALETTE: &[PaletteItem] = &[
    PaletteItem::new("bed 1", "bed01.png"),
    PaletteItem::new("bed 2", "bed02.png"),
    PaletteItem::new("bed 3", "bed03.png"),
    PaletteItem::new("bed 4", "bed04.png"),
    PaletteItem::new("bed 5", "bed05.png"),
    PaletteItem::new("bed 6", "bed06.png"),
    PaletteItem::new("toilet", "toilet.png"),
    PaletteItem::new("trash can", "trash_can48.png"),
    PaletteItem::upscaled("pipe", "pipe_vertical.png"),
    PaletteItem::new("fire", "fire_static.png"),
    PaletteItem::upscaled("crate", "untitled.png"),
    PaletteItem::new("crate tall", "untitledtallhd.png"),
    // Something to eat from: a brain finds it by this name (`sim::feature`).
    PaletteItem::upscaled("fridge", "fridge.png"),
    // Something to do. Two strips: the screen dark and idling, and the screen
    // on for as long as somebody is sitting at it — `game::props` is what
    // swaps between them, from what the simulation says that unit is doing.
    PaletteItem::animated("computer", "computer_idle.png", 10).used("computer.png", 11),
    // Where the networks under the floor start (`map::utilities`): each is a
    // prop in the street *and* a node of its network in the same cell.
    PaletteItem::upscaled("transformer", "transformer.png"),
    PaletteItem::upscaled("sewer", "sewer.png"),
];

/// What can hang from the ceiling. Each one's light is its entry in
/// `lighting::scene::EMITTERS`, under the same name, and a test says so.
pub const LAMP_PALETTE: &[PaletteItem] = &[
    // Warm, for a room: a house's.
    PaletteItem::upscaled("ceiling lamp", "ceiling_lamp.png"),
    // Cool, in rows: an office's.
    PaletteItem::upscaled("tube lamp", "tube_lamp.png"),
];

/// The palette an object layer is placed from. Spawners have none — the
/// editor keeps the ones a map has and places no more.
pub fn palette_of(layer: ObjectLayer) -> &'static [PaletteItem] {
    match layer {
        ObjectLayer::Props => PALETTE,
        ObjectLayer::Lamps => LAMP_PALETTE,
        ObjectLayer::Spawners => &[],
    }
}

/// The object layers the window draws.
const DRAWN: [ObjectLayer; 2] = [ObjectLayer::Props, ObjectLayer::Lamps];

/// A placed prop, carrying the object it stands for so erasing it can take
/// that object out of the map without guessing which one it was.
#[derive(Component)]
pub struct Prop {
    layer: ObjectLayer,
    at: Point,
    kind: &'static str,
}

impl Prop {
    fn object(&self) -> Object {
        Object {
            at: self.at,
            kind: ObjectKind::new(self.kind),
        }
    }

    /// The palette entry it was placed from, which is the name a map file
    /// stores it under.
    pub fn kind(&self) -> &'static str {
        self.kind
    }

    /// The cell it blocks and is addressed by — the one its centre falls in,
    /// [`Object::cell`]'s rule, which is the same cell the simulation indexes
    /// it under (`sim::feature::Features`).
    pub fn cell(&self) -> Point {
        self.object().cell()
    }
}

/// A prop that looks different while somebody is using it: both strips, and
/// which of them is on screen.
///
/// Spawned by the editor with every other prop, because the two pictures are
/// one palette entry ([`PaletteItem::in_use`]) — but only the game screen ever
/// turns it on, since only there is anybody using anything. Holding the
/// handles rather than the paths means the swap is a clone and not an asset
/// lookup by string, on a component that is asked about every frame.
#[derive(Component)]
pub struct Usable {
    idle: Handle<Image>,
    idle_frames: u32,
    busy: Handle<Image>,
    busy_frames: u32,
    in_use: bool,
}

impl Usable {
    /// Show the strip for `in_use`, and say what changed: the image to put on
    /// the sprite and how many frames it has, or `None` when it is already
    /// the one showing.
    pub fn show(&mut self, in_use: bool) -> Option<(Handle<Image>, u32)> {
        if in_use == self.in_use {
            return None;
        }
        self.in_use = in_use;
        Some(if in_use {
            (self.busy.clone(), self.busy_frames)
        } else {
            (self.idle.clone(), self.idle_frames)
        })
    }

    pub fn is_in_use(&self) -> bool {
        self.in_use
    }
}

/// The palette entry that draws an object kind on a layer, or `None` for one
/// this build has no art for.
pub fn item_of(layer: ObjectLayer, kind: &ObjectKind) -> Option<usize> {
    palette_of(layer).iter().position(|item| item.name == kind.as_str())
}

/// Put a prop — or a lamp, by `layer` — in the map. What draws it is
/// [`sync_prop_window`], from the map, so this cannot show one the saved file
/// does not contain.
pub fn place(window: &mut PropWindow, map: &mut Map, layer: ObjectLayer, pos: Vec2, item: usize) {
    // Whole pixels only: a sprite on a fractional coordinate samples between
    // texels and puts a seam through the pixel grid. It is also what lets a
    // position be an exact key when the prop is erased again.
    let pos = pos.round();
    let at = Point::new(pos.x as i32, pos.y as i32);

    map.add_object(
        layer,
        Object {
            at,
            kind: ObjectKind::new(palette_of(layer)[item].name),
        },
    );
    window.touch();
}

/// Delete the prop nearest the cursor, if one is close enough.
///
/// Props have no uniform size — `crate tall` is 48x64 while the rest are 48x48
/// — so this matches on distance to the centre rather than pretending every
/// prop has the same bounding box.
pub fn erase_nearest(
    window: &mut PropWindow,
    map: &mut Map,
    layer: ObjectLayer,
    props: &Query<(Entity, &Prop, &Transform)>,
    pos: Vec2,
) {
    let nearest = props
        .iter()
        // Only the layer being edited: a lamp over a bed is erased with the
        // lamps layer and the bed with the props layer, whichever is nearer.
        .filter(|(_, prop, _)| prop.layer == layer)
        .map(|(entity, prop, transform)| {
            (
                entity,
                prop,
                transform.translation.truncate().distance_squared(pos),
            )
        })
        .filter(|(_, _, distance)| *distance <= ERASE_RADIUS * ERASE_RADIUS)
        .min_by(|a, b| a.2.total_cmp(&b.2));

    if let Some((_, prop, _)) = nearest {
        // One object per sprite, so a stack of props erases one at a time.
        // The sprite goes when the window is told the map changed, which is
        // the same route placing one takes.
        if map.remove_object(layer, &prop.object()) {
            window.touch();
        }
    }
}

/// The props on the canvas.
///
/// Culled to the view like everything else, but **not** pooled, and that is a
/// decision rather than an omission: a prop carries a `StripAnimation` only if
/// its art moves and a [`Usable`] only if it has a second strip, so recycling
/// one means inserting and removing components — an archetype move — where a
/// tile needs three writes. Props are also few. The largest fixture in the
/// repo, a generated 256x256 map, carries 256 of them against 65,536 tiles, so
/// the handful entering and leaving on a cell crossing are cheap to spawn
/// outright. Revisit when a map carries thousands; the prerequisite is a real
/// art-size field on [`PaletteItem`], since prop art has no uniform height
/// (`crate tall` is 48x64) and a pool would need an honest margin.
#[derive(Resource, Default)]
pub struct PropWindow {
    /// `(layer, index into it)`, and the sprite drawing it.
    drawn: HashMap<(usize, usize), Entity>,
    /// What `drawn` covers, so a camera that has not crossed a cell boundary
    /// does no work.
    rect: CellRect,
    /// Set when a prop was placed or erased, so the next run rebuilds.
    dirty: bool,
}

impl PropWindow {
    /// Forget everything without despawning it — the sprites carry
    /// `DespawnOnExit`, as [`background::TileWindow::clear`] explains.
    pub fn clear(&mut self) {
        self.drawn.clear();
        self.rect = CellRect::EMPTY;
        self.dirty = false;
    }

    /// Say the layer changed, as [`background::TileWindow::touch`] does and
    /// for the same reason: `CurrentMap`'s change flag cannot be trusted to
    /// mean it. The editor borrows the map mutably on every frame a brush is
    /// held, whether or not a cell changed, and a window that rebuilt on that
    /// flag respawned every prop on screen — restarting every animation — for
    /// the whole of a drag. A map *loaded* needs no flag at all: every load is
    /// a change of screen, and `reset_map_windows` empties the window on the
    /// way in.
    pub fn touch(&mut self) {
        self.dirty = true;
    }
}

/// Keep the drawn props equal to the ones whose cell is under the canvas.
///
/// Scanned straight out of the map's object layer rather than through an index
/// of its own: props are a `Vec` of a few hundred at most, and this runs only
/// when the view moved by a whole cell or a prop was placed or erased.
pub fn sync_prop_window(
    mut commands: Commands,
    assets: Res<AssetServer>,
    area: Res<VisibleArea>,
    current: Res<CurrentMap>,
    state: Res<State<AppState>>,
    mut window: ResMut<PropWindow>,
) {
    if !area.ready {
        return;
    }
    if area.tiles == window.rect && !window.dirty {
        return;
    }

    // Placing or erasing renumbers the layer, so an index is only meaningful
    // within one version of the map. Rather than track that, an edit rebuilds
    // the window outright — a few hundred objects, and only on the frame an
    // edit landed.
    if window.dirty {
        for (_, entity) in window.drawn.drain() {
            commands.entity(entity).despawn();
        }
        window.rect = CellRect::EMPTY;
        window.dirty = false;
    }

    let map = &current.map;
    let wanted = area.tiles;
    let on_canvas = |object: &Object| {
        let cell = object.cell();
        wanted.contains(IVec2::new(cell.x, cell.y))
    };

    window.drawn.retain(|&(layer, index), entity| {
        if map.objects(DRAWN[layer]).get(index).is_some_and(on_canvas) {
            return true;
        }
        commands.entity(*entity).despawn();
        false
    });

    for (slot, &layer) in DRAWN.iter().enumerate() {
        for (index, object) in map.objects(layer).iter().enumerate() {
            if !on_canvas(object) || window.drawn.contains_key(&(slot, index)) {
                continue;
            }
            // One this build has no art for is left in the map, so saving does
            // not delete a prop it merely has no picture for. Not warned here:
            // this runs every time the view moves, and a map with one unknown
            // prop would fill the log with it.
            if let Some(item) = item_of(layer, &object.kind) {
                let prop = spawn_prop(&mut commands, &assets, layer, object.at, item, *state.get());
                window.drawn.insert((slot, index), prop);
            }
        }
    }

    window.rect = wanted;
}

/// One prop: its sprite, whatever animation its art asks for, and — for a
/// prop whose picture says whether it is being used — the second strip the
/// game screen swaps in.
fn spawn_prop(
    commands: &mut Commands,
    assets: &AssetServer,
    layer: ObjectLayer,
    at: Point,
    item: usize,
    state: AppState,
) -> Entity {
    let item = &palette_of(layer)[item];
    let y = at.y as f32;
    let z = if layer == ObjectLayer::Lamps { LAMP_Z } else { depth_for(y) };
    let prop = commands
        .spawn((
            Name::new(if layer == ObjectLayer::Lamps { "lamp" } else { "prop" }),
            Prop {
                layer,
                at,
                kind: item.name,
            },
            Sprite {
                image: assets.load(item.art.path),
                // One frame of a strip; the whole PNG for a still picture.
                rect: item.first_frame(),
                ..default()
            },
            Transform::from_xyz(at.x as f32, y, z).with_scale(upscale(item.scale)),
            WORLD_LAYER,
            DespawnOnExit(state),
        ))
        .id();

    if item.art.frames > 1 {
        commands.entity(prop).insert(StripAnimation::new(
            item.frame(),
            item.art.frames,
            SECONDS_PER_FRAME,
        ));
    }
    if let Some(busy) = item.in_use {
        commands.entity(prop).insert(Usable {
            idle: assets.load(item.art.path),
            idle_frames: item.art.frames,
            busy: assets.load(busy.path),
            busy_frames: busy.frames,
            in_use: false,
        });
    }
    prop
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A prop is stored under its palette name, and found again by it. A name
    /// that did not round-trip would come back from a file undrawable.
    #[test]
    fn every_prop_is_stored_under_a_name_that_finds_it_again() {
        for layer in DRAWN {
            for (index, item) in palette_of(layer).iter().enumerate() {
                assert_eq!(item_of(layer, &ObjectKind::new(item.name)), Some(index));
            }
        }
        assert_eq!(item_of(ObjectLayer::Props, &ObjectKind::new("hat stand")), None);
        assert_eq!(item_of(ObjectLayer::Props, &ObjectKind::new("ceiling lamp")), None, "a lamp is not a prop");
    }

    /// A lamp that gave off no light would be a picture of a lamp.
    #[test]
    fn every_lamp_gives_off_light() {
        for item in LAMP_PALETTE {
            let emission = crate::lighting::scene::emission(item.name)
                .unwrap_or_else(|| panic!("{:?} has no entry in EMITTERS", item.name));
            assert!(emission.always_on, "{:?}: a lamp is never switched off by anybody", item.name);
        }
    }

    /// Names are unique across the palettes, since `Tool::select` and a QA
    /// script's `tool` step find an entry by name alone.
    #[test]
    fn no_two_palettes_share_a_name() {
        let mut names: Vec<&str> = PALETTE
            .iter()
            .chain(LAMP_PALETTE)
            .chain(super::super::background::PALETTE)
            .map(|item| item.name)
            .collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count);
    }

    /// A prop the simulation gives a use to has to be one the editor can place,
    /// or the use is unreachable from any map a person can make.
    #[test]
    fn every_feature_the_simulation_knows_is_a_prop_the_editor_can_place() {
        for feature in crate::sim::feature::FEATURES {
            assert!(
                item_of(ObjectLayer::Props, &ObjectKind::new(feature.name)).is_some(),
                "no palette entry for {:?}",
                feature.name
            );
        }
    }

    /// Every prop that can be placed says whether it can be walked through.
    /// One the catalogue does not know would still block — unknown props do —
    /// but a rug added to the palette alone would block by accident.
    #[test]
    fn every_palette_prop_is_in_the_map_s_prop_catalogue() {
        for item in PALETTE {
            assert!(
                ObjectKind::new(item.name).prop().is_some(),
                "{:?} is not in map::PROPS",
                item.name
            );
        }
    }

    /// ...and the other way: a catalogue entry nobody can place is a name that
    /// has drifted from the one the palette saves.
    #[test]
    fn every_catalogue_prop_can_be_placed() {
        for prop in crate::map::PROPS {
            assert!(
                item_of(ObjectLayer::Props, &ObjectKind::new(prop.name)).is_some(),
                "no palette entry for {:?}",
                prop.name
            );
        }
    }

    /// The two strips of a prop that shows whether it is in use are two
    /// pictures of the same thing, so they are drawn at the same size and
    /// swapping one for the other cannot resize the sprite.
    #[test]
    fn a_prop_that_lights_up_has_one_art_size_for_both_its_strips() {
        let computer = &PALETTE[item_of(ObjectLayer::Props, &ObjectKind::new("computer")).expect("in the palette")];
        let busy = computer.in_use.expect("a computer has a screen-on strip");
        assert!(computer.art.frames > 1, "and both of them move");
        assert!(busy.frames > 1);
        // The sizes themselves are checked against the PNGs by
        // `every_palette_item_is_one_cell_wide_once_scaled`.
        assert_ne!(busy.path, computer.art.path, "two strips, not one");
    }

    /// Anything the simulation can be *using* has to be a prop that says so
    /// on screen, or there is no way to tell a computer somebody is sitting
    /// at from one nobody is. Every feature, since the ones used from beside
    /// them are exactly the ones a body does not visibly occupy.
    #[test]
    fn a_prop_that_shows_it_is_in_use_shows_it_with_its_own_art() {
        for item in PALETTE {
            let Some(busy) = item.in_use else {
                continue;
            };
            assert!(
                crate::sim::feature::kinds_of(item.name).next().is_some(),
                "{:?} has in-use art but nothing can use it",
                item.name
            );
            assert!(!busy.path.is_empty());
        }
    }

    #[test]
    fn a_prop_erases_the_object_it_was_placed_from() {
        let prop = Prop {
            layer: ObjectLayer::Props,
            at: Point::new(72, 24),
            kind: PALETTE[0].name,
        };
        let mut map = crate::map::Map::new(crate::map::Size::new(4, 4), crate::map::VOID);
        map.add_object(ObjectLayer::Props, prop.object());

        assert!(map.remove_object(ObjectLayer::Props, &prop.object()));
        assert!(map.objects(ObjectLayer::Props).is_empty());
    }
}
