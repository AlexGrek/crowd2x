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
    /// Playing a map: the simulation, or as much of it as exists.
    Game,
}

impl AppState {
    /// Every screen this build has, by the name [`AppState::from_name`] takes.
    ///
    /// Kept next to that function so a message listing the screens cannot go
    /// stale when one is added — which is exactly what a script asking for a
    /// screen that does not exist needs to be told.
    pub const NAMES: &'static str = "menu, maps, editor, game";

    /// Parse the value of `CROWD2X_STATE`, used by the debug harness to boot
    /// straight into a screen so a capture does not have to click through the
    /// menu. Unknown values fall back to the default.
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "menu" | "mainmenu" | "main_menu" => Some(Self::MainMenu),
            "maps" | "browser" | "files" => Some(Self::Maps),
            "editor" | "edit" => Some(Self::Editor),
            "game" | "play" => Some(Self::Game),
            _ => None,
        }
    }
}
