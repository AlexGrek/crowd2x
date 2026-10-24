//! [`UseArcade`]: stand at an arcade cabinet and play it.
//!
//! [`UseComputer`](super::UseComputer)'s twin, and the same
//! [`Event::Entertained`] at the end of it — but **quick**: a go at a cabinet
//! is a third of a go at a computer ([`ARCADE_SECONDS`]), so fun comes back
//! three times as fast — and **only while the playzone is run**. The cabinet
//! has to be on (power, like any screen) *and* its playzone's computer has to
//! have an administrator at it ([`Playzones::runs`](crate::sim::Playzones::runs)),
//! checked every tick: an administrator who steps away, goes off shift, is let
//! go or dies switches every game in the playzone off, and a game cut short is
//! no fun at all.

use crate::map::Point;
use crate::sim::biology::Event;
use crate::sim::clock::{watched, MINUTE};
use crate::sim::feature::FeatureKind;

use super::super::action::{Action, ActionState};
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};

/// Ten minutes of world a game: a third of a go at a computer
/// ([`PLAY_SECONDS`](crate::sim::brain::goals::play::PLAY_SECONDS)) for the same
/// fun — which is why an arcade is the quick way to stop being bored.
pub const ARCADE_SECONDS: f32 = watched(10.0 * MINUTE);

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct UseArcade {
    /// The cabinet's cell. Played from **beside** it, like a computer: its
    /// own cell is furniture, so this is what the unit is turned towards,
    /// never where it walks to.
    pub cabinet: Point,
    /// How long a game takes.
    pub seconds: f32,
}

impl TaskExecutor for UseArcade {
    /// **Only from one step away, with the cabinet working, checked every
    /// tick**: no body to entertain, a unit pulled away, a box switched off
    /// or the administrator gone from the computer ends the game
    /// unfinished. The body hears [`Event::Entertained`] only once the game
    /// is over; a go had again while the last is in recent memory is less
    /// fun and less of a treat, and the log says so.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        if ctx.here().manhattan_distance(self.cabinet) > 1
            || ctx.biology.is_none()
            || !ctx.think.features.has(FeatureKind::Arcade, self.cabinet)
            || !ctx.think.playzones.runs(self.cabinet)
        {
            return TaskResult::Failed;
        }

        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.cabinet, self.seconds);
            return TaskResult::Executing;
        }

        match ctx.advance_action() {
            ActionState::Finished => {
                let recalled = ctx.biology.as_deref_mut().and_then(|b| b.handle(Event::Entertained));
                super::report_recalled(ctx, recalled);
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }

    fn describe(&self) -> String {
        format!("play the arcade cabinet at {}, {}", self.cabinet.x, self.cabinet.y)
    }
}

impl Task {
    /// Play the arcade cabinet in `cabinet` for `seconds`, from a cell beside
    /// it.
    pub const fn use_arcade(cabinet: Point, seconds: f32) -> Task {
        Task::UseArcade(UseArcade { cabinet, seconds })
    }
}

/// Saved as `use arcade`: the cabinet's cell and the seconds.
impl TimedTask for UseArcade {
    const NAME: &'static str = "use arcade";

    fn new(cell: Point, seconds: f32) -> UseArcade {
        UseArcade { cabinet: cell, seconds }
    }

    fn cell(&self) -> Point {
        self.cabinet
    }

