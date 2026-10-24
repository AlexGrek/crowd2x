//! Camera tracking and the brief cell marker requested by the target button.
//! Both are view state; neither changes the simulation.

use bevy::prelude::*;

use crate::characters::depth_for;
use crate::editor::background::{TILE, cell_centre};
use crate::map::Point;
use crate::render::WORLD_LAYER;
use crate::sim::Uid;
use crate::state::AppState;
use crate::ui::{FIELD, HIGHLIGHT_SOLID, TEXT_ACCENT};

use super::selection::Selected;

const FLASH_SECONDS: f32 = 1.2;

#[derive(Resource, Default)]
pub struct CameraFollow {
    pub target: Option<Uid>,
    flash: Option<(Point, f64)>,
}

impl CameraFollow {
    pub fn toggle(&mut self, uid: Uid, cell: Point, now: f64) {
        self.target = (self.target != Some(uid)).then_some(uid);
        self.flash = Some((cell, now + FLASH_SECONDS as f64));
    }
}

#[derive(Component)]
pub(crate) struct FollowButton;

#[derive(Component)]
pub(crate) struct TargetCell {
    offset: Vec2,
    opacity: f32,
}

pub struct FollowPlugin;

impl Plugin for FollowPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraFollow>()
            .add_systems(OnEnter(AppState::Game), spawn_marker)
            .add_systems(OnExit(AppState::Game), reset)
            .add_systems(Update, update_marker.run_if(in_state(AppState::Game)));
    }
}

fn reset(mut follow: ResMut<CameraFollow>) {
    *follow = CameraFollow::default();
}

/// A small crosshair drawn with UI rectangles, so it needs no font glyph.
pub fn button() -> impl Bundle {
    (
        Name::new("follow"),
        FollowButton,
        Button,
        Node {
            width: px(11),
            height: px(11),
            ..default()
        },
        BackgroundColor(FIELD),
        children![
            icon_rect(3.0, 3.0, 5.0, 1.0),
            icon_rect(3.0, 7.0, 5.0, 1.0),
            icon_rect(3.0, 4.0, 1.0, 3.0),
            icon_rect(7.0, 4.0, 1.0, 3.0),
            icon_rect(5.0, 1.0, 1.0, 3.0),
            icon_rect(5.0, 7.0, 1.0, 3.0),
            icon_rect(1.0, 5.0, 3.0, 1.0),
            icon_rect(7.0, 5.0, 3.0, 1.0),
            icon_rect(5.0, 5.0, 1.0, 1.0),
        ],
    )
}

fn icon_rect(x: f32, y: f32, width: f32, height: f32) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            left: px(x),
            top: px(y),
            width: px(width),
            height: px(height),
            ..default()
        },
        BackgroundColor(TEXT_ACCENT),
    )
}

pub fn highlight_button(
    follow: Res<CameraFollow>,
    selected: Res<Selected>,
    mut buttons: Query<(&Interaction, &mut BackgroundColor), With<FollowButton>>,
) {
    let active = follow.target.is_some() && follow.target == selected.get();
    for (interaction, mut background) in &mut buttons {
        let wanted = if active || *interaction != Interaction::None {
            HIGHLIGHT_SOLID
        } else {
            FIELD
        };
        if background.0 != wanted {
            background.0 = wanted;
        }
    }
}

fn update_marker(
    time: Res<Time<Real>>,
    mut follow: ResMut<CameraFollow>,
    mut markers: Query<(&TargetCell, &mut Sprite, &mut Transform, &mut Visibility)>,
) {
    if follow
        .flash
        .is_some_and(|(_, until)| time.elapsed_secs_f64() >= until)
    {
        follow.flash = None;
    }
    for (marker, mut sprite, mut transform, mut visibility) in &mut markers {
        let Some((cell, until)) = follow.flash else {
            *visibility = Visibility::Hidden;
            continue;
        };
        let centre = cell_centre(IVec2::new(cell.x, cell.y));
        let alpha = ((until - time.elapsed_secs_f64()) as f32 / FLASH_SECONDS).min(1.0);
        *visibility = Visibility::Visible;
        sprite.color = Color::srgba(0.98, 0.82, 0.31, marker.opacity * alpha);
        transform.translation = (centre + marker.offset).extend(depth_for(centre.y) + 500.0);
    }
}

fn spawn_marker(mut commands: Commands) {
    // Whole-pixel rectangles over the world, with a fading fill and border.
    for (offset, size, opacity) in [
        (Vec2::ZERO, Vec2::splat(TILE), 0.20),
        (Vec2::new(0.0, 23.0), Vec2::new(TILE, 2.0), 0.95),
        (Vec2::new(0.0, -23.0), Vec2::new(TILE, 2.0), 0.95),
        (Vec2::new(23.0, 0.0), Vec2::new(2.0, TILE), 0.95),
        (Vec2::new(-23.0, 0.0), Vec2::new(2.0, TILE), 0.95),
    ] {
        commands.spawn((
            TargetCell { offset, opacity },
            Sprite::from_color(TEXT_ACCENT, size),
            Visibility::Hidden,
            WORLD_LAYER,
            DespawnOnExit(AppState::Game),
        ));
    }
}
