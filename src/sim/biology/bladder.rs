//! [`Bladder`]: fills slowly with time, quickly after a drink, and is emptied
//! by a toilet.
//!
//! The first process with state of its own. A drink does not arrive in the
//! bladder the moment it is swallowed — it is put *on its way*, and arrives
//! over the next few seconds at [`FILLING_PER_SECOND`], on top of the slow
//! filling that goes on regardless. So drinking visibly fills a bladder fast,
//! rather than stepping it up in one tick.
//!
//! Food does not reach the bladder: food has no water in it
//! ([`ItemKind::hydration`](crate::sim::item::ItemKind::hydration)). What
//! eating does to a body beyond hunger is a later, more involved process.

use super::{Event, Process, ProcessId, Stats};

/// Bladder filled per second whatever a person does: empty to
/// [`BURSTING`] in a little under five minutes for somebody who never drinks.
///
/// [`BURSTING`]: crate::sim::brain::routines::BURSTING
pub const BLADDER_PER_SECOND: f32 = 0.25;

/// Bladder filled per point of hydration drunk. One drink ([`DRINK`]) is
/// thirty points on its way: between two and three drinks, and the slow
/// filling meanwhile, send a person to the toilet.
///
/// [`DRINK`]: crate::sim::item::DRINK
pub const BLADDER_PER_HYDRATION: f32 = 0.5;

/// How fast what was drunk arrives in the bladder: one drink over about six
/// seconds.
pub const FILLING_PER_SECOND: f32 = 5.0;

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Bladder {
    /// Drunk, and not in the bladder yet, in bladder points.
    on_its_way: f32,
}

impl Bladder {
    pub fn on_its_way(&self) -> f32 {
        self.on_its_way
    }
}

impl Process for Bladder {
    fn id(&self) -> ProcessId {
        ProcessId::Bladder
    }

    fn advance(&mut self, stats: &mut Stats, dt: f32) {
        let arriving = self.on_its_way.min(FILLING_PER_SECOND * dt);
        self.on_its_way -= arriving;
        stats.change_bladder(BLADDER_PER_SECOND * dt + arriving);
    }

    /// A drink goes on its way in; a toilet empties what has arrived. What is
    /// still on its way keeps coming — it was not in the bladder to empty.
    fn handle(&mut self, event: Event, stats: &mut Stats) {
        match event {
            Event::Ingested(item) => self.on_its_way += item.hydration() * BLADDER_PER_HYDRATION,
            Event::Relieved => stats.change_bladder(-stats.bladder()),
        }
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![("bladder on its way", format!("{:.1}", self.on_its_way))]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::item::{ItemKind, DRINK};

    const DT: f32 = 1.0 / 64.0;

    fn run(bladder: &mut Bladder, stats: &mut Stats, seconds: f32) {
        for _ in 0..(seconds / DT) as u32 {
            bladder.advance(stats, DT);
        }
    }

    #[test]
    fn a_bladder_fills_slowly_with_time_alone() {
        let (mut bladder, mut stats) = (Bladder::default(), Stats::calm().with_bladder(0.0));
        run(&mut bladder, &mut stats, 10.0);
        assert!((stats.bladder() - 10.0 * BLADDER_PER_SECOND).abs() < 1e-3, "{}", stats.bladder());
    }

    #[test]
    fn a_drink_fills_the_bladder_quickly_but_not_all_at_once() {
        let (mut bladder, mut stats) = (Bladder::default(), Stats::calm().with_bladder(0.0));
        bladder.handle(Event::Ingested(ItemKind::Water), &mut stats);
        assert_eq!(stats.bladder(), 0.0, "swallowed is not arrived");

        run(&mut bladder, &mut stats, 1.0);
        let after_a_second = stats.bladder();
        assert!(
            after_a_second > 10.0 * BLADDER_PER_SECOND,
            "a second after a drink should beat ten seconds without: {after_a_second}"
        );
        assert!(after_a_second < DRINK * BLADDER_PER_HYDRATION, "not all of it yet");

        run(&mut bladder, &mut stats, 30.0);
        let all = DRINK * BLADDER_PER_HYDRATION + 31.0 * BLADDER_PER_SECOND;
        assert!((stats.bladder() - all).abs() < 0.1, "{} of {all}", stats.bladder());
        assert_eq!(bladder.on_its_way(), 0.0);
    }

    #[test]
    fn food_does_not_reach_the_bladder() {
        let (mut bladder, mut stats) = (Bladder::default(), Stats::calm().with_bladder(0.0));
        bladder.handle(Event::Ingested(ItemKind::Food), &mut stats);
        assert_eq!(bladder.on_its_way(), 0.0);
    }

    #[test]
    fn a_toilet_empties_what_arrived_and_what_is_on_its_way_keeps_coming() {
        let (mut bladder, mut stats) = (Bladder::default(), Stats::calm().with_bladder(80.0));
        bladder.handle(Event::Ingested(ItemKind::Water), &mut stats);
        run(&mut bladder, &mut stats, 1.0);

        bladder.handle(Event::Relieved, &mut stats);
        assert_eq!(stats.bladder(), 0.0);
        assert!(bladder.on_its_way() > 0.0);
        run(&mut bladder, &mut stats, 1.0);
        assert!(stats.bladder() > BLADDER_PER_SECOND, "{}", stats.bladder());
    }

    #[test]
    fn a_bladder_never_fills_past_desperate() {
        let (mut bladder, mut stats) = (Bladder::default(), Stats::calm().with_bladder(95.0));
        for _ in 0..5 {
            bladder.handle(Event::Ingested(ItemKind::Water), &mut stats);
        }
        run(&mut bladder, &mut stats, 60.0);
        assert_eq!(stats.bladder(), 100.0);
    }
}
