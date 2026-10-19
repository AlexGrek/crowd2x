//! The world in numbers: who is alive, what the warehouse and the steel
//! factory hold, who works at what, and the dead.
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

use super::entities::Entities;
use super::feature::{FeatureKind, Features};
use super::item::ItemKind;
use super::job::JobKind;
use super::uid::EntityType;
use super::{Bins, Corpses};

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Statistics {
    /// The living, by kind, in [`EntityType::ALL`] order.
    pub alive: Vec<(EntityType, u32)>,
    /// What every building with stores holds, one entry a site, in the order
    /// their store kinds are listed — every site there is, on this map or
    /// not.
    pub sites: Vec<SiteStock>,
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
    pub fn of(entities: &Entities, features: &Features, bins: &Bins, corpses: &Corpses, tally: Tally) -> Statistics {
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
        Statistics {
            alive: alive.to_vec(),
            sites,
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
