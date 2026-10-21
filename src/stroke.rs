//! Stroke geometry and drawing, shared by the overlay and the settings preview.

use std::f32::consts::PI;
use std::time::Instant;

use windows::core::Result;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED, D2D1_FILL_MODE_WINDING,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1DeviceContext1, ID2D1Factory, ID2D1GeometryRealization, ID2D1PathGeometry, ID2D1RenderTarget,
    ID2D1SolidColorBrush, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_DEFAULT_FLATTENING_TOLERANCE,
    D2D1_LAYER_OPTIONS1_NONE, D2D1_LAYER_PARAMETERS1,
};
use windows_numerics::Matrix3x2;
use windows_numerics::Vector2;

use crate::settings::{Look, STYLE_GLOW, STYLE_NEON};

// Dropping tiny moves keeps a slow hand from leaving a jittery cluster of points.
pub const MIN_STEP: f32 = 2.0;
// Longest straight segment left after curving between samples, in DIPs.
const CURVE_STEP: f32 = 3.0;
const CAP_STEPS: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct Point {
    pub x: f32,
    pub y: f32,
    pub at: Instant,
}

#[derive(Clone, Debug)]
pub struct Stroke {
    pub points: Vec<Point>,
    /// Captured when the stroke began, so switching pens never restyles what is already drawn.
    pub look: Look,
    /// DPI scale of the monitor it was drawn on.
    pub scale: f32,
    /// A corrected shape fades out all at once instead of shrinking along its trail.
    pub whole: bool,
    /// Where each part after the first begins, for shapes drawn in pieces like an arrow.
    pub breaks: Vec<usize>,
}

impl Stroke {
    pub fn new(point: Point, look: Look, scale: f32) -> Self {
        Self {
            points: vec![point],
            look,
            scale,
            whole: false,
            breaks: Vec::new(),
        }
    }

    /// Where it could paint, glow included, without building its geometry.
    pub fn bounds(&self) -> Option<Bounds> {
        self.bounds_from(0)
    }

    /// The same for its points from `start` on.
    pub fn bounds_from(&self, start: usize) -> Option<Bounds> {
        let points = self.points.get(start..)?;
        let first = points.first()?;
        let reach = self.look.width as f32 / 2.0 * self.scale * layers(self.look.style)[0].0 + 2.0;
        let mut bounds = Bounds::around(first.x, first.y, reach);
        for point in points {
            bounds = bounds.union(Bounds::around(point.x, point.y, reach));
        }
        Some(bounds)
    }

    /// Fading strokes are done once every point has aged out.
    pub fn expired(&self, now: Instant) -> bool {
        !self.look.ink
            && self.points.last().is_none_or(|last| {
                now.duration_since(last.at).as_millis() >= u128::from(self.look.fade_ms)
            })
    }

    /// Drops points that have aged out, keeping the newest faded one so the tail still tapers.
    /// Painting does this too, but a stroke can be off screen and never painted.
    pub fn prune(&mut self, now: Instant) {
        if self.look.ink || self.whole {
            return;
        }
        let fade = u128::from(self.look.fade_ms);
        let aged = self
            .points
            .iter()
            .take_while(|point| now.duration_since(point.at).as_millis() >= fade)
            .count();
        let drop = if aged == self.points.len() { aged } else { aged.saturating_sub(1) };
        self.points.drain(..drop);
    }

    pub fn translate(&mut self, dx: f32, dy: f32) {
        for point in &mut self.points {
            point.x += dx;
            point.y += dy;
        }
    }
}

/// Buffers reused frame to frame so drawing doesn't allocate.
#[derive(Default)]
pub struct Scratch {
    radii: Vec<f32>,
    path: Vec<Vector2>,
    centre: Vec<Vector2>,
    widths: Vec<f32>,
    layer: Vec<f32>,
    outline: Vec<Vector2>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Bounds {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Bounds {
    pub fn around(x: f32, y: f32, radius: f32) -> Self {
        Self {
            left: x - radius,
            top: y - radius,
            right: x + radius,
            bottom: y + radius,
        }
    }

    pub fn union(self, other: Self) -> Self {
        Self {
            left: self.left.min(other.left),
            top: self.top.min(other.top),
            right: self.right.max(other.right),
            bottom: self.bottom.max(other.bottom),
        }
    }
}

pub fn union(a: Option<Bounds>, b: Option<Bounds>) -> Option<Bounds> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.union(b)),
        (a, b) => a.or(b),
    }
}

