//! The world in numbers: who is alive, what the warehouse and the steel
//! factory hold, who works at what, and the dead.
//!
//! [`Statistics`] is a snapshot, worked out on asking
//! ([`GameState::statistics`](super::GameState::statistics)) by one walk of the
//! arena — never per tick, only while somebody is reading it. Most of it is
//! there to be counted: the living and the dead lying in the world are
//! entities, the warehouse is the [`Bins`](super::Bins), the posts are the
//! workplace features. What is not is a body gone to the crematorium, which
//! leaves the world, and so the [`Tally`]: counted as it happens, and saved.

use crate::save::pb;

use super::entities::Entities;
use super::feature::Features;
use super::item::ItemKind;
use super::job::JobKind;
use super::bins::Store;
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Statistics {
    /// The living, by kind, in [`EntityType::ALL`] order.
    pub alive: Vec<(EntityType, u32)>,
    /// What the warehouse holds — scrap metal in its bins, steel on its
    /// rack — and whether there is one (any of its stores on the map).
    pub scrap_in_warehouse: u32,
    pub steel_in_warehouse: u32,
    pub warehouse: bool,
    /// What the steel factory holds — scrap metal in its hoppers, steel on
    /// its pallets — and whether there is one.
    pub scrap_in_factory: u32,
    pub steel_in_factory: u32,
    pub factory: bool,
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
        Statistics {
            alive: alive.to_vec(),
            scrap_in_warehouse: bins.total_in(Store::ScrapBin),
            steel_in_warehouse: bins.total_in(Store::SteelRack),
            warehouse: bins.count(Store::ScrapBin) + bins.count(Store::SteelRack) > 0,
            scrap_in_factory: bins.total_in(Store::ScrapHopper),
            steel_in_factory: bins.total_in(Store::SteelPallet),
            factory: bins.count(Store::ScrapHopper) + bins.count(Store::SteelPallet) > 0,
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
}
