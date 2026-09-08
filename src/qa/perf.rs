//! What a performance test measures, and how it judges what it measured.
//!
//! The rest of the QA harness answers *does the right thing happen*. This
//! answers *how long did it take*, which is a different question with a
//! different failure mode: a number is not a pass or a fail on its own, and a
//! test that turns one into a pass or a fail on the wrong grounds is worse
//! than no test, because it fails on a loaded machine and gets muted.
//!
//! So this module is built around two ideas.
//!
//! **A measurement is the deliverable.** Every `measure` step records a
//! [`Measurement`] whether or not anything asserts on it, the run writes them
//! all out as JSON, and `tools/qa.py` prints them. A perf test with no
//! assertion in it is still doing its job; it is a recorded number to compare
//! the next one against.
//!
//! **Scaling is the assertion worth making.** How many microseconds a tick
//! takes depends on the machine, the build profile and what else is running.
//! How that time grows with the size of the crowd does not: doubling the
//! entities should roughly double the work, and the failure this project
//! actually has to guard against — the earlier prototype's O(parts x workers)
//! loop, an unbudgeted per-agent search — shows up as cost *per entity* going
//! up with the crowd. [`scaling`] compares two measurements on exactly that,
//! and it is the assertion a perf test should reach for first.
//! [`Budget`] is the blunter one: a wall, set well above what the machine
//! does today, that catches something becoming ten times slower rather than
//! ten percent.
//!
//! The two assertions read different statistics on purpose. A budget asks what
//! the game *typically* does, so it judges the median. Scaling asks what the
//! code does, so it divides the *fastest* samples: noise only ever adds time,
//! and a ratio of two medians inherits the noise of both — with medians, three
//! consecutive runs of an unchanged build gave 0.52x, 0.68x and 1.60x, and the
//! third failed a check the code had not earned.
//!
//! Everything here is plain Rust — the same rule `src/map/` and `src/sim/`
//! follow — so the statistics and the judgements are `cargo test`able without
//! an `App`, a window or a stopwatch.

use serde::Serialize;

/// What was being timed.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Measured {
    /// One processing pass of the simulation, called directly. No renderer,
    /// no window, no frame — just [`crate::sim::process_pass`].
    Tick,
    /// One frame of the whole app: the simulation on its fixed step, the
    /// sprite sync, the UI, and the render.
    Frame,
}

impl Measured {
    pub fn unit(self) -> &'static str {
        match self {
            Measured::Tick => "tick",
            Measured::Frame => "frame",
        }
    }
}

/// One recorded measurement: what was timed, how big the world was, and the
/// distribution of the samples.
///
/// The whole distribution rather than a mean, because the mean is the one
/// number that hides the thing worth seeing. A tick that is usually 200us and
/// occasionally 40ms is a stutter the player feels and a mean of 1ms does not
/// mention.
#[derive(Serialize, Clone, Debug)]
pub struct Measurement {
    pub name: String,
    pub measured: Measured,
    /// How many entities were alive while this was measured. The denominator
    /// for [`Measurement::per_entity_us`], and the reason a measurement is
    /// comparable to another one at all.
    pub entities: usize,
    pub samples: usize,
    pub total_ms: f64,
    pub mean_ms: f64,
    /// The middle sample. What a [`Budget`] is judged against: it is what the
    /// game typically does, which is the question a budget is asking.
    pub median_ms: f64,
    pub p95_ms: f64,
    pub worst_ms: f64,
    /// The fastest sample, and the one [`scaling`] is computed from.
    ///
    /// Interference only ever *adds* time — the OS scheduling the process out,
    /// the render thread taking a core, another test's compile — so the
    /// quickest tick observed is the one that got closest to running
    /// undisturbed, and it is far steadier between runs than the median. That
    /// matters here because a ratio of two noisy numbers is much noisier than
    /// either: with medians, an unchanged build produced 0.52x, 0.68x and 1.60x
    /// on three consecutive runs of the same test, and the third failed.
    pub best_ms: f64,
    /// Microseconds per entity, from [`Measurement::best_ms`]. `None` for an
    /// empty world, where the question does not mean anything.
    pub per_entity_us: Option<f64>,
}

