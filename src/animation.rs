//! Minimal frame animation for sprites whose frames sit side by side in one
//! PNG.
//!
//! Two of them, because a frame can be addressed two ways and the choice is
//! about who is spawning the sprite rather than about the art:
//!
//! * [`FrameAnimation`] steps a [`TextureAtlas`], which is how Bevy would have
//!   you do it — and needs a `Handle<TextureAtlasLayout>`, which needs
//!   `Assets<TextureAtlasLayout>` in reach at spawn time. The dogs have that:
//!   one resource holds both sheets and the layout they share.
//! * [`StripAnimation`] steps `Sprite::rect` instead, and needs nothing but
//!   the image. That is what an animated **prop** wants: a prop is spawned
//!   from a palette entry in four places, one of which (a screen's
//!   `OnEnter`) runs before every `Startup` system and so before any resource
//!   a `Startup` system would have built. Threading `Assets` through all four
//!   to save a `Rect` per sprite is the worse trade.
//!
//! Both run on real [`Time`], not the fixed timestep: a picture flickering is
//! a fact about the art and not about the world, so it does not slow down when
//! the game does.

use bevy::prelude::*;

pub struct AnimationPlugin;

impl Plugin for AnimationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (advance_frames, advance_strips));
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

/// Cycles `Sprite::rect` along a horizontal strip of square frames.
///
/// The frames are counted in **source texels** — the art's own pixels — like
/// an atlas grid is, so a 16px strip upscaled to a cell still steps by 16.
#[derive(Component)]
pub struct StripAnimation {
    /// The side of one frame, in source texels. Square, because every strip
    /// in this game is a prop or a character drawn to fill a cell.
    frame: f32,
    /// How many of them the current strip has.
    frames: u32,
    index: u32,
    timer: Timer,
}

impl StripAnimation {
    pub fn new(frame: f32, frames: u32, seconds_per_frame: f32) -> Self {
        Self {
            frame,
            frames: frames.max(1),
            index: 0,
            timer: Timer::from_seconds(seconds_per_frame, TimerMode::Repeating),
        }
    }

    /// The region of the strip the frame now showing covers — what
    /// `Sprite::rect` is set to.
    pub fn rect(&self) -> Rect {
        let left = self.index as f32 * self.frame;
        Rect::new(left, 0.0, left + self.frame, self.frame)
    }

    /// Show a different strip of the same art size from its first frame —
    /// a computer's screen coming on. Its own frame count, since two strips
    /// of one thing need not be the same length.
    pub fn restart(&mut self, frames: u32) {
        self.frames = frames.max(1);
        self.index = 0;
        self.timer.reset();
    }
}

fn advance_strips(time: Res<Time>, mut query: Query<(&mut StripAnimation, &mut Sprite)>) {
    for (mut anim, mut sprite) in &mut query {
        anim.timer.tick(time.delta());
        if !anim.timer.just_finished() {
            continue;
        }
        anim.index = (anim.index + 1) % anim.frames;
        sprite.rect = Some(anim.rect());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_strip_walks_across_the_art_a_frame_at_a_time_and_wraps() {
        let mut anim = StripAnimation::new(16.0, 3, 0.1);
        assert_eq!(anim.rect(), Rect::new(0.0, 0.0, 16.0, 16.0));
        anim.index = 2;
        assert_eq!(anim.rect(), Rect::new(32.0, 0.0, 48.0, 16.0));
        anim.index = (anim.index + 1) % anim.frames;
        assert_eq!(anim.index, 0, "back to the first frame");
    }

    #[test]
    fn switching_strips_starts_the_new_one_at_its_first_frame() {
        let mut anim = StripAnimation::new(16.0, 10, 0.1);
        anim.index = 9;
        anim.restart(11);
        assert_eq!(anim.index, 0);
        assert_eq!(anim.frames, 11);
        assert_eq!(anim.rect(), Rect::new(0.0, 0.0, 16.0, 16.0));
    }

    /// A strip with no frames in it would divide by zero every tick.
    #[test]
    fn a_strip_always_has_at_least_one_frame() {
        assert_eq!(StripAnimation::new(16.0, 0, 0.1).frames, 1);
        let mut anim = StripAnimation::new(16.0, 4, 0.1);
        anim.restart(0);
        assert_eq!(anim.frames, 1);
    }
}
