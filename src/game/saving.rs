//! Saving the game from the game screen, and opening one that was loaded.
//!
//! Two ways to save, side by side under the world's controls:
//!
//! * **`save`** asks for a name — the on-screen keyboard, with a sensible
//!   one already typed — with the world paused behind it. `enter` or `done`
//!   saves; `esc` or `B` thinks better of it. The speed it had comes back
//!   either way.
//! * **`quick save`** (and `F5`, as in the editor) saves at once, as
//!   "quick save". Only the newest [`save::QUICK_SAVES_KEPT`] are kept.
//!
//! Loading is the main menu's (`crate::saves`): a game is never swapped for
//! another under the player's feet.
//!
//! # Where the time goes
//!
//! A save is encoded here, from `Res<Sim>` — the world between two ticks,
//! read and never written, so this is not a second writer of it — with the
//! crowd's shards encoded in parallel ([`save::snapshot`]). Compressing and
//! writing need nothing of the world, so they go to a thread of their own,
//! and the line in the log saying it is done, and how long each half took,
//! arrives when that thread finishes. The frame only ever waits for the
//! first half.
//!
//! # A loaded game
//!
//! The Load screen leaves the world it read in [`PendingWorld`], which
//! `actors::build_world` takes in place of a fresh one, and how the screen
//! was looking at it in [`LoadedView`], which [`look_where_it_was`] puts back:
//! the camera, the zoom and the speed.

use std::path::PathBuf;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::ui::FocusPolicy;

use crate::editor::CurrentMap;
use crate::render::{CameraPan, CameraTarget, PixelZoom, WorldCamera};
use crate::save::{self, pb, SaveError, SaveMeta, SaveStore};
use crate::sim::GameState;
use crate::state::AppState;
use crate::ui::keyboard::{self, TextEntry, TextEntryClosed};
use crate::ui::nav::{not_typing, Activated, Cancelled, NavSystems, Scope};
use crate::ui::{hover_highlight, label, plain_button, FONT_BODY, FONT_TITLE, TEXT_ACCENT};

use super::actors::Sim;
use super::speed::GameSpeed;

/// The focus layer the save dialog's keyboard occupies, so nothing behind it
/// can be reached while a name is typed.
const SAVE_DIALOG: u8 = 2;

/// What a quick save is called.
pub const QUICK_SAVE: &str = "quick save";

/// The two buttons, and what pressing one asks for.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum SaveControl {
    /// Name it, then save it.
    Save,
    /// Save it now.
    Quick,
}

/// The open save dialog, if there is one, and whether the world was paused
/// before it opened — what to put back when it closes.
#[derive(Resource, Default)]
pub struct SaveDialog {
    overlay: Option<Entity>,
    was_paused: bool,
}

impl SaveDialog {
    pub fn is_open(&self) -> bool {
        self.overlay.is_some()
    }
}

/// A world read from a save, waiting for the game screen to open on it.
#[derive(Resource, Default)]
pub struct PendingWorld(pub Option<GameState>);

/// How the game screen was looking at a loaded world, waiting to be put
/// back when it opens.
#[derive(Resource, Default)]
pub struct LoadedView(pub Option<pb::View>);

/// Saves on their way to disk, each on its own thread.
#[derive(Resource, Default)]
struct Writing(Vec<Writer>);

struct Writer {
    name: String,
    population: usize,
    encoding: Duration,
    thread: JoinHandle<Result<Written, SaveError>>,
}

/// What a writer thread did.
struct Written {
    path: PathBuf,
    compressing: Duration,
    bytes: usize,
}

pub struct SavingPlugin;

impl Plugin for SavingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SaveDialog>()
            .init_resource::<PendingWorld>()
            .init_resource::<LoadedView>()
            .init_resource::<Writing>()
            .add_systems(OnEnter(AppState::Game), look_where_it_was.after(super::aim_camera_at_map))
            .add_systems(OnExit(AppState::Game), forget_the_dialog)
            .add_systems(
                Update,
                (
                    press_save_controls,
                    quick_save_key.run_if(not_typing),
                    finish_naming,
                    cancel_naming,
                    hover_highlight::<SaveControl>,
                )
                    .chain()
                    .after(NavSystems)
                    .run_if(in_state(AppState::Game).and_then(resource_exists::<Sim>)),
            )
            // Not only on the game screen: a save started just before
            // leaving it still finishes, and still says so.
            .add_systems(Update, report_written);
    }
}

/// The row of save buttons, for the HUD's world corner.
pub fn buttons() -> impl Bundle {
    children![
        (SaveControl::Save, plain_button("save", px(22))),
        (SaveControl::Quick, plain_button(QUICK_SAVE, px(48))),
    ]
}

