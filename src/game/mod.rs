//! The game screen: a map, a simulation running on it, and a camera to look
//! around with.
//!
//! The simulation itself is not here. It is [`crate::sim`], plain Rust with no
//! `bevy::` imports at all, and [`actors`] is the whole of the bridge: it
//! builds a `GameState` from the open map, steps it as fast as [`speed`] says
//! to, and keeps a sprite alongside each entity. [`hud`] is the two corners of
//! controls over the map, and [`logview`] drains what the simulation had to
//! say into the panel under them.
//!
//! One thing on this screen is about a single member of the crowd rather than
//! about the whole world: [`selection`] is the click that picks somebody and
//! the green frame that says who, and [`unitpanel`] is the bar along the
//! bottom — their portrait, and the menus that inspect them or act on them.
//! Acting on them is still a [`crate::sim::Command`] on the same queue
//! everything else uses; the panel decides nothing either.
//!
//! Three things this screen deliberately does *not* do:
//!
//! * **It does not own the map's art.** Terrain and props are drawn through
//!   [`editor::draw_map`], from the same palettes the editor paints with, so
//!   there is one catalogue binding a tile's name to its PNG rather than two
//!   that can drift apart.
//! * **It does not edit anything.** The map is read and never written — the
//!   simulation gets a clone — which is why leaving is instant and there is
//!   nothing to save.
//! * **It does not zoom the camera.** Zooming is [`PixelZoom`], a whole-number
//!   change to the size of the canvas the world is drawn into, so a sprite is
//!   on exact pixel blocks at every zoom level rather than only at the default
//!   one. See `render`.
//!
//! The camera is kept inside the map. A map has edges and nothing outside
//! them, so flying off into the void is not navigation, it is getting lost —
//! and on a map smaller than the window there is nothing to scroll to at all,
//! so that axis is centred instead.
//!
//! # Controls
//!
//! Move with `WASD`, the arrow keys, the left stick or the d-pad; zoom with
//! `q` / `e` or `A` / `B`; change speed with `+` / `-` or the bumpers and
//! pause with `p` or `Y`; `esc` or `start` goes back to the browser. Every one
//! of those also has a button in a corner ([`hud`]).
//!
//! `B` is the one departure from the convention the rest of the game follows,
//! where `esc`/`B` means back ([`ui::nav::Cancelled`]). Here `B` is zoom out,
//! so this screen reads the two leave buttons directly instead of listening
//! for a cancel it would otherwise fire on every zoom.

pub mod actors;
pub mod hud;
pub mod logview;
pub mod selection;
pub mod speed;
pub mod unitpanel;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::editor::{background, draw_map, map_centre, CurrentMap};
use crate::render::{half_view, CameraPan, CameraTarget, PixelZoom, PIXEL_SCALE, WorldCamera};
use crate::state::AppState;
use crate::ui::nav::{NavSystems, Scope};

/// Camera speed at the default zoom, in canvas pixels per second.
///
/// Scaled by the zoom in [`pan_speed`] so that it is the *screen* that moves at
/// a constant rate: a fixed world speed crawls when zoomed out and flies when
/// zoomed in, and both feel like the controls changed under you.
const PAN_SPEED: f32 = 140.0;

/// How far a stick must be pushed before it counts as pushed at all.
const STICK_DEADZONE: f32 = 0.2;

pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            actors::ActorsPlugin,
            speed::SpeedPlugin,
            hud::HudPlugin,
            logview::LogViewPlugin,
            selection::SelectionPlugin,
            unitpanel::UnitPanelPlugin,
        ))
            .add_systems(OnEnter(AppState::Game), (build_scene, aim_camera_at_map))
            .add_systems(OnExit(AppState::Game), restore_zoom)
            .add_systems(
                Update,
                (move_camera, change_zoom, leave)
                    .chain()
                    .after(NavSystems)
                    .run_if(in_state(AppState::Game)),
            );
    }
}

/// Put the zoom back on the way out.
///
/// This screen is the only one with a zoom control, so a zoom left behind
/// would strand the editor and the menus at it with no way to undo. Resetting
/// on the way *out* rather than on the way in is what lets the debug harness
/// boot straight into a chosen zoom (`CROWD2X_ZOOM`) and still photograph it.
fn restore_zoom(mut zoom: ResMut<PixelZoom>) {
    zoom.reset();
}

