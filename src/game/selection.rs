//! Who the player is looking at: the click that picks somebody, and the green
//! frame that says so.
//!
//! What the selection *is* lives here; what it puts on screen is
//! [`super::unitpanel`], which is the bottom bar with the portrait and the
//! menus. The split is the same one the rest of the screen makes — this module
//! answers "who", the panel answers "and what about them".
//!
//! # Selection is not simulation state
//!
//! [`Selected`] is a Bevy resource and not a field of `GameState`, on purpose.
//! Which entity somebody is looking at changes nothing about the world: the
//! crowd wanders identically whether or not one of them is highlighted, and a
//! replay of the same seed is the same replay. What the panel does *with* a
//! selection — freezing, despawning — goes back through [`sim::Command`] like
//! any other thing done to the world.
//!
//! # Picking is by cell, not by sprite
//!
//! A click is turned into a world position, the world position into a cell,
//! and the cell asked of [`sim::Occupancy`] — one lookup, whatever the size of
//! the crowd, and the same addressing the simulation itself uses. Hit-testing
//! the sprites instead would mean a scan over every actor on screen, and it
//! would disagree with the simulation about who is standing where: a walker
//! halfway across a boundary is *in* the cell it is entering, while its 48x48
//! art still overlaps the one it is leaving.
//!
//! The cost is that clicking the overhanging half of somebody's art picks
//! whoever is in the cell under the pointer, which may be nobody. That reads
//! correctly on a tile grid, which is what this is.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::characters::{depth_for, upscale, ART_SCALE};
use crate::editor::background;
use crate::render::{cursor_world_pos, PixelZoom, WorldCamera, WORLD_LAYER};
use crate::sim::Uid;
use crate::state::AppState;
use crate::ui::nav::NavSystems;

use super::actors::{world_pos, Sim};

/// The green corner brackets. The editor's cursor is drawn from the same file:
/// both mean "this is the thing being pointed at".
const FRAME: &str = "selection.png";

/// How far in front of the selected character the frame is drawn.
///
/// [`depth_for`] steps by 0.01 per world pixel and a paperdoll's own layers
/// reach 0.003, so this sits over the hair of the character it is framing and
/// still behind anybody standing one pixel further down the screen.
const FRAME_LIFT: f32 = 0.004;

/// Who is selected, if anybody.
///
/// `Option<Uid>` and not an `Entity`: the selection outlives the sprite drawing
/// it — an actor culled off the canvas, or one whose sprite has not been
/// spawned yet, is still somebody you can be looking at.
#[derive(Resource, Default)]
pub struct Selected(Option<Uid>);

impl Selected {
    pub fn get(&self) -> Option<Uid> {
        self.0
    }

    /// Stop looking at anybody. What the panel's `close` button asks for.
    pub fn clear(&mut self) {
        self.0 = None;
    }

    /// Look at somebody named directly, rather than pointed at.
    ///
    /// For the QA harness's `select` step, which addresses the crowd by
    /// arrival order because a `Uid` is random and a test cannot know one in
    /// advance. The mouse path is [`pick`] and stays the thing under test;
    /// this is how a script gets *past* it to check what the panel then does.
    pub fn select(&mut self, uid: Uid) {
        self.0 = Some(uid);
    }

    /// Select somebody, or nobody.
    ///
    /// Takes the `ResMut` rather than `&mut self` so it can leave the resource
    /// unchanged when nothing changed: [`super::unitpanel`] rebuilds itself on
    /// `is_changed`, and a click that reselects the same entity must not throw
    /// away the menu that is open on them — nor must a click on empty ground
    /// when nobody was selected in the first place.
    fn set(this: &mut ResMut<Selected>, uid: Option<Uid>) {
        if this.0 != uid {
            this.0 = uid;
        }
    }
}

/// The sprite that frames whoever is selected.
#[derive(Component)]
struct SelectionFrame;

pub struct SelectionPlugin;

