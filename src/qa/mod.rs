//! Scripted QA: drive the real binary from a JSON file and check what happened.
//!
//! `cargo test` can check the parts of this game that are plain Rust, and
//! `debug.rs` can photograph a frame, but neither can answer the question that
//! actually matters for an interface: *if someone presses these buttons in this
//! order, does the right thing happen?* This runs the whole app — window,
//! renderer, states, plugins — and presses the buttons.
//!
//! ```sh
//! CROWD2X_QA=qa/create_map.json cargo run     # one test, exit code says pass
//! python3 tools/qa.py                         # every test in qa/
//! ```
//!
//! Input is injected where the operating system would have put it, not where
//! it is convenient to fake it:
//!
//! * **Keys** press `ButtonInput<KeyCode>` *and* send a `KeyboardInput`
//!   message, because the game reads both — held keys from the first, typed
//!   text from the second.
//! * **A gamepad** is a real one as far as the game can tell: the test spawns
//!   an entity, sends a connection message, and then the raw button and axis
//!   messages a driver would send. `bevy_input` turns those into the `Gamepad`
//!   component the game queries, so nothing in the game knows the difference.
//! * **The pointer** moves by setting the window's cursor position, which is
//!   the same field `bevy_ui`'s focus system reads to decide what is hovered.
//!   Clicks then land on the widget under it, so a mouse test exercises the
//!   real hover-and-click path rather than calling a handler directly.
//!
//! Screenshots are named, not pathed: `{"shot": "the file list"}` writes
//! `qa-screenshots/<test>/01-the-file-list.png`, numbered in the order they
//! were taken, so a test can photograph as many moments as it wants without
//! inventing file names. A capture that beat the renderer to the frame comes
//! back as one flat colour rather than as an error, so every shot is inspected
//! and retaken until it has something in it — waiting a fixed time instead is
//! guesswork, and the guess is wrong on a cold start. The directory is per test and wiped by `tools/qa.py`
//! before each run, so what is in it is always from the last run. Everything
//! there is gitignored: a screenshot is evidence, not source.
//!
//! Assertions are deliberately about *outcomes* — which screen is up, what is
//! focused, what is on disk — and not about internals, so a test keeps passing
//! when the code behind it is rearranged. Checking a tile reads the **saved**
//! map, so "I painted a floor" is only true once the file says so.
//!
//! Point `CROWD2X_MAPS` at a scratch directory when running these, or a test
//! will delete maps somebody meant to keep; `tools/qa.py` does that for you.

pub mod script;

use std::num::NonZero;

use bevy::input::gamepad::{
    GamepadConnection, GamepadConnectionEvent, RawGamepadAxisChangedEvent,
    RawGamepadButtonChangedEvent, RawGamepadEvent,
};
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::{MouseButtonInput, MouseScrollUnit, MouseWheel};
use bevy::ecs::system::SystemParam;
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::image::Image;
use bevy::render::view::screenshot::{save_to_disk, Screenshot, ScreenshotCaptured};
use bevy::window::PrimaryWindow;

use crate::browser::Maps;
use crate::map::{Map, MapStore, Size, TerrainId, VOID};
use crate::render::{PixelZoom, PIXEL_SCALE};
use crate::state::AppState;
use crate::editor::{CurrentMap, Cursor as EditorCursor, Tool};
use crate::game::actors::{Actor, Sim, SimInput};
use crate::game::logview::LogView;
use crate::sim::{process_pass, spawn_pass, EntityType};
use crate::ui::keyboard::TextEntry;
use crate::ui::nav::{Activated, Focus, Focusable, Scope};
use script::{gamepad_button, key_char, key_code, GivenMap, Script, Side, Step};

const ENV_SCRIPT: &str = "CROWD2X_QA";
/// Where screenshots go. Overridden per test by `tools/qa.py`.
const ENV_SHOTS: &str = "CROWD2X_QA_SHOTS";
const SHOTS_DIR: &str = "qa-screenshots";

/// Size of the maps a `given` fixture creates.
const FIXTURE_SIZE: (i32, i32) = (8, 6);

/// How many times a blank frame is retaken before the run gives up on
/// screenshots. Bounded twice over: a machine with no display would otherwise
/// spend the whole test budget photographing nothing, so the first shot to
/// exhaust these turns the rest of the run's shots into single attempts.
const SHOT_RETRIES: u32 = 24;

