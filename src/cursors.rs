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

pub struct CursorStyle {
    /// Stored in the settings file.
    pub id: &'static str,
    pub name: &'static str,
    /// The point that clicks, in viewBox units.
    pub hotspot: (f32, f32),
    source: &'static str,
}

impl CursorStyle {
    pub fn svg(&self, color: u32) -> String {
        self.source.replace("COLOR", &format!("#{color:06x}"))
    }
}

const CENTRE: (f32, f32) = (SIZE / 2.0, SIZE / 2.0);

pub const STYLES: [CursorStyle; 8] = [
    CursorStyle {
        id: "dot",
        name: "Laser dot",
        hotspot: CENTRE,
        source: include_str!("../assets/cursors/dot.svg"),
    },
    CursorStyle {
        id: "halo",
        name: "Halo",
        hotspot: CENTRE,
        source: include_str!("../assets/cursors/halo.svg"),
    },
    CursorStyle {
        id: "ring",
        name: "Ring",
        hotspot: CENTRE,
        source: include_str!("../assets/cursors/ring.svg"),
    },
    CursorStyle {
        id: "focus",
        name: "Focus",
        hotspot: CENTRE,
        source: include_str!("../assets/cursors/focus.svg"),
    },
    CursorStyle {
        id: "neon",
        name: "Neon",
        hotspot: CENTRE,
        source: include_str!("../assets/cursors/neon.svg"),
    },
    CursorStyle {
        id: "cross",
        name: "Crosshair",
        hotspot: CENTRE,
        source: include_str!("../assets/cursors/cross.svg"),
    },
    CursorStyle {
        id: "pen",
        name: "Pen",
        hotspot: (7.0, 7.0),
        source: include_str!("../assets/cursors/pen.svg"),
    },
    CursorStyle {
        id: "arrow",
        name: "Arrow",
        hotspot: (7.0, 5.5),
        source: include_str!("../assets/cursors/arrow.svg"),
    },
];

/// Unknown ids fall back to the first style, the laser dot.
pub fn index_of(id: &str) -> usize {
    STYLES.iter().position(|style| style.id == id).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
