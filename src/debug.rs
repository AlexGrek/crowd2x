//! Development capture harness.
//!
//! Lets a frame of the running game be pulled out to a PNG, either by hand
//! while playing or unattended from a script. The unattended path exists so an
//! agent (or CI) can *look* at what the renderer actually produced instead of
//! trusting that it compiled.
//!
//! # By hand
//!
//! Press `F12`. The frame lands in `screenshots/shot-<epoch-millis>.png`.
//!
//! # Scripted
//!
//! Driven entirely by environment variables, so no code changes are needed:
//!
//! | Variable | Meaning | Default |
//! | --- | --- | --- |
//! | `CROWD2X_SHOT` | Write one screenshot here, then quit. Enables scripted mode. | off |
//! | `CROWD2X_SHOT_DELAY` | Seconds to run before capturing, so assets finish loading. | `3.0` |
//! | `CROWD2X_WINDOW` | Resize the window to `WxH` on startup, before capturing. | window default |
//! | `CROWD2X_EXIT` | Quit after this many seconds even without a capture. | off |
//! | `CROWD2X_STATE` | Boot straight into `menu`, `maps` or `editor`. | `menu` |
//! | `CROWD2X_MAP` | Open this saved map in the editor, instead of a scratch one. | scratch |
//! | `CROWD2X_HIDE_UI` | Hide all `bevy_ui`, leaving only the canvas. | off |
//!
//! ```sh
//! CROWD2X_SHOT=/tmp/frame.png cargo run
//! CROWD2X_SHOT=/tmp/wide.png CROWD2X_WINDOW=1002x602 CROWD2X_SHOT_DELAY=2 cargo run
//! CROWD2X_SHOT=/tmp/editor.png CROWD2X_STATE=editor CROWD2X_MAP=office cargo run
//! CROWD2X_EXIT=5 cargo run          # smoke run, no capture
//! ```
//!
//! The window still opens — this is not headless rendering, it just does not
//! need anyone watching it.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use bevy::window::PrimaryWindow;

use crate::browser::maps_dir;
use crate::editor::CurrentMap;
use crate::map::MapStore;
use crate::state::AppState;

const ENV_SHOT: &str = "CROWD2X_SHOT";
const ENV_SHOT_DELAY: &str = "CROWD2X_SHOT_DELAY";
const ENV_WINDOW: &str = "CROWD2X_WINDOW";
const ENV_EXIT: &str = "CROWD2X_EXIT";
const ENV_STATE: &str = "CROWD2X_STATE";
const ENV_MAP: &str = "CROWD2X_MAP";
const ENV_HIDE_UI: &str = "CROWD2X_HIDE_UI";

/// Generous on purpose: with `bevy_ui` in the build, the first frame does not
/// reach the screen for a couple of seconds on a cold start, and capturing
/// early silently produces a pure black PNG rather than an error.
const DEFAULT_SHOT_DELAY: f32 = 3.0;
const MANUAL_SHOT_DIR: &str = "screenshots";
const MANUAL_SHOT_KEY: KeyCode = KeyCode::F12;

pub struct DebugPlugin;

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        let script = CaptureScript::from_env();
        if script.is_active() {
            info!("debug capture script: {script:?}");
        }
        app.insert_resource(script)
            .add_systems(Startup, apply_window_override)
            .add_systems(Update, (manual_screenshot, run_capture_script));

        open_map_from_env(app);

        if std::env::var(ENV_HIDE_UI).is_ok() {
            info!("debug: {ENV_HIDE_UI} set, hiding all UI");
            app.add_systems(Update, hide_ui);
        }
    }
}

/// Hide every UI root, so a capture contains only the upscaled canvas.
///
/// `bevy_ui` draws at window resolution on top of the finished upscale, so any
/// text on screen makes `tools/check_pixel_grid.py` report non-uniform blocks.
/// Turning the UI off is what makes that check meaningful again.
fn hide_ui(mut roots: Query<&mut Visibility, (With<Node>, Without<ChildOf>)>) {
    for mut visibility in &mut roots {
        if *visibility != Visibility::Hidden {
            *visibility = Visibility::Hidden;
        }
    }
}

