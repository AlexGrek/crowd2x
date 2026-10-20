//! [`Effect`]: a change a unit asks of the world beyond itself, and
//! [`apply`], the one place each is granted.
//!
//! **A new effect is a variant here and its arm in [`apply`]**, which the
//! world step (`sim::world_step`, every unit in slot order) and the tests'
//! one-unit world (`testing::World::step`) both call, so the two cannot grant
//! one differently.

use crate::map::{Map, Point};

use super::bins::Bins;
use super::clock::Clock;
use super::entity::GameEntity;
use super::facilities::Facilities;
use super::feature::FeatureKind;
use super::fields::PLOTS;
use super::inventory::Inventory;
use super::item::ItemKind;
use super::log::Log;
use super::occupancy::Occupancy;
use super::property::PropertyId;
use super::uid::Uid;

/// A change asked for *outside* the entity that asked for it.
///
/// [`Intent`](super::Intent)'s counterpart for the one kind of change react
/// is not allowed to make directly: **react may write only to itself**
/// ([`GameEntity::react`]), so a fridge's door — a fact about the world, read
/// by every entity beside it — cannot be flipped from inside the parallel
/// round that produces this. An `Effect` is the request; the world step is
/// the sequential step, afterwards, that is allowed to grant it ([`apply`]).
/// Small and `Copy` for the same reason `Intent` is: one per entity per tick,
/// in a buffer that is reused rather than reallocated.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Effect {
    /// Nothing. What every entity that never touches a fridge returns.
    #[default]
    None,
    /// Open or close the fridge at `at`.
    Fridge { at: Point, open: bool },
    /// Open the door at `at`, or keep it open: somebody holding `key` is
    /// about to go through it. Refused for a door locked to another key —
    /// see [`super::door`].
    Door { at: Point, key: Option<PropertyId> },
    /// Take a piece of scrap metal off the heap at `at` and stow it: the
    /// unit has finished salvaging it (`brain::tasks::Salvage`). Granted
    /// only while the heap has a piece left and the unit has room for one,
    /// so two finishing on the last piece in one tick get one between them,
    /// the lower slot's.
    Salvage { at: Point },
    /// Put `pieces` of what the store at `at` holds in it: a unit has
    /// finished emptying its pack into it (`brain::tasks::Deposit`, which
    /// has already taken them out of the pack). See [`super::bins`].
    Deposit { at: Point, pieces: u8 },
    /// Take as much of what the store at `at` holds as the unit has room
    /// for, and stow it: the unit has finished filling its pack there
    /// (`brain::tasks::Withdraw`). Granted piece by piece while the store
    /// has any and the pack has room, so two at the last pieces share them
    /// out in slot order.
    Withdraw { at: Point },
    /// The unit died this tick, of `cause` (`biology::health`): it has
    /// already marked its own body dead, and the world gives back what it
    /// held — its cell, its bed, its workplace (with the job, so a worker is
    /// not left on the books) — and says so in the log.
    Died { cause: &'static str },
    /// Take up the body lying at `at`: the unit, an undertaker, has finished
    /// lifting it (`brain::tasks::TakeBody`). Granted to a free hand while
    /// the body is still there, so two undertakers at one body get one
    /// between them, the lower slot's; the body is then carried as an item
    /// and leaves the world at the end of the tick.
    TakeBody { at: Point },
    /// A body was put in the crematorium at `at` (`brain::tasks::Cremate`,
    /// which has already taken it out of the hand). Counted and logged.
    Cremate { at: Point },
    /// Sow a crop in the plot at `at`, `when` the unit finished
    /// (`brain::tasks::Sow`), for the farm it works for — its job's grain
    /// bin. Refused where somebody stands in the plot, where it was sown
    /// first, or where the farm's field is full ([`super::fields`]).
    Sow { at: Point, when: Clock },
    /// Count a day's tending of the crop at `at` (`brain::tasks::Tend`):
    /// its quality goes up, once a day.
    Tend { at: Point, when: Clock },
    /// Harvest the ripe crop at `at` (`brain::tasks::Harvest`): out of the
    /// fields, its plot open again, and what it yields into the unit's pack —
    /// granted only with room for all of it.
    Harvest { at: Point, when: Clock },
}

