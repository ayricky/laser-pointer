//! Where the cursor is heading, so the head of a stroke can be drawn a frame ahead of it.
//!
//! The overlay's frames reach the screen a refresh or so after the mouse moved, so a head drawn
//! where the mouse was trails behind the hardware cursor. Velocity comes from a 1€ filter (Casiez
//! et al., 2012): smoothed hard while the hand is slow, so the head doesn't jitter, and barely
//! while it is fast, so it doesn't lag. Prediction stays off for slow moves and sharp turns,
//! where a guess would overshoot more than it helps.

use std::time::{Duration, Instant};

/// How far ahead the head is drawn, in seconds. Windows Ink's sample settles on 8 ms.
pub const AHEAD: f32 = 0.008;
/// Slower than this, in pixels a second at 100 % scale, the head isn't moved ahead at all.
const SLOW: f32 = 150.0;
/// The furthest ahead the head is drawn, in pixels at 100 % scale.
const REACH: f32 = 24.0;
/// A newer step turning further than this from the smoothed direction is a turn, not a curve.
const TURN_COS: f32 = 0.866;
/// Samples closer together than this, in milliseconds, are taken together.
const MIN_GAP: u32 = 2;
/// Once no input has come for this long, the hand has stopped: the head goes back to the cursor.
const STILL: Duration = Duration::from_millis(12);
/// The 1€ filter's cutoff at rest, in hertz, and how fast it opens up with speed.
const MIN_CUTOFF: f32 = 4.0;
const BETA: f32 = 0.02;

#[derive(Default)]
pub struct Predictor {
    /// The last sample a step was measured from: position and time in milliseconds.
    anchor: Option<(f32, f32, u32)>,
    velocity: Option<(f32, f32)>,
    /// The direction of the newest step, to spot sharp turns.
    step: (f32, f32),
    /// When the last sample came in.
    fed: Option<Instant>,
}

impl Predictor {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Takes a sample, at `time` in milliseconds as Windows stamps mouse input.
    pub fn add(&mut self, x: f32, y: f32, time: u32) {
        self.fed = Some(Instant::now());
        let Some((ax, ay, at)) = self.anchor else {
            self.anchor = Some((x, y, time));
            return;
        };
        let gap = time.wrapping_sub(at);
        // Out of order, or stamped in the same few milliseconds: wait for a clearer step.
        if !(MIN_GAP..0x8000_0000).contains(&gap) {
            return;
        }
        // A pause long enough to start over, say the hand stopped.
        if gap > 100 {
            self.anchor = Some((x, y, time));
            self.velocity = None;
            return;
        }
        let dt = gap as f32 / 1000.0;
        let raw = ((x - ax) / dt, (y - ay) / dt);
        self.step = raw;
        self.velocity = Some(match self.velocity {
            None => raw,
            Some((vx, vy)) => {
                let cutoff = MIN_CUTOFF + BETA * vx.hypot(vy);
                let tau = 1.0 / (std::f32::consts::TAU * cutoff);
                let alpha = 1.0 / (1.0 + tau / dt);
                (vx + alpha * (raw.0 - vx), vy + alpha * (raw.1 - vy))
            }
        });
        self.anchor = Some((x, y, time));
    }

    /// How far ahead of the cursor to draw the head, in pixels, at `scale` times 100 %, as of
    /// `now`.
    pub fn lead(&self, scale: f32, now: Instant) -> (f32, f32) {
        if self.fed.is_none_or(|fed| now.saturating_duration_since(fed) > STILL) {
            return (0.0, 0.0);
        }
        let Some((vx, vy)) = self.velocity else { return (0.0, 0.0) };
        let speed = vx.hypot(vy);
        if speed < SLOW * scale {
            return (0.0, 0.0);
        }
        let (sx, sy) = self.step;
        let step = sx.hypot(sy);
        if step < 1e-3 || (sx * vx + sy * vy) / (step * speed) < TURN_COS {
            return (0.0, 0.0);
        }
        let reach = (speed * AHEAD).min(REACH * scale);
        (vx / speed * reach, vy / speed * reach)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moving(velocity: (f32, f32), samples: u32) -> Predictor {
        let mut predictor = Predictor::default();
        for i in 0..samples {
            let t = i as f32 * 0.004;
            predictor.add(velocity.0 * t, velocity.1 * t, i * 4);
        }
        predictor
    }

    #[test]
    fn a_steady_move_is_led_by_one_frame() {
        let (dx, dy) = moving((1000.0, 0.0), 20).lead(1.0, Instant::now());
        assert!((dx - 8.0).abs() < 0.5 && dy.abs() < 1e-3, "{dx}, {dy}");
    }

    #[test]
    fn a_hand_that_stops_is_not_led() {
        let predictor = moving((1000.0, 0.0), 20);
        assert_eq!(predictor.lead(1.0, Instant::now() + STILL * 2), (0.0, 0.0));
    }

    #[test]
    fn slow_hands_and_new_strokes_are_not_led() {
        assert_eq!(moving((100.0, 0.0), 20).lead(1.0, Instant::now()), (0.0, 0.0));
        assert_eq!(moving((1000.0, 0.0), 1).lead(1.0, Instant::now()), (0.0, 0.0));
    }

    #[test]
    fn a_fast_flick_is_led_no_further_than_the_reach() {
        let (dx, dy) = moving((0.0, 9000.0), 20).lead(1.5, Instant::now());
        assert!(dx.abs() < 1e-3 && (dy - REACH * 1.5).abs() < 1e-3);
    }

    #[test]
    fn a_sharp_turn_stops_the_lead() {
        let mut predictor = moving((1000.0, 0.0), 20);
        // Straight down from where it was, 4 ms later.
        predictor.add(76.0, 4.0, 80);
        assert_eq!(predictor.lead(1.0, Instant::now()), (0.0, 0.0));
    }

    #[test]
    fn samples_stamped_together_wait_and_a_pause_starts_over() {
        let mut predictor = moving((1000.0, 0.0), 20);
        let before = predictor.lead(1.0, Instant::now());
        predictor.add(80.0, 0.0, 76);
        assert_eq!(predictor.lead(1.0, Instant::now()), before);
        predictor.add(80.0, 0.0, 400);
        assert_eq!(predictor.lead(1.0, Instant::now()), (0.0, 0.0));
    }
}
