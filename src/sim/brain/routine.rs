//! Routines: what arranges the priority list.
//!
//! A routine owns **priorities and nothing else** — it does not queue tasks,
//! does not move anything, and does not know what pursuing a goal involves.
//! Keeping the two halves apart is what lets several routines run on one brain
//! without fighting: each raises what it cares about ([`Goals::raise_to`] is a
//! max), and the executors below never see who asked.
//!
//! # Routines without a box, and without a cap
//!
//! Every routine is its own struct ([`super::routines`]) with a
//! [`RoutineExecutor`] impl, and [`Routine`] is the enum over them, dispatched
//! by `match` — the shape [`Task`](super::task::Task) has, for a sharper
//! version of the same reason. Every routine a unit has runs every tick, a
//! human is going to have dozens, and a crowd is thousands of humans. A
//! `Box<dyn>` per routine would be dozens of allocations per unit at spawn and
//! a pointer chase per routine per unit per tick; as an enum, a unit's routines
//! are **one allocation, made when its brain is built, with every routine's
//! state inline and contiguous in it**.
//!
//! How many a brain has is whatever it was built with. The list lives in a
//! boxed slice rather than a fixed array, so there is no cap to raise and no
//! empty slots paid for by a kind with few routines — and a `Box<[Routine]>`
//! has no `push`, so the list and its order cannot change after construction,
//! which is what determinism needs from it.
//!
//! Adding a routine type is a struct, a [`RoutineExecutor`] impl, a variant
//! here, and its arm in each of [`Routine`]'s three methods. A new *need* is
//! not a new type: it is a [`Need`](super::routines::Need) for the
//! [`NeedRoutine`] that exists — unless it depends on something a stat is not,
//! like the time of day, which is what [`SleepRoutine`] is.

use crate::sim::biology::Biology;
use crate::sim::entity::{Body, Think};

use super::goal::Goals;
use super::routines::{Need, NeedRoutine, SleepRoutine, StayBusyRoutine};

#[cfg(test)]
use super::routines::Dial;

/// What a routine may read while it arranges the list. Read-only: the one
/// thing a routine writes is the list, and it is handed that separately.
pub struct RoutineCtx<'a> {
    pub think: &'a Think<'a>,
    pub body: &'a Body,
    /// The body's stats and which processes are running in it. `None` for a
    /// kind that has no needs.
    pub biology: Option<&'a Biology>,
}

/// What one routine does.
pub trait RoutineExecutor {
    fn name(&self) -> &'static str;

    /// Raise the goals this routine cares about.
    ///
    /// `&mut self`: a routine that cannot remember what it decided last tick
    /// flips its mind every time a stat crosses a threshold. Hysteresis is
    /// state, and it belongs to the routine that needs it.
    fn arrange(&mut self, ctx: &RoutineCtx<'_>, goals: &mut Goals);

    /// What this routine would tell a debugger.
    fn debug_fields(&self) -> Vec<(&'static str, String)> {
        Vec::new()
    }
}

/// One routine on a brain.
#[derive(Debug)]
pub enum Routine {
    Need(NeedRoutine),
    Sleep(SleepRoutine),
    StayBusy(StayBusyRoutine),
    /// A priority a test turns up and down from outside the brain.
    #[cfg(test)]
    Dial(Dial),
}

impl Routine {
    /// Keep `need` met.
    pub const fn need(need: &'static Need) -> Routine {
        Routine::Need(NeedRoutine::new(need))
    }

    /// Go to bed when tired enough, and stay there through the night.
    pub const fn sleep() -> Routine {
        Routine::Sleep(SleepRoutine::new())
    }

    /// Wander, a little, always.
    pub const fn stay_busy() -> Routine {
        Routine::StayBusy(StayBusyRoutine)
    }

    pub fn name(&self) -> &'static str {
        match self {
            Routine::Need(routine) => routine.name(),
            Routine::Sleep(routine) => routine.name(),
            Routine::StayBusy(routine) => routine.name(),
            #[cfg(test)]
            Routine::Dial(routine) => routine.name(),
        }
    }

    /// Run this routine's executor for a tick.
    pub fn arrange(&mut self, ctx: &RoutineCtx<'_>, goals: &mut Goals) {
        match self {
            Routine::Need(routine) => routine.arrange(ctx, goals),
            Routine::Sleep(routine) => routine.arrange(ctx, goals),
            Routine::StayBusy(routine) => routine.arrange(ctx, goals),
            #[cfg(test)]
            Routine::Dial(routine) => routine.arrange(ctx, goals),
        }
    }

    pub fn debug_fields(&self) -> Vec<(&'static str, String)> {
        match self {
            Routine::Need(routine) => routine.debug_fields(),
            Routine::Sleep(routine) => routine.debug_fields(),
            Routine::StayBusy(routine) => routine.debug_fields(),
            #[cfg(test)]
            Routine::Dial(routine) => routine.debug_fields(),
        }
    }
}
