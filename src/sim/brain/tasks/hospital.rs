//! Hospital admission, bed rest, and a nurse's completed treatment.
use super::super::action::{Action, ActionState};
use super::super::task::{Task, TaskCtx, TaskExecutor, TaskResult, TimedTask};
use crate::map::Point;
use crate::sim::Effect;
use crate::sim::biology::Event;
use crate::sim::clock::{HOUR, TIME_SCALE};

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Admission {
    pub bed: Point,
    pub seconds: f32,
}
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ReleaseBed {
    pub bed: Point,
    pub seconds: f32,
}
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct HospitalRest {
    pub bed: Point,
    pub seconds: f32,
}
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CarePatient {
    pub bed: Point,
    pub seconds: f32,
}

impl TaskExecutor for Admission {
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        let uid = ctx.walk.body().uid();
        if ctx.biology.is_none()
            || !ctx
                .think
                .hospital
                .at(self.bed)
                .is_some_and(|b| b.patient.is_none() || b.patient == Some(uid))
        {
            return TaskResult::Failed;
        }
        *ctx.effect = Effect::Admit { at: self.bed };
        TaskResult::Success
    }
    fn describe(&self) -> String {
        "admission to hospital".into()
    }
}

impl TaskExecutor for ReleaseBed {
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        *ctx.effect = Effect::ReleaseBed { at: self.bed };
        TaskResult::Success
    }
    fn describe(&self) -> String {
        "return an unreachable hospital bed".into()
    }
}

impl TaskExecutor for HospitalRest {
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        let uid = ctx.walk.body().uid();
        if ctx.here().manhattan_distance(self.bed) > 1
            || ctx.biology.is_none()
            || !ctx.think.hospital.bed_for(uid).is_some_and(|b| b.cell == self.bed)
        {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = if ctx.here() == self.bed {
                Action::interact(self.bed, self.seconds)
            } else {
                Action::enter(ctx.walk, self.bed)
            };
            return TaskResult::Executing;
        }
        let getting_in = matches!(ctx.action, Action::Move { .. });
        if !getting_in {
            if ctx.here() != self.bed {
                return TaskResult::Failed;
            }
            // Count only actual bed time, including interrupted stretches.
            let elapsed = ctx.action.elapsed();
            let world_seconds = ctx.think.dt.min((self.seconds - elapsed).max(0.0)) * TIME_SCALE;
            // A stable rate per patient: 2, 3, 4 or 5 percentage points/hour.
            let rate = 2.0 + (uid.body() % 4) as f32;
            if let Some(biology) = ctx.biology.as_deref_mut() {
                biology.handle(Event::Treated {
                    hp: rate * world_seconds / HOUR,
                });
                biology.handle(Event::Slept { world_seconds });
            }
        }
        match ctx.advance_action() {
            ActionState::Finished if getting_in => {
                *ctx.action = Action::interact(self.bed, self.seconds);
                TaskResult::Executing
            }
            state => TaskResult::of(state),
        }
    }
    fn describe(&self) -> String {
        format!("recover in hospital bed at {}, {}", self.bed.x, self.bed.y)
    }
}

impl TaskExecutor for CarePatient {
    fn execute(&mut self, ctx: &mut TaskCtx<'_>) -> TaskResult {
        let Some(patient) = ctx.think.hospital.at(self.bed).and_then(|b| b.patient) else {
            return TaskResult::Failed;
        };
        if ctx.here().manhattan_distance(self.bed) != 1 || ctx.think.occupancy.occupant(self.bed) != Some(patient) {
            return TaskResult::Failed;
        }
        if ctx.action.is_none() {
            *ctx.action = Action::interact(self.bed, self.seconds);
            return TaskResult::Executing;
        }
        match ctx.advance_action() {
            ActionState::Finished => {
                if let Some(biology) = ctx.biology.as_deref_mut() {
                    biology.handle(Event::Worked {
                        world_seconds: self.seconds * TIME_SCALE,
                    });
                }
                *ctx.effect = Effect::Care { at: self.bed, patient };
                TaskResult::Success
            }
            state => TaskResult::of(state),
        }
    }
    fn interrupted(&mut self, action: &Action, biology: Option<&mut crate::sim::biology::Biology>) {
        if let (Action::Interact { elapsed, .. }, Some(biology)) = (action, biology) {
            biology.handle(Event::Worked {
                world_seconds: *elapsed * TIME_SCALE,
            });
        }
    }
    fn describe(&self) -> String {
        format!("take care of patient at {}, {}", self.bed.x, self.bed.y)
    }
}

impl Task {
    pub const fn admission(bed: Point) -> Self {
        Self::Admission(Admission { bed, seconds: 0.0 })
    }
    pub const fn release_bed(bed: Point) -> Self {
        Self::ReleaseBed(ReleaseBed { bed, seconds: 0.0 })
    }
    pub const fn hospital_rest(bed: Point, seconds: f32) -> Self {
        Self::HospitalRest(HospitalRest { bed, seconds })
    }
    pub const fn care_patient(bed: Point, seconds: f32) -> Self {
        Self::CarePatient(CarePatient { bed, seconds })
    }
}

macro_rules! timed {
    ($kind:ident, $name:literal) => {
        impl TimedTask for $kind {
            const NAME: &'static str = $name;
            fn new(bed: Point, seconds: f32) -> Self {
                Self { bed, seconds }
            }
            fn cell(&self) -> Point {
                self.bed
            }
            fn seconds(&self) -> f32 {
                self.seconds
            }
        }
    };
}
timed!(Admission, "hospital admission");
timed!(ReleaseBed, "release hospital bed");
timed!(HospitalRest, "hospital rest");
timed!(CarePatient, "care patient");