/// Load the script named by `CROWD2X_QA`, if there is one.
///
/// Parsing happens before the app is built so a broken test file is a clear
/// message rather than a window that opens and does nothing.
pub fn script_from_env() -> Option<Script> {
    let path = std::env::var(ENV_SCRIPT).ok()?;
    match std::fs::read_to_string(&path) {
        Ok(json) => match Script::parse(&json) {
            Ok(mut script) => {
                script.source = path;
                Some(script)
            }
            Err(error) => {
                eprintln!("qa: {path}: {error}");
                std::process::exit(2);
            }
        },
        Err(error) => {
            eprintln!("qa: cannot read {path}: {error}");
            std::process::exit(2);
        }
    }
}

/// Holds the parsed script until the app is built.
///
/// `Plugin::build` only gets `&self`, and a script is a one-shot thing, so the
/// mutex is doing nothing more than making that shape legal.
/// The screen a script asks to start on.
///
/// A script naming a screen this build does not have stops the run here rather
/// than starting somewhere else and failing later for the wrong reason — which
/// is what a test written against a game mode that does not exist yet should
/// do.
pub fn initial_state(script: Option<&Script>) -> Option<AppState> {
    match script?.initial_state() {
        Ok(state) => state,
        Err(error) => fatal(&error),
    }
}

pub struct QaPlugin(std::sync::Mutex<Option<Script>>);

impl QaPlugin {
    pub fn new(script: Option<Script>) -> Self {
        Self(std::sync::Mutex::new(script))
    }
}

impl Plugin for QaPlugin {
    fn build(&self, app: &mut App) {
        let Some(script) = self.0.lock().expect("qa script lock").take() else {
            return;
        };
        // Before anything runs, and deliberately not in `Startup`: the
        // browser lists the maps directory in `OnEnter`, and for the screen
        // the app *starts* on that happens before every `Startup` system. A
        // fixture created there would not be in the list the test then reads,
        // and a map to open would not be open yet when the editor draws it.
        if let Some(open) = seed_fixture_maps(&script) {
            app.insert_resource(open);
        }

        app.insert_resource(Run::new(script))
            .add_systems(Startup, connect_virtual_gamepad)
            // After the input plugins have cleared last frame's state, so a
            // press made here is still `just_pressed` when the game reads it
            // in `Update`.
            .add_systems(
                PreUpdate,
                drive.after(bevy::input::InputSystems).run_if(unfinished),
            );
    }
}

fn unfinished(run: Res<Run>) -> bool {
    !run.finished
}

#[derive(Resource)]
struct Run {
    script: Script,
    step: usize,
    /// When the next step may start.
    ready_at: f32,
    /// Something pressed that has to be let go of, and when.
    holding: Option<(Held, f32)>,
    failures: Vec<String>,
    finished: bool,
    /// The virtual gamepad every script gets, connected at startup.
    pad: Entity,
    /// This test's screenshot directory, and how many it has taken.
    shot_dir: std::path::PathBuf,
    shots: u32,
    /// A shot that came back blank, and how many times it has been retaken.
    retake: Option<(std::path::PathBuf, u32)>,
    /// Set once a shot has run out of retries: this window is not being
    /// presented, and every later shot in the run would fail the same way.
    shots_are_hopeless: bool,
}

#[derive(Clone, Copy)]
enum Held {
    Key(KeyCode),
    Pad(GamepadButton),
    Mouse(MouseButton),
    Stick(Side),
}

impl Run {
    fn new(script: Script) -> Self {
        info!("qa: running {:?}", script.name);
        let shot_dir = std::env::var(ENV_SHOTS)
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::Path::new(SHOTS_DIR).join(script.stem()));
        Self {
            ready_at: script.settle,
            script,
            step: 0,
            holding: None,
            failures: Vec::new(),
            finished: false,
            pad: Entity::PLACEHOLDER,
            shot_dir,
            shots: 0,
            retake: None,
            shots_are_hopeless: false,
        }
    }

    /// The file the next screenshot goes in.
    ///
    /// Numbered, because the order shots were taken in is most of what makes a
    /// directory of them readable afterwards, and because two moments in one
    /// test can reasonably be given the same name.
    fn next_shot_path(&mut self, label: &str) -> std::path::PathBuf {
        self.shots += 1;
        self.shot_dir
            .join(format!("{:02}-{}.png", self.shots, slug(label)))
    }

    fn fail(&mut self, message: String) {
        error!("qa: FAIL at step {}: {message}", self.step);
        self.failures.push(message);
    }
}

