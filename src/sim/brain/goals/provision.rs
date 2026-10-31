//! [`ProvisionGoal`]: fill the pocket for a trip out — a meal and a drink,
//! kept there, so that a worker far out in the wasteland who gets hungry or
//! thirsty stops and eats where it stands instead of walking home to a fridge
//! ([`EatGoal`](super::EatGoal) and [`DrinkGoal`](super::DrinkGoal) use the
//! pocket once off the site).
//!
//! Part of **getting ready** ([`ReadyRoutine`](super::super::routines::ReadyRoutine)),
//! for the jobs worked out there ([`JobKind::goes_out_for`](crate::sim::job::JobKind::goes_out_for)):
//! once its needs are seen to and before it sets out, a worker whose pocket is
//! short of [`PROVISIONS`] fetches them. It is what eating is up to the fridge:
//! walk beside the nearest one it may use, open it, **take what is missing out
//! of it — a meal costs what a meal costs, a dollar — and put each away**
//! ([`Task::stow`]), close it. A worker with no fridge to go to is not waited
//! for: the goal is held off and the routine lets it go out without.
//!
//! **What a pocket holds is a limit like any other**: a meal and a drink are a
//! kilo, and three pieces of scrap are nine of the ten a person carries, so
//! one of each is all a scrap collector can keep and still bring a full
//! packful home.

use crate::save::{pb, Invalid};
use crate::sim::feature::FeatureKind;
use crate::sim::inventory::Inventory;
use crate::sim::item::ItemKind;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::super::tasks::{CLOSE_SECONDS, OPEN_SECONDS, STOW_SECONDS};
use super::eat::TAKE_SECONDS;
use super::{choose_nearest, nearest_in_reach, stand_beside, Seek, Stand};

/// What a worker going out keeps in its pocket: how many of each.
pub const PROVISIONS: [(ItemKind, u8); 2] = [(ItemKind::Food, 1), (ItemKind::Water, 1)];

/// Whether `pack` is short of what a worker going out keeps **in its pocket**:
/// a drink still in the hand is not yet kept, and a worker is not ready to go
/// out with the fridge open and the drink in its hand.
pub fn is_short(pack: &Inventory) -> bool {
    PROVISIONS.iter().any(|&(item, wanted)| pack.count(item) < wanted)
}

