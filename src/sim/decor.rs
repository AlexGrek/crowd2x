//! Household furniture, purchases in transit and safe placement. World effects are the only writers.
use super::clock::Clock;
use super::facilities::Facilities;
use super::item::ItemKind;
use super::property::PropertyId;
use super::{GameEntity, GameState, Occupancy, Properties, Uid};
use crate::map::{Map, Object, ObjectKind, ObjectLayer, PIXELS_PER_CELL, Point};
use crate::save::{Invalid, invalid, pb};
use std::collections::{BTreeMap, BTreeSet};

pub const VASE: &str = "flower vase";
pub const SECTION: &str = "decor section";
pub const LIMIT: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Receipt {
    pub home: PropertyId,
    pub cell: Point,
}
#[derive(Default)]
pub struct Decor {
    rooms: BTreeMap<PropertyId, BTreeSet<Point>>,
    installed: BTreeMap<PropertyId, usize>,
    pending: BTreeMap<Uid, Receipt>,
    reserved: BTreeMap<PropertyId, usize>,
}
impl Decor {
    pub fn from_map(map: &Map, properties: &Properties) -> Self {
        let mut result = Self::default();
        for cell in map.size().points() {
            if let Some(home) = properties.of(cell) {
                result.rooms.entry(home).or_default().insert(cell);
            }
        }
        for prop in map
            .objects(ObjectLayer::Props)
            .iter()
            .filter(|p| p.kind.as_str() == VASE)
        {
            if let Some(home) = properties.of(prop.cell()) {
                *result.installed.entry(home).or_default() += 1;
            }
        }
        result
    }
    pub fn count(&self, home: PropertyId) -> usize {
        self.installed.get(&home).copied().unwrap_or(0)
            + self.reserved.get(&home).copied().unwrap_or(0)
    }
    pub fn receipt(&self, uid: Uid) -> Option<Receipt> {
        self.pending.get(&uid).copied()
    }
    pub fn release(&mut self, uid: Uid) {
        if let Some(r) = self.pending.remove(&uid) {
            *self.reserved.entry(r.home).or_default() -= 1;
        }
    }
    fn safe(&self, map: &Map, home: PropertyId, at: Point) -> bool {
        let Some(room) = self.rooms.get(&home) else {
            return false;
        };
        if !room.contains(&at) || !map.is_passable(at) {
            return false;
        }
        let props: Vec<_> = map
            .objects(ObjectLayer::Props)
            .iter()
            .filter(|p| room.contains(&p.cell()))
            .collect();
        if props.iter().any(|p| {
            p.cell() == at
                || (super::door::is_door(p.kind.as_str()) && p.cell().manhattan_distance(at) <= 1)
        }) {
            return false;
        }
        if !at
            .cardinal_neighbours()
            .iter()
            .any(|&n| map.terrain(n).is_some_and(|t| !t.is_passable()))
        {
            return false;
        }
        let free = |cell: Point| {
            cell != at
                && room.contains(&cell)
                && map.is_passable(cell)
                && !self.pending.values().any(|r| r.cell == cell)
        };
        // Existing household furniture keeps its two approaches. Door leaves are walkable.
        if props
            .iter()
            .filter(|p| !p.kind.prop_passability().is_passable())
            .any(|p| {
                p.cell()
                    .cardinal_neighbours()
                    .into_iter()
                    .filter(|&n| free(n))
                    .count()
                    < 2
            })
        {
            return false;
        }
        if at
            .cardinal_neighbours()
            .into_iter()
            .filter(|&n| free(n))
            .count()
            < 2
        {
            return false;
        }
        if self
            .pending
            .values()
            .filter(|r| r.home == home && r.cell != at)
            .any(|r| {
                r.cell
                    .cardinal_neighbours()
                    .into_iter()
                    .filter(|&n| free(n))
                    .count()
                    < 2
            })
        {
            return false;
        }
        let Some(start) = room.iter().copied().find(|&c| free(c)) else {
            return false;
        };
        let mut seen = BTreeSet::from([start]);
        let mut queue = vec![start];
        let mut index = 0;
        while index < queue.len() {
            for next in queue[index].cardinal_neighbours() {
                if free(next) && seen.insert(next) {
                    queue.push(next);
                }
            }
            index += 1;
        }
        seen.len() == room.iter().filter(|&&c| free(c)).count()
    }
    fn spot(
        &self,
        map: &Map,
        home: PropertyId,
        occupancy: &Occupancy,
        corpses: &super::Corpses,
    ) -> Option<Point> {
        self.rooms.get(&home)?.iter().copied().find(|&at| {
            occupancy.occupant(at).is_none()
                && corpses.at(at).is_none()
                && !self.pending.values().any(|r| r.cell == at)
                && self.safe(map, home, at)
        })
    }
    pub fn save(&self) -> Vec<pb::DecorPurchase> {
        self.pending
            .iter()
            .map(|(&uid, r)| pb::DecorPurchase {
                owner: uid.raw(),
                home: r.home.get() as u32,
                cell: Some(r.cell.into()),
            })
            .collect()
    }
    pub fn restore(
        &mut self,
        saved: &[pb::DecorPurchase],
        entities: &super::entities::Entities,
        properties: &Properties,
        map: &Map,
    ) -> Result<(), Invalid> {
        let mut cells = BTreeSet::new();
        for r in saved {
            let Some(uid) = Uid::from_raw(r.owner) else {
                return invalid("decor: invalid owner");
            };
            let home = u16::try_from(r.home)
                .ok()
                .and_then(PropertyId::new)
                .ok_or_else(|| Invalid::new("decor: invalid home"))?;
            let cell = pb::cell(r.cell, "decor destination")?;
            let valid = entities.get(uid).is_some_and(|e| {
                !e.is_dead()
                    && e.body().home() == Some(home)
                    && e.inventory()
                        .is_some_and(|p| p.count(ItemKind::FlowerVase) > 0)
            });
            if !valid
                || properties.of(cell) != Some(home)
                || !self.safe(map, home, cell)
                || self.count(home) >= LIMIT
                || self.pending.contains_key(&uid)
                || !cells.insert(cell)
            {
                return invalid("decor: invalid or duplicate purchase");
            }
            self.pending.insert(uid, Receipt { home, cell });
            *self.reserved.entry(home).or_default() += 1;
        }
        Ok(())
    }
}
pub fn day_off(job: Option<&super::job::Job>, clock: Clock) -> bool {
    job.is_none_or(|j| !j.shift.works_on(clock.weekday()) && !j.is_due(clock))
}
pub fn eligible(entity: &dyn GameEntity, clock: Clock) -> bool {
    !entity.is_dead()
        && !entity.is_frozen()
        && !entity.is_noble()
        && entity.body().home().is_some()
        && !entity.is_on_mental_health_leave()
        && !entity.is_on_sick_leave(clock)
        && day_off(entity.job().as_ref(), clock)
}
pub fn buy(
    entity: &mut dyn GameEntity,
    map: &Map,
    f: &mut Facilities,
    occupancy: &Occupancy,
    at: Point,
    clock: Clock,
    log: &super::Log,
) {
    let uid = entity.uid();
    let Some(home) = entity.body().home() else {
        return;
    };
    if !eligible(entity, clock)
        || entity.center_position().manhattan_distance(at) > 1
        || !f
            .features
            .cells_of(super::FeatureKind::DecorSection)
            .contains(&at)
        || f.properties.of(at).is_some()
        || f.decor.receipt(uid).is_some()
        || f.decor.count(home) >= LIMIT
    {
        return;
    }
    let price = f.economy.tariffs().price(ItemKind::FlowerVase);
    if !f.economy.account(uid).is_some_and(|a| a.cash >= price)
        || !entity
            .inventory()
            .is_some_and(|p| p.room_for(ItemKind::FlowerVase))
    {
        return;
    }
    let Some(cell) = f.decor.spot(map, home, occupancy, &f.corpses) else {
        return;
    };
    let Some(pack) = entity.inventory_mut() else {
        return;
    };
    if !pack.stow(ItemKind::FlowerVase) {
        return;
    }
    let _ = f.economy.buy(uid, ItemKind::FlowerVase);
    f.decor.pending.insert(uid, Receipt { home, cell });
    *f.decor.reserved.entry(home).or_default() += 1;
    log.unit(uid, format!("{uid} bought a flower vase for {price}"));
}
fn insert(map: &mut Map, f: &mut Facilities, home: PropertyId, cell: Point) {
    let half = PIXELS_PER_CELL / 2;
    map.add_object(
        ObjectLayer::Props,
        Object {
            kind: ObjectKind::new(VASE),
            at: Point::new(
                cell.x * PIXELS_PER_CELL + half,
                cell.y * PIXELS_PER_CELL + half,
            ),
        },
    );
    *f.decor.installed.entry(home).or_default() += 1;
    f.smell.add_vase(cell);
}
/// A changed home releases its old space; paid goods can be reserved in the new flat without another sale.
pub fn rehome(entity: &dyn GameEntity, map: &Map, f: &mut Facilities, occupancy: &Occupancy) {
    let uid = entity.uid();
    f.decor.release(uid);
    if entity.is_dead() {
        return;
    }
    let Some(home) = entity.body().home() else {
        return;
    };
    if f.decor.count(home) >= LIMIT
        || !entity
            .inventory()
            .is_some_and(|p| p.count(ItemKind::FlowerVase) > 0)
    {
        return;
    }
    if let Some(cell) = f.decor.spot(map, home, occupancy, &f.corpses) {
        f.decor.pending.insert(uid, Receipt { home, cell });
        *f.decor.reserved.entry(home).or_default() += 1;
    }
}
pub fn place(
    entity: &mut dyn GameEntity,
    map: &mut Map,
    f: &mut Facilities,
    occupancy: &Occupancy,
    at: Point,
    log: &super::Log,
) {
    let uid = entity.uid();
    let Some(receipt) = f.decor.receipt(uid) else {
        return;
    };
    if entity.is_dead()
        || entity.is_frozen()
        || entity.body().home() != Some(receipt.home)
        || receipt.cell != at
        || entity.center_position().manhattan_distance(at) != 1
        || occupancy.occupant(at).is_some()
        || f.corpses.at(at).is_some()
        || f.decor.count(receipt.home) > LIMIT
        || !f.decor.safe(map, receipt.home, at)
    {
        return;
    }
    let Some(pack) = entity.inventory_mut() else {
        return;
    };
    if !pack.take_out(ItemKind::FlowerVase) {
        return;
    }
    f.decor.release(uid);
    insert(map, f, receipt.home, at);
    log.unit(
        uid,
        format!("{uid} placed a flower vase at {}, {}", at.x, at.y),
    );
}
impl GameState {
    pub(super) fn furnish_noble_homes(&mut self) {
        let mut homes = BTreeSet::new();
        for entity in self.entities.iter().filter(|e| e.is_noble()) {
            if let Some(home) = entity.body().home() {
                homes.insert(home);
            }
        }
        for home in homes {
            // Stable household stream, separate from all identity/class rolls.
            use rand::RngExt;
            let uid = Uid::new(super::EntityType::Human, home.get() as u64);
            if !super::rng::tick_rng(uid, 0x76617365).random_bool(0.5)
                || self.facilities.decor.count(home) != 0
            {
                continue;
            }
            if let Some(at) = self.facilities.decor.spot(
                &self.map,
                home,
                &self.occupancy,
                &self.facilities.corpses,
            ) {
                insert(&mut self.map, &mut self.facilities, home, at);
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::map::{FLOOR, Size, WALL};
    use crate::sim::clock::{DAY, HOUR, OPENING_TIME, TIME_SCALE};
    use crate::sim::economy::Money;
    use crate::sim::testing::{prop_at, skip_to};
    use crate::sim::{EntityType, ProcessId, process_pass};
    const SHELF: Point = Point::new(12, 4);

    pub(crate) fn flat() -> Map {
        let mut map = Map::new(Size::new(16, 10), FLOOR);
        for y in 0..9 {
            for x in 0..9 {
                if x == 0 || y == 0 || x == 8 || y == 8 {
                    map.set_terrain(Point::new(x, y), WALL);
                }
            }
        }
        map.set_terrain(Point::new(8, 4), FLOOR);
        prop_at(&mut map, "house door", Point::new(8, 4));
        prop_at(&mut map, "bed 1", Point::new(2, 6));
        prop_at(&mut map, SECTION, SHELF);
        map
    }
    fn noon(day: u32) -> Clock {
        Clock::after_watching(
            ((day - 1) as f64 * DAY as f64 + 15.0 * HOUR as f64 - OPENING_TIME) / TIME_SCALE as f64,
        )
    }
    fn shopper(f: &mut Facilities, n: u64) -> crate::sim::kinds::Human {
        use rand::SeedableRng;
        let mut rng = rand::rngs::SmallRng::seed_from_u64(n);
        let mut h = crate::sim::kinds::Human::new(
            Uid::new(EntityType::Human, n),
            SHELF.offset(1, 0),
            &mut rng,
        );
        h.body_mut().set_home(f.properties.of(Point::new(2, 6)));
        f.economy.open(h.uid(), Money::dollars(100));
        h
    }
    #[test]
    fn purchases_charge_twenty_once_and_reserve_a_shared_flat_limit() {
        let map = flat();
        let mut f = Facilities::new(&map, &crate::map::utilities::Supply::everywhere(map.size()));
        let occ = Occupancy::new(map.size());
        let log = crate::sim::Log::new();
        let mut a = shopper(&mut f, 100);
        let mut b = shopper(&mut f, 101);
        let mut c = shopper(&mut f, 102);
        buy(&mut a, &map, &mut f, &occ, SHELF, noon(1), &log);
        let first = f
            .decor
            .receipt(a.uid())
            .expect("purchase has a destination");
        assert_eq!(f.economy.account(a.uid()).unwrap().cash, Money::dollars(80));
        buy(&mut a, &map, &mut f, &occ, SHELF, noon(1), &log);
        assert_eq!(f.economy.account(a.uid()).unwrap().cash, Money::dollars(80));
        buy(&mut b, &map, &mut f, &occ, SHELF, noon(1), &log);
        assert_ne!(first.cell, f.decor.receipt(b.uid()).unwrap().cell);
        buy(&mut c, &map, &mut f, &occ, SHELF, noon(1), &log);
        assert!(f.decor.receipt(c.uid()).is_none());
        assert_eq!(
            f.economy.account(c.uid()).unwrap().cash,
            Money::dollars(100)
        );
        assert_eq!(f.decor.count(first.home), 2);
    }
    #[test]
    fn placement_waits_for_a_body_and_consumes_one_vase_without_recharging() {
        let mut map = flat();
        let mut f = Facilities::new(&map, &crate::map::utilities::Supply::everywhere(map.size()));
        let mut occ = Occupancy::new(map.size());
        let log = crate::sim::Log::new();
        let mut a = shopper(&mut f, 100);
        buy(&mut a, &map, &mut f, &occ, SHELF, noon(1), &log);
        let r = f.decor.receipt(a.uid()).unwrap();
        a.body_mut()
            .set_position(((r.cell.x + 1) as f32 + 0.5, r.cell.y as f32 + 0.5));
        let other = Uid::new(EntityType::Human, 999);
        let _ = occ.claim(r.cell, other);
        place(&mut a, &mut map, &mut f, &occ, r.cell, &log);
        assert_eq!(a.inventory().count(ItemKind::FlowerVase), 1);
        occ.release(r.cell, other);
        place(&mut a, &mut map, &mut f, &occ, r.cell, &log);
        assert_eq!(a.inventory().count(ItemKind::FlowerVase), 0);
        assert!(f.decor.receipt(a.uid()).is_none());
        assert!(!map.is_passable(r.cell));
        assert_eq!(f.decor.count(r.home), 1);
        assert_eq!(f.economy.account(a.uid()).unwrap().cash, Money::dollars(80));
    }
    #[test]
    fn a_day_off_is_a_schedule_day_not_an_evening_between_shifts() {
        let office = crate::sim::job::Job::office(Point::new(1, 1));
        assert!(!day_off(Some(&office), noon(1)));
        assert!(day_off(Some(&office), noon(6)));
        assert!(day_off(None, noon(1)));
    }
    #[test]
    fn lack_of_cash_or_placement_space_never_charges_or_creates_an_item() {
        let mut map = flat();
        let mut f = Facilities::new(&map, &crate::map::utilities::Supply::everywhere(map.size()));
        let occ = Occupancy::new(map.size());
        let log = crate::sim::Log::new();
        let mut a = shopper(&mut f, 100);
        f.economy.close(a.uid());
        f.economy.open(a.uid(), Money::dollars(19));
        buy(&mut a, &map, &mut f, &occ, SHELF, noon(1), &log);
        assert!(f.decor.receipt(a.uid()).is_none());
        assert_eq!(a.inventory().count(ItemKind::FlowerVase), 0);
        f.economy.close(a.uid());
        f.economy.open(a.uid(), Money::dollars(100));
        for cell in f.decor.rooms.values().next().unwrap() {
            if map.is_passable(*cell) {
                prop_at(&mut map, "crate", *cell);
            }
        }
        buy(&mut a, &map, &mut f, &occ, SHELF, noon(1), &log);
        assert!(f.decor.receipt(a.uid()).is_none());
        assert_eq!(
            f.economy.account(a.uid()).unwrap().cash,
            Money::dollars(100)
        );
    }

    #[test]
    fn workers_wait_for_their_actual_day_off_and_a_full_pack_cannot_buy() {
        let map = flat();
        let mut f = Facilities::new(&map, &crate::map::utilities::Supply::everywhere(map.size()));
        let occ = Occupancy::new(map.size());
        let log = crate::sim::Log::new();
        let mut a = shopper(&mut f, 100);
        assert!(a.hire(crate::sim::job::Job::office(Point::new(10, 4))));
        buy(&mut a, &map, &mut f, &occ, SHELF, noon(1), &log);
        assert!(f.decor.receipt(a.uid()).is_none());
        for _ in 0..10 {
            assert!(a.inventory_mut().stow(ItemKind::Steel));
        }
        buy(&mut a, &map, &mut f, &occ, SHELF, noon(6), &log);
        assert!(f.decor.receipt(a.uid()).is_none());
        assert_eq!(
            f.economy.account(a.uid()).unwrap().cash,
            Money::dollars(100)
        );
        assert!(a.inventory_mut().take_out(ItemKind::Steel));
        buy(&mut a, &map, &mut f, &occ, SHELF, noon(6), &log);
        assert!(f.decor.receipt(a.uid()).is_some());
    }
    #[test]
    fn nobles_and_unhoused_residents_do_not_buy_decorations() {
        use rand::SeedableRng;
        let map = flat();
        let mut f = Facilities::new(&map, &crate::map::utilities::Supply::everywhere(map.size()));
        let occ = Occupancy::new(map.size());
        let log = crate::sim::Log::new();
        let mut rng = rand::rngs::SmallRng::seed_from_u64(1);
        let mut noble = super::super::kinds::build(
            Uid::new(EntityType::Human, 100),
            EntityType::Human,
            SHELF.offset(1, 0),
            &mut rng,
            true,
        );
        noble.body_mut().set_home(f.properties.of(Point::new(2, 6)));
        f.economy.open(noble.uid(), Money::dollars(100));
        buy(noble.as_mut(), &map, &mut f, &occ, SHELF, noon(6), &log);
        assert!(f.decor.receipt(noble.uid()).is_none());
        let mut commoner = shopper(&mut f, 101);
        commoner.body_mut().set_home(None);
        buy(&mut commoner, &map, &mut f, &occ, SHELF, noon(6), &log);
        assert!(f.decor.receipt(commoner.uid()).is_none());
    }
    #[test]
    fn a_missing_or_unreachable_section_does_not_generate_a_route_every_tick() {
        for missing in [true, false] {
            let mut map = flat();
            if missing {
                let shelf = map
                    .objects(ObjectLayer::Props)
                    .iter()
                    .find(|p| p.kind.as_str() == SECTION)
                    .unwrap()
                    .clone();
                assert!(map.remove_object(ObjectLayer::Props, &shelf));
            } else {
                for at in SHELF.cardinal_neighbours() {
                    map.set_terrain(at, WALL);
                }
            }
            let mut state = GameState::new(map, 4);
            let mut tariffs = super::super::economy::Tariffs::default();
            tariffs.starting_cash = (Money::dollars(100), Money::dollars(100));
            state.set_tariffs(tariffs);
            let uid = state.spawn_commoner(Point::new(3, 6));
            for process in [
                ProcessId::Hunger,
                ProcessId::Thirst,
                ProcessId::Bladder,
                ProcessId::Fun,
                ProcessId::Energy,
                ProcessId::Mind,
            ] {
                state.set_process(uid, process, false);
            }
            skip_to(&mut state, 6, 15.0);
            let before = crate::sim::walker::ROUTES_ASKED.with(|n| n.get());
            for _ in 0..2000 {
                process_pass(&mut state, 1.0 / 60.0);
            }
            let routes = crate::sim::walker::ROUTES_ASKED.with(|n| n.get()) - before;
            assert!(routes < 100, "{routes} routes for missing={missing}");
            assert!(state.facilities.decor.receipt(uid).is_none());
            assert_eq!(
                state.facilities.economy.account(uid).unwrap().cash,
                Money::dollars(100)
            );
        }
    }
    #[test]
    fn a_commoner_shops_then_returns_home_without_losing_the_vase_to_a_need() {
        let mut state = GameState::new(flat(), 4);
        let mut tariffs = super::super::economy::Tariffs::default();
        tariffs.starting_cash = (Money::dollars(100), Money::dollars(100));
        state.set_tariffs(tariffs);
        let uid = state.spawn_commoner(Point::new(3, 6));
        for process in [
            ProcessId::Hunger,
            ProcessId::Thirst,
            ProcessId::Bladder,
            ProcessId::Fun,
            ProcessId::Energy,
        ] {
            state.set_process(uid, process, false);
        }
        state
            .entities
            .get_mut(uid)
            .unwrap()
            .biology_mut()
            .unwrap()
            .edit(|_| {
                crate::sim::Stats::calm()
                    .with_stamina(100.0)
                    .with_mental_health(100.0)
            });
        skip_to(&mut state, 6, 15.0);
        let home = state.entities.get(uid).unwrap().body().home().unwrap();
        for _ in 0..6000 {
            process_pass(&mut state, 1.0 / 60.0);
            if state.facilities.decor.installed.get(&home) == Some(&1) {
                break;
            }
        }
        assert_eq!(
            state.facilities.decor.installed.get(&home),
            Some(&1),
            "{:?}; {:?}",
            state.entities.get(uid).unwrap().brain_fields(),
            state.log.drain()
        );
        assert_eq!(
            state
                .entities
                .get(uid)
                .unwrap()
                .inventory()
                .unwrap()
                .count(ItemKind::FlowerVase),
            0
        );
        for _ in 0..300 {
            process_pass(&mut state, 1.0 / 60.0);
        }
        assert_eq!(
            state.facilities.decor.installed.get(&home),
            Some(&1),
            "one trip per day"
        );
    }
}
