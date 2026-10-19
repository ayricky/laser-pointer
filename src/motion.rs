//! Time-based transitions keep interaction independent of the display refresh rate.

use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
pub enum Easing {
    Cubic,
    Decelerate,
}

pub fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

/// For (0,0,0,1), x = t³, so the inverse needs no iterative solver.
pub fn decelerate(t: f32) -> f32 {
    let parameter = t.cbrt();
    parameter * parameter * (3.0 - 2.0 * parameter)
}

#[derive(Clone, Copy)]
pub struct Motion {
    pub value: f32,
    pub from: f32,
    pub to: f32,
    pub start: Instant,
    pub duration: Duration,
    easing: Easing,
}

impl Motion {
    pub fn new(value: f32, now: Instant) -> Self {
        Self {
            value,
            from: value,
            to: value,
            start: now,
            duration: Duration::ZERO,
            easing: Easing::Cubic,
        }
    }

    pub fn sample(&mut self, now: Instant) -> f32 {
        let t = if self.duration.is_zero() {
            1.0
        } else {
            (now.saturating_duration_since(self.start).as_secs_f32() / self.duration.as_secs_f32())
                .min(1.0)
        };
        let amount = match self.easing {
            Easing::Cubic => ease_out_cubic(t),
            Easing::Decelerate => decelerate(t),
        };
        self.value = if t >= 1.0 {
            self.to
        } else {
            self.from + (self.to - self.from) * amount
        };
        self.value
    }

    pub fn retarget(&mut self, to: f32, now: Instant, milliseconds: u64, easing: Easing) {
        if to == self.to {
            return;
        }
        self.sample(now);
        self.from = self.value;
        self.to = to;
        self.start = now;
        self.duration = Duration::from_millis(milliseconds);
        self.easing = easing;
        self.sample(now);
    }

    pub fn active(&self) -> bool {
        self.value != self.to
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easings_are_monotonic_and_meet_endpoints() {
        for easing in [ease_out_cubic, decelerate] {
            assert_eq!(easing(0.0), 0.0);
            assert_eq!(easing(1.0), 1.0);
            let values: Vec<_> = (0..=100).map(|i| easing(i as f32 / 100.0)).collect();
            assert!(values.windows(2).all(|pair| pair[0] <= pair[1]));
        }
        assert!((decelerate(0.125) - 0.5).abs() < 0.0001);
    }

    #[test]
    fn retargeting_preserves_continuity_and_finishes_exactly() {
        let now = Instant::now();
        let mut motion = Motion::new(0.0, now);
        motion.retarget(1.0, now, 100, Easing::Cubic);
        let middle = now + Duration::from_millis(40);
        let value = motion.sample(middle);
        motion.retarget(0.0, middle, 83, Easing::Cubic);
        assert_eq!(motion.value, value);
        assert!(motion.active());
        assert_eq!(motion.sample(middle + Duration::from_millis(83)), 0.0);
        assert!(!motion.active());
        motion.retarget(1.0, now, 0, Easing::Decelerate);
        assert_eq!(motion.value, 1.0);
        assert!(!motion.active());
    }
}
