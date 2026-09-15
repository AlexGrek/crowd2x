//! Actions: what a unit is physically doing this tick.
//!
//! **Pathfinding and animation only — no decision at this level.** An action
//! is what a task is doing with the body right now: walking a route, or
//! playing out something that takes time. It knows how long it has been going
//! and whether it is over, and nothing about why it was started. Whether it
//! was allowed to start, and what finishing it *means* — food in hand, a need
//! met — belongs to the task that started it ([`super::tasks`]).

use crate::map::Point;
use crate::sim::entity::Think;
use crate::sim::walker::Walker;
use crate::sim::MoveOutcome;

/// What the body is doing, and how far into it.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Action {
    /// Nothing.
    #[default]
    None,
    /// Walking. The route lives in the [`Walker`]; this is the fact that
    /// walking is what is being done, and where it ends.
    Move { to: Point, elapsed: f32 },
    /// Using something in `cell` — getting food out of a fridge.
    Interact { cell: Point, seconds: f32, elapsed: f32 },
    /// Using up what is in hand — eating.
    Consume { seconds: f32, elapsed: f32 },
    Wait { seconds: f32, elapsed: f32 },
}

/// Where an action has got to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ActionState {
    Running,
    Finished,
    Failed,
}

impl Action {
    /// **The far route is asked for here, and nowhere else.** A walk to `cell`,
    /// or `None` when there is no way there — in which case the walker is left
    /// with no route.
    ///
    /// One call per walk started, which is what keeps the number of searches a
    /// function of how many decisions were made rather than of how many ticks
    /// went by.
    pub fn walk_to(walk: &mut Walker, think: &Think<'_>, cell: Point) -> Option<Action> {
        walk.route_to(think, cell).then_some(Action::Move {
            to: cell,
            elapsed: 0.0,
        })
    }

    pub const fn interact(cell: Point, seconds: f32) -> Action {
        Action::Interact {
            cell,
            seconds,
            elapsed: 0.0,
        }
    }

    pub const fn consume(seconds: f32) -> Action {
        Action::Consume {
            seconds,
            elapsed: 0.0,
        }
    }

    pub const fn wait(seconds: f32) -> Action {
        Action::Wait {
            seconds,
            elapsed: 0.0,
        }
    }

