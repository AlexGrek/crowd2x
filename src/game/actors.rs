//! The bridge between the simulation and what is on screen.
//!
//! This is the "thin adapter" the architecture rules keep talking about, and
//! it is worth being precise about how thin. It does four things:
//!
//! 1. Builds a [`GameState`] when the game screen opens, and drops it on the
//!    way out.
//! 2. Runs a spawn pass every fixed step, and as many processing passes as
//!    [`GameSpeed`] asks for — which is none while the game is paused.
//! 3. Spawns a sprite for an entity that appeared, despawns one for an entity
//!    that went, and moves the rest.
//! 4. Turns simulation facts into art — an appearance seed into a paperdoll, a
//!    [`sim::Facing`] into the component of the same name.
//!
//! It decides nothing. Every question of *what happens* is answered inside
//! `src/sim/`, which has no idea any of this exists.
//!
//! # One `ResMut`, on purpose
//!
//! [`tick_sim`] is the only system in the app that takes [`Sim`] mutably.
//! Every `ResMut<T>` is an exclusive lock on Bevy's schedule, so a second
//! writer would serialise the whole frame around the simulation while buying
//! nothing: the tick is already the one place the world changes. Anything that
//! wants to *ask* for something takes [`SimInput`] instead.
//!
//! # What this does not do yet
//!
//! One sprite per entity, spawned when the entity is, with no culling and no
//! pooling. That is honest for a screen whose worlds are built by hand, and it
//! is *not* what a crowd needs: the canvas is 320x180, so the visible set stays
//! small however big the simulation gets, and the next step here is to spawn
//! sprites only for entities on the canvas and to reuse them from a pool
//! rather than despawning. See the `dev` skill's "Rendering cost".
//!
//! There is a second, quieter cost in [`sync_sprites`]: finding what *arrived*
//! means asking the sprite map about every entity, every frame — one hash
//! lookup per entity per frame that almost always answers "already drawn". The
//! fix is not a faster map, it is for the simulation to hand over a list of
//! what spawned this tick, so the renderer stops rediscovering it. Worth doing
//! at the same time as the culling, and not before: both want the same list.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use rand::rngs::SmallRng;
use rand::SeedableRng;

use crate::characters::{depth_for, dog, human, snap_to_texel};
use crate::editor::{background, CurrentMap};
use crate::sim::{self, process_pass, spawn_pass, EntityType, GameEntity, GameState, Input, Uid};
use crate::state::AppState;

use super::speed::GameSpeed;

/// How the world is seeded when the game screen opens.
///
/// Fixed rather than taken from the clock, so opening the same map twice gives
/// the same world and a bug seen once can be seen again. A seed the player
/// chooses is a later feature; a seed nobody chose is a bug that happens
/// rarely and cannot be reproduced.
pub const DEFAULT_SEED: u64 = 0x5eed_c0de;

/// The simulation, as Bevy holds it.
///
/// One field, because the super-object is the point — this wrapper exists only
/// so `Resource` can be derived on a type this crate owns.
#[derive(Resource)]
pub struct Sim(pub GameState);

/// What has been asked of the simulation but not yet handed to it.
///
/// Separate from [`Sim`] so that anything wanting to spawn something — the
/// debug harness, a QA script, a future click — takes `ResMut<SimInput>`
/// rather than `ResMut<Sim>`, and so cannot become a second writer of the
/// world by accident.
#[derive(Resource, Default)]
pub struct SimInput(pub Input);

/// Which Bevy entity is drawing which simulated one.
#[derive(Resource, Default)]
pub struct ActorSprites {
    by_uid: HashMap<Uid, Entity>,
}

impl ActorSprites {
    fn clear(&mut self) {
        self.by_uid.clear();
    }
}

/// Marks a sprite that is drawing a simulated entity, and says which one.
///
/// The link runs this way as well as through [`ActorSprites`] so the sync
/// system can walk the sprites it already has in one query rather than looking
/// each one up by id.
#[derive(Component, Clone, Copy)]
pub struct Actor(pub Uid);

pub struct ActorsPlugin;

impl Plugin for ActorsPlugin {
    fn build(&self, app: &mut App) {
        // Created once and cleared between visits, not inserted on entry:
        // anything that wants to ask the simulation for something does it in
        // `OnEnter` too, and `OnEnter` systems have no order between them, so
        // a resource that appears there is a resource half of them will not
        // find. `Sim` itself has to wait — it needs the map.
        app.init_resource::<SimInput>()
            .init_resource::<ActorSprites>()
            .add_systems(OnEnter(AppState::Game), build_world)
            .add_systems(OnExit(AppState::Game), tear_down_world)
            // Fixed, not per-frame: the simulation advances in equal steps
            // whatever the renderer is managing, which is what makes a run
            // reproducible and what stops a slow frame teleporting everybody
            // across the map in one go.
            .add_systems(
                FixedUpdate,
                tick_sim.run_if(in_state(AppState::Game).and_then(resource_exists::<Sim>)),
            )
            .add_systems(
                Update,
                sync_sprites.run_if(in_state(AppState::Game).and_then(resource_exists::<Sim>)),
            );
    }
}

