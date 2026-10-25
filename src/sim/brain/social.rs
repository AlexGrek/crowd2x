//! Step 0's company: starting a chat with somebody just seen, taking one up,
//! keeping up with how one is going, and the nightly loss.
//!
//! A chat is agreed with no brain reading another's (`sim::relationships`):
//!
//! ```text
//! A wanders, sees B for the first time in a while, and rolls the chance
//!   (`relationships::chance`, from the level of what A thinks of B):
//!   A remembers   Chat { with: B, Inviting, leads }      and wanders on
//! the next tick, the think step lists A as asking B (`Chats`)
//! B, wandering, sees itself asked:
//!   B remembers   Chat { with: A, Talking, waits }       and stops (the goal)
//! the next tick, A sees B has agreed:
//!   A remembers   Chat { with: B, Talking, leads }       and goes over
//! ```
//!
//! Two who ask each other at once both see the other asking, and agree; the
//! lower uid goes over. From then on [`ChatGoal`](super::goals::ChatGoal) has
//! the walk and the talk, and this keeps watch: a partner who drops out of
//! the crowd's chats has given up, so this side does too; an invitation
//! nobody takes up within the minutes to answer, and a chat that has gone on
//! the minutes to meet without ending, are forgotten ([`Rules`]). **A chat is only kept
//! while it is what the unit is doing**: an invitation by somebody who has
//! stopped wandering, and an agreed chat whose goal was not in charge last
//! tick — something more pressing came up, or the goal is held off — are
//! dropped, so the other is never left standing waiting for somebody busy
//! elsewhere. One side done waits the minutes to wait for the other, no more.
//!
//! [`Rules`]: crate::sim::relationships::Rules
//!
//! A person's only: nobody else is ever asked, and nobody else asks.

use rand::RngExt;

use crate::sim::entity::{Body, Think};
use crate::map::Point;
use crate::sim::relationships::{Chat, Phase};
use crate::sim::rng::tick_rng;
use crate::sim::uid::{EntityType, Uid};

use super::goal::GoalId;
use super::memory::Memory;

/// `other`, a person, has just come into view in `cell` — news to
/// attention. If this unit is wandering, in no chat, and has not chatted with
/// them in the last hour, it may ask them for one, at the chance what it
/// thinks of them says ([`Rules::chance`]). `top` is the goal that
/// was in charge as the last tick ended. Somebody already in a chat is not
/// asked, nor somebody it could not walk over to: seen through a window in a
/// house it holds no key to, or across water (an O(1) look at the map's
/// components, never a route).
///
/// [`Rules::chance`]: crate::sim::relationships::Rules::chance
pub(super) fn notice(ctx: &Think<'_>, body: &Body, memory: &mut Memory, top: GoalId, other: Uid, cell: Point) {
    if top != GoalId::Wander
        || other.kind() != Some(EntityType::Human)
        || memory.chat().is_some()
        || ctx.chats.get(other).is_some()
        || !ctx.may_use(cell, body.home())
        || !ctx.map.navigation().may_reach(body.center_position(), cell)
    {
        return;
    }
    let me = body.uid();
    let now = ctx.clock.elapsed();
    let relations = memory.relations();
    let rules = ctx.society.rules();
    if relations.chatted_lately(other, now, rules) {
        return;
    }
    let level = ctx.society.level(me, other, relations.points(other));
    // A roll of its own for each person seen: two seen in one look are two
    // chances, not one.
    let roll: f32 = tick_rng(me, ctx.tick ^ other.raw()).random();
    if roll < rules.chance(level) {
        memory.set_chat(Chat { with: other, phase: Phase::Inviting, leads: true, since: now });
    }
}

