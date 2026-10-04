//! [`Recollection`]: what a person has done lately, fading as the day goes on.
//!
//! Doing the same pleasant thing twice in a day is not as good the second
//! time. A meal of the same food as the last one is as filling as ever — the
//! stomach does not remember — but it is less of a treat, and a second go on
//! the computer the same afternoon is less fun *and* less of a treat. This is
//! the memory that makes that so: one number per [`Experience`], its
//! **familiarity**, raised by one each time it happens and fading away with
//! world time.
//!
//! It is not a [`Process`](super::Process): it changes no stat, and it cannot
//! be switched off. It is what [`Biology::handle`](super::Biology::handle)
//! asks before an event reaches the processes, so each of them is told how
//! *fresh* the experience was ([`Recollection::novelty`]) and decides what that
//! means for the stat it owns — [`Satisfaction`](super::Satisfaction) and
//! [`Fun`](super::Fun) scale by it, hunger and the bladder ignore it.
//!
//! Fading is a [`BackgroundTask`] at [`Priority::VeryLow`]: brought up to date
//! every 37th tick, about a minute of world, which against a four-hour
//! half-life is a difference no meal could notice — and it is the one part of
//! a body that would otherwise cost a `powf` per unit per tick.
//!
//! Inline and `Copy`: a fixed array, one slot per experience, no allocation.
//! The brain's [`Memory`](crate::sim::brain::memory::Memory) is for places and
//! people and allocates a key; this is written every meal by a task in the
//! middle of a tick, and must not.

use crate::sim::background::{BackgroundTask, Priority};
use crate::sim::clock::HOUR;
use crate::sim::item::ItemKind;

use super::Event;

/// World hours for a memory to fade to half of what it was.
///
/// Short enough that a computer played once in the morning is all but
/// forgotten by the next morning, long enough that lunch is still remembered
/// at dinner.
pub const HOURS_TO_HALF_FORGET: f32 = 4.0;

/// Familiarity below which an experience is forgotten outright.
///
/// Fading is exponential and would never quite reach zero; below this it is
/// let go, so "forgotten" is a state a debugger can see and not a number too
/// small to print. From one experience that takes a little over thirteen world
/// hours — "today", which is what the dulling is meant to be about.
pub const IN_RECENT_MEMORY: f32 = 0.1;

/// The most familiar anything gets, however often it is done.
///
/// Caps the dulling: [`Recollection::novelty`] never falls below a quarter, so
/// a thoroughly bored person at a computer still gets more fun from a go than
/// half an hour of world takes away, and does not sit there for ever.
pub const MOST_FAMILIAR: f32 = 3.0;

/// How fresh something never done before is. What a process is handed for an
/// event that is not an experience at all, and what a test hands it directly.
pub const FRESH: f32 = 1.0;

/// Something a person can tire of.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Experience {
    /// Eating or drinking something with a flavour to it: food of one kind
    /// dulls food of the same kind and nothing else.
    Taste(ItemKind),
    /// A go at something entertaining.
    Entertainment,
}

impl Experience {
    /// One slot per item kind, then entertainment.
    pub const COUNT: usize = ItemKind::COUNT + 1;

    /// Which experience an event is, if it is one. **Something with no taste is
    /// not an experience** — there is no flavour to tire of in a glass of
    /// water — and neither is the toilet or a night's sleep.
    pub fn of(event: Event) -> Option<Experience> {
        match event {
            Event::Ingested(item) if item.taste() > 0.0 => Some(Experience::Taste(item)),
            Event::Entertained => Some(Experience::Entertainment),
            Event::Ingested(_) | Event::Relieved | Event::Slept { .. } => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Experience::Taste(item) => item.name(),
            Experience::Entertainment => "entertainment",
        }
    }

    const fn slot(self) -> usize {
        match self {
            Experience::Taste(item) => item as usize,
            Experience::Entertainment => ItemKind::COUNT,
        }
    }

    const ALL: [Experience; Experience::COUNT] = {
        let mut all = [Experience::Entertainment; Experience::COUNT];
        let mut i = 0;
        while i < ItemKind::COUNT {
            all[i] = Experience::Taste(ItemKind::ALL[i]);
            i += 1;
        }
        all
    };
}

/// How familiar each experience is right now: 0 for forgotten, one more for
/// each time it happened, fading by half every [`HOURS_TO_HALF_FORGET`].
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Recollection {
    familiarity: [f32; Experience::COUNT],
}

impl BackgroundTask for Recollection {
    /// Hours to fade by half: nothing a minute late could get wrong.
    const PRIORITY: Priority = Priority::VeryLow;

    /// `elapsed` **world** seconds of forgetting.
    fn run(&mut self, elapsed: f32) {
        if self.familiarity.iter().all(|&f| f == 0.0) {
            return;
        }
        let fade = 0.5f32.powf(elapsed / (HOURS_TO_HALF_FORGET * HOUR));
        for familiarity in &mut self.familiarity {
            *familiarity *= fade;
            if *familiarity < IN_RECENT_MEMORY {
                *familiarity = 0.0;
            }
        }
    }
}

impl Recollection {
    pub fn familiarity(&self, experience: Experience) -> f32 {
        self.familiarity[experience.slot()]
    }

    /// Whether `experience` is still remembered at all.
    pub fn recalls(&self, experience: Experience) -> bool {
        self.familiarity(experience) > 0.0
    }

