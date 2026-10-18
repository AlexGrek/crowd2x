//! The entities that exist: humans and dogs.
//!
//! A kind is a body, a mind and whatever else only that kind has. The body is
//! a [`Walker`] — the movement action, shared by everything that walks — and
//! the mind is a [`Brain`], built from the routines and goals that kind has:
//! a human keeps fed and hydrated and stays busy, a dog only stays busy. Neither kind
//! decides anything in here. What a kind does with its three hooks is hand
//! them on:
//!
//! * `think` — walk the current route. Arithmetic, and parallel.
//! * `apply` — take the step the world granted.
//! * `react` — time passes for the body ([`Biology::advance`]), then the brain
//!   runs its pipeline.
//!
//! # Nothing here knows what a human looks like
//!
//! A [`Human`] has no hairstyle or sprite. Its mutable outfit is a
//! [`Clothing`] value, independent of its identity. Which PNGs it is
//! drawn from is a fact about the art, and the art lives in `src/characters/`;
//! personal appearance comes from a number that stays the same for this
//! entity, [`GameEntity::appearance_seed`], while the worn outfit can change.
//!
//! Same line for [`Dog`]: its [`Facing`] is this module's own enum rather than
//! `characters::dog::Facing`, because that one is a Bevy `Component` and
//! nothing here imports Bevy.

use rand::rngs::SmallRng;
use rand::RngExt;

use crate::map::Point;
use crate::save::{invalid, pb, Invalid};

use super::background::{Due, Schedule};
use super::biology::{health, Biology, Stats};
use super::brain::{Brain, GoalId};
use super::clothing::Clothing;
use super::entity::{Body, GameEntity, Think};
use super::identity::Identity;
use super::inventory::Inventory;
use super::job::Job;
use super::item::ItemKind;
use super::property::PropertyId;
use super::talents::Talents;
use super::uid::{EntityType, Uid};
use super::walker::Walker;
use super::{Effect, Intent, MoveOutcome};

const HUMAN_SPEED: f32 = 2.0;
const DOG_SPEED: f32 = 3.5;

/// Which way something is facing. The simulation's own, deliberately not
/// `characters::dog::Facing` — see the module docs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Facing {
    Left,
    Right,
}

/// A person.
///
/// Hair and eyes come from the stable appearance seed. Clothing is mutable
/// state: changing an outfit never rerolls the person wearing it.
pub struct Human {
    clothing: Clothing,
    walk: Walker,
    brain: Brain,
    /// Needs and condition, and the processes that change them. Stats are
    /// rolled at spawn, then moved by time passing and by what the brain gets
    /// done — hunger and thirst rise, a meal and a drink bring them down, a
    /// drink fills the bladder and a toilet empties it.
    biology: Biology,
    /// Name and gender, rolled once at spawn — see [`Identity`] for why the
    /// two need not agree.
    identity: Identity,
    /// What it is carrying: one hand, and what it has stowed away. Inline and
    /// `Copy`, like the biology — see [`Inventory`] for which of its two
    /// limits counts the hand.
    inventory: Inventory,
    /// Lasting aptitudes, rolled once and changed only explicitly.
    talents: Talents,
    /// Its own background timers, from its own seed: when this person's
    /// upkeep that need not run every tick — a memory fading — is next due.
    /// See [`crate::sim::background`].
    schedule: Schedule,
}

impl Human {
    pub fn new(uid: Uid, cell: Point, rng: &mut SmallRng) -> Human {
        Human {
            clothing: Clothing::initial(uid.body()),
            walk: Walker::new(uid, cell, HUMAN_SPEED),
            brain: Brain::human(),
            biology: Biology::new(Stats::random(rng)),
            identity: Identity::human(rng),
            inventory: Inventory::human(),
            talents: Talents::random(rng),
            // Rolled last, so everything rolled before it is what it was
            // before there were schedules.
            schedule: Schedule::new(rng.random()),
        }
    }

    /// What this person's body and mind are doing right now.
    pub fn stats(&self) -> Stats {
        *self.biology.stats()
    }

    /// Who this person is: name and gender.
    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    pub fn talents(&self) -> &Talents {
        &self.talents
    }

    pub fn brain(&self) -> &Brain {
        &self.brain
    }

    /// What is in its hand, which is the one slot the brain uses.
    pub fn carried(&self) -> Option<ItemKind> {
        self.inventory.hand()
    }

