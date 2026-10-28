//! The **courier dispatch**: [`Dispatch`] — the store's calls for food, and
//! which courier each was given to.
//!
//! **Couriers are shared between tasks.** Every job with the catalogue's
//! `courier` column — the steel works' couriers, the food couriers, the
//! store's own — does its own hauling between dispatches. When a shopkeeper
//! finds the superfridge at its call level ([`super::superfridge`]) it calls
//! the dispatch (`Effect::CallDispatch`): an [`Order`] for one unit of food
//! for that superfridge. Every tick an order waits, the dispatch sends **the
//! free courier on duty nearest the warehouse** — nearest the stocked meal
//! rack nearest the store, as the crow flies, the lower uid on a tie — to
//! fetch a meal from the rack and bring it to the superfridge
//! ([`Dispatch::send`]). Its brain sees the order through `Think::dispatch`
//! and `WorkRoutine` raises `GoalId::Deliver` over its own job until the meal
//! is in (`Effect::Deposit` at the superfridge, [`Dispatch::delivered`]).
//!
//! **Free** is on duty (the shift due, not on sick leave), carrying nothing,
//! not already sent, and about its job or nothing — not seeing to a need,
//! asleep or fleeing. A store calls again only once its last call has a
//! courier ([`Dispatch::call`]), so the calls never pile up while nobody is
//! on duty. An order goes back to waiting when its courier dies, is let go,
//! goes on sick leave, has not delivered within [`ORDER_HOURS`], or has
//! nothing to fetch it from.
//!
//! The world's, like the stores: changed only by the world step (the
//! effects, and [`Dispatch::send`] at the end of the processing pass), read by
//! every brain. A walk of the crowd a tick while an order waits — the world's
//! one look, never a brain's.

use crate::map::Point;
use crate::save::{invalid, pb, Invalid};

use super::bins::Bins;
use super::brain::GoalId;
use super::clock::{Clock, HOUR};
use super::entity::GameEntity;
use super::feature::FeatureKind;
use super::item::ItemKind;
use super::log::Log;
use super::uid::Uid;

/// How long a courier has to bring the food in, in world hours, before the
/// order goes back to waiting for somebody else.
pub const ORDER_HOURS: f64 = 3.0;

/// One unit of food for one superfridge.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Order {
    /// The superfridge it is for.
    pub store: Point,
    /// Who was sent, or `None` while it waits for somebody free.
    pub courier: Option<Uid>,
    /// When it was given to the courier, in world seconds since the world
    /// began; 0 while it waits.
    pub since: f64,
}

/// Every order not yet delivered, in the order they were called.
#[derive(Clone, Default, Debug)]
pub struct Dispatch {
    orders: Vec<Order>,
    /// Reused dispatch scratch, never saved or part of world equality.
    free: Vec<(Uid, Point)>,
}

impl PartialEq for Dispatch {
    fn eq(&self, other: &Self) -> bool {
        self.orders == other.orders
    }
}

impl Dispatch {
    pub fn new() -> Dispatch {
        Dispatch::default()
    }

    /// A shopkeeper's call for a unit of food for the superfridge at `store`.
    /// `false`, and no new order, while an earlier call for it still waits
    /// for a courier: the dispatch has it already.
    pub fn call(&mut self, store: Point) -> bool {
        if self.orders.iter().any(|order| order.store == store && order.courier.is_none()) {
            return false;
        }
        self.orders.push(Order { store, courier: None, since: 0.0 });
        true
    }

    /// The order `courier` was sent on, if it was.
    pub fn order_of(&self, courier: Uid) -> Option<Order> {
        self.orders.iter().copied().find(|order| order.courier == Some(courier))
    }

    /// Every order not yet delivered.
    pub fn orders(&self) -> &[Order] {
        &self.orders
    }

    /// How many orders wait for a courier.
    pub fn waiting(&self) -> usize {
        self.orders.iter().filter(|order| order.courier.is_none()).count()
    }

    /// `courier` put food in the superfridge at `store`: its order, if it
    /// was sent there, is done. Whether it was.
    pub fn delivered(&mut self, courier: Uid, store: Point) -> bool {
        match self.orders.iter().position(|order| order.courier == Some(courier) && order.store == store) {
            Some(i) => {
                self.orders.remove(i);
                true
            }
            None => false,
        }
    }