/// Grant what `entity` asked for. The heap and the pack, the store and the
/// pack, the body and the hand change **together** here, which is why it
/// gets both the entity and the world; called in slot order, so whoever is
/// first to the last piece is the same on every run.
pub(crate) fn apply(
    effect: Effect,
    entity: &mut dyn GameEntity,
    map: &mut Map,
    facilities: &mut Facilities,
    occupancy: &mut Occupancy,
    log: &Log,
) {
    match effect {
        Effect::None => {}
        Effect::Fridge { at, open } => {
            let _ = facilities.fridges.set_open(at, open);
        }
        Effect::Door { at, key } => {
            // Not logged: a crowd opens doors all day, and a line each
            // would push everything else out of the log.
            let _ = facilities.doors.want(at, key);
        }
        Effect::Salvage { at } => salvage(entity, map, log, at),
        Effect::Deposit { at, pieces } => deposit(entity.uid(), &mut facilities.bins, log, at, pieces),
        Effect::Withdraw { at } => {
            let uid = entity.uid();
            withdraw(uid, entity.inventory_mut(), &mut facilities.bins, log, at);
        }
        Effect::Died { cause } => died(entity, facilities, occupancy, log, cause),
        Effect::TakeBody { at } => take_body(entity, facilities, log, at),
        Effect::Cremate { at } => {
            facilities.tally.cremated += 1;
            log.push(format!("{} cremated a body in the crematorium at {}, {}", entity.uid(), at.x, at.y));
        }
        Effect::Sow { at, when } => sow(entity, map, facilities, occupancy, log, at, when),
        Effect::Tend { at, when } => match facilities.fields.tend(at, when) {
            Some(quality) => log.push(format!("{} tended the wheat at {}, {}: kept at {quality}%", entity.uid(), at.x, at.y)),
            None => log.push(format!("{} found nothing to tend at {}, {}", entity.uid(), at.x, at.y)),
        },
        Effect::Harvest { at, when } => harvest(entity, map, facilities, log, at, when),
    }
}

/// [`Effect::Sow`]: a crop in the plot at `at`, for the farm the unit works
/// for, unless somebody stands there or the fields will not have it. Logged
/// either way — once a crop.
fn sow(entity: &dyn GameEntity, map: &mut Map, facilities: &mut Facilities, occupancy: &Occupancy, log: &Log, at: Point, when: Clock) {
    let uid = entity.uid();
    let Some(farm) = entity.job().map(|job| job.workplace) else {
        return;
    };
    if let Some(other) = occupancy.occupant(at) {
        log.push(format!("{uid} could not sow at {}, {}: {other} is standing there", at.x, at.y));
        return;
    }
    // The dead let their cell go (`died`), so the occupancy does not say so:
    // a body is not sown over.
    if let Some(dead) = facilities.corpses.at(at) {
        log.push(format!("{uid} could not sow at {}, {}: the body of {dead} lies there", at.x, at.y));
        return;
    }
    if facilities.fields.sow(map, at, farm, when) {
        let sown = facilities.fields.of(farm).count();
        log.push(format!("{uid} sowed wheat at {}, {}: {sown} of {PLOTS} sown", at.x, at.y));
    } else {
        log.push(format!("{uid} could not sow at {}, {}", at.x, at.y));
    }
}

/// [`Effect::Harvest`]: the ripe crop at `at` out of the fields and what it
/// yields into the unit's pack, while the pack has room for all of it.
/// Logged either way — once a crop.
fn harvest(entity: &mut dyn GameEntity, map: &mut Map, facilities: &mut Facilities, log: &Log, at: Point, when: Clock) {
    let uid = entity.uid();
    let Some(inventory) = entity.inventory_mut() else {
        return;
    };
    let Some(crop) = facilities.fields.at(at).copied() else {
        log.push(format!("{uid} found nothing to harvest at {}, {}", at.x, at.y));
        return;
    };
    if !inventory.room_for_many(ItemKind::Wheat, crop.harvest()) {
        log.push(format!("{uid} has no room for the wheat at {}, {}: {}", at.x, at.y, inventory.describe_load()));
        return;
    }
    match facilities.fields.harvest(map, at, when) {
        Some(crop) => {
            for _ in 0..crop.harvest() {
                let _ = inventory.stow(ItemKind::Wheat);
            }
            log.push(format!("{uid} harvested {} wheat at {}, {}, kept at {}%", crop.harvest(), at.x, at.y, crop.quality));
        }
        None => log.push(format!("{uid} found nothing ripe at {}, {}", at.x, at.y)),
    }
}

