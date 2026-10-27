//! Looking into a paused world: what each of the lab's commands prints.
//!
//! Everything is text meant to be read — by a person at a terminal or an agent
//! through `tools/lab.py` — built from what the unit panel and the statistics
//! menu are built from (`GameEntity::debug_fields` / `brain_fields`,
//! `GameState::statistics`), so the lab and the game screen say the same
//! things about the same world.

use crate::sim::{EntityType, FeatureKind, GameEntity, GameState, JobKind, Uid};

use super::metrics::number;
use super::world::{activity, duration, task_of, Lab, SHIFT_EVERY};

/// Where the world stands, and how the lab came to be there.
pub fn status(lab: &Lab, running: &str) -> String {
    let state = &lab.state;
    let mut out = format!(
        "{}  {}  tick {}  {} units  {}\n",
        lab.title,
        state.clock(),
        state.tick(),
        state.len(),
        running
    );
    out.push_str(&format!(
        "lab ran {} ({} ticks); {} samples; {} log lines; out {}\n",
        duration(lab.ticks_run),
        lab.ticks_run,
        lab.series.samples.len(),
        lab.lines_read,
        lab.out.display()
    ));
    if lab.anomalies.is_empty() {
        out.push_str("no anomalies");
    } else {
        out.push_str(&format!("{} anomalies, the last:\n  {}", lab.anomalies.len(), lab.anomalies.last().unwrap()));
    }
    out
}

/// Every anomaly so far.
pub fn anomalies(lab: &Lab) -> String {
    if lab.anomalies.is_empty() {
        return "no anomalies".to_string();
    }
    lab.anomalies.iter().map(|a| a.to_string()).collect::<Vec<_>>().join("\n")
}

/// The statistics menu, as the game shows it.
pub fn statistics(state: &GameState) -> String {
    crate::game::statistics::describe(&state.statistics())
}

/// Every store on the map, by kind: where it is and what is in it.
pub fn stores(state: &GameState, filter: &str) -> String {
    let mut rows: Vec<(FeatureKind, String)> = state
        .bins()
        .iter_stores()
        .filter(|(_, kind, _)| filter.is_empty() || kind.prop().contains(filter))
        .map(|(at, kind, stored)| (kind, format!("{:<14} at {:>4}, {:<4} {:>4} {}", kind.prop(), at.x, at.y, stored, holds(kind))))
        .collect();
    rows.sort_by_key(|(kind, _)| kind.number());
    if rows.is_empty() {
        return format!("no store matches {filter:?}");
    }
    rows.into_iter().map(|(_, row)| row).collect::<Vec<_>>().join("\n")
}

fn holds(kind: FeatureKind) -> &'static str {
    kind.store().map_or("", |store| store.holds.name())
}

/// The unit `who` names: its id as printed (`human:3f1c...`), the start of
/// the hex after the colon, or part of its name.
pub fn resolve(state: &GameState, who: &str) -> Result<Uid, String> {
    let who = who.trim();
    if who.is_empty() {
        return Err("whom? an id, the start of one, or part of a name".to_string());
    }
    let lower = who.to_lowercase();
    let mut found: Vec<Uid> = Vec::new();
    for entity in state.entities().iter() {
        let id = entity.uid().to_string();
        if id == who {
            return Ok(entity.uid());
        }
        let hex = id.split(':').nth(1).unwrap_or("");
        let named = entity.display_name().is_some_and(|name| name.to_lowercase().contains(&lower));
        if (lower.len() >= 3 && hex.starts_with(&lower)) || named {
            found.push(entity.uid());
        }
    }
    match found.len() {
        0 => Err(format!("nobody is {who:?}")),
        1 => Ok(found[0]),
        n => Err(format!(
            "{n} match {who:?}:\n{}",
            found.iter().take(12).map(|&uid| line(state, uid)).collect::<Vec<_>>().join("\n")
        )),
    }
}

/// One line about a unit: who, its job, what it is at, where, what it carries.
pub fn line(state: &GameState, uid: Uid) -> String {
    let Some(entity) = state.entities().get(uid) else {
        return format!("{uid} is gone");
    };
    let name = entity.display_name().unwrap_or_default();
    let job = entity.job().map_or("-".to_string(), |job| {
        let due = if job.is_due(state.clock()) { "*" } else { "" };
        format!("{}{due}", job.kind.name())
    });
    let at = entity.center_position();
    let pack = entity.inventory().map(|i| i.describe_stowed()).filter(|s| !s.is_empty()).unwrap_or("-".to_string());
    let dead = if entity.is_dead() { " DEAD" } else { "" };
    format!(
        "{uid}  {name:<22} {job:<18} {:<34} at {:>4},{:<4} pack {pack}{dead}",
        activity(state, entity),
        at.x,
        at.y
    )
}

