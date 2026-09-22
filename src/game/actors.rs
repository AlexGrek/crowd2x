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
//! # Only what is on the canvas, and from a pool
//!
//! A sprite exists for a unit **that is on the canvas**, and for no other. The
//! canvas is 320x180 world units at the default zoom — under seven cells by
//! four — so the drawn crowd is bounded by the size of the view and not by the
//! size of the simulation: twenty thousand humans on a big map draw a few
//! dozen sprites, and the number does not move when the crowd grows.
//!
//! [`crate::view::VisibleArea`] is the rect, [`collect_visible_crowd`] resolves
//! it into [`VisibleCrowd`] once per frame, and everything that draws something
//! about a unit — the bodies here, the action bars, the goal labels,
//! [`super::held`] — reads that one list rather than each walking the whole
//! crowd again.
//!
//! A body that leaves the canvas is **parked**, not despawned: hidden, with
//! `Actor` taken off it, on [`super::pool::ActorPool`] for the next unit that
//! needs one. The pool is bounded by the view, because a hidden sprite is
//! cheaper than a spawn and is *not* free — Bevy walks every sprite entity in
//! the world each frame before it looks at visibility.
//!
//! This module's header used to ask for one more thing: a list from the
//! simulation of what spawned each tick, so that finding arrivals stopped
//! costing a hash probe per entity per frame. It turned out not to be needed.
//! Once the renderer only asks about what it can see, that probe runs over the
//! forty units on the canvas rather than over the whole crowd, and a list of
//! new arrivals would save nothing worth the coupling. The old note was right
//! that culling and arrivals wanted the same list, and wrong about what the
//! list would be.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::sprite::Anchor;
use rand::rngs::SmallRng;
use rand::SeedableRng;

use crate::characters::{depth_for, dog, human, snap_to_texel, CELL};
use crate::editor::{background, CurrentMap};
use crate::render::WORLD_LAYER;
use crate::sim::{
    self, process_pass, spawn_pass, EntityType, GameEntity, GameState, GoalId, Input, Slot, Uid,
};
use crate::state::AppState;
use crate::ui::{MODAL, TEXT_ACCENT};
use crate::view::VisibleArea;

use super::pool::{park_budget, ActorPool};
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
///
/// **It means "drawing a live unit right now", and nothing weaker.** A body
/// parked on [`ActorPool`] has this taken off it, which is what lets a query
/// for `Actor` be the honest answer to "how many actors are on screen" — for
/// the sync below, and for the QA assertion that checks the renderer is
/// keeping up.
#[derive(Component, Clone, Copy)]
pub struct Actor(pub Uid);

/// Who is on the canvas this frame.
///
/// Slots rather than [`Uid`]s: `FixedUpdate` finishes before `Update` begins,
/// so the entity table cannot change under a frame, and a slot is a direct
/// index into the arena where a `Uid` is a hash lookup. Valid only within the
/// `Update` that built it.
///
/// In slot order, which is the simulation's own order, so what is drawn first
/// does not depend on a hash.
#[derive(Resource, Default)]
pub struct VisibleCrowd {
    on_canvas: Vec<Slot>,
}

impl VisibleCrowd {
    pub fn slots(&self) -> &[Slot] {
        &self.on_canvas
    }

    fn clear(&mut self) {
        self.on_canvas.clear();
    }
}

/// Which pair of plain-colour sprites (track, then fill) draws the reverse
/// progress bar over an entity mid-way through a timed action.
///
/// A pair per entity that currently needs one — spawned the tick a timed
/// action starts and despawned the moment it ends or the entity does, rather
/// than kept around hidden: most of a crowd is walking or waiting at any
/// given moment, and a hidden sprite still costs a place in every query that
/// visits it.
#[derive(Resource, Default)]
struct ActionBarSprites {
    by_uid: HashMap<Uid, (Entity, Entity)>,
}

impl ActionBarSprites {
    fn clear(&mut self) {
        self.by_uid.clear();
    }
}

/// One entity's goal-change notification: the emoji for whichever
/// [`GoalId`] was last seen in charge, and how long it has left to show.
///
/// Kept only for units on the canvas, so this map is the size of the view and
/// not of the crowd. A unit that walks off screen loses its entry and gets a
/// fresh one when it comes back, seeded with the goal it has *now* — so a
/// handover that happened while it was out of sight does not flash when it
/// returns. That is the same rule as a unit's very first goal, below: **a
/// handover you could not have seen is not a notification you are owed.**
struct GoalLabel {
    /// `None` until the goal has changed at least once. There is nothing to
    /// show for a unit's very first goal — it did not change from
    /// anything — so nothing is spawned until the first real handover.
    sprite: Option<Entity>,
    last_goal: GoalId,
    /// Seconds left before the label is fully faded: `0.0` both before the
    /// first change and once one has finished fading, and
    /// [`GOAL_EMOJI_FADE`] the instant one is seen.
    remaining: f32,
}

