//! Watches: rules over the world's numbers ([`super::metrics`]) that say
//! something has gone wrong, checked at every sample.
//!
//! ```json
//! { "name": "steel stopped reaching the warehouse", "metric": "in.steel rack",
//!   "grows_less_than": 1, "within_hours": 72, "after_hours": 48 }
//! ```
//!
//! Each names one metric and one condition: `below` / `above` a value, or a
//! change over the last `within_hours` — `rises_by` at least so much (somebody
//! died) or `grows_less_than` so much (the steel has stopped). `after_hours`
//! keeps it quiet until the world has been open that long, for a chain that
//! takes a day or two to fill. A watch fires when its condition **becomes**
//! true, and not again until it has been false: a stall is one anomaly, not
//! one an hour.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::metrics::{number, Series};
use super::world::TICKS_PER_HOUR;

/// The rules a run watches when it is given none: `lab/watches.json`.
const DEFAULTS: &str = include_str!("../../lab/watches.json");

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WatchSpec {
    pub name: String,
    pub metric: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub below: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub above: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rises_by: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grows_less_than: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub within_hours: Option<f64>,
    #[serde(default)]
    pub after_hours: f64,
}

/// A watch and what it has seen.
pub struct Watch {
    pub spec: WatchSpec,
    /// Whether its condition held at the last sample.
    holding: bool,
    pub fired: u32,
    /// The metric at the last sample; `None` if the world has no such number.
    pub last: Option<f64>,
}

/// A watch that fired.
pub struct Anomaly {
    pub watch: String,
    pub tick: u64,
    pub clock: String,
    pub message: String,
    /// The world as it stood, saved.
    pub snapshot: Option<std::path::PathBuf>,
}

impl std::fmt::Display for Anomaly {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}, tick {}] {}: {}", self.clock, self.tick, self.watch, self.message)?;
        if let Some(path) = &self.snapshot {
            write!(f, "\n  snapshot {}  (open it: CROWD2X_LOAD={} cargo run)", path.display(), path.display())?;
        }
        Ok(())
    }
}

impl Watch {
    pub fn new(spec: WatchSpec) -> Result<Watch, String> {
        let conditions = [spec.below, spec.above, spec.rises_by, spec.grows_less_than].iter().filter(|c| c.is_some()).count();
        if conditions != 1 {
            return Err(format!("watch {:?}: needs exactly one of below, above, rises_by, grows_less_than", spec.name));
        }
        if (spec.rises_by.is_some() || spec.grows_less_than.is_some()) && spec.within_hours.is_none_or(|h| h <= 0.0) {
            return Err(format!("watch {:?}: a change needs within_hours", spec.name));
        }
        Ok(Watch { spec, holding: false, fired: 0, last: None })
    }

    /// Look at the latest sample: the message, if this is the sample the
    /// condition became true at.
    pub fn check(&mut self, series: &Series) -> Option<String> {
        let latest = series.last()?;
        let spec = &self.spec;
        self.last = latest.get(&spec.metric);
        let value = self.last?;
        let old_enough = latest.tick as f64 / TICKS_PER_HOUR as f64 >= spec.after_hours;
        let message = if !old_enough {
            None
        } else if let Some(below) = spec.below {
            (value < below).then(|| format!("{} is {}, below {}", spec.metric, number(value), number(below)))
        } else if let Some(above) = spec.above {
            (value > above).then(|| format!("{} is {}, above {}", spec.metric, number(value), number(above)))
        } else {
            // The value a window ago; nothing while the record is younger
            // than the window, so a world just opened cannot have "stalled".
            let hours = spec.within_hours.unwrap_or(1.0);
            let window = (hours * TICKS_PER_HOUR as f64) as u64;
            let then = latest
                .tick
                .checked_sub(window)
                .and_then(|tick| series.at_or_before(tick))
                .and_then(|sample| sample.get(&spec.metric));
            match (then, spec.rises_by, spec.grows_less_than) {
                (Some(then), Some(by), _) => (value - then >= by).then(|| {
                    format!("{} rose by {} in {} h ({} to {})", spec.metric, number(value - then), number(hours), number(then), number(value))
                }),
                (Some(then), _, Some(least)) => (value - then < least).then(|| {
                    format!(
                        "{} grew by only {} in {} h ({} to {}); wanted at least {}",
                        spec.metric,
                        number(value - then),
                        number(hours),
                        number(then),
                        number(value),
                        number(least)
                    )
                }),
                _ => None,
            }
        };
        let fires = message.is_some() && !self.holding;
        self.holding = message.is_some();
        if fires {
            self.fired += 1;
            message
        } else {
            None
        }
    }

    /// One line: the rule, and what it last saw.
    pub fn describe(&self) -> String {
        let spec = &self.spec;
        let rule = if let Some(v) = spec.below {
            format!("{} below {}", spec.metric, number(v))
        } else if let Some(v) = spec.above {
            format!("{} above {}", spec.metric, number(v))
        } else if let Some(v) = spec.rises_by {
            format!("{} rises by {} within {} h", spec.metric, number(v), number(spec.within_hours.unwrap_or(0.0)))
        } else {
            let least = spec.grows_less_than.unwrap_or(0.0);
            format!("{} grows less than {} within {} h", spec.metric, number(least), number(spec.within_hours.unwrap_or(0.0)))
        };
        let after = if spec.after_hours > 0.0 { format!(", after {} h", number(spec.after_hours)) } else { String::new() };
        let last = self.last.map_or("never seen".to_string(), |v| format!("now {}", number(v)));
        format!("{:<40} {rule}{after}; {last}; fired {}", spec.name, self.fired)
    }
}

pub fn parse(text: &str) -> Result<Vec<Watch>, String> {
    let specs: Vec<WatchSpec> = serde_json::from_str(text).map_err(|e| e.to_string())?;
    specs.into_iter().map(Watch::new).collect()
}

pub fn read(path: &Path) -> Result<Vec<Watch>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse(&text).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn defaults() -> Vec<Watch> {
    parse(DEFAULTS).expect("lab/watches.json is valid")
}

#[cfg(test)]
mod tests {
    use super::super::metrics::Sample;
    use super::*;

    fn series(values: &[f64]) -> Series {
        let mut series = Series::default();
        for (hour, value) in values.iter().enumerate() {
            series.push(Sample {
                tick: hour as u64 * TICKS_PER_HOUR,
                elapsed: 0.0,
                values: [("in.steel rack".to_string(), *value)].into(),
            });
        }
        series
    }

    fn stall() -> Watch {
        Watch::new(WatchSpec {
            name: "stall".into(),
            metric: "in.steel rack".into(),
            below: None,
            above: None,
            rises_by: None,
            grows_less_than: Some(1.0),
            within_hours: Some(2.0),
            after_hours: 0.0,
        })
        .unwrap()
    }

    #[test]
    fn a_stall_fires_once_when_it_starts_and_again_only_after_growth() {
        let mut watch = stall();
        let values = [0.0, 1.0, 2.0, 2.0, 2.0, 2.0, 3.0, 4.0, 4.0, 4.0];
        let fired: Vec<usize> = (1..=values.len())
            .filter(|&n| watch.check(&series(&values[..n])).is_some())
            .map(|n| n - 1)
            .collect();
        assert_eq!(fired, vec![4, 9], "hour 4 is two hours with no growth; so is hour 9");
    }

    #[test]
    fn a_window_older_than_the_world_says_nothing() {
        let mut watch = stall();
        assert!(watch.check(&series(&[0.0, 0.0])).is_none());
    }

    #[test]
    fn the_default_watches_parse() {
        assert!(!defaults().is_empty());
    }
}