pub fn rgba(rgb: u32, alpha: f32) -> D2D1_COLOR_F {
    let channel = |shift: u32| ((rgb >> shift) & 0xff) as f32 / 255.0;
    D2D1_COLOR_F {
        r: channel(16),
        g: channel(8),
        b: channel(0),
        a: alpha,
    }
}

/// (width multiplier, opacity, mix towards white) for each pass, widest first.
fn layers(style: u32) -> &'static [(f32, f32, f32)] {
    match style {
        STYLE_GLOW => &[(3.4, 0.10, 0.0), (2.1, 0.20, 0.0), (1.0, 1.0, 0.0)],
        STYLE_NEON => &[
            (3.8, 0.12, 0.0),
            (2.3, 0.26, 0.0),
            (1.0, 1.0, 0.0),
            (0.45, 1.0, 0.85),
        ],
        _ => &[(1.0, 1.0, 0.0)],
    }
}

/// Drops points that have faded out, then fills what is left, `fade` times as opaque as usual.
/// Returns the area drawn on.
pub fn draw(
    factory: &ID2D1Factory,
    target: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    stroke: &mut Stroke,
    now: Instant,
    fade: f32,
    scratch: &mut Scratch,
) -> Option<Bounds> {
    let Stroke {
        points,
        look,
        scale,
        whole,
        breaks,
    } = stroke;
    let (look, scale) = (*look, *scale);
    let base = look.width as f32 / 2.0 * scale;
    radii(points, now, base, &look, scale, *whole, &mut scratch.radii);
    if !look.ink && !*whole {
        let faded = scratch.radii.iter().take_while(|&&radius| radius < 0.1).count();
        // Keep one faded point so the tail still tapers all the way to nothing.
        let drop = if faded == points.len() {
            faded
        } else {
            faded.saturating_sub(1)
        };
        points.drain(..drop);
        scratch.radii.drain(..drop);
    }
    if points.is_empty() {
        return None;
    }
    let mut bounds = None;
    let mut start = 0;
    for end in breaks.iter().copied().chain([points.len()]) {
        let part = start..end.min(points.len());
        start = end;
        if part.is_empty() {
            continue;
        }
        smooth(&points[part.clone()], look.smoothing as usize, &mut scratch.path);
        if *whole {
            // A snapped shape is already its clean outline, kept to its corners once it settles:
            // curving through those would round them off.
            scratch.centre.clone_from(&scratch.path);
            scratch.widths.clear();
            scratch.widths.extend_from_slice(&scratch.radii[part]);
        } else {
            curve_through(
                &scratch.path,
                &scratch.radii[part],
                CURVE_STEP * scale,
                &mut scratch.centre,
                &mut scratch.widths,
            );
        }
        let opacity = look.opacity as f32 / 100.0 * fade;
        for &(width, alpha, white) in layers(look.style) {
            scratch.layer.clear();
            scratch.layer.extend(scratch.widths.iter().map(|radius| radius * width));
            outline(&scratch.centre, &scratch.layer, &mut scratch.outline);
            bounds = union(bounds, extent(&scratch.outline));
            let mut color = rgba(look.color, alpha * opacity);
            color.r += (1.0 - color.r) * white;
            color.g += (1.0 - color.g) * white;
            color.b += (1.0 - color.b) * white;
            unsafe { brush.SetColor(&color) };
            let _ = fill(factory, target, brush, &scratch.outline);
        }
    }
    bounds
}

fn ease_out(k: f32) -> f32 {
    1.0 - (1.0 - k).powi(4)
}