/// Which entity is showing what about its last goal handover — see
/// [`GoalLabel`]. Kept for as long as the entity is, so a second handover
/// reuses the same sprite rather than despawning and respawning it.
#[derive(Resource, Default)]
struct GoalLabelSprites {
    by_uid: HashMap<Uid, GoalLabel>,
}

impl GoalLabelSprites {
    fn clear(&mut self) {
        self.by_uid.clear();
    }
}

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
            .init_resource::<ActionBarSprites>()
            .init_resource::<GoalLabelSprites>()
            .init_resource::<VisibleCrowd>()
            .init_resource::<ActorPool>()
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
                collect_visible_crowd
                    .in_set(super::CrowdSystems)
                    .run_if(in_state(AppState::Game).and_then(resource_exists::<Sim>)),
            )
            // In `SpriteSync`, which `game` orders after `CrowdSystems`: all
            // three read the one visible list rather than each deciding for
            // itself who is on screen.
            .add_systems(
                Update,
                (sync_sprites, sync_action_bars, sync_goal_labels)
                    .in_set(super::SpriteSync)
                    .run_if(in_state(AppState::Game).and_then(resource_exists::<Sim>)),
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
    mut bars: ResMut<ActionBarSprites>,
    mut labels: ResMut<GoalLabelSprites>,
    mut crowd: ResMut<VisibleCrowd>,
    mut pool: ResMut<ActorPool>,
    mut input: ResMut<SimInput>,
) {
    commands.remove_resource::<Sim>();
    sprites.clear();
    bars.clear();
    labels.clear();
    crowd.clear();
    // Forgets the parked bodies without despawning them: they carry
    // `DespawnOnExit` like every other sprite, and a second command against an
    // entity Bevy is already removing is a command against nothing.
    pool.clear();
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

/// Work out who is on the canvas, once, for everything that draws.
///
/// A linear scan of the entity arena testing each position against the view.
/// Deliberately *not* a region query on `sim::Occupancy`, even though that is
/// a dense grid and would be crowd-independent: occupancy holds one unit per
/// cell and `GameState::spawn` places a unit where it was asked for whether or
/// not the cell was taken, so a stack of units reports as one and the rest
/// would silently never be drawn. `qa::perf::spread` stacks exactly that way
/// once a crowd outgrows the passable cells.
///
/// The scan is also not where the cost was. What it replaces is five separate
/// walks of the whole crowd, each with a hash probe per entity, and Bevy's own
/// three passes over every sprite entity plus their transform propagation.
/// Going from those to one cheap read pass is the win; going from one pass to
/// zero is worth an order of magnitude less and costs a second structure in
/// `sim/` that the move step would have to keep true.
fn collect_visible_crowd(
    sim: Res<Sim>,
    area: Res<VisibleArea>,
    sprites: Res<ActorSprites>,
    mut crowd: ResMut<VisibleCrowd>,
) {
    // Cleared rather than rebuilt: a `Vec` keeps its capacity, so after the
    // first few frames this allocates nothing.
    crowd.clear();
    if !area.ready {
        return;
    }

    for (slot, entity) in sim.0.entities().iter_slots() {
        let pos = world_pos(entity.position());
        // Already drawn units are judged against the wider rect — see
        // `view::HYSTERESIS`. Without the split, a unit standing on the
        // boundary would be taken from and returned to the pool every frame,
        // and each of those moves it between archetypes.
        let wanted = if sprites.by_uid.contains_key(&entity.uid()) {
            area.should_keep(pos)
        } else {
            area.should_draw(pos)
        };
        if wanted {
            crowd.on_canvas.push(slot);
        }
    }
}

/// Make the sprites match the *visible* entities.
///
/// Three passes, because they are three different jobs against two collections
/// — doing them in one loop would mean spawning into a map that is being
/// walked and despawning out of it at the same time.
fn sync_sprites(
    mut commands: Commands,
    assets: Res<AssetServer>,
    sheets: Option<Res<dog::DogSheets>>,
    sim: Res<Sim>,
    area: Res<VisibleArea>,
    crowd: Res<VisibleCrowd>,
    mut sprites: ResMut<ActorSprites>,
    mut pool: ResMut<ActorPool>,
    dolls: Query<&human::Paperdoll>,
    mut actors: Query<(&Actor, &mut Transform, Option<&mut dog::Facing>)>,
) {
    let state = &sim.0;
    let budget = park_budget(area.tiles.area());

    // 1. Anything that came into view — newly spawned or newly on screen.
    for &slot in crowd.slots() {
        let Some(entity) = state.entities().slot(slot) else {
            continue;
        };
        let uid = entity.uid();
        if sprites.by_uid.contains_key(&uid) {
            continue;
        }
        let pos = world_pos(entity.position());
        let sprite = match entity.kind() {
            Some(EntityType::Human) => {
                let look = look_from(entity);
                match pool.take_human() {
                    // Dressed as whoever needs it, positioned, and only then
                    // made visible — see `human::redress`.
                    Some(body) => match dolls.get(body) {
                        Ok(doll) => {
                            human::redress(&mut commands, &assets, body, doll, pos, look);
                            body
                        }
                        // A parked body without its own layer index is not a
                        // paperdoll any more. Drop it rather than dress it.
                        Err(_) => {
                            commands.entity(body).despawn();
                            human::spawn(&mut commands, &assets, pos, look)
                        }
                    },
                    None => human::spawn(&mut commands, &assets, pos, look),
                }
            }
            Some(EntityType::Dog) => {
                let Some(sheets) = sheets.as_deref() else {
                    // The atlas is inserted alongside the world, so this only
                    // happens if the two land in different frames. Leaving the
                    // dog undrawn picks it up next frame; making one up now
                    // would need a handle that does not exist.
                    continue;
                };
                let facing = facing_of(entity);
                match pool.take_dog() {
                    Some(body) => {
                        dog::redress(&mut commands, sheets, body, pos, facing);
                        body
                    }
                    None => dog::spawn(&mut commands, sheets, pos, facing),
                }
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

    // 2. Anything that left the world, or left the canvas.
    //
    // Asked per sprite rather than by collecting every live id into a set
    // first: the set was an allocation and one hash insert per *entity* every
    // frame, to answer a question that is almost always "nobody left".
    sprites.by_uid.retain(|uid, sprite| {
        let still_here = state
            .entities()
            .get(*uid)
            .is_some_and(|entity| area.should_keep(world_pos(entity.position())));
        if still_here {
            return true;
        }
        park_or_despawn(&mut commands, &mut pool, *sprite, uid.kind(), budget);
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

/// Put a body that has left the canvas on the pool, or despawn it if the pool
/// is full.
///
/// Parking takes [`Actor`] off it — so it stops being counted as drawing
/// anybody — and hides it. It is deliberately **not moved**: writing a
/// `Transform` marks it changed and pushes it and its three children through
/// propagation, which is the exact cost the movement pass below already
/// refuses to pay. A hidden body is not drawn wherever it happens to be.
fn park_or_despawn(
    commands: &mut Commands,
    pool: &mut ActorPool,
    body: Entity,
    kind: Option<EntityType>,
    budget: usize,
) {
    let parked = match kind {
        Some(EntityType::Human) => pool.park_human(body, budget),
        Some(EntityType::Dog) => pool.park_dog(body, budget),
        None => false,
    };
    if parked {
        commands
            .entity(body)
            .remove::<Actor>()
            .insert(Visibility::Hidden);
    } else {
        commands.entity(body).despawn();
    }
}

/// Reverse progress bar dimensions and placement, all in canvas pixels — the
/// world camera's own unit. A plain colour has no texture to keep from
/// splitting across a texel, only a canvas pixel to land on whole, which
/// rounding the fill's width to the nearest one already gives it; nothing here
/// needs [`snap_to_texel`].
const BAR_WIDTH: f32 = 20.0;
const BAR_HEIGHT: f32 = 3.0;
/// Above the top of a character sprite, which [`characters::human::spawn`] and
/// [`dog::spawn`] both centre on their position.
const BAR_LIFT: f32 = CELL as f32 / 2.0 + 5.0;
/// Clear of a paperdoll's own layers (hair reaches 0.003) and of the selection
/// frame (0.004), so the bar draws in front of both.
const BAR_DEPTH: f32 = 0.006;

/// Show a reverse progress bar over anyone mid-way through something that
/// takes time to do — not walking, not waiting, see
/// [`sim::brain::Action::progress`] — and take it down the moment they are
/// not, rather than leaving it hidden: see [`ActionBarSprites`].
fn sync_action_bars(
    mut commands: Commands,
    sim: Res<Sim>,
    area: Res<VisibleArea>,
    crowd: Res<VisibleCrowd>,
    mut bars: ResMut<ActionBarSprites>,
    mut parts: Query<(&mut Transform, &mut Sprite)>,
) {
    let state = &sim.0;

    // Anyone who finished, left the world, or left the canvas.
    bars.by_uid.retain(|uid, &mut (track, fill)| {
        let still_running = state.entities().get(*uid).is_some_and(|entity| {
            entity.action_progress().is_some() && area.should_keep(world_pos(entity.position()))
        });
        if still_running {
            return true;
        }
        commands.entity(track).despawn();
        commands.entity(fill).despawn();
        false
    });

    // Anyone visible running one, new or continuing.
    for &slot in crowd.slots() {
        let Some(entity) = state.entities().slot(slot) else {
            continue;
        };
        let Some(progress) = entity.action_progress() else {
            continue;
        };
        let uid = entity.uid();
        let &mut (track, fill) = bars
            .by_uid
            .entry(uid)
            .or_insert_with(|| spawn_action_bar(&mut commands));

        let pos = world_pos(entity.position());
        let base = Vec3::new(pos.x, pos.y + BAR_LIFT, depth_for(pos.y) + BAR_DEPTH);
        let remaining = ((1.0 - progress) * BAR_WIDTH).round();

        if let Ok((mut transform, _)) = parts.get_mut(track) {
            transform.translation = base;
        }
        if let Ok((mut transform, mut sprite)) = parts.get_mut(fill) {
            transform.translation = Vec3::new(base.x - BAR_WIDTH / 2.0, base.y, base.z + 0.0001);
            sprite.custom_size = Some(Vec2::new(remaining, BAR_HEIGHT));
        }
    }
}

/// One bar: a fixed-width track and a fill that shrinks from its right edge as
/// [`sim::brain::Action::progress`] rises, anchored on its left so the shrink
/// reads as time running out rather than as sliding sideways.
fn spawn_action_bar(commands: &mut Commands) -> (Entity, Entity) {
    let track = commands
        .spawn((
            Name::new("action bar track"),
            Sprite::from_color(MODAL, Vec2::new(BAR_WIDTH, BAR_HEIGHT)),
            Transform::default(),
            WORLD_LAYER,
            DespawnOnExit(AppState::Game),
        ))
        .id();
    let fill = commands
        .spawn((
            Name::new("action bar fill"),
            Sprite::from_color(TEXT_ACCENT, Vec2::new(BAR_WIDTH, BAR_HEIGHT)),
            Anchor::CENTER_LEFT,
            Transform::default(),
            WORLD_LAYER,
            DespawnOnExit(AppState::Game),
        ))
        .id();
    (track, fill)
}

/// The bundled color-emoji font — see the `dev` skill for why this needs one
/// of its own rather than reusing Bevy's built-in default: a COLRv0 table is
/// what `swash` (which `bevy_text` rasterizes glyphs through) knows how to
/// turn into a colored glyph rather than a blank one.
pub(super) const EMOJI_FONT: &str = "fonts/Twemoji.Mozilla.ttf";

/// Canvas pixels tall the goal emoji is drawn at — the world camera's own
/// unit, the same one [`BAR_WIDTH`] is in. Large enough to read as a status
/// icon rather than a decoration, since it is only up for
/// [`GOAL_EMOJI_FADE`] seconds at a time.
const GOAL_EMOJI_SIZE: f32 = 22.0;
/// Above the reverse progress bar, which is itself above the character —
/// see [`BAR_LIFT`] — so the two never overlap when both are showing.
const GOAL_EMOJI_LIFT: f32 = CELL as f32 / 2.0 + 20.0;
/// Clear of a paperdoll's own layers, the selection frame and the action bar
/// (0.006), so the goal emoji draws in front of everything about its
/// character.
const GOAL_EMOJI_DEPTH: f32 = 0.007;
/// Seconds a goal-change label stays up before it has faded out completely.
const GOAL_EMOJI_FADE: f32 = 3.0;

/// Flash the emoji for whichever goal just took charge of a unit's mind
/// above its head, fading it out over [`GOAL_EMOJI_FADE`] seconds — see
/// [`sim::GameEntity::current_goal`]. Nothing is shown while a goal holds
/// charge; this is a notification of the handover, not a status bar.
///
/// Real time, [`Time`] rather than [`Time::<Fixed>`]: the fade is a reading
/// aid, not a fact about the simulation, so it runs at the same rate whatever
/// [`GameSpeed`] the world itself is ticking at.
fn sync_goal_labels(
    mut commands: Commands,
    assets: Res<AssetServer>,
    time: Res<Time>,
    sim: Res<Sim>,
    area: Res<VisibleArea>,
    crowd: Res<VisibleCrowd>,
    mut labels: ResMut<GoalLabelSprites>,
    mut texts: Query<(&mut Transform, &mut TextColor, &mut Text2d, &mut Visibility)>,
) {
    let state = &sim.0;
    let dt = time.delta_secs();

    // Anyone who left the world, or left the canvas.
    //
    // Dropped rather than kept, so this map stays the size of the visible
    // crowd instead of the size of the simulation — a per-frame `retain` over
    // twenty thousand entries is one of the walks the culling exists to
    // delete. What it costs is handled where a label is created, below.
    labels.by_uid.retain(|uid, label| {
        let keep = state
            .entities()
            .get(*uid)
            .is_some_and(|entity| area.should_keep(world_pos(entity.position())));
        if !keep {
            if let Some(sprite) = label.sprite {
                commands.entity(sprite).despawn();
            }
        }
        keep
    });

    for &slot in crowd.slots() {
        let Some(entity) = state.entities().slot(slot) else {
            continue;
        };
        let Some(goal) = entity.current_goal() else {
            continue;
        };
        let uid = entity.uid();
        let pos = world_pos(entity.position());
        let base = Vec3::new(
            pos.x,
            pos.y + GOAL_EMOJI_LIFT,
            depth_for(pos.y) + GOAL_EMOJI_DEPTH,
        );

        let label = labels.by_uid.entry(uid).or_insert(GoalLabel {
            sprite: None,
            last_goal: goal,
            remaining: 0.0,
        });
        let changed = label.last_goal != goal;
        label.last_goal = goal;
        label.remaining = if changed {
            GOAL_EMOJI_FADE
        } else {
            (label.remaining - dt).max(0.0)
        };

        let sprite = match label.sprite {
            Some(sprite) => sprite,
            // Nothing to show until the first handover — see `GoalLabel`.
            None if !changed => continue,
            None => {
                let sprite = commands
                    .spawn((
                        Text2d::new(goal.emoji()),
                        TextFont {
                            font: assets.load(EMOJI_FONT).into(),
                            font_size: GOAL_EMOJI_SIZE.into(),
                            ..default()
                        },
                        Transform::from_translation(base),
                        WORLD_LAYER,
                        DespawnOnExit(AppState::Game),
                    ))
                    .id();
                label.sprite = Some(sprite);
                sprite
            }
        };

        let Ok((mut transform, mut color, mut text, mut visibility)) = texts.get_mut(sprite)
        else {
            continue;
        };

        if transform.translation != base {
            transform.translation = base;
        }
        if changed {
            *text = Text2d::new(goal.emoji());
        }

        let alpha = (label.remaining / GOAL_EMOJI_FADE).clamp(0.0, 1.0);
        if color.0.alpha() != alpha {
            color.0.set_alpha(alpha);
        }
        let wanted = if alpha > 0.0 { Visibility::Visible } else { Visibility::Hidden };
        if *visibility != wanted {
            *visibility = wanted;
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

    /// Everything drawn *about* a unit has to fit inside the margin
    /// `view::VisibleArea` grows the canvas by, or it is clipped the moment
    /// its unit's own position leaves the screen. Adding a taller overlay
    /// should fail here rather than show up as a label cut in half.
    #[test]
    fn every_overlay_fits_inside_the_culling_margin() {
        use crate::characters::{OVERHANG_ABOVE, OVERHANG_BELOW, OVERHANG_SIDE};

        assert!(
            GOAL_EMOJI_LIFT + GOAL_EMOJI_SIZE / 2.0 <= OVERHANG_ABOVE,
            "the goal label reaches above the margin"
        );
        assert!(
            BAR_LIFT + BAR_HEIGHT / 2.0 <= OVERHANG_ABOVE,
            "the action bar reaches above the margin"
        );
        assert!(
            BAR_WIDTH / 2.0 <= OVERHANG_SIDE,
            "the action bar reaches past the sides of the margin"
        );
        // The art itself is centred on the position, so half a cell of it
        // hangs off every edge.
        assert!(CELL as f32 / 2.0 <= OVERHANG_BELOW);
        assert!(CELL as f32 / 2.0 <= OVERHANG_SIDE);
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