    /// A courier leaving in the spawn pass gives its request back before a
    /// paused world can be saved with an assignment to somebody absent.
    pub(crate) fn release(&mut self, courier: Uid) {
        if let Some(i) = self.orders.iter().position(|order| order.courier == Some(courier)) {
            let store = self.orders[i].store;
            if self.orders.iter().any(|order| order.store == store && order.courier.is_none()) {
                self.orders.remove(i);
            } else {
                self.orders[i] = Order { store, courier: None, since: 0.0 };
            }
        }
    }

    /// **The dispatch's tick**, at the end of the processing pass: every
    /// order whose courier can no longer bring it goes back to waiting, then
    /// every waiting order goes to the nearest free courier on duty, while
    /// the warehouse has a meal to send. `crowd` is everybody; `find` looks
    /// one up.
    pub(crate) fn send<'a>(
        &mut self,
        crowd: impl Iterator<Item = &'a dyn GameEntity>,
        find: impl Fn(Uid) -> Option<&'a dyn GameEntity>,
        bins: &Bins,
        superfridge: &super::superfridge::Superfridge,
        clock: Clock,
        log: &Log,
    ) {
        if self.orders.is_empty() {
            return;
        }
        let now = clock.elapsed();
        for order in &mut self.orders {
            let Some(courier) = order.courier else {
                continue;
            };
            let carries_food = |entity: &dyn GameEntity| entity.inventory().is_some_and(|pack| pack.count(ItemKind::Meal) > 0);
            let why = match find(courier) {
                None => Some("is gone"),
                Some(entity) if entity.is_dead() => Some("died"),
                Some(entity) if entity.is_frozen() => Some("is frozen"),
                Some(entity) if !entity.job().is_some_and(|job| job.kind.is_courier()) => Some("is no courier now"),
                Some(entity) if entity.is_on_sick_leave(clock) => Some("is on sick leave"),
                Some(entity) if !carries_food(entity) && pickup(bins, order.store).is_none() => Some("has no food to fetch"),
                Some(_) if now - order.since > ORDER_HOURS * HOUR as f64 => Some("has not brought it in time"),
                Some(_) => None,
            };
            if let Some(why) = why {
                log.unit(courier, format!("the courier dispatch took back the food for the superfridge at {}, {}: {courier} {why}", order.store.x, order.store.y));
                *order = Order { store: order.store, courier: None, since: 0.0 };
            }
        }
        // Recalled assignments can meet a newer waiting call. Keep one
        // waiting request per store, and discard requests for full stores.
        self.orders.retain(|order| superfridge.room(order.store, bins) > 0);
        let mut i = 0;
        while i < self.orders.len() {
            let order = self.orders[i];
            if order.courier.is_none() && self.orders[..i].iter().any(|earlier| earlier.store == order.store && earlier.courier.is_none()) {
                self.orders.remove(i);
            } else {
                i += 1;
            }
        }
        if self.waiting() == 0 {
            return;
        }
        // With no source there is no reason to scan the crowd.
        if !self.orders.iter().any(|order| order.courier.is_none() && pickup(bins, order.store).is_some()) {
            return;
        }
        self.free.clear();
        self.free.extend(crowd
            .filter(|entity| !self.orders.iter().any(|order| order.courier == Some(entity.uid())) && is_free(*entity, clock))
            .map(|entity| (entity.uid(), entity.center_position())));
        for order in self.orders.iter_mut().filter(|order| order.courier.is_none()) {
            let Some(rack) = pickup(bins, order.store) else {
                continue;
            };
            let Some(i) = (0..self.free.len()).min_by_key(|&i| (self.free[i].1.manhattan_distance(rack), self.free[i].0)) else {
                break;
            };
            let (courier, _) = self.free.swap_remove(i);
            *order = Order { store: order.store, courier: Some(courier), since: now };
            log.unit(courier, format!(
                "the courier dispatch sent {courier} to the meal rack at {}, {} for the superfridge at {}, {}",
                rack.x, rack.y, order.store.x, order.store.y
            ));
        }
    }

    // --- saving ---

    pub(crate) fn save(&self) -> Vec<pb::Order> {
        self.orders
            .iter()
            .map(|&Order { store, courier, since }| pb::Order {
                store: Some(store.into()),
                courier: courier.map_or(0, Uid::raw),
                since,
            })
            .collect()
    }

    /// The orders as saved, every one for a superfridge on the map. Who was
    /// sent is checked against the crowd once it is back
    /// ([`Dispatch::couriers`]).
    pub(crate) fn restore(saved: &[pb::Order], bins: &Bins) -> Result<Dispatch, Invalid> {
        let mut orders = Vec::with_capacity(saved.len());
        for order in saved {
            let store = pb::cell(order.store, "order")?;
            if bins.store(store) != Some(FeatureKind::Superfridge) {
                return invalid(format!("an order for {}, {}, where there is no superfridge", store.x, store.y));
            }
            let courier = match order.courier {
                0 => None,
                raw => Some(Uid::from_raw(raw).ok_or_else(|| Invalid(format!("no such courier {raw:#x}")))?),
            };
            if courier.is_some_and(|uid| uid.kind() != Some(super::uid::EntityType::Human)) {
                return invalid("a courier order assigned to a non-human".to_string());
            }
            if !order.since.is_finite() || order.since < 0.0 {
                return invalid(format!("an order given at {} seconds", order.since));
            }
            if courier.is_some_and(|uid| orders.iter().any(|other: &Order| other.courier == Some(uid))) {
                return invalid("a courier sent on two orders".to_string());
            }
            if courier.is_none() && (order.since != 0.0 || orders.iter().any(|other: &Order| other.store == store && other.courier.is_none())) {
                return invalid("duplicate or dated waiting courier order".to_string());
            }
            orders.push(Order { store, courier, since: order.since });
        }
        Ok(Dispatch { orders, free: Vec::new() })
    }

    /// Everybody sent on an order, for the restore to find in the crowd.
    pub(crate) fn couriers(&self) -> impl Iterator<Item = Uid> + '_ {
        self.orders.iter().filter_map(|order| order.courier)
    }
}

