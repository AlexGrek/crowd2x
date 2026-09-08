//! How fast the world runs, and whether it runs at all.
//!
//! Speed is **how many steps happen**, never how big one is. A tick is always
//! the same slice of world — the fixed timestep — so a run at 4x is the same
//! run as one at 1x, watched four times as fast, and a pause is running none
//! of them. Scaling `dt` instead would change what the simulation *does*
//! rather than only when it does it: a wanderer would cross a whole cell in
//! one step, and two runs of one seed at two speeds would end in two different
//! worlds.
//!
//! [`GameSpeed::steps`] is where that lands. It carries the fraction between
//! fixed steps, so 0.5x is every other step rather than half a step, and the
//! ladder is powers of two so that arithmetic is exact.
//!
//! The keys live here, beside the thing they change; the buttons that do the
//! same are [`super::hud`]'s, and both go through [`apply`] so the log line
//! and the unpause rule cannot differ depending on which was used.

use bevy::prelude::*;

use crate::state::AppState;
use crate::ui::nav::NavSystems;

use super::actors::Sim;

/// The speeds that can be asked for, slowest first.
///
/// Powers of two either side of 1, so [`GameSpeed::steps`]'s accumulator is
/// exact in binary and 0.25x is genuinely every fourth step rather than nearly
/// every fourth. Below 0.25x a wanderer stops reading as slow and starts
/// reading as stuck; above 8x a fixed step already asks more of a frame than
/// it has, and asking for more only drops frames.
pub const SPEEDS: [f32; 6] = [0.25, 0.5, 1.0, 2.0, 4.0, 8.0];

/// Where a fresh game starts: 1x.
const NORMAL: usize = 2;

/// How fast the simulation is being asked to run.
///
/// Pause is a flag beside the speed rather than a rung at the bottom of the
/// ladder, so letting go of a pause puts the world back at the speed it was
/// being watched at instead of at 1x.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct GameSpeed {
    index: usize,
    paused: bool,
    /// Steps earned but not yet taken, always below one.
    pending: f32,
}

impl Default for GameSpeed {
    fn default() -> Self {
        Self {
            index: NORMAL,
            paused: false,
            pending: 0.0,
        }
    }
}

impl GameSpeed {
    /// Steps of world per fixed step, ignoring the pause.
    pub fn multiplier(self) -> f32 {
        SPEEDS[self.index]
    }

    /// What the readout says, and what a QA test asserts on: `x2`, or
    /// `paused`. One string rather than a number and a flag, because what a
    /// test is asking about is what the player can see.
    pub fn label(self) -> String {
        if self.paused {
            "paused".to_string()
        } else {
            format!("x{}", self.multiplier())
        }
    }

    /// What the pause button says, which is what pressing it will do.
    pub fn toggle_label(self) -> &'static str {
        if self.paused {
            "resume"
        } else {
            "pause"
        }
    }

    /// Move along the ladder, clamped at both ends. Answers whether anything
    /// moved, so the far end of the ladder does not announce a change that did
    /// not happen.
    ///
    /// Asking for a speed lets go of the pause: `+` on a paused game means
    /// "run, and faster", and a button that visibly does nothing reads as a
    /// broken button.
    pub fn shift(&mut self, steps: i32) -> bool {
        let wanted = (self.index as i32 + steps).clamp(0, SPEEDS.len() as i32 - 1) as usize;
        let moved = wanted != self.index || self.paused;
        self.index = wanted;
        self.paused = false;
        moved
    }

    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }

    /// How many processing passes this fixed step is worth.
    ///
    /// The fraction is carried rather than rounded, so 0.5x is exactly one
    /// step every other fixed step and not one small step every fixed step. A
    /// pause keeps whatever had been carried: pausing part-way through a
    /// fraction and letting go again must not cost the world a step it had
    /// already earned.
    pub fn steps(&mut self) -> u32 {
        if self.paused {
            return 0;
        }
        self.pending += self.multiplier();
        let whole = self.pending.floor();
        self.pending -= whole;
        whole as u32
    }

    /// Back to 1x, running.
    ///
    /// Done on the way *out* of the game for the same reason the zoom is: a
    /// world left paused would be re-entered frozen, which reads as a game
    /// that has hung rather than one that is waiting.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Everything that can be asked of the speed, however it was asked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Change {
    Faster,
    Slower,
    TogglePause,
}

pub struct SpeedPlugin;

impl Plugin for SpeedPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GameSpeed>()
            .add_systems(OnExit(AppState::Game), leave)
            .add_systems(
                Update,
                read_keys.after(NavSystems).run_if(in_state(AppState::Game)),
            );
    }
}

fn leave(mut speed: ResMut<GameSpeed>) {
    speed.reset();
}

