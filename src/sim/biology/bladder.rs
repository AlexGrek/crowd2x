//! [`Bladder`]: fills slowly with time, quickly after a drink, and is emptied
//! by a toilet.
//!
//! The first process with state of its own. A drink does not arrive in the
//! bladder the moment it is swallowed — it is put *on its way*, and arrives
//! over the next half hour at [`FILLING_PER_SECOND`], on top of the slow
//! filling that goes on regardless. So drinking visibly fills a bladder fast,
//! rather than stepping it up in one tick.
//!
//! Food does not reach the bladder: food has no water in it
//! ([`ItemKind::hydration`](crate::sim::item::ItemKind::hydration)). What
//! eating does to a body beyond hunger is a later, more involved process.

use crate::sim::clock::{HOUR, MINUTE};
use crate::sim::item::DRINK;

use super::{Event, Process, ProcessId, Stats};

/// World hours from empty to full for somebody who never drinks anything.
/// [`BURSTING`] is four hours of that — which nobody reaches, because
/// drinking is what really fills a bladder.
///
/// [`BURSTING`]: crate::sim::brain::routines::BURSTING
pub const HOURS_TO_FULL: f32 = 6.0;

/// Bladder filled per **world** second whatever a person does —
/// [`Think::game_dt`], never `dt`.
///
/// [`Think::game_dt`]: crate::sim::entity::Think::game_dt
pub const BLADDER_PER_SECOND: f32 = 100.0 / (HOURS_TO_FULL * HOUR);

/// Bladder filled per point of hydration drunk. One drink ([`DRINK`]) is
/// thirty points on its way, so it is a drink and the hours around it that
/// send somebody to the toilet rather than the slow filling alone — several
/// times a day, as a person does.
///
/// [`DRINK`]: crate::sim::item::DRINK
pub const BLADDER_PER_HYDRATION: f32 = 0.5;

/// World seconds for a drink to finish arriving in the bladder: half an hour,
/// which is fifteen seconds of watching somebody's bladder climb after they
/// have had a glass of water.
pub const ARRIVES_OVER: f32 = 30.0 * MINUTE;

/// How fast what was drunk arrives in the bladder, per world second.
pub const FILLING_PER_SECOND: f32 = DRINK * BLADDER_PER_HYDRATION / ARRIVES_OVER;

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
    use crate::sim::brain::routines::BURSTING;
    use crate::sim::clock::TIME_SCALE;
    use crate::sim::item::ItemKind;

    /// One tick, in world seconds: the fixed timestep, scaled the way
    /// `Think::game_dt` scales it.
    const DT: f32 = TIME_SCALE / 64.0;

    fn run(bladder: &mut Bladder, stats: &mut Stats, world_seconds: f32) {
        for _ in 0..(world_seconds / DT) as u32 {
            bladder.advance(stats, DT);
        }
    }

    #[test]
    fn a_bladder_fills_slowly_with_time_alone() {
        let (mut bladder, mut stats) = (Bladder::default(), Stats::calm().with_bladder(0.0));
        run(&mut bladder, &mut stats, 10.0 * MINUTE);
        assert!(
            (stats.bladder() - 10.0 * MINUTE * BLADDER_PER_SECOND).abs() < 1e-3,
            "{}",
            stats.bladder()
        );
    }

    /// Slowly means hours, and a drink is what really fills one.
    #[test]
    fn hours_alone_are_what_it_takes_without_a_drink() {
        let (mut bladder, mut stats) = (Bladder::default(), Stats::calm().with_bladder(0.0));
        run(&mut bladder, &mut stats, 3.0 * HOUR);
        assert!(stats.bladder() < BURSTING, "{}", stats.bladder());

        bladder.handle(Event::Ingested(ItemKind::Water), &mut stats);
        run(&mut bladder, &mut stats, ARRIVES_OVER);
        assert!(stats.bladder() > BURSTING, "a drink is what tips it: {}", stats.bladder());
    }

    #[test]
    fn a_drink_fills_the_bladder_quickly_but_not_all_at_once() {
        let (mut bladder, mut stats) = (Bladder::default(), Stats::calm().with_bladder(0.0));
        bladder.handle(Event::Ingested(ItemKind::Water), &mut stats);
        assert_eq!(stats.bladder(), 0.0, "swallowed is not arrived");

        run(&mut bladder, &mut stats, MINUTE);
        let after_a_minute = stats.bladder();
        assert!(
            after_a_minute > 3.0 * MINUTE * BLADDER_PER_SECOND,
            "a minute after a drink should beat a minute without several times over: {after_a_minute}"
        );
        assert!(after_a_minute < DRINK * BLADDER_PER_HYDRATION, "not all of it yet");

        run(&mut bladder, &mut stats, HOUR);
        let all = DRINK * BLADDER_PER_HYDRATION + (HOUR + MINUTE) * BLADDER_PER_SECOND;
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
        run(&mut bladder, &mut stats, MINUTE);

        bladder.handle(Event::Relieved, &mut stats);
        assert_eq!(stats.bladder(), 0.0);
        assert!(bladder.on_its_way() > 0.0);
        run(&mut bladder, &mut stats, MINUTE);
        assert!(stats.bladder() > MINUTE * BLADDER_PER_SECOND, "{}", stats.bladder());
    }

    #[test]
    fn a_bladder_never_fills_past_desperate() {
        let (mut bladder, mut stats) = (Bladder::default(), Stats::calm().with_bladder(95.0));
        for _ in 0..5 {
            bladder.handle(Event::Ingested(ItemKind::Water), &mut stats);
        }
        run(&mut bladder, &mut stats, HOUR);
        assert_eq!(stats.bladder(), 100.0);
    }
}