/// Draw the open map. What goes over it is [`hud`]'s.
///
/// The scene is built on entry and despawned by `DespawnOnExit` on the way
/// out, exactly as the editor's is: the map persists, its entities do not.
fn build_scene(mut commands: Commands, assets: Res<AssetServer>, current: Res<CurrentMap>) {
    draw_map(&mut commands, &assets, &current.map, AppState::Game);
}

/// Start looking at the middle of the map rather than at its bottom-left
/// corner. Asked for rather than done, because booting straight into this
/// screen reaches here before the camera exists — see [`CameraTarget`].
fn aim_camera_at_map(current: Res<CurrentMap>, mut target: ResMut<CameraTarget>) {
    target.0 = Some(map_centre(&current.map));
}

/// Speed the camera should move at, in world units per second.
///
/// Inversely proportional to the zoom, so crossing the window takes the same
/// time however far in the view is.
fn pan_speed(zoom: PixelZoom) -> f32 {
    PAN_SPEED * PIXEL_SCALE as f32 / zoom.factor()
}

/// Where the camera is allowed to be, on one axis.
///
/// `centre` is the camera, `half` how much of the world it can see either side
/// of itself, and `extent` the size of the map. An axis with less map than
/// view is centred: there is nothing to scroll to, and pinning a small map to
/// one edge of the window looks like a bug rather than a decision.
fn clamp_axis(centre: f32, half: f32, extent: f32) -> f32 {
    if half * 2.0 >= extent {
        extent / 2.0
    } else {
        centre.clamp(half, extent - half)
    }
}

fn clamp_to_map(centre: Vec2, half: Vec2, extent: Vec2) -> Vec2 {
    Vec2::new(
        clamp_axis(centre.x, half.x, extent.x),
        clamp_axis(centre.y, half.y, extent.y),
    )
}

/// WASD, the arrows, the d-pad and the left stick all move the camera; the
/// clamp then keeps it over the map.
///
/// One system rather than a move and a separate clamp, because the clamp has
/// to run after a zoom as well as after a move: zooming out at the edge of the
/// map widens the view past it, and the camera has to come back in.
///
/// This writes [`CameraPan`] and never the transform — `render` owns that and
/// keeps it snapped to whole pixels.
fn move_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    zoom: Res<PixelZoom>,
    current: Res<CurrentMap>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<&mut CameraPan, With<WorldCamera>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };

    let mut dir = Vec2::ZERO;
    if keys.any_pressed([KeyCode::KeyW, KeyCode::ArrowUp]) {
        dir.y += 1.0;
    }
    if keys.any_pressed([KeyCode::KeyS, KeyCode::ArrowDown]) {
        dir.y -= 1.0;
    }
    if keys.any_pressed([KeyCode::KeyA, KeyCode::ArrowLeft]) {
        dir.x -= 1.0;
    }
    if keys.any_pressed([KeyCode::KeyD, KeyCode::ArrowRight]) {
        dir.x += 1.0;
    }
    for pad in &gamepads {
        let stick = pad.left_stick();
        if stick.length() > STICK_DEADZONE {
            dir += stick;
        }
        if pad.pressed(GamepadButton::DPadUp) {
            dir.y += 1.0;
        }
        if pad.pressed(GamepadButton::DPadDown) {
            dir.y -= 1.0;
        }
        if pad.pressed(GamepadButton::DPadLeft) {
            dir.x -= 1.0;
        }
        if pad.pressed(GamepadButton::DPadRight) {
            dir.x += 1.0;
        }
    }

    // Capped rather than normalised: a stick pushed halfway should move the
    // camera at half speed, while two keys held diagonally must not add up to
    // 1.41 times the speed of one.
    let velocity = dir.clamp_length_max(1.0) * pan_speed(*zoom) * time.delta_secs();
    let half = half_view(window, zoom.get());
    let extent = background::map_extent(&current.map);

    for mut pan in &mut cameras {
        let wanted = clamp_to_map(pan.0 + velocity, half, extent);
        if wanted != pan.0 {
            pan.0 = wanted;
        }
    }
}