impl Measurement {
    /// Summarise a set of samples, in milliseconds.
    ///
    /// Sorts a copy: the caller's buffer is a live recording and the order it
    /// was taken in is worth keeping.
    pub fn of(name: &str, measured: Measured, entities: usize, samples: &[f64]) -> Measurement {
        let mut sorted = samples.to_vec();
        sorted.sort_by(f64::total_cmp);

        let count = sorted.len();
        let total: f64 = sorted.iter().sum();
        let best = sorted.first().copied().unwrap_or(0.0);
        Measurement {
            name: name.to_string(),
            measured,
            entities,
            samples: count,
            total_ms: total,
            mean_ms: if count == 0 { 0.0 } else { total / count as f64 },
            median_ms: percentile(&sorted, 0.5),
            p95_ms: percentile(&sorted, 0.95),
            worst_ms: sorted.last().copied().unwrap_or(0.0),
            best_ms: best,
            per_entity_us: (entities > 0).then(|| best * 1000.0 / entities as f64),
        }
    }

    /// One line for a log or a terminal.
    pub fn line(&self) -> String {
        let per_entity = match self.per_entity_us {
            Some(us) => format!(", {us:.2}us/entity"),
            None => String::new(),
        };
        format!(
            "{:?}: {} {}s, {} entities - best {:.3}ms, median {:.3}ms, mean {:.3}ms, p95 {:.3}ms, worst {:.3}ms{}",
            self.name,
            self.samples,
            self.measured.unit(),
            self.entities,
            self.best_ms,
            self.median_ms,
            self.mean_ms,
            self.p95_ms,
            self.worst_ms,
            per_entity,
        )
    }
}

/// The sample at a fraction of the way through a sorted set.
///
/// Nearest-rank, not interpolated: these are timings, and reporting a duration
/// that was never actually observed to make a percentile prettier is not worth
/// the confusion when it is compared against a raw sample.
fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (fraction * sorted.len() as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

/// A duration budget: this measurement's **median** sample must come in under
/// `ms`.
///
/// The median rather than the mean or the worst, on purpose. One hitch — the
/// OS scheduling the process out, a screenshot being written, a texture
/// arriving — lands in every run and would decide the test if the worst sample
/// did. The worst is still recorded, and a run where it matters is a run to
/// look at rather than one to fail automatically.
///
/// Set a budget as a **wall, not a target**: several times what the machine
/// does today, so it catches a change of order rather than the difference
/// between a laptop on battery and a desktop. The scaling check below is what
/// catches the smaller regressions.
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    pub ms: f64,
}

impl Budget {
    pub fn check(self, measurement: &Measurement) -> Result<(), String> {
        if measurement.samples == 0 {
            return Err(format!("{:?} has no samples to judge", measurement.name));
        }
        if measurement.median_ms <= self.ms {
            return Ok(());
        }
        Err(format!(
            "{:?}: median {} took {:.3}ms, over the {:.3}ms budget (mean {:.3}ms, p95 {:.3}ms, worst {:.3}ms)",
            measurement.name,
            measurement.measured.unit(),
            measurement.median_ms,
            self.ms,
            measurement.mean_ms,
            measurement.p95_ms,
            measurement.worst_ms,
        ))
    }
}

/// How much more each entity costs in the bigger world than in the smaller
/// one.
///
/// Computed from each measurement's *fastest* sample — see
/// [`Measurement::best_ms`] — because this is a ratio, and a ratio of two
/// medians inherits the noise of both.
///
/// `1.0` is perfectly linear: ten times the crowd for ten times the work.
/// Below `1.0` is normal and healthy — fixed per-tick costs are amortised over
/// more entities. Above `1.0` means each entity is getting more expensive as
/// the crowd grows, which is the signature of a per-entity scan over all the
/// others, and it is the number this project has the most reason to watch.
///
/// `Err` for the cases where the ratio would not mean anything: an empty
/// world, or two measurements of the same size (nothing was varied), or two
/// that are not measuring the same thing.
pub fn scaling(from: &Measurement, to: &Measurement) -> Result<f64, String> {
    if from.measured != to.measured {
        return Err(format!(
            "{:?} measures {}s and {:?} measures {}s; they cannot be compared",
            from.name,
            from.measured.unit(),
            to.name,
            to.measured.unit(),
        ));
    }
    let (Some(small), Some(large)) = (from.per_entity_us, to.per_entity_us) else {
        return Err(format!(
            "{:?} ({} entities) and {:?} ({} entities): an empty world has no per-entity cost",
            from.name, from.entities, to.name, to.entities,
        ));
    };
    if to.entities <= from.entities {
        return Err(format!(
            "{:?} has {} entities and {:?} has {}; the second has to be the bigger world",
            from.name, from.entities, to.name, to.entities,
        ));
    }
    if small <= 0.0 {
        return Err(format!(
            "{:?} measured 0ms per {}; the world is too small or the clock too coarse to compare against",
            from.name,
            from.measured.unit(),
        ));
    }
    Ok(large / small)
}

