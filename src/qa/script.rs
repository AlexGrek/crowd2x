//! What a QA test is: a JSON file of inputs to feed the game and things to
//! check afterwards.
//!
//! The schema is deliberately flat and named after what a person would say —
//! `{"key": "enter"}`, `{"type": "office"}`, `{"pad": "south"}` — because the
//! point of writing tests as data is that they read as a description of the
//! session. Anything that needs a duration takes one; everything else is a tap.
//!
//! Steps come at two levels, and a test is expected to mix them:
//!
//! * **Intent** — `{"press": "create"}`, `{"goto": "maps"}`, `{"name": "office"}`,
//!   `{"tool": "wall brown"}`, `{"cursor_cell": {"x": 3, "y": 2}}`. These say
//!   what the session is doing and are addressed by *label*, so they survive
//!   the palette being reordered, a button moving, or a list growing a row.
//!   Reach for these unless the input itself is the thing under test.
//! * **Input** — `key`, `type`, `pad`, `stick`, `mouse`, `click`, `wheel`.
//!   These are the actual devices, and they are how you test that the devices
//!   work: that a gamepad can reach every button, that typing goes into the
//!   field and not into the palette. They depend on layout and focus order,
//!   which is the point.
//!
//! ```json
//! {
//!   "name": "create a map from the browser",
//!   "state": "maps",
//!   "given": { "maps": ["alpha"] },
//!   "steps": [
//!     { "key": "enter" },
//!     { "type": "office" },
//!     { "key": "enter" },
//!     { "expect_focus": "create" },
//!     { "key": "enter" },
//!     { "expect_state": "editor" },
//!     { "expect_map": "office" }
//!   ]
//! }
//! ```

use bevy::prelude::{GamepadButton, KeyCode, MouseButton};
use serde::Deserialize;

use crate::state::AppState;

/// Seconds to let the game settle before the first step: plugins start,
/// assets load, the first `OnEnter` runs.
const DEFAULT_SETTLE: f32 = 1.0;
/// Seconds between steps. Several frames, so a press is seen, acted on, and
/// its consequences (a state transition, a rebuilt list) have landed before
/// the next step looks at them.
const DEFAULT_GAP: f32 = 0.1;
/// How long a tap is held. Long enough to survive a frame either side.
pub const DEFAULT_TAP: f32 = 0.05;
/// A test that stops making progress fails rather than hanging a CI job.
const DEFAULT_TIMEOUT: f32 = 60.0;
/// Seconds either side of a screenshot: long enough for the frame being asked
/// for to have been drawn, and for the file to be written before the next step
/// changes what is on screen.
const DEFAULT_SHOT_DELAY: f32 = 0.5;

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Script {
    pub name: String,
    /// Screen to start on: `menu`, `maps`, `editor` or `game`.
    #[serde(default)]
    pub state: Option<String>,
    /// Maps that must exist before the test starts.
    #[serde(default)]
    pub given: Given,
    /// A map to have open when the app starts, instead of the scratch one.
    ///
    /// Which screen it opens *in* is [`Script::state`] — `editor` or `game`.
    /// It must be one of the maps in `given`, or already saved.
    #[serde(default)]
    pub open: Option<String>,
    #[serde(default = "default_settle")]
    pub settle: f32,
    #[serde(default = "default_gap")]
    pub gap: f32,
    #[serde(default = "default_timeout")]
    pub timeout: f32,
    /// Seconds to hold still either side of a screenshot.
    ///
    /// Captures are the one thing here that needs the renderer to have caught
    /// up rather than just the systems: a frame grabbed too early comes out
    /// pure black, which is a photograph of nothing rather than an error.
    #[serde(default = "default_shot_delay")]
    pub shot_delay: f32,
    pub steps: Vec<Step>,

    /// Where the script was read from, for naming its screenshots. Not part of
    /// the file.
    #[serde(skip)]
    pub source: String,
}