/// `q` / `e` and the two face buttons zoom, one whole step at a time.
///
/// `+` and `-` used to zoom too and are the speed's now (see [`speed`]): the
/// zoom already had two bindings on each device and a button in the corner,
/// and a keyboard with no way to pause was the gap worth filling.
fn change_zoom(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut zoom: ResMut<PixelZoom>,
) {
    let mut step = 0;
    if keys.just_pressed(KeyCode::KeyE) {
        step += 1;
    }
    if keys.just_pressed(KeyCode::KeyQ) {
        step -= 1;
    }
    for pad in &gamepads {
        step += pad.just_pressed(GamepadButton::South) as i32;
        step -= pad.just_pressed(GamepadButton::East) as i32;
    }

    // Guarded, and not merely because there is nothing to do: taking
    // `ResMut` mutably marks the resource changed, and a changed zoom is what
    // makes `render` rebuild the canvas.
    if step != 0 {
        zoom.step(step);
    }
}

/// Back to the browser, which is where another map is chosen — or, if the
/// spawn menu is open, back to the game: escape closes a dialog before it
/// closes the screen it is over, exactly as it does in the browser.
///
/// Read from the devices rather than from `Cancelled`, because `B` is the
/// zoom-out button on this screen and nav writes a cancel for it, and because
/// this is the one system that gets to decide which of the two escape does —
/// splitting that across two systems reading the same press is a race.
fn leave(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut menu: ResMut<hud::SpawnMenu>,
    mut scope: ResMut<Scope>,
    mut next: ResMut<NextState<AppState>>,
) {
    let asked = keys.just_pressed(KeyCode::Escape)
        || gamepads.iter().any(|pad| {
            pad.just_pressed(GamepadButton::Start) || pad.just_pressed(GamepadButton::Select)
        });
    if !asked {
        return;
    }
    if menu.0.is_some() {
        hud::close_spawn_menu(&mut commands, &mut menu, &mut scope);
    } else {
        next.set(AppState::Maps);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1280x720 window at the default zoom sees 320x180 world units, so a
    /// map has to be bigger than that before there is anything to pan to.
    const HALF: Vec2 = Vec2::new(160.0, 90.0);

    #[test]
    fn the_view_stops_at_the_edges_of_the_map() {
        let extent = Vec2::new(1152.0, 768.0);
        // Pushed left past the edge: the left of the view lands on x = 0.
        assert_eq!(clamp_to_map(Vec2::new(-500.0, 400.0), HALF, extent).x, 160.0);
        // ...and the same on the right.
        assert_eq!(
            clamp_to_map(Vec2::new(9000.0, 400.0), HALF, extent).x,
            1152.0 - 160.0
        );
        // Somewhere in the middle is left alone.
        let inside = Vec2::new(500.0, 400.0);
        assert_eq!(clamp_to_map(inside, HALF, extent), inside);
    }

    #[test]
    fn a_map_smaller_than_the_view_is_centred_on_that_axis() {
        // Wide enough to pan across, but only three cells tall.
        let extent = Vec2::new(1152.0, 144.0);
        let camera = clamp_to_map(Vec2::new(600.0, 9000.0), HALF, extent);
        assert_eq!(camera.y, 72.0);
        // The axis with room to move still moves.
        assert_eq!(camera.x, 600.0);
    }

    #[test]
    fn a_map_exactly_the_size_of_the_view_does_not_jitter_at_the_edges() {
        // The two branches meet here; both answer "the middle", so there is no
        // zoom level at which the camera flips between two positions.
        let extent = HALF * 2.0;
        assert_eq!(clamp_to_map(Vec2::ZERO, HALF, extent), HALF);
        assert_eq!(clamp_to_map(extent, HALF, extent), HALF);
    }

    /// Crossing the screen should take the same time at every zoom, or the
    /// controls feel like they changed speed when only the view did.
    #[test]
    fn panning_covers_the_same_screen_distance_at_every_zoom() {
        let mut zoom = PixelZoom::default();
        let at_default = pan_speed(zoom);
        assert_eq!(at_default, PAN_SPEED);

        zoom.step(PIXEL_SCALE as i32);
        assert_eq!(zoom.get(), PIXEL_SCALE * 2);
        // Twice as close, so half as far through the world per second.
        assert_eq!(pan_speed(zoom), PAN_SPEED / 2.0);
    }
}
