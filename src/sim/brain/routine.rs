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

use crate::save::{invalid, pb, Invalid};
use crate::sim::biology::Biology;
use crate::sim::entity::{Body, Think};
use crate::sim::inventory::Inventory;
use crate::sim::job::Job;

use super::goal::Goals;
use super::memory::Memory;
use super::routines::{Need, NeedRoutine, OrdersRoutine, ReadyRoutine, SleepRoutine, StayBusyRoutine, WorkRoutine};

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
    /// What it remembers — an order it was given, so far
    /// ([`OrdersRoutine`]).
    pub memory: &'a Memory,
    /// Its job, if it has one — when the shift is, for
    /// [`WorkRoutine`](super::routines::WorkRoutine).
    pub job: Option<&'a Job>,
    /// What it is carrying, `None` for a kind that carries nothing: a body in
    /// hand an undertaker has still to see to the crematorium
    /// ([`WorkRoutine`](super::routines::WorkRoutine)), what a trip out has
    /// brought back so far ([`ReadyRoutine`]).
    pub inventory: Option<&'a Inventory>,
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
    Orders(OrdersRoutine),
    Work(WorkRoutine),
    Ready(ReadyRoutine),
    /// A priority a test turns up and down from outside the brain.
    #[cfg(test)]
    Dial(Dial),
}

impl Routine {
    /// Keep `need` met.
    pub const fn need(need: &'static Need) -> Routine {
        Routine::Need(NeedRoutine::new(need))
    }

    /// Go to bed at night or when exhausted, and stay there through the night.
    pub const fn sleep() -> Routine {
        Routine::Sleep(SleepRoutine::new())
    }

    /// Wander, a little, always.
    pub const fn stay_busy() -> Routine {
        Routine::StayBusy(StayBusyRoutine)
    }

    /// Carry out an order, when nothing more pressing is wanted.
    pub const fn orders() -> Routine {
        Routine::Orders(OrdersRoutine)
    }

    /// Go to work while the shift is due, if it has a job.
    pub const fn work() -> Routine {
        Routine::Work(WorkRoutine::new())
    }

    /// Before a trip out into the wasteland, see to every need first.
    pub const fn ready() -> Routine {
        Routine::Ready(ReadyRoutine::new())
    }

    pub fn name(&self) -> &'static str {
        match self {
            Routine::Need(routine) => routine.name(),
            Routine::Sleep(routine) => routine.name(),
            Routine::StayBusy(routine) => routine.name(),
            Routine::Orders(routine) => routine.name(),
            Routine::Work(routine) => routine.name(),
            Routine::Ready(routine) => routine.name(),
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
            Routine::Orders(routine) => routine.arrange(ctx, goals),
            Routine::Work(routine) => routine.arrange(ctx, goals),
            Routine::Ready(routine) => routine.arrange(ctx, goals),
            #[cfg(test)]
            Routine::Dial(routine) => routine.arrange(ctx, goals),
        }
    }

    pub fn debug_fields(&self) -> Vec<(&'static str, String)> {
        match self {
            Routine::Need(routine) => routine.debug_fields(),
            Routine::Sleep(routine) => routine.debug_fields(),
            Routine::StayBusy(routine) => routine.debug_fields(),
            Routine::Orders(routine) => routine.debug_fields(),
            Routine::Work(routine) => routine.debug_fields(),
            Routine::Ready(routine) => routine.debug_fields(),
            #[cfg(test)]
            Routine::Dial(routine) => routine.debug_fields(),
        }
    }

    // --- saving ---

    /// What this routine remembers between ticks, under its name. A routine
    /// with nothing to remember is named all the same, so that a load can
    /// check it is restoring the brain it saved.
    pub(crate) fn save(&self) -> pb::RoutineState {
        let (committed, pending) = match self {
            Routine::Need(routine) => (routine.is_committed(), 0),
            Routine::Sleep(routine) => (routine.is_committed(), 0),
            Routine::StayBusy(StayBusyRoutine) => (false, 0),
            Routine::Orders(OrdersRoutine) => (false, 0),
            Routine::Work(routine) => (routine.is_on_shift(), 0),
            Routine::Ready(routine) => {
                let (ready, pending) = routine.state();
                (ready, pending as u32)
            }
            // A test's dial is set from outside every tick; nothing to keep.
            #[cfg(test)]
            Routine::Dial(_) => (false, 0),
        };
        pb::RoutineState { name: self.name().to_string(), committed, pending }
    }

    /// Put back what [`Routine::save`] kept, into the routine in the same
    /// place in a freshly built brain — which must be the same routine.
    pub(crate) fn restore(&mut self, saved: &pb::RoutineState) -> Result<(), Invalid> {
        if saved.name != self.name() {
            return invalid(format!("routine {:?} where {:?} should be", saved.name, self.name()));
        }
        if saved.pending != 0 && !matches!(self, Routine::Ready(_)) {
            return invalid(format!("routine {:?} has no needs to see to", saved.name));
        }
        match self {
            Routine::Need(routine) => routine.set_committed(saved.committed),
            Routine::Sleep(routine) => routine.set_committed(saved.committed),
            Routine::Work(routine) => routine.set_on_shift(saved.committed),
            Routine::Ready(routine) => routine.set_state(saved.committed, saved.pending)?,
            Routine::StayBusy(_) | Routine::Orders(_) => {}
            #[cfg(test)]
            Routine::Dial(_) => {}
        }
        Ok(())
    }

    /// Whether this is the routine that gets a worker ready to go out, and it
    /// is seeing to a need for it right now — for the job menu.
    pub fn is_getting_ready(&self) -> bool {
        matches!(self, Routine::Ready(routine) if routine.is_getting_ready())
    }

    /// Whether this is the routine that gets a worker ready to go out, and it
    /// says the worker is ready — for the job's goal.
    pub fn is_ready_to_go_out(&self) -> bool {
        matches!(self, Routine::Ready(routine) if routine.state().0)
    }
}
