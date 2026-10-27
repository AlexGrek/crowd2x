//! crowd2x - a 2D pixel-art crowd simulation, rebuilt on Bevy.

mod animation;
mod awake;
mod browser;
mod characters;
mod debug;
mod editor;
mod game;
mod lab;
mod lighting;
mod map;
mod menu;
mod qa;
mod render;
mod save;
mod saves;
mod settings;
mod sim;
mod state;
mod ui;
mod view;

use bevy::prelude::*;
use bevy::window::WindowResolution;

use crate::render::PIXEL_SCALE;

// Returning the exit status rather than discarding it: a scripted QA run
// reports its result that way, and `App::run` is the only thing that knows it.
fn main() -> AppExit {
    // `crowd2x lab ...` lives a world through headless (`src/lab/`): no
    // window, no Bevy app at all, so it is decided before one is built.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("lab") {
        return match lab::main(&args[2..]) {
            0 => AppExit::Success,
            code => AppExit::from_code(code.clamp(1, 255) as u8),
        };
    }

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
            view::ViewPlugin,
            ui::UiPlugin,
            animation::AnimationPlugin,
            characters::CharacterPlugin,
            menu::MainMenuPlugin,
            settings::SettingsPlugin,
            browser::BrowserPlugin,
            editor::EditorPlugin,
            game::GamePlugin,
            lighting::LightingPlugin,
            awake::AwakePlugin,
            debug::DebugPlugin,
            // After the editor and the debug harness, each of which may put a
            // map in `CurrentMap` as it is built: `CROWD2X_LOAD` wins.
            saves::SavesPlugin,
            qa::QaPlugin::new(script),
        ))
        .run()
}
