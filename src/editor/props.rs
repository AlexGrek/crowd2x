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
use crate::characters::{depth_for, upscale, CELL};
use crate::map::{Map, Object, ObjectKind, ObjectLayer, Point};
use crate::render::WORLD_LAYER;
use crate::state::AppState;

/// How close the cursor has to be to a prop's centre to delete it.
const ERASE_RADIUS: f32 = CELL as f32 / 2.0;

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
    spawn_prop(commands, assets, at, item);
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

/// Draw a map's props, for entering the editor with a map already loaded.
pub fn spawn_map(commands: &mut Commands, assets: &AssetServer, map: &Map) {
    for object in map.objects(ObjectLayer::Props) {
        match item_of(&object.kind) {
            Some(item) => spawn_prop(commands, assets, object.at, item),
            // Left in the map, so saving does not delete a prop this build
            // merely has no picture for.
            None => warn!("editor: no art for prop {:?}, not drawn", object.kind.as_str()),
        }
    }
}

fn spawn_prop(commands: &mut Commands, assets: &AssetServer, at: Point, item: usize) {
    let y = at.y as f32;
    commands.spawn((
        Name::new("prop"),
        Prop {
            at,
            kind: PALETTE[item].name,
        },
        Sprite::from_image(assets.load(PALETTE[item].path)),
        Transform::from_xyz(at.x as f32, y, depth_for(y)).with_scale(upscale(PALETTE[item].scale)),
        WORLD_LAYER,
        DespawnOnExit(AppState::Editor),
    ));
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