    /// Everything it is carrying, hand and stowed together.
    pub fn inventory(&self) -> &Inventory {
        &self.inventory
    }

    #[cfg(test)]
    pub(crate) fn brain_mut(&mut self) -> &mut Brain {
        &mut self.brain
    }

    #[cfg(test)]
    pub(crate) fn set_hunger(&mut self, hunger: f32) {
        self.biology.edit(|stats| stats.with_hunger(hunger));
    }

    #[cfg(test)]
    pub(crate) fn set_thirst(&mut self, thirst: f32) {
        self.biology.edit(|stats| stats.with_thirst(thirst));
    }

    #[cfg(test)]
    pub(crate) fn set_bladder(&mut self, bladder: f32) {
        self.biology.edit(|stats| stats.with_bladder(bladder));
    }

    #[cfg(test)]
    pub(crate) fn set_fun(&mut self, fun: f32) {
        self.biology.edit(|stats| stats.with_fun(fun));
    }

    #[cfg(test)]
    pub(crate) fn set_stamina(&mut self, stamina: f32) {
        self.biology.edit(|stats| stats.with_stamina(stamina));
    }

    #[cfg(test)]
    pub(crate) fn set_carried(&mut self, item: Option<ItemKind>) {
        let _ = self.inventory.set_hand(item);
    }

    #[cfg(test)]
    pub(crate) fn inventory_mut(&mut self) -> &mut Inventory {
        &mut self.inventory
    }
}

impl GameEntity for Human {
    fn clothing(&self) -> Option<Clothing> {
        Some(self.clothing)
    }

    fn clothing_mut(&mut self) -> Option<&mut Clothing> {
        Some(&mut self.clothing)
    }
    fn body(&self) -> &Body {
        self.walk.body()
    }

    fn body_mut(&mut self) -> &mut Body {
        self.walk.body_mut()
    }

    fn think(&self, ctx: &Think<'_>) -> Intent {
        self.walk.think(ctx)
    }

    fn apply(&mut self, intent: &Intent) {
        self.walk.apply(intent);
    }

    fn walker(&self) -> Option<&Walker> {
        Some(&self.walk)
    }

    fn walker_mut(&mut self) -> Option<&mut Walker> {
        Some(&mut self.walk)
    }

    /// Time passes for the body first, so a routine reads this tick's needs
    /// and not last tick's.
    ///
    /// The world's clock, not the player's: a body gets hungry over hours, and
    /// an hour is thirty seconds of watching ([`crate::sim::clock`]).
    ///
    /// Background upkeep ([`crate::sim::background`]) comes with it, on the
    /// ticks it is due — and so does the brain looking round, which is handed
    /// the same schedule.
    ///
    /// Health run out ([`health`](crate::sim::biology::health)) is death: the
    /// walk stops, the body is marked dead, the brain is not asked, and the
    /// world is told ([`Effect::Died`]) so it can take back the cell, the bed
    /// and the job.
    fn react(&mut self, ctx: &Think<'_>, outcome: MoveOutcome) -> Effect {
        // Asleep as of last tick's brain: a body lying in bed ages at the
        // pace of a sleeper (`Process::asleep_pace`).
        if self.brain.is_asleep() {
            self.biology.advance_asleep(ctx.game_dt());
        } else {
            self.biology.advance(ctx.game_dt());
        }
        if self.biology.stats().health() <= 0.0 {
            self.walk.halt();
            self.walk.body_mut().set_dead();
            return Effect::Died { cause: health::cause_of_death(self.biology.stats()) };
        }
        let due = self.schedule.tick(ctx.game_dt());
        self.biology.background(&due);
        self.brain.react(
            ctx,
            &due,
            outcome,
            &mut self.walk,
            Some(&mut self.biology),
            Some(&mut self.inventory),
        )
    }

    fn biology(&self) -> Option<&Biology> {
        Some(&self.biology)
    }

    fn biology_mut(&mut self) -> Option<&mut Biology> {
        Some(&mut self.biology)
    }

    fn inventory(&self) -> Option<&Inventory> {
        Some(&self.inventory)
    }

    fn inventory_mut(&mut self) -> Option<&mut Inventory> {
        Some(&mut self.inventory)
    }

    fn talents(&self) -> Option<&Talents> {
        Some(&self.talents)
    }

    fn talents_mut(&mut self) -> Option<&mut Talents> {
        Some(&mut self.talents)
    }