/// Where a courier sent for `store`'s food fetches it: the meal rack with a
/// meal on it nearest the superfridge, the lower cell on a tie. `None` when
/// the warehouse has none.
pub fn pickup(bins: &Bins, store: Point) -> Option<Point> {
    bins.iter_stores()
        .filter(|&(_, kind, stored)| kind == FeatureKind::MealRack && stored > 0)
        .map(|(cell, _, _)| cell)
        .min_by_key(|&cell| (cell.manhattan_distance(store), cell))
}

/// Whether the dispatch may send `entity`: a courier on duty, not on sick
/// leave, carrying nothing, and about its job or nothing.
fn is_free(entity: &dyn GameEntity, clock: Clock) -> bool {
    let Some(job) = entity.job() else {
        return false;
    };
    job.kind.is_courier()
        && !entity.is_dead()
        && !entity.is_frozen()
        && job.is_due(clock)
        && !entity.is_on_sick_leave(clock)
        && entity.inventory().is_some_and(|pack| pack.is_empty())
        && entity.current_goal().is_some_and(|goal| goal == job.kind.goal() || matches!(goal, GoalId::Wander | GoalId::Idle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::testing::store_world;

    fn send(state: &mut crate::sim::GameState) {
        let now = state.clock();
        let entities = &state.entities;
        state.facilities.dispatch.send(entities.iter(), |uid| entities.get(uid), &state.facilities.bins, &state.facilities.superfridge, now, &state.log);
    }

    #[test]
    fn dispatch_chooses_the_nearest_free_on_duty_courier_and_reassigns_a_frozen_one() {
        use crate::sim::job::{Job, JobKind, STORE_COURIER_HOURS};
        let (mut state, people) = store_world(30, 10, 7);
        let courier = people[4];
        let closer = state.spawn_commoner(Point::new(3, 5));
        assert!(state.entities.get_mut(closer).unwrap().hire(Job::new(JobKind::Courier, Point::new(9, 9), STORE_COURIER_HOURS)));
        state.facilities.dispatch.call(Point::new(12, 5));
        send(&mut state);
        assert!(state.dispatch().order_of(closer).is_some());
        assert!(state.dispatch().order_of(courier).is_none());
        state.entities.get_mut(closer).unwrap().body_mut().set_frozen(true);
        send(&mut state);
        assert!(state.dispatch().order_of(closer).is_none());
        assert!(state.dispatch().order_of(courier).is_some());
    }

    #[test]
    fn a_courier_carrying_goods_or_off_duty_is_not_free() {
        let (mut state, people) = store_world(30, 10, 7);
        let courier = people[4];
        assert!(is_free(state.entities.get(courier).unwrap(), state.clock()));
        assert!(state.entities.get_mut(courier).unwrap().inventory_mut().unwrap().stow(ItemKind::Wheat));
        assert!(!is_free(state.entities.get(courier).unwrap(), state.clock()));
        assert!(state.entities.get_mut(courier).unwrap().inventory_mut().unwrap().take_out(ItemKind::Wheat));
        crate::sim::testing::skip_to(&mut state, 1, 19.0);
        assert!(!is_free(state.entities.get(courier).unwrap(), state.clock()), "off duty");
    }

    #[test]
    fn a_timed_out_courier_keeps_its_food_and_the_request_goes_back_to_waiting() {
        let (mut state, people) = store_world(30, 10, 7);
        let store = Point::new(12, 5);
        let uid = people[4];
        state.facilities.dispatch.call(store);
        send(&mut state);
        assert!(state.dispatch().order_of(uid).is_some());
        assert!(state.entities.get_mut(uid).unwrap().inventory_mut().unwrap().stow(ItemKind::Meal));
        crate::sim::testing::skip_to(&mut state, 1, 13.1);
        send(&mut state);
        assert!(state.dispatch().order_of(uid).is_none());
        assert_eq!(state.dispatch().waiting(), 1);
        assert_eq!(state.entities().get(uid).unwrap().inventory().unwrap().count(ItemKind::Meal), 1);
    }

    #[test]
    fn recalled_orders_merge_with_waiting_calls_and_no_source_does_not_scan_the_crowd() {
        let (mut state, people) = store_world(30, 0, 7);
        let store = Point::new(12, 5);
        let courier = people[4];
        state.facilities.dispatch.orders.push(Order { store, courier: Some(courier), since: state.clock().elapsed() });
        assert!(state.facilities.dispatch.call(store));
        send(&mut state);
        assert_eq!(state.dispatch().waiting(), 1);
        assert_eq!(state.dispatch().orders().len(), 1);
        assert_eq!(state.dispatch().free.capacity(), 0, "no crowd buffer needed without food");
    }

    #[test]
    fn dispatch_refuses_duplicate_waiting_calls_and_nonhuman_or_reused_couriers_in_saves() {
        let (state, people) = store_world(30, 10, 7);
        let waiting = pb::Order { store: Some(Point::new(12, 5).into()), courier: 0, since: 0.0 };
        assert!(Dispatch::restore(&[waiting, waiting], state.bins()).is_err());
        let assigned = pb::Order { courier: people[4].raw(), ..waiting };
        assert!(Dispatch::restore(&[assigned, assigned], state.bins()).is_err());
        let dog = super::super::uid::Uid::new(super::super::uid::EntityType::Dog, 99);
        assert!(Dispatch::restore(&[pb::Order { courier: dog.raw(), ..waiting }], state.bins()).is_err());
    }

    #[test]
    fn a_store_calls_again_only_once_its_last_call_has_a_courier() {
        let mut dispatch = Dispatch::new();
        let (store, other) = (Point::new(4, 4), Point::new(9, 9));
        assert!(dispatch.call(store));
        assert!(!dispatch.call(store), "the dispatch has it already");
        assert!(dispatch.call(other), "another store's call is its own");
        assert_eq!(dispatch.waiting(), 2);
        let courier = Uid::from_raw(0x0001_0000_0000_0007).unwrap();
        dispatch.orders[0] = Order { store, courier: Some(courier), since: 10.0 };
        assert!(dispatch.call(store), "the first is on its way");
        assert_eq!(dispatch.order_of(courier).map(|order| order.store), Some(store));
        assert!(!dispatch.delivered(courier, other), "not where it was sent");
        assert!(dispatch.delivered(courier, store));
        assert_eq!(dispatch.order_of(courier), None);
        assert_eq!(dispatch.orders().len(), 2);
    }
}
