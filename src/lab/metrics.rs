//! The world's numbers, sampled: what the lab plots, prints and watches.
//!
//! A sample is a flat map of names to numbers, so a watch, a table and a CSV
//! column all address one the same way, by name:
//!
//! | name | what |
//! | --- | --- |
//! | `alive`, `alive.<kind>`, `died`, `lying`, `carried`, `cremated`, `jobless` | the statistics menu's people |
//! | `store.<prop>` | pieces in every store of a kind now: `store.steel rack`, `store.scrap bin`, ... |
//! | `in.<prop>`, `out.<prop>` | pieces ever put into / taken out of stores of that kind (from the log) |
//! | `scrap.heaps`, `scrap.left`, `scrap.nearest` | heaps and pieces left where collectors look (the navigation area); the nearest heap's distance from a scrap bin, in cells |
//! | `crops`, `crops.quality` | what is growing |
//! | `jobs.<job>` | how many are employed as it |
//! | `budget`, `cash`, `in debt` | money, in dollars |
//! | `goal.<goal>` | the living on each goal |
//! | `asleep`, `outside` | asleep anywhere; off the site |
//! | `<job>.on shift`, `<job>.<goal>` | workers whose shift is due, and what they are at |
//! | `<stat>.avg`, `<stat>.min` | the living humans' bodies: `health.min`, `stamina.min`, `hunger.avg`... |

use std::collections::BTreeMap;

use serde_json::json;

use crate::sim::{Clock, EntityType, FeatureKind, GameState};

use super::world::duration;

/// The world at one moment.
pub struct Sample {
    pub tick: u64,
    /// Watched seconds: the clock.
    pub elapsed: f64,
    pub values: BTreeMap<String, f64>,
}

impl Sample {
    pub fn clock(&self) -> Clock {
        Clock::after_watching(self.elapsed)
    }

    pub fn get(&self, name: &str) -> Option<f64> {
        self.values.get(name).copied()
    }

    pub fn to_json(&self) -> String {
        json!({ "tick": self.tick, "clock": self.clock().to_string(), "values": self.values }).to_string()
    }
}

/// Every sample, oldest first.
#[derive(Default)]
pub struct Series {
    pub samples: Vec<Sample>,
}

impl Series {
    pub fn push(&mut self, sample: Sample) {
        self.samples.push(sample);
    }

    pub fn last(&self) -> Option<&Sample> {
        self.samples.last()
    }

    /// The latest sample taken at or before `tick`.
    pub fn at_or_before(&self, tick: u64) -> Option<&Sample> {
        let after = self.samples.partition_point(|sample| sample.tick <= tick);
        after.checked_sub(1).map(|index| &self.samples[index])
    }

    /// Every name any sample has, in order.
    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = Vec::new();
        for sample in &self.samples {
            for name in sample.values.keys() {
                if let Err(at) = names.binary_search(&name.as_str()) {
                    names.insert(at, name);
                }
            }
        }
        names
    }

    pub fn to_csv(&self) -> String {
        let names = self.names();
        let mut out = String::from("tick,hours,clock");
        for name in &names {
            out.push(',');
            out.push_str(name);
        }
        out.push('\n');
        for sample in &self.samples {
            out.push_str(&format!("{},{:.3},{}", sample.tick, sample.tick as f64 / 1920.0, sample.clock()));
            for name in &names {
                out.push(',');
                if let Some(value) = sample.get(name) {
                    out.push_str(&number(value));
                }
            }
            out.push('\n');
        }
        out
    }

    /// A table of the names containing any of `patterns` — the headline for
    /// none: people, stores, flows, crops and money — a row every `step`
    /// ticks and the latest, the last `last` rows (0: all), at most `columns`
    /// columns.
    pub fn table(&self, patterns: &[&str], step: u64, last: usize, columns: usize) -> String {
        let Some(latest) = self.last() else {
            return "no samples yet".to_string();
        };
        let names: Vec<&str> = if patterns.is_empty() {
            self.names().into_iter().filter(|name| is_headline(name)).collect()
        } else {
            self.names()
                .into_iter()
                .filter(|name| patterns.iter().any(|pattern| name.contains(pattern)))
                .collect()
        };
        if names.is_empty() {
            return format!("no metric matches {patterns:?}; there are: {}", self.names().join(", "));
        }
        let shown = &names[..names.len().min(columns)];
        let mut rows: Vec<&Sample> = Vec::new();
        let mut next = 0;
        for sample in &self.samples {
            if sample.tick >= next || std::ptr::eq(sample, latest) {
                rows.push(sample);
                next = (sample.tick / step.max(1) + 1) * step.max(1);
            }
        }
        if last > 0 && rows.len() > last {
            rows.drain(..rows.len() - last);
        }
        let widths: Vec<usize> = shown.iter().map(|name| name.len().max(5)).collect();
        let mut out = format!("{:<20}", "clock");
        for (name, width) in shown.iter().zip(&widths) {
            out.push_str(&format!(" {name:>width$}"));
        }
        out.push('\n');
        for row in rows {
            out.push_str(&format!("{:<20}", row.clock().to_string()));
            for (name, width) in shown.iter().zip(&widths) {
                let cell = row.get(name).map_or("-".to_string(), number);
                out.push_str(&format!(" {cell:>width$}"));
            }
            out.push('\n');
        }
        if names.len() > shown.len() {
            out.push_str(&format!("... and {} more columns: {}\n", names.len() - shown.len(), names[shown.len()..].join(", ")));
        }
        let span = latest.tick - self.samples[0].tick;
        out.push_str(&format!("{} samples over {}", self.samples.len(), duration(span)));
        out
    }
}