    fn seconds(&self) -> f32 {
        self.seconds
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::task::Task;
    use super::super::rig::Rig;
    use super::*;
    use crate::map::{Map, Size, FLOOR};

    const CABINET: Point = Point::new(6, 6);
    const COMPUTER: Point = Point::new(2, 2);

    /// A unit in `cell` of a room with a working arcade cabinet at
    /// [`CABINET`] and a playzone computer at [`COMPUTER`] — run, unless a
    /// test says otherwise.
    fn rig_at(cell: Point, fun: f32, run: bool) -> Rig {
        let mut map = Map::new(Size::new(9, 9), FLOOR);
        crate::sim::testing::prop_at(&mut map, "arcade cabinet", CABINET);
        crate::sim::testing::prop_at(&mut map, "playzone computer", COMPUTER);
        let mut rig = Rig::new(map, cell);
        rig.biology.edit(|stats| stats.with_fun(fun));
        if run {
            assert!(rig.world.playzones.run(COMPUTER));
        }
        rig
    }

    #[test]
    fn a_game_is_a_third_of_a_go_at_a_computer() {
        use crate::sim::brain::goals::play::PLAY_SECONDS;
        assert!((ARCADE_SECONDS * 3.0 - PLAY_SECONDS).abs() < 1e-3, "{ARCADE_SECONDS} against {PLAY_SECONDS}");
    }

    #[test]
    fn a_game_is_worth_something_only_once_it_is_over_and_lights_the_cabinet() {
        let mut rig = rig_at(Point::new(5, 6), 20.0, true);
        let mut task = Task::use_arcade(CABINET, 0.5);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.action.interacting_with(), Some(CABINET), "its screen is lit while it is played");
        assert_eq!(rig.biology.stats().fun(), 20.0, "not yet");
        assert_eq!(rig.run(&mut task, 600), TaskResult::Success);
        assert!(rig.biology.stats().fun() > 20.0, "{}", rig.biology.stats().fun());
        assert_eq!(rig.walk.body().center_position(), Point::new(5, 6), "played from where it stood");
    }

    #[test]
    fn nobody_runs_the_playzone_so_the_cabinet_does_not_work() {
        let mut rig = rig_at(Point::new(5, 6), 20.0, false);
        let mut task = Task::use_arcade(CABINET, 0.5);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert!(rig.action.is_none());
        assert_eq!(rig.biology.stats().fun(), 20.0);
    }

    /// The administrator walks away mid-game: every cabinet goes dark, and
    /// half a game is no fun at all.
    #[test]
    fn the_administrator_leaving_mid_game_ends_it_and_is_no_fun_at_all() {
        let mut rig = rig_at(Point::new(5, 6), 20.0, true);
        let mut task = Task::use_arcade(CABINET, 10.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);

        rig.world.playzones.begin_tick();
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert_eq!(rig.biology.stats().fun(), 20.0);
    }

    #[test]
    fn losing_power_mid_game_ends_it() {
        let mut rig = rig_at(Point::new(5, 6), 20.0, true);
        let mut task = Task::use_arcade(CABINET, 10.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        let dead = crate::map::utilities::Supply::from_map(&rig.world.map);
        rig.world.features = crate::sim::feature::Features::from_map(&rig.world.map, &dead);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert_eq!(rig.biology.stats().fun(), 20.0);
    }

    #[test]
    fn playing_from_across_the_room_or_walking_away_fails_without_fun() {
        let mut rig = rig_at(Point::new(1, 6), 20.0, true);
        let mut task = Task::use_arcade(CABINET, 1.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert!(rig.action.is_none());

        let mut rig = rig_at(Point::new(5, 6), 20.0, true);
        let mut task = Task::use_arcade(CABINET, 10.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        rig.walk.body_mut().set_position((1.5, 1.5));
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert_eq!(rig.biology.stats().fun(), 20.0);
    }

    #[test]
    fn a_game_fills_a_bored_unit_with_fun_faster_than_a_go_at_a_computer_would() {
        use crate::sim::biology::fun::AMUSEMENT;
        let mut rig = rig_at(Point::new(5, 6), 10.0, true);
        let mut task = Task::use_arcade(CABINET, ARCADE_SECONDS);
        assert_eq!(rig.run(&mut task, 20_000), TaskResult::Success);
        assert!(rig.biology.stats().fun() >= 10.0 + AMUSEMENT - 0.5, "{}", rig.biology.stats().fun());
    }
}