/// Everybody `filter` picks: `all`, `humans`, `dogs`, `deathdogs`, `dead`,
/// `asleep`, `outside`, `jobless`, `goal:<name>`, a stat against a number
/// (`health<20`, `stamina>90`: the living only), or a job (`courier`,
/// `scrap collector`, plural or not). `*` beside a job: its shift is due.
pub fn units(state: &GameState, filter: &str) -> String {
    let filter = filter.trim().to_lowercase();
    let job = job_named(&filter);
    if !filter.is_empty() && job.is_none() && !is_known_filter(&filter) {
        return format!(
            "no filter {filter:?}: all, humans, dogs, deathdogs, dead, asleep, outside, jobless, goal:<name>, <stat><N, <stat>>N, or a job: {}",
            JobKind::ALL.iter().map(|kind| kind.name()).collect::<Vec<_>>().join(", ")
        );
    }
    let picked: Vec<Uid> = state
        .entities()
        .in_spawn_order()
        .into_iter()
        .filter(|&uid| state.entities().get(uid).is_some_and(|entity| picks(state, entity, &filter, job)))
        .collect();
    let mut out: Vec<String> = picked.iter().take(200).map(|&uid| line(state, uid)).collect();
    if picked.len() > 200 {
        out.push(format!("... and {} more", picked.len() - 200));
    }
    out.push(format!("{} units", picked.len()));
    out.join("\n")
}

fn is_known_filter(filter: &str) -> bool {
    matches!(filter, "" | "all" | "humans" | "human" | "dogs" | "dog" | "deathdogs" | "deathdog" | "dead" | "asleep" | "outside" | "jobless")
        || filter.starts_with("goal:")
        || stat_test(filter).is_some()
}

/// `health<20` or `stamina>90`: a stat [`Stats::fields`] names, which way,
/// and the number.
///
/// [`Stats::fields`]: crate::sim::Stats::fields
fn stat_test(filter: &str) -> Option<(&str, bool, f32)> {
    let (at, below) = filter.find('<').map(|at| (at, true)).or_else(|| filter.find('>').map(|at| (at, false)))?;
    let (stat, value) = (filter[..at].trim(), filter[at + 1..].trim().parse().ok()?);
    crate::sim::Stats::named().contains(&stat).then_some((stat, below, value))
}

/// A unit's stat by name, if it has a body.
fn stat_of(entity: &dyn GameEntity, stat: &str) -> Option<f32> {
    entity.biology()?.stats().fields().into_iter().find(|(name, _)| *name == stat).map(|(_, value)| value)
}

fn job_named(filter: &str) -> Option<JobKind> {
    let singular = filter.strip_suffix('s').unwrap_or(filter);
    JobKind::ALL.into_iter().find(|kind| kind.name() == filter || kind.name() == singular)
}

fn picks(state: &GameState, entity: &dyn GameEntity, filter: &str, job: Option<JobKind>) -> bool {
    if let Some(job) = job {
        return entity.job().is_some_and(|j| j.kind == job);
    }
    match filter {
        "" | "all" => true,
        "humans" | "human" => entity.kind() == Some(EntityType::Human),
        "dogs" | "dog" => entity.kind() == Some(EntityType::Dog),
        "deathdogs" | "deathdog" => entity.kind() == Some(EntityType::Deathdog),
        "dead" => entity.is_dead(),
        "asleep" => entity.is_asleep(),
        "outside" => !state.map.contains(entity.center_position()),
        "jobless" => entity.kind() == Some(EntityType::Human) && !entity.is_dead() && entity.job().is_none(),
        _ if stat_test(filter).is_some() => {
            let (stat, below, value) = stat_test(filter).expect("just checked");
            !entity.is_dead()
                && stat_of(entity, stat).is_some_and(|have| if below { have < value } else { have > value })
        }
        _ => filter
            .strip_prefix("goal:")
            .is_some_and(|goal| entity.current_goal().map_or("none", |g| g.name()) == goal),
    }
}

/// Everything about one unit: body, brain, stats, pack, and its last lines
/// of log.
pub fn unit(lab: &Lab, uid: Uid, lines: usize) -> String {
    let state = &lab.state;
    let Some(entity) = state.entities().get(uid) else {
        return format!("{uid} is gone");
    };
    let mut out = line(state, uid);
    out.push('\n');
    if let Some(job) = entity.job() {
        out.push_str(&format!(
            "job       {} at {}, {} ({} {}), {}\n",
            job.kind.name(),
            job.workplace.x,
            job.workplace.y,
            job.shift.hours(),
            job.shift.days(),
            if job.is_due(state.clock()) { "due now" } else { "off" }
        ));
    }
    out.push_str("-- body\n");
    out.push_str(&fields(&entity.debug_fields()));
    out.push_str("-- brain\n");
    out.push_str(&fields(&entity.brain_fields()));
    if let Some(biology) = entity.biology() {
        let stats: Vec<String> = biology.stats().fields().iter().map(|(name, value)| format!("{name} {value:.0}")).collect();
        out.push_str(&format!("-- stats\n{}\n", stats.join("  ")));
    }
    let path = entity.planned_path();
    if !path.is_empty() {
        let end = path.last().unwrap();
        out.push_str(&format!("-- route: {} cells, to {}, {}\n", path.len(), end.x, end.y));
    }
    out.push_str(&format!("-- its last {lines} lines\n"));
    out.push_str(&log(lab, Some(uid), &[], lines));
    out
}

