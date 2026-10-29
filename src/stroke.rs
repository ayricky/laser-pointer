//! Stroke geometry and drawing, shared by the overlay and the settings preview.

use std::f32::consts::PI;
use std::time::Instant;

use windows::core::{Interface, Result};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_COLOR_F, D2D1_COMPOSITE_MODE_SOURCE_OVER, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED,
    D2D1_FILL_MODE_WINDING,
};
use windows::Win32::Graphics::Direct2D::{
    CLSID_D2D1GaussianBlur, CLSID_D2D1Opacity, ID2D1DeviceContext1, ID2D1Factory, ID2D1GeometryRealization,
    ID2D1Image, ID2D1PathGeometry, ID2D1RenderTarget, ID2D1SolidColorBrush, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
    D2D1_DEFAULT_FLATTENING_TOLERANCE, D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION, D2D1_INTERPOLATION_MODE_LINEAR,
    D2D1_LAYER_OPTIONS1_NONE, D2D1_LAYER_PARAMETERS1, D2D1_OPACITY_PROP_OPACITY, D2D1_PROPERTY_TYPE_FLOAT, ID2D1Effect,
};
use windows_numerics::Matrix3x2;
use windows_numerics::Vector2;

use crate::settings::{Look, STYLE_GLOW, STYLE_NEON};

// Dropping tiny moves keeps a slow hand from leaving a jittery cluster of points.
pub const MIN_STEP: f32 = 2.0;
// How far the curve between samples may stray from true once flattened, in DIPs, and the range
// each flat piece may turn through, so a wide glow's outer edge stays round as well.
const CURVE_TOLERANCE: f32 = 0.1;
const CURVE_TURN: f32 = 5.0 * PI / 180.0;
const MAX_CURVE_TURN: f32 = 20.0 * PI / 180.0;
/// How far thinning may move the outline, in DIPs. Any further and Direct2D's antialiasing of a
/// long, nearly level edge starts to show where it crosses a row of pixels.
const THIN_TOLERANCE: f32 = 0.02;
/// The most centre points one thinned run may stand for.
const MAX_THIN_RUN: usize = 32;
const CAP_STEPS: usize = 8;
/// A snapped shape glows brighter once as it settles, for this long.
pub const FLASH_SECS: f32 = 0.4;
/// The flash at its brightest: (width multiplier, opacity, mix towards white), drawn beneath.
const FLASH: (f32, f32, f32) = (3.0, 0.45, 0.35);
/// A snapped shape that fades holds still for its fade time, then fades out over this long.
pub const SHAPE_FADE_MS: u32 = 500;

/// How many samples each rectangle of a stroke's `coverage` spans.
const COVER: usize = 16;

/// The rectangle around `points`, `reach` out from each.
fn extent_of(points: &[Point], reach: f32) -> Option<Bounds> {
    let first = points.first()?;
    let mut bounds = Bounds::around(first.x, first.y, reach);
    for point in points {
        bounds = bounds.union(Bounds::around(point.x, point.y, reach));
    }
    Some(bounds)
}

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
    /// When it snapped to a shape, while that shape still glows from it.
    pub flash: Option<Instant>,
}

impl Stroke {
    pub fn new(point: Point, look: Look, scale: f32) -> Self {
        Self {
            points: vec![point],
            look,
            scale,
            whole: false,
            breaks: Vec::new(),
            flash: None,
        }
    }

    /// Where it could paint, glow included, without building its geometry.
    pub fn bounds(&self) -> Option<Bounds> {
        self.bounds_from(0)
    }

    /// The same for its points from `start` on.
    pub fn bounds_from(&self, start: usize) -> Option<Bounds> {
        let points = self.points.get(start..)?;
        extent_of(points, self.reach())
    }

    /// Where it could paint, as rectangles along it, so a long diagonal stroke covers little
    /// more than itself. Each takes in the points that smooth its ends and the next sample, since
    /// the curve between samples stays within the samples around it.
    pub fn coverage(&self) -> impl Iterator<Item = Bounds> + '_ {
        let (reach, span, count) = (self.reach(), self.look.smoothing as usize, self.points.len());
        (0..count).step_by(COVER).filter_map(move |start| {
            let points = &self.points[start.saturating_sub(span)..(start + COVER + 1 + span).min(count)];
            extent_of(points, reach)
        })
    }

    /// How far from its centre line it can paint, glow included.
    fn reach(&self) -> f32 {
        let mut widest = reach(self.look.style);
        if self.flash.is_some() {
            widest = widest.max(FLASH.0);
        }
        self.look.width as f32 / 2.0 * self.scale * widest + 2.0
    }

    /// Fading strokes are done once every point has aged out, and a shape once it has held and
    /// faded.
    pub fn expired(&self, now: Instant) -> bool {
        let life = self.look.fade_ms + if self.whole { SHAPE_FADE_MS } else { 0 };
        !self.look.ink
            && self.points.last().is_none_or(|last| {
                now.duration_since(last.at).as_millis() >= u128::from(life)
            })
    }

    /// The flash is over by `now`; drops it, so the stroke can be baked and stop repainting.
    /// Returns whether it is still glowing.
    pub fn settle_flash(&mut self, now: Instant) -> bool {
        if self.flash.is_some_and(|at| now.duration_since(at).as_secs_f32() >= FLASH_SECS) {
            self.flash = None;
        }
        self.flash.is_some()
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
    /// The direction of the centre line at each of its points, found before thinning.
    tangents: Vec<Vector2>,
    outline: Vec<Vector2>,
    halos: Halos,
}

impl Scratch {
    /// Called once each batch of drawing has ended in EndDraw: the halos drawn in the next reuse
    /// the effects this one made, and those let go of what they blurred, so a stroke that is
    /// gone isn't kept in memory by the last halo drawn from it.
    pub fn finish_frame(&mut self) {
        for (blur, _) in &self.halos.pairs[..self.halos.used] {
            unsafe { blur.SetInput(0, None::<&ID2D1Image>, false) };
        }
        self.halos.used = 0;
    }