/// Create the maps a script's `given` block says already exist, and hand back
/// the one it wants open.
///
/// A failure here is fatal rather than a test failure: every later step was
/// written on the assumption that these exist, so running them would report
/// something other than what actually went wrong.
fn seed_fixture_maps(script: &Script) -> Option<CurrentMap> {
    let store = MapStore::new(crate::browser::maps_dir());

    for fixture in &script.given.maps {
        let map = match build_fixture(fixture) {
            Ok(map) => map,
            Err(error) => fatal(&format!(
                "the fixture map {:?} could not be built: {error}",
                fixture.name()
            )),
        };
        if let Err(error) = store.save(fixture.name(), &map) {
            fatal(&format!(
                "could not write the fixture map {:?}: {error}",
                fixture.name()
            ));
        }
    }

    let name = script.open.as_ref()?;
    match store.load(name) {
        Ok(map) => {
            info!("qa: opening {name:?}");
            Some(CurrentMap::new(name.clone(), map))
        }
        Err(error) => fatal(&format!("cannot open {name:?}: {error}")),
    }
}

/// An empty map, a map from a file, or a map written into the test itself.
///
/// A file or an inline document goes through the real format, so a fixture
/// that has drifted fails here, named, rather than halfway through a test as a
/// wrong tile.
fn build_fixture(fixture: &GivenMap) -> Result<Map, String> {
    let (file, inline) = match fixture {
        GivenMap::Empty(_) => (None, None),
        GivenMap::Built { file, map, .. } => (file.as_ref(), map.as_ref()),
    };

    match (file, inline) {
        (None, None) => Ok(Map::new(Size::new(FIXTURE_SIZE.0, FIXTURE_SIZE.1), VOID)),
        (Some(path), None) => {
            let json = std::fs::read_to_string(path)
                .map_err(|error| format!("cannot read {path}: {error}"))?;
            Map::from_json(&json).map_err(|error| format!("{path}: {error}"))
        }
        (None, Some(document)) => {
            Map::from_json(&document.to_string()).map_err(|error| error.to_string())
        }
        (Some(_), Some(_)) => {
            Err("has both a `file` and a `map`; it can only come from one of them".to_string())
        }
    }
}

/// Stop the run outright, with a message that is about the test rather than
/// about Bevy.
fn fatal(message: &str) -> ! {
    eprintln!("qa: {message}");
    std::process::exit(2);
}

/// Plug in a gamepad for the whole run.
///
/// Connected once rather than on first use: the game only ever looks at a
/// gamepad when input arrives from it, and a controller that is already
/// plugged in is also the more realistic setup.
fn connect_virtual_gamepad(
    mut commands: Commands,
    mut run: ResMut<Run>,
    mut connections: MessageWriter<GamepadConnectionEvent>,
) {
    let pad = commands.spawn(Name::new("qa gamepad")).id();
    connections.write(GamepadConnectionEvent::new(
        pad,
        GamepadConnection::Connected {
            name: "crowd2x qa".into(),
            vendor_id: None,
            product_id: None,
        },
    ));
    run.pad = pad;
}

/// Everything a step might press.
#[derive(SystemParam)]
struct Devices<'w> {
    keys: ResMut<'w, ButtonInput<KeyCode>>,
    key_messages: MessageWriter<'w, KeyboardInput>,
    buttons: ResMut<'w, ButtonInput<MouseButton>>,
    click_messages: MessageWriter<'w, MouseButtonInput>,
    wheel: MessageWriter<'w, MouseWheel>,
    pad_input: MessageWriter<'w, RawGamepadEvent>,
}

/// Everything a check might look at, and everything an intent step reaches
/// for: the labels a widget is addressed by, and the two resources the editor
/// is aimed with.
#[derive(SystemParam)]
struct Checks<'w, 's> {
    state: Res<'w, State<AppState>>,
    maps: Res<'w, Maps>,
    labels: Query<'w, 's, &'static Text>,
    children: Query<'w, 's, &'static Children>,
    focusables: Query<'w, 's, (Entity, &'static Focusable)>,
    scope: Res<'w, Scope>,
    zoom: Res<'w, PixelZoom>,
    /// The sprites themselves, not the map that tracks them: a count taken
    /// from the bookkeeping would pass while nothing had actually reached the
    /// world, which is the failure this assertion exists to catch.
    sprites: Query<'w, 's, &'static Actor>,
    log: Res<'w, LogView>,
    /// So a scripted tick is the same size as one the game takes, read rather
    /// than written down twice.
    fixed: Res<'w, Time<Fixed>>,
}

/// What an intent step drives, as opposed to what it inspects.
#[derive(SystemParam)]
struct Intents<'w> {
    activated: MessageWriter<'w, Activated>,
    focus: ResMut<'w, Focus>,
    entry: ResMut<'w, TextEntry>,
    tool: ResMut<'w, Tool>,
    cursor: ResMut<'w, EditorCursor>,
    next_state: ResMut<'w, NextState<AppState>>,
    /// Where a `spawn` step puts its request, so it travels the same route a
    /// spawn from the game would.
    sim_input: ResMut<'w, SimInput>,
    /// The world, for the steps that step it and the ones that ask about it.
    ///
    /// Mutable, and here rather than in [`Checks`], because two parameters of
    /// one system claiming the same resource is a hard conflict — and `tick`
    /// needs to write. A second writer of `Sim` is something the game itself
    /// deliberately does not have; a test harness stepping the world on
    /// purpose is the exception, and it lives in a different schedule from
    /// `tick_sim` so the two never contend.
    ///
    /// `None` anywhere but the game screen.
    sim: Option<ResMut<'w, Sim>>,
}