/// Thinner the older a point is and the further it sits behind the head, whichever is smaller.
/// Measured in distance rather than point count, so the tail is the same length however fast
/// the mouse reports. A whole shape thins evenly, by the age of its newest point.
fn radii(
    points: &[Point],
    now: Instant,
    base: f32,
    look: &Look,
    scale: f32,
    whole: bool,
    radii: &mut Vec<f32>,
) {
    radii.clear();
    radii.resize(points.len(), base);
    if look.ink {
        return;
    }
    let fade = look.fade_ms as f32 / 1000.0;
    if whole {
        let newest = points.iter().map(|point| point.at).max();
        let age = newest.map_or(0.0, |at| 1.0 - now.duration_since(at).as_secs_f32() / fade);
        radii.fill(base * ease_out(age.max(0.0)));
        return;
    }
    let trail = look.trail as f32 * scale;
    let mut behind = 0.0;
    for index in (0..points.len()).rev() {
        let point = &points[index];
        if let Some(next) = points.get(index + 1) {
            behind += (next.x - point.x).hypot(next.y - point.y);
        }
        let age = 1.0 - now.duration_since(point.at).as_secs_f32() / fade;
        let length = 1.0 - behind / trail;
        radii[index] = base * ease_out(length.max(0.0)).min(ease_out(age.max(0.0)));
    }
}

/// Symmetric window that shrinks to nothing at both ends, so the head stays under the cursor.
fn smooth(points: &[Point], span: usize, out: &mut Vec<Vector2>) {
    out.clear();
    out.extend((0..points.len()).map(|index| {
        let span = span.min(index).min(points.len() - 1 - index);
        let (mut x, mut y, mut weights) = (0.0, 0.0, 0.0);
        for (offset, point) in points[index - span..=index + span].iter().enumerate() {
            let weight = (span + 1 - offset.abs_diff(span)) as f32;
            x += point.x * weight;
            y += point.y * weight;
            weights += weight;
        }
        Vector2 {
            X: x / weights,
            Y: y / weights,
        }
    }));
}

fn midpoint(a: Vector2, b: Vector2) -> Vector2 {
    Vector2 {
        X: (a.X + b.X) / 2.0,
        Y: (a.Y + b.Y) / 2.0,
    }
}

fn distance(a: Vector2, b: Vector2) -> f32 {
    (b.X - a.X).hypot(b.Y - a.Y)
}

/// Quadratic curves midpoint to midpoint with each sample as the control, flattened finely
/// enough that sparse samples from a fast flick still come out round.
fn curve_through(
    path: &[Vector2],
    radii: &[f32],
    step: f32,
    points: &mut Vec<Vector2>,
    widths: &mut Vec<f32>,
) {
    points.clear();
    widths.clear();
    let count = path.len();
    if count < 3 {
        points.extend_from_slice(path);
        widths.extend_from_slice(radii);
        return;
    }
    let mid = |index: usize| midpoint(path[index], path[index + 1]);
    let mid_radius = |index: usize| (radii[index] + radii[index + 1]) / 2.0;
    points.extend([path[0], mid(0)]);
    widths.extend([radii[0], mid_radius(0)]);
    for index in 1..count - 1 {
        let (from, control, to) = (mid(index - 1), path[index], mid(index));
        let (r0, r1, r2) = (mid_radius(index - 1), radii[index], mid_radius(index));
        let length = distance(from, control) + distance(control, to);
        let steps = ((length / step).ceil() as usize).clamp(1, 16);
        for k in 1..=steps {
            let t = k as f32 / steps as f32;
            let (a, b, c) = ((1.0 - t) * (1.0 - t), 2.0 * (1.0 - t) * t, t * t);
            points.push(Vector2 {
                X: a * from.X + b * control.X + c * to.X,
                Y: a * from.Y + b * control.Y + c * to.Y,
            });
            widths.push(a * r0 + b * r1 + c * r2);
        }
    }
    points.push(path[count - 1]);
    widths.push(radii[count - 1]);
}

