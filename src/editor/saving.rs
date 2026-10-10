//! Saving is a prerequisite for leaving, not an effect of having left.
//! A failed save keeps both the map and the window open for another attempt.

use bevy::ecs::message::Messages;
use bevy::prelude::*;
use bevy::state::state::StateTransitionSystems;
use bevy::window::{WindowCloseRequested, close_when_requested};

use crate::browser::Maps;
use crate::state::AppState;
use crate::ui::{FONT_BODY, PANEL};

use super::CurrentMap;

#[derive(Resource, Default)]
struct SaveStatus {
    error: Option<String>,
}

impl SaveStatus {
    fn save(&mut self, maps: &Maps, current: &CurrentMap) -> bool {
        let Some(name) = &current.name else {
            self.error = None;
            return true;
        };
        match maps.0.save(name, &current.map) {
            Ok(()) => {
                self.error = None;
                info!("editor: saved {name}");
                true
            }
            Err(error) => {
                error!("editor: could not save {name}: {error}");
                self.error = Some(format!(
                    "Could not save {name}. Your changes are still open.\n\
                     {error}\n\
                     Fix the storage problem, then F5 / Start to retry."
                ));
                false
            }
        }
    }
}

#[derive(Component)]
struct SaveNotice;

pub(super) struct SavingPlugin;

impl Plugin for SavingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SaveStatus>()
            .add_systems(OnEnter(AppState::Editor), spawn_notice)
            .add_systems(
                StateTransition,
                guard_transition
                    .before(StateTransitionSystems::DependentTransitions)
                    .run_if(in_state(AppState::Editor)),
            )
            .add_systems(
                Update,
                (save_on_demand, update_notice)
                    .chain()
                    .after(super::edit)
                    .run_if(in_state(AppState::Editor)),
            )
            .add_systems(
                Last,
                guard_app_exit
                    .before(close_when_requested)
                    .run_if(in_state(AppState::Editor)),
            );
    }
}

/// This must precede applying NextState: OnExit is already too late to refuse.
fn guard_transition(
    mut next: ResMut<NextState<AppState>>,
    maps: Res<Maps>,
    current: Res<CurrentMap>,
    mut status: ResMut<SaveStatus>,
) {
    let leaving = matches!(&*next,
        NextState::Pending(state) | NextState::PendingIfNeq(state) if *state != AppState::Editor
    );
    if leaving && !status.save(&maps, &current) {
        next.reset();
    }
}

/// Intercept the OS close before Bevy marks the window ClosingWindow. On a
/// successful save, exit directly with the window still alive until shutdown;
/// on failure, clearing the request leaves the editor usable. Programmatic
/// success exits take the same path. A fatal/QA error retains its exit code.
fn guard_app_exit(
    mut close: ResMut<Messages<WindowCloseRequested>>,
    mut exit: ResMut<Messages<AppExit>>,
    maps: Res<Maps>,
    current: Res<CurrentMap>,
    mut status: ResMut<SaveStatus>,
) {
    if close.is_empty() && exit.is_empty() {
        return;
    }
    let failed = exit.get_cursor().read(&exit).any(AppExit::is_error);
    close.clear();
    if status.save(&maps, &current) {
        if exit.is_empty() {
            exit.write(AppExit::Success);
        }
    } else if !failed {
        exit.clear();
    }
}

fn save_on_demand(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    maps: Res<Maps>,
    current: Res<CurrentMap>,
    mut status: ResMut<SaveStatus>,
) {
    if keys.just_pressed(KeyCode::F5) || gamepads.iter().any(|pad| pad.just_pressed(GamepadButton::Start)) {
        status.save(&maps, &current);
    }
}

fn spawn_notice(mut commands: Commands, mut status: ResMut<SaveStatus>) {
    status.error = None;
    commands.spawn((
        SaveNotice,
        Name::new("save error"),
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            bottom: px(4),
            left: px(4),
            right: px(4),
            padding: UiRect::all(px(4)),
            ..default()
        },
        GlobalZIndex(10),
        BackgroundColor(PANEL),
        Text::new(""),
        TextFont::from_font_size(FONT_BODY),
        TextColor(Color::srgb(1.0, 0.65, 0.55)),
        DespawnOnExit(AppState::Editor),
    ));
}

fn update_notice(status: Res<SaveStatus>, mut notices: Query<(&mut Node, &mut Text), With<SaveNotice>>) {
    if !status.is_changed() {
        return;
    }
    for (mut node, mut text) in &mut notices {
        node.display = if status.error.is_some() { Display::Flex } else { Display::None };
        **text = status.error.clone().unwrap_or_default();
    }
}
