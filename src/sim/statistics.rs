//! The world in numbers: who is alive, what the warehouse, the steel
//! factory and the farm hold, what grows in the fields, who works at what,
//! and the dead.
//!
//! [`Statistics`] is a snapshot, worked out on asking
//! ([`GameState::statistics`](super::GameState::statistics)) by one walk of the
//! arena — never per tick, only while somebody is reading it. Most of it is
//! there to be counted: the living and the dead lying in the world are
//! entities, the warehouse and the factory are the [`Bins`](super::Bins) —
//! grouped by the site each store kind's row names, so a new store or a new
//! building with stores shows up here with no change — and the posts are the
//! workplace features. What is not is a body gone to the crematorium, which
//! leaves the world, and so the [`Tally`]: counted as it happens, and saved.

use crate::save::pb;

use super::clock::Clock;
use super::economy::{Economy, DayBook, Money};
use super::entities::Entities;
use super::facilities::Facilities;
use super::feature::FeatureKind;
use super::fields::Stage;
use super::item::ItemKind;
use super::job::JobKind;
use super::uid::EntityType;

/// What happened that the world itself no longer shows.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Tally {
    /// Units that have died, however they came to leave the world since.
    pub died: u32,
    /// Bodies an undertaker has cremated: gone from the world.
    pub cremated: u32,
}

impl Tally {
    pub(crate) fn save(&self) -> pb::Deaths {
        let Tally { died, cremated } = *self;
        pb::Deaths { died, cremated }
    }

    pub(crate) fn restore(saved: Option<&pb::Deaths>) -> Tally {
        saved.map_or_else(Tally::default, |saved| Tally { died: saved.died, cremated: saved.cremated })
    }
}

/// One kind of job: how many have it, and how many could.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JobCount {
    pub kind: JobKind,
    /// Living units employed as it.
    pub workers: u32,
    /// Every workplace of the kind times its shifts: the most it can employ.
    pub posts: u32,
}

/// What one building's stores hold, all of them together: the warehouse's
/// bins and rack, the factory's hoppers and pallets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SiteStock {
    /// The building, as its stores' rows name it
    /// ([`StoreDef::site`](super::feature::StoreDef::site)).
    pub site: &'static str,
    /// Whether the map has any of its stores — a map without a warehouse
    /// says so, rather than that it is empty.
    pub present: bool,
    /// Every kind of thing kept there and how many pieces, in the order the
    /// store kinds are listed.
    pub items: Vec<(ItemKind, u32)>,
}

/// The fields: how many crops are at each stage, and how well kept they are.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct FieldStock {
    /// Crops at each [`Stage`], in [`Stage::ALL`] order.
    pub stages: [u32; Stage::ALL.len()],
    /// Their quality on average, in percent, rounded down; `None` with
    /// nothing sown.
    pub quality: Option<u32>,
}

impl FieldStock {
    /// Every crop in the fields.
    pub fn sown(&self) -> u32 {
        self.stages.iter().sum()
    }
}

/// Money is counted on demand, never with a per-unit scan during a tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct EconomyStats {
    pub budget: Money,
    pub tax_percent: u32,
    pub today: Option<DayBook>,
    pub accounts: u32,
    pub cash: Money,
    pub average: Money,
    pub in_debt: u32,
    pub debt: Money,
    pub shifts: u32,
    pub care_days: u32,
}