/// What the driver should do once a step has been performed.
enum Next {
    /// Carry on after the usual gap.
    Now,
    /// Wait this long first.
    After(f32),
    /// Let go of this after that long, then carry on.
    Holding(Held, f32),
}

// A system takes its dependencies as parameters; the lint counts a Bevy
// signature as if it were a call site.
#[allow(clippy::too_many_arguments)]
fn drive(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<Run>,
    mut devices: Devices,
    mut intents: Intents,
    checks: Checks,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut exit: MessageWriter<AppExit>,
) {
    let now = time.elapsed_secs();

    if now > run.script.timeout {
        let timeout = run.script.timeout;
        run.fail(format!("timed out after {timeout}s"));
        finish(&mut run, &mut exit);
        return;
    }

    // A blank capture is retaken before anything else happens, so the frame
    // being photographed is still the one the test asked for.
    if let Some((path, attempts)) = run.retake.take() {
        if now < run.ready_at {
            run.retake = Some((path, attempts));
            return;
        }
        request_screenshot(&mut commands, path, attempts);
        run.ready_at = now + run.script.shot_delay;
        return;
    }

    // Let go of anything whose time is up before the next step starts, so a
    // held key cannot leak into the step after it.
    if let Some((held, until)) = run.holding {
        if now < until {
            return;
        }
        run.holding = None;
        release(held, &mut devices, run.pad);
        run.ready_at = now + run.script.gap;
        return;
    }

    if now < run.ready_at {
        return;
    }

    // Cloned out so the step can report a failure through `run`.
    let Some(step) = run.script.steps.get(run.step).cloned() else {
        finish(&mut run, &mut exit);
        return;
    };
    run.step += 1;

    // Screenshots are the one step that needs the run itself: the file is
    // named after the test and numbered in the order the shots were taken.
    if let Step::Shot(label) = &step {
        match take_screenshot(&mut run, label, &mut commands) {
            Ok(()) => run.ready_at = now + run.script.shot_delay,
            Err(message) => {
                run.fail(message);
                finish(&mut run, &mut exit);
            }
        }
        return;
    }

    match perform(
        &step,
        run.pad,
        &mut devices,
        &mut intents,
        &checks,
        &mut windows,
    ) {
        Ok(Next::Now) => run.ready_at = now + run.script.gap,
        Ok(Next::After(seconds)) => run.ready_at = now + seconds + run.script.gap,
        Ok(Next::Holding(held, seconds)) => run.holding = Some((held, now + seconds)),
        Err(message) => {
            run.fail(message);
            finish(&mut run, &mut exit);
        }
    }
}