    /// None for the dead: nobody is after anything any more.
    fn current_goal(&self) -> Option<GoalId> {
        (!self.is_dead()).then(|| self.brain.top_goal())
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        let mut fields = vec![
            ("name", self.identity.name().to_string()),
            ("gender", self.identity.gender().label().to_string()),
            ("condition", if self.is_dead() { "dead" } else { "alive" }.to_string()),
            ("clothing", self.clothing.name().to_string()),
            ("goal", self.brain.top_goal().name().to_string()),
            (
                "home",
                self.walk.body().home().map_or_else(|| "nowhere".to_string(), |home| home.to_string()),
            ),
        ];
        fields.extend(self.walk.debug_fields());
        fields.extend(self.inventory.debug_fields());
        fields.extend(self.biology.debug_fields());
        fields.push(self.schedule.debug_field());
        fields
    }

    fn brain_fields(&self) -> Vec<(&'static str, String)> {
        self.brain.debug_fields()
    }

    fn planned_path(&self) -> Vec<Point> {
        self.walk.path_cells()
    }

    /// None for the dead, whatever it was in the middle of.
    fn action_progress(&self) -> Option<f32> {
        if self.is_dead() {
            return None;
        }
        self.brain.action().progress()
    }

    fn interacting_with(&self) -> Option<Point> {
        if self.is_dead() {
            return None;
        }
        self.brain.action().interacting_with()
    }

    fn set_home(&mut self, bed: Point, property: Option<PropertyId>) -> bool {
        self.brain.set_home(bed);
        self.walk.body_mut().set_home(property);
        true
    }

    fn order_salvage(&mut self, heap: Point) -> bool {
        self.brain.order_salvage(heap);
        true
    }

    fn hire(&mut self, job: Job) -> bool {
        self.brain.hire(job);
        true
    }

    fn dismiss(&mut self) -> Option<Job> {
        self.brain.dismiss()
    }

    fn job(&self) -> Option<Job> {
        self.brain.job().copied()
    }

    fn display_name(&self) -> Option<String> {
        Some(self.identity.name().to_string())
    }

    fn save(&self) -> pb::Kind {
        let Human { walk, brain, biology, identity, inventory, talents, schedule, clothing } = self;
        pb::Kind::Human(pb::Human {
            walker: Some(walk.save()),
            brain: Some(brain.save()),
            biology: Some(biology.save()),
            identity: Some(identity.save()),
            inventory: Some(inventory.save()),
            talents: talents.save(),
            schedule: Some(schedule.save()),
            clothing: Some(clothing.name().to_string()),
        })
    }
}

impl Human {
    /// A person as saved, standing beside a map whose navigation is at
    /// `revision` ([`Walker::restore`]).
    fn restore(saved: &pb::Human, revision: u64) -> Result<Human, Invalid> {
        let mut brain = Brain::human();
        brain.restore(pb::required(&saved.brain, "brain")?).map_err(|error| error.within("brain"))?;
        Ok(Human {
            clothing: match saved.clothing.as_deref() {
                Some(name) => Clothing::from_name(name)
                    .ok_or_else(|| Invalid::new(format!("unknown clothing {name:?}")))?,
                None => {
                    let walker = pb::required(&saved.walker, "walker")?;
                    let body = pb::required(&walker.body, "body")?;
                    let uid = Uid::from_raw(body.uid).ok_or_else(|| Invalid::new("zero uid"))?;
                    Clothing::initial(uid.body())
                }
            },
            walk: Walker::restore(pb::required(&saved.walker, "walker")?, revision)?,
            brain,
            biology: Biology::restore(pb::required(&saved.biology, "biology")?)?,
            identity: Identity::restore(pb::required(&saved.identity, "identity")?)?,
            inventory: Inventory::restore(pb::required(&saved.inventory, "inventory")?)?,
            talents: Talents::restore(&saved.talents)?,
            schedule: Schedule::restore(pb::required(&saved.schedule, "schedule")?)?,
        })
    }
}

/// A dog.
pub struct Dog {
    walk: Walker,
    brain: Brain,
    facing: Facing,
    /// Name and gender, rolled once at spawn — [`Identity::pet`], not
    /// [`Identity::human`]: a dog gets a first name and no surname.
    identity: Identity,
}

impl Dog {
    pub fn new(uid: Uid, cell: Point, facing: Facing, rng: &mut SmallRng) -> Dog {
        Dog {
            walk: Walker::new(uid, cell, DOG_SPEED),
            brain: Brain::dog(),
            facing,
            identity: Identity::pet(rng),
        }
    }