    /// The short name an animation would be picked by.
    pub const fn name(&self) -> &'static str {
        match self {
            Action::None => "none",
            Action::Move { .. } => "move",
            Action::Interact { .. } => "interact",
            Action::Consume { .. } => "consume",
            Action::Wait { .. } => "wait",
        }
    }

    /// Seconds since it started — what an animation would be driven from.
    pub const fn elapsed(&self) -> f32 {
        match self {
            Action::None => 0.0,
            Action::Move { elapsed, .. }
            | Action::Interact { elapsed, .. }
            | Action::Consume { elapsed, .. }
            | Action::Wait { elapsed, .. } => *elapsed,
        }
    }

    pub const fn is_none(&self) -> bool {
        matches!(self, Action::None)
    }

    /// Fraction complete, for an action worth drawing a reverse progress bar
    /// over. `None` for [`Action::None`]; for a walk, which has no notion of
    /// "done in n seconds" — only of having arrived or not; and for
    /// [`Action::Wait`], which a bar is deliberately not shown for either.
    /// Only [`Action::Interact`] and [`Action::Consume`] answer.
    pub fn progress(&self) -> Option<f32> {
        match self {
            Action::Interact {
                seconds, elapsed, ..
            }
            | Action::Consume { seconds, elapsed } => Some((elapsed / seconds).clamp(0.0, 1.0)),
            Action::None | Action::Move { .. } | Action::Wait { .. } => None,
        }
    }

    /// One tick of the action.
    ///
    /// A walk is the walker's to judge — it repairs the route if this tick's
    /// step was refused, and knows when the route is walked. Everything else
    /// is a clock. [`Action::None`] is finished, having nothing left to do.
    pub fn advance(&mut self, think: &Think<'_>, outcome: MoveOutcome, walk: &mut Walker) -> ActionState {
        match self {
            Action::None => ActionState::Finished,
            Action::Move { elapsed, .. } => {
                *elapsed += think.dt;
                walk.advance(think, outcome)
            }
            Action::Interact {
                seconds, elapsed, ..
            }
            | Action::Consume { seconds, elapsed }
            | Action::Wait { seconds, elapsed } => {
                *elapsed += think.dt;
                if *elapsed >= *seconds {
                    ActionState::Finished
                } else {
                    ActionState::Running
                }
            }
        }
    }

    /// How it reads in a debug view.
    pub fn describe(&self) -> String {
        match self {
            Action::None => "none".to_string(),
            Action::Move { to, elapsed } => {
                format!("move -> {}, {}   {elapsed:.2}s", to.x, to.y)
            }
            Action::Interact {
                cell,
                seconds,
                elapsed,
            } => format!("interact {}, {}   {elapsed:.2}/{seconds:.1}s", cell.x, cell.y),
            Action::Consume { seconds, elapsed } => format!("consume   {elapsed:.2}/{seconds:.1}s"),
            Action::Wait { seconds, elapsed } => format!("wait   {elapsed:.2}/{seconds:.1}s"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Map, Size, FLOOR};
    use crate::sim::testing::World;
    use crate::sim::uid::{EntityType, Uid};

    fn walker() -> Walker {
        Walker::new(Uid::new(EntityType::Human, 3), Point::new(1, 1), 2.0)
    }

    #[test]
    fn no_action_is_already_finished() {
        let world = World::new(Map::new(Size::new(4, 4), FLOOR));
        assert_eq!(
            Action::None.advance(&world.ctx(), MoveOutcome::Idle, &mut walker()),
            ActionState::Finished
        );
    }

    #[test]
    fn a_timed_action_runs_for_as_long_as_it_was_asked_and_then_finishes() {
        let mut world = World::new(Map::new(Size::new(4, 4), FLOOR));
        world.dt = 0.25;
        let mut walk = walker();
        for mut action in [Action::wait(1.0), Action::consume(1.0), Action::interact(Point::new(2, 1), 1.0)] {
            for _ in 0..3 {
                assert_eq!(action.advance(&world.ctx(), MoveOutcome::Idle, &mut walk), ActionState::Running);
            }
            assert_eq!(action.advance(&world.ctx(), MoveOutcome::Idle, &mut walk), ActionState::Finished);
            assert_eq!(action.elapsed(), 1.0);
        }
    }

    #[test]
    fn progress_only_answers_for_an_interact_or_a_consume() {
        let mut world = World::new(Map::new(Size::new(4, 4), FLOOR));
        world.dt = 0.25;
        let mut walk = walker();

        assert_eq!(Action::None.progress(), None);
        assert_eq!(Action::wait(1.0).progress(), None);

        let mut moving = Action::walk_to(&mut walk, &world.ctx(), Point::new(2, 1)).expect("open floor");
        assert_eq!(moving.progress(), None);
        moving.advance(&world.ctx(), MoveOutcome::Idle, &mut walk);
        assert_eq!(moving.progress(), None, "still no notion of a fraction done");

        let mut interacting = Action::interact(Point::new(2, 1), 1.0);
        assert_eq!(interacting.progress(), Some(0.0));
        interacting.advance(&world.ctx(), MoveOutcome::Idle, &mut walk);
        assert_eq!(interacting.progress(), Some(0.25));

        let mut consuming = Action::consume(1.0);
        for _ in 0..4 {
            consuming.advance(&world.ctx(), MoveOutcome::Idle, &mut walk);
        }
        assert_eq!(consuming.progress(), Some(1.0), "never past done, however long it ran");
    }

    #[test]
    fn a_walk_is_never_finished_by_its_own_clock() {
        // Only the walker knows when a route is walked; a body that never moves
        // never gets there, however long it has been going.
        let world = World::new(Map::new(Size::new(9, 9), FLOOR));
        let mut walk = walker();
        let mut action = Action::walk_to(&mut walk, &world.ctx(), Point::new(6, 6)).expect("open floor");
        for _ in 0..1000 {
            assert_eq!(action.advance(&world.ctx(), MoveOutcome::Idle, &mut walk), ActionState::Running);
        }
    }

    #[test]
    fn a_walk_with_no_way_there_is_no_action_at_all() {
        let world = World::new(Map::new(Size::new(9, 9), FLOOR));
        assert_eq!(Action::walk_to(&mut walker(), &world.ctx(), Point::new(40, 40)), None);
    }
}