/// The dialog goes with every other `DespawnOnExit`; what points at it, and
/// the scope it raised, go here.
fn forget_the_dialog(mut dialog: ResMut<SaveDialog>, mut entry: ResMut<TextEntry>) {
    if dialog.overlay.take().is_some() {
        entry.close();
    }
}

/// A click on a save button, or a script pressing one by name.
#[allow(clippy::too_many_arguments)]
fn press_save_controls(
    mut commands: Commands,
    mut activated: MessageReader<Activated>,
    clicked: Query<(&SaveControl, &Interaction), Changed<Interaction>>,
    controls: Query<&SaveControl>,
    mut dialog: ResMut<SaveDialog>,
    mut speed: ResMut<GameSpeed>,
    mut scope: ResMut<Scope>,
    mut entry: ResMut<TextEntry>,
    mut writing: ResMut<Writing>,
    sim: Res<Sim>,
    current: Res<CurrentMap>,
    view: ViewParams,
) {
    let mut pressed: Vec<SaveControl> =
        clicked.iter().filter(|(_, interaction)| **interaction == Interaction::Pressed).map(|(control, _)| *control).collect();
    pressed.extend(activated.read().filter_map(|message| controls.get(message.0).ok().copied()));
    for control in pressed {
        if dialog.is_open() {
            continue;
        }
        match control {
            SaveControl::Save => {
                dialog.was_paused = speed.is_paused();
                speed.set_paused(true);
                entry.set(default_name(&current, &sim.0));
                entry.open();
                *scope = Scope(SAVE_DIALOG);
                dialog.overlay = Some(open_dialog(&mut commands));
            }
            SaveControl::Quick => {
                writing.0.push(start_saving(&sim.0, QUICK_SAVE, true, &current, view.read(&speed)));
            }
        }
    }
}

/// The camera, the zoom and the speed: what a save keeps of the screen.
#[derive(bevy::ecs::system::SystemParam)]
struct ViewParams<'w, 's> {
    zoom: Res<'w, PixelZoom>,
    cameras: Query<'w, 's, &'static CameraPan, With<WorldCamera>>,
}

impl ViewParams<'_, '_> {
    fn read(&self, speed: &GameSpeed) -> pb::View {
        let centre = self.cameras.iter().next().map_or(Vec2::ZERO, |pan| pan.0);
        pb::View {
            camera_x: centre.x,
            camera_y: centre.y,
            zoom: self.zoom.get(),
            speed: speed.multiplier(),
            paused: speed.is_paused(),
        }
    }
}

/// `F5`: quick save, as the editor saves a map with it.
fn quick_save_key(
    keys: Res<ButtonInput<KeyCode>>,
    dialog: Res<SaveDialog>,
    speed: Res<GameSpeed>,
    mut writing: ResMut<Writing>,
    sim: Res<Sim>,
    current: Res<CurrentMap>,
    view: ViewParams,
) {
    if keys.just_pressed(KeyCode::F5) && !dialog.is_open() {
        writing.0.push(start_saving(&sim.0, QUICK_SAVE, true, &current, view.read(&speed)));
    }
}

/// What the name starts as: the map, and the hour and minute in the world.
fn default_name(current: &CurrentMap, state: &GameState) -> String {
    let clock = state.clock();
    let time = format!("{:02}-{:02}", clock.hour(), clock.minute());
    let map: String = current.title().chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_')).collect();
    let map = map.trim();
    // The time is what tells two saves of one map apart, so it is the map
    // that gives way to the length limit.
    let room = crate::map::MAX_NAME.saturating_sub(time.len() + 1);
    let map: String = map.chars().take(room).collect();
    format!("{} {time}", map.trim()).trim().to_string()
}

/// The on-screen keyboard, with a title saying what the name is for. It
/// takes the clicks over it, so nothing on the HUD behind is pressed through
/// it.
fn open_dialog(commands: &mut Commands) -> Entity {
    let overlay = keyboard::spawn(commands, SAVE_DIALOG);
    let title = commands
        .spawn((
            label("save the game as", FONT_TITLE, TEXT_ACCENT),
            Node { margin: UiRect::bottom(px(2)), ..default() },
        ))
        .id();
    let hint = commands.spawn(label("enter to save, esc to cancel", FONT_BODY, crate::ui::TEXT_DIM)).id();
    commands
        .entity(overlay)
        .insert((Name::new("save dialog"), FocusPolicy::Block, Interaction::default(), DespawnOnExit(AppState::Game)))
        .insert_children(0, &[title])
        .add_child(hint);
    overlay
}

fn close_dialog(commands: &mut Commands, dialog: &mut SaveDialog, speed: &mut GameSpeed, scope: &mut Scope, entry: &mut TextEntry) {
    if let Some(overlay) = dialog.overlay.take() {
        commands.entity(overlay).despawn();
    }
    entry.close();
    speed.set_paused(dialog.was_paused);
    *scope = Scope(0);
}

