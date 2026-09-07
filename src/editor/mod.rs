//! The map editor.
//!
//! Two layers, built on two deliberately different sprite functions because
//! they obey different rules:
//!
//! * [`background`] — one tile per grid cell, all at a single depth behind the
//!   world. Painted by dragging, since filling a floor is the common case.
//! * [`props`] — objects placed freely at the cursor and depth-sorted by world
//!   Y exactly like characters, so they interleave with the crowd. Placed one
//!   per click, since dragging would bury a pile of beds in one spot.
//!
//! Only the background keeps an index (a cell can hold one tile, so painting
//! has to find and replace it); props are just entities.
//!
//! The editor edits a [`CurrentMap`], not the screen: a click writes the tile
//! or the prop into the map and only then spawns a sprite for it, so what is
//! drawn can never be something the saved file does not contain. Entering the
//! editor rebuilds the scene from the map and leaving it despawns the scene
//! and writes the map back, which is why the map survives a trip to the menu
//! even though none of its sprites do.
//!
//! Painting is a pointing task, so it is driven by a [`Cursor`] that either
//! device can move: the mouse when it moves, the left stick when it does not.
//! Everything else — palette, layer, save, leave — has a button on both.

pub mod background;
pub mod props;

use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::window::{CursorMoved, PrimaryWindow};

use crate::browser::Maps;
use crate::characters::{upscale, ART_SCALE};
use crate::map::{Map, Size, VOID};
use crate::render::{cursor_world_pos, CameraPan, CameraTarget, PixelZoom, WorldCamera, WORLD_LAYER};
use crate::state::AppState;
use crate::ui::nav::{Cancelled, NavSystems};
use crate::ui::{FONT_BODY, PANEL, TEXT, TEXT_DIM};

/// Camera pan speed, in canvas pixels per second.
const PAN_SPEED: f32 = 80.0;
/// Cursor speed on a stick, in canvas pixels per second. Slower than the
/// camera: it is aiming at a 48px cell, not crossing the map.
const CURSOR_SPEED: f32 = 90.0;
/// How far a stick must be pushed before it counts as pushed at all.
const STICK_DEADZONE: f32 = 0.2;

/// Size of the map the editor falls back to when it was opened without one —
/// `CROWD2X_STATE=editor`, and nothing else.
const SCRATCH: (i32, i32) = (24, 16);

/// The cursor overlay sits above every prop and character. The HUD is
/// `bevy_ui` and composited over the whole canvas, so it needs no depth.
const CURSOR_Z: f32 = 100.0;

const KEY_HINTS: &str = "lmb/A place   rmb/X erase   q/e or bumpers item   tab/Y layer\n                         wasd or right stick pan   f5/start save   esc/B maps";

/// Corner brackets drawn at the cursor, at the 16px art size like the rest.
const SELECTION_FRAME: &str = "selection.png";
/// The about-to-be-placed sprite is drawn washed out under the frame.
const GHOST_TINT: Color = Color::srgba(1.0, 1.0, 1.0, 0.55);

/// One placeable entry in a layer's palette.
///
/// The imported art is a mix of two resolutions — some of it is drawn at the
/// 16x16 character size and some genuinely at 48x48 — so an entry has to say
/// which it is. Getting it wrong is loud rather than subtle: the sprite comes
/// out at a third of its size, or three times it.
pub struct PaletteItem {
    /// Shown in the HUD.
    pub name: &'static str,
    /// Path under `assets/`.
    pub path: &'static str,
    /// Canvas pixels per source texel.
    pub scale: f32,
}

impl PaletteItem {
    /// Art already drawn at cell resolution, placed one texel per pixel.
    pub const fn new(name: &'static str, path: &'static str) -> Self {
        Self {
            name,
            path,
            scale: 1.0,
        }
    }

    /// Art drawn at [`characters::ART`] resolution, upscaled to fill a cell.
    pub const fn upscaled(name: &'static str, path: &'static str) -> Self {
        Self {
            name,
            path,
            scale: ART_SCALE,
        }
    }
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    #[default]
    Background,
    Props,
}

impl Layer {
    fn name(self) -> &'static str {
        match self {
            Layer::Background => "background",
            Layer::Props => "props",
        }
    }

    fn other(self) -> Self {
        match self {
            Layer::Background => Layer::Props,
            Layer::Props => Layer::Background,
        }
    }
}

