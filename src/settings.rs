//! Application preferences, independent of a map or saved game.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::state::AppState;
use crate::ui::nav::{Activated, Cancelled, Focusable, NavSystems};
use crate::ui::{self, FONT_BODY, FONT_TITLE, PANEL, TEXT_ACCENT, TEXT_DANGER, TEXT_DIM};

#[derive(Resource, Default, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub screen_edge_scrolling: bool,
}

#[derive(Resource)]
struct SettingsFile(PathBuf);

impl Settings {
    fn load(path: &Path) -> Result<Self, String> {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| error.to_string()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.to_string()),
        }
    }

    fn save(&self, path: &Path) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
        let temporary = path.with_extension("json.tmp");
        // Replace only after a complete write, so a failed write keeps the old preferences.
        std::fs::write(&temporary, bytes)
            .and_then(|()| std::fs::rename(&temporary, path))
            .map_err(|error| error.to_string())
    }

    fn edge_label(&self) -> &'static str {
        if self.screen_edge_scrolling {
            "Screen edge scrolling: on"
        } else {
            "Screen edge scrolling: off"
        }
    }
}

#[derive(Resource, Default)]
struct SettingsError(String);

#[derive(Component)]
enum Action {
    ToggleEdgeScrolling,
    Back,
}

#[derive(Component)]
struct EdgeLabel;

#[derive(Component)]
struct ErrorLabel;

pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        let path = std::env::var_os("CROWD2X_SETTINGS")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".settings.json"));
        let (settings, error) = match Settings::load(&path) {
            Ok(settings) => (settings, String::new()),
            Err(error) => {
                warn!("could not load settings from {}: {error}", path.display());
                (
                    Settings::default(),
                    format!("Could not load settings: {error}"),
                )
            }
        };
        app.insert_resource(settings)
            .insert_resource(SettingsFile(path))
            .insert_resource(SettingsError(error))
            .add_systems(OnEnter(AppState::Settings), spawn_screen)
            .add_systems(
                Update,
                choose
                    .after(NavSystems)
                    .run_if(in_state(AppState::Settings)),
            );
    }
}

fn spawn_screen(mut commands: Commands, settings: Res<Settings>, error: Res<SettingsError>) {
    commands
        .spawn((
            Name::new("settings"),
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(6),
                padding: UiRect::all(px(8)),
                ..default()
            },
            BackgroundColor(PANEL),
            DespawnOnExit(AppState::Settings),
        ))
        .with_children(|screen| {
            screen.spawn(ui::label("Settings", FONT_TITLE, TEXT_ACCENT));
            screen.spawn((
                Action::ToggleEdgeScrolling,
                ui::labelled_button(
                    (
                        EdgeLabel,
                        ui::label(settings.edge_label(), FONT_BODY, ui::TEXT),
                    ),
                    px(150),
                ),
                Focusable::new(0, 0),
            ));
            screen.spawn(ui::label(
                "Move the camera at the screen edges in the game and editor.",
                FONT_BODY,
                TEXT_DIM,
            ));
            screen.spawn(ui::label(
                "Changes are saved automatically.",
                FONT_BODY,
                TEXT_DIM,
            ));
            screen.spawn((ErrorLabel, ui::label(&error.0, FONT_BODY, TEXT_DANGER)));
            screen.spawn((
                Action::Back,
                ui::button("Back", Focusable::new(1, 0), px(80)),
            ));
        });
}

