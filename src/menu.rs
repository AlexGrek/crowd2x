//! Main menu.
//!
//! Ordinary `bevy_ui`: a full-screen column of `Button`s over a dimming
//! backdrop. The world keeps rendering behind it, so leaving the editor never
//! hides the map being edited.
//!
//! Selection, highlighting and activation come from [`ui::nav`], so the mouse,
//! the keyboard and a gamepad drive one highlight here exactly as they do in
//! the map browser.

use bevy::prelude::*;

use crate::state::AppState;
use crate::ui::nav::{Activated, Cancelled, Focusable, NavSystems};
use crate::ui::{button, label, FONT_BODY, FONT_TITLE, PANEL, TEXT_ACCENT, TEXT_DIM};

/// One way in, because there is only one thing to choose first: a map. The
/// browser is where that choice is made, and it is also where a map is edited,
/// copied or thrown away.
const ENTRIES: [(&str, MenuAction); 2] = [
    ("Play", MenuAction::Maps),
    ("Quit", MenuAction::Quit),
];

#[derive(Component, Clone, Copy)]
enum MenuAction {
    Maps,
    Quit,
}

pub struct MainMenuPlugin;

impl Plugin for MainMenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::MainMenu), spawn_menu)
            .add_systems(
                Update,
                choose
                    .after(NavSystems)
                    .run_if(in_state(AppState::MainMenu)),
            );
    }
}

fn spawn_menu(mut commands: Commands) {
    commands
        .spawn((
            Name::new("main menu"),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(2),
                ..default()
            },
            BackgroundColor(PANEL),
            DespawnOnExit(AppState::MainMenu),
        ))
        .with_children(|menu| {
            menu.spawn((
                Text::new("crowd2x"),
                TextFont::from_font_size(FONT_TITLE),
                TextColor(TEXT_ACCENT),
            ));
            menu.spawn((
                Text::new("a pixel crowd simulation"),
                TextFont::from_font_size(FONT_BODY),
                TextColor(TEXT_DIM),
                Node {
                    margin: UiRect::bottom(px(10)),
                    ..default()
                },
            ));

            for (index, (text, action)) in ENTRIES.iter().enumerate() {
                menu.spawn((
                    *action,
                    button(text, Focusable::new(index as i32, 0), px(80)),
                ));
            }

            menu.spawn((
                Node {
                    margin: UiRect::top(px(12)),
                    ..default()
                },
                children![label(
                    "arrows/dpad to select    enter/A to confirm",
                    FONT_BODY,
                    TEXT_DIM,
                )],
            ));
        });
}

fn choose(
    mut activated: MessageReader<Activated>,
    mut cancelled: MessageReader<Cancelled>,
    entries: Query<&MenuAction>,
    mut next: ResMut<NextState<AppState>>,
    mut exit: MessageWriter<AppExit>,
) {
    // Escape at the top level is the only way out of the game.
    if cancelled.read().count() > 0 {
        exit.write(AppExit::Success);
        return;
    }

    for Activated(entity) in activated.read() {
        match entries.get(*entity) {
            Ok(MenuAction::Maps) => next.set(AppState::Maps),
            Ok(MenuAction::Quit) => {
                exit.write(AppExit::Success);
            }
            Err(_) => {}
        }
    }
}