/// Whether a name is one of the headline's: what a day line and a bare
/// `metrics` show.
fn is_headline(name: &str) -> bool {
    name == "alive.human"
        || name == "died"
        || name == "budget"
        || name == "crops"
        || name.starts_with("store.")
        || name == "in.steel rack"
        || name == "in.scrap bin"
        || name == "in.steel pallet"
        || name == "scrap.left"
        || name == "scrap.nearest"
}

/// The headline of the latest sample on one line, for a day gone by.
pub fn headline(series: &Series) -> String {
    let Some(latest) = series.last() else { return String::new() };
    let day_before = latest.tick.checked_sub(super::world::TICKS_PER_DAY).and_then(|tick| series.at_or_before(tick));
    latest
        .values
        .iter()
        .filter(|(name, _)| is_headline(name))
        .map(|(name, value)| {
            let change = day_before.and_then(|before| before.get(name)).map(|before| value - before);
            match change {
                Some(change) if change != 0.0 => format!("{} {} ({:+})", short(name), number(*value), number(change)),
                _ => format!("{} {}", short(name), number(*value)),
            }
        })
        .collect::<Vec<_>>()
        .join("  ")
}

pub fn headline_header(_series: &Series) -> String {
    "headline numbers (change over the day)".to_string()
}

fn short(name: &str) -> &str {
    name.strip_prefix("store.").unwrap_or(name)
}

/// A number as briefly as it can be said: whole numbers without a point.
pub fn number(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e12 {
        format!("{}", value as i64)
    } else {
        format!("{value:.1}")
    }
}

/// Everything the lab samples, now. Hourly, not per tick: it walks the crowd
/// once and asks every unit's goal.
pub fn sample(state: &GameState, flows: &BTreeMap<String, f64>) -> BTreeMap<String, f64> {
    let mut values: BTreeMap<String, f64> = BTreeMap::new();
    let add = |values: &mut BTreeMap<String, f64>, name: String, by: f64| {
        *values.entry(name).or_default() += by;
    };
    let stats = state.statistics();
    values.insert("alive".into(), stats.living() as f64);
    for (kind, count) in &stats.alive {
        values.insert(format!("alive.{}", kind.name()), *count as f64);
    }
    values.insert("died".into(), stats.died as f64);
    values.insert("lying".into(), stats.lying as f64);
    values.insert("carried".into(), stats.carried as f64);
    values.insert("cremated".into(), stats.cremated as f64);
    values.insert("jobless".into(), stats.jobless as f64);
    let bins = state.bins();
    for kind in FeatureKind::ALL {
        if kind.store().is_some() && bins.count(kind) > 0 {
            values.insert(format!("store.{}", kind.prop()), bins.total_in(kind) as f64);
        }
    }
    for (name, value) in flows {
        values.insert(name.clone(), *value);
    }
    // The scrap the collectors can get at: the heaps of the navigation area,
    // which is where `CollectGoal` looks and which never grow back.
    let bin = bins.iter_stores().find(|(_, kind, _)| *kind == FeatureKind::ScrapBin).map(|(at, _, _)| at);
    let (mut heaps, mut left, mut nearest) = (0.0, 0.0, f64::MAX);
    state.map.heaps(state.map.navigation_area(), |cell, scrap| {
        heaps += 1.0;
        left += scrap.pieces as f64;
        if let Some(bin) = bin {
            nearest = nearest.min(bin.manhattan_distance(cell) as f64);
        }
    });
    values.insert("scrap.heaps".into(), heaps);
    values.insert("scrap.left".into(), left);
    if nearest < f64::MAX {
        values.insert("scrap.nearest".into(), nearest);
    }
    values.insert("crops".into(), stats.fields.sown() as f64);
    if let Some(quality) = stats.fields.quality {
        values.insert("crops.quality".into(), quality as f64);
    }
    for job in &stats.jobs {
        if job.posts > 0 {
            values.insert(format!("jobs.{}", job.kind.name()), job.workers as f64);
        }
    }
    values.insert("budget".into(), stats.economy.budget.as_cents() as f64 / 100.0);
    values.insert("cash".into(), stats.economy.cash.as_cents() as f64 / 100.0);
    values.insert("in debt".into(), stats.economy.in_debt as f64);

    let clock = state.clock();
    let mut humans = 0.0;
    let mut sums: BTreeMap<&'static str, f64> = BTreeMap::new();
    let mut lowest: BTreeMap<&'static str, f64> = BTreeMap::new();
    for entity in state.entities().iter() {
        if entity.is_dead() {
            continue;
        }
        let goal = entity.current_goal().map_or("none", |goal| goal.name());
        add(&mut values, format!("goal.{goal}"), 1.0);
        if entity.is_asleep() {
            add(&mut values, "asleep".into(), 1.0);
        }
        if !state.map.contains(entity.center_position()) {
            add(&mut values, "outside".into(), 1.0);
        }
        if let Some(job) = entity.job().filter(|job| job.is_due(clock)) {
            add(&mut values, format!("{}.on shift", job.kind.name()), 1.0);
            add(&mut values, format!("{}.{goal}", job.kind.name()), 1.0);
        }
        if entity.kind() == Some(EntityType::Human) {
            if let Some(biology) = entity.biology() {
                humans += 1.0;
                for (name, value) in biology.stats().fields() {
                    *sums.entry(name).or_default() += value as f64;
                    let low = lowest.entry(name).or_insert(f64::MAX);
                    *low = low.min(value as f64);
                }
            }
        }
    }
    if humans > 0.0 {
        for (name, sum) in sums {
            values.insert(format!("{name}.avg"), (sum / humans * 10.0).round() / 10.0);
        }
        for (name, low) in lowest {
            values.insert(format!("{name}.min"), (low * 10.0).round() / 10.0);
        }
    }
    values
}
