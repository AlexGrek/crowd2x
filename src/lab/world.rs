//! The world the lab lives through, and what it keeps of it as it goes.

use std::collections::{BTreeMap, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::map::{Map, Point};
use crate::save::{self, SaveMeta};
use crate::sim::log::Audience;
use crate::sim::{process_game_state, Clock, GameEntity, GameState, Input, Uid};

use super::metrics::{self, Series};
use super::watch::{Anomaly, Watch};

/// The game's fixed step: Bevy's default 64 Hz, which nothing in the game
/// changes. A lab tick is a game tick, so a snapshot carries on in the game as
/// it would have here.
pub const DT: f32 = 1.0 / 64.0;
/// A world minute: two of them go by in a watched second (`TIME_SCALE` 120).
pub const TICKS_PER_MINUTE: u64 = 32;
pub const TICKS_PER_HOUR: u64 = 60 * TICKS_PER_MINUTE;
pub const TICKS_PER_DAY: u64 = 24 * TICKS_PER_HOUR;
/// How many lines of the log are kept for `log` and `unit`.
const LOG_KEPT: usize = 50_000;
/// How often every worker on shift is looked at, for `shifts`: five world
/// minutes, as the steel probe did.
pub const SHIFT_EVERY: u64 = 5 * TICKS_PER_MINUTE;

/// Where a world comes from.
pub enum Source {
    District(u64),
    MapFile(PathBuf),
    Save(PathBuf),
}

impl Source {
    /// A name for the run's directory.
    pub fn slug(&self) -> String {
        match self {
            Source::District(seed) => format!("district-{seed}"),
            Source::MapFile(path) | Source::Save(path) => {
                path.file_stem().map_or("world".to_string(), |stem| stem.to_string_lossy().into_owned())
            }
        }
    }
}

/// A line of the log, as the lab keeps it.
pub struct Line {
    /// Watched seconds when it was read, for its clock.
    pub elapsed: f64,
    /// Who it is about: one unit, two, or nobody in particular.
    pub about: (Option<Uid>, Option<Uid>),
    pub message: String,
}

impl Line {
    pub fn clock(&self) -> Clock {
        Clock::after_watching(self.elapsed)
    }

    pub fn concerns(&self, uid: Uid) -> bool {
        self.about.0 == Some(uid) || self.about.1 == Some(uid)
    }
}

/// Where the workers' shifts go: every [`SHIFT_EVERY`] ticks, each worker
/// whose shift is due counted against its job and what it is doing.
#[derive(Default)]
pub struct Shifts {
    /// The tick counting started (or was last reset).
    pub since: u64,
    /// (job, what it was doing) -> how many looks found it so.
    pub time: BTreeMap<(&'static str, String), u32>,
}

impl Shifts {
    fn look(&mut self, state: &GameState) {
        let clock = state.clock();
        for entity in state.entities().iter() {
            let Some(job) = entity.job() else { continue };
            if entity.is_dead() || !job.is_due(clock) {
                continue;
            }
            *self.time.entry((job.kind.name(), activity(state, entity))).or_default() += 1;
        }
    }
}

/// What a unit is doing, in a few words: its goal, the kind of task it is on,
/// and where, if not on the site. What `shifts` groups by.
pub fn activity(state: &GameState, entity: &dyn GameEntity) -> String {
    let goal = entity.current_goal().map_or("none", |goal| goal.name());
    let task = task_of(entity);
    let task = task.split([' ', '(']).next().unwrap_or("");
    let task = if task.is_empty() { "idle" } else { task };
    let asleep = if entity.is_asleep() { " asleep" } else { "" };
    let out = if state.map.contains(entity.center_position()) { "" } else { " outside" };
    format!("{goal} / {task}{asleep}{out}")
}

/// The brain's own description of the task in hand, or nothing.
pub fn task_of(entity: &dyn GameEntity) -> String {
    entity
        .brain_fields()
        .into_iter()
        .find(|(key, _)| *key == "task")
        .map(|(_, value)| value)
        .unwrap_or_default()
}

/// A world being lived through.
pub struct Lab {
    pub state: GameState,
    pub title: String,
    pub out: PathBuf,
    /// Commands for the next tick.
    pub input: Input,
    /// Ticks between samples of [`metrics`].
    pub every: u64,
    pub series: Series,
    pub log: VecDeque<Line>,
    /// Every line ever read, kept or not: how a reader tells new from old.
    pub lines_read: u64,
    pub shifts: Shifts,
    /// Pieces put into (`in.<store>`) and taken out of (`out.<store>`) every
    /// kind of store since the lab started, read off the log's lines.
    pub flows: BTreeMap<String, f64>,
    pub watches: Vec<Watch>,
    pub anomalies: Vec<Anomaly>,
    /// Ticks this lab has run, whatever the world's own count.
    pub ticks_run: u64,
    metrics_file: Option<std::fs::File>,
    /// Every line of the log, `tick<TAB>clock<TAB>message`: the whole history,
    /// which outlives the lab and the [`LOG_KEPT`] lines it holds — a world
    /// carried on from a snapshot starts with an empty log.
    log_file: Option<std::io::BufWriter<std::fs::File>>,
}

impl Lab {
    pub fn new(source: &Source, out: PathBuf, every: u64, watches: Vec<Watch>) -> Result<Lab, String> {
        let (state, title) = match source {
            Source::District(seed) => (fresh(crate::map::district::generate(*seed).map), format!("district {seed}")),
            Source::MapFile(path) => {
                let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
                let map = Map::from_json(&text).map_err(|e| format!("{}: {e}", path.display()))?;
                (fresh(map), path.display().to_string())
            }
            Source::Save(path) => (load(path)?, path.display().to_string()),
        };
        std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
        let metrics_file = std::fs::File::create(out.join("metrics.jsonl")).ok();
        let log_file = std::fs::File::create(out.join("log.txt")).ok().map(std::io::BufWriter::new);
        let mut lab = Lab {
            state,
            title,
            out,
            input: Input::new(),
            every,
            series: Series::default(),
            log: VecDeque::new(),
            lines_read: 0,
            shifts: Shifts::default(),
            flows: BTreeMap::new(),
            watches,
            anomalies: Vec::new(),
            ticks_run: 0,
            metrics_file,
            log_file,
        };
        lab.shifts.since = lab.state.tick();
        lab.flows = zero_flows(&lab.state);
        lab.read_log();
        lab.sample();
        Ok(lab)
    }

    /// Put another world in this one's place — a snapshot read back — and
    /// start its record afresh.
    pub fn replace(&mut self, path: &Path) -> Result<(), String> {
        self.state = load(path)?;
        self.title = path.display().to_string();
        if let Some(file) = self.log_file.as_mut() {
            let _ = writeln!(file, "{}\t{}\t-- carried on from {}", self.state.tick(), self.state.clock(), self.title);
        }
        self.series = Series::default();
        self.log.clear();
        self.shifts = Shifts { since: self.state.tick(), ..Shifts::default() };
        self.flows = zero_flows(&self.state);
        self.input = Input::new();
        self.read_log();
        self.sample();
        Ok(())
    }

    /// One tick of the world, and whatever falls due with it. Answers how
    /// many anomalies it fired, which are the last that many of
    /// [`Lab::anomalies`].
    pub fn step(&mut self) -> usize {
        let input = std::mem::take(&mut self.input);
        process_game_state(&mut self.state, DT, &input);
        self.ticks_run += 1;
        self.read_log();
        let tick = self.state.tick();
        if tick % SHIFT_EVERY == 0 {
            self.shifts.look(&self.state);
        }
        if tick % self.every == 0 { self.sample() } else { 0 }
    }

    /// Take the world's log, keeping the newest [`LOG_KEPT`] lines and
    /// counting the pieces they say went into and out of a store.
    fn read_log(&mut self) {
        if self.state.log.is_empty() {
            return;
        }
        let elapsed = self.state.elapsed();
        let stamp = self.log_file.as_ref().map(|_| format!("{}\t{}", self.state.tick(), self.state.clock()));
        for entry in self.state.log.drain_entries() {
            if let (Some(file), Some(stamp)) = (self.log_file.as_mut(), stamp.as_ref()) {
                let _ = writeln!(file, "{stamp}\t{}", entry.message);
            }
            if let Some((incoming, pieces, at)) = flow(&entry.message) {
                if let Some(kind) = self.state.bins().store(at) {
                    let key = format!("{}.{}", if incoming { "in" } else { "out" }, kind.prop());
                    *self.flows.entry(key).or_default() += pieces as f64;
                }
            }
            let about = match entry.audience {
                Audience::Unit(uid) => (Some(uid), None),
                Audience::Related(a, b) => (Some(a), Some(b)),
                Audience::Routine | Audience::Important => (None, None),
            };
            if self.log.len() == LOG_KEPT {
                self.log.pop_front();
            }
            self.log.push_back(Line { elapsed, about, message: entry.message });
            self.lines_read += 1;
        }
    }

    /// Sample the world's numbers, write them down and look at them.
    fn sample(&mut self) -> usize {
        let values = metrics::sample(&self.state, &self.flows);
        let sample = metrics::Sample { tick: self.state.tick(), elapsed: self.state.elapsed(), values };
        if let Some(file) = self.metrics_file.as_mut() {
            let _ = writeln!(file, "{}", sample.to_json());
        }
        self.series.push(sample);
        let mut fired = 0;
        for index in 0..self.watches.len() {
            if let Some(message) = self.watches[index].check(&self.series) {
                let name = self.watches[index].spec.name.clone();
                let snapshot = self.snapshot(&format!("anomaly-{:02}-{}", self.anomalies.len() + 1, slug(&name)));
                self.anomalies.push(Anomaly {
                    watch: name,
                    tick: self.state.tick(),
                    clock: self.state.clock().to_string(),
                    message,
                    snapshot: snapshot.ok(),
                });
                fired += 1;
            }
        }
        fired
    }

    /// Save the world where it stands as `<out>/<name>.savegame`.
    pub fn snapshot(&self, name: &str) -> Result<PathBuf, String> {
        let path = self.out.join(format!("{name}.savegame"));
        let zip = self.save_bytes(name)?;
        crate::map::write_atomic(&path, |file| file.write_all(&zip)).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path)
    }

    /// The world as a `.savegame`'s bytes, paused, with the camera on the
    /// middle of the site.
    pub fn save_bytes(&self, name: &str) -> Result<Vec<u8>, String> {
        let moment = crate::save::store::Moment::now();
        let meta = SaveMeta {
            name: format!("lab: {name}"),
            quick: false,
            map_title: self.title.clone(),
            created_unix_ms: moment.unix_ms,
            created_local: moment.local.clone(),
        };
        let site = self.state.map.site();
        let middle = Point::new(site.origin.x + site.size.width as i32 / 2, site.origin.y + site.size.height as i32 / 2);
        let cell = crate::map::PIXELS_PER_CELL as f32;
        let view = crate::save::pb::View {
            camera_x: (middle.x as f32 + 0.5) * cell,
            camera_y: (middle.y as f32 + 0.5) * cell,
            zoom: 0,
            speed: 1.0,
            paused: true,
            zoom_factor: 0.0,
        };
        save::snapshot(&self.state, &meta, Some(&view)).to_zip(Some(moment.stamp)).map_err(|e| e.to_string())
    }

    /// Write what the lab has kept so far where it can be read: the metrics
    /// as a CSV.
    pub fn finish(&mut self) {
        let _ = std::fs::write(self.out.join("metrics.csv"), self.series.to_csv());
        if let Some(file) = self.metrics_file.as_mut() {
            let _ = file.flush();
        }
        if let Some(file) = self.log_file.as_mut() {
            let _ = file.flush();
        }
    }

    pub fn day_line_header(&self) -> String {
        format!("{:<18} {:>6}  {}", "world", "real", metrics::headline_header(&self.series))
    }

    /// A line for a world day gone by: when, how long it took, the headline numbers.
    pub fn day_line(&self, seconds: f32) -> String {
        format!("{:<18} {:>5.1}s  {}", self.state.clock().to_string(), seconds, metrics::headline(&self.series))
    }
}

/// A world fresh from a map, the way the game screen builds one
/// (`game::actors::build_world`): its seed, the economy's and the
/// relationships' rules, and whoever the map says lives there.
fn fresh(map: Map) -> GameState {
    let mut state = GameState::new(map, crate::game::actors::DEFAULT_SEED);
    let (tariffs, warning) = crate::game::economy::load_tariffs();
    state.set_tariffs(tariffs);
    if let Some(warning) = warning {
        eprintln!("lab: {warning}");
    }
    let (rules, warning) = crate::game::relationships::load_rules();
    state.set_relationship_rules(rules);
    if let Some(warning) = warning {
        eprintln!("lab: {warning}");
    }
    let (rules, warning) = crate::game::store::load_rules();
    state.set_store_rules(rules);
    if let Some(warning) = warning {
        eprintln!("lab: {warning}");
    }
    state.spawn_from_spawners();
    state
}

/// Every kind of store the map has, nothing in or out yet: a watch on a
/// chain that never moved a piece has a number to see.
fn zero_flows(state: &GameState) -> BTreeMap<String, f64> {
    let mut flows = BTreeMap::new();
    for kind in crate::sim::FeatureKind::ALL {
        if kind.store().is_some() && state.bins().count(kind) > 0 {
            flows.insert(format!("in.{}", kind.prop()), 0.0);
            flows.insert(format!("out.{}", kind.prop()), 0.0);
        }
    }
    flows
}

fn load(path: &Path) -> Result<GameState, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let files = save::SaveFiles::from_zip(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(save::restore(&files).map_err(|e| format!("{}: {e}", path.display()))?.state)
}

/// What a store's log line says moved: `"<uid> stored 3 scrap metal in the
/// bin at 4, 3: 9 stored"` or `"<uid> took 2 steel from the steel pallet at
/// 9, 5: 0 left"` — in or out, how many, and the store's cell.
fn flow(message: &str) -> Option<(bool, u32, Point)> {
    let (_, rest) = message.split_once(' ')?;
    let (verb, rest) = rest.split_once(' ')?;
    let incoming = match verb {
        "stored" => true,
        "took" => false,
        _ => return None,
    };
    let (count, rest) = rest.split_once(' ')?;
    let pieces: u32 = count.parse().ok()?;
    let (_, at) = rest.rsplit_once(" at ")?;
    let (x, y) = at.split(':').next()?.split_once(", ")?;
    Some((incoming, pieces, Point::new(x.trim().parse().ok()?, y.trim().parse().ok()?)))
}

/// A name fit for a file: lowercase words joined by dashes.
pub fn slug(text: &str) -> String {
    let words: Vec<String> = text
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(|word| word.to_ascii_lowercase())
        .collect();
    words.join("-")
}

/// `90m`, `6h`, `3d`, `2w` of world time, or `5000t` ticks, as ticks.
pub fn parse_duration(text: &str) -> Result<u64, String> {
    let text = text.trim();
    let split = text.find(|c: char| !c.is_ascii_digit() && c != '.').ok_or(format!(
        "{text:?} needs a unit: m, h, d, w (world time) or t (ticks)"
    ))?;
    let (number, unit) = text.split_at(split);
    let number: f64 = number.parse().map_err(|_| format!("{text:?} is not a duration"))?;
    let ticks = match unit {
        "t" => number,
        "m" => number * TICKS_PER_MINUTE as f64,
        "h" => number * TICKS_PER_HOUR as f64,
        "d" => number * TICKS_PER_DAY as f64,
        "w" => number * 7.0 * TICKS_PER_DAY as f64,
        _ => return Err(format!("{text:?}: the unit is m, h, d, w or t")),
    };
    Ok(ticks.round() as u64)
}

/// A number of ticks as world time, `2d 3h 15m`.
pub fn duration(ticks: u64) -> String {
    let minutes = ticks / TICKS_PER_MINUTE;
    let (days, hours, minutes) = (minutes / (24 * 60), minutes / 60 % 24, minutes % 60);
    match (days, hours) {
        (0, 0) => format!("{minutes}m"),
        (0, _) => format!("{hours}h {minutes:02}m"),
        _ => format!("{days}d {hours}h {minutes:02}m"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_store_s_log_line_says_what_went_in_or_out_and_where() {
        assert_eq!(flow("human:00ab stored 3 scrap metal in the bin at 4, 3: 9 stored"), Some((true, 3, Point::new(4, 3))));
        assert_eq!(flow("human:00ab took 2 steel from the steel pallet at -9, 5: 0 left"), Some((false, 2, Point::new(-9, 5))));
        assert_eq!(flow("human:00ab took no steel from the steel pallet at 9, 5: 0 left"), None);
        assert_eq!(flow("human:00ab found nothing to carry"), None);
    }

    #[test]
    fn durations_are_world_time_or_ticks() {
        assert_eq!(parse_duration("1h"), Ok(1920));
        assert_eq!(parse_duration("1d"), Ok(46080));
        assert_eq!(parse_duration("90m"), Ok(2880));
        assert_eq!(parse_duration("500t"), Ok(500));
        assert!(parse_duration("12").is_err());
        assert_eq!(duration(46080 + 1920 + 32 * 5), "1d 1h 05m");
    }
}
