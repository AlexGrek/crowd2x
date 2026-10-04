//! Background work: what a unit keeps up with **on a cadence** rather than on
//! every tick — a [`BackgroundTask`] run at its [`Priority`], timed by the
//! unit's own [`Schedule`].
//!
//! Most of what a unit does has to happen every tick: a walk moves a little
//! each one, a need that crosses its threshold has to be noticed the tick it
//! does. Some upkeep does not. A memory fading over hours is the same whether
//! it is brought up to date every tick or every thirty-seventh, and doing it
//! every tick is a crowd's worth of arithmetic per tick for a difference
//! nobody could see. That kind of work is a background task: it is handed
//! all the time that went by since it last ran, and runs only when due.
//!
//! # Priorities
//!
//! | priority | runs every | world time between runs |
//! | --- | --- | --- |
//! | [`Priority::High`] | 3rd tick | ~6 seconds |
//! | [`Priority::Med`] | 7th tick | ~13 seconds |
//! | [`Priority::Low`] | 17th tick | ~32 seconds |
//! | [`Priority::VeryLow`] | 37th tick | ~69 seconds |
//!
//! A task's priority is how stale it may be allowed to go, so pick it by what
//! a run that comes late would get wrong: a rate over hours is `VeryLow`,
//! anything a decision might read the tick it changes is not background work
//! at all.
//!
//! # Every unit keeps its own time
//!
//! A unit carries a [`Schedule`]: a seed rolled when it spawns, and one
//! countdown per priority. The seed decides where in its period each
//! countdown starts, so two units — and two priorities of one unit — run
//! their background work on different ticks, and on any one tick roughly a
//! period's fraction of the crowd runs each priority. Never the whole crowd
//! every 37th tick and nobody in between, which would trade a steady cost for
//! a spike. The periods are primes, so one unit's priorities drift through
//! each other rather than lining up.
//!
//! Because the timers are the unit's own and only move when the unit does
//! ([`Schedule::tick`], from its `react`), they also say exactly how much time
//! the unit lived through: each countdown adds up the world time it was handed,
//! and a run is given that sum. A unit that spawned two ticks before its first
//! run is handed two ticks; a frozen unit's `react` is skipped, so its timers
//! stop with the rest of it and no time passes for its background work either.
//!
//! Plain Rust, inline, `Copy`, no allocation, deterministic: the seed comes
//! from the spawn RNG, and after that a schedule is a function of how many
//! times it has been ticked.

/// How often a background task runs. The discriminant is its period, in ticks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Priority {
    /// Every 3rd tick.
    High = 3,
    /// Every 7th tick.
    Med = 7,
    /// Every 17th tick.
    Low = 17,
    /// Every 37th tick.
    VeryLow = 37,
}

impl Priority {
    pub const COUNT: usize = 4;
    pub const ALL: [Priority; Priority::COUNT] = [Priority::High, Priority::Med, Priority::Low, Priority::VeryLow];

    /// Ticks from one run to the next.
    pub const fn period(self) -> u8 {
        self as u8
    }

    pub const fn name(self) -> &'static str {
        match self {
            Priority::High => "high",
            Priority::Med => "med",
            Priority::Low => "low",
            Priority::VeryLow => "very low",
        }
    }

    /// Its slot in a [`Schedule`].
    const fn slot(self) -> usize {
        match self {
            Priority::High => 0,
            Priority::Med => 1,
            Priority::Low => 2,
            Priority::VeryLow => 3,
        }
    }
}

/// Something a unit keeps up with in the background.
pub trait BackgroundTask {
    /// How often it runs.
    const PRIORITY: Priority;

    /// Bring it up to date: `elapsed` **world** seconds went by since it last
    /// ran (or since the unit arrived, the first time).
    fn run(&mut self, elapsed: f32);
}

/// One unit's background timers: its own seed, and a countdown per priority.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Schedule {
    /// Rolled at spawn. Where in its period each countdown started.
    seed: u64,
    /// Ticks until each priority is next due, by [`Priority::slot`]. Never 0
    /// between ticks: a countdown that reaches it is due and starts again.
    left: [u8; Priority::COUNT],
    /// World seconds each priority's tasks have not been handed yet.
    pending: [f32; Priority::COUNT],
}

impl Schedule {
    /// A schedule for a unit whose seed is `seed`. Each priority is first due
    /// somewhere within its first period, at a point the seed picks — a
    /// different byte of it per priority, so one unit's are independent.
    pub fn new(seed: u64) -> Schedule {
        let mut left = [0; Priority::COUNT];
        for priority in Priority::ALL {
            let byte = (seed >> (8 * priority.slot())) as u8;
            left[priority.slot()] = 1 + byte % priority.period();
        }
        Schedule {
            seed,
            left,
            pending: [0.0; Priority::COUNT],
        }
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Ticks until `priority` is next due.
    pub fn ticks_until(&self, priority: Priority) -> u8 {
        self.left[priority.slot()]
    }

    /// One tick of this unit's life, `dt` **world** seconds of it: every
    /// countdown moves on, and what is due now is returned with the time each
    /// due priority's tasks are owed.
    pub fn tick(&mut self, dt: f32) -> Due {
        let mut due = Due::NOTHING;
        for priority in Priority::ALL {
            let slot = priority.slot();
            self.pending[slot] += dt;
            self.left[slot] -= 1;
            if self.left[slot] == 0 {
                due.elapsed[slot] = Some(self.pending[slot]);
                self.pending[slot] = 0.0;
                self.left[slot] = priority.period();
            }
        }
        due
    }

    /// Ticks until each priority is due, for the debug menu. Allocates; never
    /// asked in a tick.
    pub fn debug_field(&self) -> (&'static str, String) {
        let left: Vec<String> = Priority::ALL
            .iter()
            .map(|&priority| format!("{} {}", priority.name(), self.ticks_until(priority)))
            .collect();
        ("background in", left.join(", "))
    }
}

/// What one unit's [`Schedule`] says is due on one tick, and how much time
/// each due priority's tasks are to be handed.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Due {
    elapsed: [Option<f32>; Priority::COUNT],
}