/// What the editor will place next. Each layer keeps its own selection, so
/// switching layers does not lose your place in the other palette.
#[derive(Resource, Default)]
pub struct Tool {
    layer: Layer,
    background: usize,
    props: usize,
}

impl Tool {
    fn palette(&self) -> &'static [PaletteItem] {
        match self.layer {
            Layer::Background => background::PALETTE,
            Layer::Props => props::PALETTE,
        }
    }

    fn index(&self) -> usize {
        match self.layer {
            Layer::Background => self.background,
            Layer::Props => self.props,
        }
    }

    fn item(&self) -> &'static PaletteItem {
        &self.palette()[self.index()]
    }

    fn cycle(&mut self, step: i32) {
        let len = self.palette().len() as i32;
        let index = match self.layer {
            Layer::Background => &mut self.background,
            Layer::Props => &mut self.props,
        };
        *index = (*index as i32 + step).rem_euclid(len) as usize;
    }

    /// Select the palette entry with this name, whichever layer it is on.
    ///
    /// The name is the one shown in the HUD, so a test says `"wall brown"`
    /// instead of pressing `e` nine times and hoping the palette order has not
    /// changed since it was written.
    pub fn select(&mut self, name: &str) -> bool {
        if let Some(index) = background::PALETTE.iter().position(|item| item.name == name) {
            self.layer = Layer::Background;
            self.background = index;
            return true;
        }
        if let Some(index) = props::PALETTE.iter().position(|item| item.name == name) {
            self.layer = Layer::Props;
            self.props = index;
            return true;
        }
        false
    }

    fn describe(&self) -> String {
        format!(
            "layer  {}\nitem   {}  {}/{}",
            self.layer.name(),
            self.item().name,
            self.index() + 1,
            self.palette().len(),
        )
    }
}

/// Root of the cursor overlay; the ghost and the frame are its children so a
/// single transform moves both.
#[derive(Component)]
struct EditorCursor;

/// Washed-out preview of the sprite about to be placed.
#[derive(Component)]
struct Ghost;

#[derive(Component)]
struct Hud;

/// The map being edited, and the file it will be written back to.
#[derive(Resource)]
pub struct CurrentMap {
    /// `None` for the scratch map the editor falls back to when it was opened
    /// without going through the browser. It is deliberately not saved: a
    /// debug capture that boots into the editor must not leave a map file
    /// behind, and a map nobody named is one nobody asked to keep.
    pub name: Option<String>,
    pub map: Map,
}

impl CurrentMap {
    pub fn new(name: String, map: Map) -> Self {
        Self {
            name: Some(name),
            map,
        }
    }

    fn scratch() -> Self {
        Self {
            name: None,
            map: Map::new(Size::new(SCRATCH.0, SCRATCH.1), VOID),
        }
    }

    pub fn title(&self) -> &str {
        self.name.as_deref().unwrap_or("scratch  (not saved)")
    }
}

/// Where the next placement lands, in world pixels.
///
/// One cursor, whichever device is driving it: the mouse sets it directly when
/// it moves, and the left stick nudges it when the mouse is still. Keeping it
/// in a resource rather than reading the mouse at each call site is what makes
/// a gamepad able to paint at all — the alternative is every editing system
/// growing its own copy of that fallback.
#[derive(Resource, Default)]
pub struct Cursor {
    world: Option<Vec2>,
}

impl Cursor {
    /// Put the cursor somewhere directly.
    ///
    /// For the QA harness, which asks for "the middle of cell (3, 2)" rather
    /// than working out where that is on screen — a test that had to know the
    /// window size and the camera position would be testing arithmetic.
    pub fn place(&mut self, world: Vec2) {
        self.world = Some(world);
    }
}

pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Tool>()
            .init_resource::<Cursor>()
            .init_resource::<background::Tiles>()
            // Replaced by the browser when a real map is opened; this is only
            // what `CROWD2X_STATE=editor` lands in.
            .insert_resource(CurrentMap::scratch())
            .add_systems(
                OnEnter(AppState::Editor),
                (spawn_overlay, build_scene, aim_camera_at_map),
            )
            .add_systems(OnExit(AppState::Editor), leave_editor)
            // Saving has to survive the window being closed, which ends the
            // app without ever running `OnExit`.
            .add_systems(Last, save_before_exit)
            .add_systems(
                Update,
                (
                    track_cursor,
                    pan_camera,
                    switch_layer,
                    cycle_item,
                    edit,
                    update_cursor,
                    update_hud,
                    save_on_demand,
                    leave,
                )
                    .chain()
                    .after(NavSystems)
                    .run_if(in_state(AppState::Editor)),
            );
    }
}