/// Check that growing the crowd did not make each entity more expensive than
/// `slack` times what it was.
///
/// How much slack depends on where the two crowds sit relative to the cache. A
/// hundred entities fit in L1 and a thousand do not, so that pair moves by
/// 1.3-1.8x between runs of an unchanged build and needs a slack around `3.0`;
/// two crowds that are both past the cache — a thousand against four thousand
/// — hold to within a few percent and can be held to `1.5`. Both still catch
/// the thing worth catching, because a quadratic is 10x over the same range.
pub fn check_scaling(from: &Measurement, to: &Measurement, slack: f64) -> Result<f64, String> {
    let ratio = scaling(from, to)?;
    if ratio <= slack {
        return Ok(ratio);
    }
    Err(format!(
        "cost per entity grew {ratio:.2}x from {:?} ({} entities, {:.2}us each) to {:?} ({} entities, {:.2}us each), over the {slack:.2}x allowed - something is scaling worse than linearly",
        from.name,
        from.entities,
        from.per_entity_us.unwrap_or_default(),
        to.name,
        to.entities,
        to.per_entity_us.unwrap_or_default(),
    ))
}

/// Everything one run measured.
///
/// Written out whole, so a run is a record and not just a verdict: the numbers
/// are the point, and the assertions are a floor under them.
#[derive(Serialize, Clone, Debug)]
pub struct Report {
    /// The test that produced these.
    pub test: String,
    /// `debug` or `release`. Recorded because it is the single biggest factor
    /// in every number below — this crate builds at `opt-level = 1` in debug —
    /// and comparing across profiles is meaningless.
    pub profile: &'static str,
    /// Whether the window was waiting for the display. Recorded for the same
    /// reason as the profile: with it on, every frame measurement is capped at
    /// the refresh rate and says nothing about the headroom.
    pub vsync: bool,
    pub measurements: Vec<Measurement>,
}

impl Report {
    pub fn new(test: &str, vsync: bool) -> Report {
        Report {
            test: test.to_string(),
            profile: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
            vsync,
            measurements: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.measurements.is_empty()
    }

    pub fn push(&mut self, measurement: Measurement) {
        self.measurements.push(measurement);
    }

    /// A measurement by the name the script gave it.
    ///
    /// `Err` names the ones that exist, because the usual mistake is an
    /// assertion that refers to a measurement by a name it does not have, or
    /// one taken later in the file.
    pub fn find(&self, name: &str) -> Result<&Measurement, String> {
        self.measurements
            .iter()
            .find(|measurement| measurement.name == name)
            .ok_or_else(|| {
                let taken: Vec<&str> = self
                    .measurements
                    .iter()
                    .map(|measurement| measurement.name.as_str())
                    .collect();
                if taken.is_empty() {
                    format!("nothing has been measured yet, so {name:?} cannot be asserted on")
                } else {
                    format!(
                        "no measurement called {name:?}; this run has taken: {}",
                        taken.join(", ")
                    )
                }
            })
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|error| format!("{{\"error\": \"{error}\"}}"))
    }
}

