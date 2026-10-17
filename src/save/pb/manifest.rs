//! `manifest.pb`: what a save is, read on its own by the Load screen.

/// What the Load screen lists, and the format check every load starts with.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Manifest {
    /// [`crate::save::FORMAT`] when written. A newer one than this build
    /// knows is refused.
    #[prost(uint32, tag = "1")]
    pub format: u32,
    /// `CARGO_PKG_VERSION` of the build that wrote it — for a person reading
    /// an error, never for a decision.
    #[prost(string, tag = "2")]
    pub game_version: String,
    /// What the player called it.
    #[prost(string, tag = "3")]
    pub name: String,
    /// A quick save: one of the few kept ([`crate::save::QUICK_SAVES_KEPT`]),
    /// pruned by this flag and never by the name.
    #[prost(bool, tag = "4")]
    pub quick: bool,
    /// When it was written, in milliseconds since the Unix epoch, UTC.
    #[prost(int64, tag = "5")]
    pub created_unix_ms: i64,
    /// The same moment as the player's clock read it, for showing.
    #[prost(string, tag = "6")]
    pub created_local: String,
    /// The map the game was playing.
    #[prost(string, tag = "7")]
    pub map_title: String,
    /// Ticks processed.
    #[prost(uint64, tag = "8")]
    pub tick: u64,
    /// Watched seconds since the world began ([`crate::sim::GameState::elapsed`]).
    #[prost(double, tag = "9")]
    pub elapsed: f64,
    /// How many entities are alive.
    #[prost(uint64, tag = "10")]
    pub population: u64,
}