/// `+`, `-` and `p`, and the two bumpers and `Y` beside them.
///
/// The zoom keeps `q` / `e` and `A` / `B`. `+` and `-` used to be aliases for
/// zooming and are the speed's now: the zoom already had two bindings on each
/// device, and a speed control is what a keyboard was missing.
fn read_keys(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut speed: ResMut<GameSpeed>,
    sim: Option<Res<Sim>>,
) {
    let pads = || gamepads.iter();
    if keys.any_just_pressed([KeyCode::Equal, KeyCode::NumpadAdd])
        || pads().any(|pad| pad.just_pressed(GamepadButton::RightTrigger))
    {
        apply(Change::Faster, &mut speed, sim.as_deref());
    }
    if keys.any_just_pressed([KeyCode::Minus, KeyCode::NumpadSubtract])
        || pads().any(|pad| pad.just_pressed(GamepadButton::LeftTrigger))
    {
        apply(Change::Slower, &mut speed, sim.as_deref());
    }
    if keys.just_pressed(KeyCode::KeyP) || pads().any(|pad| pad.just_pressed(GamepadButton::North)) {
        apply(Change::TogglePause, &mut speed, sim.as_deref());
    }
}

/// Carry out a change, and say so.
///
/// The line goes into the simulation's own log, which is the panel under the
/// controls: a speed that changed is exactly what you want to find in there
/// when the crowd has suddenly stopped. [`crate::sim::Log::push`] takes
/// `&self`, so this needs no second writer of [`Sim`].
pub fn apply(change: Change, speed: &mut GameSpeed, sim: Option<&Sim>) {
    let changed = match change {
        Change::Faster => speed.shift(1),
        Change::Slower => speed.shift(-1),
        Change::TogglePause => {
            speed.toggle_pause();
            true
        }
    };
    if !changed {
        return;
    }
    if let Some(sim) = sim {
        sim.0.log.push(format!("speed {}", speed.label()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_game_runs_at_one_step_per_fixed_step() {
        let mut speed = GameSpeed::default();
        assert_eq!(speed.label(), "x1");
        for _ in 0..10 {
            assert_eq!(speed.steps(), 1);
        }
    }

    #[test]
    fn a_faster_game_takes_whole_extra_steps() {
        let mut speed = GameSpeed::default();
        speed.shift(2);
        assert_eq!(speed.label(), "x4");
        assert_eq!(speed.steps(), 4);
    }

    /// The point of carrying the fraction: half speed is every other step, and
    /// over any stretch it is exactly half as much world.
    #[test]
    fn a_slower_game_skips_steps_rather_than_shrinking_them() {
        let mut speed = GameSpeed::default();
        speed.shift(-1);
        assert_eq!(speed.label(), "x0.5");
        let taken: u32 = (0..8).map(|_| speed.steps()).sum();
        assert_eq!(taken, 4);

        let mut quarter = GameSpeed::default();
        quarter.shift(-2);
        assert_eq!(quarter.label(), "x0.25");
        assert_eq!(
            (0..8).map(|_| quarter.steps()).collect::<Vec<_>>(),
            [0, 0, 0, 1, 0, 0, 0, 1]
        );
    }

    #[test]
    fn the_ladder_stops_at_both_ends() {
        let mut speed = GameSpeed::default();
        assert!(speed.shift(-99));
        assert_eq!(speed.multiplier(), SPEEDS[0]);
        // Already at the bottom: nothing changed, and nothing should be
        // announced as if it had.
        assert!(!speed.shift(-1));

        assert!(speed.shift(99));
        assert_eq!(speed.multiplier(), SPEEDS[SPEEDS.len() - 1]);
        assert!(!speed.shift(1));
    }

    #[test]
    fn a_paused_game_takes_no_steps_at_all() {
        let mut speed = GameSpeed::default();
        speed.shift(3);
        speed.toggle_pause();
        assert_eq!(speed.label(), "paused");
        assert_eq!(speed.toggle_label(), "resume");
        for _ in 0..10 {
            assert_eq!(speed.steps(), 0);
        }
    }

    /// Letting go of a pause resumes the speed that was being watched, which
    /// is why a pause is a flag and not a rung on the ladder.
    #[test]
    fn letting_go_of_a_pause_goes_back_to_the_same_speed() {
        let mut speed = GameSpeed::default();
        speed.shift(1);
        speed.toggle_pause();
        speed.toggle_pause();
        assert_eq!(speed.label(), "x2");
        assert_eq!(speed.steps(), 2);
    }

    #[test]
    fn asking_for_a_speed_lets_go_of_the_pause() {
        let mut speed = GameSpeed::default();
        // Paused at the top of the ladder, so the index cannot move — and the
        // press still has to do something, or `+` on a paused game does
        // nothing at all.
        speed.shift(99);
        speed.toggle_pause();
        assert!(speed.shift(1));
        assert_eq!(speed.label(), "x8");
        assert_eq!(speed.steps(), 8);
    }

    #[test]
    fn a_pause_keeps_the_fraction_of_a_step_it_had_earned() {
        let mut speed = GameSpeed::default();
        speed.shift(-1);
        // Half a step earned, none taken.
        assert_eq!(speed.steps(), 0);
        speed.toggle_pause();
        assert_eq!(speed.steps(), 0);
        speed.toggle_pause();
        // The other half arrives, and the step it was owed is taken.
        assert_eq!(speed.steps(), 1);
    }

    #[test]
    fn leaving_puts_the_speed_back() {
        let mut speed = GameSpeed::default();
        speed.shift(-2);
        speed.toggle_pause();
        speed.reset();
        assert_eq!(speed, GameSpeed::default());
        assert_eq!(speed.label(), "x1");
    }
}
