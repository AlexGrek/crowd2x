//! The map editor.
//!
//! Two layers, built on two deliberately different sprite functions because
//! they obey different rules:
//!
//! * [`background`] — one tile per grid cell, all at a single depth behind the
//!   world. Painted by dragging, since filling a floor is the common case.
//!   `wall*` and `block` are the exception: [`Instrument::Wall`] and
//!   [`Instrument::Block`] turn a drag into a rectangle — hollow for a wall,
//!   filled for a block — committed on release rather than as it is dragged,
//!   since only the release knows where the far corner landed.
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
use crate::characters::{upscale, ART_SCALE, CELL};
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

/// One strip of art: a PNG holding `frames` square frames side by side.
///
/// One frame is a still picture, which is what most of the palette is.
#[derive(Clone, Copy)]
pub struct Strip {
    /// Path under `assets/`.
    pub path: &'static str,
    /// How many frames are in it, left to right.
    pub frames: u32,
}

/// One placeable entry in a layer's palette.
///
/// Art is drawn at [`characters::ART`] and upscaled by the game, so all but a
/// handful of entries are [`upscaled`](Self::upscaled). The exceptions are
/// imported props and two floors that are *genuinely* drawn at 48x48 and
/// cannot be reduced without mangling them (`tools/art_scale.py` refuses); they
/// are art debt to redraw at 16x16, not a second supported resolution. Until
/// they are, an entry has to say which it is, and getting it wrong is loud
/// rather than subtle: the sprite comes out at a third of its size, or three
/// times it.
pub struct PaletteItem {
    /// Shown in the HUD.
    pub name: &'static str,
    /// The art, and how many frames of it there are — see [`Strip`].
    pub art: Strip,
    /// Canvas pixels per source texel.
    pub scale: f32,
    /// What it looks like while somebody is *using* it — a computer with its
    /// screen on. `None` for everything that looks the same either way, which
    /// is every entry but one.
    ///
    /// Here rather than in the game screen that shows it, because a prop's
    /// pictures are all one catalogue: two lists keyed by the same name are
    /// two lists that can drift apart, and the editor's is the one the map
    /// file's names come from. The editor simply never has a use for it —
    /// nothing is in use where nothing is simulated.
    pub in_use: Option<Strip>,
}

impl PaletteItem {
    /// Art already drawn at cell resolution, placed one texel per pixel.
    pub const fn new(name: &'static str, path: &'static str) -> Self {
        Self {
            name,
            art: Strip { path, frames: 1 },
            scale: 1.0,
            in_use: None,
        }
    }

    /// Art drawn at [`characters::ART`] resolution, upscaled to fill a cell.
    pub const fn upscaled(name: &'static str, path: &'static str) -> Self {
        Self {
            name,
            art: Strip { path, frames: 1 },
            scale: ART_SCALE,
            in_use: None,
        }
    }

    /// Art that moves: `frames` frames of [`characters::ART`] side by side in
    /// one PNG, upscaled to fill a cell like every other piece of new art.
    pub const fn animated(name: &'static str, path: &'static str, frames: u32) -> Self {
        Self {
            name,
            art: Strip { path, frames },
            scale: ART_SCALE,
            in_use: None,
        }
    }

    /// ...and what it looks like while it is being used. See
    /// [`PaletteItem::in_use`].
    pub const fn used(mut self, path: &'static str, frames: u32) -> Self {
        self.in_use = Some(Strip { path, frames });
        self
    }

    /// The side of one frame of this item's art, in source texels: 16 for art
    /// the game upscales, 48 for art already drawn at cell resolution.
    ///
    /// Derived from the scale rather than stored, since a palette entry that
    /// is one cell wide once scaled is what
    /// `every_palette_item_is_one_cell_wide_once_scaled` already insists on.
    pub fn frame(&self) -> f32 {
        CELL as f32 / self.scale
    }