/// Up one side of the stroke, round the head, back down the other side and round the tail.
fn outline(centre: &[Vector2], radii: &[f32], out: &mut Vec<Vector2>) {
    out.clear();
    let count = centre.len();
    let mut direction = Vector2 { X: 1.0, Y: 0.0 };
    let tangent = |index: usize, direction: &mut Vector2| {
        let (a, b) = (
            centre[index.saturating_sub(1)],
            centre[(index + 1).min(count - 1)],
        );
        let length = distance(a, b);
        if length > 1e-3 {
            *direction = Vector2 {
                X: (b.X - a.X) / length,
                Y: (b.Y - a.Y) / length,
            };
        }
        *direction
    };
    let offset = |index: usize, t: Vector2, along: f32, across: f32| {
        let (c, r) = (centre[index], radii[index]);
        Vector2 {
            X: c.X + (t.X * along - t.Y * across) * r,
            Y: c.Y + (t.Y * along + t.X * across) * r,
        }
    };
    let angle = |step: usize| PI * step as f32 / CAP_STEPS as f32;

    let first = tangent(0, &mut direction);
    let mut tangents_end = first;
    for index in 0..count {
        let t = tangent(index, &mut direction);
        out.push(offset(index, t, 0.0, 1.0));
        tangents_end = t;
    }
    let head = count - 1;
    out.extend((1..CAP_STEPS).map(|step| {
        offset(head, tangents_end, angle(step).sin(), angle(step).cos())
    }));
    // Walk back down the other side; tangents again, since they weren't kept.
    let start = out.len();
    let mut direction = first;
    for index in 0..count {
        let t = tangent(index, &mut direction);
        out.push(offset(index, t, 0.0, -1.0));
    }
    out[start..].reverse();
    out.extend((1..CAP_STEPS).map(|step| offset(0, first, -angle(step).sin(), -angle(step).cos())));
}

fn extent(points: &[Vector2]) -> Option<Bounds> {
    let first = points.first()?;
    let mut bounds = Bounds::around(first.X, first.Y, 0.0);
    for point in points {
        bounds.left = bounds.left.min(point.X);
        bounds.top = bounds.top.min(point.Y);
        bounds.right = bounds.right.max(point.X);
        bounds.bottom = bounds.bottom.max(point.Y);
    }
    Some(bounds)
}

fn fill(
    factory: &ID2D1Factory,
    target: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    outline: &[Vector2],
) -> Result<()> {
    if outline.len() < 3 {
        return Ok(());
    }
    unsafe { target.FillGeometry(&geometry(factory, outline)?, brush, None) };
    Ok(())
}

fn geometry(factory: &ID2D1Factory, outline: &[Vector2]) -> Result<ID2D1PathGeometry> {
    unsafe {
        let geometry = factory.CreatePathGeometry()?;
        let sink = geometry.Open()?;
        sink.SetFillMode(D2D1_FILL_MODE_WINDING);
        sink.BeginFigure(outline[0], D2D1_FIGURE_BEGIN_FILLED);
        sink.AddLines(&outline[1..]);
        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
        sink.Close()?;
        Ok(geometry)
    }
}

/// Points of a kept stroke in each settled piece, and how many each shares with the next, so
/// the pieces join without a seam.
const PIECE: usize = 192;
const OVERLAP: usize = 6;

/// A kept stroke grows for as long as the button is held, and drawing it whole every frame would
/// cost more the longer it got. Its settled stretch is tessellated once, a piece at a time, so a
/// frame costs the same however long the stroke is. Tied to the device that made it.
#[derive(Default)]
pub struct Pieces {
    /// Each piece's bounds, and its tessellation for each pass.
    done: Vec<(Bounds, Vec<ID2D1GeometryRealization>)>,
}

impl Pieces {
    pub fn clear(&mut self) {
        self.done.clear();
    }

    /// Where the stretch still drawn afresh each frame begins. Nothing before it changes, as
    /// long as the pieces are drawn.
    pub fn settled(&self) -> usize {
        self.done.len() * PIECE
    }
}

/// Whether `draw_kept` applies: a kept stroke, drawn as one piece of freehand.
pub fn grows(stroke: &Stroke) -> bool {
    stroke.look.ink && !stroke.whole && stroke.breaks.is_empty()
}

/// The centre line of `points[range]` and its widths, smoothed as if the whole stroke were.
fn centre_of(points: &[Point], range: std::ops::Range<usize>, look: &Look, scale: f32, scratch: &mut Scratch) {
    let span = look.smoothing as usize;
    let (from, to) = (range.start.saturating_sub(span), (range.end + span).min(points.len()));
    smooth(&points[from..to], span, &mut scratch.path);
    scratch.path.truncate(range.end - from);
    scratch.path.drain(..range.start - from);
    scratch.radii.clear();
    scratch.radii.resize(scratch.path.len(), look.width as f32 / 2.0 * scale);
    curve_through(&scratch.path, &scratch.radii, CURVE_STEP * scale, &mut scratch.centre, &mut scratch.widths);
}

