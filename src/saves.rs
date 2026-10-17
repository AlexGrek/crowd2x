//! The Load screen: every saved game, newest first, and the only way back
//! into one.
//!
//! Reached from the main menu (`load`), and only from there: a game is never
//! swapped for another under the player's feet. A save's **name** is the
//! button that loads it — loading is what a save is for — with what it is of
//! beside it: the map, when it was saved, how far into the world's days it
//! had got, and how many lived in it. `delete` asks first, as it does for a
//! map.
//!
//! A save this build cannot read — a newer format, a broken file — is still
//! listed, with why, so it can be deleted; it just does not load.
//!
//! Loading reads and builds the world on a thread of its own, with the
//! status line saying so, and then opens the game screen on it: the world
//! goes into [`PendingWorld`], the site into [`CurrentMap`] (which the
//! screen draws the ground from), and how the screen was looking into
//! [`LoadedView`]. `CROWD2X_LOAD` does the same without the screen
//! ([`load_from_env`]).
//!
//! Built like the map browser ([`crate::browser`]): nav focusables, a scroll
//! list that follows the focus, a confirmation that starts on `cancel`.

use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::ScrollPosition;

use crate::editor::CurrentMap;
use crate::game::saving::{LoadedView, PendingWorld};
use crate::save::{self, LoadedGame, SaveEntry, SaveError, SaveFiles, SaveStore};
use crate::sim::Clock;
use crate::state::AppState;
use crate::ui::keyboard::MODAL_Z;
use crate::ui::nav::{Activated, Cancelled, Focus, Focusable, NavSystems, Scope};
use crate::ui::{button, danger_button, label, scroll_to_show, FONT_BODY, FONT_TITLE, MODAL, ROW, TEXT, TEXT_ACCENT, TEXT_DIM};

const BACKDROP: Color = Color::srgba(0.04, 0.04, 0.06, 0.97);

/// Focus layers, as in the browser.
const LIST: u8 = 0;
const CONFIRMING: u8 = 2;

/// The toolbar above the list.
const TOOLBAR_ROW: i32 = -1;

const ROW_HEIGHT: f32 = 12.0;
const ROW_GAP: f32 = 1.0;
const ROW_STRIDE: f32 = ROW_HEIGHT + ROW_GAP;
const LIST_HEIGHT: f32 = 130.0;
const NAME_WIDTH: f32 = 80.0;
const ACTION_WIDTH: f32 = 30.0;
const WHEEL_LINE: f32 = ROW_STRIDE;

const HINTS: &str = "press a name to load it   arrows/dpad move   esc/B back";

/// The saves directory, opened once.
#[derive(Resource)]
pub struct Saves(pub SaveStore);

/// The saves, as listed: newest first.
#[derive(Resource, Default)]
struct Listed(Vec<SaveEntry>);

/// The line under the list.
#[derive(Resource, Default)]
struct Status(String);

/// The delete confirmation, if it is open.
#[derive(Resource, Default)]
struct Modal(Option<Entity>);

/// A save being read, on its own thread.
#[derive(Resource, Default)]
struct Loading(Option<(String, JoinHandle<Result<LoadedGame, SaveError>>)>);

#[derive(Component)]
struct SaveList;

#[derive(Component)]
struct StatusLabel;

/// What a button here does. The save's file travels in the component, so an
/// action cannot land on whichever row is selected afterwards.
#[derive(Component, Clone)]
enum Action {
    Load(PathBuf),
    AskDelete(PathBuf, String),
    ConfirmDelete(PathBuf, String),
    CloseModal,
    Back,
}

pub struct SavesPlugin;

impl Plugin for SavesPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Saves(SaveStore::default_store()))
            .init_resource::<Listed>()
            .init_resource::<Status>()
            .init_resource::<Modal>()
            .init_resource::<Loading>()
            .add_systems(OnEnter(AppState::Saves), open_screen)
            .add_systems(OnExit(AppState::Saves), close_screen)
            .add_systems(
                Update,
                (run_actions, go_back, finish_loading, rebuild_list, follow_focus, wheel_scroll, show_status)
                    .chain()
                    .after(NavSystems)
                    .run_if(in_state(AppState::Saves)),
            );
        load_from_env(app);
    }
}

