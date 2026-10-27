//! The find menu: ask where something is, and be shown.
//!
//! `find`, last in the world's corner, opens a full-screen menu like the
//! statistics menu ([`super::statistics`]) with four kinds of answer:
//!
//! * **units** — every job, and the pet dogs. Each one found gets a gold square
//!   on its cell, which follows it about.
//! * **squares** — the dirty ones, and the broken objects (the distribution
//!   boxes that failed), each marked the same way.
//! * **buildings** — the warehouse, the hospital and the rest a district must
//!   have ([`landmarks`]). These are not marked: the camera is centred over
//!   the building, and pressing the button of a kind there are several of
//!   (the restaurants) goes on to the next.
//! * `clear`, which puts the squares away. A search stays up until it is
//!   cleared or another is chosen, and the `find` button stays lit while one is.
//!
//! # View state, not the world's
//!
//! Like [`super::selection`], none of this is simulation state and none of it
//! is saved: [`Finding`] is a Bevy resource, the squares are sprites, and the
//! questions are answered by [`crate::sim::find`], which only reads. The
//! answer is **rescanned twice a second**, so somebody hired, dead or
//! adopted is caught up with; between scans a unit is looked up by its [`Uid`]
//! every frame, so the square keeps up with a walker.
//!
//! # The square is the camera tracking's
//!
//! The gold fill and border of [`super::follow`]'s flash, as one 48x48 image
//! at one pixel to the pixel (the cell is 48 canvas pixels), so a square is
//! one sprite and a few hundred cost no more than the dirt does. They are
//! pooled and bounded by the view ([`VisibleArea::tiles`]), like every other
//! thing drawn about the crowd.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::characters::depth_for;
use crate::editor::background::{cell_centre, TILE};
use crate::map::landmarks::{self, Landmark, LANDMARKS};
use crate::map::Point;
use crate::render::{CameraPan, WorldCamera, WORLD_LAYER};
use crate::sim::find::Search;
use crate::sim::{cell_of, JobKind, Uid};
use crate::state::AppState;
use crate::ui::keyboard::MODAL_Z;
use crate::ui::nav::{Activated, Focus, Focusable, NavSystems, Scope};
use crate::ui::{button, label, plain_button, FIELD, FONT_BODY, FONT_TITLE, HIGHLIGHT_SOLID, OVERLAY, TEXT, TEXT_ACCENT};
use crate::view::{ViewSystems, VisibleArea};

use super::actors::Sim;
use super::follow::CameraFollow;

/// The focus layer the menu occupies while it is open.
const FIND_MENU: u8 = 1;

/// Seconds, in real time, between two scans of the world for a search that
/// is up.
const RESCAN: f64 = 0.5;

/// The most squares drawn at once. The view bounds them far below this; it is
/// there for a zoom out over a district that is all dirt.
const MAX_MARKS: usize = 2048;

/// How wide a row of buttons may be, in canvas pixels, and the gap between
/// two buttons in it.
const ROW_WIDTH: f32 = 300.0;
const GAP: f32 = 3.0;

/// How far over the ground and everything standing on it a square is drawn:
/// [`super::follow`]'s flash, which it is the same square as.
const LIFT: f32 = 500.0;

/// Pixels in the border of a square, of the 48 across.
const BORDER: usize = 2;

/// The menu, if it is open. A resource, like [`super::statistics::StatisticsMenu`],
/// so [`super::leave`] can ask whether `esc` closes it or leaves the game.
#[derive(Resource, Default)]
pub struct FindMenu(pub Option<Entity>);

/// What is being looked for, and what was found.
#[derive(Resource, Default)]
pub struct Finding {
    search: Option<Search>,
    /// The answer to a search for units: who, to be asked where every frame.
    units: Vec<Uid>,
    /// The answer to a search for squares.
    cells: Vec<Point>,
    /// The real time at which the next scan is due.
    scan_due: f64,
    /// The buildings on this map, found when the menu was opened and so the
    /// ones its buttons are for.
    buildings: Vec<Landmark>,
    /// Which of the buildings of a kind the next press of its button goes to.
    next: [usize; landmarks::KINDS],
}

/// The `find` button in the world's corner.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub struct OpenFind;

/// A button in the menu.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Choice {
    Search(Search),
    /// Centre on a building: its row in [`LANDMARKS`].
    Building(usize),
    Clear,
    Close,
}

