//! Actions: what a unit is physically doing this tick.
//!
//! **Pathfinding and animation only — no decision at this level.** An action
//! is a task being carried out: it knows how long it has been going and
//! whether it is over, and nothing about why it was started or what should
//! come after it. What finishing one *means* — food in hand, a need met — is
//! for the goal that queued it to say, when it is told the result.

use crate::map::Point;

use super::task::Task;

/// The task currently being carried out, and how far into it.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Action {
    /// Nothing. Advances straight to [`ActionState::Finished`], so a fresh
    /// brain runs its executor on its first reaction rather than a tick later.
    #[default]
    None,
    /// The route lives in `Walker`; this is the fact that walking is what is
    /// being done, and where it ends.
    Move { to: Point, elapsed: f32 },
    Interact { cell: Point, seconds: f32, elapsed: f32 },
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
    /// The action that carries out `task`, at its very start.
    pub fn start(task: Task) -> Action {
        match task {
            Task::MoveTo { cell } => Action::Move {
                to: cell,
                elapsed: 0.0,
            },
            Task::Interact { cell, seconds } => Action::Interact {
                cell,
                seconds,
                elapsed: 0.0,
            },
            Task::Wait { seconds } => Action::Wait {
                seconds,
                elapsed: 0.0,
            },
        }
    }

    /// The short name an animation would be picked by.
    pub const fn name(&self) -> &'static str {
        match self {
            Action::None => "none",
            Action::Move { .. } => "move",
            Action::Interact { .. } => "interact",
            Action::Wait { .. } => "wait",
        }
    }

    /// Seconds since it started — what an interaction animation would be
    /// driven from.
    pub const fn elapsed(&self) -> f32 {
        match self {
            Action::None => 0.0,
            Action::Move { elapsed, .. }
            | Action::Interact { elapsed, .. }
            | Action::Wait { elapsed, .. } => *elapsed,
        }
    }

    pub const fn is_none(&self) -> bool {
        matches!(self, Action::None)
    }

    /// Advance a timed action by `dt`, standing in `here`.
    ///
    /// Walking is not timed — it ends when the route does, and the walker is
    /// the one who knows that — so a [`Action::Move`] only has its clock moved
    /// here and reports [`ActionState::Running`]; the brain asks the walker
    /// for the real answer.
    ///
    /// An interaction checks where it is standing on every tick rather than
    /// only when it starts: nothing moves a unit that is not walking today,
    /// and the day something can, a check made once would go on using a
    /// fridge from across the room.
    pub fn advance(&mut self, dt: f32, here: Point) -> ActionState {
        match self {
            Action::None => ActionState::Finished,
            Action::Move { elapsed, .. } => {
                *elapsed += dt;
                ActionState::Running
            }
            Action::Interact {
                cell,
                seconds,
                elapsed,
            } => {
                if here.manhattan_distance(*cell) > 1 {
                    return ActionState::Failed;
                }
                *elapsed += dt;
                timed(*elapsed, *seconds)
            }
            Action::Wait { seconds, elapsed } => {
                *elapsed += dt;
                timed(*elapsed, *seconds)
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
            Action::Wait { seconds, elapsed } => format!("wait   {elapsed:.2}/{seconds:.1}s"),
        }
    }
}

fn timed(elapsed: f32, seconds: f32) -> ActionState {
    if elapsed >= seconds {
        ActionState::Finished
    } else {
        ActionState::Running
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 0.25;

    #[test]
    fn no_action_is_already_finished() {
        // So a brain with nothing to do asks its goal for something on the
        // same reaction, not one tick after.
        assert_eq!(Action::None.advance(DT, Point::new(0, 0)), ActionState::Finished);
    }

    #[test]
    fn a_wait_runs_for_as_long_as_it_was_asked_and_then_finishes() {
        let mut wait = Action::start(Task::Wait { seconds: 1.0 });
        let here = Point::new(0, 0);
        for _ in 0..3 {
            assert_eq!(wait.advance(DT, here), ActionState::Running);
        }
        assert_eq!(wait.advance(DT, here), ActionState::Finished);
        assert_eq!(wait.elapsed(), 1.0);
    }

    #[test]
    fn an_interaction_from_across_the_room_fails_rather_than_walking() {
        let fridge = Point::new(5, 5);
        let mut far = Action::start(Task::Interact {
            cell: fridge,
            seconds: 1.0,
        });
        assert_eq!(far.advance(DT, Point::new(1, 1)), ActionState::Failed);

        let mut beside = Action::start(Task::Interact {
            cell: fridge,
            seconds: 0.5,
        });
        assert_eq!(beside.advance(DT, Point::new(5, 4)), ActionState::Running);
        assert_eq!(beside.advance(DT, Point::new(5, 4)), ActionState::Finished);
    }

    #[test]
    fn a_move_is_never_finished_by_its_own_clock() {
        // Only the walker knows when a route is walked.
        let mut walk = Action::start(Task::MoveTo {
            cell: Point::new(3, 3),
        });
        for _ in 0..1000 {
            assert_eq!(walk.advance(DT, Point::new(0, 0)), ActionState::Running);
        }
    }
}