/// Every tick: take up an invitation while wandering, see whether one made
/// was taken up, and let go of a chat the other has dropped out of or that
/// has gone on too long. Reads the crowd's chats as the think step gathered
/// them this tick — the other's side as it ended the last.
pub(super) fn tend(ctx: &Think<'_>, body: &Body, memory: &mut Memory, top: GoalId) {
    let me = body.uid();
    let Some(chat) = memory.chat() else {
        if top != GoalId::Wander || ctx.chats.is_empty() {
            return;
        }
        let now = ctx.clock.elapsed();
        let inviter = ctx
            .chats
            .invitations(me)
            .map(|invitation| invitation.who)
            .find(|&inviter| !memory.relations().chatted_lately(inviter, now, ctx.society.rules()));
        if let Some(inviter) = inviter {
            memory.set_chat(Chat { with: inviter, phase: Phase::Talking, leads: false, since: now });
        }
        return;
    };
    let now = ctx.clock.elapsed();
    let rules = ctx.society.rules();
    let partner = ctx.chats.get(chat.with).filter(|partner| partner.with == me);
    match chat.phase {
        Phase::Inviting => match partner {
            // No longer wandering: something else came up before anybody
            // answered, and nobody is to wait for it.
            _ if top != GoalId::Wander => memory.forget_chat(),
            // Taken up — or asked back at the same moment, in which case
            // the lower uid goes over.
            Some(partner) => memory.set_chat(Chat {
                phase: Phase::Talking,
                leads: partner.phase != Phase::Inviting || me < chat.with,
                since: now,
                ..chat
            }),
            None if now - chat.since >= rules.invite_patience() => memory.forget_chat(),
            None => {}
        },
        Phase::Talking => {
            // Agreed on an earlier tick, so the chat should have been in
            // charge since: if it was not, something more pressing is, or
            // the goal is held off, and the other would wait for nothing.
            let elsewhere = top != GoalId::Chat && chat.since < now;
            if elsewhere || partner.is_none() || now - chat.since >= rules.chat_patience() {
                memory.forget_chat();
            }
        }
        // Waiting for the other to have its say: over once it has, or gone,
        // or not coming.
        Phase::Done => {
            if !partner.is_some_and(|partner| partner.phase == Phase::Talking) || now - chat.since >= rules.done_patience() {
                memory.forget_chat();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Map, Size, FLOOR};
    use crate::sim::clock::{Clock, MINUTE, TIME_SCALE};
    use crate::sim::relationships::{Chatter, Rules};
    use crate::sim::testing::World;

    const ME: Point = Point::new(3, 3);
    const OTHER: Point = Point::new(5, 3);

    fn me() -> Body {
        Body::at_cell(Uid::new(EntityType::Human, 1), ME)
    }

    fn other() -> Uid {
        Uid::new(EntityType::Human, 2)
    }

    /// A world `minutes` of world time in, with the other in `phase` of a
    /// chat with [`me`] — or in none.
    fn world(minutes: f32, phase: Option<Phase>) -> World {
        let mut world = World::new(Map::new(Size::new(9, 9), FLOOR));
        world.clock = Clock::after_watching((minutes * MINUTE / TIME_SCALE) as f64);
        let chatter = phase.map(|phase| Chatter { who: other(), with: me().uid(), phase, cell: OTHER });
        world.chats.set(other(), chatter);
        world
    }

    fn chatting(phase: Phase, since_minutes: f32) -> Memory {
        let mut memory = Memory::new();
        memory.set_chat(Chat { with: other(), phase, leads: true, since: (since_minutes * MINUTE) as f64 });
        memory
    }

    #[test]
    fn an_invitation_is_dropped_by_somebody_who_stopped_wandering() {
        let world = world(10.0, None);
        let mut memory = chatting(Phase::Inviting, 10.0);
        tend(&world.ctx(), &me(), &mut memory, GoalId::Wander);
        assert!(memory.chat().is_some(), "still wandering, still asking");
        tend(&world.ctx(), &me(), &mut memory, GoalId::Eat);
        assert!(memory.chat().is_none());
    }

    #[test]
    fn an_invitation_taken_up_is_agreed_and_one_nobody_takes_up_is_forgotten() {
        let world_ = world(10.0, Some(Phase::Talking));
        let mut memory = chatting(Phase::Inviting, 10.0);
        tend(&world_.ctx(), &me(), &mut memory, GoalId::Wander);
        assert_eq!(memory.chat().map(|chat| (chat.phase, chat.leads)), Some((Phase::Talking, true)));

        let world_ = world(10.0 + 2.0, None);
        let mut memory = chatting(Phase::Inviting, 10.0);
        tend(&world_.ctx(), &me(), &mut memory, GoalId::Wander);
        assert!(memory.chat().is_none(), "two minutes and no answer");
    }

    #[test]
    fn two_who_ask_each_other_at_once_agree_and_the_lower_uid_goes_over() {
        let world = world(10.0, Some(Phase::Inviting));
        let mut memory = chatting(Phase::Inviting, 10.0);
        tend(&world.ctx(), &me(), &mut memory, GoalId::Wander);
        assert_eq!(memory.chat().map(|chat| (chat.phase, chat.leads)), Some((Phase::Talking, me().uid() < other())));
    }

    #[test]
    fn an_agreed_chat_is_dropped_once_something_else_is_in_charge() {
        // The tick it was agreed, the chat has not had its chance yet.
        let world_ = world(10.0, Some(Phase::Talking));
        let mut memory = chatting(Phase::Talking, 10.0);
        tend(&world_.ctx(), &me(), &mut memory, GoalId::Wander);
        assert!(memory.chat().is_some());
        // A tick on, with the chat in charge, it goes on...
        let world_ = world(10.5, Some(Phase::Talking));
        tend(&world_.ctx(), &me(), &mut memory, GoalId::Chat);
        assert!(memory.chat().is_some());
        // ...and with anything else in charge — a meal, or wandering on
        // with the chat held off — it is over.
        for top in [GoalId::Eat, GoalId::Wander] {
            let mut memory = chatting(Phase::Talking, 10.0);
            tend(&world_.ctx(), &me(), &mut memory, top);
            assert!(memory.chat().is_none(), "{top:?}");
        }
    }

    #[test]
    fn a_chat_whose_partner_dropped_out_is_over() {
        let world = world(11.0, None);
        let mut memory = chatting(Phase::Talking, 10.0);
        tend(&world.ctx(), &me(), &mut memory, GoalId::Chat);
        assert!(memory.chat().is_none());
    }

    #[test]
    fn one_side_done_waits_for_the_other_a_few_minutes_and_no_more() {
        let mut memory = chatting(Phase::Done, 10.0);
        tend(&world(12.0, Some(Phase::Talking)).ctx(), &me(), &mut memory, GoalId::Chat);
        assert!(memory.chat().is_some(), "the other is still talking");
        tend(&world(16.0, Some(Phase::Talking)).ctx(), &me(), &mut memory, GoalId::Chat);
        assert!(memory.chat().is_none(), "five minutes is long enough");

        let mut memory = chatting(Phase::Done, 10.0);
        tend(&world(10.5, Some(Phase::Done)).ctx(), &me(), &mut memory, GoalId::Chat);
        assert!(memory.chat().is_none(), "both done: over for both");
    }

    #[test]
    fn a_wanderer_takes_up_an_invitation_but_not_from_somebody_it_chatted_with_lately() {
        let world_ = world(10.0, Some(Phase::Inviting));
        let mut memory = Memory::new();
        tend(&world_.ctx(), &me(), &mut memory, GoalId::Eat);
        assert!(memory.chat().is_none(), "busy");
        tend(&world_.ctx(), &me(), &mut memory, GoalId::Wander);
        assert_eq!(memory.chat().map(|chat| (chat.with, chat.phase, chat.leads)), Some((other(), Phase::Talking, false)));

        let mut memory = Memory::new();
        memory.relations_mut().chatted(other(), 0.0, 5.0, (9.0 * MINUTE) as f64, &Rules::default());
        tend(&world_.ctx(), &me(), &mut memory, GoalId::Wander);
        assert!(memory.chat().is_none(), "a minute since the last");
    }

    /// Asked over many ticks — each its own roll — how often a wanderer
    /// `from` asks somebody seen in `cell` for a chat.
    fn invitations(world: &mut World, from: Body, cell: Point) -> usize {
        (0..400)
            .filter(|_| {
                world.tick += 1;
                let mut memory = Memory::new();
                notice(&world.ctx(), &from, &mut memory, GoalId::Wander, other(), cell);
                memory.chat().is_some()
            })
            .count()
    }

    #[test]
    fn a_wanderer_asks_a_stranger_now_and_then_but_never_one_it_could_not_walk_to() {
        let mut open = world(10.0, None);
        let asked = invitations(&mut open, me(), OTHER);
        assert!((10..80).contains(&asked), "{asked} of 400 at a tenth");

        // Somebody seen through the window of a house it holds no key to.
        let mut house = World::new(crate::sim::property::tests::house());
        let outside = Body::at_cell(me().uid(), Point::new(5, 0));
        assert_eq!(invitations(&mut house, outside, Point::new(5, 5)), 0);
        assert!(invitations(&mut house, outside, Point::new(9, 1)) > 0, "while one in the street is asked");
        // ...and nobody asks when it is not wandering.
        let mut memory = Memory::new();
        notice(&open.ctx(), &me(), &mut memory, GoalId::Eat, other(), OTHER);
        assert!(memory.chat().is_none());
    }
}
