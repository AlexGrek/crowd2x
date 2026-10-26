//! Looking for things: what the game screen's find menu can ask the world for.
//!
//! A [`Search`] is a question about the world as it is *now* — who has a job,
//! which dogs belong to somebody, where the ground is dirty, which box is
//! broken — answered by one read of the arena or of a layer
//! ([`Search::scan`]). Nothing here decides or changes anything, so a search
//! is view state to its caller (`game::find`), never saved.
//!
//! The units come back as [`Uid`]s and not as cells, because they move: the
//! screen keeps the list and asks [`GameState::position_of`] for each every
//! frame, and scans again only now and then to catch somebody hired, dead or
//! adopted in the meantime. The squares come back as cells.

use crate::map::Point;

use super::brain::goals::clean::DIRTY;
use super::job::JobKind;
use super::uid::{EntityType, Uid};
use super::{GameEntity, GameState};

/// One thing to look for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Search {
    /// Every living unit with this job.
    Workers(JobKind),
    /// Every living dog that has an owner.
    PetDogs,
    /// Every cell of the site a cleaner would take notice of: dirtier than
    /// [`DIRTY`].
    DirtySquares,
    /// Every broken thing. Only a distribution box breaks so far
    /// ([`Electrical::damaged`](super::electrical::Electrical::damaged)).
    BrokenObjects,
}

impl Search {
    /// Whether the answer is units, which move, and so is kept as [`Uid`]s —
    /// and not cells.
    pub const fn follows_units(self) -> bool {
        matches!(self, Search::Workers(_) | Search::PetDogs)
    }

    /// What one of the things found is called.
    pub fn noun(self) -> &'static str {
        match self {
            Search::Workers(kind) => kind.name(),
            Search::PetDogs => "pet dog",
            Search::DirtySquares => "dirty square",
            Search::BrokenObjects => "broken object",
        }
    }

    /// `3 guards`, `1 pet dog`, `no dirty squares`: what `count` of them
    /// are called.
    pub fn counted(self, count: usize) -> String {
        let noun = self.noun();
        let plural = match noun.strip_suffix("man") {
            Some(stem) => format!("{stem}men"),
            None => format!("{noun}s"),
        };
        match count {
            0 => format!("no {plural}"),
            1 => format!("1 {noun}"),
            _ => format!("{count} {plural}"),
        }
    }

    /// Look the world over and put the answer in `units` or in `cells`,
    /// whichever [`Search::follows_units`] says — both are cleared first, so
    /// the caller's buffers are reused from scan to scan.
    pub fn scan(self, state: &GameState, units: &mut Vec<Uid>, cells: &mut Vec<Point>) {
        units.clear();
        cells.clear();
        match self {
            Search::Workers(kind) => units.extend(
                state
                    .entities()
                    .iter()
                    .filter(|entity| !entity.is_dead() && entity.job().is_some_and(|job| job.kind == kind))
                    .map(|entity| entity.uid()),
            ),
            Search::PetDogs => units.extend(
                state
                    .entities()
                    .iter()
                    .filter(|entity| {
                        !entity.is_dead() && entity.kind() == Some(EntityType::Dog) && entity.companion().is_some()
                    })
                    .map(|entity| entity.uid()),
            ),
            Search::DirtySquares => cells.extend(
                state
                    .dirt()
                    .site_stains()
                    .filter(|(_, stain)| stain.overall() > DIRTY)
                    .map(|(cell, _)| cell),
            ),
            Search::BrokenObjects => cells.extend_from_slice(state.electrical().damaged()),
        }
    }
}
