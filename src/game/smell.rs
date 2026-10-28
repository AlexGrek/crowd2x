//! Visible tiles only, drawn exclusively in the smell X-ray mode.
use super::actors::Sim;
use crate::{
    editor::{
        background::{TILE, cell_centre},
        grids::LayerView,
    },
    map::Point,
    render::WORLD_LAYER,
    state::AppState,
    view::{ViewSystems, VisibleArea},
};
use bevy::prelude::*;
use std::collections::HashMap;

#[derive(Component)]
pub struct SmellTile;
#[derive(Resource, Default)]
struct Overlay(HashMap<Point, Entity>);
pub struct SmellPlugin;
impl Plugin for SmellPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Overlay>()
            .add_systems(OnEnter(AppState::Game), |mut overlay: ResMut<Overlay>| {
                overlay.0.clear()
            })
            .add_systems(
                Update,
                sync.after(ViewSystems).run_if(in_state(AppState::Game)),
            );
    }
}

fn color(value: f32) -> Color {
    let alpha = (value.abs() / crate::sim::smell::MAXIMUM).sqrt() * 0.8;
    if value > 0.0 {
        Color::srgba(0.15, 0.95, 0.28, alpha)
    } else {
        Color::srgba(0.95, 0.12, 0.18, alpha)
    }
}

fn sync(
    mut commands: Commands,
    sim: Option<Res<Sim>>,
    view: Res<LayerView>,
    area: Res<VisibleArea>,
    mut overlay: ResMut<Overlay>,
    mut sprites: Query<&mut Sprite, With<SmellTile>>,
) {
    let Some(sim) = sim else {
        return;
    };
    let show = area.ready && view.shows_smell();
    overlay.0.retain(|cell, entity| {
        if show && area.tiles.contains(IVec2::new(cell.x, cell.y)) && sim.0.smell_at(*cell) != 0.0 {
            return true;
        }
        commands.entity(*entity).despawn();
        false
    });
    if !show {
        return;
    }
    for cell in area.tiles.cells() {
        let point = Point::new(cell.x, cell.y);
        let value = sim.0.smell_at(point);
        if value == 0.0 {
            continue;
        }
        let tint = color(value);
        if let Some(&entity) = overlay.0.get(&point) {
            if let Ok(mut sprite) = sprites.get_mut(entity) {
                sprite.color = tint;
            }
        } else {
            let centre = cell_centre(cell);
            let entity = commands
                .spawn((
                    SmellTile,
                    Sprite::from_color(tint, Vec2::splat(TILE)),
                    Transform::from_xyz(centre.x, centre.y, 700.0),
                    WORLD_LAYER,
                    DespawnOnExit(AppState::Game),
                ))
                .id();
            overlay.0.insert(point, entity);
        }
    }
}
