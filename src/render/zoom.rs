//! A short blink over a zoom cut. The old canvas is retained until fully black.

use super::PixelZoom;
use bevy::prelude::*;

const CLOSE_SECONDS: f32 = 0.14;
const BLACK_SECONDS: f32 = 0.045;
const OPEN_SECONDS: f32 = 0.20;

#[derive(Clone, Copy)]
enum Phase {
    Idle,
    Closing(f32),
    Black(f32),
    Opening(f32),
}

#[derive(Resource)]
pub(super) struct ZoomBlink {
    zoom: PixelZoom,
    phase: Phase,
}

impl ZoomBlink {
    pub(super) fn new(zoom: PixelZoom) -> Self {
        Self {
            zoom,
            phase: Phase::Idle,
        }
    }

    pub(super) fn advance(&mut self, zoom: PixelZoom, dt: f32) {
        if zoom != self.zoom {
            self.zoom = zoom;
            // Coalesce rapid presses into the current blink. Reversing a
            // reopening blink keeps the eyelids continuous, without flashing.
            match self.phase {
                Phase::Idle => {
                    self.phase = Phase::Closing(0.0);
                    return;
                }
                Phase::Opening(amount) => {
                    self.phase = Phase::Closing(amount);
                    return;
                }
                Phase::Black(_) => self.phase = Phase::Black(0.0),
                Phase::Closing(_) => {}
            }
        }
        self.phase = match self.phase {
            Phase::Closing(amount) => {
                let amount = amount + dt / CLOSE_SECONDS;
                if amount >= 1.0 {
                    Phase::Black(0.0)
                } else {
                    Phase::Closing(amount)
                }
            }
            // Do not skip straight across black, even on a very slow frame:
            // the replacement canvas needs a presented frame under cover.
            Phase::Black(elapsed) if elapsed + dt >= BLACK_SECONDS => Phase::Opening(1.0),
            Phase::Black(elapsed) => Phase::Black(elapsed + dt),
            Phase::Opening(amount) => {
                let amount = amount - dt / OPEN_SECONDS;
                if amount <= 0.0 {
                    Phase::Idle
                } else {
                    Phase::Opening(amount)
                }
            }
            Phase::Idle => Phase::Idle,
        };
    }

    pub(super) fn closing(&self) -> bool {
        matches!(self.phase, Phase::Closing(_))
    }

    pub(super) fn closure(&self) -> f32 {
        let amount = match self.phase {
            Phase::Idle => 0.0,
            Phase::Black(_) => 1.0,
            Phase::Closing(amount) | Phase::Opening(amount) => amount,
        };
        amount * amount * (3.0 - 2.0 * amount)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_is_hidden_by_black_before_the_new_canvas_is_revealed() {
        for dt in [1.0 / 144.0, 1.0 / 30.0, 0.5] {
            let mut blink = ZoomBlink::new(PixelZoom::default());
            let target = PixelZoom::new(6.0);
            blink.advance(target, dt);
            assert!(blink.closing());
            let mut saw_black = false;
            for _ in 0..150 {
                blink.advance(target, dt);
                if !blink.closing() && !saw_black {
                    assert_eq!(blink.closure(), 1.0, "the cut must be completely covered");
                    saw_black = true;
                }
            }
            assert!(saw_black);
            assert_eq!(
                blink.closure(),
                0.0,
                "idle must restore exact unblurred pixels"
            );
        }
    }

    #[test]
    fn rapid_zoom_changes_reverse_the_lids_without_a_flash() {
        let mut blink = ZoomBlink::new(PixelZoom::default());
        let target = PixelZoom::new(5.0);
        blink.advance(target, 0.0);
        blink.advance(target, CLOSE_SECONDS);
        blink.advance(target, BLACK_SECONDS);
        blink.advance(target, OPEN_SECONDS * 0.5);
        let before = blink.closure();
        assert!(before > 0.0 && before < 1.0);
        let target = PixelZoom::new(3.0);
        blink.advance(target, 0.016);
        assert_eq!(blink.closure(), before);
        assert!(blink.closing());
        blink.advance(target, CLOSE_SECONDS);
        assert_eq!(blink.closure(), 1.0);
    }

    #[test]
    fn initial_zoom_and_unchanged_limits_do_not_blink() {
        for factor in PixelZoom::LEVELS {
            let zoom = PixelZoom::new(factor);
            let mut blink = ZoomBlink::new(zoom);
            blink.advance(zoom, 1.0);
            assert_eq!(blink.closure(), 0.0);
        }
    }
}
