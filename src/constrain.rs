//! Straight lines on demand: hold Alt while drawing for a horizontal line, Shift+Alt for a
//! vertical one.

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Lock {
    #[default]
    None,
    Horizontal,
    Vertical,
}

impl Lock {
    pub fn from_modifiers(alt: bool, shift: bool) -> Self {
        match (alt, shift) {
            (true, false) => Self::Horizontal,
            (true, true) => Self::Vertical,
            _ => Self::None,
        }
    }

    /// Pins a sample to the line through `anchor`.
    pub fn project(self, (x, y): (f32, f32), (ax, ay): (f32, f32)) -> (f32, f32) {
        match self {
            Self::None => (x, y),
            Self::Horizontal => (x, ay),
            Self::Vertical => (ax, y),
        }
    }
}

/// What the stroke in progress should do when the held modifiers change.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Change {
    /// Nothing changed.
    Keep,
    /// A line starts here: pin the following samples to a line through the latest point.
    /// Switching direction re-anchors too, so one stroke can turn a sharp corner.
    Anchor,
    /// The line ended: start a fresh stroke at the cursor rather than drawing a diagonal back
    /// to it from the end of the line.
    Restart,
}

pub fn change(before: Lock, after: Lock) -> Change {
    match (before, after) {
        _ if before == after => Change::Keep,
        (_, Lock::None) => Change::Restart,
        _ => Change::Anchor,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_choose_the_direction() {
        assert_eq!(Lock::from_modifiers(false, false), Lock::None);
        assert_eq!(Lock::from_modifiers(false, true), Lock::None);
        assert_eq!(Lock::from_modifiers(true, false), Lock::Horizontal);
        assert_eq!(Lock::from_modifiers(true, true), Lock::Vertical);
    }

    #[test]
    fn projection_pins_one_axis_to_the_anchor() {
        let anchor = (10.0, 20.0);
        assert_eq!(Lock::Horizontal.project((55.0, 31.0), anchor), (55.0, 20.0));
        assert_eq!(Lock::Vertical.project((55.0, 31.0), anchor), (10.0, 31.0));
        assert_eq!(Lock::None.project((55.0, 31.0), anchor), (55.0, 31.0));
    }

    #[test]
    fn transitions() {
        use Lock::*;
        assert_eq!(change(None, None), Change::Keep);
        assert_eq!(change(Horizontal, Horizontal), Change::Keep);
        assert_eq!(change(None, Horizontal), Change::Anchor);
        assert_eq!(change(Horizontal, Vertical), Change::Anchor);
        assert_eq!(change(Vertical, None), Change::Restart);
    }
}