fn perform(
    step: &Step,
    pad: Entity,
    devices: &mut Devices,
    intents: &mut Intents,
    checks: &Checks,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
) -> Result<Next, String> {
    match step {
        Step::Wait(seconds) => Ok(Next::After(*seconds)),

        Step::Note(text) => {
            info!("qa: {text}");
            Ok(Next::Now)
        }

        // --- what the session is doing, addressed by name -------------------
        Step::Goto(name) => {
            let state = AppState::from_name(name).ok_or(format!("no screen called {name:?}"))?;
            intents.next_state.set(state);
            Ok(Next::Now)
        }

        Step::Focus(label) => {
            let entity = widget(label, checks)?;
            intents.focus.0 = Some(entity);
            Ok(Next::Now)
        }

        Step::Press(label) => {
            let entity = widget(label, checks)?;
            // Focused as well as activated, because that is what a click does
            // and what the screen behind it will assume happened.
            intents.focus.0 = Some(entity);
            intents.activated.write(Activated(entity));
            Ok(Next::Now)
        }

        Step::Name(text) => {
            intents.entry.set(text.clone());
            Ok(Next::Now)
        }

        Step::Tool(name) => intents
            .tool
            .select(name)
            .then_some(Next::Now)
            .ok_or(format!("nothing in either palette is called {name:?}")),

        Step::CursorCell { x, y } => {
            intents
                .cursor
                .place(crate::editor::background::cell_centre(IVec2::new(*x, *y)));
            Ok(Next::Now)
        }

        Step::Cursor { x, y } => {
            intents.cursor.place(Vec2::new(*x, *y));
            Ok(Next::Now)
        }

        // --- the devices themselves -----------------------------------------

        Step::Key(name) => {
            let key = named_key(name)?;
            press_key(key, devices);
            Ok(Next::Holding(Held::Key(key), script::DEFAULT_TAP))
        }
        Step::HoldKey { key, seconds } => {
            let key = named_key(key)?;
            press_key(key, devices);
            Ok(Next::Holding(Held::Key(key), *seconds))
        }

        Step::Type(text) => {
            for c in text.chars() {
                type_char(c, devices);
            }
            Ok(Next::Now)
        }

        Step::Pad(name) => {
            let button = named_pad(name)?;
            press_pad(button, pad, devices, 1.0);
            Ok(Next::Holding(Held::Pad(button), script::DEFAULT_TAP))
        }
        Step::HoldPad { button, seconds } => {
            let button = named_pad(button)?;
            press_pad(button, pad, devices, 1.0);
            Ok(Next::Holding(Held::Pad(button), *seconds))
        }

        Step::Stick {
            side,
            x,
            y,
            seconds,
        } => {
            push_stick(*side, Vec2::new(*x, *y), pad, devices);
            Ok(Next::Holding(Held::Stick(*side), *seconds))
        }

        Step::Mouse { x, y } => {
            let mut window = windows
                .single_mut()
                .map_err(|_| "no window to move the pointer in".to_string())?;
            // Canvas pixels to window pixels: the window's scale factor is
            // overridden to 1, so this is the same factor the UI is scaled by
            // and a test can use the coordinates it sees on the canvas.
            window.set_cursor_position(Some(Vec2::new(*x, *y) * PIXEL_SCALE as f32));
            Ok(Next::Now)
        }

        Step::Click { button, seconds } => {
            let button = button.code();
            devices.buttons.press(button);
            devices.click_messages.write(MouseButtonInput {
                button,
                state: ButtonState::Pressed,
                window: Entity::PLACEHOLDER,
            });
            Ok(Next::Holding(Held::Mouse(button), *seconds))
        }

        Step::Wheel(y) => {
            devices.wheel.write(MouseWheel {
                unit: MouseScrollUnit::Line,
                x: 0.0,
                y: *y,
                window: Entity::PLACEHOLDER,
                // What a mouse always reports; only a touchpad has phases.
                phase: bevy::input::touch::TouchPhase::Moved,
            });
            Ok(Next::Now)
        }


        // Handled by the driver, which is where the shot counter lives.
        Step::Shot(_) => Ok(Next::Now),

        Step::ExpectState(name) => {
            let wanted = AppState::from_name(name).ok_or(format!("no screen called {name:?}"))?;
            let actual = *checks.state.get();
            (actual == wanted)
                .then_some(Next::Now)
                .ok_or(format!("expected to be on {wanted:?}, was on {actual:?}"))
        }

        Step::ExpectFocus(label) => {
            let focused = intents
                .focus
                .0
                .and_then(|entity| label_of(entity, checks))
                .ok_or("nothing is focused".to_string())?;
            (focused == *label)
                .then_some(Next::Now)
                .ok_or(format!("expected {label:?} focused, found {focused:?}"))
        }

        Step::Spawn { kind, x, y } => {
            let kind = EntityType::from_name(kind).ok_or_else(|| {
                format!(
                    "no entity kind called {kind:?}; this build has: {}",
                    EntityType::ALL
                        .map(|kind| kind.name())
                        .join(", ")
                )
            })?;
            intents
                .sim_input
                .0
                .spawn(kind, crate::map::Point::new(*x, *y));
            Ok(Next::Now)
        }

        Step::Tick(count) => {
            // Reported wherever it is asked for, count or no count: a script
            // ticking off the game screen has misunderstood something, and
            // saying so is the point of the step existing.
            if intents.sim.is_none() {
                return Err(
                    "there is no simulation to tick — `tick` only means anything on the game screen"
                        .to_string(),
                );
            }
            // `{"tick": 0}` does nothing at all, spawn pass included: the
            // pending commands stay queued for the game's own `tick_sim`
            // rather than being taken and dropped on the floor here.
            if *count == 0 {
                return Ok(Next::Now);
            }
            let commands = std::mem::take(&mut intents.sim_input.0);
            let sim = intents.sim.as_deref_mut().expect("checked just above");
            // The step the game itself takes, so a tick in a script and a tick
            // in play are the same amount of world.
            let dt = checks.fixed.timestep().as_secs_f32();

            // One spawn pass, then `count` processing passes. Pending spawns
            // are applied exactly once however many ticks were asked for,
            // which is what the two passes being separable is *for* — through
            // `process_game_state` this needed the commands handed to the
            // first tick and an empty `Input` to every one after it.
            spawn_pass(&mut sim.0, &commands);
            for _ in 0..*count {
                process_pass(&mut sim.0, dt);
            }
            Ok(Next::Now)
        }

        Step::ExpectZoom(wanted) => {
            let actual = checks.zoom.get();
            (actual == *wanted)
                .then_some(Next::Now)
                .ok_or(format!("expected zoom x{wanted}, found x{actual}"))
        }

        Step::ExpectMap(name) => checks
            .maps
            .0
            .exists(name)
            .then_some(Next::Now)
            .ok_or(format!("expected a saved map called {name:?}")),

        Step::ExpectNoMap(name) => (!checks.maps.0.exists(name))
            .then_some(Next::Now)
            .ok_or(format!("expected no saved map called {name:?}")),

        Step::ExpectTile {
            map,
            x,
            y,
            terrain,
        } => {
            let wanted =
                TerrainId::from_name(terrain).ok_or(format!("no terrain called {terrain:?}"))?;
            // The saved file, not the running scene: painting is only real
            // once it has been written out.
            let saved = checks
                .maps
                .0
                .load(map)
                .map_err(|error| format!("{map}: {error}"))?;
            let at = crate::map::Point::new(*x, *y);
            match saved.terrain(at) {
                Some(found) if found == wanted => Ok(Next::Now),
                Some(found) => Err(format!(
                    "{map} ({x}, {y}) is {:?}, expected {terrain:?}",
                    found.name()
                )),
                None => Err(format!("({x}, {y}) is outside {map}")),
            }
        }

        Step::ExpectEntities(wanted) => {
            let actual = intents
                .sim
                .as_deref()
                .ok_or("there is no simulation — expected the game screen".to_string())?
                .0
                .len();
            (actual == *wanted)
                .then_some(Next::Now)
                .ok_or(format!("expected {wanted} entities, found {actual}"))
        }

        Step::ExpectSprites(wanted) => {
            let actual = checks.sprites.iter().count();
            (actual == *wanted)
                .then_some(Next::Now)
                .ok_or(format!("expected {wanted} actor sprites, found {actual}"))
        }

        Step::ExpectLog(needle) => checks
            .log
            .contains(needle)
            .then_some(Next::Now)
            .ok_or(format!(
                "nothing in the log contains {needle:?}; it holds: {:?}",
                checks.log.lines()
            )),
    }
}