/// Build the world the game screen is about to show.
///
/// The map is cloned: the simulation owns its copy and the editor keeps
/// editing the original, so a wall cannot move under somebody's cursor because
/// the game was open.
fn build_world(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut layouts: ResMut<Assets<TextureAtlasLayout>>,
    current: Res<CurrentMap>,
) {
    let size = current.map.size();
    let state = GameState::new(current.map.clone(), DEFAULT_SEED);
    state.log.push(format!(
        "opened {} ({}x{})",
        current.title(),
        size.width,
        size.height
    ));

    commands.insert_resource(Sim(state));
    // The dog atlas used to be loaded by the demo crowd on `Startup`. It
    // belongs to whoever draws dogs, which is now this screen.
    commands.insert_resource(dog::DogSheets::load(&assets, &mut layouts));
}

/// Drop the world on the way out.
///
/// The sprites go by `DespawnOnExit`, so what is left is the bookkeeping that
/// would otherwise point at entities that no longer exist — and any command
/// nobody got round to applying, which would otherwise be carried out against
/// the *next* world.
///
/// Removing `Sim` rather than keeping it means re-entering a map starts it
/// again from the seed, which is the honest behaviour for a screen that saves
/// nothing.
fn tear_down_world(
    mut commands: Commands,
    mut sprites: ResMut<ActorSprites>,
    mut input: ResMut<SimInput>,
) {
    commands.remove_resource::<Sim>();
    sprites.clear();
    input.0.clear();
}

/// One step of the game. The only `ResMut<Sim>` in the app.
///
/// The two passes are taken separately rather than through
/// [`process_game_state`] because the speed is a count of *processing* passes
/// and never of spawn passes:
///
/// * The spawn pass runs every fixed step whatever the speed is, pause
///   included. Something asked for while the game is paused should appear,
///   standing still — waiting for the world to be let go again would look like
///   the request had been lost — and a command held over would be applied to
///   however many ticks arrive at once when it is.
/// * The processing passes are the speed. A tick is always the same slice of
///   world, so 4x is four of them; stretching `dt` instead would change what
///   the simulation does rather than only when.
///
/// [`process_game_state`]: crate::sim::process_game_state
fn tick_sim(
    time: Res<Time<Fixed>>,
    mut speed: ResMut<GameSpeed>,
    mut sim: ResMut<Sim>,
    mut input: ResMut<SimInput>,
) {
    spawn_pass(&mut sim.0, &input.0);
    // Consumed: a command left in place would be replayed every step, which
    // for a spawn means an unbounded crowd.
    input.0.clear();

    let dt = time.delta_secs();
    for _ in 0..speed.steps() {
        process_pass(&mut sim.0, dt);
    }
}

/// Make the sprites match the entities.
///
/// Three passes, because they are three different jobs against two collections
/// — doing them in one loop would mean spawning into a map that is being
/// walked and despawning out of it at the same time.
fn sync_sprites(
    mut commands: Commands,
    assets: Res<AssetServer>,
    sheets: Option<Res<dog::DogSheets>>,
    sim: Res<Sim>,
    mut sprites: ResMut<ActorSprites>,
    mut actors: Query<(&Actor, &mut Transform, Option<&mut dog::Facing>)>,
) {
    let state = &sim.0;

    // 1. Anything that arrived.
    for entity in state.entities().iter() {
        let uid = entity.uid();
        if sprites.by_uid.contains_key(&uid) {
            continue;
        }
        let pos = world_pos(entity.position());
        let sprite = match entity.kind() {
            Some(EntityType::Human) => {
                human::spawn(&mut commands, &assets, pos, look_from(entity))
            }
            Some(EntityType::Dog) => {
                let Some(sheets) = sheets.as_deref() else {
                    // The atlas is inserted alongside the world, so this only
                    // happens if the two land in different frames. Leaving the
                    // dog undrawn picks it up next frame; making one up now
                    // would need a handle that does not exist.
                    continue;
                };
                dog::spawn(&mut commands, sheets, pos, facing_of(entity))
            }
            // An id tagged with a type this build has no art for. Refusing to
            // draw it is better than guessing which sprite it meant.
            None => continue,
        };
        commands
            .entity(sprite)
            .insert((Actor(uid), DespawnOnExit(AppState::Game)));
        sprites.by_uid.insert(uid, sprite);
    }

    // 2. Anything that left.
    //
    // Asked per sprite rather than by collecting every live id into a set
    // first: the set was an allocation and one hash insert per *entity* every
    // frame, to answer a question that is almost always "nobody left".
    sprites.by_uid.retain(|uid, sprite| {
        if state.entities().contains(*uid) {
            return true;
        }
        commands.entity(*sprite).despawn();
        false
    });

    // 3. Everything that moved.
    for (actor, mut transform, facing) in &mut actors {
        let Some(entity) = state.entities().get(actor.0) else {
            // Despawned above; the command has not been applied yet.
            continue;
        };
        let pos = world_pos(entity.position());
        // Painter's order from world Y — the same function the props use, so
        // an actor walks in front of a bed it is standing below.
        let wanted = Vec3::new(pos.x, pos.y, depth_for(pos.y));

        // Compared before writing, and not as a micro-optimisation: taking
        // `Mut<Transform>` mutably marks it `Changed`, and Bevy propagates
        // every changed transform to its children. Writing unconditionally
        // would push the whole crowd through propagation each frame even when
        // nobody moved — and positions are rounded to whole pixels, so an
        // actor is genuinely unchanged for most of the ticks it spends
        // crossing a cell.
        if transform.translation != wanted {
            transform.translation = wanted;
        }

        // Written only when it actually changed: `dog::apply_facing` swaps the
        // sheet on `Changed<Facing>`, and an unconditional write would make
        // that filter match every dog every frame.
        if let Some(mut facing) = facing {
            let wanted = facing_of(entity);
            if *facing != wanted {
                *facing = wanted;
            }
        }
    }
}