    /// Who this dog is: name and gender.
    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    pub fn brain(&self) -> &Brain {
        &self.brain
    }
}

impl GameEntity for Dog {
    fn body(&self) -> &Body {
        self.walk.body()
    }

    fn body_mut(&mut self) -> &mut Body {
        self.walk.body_mut()
    }

    fn think(&self, ctx: &Think<'_>) -> Intent {
        self.walk.think(ctx)
    }

    fn walker(&self) -> Option<&Walker> {
        Some(&self.walk)
    }

    fn walker_mut(&mut self) -> Option<&mut Walker> {
        Some(&mut self.walk)
    }

    fn facing(&self) -> Option<Facing> {
        Some(self.facing)
    }

    /// No needs and no hands: the brain gets the walker and nothing else.
    /// No schedule either, so it never looks round — a dog has nothing yet
    /// that seeing somebody would change.
    fn react(&mut self, ctx: &Think<'_>, outcome: MoveOutcome) -> Effect {
        self.brain.react(ctx, &Due::NOTHING, outcome, &mut self.walk, None, None)
    }

    /// Turns to face the way it is walking.
    ///
    /// Derived here rather than sent in the intent because it is a consequence
    /// of moving, not a decision: an intent describes what an entity wants,
    /// and no dog wants to face left.
    fn apply(&mut self, intent: &Intent) {
        let was = self.walk.body().position().0;
        self.walk.apply(intent);
        let now = self.walk.body().position().0;

        // Left alone when it did not move horizontally, so a dog walking
        // straight up does not flip to an arbitrary side.
        if now < was {
            self.facing = Facing::Left;
        } else if now > was {
            self.facing = Facing::Right;
        }
    }

    fn current_goal(&self) -> Option<GoalId> {
        Some(self.brain.top_goal())
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        let mut fields = vec![
            ("name", self.identity.name().to_string()),
            ("gender", self.identity.gender().label().to_string()),
            ("goal", self.brain.top_goal().name().to_string()),
        ];
        fields.extend(self.walk.debug_fields());
        fields
    }

    fn brain_fields(&self) -> Vec<(&'static str, String)> {
        self.brain.debug_fields()
    }

    fn planned_path(&self) -> Vec<Point> {
        self.walk.path_cells()
    }

    fn action_progress(&self) -> Option<f32> {
        self.brain.action().progress()
    }

    fn interacting_with(&self) -> Option<Point> {
        self.brain.action().interacting_with()
    }

    fn display_name(&self) -> Option<String> {
        Some(self.identity.name().to_string())
    }

    fn save(&self) -> pb::Kind {
        let Dog { walk, brain, facing, identity } = self;
        pb::Kind::Dog(pb::Dog {
            walker: Some(walk.save()),
            brain: Some(brain.save()),
            facing: match facing {
                Facing::Left => pb::Facing::Left,
                Facing::Right => pb::Facing::Right,
            } as i32,
            identity: Some(identity.save()),
        })
    }
}

impl Dog {
    fn restore(saved: &pb::Dog, revision: u64) -> Result<Dog, Invalid> {
        let mut brain = Brain::dog();
        brain.restore(pb::required(&saved.brain, "brain")?).map_err(|error| error.within("brain"))?;
        Ok(Dog {
            walk: Walker::restore(pb::required(&saved.walker, "walker")?, revision)?,
            brain,
            facing: match pb::known::<pb::Facing>(saved.facing, "facing")? {
                pb::Facing::Left => Facing::Left,
                pb::Facing::Right => Facing::Right,
            },
            identity: Identity::restore(pb::required(&saved.identity, "identity")?)?,
        })
    }
}

/// Build an entity of a kind, at a cell.
///
/// The one place a `Uid`'s type tag and the concrete type behind it are tied
/// together, so they cannot drift apart.
pub(super) fn build(
    uid: Uid,
    kind: EntityType,
    cell: Point,
    rng: &mut SmallRng,
) -> Box<dyn GameEntity> {
    match kind {
        EntityType::Human => Box::new(Human::new(uid, cell, rng)),
        EntityType::Dog => Box::new(Dog::new(
            uid,
            cell,
            if rng.random() { Facing::Left } else { Facing::Right },
            rng,
        )),
    }
}