    /// How good `experience` would be now, as a fraction of how good it was the
    /// first time: 1 when forgotten, a half with it done once just now, a third
    /// for twice, never below `1 / (1 + MOST_FAMILIAR)`.
    pub fn novelty(&self, experience: Experience) -> f32 {
        1.0 / (1.0 + self.familiarity(experience))
    }

    /// It happened: one more time to remember.
    pub fn remember(&mut self, experience: Experience) {
        let familiarity = &mut self.familiarity[experience.slot()];
        *familiarity = (*familiarity + 1.0).min(MOST_FAMILIAR);
    }

    /// One field however much is remembered, so the unit panel's layout does
    /// not change with it.
    pub fn debug_field(&self) -> (&'static str, String) {
        let recalled: Vec<String> = Experience::ALL
            .iter()
            .filter(|&&experience| self.recalls(experience))
            .map(|&experience| format!("{} {:.2}", experience.name(), self.familiarity(experience)))
            .collect();
        (
            "recent memory",
            if recalled.is_empty() { "nothing".to_string() } else { recalled.join(", ") },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::background::Schedule;

    const FOOD: Experience = Experience::Taste(ItemKind::Food);

    #[test]
    fn every_experience_has_a_slot_of_its_own() {
        for (i, experience) in Experience::ALL.iter().enumerate() {
            assert_eq!(experience.slot(), i, "{}", experience.name());
        }
    }

    #[test]
    fn something_never_done_is_as_good_as_it_gets() {
        let recollection = Recollection::default();
        assert!(!recollection.recalls(FOOD));
        assert_eq!(recollection.novelty(FOOD), FRESH);
    }

    #[test]
    fn doing_something_again_straight_away_is_half_as_good_and_a_third_time_a_third() {
        let mut recollection = Recollection::default();
        recollection.remember(FOOD);
        assert_eq!(recollection.novelty(FOOD), 0.5);
        recollection.remember(FOOD);
        assert!((recollection.novelty(FOOD) - 1.0 / 3.0).abs() < 1e-6);
        assert_eq!(recollection.novelty(Experience::Entertainment), FRESH, "only food was had");
    }

    #[test]
    fn however_often_something_is_done_it_is_never_worth_nothing() {
        let mut recollection = Recollection::default();
        for _ in 0..100 {
            recollection.remember(Experience::Entertainment);
        }
        assert_eq!(recollection.novelty(Experience::Entertainment), 1.0 / (1.0 + MOST_FAMILIAR));
    }

    #[test]
    fn a_memory_fades_by_half_in_four_hours_and_is_gone_by_the_next_day() {
        let mut recollection = Recollection::default();
        recollection.remember(FOOD);
        recollection.run(HOURS_TO_HALF_FORGET * HOUR);
        assert!((recollection.familiarity(FOOD) - 0.5).abs() < 1e-4, "{}", recollection.familiarity(FOOD));
        recollection.run(6.0 * HOUR);
        assert!(recollection.recalls(FOOD), "still remembered ten hours on");
        recollection.run(8.0 * HOUR);
        assert!(!recollection.recalls(FOOD), "a day later it is new again");
        assert_eq!(recollection.novelty(FOOD), FRESH);
    }

    /// The same forgetting whether a tick is two minutes or the whole stretch
    /// is one step: fading is a rate, not a count of calls.
    #[test]
    fn forgetting_in_many_small_steps_comes_to_the_same_as_one_big_one() {
        let mut once = Recollection::default();
        once.remember(FOOD);
        let mut often = once;
        once.run(3.0 * HOUR);
        for _ in 0..90 {
            often.run(2.0 * 60.0);
        }
        assert!((once.familiarity(FOOD) - often.familiarity(FOOD)).abs() < 1e-4);
    }

    /// Run in the background, a memory fades as it would have run every tick
    /// — to within a run's worth of the time.
    #[test]
    fn fading_in_the_background_comes_to_what_fading_every_tick_does() {
        let dt = 120.0 / 64.0;
        let mut every_tick = Recollection::default();
        every_tick.remember(FOOD);
        let mut background = every_tick;
        let mut schedule = Schedule::new(3);
        let ticks = (3.0 * HOUR / dt) as u64;
        for _ in 0..ticks {
            every_tick.run(dt);
            schedule.tick(dt).run(&mut background);
        }
        let (exact, late) = (every_tick.familiarity(FOOD), background.familiarity(FOOD));
        assert!(late >= exact, "never more forgotten than the time allows: {late} vs {exact}");
        let a_run = 0.5f32.powf(Priority::VeryLow.period() as f32 * dt / (HOURS_TO_HALF_FORGET * HOUR));
        assert!(late * a_run <= exact * 1.0001, "and at most one run behind: {late} vs {exact}");
    }

    #[test]
    fn water_the_toilet_and_sleep_are_nothing_to_tire_of() {
        assert_eq!(Experience::of(Event::Ingested(ItemKind::Water)), None);
        assert_eq!(Experience::of(Event::Relieved), None);
        assert_eq!(Experience::of(Event::Slept { world_seconds: HOUR }), None);
        assert_eq!(Experience::of(Event::Ingested(ItemKind::Food)), Some(FOOD));
        assert_eq!(Experience::of(Event::Entertained), Some(Experience::Entertainment));
    }

    #[test]
    fn the_debug_view_is_one_field_whatever_is_remembered() {
        let mut recollection = Recollection::default();
        assert_eq!(recollection.debug_field(), ("recent memory", "nothing".to_string()));
        recollection.remember(FOOD);
        recollection.remember(Experience::Entertainment);
        assert_eq!(recollection.debug_field(), ("recent memory", "food 1.00, entertainment 1.00".to_string()));
    }
}