    /// Lets go of the halo effects when the context that made them is going away, which they
    /// would otherwise keep alive, along with its device, until the next halo is drawn.
    pub fn forget_device(&mut self) {
        self.halos = Halos::default();
    }
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

/// (width multiplier, opacity, mix towards white) for each pass, the stroke itself first.
fn layers(style: u32) -> &'static [(f32, f32, f32)] {
    match style {
        // A thin near-white core, so the trail reads as light rather than paint.
        STYLE_GLOW => &[(1.0, 1.0, 0.0), (0.38, 0.85, 0.6)],
        STYLE_NEON => &[(1.0, 1.0, 0.0), (0.45, 1.0, 0.85)],
        _ => &[(1.0, 1.0, 0.0)],
    }
}

/// The light around a Glow or Neon stroke: the stroke itself blurred by `spread` times its
/// radius and drawn `strength` times as opaque beneath it, so it falls off smoothly where
/// wider copies of the stroke would end in hard steps.
#[derive(Clone, Copy)]
struct Halo {
    strength: f32,
    spread: f32,
}

fn halo(style: u32) -> Option<Halo> {
    match style {
        STYLE_GLOW => Some(Halo { strength: 0.85, spread: 2.0 }),
        STYLE_NEON => Some(Halo { strength: 1.0, spread: 2.2 }),
        _ => None,
    }
}

/// How far from its centre line a stroke paints, in multiples of its radius: a halo fades to
/// nothing three of its standard deviations past the stroke's edge.
fn reach(style: u32) -> f32 {
    halo(style).map_or(layers(style)[0].0, |halo| 1.0 + 3.0 * halo.spread)
}

/// The shapes `record` draws, blurred into a halo `opacity` times as opaque as they are. Called
/// between BeginDraw and EndDraw: they are recorded by pointing the context at a command list
/// for a moment. Recorded without the context's transform, which applies once the halo is drawn.
unsafe fn halo_image(
    dc: &ID2D1DeviceContext1,
    halos: &mut Halos,
    halo: Halo,
    radius: f32,
    opacity: f32,
    record: impl FnOnce(),
) -> Result<ID2D1Image> {
    let list = dc.CreateCommandList()?;
    let target = dc.GetTarget()?;
    let mut transform = Matrix3x2::default();
    dc.GetTransform(&mut transform);
    dc.SetTransform(&Matrix3x2::identity());
    dc.SetTarget(&list);
    record();
    dc.SetTarget(&target);
    dc.SetTransform(&transform);
    list.Close()?;
    let (blur, fade) = halos.take(dc)?;
    blur.SetInput(0, &list, true);
    blur.SetValue(D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION.0 as u32, D2D1_PROPERTY_TYPE_FLOAT, &(halo.spread * radius).to_le_bytes())?;
    fade.SetValue(D2D1_OPACITY_PROP_OPACITY.0 as u32, D2D1_PROPERTY_TYPE_FLOAT, &(halo.strength * opacity).to_le_bytes())?;
    fade.GetOutput()
}

/// Blur and opacity effects for halos, kept from frame to frame: Direct2D readies an effect the
/// first time it is drawn, which costs a good deal more than drawing it again. Each halo in a
/// batch of drawing takes a pair of its own, and `Scratch::finish_frame` hands them out again
/// from the first. Kept with the context that made them, and made afresh for any other.
#[derive(Default)]
pub struct Halos {
    dc: Option<ID2D1DeviceContext1>,
    pairs: Vec<(ID2D1Effect, ID2D1Effect)>,
    used: usize,
}

impl Halos {
    unsafe fn take(&mut self, dc: &ID2D1DeviceContext1) -> Result<(ID2D1Effect, ID2D1Effect)> {
        if self.dc.as_ref() != Some(dc) {
            *self = Self { dc: Some(dc.clone()), ..Self::default() };
        }
        if self.used == self.pairs.len() {
            let blur = dc.CreateEffect(&CLSID_D2D1GaussianBlur)?;
            let fade = dc.CreateEffect(&CLSID_D2D1Opacity)?;
            fade.SetInput(0, &blur.GetOutput()?, true);
            self.pairs.push((blur, fade));
        }
        self.used += 1;
        Ok(self.pairs[self.used - 1].clone())
    }
}

unsafe fn draw_image(dc: &ID2D1DeviceContext1, image: &ID2D1Image) {
    dc.DrawImage(image, None, None, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_COMPOSITE_MODE_SOURCE_OVER);
}

fn opaque(color: D2D1_COLOR_F) -> D2D1_COLOR_F {
    D2D1_COLOR_F { a: 1.0, ..color }
}

/// Drops points that have faded out, then fills what is left, `fade` times as opaque as usual.
/// Fails if any of it couldn't be drawn, so a stroke isn't taken as baked when it wasn't.
pub fn draw(
    factory: &ID2D1Factory,
    target: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    stroke: &mut Stroke,
    now: Instant,
    fade: f32,
    scratch: &mut Scratch,
) -> Result<()> {
    let dc = target.cast::<ID2D1DeviceContext1>()?;
    let frame = prepare(factory, &dc, brush, stroke, now, fade, scratch)?;
    frame.paint(&dc, brush);
    Ok(())
}

/// A stroke tessellated as it stands at one moment, each pass with its colour. A frame makes
/// it once and draws it into every rectangle it repaints, where filling the stroke afresh in
/// each would tessellate it again every time.
#[derive(Default)]
pub struct Frame {
    passes: Vec<Pass>,
}

enum Pass {
    Fill(ID2D1GeometryRealization, D2D1_COLOR_F),
    Halo(ID2D1Image),
}

