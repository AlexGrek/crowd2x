//! [`Talk`]: stand by somebody and chat with them.
//!
//! A place and a time, like any timed task — the place being the cell the
//! other stood in when the talk began, which is what the unit turns towards.
//! **Who** the other is, and where it is now, is asked of the crowd every
//! tick ([`Chats`](crate::sim::relationships::Chats)): this unit's own entry
//! says who it is chatting with, and theirs must say the same, agreed, and
//! within a step — wherever they stand, since one that was still walking up
//! when the talk began takes another step to stand beside it. It may be done
//! already — the other started a moment sooner — and waiting for this one to
//! finish, which is still a chat.

use crate::map::Point;
use crate::sim::biology::Event;
use crate::sim::relationships::Phase;

use super::super::action::{Action, ActionState};
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Talk {
    /// The cell the other stood in when the talk began: what this unit
    /// faces.
    pub with: Point,
    /// How long the chat takes.
    pub seconds: f32,
}

impl TaskExecutor for Talk {
    /// **Only within a step of the other, and only while it is still in this
    /// chat, checked every tick.** What finishing means — what it thinks of
    /// the other going up — is the goal's, which hears of it next tick: a
    /// task never writes memory.
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        let me = ctx.walk.body().uid();
        let chats = ctx.think.chats;
        let here = ctx.here();
        let listening = chats
            .get(me)
            .and_then(|mine| chats.get(mine.with))
            .is_some_and(|other| other.with == me && other.phase != Phase::Inviting && here.chebyshev_distance(other.cell) <= 1);
        if !listening {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.with, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                if let Some(biology) = ctx.biology.as_deref_mut() {
                    biology.handle(Event::Chatted);
                }
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }

    fn describe(&self) -> String {
        format!("talk to whoever was at {}, {}", self.with.x, self.with.y)
    }
}

impl Task {
    /// Chat for `seconds` with whoever stands in `with`, from a step away.
    pub const fn talk(with: Point, seconds: f32) -> Task {
        Task::Talk(Talk { with, seconds })
    }
}

/// Saved as `talk`: the other's cell and the seconds.
impl TimedTask for Talk {
    const NAME: &'static str = "talk";

    fn new(cell: Point, seconds: f32) -> Talk {
        Talk { with: cell, seconds }
    }

    fn cell(&self) -> Point {
        self.with
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
    use crate::sim::relationships::Chatter;
    use crate::sim::uid::{EntityType, Uid};

    const ME: Point = Point::new(3, 3);
    const OTHER: Point = Point::new(4, 3);

    /// The rig at [`ME`], in a chat with somebody at `cell` who is in `phase`
    /// of it with the rig — or in none.
    fn talking(phase: Option<Phase>, cell: Point) -> (Rig, Uid) {
        let mut rig = Rig::new(Map::new(Size::new(9, 9), FLOOR), ME);
        let me = rig.walk.body().uid();
        let other = Uid::new(EntityType::Human, 77);
        rig.world.chats.set(me, Some(Chatter { who: me, with: other, phase: Phase::Talking, cell: ME, talk: 1.0 }));
        rig.world.chats.set(other, phase.map(|phase| Chatter { who: other, with: me, phase, cell, talk: 1.0 }));
        (rig, other)
    }

    #[test]
    fn a_talk_runs_its_time_while_the_other_stays_in_the_chat() {
        let (mut rig, _) = talking(Some(Phase::Talking), OTHER);
        let mut task = Task::talk(OTHER, 1.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.run(&mut task, 200), TaskResult::Success);
    }

    /// A chat had is fun — once it is over, and not before; one cut short by
    /// the other walking off is none.
    #[test]
    fn a_talk_had_to_its_end_is_fun_and_one_cut_short_is_not() {
        use crate::sim::biology::fun::CHAT_FUN;
        let (mut rig, _) = talking(Some(Phase::Talking), OTHER);
        rig.biology.edit(|stats| stats.with_fun(30.0));
        let mut task = Task::talk(OTHER, 1.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        assert_eq!(rig.biology.stats().fun(), 30.0, "not yet");
        assert_eq!(rig.run(&mut task, 200), TaskResult::Success);
        assert_eq!(rig.biology.stats().fun(), 30.0 + CHAT_FUN);

        let (mut rig, other) = talking(Some(Phase::Talking), OTHER);
        rig.biology.edit(|stats| stats.with_fun(30.0));
        let mut task = Task::talk(OTHER, 1.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        rig.world.chats.set(other, None);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);
        assert_eq!(rig.biology.stats().fun(), 30.0, "walked off mid-sentence");
    }

    #[test]
    fn a_talk_ends_well_with_somebody_who_has_already_had_their_say() {
        let (mut rig, _) = talking(Some(Phase::Done), OTHER);
        let mut task = Task::talk(OTHER, 1.0);
        assert_eq!(rig.run(&mut task, 200), TaskResult::Success);
    }

    #[test]
    fn a_talk_goes_on_while_the_other_takes_its_last_step_up() {
        // It began with the other passing a corner, and the other stepped
        // round to stand beside: still within a step, still a chat.
        let (mut rig, other) = talking(Some(Phase::Talking), Point::new(4, 4));
        let mut task = Task::talk(Point::new(4, 4), 1.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        let me = rig.walk.body().uid();
        rig.world.chats.set(other, Some(Chatter { who: other, with: me, phase: Phase::Talking, cell: OTHER, talk: 1.0 }));
        assert_eq!(rig.run(&mut task, 200), TaskResult::Success);
    }

    #[test]
    fn a_talk_fails_the_moment_the_other_leaves_the_chat_or_walks_off() {
        let (mut rig, other) = talking(Some(Phase::Talking), OTHER);
        let mut task = Task::talk(OTHER, 1.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        rig.world.chats.set(other, None);
        assert_eq!(rig.tick(&mut task), TaskResult::Failed);

        let (mut rig, other) = talking(Some(Phase::Talking), OTHER);
        let mut task = Task::talk(OTHER, 1.0);
        assert_eq!(rig.tick(&mut task), TaskResult::Executing);
        let me = rig.walk.body().uid();
        rig.world.chats.set(other, Some(Chatter { who: other, with: me, phase: Phase::Done, cell: Point::new(6, 3), talk: 1.0 }));
        assert_eq!(rig.tick(&mut task), TaskResult::Failed, "two steps off");
    }

    #[test]
    fn nobody_talks_to_somebody_who_has_not_agreed_or_is_out_of_reach() {
        let (mut rig, _) = talking(Some(Phase::Inviting), OTHER);
        assert_eq!(rig.tick(&mut Task::talk(OTHER, 1.0)), TaskResult::Failed);
        let (mut rig, _) = talking(Some(Phase::Talking), Point::new(6, 3));
        assert_eq!(rig.tick(&mut Task::talk(Point::new(6, 3), 1.0)), TaskResult::Failed, "two steps away");
        let (mut rig, _) = talking(None, OTHER);
        assert_eq!(rig.tick(&mut Task::talk(OTHER, 1.0)), TaskResult::Failed, "in no chat at all");
    }
}