/// The menu's line saying what is being shown, rewritten as it changes.
#[derive(Component)]
struct StatusLine;

/// One gold square on the map.
#[derive(Component)]
struct Mark;

/// The square's picture, made once per visit to the game screen.
#[derive(Resource)]
struct MarkImage(Handle<Image>);

/// Every square sprite made, hidden or shown: the first `n` are the `n`
/// squares wanted.
#[derive(Resource, Default)]
struct Marks(Vec<Entity>);

pub struct FindPlugin;

impl Plugin for FindPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FindMenu>()
            .init_resource::<Finding>()
            .init_resource::<Marks>()
            .add_systems(OnEnter(AppState::Game), make_the_square)
            .add_systems(OnExit(AppState::Game), forget)
            .add_systems(
                Update,
                (open, choose.before(super::move_camera), scan, highlight_button, update_status)
                    .chain()
                    .after(NavSystems)
                    .run_if(in_state(AppState::Game)),
            )
            .add_systems(Update, show.after(ViewSystems).after(scan).run_if(in_state(AppState::Game)));
    }
}

/// The button that opens the menu, for the corner to put where it likes.
pub fn open_button() -> impl Bundle {
    (OpenFind, plain_button("find", px(24)))
}

/// The gold square: [`super::follow`]'s fill and border, in one picture.
fn make_the_square(mut commands: Commands, mut images: ResMut<Assets<Image>>, mut marks: ResMut<Marks>) {
    marks.0.clear();
    let side = TILE as usize;
    let mut image = Image::new_fill(
        Extent3d { width: side as u32, height: side as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    let mut pixels = Vec::with_capacity(side * side * 4);
    for y in 0..side {
        for x in 0..side {
            let edge = x < BORDER || y < BORDER || x >= side - BORDER || y >= side - BORDER;
            let alpha = if edge { 0.95 } else { 0.20 };
            pixels.extend_from_slice(&[250, 209, 79, (alpha * 255.0) as u8]);
        }
    }
    image.data = Some(pixels);
    commands.insert_resource(MarkImage(images.add(image)));
}

/// The sprites go with every other `DespawnOnExit`; what points at them, and
/// the scope the menu raised, go here. A search does not outlive the screen.
fn forget(mut menu: ResMut<FindMenu>, mut scope: ResMut<Scope>, mut finding: ResMut<Finding>, mut marks: ResMut<Marks>) {
    if menu.0.take().is_some() {
        *scope = Scope(0);
    }
    *finding = Finding::default();
    marks.0.clear();
}

/// How wide a button for `text` is, in canvas pixels: four a letter and the
/// padding, which is what the other buttons on this screen are cut to.
fn width_of(text: &str) -> f32 {
    text.chars().count() as f32 * 4.0 + 8.0
}

/// What the menu's status line says.
fn status(finding: &Finding) -> String {
    let Some(search) = finding.search else {
        return "choose what to find".to_string();
    };
    let found = if search.follows_units() { finding.units.len() } else { finding.cells.len() };
    if found == 0 {
        format!("{} found", search.counted(0))
    } else {
        format!("showing {}", search.counted(found))
    }
}

/// A click on `find`, or a script pressing it by name.
#[allow(clippy::too_many_arguments)]
fn open(
    mut commands: Commands,
    mut activated: MessageReader<Activated>,
    clicked: Query<&Interaction, (With<OpenFind>, Changed<Interaction>)>,
    buttons: Query<(), With<OpenFind>>,
    mut menu: ResMut<FindMenu>,
    mut scope: ResMut<Scope>,
    mut focus: ResMut<Focus>,
    mut finding: ResMut<Finding>,
    sim: Option<Res<Sim>>,
) {
    let pressed = clicked.iter().any(|interaction| *interaction == Interaction::Pressed)
        | activated.read().any(|Activated(entity)| buttons.contains(*entity));
    if !pressed || menu.0.is_some() || scope.0 != 0 {
        return;
    }
    let Some(sim) = sim else { return };

    finding.buildings = landmarks::find(&sim.0.map);

    let mut units: Vec<(String, Choice)> = JobKind::ALL
        .into_iter()
        .map(|kind| (kind.name().to_string(), Choice::Search(Search::Workers(kind))))
        .collect();
    units.push(("pet dogs".to_string(), Choice::Search(Search::PetDogs)));
    let squares = vec![
        ("dirty squares".to_string(), Choice::Search(Search::DirtySquares)),
        ("broken objects".to_string(), Choice::Search(Search::BrokenObjects)),
    ];
    let buildings: Vec<(String, Choice)> = LANDMARKS
        .iter()
        .enumerate()
        .filter_map(|(def, landmark)| {
            let count = finding.buildings.iter().filter(|building| building.def == def).count();
            let text = match count {
                0 => return None,
                1 => landmark.name.to_string(),
                _ => format!("{} ({count})", landmark.name),
            };
            Some((text, Choice::Building(def)))
        })
        .collect();
    let actions = vec![("clear".to_string(), Choice::Clear), ("close".to_string(), Choice::Close)];

    let overlay = commands
        .spawn((
            Name::new("find menu"),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(GAP),
                ..default()
            },
            BackgroundColor(OVERLAY),
            GlobalZIndex(MODAL_Z),
            DespawnOnExit(AppState::Game),
        ))
        .id();
    commands.spawn((label("find", FONT_TITLE, TEXT), ChildOf(overlay)));
    commands.spawn((StatusLine, label(status(&finding), FONT_BODY, TEXT_ACCENT), ChildOf(overlay)));

    // Rows of buttons packed to the width of the canvas, a section after
    // another with a little air between; `row` runs on across them so the
    // highlight walks the menu the way it looks.
    let mut row = 0;
    let mut first = None;
    for (section, entries) in [units, squares, buildings, actions].into_iter().enumerate() {
        let mut lines: Vec<Vec<&(String, Choice)>> = Vec::new();
        let mut used = 0.0;
        for entry in &entries {
            let width = width_of(&entry.0);
            match lines.last_mut() {
                Some(line) if used + GAP + width <= ROW_WIDTH => {
                    line.push(entry);
                    used += GAP + width;
                }
                _ => {
                    lines.push(vec![entry]);
                    used = width;
                }
            }
        }
        for (index, line) in lines.into_iter().enumerate() {
            let air = if index == 0 && section > 0 { px(GAP) } else { px(0) };
            let strip = commands
                .spawn((
                    Node { flex_direction: FlexDirection::Row, column_gap: px(GAP), margin: UiRect::top(air), ..default() },
                    ChildOf(overlay),
                ))
                .id();
            for (col, (text, choice)) in line.into_iter().enumerate() {
                let id = commands
                    .spawn((
                        *choice,
                        button(text, Focusable::new(row, col as i32).in_scope(FIND_MENU), px(width_of(text))),
                        ChildOf(strip),
                    ))
                    .id();
                first.get_or_insert(id);
            }
            row += 1;
        }
    }

    focus.0 = first;
    *scope = Scope(FIND_MENU);
    menu.0 = Some(overlay);
}

/// A button in the menu pressed.
#[allow(clippy::too_many_arguments)]
fn choose(
    mut commands: Commands,
    mut activated: MessageReader<Activated>,
    choices: Query<&Choice>,
    mut menu: ResMut<FindMenu>,
    mut scope: ResMut<Scope>,
    mut finding: ResMut<Finding>,
    mut follow: ResMut<CameraFollow>,
    mut cameras: Query<&mut CameraPan, With<WorldCamera>>,
) {
    if menu.0.is_none() {
        return;
    }
    let Some(choice) = activated.read().filter_map(|Activated(entity)| choices.get(*entity).ok()).last().copied() else {
        return;
    };
    match choice {
        Choice::Search(search) => {
            finding.search = Some(search);
            // Scan this frame, so there is something to show at once.
            finding.scan_due = 0.0;
        }
        Choice::Clear => {
            finding.search = None;
            finding.units.clear();
            finding.cells.clear();
        }
        Choice::Building(def) => {
            let here: Vec<Point> =
                finding.buildings.iter().filter(|building| building.def == def).map(|building| building.at).collect();
            if !here.is_empty() {
                let which = finding.next[def] % here.len();
                finding.next[def] = which + 1;
                let at = here[which];
                // Tracking somebody would take the camera straight back.
                follow.target = None;
                for mut pan in &mut cameras {
                    pan.0 = cell_centre(IVec2::new(at.x, at.y));
                }
            }
        }
        Choice::Close => {}
    }
    close_find(&mut commands, &mut menu, &mut scope);
}

/// Close the menu, from a choice in it or from `esc` ([`super::leave`]).
pub(super) fn close_find(commands: &mut Commands, menu: &mut FindMenu, scope: &mut Scope) {
    if let Some(overlay) = menu.0.take() {
        commands.entity(overlay).despawn();
    }
    *scope = Scope(0);
}

/// Look the world over for a search that is up, now and twice a second.
fn scan(time: Res<Time<Real>>, sim: Option<Res<Sim>>, mut finding: ResMut<Finding>) {
    let (Some(search), Some(sim)) = (finding.search, sim) else {
        return;
    };
    let now = time.elapsed_secs_f64();
    if now < finding.scan_due {
        return;
    }
    finding.scan_due = now + RESCAN;
    let finding = &mut *finding;
    search.scan(&sim.0, &mut finding.units, &mut finding.cells);
}

/// `find` stays lit while there is something to see.
fn highlight_button(finding: Res<Finding>, mut buttons: Query<(&Interaction, &mut BackgroundColor), With<OpenFind>>) {
    let active = finding.search.is_some();
    for (interaction, mut background) in &mut buttons {
        let wanted = if active || *interaction != Interaction::None { HIGHLIGHT_SOLID } else { FIELD };
        if background.0 != wanted {
            background.0 = wanted;
        }
    }
}

/// Keep the menu's status line true while it is open.
fn update_status(menu: Res<FindMenu>, finding: Res<Finding>, mut texts: Query<&mut Text, With<StatusLine>>) {
    if menu.0.is_none() {
        return;
    }
    let wanted = status(&finding);
    for mut text in &mut texts {
        // Compared before writing: the same string written again still costs
        // a re-layout.
        if **text != wanted {
            **text = wanted.clone();
        }
    }
}

/// Put a square on every cell the search found that is on the canvas.
///
/// Units are asked where they are every frame, and the squares are the pool's
/// first `n` sprites moved to them; the rest are hidden, never despawned.
#[allow(clippy::too_many_arguments)]
fn show(
    mut commands: Commands,
    sim: Option<Res<Sim>>,
    finding: Res<Finding>,
    area: Res<VisibleArea>,
    image: Option<Res<MarkImage>>,
    mut pool: ResMut<Marks>,
    mut marks: Query<(&mut Transform, &mut Visibility), With<Mark>>,
    mut wanted: Local<Vec<Point>>,
) {
    wanted.clear();
    if let (Some(search), Some(sim)) = (finding.search, sim.as_deref())
        && area.ready
    {
        let seen = |cell: Point| area.tiles.contains(IVec2::new(cell.x, cell.y));
        if search.follows_units() {
            wanted.extend(
                finding
                    .units
                    .iter()
                    .filter_map(|&uid| sim.0.position_of(uid))
                    .map(cell_of)
                    .filter(|&cell| seen(cell)),
            );
        } else {
            wanted.extend(finding.cells.iter().copied().filter(|&cell| seen(cell)));
        }
        // Two units in one cell are one square.
        wanted.sort_unstable();
        wanted.dedup();
        wanted.truncate(MAX_MARKS);
    }

    let Some(image) = image else { return };
    for (index, &cell) in wanted.iter().enumerate() {
        let centre = cell_centre(IVec2::new(cell.x, cell.y));
        let at = centre.extend(depth_for(centre.y) + LIFT);
        match pool.0.get(index) {
            Some(&entity) => {
                if let Ok((mut transform, mut visibility)) = marks.get_mut(entity) {
                    if transform.translation != at {
                        transform.translation = at;
                    }
                    if *visibility != Visibility::Visible {
                        *visibility = Visibility::Visible;
                    }
                }
            }
            None => {
                let entity = commands
                    .spawn((
                        Name::new("find mark"),
                        Mark,
                        Sprite::from_image(image.0.clone()),
                        Transform::from_translation(at),
                        Visibility::Visible,
                        WORLD_LAYER,
                        DespawnOnExit(AppState::Game),
                    ))
                    .id();
                pool.0.push(entity);
            }
        }
    }
    for &entity in pool.0.iter().skip(wanted.len()) {
        if let Ok((_, mut visibility)) = marks.get_mut(entity)
            && *visibility != Visibility::Hidden
        {
            *visibility = Visibility::Hidden;
        }
    }
}
