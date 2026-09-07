//! Minimal frame animation for sprites backed by a horizontal texture atlas.

use bevy::prelude::*;

pub struct AnimationPlugin;

impl Plugin for AnimationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, advance_frames);
    }
}

/// Cycles `Sprite::texture_atlas` between `first` and `last` inclusive.
#[derive(Component)]
pub struct FrameAnimation {
    pub first: usize,
    pub last: usize,
    pub timer: Timer,
}

impl FrameAnimation {
    pub fn new(first: usize, last: usize, seconds_per_frame: f32) -> Self {
        Self {
            first,
            last,
            timer: Timer::from_seconds(seconds_per_frame, TimerMode::Repeating),
        }
    }
}

fn advance_frames(time: Res<Time>, mut query: Query<(&mut FrameAnimation, &mut Sprite)>) {
    for (mut anim, mut sprite) in &mut query {
        anim.timer.tick(time.delta());
        if !anim.timer.just_finished() {
            continue;
        }
        let Some(atlas) = sprite.texture_atlas.as_mut() else {
            continue;
        };
        atlas.index = if atlas.index >= anim.last {
            anim.first
        } else {
            atlas.index + 1
        };
    }
}
