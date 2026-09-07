//! Top-level application state: which screen the app is showing.
//!
//! Systems belonging to a screen are gated with `run_if(in_state(..))`, and
//! entities belonging to one carry `DespawnOnExit(..)` so they clean themselves
//! up on the way out instead of needing a teardown system each.

use bevy::prelude::*;

#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AppState {
    #[default]
    MainMenu,
    /// The map browser: saved maps, and everything done to a map from outside
    /// the editor.
    Maps,
    Editor,
}

impl AppState {
    /// Parse the value of `CROWD2X_STATE`, used by the debug harness to boot
    /// straight into a screen so a capture does not have to click through the
    /// menu. Unknown values fall back to the default.
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "menu" | "mainmenu" | "main_menu" => Some(Self::MainMenu),
            "maps" | "browser" | "files" => Some(Self::Maps),
            "editor" | "edit" => Some(Self::Editor),
            _ => None,
        }
    }
}
