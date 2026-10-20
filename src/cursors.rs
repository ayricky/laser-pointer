//! The cursor styles the laser can wear. Each is an SVG in `assets/cursors/`:
//! - `viewBox="0 0 48 48"`, drawn at 48 logical pixels and scaled with the monitor's DPI;
//! - the text `COLOR` becomes the laser colour as `#rrggbb`;
//! - only what Direct2D's SVG renderer supports: shapes, paths, groups, transforms, fills and
//!   strokes, and linear or radial gradients. No filters, masks, text, CSS or `currentColor`.
//!
//! Glows are radial gradients, and every style carries a thin contrasting edge so it reads on
//! white slides, black slides and busy footage alike.

/// The square every style is drawn in, in logical pixels.
pub const SIZE: f32 = 48.0;

/// How a style moves as the laser turns on and while the button is held. The picker page
/// (docs/picker/engine.js) shows the same motions.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Motion {
    /// Swells into place with a little overshoot, squeezes while drawing.
    Grow,
    /// Turns in from a quarter turn, twists 45° while drawing.
    Spin,
    /// Settles in from larger, contracts while drawing.
    Pulse,
    /// Drops in tilted, leans in while drawing.
    Tilt,
}

pub struct CursorStyle {
    /// Stored in the settings file.
    pub id: &'static str,
    pub name: &'static str,
    /// The point that clicks, in viewBox units.
    pub hotspot: (f32, f32),
    pub motion: Motion,
    source: &'static str,
}

fn ease_out(k: f32) -> f32 {
    1.0 - (1.0 - k).powi(3)
}

/// Overshoots a little before settling, like something switched on.
pub fn back_out(k: f32) -> f32 {
    let (c1, c3) = (1.4, 2.4);
    1.0 + c3 * (k - 1.0).powi(3) + c1 * (k - 1.0).powi(2)
}

impl Motion {
    /// How long switching on takes, in seconds.
    pub fn ignite_secs(self) -> f32 {
        if self == Self::Spin {
            0.26
        } else {
            0.2
        }
    }

    /// Scale, and rotation in degrees, about the hotspot. `ignite` runs 0→1 as the laser turns
    /// on (1 once settled), `press` 0→1 while the button is held.
    pub fn pose(self, ignite: f32, press: f32) -> (f32, f32) {
        let k = ignite.clamp(0.0, 1.0);
        let settling = k < 1.0;
        let (mut scale, mut turn) = (1.0, 0.0);
        match self {
            Self::Grow => {
                if settling {
                    scale = 0.35 + 0.65 * back_out(k);
                }
                scale *= 1.0 - 0.14 * press;
            }
            Self::Spin => {
                if settling {
                    scale = 0.5 + 0.5 * back_out(k);
                    turn = -90.0 * (1.0 - ease_out(k));
                }
                scale *= 1.0 - 0.08 * press;
                turn += 45.0 * press;
            }
            Self::Pulse => {
                if settling {
                    scale = 1.6 - 0.6 * ease_out(k);
                }
                scale *= 1.0 - 0.25 * press;
            }
            Self::Tilt => {
                if settling {
                    scale = 0.6 + 0.4 * back_out(k);
                    turn = 20.0 * (1.0 - ease_out(k));
                }
                scale *= 1.0 - 0.06 * press;
                turn -= 12.0 * press;
            }
        }
        (scale, turn)
    }
}

impl CursorStyle {
    pub fn svg(&self, color: u32) -> String {
        self.source.replace("COLOR", &format!("#{color:06x}"))
    }
}

const CENTRE: (f32, f32) = (SIZE / 2.0, SIZE / 2.0);

pub const STYLES: [CursorStyle; 8] = [
    CursorStyle {
        id: "soft-glow",
        name: "Soft glow",
        hotspot: CENTRE,
        motion: Motion::Grow,
        source: include_str!("../assets/cursors/soft-glow.svg"),
    },
    CursorStyle {
        id: "segment-ring",
        name: "Segment ring",
        hotspot: CENTRE,
        motion: Motion::Spin,
        source: include_str!("../assets/cursors/segment-ring.svg"),
    },
    CursorStyle {
        id: "classic-cross",
        name: "Classic cross",
        hotspot: CENTRE,
        motion: Motion::Spin,
        source: include_str!("../assets/cursors/classic-cross.svg"),
    },
    CursorStyle {
        id: "cross-dot",
        name: "Cross and dot",
        hotspot: CENTRE,
        motion: Motion::Spin,
        source: include_str!("../assets/cursors/cross-dot.svg"),
    },
    CursorStyle {
        id: "circle-dot",
        name: "Circle and dot",
        hotspot: CENTRE,
        motion: Motion::Pulse,
        source: include_str!("../assets/cursors/circle-dot.svg"),
    },
    CursorStyle {
        id: "chevron",
        name: "Chevron",
        hotspot: CENTRE,
        motion: Motion::Tilt,
        source: include_str!("../assets/cursors/chevron.svg"),
    },
    CursorStyle {
        id: "brackets",
        name: "Brackets",
        hotspot: CENTRE,
        motion: Motion::Spin,
        source: include_str!("../assets/cursors/brackets.svg"),
    },
    CursorStyle {
        id: "micro-dot",
        name: "Micro dot",
        hotspot: CENTRE,
        motion: Motion::Grow,
        source: include_str!("../assets/cursors/micro-dot.svg"),
    },
];