impl Frame {
    pub fn paint(&self, dc: &ID2D1DeviceContext1, brush: &ID2D1SolidColorBrush) {
        for pass in &self.passes {
            unsafe {
                match pass {
                    Pass::Fill(realization, color) => {
                        brush.SetColor(color);
                        dc.DrawGeometryRealization(realization, brush);
                    }
                    Pass::Halo(image) => draw_image(dc, image),
                }
            }
        }
    }
}

/// Drops points that have faded out, as `draw` does, and tessellates what is left, the halo
/// from the same tessellation as the stroke. Called between BeginDraw and EndDraw.
pub fn prepare(
    factory: &ID2D1Factory,
    dc: &ID2D1DeviceContext1,
    brush: &ID2D1SolidColorBrush,
    stroke: &mut Stroke,
    now: Instant,
    fade: f32,
    scratch: &mut Scratch,
) -> Result<Frame> {
    let tolerance = flattening_tolerance(dc);
    let (glow, radius) = (halo(stroke.look.style), stroke.look.width as f32 / 2.0 * stroke.scale);
    // Flashes beneath everything, then one halo beneath every part, as a settled shape draws.
    let (mut flashes, mut bodies, mut cores) = (Vec::new(), Vec::new(), Vec::new());
    passes(stroke, now, fade, scratch, |kind, outline, color| {
        if outline.len() < 3 {
            return Ok(());
        }
        let realization = unsafe { dc.CreateFilledGeometryRealization(&geometry(factory, outline)?, tolerance)? };
        if kind == PassKind::Stroke {
            cores.push((realization.clone(), color));
        }
        if kind == PassKind::Flash { &mut flashes } else { &mut bodies }.push(Pass::Fill(realization, color));
        Ok(())
    })?;
    let mut frame = Frame { passes: flashes };
    if let (Some(glow), Some(&(_, color))) = (glow, cores.first()) {
        let image = unsafe {
            halo_image(dc, &mut scratch.halos, glow, radius, color.a, || {
                brush.SetColor(&opaque(color));
                for (core, _) in &cores {
                    dc.DrawGeometryRealization(core, brush);
                }
            })?
        };
        frame.passes.push(Pass::Halo(image));
    }
    frame.passes.extend(bodies);
    Ok(frame)
}

/// What a pass handed out by `passes` is: the flash beneath a shape as it snaps, the stroke
/// itself, which a halo is blurred from, or a brighter centre over it.
#[derive(Clone, Copy, PartialEq)]
enum PassKind {
    Flash,
    Stroke,
    Centre,
}

/// Direct2D's default tolerance, in DIPs at the target's DPI, so a realization is as fine on a
/// scaled panel as filling the geometry would be.
fn flattening_tolerance(dc: &ID2D1DeviceContext1) -> f32 {
    let (mut dpi_x, mut dpi_y) = (96.0, 96.0);
    unsafe { dc.GetDpi(&mut dpi_x, &mut dpi_y) };
    D2D1_DEFAULT_FLATTENING_TOLERANCE * 96.0 / dpi_x.max(dpi_y).max(1.0)
}

/// Drops points that have faded out, then hands each pass of what is left to `emit`, widest
/// first, as an outline and the colour to fill it with.
fn passes(
    stroke: &mut Stroke,
    now: Instant,
    fade: f32,
    scratch: &mut Scratch,
    mut emit: impl FnMut(PassKind, &[Vector2], D2D1_COLOR_F) -> Result<()>,
) -> Result<()> {
    let Stroke {
        points,
        look,
        scale,
        whole,
        breaks,
        flash,
    } = stroke;
    let (look, scale) = (*look, *scale);
    let base = look.width as f32 / 2.0 * scale;
    let life = radii(points, now, base, &look, *whole, &mut scratch.radii);
    let flash = flash
        .map(|at| 1.0 - now.duration_since(at).as_secs_f32() / FLASH_SECS)
        .filter(|left| *left > 0.0)
        .map(|left| (FLASH.0, FLASH.1 * left * left, FLASH.2));
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
        return Ok(());
    }
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
            tangents(&scratch.centre, &mut scratch.tangents);
        } else {
            curve_through(
                &scratch.path,
                &scratch.radii[part],
                CURVE_TOLERANCE * scale,
                max_turn(&look, scale),
                &mut scratch.centre,
                &mut scratch.widths,
            );
            tangents(&scratch.centre, &mut scratch.tangents);
            thin(&mut scratch.centre, &mut scratch.widths, &mut scratch.tangents, THIN_TOLERANCE * scale, layers(look.style)[0].0, max_turn(&look, scale));
        }
        let opacity = look.opacity as f32 / 100.0 * fade * life;
        let kinds = flash.iter().map(|_| PassKind::Flash).chain([PassKind::Stroke]).chain(std::iter::repeat(PassKind::Centre));
        for (kind, &(width, alpha, white)) in kinds.zip(flash.iter().chain(layers(look.style))) {
            scratch.layer.clear();
            scratch.layer.extend(scratch.widths.iter().map(|radius| radius * width));
            outline(&scratch.centre, &scratch.tangents, &scratch.layer, &mut scratch.outline);
            let mut color = rgba(look.color, alpha * opacity);
            color.r += (1.0 - color.r) * white;
            color.g += (1.0 - color.g) * white;
            color.b += (1.0 - color.b) * white;
            emit(kind, &scratch.outline, color)?;
        }
    }
    Ok(())
}

fn ease_out(k: f32) -> f32 {
    1.0 - (1.0 - k).powi(4)
}

/// How opaque a whole shape is: fully, for its fade time after its newest point, then fading
/// out over SHAPE_FADE_MS.
fn shape_life(points: &[Point], now: Instant, look: &Look) -> f32 {
    if look.ink {
        return 1.0;
    }
    let newest = points.iter().map(|point| point.at).max();
    let age = newest.map_or(0.0, |at| now.duration_since(at).as_secs_f32());
    let k = ((age - look.fade_ms as f32 / 1000.0) / (SHAPE_FADE_MS as f32 / 1000.0)).clamp(0.0, 1.0);
    1.0 - k * k * (3.0 - 2.0 * k)
}

