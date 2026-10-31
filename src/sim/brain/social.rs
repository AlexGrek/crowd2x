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
//! **Small talk** is agreed the same way and is far less: a person awake with
//! somebody close by starts one now and then on a look round ([`strike_up`]),
//! whatever it is doing — at work too — the other answers through the crowd's
//! `Smalltalks` ([`small_talk`]), and each thinks the smalltalk gain better of
//! the other. No goal, no walk, no task: nothing is put down for it, and the
//! game draws the sparks.
//!
//! A person's only: nobody else is ever asked, and nobody else asks.

use rand::RngExt;

use crate::sim::entity::{Body, Think};
use crate::map::Point;
use crate::sim::relationships::{Chat, Phase, Smalltalk};
use crate::sim::rng::tick_rng;
use crate::sim::uid::{EntityType, Uid};

use super::goal::GoalId;
use super::memory::Memory;
use super::perception::Sightings;

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
        // How long the talk is to be is decided now, with the asking, and
        // the other talks for as long: drawn from a stream of its own.
        let talk = rules.draw_talk_seconds(&mut tick_rng(me, ctx.tick ^ other.raw() ^ TALK_STREAM));
        memory.set_chat(Chat { with: other, phase: Phase::Inviting, leads: true, since: now, talk });
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
        let invitation = ctx
            .chats
            .invitations(me)
            .find(|invitation| !memory.relations().chatted_lately(invitation.who, now, ctx.society.rules()));
        if let Some(invitation) = invitation {
            // For as long as the one who asked said.
            memory.set_chat(Chat { with: invitation.who, phase: Phase::Talking, leads: false, since: now, talk: invitation.talk });
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
            Some(partner) => {
                let leads = partner.phase != Phase::Inviting || me < chat.with;
                // Of two who asked at once, the length is the one who goes
                // over's.
                let talk = if leads { chat.talk } else { partner.talk };
                memory.set_chat(Chat { phase: Phase::Talking, leads, since: now, talk, ..chat });
            }
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

/// The stream a chat's length is drawn from, mixed into the tick so it is not
/// the roll that asked.
const TALK_STREAM: u64 = 0x7461_6c6b_6c65_6e67;

/// The stream small talk's rolls are drawn from, mixed into the tick so they
/// are not the draws a wander or a first look makes.
const SMALLTALK_STREAM: u64 = 0x736d_616c_6c74_616c;

/// On a look round, awake: with somebody awake close by — within the small
/// talk reach, a person, in no chat or small talk of its own — a person in
/// neither may start a small talk, at the smalltalks an hour spread over the
/// looks, scaled by mental health (zero at 0, full rate at 100), and not within the minutes between
/// smalltalks of the last it started. Whatever it is doing: at work, on its
/// way, waiting. One roll a look, however many are near; the first of them
/// seen is who it talks to.
///
/// [`Rules::smalltalk_chance`]: crate::sim::relationships::Rules::smalltalk_chance
pub(super) fn strike_up(ctx: &Think<'_>, body: &Body, memory: &mut Memory, sightings: &Sightings, elapsed: f32, mental_health: f32) {
    if memory.smalltalk().is_some() || memory.chat().is_some() {
        return;
    }
    let me = body.uid();
    let now = ctx.clock.elapsed();
    let rules = ctx.society.rules();
    if memory.smalltalked().is_some_and(|when| now - when < rules.smalltalk_again_after()) {
        return;
    }
    let roll: f32 = tick_rng(me, ctx.tick ^ SMALLTALK_STREAM).random();
    if roll >= rules.smalltalk_chance(elapsed, mental_health) {
        return;
    }
    let here = body.center_position();
    let reach = rules.smalltalk_reach();
    let near = sightings.iter().find(|seen| {
        seen.uid != me
            && seen.uid.kind() == Some(EntityType::Human)
            && here.chebyshev_distance(seen.cell) <= reach
            && ctx.chats.get(seen.uid).is_none()
            && ctx.smalltalks.get(seen.uid).is_none()
    });
    if let Some(other) = near {
        memory.set_smalltalk(Smalltalk { with: other.uid, since: now, answered: false });
        memory.started_smalltalk(now);
    }
}

/// Every tick: answer a small talk somebody close by has started with it,
/// see whether one it started was answered, and end one that has gone on its
/// minutes, whose other side has stopped, or whose other has walked off. Each side has its points — the
/// smalltalk gain on the level — when both are in it. Asleep, or in a chat,
/// it is in none.
///
/// Whether it had its say **this tick** — answered, or heard the answer — which
/// the brain tells the body of ([`Event::SmallTalk`]): this module writes memory
/// and never the body.
///
/// [`Event::SmallTalk`]: crate::sim::biology::Event::SmallTalk
pub(super) fn small_talk(ctx: &Think<'_>, body: &Body, memory: &mut Memory, asleep: bool) -> bool {
    let me = body.uid();
    let busy = asleep || memory.chat().is_some();
    let Some(smalltalk) = memory.smalltalk() else {
        if busy || ctx.smalltalks.is_empty() {
            return false;
        }
        let here = body.center_position();
        let reach = ctx.society.rules().smalltalk_reach();
        let opener = ctx.smalltalks.openings(me).find(|opener| here.chebyshev_distance(opener.cell) <= reach).map(|opener| opener.who);
        if let Some(opener) = opener {
            let now = ctx.clock.elapsed();
            memory.set_smalltalk(Smalltalk { with: opener, since: now, answered: true });
            memory.started_smalltalk(now);
            let (before, after) = gain(ctx, me, memory, opener);
            ctx.log.related(me, opener, format!("{opener} made small talk with {me}: {before:.1} -> {after:.1}"));
            return true;
        }
        return false;
    };
    let now = ctx.clock.elapsed();
    let rules = ctx.society.rules();
    let other = ctx.smalltalks.get(smalltalk.with).filter(|other| other.with == me);
    if busy {
        memory.forget_smalltalk();
    } else if !smalltalk.answered {
        match other {
            Some(_) => {
                memory.set_smalltalk(Smalltalk { answered: true, ..smalltalk });
                gain(ctx, me, memory, smalltalk.with);
                return true;
            }
            None if now - smalltalk.since >= rules.invite_patience() => memory.forget_smalltalk(),
            None => {}
        }
    } else if other.is_none_or(|other| drifted_apart(body, other.cell, rules.smalltalk_reach()))
        || now - smalltalk.since >= rules.smalltalk_length()
    {
        memory.forget_smalltalk();
    }
    false
}

/// Whether the other, in `cell`, has gone too far off to talk to: a step past
/// the reach — small talk walks on with whoever is having it, and one step's
/// slack keeps two walking side by side from breaking off and starting again.
fn drifted_apart(body: &Body, cell: Point, reach: i32) -> bool {
    body.center_position().chebyshev_distance(cell) > reach + 1
}

/// A small talk with `with` is had: what `me` thinks of them goes up by the
/// smalltalk gain, on the level, floor and all. What it was and came to.
fn gain(ctx: &Think<'_>, me: Uid, memory: &mut Memory, with: Uid) -> (f32, f32) {
    let rules = ctx.society.rules();
    let before = ctx.society.level(me, with, memory.relations().points(with));
    let after = memory.relations_mut().smalltalked(with, before, rules.smalltalk_gain, rules);
    (before, after)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Map, Size, FLOOR};
    use crate::sim::clock::{watched, Clock, MINUTE, TIME_SCALE};
    use crate::sim::relationships::{Chatter, Rule, Rules, Smalltalker};
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
        let chatter = phase.map(|phase| Chatter { who: other(), with: me().uid(), phase, cell: OTHER, talk: 1.0 });
        world.chats.set(other(), chatter);
        world
    }

    fn chatting(phase: Phase, since_minutes: f32) -> Memory {
        let mut memory = Memory::new();
        memory.set_chat(Chat { with: other(), phase, leads: true, since: (since_minutes * MINUTE) as f64, talk: 1.0 });
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

    /// How long a chat is to be is drawn by whoever asks, when it asks, between
    /// the rules' least and most minutes of talk — and the one who takes it up
    /// talks for as long.
    #[test]
    fn whoever_asks_draws_how_long_the_chat_is_and_the_other_talks_as_long() {
        let mut world_ = world(10.0, None);
        world_.society.change_rule(Rule::StrangerChance, 1.0);
        let rules = *world_.society.rules();
        let (least, most) = (watched(rules.least_minutes_of_talk * MINUTE), watched(rules.most_minutes_of_talk * MINUTE));
        let mut drawn = std::collections::BTreeSet::new();
        for _ in 0..50 {
            world_.tick += 1;
            let mut memory = Memory::new();
            notice(&world_.ctx(), &me(), &mut memory, GoalId::Wander, other(), OTHER);
            let talk = memory.chat().expect("a sure chance").talk;
            assert!((least..=most).contains(&talk), "{talk} not in {least}..={most}");
            drawn.insert(talk.to_bits());
        }
        assert!(drawn.len() > 10, "drawn afresh for every invitation: {drawn:?}");

        // Taken up: for as long as the one who asked said.
        let mut world_ = world(10.0, None);
        world_.chats.set(other(), Some(Chatter { who: other(), with: me().uid(), phase: Phase::Inviting, cell: OTHER, talk: 3.25 }));
        let mut memory = Memory::new();
        tend(&world_.ctx(), &me(), &mut memory, GoalId::Wander);
        assert_eq!(memory.chat().map(|chat| (chat.leads, chat.talk)), Some((false, 3.25)));
    }

    /// Two who asked each other at once talk for as long as the one who goes
    /// over — the lower uid — said, so both talk for the same.
    #[test]
    fn two_who_asked_at_once_talk_for_as_long_as_the_one_who_goes_over_said() {
        let (low, high) = (me(), Body::at_cell(Uid::new(EntityType::Human, 3), ME));
        for (body, partner, leads, expect) in [(low, other(), true, 1.0), (high, other(), false, 7.0)] {
            let mut world_ = world(10.0, None);
            world_.chats.set(partner, Some(Chatter { who: partner, with: body.uid(), phase: Phase::Inviting, cell: OTHER, talk: 7.0 }));
            let mut memory = Memory::new();
            memory.set_chat(Chat { with: partner, phase: Phase::Inviting, leads: true, since: (10.0 * MINUTE) as f64, talk: 1.0 });
            tend(&world_.ctx(), &body, &mut memory, GoalId::Wander);
            assert_eq!(memory.chat().map(|chat| (chat.leads, chat.talk)), Some((leads, expect)), "{}", body.uid());
        }
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

    // --- small talk ---

    /// The other, seen in `cell`.
    fn seen(cell: Point) -> Sightings {
        use super::super::perception::{Range, Sighting};
        Sightings::of(&[Sighting { uid: other(), cell, range: Range::Near }])
    }

    /// A world `minutes` in, the other in `talk` with [`me`], from [`OTHER`].
    fn talking_world(minutes: f32, talk: Option<bool>) -> World {
        let mut world = world(minutes, None);
        let talker = talk.map(|answered| Smalltalker { who: other(), with: me().uid(), answered, cell: OTHER });
        world.smalltalks.set(other(), talker);
        world
    }

    /// Looks a few seconds apart — each its own roll — and how many of them
    /// started a small talk with somebody seen in `cell`.
    fn openings(world: &mut World, cell: Point, smalltalks_an_hour: f64) -> usize {
        world.society.change_rule(Rule::SmalltalksAnHour, smalltalks_an_hour);
        (0..400)
            .filter(|_| {
                world.tick += 1;
                let mut memory = Memory::new();
                strike_up(&world.ctx(), &me(), &mut memory, &seen(cell), 60.0, 100.0);
                memory.smalltalk().is_some()
            })
            .count()
    }

    #[test]
    fn poorer_mental_health_starts_fewer_smalltalks_and_zero_starts_none() {
        let mut world = world(10.0, None);
        world.society.change_rule(Rule::SmalltalksAnHour, 10.0);
        let count = |world: &mut World, mental_health| {
            (0..4000).filter(|tick| {
                world.tick = *tick;
                let mut memory = Memory::new();
                strike_up(&world.ctx(), &me(), &mut memory, &seen(OTHER), 60.0, mental_health);
                memory.smalltalk().is_some()
            }).count()
        };
        let well = count(&mut world, 100.0);
        let low = count(&mut world, 25.0);
        assert!((550..800).contains(&well), "{well}");
        assert!((100..240).contains(&low), "{low}");
        assert!(low < well / 2);
        assert_eq!(count(&mut world, 0.0), 0);
    }

    #[test]
    fn somebody_close_by_is_drawn_into_small_talk_now_and_then_and_nobody_further_off() {
        // Ten an hour, a look a minute: one look in six.
        let started = openings(&mut world(10.0, None), OTHER, 10.0);
        assert!((40..100).contains(&started), "{started} of 400 at a sixth");
        // Three cells off, past the reach of two.
        assert_eq!(openings(&mut world(10.0, None), Point::new(6, 3), 1000.0), 0);
        // None while it is in a chat, or a small talk already.
        let mut world = world(10.0, None);
        world.society.change_rule(Rule::SmalltalksAnHour, 1000.0);
        let mut memory = chatting(Phase::Talking, 10.0);
        strike_up(&world.ctx(), &me(), &mut memory, &seen(OTHER), 60.0, 100.0);
        assert!(memory.smalltalk().is_none());
    }

    #[test]
    fn nobody_starts_a_small_talk_within_the_minutes_between_smalltalks_of_the_last() {
        let mut world = world(40.0, None);
        world.society.change_rule(Rule::SmalltalksAnHour, 1000.0);
        let mut memory = Memory::new();
        memory.started_smalltalk((20.0 * MINUTE) as f64);
        strike_up(&world.ctx(), &me(), &mut memory, &seen(OTHER), 60.0, 100.0);
        assert!(memory.smalltalk().is_none(), "twenty minutes since the last");
        world.clock = Clock::after_watching((51.0 * MINUTE / TIME_SCALE) as f64);
        strike_up(&world.ctx(), &me(), &mut memory, &seen(OTHER), 60.0, 100.0);
        assert_eq!(memory.smalltalk().map(|talk| (talk.with, talk.answered)), Some((other(), false)));
    }

    #[test]
    fn a_small_talk_answered_is_half_a_point_to_each_and_over_in_its_minutes() {
        // The other starts one; this side, awake and close by, answers...
        let world_ = talking_world(10.0, Some(false));
        let mut memory = Memory::new();
        small_talk(&world_.ctx(), &me(), &mut memory, false);
        assert_eq!(memory.smalltalk().map(|talk| (talk.with, talk.answered)), Some((other(), true)));
        assert_eq!(memory.relations().points(other()), 0.5);
        assert_eq!(memory.smalltalked(), Some((10.0 * MINUTE) as f64));
        // ...and goes on while the other does, for four minutes.
        small_talk(&talking_world(12.0, Some(true)).ctx(), &me(), &mut memory, false);
        assert!(memory.smalltalk().is_some());
        small_talk(&talking_world(14.0, Some(true)).ctx(), &me(), &mut memory, false);
        assert!(memory.smalltalk().is_none());
        assert_eq!(memory.relations().points(other()), 0.5, "the points stay");

        // The side that started one has its half point once it sees the
        // answer, on the level: colleagues' floor and all.
        let mut world_ = talking_world(10.0, Some(true));
        world_.society.join(me().uid(), None);
        world_.society.join(other(), None);
        world_.society.set_job(me().uid(), Some(crate::sim::job::JobKind::Guard));
        world_.society.set_job(other(), Some(crate::sim::job::JobKind::Guard));
        let mut memory = Memory::new();
        memory.set_smalltalk(Smalltalk { with: other(), since: (10.0 * MINUTE) as f64, answered: false });
        small_talk(&world_.ctx(), &me(), &mut memory, false);
        assert_eq!(memory.smalltalk().map(|talk| talk.answered), Some(true));
        assert_eq!(memory.relations().points(other()), 10.5);
    }

    /// Each side has its say once — answering, or seeing its opening answered
    /// — and that tick alone says so, for the brain to tell the body (a little
    /// fun, `Event::SmallTalk`).
    #[test]
    fn a_small_talk_is_had_once_by_each_side_and_says_so_on_that_tick_alone() {
        let mut memory = Memory::new();
        assert!(small_talk(&talking_world(10.0, Some(false)).ctx(), &me(), &mut memory, false), "answered");
        assert!(!small_talk(&talking_world(11.0, Some(true)).ctx(), &me(), &mut memory, false), "going on");
        assert!(!small_talk(&talking_world(14.0, Some(true)).ctx(), &me(), &mut memory, false), "over");

        let mut memory = Memory::new();
        memory.set_smalltalk(Smalltalk { with: other(), since: (10.0 * MINUTE) as f64, answered: false });
        assert!(!small_talk(&talking_world(10.0, None).ctx(), &me(), &mut memory, false), "not answered yet");
        assert!(small_talk(&talking_world(10.5, Some(true)).ctx(), &me(), &mut memory, false), "its opening answered");
        assert!(!small_talk(&talking_world(11.0, Some(true)).ctx(), &me(), &mut memory, false), "and no more");

        let mut memory = Memory::new();
        assert!(!small_talk(&talking_world(10.0, Some(false)).ctx(), &me(), &mut memory, true), "nobody answers asleep");
    }

    #[test]
    fn small_talk_is_never_answered_asleep_in_a_chat_or_from_too_far_and_ends_with_the_other() {
        let world_ = talking_world(10.0, Some(false));
        for (asleep, mut memory) in [(true, Memory::new()), (false, chatting(Phase::Talking, 10.0))] {
            small_talk(&world_.ctx(), &me(), &mut memory, asleep);
            assert!(memory.smalltalk().is_none());
        }
        let mut far = talking_world(10.0, None);
        far.smalltalks.set(other(), Some(Smalltalker { who: other(), with: me().uid(), answered: false, cell: Point::new(6, 6) }));
        let mut memory = Memory::new();
        small_talk(&far.ctx(), &me(), &mut memory, false);
        assert!(memory.smalltalk().is_none(), "three cells off");

        // Answered, then the other stops: over this side too.
        let mut memory = Memory::new();
        memory.set_smalltalk(Smalltalk { with: other(), since: (10.0 * MINUTE) as f64, answered: true });
        small_talk(&talking_world(11.0, None).ctx(), &me(), &mut memory, false);
        assert!(memory.smalltalk().is_none());
        // Walked a step past the reach of each other: over; a step inside
        // it, still on.
        let walked = |cell| {
            let mut world = talking_world(11.0, None);
            world.smalltalks.set(other(), Some(Smalltalker { who: other(), with: me().uid(), answered: true, cell }));
            world
        };
        memory.set_smalltalk(Smalltalk { with: other(), since: (10.0 * MINUTE) as f64, answered: true });
        small_talk(&walked(Point::new(6, 3)).ctx(), &me(), &mut memory, false);
        assert!(memory.smalltalk().is_some(), "three cells off");
        small_talk(&walked(Point::new(7, 3)).ctx(), &me(), &mut memory, false);
        assert!(memory.smalltalk().is_none(), "four");
        // Falling asleep ends one, too.
        memory.set_smalltalk(Smalltalk { with: other(), since: (10.0 * MINUTE) as f64, answered: true });
        small_talk(&talking_world(11.0, Some(true)).ctx(), &me(), &mut memory, true);
        assert!(memory.smalltalk().is_none());
        // One nobody answers is given up on in the minutes to answer.
        memory.set_smalltalk(Smalltalk { with: other(), since: (10.0 * MINUTE) as f64, answered: false });
        small_talk(&talking_world(11.0, None).ctx(), &me(), &mut memory, false);
        assert!(memory.smalltalk().is_some(), "a minute");
        small_talk(&talking_world(12.0, None).ctx(), &me(), &mut memory, false);
        assert!(memory.smalltalk().is_none(), "two");
    }
}
