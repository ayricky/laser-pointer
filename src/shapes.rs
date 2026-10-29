//! Shape snapping: when a stroke ends, it may be recognised as a line, an arrow, an ellipse or
//! circle, a rectangle or square, or a triangle, and pulled onto the clean shape.
//!
//! `strength` (0–1, from the pen's "Shape snap" setting) is how sloppy a stroke may be and still
//! count: every check below loosens as it rises. At the default, scribbles, ticks, zig-zags,
//! S-curves and spirals are never mistaken for shapes; at the top, a nearly closed C is a circle.

use std::f32::consts::{PI, TAU};

type P = (f32, f32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Line,
    Arrow,
    Ellipse,
    Rectangle,
    Triangle,
}

#[derive(Clone, Debug)]
pub struct Correction {
    #[allow(dead_code, reason = "the app draws the parts; the tests check what was recognised")]
    pub shape: Shape,
    /// Separate polylines; an arrow is its shaft and its head.
    pub parts: Vec<Vec<P>>,
}

/// Points along the result, about this far apart, so corners stay sharp once drawn.
const SPACING: f32 = 3.0;
const SAMPLES: usize = 96;

fn lerp(a: f32, b: f32, k: f32) -> f32 {
    a + (b - a) * k.clamp(0.0, 1.0)
}

fn distance(a: P, b: P) -> f32 {
    (b.0 - a.0).hypot(b.1 - a.1)
}

fn length(points: &[P]) -> f32 {
    points.windows(2).map(|pair| distance(pair[0], pair[1])).sum()
}

/// `count` points evenly spaced along the path, both ends included.
fn resample(points: &[P], count: usize) -> Vec<P> {
    let total = length(points);
    if points.len() < 2 || total <= 0.0 || count < 2 {
        return vec![points.first().copied().unwrap_or_default(); count.max(1)];
    }
    let step = total / (count - 1) as f32;
    let mut out = Vec::with_capacity(count);
    out.push(points[0]);
    let (mut segment, mut walked) = (0, 0.0);
    for k in 1..count - 1 {
        let target = k as f32 * step;
        while segment < points.len() - 2
            && walked + distance(points[segment], points[segment + 1]) < target
        {
            walked += distance(points[segment], points[segment + 1]);
            segment += 1;
        }
        let (a, b) = (points[segment], points[segment + 1]);
        let span = distance(a, b).max(1e-6);
        let t = ((target - walked) / span).clamp(0.0, 1.0);
        out.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
    }
    out.push(points[points.len() - 1]);
    out
}

/// Turns a closed polyline (last point repeating the first) so it begins at the point nearest
/// `start`, keeping its direction, so it lines up point for point with the stroke as drawn.
fn begin_near(closed: Vec<P>, start: P) -> Vec<P> {
    let mut ring = closed;
    ring.pop();
    let first = (0..ring.len())
        .min_by(|&i, &j| distance(ring[i], start).total_cmp(&distance(ring[j], start)))
        .unwrap_or(0);
    ring.rotate_left(first);
    ring.push(ring[0]);
    ring
}

/// Densely samples straight segments between corners.
fn polyline(corners: &[P], scale: f32) -> Vec<P> {
    let mut out = vec![corners[0]];
    for pair in corners.windows(2) {
        let steps = (distance(pair[0], pair[1]) / (SPACING * scale)).ceil().max(1.0) as usize;
        for k in 1..=steps {
            let t = k as f32 / steps as f32;
            out.push((
                pair[0].0 + (pair[1].0 - pair[0].0) * t,
                pair[0].1 + (pair[1].1 - pair[0].1) * t,
            ));
        }
    }
    out
}

fn centroid(points: &[P]) -> P {
    let n = points.len() as f32;
    let (x, y) = points.iter().fold((0.0, 0.0), |(x, y), p| (x + p.0, y + p.1));
    (x / n, y / n)
}

/// Principal axis angle through the centroid, and the spread along and across it.
fn principal_axis(points: &[P]) -> (P, f32, f32, f32) {
    let c = centroid(points);
    let (mut xx, mut xy, mut yy) = (0.0, 0.0, 0.0);
    for p in points {
        let (dx, dy) = (p.0 - c.0, p.1 - c.1);
        xx += dx * dx;
        xy += dx * dy;
        yy += dy * dy;
    }
    let n = points.len() as f32;
    let (xx, xy, yy) = (xx / n, xy / n, yy / n);
    let angle = 0.5 * (2.0 * xy).atan2(xx - yy);
    let mean = (xx + yy) / 2.0;
    let spread = ((xx - yy) * (xx - yy) / 4.0 + xy * xy).sqrt();
    (c, angle, mean + spread, (mean - spread).max(0.0))
}

/// Turns `angle` onto the nearest multiple of `step` if it's within `tolerance`.
fn snap_angle(angle: f32, step: f32, tolerance: f32) -> f32 {
    let nearest = (angle / step).round() * step;
    if (angle - nearest).abs() <= tolerance {
        nearest
    } else {
        angle
    }
}

fn rotate(p: P, about: P, angle: f32) -> P {
    let (sin, cos) = angle.sin_cos();
    let (dx, dy) = (p.0 - about.0, p.1 - about.1);
    (about.0 + dx * cos - dy * sin, about.1 + dx * sin + dy * cos)
}

fn signed_turn(previous: P, at: P, next: P) -> f32 {
    // A repeated point has no direction; treat it as going straight on.
    if distance(previous, at) < 1e-3 || distance(at, next) < 1e-3 {
        return 0.0;
    }
    let a = (at.1 - previous.1).atan2(at.0 - previous.0);
    let b = (next.1 - at.1).atan2(next.0 - at.0);
    wrap(b - a)
}

/// The same angle, within -π..π.
fn wrap(mut angle: f32) -> f32 {
    while angle > PI {
        angle -= TAU;
    }
    while angle < -PI {
        angle += TAU;
    }
    angle
}

/// Sum of the signed turns along the path, in radians.
#[cfg(test)]
fn turning(points: &[P]) -> f32 {
    points.windows(3).map(|w| signed_turn(w[0], w[1], w[2])).sum()
}

/// Signed and absolute turning around a closed loop (last point equal to the first), including
/// the turn back into the first side. One clean loop winds exactly once.
fn loop_turning(loop_: &[P]) -> (f32, f32) {
    let open = &loop_[..loop_.len() - 1];
    let n = open.len();
    (0..n)
        .map(|i| signed_turn(open[(i + n - 1) % n], open[i], open[(i + 1) % n]))
        .fold((0.0, 0.0), |(signed, absolute), turn| (signed + turn, absolute + turn.abs()))
}

/// Largest distance from the chord between the ends, relative to the chord's length.
fn bend(points: &[P]) -> f32 {
    let (a, b) = (points[0], points[points.len() - 1]);
    let chord = distance(a, b).max(1e-6);
    let (ux, uy) = ((b.0 - a.0) / chord, (b.1 - a.1) / chord);
    points
        .iter()
        .map(|p| ((p.0 - a.0) * uy - (p.1 - a.1) * ux).abs())
        .fold(0.0, f32::max)
        / chord
}

/// Douglas–Peucker: the points that matter, within `epsilon`.
fn simplify(points: &[P], epsilon: f32) -> Vec<P> {
    fn recurse(points: &[P], epsilon: f32, out: &mut Vec<P>) {
        let (a, b) = (points[0], points[points.len() - 1]);
        let chord = distance(a, b);
        let (mut far, mut index) = (0.0, 0);
        for (i, p) in points.iter().enumerate().skip(1).take(points.len().saturating_sub(2)) {
            let d = if chord < 1e-6 {
                distance(*p, a)
            } else {
                ((p.0 - a.0) * (b.1 - a.1) - (p.1 - a.1) * (b.0 - a.0)).abs() / chord
            };
            if d > far {
                far = d;
                index = i;
            }
        }
        if far > epsilon && index > 0 {
            recurse(&points[..=index], epsilon, out);
            out.pop();
            recurse(&points[index..], epsilon, out);
        } else {
            out.push(a);
            out.push(b);
        }
    }
    let mut out = Vec::new();
    recurse(points, epsilon, &mut out);
    out
}

fn turn_at(previous: P, at: P, next: P) -> f32 {
    let a = (at.1 - previous.1).atan2(at.0 - previous.0);
    let b = (next.1 - at.1).atan2(next.0 - at.0);
    let mut turn = (b - a).abs();
    if turn > PI {
        turn = TAU - turn;
    }
    turn
}

struct Tolerances {
    /// Straightness: largest deviation / length.
    line: f32,
    /// The same for a polygon's sides, which a hand bows more than it does a line.
    side: f32,
    /// Ellipse: RMS of the normalised radius error.
    ellipse: f32,
    /// How near an angle must be to a right angle or axis to be snapped, in radians.
    angle: f32,
    /// How close two sides or radii must be to be made equal.
    equal: f32,
    /// How near the end must come back to the start to count as closed, of the loop's length
    /// once closed. A C, a U or a V left open by a whole side's worth stays as drawn.
    close: f32,
    /// How far a loop may run on past its start, of the stroke's length.
    overrun: f32,
    /// How far into the stroke the loop may begin, after a lead-in, of the stroke's length. Kept
    /// short of `overrun`: the end is looked for from 60% of the way, and a lead-in reaching
    /// towards that could find the end beside the middle of the loop rather than its start.
    lead: f32,
    /// How near the loop a stub beyond it must keep, relative to size, unless it is short.
    beside: f32,
    /// The outline a loop's turning is counted on, relative to size: coarser ignores more tremor.
    outline: f32,
    /// How much a once-round loop may turn in all, before it reads as a scribble doubling back.
    doubling: f32,
    /// The largest arrowhead, relative to the shaft.
    head: f32,
    /// How sharply a wing must turn back from the shaft, in radians.
    hook: f32,
    /// How far out to the side a wing must reach, relative to its length.
    wing: f32,
    /// Wobble along an arrowhead ignored when finding its corners, relative to its size.
    barbs: f32,
}

impl Tolerances {
    fn new(strength: f32) -> Self {
        Self {
            line: lerp(0.04, 0.085, strength),
            side: lerp(0.055, 0.19, strength),
            ellipse: lerp(0.07, 0.22, strength),
            angle: lerp(3.0, 12.0, strength).to_radians(),
            equal: lerp(0.07, 0.15, strength),
            close: lerp(0.09, 0.19, strength),
            overrun: lerp(0.18, 0.4, strength),
            lead: lerp(0.12, 0.25, strength),
            beside: lerp(0.08, 0.14, strength),
            outline: lerp(0.03, 0.05, strength),
            doubling: lerp(3.4, 4.6, strength) * PI,
            head: lerp(0.45, 0.6, strength),
            hook: lerp(110.0, 100.0, strength).to_radians(),
            wing: lerp(0.35, 0.25, strength),
            barbs: lerp(0.18, 0.22, strength),
        }
    }
}

pub fn correct(points: &[P], scale: f32, strength: f32) -> Option<Correction> {
    let strength = strength.clamp(0.0, 1.0);
    let walked = length(points);
    if strength <= 0.0 || points.len() < 4 || !walked.is_finite() || walked < 40.0 * scale {
        return None;
    }
    let path = resample(points, SAMPLES);
    // Bends finer than the samples are lost to them: a tight zigzag can sample as a straight
    // line. Whatever goes that much further than its samples isn't a shape.
    if length(&path) < 0.5 * walked {
        return None;
    }
    let tolerance = Tolerances::new(strength);
    match closed_part(&path, &tolerance) {
        // A box with rounded corners may pass for an ellipse too, and a lumpy circle for a box:
        // whichever lies closer to the stroke is the one meant.
        Some(loop_) => match (polygon(&loop_, &tolerance, scale), ellipse(&loop_, &tolerance, scale)) {
            (Some(polygon), Some(ellipse)) => Some(if misfit(&loop_, &polygon) <= misfit(&loop_, &ellipse) {
                polygon
            } else {
                ellipse
            }),
            (polygon, ellipse) => polygon.or(ellipse),
        },
        // An arrow's hook is short enough to pass for a wobble, so arrows are tried first.
        None => arrow(&path, &tolerance, strength, scale)
            .or_else(|| line(&path, &tolerance, strength, scale)),
    }
}