fn open_screen(
    mut commands: Commands,
    saves: Res<Saves>,
    mut listed: ResMut<Listed>,
    mut status: ResMut<Status>,
    mut scope: ResMut<Scope>,
    mut modal: ResMut<Modal>,
) {
    *scope = Scope(LIST);
    modal.0 = None;
    status.0.clear();
    reload(&saves, &mut listed, &mut status);

    commands.spawn((
        Name::new("load screen"),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(6)),
            row_gap: px(3),
            ..default()
        },
        BackgroundColor(BACKDROP),
        DespawnOnExit(AppState::Saves),
        children![
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Baseline,
                    ..default()
                },
                children![
                    label("saves", FONT_TITLE, TEXT_ACCENT),
                    label(HINTS, FONT_BODY, TEXT_DIM),
                    (Action::Back, button("back", Focusable::new(TOOLBAR_ROW, 0), px(30))),
                ],
            ),
            (
                SaveList,
                Node {
                    flex_direction: FlexDirection::Column,
                    height: px(LIST_HEIGHT),
                    overflow: Overflow { x: OverflowAxis::Clip, y: OverflowAxis::Scroll },
                    row_gap: px(ROW_GAP),
                    ..default()
                },
                ScrollPosition::default(),
            ),
            (StatusLabel, Text::new(String::new()), TextFont::from_font_size(FONT_BODY), TextColor(TEXT_DIM)),
        ],
    ));
}

fn close_screen(mut scope: ResMut<Scope>, mut modal: ResMut<Modal>) {
    *scope = Scope(LIST);
    modal.0 = None;
}

fn reload(saves: &Saves, listed: &mut Listed, status: &mut Status) {
    match saves.0.list() {
        Ok(entries) => {
            listed.0 = entries;
            if status.0.is_empty() {
                let count = listed.0.len();
                status.0 = format!("{count} {} in {}/", if count == 1 { "save" } else { "saves" }, saves.0.root().display());
            }
        }
        Err(error) => {
            error!("saves: {error}");
            status.0 = format!("cannot read {}: {error}", saves.0.root().display());
        }
    }
}

/// What a row says about a save beside its name.
fn details(entry: &SaveEntry) -> String {
    match &entry.manifest {
        Ok(manifest) => {
            let clock = Clock::after_watching(manifest.elapsed);
            format!(
                "{}  {}  day {} {}  {} units",
                manifest.map_title,
                manifest.created_local,
                clock.day(),
                clock.time(),
                manifest.population
            )
        }
        Err(error) => format!("cannot be loaded: {error}"),
    }
}

/// What a row is called: its name, or for one that cannot be read, its file.
fn title(entry: &SaveEntry) -> String {
    match &entry.manifest {
        Ok(manifest) if !manifest.name.is_empty() => manifest.name.clone(),
        _ => entry.file_name.clone(),
    }
}