impl Plugin for SelectionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Selected>()
            .add_systems(OnEnter(AppState::Game), spawn_frame)
            .add_systems(OnExit(AppState::Game), clear)
            .add_systems(
                Update,
                (pick, forget_the_departed, follow_selection)
                    .chain()
                    .after(NavSystems)
                    .run_if(in_state(AppState::Game)),
            );
    }
}

/// Nobody is selected on the way out.
///
/// Cleared here rather than on entry for the same reason the log is: the world
/// is rebuilt from its seed each time the screen opens, so a `Uid` kept from
/// the last visit would name somebody who no longer exists — and would name
/// them convincingly, since ids are minted from a seeded RNG and the second
/// world hands out the same ones.
fn clear(mut selected: ResMut<Selected>) {
    Selected::set(&mut selected, None);
}

/// The frame exists for the life of the screen and is moved and hidden, rather
/// than being spawned and despawned with the selection: it is one sprite, and
/// a sprite that comes and goes is a frame of asset loading before it appears.
fn spawn_frame(mut commands: Commands, assets: Res<AssetServer>) {
    commands.spawn((
        Name::new("selection frame"),
        SelectionFrame,
        Sprite::from_image(assets.load(FRAME)),
        Transform::from_scale(upscale(ART_SCALE)),
        Visibility::Hidden,
        WORLD_LAYER,
        DespawnOnExit(AppState::Game),
    ));
}

/// A left click on the map selects whoever is standing in that cell, and a
/// click on empty ground selects nobody.
///
/// Clicks that landed on the interface are not clicks on the map: the panel
/// this feature puts at the bottom of the screen is large, and selecting the
/// crowd behind it every time somebody pressed `freeze` would be absurd. The
/// question is asked of `bevy_ui` rather than of the layout — any node under
/// the pointer reports [`Interaction::Hovered`], and the panels carry an
/// `Interaction` of their own so their backgrounds absorb a press as well as
/// their buttons do.
fn pick(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<&Transform, With<WorldCamera>>,
    zoom: Res<PixelZoom>,
    interactions: Query<&Interaction>,
    sim: Option<Res<Sim>>,
    mut selected: ResMut<Selected>,
) {
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    if interactions
        .iter()
        .any(|interaction| *interaction != Interaction::None)
    {
        return;
    }

    let (Ok(window), Ok(camera), Some(sim)) = (windows.single(), cameras.single(), sim) else {
        return;
    };
    let Some(world) = cursor_world_pos(window, camera.translation.truncate(), zoom.get()) else {
        return;
    };

    let cell = background::point_of(background::cell_of(world));
    Selected::set(&mut selected, sim.0.occupancy().occupant(cell));
}

/// Somebody who has left the world is nobody.
///
/// Despawning through the panel is the obvious way this happens; a future
/// entity that removes itself is the other, and neither is allowed to leave a
/// panel on screen describing an entity that is gone.
fn forget_the_departed(sim: Option<Res<Sim>>, mut selected: ResMut<Selected>) {
    let Some(uid) = selected.0 else {
        return;
    };
    let gone = match sim {
        Some(sim) => !sim.0.entities().contains(uid),
        // The world is torn down a frame before the screen changes.
        None => true,
    };
    if gone {
        Selected::set(&mut selected, None);
    }
}

/// Keep the frame on whoever is selected.
///
/// It follows the position the *renderer* put the sprite at, snapped the same
/// way, so the brackets sit exactly on the art rather than a texel off it once
/// a walker is between cells.
fn follow_selection(
    selected: Res<Selected>,
    sim: Option<Res<Sim>>,
    mut frames: Query<(&mut Transform, &mut Visibility), With<SelectionFrame>>,
) {
    let at = selected
        .0
        .zip(sim)
        .and_then(|(uid, sim)| sim.0.position_of(uid))
        .map(world_pos);

    for (mut transform, mut visibility) in &mut frames {
        let wanted = match at {
            Some(_) => Visibility::Visible,
            None => Visibility::Hidden,
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
        let Some(pos) = at else {
            continue;
        };
        let wanted = Vec3::new(pos.x, pos.y, depth_for(pos.y) + FRAME_LIFT);
        if transform.translation != wanted {
            transform.translation = wanted;
        }
    }
}
