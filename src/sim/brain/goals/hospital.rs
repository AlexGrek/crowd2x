//! Seek a reserved hospital bed; nurses make rounds between occupied beds.
use super::super::{GoalCtx, GoalExecutor, GoalId, GoalProgress, Task, TaskResult};
use super::{Seek, Stand, stand_beside};
use crate::map::Point;
use crate::save::{Invalid, pb};
use crate::sim::JobKind;
use crate::sim::clock::{HOUR, MINUTE, watched};

#[derive(Default)]
pub struct RecoverGoal {
    seek: Seek,
}
impl RecoverGoal {
    pub fn new() -> Self {
        Self::default()
    }
    fn choose(ctx: &GoalCtx<'_>) -> Option<Point> {
        let uid = ctx.body.uid();
        if let Some(bed) = ctx.think.hospital.bed_for(uid) {
            return Some(bed.cell);
        }
        ctx.think
            .hospital
            .beds()
            .iter()
            .filter(|b| {
                b.patient.is_none()
                    && ctx.think.features.has(crate::sim::feature::FeatureKind::HospitalBed, b.cell)
                    && ctx.think.may_use(b.cell, ctx.body.home())
                    && ctx.think.occupancy.is_free_for(b.cell, uid)
                    && !ctx.memory.out_of_reach().contains(b.cell, ctx.think.clock.elapsed())
            })
            .min_by_key(|b| (ctx.body.center_position().manhattan_distance(b.cell), b.cell))
            .map(|b| b.cell)
    }
    fn plan(&self, ctx: &mut GoalCtx<'_>) -> bool {
        let Some(bed) = self.seek.target else { return false };
        let approach = stand_beside(ctx, bed);
        if matches!(approach, Stand::Nowhere) {
            return false;
        }
        if ctx.think.hospital.bed_for(ctx.body.uid()).is_none() {
            // Reserve before setting out, so a crowd does not all walk to
            // the same bed and block the nurses from reaching it.
            let _ = ctx.tasks.push_back(Task::admission(bed));
        }
        match approach {
            Stand::Nowhere => return false,
            Stand::Here => {}
            Stand::At(cell) => {
                let _ = ctx.tasks.push_back(Task::move_beside(cell));
            }
        }
        let _ = ctx.tasks.push_back(Task::hospital_rest(bed, watched(HOUR)));
        true
    }
}
impl GoalExecutor for RecoverGoal {
    fn goal(&self) -> GoalId {
        GoalId::Recover
    }
    fn save(&self) -> pb::Executor {
        pb::Executor::Seek(self.seek.save())
    }
    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        self.seek = Seek::restore_only(saved, self.goal())?;
        Ok(())
    }
    fn prioritized(&mut self, ctx: &mut GoalCtx<'_>) {
        self.seek.steady();
        // Pick afresh: a bed may have been taken while a need was in charge.
        self.seek.target = Self::choose(ctx);
        if !self.plan(ctx) {
            self.seek.forget();
        }
    }
    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        if ctx.biology.is_none() {
            return GoalProgress::Blocked;
        }
        if last == TaskResult::Success && matches!(ctx.finished, Some(Task::ReleaseBed(_))) {
            return GoalProgress::Blocked;
        }
        if last == TaskResult::Success
            && matches!(ctx.finished, Some(Task::Admission(_)))
            && ctx.think.hospital.bed_for(ctx.body.uid()).is_none()
        {
            ctx.tasks.clear();
            self.seek.forget();
            return GoalProgress::Blocked;
        }
        if last == TaskResult::Failed {
            ctx.tasks.clear();
            if matches!(ctx.finished, Some(Task::MoveTo(_)) | Some(Task::HospitalRest(_)))
                && self.seek.setback(ctx, Self::choose)
                && self.plan(ctx)
            {
                return GoalProgress::Working;
            }
            if let Some(bed) = ctx.think.hospital.bed_for(ctx.body.uid()) {
                let _ = ctx.tasks.push_back(Task::release_bed(bed.cell));
                self.seek.forget();
                return GoalProgress::Working;
            }
            self.seek.forget();
            return GoalProgress::Blocked;
        }
        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }
        self.seek.target = Self::choose(ctx);
        if self.plan(ctx) {
            GoalProgress::Working
        } else {
            GoalProgress::Blocked
        }
    }
    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![("hospital bed", format!("{:?}", self.seek.target))]
    }
}

#[derive(Default)]
pub struct CareGoal {
    seek: Seek,
}
impl CareGoal {
    pub fn new() -> Self {
        Self::default()
    }
    fn plan(&self, ctx: &mut GoalCtx<'_>) -> bool {
        let Some(bed) = self.seek.target else { return false };
        match stand_beside(ctx, bed) {
            Stand::Nowhere => return false,
            Stand::Here => {}
            Stand::At(cell) => {
                let _ = ctx.tasks.push_back(Task::move_beside(cell));
            }
        }
        let _ = ctx.tasks.push_back(Task::care_patient(bed, watched(10.0 * MINUTE)));
        true
    }
}
impl GoalExecutor for CareGoal {
    fn goal(&self) -> GoalId {
        GoalId::Care
    }
    fn save(&self) -> pb::Executor {
        pb::Executor::Seek(self.seek.save())
    }
    fn restore(&mut self, saved: &pb::Executor) -> Result<(), Invalid> {
        self.seek = Seek::restore_only(saved, self.goal())?;
        Ok(())
    }
    fn prioritized(&mut self, _ctx: &mut GoalCtx<'_>) {
        self.seek.forget();
    }
    fn process(&mut self, ctx: &mut GoalCtx<'_>, last: TaskResult) -> GoalProgress {
        let Some(job) = ctx.job.filter(|j| j.kind == JobKind::Nurse) else {
            return GoalProgress::Blocked;
        };
        if last == TaskResult::Failed {
            ctx.tasks.clear();
            if matches!(ctx.finished, Some(Task::MoveTo(_))) && self.seek.setback(ctx, |_| None) && self.plan(ctx) {
                return GoalProgress::Working;
            }
            self.seek.forget();
            return GoalProgress::Blocked;
        }
        if last == TaskResult::Success && matches!(ctx.finished, Some(Task::CarePatient(_))) {
            self.seek.forget();
            return GoalProgress::Achieved;
        }
        if !ctx.tasks.is_empty() {
            return GoalProgress::Working;
        }
        let here = ctx.body.center_position();
        let target = ctx
            .think
            .hospital
            .beds()
            .iter()
            .filter(|_| job.shift.is_working_hours(ctx.think.clock))
            .filter(|b| {
                b.patient.is_some()
                    && ctx.think.occupancy.occupant(b.cell) == b.patient
                    && !ctx.memory.out_of_reach().contains(b.cell, ctx.think.clock.elapsed())
            })
            .min_by_key(|b| (b.care, here.manhattan_distance(b.cell), b.cell))
            .map(|b| b.cell);
        if let Some(target) = target {
            self.seek.choose(target);
            if self.plan(ctx) {
                GoalProgress::Working
            } else {
                GoalProgress::Blocked
            }
        } else {
            // Stay at the hospital between rounds; no per-tick route search.
            match stand_beside(ctx, job.workplace) {
                Stand::Nowhere => return GoalProgress::Blocked,
                Stand::At(cell) => {
                    let _ = ctx.tasks.push_back(Task::move_beside(cell));
                }
                Stand::Here => {}
            }
            let _ = ctx.tasks.push_back(Task::wait(watched(5.0 * MINUTE)));
            GoalProgress::Working
        }
    }
    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        vec![("patient bed", format!("{:?}", self.seek.target))]
    }
}
