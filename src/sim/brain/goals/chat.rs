//! [`ChatGoal`]: go over to somebody and have a chat.
//!
//! What a chat *is* — who asked whom, whether both have agreed, whether this
//! side is done — is the unit's memory ([`Memory::chat`]), started and kept
//! up in step 0 of the brain (`brain::social`), where a wanderer sees
//! somebody and asks, or sees itself asked and takes it up. This goal is in
//! charge once both have agreed ([`ChatRoutine`](super::super::routines::ChatRoutine)),
//! and is only the walk and the talk:
//!
//! 1. **The one who asked walks over** ([`Chat::leads`]) beside the other,
//!    whose cell the crowd's [`Chats`](crate::sim::relationships::Chats) says;
//!    the other waits where it is, looking again every [`LISTEN_SECONDS`].
//! 2. **Within a step of each other, both talk** ([`Task::talk`], for the
//!    rules' minutes of talk) — each its own talk, each started when it sees
//!    the other there and agreed.
//! 3. **A talk that ends raises what it thinks of the other**
//!    ([`Relations::chatted`]: the level, floor and all, and a chat's gain
//!    more, [`Rules::chat_gain`]), remembers when, and marks its side done. It stays
//!    put, waiting, until the other is done too — so a chat ends for both at
//!    once and neither walks off from the other mid-sentence.
//!
//! Put down for anything more pressing mid-chat, it is over
//! ([`GoalExecutor::deprioritized`] forgets it), and the other, seeing its
//! partner gone from the crowd's chats, gives up as well.
//!
//! [`Memory::chat`]: super::super::memory::Memory::chat
//! [`Chat::leads`]: crate::sim::relationships::Chat
//! [`Relations::chatted`]: crate::sim::relationships::Relations::chatted
//! [`Rules::chat_gain`]: crate::sim::relationships::Rules::chat_gain

use crate::save::{invalid, pb, Invalid};
use crate::sim::clock::{watched, MINUTE};
use crate::sim::relationships::{Chat, Phase};
use crate::sim::rng::tick_rng;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::{stand_beside, Stand, PATIENCE, WAIT_FOR_A_GAP};

/// How long the one who waits — for the other to come over, to agree, or to
/// finish — stands before looking again. Half a second of watching.
pub const LISTEN_SECONDS: f32 = watched(MINUTE);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ChatGoal {
    /// Walks over cut short by somebody in the way, in a row.
    retries: u8,
}

impl ChatGoal {
    pub fn new() -> ChatGoal {
        ChatGoal::default()
    }
}

/// This side's say is had: what it thinks of the other goes up, and it is
/// done ([`Phase::Done`]) — but stays, until the other is too.
fn finish(ctx: &mut GoalCtx<'_>, chat: Chat) {
    let me = ctx.body.uid();
    let now = ctx.think.clock.elapsed();
    let rules = ctx.think.society.rules();
    let before = ctx.think.society.level(me, chat.with, ctx.memory.relations().points(chat.with));
    // A stream of its own: not the one a wander or a first look drew from.
    let gain = rules.chat_gain(&mut tick_rng(me, ctx.think.tick ^ chat.with.raw()));
    let after = ctx.memory.relations_mut().chatted(chat.with, before, gain, now, rules);
    ctx.memory.set_chat(Chat { phase: Phase::Done, since: now, ..chat });
    ctx.think
        .log
        .related(me, chat.with, format!("{me} chatted with {}: {before:.0} -> {after:.0}", chat.with));
}

/// Whether what just ended was a talk.
fn talked(ctx: &GoalCtx<'_>) -> bool {
    matches!(ctx.finished, Some(Task::Talk(_)))
}

impl GoalExecutor for ChatGoal {
    fn goal(&self) -> GoalId {
        GoalId::Chat
    }

    fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        let _ = ctx;
        self.retries = 0;
    }

    /// A talk that ended just before something else took over still counts;
    /// a chat put down before that is over.
    fn deprioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        let Some(chat) = ctx.memory.chat() else {
            return;
        };
        if chat.phase != Phase::Talking {
            return;
        }
        if ctx.tasks.result() == TaskResult::Success && talked(ctx) {
            finish(ctx, chat);
        } else {
            ctx.memory.forget_chat();
        }
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        let Some(chat) = ctx.memory.chat() else {
            // Over already; the routine lets go next tick.
            return GoalProgress::Achieved;
        };
        match chat.phase {
            // Not agreed yet: nothing for this goal to do, and the routine
            // does not want it.
            Phase::Inviting => return GoalProgress::Achieved,
            // Its say had: wait for the other to have theirs.
            Phase::Done => {
                let _ = ctx.tasks.push_back(Task::wait(LISTEN_SECONDS));
                return GoalProgress::Working;
            }
            Phase::Talking => {}
        }
        if talked(ctx) {
            if last == TaskResult::Success {
                finish(ctx, chat);
                let _ = ctx.tasks.push_back(Task::wait(LISTEN_SECONDS));
                return GoalProgress::Working;
            }
            if last == TaskResult::Failed {
                // The other walked off, or was taken off, mid-sentence.
                ctx.memory.forget_chat();
                return GoalProgress::Achieved;
            }
        }

        let me = ctx.body.uid();
        let Some(partner) = ctx.think.chats.get(chat.with).filter(|partner| partner.with == me).copied() else {
            // Gone: busy with something else, frozen, dead, or gone off with
            // somebody else.
            ctx.memory.forget_chat();
            return GoalProgress::Achieved;
        };

        let here = ctx.body.center_position();
        if here.chebyshev_distance(partner.cell) <= 1 {
            self.retries = 0;
            if partner.phase == Phase::Inviting {
                // It has not seen this side agree yet: a tick or two.
                let _ = ctx.tasks.push_back(Task::wait(LISTEN_SECONDS));
            } else {
                let _ = ctx.tasks.push_back(Task::talk(partner.cell, ctx.think.society.rules().talk_seconds()));
            }
            return GoalProgress::Working;
        }
        if !chat.leads {
            // The other comes over.
            let _ = ctx.tasks.push_back(Task::wait(LISTEN_SECONDS));
            return GoalProgress::Working;
        }

        if last == TaskResult::Failed && matches!(ctx.finished, Some(Task::MoveTo(_))) {
            if ctx.blocked_by.is_some() && self.retries < PATIENCE {
                self.retries += 1;
                let _ = ctx.tasks.push_back(Task::wait(WAIT_FOR_A_GAP));
                return GoalProgress::Working;
            }
            // No way over, or no end to the crowd between them.
            ctx.memory.forget_chat();
            return GoalProgress::Blocked;
        }
        match stand_beside(ctx, partner.cell) {
            Stand::Here => {
                let _ = ctx.tasks.push_back(Task::talk(partner.cell, ctx.think.society.rules().talk_seconds()));
                GoalProgress::Working
            }
            Stand::At(cell) => {
                let _ = ctx.tasks.push_back(Task::move_beside(cell));
                GoalProgress::Working
            }
            Stand::Nowhere => {
                ctx.memory.forget_chat();
                GoalProgress::Blocked
            }
        }
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![("chat retries", self.retries.to_string())]
    }

    // --- saving ---

    /// A seek with no place, its retries the walks over cut short.
    fn save(&self) -> pb::Executor {
        let ChatGoal { retries } = *self;
        pb::Executor::Seek(pb::Seek { target: None, retries: retries as u32, reroutes: 0 })
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        let pb::Executor::Seek(seek) = saved else {
            return invalid("chat is a seek");
        };
        let Ok(retries) = u8::try_from(seek.retries) else {
            return invalid(format!("chat: {} retries", seek.retries));
        };
        self.retries = retries;
        Ok(())
    }
}