impl Script {
    /// The test's own name for its output directory, from the file name.
    pub fn stem(&self) -> String {
        std::path::Path::new(&self.source)
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "qa".to_string())
    }
}

/// The world a test starts in. Empty by default, which is itself a fixture:
/// most browser tests want to know exactly what is in the list.
#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct Given {
    #[serde(default)]
    pub maps: Vec<GivenMap>,
}

/// A map that exists before the test starts.
///
/// Three ways to say it, because tests want different things from a fixture:
///
/// * `"alpha"` — an empty map of that name. Enough for anything about the
///   *list*: browsing, copying, deleting.
/// * `{"name": "office", "file": "qa/fixtures/office.json"}` — a map kept on
///   disk, for a fixture worth looking at in the editor or sharing between
///   tests.
/// * `{"name": "office", "map": { ...a whole map document... }}` — the map
///   written inline, which keeps a test that depends on exactly which cells
///   are painted readable in one place.
///
/// A file or an inline document is loaded through the real format, so a
/// fixture that has drifted out of date fails at startup with the format's own
/// error rather than halfway through a test.
#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum GivenMap {
    Empty(String),
    Built {
        name: String,
        #[serde(default)]
        file: Option<String>,
        #[serde(default)]
        map: Option<serde_json::Value>,
    },
}

impl GivenMap {
    pub fn name(&self) -> &str {
        match self {
            GivenMap::Empty(name) => name,
            GivenMap::Built { name, .. } => name,
        }
    }
}

fn default_settle() -> f32 {
    DEFAULT_SETTLE
}

fn default_gap() -> f32 {
    DEFAULT_GAP
}

fn default_timeout() -> f32 {
    DEFAULT_TIMEOUT
}

fn default_shot_delay() -> f32 {
    DEFAULT_SHOT_DELAY
}

/// One line of a test.
///
/// Externally tagged, so every step is a one-key object naming what it does.
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    /// Do nothing for this many seconds.
    Wait(f32),
    /// Say what the next steps are for. Goes to the log, so a failing run
    /// reads as a story rather than as a list of presses.
    Note(String),

    /// Go straight to a screen, skipping however you would get there.
    Goto(String),
    /// Move the highlight to the widget with this label.
    Focus(String),
    /// Choose the widget with this label: the "click the create button"
    /// step, without caring where the create button is.
    Press(String),
    /// Put a name in the name field, as if it had been typed.
    Name(String),
    /// Select a palette entry by name, on whichever layer it lives.
    Tool(String),
    /// Put the editor cursor in the middle of a cell.
    CursorCell { x: i32, y: i32 },
    /// Put the editor cursor at a world position, in pixels.
    Cursor { x: f32, y: f32 },
    /// Tap a key: `enter`, `esc`, `tab`, `f5`, `a`, `1`, `up`.
    Key(String),
    /// Hold a key down — panning, or painting by dragging.
    HoldKey { key: String, seconds: f32 },
    /// Type text into whatever is listening, one character at a time.
    Type(String),
    /// Tap a gamepad button: `south`, `east`, `dpad_down`, `start`.
    Pad(String),
    HoldPad { button: String, seconds: f32 },
    /// Push a stick, then let it go.
    Stick {
        #[serde(default)]
        side: Side,
        x: f32,
        y: f32,
        seconds: f32,
    },
    /// Move the pointer, in canvas pixels from the top left.
    Mouse { x: f32, y: f32 },
    Click {
        #[serde(default)]
        button: Button,
        #[serde(default = "default_tap")]
        seconds: f32,
    },
    /// Scroll: positive is up, in list rows.
    Wheel(f32),
    /// Save a screenshot, named rather than pathed: the file lands in this
    /// test's own directory, numbered in the order the shots were taken.
    ///
    /// A test may take as many as it likes; that is the point of naming them.
    Shot(String),

    ExpectState(String),
    /// The current zoom, in screen pixels per canvas pixel.
    ///
    /// Zooming changes the size of the canvas rather than the scale of a
    /// camera, so it cannot be read off a screenshot without counting texels —
    /// which is exactly the kind of thing a test should not have to do.
    ExpectZoom(u32),
    /// The focused widget's label, which is how a test says "the highlight is
    /// where I think it is" without knowing about entities.
    ExpectFocus(String),
    ExpectMap(String),
    ExpectNoMap(String),
    /// A cell of a *saved* map, so painting can be checked against the file
    /// rather than against the screen.
    ExpectTile {
        map: String,
        x: i32,
        y: i32,
        terrain: String,
    },
}

