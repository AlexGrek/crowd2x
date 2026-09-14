//! What a unit can see: [`Perception`].

use crate::sim::entity::{Body, Think};

/// What a unit can currently see. **Not implemented.**
///
/// Here as a zero-sized field and an empty `observe` because the pipeline has a
/// step 0, and a shape with a hole in it reads as a bug where a named empty
/// reads as a plan. What this wants is a spatial index on `Think` — a uniform
/// grid of cell buckets rebuilt once per tick — and **not** a per-unit scan of
/// the crowd, which is one of the three mistakes that killed the earlier
/// prototype. `Think` is a struct precisely so that index can be added without
/// touching a signature.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Perception;

impl Perception {
    /// Look around. Costs nothing, because it does nothing yet.
    #[inline]
    pub fn observe(&mut self, ctx: &Think<'_>, body: &Body) {
        let _ = (ctx, body);
    }
}
