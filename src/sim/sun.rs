//! The sun: how high it is, how bright and what colour, at a time of day.
//!
//! A function of the [`Clock`](super::Clock) and nothing else, so it is
//! never stored — anything that wants the sun asks for it. Two things do:
//!
//! * **the picture** — `crate::lighting` multiplies the canvas by
//!   [`Sun::ambient`], the light under open sky before any lamp;
//! * **the street light controllers** — a photocell on a distribution box
//!   (`crate::map::utilities::CONTROLLER`) that switches the box on while
//!   [`Sun::is_dark`] — read by `GameState::follow_daylight`.
//!
//! Both read this one model, so a street lamp comes on exactly when the
//! street on screen has gone dark, and a test can say when that is without
//! a renderer.
//!
//! # The model
//!
//! The sun's **elevation** is a cosine of the hour, highest at [`NOON`] and
//! crossing the horizon at [`SUNRISE`] and [`SUNSET`] — normalised so it is
//! 1 at noon and 0 on the horizon; below it at night. **Intensity** rises
//! out of the night through a twilight while the sun is still under the
//! horizon and is whole by mid-morning, so most of a day is drawn exactly as
//! the art was painted. **Colour** is white high up, golden near the horizon
//! and a violet twilight just under it. The night is not black: moonlight,
//! [`NIGHT`], is what is left with no sun at all.

/// Where the sun crosses the horizon in the morning, in hours.
pub const SUNRISE: f32 = 6.0;
/// Where it goes back under, in hours.
pub const SUNSET: f32 = 20.0;
/// Halfway between: the sun at its highest.
pub const NOON: f32 = (SUNRISE + SUNSET) / 2.0;

/// The light with no sun at all: a dark blue moonlight in which a lamp is
/// what lights a room, and a street.
pub const NIGHT: [f32; 3] = [0.12, 0.14, 0.26];
/// The sun high up: white, so the art looks as drawn.
const WHITE: [f32; 3] = [1.0, 1.0, 1.0];
/// The sun on the horizon.
const GOLDEN: [f32; 3] = [1.0, 0.62, 0.38];
/// What the sky gives off with the sun just under the horizon.
const TWILIGHT: [f32; 3] = [0.62, 0.45, 0.72];

/// Elevation at which twilight begins: the sun this far under the horizon
/// still lights the sky a little.
const TWILIGHT_DEPTH: f32 = -0.2;
/// Elevation above which the sun is whole and white: half past seven in the
/// morning to half past six at night.
const FULL_DAY: f32 = 0.3;

/// The intensity under which a street light controller's photocell calls it
/// night: ten to eight in the evening to ten past six in the morning.
pub const PHOTOCELL_DARK: f32 = 0.45;

/// The sun at one moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sun {
    /// 1 at noon, 0 on the horizon, negative under it.
    pub elevation: f32,
    /// How much of a full day's light the sun gives, 0 to 1.
    pub intensity: f32,
    /// Linear RGB of the sunlight, before intensity.
    pub color: [f32; 3],
}

impl Sun {
    /// The sun at `hours` since midnight, fractional. Wraps at 24.
    pub fn at(hours: f32) -> Sun {
        let h = hours.rem_euclid(24.0);
        let tau = std::f32::consts::TAU;
        // The cosine is 1 at noon and `horizon` at sunrise and sunset.
        let horizon = (tau * (SUNSET - NOON) / 24.0).cos();
        let elevation = ((tau * (h - NOON) / 24.0).cos() - horizon) / (1.0 - horizon);
        let intensity = smoothstep(TWILIGHT_DEPTH, FULL_DAY, elevation);
        let color = if elevation >= 0.0 {
            mix(GOLDEN, WHITE, smoothstep(0.0, FULL_DAY, elevation))
        } else {
            mix(TWILIGHT, GOLDEN, smoothstep(TWILIGHT_DEPTH, 0.0, elevation))
        };
        Sun { elevation, intensity, color }
    }

    /// The light under open sky, before any lamp: the sunlight as strong as
    /// it is, and the moonlight for the rest. Exactly white by day.
    pub fn ambient(&self) -> [f32; 3] {
        let i = self.intensity;
        std::array::from_fn(|c| self.color[c] * i + NIGHT[c] * (1.0 - i))
    }

    /// Whether a street light controller's photocell sees night.
    pub fn is_dark(&self) -> bool {
        self.intensity < PHOTOCELL_DARK
    }
}

fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|c| a[c] + (b[c] - a[c]) * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_day_is_white_and_the_night_is_moonlight() {
        assert_eq!(Sun::at(13.0).ambient(), WHITE);
        assert_eq!(Sun::at(9.0).ambient(), WHITE, "mid-morning is a whole day");
        assert_eq!(Sun::at(17.0).ambient(), WHITE);
        assert_eq!(Sun::at(1.0).ambient(), NIGHT);
        assert_eq!(Sun::at(23.0).ambient(), NIGHT);
        assert_eq!(Sun::at(3.5).intensity, 0.0);
    }

    #[test]
    fn the_sun_is_on_the_horizon_at_sunrise_and_sunset_and_highest_at_noon() {
        assert!(Sun::at(SUNRISE).elevation.abs() < 1e-5);
        assert!(Sun::at(SUNSET).elevation.abs() < 1e-5);
        assert!((Sun::at(NOON).elevation - 1.0).abs() < 1e-5);
        assert!(Sun::at(1.0).elevation < -0.5);
    }

    #[test]
    fn the_evening_sun_is_dimmer_and_warmer_than_the_midday_sun() {
        let noon = Sun::at(NOON);
        for hours in [19.0, 19.5, 20.0] {
            let evening = Sun::at(hours);
            assert!(evening.intensity < noon.intensity, "{hours}");
            let warmth = |sun: Sun| sun.color[0] / sun.color[2];
            assert!(warmth(evening) > warmth(noon), "{hours}: {:?}", evening.color);
            assert!(evening.ambient()[0] > evening.ambient()[2], "{hours}: dusk is warm");
        }
    }

    #[test]
    fn the_light_rises_all_morning_and_falls_all_evening() {
        let mut last = Sun::at(4.0).intensity;
        for minute in 1..=(NOON - 4.0) as i32 * 60 {
            let now = Sun::at(4.0 + minute as f32 / 60.0).intensity;
            assert!(now >= last, "{minute}");
            last = now;
        }
        for minute in 1..=(22.0 - NOON) as i32 * 60 {
            let now = Sun::at(NOON + minute as f32 / 60.0).intensity;
            assert!(now <= last, "{minute}");
            last = now;
        }
    }

    #[test]
    fn nothing_jumps_from_one_minute_to_the_next_midnight_included() {
        let mut last = Sun::at(0.0).ambient();
        for minute in 1..=24 * 60 {
            let now = Sun::at(minute as f32 / 60.0).ambient();
            for c in 0..3 {
                assert!((now[c] - last[c]).abs() < 0.02, "{minute}: {last:?} -> {now:?}");
            }
            last = now;
        }
        assert_eq!(Sun::at(24.0), Sun::at(0.0));
    }

    #[test]
    fn a_photocell_sees_night_from_dusk_to_dawn() {
        assert!(!Sun::at(8.0).is_dark());
        assert!(!Sun::at(19.0).is_dark(), "still light at seven");
        assert!(Sun::at(20.5).is_dark());
        assert!(Sun::at(0.0).is_dark());
        assert!(Sun::at(5.5).is_dark());
        assert!(!Sun::at(7.0).is_dark());
    }
}