fn default_tap() -> f32 {
    DEFAULT_TAP
}

#[derive(Deserialize, Debug, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    #[default]
    Left,
    Right,
}

#[derive(Deserialize, Debug, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Button {
    #[default]
    Left,
    Right,
    Middle,
}

impl Button {
    pub fn code(self) -> MouseButton {
        match self {
            Button::Left => MouseButton::Left,
            Button::Right => MouseButton::Right,
            Button::Middle => MouseButton::Middle,
        }
    }
}

impl Script {
    pub fn parse(json: &str) -> Result<Script, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// The screen the test starts on.
    ///
    /// `Err` names what this build actually has, which is the answer a script
    /// asking for a screen that does not exist yet — `game` — needs to get.
    /// Falling back to the menu instead would run the whole test against the
    /// wrong screen and report a pile of confusing failures.
    pub fn initial_state(&self) -> Result<Option<AppState>, String> {
        match &self.state {
            None => Ok(None),
            Some(name) => match AppState::from_name(name) {
                Some(state) => Ok(Some(state)),
                None => Err(format!(
                    "this build has no {name:?} screen; it has: {}",
                    AppState::NAMES
                )),
            },
        }
    }
}

const LETTERS: [KeyCode; 26] = [
    KeyCode::KeyA, KeyCode::KeyB, KeyCode::KeyC, KeyCode::KeyD, KeyCode::KeyE, KeyCode::KeyF,
    KeyCode::KeyG, KeyCode::KeyH, KeyCode::KeyI, KeyCode::KeyJ, KeyCode::KeyK, KeyCode::KeyL,
    KeyCode::KeyM, KeyCode::KeyN, KeyCode::KeyO, KeyCode::KeyP, KeyCode::KeyQ, KeyCode::KeyR,
    KeyCode::KeyS, KeyCode::KeyT, KeyCode::KeyU, KeyCode::KeyV, KeyCode::KeyW, KeyCode::KeyX,
    KeyCode::KeyY, KeyCode::KeyZ,
];

const DIGITS: [KeyCode; 10] = [
    KeyCode::Digit0, KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4,
    KeyCode::Digit5, KeyCode::Digit6, KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9,
];

/// A key by the name a test writer would use.
///
/// Aliases are on purpose: a test that says `esc` and one that says `escape`
/// mean the same thing, and neither should have to be looked up.
pub fn key_code(name: &str) -> Option<KeyCode> {
    let name = name.trim().to_ascii_lowercase();
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_lowercase() {
            return Some(LETTERS[c as usize - 'a' as usize]);
        }
        if c.is_ascii_digit() {
            return Some(DIGITS[c as usize - '0' as usize]);
        }
    }

    Some(match name.as_str() {
        "enter" | "return" => KeyCode::Enter,
        "escape" | "esc" => KeyCode::Escape,
        "tab" => KeyCode::Tab,
        "space" => KeyCode::Space,
        "backspace" => KeyCode::Backspace,
        "up" | "arrow_up" => KeyCode::ArrowUp,
        "down" | "arrow_down" => KeyCode::ArrowDown,
        "left" | "arrow_left" => KeyCode::ArrowLeft,
        "right" | "arrow_right" => KeyCode::ArrowRight,
        "f5" => KeyCode::F5,
        "f12" => KeyCode::F12,
        _ => return None,
    })
}

