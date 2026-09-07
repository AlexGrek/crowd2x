//! crowd2x - a 2D pixel-art crowd simulation, rebuilt on Bevy.

mod animation;
mod awake;
mod browser;
mod characters;
mod debug;
mod editor;
mod map;
mod menu;
mod qa;
mod render;
mod state;
mod ui;

use bevy::prelude::*;
use bevy::window::WindowResolution;

use crate::render::PIXEL_SCALE;

// Returning the exit status rather than discarding it: a scripted QA run
// reports its result that way, and `App::run` is the only thing that knows it.
fn main() -> AppExit {
    // A QA script chooses the screen it starts on, so it is read before the
    // app is built; without one this is the ordinary debug/default choice.
    let script = qa::script_from_env();
    let initial_state = qa::initial_state(script.as_ref()).unwrap_or_else(debug::initial_state);

    App::new()
        .add_plugins(
            DefaultPlugins
                // Pixel art must never be filtered when sampled.
                .set(ImagePlugin::default_nearest())
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "crowd2x".into(),
                        // Overriding the scale factor makes logical and physical
                        // pixels identical, so the upscale factor is exactly
                        // PIXEL_SCALE on every display, HiDPI included.
                        resolution: WindowResolution::new(320 * PIXEL_SCALE, 180 * PIXEL_SCALE)
                            .with_scale_factor_override(1.0),
                        ..default()
                    }),
                    ..default()
                }),
        )
        // Must come after DefaultPlugins, which brings the state machinery.
        .insert_state(initial_state)
        .add_plugins((
            render::PixelRenderPlugin,
            ui::UiPlugin,
            animation::AnimationPlugin,
            characters::CharacterPlugin,
            menu::MainMenuPlugin,
            browser::BrowserPlugin,
            editor::EditorPlugin,
            awake::AwakePlugin,
            debug::DebugPlugin,
            qa::QaPlugin::new(script),
        ))
        .run()
}
