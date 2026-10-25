//! [`Wounds`]: the bites dealt this tick, waiting for whoever was bitten to
//! feel them.
//!
//! A bite is a deathdog's [`Effect::Bite`](super::Effect::Bite), granted in
//! the world step — but what a bite does is to the body bitten, and **react
//! writes only to itself**: the world step may not reach into a person's
//! biology. So the world step files the wound here, and the person bitten
//! reads its own wounds in its own react on the next tick
//! ([`Think::wounds`](super::Think)): its health, its fear, its cause of
//! death.
//!
//! Filed at the end of one tick and read in the next, so a wound is state
//! **between ticks, and saved** (`pb::World.wounds`). The world step forgets
//! the last tick's at its start, as it forgets who ran a playzone
//! ([`Wounds::begin_tick`]): by then every one has been read.

use crate::map::Point;
use crate::save::{invalid, pb, Invalid};

use super::uid::Uid;

/// One bite.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Wound {
    /// Who was bitten.
    pub target: Uid,
    /// What bit it.
    pub by: Uid,
    /// Where the biter stood.
    pub from: Point,
    /// Health it takes.
    pub damage: u8,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Wounds {
    /// In the order they were dealt: slot order, the same on every run.
    dealt: Vec<Wound>,
}

impl Wounds {
    /// Forget the wounds the last tick dealt, read since by whoever took them.
    pub fn begin_tick(&mut self) {
        self.dealt.clear();
    }

    /// File a bite for its target to feel next tick.
    pub fn inflict(&mut self, wound: Wound) {
        self.dealt.push(wound);
    }

    /// The wounds dealt to `uid`, oldest first. A handful at most, and none
    /// at all nearly always, so a walk of the list: nothing to index.
    pub fn on(&self, uid: Uid) -> impl Iterator<Item = &Wound> + '_ {
        self.dealt.iter().filter(move |wound| wound.target == uid)
    }

    pub fn is_empty(&self) -> bool {
        self.dealt.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Wound> + '_ {
        self.dealt.iter()
    }

    // --- saving ---

    pub(crate) fn save(&self) -> Vec<pb::Wound> {
        let Wounds { dealt } = self;
        dealt
            .iter()
            .map(|&Wound { target, by, from, damage }| pb::Wound {
                target: target.raw(),
                by: by.raw(),
                from: Some(from.into()),
                damage: damage as u32,
            })
            .collect()
    }

    pub(crate) fn restore(saved: &[pb::Wound]) -> Result<Wounds, Invalid> {
        let mut wounds = Wounds::default();
        for wound in saved {
            let (Some(target), Some(by)) = (Uid::from_raw(wound.target), Uid::from_raw(wound.by)) else {
                return invalid("a wound dealt by or to nobody");
            };
            let Ok(damage) = u8::try_from(wound.damage) else {
                return invalid(format!("a wound of {} damage", wound.damage));
            };
            wounds.inflict(Wound { target, by, from: pb::cell(wound.from, "wound")?, damage });
        }
        Ok(wounds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::uid::EntityType;

    #[test]
    fn a_wound_is_found_by_its_target_until_the_next_tick_begins() {
        let mut wounds = Wounds::default();
        let (dog, human, other) =
            (Uid::new(EntityType::Deathdog, 1), Uid::new(EntityType::Human, 2), Uid::new(EntityType::Human, 3));
        wounds.inflict(Wound { target: human, by: dog, from: Point::new(1, 2), damage: 30 });
        assert_eq!(wounds.on(human).map(|wound| wound.damage).collect::<Vec<_>>(), [30]);
        assert_eq!(wounds.on(other).count(), 0);
        assert_eq!(Wounds::restore(&wounds.save()).unwrap(), wounds, "saved and back");
        wounds.begin_tick();
        assert!(wounds.is_empty());
    }
}
