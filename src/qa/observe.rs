//! Watching a world: what an `observe` step records, and how `expect_stat`
//! reads a stat.
//!
//! A measurement says how long a tick took; an observation says what the
//! ticks *did* to the people in them. Every unit with a body is sampled at a
//! fixed cadence — each stat the debug menu shows, and the goal it was
//! pursuing — and the record is written out two ways:
//!
//! * `<name>.csv`, a row per unit per sample: everything, for a spreadsheet or
//!   a plot.
//! * `<name>.json`, a row per sample: the crowd's average of every stat and
//!   how many were on each goal — what `tools/qa.py` prints, so a run reads as
//!   a day going by.
//!
//! Plain data over [`GameState`], with nothing of Bevy in it: the driver in
//! `qa::mod` decides when to sample and where the files go.

use serde_json::{json, Map, Value};

use crate::sim::biology::Stats;
use crate::sim::{GameEntity, GameState};

/// One unit, at one sample.
struct Row {
    /// Its name, falling back to its id.
    unit: String,
    goal: &'static str,
    /// In [`Stats::fields`] order.
    values: Vec<f32>,
}

/// The crowd at one moment.
struct Sample {
    /// Ticks since the observation started.
    tick: u32,
    /// World hours since the world opened.
    hours: f64,
    /// The time of day, `day 1  14:00:00`.
    clock: String,
    rows: Vec<Row>,
}

/// Everything one `observe` step saw.
pub struct Observation {
    name: String,
    /// The names of the stats, in [`Stats::fields`] order: every row's
    /// `values` lines up with these.
    stats: Vec<&'static str>,
    samples: Vec<Sample>,
}

impl Observation {
    pub fn new(name: &str) -> Observation {
        Observation {
            name: name.to_string(),
            stats: stat_names(),
            samples: Vec::new(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Record the world as it is, `tick` ticks into the observation.
    pub fn sample(&mut self, state: &GameState, tick: u32) {
        let rows = state
            .entities()
            .in_spawn_order()
            .into_iter()
            .filter_map(|uid| state.entities().get(uid))
            .filter_map(|entity| {
                let biology = entity.biology()?;
                Some(Row {
                    unit: entity.display_name().unwrap_or_else(|| entity.uid().to_string()),
                    goal: entity.current_goal().map_or("-", |goal| goal.name()),
                    values: biology.stats().fields().into_iter().map(|(_, value)| value).collect(),
                })
            })
            .collect();
        let clock = state.clock();
        self.samples.push(Sample {
            tick,
            hours: clock.elapsed() / crate::sim::clock::HOUR as f64,
            clock: clock.to_string(),
            rows,
        });
    }

    /// One line for the log: when, how many, and the crowd's averages.
    pub fn line(&self) -> String {
        let Some(sample) = self.samples.last() else {
            return format!("{:?}: nothing sampled", self.name);
        };
        let means = means(&self.stats, sample);
        let stats: Vec<String> = self
            .stats
            .iter()
            .zip(means)
            .map(|(name, mean)| format!("{name} {mean:.1}"))
            .collect();
        format!(
            "{:?} at {} ({} units): {}",
            self.name,
            sample.clock,
            sample.rows.len(),
            stats.join(", ")
        )
    }

    /// Every unit at every sample, one row each.
    pub fn to_csv(&self) -> String {
        let mut csv = format!("tick,hours,clock,unit,goal,{}\n", self.stats.join(","));
        for sample in &self.samples {
            for row in &sample.rows {
                let values: Vec<String> = row.values.iter().map(|value| format!("{value:.2}")).collect();
                csv.push_str(&format!(
                    "{},{:.3},{},{},{},{}\n",
                    sample.tick,
                    sample.hours,
                    sample.clock.trim(),
                    row.unit.replace(',', " "),
                    row.goal,
                    values.join(",")
                ));
            }
        }
        csv
    }

    /// The crowd per sample: averages and goals.
    pub fn to_json(&self) -> String {
        let samples: Vec<Value> = self
            .samples
            .iter()
            .map(|sample| {
                let mut mean = Map::new();
                for (name, value) in self.stats.iter().zip(means(&self.stats, sample)) {
                    mean.insert(name.to_string(), json!(value));
                }
                let mut goals = Map::new();
                for row in &sample.rows {
                    let count = goals.get(row.goal).and_then(Value::as_u64).unwrap_or(0);
                    goals.insert(row.goal.to_string(), json!(count + 1));
                }
                json!({
                    "tick": sample.tick,
                    "hours": sample.hours,
                    "clock": sample.clock,
                    "units": sample.rows.len(),
                    "mean": mean,
                    "goals": goals,
                })
            })
            .collect();
        let report = json!({ "name": self.name, "stats": self.stats, "samples": samples });
        serde_json::to_string_pretty(&report).expect("plain data serialises")
    }
}

/// The crowd's average of every stat at one sample; zeros for nobody.
fn means(stats: &[&'static str], sample: &Sample) -> Vec<f32> {
    let count = sample.rows.len().max(1) as f32;
    (0..stats.len())
        .map(|i| sample.rows.iter().map(|row| row.values[i]).sum::<f32>() / count)
        .collect()
}

/// Every stat's name, in [`Stats::fields`] order.
fn stat_names() -> Vec<&'static str> {
    Stats::named().to_vec()
}

/// A stat of one unit, by the name the debug menu gives it.
pub fn stat_of(entity: &dyn GameEntity, stat: &str) -> Result<f32, String> {
    let biology = entity
        .biology()
        .ok_or(format!("{} has no body, so no {stat}", entity.uid()))?;
    biology
        .stats()
        .fields()
        .into_iter()
        .find(|(name, _)| *name == stat)
        .map(|(_, value)| value)
        .ok_or_else(|| unknown_stat(stat))
}

/// A stat averaged over every unit with a body. An error for a world of
/// nobody, rather than a zero that would pass any `max`.
pub fn crowd_stat(state: &GameState, stat: &str) -> Result<f32, String> {
    if !Stats::named().contains(&stat) {
        return Err(unknown_stat(stat));
    }
    let values: Vec<f32> = state
        .entities()
        .iter()
        .filter(|entity| entity.biology().is_some())
        .map(|entity| stat_of(entity, stat))
        .collect::<Result<_, _>>()?;
    if values.is_empty() {
        return Err(format!("nobody in the world has a body, so there is no {stat} to average"));
    }
    Ok(values.iter().sum::<f32>() / values.len() as f32)
}

fn unknown_stat(stat: &str) -> String {
    format!("no stat is called {stat:?}; there are: {}", Stats::named().join(", "))
}