/// Ask for a screenshot of this frame.
///
/// The wait around it is the script's `shot_delay`: a capture taken before the
/// renderer has presented the frame being asked for is a pure black PNG rather
/// than an error, which is a slow and confusing thing to debug from.
fn take_screenshot(run: &mut Run, label: &str, commands: &mut Commands) -> Result<(), String> {
    std::fs::create_dir_all(&run.shot_dir)
        .map_err(|error| format!("cannot create {}: {error}", run.shot_dir.display()))?;

    let path = run.next_shot_path(label);
    info!("qa: shot {}", path.display());
    request_screenshot(commands, path, 0);
    Ok(())
}

/// Ask for one capture, and judge it when it arrives.
///
/// Two observers on the same capture: one writes the file, one decides whether
/// what was written is a picture of anything. A retake overwrites the same
/// path, so the numbering stays in step with the script.
fn request_screenshot(commands: &mut Commands, path: std::path::PathBuf, attempts: u32) {
    let judged = path.clone();
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path))
        .observe(
            move |captured: On<ScreenshotCaptured>, mut run: ResMut<Run>| {
                if !is_flat(&captured.image) {
                    return;
                }
                if attempts >= SHOT_RETRIES || run.shots_are_hopeless {
                    run.shots_are_hopeless = true;
                    warn!(
                        "qa: {} is blank after {} attempts - the window is not being presented",
                        judged.display(),
                        attempts + 1
                    );
                    return;
                }
                run.retake = Some((judged.clone(), attempts + 1));
            },
        );
}