impl Due {
    /// Nothing due. What a test hands a body that should not be kept up.
    pub const NOTHING: Due = Due {
        elapsed: [None; Priority::COUNT],
    };

    pub fn is_due(&self, priority: Priority) -> bool {
        self.elapsed[priority.slot()].is_some()
    }

    /// Run `task` if its priority is due, handing it the time since it last
    /// ran.
    pub fn run<T: BackgroundTask>(&self, task: &mut T) {
        if let Some(elapsed) = self.elapsed[T::PRIORITY.slot()] {
            task.run(elapsed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::rng::mix;

    /// Adds up the time it was handed, and counts its runs.
    #[derive(Default)]
    struct Tally {
        runs: u32,
        elapsed: f32,
    }

    impl BackgroundTask for Tally {
        const PRIORITY: Priority = Priority::Low;
        fn run(&mut self, elapsed: f32) {
            self.runs += 1;
            self.elapsed += elapsed;
        }
    }

    #[test]
    fn the_periods_are_the_ones_asked_for() {
        let periods: Vec<u8> = Priority::ALL.iter().map(|p| p.period()).collect();
        assert_eq!(periods, [3, 7, 17, 37]);
        for (i, priority) in Priority::ALL.iter().enumerate() {
            assert_eq!(priority.slot(), i, "{}", priority.name());
        }
    }

    #[test]
    fn each_priority_runs_once_per_period_once_it_has_started() {
        for seed in [0, 1, 0xdead_beef, u64::MAX] {
            let mut schedule = Schedule::new(seed);
            for priority in Priority::ALL {
                assert!((1..=priority.period()).contains(&schedule.ticks_until(priority)));
            }
            let mut runs = [0u32; Priority::COUNT];
            let ticks = 3 * 7 * 17 * 37;
            for _ in 0..ticks {
                let due = schedule.tick(1.0);
                for priority in Priority::ALL {
                    runs[priority.slot()] += due.is_due(priority) as u32;
                }
            }
            for priority in Priority::ALL {
                assert_eq!(runs[priority.slot()], ticks / priority.period() as u32, "{}", priority.name());
            }
        }
    }

    /// The time handed out adds up to the time lived through, the first run
    /// included — a unit that arrived a tick before its first run is handed
    /// one tick, not a period.
    #[test]
    fn a_task_is_handed_exactly_the_time_the_unit_lived_through() {
        let mut schedule = Schedule::new(12345);
        let mut tally = Tally::default();
        let first = schedule.ticks_until(Priority::Low);
        let mut lived = 0.0;
        let mut at_first_run = None;
        for tick in 1..=1000u32 {
            let dt = if tick % 2 == 0 { 2.0 } else { 0.5 };
            lived += dt;
            schedule.tick(dt).run(&mut tally);
            if tally.runs == 1 && at_first_run.is_none() {
                at_first_run = Some((tick, tally.elapsed, lived));
            }
        }
        let (tick, handed, lived_then) = at_first_run.unwrap();
        assert_eq!(tick, first as u32, "first due when its countdown said");
        assert_eq!(handed, lived_then, "and handed only the ticks since spawning");
        assert_eq!(tally.elapsed + schedule.pending[Priority::Low.slot()], lived);
    }

    /// The point of a seed each: a crowd's background work is spread over the
    /// ticks of a period, not piled on one of them.
    #[test]
    fn a_crowd_takes_its_turns_on_different_ticks() {
        let mut crowd: Vec<Schedule> = (1..=3700).map(|n| Schedule::new(mix(n))).collect();
        let priority = Priority::VeryLow;
        let busiest = (0..priority.period())
            .map(|_| crowd.iter_mut().map(|schedule| schedule.tick(1.0)).filter(|due| due.is_due(priority)).count())
            .max()
            .unwrap();
        // 100 a tick if perfectly even; a pile-up would be all 3700.
        assert!(busiest < 200, "{busiest} of {} on one tick", crowd.len());
    }

    /// And one unit's priorities do not all fall on the same tick.
    #[test]
    fn one_unit_s_priorities_start_at_different_points_of_their_periods() {
        let lined_up = (1..=1000)
            .map(|n| Schedule::new(mix(n)))
            .filter(|s| Priority::ALL.iter().all(|&p| s.ticks_until(p) == s.ticks_until(Priority::High)))
            .count();
        assert!(lined_up < 50, "{lined_up} of 1000 units have every priority due together");
    }

    #[test]
    fn nothing_is_due_in_nothing() {
        let mut tally = Tally::default();
        Due::NOTHING.run(&mut tally);
        assert_eq!(tally.runs, 0);
        assert!(Priority::ALL.iter().all(|&p| !Due::NOTHING.is_due(p)));
    }

    #[test]
    fn the_debug_view_counts_down_every_priority() {
        let mut schedule = Schedule::new(0);
        assert_eq!(schedule.debug_field(), ("background in", "high 1, med 1, low 1, very low 1".to_string()));
        let _ = schedule.tick(1.0);
        assert_eq!(schedule.debug_field().1, "high 3, med 7, low 17, very low 37");
    }
}