/// Where to put `count` entities on a map, spread over the cells they can
/// stand on.
///
/// Even coverage rather than a heap in one corner: a crowd packed into one
/// cell measures the wrong thing once anything is spatial, and a crowd stacked
/// on one sprite position measures the wrong thing for the renderer too.
/// Deterministic, so a perf run is comparable with the one before it.
///
/// Asking for more entities than there are cells is allowed and wraps around,
/// because a dense crowd is a real thing to want to measure.
pub fn spread(passable: &[crate::map::Point], count: usize) -> Vec<crate::map::Point> {
    if passable.is_empty() || count == 0 {
        return Vec::new();
    }
    // Walk the cells in strides big enough to reach the far side of the map,
    // so a small crowd on a big map is spread across it rather than filling
    // the first few rows.
    let stride = (passable.len() / count).max(1);
    (0..count)
        .map(|i| passable[(i * stride) % passable.len()])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::Point;

    fn measurement(name: &str, entities: usize, samples: &[f64]) -> Measurement {
        Measurement::of(name, Measured::Tick, entities, samples)
    }

    #[test]
    fn a_measurement_summarises_the_samples_it_was_given() {
        let m = measurement("ten", 10, &[3.0, 2.0, 1.0, 4.0, 100.0]);
        assert_eq!(m.samples, 5);
        assert_eq!(m.total_ms, 110.0);
        assert_eq!(m.mean_ms, 22.0);
        assert_eq!(m.median_ms, 3.0);
        assert_eq!(m.worst_ms, 100.0);
        assert_eq!(m.best_ms, 1.0);
        // The fastest tick, 1ms, across ten entities, in microseconds.
        assert_eq!(m.per_entity_us, Some(100.0));
    }

    /// The mean is the number that hides a stutter; the point of keeping the
    /// distribution is that one 100ms tick is visible next to a 3ms median.
    #[test]
    fn one_terrible_sample_moves_the_mean_and_not_the_median() {
        let steady = measurement("steady", 1, &[3.0, 3.0, 3.0, 3.0, 3.0]);
        let hitched = measurement("hitched", 1, &[3.0, 3.0, 3.0, 3.0, 100.0]);
        assert_eq!(steady.median_ms, hitched.median_ms);
        assert!(hitched.mean_ms > steady.mean_ms * 5.0);
        assert_eq!(hitched.worst_ms, 100.0);
    }

    #[test]
    fn an_empty_world_has_no_per_entity_cost() {
        assert_eq!(measurement("empty", 0, &[1.0]).per_entity_us, None);
    }

    #[test]
    fn a_measurement_with_no_samples_does_not_divide_by_zero() {
        let m = measurement("nothing", 5, &[]);
        assert_eq!(m.samples, 0);
        assert_eq!(m.mean_ms, 0.0);
        assert_eq!(m.median_ms, 0.0);
        assert_eq!(m.worst_ms, 0.0);
        assert_eq!(m.best_ms, 0.0);
    }

    /// The reason scaling divides the fastest samples: interference only adds
    /// time, so a disturbed run must not read as a code regression.
    #[test]
    fn a_measurement_taken_on_a_busy_machine_still_compares_as_itself() {
        let quiet = measurement("1000 humans", 1000, &[10.0, 10.0, 10.1, 10.2]);
        // Four times the crowd for four times the work — perfectly linear —
        // measured while the machine was busy with something else, so most of
        // the samples came out slower than the work was.
        let busy = measurement("4000 humans", 4000, &[40.0, 72.0, 90.0, 120.0]);
        assert!(busy.median_ms > quiet.median_ms * 4.0 * 1.5);
        // The medians would call that a 1.9x regression. The fastest samples
        // call it what it is.
        assert_eq!(scaling(&quiet, &busy), Ok(1.0));
        assert!(check_scaling(&quiet, &busy, 1.5).is_ok());
    }

    /// A budget asks what the game typically does, so it reads the median and
    /// not the fastest sample: best-of would pass a build that hit its budget
    /// once and missed it every other tick.
    #[test]
    fn a_budget_is_judged_on_the_median_and_not_on_the_worst_sample() {
        let hitched = measurement("hitched", 1, &[1.0, 1.0, 1.0, 1.0, 50.0]);
        assert!(Budget { ms: 2.0 }.check(&hitched).is_ok());
        let slow = measurement("slow", 1, &[9.0, 9.0, 9.0]);
        let error = Budget { ms: 2.0 }.check(&slow).unwrap_err();
        assert!(error.contains("9.000ms"), "{error}");
        assert!(error.contains("2.000ms budget"), "{error}");
    }

    #[test]
    fn a_budget_on_a_measurement_that_never_ran_is_a_failure_not_a_pass() {
        // Otherwise a measure step that recorded nothing would sail through
        // every budget in the file.
        let error = Budget { ms: 1.0 }
            .check(&measurement("nothing", 5, &[]))
            .unwrap_err();
        assert!(error.contains("no samples"), "{error}");
    }

    #[test]
    fn linear_work_costs_the_same_per_entity_however_big_the_crowd() {
        let small = measurement("100", 100, &[1.0]);
        let large = measurement("1000", 1000, &[10.0]);
        assert_eq!(scaling(&small, &large), Ok(1.0));
        assert!(check_scaling(&small, &large, 1.2).is_ok());
    }

    /// The failure this project exists to avoid: each agent looking at every
    /// other one. Ten times the crowd, a hundred times the work.
    #[test]
    fn quadratic_work_is_caught_by_the_scaling_check() {
        let small = measurement("100", 100, &[1.0]);
        let large = measurement("1000", 1000, &[100.0]);
        assert_eq!(scaling(&small, &large), Ok(10.0));
        let error = check_scaling(&small, &large, 1.5).unwrap_err();
        assert!(error.contains("10.00x"), "{error}");
        assert!(error.contains("worse than linearly"), "{error}");
    }

    #[test]
    fn amortising_a_fixed_cost_over_a_bigger_crowd_is_not_a_regression() {
        // 0.5ms of per-tick overhead plus 10us an entity: the per-entity cost
        // falls as the crowd grows, and that must not read as a failure.
        let small = measurement("100", 100, &[1.5]);
        let large = measurement("1000", 1000, &[10.5]);
        let ratio = scaling(&small, &large).expect("comparable");
        assert!(ratio < 1.0, "{ratio}");
        assert!(check_scaling(&small, &large, 1.0).is_ok());
    }

    #[test]
    fn comparing_a_tick_with_a_frame_is_refused_rather_than_answered() {
        let ticks = Measurement::of("ticks", Measured::Tick, 100, &[1.0]);
        let frames = Measurement::of("frames", Measured::Frame, 1000, &[10.0]);
        let error = scaling(&ticks, &frames).unwrap_err();
        assert!(error.contains("cannot be compared"), "{error}");
    }

    #[test]
    fn a_comparison_that_varied_nothing_is_an_error() {
        let a = measurement("a", 100, &[1.0]);
        let b = measurement("b", 100, &[2.0]);
        let error = scaling(&a, &b).unwrap_err();
        assert!(error.contains("bigger world"), "{error}");
    }

    #[test]
    fn a_report_names_the_measurements_it_has_when_asked_for_one_it_does_not() {
        let mut report = Report::new("perf_simulation", true);
        assert!(report.find("early").unwrap_err().contains("nothing has been measured"));
        report.push(measurement("100 humans", 100, &[1.0]));
        assert_eq!(report.find("100 humans").expect("just pushed").entities, 100);
        let error = report.find("1000 humans").unwrap_err();
        assert!(error.contains("100 humans"), "{error}");
    }

    #[test]
    fn a_report_says_which_build_produced_it() {
        // The numbers are meaningless without it: this crate builds at
        // opt-level 1 in debug and 3 in release.
        let report = Report::new("x", false);
        assert!(matches!(report.profile, "debug" | "release"));
        assert!(report.to_json().contains("\"profile\""));
    }

    fn cells(width: i32, height: i32) -> Vec<Point> {
        (0..height)
            .flat_map(|y| (0..width).map(move |x| Point::new(x, y)))
            .collect()
    }

    #[test]
    fn a_crowd_is_spread_over_the_map_rather_than_piled_in_one_corner() {
        let places = spread(&cells(20, 20), 40);
        assert_eq!(places.len(), 40);
        let distinct: std::collections::HashSet<_> = places.iter().collect();
        assert_eq!(distinct.len(), 40, "every one of these should be its own cell");
        // ...and they reach the far end of the map, not just the first rows.
        assert!(places.iter().any(|place| place.y > 15), "{places:?}");
    }

    #[test]
    fn asking_for_more_entities_than_cells_stacks_them_instead_of_refusing() {
        let places = spread(&cells(4, 4), 40);
        assert_eq!(places.len(), 40);
        // Every cell used, and used evenly.
        let distinct: std::collections::HashSet<_> = places.iter().collect();
        assert_eq!(distinct.len(), 16);
    }

    #[test]
    fn a_map_with_nowhere_to_stand_gets_no_crowd() {
        assert!(spread(&[], 10).is_empty());
        assert!(spread(&cells(4, 4), 0).is_empty());
    }
}