/// How well the polygon through `corners` follows a closed stroke, as found, before it is squared
/// up: (furthest any stroke point lies from it, relative to the stroke's spread across its main
/// axis; how far out its corners reach, as `reach` measures it).
fn fit(points: &[P], corners: &[P]) -> (f32, f32) {
    let (_, _, _, minor) = principal_axis(points);
    let mut ring = corners.to_vec();
    ring.push(corners[0]);
    let to_ring = |p: P| ring.windows(2).map(|edge| distance(p, nearest_on(p, edge[0], edge[1]))).fold(f32::MAX, f32::min);
    let across = points.iter().map(|&p| to_ring(p)).fold(0.0, f32::max);
    (across / minor.sqrt().max(1e-3), reach(points, corners))
}

/// The furthest any of a closed polygon's corners lies from the stroke, relative to the shorter
/// side either side of it.
fn reach(points: &[P], corners: &[P]) -> f32 {
    let n = corners.len();
    let to_stroke = |p: P| points.windows(2).map(|edge| distance(p, nearest_on(p, edge[0], edge[1]))).fold(f32::MAX, f32::min);
    (0..n)
        .map(|k| {
            let (before, at, after) = (corners[(k + n - 1) % n], corners[k], corners[(k + 1) % n]);
            to_stroke(at) / distance(before, at).min(distance(at, after)).max(1e-3)
        })
        .fold(0.0, f32::max)
}

/// How far a stroke lies from a shape: the root mean square of each point's distance to it.
fn misfit(points: &[P], shape: &Correction) -> f32 {
    let outline: Vec<P> = shape.parts.iter().flatten().copied().collect();
    let squares: f32 = points
        .iter()
        .map(|&p| {
            outline
                .windows(2)
                .map(|edge| distance(p, nearest_on(p, edge[0], edge[1])))
                .fold(f32::MAX, f32::min)
                .powi(2)
        })
        .sum();
    (squares / points.len() as f32).sqrt()
}

/// The same shape kept to the points that bend it, such as a box's corners, for drawing once it
/// has stopped moving. Every frame of its fade draws each point; the morph needs them all.
pub fn lean(parts: &[Vec<P>]) -> Vec<Vec<P>> {
    parts.iter().map(|part| if part.len() > 2 { simplify(part, 0.1) } else { part.clone() }).collect()
}

/// For the morph animation: a point on the drawn path for each point of the shape, in order, so
/// the stroke can slide onto it point for point.
pub fn correspondence(drawn: &[P], to: &Correction) -> Vec<P> {
    let total: usize = to.parts.iter().map(Vec::len).sum();
    resample(drawn, total.max(2))
}

/// The point nearest `p` on the segment from `a` to `b`.
fn nearest_on(p: P, a: P, b: P) -> P {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length2 = dx * dx + dy * dy;
    let t = if length2 > 0.0 {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / length2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a.0 + dx * t, a.1 + dy * t)
}

/// Where the segments `a`–`b` and `c`–`d` cross, if they do.
fn crossing(a: P, b: P, c: P, d: P) -> Option<P> {
    let (r, s) = ((b.0 - a.0, b.1 - a.1), (d.0 - c.0, d.1 - c.1));
    let denominator = r.0 * s.1 - r.1 * s.0;
    if denominator.abs() < 1e-9 {
        return None;
    }
    let t = ((c.0 - a.0) * s.1 - (c.1 - a.1) * s.0) / denominator;
    let u = ((c.0 - a.0) * r.1 - (c.1 - a.1) * r.0) / denominator;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some((a.0 + r.0 * t, a.1 + r.1 * t))
}

/// Where the end of a stroke comes back to its beginning.
struct Seam {
    /// The segment from `path[first]` near the start, and from `path[last]` near the end.
    first: usize,
    last: usize,
    /// The point on each segment nearest the other: the same point where they cross.
    on_first: P,
    on_last: P,
    gap: f32,
}

/// The first place the end crosses the beginning, or else where the two come nearest, counting
/// against each pair a quarter of the stroke it would leave out of the loop: a long thin box
/// left open has a side partway in that comes nearer the end than its start does. The
/// beginning is the segments before `lead`, the end those from `from` on.
fn seam(path: &[P], lead: usize, from: usize) -> Option<Seam> {
    let mut walked = vec![0.0; path.len()];
    for i in 1..path.len() {
        walked[i] = walked[i - 1] + distance(path[i - 1], path[i]);
    }
    let total = walked[path.len() - 1];
    let (mut best, mut best_cost): (Option<Seam>, f32) = (None, f32::MAX);
    for last in from..path.len() - 1 {
        let (c, d) = (path[last], path[last + 1]);
        for first in 0..lead.min(last.saturating_sub(2)) {
            let (a, b) = (path[first], path[first + 1]);
            if let Some(x) = crossing(a, b, c, d) {
                return Some(Seam { first, last, on_first: x, on_last: x, gap: 0.0 });
            }
            let pairs = [(nearest_on(c, a, b), c), (nearest_on(d, a, b), d), (a, nearest_on(a, c, d)), (b, nearest_on(b, c, d))];
            for (on_first, on_last) in pairs {
                let gap = distance(on_first, on_last);
                let left_out = walked[first] + distance(a, on_first) + total - walked[last] - distance(c, on_last);
                let cost = gap + 0.25 * left_out;
                if cost < best_cost {
                    (best, best_cost) = (Some(Seam { first, last, on_first, on_last, gap }), cost);
                }
            }
        }
    }
    best
}

/// If the end comes back to the beginning (or past it), the loop between.
fn closed_part(path: &[P], tolerance: &Tolerances) -> Option<Vec<P>> {
    let (_, _, major, minor) = principal_axis(path);
    let size = (major.sqrt() + minor.sqrt()) * 2.0;
    let total = length(path);
    // The loop may begin a little way in, where the end comes back to: the start can be a short
    // lead-in, as when the end lands on the first side rather than on the very first point.
    let (mut lead, mut walked) = (1, 0.0);
    while lead < path.len() - 1 && walked + distance(path[lead - 1], path[lead]) <= tolerance.lead * total {
        walked += distance(path[lead - 1], path[lead]);
        lead += 1;
    }
    // People overshoot when circling something: the end is looked for over the last part.
    let seam = seam(path, lead, path.len() * 6 / 10)?;
    let before: Vec<P> = path[..=seam.first].iter().copied().chain([seam.on_first]).collect();
    let after: Vec<P> = [seam.on_last].into_iter().chain(path[seam.last + 1..].iter().copied()).collect();
    // Overshooting the start a little is how people circle things; a real tail beyond it is
    // something else, and mustn't be thrown away.
    if length(&after) > tolerance.overrun * total {
        return None;
    }
    // Cut at the seam, the loop has no jog where its ends meet, which would read as a corner.
    let mut loop_: Vec<P> = Vec::with_capacity(seam.last - seam.first + 3);
    let ring = path[seam.first + 1..=seam.last].iter().copied().chain([seam.on_last, seam.on_first]);
    for p in [seam.on_first].into_iter().chain(ring) {
        if loop_.last().is_none_or(|&last| distance(last, p) > 1e-3) {
            loop_.push(p);
        }
    }
    if loop_.len() < 4 || seam.gap > tolerance.close * length(&loop_) {
        return None;
    }
    // Left open just short of a corner, the last side and the first carried on meet where the
    // corner was meant; a straight bridge would cut it off. Not where they are near parallel,
    // as across a gap partway along a side or round a circle.
    if seam.gap > 0.0 {
        let (end_from, start_to) = (path[seam.last.saturating_sub(3)], path[(seam.first + 4).min(path.len() - 1)]);
        let corner = meeting(end_from, seam.on_last, seam.on_first, start_to).filter(|&m| {
            let ahead = |from: P, to: P, of: P| (to.0 - from.0) * (of.0 - to.0) + (to.1 - from.1) * (of.1 - to.1) > 0.0;
            ahead(end_from, seam.on_last, m)
                && ahead(m, seam.on_first, start_to)
                && distance(m, seam.on_last).max(distance(m, seam.on_first)) <= seam.gap
                && turn_at(end_from, m, start_to) >= 60f32.to_radians()
        });
        if let Some(corner) = corner {
            loop_.insert(loop_.len() - 1, corner);
        }
    }
    // Beyond the seam the stroke must stay on the loop, as an overshoot retracing it does, or stop
    // soon, as where a corner is crossed; a longer tail heads off somewhere else.
    let near_loop = |p: &P| loop_.iter().map(|q| distance(*p, *q)).fold(f32::MAX, f32::min) < tolerance.beside * size;
    let stub = |part: &[P]| length(part) <= 0.06 * total || part.iter().all(near_loop);
    if !stub(&before) || !stub(&after) {
        return None;
    }
    // Once round, counted about the middle, so a jog where the ends meet can't flip it; and
    // without the doubling back of a scribble. That is counted on the loop's outline: a hand's
    // tremor turns a little at every sample, and along the sides of a box that adds up to more
    // than a scribble's worth, though the box never doubles back.
    let middle = centroid(&loop_);
    let bearing = |p: P| (p.1 - middle.1).atan2(p.0 - middle.0);
    let winding: f32 = loop_.windows(2).map(|pair| wrap(bearing(pair[1]) - bearing(pair[0]))).sum();
    let (_, absolute) = loop_turning(&simplify(&loop_, tolerance.outline * size));
    let winds_once = (winding.abs() / TAU).round() == 1.0;
    (winds_once && absolute < tolerance.doubling).then_some(loop_)
}

fn line(path: &[P], tolerance: &Tolerances, strength: f32, scale: f32) -> Option<Correction> {
    let (centre, angle, _, _) = principal_axis(path);
    let (ux, uy) = (angle.cos(), angle.sin());
    let along = |p: P| (p.0 - centre.0) * ux + (p.1 - centre.1) * uy;
    let across = |p: P| ((p.0 - centre.0) * uy - (p.1 - centre.1) * ux).abs();
    let (first, last) = (along(path[0]), along(path[path.len() - 1]));
    let span = (last - first).abs();
    let deviation = path.iter().map(|&p| across(p)).fold(0.0, f32::max);
    if span < 30.0 * scale || deviation / span > tolerance.line {
        return None;
    }
    // A stroke that doubles back on itself isn't a line, however straight.
    let walked = length(path);
    if walked > span * 1.25 {
        return None;
    }
    let ends = [
        (centre.0 + ux * first, centre.1 + uy * first),
        (centre.0 + ux * last, centre.1 + uy * last),
    ];
    let mid = ((ends[0].0 + ends[1].0) / 2.0, (ends[0].1 + ends[1].1) / 2.0);
    let direction = (ends[1].1 - ends[0].1).atan2(ends[1].0 - ends[0].0);
    let step = if strength >= 0.5 { PI / 4.0 } else { PI / 2.0 };
    let turn = snap_angle(direction, step, tolerance.angle) - direction;
    let ends = ends.map(|end| rotate(end, mid, turn));
    Some(Correction {
        shape: Shape::Line,
        parts: vec![polyline(&ends, scale)],
    })
}