fn fields(fields: &[(&'static str, String)]) -> String {
    let mut out = String::new();
    for (key, value) in fields {
        out.push_str(&format!("{key:<20} {value}\n"));
    }
    out
}

/// The last `count` lines of the log about `uid` (anybody, for `None`) that
/// contain every word of `filter`.
pub fn log(lab: &Lab, uid: Option<Uid>, filter: &[&str], count: usize) -> String {
    let lines: Vec<String> = lab
        .log
        .iter()
        .rev()
        .filter(|line| uid.is_none_or(|uid| line.concerns(uid)))
        .filter(|line| filter.iter().all(|word| line.message.contains(word)))
        .take(count)
        .map(|line| format!("{}  {}", line.clock(), line.message))
        .collect();
    if lines.is_empty() {
        return "no lines".to_string();
    }
    lines.into_iter().rev().collect::<Vec<_>>().join("\n")
}

/// Where the shifts of the jobs named (every job, for none) have gone since
/// counting started: each kind's share of on-shift looks by what the worker
/// was doing.
pub fn shifts(lab: &Lab, jobs: &[&str]) -> String {
    let shifts = &lab.shifts;
    let wanted: Vec<JobKind> = if jobs.is_empty() {
        JobKind::ALL.to_vec()
    } else {
        let joined = jobs.join(" ");
        match job_named(&joined) {
            Some(job) => vec![job],
            None => return format!("no job {joined:?}"),
        }
    };
    let mut out = format!(
        "where the shifts went, a look every {} since {} ago\n",
        duration(SHIFT_EVERY),
        duration(lab.state.tick().saturating_sub(shifts.since))
    );
    for kind in wanted {
        let rows: Vec<(&String, u32)> =
            shifts.time.iter().filter(|((job, _), _)| *job == kind.name()).map(|((_, what), n)| (what, *n)).collect();
        let total: u32 = rows.iter().map(|(_, n)| n).sum();
        if total == 0 {
            continue;
        }
        out.push_str(&format!("== {} ({} looks, {} on shift)\n", kind.name(), total, duration(total as u64 * SHIFT_EVERY)));
        let mut rows = rows;
        rows.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        for (what, n) in rows.iter().take(16) {
            out.push_str(&format!("  {:5.1}%  {what}\n", *n as f32 * 100.0 / total as f32));
        }
        if rows.len() > 16 {
            out.push_str(&format!("  ... {} rarer\n", rows.len() - 16));
        }
    }
    out
}

/// Live `ticks` more ticks watching one unit, and say every time its goal,
/// its task or its pack changed, with its log lines between. The world goes
/// on as it would have — samples, watches and all.
pub fn trace(lab: &mut Lab, uid: Uid, ticks: u64) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut last = String::new();
    let mut seen = lab.lines_read;
    let mut fired = 0;
    let started = lab.state.tick();
    for _ in 0..ticks {
        fired += lab.step();
        // Lines about it read this tick.
        let new = ((lab.lines_read - seen) as usize).min(lab.log.len());
        for line in lab.log.iter().skip(lab.log.len() - new) {
            if line.concerns(uid) {
                out.push(format!("{}    | {}", line.clock(), line.message));
            }
        }
        seen = lab.lines_read;
        let Some(entity) = lab.state.entities().get(uid) else {
            out.push(format!("{}  gone", lab.state.clock()));
            break;
        };
        let goal = entity.current_goal().map_or("none", |goal| goal.name());
        let pack = entity.inventory().map(|i| i.describe_stowed()).unwrap_or_default();
        let at = entity.center_position();
        let now = format!("{goal} | {} | pack {pack}", task_of(entity));
        if now != last {
            out.push(format!("{}  {now}  at {},{}", lab.state.clock(), at.x, at.y));
            last = now;
        }
    }
    let mut text = if out.len() > 400 {
        let skipped = out.len() - 400;
        format!("... {skipped} earlier changes\n{}", out[skipped..].join("\n"))
    } else {
        out.join("\n")
    };
    text.push_str(&format!("\ntraced {} ({} ticks)", duration(lab.state.tick() - started), lab.state.tick() - started));
    if fired > 0 {
        text.push_str(&format!("\n{fired} anomalies fired meanwhile:\n{}", anomalies(lab)));
    }
    text
}

/// One metric's value now, or every metric containing `pattern`.
pub fn values(lab: &Lab, pattern: &str) -> String {
    let Some(latest) = lab.series.last() else { return "no samples yet".to_string() };
    let rows: Vec<String> = latest
        .values
        .iter()
        .filter(|(name, _)| name.contains(pattern))
        .map(|(name, value)| format!("{name:<36} {}", number(*value)))
        .collect();
    if rows.is_empty() {
        return format!("no metric contains {pattern:?}");
    }
    format!("at {}\n{}", latest.clock(), rows.join("\n"))
}
