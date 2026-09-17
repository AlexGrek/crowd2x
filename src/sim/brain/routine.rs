//! Routines: what arranges the priority list.
//!
//! A routine owns **priorities and nothing else** — it does not queue tasks,
//! does not move anything, and does not know what pursuing a goal involves.
//! Keeping the two halves apart is what lets several routines run on one brain
//! without fighting: each raises what it cares about ([`Goals::raise_to`] is a
//! max), and the executors below never see who asked.

use crate::sim::biology::Biology;
use crate::sim::entity::{Body, Think};

use super::goal::Goals;

/// What a routine may read while it arranges the list. Read-only: the one
/// thing a routine writes is the list, and it is handed that separately.
pub struct RoutineCtx<'a> {
    pub think: &'a Think<'a>,
    pub body: &'a Body,
    /// The body's stats and which processes are running in it. `None` for a
    /// kind that has no needs.
    pub biology: Option<&'a Biology>,
}

pub trait Routine: Send + Sync {
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
