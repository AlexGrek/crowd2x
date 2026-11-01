//! Speech presentation: short words rise and fade; longer speech has a fitted bubble.
use super::actors::{Sim, world_pos};
use crate::characters::depth_for;
use crate::render::WORLD_LAYER;
use crate::sim::{Uid, sound::Volume};
use crate::state::AppState;
use crate::view::VisibleArea;
use bevy::prelude::*;
use bevy::sprite::Anchor;
use bevy::text::{TextBounds, TextLayoutInfo};

const SHORT_LIMIT: usize = 24;
const TEXT_SIZE: f32 = 8.0;
#[derive(Resource, Default)]
struct SeenSpeech(u64);
#[derive(Component)]
struct Speech {
    speaker: Uid,
    origin: Vec2,
    age: f32,
    duration: f32,
    bubble: bool,
}
#[derive(Component)]
struct Ink {
    root: Entity,
    base: Color,
}
#[derive(Component)]
struct BubblePanel {
    root: Entity,
    border: bool,
}
#[derive(Component)]
struct SpeechWords(Entity);

pub struct SpeechPlugin;
impl Plugin for SpeechPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SeenSpeech>()
            .add_systems(OnExit(AppState::Game), reset)
            .add_systems(
                Update,
                (read_speech, animate, fit_bubbles)
                    .chain()
                    .in_set(super::SpriteSync)
                    .run_if(in_state(AppState::Game).and_then(resource_exists::<Sim>)),
            );
    }
}
fn reset(mut seen: ResMut<SeenSpeech>) {
    seen.0 = 0;
}
fn read_speech(
    mut commands: Commands,
    sim: Res<Sim>,
    area: Res<VisibleArea>,
    mut seen: ResMut<SeenSpeech>,
) {
    let field = sim.0.sound();
    for sound in field.since(seen.0) {
        let origin = sim
            .0
            .entities()
            .get(sound.speaker)
            .map(|e| world_pos(e.position()))
            .unwrap_or(Vec2::new(
                (sound.at.x as f32 + 0.5) * 48.0,
                (sound.at.y as f32 + 0.5) * 48.0,
            ));
        if area.should_keep(origin) {
            show(
                &mut commands,
                sound.speaker,
                origin,
                sound.phrase.text(),
                sound.volume,
            );
        }
    }
    seen.0 = field.sequence();
}
fn is_bubble(text: &str) -> bool {
    text.chars().count() > SHORT_LIMIT || text.contains('\n')
}
fn lifetime(text: &str) -> f32 {
    if is_bubble(text) {
        (2.5 + text.chars().count() as f32 * 0.04).min(9.0)
    } else {
        1.8
    }
}
/// Shared by real emissions and the debug capture preview.
pub(crate) fn show(
    commands: &mut Commands,
    speaker: Uid,
    origin: Vec2,
    text: &str,
    volume: Volume,
) {
    let bubble = is_bubble(text);
    let colour = if bubble {
        Color::srgb(0.08, 0.09, 0.12)
    } else if volume == Volume::Shout {
        Color::srgb(1.0, 0.87, 0.34)
    } else {
        Color::WHITE
    };
    let root = commands
        .spawn((
            Name::new("unit speech"),
            Transform::from_translation(
                (origin + Vec2::Y * 36.0)
                    .round()
                    .extend(depth_for(origin.y) + 0.009),
            ),
            Visibility::default(),
            DespawnOnExit(AppState::Game),
            Speech {
                speaker,
                origin,
                age: 0.0,
                duration: lifetime(text),
                bubble,
            },
        ))
        .id();
    if bubble {
        for border in [true, false] {
            let base = if border {
                Color::srgb(0.08, 0.09, 0.12)
            } else {
                Color::srgb(1.0, 0.98, 0.9)
            };
            commands.spawn((
                Sprite::from_color(base, Vec2::ONE),
                Anchor::BOTTOM_LEFT,
                Transform::from_xyz(0.0, 0.0, if border { 0.0 } else { 0.0001 }),
                WORLD_LAYER,
                ChildOf(root),
                Ink { root, base },
                BubblePanel { root, border },
            ));
        }
        // A small stepped tail points down at the speaker, aligned to canvas pixels.
        for (size, y, base, z) in [
            (
                Vec2::new(5.0, 3.0),
                -2.0,
                Color::srgb(0.08, 0.09, 0.12),
                0.0002,
            ),
            (
                Vec2::new(3.0, 2.0),
                -1.0,
                Color::srgb(1.0, 0.98, 0.9),
                0.0003,
            ),
        ] {
            commands.spawn((
                Sprite::from_color(base, size),
                Anchor::BOTTOM_LEFT,
                Transform::from_xyz(-(size.x / 2.0).floor(), y, z),
                WORLD_LAYER,
                ChildOf(root),
                Ink { root, base },
            ));
        }
    } else {
        // A dark one-pixel shadow keeps the moving words legible over terrain.
        commands.spawn((
            Text2d::new(text),
            TextFont::from_font_size(TEXT_SIZE),
            TextColor(Color::BLACK),
            Anchor::BOTTOM_CENTER,
            Transform::from_xyz(1.0, -1.0, 0.0),
            TextLayout::no_wrap(),
            WORLD_LAYER,
            ChildOf(root),
            Ink {
                root,
                base: Color::BLACK,
            },
        ));
    }
    commands.spawn((
        Text2d::new(text),
        TextFont::from_font_size(TEXT_SIZE),
        TextColor(colour),
        TextLayout::justify(Justify::Center),
        if bubble {
            TextBounds::new_horizontal(110.0)
        } else {
            TextBounds::UNBOUNDED
        },
        Anchor::BOTTOM_CENTER,
        Transform::from_xyz(0.0, if bubble { 4.0 } else { 0.0 }, 0.0004),
        WORLD_LAYER,
        ChildOf(root),
        Ink { root, base: colour },
        SpeechWords(root),
    ));
}
fn alpha(s: &Speech) -> f32 {
    ((s.duration - s.age) / if s.bubble { 0.5 } else { 1.1 }).clamp(0.0, 1.0)
}
fn animate(
    mut commands: Commands,
    time: Res<Time>,
    sim: Res<Sim>,
    mut roots: Query<(Entity, &mut Speech, &mut Transform)>,
    mut inks: Query<(&Ink, Option<&mut TextColor>, Option<&mut Sprite>)>,
) {
    for (id, mut speech, mut transform) in &mut roots {
        speech.age += time.delta_secs();
        if speech.age >= speech.duration {
            commands.entity(id).despawn();
            continue;
        }
        let origin = if speech.bubble {
            sim.0
                .entities()
                .get(speech.speaker)
                .map(|e| world_pos(e.position()))
                .unwrap_or(speech.origin)
        } else {
            speech.origin
        };
        let rise = if speech.bubble {
            0.0
        } else {
            speech.age * 15.0
        };
        transform.translation = (origin + Vec2::Y * (36.0 + rise))
            .round()
            .extend(depth_for(origin.y) + 0.009);
    }
    for (ink, text, sprite) in &mut inks {
        if let Ok((_, speech, _)) = roots.get(ink.root) {
            let colour = ink.base.with_alpha(alpha(speech));
            if let Some(mut text) = text {
                text.0 = colour;
            }
            if let Some(mut sprite) = sprite {
                sprite.color = colour;
            }
        }
    }
}
fn fit_bubbles(
    words: Query<(&SpeechWords, &TextLayoutInfo)>,
    mut panels: Query<(&BubblePanel, &mut Sprite, &mut Transform)>,
) {
    for (panel, mut sprite, mut transform) in &mut panels {
        if let Some((_, layout)) = words.iter().find(|(word, _)| word.0 == panel.root) {
            let padding = if panel.border {
                Vec2::new(10.0, 8.0)
            } else {
                Vec2::new(8.0, 6.0)
            };
            let size = layout.size.ceil() + padding;
            sprite.custom_size = Some(size);
            transform.translation.x = -(size.x / 2.0).floor();
            transform.translation.y = if panel.border { 0.0 } else { 1.0 };
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn short_phrases_float_and_long_or_multiline_speech_gets_time_to_read() {
        assert!(!is_bubble("GET BACK IN!"));
        assert!(is_bubble(
            "It is dangerous outside. Get back inside the walls."
        ));
        assert!(is_bubble("Hello\nthere"));
        assert!(
            lifetime("It is dangerous outside. Get back inside the walls.")
                > lifetime("GET BACK IN!")
        );
        let mut speech = Speech {
            speaker: Uid::new(crate::sim::EntityType::Human, 1),
            origin: Vec2::ZERO,
            age: 0.0,
            duration: 1.8,
            bubble: false,
        };
        assert_eq!(alpha(&speech), 1.0);
        speech.age = 1.4;
        assert!(alpha(&speech) > 0.0 && alpha(&speech) < 0.5);
        speech.age = 1.8;
        assert_eq!(alpha(&speech), 0.0);
    }
}
