//! Bodily stains, settled in slot order after reactions and before new bites.
//! Independent of the brain's effect slot: a fatal bite still leaves blood,
//! and an accident cannot swallow a completed task's effect.

use super::biology::{ProcessId, bladder::CRITICAL};
use super::dirt::{Dirt, PEE};
use super::entity::GameEntity;
use super::facilities::Facilities;
use super::feature::FeatureKind;
use super::log::Log;

pub(super) fn settle(entity: &mut dyn GameEntity, facilities: &mut Facilities, log: &Log) {
    if entity.is_frozen() {
        return;
    }
    let Some(biology) = entity.biology() else {
        return;
    };
    // Almost every body has nothing to leave this tick. Avoid looking up
    // its position or toilet on that common path, especially in big crowds.
    let wetting = biology.is_running(ProcessId::Bladder)
        && biology.stats().bladder() >= CRITICAL
        && !entity.is_dead();
    let bleeds = biology.is_running(ProcessId::Health) && !facilities.wounds.is_empty();
    if !wetting && !bleeds {
        return;
    }
    let uid = entity.uid();
    let cell = entity.center_position();
    if bleeds {
        for wound in facilities.wounds.on(uid) {
            let blood = Dirt {
                blood: (wound.damage as f32 / 100.0).min(1.0),
                ..Dirt::CLEAN
            };
            entity.body_mut().dirt_mut().soil(blood);
            facilities.dirt.soil(cell, blood);
        }
    }
    if !wetting || facilities.features.has(FeatureKind::Toilet, cell) {
        return;
    }
    if entity
        .biology_mut()
        .is_some_and(|b| b.relieve_if_critical())
    {
        entity.body_mut().dirt_mut().soil(PEE);
        facilities.dirt.soil(cell, PEE);
        log.unit(
            uid,
            format!(
                "{uid} peed: bladder critical; left a wet puddle at {}, {}",
                cell.x, cell.y
            ),
        );
    }
}

#[cfg(test)]
mod tests;