/// The name is typed: save under it, and close.
#[allow(clippy::too_many_arguments)]
fn finish_naming(
    mut commands: Commands,
    mut closed: MessageReader<TextEntryClosed>,
    mut dialog: ResMut<SaveDialog>,
    mut speed: ResMut<GameSpeed>,
    mut scope: ResMut<Scope>,
    mut entry: ResMut<TextEntry>,
    mut writing: ResMut<Writing>,
    sim: Res<Sim>,
    current: Res<CurrentMap>,
    view: ViewParams,
) {
    if closed.read().count() == 0 || !dialog.is_open() {
        return;
    }
    let typed = entry.text().trim().to_string();
    let name = if typed.is_empty() { default_name(&current, &sim.0) } else { typed };
    // The view as it was before the dialog paused the world.
    let mut saved_view = view.read(&speed);
    saved_view.paused = dialog.was_paused;
    writing.0.push(start_saving(&sim.0, &name, false, &current, saved_view));
    close_dialog(&mut commands, &mut dialog, &mut speed, &mut scope, &mut entry);
}

/// `esc` or `B` while naming: no save, and back to the game as it was.
fn cancel_naming(
    mut commands: Commands,
    mut cancelled: MessageReader<Cancelled>,
    mut dialog: ResMut<SaveDialog>,
    mut speed: ResMut<GameSpeed>,
    mut scope: ResMut<Scope>,
    mut entry: ResMut<TextEntry>,
) {
    if cancelled.read().count() > 0 && dialog.is_open() {
        close_dialog(&mut commands, &mut dialog, &mut speed, &mut scope, &mut entry);
    }
}

/// Encode the world now, and hand compressing and writing it to a thread.
fn start_saving(state: &GameState, name: &str, quick: bool, current: &CurrentMap, view: pb::View) -> Writer {
    let started = Instant::now();
    let moment = save::store::Moment::now();
    let meta = SaveMeta {
        name: name.to_string(),
        quick,
        map_title: current.title().to_string(),
        created_unix_ms: moment.unix_ms,
        created_local: moment.local.clone(),
    };
    let files = save::snapshot(state, &meta, Some(&view));
    let encoding = started.elapsed();
    state.log.push(format!("saving {name:?}..."));
    let thread = std::thread::Builder::new()
        .name("save writer".to_string())
        .spawn(move || {
            let started = Instant::now();
            let zip = files.to_zip(Some(moment.stamp))?;
            let store = SaveStore::default_store();
            let path = store.write(&zip, &moment)?;
            if quick {
                store.prune_quick(save::QUICK_SAVES_KEPT)?;
            }
            Ok(Written { path, compressing: started.elapsed(), bytes: zip.len() })
        })
        .expect("a thread to write the save on");
    Writer { name: name.to_string(), population: state.len(), encoding, thread }
}

/// Say in the log how each save that has finished went.
fn report_written(mut writing: ResMut<Writing>, sim: Option<Res<Sim>>) {
    if writing.0.iter().all(|writer| !writer.thread.is_finished()) {
        return;
    }
    let (done, still): (Vec<Writer>, Vec<Writer>) = writing.0.drain(..).partition(|writer| writer.thread.is_finished());
    writing.0 = still;
    for writer in done {
        let line = match writer.thread.join() {
            Ok(Ok(written)) => format!(
                "saved {:?}: {} units, {} KB, in {:.0} ms ({:.0} encoding, {:.0} compressing and writing) to {}",
                writer.name,
                writer.population,
                written.bytes / 1024,
                (writer.encoding + written.compressing).as_secs_f64() * 1e3,
                writer.encoding.as_secs_f64() * 1e3,
                written.compressing.as_secs_f64() * 1e3,
                written.path.display(),
            ),
            Ok(Err(error)) => format!("could not save {:?}: {error}", writer.name),
            Err(_) => format!("could not save {:?}: the writer stopped", writer.name),
        };
        info!("{line}");
        if let Some(sim) = sim.as_deref() {
            sim.0.log.push(line);
        }
    }
}

/// Put the camera, the zoom and the speed back as a loaded game had them.
/// After `aim_camera_at_map`, which aims at the middle of the map first.
fn look_where_it_was(
    mut loaded: ResMut<LoadedView>,
    mut target: ResMut<CameraTarget>,
    mut zoom: ResMut<PixelZoom>,
    mut speed: ResMut<GameSpeed>,
) {
    let Some(view) = loaded.0.take() else {
        return;
    };
    if view.camera_x.is_finite() && view.camera_y.is_finite() {
        target.0 = Some(Vec2::new(view.camera_x, view.camera_y));
    }
    if view.zoom != 0 {
        *zoom = PixelZoom::new(view.zoom);
    }
    if view.speed != 0.0 && !speed.restore(view.speed, view.paused) {
        warn!("saves: no game speed x{}, keeping the default", view.speed);
    }
}
