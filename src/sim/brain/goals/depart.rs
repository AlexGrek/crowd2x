//! [`DepartGoal`]: by day, a deathdog walks off into the wasteland.
//!
//! Straight away from the site: a cell on the line from the site's middle
//! through the deathdog, [`DEPART_DISTANCE`] and a few more cells out, give
//! or take a little ([`JITTER`]), checked as a prowl's is. Where that is all
//! water — a lake across the way out — the line swings round the site, a
//! [`SWING`] at a time either side, until it clears the lake. It never gets
//! there: once [`DEPART_DISTANCE`] out it is gone (`kinds::Deathdog::react`,
//! `Effect::Depart`). Misses, waits and walls as a prowl's.

use rand::RngExt;

use crate::map::{Bounds, Point};
use crate::save::{invalid, pb, Invalid};
use crate::sim::deathdogs::{distance_from_site, DEPART_DISTANCE};
use crate::sim::rng::tick_rng;

use super::super::goal::{GoalCtx, GoalExecutor, GoalId, GoalProgress};
use super::super::task::{Task, TaskResult};
use super::{PATIENCE, WAIT_FOR_A_GAP};

/// How much further out than [`DEPART_DISTANCE`] it heads for, in cells.
pub const BEYOND: i32 = 4;

/// How far either way of the line it may head, in cells.
pub const JITTER: i32 = 3;

/// How many cells round the line out it tries before swinging the line.
pub const DEPART_TRIES: u32 = 8;

/// How far round the site's middle each swing turns the line out, in
/// radians: about ten degrees, twenty-odd cells along the way out of a
/// district, past the side of most lakes at the first or second turn.
pub const SWING: f32 = 0.18;

/// How many swings it tries either side before standing still this tick:
/// round to about eighty degrees.
pub const SWINGS: u32 = 8;

/// Where on the line out from `site`'s middle through `here`, swung `swing`
/// radians round the middle, a cell is `out` cells from the site: the nearer
/// of the two axes' answers, since the site's distance is a king's steps.
pub fn way_out(site: Bounds, here: Point, out: i32, swing: f32) -> Point {
    let (half_x, half_y) = (site.size.width as f32 / 2.0, site.size.height as f32 / 2.0);
    let (mid_x, mid_y) = (site.origin.x as f32 + half_x, site.origin.y as f32 + half_y);
    let (mut dx, mut dy) = (here.x as f32 + 0.5 - mid_x, here.y as f32 + 0.5 - mid_y);
    if dx == 0.0 && dy == 0.0 {
        dx = 1.0;
    }
    let length = (dx * dx + dy * dy).sqrt();
    (dx, dy) = (dx / length, dy / length);
    if swing != 0.0 {
        let (sin, cos) = swing.sin_cos();
        (dx, dy) = (dx * cos - dy * sin, dx * sin + dy * cos);
    }
    let along = |half: f32, d: f32| if d.abs() > 1e-6 { (out as f32 + half) / d.abs() } else { f32::INFINITY };
    let t = along(half_x, dx).min(along(half_y, dy));
    Point::new((mid_x + dx * t).floor() as i32, (mid_y + dy * t).floor() as i32)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DepartGoal {
    target: Option<Point>,
    retries: u8,
}

impl DepartGoal {
    pub fn new() -> DepartGoal {
        DepartGoal::default()
    }

    /// [`DEPART_TRIES`] cells round the line out, then one each on lines
    /// swung further and further round the site, either side in turn: where
    /// the line out runs into a lake, the way out is round its side, and
    /// standing still waiting for a lake to move kept a deathdog in all day.
    fn pick(ctx: &GoalCtx<'_>) -> Option<Point> {
        let map = ctx.think.map;
        let here = ctx.body.center_position();
        let site = map.site();
        let ahead = way_out(site, here, DEPART_DISTANCE + BEYOND, 0.0);
        let navigation = map.navigation();
        let mut rng = tick_rng(ctx.body.uid(), ctx.think.tick);
        let swings = (1..=SWINGS).flat_map(|turn| [turn as f32 * SWING, -(turn as f32) * SWING]);
        let aims = std::iter::repeat_n(ahead, DEPART_TRIES as usize)
            .chain(swings.map(|swing| way_out(site, here, DEPART_DISTANCE + BEYOND, swing)));
        for away in aims {
            let candidate = away.offset(rng.random_range(-JITTER..=JITTER), rng.random_range(-JITTER..=JITTER));
            if candidate != here
                && distance_from_site(map, candidate) > 0
                && map.is_passable(candidate)
                && navigation.may_reach(here, candidate)
            {
                return Some(candidate);
            }
        }
        None
    }

    fn forget(&mut self) {
        self.target = None;
        self.retries = 0;
    }
}

impl GoalExecutor for DepartGoal {
    fn goal(&self) -> GoalId {
        GoalId::Depart
    }

    fn save(&self) -> pb::Executor {
        let DepartGoal { target, retries } = *self;
        pb::Executor::Wander(pb::Wander { target: target.map(Into::into), retries: retries as u32 })
    }

    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        let pb::Executor::Wander(departure) = saved else {
            return invalid("departing is a target and its retries");
        };
        let Ok(retries) = u8::try_from(departure.retries) else {
            return invalid(format!("depart: {} retries", departure.retries));
        };
        self.target = departure.target.map(Into::into);
        self.retries = retries;
        Ok(())
    }

    /// Picked from where it stands when the day comes.
    fn prioritized(&mut self, _ctx: &mut GoalCtx<'_>) {
        self.forget();
    }

    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        match last {
            TaskResult::Success if !ctx.tasks.is_empty() => return GoalProgress::Working,
            TaskResult::Success => {
                self.forget();
                return GoalProgress::Achieved;
            }
            TaskResult::Failed => {
                ctx.tasks.clear();
                match (self.target, ctx.blocked_by) {
                    (Some(target), Some(_)) if self.retries < PATIENCE => {
                        self.retries += 1;
                        let _ = ctx.tasks.push_back(Task::wait(WAIT_FOR_A_GAP));
                        let _ = ctx.tasks.push_back(Task::move_to(target));
                        return GoalProgress::Working;
                    }
                    (_, Some(_)) => {
                        self.forget();
                        return GoalProgress::Working;
                    }
                    (_, None) => {
                        self.forget();
                        return GoalProgress::Blocked;
                    }
                }
            }
            TaskResult::InProgress | TaskResult::Executing => {}
        }
        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }
        if let Some(target) = Self::pick(ctx) {
            self.target = Some(target);
            self.retries = 0;
            let _ = ctx.tasks.push_back(Task::move_to(target));
        }
        GoalProgress::Working
    }

    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![(
            "leaving for",
            match self.target {
                Some(cell) => format!("{}, {}", cell.x, cell.y),
                None => "nowhere yet".to_string(),
            },
        )]
    }
}