/// Screen the app boots into, from `CROWD2X_STATE`.
///
/// Without this a scripted capture only ever sees the main menu, which makes
/// the editor impossible to check unattended.
pub fn initial_state() -> AppState {
    let Ok(raw) = std::env::var(ENV_STATE) else {
        return AppState::default();
    };
    AppState::from_name(&raw).unwrap_or_else(|| {
        warn!("debug: unknown {ENV_STATE}={raw:?}, starting in the menu");
        AppState::default()
    })
}

/// Open a saved map without going through the browser.
///
/// Done here in `build` rather than in a `Startup` system, and it has to be:
/// booting straight into the editor runs `OnEnter(Editor)` — which draws the
/// map — *before* every `Startup` system. `DebugPlugin` is registered after
/// `EditorPlugin`, so this replaces the scratch map that plugin inserts.
fn open_map_from_env(app: &mut App) {
    let Ok(name) = std::env::var(ENV_MAP) else {
        return;
    };
    match MapStore::new(maps_dir()).load(&name) {
        Ok(map) => {
            info!("debug: {ENV_MAP}={name:?}, opening it in the editor");
            app.insert_resource(CurrentMap::new(name, map));
        }
        Err(error) => error!("debug: cannot open {ENV_MAP}={name:?}: {error}"),
    }
}

/// The scripted run described by the environment, if any.
#[derive(Resource, Debug, Default)]
struct CaptureScript {
    shot_path: Option<PathBuf>,
    shot_delay: f32,
    window: Option<UVec2>,
    exit_after: Option<f32>,
    /// Set once the capture has been requested, so it only happens once.
    fired: bool,
}

impl CaptureScript {
    fn from_env() -> Self {
        Self {
            shot_path: std::env::var(ENV_SHOT).ok().map(PathBuf::from),
            shot_delay: std::env::var(ENV_SHOT_DELAY)
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(DEFAULT_SHOT_DELAY),
            window: std::env::var(ENV_WINDOW).ok().and_then(|v| parse_size(&v)),
            exit_after: std::env::var(ENV_EXIT).ok().and_then(|v| v.parse().ok()),
            fired: false,
        }
    }

    fn is_active(&self) -> bool {
        self.shot_path.is_some() || self.window.is_some() || self.exit_after.is_some()
    }
}

/// Parse a `WxH` size, e.g. `1002x602`.
fn parse_size(raw: &str) -> Option<UVec2> {
    let (w, h) = raw.split_once(['x', 'X'])?;
    Some(UVec2::new(w.trim().parse().ok()?, h.trim().parse().ok()?))
}

fn apply_window_override(
    script: Res<CaptureScript>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    let Some(size) = script.window else {
        return;
    };
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    window.resolution.set(size.x as f32, size.y as f32);
    info!("debug: window forced to {}x{}", size.x, size.y);
}

/// F12 saves a frame while playing.
fn manual_screenshot(mut commands: Commands, keys: Res<ButtonInput<KeyCode>>) {
    if !keys.just_pressed(MANUAL_SHOT_KEY) {
        return;
    }
    if let Err(e) = std::fs::create_dir_all(MANUAL_SHOT_DIR) {
        error!("debug: cannot create {MANUAL_SHOT_DIR}/: {e}");
        return;
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = PathBuf::from(MANUAL_SHOT_DIR).join(format!("shot-{stamp}.png"));
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path));
}

/// Drives the unattended capture: wait, shoot, quit.
fn run_capture_script(
    mut commands: Commands,
    mut script: ResMut<CaptureScript>,
    time: Res<Time>,
    mut exit: MessageWriter<AppExit>,
) {
    let elapsed = time.elapsed_secs();

    if let Some(deadline) = script.exit_after
        && elapsed >= deadline
    {
        info!("debug: CROWD2X_EXIT reached, quitting");
        exit.write(AppExit::Success);
        return;
    }

    if script.fired || script.shot_path.is_none() || elapsed < script.shot_delay {
        return;
    }

    let path = script.shot_path.clone().expect("checked above");
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        error!("debug: cannot create {}: {e}", dir.display());
    }

    script.fired = true;
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path))
        // save_to_disk writes synchronously, and AppExit is only acted on once
        // the update finishes, so the file is always complete before we quit.
        .observe(|_: On<bevy::render::view::screenshot::ScreenshotCaptured>,
                  mut exit: MessageWriter<AppExit>| {
            exit.write(AppExit::Success);
        });
}