/// Unknown ids fall back to the first style, the soft glow.
pub fn index_of(id: &str) -> usize {
    STYLES.iter().position(|style| style.id == id).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_motion_settles_exactly_at_rest() {
        for motion in [Motion::Grow, Motion::Spin, Motion::Pulse, Motion::Tilt] {
            assert_eq!(motion.pose(1.0, 0.0), (1.0, 0.0), "{motion:?}");
            let (start, _) = motion.pose(0.0, 0.0);
            assert!(start != 1.0, "{motion:?} doesn't move as it switches on");
            assert!(motion.pose(1.0, 1.0) != (1.0, 0.0), "{motion:?} doesn't react to a press");
        }
    }

    #[test]
    fn ids_are_unique_and_found() {
        for (index, style) in STYLES.iter().enumerate() {
            assert_eq!(index_of(style.id), index);
        }
        assert_eq!(index_of("no-such-cursor"), 0);
    }

    #[test]
    fn every_style_takes_the_colour_and_avoids_unsupported_svg() {
        for style in &STYLES {
            let svg = style.svg(0x123456);
            assert!(svg.contains("#123456"), "{} ignores the laser colour", style.id);
            assert!(!svg.contains("COLOR"));
            for unsupported in ["filter", "mask", "<text", "<style", "var(", "currentColor"] {
                assert!(!svg.contains(unsupported), "{} uses {unsupported}", style.id);
            }
            let (x, y) = style.hotspot;
            assert!((0.0..SIZE).contains(&x) && (0.0..SIZE).contains(&y));
        }
    }

    /// Each style through its motion: switching on, settled, half pressed, pressed. Drawn with
    /// the transform the app uses for cursor frames, to check pivots and directions by eye.
    #[test]
    #[ignore = "writes a PNG to look at"]
    fn motion_frames() {
        use crate::snapshot;
        use windows::core::Interface;
        use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_SIZE_F};
        use windows::Win32::Graphics::Direct2D::ID2D1DeviceContext5;
        use windows::Win32::UI::Shell::SHCreateMemStream;
        use windows_numerics::Matrix3x2;

        snapshot::init();
        let poses = [(0.0, 0.0), (0.35, 0.0), (0.7, 0.0), (1.0, 0.0), (1.0, 0.5), (1.0, 1.0)];
        let cell = 96.0;
        let canvas = snapshot::Canvas::new((cell * poses.len() as f32) as u32, (cell * STYLES.len() as f32) as u32, 96);
        let context: ID2D1DeviceContext5 = canvas.target.cast().unwrap();
        unsafe {
            context.BeginDraw();
            context.Clear(Some(&D2D1_COLOR_F { r: 0.95, g: 0.95, b: 0.96, a: 1.0 }));
            for (row, style) in STYLES.iter().enumerate() {
                let stream = SHCreateMemStream(Some(style.svg(0xff2a2a).as_bytes())).unwrap();
                let document = context.CreateSvgDocument(&stream, D2D_SIZE_F { width: SIZE, height: SIZE }).unwrap();
                for (column, (ignite, press)) in poses.iter().enumerate() {
                    let (zoom, degrees) = style.motion.pose(*ignite, *press);
                    let (sin, cos) = degrees.to_radians().sin_cos();
                    let (x, y) = style.hotspot;
                    let (m11, m12, m21, m22) = (zoom * cos, zoom * sin, -zoom * sin, zoom * cos);
                    let (ox, oy) = (column as f32 * cell + 24.0, row as f32 * cell + 24.0);
                    context.SetTransform(&Matrix3x2 {
                        M11: m11,
                        M12: m12,
                        M21: m21,
                        M22: m22,
                        M31: ox + x - (m11 * x + m21 * y),
                        M32: oy + y - (m12 * x + m22 * y),
                    });
                    context.DrawSvgDocument(&document);
                }
            }
            context.SetTransform(&Matrix3x2::identity());
            context.EndDraw(None, None).unwrap();
        }
        canvas.save("cursor-motions.png");
    }
}