/// Is every pixel the same? That is what a frame captured before the renderer
/// has presented anything looks like — a photograph of nothing, and never a
/// real frame of this game, which always has a canvas or a panel in it.
///
/// Every pixel, not a sample of them: the interesting case is a frame that is
/// *nearly* flat — a HUD over an unpainted map — and a sparse scan steps
/// straight over a line of 6px text. It stops at the first difference, so the
/// full scan only happens for a frame that really is blank, and only while a
/// screenshot is pending.
fn is_flat(image: &Image) -> bool {
    let Some(data) = &image.data else {
        return true;
    };
    let (pixels, _) = data.as_chunks::<4>();
    let Some(first) = pixels.first() else {
        return true;
    };
    !pixels.iter().any(|pixel| pixel != first)
}

/// A label as a file name: lower case, one dash per run of anything else, and
/// never empty — a shot called `"..."` still has to land somewhere.
fn slug(label: &str) -> String {
    let mut slug = String::new();
    for c in label.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "shot".to_string()
    } else {
        slug.to_string()
    }
}

/// End the run, and say so in the exit code: a QA script is meant to be run by
/// something that only reads that.
fn finish(run: &mut Run, exit: &mut MessageWriter<AppExit>) {
    run.finished = true;
    if run.failures.is_empty() {
        info!("qa: PASS {:?} ({} steps)", run.script.name, run.step);
        exit.write(AppExit::Success);
    } else {
        error!(
            "qa: FAIL {:?} - {} of {} steps failed",
            run.script.name,
            run.failures.len(),
            run.step
        );
        exit.write(AppExit::Error(NonZero::new(1).expect("1 is not zero")));
    }
}

fn named_key(name: &str) -> Result<KeyCode, String> {
    key_code(name).ok_or(format!("no key called {name:?}"))
}

fn named_pad(name: &str) -> Result<GamepadButton, String> {
    gamepad_button(name).ok_or(format!("no gamepad button called {name:?}"))
}

/// Press a key on both paths the game reads: the button state for held keys,
/// and a message for anything listening for typing.
fn press_key(key: KeyCode, devices: &mut Devices) {
    devices.keys.press(key);
    devices.key_messages.write(key_message(key, ButtonState::Pressed));
}

fn key_message(key: KeyCode, state: ButtonState) -> KeyboardInput {
    let logical = match key {
        KeyCode::Enter | KeyCode::NumpadEnter => Key::Enter,
        KeyCode::Escape => Key::Escape,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Tab => Key::Tab,
        _ => match key_char(key) {
            Some(c) => Key::Character(c.to_string().into()),
            None => Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
        },
    };
    KeyboardInput {
        key_code: key,
        text: key_char(key).map(|c| c.to_string().into()),
        logical_key: logical,
        state,
        repeat: false,
        window: Entity::PLACEHOLDER,
    }
}

/// Type one character as a keypress that produced text.
///
/// Text goes only as a message, never through `ButtonInput`: a name containing
/// an `e` would otherwise also cycle the editor's palette, which is exactly the
/// kind of thing a real keyboard does not do to a focused text field.
fn type_char(c: char, devices: &mut Devices) {
    let text = c.to_string();
    for state in [ButtonState::Pressed, ButtonState::Released] {
        devices.key_messages.write(KeyboardInput {
            key_code: KeyCode::Unidentified(bevy::input::keyboard::NativeKeyCode::Unidentified),
            logical_key: Key::Character(text.clone().into()),
            text: Some(text.clone().into()),
            state,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
    }
}

fn press_pad(button: GamepadButton, pad: Entity, devices: &mut Devices, value: f32) {
    devices
        .pad_input
        .write(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
            pad, button, value,
        )));
}

fn push_stick(side: Side, value: Vec2, pad: Entity, devices: &mut Devices) {
    let (x_axis, y_axis) = match side {
        Side::Left => (GamepadAxis::LeftStickX, GamepadAxis::LeftStickY),
        Side::Right => (GamepadAxis::RightStickX, GamepadAxis::RightStickY),
    };
    for (axis, value) in [(x_axis, value.x), (y_axis, value.y)] {
        devices
            .pad_input
            .write(RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(
                pad, axis, value,
            )));
    }
}

