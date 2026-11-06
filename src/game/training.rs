//! The soldiers' training ground, drawn: a chalk-marked patch of trampled
//! earth on each waypoint of a soldier's lap and a bigger one, crossed, in
//! the middle, where the commander stands.
//!
//! Like the outposts (`game::outposts`) it is the simulation's alone, read
//! from [`GameState::training`](crate::sim::GameState::training), never from a
//! map. Once the king has picked it the ground never moves, so its seventeen
//! sprites are spawned once, when it appears (or a game with one is loaded),
//! and live as long as the game screen. Art: `tools/training_art.py`.

use bevy::prelude::*;

use crate::characters::{depth_for, upscale, ART_SCALE};
use crate::editor::background::cell_centre;
use crate::map::Point;
use crate::render::WORLD_LAYER;
use crate::sim::training::ring;
use crate::state::AppState;

use super::actors::Sim;

/// A waypoint of the lap: a chalk dot on trampled earth, 16x16.
pub const MARK: &str = "drill_mark.png";

/// The middle of the ground: a chalk cross on trampled earth, 16x16.
pub const CENTRE: &str = "drill_centre.png";

/// How far above the ground under it a mark is drawn: over the tile, under
/// anybody walking over it (the same as a built wall's, `game::outposts`).
const DEPTH: f32 = -990.0;

/// Which ground is drawn, and its sprites.
#[derive(Resource, Default)]
struct Marks {
    ground: Option<Point>,
    sprites: Vec<Entity>,
}

pub struct TrainingPlugin;

impl Plugin for TrainingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Marks>()
            // Forgotten on the way in, not out: the sprites go with
            // `DespawnOnExit`, which closing the window skips `OnExit` for.
            .add_systems(OnEnter(AppState::Game), |mut marks: ResMut<Marks>| *marks = Marks::default())
            .add_systems(Update, sync_ground.run_if(in_state(AppState::Game)));
    }
}

/// Draw the ground the king picked, once.
fn sync_ground(mut commands: Commands, assets: Res<AssetServer>, sim: Option<Res<Sim>>, mut marks: ResMut<Marks>) {
    let Some(sim) = sim else {
        return;
    };
    let ground = sim.0.training().center();
    if marks.ground == ground {
        return;
    }
    for sprite in marks.sprites.drain(..) {
        commands.entity(sprite).despawn();
    }
    marks.ground = ground;
    let Some(center) = ground else {
        return;
    };
    let cells = ring(center).into_iter().map(|cell| (cell, MARK)).chain(std::iter::once((center, CENTRE)));
    for (cell, art) in cells {
        let at = cell_centre(IVec2::new(cell.x, cell.y));
        let sprite = commands
            .spawn((
                Name::new("training ground"),
                Sprite { image: assets.load(art), ..default() },
                Transform::from_xyz(at.x, at.y, depth_for(at.y) + DEPTH).with_scale(upscale(ART_SCALE)),
                WORLD_LAYER,
                DespawnOnExit(AppState::Game),
            ))
            .id();
        marks.sprites.push(sprite);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The marks have art of their own, one 16x16 frame each.
    #[test]
    fn the_marks_have_their_own_art_at_the_art_resolution() {
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        for path in [MARK, CENTRE] {
            let bytes = std::fs::read(assets.join(path)).unwrap_or_else(|_| panic!("no {path}"));
            let (width, height) = (u32::from_be_bytes(bytes[16..20].try_into().unwrap()), u32::from_be_bytes(bytes[20..24].try_into().unwrap()));
            assert_eq!((width, height), (16, 16), "{path}");
        }
    }
}