impl EconomyStats {
    fn of(economy: &Economy) -> Self {
        let mut stats = Self { budget: economy.budget(), tax_percent: economy.tariffs().tax_percent,
            today: economy.today().copied(), ..Self::default() };
        for account in economy.accounts().iter().filter(|account| !account.dead) {
            stats.accounts += 1;
            stats.cash += account.cash;
            if account.cash.is_negative() { stats.in_debt += 1; stats.debt -= account.cash; }
            stats.shifts += account.shifts as u32;
            stats.care_days += account.care_days as u32;
        }
        if stats.accounts > 0 { stats.average = Money(stats.cash.as_cents() / stats.accounts as i64); }
        stats
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Statistics {
    pub economy: EconomyStats,
    /// The living, by kind, in [`EntityType::ALL`] order.
    pub alive: Vec<(EntityType, u32)>,
    /// What every building with stores holds, one entry a site, in the order
    /// their store kinds are listed — every site there is, on this map or
    /// not.
    pub sites: Vec<SiteStock>,
    /// What is growing in the fields.
    pub fields: FieldStock,
    /// Every [`JobKind`], in [`JobKind::ALL`] order, posts or none.
    pub jobs: Vec<JobCount>,
    /// Living humans with no job.
    pub jobless: u32,
    /// Everybody who has died.
    pub died: u32,
    /// Bodies lying where they fell.
    pub lying: u32,
    /// Bodies in an undertaker's hands, on the way to the crematorium.
    pub carried: u32,
    pub cremated: u32,
}

impl Statistics {
    /// The world's numbers at `now`.
    pub(crate) fn of(entities: &Entities, facilities: &Facilities, now: Clock) -> Statistics {
        let Facilities { features, bins, corpses, tally, fields, .. } = facilities;
        let mut alive = EntityType::ALL.map(|kind| (kind, 0u32));
        let mut workers = [0u32; JobKind::ALL.len()];
        let (mut jobless, mut carried) = (0, 0);
        for (_, entity) in entities.iter_slots() {
            if entity.inventory().and_then(|inventory| inventory.hand()) == Some(ItemKind::Body) {
                carried += 1;
            }
            if entity.is_dead() {
                continue;
            }
            let kind = entity.uid().kind();
            if let Some(count) = alive.iter_mut().find(|(each, _)| Some(*each) == kind) {
                count.1 += 1;
            }
            match entity.job() {
                Some(job) => workers[JobKind::ALL.iter().position(|&each| each == job.kind).unwrap_or(0)] += 1,
                None if kind == Some(EntityType::Human) => jobless += 1,
                None => {}
            }
        }
        let jobs = JobKind::ALL
            .into_iter()
            .zip(workers)
            .map(|(kind, workers)| JobCount {
                kind,
                workers,
                posts: (features.cells_of(kind.workplace()).len() * kind.shifts().len()) as u32,
            })
            .collect();
        let mut sites: Vec<SiteStock> = Vec::new();
        for kind in FeatureKind::ALL {
            let Some(store) = kind.store() else {
                continue;
            };
            let at = match sites.iter().position(|stock| stock.site == store.site) {
                Some(at) => at,
                None => {
                    sites.push(SiteStock { site: store.site, present: false, items: Vec::new() });
                    sites.len() - 1
                }
            };
            let stock = &mut sites[at];
            stock.present |= bins.count(kind) > 0;
            let stored = bins.total_in(kind);
            match stock.items.iter_mut().find(|(item, _)| *item == store.holds) {
                Some((_, count)) => *count += stored,
                None => stock.items.push((store.holds, stored)),
            }
        }
        let mut stock = FieldStock::default();
        let mut quality = 0u32;
        for crop in fields.iter() {
            stock.stages[crop.stage(now) as usize] += 1;
            quality += crop.quality as u32;
        }
        stock.quality = (!fields.is_empty()).then(|| quality / fields.len() as u32);
        Statistics {
            economy: EconomyStats::of(&facilities.economy),
            alive: alive.to_vec(),
            sites,
            fields: stock,
            jobs,
            jobless,
            died: tally.died,
            lying: corpses.len() as u32,
            carried,
            cremated: tally.cremated,
        }
    }

    /// Every living unit, of every kind.
    pub fn living(&self) -> u32 {
        self.alive.iter().map(|&(_, count)| count).sum()
    }

    /// How many pieces of `item` the stores of `site` hold.
    pub fn stock(&self, site: &str, item: ItemKind) -> u32 {
        self.sites
            .iter()
            .filter(|stock| stock.site == site)
            .flat_map(|stock| &stock.items)
            .filter(|(each, _)| *each == item)
            .map(|(_, count)| count)
            .sum()
    }

    /// Whether the map has any of `site`'s stores.
    pub fn has(&self, site: &str) -> bool {
        self.sites.iter().any(|stock| stock.site == site && stock.present)
    }
}