/// Draws a kept stroke as `draw` would, from settled pieces and its newest stretch. Each pass is
/// filled solid inside a layer of its opacity, so where pieces overlap they don't show darker.
pub fn draw_kept(
    factory: &ID2D1Factory,
    dc: &ID2D1DeviceContext1,
    brush: &ID2D1SolidColorBrush,
    stroke: &Stroke,
    pieces: &mut Pieces,
    scratch: &mut Scratch,
) {
    draw_kept_settling(factory, dc, brush, stroke, pieces, scratch, true);
}

/// For tests: the same stroke drawn as one stretch, nothing settled.
#[cfg(test)]
pub fn draw_kept_unsettled(
    factory: &ID2D1Factory,
    dc: &ID2D1DeviceContext1,
    brush: &ID2D1SolidColorBrush,
    stroke: &Stroke,
    scratch: &mut Scratch,
) {
    draw_kept_settling(factory, dc, brush, stroke, &mut Pieces::default(), scratch, false);
}

fn draw_kept_settling(
    factory: &ID2D1Factory,
    dc: &ID2D1DeviceContext1,
    brush: &ID2D1SolidColorBrush,
    stroke: &Stroke,
    pieces: &mut Pieces,
    scratch: &mut Scratch,
    settle: bool,
) {
    let (points, look, scale) = (&stroke.points, stroke.look, stroke.scale);
    let passes = layers(look.style);
    let span = look.smoothing as usize;
    // A piece settles once every point that smooths its end is in.
    while settle && (pieces.done.len() + 1) * PIECE + OVERLAP + span < points.len() {
        let start = pieces.done.len() * PIECE;
        centre_of(points, start..start + PIECE + OVERLAP, &look, scale, scratch);
        let mut tessellated = Vec::with_capacity(passes.len());
        let mut bounds = None;
        for &(width, _, _) in passes {
            scratch.layer.clear();
            scratch.layer.extend(scratch.widths.iter().map(|radius| radius * width));
            outline(&scratch.centre, &scratch.layer, &mut scratch.outline);
            bounds = union(bounds, extent(&scratch.outline));
            let Ok(realization) = geometry(factory, &scratch.outline)
                .and_then(|geometry| unsafe { dc.CreateFilledGeometryRealization(&geometry, D2D1_DEFAULT_FLATTENING_TOLERANCE) })
            else {
                return;
            };
            tessellated.push(realization);
        }
        let Some(bounds) = bounds else { return };
        pieces.done.push((bounds, tessellated));
    }
    let start = pieces.done.len() * PIECE;
    let tail = start < points.len();
    if tail {
        centre_of(points, start..points.len(), &look, scale, scratch);
    }
    let opacity = look.opacity as f32 / 100.0;
    for (pass, &(width, alpha, white)) in passes.iter().enumerate() {
        let mut color = rgba(look.color, 1.0);
        color.r += (1.0 - color.r) * white;
        color.g += (1.0 - color.g) * white;
        color.b += (1.0 - color.b) * white;
        unsafe {
            dc.PushLayer(
                &D2D1_LAYER_PARAMETERS1 {
                    contentBounds: crate::graphics::rect_f(stroke.bounds().unwrap_or(Bounds::around(0.0, 0.0, 0.0))),
                    maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                    maskTransform: Matrix3x2::identity(),
                    opacity: alpha * opacity,
                    layerOptions: D2D1_LAYER_OPTIONS1_NONE,
                    ..Default::default()
                },
                None,
            );
            brush.SetColor(&color);
            for (_, tessellated) in &pieces.done {
                dc.DrawGeometryRealization(&tessellated[pass], brush);
            }
            if tail {
                scratch.layer.clear();
                scratch.layer.extend(scratch.widths.iter().map(|radius| radius * width));
                outline(&scratch.centre, &scratch.layer, &mut scratch.outline);
                // Tessellated like the pieces, though it changes every frame, so a stretch doesn't
                // shift by a pixel as it settles into one.
                if scratch.outline.len() >= 3 {
                    if let Ok(realization) = geometry(factory, &scratch.outline)
                        .and_then(|geometry| dc.CreateFilledGeometryRealization(&geometry, D2D1_DEFAULT_FLATTENING_TOLERANCE))
                    {
                        dc.DrawGeometryRealization(&realization, brush);
                    }
                }
            }
            dc.PopLayer();
        }
    }
}