/// Draw a map's terrain and props into the world, for any screen that shows
/// one.
///
/// The art catalogue is the two palettes in this module: a tile's name is
/// bound to its PNG next to the code that paints it. The game screen draws the
/// same map from the same catalogue, so it comes through here rather than
/// growing a second one that could drift out of step. `state` is whose sprites
/// these are, so each screen takes its own away on the way out.
pub fn draw_map(commands: &mut Commands, assets: &AssetServer, map: &Map, state: AppState) {
    background::spawn_map(commands, assets, map, state, None);
    props::spawn_map(commands, assets, map, state);
}

/// Draw the open map. The scene is rebuilt on every entry and thrown away on
/// every exit, so the map — not the entities — is the thing that persists.
fn build_scene(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut tiles: ResMut<background::Tiles>,
    current: Res<CurrentMap>,
) {
    // The editor keeps the tile index the game has no use for: painting has to
    // find the sprite already in a cell to replace it.
    background::spawn_map(
        &mut commands,
        &assets,
        &current.map,
        AppState::Editor,
        Some(&mut tiles),
    );
    props::spawn_map(&mut commands, &assets, &current.map, AppState::Editor);
}

/// Aim at the middle of the map rather than at its bottom-left corner, which
/// on a fresh map is a screen of nothing.
///
/// Asked for rather than done, because `OnEnter` for the *initial* state runs
/// before the camera exists — see [`CameraTarget`].
fn aim_camera_at_map(current: Res<CurrentMap>, mut target: ResMut<CameraTarget>) {
    target.0 = Some(map_centre(&current.map));
}

/// The middle of a map, in world pixels.
pub fn map_centre(map: &Map) -> Vec2 {
    background::map_extent(map) / 2.0
}

/// Leaving the editor saves. There is no unsaved-changes dialog because there
/// is nothing to decide: every edit is already in the map, and writing it out
/// is cheap.
fn leave_editor(
    maps: Res<Maps>,
    current: Res<CurrentMap>,
    mut tiles: ResMut<background::Tiles>,
    mut cursor: ResMut<Cursor>,
) {
    save(&maps, &current);
    // The sprites go with `DespawnOnExit`; the index of them must not outlive
    // them or the next entry would think cells are already painted.
    tiles.clear();
    cursor.world = None;
}

fn save(maps: &Maps, current: &CurrentMap) {
    let Some(name) = &current.name else {
        return;
    };
    match maps.0.save(name, &current.map) {
        Ok(()) => info!("editor: saved {name}"),
        Err(error) => error!("editor: could not save {name}: {error}"),
    }
}

/// Closing the window ends the app without a state transition, so `OnExit`
/// never runs. This is the same save, hung off the exit itself.
fn save_before_exit(
    exit: MessageReader<AppExit>,
    state: Res<State<AppState>>,
    maps: Res<Maps>,
    current: Res<CurrentMap>,
) {
    if exit.is_empty() || *state.get() != AppState::Editor {
        return;
    }
    save(&maps, &current);
}

fn save_on_demand(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    maps: Res<Maps>,
    current: Res<CurrentMap>,
) {
    let asked = keys.just_pressed(KeyCode::F5)
        || gamepads
            .iter()
            .any(|pad| pad.just_pressed(GamepadButton::Start));
    if asked {
        save(&maps, &current);
    }
}

fn spawn_overlay(mut commands: Commands, assets: Res<AssetServer>, tool: Res<Tool>) {
    commands
        .spawn((
            Name::new("editor cursor"),
            EditorCursor,
            Transform::from_xyz(0.0, 0.0, CURSOR_Z),
            // Shown once the cursor is known to be over the window.
            Visibility::Hidden,
            DespawnOnExit(AppState::Editor),
        ))
        .with_children(|parent| {
            parent.spawn((
                Ghost,
                Sprite {
                    image: assets.load(tool.item().path),
                    color: GHOST_TINT,
                    ..default()
                },
                Transform::from_scale(upscale(tool.item().scale)),
                WORLD_LAYER,
            ));
            parent.spawn((
                Sprite::from_image(assets.load(SELECTION_FRAME)),
                Transform::from_xyz(0.0, 0.0, 0.5).with_scale(upscale(ART_SCALE)),
                WORLD_LAYER,
            ));
        });

    // The panel sizes itself to the text, so nothing here has to guess how
    // wide the longest palette name is.
    commands.spawn((
        Name::new("editor hud"),
        Node {
            position_type: PositionType::Absolute,
            top: px(4),
            left: px(4),
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(px(4), px(3)),
            row_gap: px(3),
            ..default()
        },
        BackgroundColor(PANEL),
        DespawnOnExit(AppState::Editor),
        children![
            (
                Hud,
                Text::new(String::new()),
                TextFont::from_font_size(FONT_BODY),
                TextColor(TEXT),
            ),
            (
                Text::new(KEY_HINTS),
                TextFont::from_font_size(FONT_BODY),
                TextColor(TEXT_DIM),
            ),
        ],
    ));
}