/// [`Effect::TakeBody`]: the body at `at` into the unit's hand, while there
/// is one there and the hand is free — and off the list of the dead and on
/// its way out of the world. Logged either way, once a body.
fn take_body(entity: &mut dyn GameEntity, facilities: &mut Facilities, log: &Log, at: Point) {
    let uid = entity.uid();
    let Some(inventory) = entity.inventory_mut() else {
        return;
    };
    let Some(dead) = facilities.corpses.at(at) else {
        log.push(format!("{uid} found no body at {}, {}", at.x, at.y));
        return;
    };
    if inventory.hand().is_some() {
        log.push(format!("{uid} has no hand free for the body at {}, {}", at.x, at.y));
        return;
    }
    let _ = inventory.set_hand(Some(ItemKind::Body));
    facilities.corpses.remove(dead);
    facilities.burials.push(dead);
    log.push(format!("{uid} took up the body of {dead} at {}, {}", at.x, at.y));
}

/// [`Effect::Salvage`]: a piece off the heap and into the unit's pack, if
/// there is a piece and room for it. Logged either way — once a piece, a few
/// seconds of work apart, so never a flood.
fn salvage(entity: &mut dyn GameEntity, map: &mut Map, log: &Log, at: Point) {
    let uid = entity.uid();
    let Some(inventory) = entity.inventory_mut() else {
        return;
    };
    if !inventory.room_for(ItemKind::Scrap) {
        log.push(format!("{uid} has no room for scrap metal: {}", inventory.describe_load()));
        return;
    }
    match map.take_scrap(at) {
        Some(left) => {
            let _ = inventory.stow(ItemKind::Scrap);
            log.push(if left == 0 {
                format!("{uid} salvaged scrap metal at {}, {}: the heap is gone", at.x, at.y)
            } else {
                format!("{uid} salvaged scrap metal at {}, {}: {left} left", at.x, at.y)
            });
        }
        None => log.push(format!("{uid} found no scrap left at {}, {}", at.x, at.y)),
    }
}

/// [`Effect::Died`]: the unit's cell is let go, so the crowd walks over the
/// body rather than round it and nobody sees it there; its bed goes back to
/// be given to the next arrival, and its workplace and job with it, as a
/// despawn's do. The body stays where it fell, filed with the
/// [`Corpses`](super::Corpses) for an undertaker to find, and is counted.
fn died(entity: &mut dyn GameEntity, facilities: &mut Facilities, occupancy: &mut Occupancy, log: &Log, cause: &'static str) {
    let uid = entity.uid();
    let cell = entity.center_position();
    occupancy.release(cell, uid);
    facilities.homes.release(uid);
    facilities.workplaces.release(uid);
    facilities.corpses.add(uid, cell);
    facilities.tally.died += 1;
    let _ = entity.dismiss();
    log.push(format!("{uid} died of {cause}"));
}

/// [`Effect::Deposit`], for the unit `uid`: its pieces into the store at
/// `at`, and a line in the log — once a packful, so never a flood.
pub(crate) fn deposit(uid: Uid, bins: &mut Bins, log: &Log, at: Point, pieces: u8) {
    let store = bins.store(at).and_then(FeatureKind::store);
    let (item, word) = store.map_or(("scrap metal", "bin"), |store| (store.holds.name(), store.called));
    match bins.add(at, pieces as u32) {
        Some(stored) => log.push(format!(
            "{uid} stored {pieces} {item} in the {word} at {}, {}: {stored} stored",
            at.x, at.y
        )),
        None => log.push(format!("{uid} found no store at {}, {}", at.x, at.y)),
    }
}

/// [`Effect::Withdraw`], for the unit `uid` and its pack: pieces out of the
/// store at `at` and into the pack, one at a time while the store has any and
/// the pack has room. Logged once a packful, either way.
pub(crate) fn withdraw(uid: Uid, inventory: Option<&mut Inventory>, bins: &mut Bins, log: &Log, at: Point) {
    let Some(inventory) = inventory else {
        return;
    };
    let Some(store) = bins.store(at).and_then(FeatureKind::store) else {
        log.push(format!("{uid} found no store at {}, {}", at.x, at.y));
        return;
    };
    let (item, word) = (store.holds, store.called);
    let mut pieces = 0;
    while inventory.room_for(item) && bins.take(at) {
        let _ = inventory.stow(item);
        pieces += 1;
    }
    let left = bins.stored(at);
    log.push(if pieces == 0 {
        format!("{uid} took no {} from the {word} at {}, {}: {left} left", item.name(), at.x, at.y)
    } else {
        format!("{uid} took {pieces} {} from the {word} at {}, {}: {left} left", item.name(), at.x, at.y)
    });
}