/// A line whose end hooks sharply back: one wing of an arrowhead, or both drawn in one go. The
/// tip is the point furthest from the start; everything after it is the head.
fn arrow(path: &[P], tolerance: &Tolerances, strength: f32, scale: f32) -> Option<Correction> {
    let start = path[0];
    // The first time the stroke gets that far: a double-winged head passes the tip twice, and
    // the second pass may reach a little further than the first.
    let furthest = path.iter().map(|&p| distance(p, start)).fold(0.0, f32::max);
    let mut tip_index = path.iter().position(|&p| distance(p, start) >= furthest - (0.05 * furthest).max(1.0))?;
    while tip_index + 1 < path.len() && distance(path[tip_index + 1], start) >= distance(path[tip_index], start) {
        tip_index += 1;
    }
    let tip = path[tip_index];
    let shaft = distance(start, tip);
    let head = &path[tip_index..];
    let reach = head.iter().map(|&p| distance(p, tip)).fold(0.0, f32::max);
    if head.len() < 3 || !(0.06..=tolerance.head).contains(&(reach / shaft)) {
        return None;
    }
    // The head must turn sharply back towards the start, not carry on or curl sideways.
    let (wing, _) = head
        .iter()
        .map(|&p| (p, distance(p, tip)))
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    if turn_at(start, tip, wing) < tolerance.hook {
        return None;
    }
    // A wing sits off to the side of the shaft; running straight back along it is a retrace.
    let (ux, uy) = ((tip.0 - start.0) / shaft, (tip.1 - start.1) / shaft);
    let sideways = head
        .iter()
        .map(|p| ((p.0 - tip.0) * uy - (p.1 - tip.1) * ux).abs())
        .fold(0.0, f32::max);
    if sideways < tolerance.wing * reach || length(head) > (4.0 + 2.0 * tolerance.barbs) * reach {
        return None;
    }
    if !arrowhead_shaped(head, start, tip, reach, tolerance) {
        return None;
    }
    let straight = line(&path[..=tip_index], tolerance, strength, scale)?;
    let shaft_points = &straight.parts[0];
    let (from, to) = (shaft_points[0], shaft_points[shaft_points.len() - 1]);
    let angle = (to.1 - from.1).atan2(to.0 - from.0);
    let length = (shaft * 0.14).clamp(14.0 * scale, 40.0 * scale);
    let spread = 28f32.to_radians();
    let wing_end = |side: f32| {
        let a = angle + PI + side * spread;
        (to.0 + length * a.cos(), to.1 + length * a.sin())
    };
    Some(Correction {
        shape: Shape::Arrow,
        parts: vec![
            straight.parts[0].clone(),
            polyline(&[wing_end(1.0), to, wing_end(-1.0)], scale),
        ],
    })
}

/// The head drawn after the tip is one straight wing back towards the start, or a wing, a
/// return to the tip and a second wing on the other side of the shaft.
fn arrowhead_shaped(head: &[P], start: P, tip: P, reach: f32, tolerance: &Tolerances) -> bool {
    let corners = simplify(head, tolerance.barbs * reach);
    // Which side of the shaft, or none when on it: a wing along the shaft is on neither.
    let side = |p: P| {
        let cross = (tip.0 - start.0) * (p.1 - start.1) - (tip.1 - start.1) * (p.0 - start.0);
        if cross > 0.0 { 1 } else if cross < 0.0 { -1 } else { 0 }
    };
    let wing = |end: P| distance(tip, end) > (tolerance.wing + 0.05) * reach && turn_at(start, tip, end) > tolerance.hook;
    match corners.len() {
        2 => wing(corners[1]),
        4 => {
            let (first, back, second) = (corners[1], corners[2], corners[3]);
            wing(first) && wing(second) && distance(back, tip) < (0.7 - tolerance.wing) * reach && side(first) * side(second) < 0
        }
        _ => false,
    }
}

fn ellipse(loop_: &[P], tolerance: &Tolerances, scale: f32) -> Option<Correction> {
    let (centre, mut angle, _, _) = principal_axis(loop_);
    let (ux, uy) = (angle.cos(), angle.sin());
    let local = |p: P| {
        let (dx, dy) = (p.0 - centre.0, p.1 - centre.1);
        (dx * ux + dy * uy, -dx * uy + dy * ux)
    };
    let projected: Vec<P> = loop_.iter().map(|&p| local(p)).collect();
    let half_range = |axis: fn(&P) -> f32| {
        let (lo, hi) = projected
            .iter()
            .map(axis)
            .fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
        (hi - lo) / 2.0
    };
    let (mut a, mut b) = (half_range(|p| p.0), half_range(|p| p.1));
    if a < 12.0 * scale || b < 6.0 * scale || b / a < 0.2 {
        return None;
    }
    let error = (projected
        .iter()
        .map(|p| ((p.0 / a).powi(2) + (p.1 / b).powi(2)).sqrt() - 1.0)
        .map(|e| e * e)
        .sum::<f32>()
        / projected.len() as f32)
        .sqrt();
    if error > tolerance.ellipse {
        return None;
    }
    if (a - b).abs() / a.max(b) < tolerance.equal {
        let r = (a + b) / 2.0;
        (a, b) = (r, r);
    }
    let snapped = snap_angle(angle, PI / 2.0, tolerance.angle);
    angle = snapped;
    // Start where the user started, going the way they went.
    let start = local(loop_[0]);
    let start_angle = (start.1 / b).atan2(start.0 / a);
    let direction = loop_turning(loop_).0.signum();
    let perimeter = PI * (3.0 * (a + b) - ((3.0 * a + b) * (a + 3.0 * b)).sqrt());
    // Past this many points a curve looks no rounder, but every frame of its fade costs more.
    let count = ((perimeter / (SPACING * scale)).ceil() as usize).clamp(24, 180);
    let (sin, cos) = angle.sin_cos();
    let points = (0..=count)
        .map(|k| {
            let t = start_angle + direction * TAU * k as f32 / count as f32;
            let (x, y) = (a * t.cos(), b * t.sin());
            (centre.0 + x * cos - y * sin, centre.1 + x * sin + y * cos)
        })
        .collect();
    Some(Correction {
        shape: Shape::Ellipse,
        parts: vec![points],
    })
}

fn polygon(loop_: &[P], tolerance: &Tolerances, scale: f32) -> Option<Correction> {
    let (_, _, major, minor) = principal_axis(loop_);
    let size = (major.sqrt() + minor.sqrt()) * 2.0;
    // Finer for a thin shape, whose corners could otherwise sit within the tolerance.
    let simple = simplify(loop_, (0.06 * size).min(0.5 * minor.sqrt()));
    // The loop starts mid-side as often as at a corner: test every vertex, the start included.
    let open = &simple[..simple.len() - 1];
    let n = open.len();
    if n < 3 {
        return None;
    }
    let corners = corners(open);
    let corner_points: Vec<P> = corners.iter().map(|corner| corner.at).collect();
    // Each side must be straight between its corners, leaving out the rounding at each end.
    let index = |p: P| loop_.iter().position(|&q| q == p).unwrap_or(0);
    let sides_straight = (0..corners.len()).all(|k| {
        let (from, to) = (index(open[corners[k].last]), index(open[corners[(k + 1) % corners.len()].first]));
        let side: Vec<P> = if from <= to {
            loop_[from..=to].to_vec()
        } else {
            loop_[from..].iter().chain(&loop_[..=to]).copied().collect()
        };
        side.len() < 3 || bend(&trim(&side, 0.15)) < tolerance.side
    });
    if !sides_straight {
        return None;
    }
    // The shape must follow the stroke: not cut across a thin loop, nor put a corner out where
    // the stroke never went, as when two corners either end of a short side are taken for one.
    if corner_points.len() >= 3 {
        let (across, reach) = fit(loop_, &corner_points);
        if across > FIT_ACROSS || reach > FIT_REACH {
            return None;
        }
    }
    match corner_points.len() {
        3 => {
            let mut closed = corner_points.clone();
            closed.push(corner_points[0]);
            Some(Correction {
                shape: Shape::Triangle,
                parts: vec![begin_near(polyline(&closed, scale), loop_[0])],
            })
        }
        4 => rectangle(&corner_points, loop_, tolerance, scale).map(|rectangle| Correction {
            parts: vec![begin_near(rectangle.parts[0].clone(), loop_[0])],
            ..rectangle
        }),
        _ => None,
    }
}

/// How far a box or triangle may lie from its stroke, relative to how thick the stroke's loop is,
/// and how far out any corner may reach, relative to the sides there. Rounded corners pass; a
/// trapezoid whose top is a third of its base or more doesn't become a triangle.
const FIT_ACROSS: f32 = 0.55;
const FIT_REACH: f32 = 0.25;

/// The path without `fraction` of its length at each end.
fn trim(points: &[P], fraction: f32) -> Vec<P> {
    let total = length(points);
    let (from, to) = (fraction * total, (1.0 - fraction) * total);
    let mut out = Vec::new();
    let mut walked = 0.0;
    for pair in points.windows(2) {
        let step = distance(pair[0], pair[1]);
        let point_at = |d: f32| {
            let k = if step > 0.0 { ((d - walked) / step).clamp(0.0, 1.0) } else { 0.0 };
            (pair[0].0 + (pair[1].0 - pair[0].0) * k, pair[0].1 + (pair[1].1 - pair[0].1) * k)
        };
        if out.is_empty() && walked + step >= from {
            out.push(point_at(from));
        }
        if walked + step >= to {
            out.push(point_at(to));
            break;
        }
        if !out.is_empty() {
            out.push(pair[1]);
        }
        walked += step;
    }
    out
}

/// A corner of a closed outline: the outline's vertices `first..=last`, wrapping round, and
/// where its sides would meet.
struct Corner {
    first: usize,
    last: usize,
    at: P,
}

/// The corners of a closed outline (no repeated last point). A hand rounds corners off, which
/// the outline keeps as two or three gentler bends close together, so neighbouring bends the
/// same way are taken as one corner where the sides either side of them meet.
fn corners(open: &[P]) -> Vec<Corner> {
    let n = open.len();
    let at = |i: usize| open[i % n];
    let turn = |i: usize| signed_turn(at(i + n - 1), at(i), at(i + 1));
    let side = |i: usize| distance(at(i), at(i + 1));
    // Start after the longest side: no rounded corner has its ends either side of it.
    let Some(longest) = (0..n).max_by(|&i, &j| side(i).total_cmp(&side(j))) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let mut i = longest + 1;
    while i <= longest + n {
        let (first, mut last, mut total) = (i, i, turn(i));
        // The next bend joins while it turns the same way, neither is sharp enough to be a
        // corner of its own, and the edge between is short beside the side leading in.
        while last < longest + n {
            let next = turn(last + 1);
            let gentle = total * next > 0.0 && turn(last).abs() < 120f32.to_radians() && next.abs() < 120f32.to_radians();
            let short = side(last) < 0.4 * side(first + n - 1);
            if !gentle || !short || (total + next).abs() > 150f32.to_radians() {
                break;
            }
            last += 1;
            total += next;
        }
        if total.abs() > 35f32.to_radians() {
            let point = if first == last {
                at(first)
            } else {
                let (a, b, c, d) = (at(first + n - 1), at(first), at(last), at(last + 1));
                meeting(a, b, c, d).filter(|&p| distance(p, b) < 2.0 * distance(b, c)).unwrap_or_else(|| {
                    centroid(&(first..=last).map(at).collect::<Vec<P>>())
                })
            };
            found.push(Corner { first: first % n, last: last % n, at: point });
        }
        i = last + 1;
    }
    found
}

/// Where the line through `a` and `b` meets the line through `c` and `d`.
fn meeting(a: P, b: P, c: P, d: P) -> Option<P> {
    let (r, s) = ((b.0 - a.0, b.1 - a.1), (d.0 - c.0, d.1 - c.1));
    let denominator = r.0 * s.1 - r.1 * s.0;
    if denominator.abs() < 1e-6 * (r.0.hypot(r.1) * s.0.hypot(s.1)).max(1e-6) {
        return None;
    }
    let t = ((c.0 - a.0) * s.1 - (c.1 - a.1) * s.0) / denominator;
    Some((a.0 + r.0 * t, a.1 + r.1 * t))
}

