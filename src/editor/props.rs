//! The props layer: free-standing objects.
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

use bevy::prelude::*;

use super::PaletteItem;
use crate::animation::StripAnimation;
use crate::characters::{depth_for, upscale, CELL};
use crate::map::{Map, Object, ObjectKind, ObjectLayer, Point};
use crate::render::WORLD_LAYER;
use crate::state::AppState;

/// How close the cursor has to be to a prop's centre to delete it.
const ERASE_RADIUS: f32 = CELL as f32 / 2.0;

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
];

/// A placed prop, carrying the object it stands for so erasing it can take
/// that object out of the map without guessing which one it was.
#[derive(Component)]
pub struct Prop {
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

/// The palette entry that draws an object kind, or `None` for one this build
/// has no art for.
pub fn item_of(kind: &ObjectKind) -> Option<usize> {
    PALETTE.iter().position(|item| item.name == kind.as_str())
}

pub fn place(commands: &mut Commands, assets: &AssetServer, map: &mut Map, pos: Vec2, item: usize) {
    // Whole pixels only: a sprite on a fractional coordinate samples between
    // texels and puts a seam through the pixel grid. It is also what lets a
    // position be an exact key when the prop is erased again.
    let pos = pos.round();
    let at = Point::new(pos.x as i32, pos.y as i32);

    map.add_object(
        ObjectLayer::Props,
        Object {
            at,
            kind: ObjectKind::new(PALETTE[item].name),
        },
    );
    spawn_prop(commands, assets, at, item, AppState::Editor);
}

/// Delete the prop nearest the cursor, if one is close enough.
///
/// Props have no uniform size — `crate tall` is 48x64 while the rest are 48x48
/// — so this matches on distance to the centre rather than pretending every
/// prop has the same bounding box.
pub fn erase_nearest(
    commands: &mut Commands,
    map: &mut Map,
    props: &Query<(Entity, &Prop, &Transform)>,
    pos: Vec2,
) {
    let nearest = props
        .iter()
        .map(|(entity, prop, transform)| {
            (
                entity,
                prop,
                transform.translation.truncate().distance_squared(pos),
            )
        })
        .filter(|(_, _, distance)| *distance <= ERASE_RADIUS * ERASE_RADIUS)
        .min_by(|a, b| a.2.total_cmp(&b.2));

    if let Some((entity, prop, _)) = nearest {
        // One object per sprite, so a stack of props erases one at a time.
        map.remove_object(ObjectLayer::Props, &prop.object());
        commands.entity(entity).despawn();
    }
}

/// Draw a map's props, for entering a screen with a map already loaded.
///
/// `state` is the screen they belong to — the editor and the game draw the
/// same props and each despawns its own on the way out.
pub fn spawn_map(commands: &mut Commands, assets: &AssetServer, map: &Map, state: AppState) {
    for object in map.objects(ObjectLayer::Props) {
        match item_of(&object.kind) {
            Some(item) => spawn_prop(commands, assets, object.at, item, state),
            // Left in the map, so saving does not delete a prop this build
            // merely has no picture for.
            None => warn!("no art for prop {:?}, not drawn", object.kind.as_str()),
        }
    }
}

/// One prop: its sprite, whatever animation its art asks for, and — for a
/// prop whose picture says whether it is being used — the second strip the
/// game screen swaps in.
fn spawn_prop(
    commands: &mut Commands,
    assets: &AssetServer,
    at: Point,
    item: usize,
    state: AppState,
) {
    let item = &PALETTE[item];
    let y = at.y as f32;
    let prop = commands
        .spawn((
            Name::new("prop"),
            Prop {
                at,
                kind: item.name,
            },
            Sprite {
                image: assets.load(item.art.path),
                // One frame of a strip; the whole PNG for a still picture.
                rect: item.first_frame(),
                ..default()
            },
            Transform::from_xyz(at.x as f32, y, depth_for(y)).with_scale(upscale(item.scale)),
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
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A prop is stored under its palette name, and found again by it. A name
    /// that did not round-trip would come back from a file undrawable.
    #[test]
    fn every_prop_is_stored_under_a_name_that_finds_it_again() {
        for (index, item) in PALETTE.iter().enumerate() {
            assert_eq!(item_of(&ObjectKind::new(item.name)), Some(index));
        }
        assert_eq!(item_of(&ObjectKind::new("hat stand")), None);
    }

    /// A prop the simulation gives a use to has to be one the editor can place,
    /// or the use is unreachable from any map a person can make.
    #[test]
    fn every_feature_the_simulation_knows_is_a_prop_the_editor_can_place() {
        for feature in crate::sim::feature::FEATURES {
            assert!(
                item_of(&ObjectKind::new(feature.name)).is_some(),
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
            assert!(item_of(&ObjectKind::new(prop.name)).is_some(), "no palette entry for {:?}", prop.name);
        }
    }

    /// The two strips of a prop that shows whether it is in use are two
    /// pictures of the same thing, so they are drawn at the same size and
    /// swapping one for the other cannot resize the sprite.
    #[test]
    fn a_prop_that_lights_up_has_one_art_size_for_both_its_strips() {
        let computer = &PALETTE[item_of(&ObjectKind::new("computer")).expect("in the palette")];
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
            at: Point::new(72, 24),
            kind: PALETTE[0].name,
        };
        let mut map = crate::map::Map::new(crate::map::Size::new(4, 4), crate::map::VOID);
        map.add_object(ObjectLayer::Props, prop.object());

        assert!(map.remove_object(ObjectLayer::Props, &prop.object()));
        assert!(map.objects(ObjectLayer::Props).is_empty());
    }
}
