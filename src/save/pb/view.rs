//! `view.pb`: how the game screen was looking at the world. Not the world —
//! a save without it loads as a game opened fresh, looking at the middle of
//! the map at the default zoom and speed.

#[derive(Clone, Copy, PartialEq, ::prost::Message)]
pub struct View {
    /// Where the camera was centred, in world pixels.
    #[prost(float, tag = "1")]
    pub camera_x: f32,
    #[prost(float, tag = "2")]
    pub camera_y: f32,
    /// Original integer zoom; 0 for the default. Kept for older readers, with
    /// the overview represented as x1. Prefer `zoom_factor` when present.
    #[prost(uint32, tag = "3")]
    pub zoom: u32,
    /// The game speed's multiplier (`crate::game::speed::SPEEDS`); 0 for the
    /// default. A multiplier and not an index, so a ladder of speeds that
    /// changes does not change what a save asks for.
    #[prost(float, tag = "4")]
    pub speed: f32,
    #[prost(bool, tag = "5")]
    pub paused: bool,
    /// Screen pixels per canvas pixel, including the x0.5 overview. Zero in
    /// older saves: fall back to the original integer `zoom` field.
    #[prost(float, tag = "6")]
    pub zoom_factor: f32,
}