/// Follow whichever device is pointing.
///
/// The mouse wins while it is moving; the stick takes over when it stops, so
/// the two never fight over the cursor and neither has to be "switched on".
fn track_cursor(
    time: Res<Time>,
    mut moved: MessageReader<CursorMoved>,
    gamepads: Query<&Gamepad>,
    zoom: Res<PixelZoom>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<&Transform, With<WorldCamera>>,
    mut cursor: ResMut<Cursor>,
) {
    let (Ok(window), Ok(camera)) = (windows.single(), cameras.single()) else {
        return;
    };
    let camera = camera.translation.truncate();

    let mouse_moved = moved.read().count() > 0;
    if (mouse_moved || cursor.world.is_none())
        && let Some(world) = cursor_world_pos(window, camera, zoom.get())
    {
        cursor.world = Some(world);
    }

    let stick = gamepads
        .iter()
        .map(|pad| pad.left_stick())
        .find(|stick| stick.length() > STICK_DEADZONE);
    if let Some(stick) = stick {
        let from = cursor.world.unwrap_or(camera);
        let moved_to = from + stick * CURSOR_SPEED * time.delta_secs();
        // Kept on screen: a cursor pushed off the canvas would be painting
        // where nobody can see it.
        let half =
            crate::render::half_view(window, zoom.get()) - Vec2::splat(background::TILE / 2.0);
        cursor.world = Some(moved_to.clamp(camera - half, camera + half));
    }

    if cursor.world.is_none() {
        cursor.world = Some(camera);
    }
}

/// WASD / arrows and the right stick pan the camera.
///
/// This writes `CameraPan` and never the transform: `render` owns the transform
/// and keeps it snapped to whole pixels.
fn pan_camera(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    time: Res<Time>,
    mut cameras: Query<&mut CameraPan, With<WorldCamera>>,
) {
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
        let stick = pad.right_stick();
        if stick.length() > STICK_DEADZONE {
            dir += stick;
        }
    }
    if dir == Vec2::ZERO {
        return;
    }

    for mut pan in &mut cameras {
        pan.0 += dir.normalize() * PAN_SPEED * time.delta_secs();
    }
}

fn switch_layer(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut tool: ResMut<Tool>,
) {
    let toggled = keys.just_pressed(KeyCode::Tab)
        || gamepads
            .iter()
            .any(|pad| pad.just_pressed(GamepadButton::North));
    if toggled {
        tool.layer = tool.layer.other();
    } else if keys.just_pressed(KeyCode::Digit1) {
        tool.layer = Layer::Background;
    } else if keys.just_pressed(KeyCode::Digit2) {
        tool.layer = Layer::Props;
    }
}

fn cycle_item(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut wheel: MessageReader<MouseWheel>,
    mut tool: ResMut<Tool>,
) {
    let mut step = 0;
    if keys.any_just_pressed([KeyCode::KeyE, KeyCode::BracketRight]) {
        step += 1;
    }
    if keys.any_just_pressed([KeyCode::KeyQ, KeyCode::BracketLeft]) {
        step -= 1;
    }
    for scroll in wheel.read() {
        // `signum` would turn a zero-delta event into a step.
        step += (scroll.y > 0.0) as i32 - (scroll.y < 0.0) as i32;
    }
    for pad in &gamepads {
        step += pad.just_pressed(GamepadButton::RightTrigger) as i32;
        step -= pad.just_pressed(GamepadButton::LeftTrigger) as i32;
    }

    if step != 0 {
        tool.cycle(step);
    }
}