/// How many of `item` are kept or on their way there: what is in the hand
/// goes in the pocket next ([`ProvisionGoal::plan`]).
fn have(pack: &Inventory, item: ItemKind) -> u8 {
    pack.count(item) + u8::from(pack.hand() == Some(item))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ProvisionGoal {
    /// The fridge it is fetching from, and how the way there has gone.
    seek: Seek,
}

impl ProvisionGoal {
    pub fn new() -> ProvisionGoal {
        ProvisionGoal::default()
    }

    /// Queue the rest **from what is true now**: what is in the hand put
    /// away if it is what is wanted, used up if not; walk beside the fridge
    /// unless already there; open it; take and put away each thing missing,
    /// **shutting the fridge before the last is put away** — the pocket is
    /// stocked when that is done, and the routine lets the worker go the moment
    /// it is, so nothing may be left to do after it. At most eight tasks,
    /// which is all the queue holds, and whatever is left over is planned
    /// again when these are done. `false` when there is nowhere to stand to use
    /// the fridge.
    fn plan(&mut self, ctx: &mut GoalCtx<'_>) -> bool {
        let (Some(pack), Some(fridge)) = (ctx.inventory.copied(), self.seek.target) else {
            return false;
        };
        let missing: u8 = PROVISIONS.iter().map(|&(item, wanted)| wanted.saturating_sub(have(&pack, item))).sum();
        if missing > 0 {
            if let Stand::Nowhere = stand_beside(ctx, fridge) {
                return false;
            }
        }
        if let Some(item) = pack.hand() {
            let kept = PROVISIONS.iter().any(|&(wanted, many)| wanted == item && pack.count(item) < many);
            let _ = ctx.tasks.push_back(if kept { Task::stow(STOW_SECONDS) } else { Task::consume(item, item.consume_seconds()) });
        }
        if missing == 0 {
            return true;
        }
        if let Stand::At(cell) = stand_beside(ctx, fridge) {
            let _ = ctx.tasks.push_back(Task::move_beside(cell));
        }
        let _ = ctx.tasks.push_back(Task::open_fridge(fridge, OPEN_SECONDS));
        let mut left = missing;
        for &(item, wanted) in &PROVISIONS {
            for _ in have(&pack, item)..wanted {
                left -= 1;
                let _ = ctx.tasks.push_back(Task::take(fridge, item, TAKE_SECONDS));
                if left == 0 {
                    let _ = ctx.tasks.push_back(Task::close_fridge(fridge, CLOSE_SECONDS));
                }
                let _ = ctx.tasks.push_back(Task::stow(STOW_SECONDS));
            }
        }
        true
    }

    /// The pocket is stocked: say so, and start the next time from scratch.
    fn packed(&mut self, ctx: &GoalCtx<'_>, pack: &Inventory) {
        let uid = ctx.body.uid();
        ctx.think.log.unit(uid, format!("{uid} packed provisions: {}", pack.describe_stowed()));
        self.seek.forget();
    }
}

impl GoalExecutor for ProvisionGoal {
    fn goal(&self) -> GoalId {
        GoalId::Provision
    }

    fn save(&self) -> pb::Executor {
        let ProvisionGoal { seek } = self;
        pb::Executor::Seek(seek.save())
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        self.seek = Seek::restore_only(saved, self.goal())?;
        Ok(())
    }

    /// The last thing put away usually stocks the pocket, and the routine
    /// lets go before this goal has been told, so it is told here.
    fn deprioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        if let Some(pack) = ctx.inventory.copied()
            && ctx.tasks.result() == TaskResult::Success
            && matches!(ctx.finished, Some(Task::Stow(_)))
            && !is_short(&pack)
        {
            self.packed(ctx, &pack);
        }
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        let Some(pack) = ctx.inventory.copied() else {
            return GoalProgress::Blocked;
        };
        // A body in hand is seen to the crematorium first, as for a meal.
        if pack.hand() == Some(ItemKind::Body) {
            return GoalProgress::Blocked;
        }

        match (last, ctx.finished) {
            (TaskResult::Success, Some(Task::Stow(_))) if !is_short(&pack) && ctx.tasks.is_empty() => {
                self.packed(ctx, &pack);
                return GoalProgress::Achieved;
            }
            (TaskResult::Failed, Some(Task::MoveTo(_))) => {
                ctx.tasks.clear();
                if self.seek.setback(ctx, |ctx| nearest_in_reach(ctx, FeatureKind::Food)) && self.plan(ctx) {
                    return GoalProgress::Working;
                }
                self.seek.forget();
                return GoalProgress::Blocked;
            }
            (TaskResult::Failed, _) => {
                // The fridge is off, or the pocket has no room: not worth
                // asking again for a while.
                ctx.tasks.clear();
                self.seek.forget();
                return GoalProgress::Blocked;
            }
            (TaskResult::Success, Some(Task::MoveTo(_))) => self.seek.steady(),
            _ => {}
        }
        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }
        if !is_short(&pack) {
            self.seek.forget();
            return GoalProgress::Achieved;
        }
        let Some(fridge) = choose_nearest(ctx, FeatureKind::Food) else {
            return GoalProgress::Blocked;
        };
        self.seek.choose(fridge);
        if self.plan(ctx) {
            GoalProgress::Working
        } else {
            self.seek.forget();
            GoalProgress::Blocked
        }
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![(
            "provisioning from",
            self.seek.target.map_or_else(|| "nowhere".to_string(), |cell| format!("{}, {}", cell.x, cell.y)),
        )]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{Map, Size, FLOOR};
    use crate::sim::biology::ProcessId;
    use crate::sim::clock::{Clock, DAY, HOUR, TIME_SCALE};
    use crate::sim::economy::Money;
    use crate::sim::job::Job;
    use crate::sim::testing::{needy_human, prop_at, World};
    use crate::sim::{GameEntity, Human};

    const BIN: crate::map::Point = crate::map::Point::new(4, 3);
    const FRIDGE: crate::map::Point = crate::map::Point::new(7, 1);

    /// `hours` o'clock on a Monday.
    fn monday(hours: f32) -> Clock {
        let world = (hours as f64 - 8.0) * HOUR as f64;
        Clock::after_watching(world / TIME_SCALE as f64 + 0.0 * DAY as f64)
    }

    /// A collector's yard with a fridge on it, `with_fridge` or not.
    fn yard(with_fridge: bool) -> World {
        let mut map = Map::new(Size::new(9, 7), FLOOR).with_wasteland(0);
        prop_at(&mut map, "scrap bin", BIN);
        if with_fridge {
            prop_at(&mut map, "fridge", FRIDGE);
        }
        map.load_chunks(map.navigation_area());
        let mut world = World::new(map);
        world.clock = monday(10.0);
        world
    }

    /// A collector beside its bin, every need met and none of them running:
    /// only what it keeps in its pocket is left for it to see to. It has ten
    /// dollars.
    fn collector(world: &mut World) -> Human {
        let mut human = needy_human(BIN.offset(-1, 0), 0.0, 0.0, 0.0);
        human.set_stamina(100.0);
        let biology = human.biology_mut().unwrap();
        for process in [ProcessId::Hunger, ProcessId::Thirst, ProcessId::Bladder, ProcessId::Fun] {
            biology.set_running(process, false);
        }
        assert!(human.hire(Job::collector(BIN)));
        world.economy.open(human.uid(), Money::dollars(10));
        human
    }

    #[test]
    fn a_collector_packs_a_meal_and_a_drink_before_it_sets_out_and_pays_for_the_meal() {
        let mut world = yard(true);
        let mut human = collector(&mut world);
        for _ in 0..4000 {
            world.step_in_time(&mut human);
            if world.log_contains("is ready to go out") {
                break;
            }
        }
        assert!(world.log_contains("packed provisions: 1 food, 1 water"), "{:?}", world.lines());
        // Ready the moment the last thing is put away — and not before the
        // fridge is shut.
        let at = |needle: &str| world.lines().iter().position(|line| line.contains(needle)).unwrap();
        assert!(at("closed the fridge") < at("is ready to go out"), "ready with the fridge open: {:?}", world.lines());
        assert_eq!(human.inventory().count(ItemKind::Food), 1);
        assert_eq!(human.inventory().count(ItemKind::Water), 1);
        assert_eq!(human.carried(), None, "stowed, not held");
        let meal = world.economy.tariffs().price(ItemKind::Food);
        assert_eq!(world.economy.account(human.uid()).unwrap().cash, Money::dollars(10) - meal, "a meal is paid for, a drink free");
        assert!(world.log_contains(&format!("paid {meal} for food")), "{:?}", world.lines());
    }

    #[test]
    fn a_collector_with_its_pocket_full_goes_straight_out() {
        let mut world = yard(true);
        let mut human = collector(&mut world);
        for (item, many) in PROVISIONS {
            for _ in 0..many {
                assert!(human.inventory_mut().stow(item));
            }
        }
        for _ in 0..600 {
            world.step_in_time(&mut human);
        }
        assert!(world.log_contains("is ready to go out"));
        assert!(!world.log_contains("packed provisions"), "{:?}", world.lines());
        assert!(!world.log_contains("opened the fridge"), "{:?}", world.lines());
    }

    /// No fridge in the district: held off, and out it goes with what it has.
    #[test]
    fn a_collector_with_no_fridge_to_stock_from_is_not_kept_waiting() {
        let mut world = yard(false);
        let mut human = collector(&mut world);
        for _ in 0..600 {
            world.step_in_time(&mut human);
            if world.log_contains("is ready to go out") {
                break;
            }
        }
        assert!(world.log_contains("is ready to go out"), "{:?}", human.brain_fields());
        assert_eq!(human.inventory().stowed_items(), 0);
    }

    #[test]
    fn where_it_is_going_for_provisions_is_saved_and_a_broken_save_refused() {
        let goal = ProvisionGoal { seek: Seek::at(FRIDGE) };
        let mut back = ProvisionGoal::new();
        back.restore(&goal.save()).unwrap();
        assert_eq!(back, goal);
        assert!(back.restore(&pb::Executor::Wander(pb::Wander { target: None, retries: 0 })).is_err());
    }

    #[test]
    fn a_pocket_is_short_until_it_has_a_meal_and_a_drink_in_it_and_not_in_the_hand() {
        let mut pack = Inventory::human();
        assert!(is_short(&pack));
        assert!(pack.stow(ItemKind::Food));
        assert!(is_short(&pack));
        let _ = pack.set_hand(Some(ItemKind::Water));
        assert!(is_short(&pack), "in the hand it is not yet kept");
        assert_eq!(have(&pack, ItemKind::Water), 1, "but it is on its way there");
        let _ = pack.set_hand(None);
        assert!(pack.stow(ItemKind::Water));
        assert!(!is_short(&pack));
    }
}
