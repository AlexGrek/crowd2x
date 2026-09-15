//! The entities that exist: humans and dogs.
//!
//! A kind is a body, a mind and whatever else only that kind has. The body is
//! a [`Walker`] — the movement action, shared by everything that walks — and
//! the mind is a [`Brain`], built from the routines and goals that kind has:
//! a human keeps fed and stays busy, a dog only stays busy. Neither kind
//! decides anything in here. What a kind does with its three hooks is hand
//! them on:
//!
//! * `think` — walk the current route. Arithmetic, and parallel.
//! * `apply` — take the step the world granted.
//! * `react` — time passes for the body, then the brain runs its pipeline.
//!
//! # Nothing here knows what a human looks like
//!
//! A [`Human`] has no hairstyle, no outfit and no sprite. Which PNGs it is
//! drawn from is a fact about the art, and the art lives in `src/characters/`;
//! all the simulation owes the renderer is a number that is the same every
//! time for this entity and different for the next one, which
//! [`GameEntity::appearance_seed`] already gives it.
//!
//! Same line for [`Dog`]: its [`Facing`] is this module's own enum rather than
//! `characters::dog::Facing`, because that one is a Bevy `Component` and
//! nothing here imports Bevy.

use rand::rngs::SmallRng;
use rand::RngExt;

use crate::map::Point;

use super::brain::{Brain, GoalId};
use super::entity::{Body, GameEntity, Think};
use super::identity::Identity;
use super::item::ItemKind;
use super::stats::Stats;
use super::uid::{EntityType, Uid};
use super::walker::Walker;
use super::{Intent, MoveOutcome};

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
/// No appearance field: [`GameEntity::appearance_seed`] already gives the
/// renderer a stable per-entity number, and storing a second one would be two
/// sources of truth for the same hairstyle.
pub struct Human {
    walk: Walker,
    brain: Brain,
    /// Needs and condition. Rolled at spawn, then changed by time passing and
    /// by what the brain gets done — hunger rises, a meal brings it down.
    stats: Stats,
    /// Name and gender, rolled once at spawn — see [`Identity`] for why the
    /// two need not agree.
    identity: Identity,
    /// What is in its hand. One hand, one thing — see [`ItemKind`].
    carried: Option<ItemKind>,
}

impl Human {
    pub fn new(uid: Uid, cell: Point, rng: &mut SmallRng) -> Human {
        Human {
            walk: Walker::new(uid, cell, HUMAN_SPEED),
            brain: Brain::human(),
            stats: Stats::random(rng),
            identity: Identity::human(rng),
            carried: None,
        }
    }

    /// What this person's body and mind are doing right now.
    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// Who this person is: name and gender.
    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    pub fn brain(&self) -> &Brain {
        &self.brain
    }

    pub fn carried(&self) -> Option<ItemKind> {
        self.carried
    }

    #[cfg(test)]
    pub(crate) fn set_hunger(&mut self, hunger: f32) {
        self.stats = self.stats.with_hunger(hunger);
    }

    #[cfg(test)]
    pub(crate) fn set_carried(&mut self, item: Option<ItemKind>) {
        self.carried = item;
    }
}

impl GameEntity for Human {
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

    /// Time passes for the body first, so a routine reads this tick's hunger
    /// and not last tick's.
    fn react(&mut self, ctx: &Think<'_>, outcome: MoveOutcome) {
        self.stats.metabolise(ctx.dt);
        self.brain.react(
            ctx,
            outcome,
            &mut self.walk,
            Some(&mut self.stats),
            Some(&mut self.carried),
        );
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
        fields.push((
            "holding",
            self.carried.map_or("nothing", ItemKind::name).to_string(),
        ));
        fields.extend(
            self.stats
                .fields()
                .into_iter()
                .map(|(name, value)| (name, format!("{value:.1}"))),
        );
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

    fn display_name(&self) -> Option<String> {
        Some(self.identity.name().to_string())
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

    fn facing(&self) -> Option<Facing> {
        Some(self.facing)
    }

    /// No needs and no hands: the brain gets the walker and nothing else.
    fn react(&mut self, ctx: &Think<'_>, outcome: MoveOutcome) {
        self.brain.react(ctx, outcome, &mut self.walk, None, None);
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

    fn display_name(&self) -> Option<String> {
        Some(self.identity.name().to_string())
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

    /// A human with no appetite to speak of, so what these tests watch is
    /// wandering and not a walk to a fridge that is not there.
    fn human(cell: Point) -> Human {
        let mut rng = SmallRng::seed_from_u64(0);
        let mut human = Human::new(Uid::new(EntityType::Human, 42), cell, &mut rng);
        human.set_hunger(0.0);
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
    fn a_wander_target_that_turns_out_walled_off_holds_wandering_off() {
        // Two cells the wander radius can see, but only one is reachable:
        // `(2, 0)` sits behind a wall with no way round, so picking it is the
        // *expensive* miss — a full flood of the reachable region before the
        // route can say no — and that is the one that holds the goal off.
        let mut map = Map::new(Size::new(3, 1), FLOOR);
        map.set_terrain(Point::new(1, 0), WALL);
        let mut world = World::new(map);

        let mut walker = human(Point::new(0, 0));
        let held = (0..2000).any(|_| {
            world.step(&mut walker);
            walker.brain.goals().cooldown(GoalId::Wander) > 0
        });
        assert!(held, "should eventually roll the unreachable candidate and back off");
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
            one.set_hunger(0.0);
            two.set_hunger(0.0);
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
    fn a_dog_wanders_and_never_thinks_about_food() {
        let mut world = World::new(room());
        let mut rng = SmallRng::seed_from_u64(0);
        let mut dog = Dog::new(Uid::new(EntityType::Dog, 7), Point::new(4, 4), Facing::Left, &mut rng);
        let start = dog.position();
        for _ in 0..300 {
            world.step(&mut dog);
            assert_ne!(dog.brain.top_goal(), GoalId::Eat);
        }
        assert_ne!(dog.position(), start);
    }

    #[test]
    fn a_human_gets_hungrier_as_time_passes() {
        let mut world = World::new(room());
        let mut walker = human(Point::new(4, 4));
        for _ in 0..64 {
            world.step(&mut walker);
        }
        assert!(walker.stats().hunger() > 0.0);
    }
}