/// Rebuild the rows whenever the list changes — wholesale, as the browser
/// does.
fn rebuild_list(mut commands: Commands, listed: Res<Listed>, lists: Query<Entity, With<SaveList>>) {
    if !listed.is_changed() {
        return;
    }
    for list in &lists {
        let mut list = commands.entity(list);
        list.despawn_related::<Children>();
        if listed.0.is_empty() {
            list.with_children(|list| {
                list.spawn(label("no saves yet - save a game with save or f5 while playing", FONT_BODY, TEXT_DIM));
            });
            continue;
        }
        list.with_children(|list| {
            for (index, entry) in listed.0.iter().enumerate() {
                let row = index as i32;
                let name = title(entry);
                let colour = if entry.is_loadable() { TEXT } else { TEXT_DIM };
                list.spawn((
                    Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        height: px(ROW_HEIGHT),
                        flex_shrink: 0.0,
                        column_gap: px(2),
                        ..default()
                    },
                    BackgroundColor(ROW),
                    children![
                        (
                            Action::Load(entry.path.clone()),
                            Focusable::new(row, 0),
                            Button,
                            Node {
                                width: px(NAME_WIDTH),
                                height: px(ROW_HEIGHT),
                                align_items: AlignItems::Center,
                                overflow: Overflow::clip_x(),
                                padding: UiRect::horizontal(px(3)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            children![(Text::new(name.clone()), TextFont::from_font_size(FONT_BODY), TextColor(colour))],
                        ),
                        (
                            Node { flex_grow: 1.0, overflow: Overflow::clip_x(), ..default() },
                            children![(Text::new(details(entry)), TextFont::from_font_size(FONT_BODY), TextColor(TEXT_DIM))],
                        ),
                        (
                            Action::AskDelete(entry.path.clone(), name),
                            danger_button("delete", Focusable::new(row, 1), px(ACTION_WIDTH)),
                        ),
                    ],
                ));
            }
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn run_actions(
    mut commands: Commands,
    mut activated: MessageReader<Activated>,
    actions: Query<&Action>,
    saves: Res<Saves>,
    mut listed: ResMut<Listed>,
    mut status: ResMut<Status>,
    mut scope: ResMut<Scope>,
    mut modal: ResMut<Modal>,
    mut focus: ResMut<Focus>,
    mut loading: ResMut<Loading>,
    mut next: ResMut<NextState<AppState>>,
) {
    for Activated(entity) in activated.read() {
        let Ok(action) = actions.get(*entity) else {
            continue;
        };
        match action {
            Action::Load(path) => {
                if loading.0.is_some() {
                    continue;
                }
                let Some(entry) = listed.0.iter().find(|entry| &entry.path == path) else {
                    continue;
                };
                if let Err(error) = &entry.manifest {
                    status.0 = format!("{} cannot be loaded: {error}", entry.file_name);
                    continue;
                }
                let name = title(entry);
                status.0 = format!("loading {name}...");
                let path = path.clone();
                let thread = std::thread::Builder::new()
                    .name("save reader".to_string())
                    .spawn(move || read(&path))
                    .expect("a thread to read the save on");
                loading.0 = Some((name, thread));
            }
            Action::AskDelete(path, name) => {
                modal.0 = Some(open_confirm(&mut commands, &mut focus, path, name));
                *scope = Scope(CONFIRMING);
            }
            Action::ConfirmDelete(path, name) => {
                match saves.0.delete(path) {
                    Ok(()) => status.0 = format!("deleted {name}"),
                    Err(error) => {
                        error!("saves: {error}");
                        status.0 = format!("could not delete {name}: {error}");
                    }
                }
                reload(&saves, &mut listed, &mut status);
                close_modal(&mut commands, &mut modal, &mut scope);
            }
            Action::CloseModal => close_modal(&mut commands, &mut modal, &mut scope),
            Action::Back => next.set(AppState::MainMenu),
        }
    }
}

/// Read a save from disk and build its world.
pub fn read(path: &Path) -> Result<LoadedGame, SaveError> {
    let bytes = std::fs::read(path).map_err(|error| SaveError::Io(path.to_path_buf(), error))?;
    save::restore(&SaveFiles::from_zip(&bytes)?)
}

/// What the game screen opens a loaded game from: the site to draw, the
/// world to run, and how it was being looked at.
///
/// The ground drawn is the save's own copy of the map — its stored chunks
/// included — so the map file it was played from need not still exist, or
/// still be the same.
pub fn open_game(loaded: LoadedGame) -> (CurrentMap, PendingWorld, LoadedView) {
    let LoadedGame { state, manifest, view } = loaded;
    (CurrentMap::new(manifest.map_title.clone(), state.map.clone()), PendingWorld(Some(state)), LoadedView(view))
}

/// When the save being read is ready, open the game on it — or say why not.
fn finish_loading(
    mut commands: Commands,
    mut loading: ResMut<Loading>,
    mut status: ResMut<Status>,
    mut next: ResMut<NextState<AppState>>,
) {
    if !loading.0.as_ref().is_some_and(|(_, thread)| thread.is_finished()) {
        return;
    }
    let (name, thread) = loading.0.take().expect("checked above");
    match thread.join() {
        Ok(Ok(loaded)) => {
            info!("saves: loaded {name}");
            let (current, world, view) = open_game(loaded);
            commands.insert_resource(current);
            commands.insert_resource(world);
            commands.insert_resource(view);
            next.set(AppState::Game);
        }
        Ok(Err(error)) => {
            error!("saves: {name}: {error}");
            status.0 = format!("could not load {name}: {error}");
        }
        Err(_) => status.0 = format!("could not load {name}: the reader stopped"),
    }
}

fn open_confirm(commands: &mut Commands, focus: &mut Focus, path: &Path, name: &str) -> Entity {
    let cancel = commands
        .spawn((Action::CloseModal, button("cancel", Focusable::new(0, 0).in_scope(CONFIRMING), px(48))))
        .id();
    let confirm = commands
        .spawn((
            Action::ConfirmDelete(path.to_path_buf(), name.to_string()),
            danger_button("delete", Focusable::new(0, 1).in_scope(CONFIRMING), px(48)),
        ))
        .id();
    let overlay = commands
        .spawn((
            Name::new("confirm delete"),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(4),
                ..default()
            },
            BackgroundColor(MODAL),
            GlobalZIndex(MODAL_Z),
            DespawnOnExit(AppState::Saves),
            children![label(format!("delete {name}?"), FONT_TITLE, TEXT), label("this cannot be undone", FONT_BODY, TEXT_DIM)],
        ))
        .id();
    commands
        .spawn((
            Node { flex_direction: FlexDirection::Row, column_gap: px(6), margin: UiRect::top(px(4)), ..default() },
            ChildOf(overlay),
        ))
        .add_children(&[cancel, confirm]);
    focus.0 = Some(cancel);
    overlay
}

fn close_modal(commands: &mut Commands, modal: &mut Modal, scope: &mut Scope) {
    if let Some(overlay) = modal.0.take() {
        commands.entity(overlay).despawn();
    }
    *scope = Scope(LIST);
}

/// Escape and B: out of the confirmation first, then back to the menu.
fn go_back(
    mut commands: Commands,
    mut cancelled: MessageReader<Cancelled>,
    mut modal: ResMut<Modal>,
    mut scope: ResMut<Scope>,
    mut next: ResMut<NextState<AppState>>,
) {
    if cancelled.read().count() == 0 {
        return;
    }
    if modal.0.is_some() {
        close_modal(&mut commands, &mut modal, &mut scope);
    } else {
        next.set(AppState::MainMenu);
    }
}

fn follow_focus(
    focus: Res<Focus>,
    listed: Res<Listed>,
    focusables: Query<&Focusable>,
    mut lists: Query<&mut ScrollPosition, With<SaveList>>,
) {
    if !focus.is_changed() {
        return;
    }
    let Some(slot) = focus.0.and_then(|entity| focusables.get(entity).ok()) else {
        return;
    };
    if slot.scope != LIST {
        return;
    }
    for mut scroll in &mut lists {
        let wanted = if slot.row < 0 {
            0.0
        } else {
            scroll_to_show(scroll.0.y, LIST_HEIGHT, slot.row as f32 * ROW_STRIDE, ROW_HEIGHT)
        };
        scroll.0.y = wanted.min(max_scroll(listed.0.len()));
    }
}

fn wheel_scroll(mut wheel: MessageReader<MouseWheel>, listed: Res<Listed>, mut lists: Query<&mut ScrollPosition, With<SaveList>>) {
    let delta: f32 = wheel
        .read()
        .map(|event| match event.unit {
            MouseScrollUnit::Line => event.y * WHEEL_LINE,
            MouseScrollUnit::Pixel => event.y,
        })
        .sum();
    if delta == 0.0 {
        return;
    }
    for mut scroll in &mut lists {
        scroll.0.y = (scroll.0.y - delta).clamp(0.0, max_scroll(listed.0.len()));
    }
}

fn max_scroll(rows: usize) -> f32 {
    (rows as f32 * ROW_STRIDE - ROW_GAP - LIST_HEIGHT).max(0.0)
}

fn show_status(status: Res<Status>, mut labels: Query<&mut Text, With<StatusLabel>>) {
    if !status.is_changed() {
        return;
    }
    for mut text in &mut labels {
        **text = status.0.clone();
    }
}

/// Where `CROWD2X_LOAD` says to boot into: a save's path, or `newest`.
pub const ENV_LOAD: &str = "CROWD2X_LOAD";

/// Open a saved game without going through the screen — what a scripted
/// capture of a loaded world needs. In `build`, for the reason
/// `debug::open_map_from_env` is: booting straight into the game runs its
/// `OnEnter` before every `Startup` system. `crate::debug::initial_state`
/// boots into the game when this is set.
fn load_from_env(app: &mut App) {
    let Ok(wanted) = std::env::var(ENV_LOAD) else {
        return;
    };
    let path = if wanted == "newest" {
        match SaveStore::default_store().newest() {
            Ok(Some(entry)) => entry.path,
            Ok(None) => {
                error!("saves: {ENV_LOAD}=newest, but there are no saves");
                return;
            }
            Err(error) => {
                error!("saves: {ENV_LOAD}: {error}");
                return;
            }
        }
    } else {
        PathBuf::from(wanted)
    };
    match read(&path) {
        Ok(loaded) => {
            info!("saves: {ENV_LOAD}, opening {}", path.display());
            let (current, world, view) = open_game(loaded);
            app.insert_resource(current).insert_resource(world).insert_resource(view);
        }
        Err(error) => error!("saves: cannot open {}: {error}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_save_can_be_scrolled_to_exactly() {
        let rows = 30;
        assert_eq!(max_scroll(rows), rows as f32 * ROW_STRIDE - ROW_GAP - LIST_HEIGHT);
        assert_eq!(max_scroll(0), 0.0);
    }
}