fn choose(
    mut activated: MessageReader<Activated>,
    mut cancelled: MessageReader<Cancelled>,
    actions: Query<&Action>,
    mut settings: ResMut<Settings>,
    file: Res<SettingsFile>,
    mut error: ResMut<SettingsError>,
    mut labels: Query<&mut Text, With<EdgeLabel>>,
    mut errors: Query<&mut Text, (With<ErrorLabel>, Without<EdgeLabel>)>,
    mut next: ResMut<NextState<AppState>>,
) {
    if cancelled.read().count() > 0 {
        next.set(AppState::MainMenu);
        return;
    }
    for Activated(entity) in activated.read() {
        match actions.get(*entity) {
            Ok(Action::Back) => next.set(AppState::MainMenu),
            Ok(Action::ToggleEdgeScrolling) => {
                let mut changed = settings.clone();
                changed.screen_edge_scrolling = !changed.screen_edge_scrolling;
                match changed.save(&file.0) {
                    Ok(()) => {
                        *settings = changed;
                        error.0.clear();
                    }
                    Err(reason) => error.0 = format!("Could not save settings: {reason}"),
                }
                for mut label in &mut labels {
                    label.0 = settings.edge_label().into();
                }
                for mut label in &mut errors {
                    label.0.clone_from(&error.0);
                }
            }
            Err(_) => {}
        }
    }
}

/// A narrow band in window pixels, independent of world zoom. UI absorbs the
/// pointer before it can pan the world, just as it absorbs selection clicks.
pub fn edge_scroll(window: &Window, settings: &Settings, over_ui: bool) -> Vec2 {
    if !settings.screen_edge_scrolling || over_ui || !window.focused {
        return Vec2::ZERO;
    }
    edge_direction(
        window.cursor_position(),
        Vec2::new(window.width(), window.height()),
    )
}

fn edge_direction(cursor: Option<Vec2>, size: Vec2) -> Vec2 {
    let Some(cursor) = cursor else {
        return Vec2::ZERO;
    };
    if cursor.x < 0.0 || cursor.y < 0.0 || cursor.x >= size.x || cursor.y >= size.y {
        return Vec2::ZERO;
    }
    let band = Vec2::splat(16.0).min(size * 0.25);
    Vec2::new(
        (cursor.x >= size.x - band.x) as i32 as f32 - (cursor.x < band.x) as i32 as f32,
        (cursor.y < band.y) as i32 as f32 - (cursor.y >= size.y - band.y) as i32 as f32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_bands_follow_world_axes_and_ignore_the_middle_and_outside() {
        let size = Vec2::new(1280.0, 720.0);
        for (point, direction) in [
            (Vec2::new(0.0, 360.0), Vec2::NEG_X),
            (Vec2::new(1279.0, 360.0), Vec2::X),
            (Vec2::new(640.0, 0.0), Vec2::Y),
            (Vec2::new(640.0, 719.0), Vec2::NEG_Y),
            (Vec2::new(0.0, 0.0), Vec2::new(-1.0, 1.0)),
            (size * 0.5, Vec2::ZERO),
            (Vec2::new(-1.0, 360.0), Vec2::ZERO),
            (size, Vec2::ZERO),
        ] {
            assert_eq!(edge_direction(Some(point), size), direction);
        }
        assert_eq!(edge_direction(None, size), Vec2::ZERO);
    }

    #[test]
    fn edge_scrolling_respects_preferences_focus_and_ui() {
        let mut window = Window::default();
        window.focused = true;
        window.set_cursor_position(Some(Vec2::new(0.0, 100.0)));
        let on = Settings {
            screen_edge_scrolling: true,
        };
        assert_eq!(edge_scroll(&window, &on, false), Vec2::NEG_X);
        assert_eq!(
            edge_scroll(&window, &Settings::default(), false),
            Vec2::ZERO
        );
        assert_eq!(edge_scroll(&window, &on, true), Vec2::ZERO);
        window.focused = false;
        assert_eq!(edge_scroll(&window, &on, false), Vec2::ZERO);
    }

    #[test]
    fn preferences_survive_reload_and_missing_fields_keep_defaults() {
        let path =
            std::env::temp_dir().join(format!("crowd2x-settings-{}.json", std::process::id()));
        let on = Settings {
            screen_edge_scrolling: true,
        };
        on.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), on);
        Settings::default().save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), Settings::default());
        std::fs::remove_file(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), Settings::default());
        assert_eq!(
            serde_json::from_str::<Settings>("{}").unwrap(),
            Settings::default()
        );
        assert!(serde_json::from_str::<Settings>(r#"{"screen_edge_scrolling":"yes"}"#).is_err());
    }
}