fn release(held: Held, devices: &mut Devices, pad: Entity) {
    match held {
        Held::Key(key) => {
            devices.keys.release(key);
            devices
                .key_messages
                .write(key_message(key, ButtonState::Released));
        }
        Held::Pad(button) => press_pad(button, pad, devices, 0.0),
        Held::Mouse(button) => {
            devices.buttons.release(button);
            devices.click_messages.write(MouseButtonInput {
                button,
                state: ButtonState::Released,
                window: Entity::PLACEHOLDER,
            });
        }
        Held::Stick(side) => push_stick(side, Vec2::ZERO, pad, devices),
    }
}

/// The focusable widget carrying this label, in the layer that currently owns
/// input.
///
/// Scoped on purpose: while a dialog is open, "delete" means the dialog's
/// button and not the row behind it — exactly as it does for the player.
fn widget(label: &str, checks: &Checks) -> Result<Entity, String> {
    let mut found: Vec<Entity> = checks
        .focusables
        .iter()
        .filter(|(_, slot)| slot.scope == checks.scope.0)
        .filter(|(entity, _)| label_of(*entity, checks).as_deref() == Some(label))
        .map(|(entity, _)| entity)
        .collect();
    found.sort();

    match found.len() {
        // Says what *is* on screen, because the interesting failures here are
        // the ones where the screen is not the one the test thought it was on.
        0 => Err(format!(
            "no widget on this screen is labelled {label:?}; it has: {}",
            labels_in_scope(checks)
        )),
        // Ambiguity is the test's problem to fix, not something to guess at:
        // every row of the file list has an `edit` button.
        1 => Ok(found[0]),
        n => Err(format!(
            "{n} widgets are labelled {label:?}; use the arrow keys or a more specific label"
        )),
    }
}

/// Every label currently reachable, for an error message to name.
fn labels_in_scope(checks: &Checks) -> String {
    let mut labels: Vec<String> = checks
        .focusables
        .iter()
        .filter(|(_, slot)| slot.scope == checks.scope.0)
        .filter_map(|(entity, _)| label_of(entity, checks))
        .collect();
    labels.sort();
    labels.dedup();
    if labels.is_empty() {
        "nothing focusable at all".to_string()
    } else {
        labels.join(", ")
    }
}

/// A widget's label: its own text, or the text of a child a couple of levels
/// down — far enough for a button whose label is a child node, not so far that
/// it picks up an unrelated panel's text.
fn label_of(entity: Entity, checks: &Checks) -> Option<String> {
    fn text_under(entity: Entity, checks: &Checks, depth: u8) -> Option<String> {
        if let Ok(text) = checks.labels.get(entity) {
            return Some(text.0.clone());
        }
        if depth == 0 {
            return None;
        }
        checks
            .children
            .get(entity)
            .ok()?
            .iter()
            .find_map(|child| text_under(child, checks, depth - 1))
    }

    text_under(entity, checks, 2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::RenderAssetUsages;
    use bevy::image::Image;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    fn image(fill: [u8; 4], stripe: Option<[u8; 4]>) -> Image {
        let mut image = Image::new_fill(
            Extent3d {
                width: 64,
                height: 64,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &fill,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::all(),
        );
        if let Some(stripe) = stripe {
            let data = image.data.as_mut().expect("filled image has data");
            // One pixel deep into the image, where a sparse check has to find
            // it: a screenshot of a menu is mostly flat backdrop.
            let at = (40 * 64 + 8) * 4;
            data[at..at + 4].copy_from_slice(&stripe);
        }
        image
    }

    #[test]
    fn a_frame_of_one_colour_is_not_a_photograph_of_anything() {
        assert!(is_flat(&image([0, 0, 0, 255], None)));
        // ...and neither is a flat panel colour, which is what a capture taken
        // before the world is drawn can also look like.
        assert!(is_flat(&image([10, 10, 15, 255], None)));
    }

    #[test]
    fn a_frame_with_anything_drawn_in_it_is_kept() {
        assert!(!is_flat(&image([10, 10, 15, 255], Some([240, 242, 250, 255]))));
    }

    #[test]
    fn a_shot_label_becomes_a_file_name_that_is_still_readable() {
        assert_eq!(slug("the file list"), "the-file-list");
        assert_eq!(slug("keyboard: 'ab'"), "keyboard-ab");
        assert_eq!(slug("Editor / painted"), "editor-painted");
    }

    #[test]
    fn a_label_that_is_all_punctuation_still_produces_a_name() {
        // Otherwise the file would be called `01-.png`.
        assert_eq!(slug("..."), "shot");
        assert_eq!(slug(""), "shot");
    }
}