/// Cell units to world pixels, on the art's own grid.
///
/// The step on screen is one texel of the source art — a sixteenth of a cell,
/// three canvas pixels — not one canvas pixel. Rounding to the canvas pixel
/// instead, which is what this used to do, keeps the renderer happy but lets a
/// character's own pixels sit a third of a texel off the grid its art is drawn
/// on, so crossing a cell reads as being nudged between sub-positions rather
/// than walking. [`characters::snap_to_texel`] is where that grid is defined.
///
/// [`characters::snap_to_texel`]: crate::characters::snap_to_texel
pub fn world_pos((x, y): (f32, f32)) -> Vec2 {
    snap_to_texel(Vec2::new(x, y) * background::TILE)
}

/// A paperdoll from a simulation seed.
///
/// The simulation supplies a stable number and nothing else about how a human
/// looks; which PNGs it turns into is decided here, next to the art, so adding
/// a hairstyle never touches `src/sim/`.
pub fn look_from(entity: &dyn GameEntity) -> human::Look {
    human::Look::random(&mut SmallRng::seed_from_u64(entity.appearance_seed()))
}

/// The simulation's facing, as the component the art uses.
///
/// An entity with no facing of its own is drawn right-facing; only dogs have
/// two sheets, so there is nothing else the answer could be.
pub fn facing_of(entity: &dyn GameEntity) -> dog::Facing {
    match entity.facing() {
        Some(sim::Facing::Left) => dog::Facing::Left,
        _ => dog::Facing::Right,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::characters::{ART, ART_SCALE};

    #[test]
    fn a_cell_position_becomes_whole_world_pixels() {
        // The middle of cell (3, 2), at 48 pixels to a cell.
        assert_eq!(world_pos((3.5, 2.5)), Vec2::new(168.0, 120.0));
        // ...and anything in between still lands on a whole pixel, or the
        // upscaled texels come out uneven.
        let awkward = world_pos((1.0 / 3.0, 2.0 / 7.0));
        assert_eq!(awkward, awkward.round());
    }

    #[test]
    fn a_position_is_snapped_to_a_whole_texel_of_art() {
        // Sixteen steps to a cell, three canvas pixels each: every position
        // in between has to collapse onto one of them, or the character's own
        // pixels stop lining up with the grid they are drawn on.
        for i in 0..64 {
            let pos = world_pos((i as f32 / 64.0 * 3.0, 0.0));
            assert_eq!(
                pos.x % ART_SCALE,
                0.0,
                "{pos:?} is not a whole texel from the origin"
            );
        }
        // One sixteenth of a cell is the step, and nothing smaller moves.
        assert_eq!(world_pos((1.0 / 16.0, 0.0)).x, ART_SCALE);
        assert_eq!(world_pos((0.03, 0.0)), world_pos((0.0, 0.0)));
    }

    #[test]
    fn the_art_grid_divides_the_tile_grid() {
        // If a cell ever stops being a whole number of texels, snapping to a
        // texel would put actors off the tiles they stand on.
        assert_eq!(ART as f32 * ART_SCALE, background::TILE);
    }

    #[test]
    fn the_origin_cell_starts_at_the_world_origin() {
        // The map is drawn from (0, 0) into +x/+y, so cell 0 must not be
        // centred on it — `background::cell_centre` puts it half a tile in.
        assert_eq!(world_pos((0.0, 0.0)), Vec2::ZERO);
        assert_eq!(
            world_pos((0.5, 0.5)),
            background::cell_centre(IVec2::ZERO)
        );
    }
}