/// An entity as a save has it — [`build`]'s twin, and like it the one place
/// a saved kind and the concrete type behind it are tied together. The kind
/// saved must be the kind its id says, or the entity would report a type its
/// own id disagrees with. `revision` is the restored map's navigation
/// revision ([`Walker::restore`]).
pub(super) fn restore(saved: &pb::Kind, revision: u64) -> Result<Box<dyn GameEntity>, Invalid> {
    let entity: Box<dyn GameEntity> = match saved {
        pb::Kind::Human(human) => Box::new(Human::restore(human, revision)?),
        pb::Kind::Dog(dog) => Box::new(Dog::restore(dog, revision)?),
    };
    let saved_as = match saved {
        pb::Kind::Human(_) => EntityType::Human,
        pb::Kind::Dog(_) => EntityType::Dog,
    };
    if entity.kind() != Some(saved_as) {
        let uid = entity.uid();
        return invalid(format!("{uid} saved as a {saved_as}"));
    }
    Ok(entity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Map, Size, FLOOR, WALL};
    use crate::sim::brain::GoalId;
    use crate::sim::testing::World;
    use rand::SeedableRng;

    fn room() -> Map {
        Map::new(Size::new(9, 9), FLOOR)
    }

    /// A worker is in its job's uniform from setting off for the shift until
    /// it ends, and in its own clothes the rest of the time, which is also
    /// what changing clothes changes; let go, it is out of uniform at once.
    #[test]
    fn a_worker_wears_its_jobs_uniform_while_its_shift_is_due_and_its_own_clothes_otherwise() {
        use crate::sim::clock::{Clock, DAY, HOUR, TIME_SCALE};
        use crate::sim::clothing::Clothing;
        use crate::sim::job::{Job, WATCHES};
        let at = |day: u32, hours: f32| {
            let world = (day - 1) as f64 * DAY as f64 + (hours as f64 - 8.0) * HOUR as f64;
            Clock::after_watching(world / TIME_SCALE as f64)
        };
        let mut human = crate::sim::testing::needy_human(Point::new(1, 1), 0.0, 0.0, 0.0);
        *human.clothing_mut().unwrap() = Clothing::SundressRed;
        assert_eq!(human.worn(at(1, 10.0)), Some(Clothing::SundressRed), "no job, no uniform");

        assert!(human.hire(Job::office(Point::new(3, 3))));
        assert_eq!(human.worn(at(1, 8.4)), Some(Clothing::SundressRed), "not yet");
        assert_eq!(human.worn(at(1, 8.6)), Some(Clothing::UniformOfficeWorker), "dressed for the walk in");
        assert_eq!(human.worn(at(1, 16.9)), Some(Clothing::UniformOfficeWorker));
        assert_eq!(human.worn(at(1, 17.1)), Some(Clothing::SundressRed), "home time");
        assert_eq!(human.worn(at(6, 10.0)), Some(Clothing::SundressRed), "Saturday");
        *human.clothing_mut().unwrap() = Clothing::GownBlack;
        assert_eq!(human.worn(at(2, 10.0)), Some(Clothing::UniformOfficeWorker), "changing changes what it goes home in");
        assert_eq!(human.worn(at(2, 18.0)), Some(Clothing::GownBlack));
        assert_eq!(human.clothing(), Some(Clothing::GownBlack), "the uniform is never its own");

        human.dismiss();
        assert!(human.hire(Job::guard(Point::new(0, 3), WATCHES[2])));
        assert_eq!(human.worn(at(2, 21.0)), Some(Clothing::GownBlack));
        assert_eq!(human.worn(at(2, 22.5)), Some(Clothing::UniformGuard), "the night watch dresses before midnight");
        assert_eq!(human.worn(at(3, 3.0)), Some(Clothing::UniformGuard));
        assert_eq!(human.worn(at(3, 8.5)), Some(Clothing::GownBlack));
        human.dismiss();
        assert!(human.hire(Job::collector(Point::new(3, 3))));
        assert_eq!(human.worn(at(3, 11.0)), Some(Clothing::UniformScrapCollector));
        human.dismiss();
        assert_eq!(human.worn(at(3, 11.0)), Some(Clothing::GownBlack), "let go");

        let dog = Dog::new(Uid::new(EntityType::Dog, 1), Point::new(0, 0), Facing::Left, &mut rand::rngs::SmallRng::seed_from_u64(1));
        assert_eq!(dog.worn(at(1, 10.0)), None);
    }

    /// A human with no needs to speak of, so what these tests watch is
    /// wandering and not a walk to a fridge or a toilet that is not there.
    fn human(cell: Point) -> Human {
        let mut rng = SmallRng::seed_from_u64(0);
        let mut human = Human::new(Uid::new(EntityType::Human, 42), cell, &mut rng);
        human.set_hunger(0.0);
        human.set_thirst(0.0);
        human.set_bladder(0.0);
        human.set_fun(100.0);
        human.set_stamina(100.0);
        human
    }

    #[test]
    fn a_fresh_wanderer_has_no_route_and_thinks_idle() {
        let world = World::new(room());
        let walker = human(Point::new(4, 4));
        assert_eq!(walker.think(&world.ctx()), Intent::Idle);
    }

    #[test]
    fn a_wanderer_decides_where_to_go_on_its_first_reaction_and_then_walks_there() {
        let mut world = World::new(room());
        let mut walker = human(Point::new(4, 4));
        let start = walker.position();

        // Missing an in-bounds candidate is cheap and retried every tick
        // (`WanderGoal`'s docs), so a few reactions are enough to be sure of
        // a route.
        let mut routed = false;
        for _ in 0..20 {
            world.step(&mut walker);
            if walker.walk.is_walking() {
                routed = true;
                break;
            }
        }
        assert!(routed, "should have a route by now");
        assert_eq!(walker.brain.top_goal(), GoalId::Wander);

        world.step(&mut walker);
        assert_ne!(walker.position(), start);
        assert!(world.map.is_passable(walker.center_position()));
    }

    #[test]
    fn a_wanderer_boxed_in_by_walls_stands_still_rather_than_escaping() {
        let mut map = Map::new(Size::new(3, 3), WALL);
        // One passable cell, in the middle: nowhere to go.
        map.set_terrain(Point::new(1, 1), FLOOR);
        let mut world = World::new(map);

        let mut walker = human(Point::new(1, 1));
        for _ in 0..200 {
            world.step(&mut walker);
        }

        assert_eq!(walker.center_position(), Point::new(1, 1));
    }

    #[test]
    fn a_wanderer_alone_on_its_cell_keeps_trying_for_free() {
        // Nothing else on the map is passable, so no candidate is ever found —
        // every offset tried is off the map or a wall. That miss is cheap (no
        // search ran), and `WanderGoal` retries it every tick rather than
        // holding wandering off.
        let mut map = Map::new(Size::new(3, 3), WALL);
        map.set_terrain(Point::new(1, 1), FLOOR);
        let mut world = World::new(map);

        let mut walker = human(Point::new(1, 1));
        for t in 0..200 {
            world.step(&mut walker);
            assert_eq!(
                walker.brain.goals().cooldown(GoalId::Wander),
                0,
                "tick {t}: a bare dice-roll miss should not back off"
            );
        }
        assert!(!walker.walk.is_walking());
    }

    #[test]
    fn a_wander_target_walled_off_is_a_cheap_miss_and_never_holds_wandering_off() {
        // Two cells the wander radius can see, but only one is reachable:
        // `(2, 0)` sits behind a wall with no way round. The map's components
        // say so for nothing, so it is never picked — a cheap miss, tried
        // again next tick — rather than routed to, failed, and held off for:
        // the expensive miss is for a cell that only *looks* reachable, a
        // pocket a lake walls in far out in the wasteland.
        let mut map = Map::new(Size::new(3, 1), FLOOR);
        map.set_terrain(Point::new(1, 0), WALL);
        let mut world = World::new(map);

        let mut walker = human(Point::new(0, 0));
        for t in 0..2000 {
            world.step(&mut walker);
            assert_eq!(
                walker.brain.goals().cooldown(GoalId::Wander),
                0,
                "tick {t}: a cell with no way to it should never be routed to"
            );
        }
    }

    #[test]
    fn the_same_entity_on_the_same_tick_plans_the_same_route() {
        let (mut one, mut two) = (World::new(room()), World::new(room()));
        let mut a = human(Point::new(4, 4));
        let mut b = human(Point::new(4, 4));
        one.tick = 5;
        two.tick = 5;

        one.step(&mut a);
        two.step(&mut b);

        assert_eq!(a.walk.path(), b.walk.path());
    }

    #[test]
    fn two_entities_on_the_same_tick_do_not_plan_in_lockstep() {
        // Neighbouring ids sharing a tick must not get correlated streams, or
        // a crowd wanders in formation.
        let differ = (0..20).any(|t| {
            let (mut w1, mut w2) = (World::new(room()), World::new(room()));
            w1.tick = t;
            w2.tick = t;
            // Whole humans, fresh each round: the brain holds the target now,
            // so resetting only the walker would not reset the decision.
            let mut rng = SmallRng::seed_from_u64(0);
            let mut one = Human::new(Uid::new(EntityType::Human, 1), Point::new(4, 4), &mut rng);
            let mut two = Human::new(Uid::new(EntityType::Human, 2), Point::new(4, 4), &mut rng);
            for human in [&mut one, &mut two] {
                human.set_hunger(0.0);
                human.set_thirst(0.0);
                human.set_bladder(0.0);
                human.set_fun(100.0);
                human.set_stamina(100.0);
            }
            w1.step(&mut one);
            w2.step(&mut two);
            one.walk.path() != two.walk.path()
        });
        assert!(differ);
    }

    #[test]
    fn a_dog_turns_to_face_the_way_it_walks() {
        let mut rng = SmallRng::seed_from_u64(0);
        let mut dog = Dog::new(Uid::new(EntityType::Dog, 7), Point::new(4, 4), Facing::Right, &mut rng);
        let here = dog.position();

        dog.apply(&Intent::Move {
            to: (here.0 - 1.0, here.1),
        });
        assert_eq!(dog.facing(), Some(Facing::Left));

        dog.apply(&Intent::Move {
            to: (here.0 + 1.0, here.1),
        });
        assert_eq!(dog.facing(), Some(Facing::Right));
    }

    #[test]
    fn a_dog_walking_straight_up_keeps_the_side_it_was_facing() {
        let mut rng = SmallRng::seed_from_u64(0);
        let mut dog = Dog::new(Uid::new(EntityType::Dog, 7), Point::new(4, 4), Facing::Left, &mut rng);
        let (x, y) = dog.position();
        dog.apply(&Intent::Move { to: (x, y + 1.0) });
        assert_eq!(dog.facing(), Some(Facing::Left));
    }

    #[test]
    fn a_dog_wanders_and_never_thinks_about_its_needs() {
        let mut world = World::new(room());
        let mut rng = SmallRng::seed_from_u64(0);
        let mut dog = Dog::new(Uid::new(EntityType::Dog, 7), Point::new(4, 4), Facing::Left, &mut rng);
        let start = dog.position();
        for _ in 0..300 {
            world.step(&mut dog);
            assert_eq!(dog.brain.top_goal(), GoalId::Wander);
        }
        assert_ne!(dog.position(), start);
        assert!(dog.biology().is_none());
    }

    #[test]
    fn a_human_s_needs_grow_as_time_passes() {
        let mut world = World::new(room());
        let mut walker = human(Point::new(4, 4));
        for _ in 0..64 {
            world.step(&mut walker);
        }
        assert!(walker.stats().hunger() > 0.0);
        assert!(walker.stats().thirst() > 0.0);
        assert!(walker.stats().bladder() > 0.0);
    }

    /// Fading runs in the background, not in `advance`: a human ticked through
    /// its reactions still forgets, a little later than every tick would.
    #[test]
    fn a_human_forgets_a_meal_by_the_next_day_through_its_background_work() {
        use crate::sim::biology::{Event, Experience, ProcessId};
        use crate::sim::clock::HOUR;

        let mut world = World::new(room());
        let mut walker = human(Point::new(4, 4));
        let biology = walker.biology_mut().unwrap();
        // Nothing hungry, so nothing eats again and remembers it afresh.
        biology.set_running(ProcessId::Hunger, false);
        let _ = biology.handle(Event::Ingested(ItemKind::Food));
        let food = Experience::Taste(ItemKind::Food);

        let ticks_per_hour = (HOUR / world.ctx().game_dt()) as u32;
        for _ in 0..4 * ticks_per_hour {
            world.step(&mut walker);
        }
        let familiarity = walker.biology().unwrap().recollection().familiarity(food);
        assert!((0.45..0.55).contains(&familiarity), "about half gone in four hours: {familiarity}");

        for _ in 0..12 * ticks_per_hour {
            world.step(&mut walker);
        }
        assert!(!walker.biology().unwrap().recollection().recalls(food), "forgotten by the next day");
    }
}