// Systems take their dependencies as parameters; the lint is counting a Bevy
// signature as if it were a function call site.
#[allow(clippy::too_many_arguments)]
fn edit(
    mut commands: Commands,
    assets: Res<AssetServer>,
    buttons: Res<ButtonInput<MouseButton>>,
    gamepads: Query<&Gamepad>,
    tool: Res<Tool>,
    cursor: Res<Cursor>,
    mut tiles: ResMut<background::Tiles>,
    mut current: ResMut<CurrentMap>,
    placed: Query<(Entity, &props::Prop, &Transform)>,
) {
    let Some(world) = cursor.world else {
        return;
    };

    let pads = || gamepads.iter();
    let place_held =
        buttons.pressed(MouseButton::Left) || pads().any(|pad| pad.pressed(GamepadButton::South));
    let erase_held =
        buttons.pressed(MouseButton::Right) || pads().any(|pad| pad.pressed(GamepadButton::West));
    let place_once = buttons.just_pressed(MouseButton::Left)
        || pads().any(|pad| pad.just_pressed(GamepadButton::South));
    let erase_once = buttons.just_pressed(MouseButton::Right)
        || pads().any(|pad| pad.just_pressed(GamepadButton::West));

    match tool.layer {
        // Held rather than clicked: floors are painted by dragging over cells.
        Layer::Background => {
            let cell = background::cell_of(world);
            if place_held {
                background::paint(
                    &mut commands,
                    &assets,
                    &mut tiles,
                    &mut current.map,
                    cell,
                    tool.background,
                );
            } else if erase_held {
                background::erase(&mut commands, &mut tiles, &mut current.map, cell);
            }
        }
        // One press, one prop.
        Layer::Props => {
            if place_once {
                props::place(&mut commands, &assets, &mut current.map, world, tool.props);
            } else if erase_once {
                props::erase_nearest(&mut commands, &mut current.map, &placed, world);
            }
        }
    }
}

/// Move the ghost and frame to the cursor, snapping the way the active layer
/// places things so the preview cannot lie about where a press will land.
fn update_cursor(
    assets: Res<AssetServer>,
    tool: Res<Tool>,
    cursor: Res<Cursor>,
    mut cursors: Query<(&mut Transform, &mut Visibility), With<EditorCursor>>,
    mut ghosts: Query<(&mut Sprite, &mut Transform), (With<Ghost>, Without<EditorCursor>)>,
) {
    if tool.is_changed() {
        for (mut sprite, mut transform) in &mut ghosts {
            sprite.image = assets.load(tool.item().path);
            // Switching between a 16px and a 48px item changes the scale too.
            transform.scale = upscale(tool.item().scale);
        }
    }

    for (mut transform, mut visibility) in &mut cursors {
        let Some(pos) = cursor.world else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let snapped = match tool.layer {
            Layer::Background => background::cell_centre(background::cell_of(pos)),
            Layer::Props => pos.round(),
        };
        transform.translation.x = snapped.x;
        transform.translation.y = snapped.y;
        *visibility = Visibility::Visible;
    }
}

fn update_hud(
    tool: Res<Tool>,
    current: Res<CurrentMap>,
    mut hud: Query<&mut Text, With<Hud>>,
) {
    if !tool.is_changed() && !current.is_changed() {
        return;
    }
    let size = current.map.size();
    for mut text in &mut hud {
        **text = format!(
            "map    {}  {}x{}\n{}",
            current.title(),
            size.width,
            size.height,
            tool.describe()
        );
    }
}

/// Escape or B goes back to the browser, which is where a map is chosen.
fn leave(mut cancelled: MessageReader<Cancelled>, mut next: ResMut<NextState<AppState>>) {
    if cancelled.read().count() > 0 {
        next.set(AppState::Maps);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::characters::{png_size, CELL};

    /// Every palette entry declares whether its art is cell-sized or drawn at
    /// the smaller art resolution, and nothing in the running game re-checks
    /// it — a wrong flag just draws the sprite at a third or triple its size.
    #[test]
    fn every_palette_item_is_one_cell_wide_once_scaled() {
        for item in background::PALETTE.iter().chain(props::PALETTE) {
            let (width, _) = png_size(&format!("assets/{}", item.path));
            assert_eq!(
                width as f32 * item.scale,
                CELL as f32,
                "{}: {} is {width}px at scale {}",
                item.name,
                item.path,
                item.scale,
            );
        }
    }
}
