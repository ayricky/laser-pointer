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

    /// Renders every candidate cursor and tray icon with Direct2D's own SVG renderer, as the app
    /// will, failing on any it rejects, and saves them as one sheet to look at.
    #[test]
    #[ignore = "renders the design candidates; writes a PNG"]
    fn candidates_render_in_direct2d() {
        use crate::snapshot;
        use windows::core::Interface;
        use windows::Win32::Graphics::Direct2D::Common::{D2D1_COLOR_F, D2D_SIZE_F};
        use windows::Win32::Graphics::Direct2D::ID2D1DeviceContext5;
        use windows::Win32::UI::Shell::SHCreateMemStream;
        use windows_numerics::Matrix3x2;

        snapshot::init();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/candidates");
        let mut sources: Vec<(String, f32)> = Vec::new();
        for (folder, size) in [("cursors", 48.0), ("tray", 16.0)] {
            let mut names: Vec<_> = std::fs::read_dir(root.join(folder))
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "svg"))
                .collect();
            names.sort();
            for path in names {
                let text = std::fs::read_to_string(&path).unwrap();
                let text = text.replace("COLOR", "#ff2a2a").replace("INK", "#ffffff").replace("DOT", "#ff2a2a");
                sources.push((text, size));
            }
        }
        assert_eq!(sources.len(), 30 + 60);
        let cell = 64.0;
        let columns = 15;
        let rows = sources.len().div_ceil(columns);
        let canvas = snapshot::Canvas::new((cell * columns as f32) as u32, (cell * rows as f32) as u32, 96);
        let context: ID2D1DeviceContext5 = canvas.target.cast().expect("SVG needs Windows 10 1703");
        unsafe {
            context.BeginDraw();
            context.Clear(Some(&D2D1_COLOR_F { r: 0.12, g: 0.12, b: 0.13, a: 1.0 }));
            for (index, (source, size)) in sources.iter().enumerate() {
                let stream = SHCreateMemStream(Some(source.as_bytes())).unwrap();
                let document = context
                    .CreateSvgDocument(&stream, D2D_SIZE_F { width: *size, height: *size })
                    .unwrap_or_else(|error| panic!("candidate {index} rejected: {error}"));
                let zoom = 48.0 / size;
                let (x, y) = ((index % columns) as f32 * cell + 8.0, (index / columns) as f32 * cell + 8.0);
                context.SetTransform(&Matrix3x2 { M11: zoom, M22: zoom, M31: x, M32: y, ..Default::default() });
                context.DrawSvgDocument(&document);
            }
            context.SetTransform(&Matrix3x2::identity());
            context.EndDraw(None, None).unwrap();
        }
        canvas.save("candidates-direct2d.png");
    }
}