/// The character a key produces, for the text-entry path. `None` for keys that
/// are not text — those travel as their own logical key instead.
pub fn key_char(key: KeyCode) -> Option<char> {
    if let Some(index) = LETTERS.iter().position(|&code| code == key) {
        return char::from_u32('a' as u32 + index as u32);
    }
    if let Some(index) = DIGITS.iter().position(|&code| code == key) {
        return char::from_u32('0' as u32 + index as u32);
    }
    (key == KeyCode::Space).then_some(' ')
}

/// A gamepad button by name, in the same vocabulary the HUD uses.
pub fn gamepad_button(name: &str) -> Option<GamepadButton> {
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "south" | "a" => GamepadButton::South,
        "east" | "b" => GamepadButton::East,
        "north" | "y" => GamepadButton::North,
        "west" | "x" => GamepadButton::West,
        "start" => GamepadButton::Start,
        "select" => GamepadButton::Select,
        "dpad_up" => GamepadButton::DPadUp,
        "dpad_down" => GamepadButton::DPadDown,
        "dpad_left" => GamepadButton::DPadLeft,
        "dpad_right" => GamepadButton::DPadRight,
        "left_bumper" | "l1" => GamepadButton::LeftTrigger,
        "right_bumper" | "r1" => GamepadButton::RightTrigger,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_script_is_the_json_a_test_writer_would_write() {
        let script = Script::parse(
            r#"{
                "name": "create a map",
                "state": "maps",
                "given": { "maps": ["alpha"] },
                "steps": [
                    { "key": "enter" },
                    { "type": "office" },
                    { "wait": 0.5 },
                    { "stick": { "side": "left", "x": 1.0, "y": 0.0, "seconds": 0.4 } },
                    { "click": { "button": "right" } },
                    { "expect_tile": { "map": "office", "x": 1, "y": 2, "terrain": "floor" } }
                ]
            }"#,
        )
        .expect("valid script");

        assert_eq!(script.name, "create a map");
        assert_eq!(script.initial_state(), Ok(Some(AppState::Maps)));
        assert_eq!(script.given.maps[0].name(), "alpha");
        assert_eq!(script.steps.len(), 6);
        // Defaults are filled in so a short test stays short.
        assert_eq!(script.gap, DEFAULT_GAP);
        assert_eq!(script.timeout, DEFAULT_TIMEOUT);
        assert_eq!(script.shot_delay, DEFAULT_SHOT_DELAY);
    }

    #[test]
    fn intent_steps_and_input_steps_live_in_the_same_list() {
        let script = Script::parse(
            r#"{
                "name": "mixed",
                "steps": [
                    { "note": "high level first" },
                    { "goto": "maps" },
                    { "name": "office" },
                    { "press": "create" },
                    { "tool": "wall brown" },
                    { "cursor_cell": { "x": 3, "y": 2 } },
                    { "click": {} },
                    { "key": "f5" }
                ]
            }"#,
        )
        .expect("valid script");
        assert_eq!(script.steps.len(), 8);
        assert!(matches!(script.steps[3], Step::Press(ref label) if label == "create"));
    }

    /// A misspelled step name has to be an error. Silently skipping one would
    /// make a test pass by not testing anything.
    #[test]
    fn an_unknown_step_is_refused() {
        let error = Script::parse(r#"{"name": "x", "steps": [{"presss": "enter"}]}"#).unwrap_err();
        assert!(error.to_string().contains("presss"), "{error}");
    }

    #[test]
    fn an_unknown_field_is_refused() {
        // Usually a typo in a field name, which would otherwise be ignored and
        // leave the test doing something other than what it says.
        let error = Script::parse(r#"{"name": "x", "stpes": [], "steps": []}"#).unwrap_err();
        assert!(error.to_string().contains("stpes"), "{error}");
    }

    #[test]
    fn keys_are_named_the_way_someone_would_say_them() {
        assert_eq!(key_code("enter"), Some(KeyCode::Enter));
        assert_eq!(key_code("Esc"), Some(KeyCode::Escape));
        assert_eq!(key_code("escape"), Some(KeyCode::Escape));
        assert_eq!(key_code("a"), Some(KeyCode::KeyA));
        assert_eq!(key_code("Z"), Some(KeyCode::KeyZ));
        assert_eq!(key_code("7"), Some(KeyCode::Digit7));
        assert_eq!(key_code("up"), Some(KeyCode::ArrowUp));
        assert_eq!(key_code("f5"), Some(KeyCode::F5));
        assert_eq!(key_code("wat"), None);
    }

    #[test]
    fn every_letter_and_digit_key_round_trips_to_its_character() {
        for c in ('a'..='z').chain('0'..='9') {
            let key = key_code(&c.to_string()).expect("letter or digit");
            assert_eq!(key_char(key), Some(c), "{c}");
        }
        assert_eq!(key_char(KeyCode::Space), Some(' '));
        assert_eq!(key_char(KeyCode::Enter), None);
    }

    #[test]
    fn gamepad_buttons_can_be_named_by_face_or_by_letter() {
        assert_eq!(gamepad_button("south"), Some(GamepadButton::South));
        assert_eq!(gamepad_button("A"), Some(GamepadButton::South));
        assert_eq!(gamepad_button("dpad_down"), Some(GamepadButton::DPadDown));
        assert_eq!(gamepad_button("r1"), Some(GamepadButton::RightTrigger));
        assert_eq!(gamepad_button("turbo"), None);
    }

    #[test]
    fn a_test_names_its_screenshot_directory_after_its_own_file() {
        let mut script = Script::parse(r#"{"name": "x", "steps": []}"#).unwrap();
        script.source = "qa/browse_and_delete.json".into();
        assert_eq!(script.stem(), "browse_and_delete");
        // A script that came from nowhere still has somewhere to put shots.
        script.source = String::new();
        assert_eq!(script.stem(), "qa");
    }

    /// The screen a test asks for is not negotiable: a script written for a
    /// screen this build has not grown yet must say so, not quietly run
    /// against the menu and fail somewhere else.
    #[test]
    fn a_screen_this_build_does_not_have_is_an_error_naming_the_ones_it_does() {
        let script = Script::parse(r#"{"name": "x", "state": "credits", "steps": []}"#).unwrap();
        let error = script.initial_state().unwrap_err();
        assert!(error.contains("credits"), "{error}");
        assert!(error.contains("editor"), "{error}");
        assert!(error.contains("game"), "{error}");
    }

    /// ...and the game screen, written about long before it existed, is one of
    /// the ones it does have now.
    #[test]
    fn a_test_can_start_in_the_game() {
        let script = Script::parse(r#"{"name": "x", "state": "game", "steps": []}"#).unwrap();
        assert_eq!(script.initial_state(), Ok(Some(AppState::Game)));
    }

    #[test]
    fn a_script_with_no_screen_takes_whatever_the_app_would_start_on() {
        let script = Script::parse(r#"{"name": "x", "steps": []}"#).unwrap();
        assert_eq!(script.initial_state(), Ok(None));
    }

    #[test]
    fn a_fixture_map_can_be_named_kept_in_a_file_or_written_inline() {
        let script = Script::parse(
            r#"{
                "name": "fixtures",
                "given": { "maps": [
                    "alpha",
                    { "name": "office", "file": "qa/fixtures/office.json" },
                    { "name": "studio", "map": { "version": 1 } }
                ] },
                "open": "office",
                "steps": []
            }"#,
        )
        .expect("valid script");

        assert_eq!(script.open.as_deref(), Some("office"));
        let names: Vec<&str> = script.given.maps.iter().map(GivenMap::name).collect();
        assert_eq!(names, ["alpha", "office", "studio"]);
        assert!(matches!(script.given.maps[0], GivenMap::Empty(_)));
        assert!(matches!(
            script.given.maps[1],
            GivenMap::Built { file: Some(_), .. }
        ));
        assert!(matches!(
            script.given.maps[2],
            GivenMap::Built { map: Some(_), .. }
        ));
    }
}