/// Thinner the older a point is, so the trail lasts as long as the fade however far the hand
/// went. A whole shape keeps its width, so its outline never changes once it settles; returns
/// how opaque it is.
fn radii(
    points: &[Point],
    now: Instant,
    base: f32,
    look: &Look,
    whole: bool,
    radii: &mut Vec<f32>,
) -> f32 {
    radii.clear();
    radii.resize(points.len(), base);
    if look.ink {
        return 1.0;
    }
    let fade = look.fade_ms as f32 / 1000.0;
    if whole {
        return shape_life(points, now, look);
    }
    for (radius, point) in radii.iter_mut().zip(points) {
        let age = 1.0 - now.duration_since(point.at).as_secs_f32() / fade;
        *radius = base * ease_out(age.max(0.0));
    }
    1.0
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
/// enough that sparse samples from a fast flick still come out round, and no finer: nearly
/// straight stretches stay as one piece, since every point is drawn again each frame.
fn curve_through(
    path: &[Vector2],
    radii: &[f32],
    tolerance: f32,
    max_turn: f32,
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
        // Cut into n pieces, a quadratic strays |from - 2 control + to| / 4n² from its chords.
        let bend = distance(Vector2 { X: from.X - control.X, Y: from.Y - control.Y }, Vector2 { X: control.X - to.X, Y: control.Y - to.Y });
        let by_error = (bend / (4.0 * tolerance)).sqrt();
        let (u, v) = ((control.X - from.X, control.Y - from.Y), (to.X - control.X, to.Y - control.Y));
        let turn = (u.0 * v.1 - u.1 * v.0).atan2(u.0 * v.0 + u.1 * v.1).abs();
        let steps = (by_error.max(turn / max_turn).ceil() as usize).clamp(1, 16);
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

/// Drops the centre points that a straight run between their neighbours would reproduce to
/// within `tolerance`, at the centre and at both edges of the widest pass, so each frame
/// tessellates only the points that shape the stroke. A fast mouse samples a gentle curve every
/// few pixels, where a run of a dozen samples strays less than a tenth of a pixel from its chord.
/// The points kept keep the directions `tangents` found for them on the whole line, so the
/// outline's edges lie where the whole line's would. A run turns through at most `max_turn`.
fn thin(
    points: &mut Vec<Vector2>,
    widths: &mut Vec<f32>,
    tangents: &mut Vec<Vector2>,
    tolerance: f32,
    reach: f32,
    max_turn: f32,
) {
    let count = points.len();
    if count < 3 {
        return;
    }
    let direction = |a: Vector2, b: Vector2| (b.Y - a.Y).atan2(b.X - a.X);
    let mut kept = 1;
    let mut from = 0;
    while from < count - 1 {
        let mut to = from + 1;
        let mut turned = 0.0f32;
        // A run is checked point by point, so it is kept short enough that a long straight
        // line costs a little per point rather than per pair of points.
        while to + 1 < count && to - from < MAX_THIN_RUN {
            let next = to + 1;
            let step = direction(points[to], points[next]) - direction(points[to - 1], points[to]);
            turned += (step + PI).rem_euclid(2.0 * PI) - PI;
            if turned.abs() > max_turn {
                break;
            }
            let (a, b) = (points[from], points[next]);
            let length = distance(a, b);
            let edge = |i: usize, side: f32| {
                let (c, t, r) = (points[i], tangents[i], widths[i] * reach * side);
                Vector2 { X: c.X - t.Y * r, Y: c.Y + t.X * r }
            };
            let strays = (from + 1..next).any(|i| {
                let p = points[i];
                let t = if length > 1e-6 { ((p.X - a.X) * (b.X - a.X) + (p.Y - a.Y) * (b.Y - a.Y)) / (length * length) } else { 0.0 };
                !(0.0..=1.0).contains(&t)
                    || [0.0, 1.0, -1.0].into_iter().any(|side| off_segment(edge(i, side), edge(from, side), edge(next, side)) > tolerance)
            });
            if strays {
                break;
            }
            to = next;
        }
        points[kept] = points[to];
        widths[kept] = widths[to];
        tangents[kept] = tangents[to];
        kept += 1;
        from = to;
    }
    points.truncate(kept);
    widths.truncate(kept);
    tangents.truncate(kept);
}

/// How far `p` is from the segment `a`-`b`.
fn off_segment(p: Vector2, a: Vector2, b: Vector2) -> f32 {
    let (dx, dy) = (b.X - a.X, b.Y - a.Y);
    let length = dx * dx + dy * dy;
    let t = if length > 1e-12 { (((p.X - a.X) * dx + (p.Y - a.Y) * dy) / length).clamp(0.0, 1.0) } else { 0.0 };
    distance(p, Vector2 { X: a.X + dx * t, Y: a.Y + dy * t })
}

/// The direction of the centre line at each point, from the neighbour before it to the one
/// after; where those meet, the last direction found.
fn tangents(centre: &[Vector2], out: &mut Vec<Vector2>) {
    out.clear();
    let count = centre.len();
    let mut direction = Vector2 { X: 1.0, Y: 0.0 };
    out.extend((0..count).map(|index| {
        let (a, b) = (centre[index.saturating_sub(1)], centre[(index + 1).min(count - 1)]);
        let length = distance(a, b);
        if length > 1e-3 {
            direction = Vector2 { X: (b.X - a.X) / length, Y: (b.Y - a.Y) / length };
        }
        direction
    }));
}

/// Up one side of the stroke, round the head, back down the other side and round the tail.
fn outline(centre: &[Vector2], tangents: &[Vector2], radii: &[f32], out: &mut Vec<Vector2>) {
    out.clear();
    let count = centre.len();
    let offset = |index: usize, t: Vector2, along: f32, across: f32| {
        let (c, r) = (centre[index], radii[index]);
        Vector2 {
            X: c.X + (t.X * along - t.Y * across) * r,
            Y: c.Y + (t.Y * along + t.X * across) * r,
        }
    };
    let angle = |step: usize| PI * step as f32 / CAP_STEPS as f32;

    out.extend((0..count).map(|index| offset(index, tangents[index], 0.0, 1.0)));
    let head = count - 1;
    out.extend((1..CAP_STEPS).map(|step| offset(head, tangents[head], angle(step).sin(), angle(step).cos())));
    out.extend((0..count).rev().map(|index| offset(index, tangents[index], 0.0, -1.0)));
    out.extend((1..CAP_STEPS).map(|step| offset(0, tangents[0], -angle(step).sin(), -angle(step).cos())));
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

type TailKey = (usize, usize, (u32, u32));

/// A stretch of stroke tessellated for each pass, and where it paints.
struct Tessellated {
    bounds: Bounds,
    passes: Vec<ID2D1GeometryRealization>,
}

/// A kept stroke grows for as long as the button is held, and drawing it whole every frame would
/// cost more the longer it got. Its settled stretch is tessellated once, a piece at a time, so a
/// frame costs the same however long the stroke is. Tied to the device that made it.
#[derive(Default)]
pub struct Pieces {
    done: Vec<Tessellated>,
    /// A settled shape's parts, each tessellated for every pass.
    shape: Vec<Tessellated>,
    /// The stretch after them, kept for as long as the same points make it: a frame draws it
    /// into each rectangle it repaints, and a stroke waiting to be baked doesn't change. Known by
    /// where it starts, how many points it has, and where the last is, since the head drawn at
    /// the cursor moves without adding a point.
    tail: Option<(TailKey, Box<Tessellated>)>,
}

impl Pieces {
    pub fn clear(&mut self) {
        self.done.clear();
        self.shape.clear();
        self.tail = None;
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

/// How far each flat piece of a curve may turn: as far as keeps the outer edge of the widest
/// pass within the tolerance, which for a thin stroke is a good deal further than for a thick.
fn max_turn(look: &Look, scale: f32) -> f32 {
    let outer = look.width as f32 / 2.0 * scale * layers(look.style)[0].0;
    let tolerance = CURVE_TOLERANCE * scale;
    if outer <= tolerance {
        return MAX_CURVE_TURN;
    }
    (2.0 * (1.0 - tolerance / outer).acos()).clamp(CURVE_TURN, MAX_CURVE_TURN)
}

/// The centre line of `points[range]` and its widths, as the whole stroke would have it there.
/// It takes a sample either side as context, so it begins and ends where the stroke's curve
/// passes between samples, not on the sample beside it, which would bulge at each join.
fn centre_of(points: &[Point], range: std::ops::Range<usize>, look: &Look, scale: f32, scratch: &mut Scratch) {
    let span = look.smoothing as usize;
    let (before, after) = (usize::from(range.start > 0), usize::from(range.end < points.len()));
    let (start, end) = (range.start - before, range.end + after);
    let (from, to) = (start.saturating_sub(span), (end + span).min(points.len()));
    smooth(&points[from..to], span, &mut scratch.path);
    scratch.path.truncate(end - from);
    scratch.path.drain(..start - from);
    scratch.radii.clear();
    scratch.radii.resize(scratch.path.len(), look.width as f32 / 2.0 * scale);
    let (tolerance, turn) = (CURVE_TOLERANCE * scale, max_turn(look, scale));
    curve_through(&scratch.path, &scratch.radii, tolerance, turn, &mut scratch.centre, &mut scratch.widths);
    if before == 1 && scratch.centre.len() > 1 {
        scratch.centre.remove(0);
        scratch.widths.remove(0);
    }
    if after == 1 && scratch.centre.len() > 1 {
        scratch.centre.pop();
        scratch.widths.pop();
    }
    // Only once the context is gone, since thinning keeps the ends it is given and the piece's
    // own ends are the ones that must meet its neighbours.
    tangents(&scratch.centre, &mut scratch.tangents);
    thin(&mut scratch.centre, &mut scratch.widths, &mut scratch.tangents, THIN_TOLERANCE * scale, layers(look.style)[0].0, turn);
}

/// Tessellates the centre line in `scratch`, with its tangents, for each pass. Nothing to draw
/// is `None`.
fn tessellate(factory: &ID2D1Factory, dc: &ID2D1DeviceContext1, look: &Look, scratch: &mut Scratch) -> Result<Option<Tessellated>> {
    if scratch.centre.is_empty() {
        return Ok(None);
    }
    let mut passes = Vec::with_capacity(4);
    let mut bounds = None;
    for &(width, _, _) in layers(look.style) {
        scratch.layer.clear();
        scratch.layer.extend(scratch.widths.iter().map(|radius| radius * width));
        outline(&scratch.centre, &scratch.tangents, &scratch.layer, &mut scratch.outline);
        bounds = union(bounds, extent(&scratch.outline));
        let realization = geometry(factory, &scratch.outline)
            .and_then(|geometry| unsafe { dc.CreateFilledGeometryRealization(&geometry, D2D1_DEFAULT_FLATTENING_TOLERANCE) })?;
        passes.push(realization);
    }
    // Where it paints includes its halo.
    let widest = scratch.widths.iter().copied().fold(0.0, f32::max);
    let margin = halo(look.style).map_or(0.0, |halo| 3.0 * halo.spread * widest);
    Ok(bounds.map(|bounds| Tessellated { bounds: inflate(bounds, margin), passes }))
}

/// Draws a shape that has settled, as `draw` would, from its parts tessellated the first time:
/// its outline stays the same while it holds and fades, so only the opacity changes per frame.
pub fn draw_shape(
    factory: &ID2D1Factory,
    dc: &ID2D1DeviceContext1,
    brush: &ID2D1SolidColorBrush,
    stroke: &Stroke,
    pieces: &mut Pieces,
    now: Instant,
    scratch: &mut Scratch,
) -> Result<()> {
    let (points, look) = (&stroke.points, stroke.look);
    if pieces.shape.is_empty() {
        // Kept only once every part is made, so a failure part-way is tried again next frame
        // rather than leaving a part missing for good.
        let mut parts = Vec::new();
        let base = look.width as f32 / 2.0 * stroke.scale;
        let mut start = 0;
        for end in stroke.breaks.iter().copied().chain([points.len()]) {
            let part = start..end.min(points.len());
            start = end;
            if part.is_empty() {
                continue;
            }
            smooth(&points[part], look.smoothing as usize, &mut scratch.centre);
            scratch.widths.clear();
            scratch.widths.resize(scratch.centre.len(), base);
            tangents(&scratch.centre, &mut scratch.tangents);
            parts.extend(tessellate(factory, dc, &look, scratch)?);
        }
        pieces.shape = parts;
    }
    let opacity = look.opacity as f32 / 100.0 * shape_life(points, now, &look);
    if let Some(glow) = halo(look.style) {
        let radius = look.width as f32 / 2.0 * stroke.scale;
        unsafe {
            let image = halo_image(dc, &mut scratch.halos, glow, radius, opacity, || {
                brush.SetColor(&rgba(look.color, 1.0));
                for part in &pieces.shape {
                    dc.DrawGeometryRealization(&part.passes[0], brush);
                }
            })?;
            draw_image(dc, &image);
        }
    }
    for part in &pieces.shape {
        for (pass, &(_, alpha, white)) in layers(look.style).iter().enumerate() {
            let mut color = rgba(look.color, alpha * opacity);
            color.r += (1.0 - color.r) * white;
            color.g += (1.0 - color.g) * white;
            color.b += (1.0 - color.b) * white;
            unsafe {
                brush.SetColor(&color);
                dc.DrawGeometryRealization(&part.passes[pass], brush);
            }
        }
    }
    Ok(())
}

fn inflate(bounds: Bounds, by: f32) -> Bounds {
    Bounds {
        left: bounds.left - by,
        top: bounds.top - by,
        right: bounds.right + by,
        bottom: bounds.bottom + by,
    }
}

fn overlap(a: Bounds, b: Bounds) -> Option<Bounds> {
    let both = Bounds {
        left: a.left.max(b.left),
        top: a.top.max(b.top),
        right: a.right.min(b.right),
        bottom: a.bottom.min(b.bottom),
    };
    (both.left < both.right && both.top < both.bottom).then_some(both)
}

/// Draws a kept stroke as `draw` would, into `area`, from settled pieces and its newest
/// stretch. Each pass is filled solid inside a layer of its opacity, so where pieces overlap
/// they don't show darker. Settling makes new pieces as the stroke grows; a stroke drawn once,
/// as when baked, needn't. Fails if any of it couldn't be tessellated.
#[allow(clippy::too_many_arguments)]
pub fn draw_kept(
    factory: &ID2D1Factory,
    dc: &ID2D1DeviceContext1,
    brush: &ID2D1SolidColorBrush,
    stroke: &Stroke,
    pieces: &mut Pieces,
    area: Bounds,
    settle: bool,
    scratch: &mut Scratch,
) -> Result<()> {
    let (points, look, scale) = (&stroke.points, stroke.look, stroke.scale);
    let span = look.smoothing as usize;
    // A piece settles once every point that smooths its end, and the one after, is in.
    while settle && (pieces.done.len() + 1) * PIECE + OVERLAP + 1 + span < points.len() {
        let start = pieces.done.len() * PIECE;
        centre_of(points, start..start + PIECE + OVERLAP, &look, scale, scratch);
        let Some(piece) = tessellate(factory, dc, &look, scratch)? else { break };
        pieces.done.push(piece);
        pieces.tail = None;
    }
    let head = points.last().map_or((0, 0), |point| (point.x.to_bits(), point.y.to_bits()));
    let made_from = (pieces.settled(), points.len(), head);
    if pieces.tail.as_ref().is_none_or(|(from, _)| *from != made_from) {
        pieces.tail = None;
        if made_from.0 < made_from.1 {
            centre_of(points, made_from.0..made_from.1, &look, scale, scratch);
            pieces.tail = tessellate(factory, dc, &look, scratch)?.map(|tail| (made_from, Box::new(tail)));
        }
    }
    let shown: Vec<&Tessellated> = pieces
        .done
        .iter()
        .chain(pieces.tail.as_ref().map(|(_, tail)| tail.as_ref()))
        .filter(|stretch| overlap(stretch.bounds, area).is_some())
        .collect();
    let Some(bounds) = shown.iter().map(|stretch| stretch.bounds).reduce(Bounds::union) else { return Ok(()) };
    let Some(bounds) = overlap(bounds, area) else { return Ok(()) };
    let opacity = look.opacity as f32 / 100.0;
    if let Some(glow) = halo(look.style) {
        unsafe {
            let image = halo_image(dc, &mut scratch.halos, glow, look.width as f32 / 2.0 * scale, opacity, || {
                brush.SetColor(&rgba(look.color, 1.0));
                for stretch in &shown {
                    dc.DrawGeometryRealization(&stretch.passes[0], brush);
                }
            })?;
            draw_image(dc, &image);
        }
    }
    for (pass, &(_, alpha, white)) in layers(look.style).iter().enumerate() {
        let mut color = rgba(look.color, 1.0);
        color.r += (1.0 - color.r) * white;
        color.g += (1.0 - color.g) * white;
        color.b += (1.0 - color.b) * white;
        unsafe {
            dc.PushLayer(
                &D2D1_LAYER_PARAMETERS1 {
                    contentBounds: crate::graphics::rect_f(bounds),
                    maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                    maskTransform: Matrix3x2::identity(),
                    opacity: alpha * opacity,
                    layerOptions: D2D1_LAYER_OPTIONS1_NONE,
                    ..Default::default()
                },
                None,
            );
            brush.SetColor(&color);
            for stretch in &shown {
                dc.DrawGeometryRealization(&stretch.passes[pass], brush);
            }
            dc.PopLayer();
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod gallery {
    //! Every kind of stroke the overlay draws, through `draw`, saved as a PNG to look at.
    use super::*;
    use crate::settings::{presets, STYLE_SOLID};
    use std::time::Duration;
    use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

    /// Builds the scenes, each a stroke and the moment it is drawn at.
    pub fn scenes(scale: f32) -> Vec<(Stroke, Instant)> {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let s = scale;
        let mut out = Vec::new();
        let laser = presets()[0].look;
        // A Glow trail a second long from a 1000 Hz mouse, mid-flight.
        let mut stroke = Stroke::new(Point { x: 0.0, y: 0.0, at: t0 }, laser, s);
        stroke.points = (0..1000).map(|i| { let t = i as f32 * 0.0034; Point { x: s * (260.0 + 200.0 * (t * 1.7).sin()), y: s * (220.0 + 160.0 * (t * 2.3).cos()), at: at(i) } }).collect();
        out.push((stroke, at(1000)));
        // Neon, a fast flick: samples 40 px apart.
        let mut neon = laser;
        neon.style = STYLE_NEON;
        neon.color = 0x00e5ff;
        let mut stroke = Stroke::new(Point { x: 0.0, y: 0.0, at: t0 }, neon, s);
        stroke.points = (0..24).map(|i| { let t = i as f32 / 23.0; Point { x: s * (560.0 + 320.0 * t), y: s * (300.0 - 230.0 * (t * PI).sin()), at: at(300 + i as u64 * 8) } }).collect();
        out.push((stroke, at(500)));
        // Solid laser, sharp zigzag.
        let mut solid = laser;
        solid.style = STYLE_SOLID;
        solid.width = 8;
        let mut stroke = Stroke::new(Point { x: 0.0, y: 0.0, at: t0 }, solid, s);
        stroke.points = (0..=120).map(|i| { let k = i as f32 / 20.0; let tooth = (k.fract() - 0.5).abs() * 2.0; Point { x: s * (960.0 + k * 90.0), y: s * (90.0 + tooth * 140.0 + (k / 6.0) * 0.0), at: at(400 + i * 5) } }).collect();
        out.push((stroke, at(1000)));
        // Highlighter over the same zigzag, at 40%.
        let high = presets()[1].look;
        let mut stroke = Stroke::new(Point { x: 0.0, y: 0.0, at: t0 }, high, s);
        stroke.points = (0..=120).map(|i| { let k = i as f32 / 20.0; let tooth = (k.fract() - 0.5).abs() * 2.0; Point { x: s * (960.0 + k * 90.0), y: s * (300.0 + tooth * 140.0), at: at(2000 + i * 5) } }).collect();
        out.push((stroke, at(2700)));
        // Snapped shapes, settled: a box in Solid, a triangle and a circle in Glow.
        let shape = |look: Look, outline: Vec<(f32, f32)>| {
            let correction = crate::shapes::correct(&outline, s, 0.35).expect("snaps");
            let parts = crate::shapes::lean(&correction.parts);
            let mut stroke = Stroke::new(Point { x: 0.0, y: 0.0, at: t0 }, look, s);
            crate::set_parts(&mut stroke, &parts, at(900));
            stroke.whole = true;
            stroke.look.smoothing = 0;
            stroke
        };
        let lerp = |a: (f32, f32), b: (f32, f32), n: usize| (0..n).map(move |i| { let t = i as f32 / n as f32; (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t) });
        let boxed: Vec<(f32, f32)> = [(80.0, 520.0), (420.0, 520.0), (420.0, 760.0), (80.0, 760.0), (80.0, 520.0)].windows(2).flat_map(|w| lerp((w[0].0 * s, w[0].1 * s), (w[1].0 * s, w[1].1 * s), 60)).collect();
        out.push((shape(solid, boxed), at(1000)));
        let triangle: Vec<(f32, f32)> = [(640.0, 520.0), (820.0, 800.0), (460.0, 800.0), (640.0, 520.0)].windows(2).flat_map(|w| lerp((w[0].0 * s, w[0].1 * s), (w[1].0 * s, w[1].1 * s), 60)).collect();
        out.push((shape(laser, triangle), at(1000)));
        let ring: Vec<(f32, f32)> = (0..=240).map(|i| { let t = i as f32 / 240.0 * std::f32::consts::TAU; (s * (1050.0 + 150.0 * t.cos()), s * (660.0 + 130.0 * t.sin())) }).collect();
        out.push((shape(laser, ring), at(1000)));
        // A slow, shaky Pen line: samples just over MIN_STEP apart with a little hand tremor.
        let pen = presets()[2].look;
        let mut stroke = Stroke::new(Point { x: 0.0, y: 0.0, at: t0 }, pen, s);
        stroke.points = (0..300).map(|i| { let t = i as f32; Point { x: s * (1300.0 + t * 2.1), y: s * (620.0 + 60.0 * (t * 0.02).sin() + 1.2 * (t * 1.9).sin()), at: at(i) } }).collect();
        out.push((stroke, at(300)));
        out
    }

    pub fn render(name: &str, dpi: u32, background: u32, frame: impl Fn(&ID2D1Factory, &ID2D1RenderTarget, &ID2D1SolidColorBrush, &mut Stroke, Instant, &mut Scratch)) {
        crate::snapshot::init();
        let scale = dpi as f32 / 96.0;
        let canvas = crate::snapshot::Canvas::new((1980.0 * scale) as u32, (880.0 * scale) as u32, dpi);
        unsafe {
            let brush = canvas.target.CreateSolidColorBrush(&D2D1_COLOR_F::default(), None).unwrap();
            canvas.target.BeginDraw();
            canvas.target.Clear(Some(&rgba(background, 1.0)));
            let mut scratch = Scratch::default();
            // Device-independent pixels from here; the scenes are laid out in them.
            for (mut stroke, now) in scenes(1.0) {
                frame(&canvas.factory, &canvas.target, &brush, &mut stroke, now, &mut scratch);
            }
            canvas.target.EndDraw(None, None).unwrap();
        }
        canvas.save(name);
    }

    #[test]
    #[ignore = "writes target/snapshots/strokes-*.png"]
    fn stroke_gallery() {
        for (dpi, background, name) in [(96, 0x16181d, "strokes-dark-96.png"), (192, 0x16181d, "strokes-dark-192.png"), (96, 0xf4f4f2, "strokes-light-96.png")] {
            render(name, dpi, background, |factory, target, brush, stroke, now, scratch| {
                draw(factory, target, brush, stroke, now, 1.0, scratch).unwrap();
            });
        }
    }
}

#[cfg(test)]
pub(crate) mod glow_gallery {
    use super::*;
    use crate::settings::presets;
    use std::time::Duration;
    use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

    /// Glow and Neon at three widths, on dark and light, to judge the halo.
    pub fn scenes() -> Vec<(Stroke, Instant)> {
        let t0 = Instant::now();
        let mut out = Vec::new();
        for (row, style) in [STYLE_GLOW, STYLE_NEON].into_iter().enumerate() {
            for (col, width) in [4u32, 12, 24].into_iter().enumerate() {
                let mut look = presets()[0].look;
                look.style = style;
                look.width = width;
                look.ink = true;
                if style == STYLE_NEON { look.color = 0x00e5ff; }
                let (x0, y0) = (60.0 + col as f32 * 330.0, 80.0 + row as f32 * 200.0);
                let mut stroke = Stroke::new(Point { x: 0.0, y: 0.0, at: t0 }, look, 1.0);
                stroke.points = (0..120).map(|i| { let t = i as f32 / 119.0; Point { x: x0 + 260.0 * t, y: y0 + 50.0 * (t * PI * 1.5).sin(), at: t0 + Duration::from_millis(i) } }).collect();
                out.push((stroke, t0 + Duration::from_millis(200)));
            }
        }
        out
    }

    pub fn render(name: &str, background: u32, frame: impl Fn(&ID2D1Factory, &ID2D1RenderTarget, &ID2D1SolidColorBrush, &mut Stroke, Instant, &mut Scratch)) {
        crate::snapshot::init();
        let canvas = crate::snapshot::Canvas::new(1040, 440, 96);
        unsafe {
            let brush = canvas.target.CreateSolidColorBrush(&D2D1_COLOR_F::default(), None).unwrap();
            canvas.target.BeginDraw();
            canvas.target.Clear(Some(&rgba(background, 1.0)));
            let mut scratch = Scratch::default();
            for (mut stroke, now) in scenes() {
                frame(&canvas.factory, &canvas.target, &brush, &mut stroke, now, &mut scratch);
            }
            canvas.target.EndDraw(None, None).unwrap();
        }
        canvas.save(name);
    }

    #[test]
    #[ignore = "writes target/snapshots/glow-*.png"]
    fn glow_widths() {
        for (background, name) in [(0x16181d, "glow-dark.png"), (0xf4f4f2, "glow-light.png")] {
            render(name, background, |factory, target, brush, stroke, now, scratch| draw(factory, target, brush, stroke, now, 1.0, scratch).unwrap());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Thinning moves neither the centre nor either edge further than its tolerance, even
    /// where a long straight run meets a bend, and keeps the ends.
    #[test]
    fn thinned_outlines_stay_within_tolerance() {
        let (tolerance, radius) = (0.02, 12.0);
        let mut centre: Vec<Vector2> = (0..40).map(|i| Vector2 { X: i as f32 * 5.0, Y: 0.0 }).collect();
        let turn = 30.0f32.to_radians();
        let corner = centre[39];
        centre.extend((1..40).map(|i| Vector2 { X: corner.X + i as f32 * 5.0 * turn.cos(), Y: corner.Y + i as f32 * 5.0 * turn.sin() }));
        // And a gentle curve sampled densely, as a fast mouse leaves.
        let end = centre[centre.len() - 1];
        centre.extend((1..200).map(|i| { let t = i as f32 * 0.01; Vector2 { X: end.X + 300.0 * t.sin(), Y: end.Y + 300.0 * (1.0 - t.cos()) } }));
        let widths = vec![radius; centre.len()];
        let mut full = Vec::new();
        tangents(&centre, &mut full);
        let (mut points, mut kept_widths, mut kept_tangents) = (centre.clone(), widths.clone(), full.clone());
        thin(&mut points, &mut kept_widths, &mut kept_tangents, tolerance, 1.0, MAX_CURVE_TURN);
        assert!(points.len() < centre.len() / 2, "{} of {}", points.len(), centre.len());
        assert_eq!((points[0].X, points[0].Y), (centre[0].X, centre[0].Y));
        assert_eq!(points.last().map(|p| (p.X, p.Y)), centre.last().map(|p| (p.X, p.Y)));
        let edge = |c: Vector2, t: Vector2, side: f32| Vector2 { X: c.X - t.Y * radius * side, Y: c.Y + t.X * radius * side };
        for side in [0.0, 1.0, -1.0] {
            let thinned: Vec<Vector2> = points.iter().zip(&kept_tangents).map(|(&c, &t)| edge(c, t, side)).collect();
            for (&c, &t) in centre.iter().zip(&full) {
                let p = edge(c, t, side);
                let off = thinned.windows(2).map(|w| off_segment(p, w[0], w[1])).fold(f32::MAX, f32::min);
                assert!(off <= tolerance * 1.01 + 1e-4, "side {side}: ({:.1}, {:.1}) is {off:.3} off", p.X, p.Y);
            }
        }
    }
}