    /// The region of the art a still preview of this item should show:
    /// `None` for a still picture — the whole PNG — and the first frame for a
    /// strip, which drawn whole would be every frame of the animation side by
    /// side, ten cells wide.
    pub fn first_frame(&self) -> Option<Rect> {
        (self.art.frames > 1).then(|| Rect::new(0.0, 0.0, self.frame(), self.frame()))
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

/// How a background palette entry responds to a drag, chosen by its name so
/// picking the tile is the only choice a player makes.
///
/// `Wall` and `Block` both hold their paint until release rather than
/// painting every cell crossed like [`Brush`](Instrument::Brush) does — a
/// rectangle is only known once both corners are, and painting eagerly would
/// leave a trail behind wherever the drag passed on its way there.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Instrument {
    /// Paints every cell held over, the way the rest of the palette works.
    Brush,
    /// Drag corner to corner; on release, paints the rectangle's four sides —
    /// a room's walls in one stroke.
    Wall,
    /// Drag corner to corner; on release, paints the whole rectangle — a
    /// solid block of tiles in one stroke.
    Block,
}

impl Instrument {
    /// `"block"` and anything named `"wall..."` get the rectangle behaviour;
    /// every other background tile keeps painting by the cell.
    fn for_background_item(name: &str) -> Self {
        if name == "block" {
            Instrument::Block
        } else if name.starts_with("wall") {
            Instrument::Wall
        } else {
            Instrument::Brush
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
        let hint = match self.layer {
            Layer::Background => match Instrument::for_background_item(self.item().name) {
                Instrument::Wall => "  drag corner to corner for a room's walls",
                Instrument::Block => "  drag corner to corner for a filled block",
                Instrument::Brush => "",
            },
            Layer::Props => "",
        };
        format!(
            "layer  {}\nitem   {}  {}/{}{}",
            self.layer.name(),
            self.item().name,
            self.index() + 1,
            self.palette().len(),
            hint,
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

/// A [`Ghost`] but not the cursor it is a sibling of — they share a parent, so
/// a query for one has to say it does not mean the other.
type GhostQuery<'w> = (&'w mut Sprite, &'w mut Transform);
type GhostFilter = (With<Ghost>, Without<EditorCursor>);

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

/// An in-progress [`Instrument::Wall`] or [`Instrument::Block`] drag.
///
/// `anchor` is the cell the press started in; `None` means no drag is under
/// way. Nothing here touches the map until release — `cells` and `preview`
/// are only the on-screen rehearsal of what release will do, mirroring the
/// single-tile ghost the cursor always shows.
#[derive(Resource, Default)]
struct RectangleDrag {
    anchor: Option<IVec2>,
    /// Placing or erasing, fixed for the life of the drag so letting go of
    /// the wrong button mid-drag can't flip what release does.
    erasing: bool,
    /// The background palette entry this drag paints, captured at the press
    /// so cycling the palette mid-drag can't change what release paints.
    item: usize,
    /// What the last frame's preview covered, so a still drag does not
    /// respawn the same sprites every frame.
    cells: Vec<IVec2>,
    preview: Vec<Entity>,
}

impl RectangleDrag {
    /// Every cell from `anchor` to `current`: the whole rectangle for
    /// [`Instrument::Block`], only its four sides for [`Instrument::Wall`].
    fn cells(anchor: IVec2, current: IVec2, instrument: Instrument) -> Vec<IVec2> {
        let min = anchor.min(current);
        let max = anchor.max(current);
        let mut cells = Vec::new();
        for y in min.y..=max.y {
            for x in min.x..=max.x {
                let border = x == min.x || x == max.x || y == min.y || y == max.y;
                if border || instrument == Instrument::Block {
                    cells.push(IVec2::new(x, y));
                }
            }
        }
        cells
    }

    /// Bring the preview sprites in line with `cells`, which is a no-op once
    /// a drag has settled since most frames do not change it.
    fn show(&mut self, commands: &mut Commands, assets: &AssetServer, cells: Vec<IVec2>) {
        if self.cells == cells {
            return;
        }
        for entity in self.preview.drain(..) {
            commands.entity(entity).despawn();
        }
        for &cell in &cells {
            let centre = background::cell_centre(cell);
            let entity = if self.erasing {
                commands.spawn((
                    Sprite::from_color(ERASE_TINT, Vec2::splat(background::TILE)),
                    Transform::from_xyz(centre.x, centre.y, RECT_PREVIEW_Z),
                    WORLD_LAYER,
                    DespawnOnExit(AppState::Editor),
                ))
            } else {
                let item = &background::PALETTE[self.item];
                commands.spawn((
                    Sprite {
                        image: assets.load(item.art.path),
                        rect: item.first_frame(),
                        color: GHOST_TINT,
                        ..default()
                    },
                    Transform::from_xyz(centre.x, centre.y, RECT_PREVIEW_Z)
                        .with_scale(upscale(item.scale)),
                    WORLD_LAYER,
                    DespawnOnExit(AppState::Editor),
                ))
            }
            .id();
            self.preview.push(entity);
        }
        self.cells = cells;
    }

    /// Drop the preview and forget the drag, whether it was just committed or
    /// abandoned (a tool switch, or leaving the editor mid-drag).
    fn cancel(&mut self, commands: &mut Commands) {
        for entity in self.preview.drain(..) {
            commands.entity(entity).despawn();
        }
        self.anchor = None;
        self.cells.clear();
    }
}

/// Depth of a rectangle-drag preview sprite — under the single-tile cursor
/// ghost at [`CURSOR_Z`], but well clear of any background tile or prop so it
/// always reads as an overlay rather than as something already placed.
const RECT_PREVIEW_Z: f32 = 90.0;
/// Tint for a cell an in-progress erase would clear. Plain colour rather than
/// the tile's own texture: erasing does not leave that texture behind, so
/// showing it would say the wrong thing about what release does.
const ERASE_TINT: Color = Color::srgba(1.0, 0.25, 0.25, 0.4);

pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Tool>()
            .init_resource::<Cursor>()
            .init_resource::<RectangleDrag>()
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
    mut rect: ResMut<RectangleDrag>,
) {
    save(&maps, &current);
    // The sprites go with `DespawnOnExit`; the index of them must not outlive
    // them or the next entry would think cells are already painted.
    tiles.clear();
    cursor.world = None;
    // Its preview sprites went with `DespawnOnExit` too — this just forgets
    // the entities so nothing here tries to despawn them a second time.
    rect.anchor = None;
    rect.cells.clear();
    rect.preview.clear();
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
                    image: assets.load(tool.item().art.path),
                    rect: tool.item().first_frame(),
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
    mut rect: ResMut<RectangleDrag>,
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
    let place_released = buttons.just_released(MouseButton::Left)
        || pads().any(|pad| pad.just_released(GamepadButton::South));
    let erase_released = buttons.just_released(MouseButton::Right)
        || pads().any(|pad| pad.just_released(GamepadButton::West));

    // Switching away from the background layer mid-drag would otherwise
    // leave the preview on screen with nothing left driving it.
    if rect.anchor.is_some() && tool.layer != Layer::Background {
        rect.cancel(&mut commands);
    }

    match tool.layer {
        Layer::Background => {
            let cell = background::cell_of(world);
            match Instrument::for_background_item(tool.item().name) {
                // Held rather than clicked: most tiles are painted by
                // dragging over cells.
                Instrument::Brush => {
                    // A tool switched away from mid-drag: same reasoning as
                    // above, just within the one layer.
                    if rect.anchor.is_some() {
                        rect.cancel(&mut commands);
                    }
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
                // Corner to corner: nothing is painted until release, so the
                // rectangle can grow, shrink or flip freely on the way there.
                instrument @ (Instrument::Wall | Instrument::Block) => {
                    if rect.anchor.is_none() {
                        if place_once {
                            rect.anchor = Some(cell);
                            rect.erasing = false;
                            rect.item = tool.background;
                        } else if erase_once {
                            rect.anchor = Some(cell);
                            rect.erasing = true;
                            rect.item = tool.background;
                        }
                    }

                    if let Some(anchor) = rect.anchor {
                        let cells: Vec<IVec2> = RectangleDrag::cells(anchor, cell, instrument)
                            .into_iter()
                            .filter(|&cell| current.map.contains(background::point_of(cell)))
                            .collect();
                        rect.show(&mut commands, &assets, cells);

                        let released = if rect.erasing {
                            erase_released
                        } else {
                            place_released
                        };
                        if released {
                            let erasing = rect.erasing;
                            let item = rect.item;
                            for cell in std::mem::take(&mut rect.cells) {
                                if erasing {
                                    background::erase(
                                        &mut commands,
                                        &mut tiles,
                                        &mut current.map,
                                        cell,
                                    );
                                } else {
                                    background::paint(
                                        &mut commands,
                                        &assets,
                                        &mut tiles,
                                        &mut current.map,
                                        cell,
                                        item,
                                    );
                                }
                            }
                            rect.cancel(&mut commands);
                        }
                    }
                }
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
    mut ghosts: Query<GhostQuery, GhostFilter>,
) {
    if tool.is_changed() {
        for (mut sprite, mut transform) in &mut ghosts {
            sprite.image = assets.load(tool.item().art.path);
            // One frame of it, for an item whose art is a strip: the ghost
            // says what will be placed, not how it moves once it is.
            sprite.rect = tool.item().first_frame();
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
    ///
    /// For a strip the same thing is asked of **one frame**: the PNG is as
    /// many frames wide as it has, and what has to come out a cell wide is
    /// each of them.
    #[test]
    fn every_palette_item_is_one_cell_wide_once_scaled() {
        let strips = background::PALETTE
            .iter()
            .chain(props::PALETTE)
            .flat_map(|item| {
                [Some((item, item.art)), item.in_use.map(|strip| (item, strip))]
            })
            .flatten();
        for (item, strip) in strips {
            let (width, height) = png_size(&format!("assets/{}", strip.path));
            assert_eq!(
                width % strip.frames,
                0,
                "{}: {} is {width}px, which is not {} whole frames",
                item.name,
                strip.path,
                strip.frames,
            );
            let frame = width / strip.frames;
            assert_eq!(
                frame as f32 * item.scale,
                CELL as f32,
                "{}: a frame of {} is {frame}px at scale {}",
                item.name,
                strip.path,
                item.scale,
            );
            // Height is only pinned for a strip, where the frames have to be
            // cut out of it: a still prop may be taller than a cell on
            // purpose (`crate tall` is 48x64, and overhangs the cell above).
            if strip.frames > 1 {
                assert_eq!(
                    height, frame,
                    "{}: {} is {height}px tall and {frame}px to a frame; a strip is one row of squares",
                    item.name, strip.path,
                );
            }
        }
    }

    /// `"block"` and every `"wall..."` variant get the rectangle instruments;
    /// nothing else in the palette does, or a plain floor would start
    /// dragging rectangles instead of painting the cell under the cursor.
    #[test]
    fn only_wall_and_block_are_rectangle_instruments() {
        for item in background::PALETTE {
            let instrument = Instrument::for_background_item(item.name);
            let expected = if item.name == "block" {
                Instrument::Block
            } else if item.name.starts_with("wall") {
                Instrument::Wall
            } else {
                Instrument::Brush
            };
            assert_eq!(instrument, expected, "{}", item.name);
        }
    }

    /// A press and release on the same cell — a plain click, not a drag — is
    /// a one-cell rectangle either way, so the wall and block instruments
    /// paint exactly the one tile a click on any other tile would.
    #[test]
    fn a_click_with_no_drag_is_a_single_cell_for_either_instrument() {
        let cell = IVec2::new(3, 2);
        assert_eq!(RectangleDrag::cells(cell, cell, Instrument::Wall), [cell]);
        assert_eq!(RectangleDrag::cells(cell, cell, Instrument::Block), [cell]);
    }

    #[test]
    fn a_wall_rectangle_is_hollow_and_a_block_rectangle_is_filled() {
        let anchor = IVec2::new(0, 0);
        let far = IVec2::new(2, 3);

        let wall = RectangleDrag::cells(anchor, far, Instrument::Wall);
        let block = RectangleDrag::cells(anchor, far, Instrument::Block);

        // A 3x4 rectangle: 10 sides, 12 filled.
        assert_eq!(wall.len(), 10);
        assert_eq!(block.len(), 12);
        // The middle of the rectangle is inside the block, not on the wall.
        assert!(!wall.contains(&IVec2::new(1, 1)));
        assert!(block.contains(&IVec2::new(1, 1)));
        // Both agree on the corners.
        for corner in [
            IVec2::new(0, 0),
            IVec2::new(2, 0),
            IVec2::new(0, 3),
            IVec2::new(2, 3),
        ] {
            assert!(wall.contains(&corner));
            assert!(block.contains(&corner));
        }
    }

    /// Dragging backwards — releasing above or to the left of where the
    /// press started — has to give the same rectangle as dragging forwards,
    /// since a player drags whichever way is toward the far corner they want.
    #[test]
    fn a_rectangle_does_not_care_which_corner_is_the_anchor() {
        let a = IVec2::new(5, 5);
        let b = IVec2::new(2, 1);
        let sorted = |anchor, current| {
            let mut cells: Vec<_> = RectangleDrag::cells(anchor, current, Instrument::Wall)
                .into_iter()
                .map(|c| (c.x, c.y))
                .collect();
            cells.sort();
            cells
        };
        assert_eq!(sorted(a, b), sorted(b, a));
    }

    /// A one-cell-wide or one-cell-tall drag has no interior to be hollow
    /// about — every cell on it is a side — so a wall and a block agree.
    #[test]
    fn a_single_row_has_no_interior_to_leave_out() {
        let a = IVec2::new(0, 0);
        let b = IVec2::new(4, 0);
        let wall = RectangleDrag::cells(a, b, Instrument::Wall);
        let block = RectangleDrag::cells(a, b, Instrument::Block);
        assert_eq!(wall.len(), 5);
        assert_eq!(block.len(), 5);
    }
}