/// The box `corners` were meant as, squared up, or none if squaring it would move it off the
/// stroke `loop_`, as it would a thin parallelogram's.
fn rectangle(corners: &[P], loop_: &[P], tolerance: &Tolerances, scale: f32) -> Option<Correction> {
    let right = (0..4).all(|i| {
        let turn = turn_at(corners[(i + 3) % 4], corners[i], corners[(i + 1) % 4]);
        (turn - PI / 2.0).abs() < tolerance.angle * 3.0
    });
    if !right {
        return None;
    }
    // Orientation: the sides' directions averaged modulo a right angle, the longer sides
    // counting for more, so a thin box's short ends can't tip it.
    let (mut sx, mut sy) = (0.0, 0.0);
    for i in 0..4 {
        let (a, b) = (corners[i], corners[(i + 1) % 4]);
        let theta = 4.0 * (b.1 - a.1).atan2(b.0 - a.0);
        sx += distance(a, b) * theta.cos();
        sy += distance(a, b) * theta.sin();
    }
    let drawn = sy.atan2(sx) / 4.0;
    let centre = centroid(corners);
    let local: Vec<P> = corners.iter().map(|&c| rotate(c, centre, -drawn)).collect();
    let (mut left, mut top, mut right_edge, mut bottom) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in &local {
        left = left.min(p.0);
        right_edge = right_edge.max(p.0);
        top = top.min(p.1);
        bottom = bottom.max(p.1);
    }
    // Average the opposite sides rather than taking the outermost corners.
    let width = (right_edge - left + {
        let mut xs: Vec<f32> = local.iter().map(|p| p.0).collect();
        xs.sort_by(f32::total_cmp);
        (xs[2] + xs[3]) / 2.0 - (xs[0] + xs[1]) / 2.0
    }) / 2.0;
    let height = (bottom - top + {
        let mut ys: Vec<f32> = local.iter().map(|p| p.1).collect();
        ys.sort_by(f32::total_cmp);
        (ys[2] + ys[3]) / 2.0 - (ys[0] + ys[1]) / 2.0
    }) / 2.0;
    let (mut width, mut height) = (width, height);
    if (width - height).abs() / width.max(height) < tolerance.equal {
        let side = (width + height) / 2.0;
        (width, height) = (side, side);
    }
    let local_centre = ((left + right_edge) / 2.0, (top + bottom) / 2.0);
    let box_at = |angle: f32| {
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(sx, sy)| {
            rotate((local_centre.0 + sx * width / 2.0, local_centre.1 + sy * height / 2.0), centre, angle)
        })
    };
    // Squared up but not yet turned to the nearest right angle, it must still follow the stroke.
    if reach(loop_, &box_at(drawn)) > FIT_REACH {
        return None;
    }
    let box_corners = box_at(snap_angle(drawn, PI / 2.0, tolerance.angle));
    // Same drawing direction and starting corner as the user's.
    let clockwise = {
        let area: f32 = (0..4)
            .map(|i| corners[i].0 * corners[(i + 1) % 4].1 - corners[(i + 1) % 4].0 * corners[i].1)
            .sum();
        area > 0.0
    };
    let mut ordered: Vec<P> = if clockwise {
        box_corners.to_vec()
    } else {
        box_corners.iter().rev().copied().collect()
    };
    let first = (0..4)
        .min_by(|&i, &j| distance(ordered[i], corners[0]).total_cmp(&distance(ordered[j], corners[0])))
        .unwrap_or(0);
    ordered.rotate_left(first);
    ordered.push(ordered[0]);
    Some(Correction {
        shape: Shape::Rectangle,
        parts: vec![polyline(&ordered, scale)],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default presets' snap (70 of 100).
    const DEFAULT: f32 = 0.7;

    /// A tiny deterministic generator, so the tests need no crates.
    struct Random(u64);

    impl Random {
        fn next(&mut self) -> f32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % 10_000) as f32 / 10_000.0
        }

        /// -1..1
        fn signed(&mut self) -> f32 {
            self.next() * 2.0 - 1.0
        }
    }

    /// Smooth hand wobble: a few low-frequency sines with random phases.
    fn wobble(points: Vec<P>, amount: f32, random: &mut Random) -> Vec<P> {
        let phases: Vec<f32> = (0..4).map(|_| random.next() * TAU).collect();
        let n = points.len() as f32;
        points
            .into_iter()
            .enumerate()
            .map(|(i, (x, y))| {
                let t = i as f32 / n;
                let dx = amount * ((t * 5.0 + phases[0]).sin() + 0.5 * (t * 11.0 + phases[1]).sin());
                let dy = amount * ((t * 6.0 + phases[2]).sin() + 0.5 * (t * 13.0 + phases[3]).sin());
                (x + dx, y + dy)
            })
            .collect()
    }

    fn sloppy_line(random: &mut Random, wobble_amount: f32) -> Vec<P> {
        let angle = random.signed() * 0.12 + if random.next() > 0.5 { 0.0 } else { PI / 2.0 };
        let len = 200.0 + random.next() * 300.0;
        let side = random.signed().signum();
        let points = (0..=60)
            .map(|i| {
                let t = i as f32 / 60.0;
                let bow = (t * PI).sin() * len * 0.015 * side;
                (100.0 + t * len * angle.cos() - bow * angle.sin(), 300.0 + t * len * angle.sin() + bow * angle.cos())
            })
            .collect();
        wobble(points, wobble_amount, random)
    }

    fn sloppy_ellipse(random: &mut Random, wobble_amount: f32, round: bool) -> Vec<P> {
        let a = 80.0 + random.next() * 120.0;
        let b = if round { a * (1.0 + random.signed() * 0.05) } else { a * (0.45 + random.next() * 0.3) };
        let start = random.next() * TAU;
        let sweep = TAU * (0.95 + random.next() * 0.15);
        let tilt = random.signed() * 0.05;
        let points = (0..=80)
            .map(|i| {
                let t = start + sweep * i as f32 / 80.0;
                let r = 1.0 + 0.04 * (t * 2.0).sin();
                rotate((400.0 + a * r * t.cos(), 400.0 + b * r * t.sin()), (400.0, 400.0), tilt)
            })
            .collect();
        wobble(points, wobble_amount, random)
    }

    fn sloppy_polygon(random: &mut Random, corners: &[P], wobble_amount: f32) -> Vec<P> {
        let jittered: Vec<P> = corners
            .iter()
            .map(|&(x, y)| (x + random.signed() * 6.0, y + random.signed() * 6.0))
            .collect();
        let mut closed = jittered.clone();
        closed.push((jittered[0].0 + random.signed() * 8.0, jittered[0].1 + random.signed() * 8.0));
        let dense = resample(&polyline(&closed, 1.0), 100);
        wobble(dense, wobble_amount, random)
    }

    fn sloppy_rectangle(random: &mut Random, wobble_amount: f32) -> Vec<P> {
        let (w, h) = (150.0 + random.next() * 250.0, 100.0 + random.next() * 150.0);
        sloppy_polygon(random, &[(100.0, 100.0), (100.0 + w, 100.0), (100.0 + w, 100.0 + h), (100.0, 100.0 + h)], wobble_amount)
    }

    fn sloppy_triangle(random: &mut Random, wobble_amount: f32) -> Vec<P> {
        let s = 150.0 + random.next() * 200.0;
        sloppy_polygon(random, &[(100.0, 100.0 + s), (100.0 + s / 2.0, 100.0), (100.0 + s, 100.0 + s)], wobble_amount)
    }

    fn sloppy_arrow(random: &mut Random, both: bool) -> Vec<P> {
        let mut points = sloppy_line(random, 1.0);
        let tip = points[points.len() - 1];
        let back = (points[0].1 - tip.1).atan2(points[0].0 - tip.0);
        let wing = distance(points[0], tip) * (0.08 + random.next() * 0.1);
        for side in if both { vec![1.0, -1.0] } else { vec![1.0] } {
            let a = back + side * 0.5;
            for i in 1..=10 {
                let t = i as f32 / 10.0;
                points.push((tip.0 + wing * t * a.cos(), tip.1 + wing * t * a.sin()));
            }
            if both && side > 0.0 {
                for i in 1..=10 {
                    let t = 1.0 - i as f32 / 10.0;
                    points.push((tip.0 + wing * t * a.cos(), tip.1 + wing * t * a.sin()));
                }
            }
        }
        points
    }

    fn negatives(random: &mut Random) -> Vec<(&'static str, Vec<P>)> {
        let tick = vec![(100.0, 200.0), (130.0, 240.0), (220.0, 120.0)];
        let zigzag: Vec<P> = (0..8).map(|i| (100.0 + i as f32 * 40.0, if i % 2 == 0 { 100.0 } else { 160.0 })).collect();
        let s_curve: Vec<P> = (0..=60).map(|i| {
            let t = i as f32 / 60.0;
            (100.0 + 300.0 * t, 200.0 + 80.0 * (t * TAU).sin())
        }).collect();
        let spiral: Vec<P> = (0..=120).map(|i| {
            let t = i as f32 / 120.0 * TAU * 2.6;
            (300.0 + (20.0 + 12.0 * t) * t.cos(), 300.0 + (20.0 + 12.0 * t) * t.sin())
        }).collect();
        let wavy: Vec<P> = (0..=80).map(|i| {
            let t = i as f32 / 80.0;
            (100.0 + 400.0 * t, 200.0 + 18.0 * (t * TAU * 4.0).sin())
        }).collect();
        let cursive: Vec<P> = (0..=150).map(|i| {
            let t = i as f32 / 150.0 * TAU * 3.0;
            (100.0 + 40.0 * t + 30.0 * t.cos(), 200.0 - 45.0 * t.sin())
        }).collect();
        let mut walk = vec![(300.0, 300.0)];
        for _ in 0..80 {
            let (x, y) = walk[walk.len() - 1];
            walk.push((x + random.signed() * 25.0, y + random.signed() * 25.0));
        }
        let check = polyline(&tick, 1.0);
        vec![
            ("tick", check),
            ("zigzag", polyline(&zigzag, 1.0)),
            ("s-curve", s_curve),
            ("spiral", spiral),
            ("wavy", wavy),
            ("cursive", cursive),
            ("scribble", walk),
        ]
    }

    /// Reviewers' cases: a zigzag finer than the samples, a second wing lying along the shaft
    /// (and its mirror image), and coordinates that aren't numbers.
    #[test]
    fn strokes_that_slip_between_the_samples_stay_freehand() {
        let fine_zigzag: Vec<P> = (0..=190).map(|i| (100.0 + 400.0 * i as f32 / 190.0, 100.0 + 60.0 * (i % 2) as f32)).collect();
        assert_eq!(correct(&fine_zigzag, 1.0, DEFAULT).map(|c| c.shape), None);
        let on_the_shaft = [(0.0, 0.0), (300.0, 0.0), (264.0, 48.0), (300.0, 0.0), (245.0, 0.0)];
        let mirrored = on_the_shaft.map(|(x, y)| (x, -y));
        for outline in [on_the_shaft, mirrored] {
            assert_ne!(correct(&polyline(&outline, 1.0), 1.0, DEFAULT).map(|c| c.shape), Some(Shape::Arrow));
        }
        for bad in [f32::NAN, f32::INFINITY] {
            assert!(correct(&[(0.0, 0.0), (100.0, 0.0), (bad, 0.0), (200.0, 0.0)], 1.0, DEFAULT).is_none());
        }
    }

    fn rate(samples: usize, mut make: impl FnMut(&mut Random) -> Vec<P>, strength: f32, shape: Shape) -> f32 {
        let mut random = Random(0x9e37_79b9_7f4a_7c15);
        let hits = (0..samples)
            .filter(|_| correct(&make(&mut random), 1.0, strength).is_some_and(|c| c.shape == shape))
            .count();
        hits as f32 / samples as f32
    }

    fn explain(points: &[P], strength: f32) -> String {
        let path = resample(points, SAMPLES);
        let tolerance = Tolerances::new(strength);
        let (_, _, major, minor) = principal_axis(&path);
        let size = (major.sqrt() + minor.sqrt()) * 2.0;
        let start = path[0];
        let from = path.len() * 6 / 10;
        let gap = path[from..].iter().map(|p| distance(*p, start)).fold(f32::MAX, f32::min);
        let closed = closed_part(&path, &tolerance);
        let mut text = format!("gap/size {:.3} (tol {:.3}) turning {:.2}pi", gap / size, tolerance.close, turning(&path) / PI);
        if let Some(loop_) = &closed {
            let (signed, absolute) = loop_turning(loop_);
            text += &format!(" loop-turn {:.2}pi abs {:.2}pi", signed / PI, absolute / PI);
            let simple = simplify(loop_, 0.06 * size);
            text += &format!(" dp {}", simple.len());
            let open = &simple[..simple.len() - 1];
            let found = corners(open);
            let turns: Vec<i32> = (0..open.len()).map(|i| signed_turn(open[(i + open.len() - 1) % open.len()], open[i], open[(i + 1) % open.len()]).to_degrees() as i32).collect();
            text += &format!(" turns {turns:?} corners {}", found.len());
            let index = |p: P| loop_.iter().position(|&q| q == p).unwrap_or(0);
            let bends: Vec<f32> = (0..found.len()).map(|k| {
                let (from, to) = (index(open[found[k].last]), index(open[found[(k + 1) % found.len()].first]));
                let side: Vec<P> = if from <= to { loop_[from..=to].to_vec() } else { loop_[from..].iter().chain(&loop_[..=to]).copied().collect() };
                if side.len() < 3 { 0.0 } else { (bend(&side) * 1000.0).round() / 1000.0 }
            }).collect();
            text += &format!(" bends {bends:?} (tol {:.3})", tolerance.side);
            text += &format!(" poly {:?} ell {:?}", polygon(loop_, &tolerance, 1.0).map(|c| c.shape), ellipse(loop_, &tolerance, 1.0).map(|c| c.shape));
        } else {
            text += " open";
            text += &format!(" line {:?} arrow {:?}", line(&path, &tolerance, strength, 1.0).is_some(), arrow(&path, &tolerance, strength, 1.0).is_some());
        }
        text
    }

    #[test]
    #[ignore = "prints why shapes were or weren't recognised"]
    fn diagnose() {
        let mut random = Random(0x9e37_79b9_7f4a_7c15);
        for i in 0..6 {
            println!("circle {i}: {}", explain(&sloppy_ellipse(&mut random, 2.0, true), 0.6));
            println!("ellipse {i}: {}", explain(&sloppy_ellipse(&mut random, 2.0, false), 0.6));
            println!("rect {i}: {}", explain(&sloppy_rectangle(&mut random, 1.5), 0.6));
            println!("tri {i}: {}", explain(&sloppy_triangle(&mut random, 1.5), 0.6));
            println!("arrow {i}: {}", explain(&sloppy_arrow(&mut random, false), 0.6));
            println!("arrow2 {i}: {}", explain(&sloppy_arrow(&mut random, true), 0.6));
            println!("line {i}: {}", explain(&sloppy_line(&mut random, 2.0), 0.6));
        }
    }

    #[test]
    fn off_never_changes_anything() {
        let mut random = Random(7);
        assert!(correct(&sloppy_ellipse(&mut random, 2.0, true), 1.0, 0.0).is_none());
    }

    #[test]
    fn recognises_tidy_attempts_at_the_default_strength() {
        for (name, make, shape) in [
            ("line", Box::new(|r: &mut Random| sloppy_line(r, 2.0)) as Box<dyn FnMut(&mut Random) -> Vec<P>>, Shape::Line),
            ("circle", Box::new(|r: &mut Random| sloppy_ellipse(r, 2.0, true)), Shape::Ellipse),
            ("ellipse", Box::new(|r: &mut Random| sloppy_ellipse(r, 2.0, false)), Shape::Ellipse),
            ("rectangle", Box::new(|r: &mut Random| sloppy_rectangle(r, 1.5)), Shape::Rectangle),
            ("triangle", Box::new(|r: &mut Random| sloppy_triangle(r, 1.5)), Shape::Triangle),
            ("arrow", Box::new(|r: &mut Random| sloppy_arrow(r, false)), Shape::Arrow),
            ("double-winged arrow", Box::new(|r: &mut Random| sloppy_arrow(r, true)), Shape::Arrow),
        ] {
            let hit = rate(40, make, 0.6, shape);
            assert!(hit >= 0.9, "{name}: only {:.0}% recognised", hit * 100.0);
        }
    }

    /// A stroke as the mouse reports it: a sample every few pixels, each a little off the line.
    fn trembling(random: &mut Random, outline: &[P], tremor: f32) -> Vec<P> {
        let samples = (length(outline) / 5.0) as usize;
        resample(outline, samples.max(8))
            .into_iter()
            .map(|(x, y)| (x + random.signed() * tremor, y + random.signed() * tremor))
            .collect()
    }

    #[test]
    fn a_trembling_hand_still_snaps_at_the_default() {
        let strength = DEFAULT;
        let corners = |random: &mut Random, corners: &[P]| {
            let mut ring: Vec<P> = corners.iter().map(|&(x, y)| (x + random.signed() * 6.0, y + random.signed() * 6.0)).collect();
            ring.push((ring[0].0 + random.signed() * 8.0, ring[0].1 + random.signed() * 8.0));
            polyline(&ring, 1.0)
        };
        for (name, make, shape) in [
            ("rectangle", Box::new(move |r: &mut Random| {
                let (w, h) = (150.0 + r.next() * 250.0, 100.0 + r.next() * 150.0);
                let outline = corners(r, &[(100.0, 100.0), (100.0 + w, 100.0), (100.0 + w, 100.0 + h), (100.0, 100.0 + h)]);
                trembling(r, &outline, 2.5)
            }) as Box<dyn FnMut(&mut Random) -> Vec<P>>, Shape::Rectangle),
            ("triangle", Box::new(move |r: &mut Random| {
                let s = 150.0 + r.next() * 200.0;
                let outline = corners(r, &[(100.0, 100.0 + s), (100.0 + s / 2.0, 100.0), (100.0 + s, 100.0 + s)]);
                trembling(r, &outline, 2.5)
            }), Shape::Triangle),
            ("circle", Box::new(|r: &mut Random| {
                let outline = sloppy_ellipse(r, 1.0, true);
                trembling(r, &outline, 2.5)
            }), Shape::Ellipse),
        ] {
            let hit = rate(40, make, strength, shape);
            assert!(hit >= 0.85, "{name}: only {:.0}% recognised", hit * 100.0);
        }
        // And trembling doesn't turn freehand into shapes.
        let mut random = Random(11);
        for (name, points) in negatives(&mut random) {
            let shaky = trembling(&mut random, &points, 2.5);
            assert!(correct(&shaky, 1.0, strength).is_none(), "a trembling {name} snapped");
        }
    }

    /// Where a hand ends a closed shape: on the first side a little past the start, across it,
    /// or just short of it; and starting a little before the first corner.
    #[test]
    fn closes_where_the_end_meets_the_beginning() {
        let strength = DEFAULT;
        let closings: [(&str, P, P); 4] = [
            // Lead-in (before the first corner), end (relative to the first corner).
            ("ends on the first side", (-12.0, -12.0), (0.0, 0.0)),
            ("crosses the first side", (0.0, 0.0), (-10.0, -14.0)),
            ("pokes out at the first corner", (-14.0, -8.0), (6.0, -12.0)),
            ("stops short", (0.0, 0.0), (5.0, 9.0)),
        ];
        for (name, lead, end) in closings {
            for (shape, corners) in [
                (Shape::Triangle, vec![(200.0, 100.0), (330.0, 320.0), (70.0, 320.0)]),
                (Shape::Rectangle, vec![(100.0, 100.0), (360.0, 100.0), (360.0, 260.0), (100.0, 260.0)]),
            ] {
                let hit = rate(30, |r| {
                    let (first, second, last) = (corners[0], corners[1], corners[corners.len() - 1]);
                    // The end comes in along the last side and stops where `end` puts it.
                    let along = |from: P, to: P, k: f32| (from.0 + (to.0 - from.0) * k, from.1 + (to.1 - from.1) * k);
                    let landing = along(first, second, if name == "ends on the first side" { 0.08 } else { 0.0 });
                    let mut ring = vec![(first.0 + lead.0, first.1 + lead.1)];
                    ring.extend(corners.iter().map(|&(x, y)| (x + r.signed() * 5.0, y + r.signed() * 5.0)));
                    ring.push(along(last, (landing.0 + end.0, landing.1 + end.1), 1.0));
                    mouse(r, &polyline(&ring, 1.0), 1.0)
                }, strength, shape);
                assert!(hit >= 0.9, "{shape:?} that {name}: only {:.0}% recognised", hit * 100.0);
            }
        }
    }

    /// What the app is given: whole pixels from a trembling hand, at least 2 px apart.
    fn mouse(random: &mut Random, outline: &[P], tremor: f32) -> Vec<P> {
        let mut out: Vec<P> = Vec::new();
        for (x, y) in resample(outline, length(outline) as usize) {
            let p = ((x + random.signed() * tremor).round(), (y + random.signed() * tremor).round());
            if out.last().is_none_or(|&last| distance(last, p) >= 2.0) {
                out.push(p);
            }
        }
        out
    }

    #[test]
    fn an_arrow_whose_head_passes_the_tip_further_out_is_still_an_arrow() {
        // Out to the tip, back along one wing, and past the tip on the way to the other.
        let outline = polyline(&[(100.0, 300.0), (400.0, 300.0), (370.0, 280.0), (406.0, 302.0), (372.0, 322.0)], 1.0);
        let hit = rate(30, |r| mouse(r, &outline, 0.8), DEFAULT, Shape::Arrow);
        assert!(hit >= 0.9, "only {:.0}% recognised", hit * 100.0);
    }

    #[test]
    fn strong_settings_forgive_sloppier_shapes() {
        // Lumpy rather than wobbly: three bulges a gentle setting shouldn't call a circle.
        let lumpy = |random: &mut Random| {
            let phase = random.next() * TAU;
            (0..=90)
                .map(|i| {
                    let t = i as f32 / 90.0 * TAU * 1.03;
                    let r = 150.0 * (1.0 + 0.14 * (3.0 * t + phase).sin());
                    (400.0 + r * t.cos(), 400.0 + r * t.sin())
                })
                .collect()
        };
        let gentle = rate(40, lumpy, 0.2, Shape::Ellipse);
        let strong = rate(40, lumpy, 0.9, Shape::Ellipse);
        assert!(strong >= 0.85, "strong: {strong}");
        assert!(strong > gentle, "gentle {gentle} vs strong {strong}");
    }

    /// Sides bowed out by up to `bow` of their length, as a quick hand draws them.
    fn bowed(random: &mut Random, corners: &[P], bow: f32) -> Vec<P> {
        let mut outline = Vec::new();
        for k in 0..corners.len() {
            let (a, b) = (corners[k], corners[(k + 1) % corners.len()]);
            let (side, push) = (distance(a, b), bow * random.next());
            let (nx, ny) = ((b.1 - a.1) / side, -(b.0 - a.0) / side);
            outline.extend((0..30).map(|i| {
                let t = i as f32 / 30.0;
                let out = push * side * (t * PI).sin();
                (a.0 + (b.0 - a.0) * t + nx * out, a.1 + (b.1 - a.1) * t + ny * out)
            }));
        }
        outline.push(corners[0]);
        outline
    }

    /// Every shape, not only circles, forgives more as the setting rises.
    #[test]
    fn each_step_up_the_slider_forgives_more() {
        type Make = fn(&mut Random) -> Vec<P>;
        let cases: [(Shape, Make); 3] = [
            (Shape::Rectangle, |r| {
                let outline = bowed(r, &[(100.0, 100.0), (400.0, 100.0), (400.0, 300.0), (100.0, 300.0)], 0.1);
                mouse(r, &outline, 1.0)
            }),
            (Shape::Triangle, |r| {
                let outline = bowed(r, &[(250.0, 80.0), (400.0, 340.0), (100.0, 340.0)], 0.1);
                mouse(r, &outline, 1.0)
            }),
            // A wide head, its wings swept out almost square to the shaft.
            (Shape::Arrow, |r| {
                let spread = 1.12 + 0.18 * r.next();
                let wing = |side: f32| (400.0 + 30.0 * (PI + side * spread).cos(), 300.0 + 30.0 * (PI + side * spread).sin());
                mouse(r, &polyline(&[(100.0, 300.0), (400.0, 300.0), wing(1.0), (400.0, 300.0), wing(-1.0)], 1.0), 1.0)
            }),
        ];
        for (shape, make) in cases {
            let rates = [0.1, 0.35, DEFAULT, 1.0].map(|strength| rate(40, make, strength, shape));
            assert!(rates.windows(2).all(|pair| pair[1] >= pair[0]), "{shape:?} {rates:?}");
            assert!(rates[3] >= 0.85 && rates[3] - rates[0] >= 0.25, "{shape:?} {rates:?}");
        }
    }

    #[test]
    fn leaves_handwriting_and_scribbles_alone() {
        let mut random = Random(42);
        for strength in [0.3, 0.6, 0.9] {
            for (name, points) in negatives(&mut random) {
                let result = correct(&points, 1.0, strength).map(|c| c.shape);
                let allowed = name == "wavy" && strength >= 0.7 && result == Some(Shape::Line);
                assert!(result.is_none() || allowed, "{name} at {strength} became {result:?}");
            }
        }
    }

    #[test]
    fn lines_snap_exactly_to_horizontal_and_vertical() {
        let line = correct(&[(100.0, 200.0), (200.0, 203.0), (300.0, 205.0), (400.0, 208.0)], 1.0, 0.6).unwrap();
        let part = &line.parts[0];
        assert!(part.iter().all(|p| (p.1 - part[0].1).abs() < 1e-3));
        let line = correct(&[(100.0, 100.0), (103.0, 200.0), (101.0, 300.0), (106.0, 400.0)], 1.0, 0.6).unwrap();
        let part = &line.parts[0];
        assert!(part.iter().all(|p| (p.0 - part[0].0).abs() < 1e-3));
    }

    #[test]
    fn closed_shapes_close_and_squares_and_circles_regularise() {
        let mut random = Random(3);
        let circle = correct(&sloppy_ellipse(&mut random, 1.0, true), 1.0, 0.8).unwrap();
        let points = &circle.parts[0];
        assert!(distance(points[0], points[points.len() - 1]) < 1.0);
        let c = centroid(points);
        let radii: Vec<f32> = points.iter().map(|&p| distance(p, c)).collect();
        let (lo, hi) = radii.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &r| (lo.min(r), hi.max(r)));
        assert!((hi - lo) / hi < 0.02, "not round: {lo}..{hi}");

        let square = sloppy_polygon(&mut random, &[(100.0, 100.0), (300.0, 100.0), (300.0, 290.0), (100.0, 290.0)], 1.0);
        let corrected = correct(&square, 1.0, 0.8).unwrap();
        assert_eq!(corrected.shape, Shape::Rectangle);
        let points = &corrected.parts[0];
        let xs = points.iter().map(|p| p.0);
        let ys = points.iter().map(|p| p.1);
        let width = xs.clone().fold(f32::MIN, f32::max) - xs.fold(f32::MAX, f32::min);
        let height = ys.clone().fold(f32::MIN, f32::max) - ys.fold(f32::MAX, f32::min);
        assert!((width - height).abs() < 0.5, "{width} x {height}");
    }

    #[test]
    fn morph_pairs_each_shape_point_with_the_drawn_path_in_order() {
        let mut random = Random(11);
        let drawn = sloppy_ellipse(&mut random, 2.0, true);
        let shape = correct(&drawn, 1.0, 0.8).unwrap();
        let from = correspondence(&drawn, &shape);
        assert_eq!(from.len(), shape.parts[0].len());
        assert!(distance(from[0], drawn[0]) < 1e-3);
        // The shape begins where the stroke began, so the morph doesn't sweep round.
        assert!(distance(shape.parts[0][0], drawn[0]) < 30.0);
    }

    #[test]
    fn a_rectangle_started_mid_side_begins_there() {
        let mut random = Random(4);
        let mid = sloppy_polygon(&mut random, &[(200.0, 100.0), (300.0, 100.0), (300.0, 250.0), (100.0, 250.0), (100.0, 100.0)], 1.0);
        let shape = correct(&mid, 1.0, 0.8).unwrap();
        assert_eq!(shape.shape, Shape::Rectangle);
        assert!(distance(shape.parts[0][0], mid[0]) < 20.0);
    }

    /// A corner rounded into three bends, the first of them at the end of the outline and the
    /// rest at its start, is one corner where the sides meet.
    #[test]
    fn a_corner_rounded_into_three_bends_is_one_corner() {
        let open = [(7.0, 7.0), (24.0, 0.0), (300.0, 0.0), (300.0, 300.0), (0.0, 300.0), (0.0, 24.0)];
        let found = corners(&open);
        assert_eq!(found.len(), 4);
        let rounded = found.iter().find(|corner| corner.first != corner.last).expect("the bends were merged");
        assert_eq!((rounded.first, rounded.last), (5, 1));
        assert!(distance(rounded.at, (0.0, 0.0)) < 1e-3, "{:?}", rounded.at);
    }

    /// Thin boxes keep all four corners, thin and tilted shapes stay what they are, and a
    /// trapezoid with a long enough top isn't a triangle, whichever way each is drawn.
    #[test]
    fn thin_shapes_and_trapezoids_keep_their_corners() {
        let both_ways = |corners: &[P]| {
            let mut ring = corners.to_vec();
            ring.push(corners[0]);
            let back: Vec<P> = ring.iter().rev().copied().collect();
            [polyline(&ring, 1.0), polyline(&back, 1.0)]
        };
        for strength in [0.35, DEFAULT, 1.0] {
            for height in [10.0, 20.0, 40.0, 80.0] {
                for thin in both_ways(&[(100.0, 100.0), (500.0, 100.0), (500.0, 100.0 + height), (100.0, 100.0 + height)]) {
                    let shape = correct(&thin, 1.0, strength).map(|c| c.shape);
                    assert!(shape.is_none() || shape == Some(Shape::Rectangle), "400x{height} box at {strength}: {shape:?}");
                }
            }
            for height in [60.0, 100.0] {
                for thin in both_ways(&[(0.0, 0.0), (400.0, 0.0), (400.0, height)]) {
                    assert_eq!(correct(&thin, 1.0, strength).map(|c| c.shape), Some(Shape::Triangle), "400x{height} triangle at {strength}");
                }
            }
            for (top, height) in [(100.0, 200.0), (150.0, 200.0), (100.0, 100.0), (150.0, 100.0)] {
                for offset in [0.0, 25.0, 50.0, 75.0, 100.0] {
                    let corners = [(0.0, 0.0), (300.0, 0.0), (offset + top, height), (offset, height)];
                    for trapezoid in both_ways(&corners) {
                        let shape = correct(&trapezoid, 1.0, strength).map(|c| c.shape);
                        assert_ne!(shape, Some(Shape::Triangle), "300x{height} trapezoid, its {top} top {offset} in, at {strength}");
                    }
                }
            }
            // Squared up, a thin parallelogram's box mustn't tip off the stroke.
            for parallelogram in both_ways(&[(0.0, 0.0), (400.0, 0.0), (416.0, 60.0), (16.0, 60.0)]) {
                if let Some(c) = correct(&parallelogram, 1.0, strength) {
                    let (low, high) = c.parts[0].iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.1), hi.max(p.1)));
                    assert!(low > -10.0 && high < 70.0, "parallelogram at {strength} became {:?} from {low} to {high}", c.shape);
                }
            }
            // Tilted, so squaring it up turns it: what it is tested against is the box as drawn.
            for (w, h, degrees) in [(400.0, 60.0, 8.0), (400.0, 100.0, 10.0), (300.0, 200.0, 20.0)] {
                let corners: Vec<P> = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)].iter().map(|&p| rotate(p, (0.0, 0.0), f32::to_radians(degrees))).collect();
                for tilted in both_ways(&corners) {
                    assert_eq!(correct(&tilted, 1.0, strength).map(|c| c.shape), Some(Shape::Rectangle), "{w}x{h} box at {degrees} degrees, {strength}");
                }
            }
            for b in [10.0, 20.0] {
                let ellipse: Vec<P> = (0..=400).map(|i| {
                    let t = 0.213 + TAU * i as f32 / 400.0;
                    (200.0 * t.cos(), b * t.sin())
                }).collect();
                let shape = correct(&ellipse, 1.0, strength).map(|c| c.shape);
                assert!(shape.is_none() || shape == Some(Shape::Ellipse), "200x{b} ellipse at {strength}: {shape:?}");
            }
        }
    }

    /// A smooth rounded box, its corners a fifth of its height, started partway along the top.
    #[test]
    fn a_smooth_rounded_box_is_a_box() {
        let (w, h, r) = (400.0, 200.0, 60.0);
        let mut outline = Vec::new();
        for (cx, cy, from) in [(w - r, r, -PI / 2.0), (w - r, h - r, 0.0), (r, h - r, PI / 2.0), (r, r, PI)] {
            outline.extend((0..=20).map(|k| {
                let a = from + PI / 2.0 * k as f32 / 20.0;
                (cx + r * a.cos(), cy + r * a.sin())
            }));
        }
        let start = outline.iter().position(|p| p.0 > 330.0).unwrap_or(0);
        outline.rotate_left(start);
        outline.push(outline[0]);
        for strength in [0.35, DEFAULT, 1.0] {
            assert_eq!(correct(&outline, 1.0, strength).map(|c| c.shape), Some(Shape::Rectangle), "at {strength}");
        }
    }

    #[test]
    fn a_box_shaped_hook_is_not_an_arrowhead() {
        let hook = polyline(&[(0.0, 0.0), (300.0, 0.0), (260.0, 25.0), (240.0, 25.0), (240.0, -25.0), (270.0, -25.0)], 1.0);
        assert_ne!(correct(&hook, 1.0, 0.9).map(|c| c.shape), Some(Shape::Arrow));
    }

    #[test]
    fn a_circle_with_a_short_outward_tail_is_not_a_circle() {
        let mut points: Vec<P> = (0..=100)
            .map(|i| {
                let t = i as f32 / 100.0 * TAU * 1.02;
                (400.0 + 200.0 * t.cos(), 400.0 + 200.0 * t.sin())
            })
            .collect();
        let end = points[points.len() - 1];
        points.extend((1..=10).map(|i| (end.0 + i as f32 * 10.0, end.1)));
        assert_ne!(correct(&points, 1.0, 0.9).map(|c| c.shape), Some(Shape::Ellipse));
    }

    #[test]
    fn a_retrace_is_not_an_arrow_and_a_tail_is_not_a_circle() {
        let retrace = polyline(&[(0.0, 0.0), (100.0, 0.0), (200.0, 0.0), (160.0, 0.0)], 1.0);
        assert_ne!(correct(&retrace, 1.0, 0.9).map(|c| c.shape), Some(Shape::Arrow));
        let mut random = Random(9);
        let mut tailed = sloppy_ellipse(&mut random, 1.0, true);
        let end = tailed[tailed.len() - 1];
        tailed.extend((1..=30).map(|i| (end.0 + i as f32 * 8.0, end.1 + i as f32 * 3.0)));
        assert_ne!(correct(&tailed, 1.0, 0.9).map(|c| c.shape), Some(Shape::Ellipse));
    }

    #[test]
    fn fast_enough_for_long_strokes() {
        let mut random = Random(5);
        let long: Vec<P> = (0..2000)
            .map(|i| {
                let t = i as f32 / 2000.0 * TAU;
                (500.0 + 300.0 * t.cos() + random.signed(), 500.0 + 200.0 * t.sin() + random.signed())
            })
            .collect();
        let started = std::time::Instant::now();
        for _ in 0..20 {
            let _ = correct(&long, 1.0, 0.6);
        }
        assert!(started.elapsed().as_secs_f32() / 20.0 < 0.002);
    }

    /// Sloppy inputs (grey) and what each strength makes of them (red), for eyeballing.
    #[test]
    #[ignore = "writes a PNG to look at"]
    fn shapes_gallery() {
        use crate::settings;
        use crate::snapshot;
        use crate::stroke::{self, Point, Scratch, Stroke};
        use std::time::Instant;
        use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
        use windows_numerics::Vector2;

        snapshot::init();
        let mut random = Random(21);
        let rows: Vec<(&str, Vec<P>)> = vec![
            ("circle", sloppy_ellipse(&mut random, 3.0, true)),
            ("ellipse", sloppy_ellipse(&mut random, 3.0, false)),
            ("rectangle", sloppy_rectangle(&mut random, 2.0)),
            ("triangle", sloppy_triangle(&mut random, 2.0)),
            ("arrow", sloppy_arrow(&mut random, false)),
            ("arrow, both wings", sloppy_arrow(&mut random, true)),
            ("line", sloppy_line(&mut random, 3.0)),
        ]
        .into_iter()
        .chain(negatives(&mut random))
        .collect();
        let cell = 300.0;
        let canvas = snapshot::Canvas::new((cell * 4.0) as u32, (cell * rows.len() as f32) as u32, 96);
        let target = &canvas.target;
        let mut scratch = Scratch::default();
        unsafe {
            let brush = target.CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, None).unwrap();
            target.BeginDraw();
            target.Clear(Some(&D2D1_COLOR_F { r: 0.08, g: 0.09, b: 0.1, a: 1.0 }));
            for (row, (_, points)) in rows.iter().enumerate() {
                // Fit each input into its cell.
                let (lo, hi) = points.iter().fold(((f32::MAX, f32::MAX), (f32::MIN, f32::MIN)), |(lo, hi), p| {
                    ((lo.0.min(p.0), lo.1.min(p.1)), (hi.0.max(p.0), hi.1.max(p.1)))
                });
                let k = (cell * 0.8) / (hi.0 - lo.0).max(hi.1 - lo.1).max(1.0);
                for (column, strength) in [0.0, 0.3, 0.6, 0.9].into_iter().enumerate() {
                    let place = |p: &P| {
                        (column as f32 * cell + cell * 0.1 + (p.0 - lo.0) * k, row as f32 * cell + cell * 0.1 + (p.1 - lo.1) * k)
                    };
                    brush.SetColor(&D2D1_COLOR_F { r: 0.5, g: 0.52, b: 0.56, a: 1.0 });
                    for pair in points.windows(2) {
                        let (a, b) = (place(&pair[0]), place(&pair[1]));
                        target.DrawLine(Vector2 { X: a.0, Y: a.1 }, Vector2 { X: b.0, Y: b.1 }, &brush, 1.0, None);
                    }
                    if strength == 0.0 {
                        continue;
                    }
                    let Some(correction) = correct(points, 1.0, strength) else { continue };
                    let mut look = settings::Settings::default().look;
                    look.ink = true;
                    look.smoothing = 0;
                    let now = Instant::now();
                    // As the app draws it once the morph has ended.
                    let mut shape = Stroke::new(Point { x: 0.0, y: 0.0, at: now }, look, 1.0);
                    shape.whole = true;
                    shape.points.clear();
                    for part in &lean(&correction.parts) {
                        if !shape.points.is_empty() {
                            shape.breaks.push(shape.points.len());
                        }
                        shape.points.extend(part.iter().map(|p| {
                            let (x, y) = place(p);
                            Point { x, y, at: now }
                        }));
                    }
                    stroke::draw(&canvas.factory, target, &brush, &mut shape, now, 1.0, &mut scratch).unwrap();
                }
            }
            target.EndDraw(None, None).unwrap();
        }
        canvas.save("shapes.png");
    }

    // ---- Shapes as a hand draws them with a mouse ------------------------------------------

    /// Bows each side of `waypoints` out by up to `bow` of its length and rounds each inner
    /// corner off, cutting `cut` of the shorter side next to it.
    fn hand_path(random: &mut Random, waypoints: &[P], cut: f32, bow: f32) -> Vec<P> {
        let n = waypoints.len();
        // Where each side starts and ends once the corners are cut.
        let mut sides: Vec<(P, P)> = (0..n - 1).map(|i| (waypoints[i], waypoints[i + 1])).collect();
        let mut curves: Vec<Option<(P, P, P)>> = vec![None; n];
        for i in 1..n - 1 {
            let (a, c, b) = (waypoints[i - 1], waypoints[i], waypoints[i + 1]);
            let d = cut * (0.5 + random.next()) * distance(a, c).min(distance(c, b));
            let towards = |from: P, to: P, k: f32| {
                let l = distance(from, to).max(1e-6);
                (from.0 + (to.0 - from.0) * k / l, from.1 + (to.1 - from.1) * k / l)
            };
            let (p, q) = (towards(c, a, d), towards(c, b, d));
            sides[i - 1].1 = p;
            sides[i].0 = q;
            curves[i] = Some((p, c, q));
        }
        let mut out = Vec::new();
        for (i, &(a, b)) in sides.iter().enumerate() {
            let l = distance(a, b);
            let (nx, ny) = ((b.1 - a.1) / l.max(1e-6), -(b.0 - a.0) / l.max(1e-6));
            let push = bow * random.signed() * l;
            let steps = (l / 2.0).ceil().max(2.0) as usize;
            out.extend((0..=steps).map(|k| {
                let t = k as f32 / steps as f32;
                let o = push * (t * PI).sin();
                (a.0 + (b.0 - a.0) * t + nx * o, a.1 + (b.1 - a.1) * t + ny * o)
            }));
            if let Some((p, c, q)) = curves[i + 1] {
                out.extend((1..12).map(|k| {
                    let t = k as f32 / 12.0;
                    let u = 1.0 - t;
                    (u * u * p.0 + 2.0 * u * t * c.0 + t * t * q.0, u * u * p.1 + 2.0 * u * t * c.1 + t * t * q.1)
                }));
            }
        }
        out
    }

    /// A slow drift a hand adds over the whole stroke, `amount` pixels at most.
    fn drift(random: &mut Random, points: Vec<P>, amount: f32) -> Vec<P> {
        let phases: Vec<f32> = (0..4).map(|_| random.next() * TAU).collect();
        let total = length(&points).max(1.0);
        let mut walked = 0.0;
        let mut previous = points[0];
        points
            .into_iter()
            .map(|(x, y)| {
                walked += distance(previous, (x, y));
                previous = (x, y);
                let t = walked / total * TAU;
                let dx = amount * 0.67 * ((t * 1.5 + phases[0]).sin() + 0.5 * (t * 3.7 + phases[1]).sin());
                let dy = amount * 0.67 * ((t * 1.3 + phases[2]).sin() + 0.5 * (t * 4.1 + phases[3]).sin());
                (x + dx, y + dy)
            })
            .collect()
    }

    /// A closed polygon as drawn: started at a corner or partway along a side, corners rounded,
    /// sides bowed, and the end landing short of the start, on it, or past it.
    fn drawn_polygon(random: &mut Random, corners: &[P], cut: f32, bow: f32) -> Vec<P> {
        let n = corners.len();
        let jittered: Vec<P> = corners.iter().map(|&(x, y)| (x + random.signed() * 6.0, y + random.signed() * 6.0)).collect();
        let size = jittered.iter().map(|&p| distance(p, centroid(&jittered))).fold(0.0, f32::max);
        let along = |a: P, b: P, k: f32| (a.0 + (b.0 - a.0) * k, a.1 + (b.1 - a.1) * k);
        // Start at a corner half the time, otherwise partway along the first side.
        let lead = if random.next() < 0.5 { 0.0 } else { 0.1 + 0.4 * random.next() };
        let start = along(jittered[0], jittered[1], lead);
        let mut waypoints = vec![start];
        waypoints.extend(jittered[1..].iter().copied());
        waypoints.push(jittered[0]);
        // Where the end lands: short of the start, or past it along the first side.
        let land = lead + random.signed() * 0.12 + 0.06;
        if land <= 0.0 {
            let last = waypoints.pop().unwrap();
            waypoints.push(along(last, jittered[n - 1], -land));
        } else {
            let mut end = along(jittered[0], jittered[1], land);
            // Off the line a little: the hand doesn't land back exactly on the side.
            end.0 += random.signed() * 0.04 * size;
            end.1 += random.signed() * 0.04 * size;
            waypoints.push(end);
        }
        let outline = hand_path(random, &waypoints, cut, bow);
        let outline = drift(random, outline, 3.0);
        mouse(random, &outline, 0.8)
    }

    fn drawn_rectangle(random: &mut Random, cut: f32) -> Vec<P> {
        let (w, h) = (150.0 + random.next() * 300.0, 90.0 + random.next() * 200.0);
        let tilt = random.signed() * 0.06;
        let c = (300.0, 300.0);
        let corners: Vec<P> = [(-w, -h), (w, -h), (w, h), (-w, h)].iter().map(|&(x, y)| rotate((c.0 + x / 2.0, c.1 + y / 2.0), c, tilt)).collect();
        let corners = if random.next() < 0.5 { corners } else { corners.into_iter().rev().collect() };
        let rolled = (random.next() * 4.0) as usize % 4;
        let mut corners = corners;
        corners.rotate_left(rolled);
        drawn_polygon(random, &corners, cut, 0.03)
    }

    fn drawn_triangle(random: &mut Random, cut: f32) -> Vec<P> {
        let s = 150.0 + random.next() * 250.0;
        let apex = (200.0 + random.signed() * s * 0.25, 100.0);
        let corners = vec![apex, (200.0 + s / 2.0, 100.0 + s * 0.85), (200.0 - s / 2.0, 100.0 + s * 0.85)];
        let corners = if random.next() < 0.5 { corners } else { corners.into_iter().rev().collect() };
        drawn_polygon(random, &corners, cut, 0.03)
    }

    /// A circle or ellipse as drawn: egg-shaped, started anywhere, ending short or past.
    fn drawn_ellipse(random: &mut Random, round: bool) -> Vec<P> {
        let a = 70.0 + random.next() * 200.0;
        let b = if round { a } else { a * (0.4 + random.next() * 0.35) };
        let (start, sweep) = (random.next() * TAU, TAU * (0.9 + random.next() * 0.25));
        let (h2, p2, h3, p3) = (0.1 * random.next(), random.next() * TAU, 0.04 * random.next(), random.next() * TAU);
        let direction = if random.next() < 0.5 { 1.0 } else { -1.0 };
        let tilt = random.signed() * 0.5;
        let steps = 200;
        let outline: Vec<P> = (0..=steps)
            .map(|i| {
                let t = start + direction * sweep * i as f32 / steps as f32;
                // The radius creeps in or out over the turn: the end doesn't come back level.
                let creep = 1.0 + 0.06 * p2.sin() * i as f32 / steps as f32;
                let r = creep * (1.0 + h2 * (2.0 * t + p2).sin() + h3 * (3.0 * t + p3).sin());
                rotate((300.0 + a * r * t.cos(), 300.0 + b * r * t.sin()), (300.0, 300.0), tilt)
            })
            .collect();
        let outline = drift(random, outline, 3.0);
        mouse(random, &outline, 0.8)
    }

    /// A line as drawn: arced by the wrist, sometimes with a flick as the button is let go.
    fn drawn_line(random: &mut Random) -> Vec<P> {
        let len = 150.0 + random.next() * 450.0;
        let angle = random.next() * TAU;
        let bow = random.signed() * 0.035;
        let mut outline: Vec<P> = (0..=100)
            .map(|i| {
                let t = i as f32 / 100.0;
                let o = bow * len * (t * PI).sin();
                rotate((300.0 + t * len, 300.0 + o), (300.0, 300.0), angle)
            })
            .collect();
        if random.next() < 0.4 {
            let end = outline[outline.len() - 1];
            let turn = angle + random.signed().signum() * (0.5 + random.next());
            let flick = 4.0 + random.next() * 10.0;
            outline.extend((1..=5).map(|k| (end.0 + flick * k as f32 / 5.0 * turn.cos(), end.1 + flick * k as f32 / 5.0 * turn.sin())));
        }
        let outline = drift(random, outline, 2.0);
        mouse(random, &outline, 0.8)
    }

    /// An arrow as drawn: a line, then one wing back, or a wing, back to the tip, and the other.
    fn drawn_arrow(random: &mut Random, both: bool) -> Vec<P> {
        let len = 200.0 + random.next() * 400.0;
        let angle = random.next() * TAU;
        let wing = len * (0.08 + random.next() * 0.17);
        let spread = (20.0 + random.next() * 25.0).to_radians();
        let start = (300.0, 300.0);
        let tip = (start.0 + len * angle.cos(), start.1 + len * angle.sin());
        let back = angle + PI;
        let side = if random.next() < 0.5 { 1.0 } else { -1.0 };
        let w = |s: f32, jitter: f32| {
            let a = back + s * (spread + jitter * 0.15);
            (tip.0 + wing * a.cos(), tip.1 + wing * a.sin())
        };
        let mut waypoints = vec![start, tip, w(side, random.signed())];
        if both {
            // Back to the tip, not exactly, and out the other side.
            let near = (tip.0 + random.signed() * 0.12 * wing, tip.1 + random.signed() * 0.12 * wing);
            waypoints.push(near);
            waypoints.push(w(-side, random.signed()));
        }
        let outline = hand_path(random, &waypoints, 0.12, 0.02);
        let outline = drift(random, outline, 2.0);
        mouse(random, &outline, 0.8)
    }

    /// Things that aren't shapes and must stay as drawn.
    fn not_shapes(random: &mut Random) -> Vec<(&'static str, Vec<P>)> {
        let mut out = negatives(random);
        // An open C, three quarters round.
        let c: Vec<P> = (0..=80).map(|i| {
            let t = 0.6 + i as f32 / 80.0 * TAU * 0.72;
            (300.0 + 120.0 * t.cos(), 300.0 + 120.0 * t.sin())
        }).collect();
        out.push(("C", c));
        out.push(("L", polyline(&[(100.0, 100.0), (100.0, 300.0), (260.0, 300.0)], 1.0)));
        out.push(("U", polyline(&[(100.0, 100.0), (110.0, 300.0), (260.0, 300.0), (270.0, 100.0)], 1.0)));
        out.push(("Z", polyline(&[(100.0, 100.0), (300.0, 100.0), (100.0, 300.0), (300.0, 300.0)], 1.0)));
        out.push(("V", polyline(&[(100.0, 100.0), (200.0, 300.0), (300.0, 100.0)], 1.0)));
        // An underline with a loop in it, as in handwriting.
        let looped: Vec<P> = (0..=120).map(|i| {
            let t = i as f32 / 120.0;
            let l = (t * TAU * 1.0).sin();
            (100.0 + 400.0 * t + 40.0 * (t * TAU * 1.0).cos() * (t * PI).sin(), 300.0 + 40.0 * l * (t * PI).sin())
        }).collect();
        out.push(("looped underline", looped));
        out.into_iter().map(|(name, points)| (name, mouse(random, &points, 0.8))).collect()
    }

    type Make = Box<dyn Fn(&mut Random) -> Vec<P>>;

    fn drawn_kinds() -> Vec<(&'static str, Make, Shape)> {
        vec![
            ("line", Box::new(drawn_line), Shape::Line),
            ("arrow", Box::new(|r| drawn_arrow(r, false)), Shape::Arrow),
            ("arrow, both wings", Box::new(|r| drawn_arrow(r, true)), Shape::Arrow),
            ("circle", Box::new(|r| drawn_ellipse(r, true)), Shape::Ellipse),
            ("ellipse", Box::new(|r| drawn_ellipse(r, false)), Shape::Ellipse),
            ("rectangle, sharp", Box::new(|r| drawn_rectangle(r, 0.03)), Shape::Rectangle),
            ("rectangle, rounded", Box::new(|r| drawn_rectangle(r, 0.12)), Shape::Rectangle),
            ("rectangle, very round", Box::new(|r| drawn_rectangle(r, 0.22)), Shape::Rectangle),
            ("triangle, sharp", Box::new(|r| drawn_triangle(r, 0.03)), Shape::Triangle),
            ("triangle, rounded", Box::new(|r| drawn_triangle(r, 0.12)), Shape::Triangle),
            ("triangle, very round", Box::new(|r| drawn_triangle(r, 0.22)), Shape::Triangle),
        ]
    }

    /// Out of `samples` strokes: how many became `shape`, and what the others became.
    fn tally(samples: usize, make: &Make, strength: f32, shape: Shape) -> (usize, std::collections::BTreeMap<String, usize>) {
        let mut random = Random(0x2545_f491_4f6c_dd1d);
        let mut others = std::collections::BTreeMap::new();
        let mut hits = 0;
        for _ in 0..samples {
            match correct(&make(&mut random), 1.0, strength).map(|c| c.shape) {
                Some(s) if s == shape => hits += 1,
                other => *others.entry(other.map_or("freehand".to_owned(), |s| format!("{s:?}"))).or_insert(0) += 1,
            }
        }
        (hits, others)
    }

    /// Mouse-drawn shapes, rounded corners and all, snap to what they are at the old Laser
    /// setting, the default and the most forgiving one, and never to something else.
    #[test]
    fn shapes_drawn_with_a_mouse_snap_to_what_they_are() {
        for (name, make, shape) in drawn_kinds() {
            for strength in [0.35, DEFAULT, 1.0] {
                let (hits, others) = tally(60, &make, strength, shape);
                let wrong: usize = others.iter().filter(|(k, _)| *k != "freehand").map(|(_, n)| n).sum();
                assert!(hits >= 52 && wrong <= 6, "{name} at {strength}: {hits} of 60, else {others:?}");
            }
        }
        for strength in [0.3, 0.6, DEFAULT, 1.0] {
            let mut random = Random(77);
            for _ in 0..5 {
                for (name, points) in not_shapes(&mut random) {
                    let result = correct(&points, 1.0, strength).map(|c| c.shape);
                    let allowed = strength >= 0.7 && result == Some(Shape::Line) && ["wavy", "looped underline"].contains(&name);
                    assert!(result.is_none() || allowed, "{name} at {strength} became {result:?}");
                }
            }
        }
    }

    #[test]
    #[ignore = "prints how often mouse-drawn shapes are recognised, and as what"]
    fn mouse_drawn_rates() {
        for (name, make, shape) in drawn_kinds() {
            let mut line = format!("{name:24}");
            for strength in [0.35, DEFAULT, 1.0] {
                let (hits, others) = tally(200, &make, strength, shape);
                line += &format!(" | {strength:.2}: {:3}% {others:?}", hits / 2);
            }
            println!("{line}");
        }
    }


    /// A closed shape drawn from a random point on it round to `end` of its perimeter short of
    /// that point (negative) or past it (positive), its last quarter drifting `drift_out` of its
    /// size outwards (or inwards, negative), as a hand that doesn't quite land where it began.
    fn closing(random: &mut Random, kind: Shape, end: f32, drift_out: f32) -> Vec<P> {
        let c = (300.0, 300.0);
        let ring: Vec<P> = match kind {
            Shape::Ellipse => {
                let (a, round) = (80.0 + random.next() * 160.0, random.next() < 0.5);
                let b = if round { a } else { a * (0.45 + random.next() * 0.3) };
                (0..=240).map(|i| {
                    let t = TAU * i as f32 / 240.0;
                    (c.0 + a * t.cos(), c.1 + b * t.sin())
                }).collect()
            }
            Shape::Rectangle => {
                let (w, h) = (150.0 + random.next() * 300.0, 90.0 + random.next() * 200.0);
                polyline(&[(c.0 - w / 2.0, c.1 - h / 2.0), (c.0 + w / 2.0, c.1 - h / 2.0), (c.0 + w / 2.0, c.1 + h / 2.0), (c.0 - w / 2.0, c.1 + h / 2.0), (c.0 - w / 2.0, c.1 - h / 2.0)], 1.0)
            }
            _ => {
                let s = 150.0 + random.next() * 250.0;
                polyline(&[(c.0, c.1 - s * 0.45), (c.0 + s / 2.0, c.1 + s * 0.4), (c.0 - s / 2.0, c.1 + s * 0.4), (c.0, c.1 - s * 0.45)], 1.0)
            }
        };
        let mut ring = ring;
        ring.pop();
        let n = ring.len();
        let first = (random.next() * n as f32) as usize % n;
        let reverse = random.next() < 0.5;
        let count = ((1.0 + end) * n as f32).round() as usize;
        let mut outline: Vec<P> = (0..=count).map(|k| ring[if reverse { (first + n * 4 - k) % n } else { (first + k) % n }]).collect();
        let size = outline.iter().map(|&p| distance(p, c)).fold(0.0, f32::max);
        let total = outline.len();
        for (k, p) in outline.iter_mut().enumerate() {
            let ramp = ((k as f32 / total as f32 - 0.75) / 0.25).max(0.0);
            let (dx, dy) = (p.0 - c.0, p.1 - c.1);
            let r = dx.hypot(dy).max(1e-3);
            let push = drift_out * size * ramp;
            *p = (p.0 + dx / r * push, p.1 + dy / r * push);
        }
        if kind != Shape::Ellipse {
            // Corners rounded off as a hand turns them.
            let corners = simplify(&outline, 2.0);
            outline = hand_path(random, &corners, 0.1, 0.02);
        }
        let outline = drift(random, outline, 3.0);
        mouse(random, &outline, 0.8)
    }

    /// Closed shapes whose end stops short of the start by up to a tenth of the way round, or
    /// runs on past it by up to two fifths, drifting in or out as it goes, close at the default
    /// strength; at the old Laser one, short by a sixteenth or past by a fifth.
    #[test]
    fn ends_that_fall_short_or_run_past_still_close() {
        for (strength, ends) in [(DEFAULT, &[-0.1, -0.06, -0.03, 0.0, 0.1, 0.2, 0.3, 0.4][..]), (0.35, &[-0.06, -0.03, 0.0, 0.1, 0.2][..])] {
            for kind in [Shape::Ellipse, Shape::Rectangle, Shape::Triangle] {
                for drift_out in [0.0, 0.1, -0.1] {
                    for &end in ends {
                        let mut random = Random(0x51_7cc1_b727_220a);
                        let hits = (0..40).filter(|_| correct(&closing(&mut random, kind, end, drift_out), 1.0, strength).is_some_and(|c| c.shape == kind)).count();
                        assert!(hits >= 34, "{kind:?} ending {end:+} round, drifting {drift_out:+}, at {strength}: {hits} of 40");
                    }
                }
            }
        }
    }

    #[test]
    #[ignore = "prints how often closed shapes snap as their ends fall short of, or run past, the start"]
    fn closing_rates() {
        let ends = [-0.2, -0.15, -0.1, -0.06, -0.03, 0.0, 0.05, 0.1, 0.2, 0.3, 0.4];
        for kind in [Shape::Ellipse, Shape::Rectangle, Shape::Triangle] {
            for drift_out in [0.0, 0.1, -0.1] {
                for strength in [0.35, 0.5, DEFAULT, 1.0] {
                    let mut line = format!("{kind:?} drift {drift_out:+.1} at {strength:.2}:");
                    for end in ends {
                        let mut random = Random(0x51_7cc1_b727_220a);
                        let hits = (0..40).filter(|_| correct(&closing(&mut random, kind, end, drift_out), 1.0, strength).is_some_and(|c| c.shape == kind)).count();
                        line += &format!(" {end:+.2}:{:3}%", hits * 100 / 40);
                    }
                    println!("{line}");
                }
            }
        }
    }
}
