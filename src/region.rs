//! A handful of rectangles covering what changed on screen, so a frame repaints and presents
//! only those parts. Two strokes at opposite corners stay two small rectangles instead of one
//! that spans the screen.

use crate::stroke::Bounds;

const CAPACITY: usize = 6;

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Region {
    rects: [Option<Bounds>; CAPACITY],
}

fn area(bounds: Bounds) -> f32 {
    (bounds.right - bounds.left).max(0.0) * (bounds.bottom - bounds.top).max(0.0)
}

fn touches(a: Bounds, b: Bounds) -> bool {
    a.left <= b.right && b.left <= a.right && a.top <= b.bottom && b.top <= a.bottom
}

impl Region {
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.rects.iter().all(Option::is_none)
    }

    pub fn rects(&self) -> impl Iterator<Item = Bounds> + '_ {
        self.rects.iter().flatten().copied()
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn union(&mut self, other: &Region) {
        for bounds in other.rects() {
            self.add(bounds);
        }
    }

    pub fn add(&mut self, bounds: Bounds) {
        if area(bounds) <= 0.0 {
            return;
        }
        // Absorb whatever it overlaps, repeatedly, since each merge can reach further.
        let mut bounds = bounds;
        while let Some(slot) = self.rects.iter_mut().find(|r| r.is_some_and(|r| touches(r, bounds))) {
            bounds = bounds.union(slot.take().unwrap_or(bounds));
        }
        if let Some(slot) = self.rects.iter_mut().find(|r| r.is_none()) {
            *slot = Some(bounds);
            return;
        }
        // Full: merge the pair that wastes the least area, then try again.
        let all = self.rects.map(|rect| rect.unwrap_or(bounds));
        let mut best = (0, 1, f32::MAX);
        for i in 0..CAPACITY {
            for j in i + 1..CAPACITY {
                let waste = area(all[i].union(all[j])) - area(all[i]) - area(all[j]);
                if waste < best.2 {
                    best = (i, j, waste);
                }
            }
        }
        let merged = all[best.0].union(all[best.1]);
        self.rects[best.1] = None;
        self.rects[best.0] = None;
        self.add(merged);
        self.add(bounds);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f32, y: f32, size: f32) -> Bounds {
        Bounds {
            left: x,
            top: y,
            right: x + size,
            bottom: y + size,
        }
    }

    #[test]
    fn far_apart_changes_stay_separate() {
        let mut region = Region::default();
        region.add(square(0.0, 0.0, 10.0));
        region.add(square(1000.0, 1000.0, 10.0));
        assert_eq!(region.rects().count(), 2);
    }

    #[test]
    fn overlapping_changes_merge_transitively() {
        let mut region = Region::default();
        region.add(square(0.0, 0.0, 10.0));
        region.add(square(20.0, 0.0, 10.0));
        region.add(square(5.0, 0.0, 20.0));
        let merged = Bounds {
            left: 0.0,
            top: 0.0,
            right: 30.0,
            bottom: 20.0,
        };
        assert_eq!(region.rects().collect::<Vec<_>>(), vec![merged]);
    }

    #[test]
    fn overflow_merges_neighbours_and_still_covers_everything() {
        let mut region = Region::default();
        let squares: Vec<Bounds> = (0..20).map(|i| square(i as f32 * 50.0, 0.0, 10.0)).collect();
        for &s in &squares {
            region.add(s);
        }
        assert!(region.rects().count() <= CAPACITY);
        for s in squares {
            assert!(region.rects().any(|r| r.left <= s.left && r.right >= s.right));
        }
    }

    #[test]
    fn empty_rectangles_are_ignored() {
        let mut region = Region::default();
        region.add(square(5.0, 5.0, 0.0));
        assert!(region.is_empty());
    }
}
