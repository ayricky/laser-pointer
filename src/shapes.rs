//! Shape snapping: when a stroke ends, it may be recognised as a line, an arrow, an ellipse or
//! circle, a rectangle or square, or a triangle, and pulled onto the clean shape.
//!
//! `strength` (0–1, from the pen's "Shape snap" setting) does two things: it widens how sloppy a
//! stroke may be and still count, and below the halfway mark it only pulls the stroke part of the
//! way, so gentle settings tidy rather than replace. Scribbles, ticks, zig-zags, S-curves and
//! spirals are never mistaken for shapes.

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
    let mut turn = b - a;
    while turn > PI {
        turn -= TAU;
    }
    while turn < -PI {
        turn += TAU;
    }
    turn
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
    /// Ellipse: RMS of the normalised radius error.
    ellipse: f32,
    /// How near an angle must be to a right angle or axis to be snapped, in radians.
    angle: f32,
    /// How close two sides or radii must be to be made equal.
    equal: f32,
    /// How near the end must come back to the start to count as closed, relative to size.
    close: f32,
}

impl Tolerances {
    fn new(strength: f32) -> Self {
        Self {
            line: lerp(0.03, 0.085, strength),
            ellipse: lerp(0.07, 0.16, strength),
            angle: lerp(3.0, 10.0, strength).to_radians(),
            equal: lerp(0.07, 0.15, strength),
            close: lerp(0.14, 0.26, strength),
        }
    }
}

pub fn correct(points: &[P], scale: f32, strength: f32) -> Option<Correction> {
    let strength = strength.clamp(0.0, 1.0);
    if strength <= 0.0 || points.len() < 4 || length(points) < 40.0 * scale {
        return None;
    }
    let path = resample(points, SAMPLES);
    let tolerance = Tolerances::new(strength);
    let ideal = match closed_part(&path, &tolerance) {
        Some(loop_) => polygon(&loop_, &tolerance, scale)
            .or_else(|| ellipse(&loop_, &tolerance, scale)),
        // An arrow's hook is short enough to pass for a wobble, so arrows are tried first.
        None => arrow(&path, &tolerance, strength, scale)
            .or_else(|| line(&path, &tolerance, strength, scale)),
    }?;
    Some(pull(&path, ideal, strength))
}

/// Below the halfway mark, moves the stroke only part of the way to the shape.
fn pull(drawn: &[P], ideal: Correction, strength: f32) -> Correction {
    let amount = lerp(0.6, 1.0, (strength - 0.1) / 0.4);
    // A shape in pieces (an arrow) can't be blended with the one path that was drawn.
    if amount >= 0.999 || ideal.parts.len() > 1 {
        return ideal;
    }
    let mut parts = ideal.parts;
    // The first part takes the drawn path; any extra part (an arrow's head) appears as drawn.
    let main = &parts[0];
    let from = resample(drawn, main.len());
    parts[0] = main
        .iter()
        .zip(&from)
        .map(|(to, from)| (lerp(from.0, to.0, amount), lerp(from.1, to.1, amount)))
        .collect();
    Correction {
        shape: ideal.shape,
        parts,
    }
}

/// For the morph animation: the drawn path, spread over the corrected shape's points, moved
/// `t` of the way there.
pub fn morph(drawn: &[P], to: &Correction, t: f32) -> Vec<Vec<P>> {
    let total: usize = to.parts.iter().map(Vec::len).sum();
    let from = resample(drawn, total.max(2));
    let mut offset = 0;
    to.parts
        .iter()
        .map(|part| {
            let mapped = part
                .iter()
                .zip(&from[offset..])
                .map(|(to, from)| (lerp(from.0, to.0, t), lerp(from.1, to.1, t)))
                .collect();
            offset += part.len();
            mapped
        })
        .collect()
}

/// If the end comes back to the start (or past it), the loop up to that point.
fn closed_part(path: &[P], tolerance: &Tolerances) -> Option<Vec<P>> {
    let (_, _, major, minor) = principal_axis(path);
    let size = (major.sqrt() + minor.sqrt()) * 2.0;
    let start = path[0];
    // People overshoot when circling something: look for the closest return over the last part.
    let from = path.len() * 6 / 10;
    let (index, gap) = path[from..]
        .iter()
        .enumerate()
        .map(|(i, p)| (from + i, distance(*p, start)))
        .min_by(|a, b| a.1.total_cmp(&b.1))?;
    if gap > tolerance.close * size {
        return None;
    }
    let mut loop_ = path[..=index].to_vec();
    // Close it without a sliver of a last side, which would read as a sharp turn.
    if distance(loop_[loop_.len() - 1], start) < 0.03 * size {
        loop_.pop();
    }
    loop_.push(start);
    // Once round, without the doubling back of a scribble.
    let (signed, absolute) = loop_turning(&loop_);
    let winds_once = (signed.abs() / TAU).round() == 1.0;
    (winds_once && absolute < 3.4 * PI).then_some(loop_)
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
    // The first time the stroke gets that far: a double-winged head passes the tip twice.
    let furthest = path.iter().map(|&p| distance(p, start)).fold(0.0, f32::max);
    let tip_index = path.iter().position(|&p| distance(p, start) >= furthest - 1.0)?;
    let tip = path[tip_index];
    let shaft = distance(start, tip);
    let head = &path[tip_index..];
    let reach = head.iter().map(|&p| distance(p, tip)).fold(0.0, f32::max);
    if head.len() < 3 || !(0.06..=0.45).contains(&(reach / shaft)) {
        return None;
    }
    // The head must turn sharply back towards the start, not carry on or curl sideways.
    let (wing, _) = head
        .iter()
        .map(|&p| (p, distance(p, tip)))
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    if turn_at(start, tip, wing) < 110f32.to_radians() {
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
    let count = ((perimeter / (SPACING * scale)).ceil() as usize).max(24);
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
    let simple = simplify(loop_, 0.06 * size);
    // The loop starts mid-side as often as at a corner: test every vertex, the start included.
    let open = &simple[..simple.len() - 1];
    let n = open.len();
    if n < 3 {
        return None;
    }
    let corners: Vec<usize> = (0..n)
        .filter(|&i| turn_at(open[(i + n - 1) % n], open[i], open[(i + 1) % n]) > 35f32.to_radians())
        .collect();
    let corner_points: Vec<P> = corners.iter().map(|&i| open[i]).collect();
    // Each side must be straight between its corners.
    let indices: Vec<usize> = corner_points
        .iter()
        .map(|c| loop_.iter().position(|p| p == c).unwrap_or(0))
        .collect();
    let sides_straight = (0..indices.len()).all(|k| {
        let (from, to) = (indices[k], indices[(k + 1) % indices.len()]);
        let side: Vec<P> = if from <= to {
            loop_[from..=to].to_vec()
        } else {
            loop_[from..].iter().chain(&loop_[..=to]).copied().collect()
        };
        side.len() < 3 || bend(&side) < tolerance.line * 1.6
    });
    if !sides_straight {
        return None;
    }
    match corner_points.len() {
        3 => {
            let mut closed = corner_points.clone();
            closed.push(corner_points[0]);
            Some(Correction {
                shape: Shape::Triangle,
                parts: vec![polyline(&closed, scale)],
            })
        }
        4 => rectangle(&corner_points, tolerance, scale),
        _ => None,
    }
}

fn rectangle(corners: &[P], tolerance: &Tolerances, scale: f32) -> Option<Correction> {
    let right = (0..4).all(|i| {
        let turn = turn_at(corners[(i + 3) % 4], corners[i], corners[(i + 1) % 4]);
        (turn - PI / 2.0).abs() < tolerance.angle * 2.5
    });
    if !right {
        return None;
    }
    // Orientation: the sides' directions averaged modulo a right angle.
    let (mut sx, mut sy) = (0.0, 0.0);
    for i in 0..4 {
        let (a, b) = (corners[i], corners[(i + 1) % 4]);
        let theta = 4.0 * (b.1 - a.1).atan2(b.0 - a.0);
        sx += theta.cos();
        sy += theta.sin();
    }
    let angle = snap_angle(sy.atan2(sx) / 4.0, PI / 2.0, tolerance.angle);
    let centre = centroid(corners);
    let local: Vec<P> = corners.iter().map(|&c| rotate(c, centre, -angle)).collect();
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
    let box_corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(sx, sy)| {
        rotate(
            (local_centre.0 + sx * width / 2.0, local_centre.1 + sy * height / 2.0),
            centre,
            angle,
        )
    });
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
    fn gentle_settings_only_pull_part_of_the_way() {
        let drawn = [(100.0, 200.0), (200.0, 212.0), (300.0, 208.0), (400.0, 214.0)];
        let gentle = correct(&drawn, 1.0, 0.1).unwrap();
        let firm = correct(&drawn, 1.0, 0.6).unwrap();
        let spread = |c: &Correction| {
            let ys = c.parts[0].iter().map(|p| p.1);
            ys.clone().fold(f32::MIN, f32::max) - ys.fold(f32::MAX, f32::min)
        };
        assert!(spread(&firm) < 1e-3);
        assert!(spread(&gentle) > 0.5);
    }

    #[test]
    fn morph_runs_from_the_drawn_path_to_the_shape() {
        let mut random = Random(11);
        let drawn = sloppy_ellipse(&mut random, 2.0, true);
        let shape = correct(&drawn, 1.0, 0.8).unwrap();
        let end = morph(&drawn, &shape, 1.0);
        for (morphed, part) in end.iter().zip(&shape.parts) {
            assert!(morphed.iter().zip(part).all(|(a, b)| distance(*a, *b) < 1e-3));
        }
        let start = morph(&drawn, &shape, 0.0);
        assert!(distance(start[0][0], drawn[0]) < 1e-3);
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
                    let mut shape = Stroke::new(Point { x: 0.0, y: 0.0, at: now }, look, 1.0);
                    shape.points.clear();
                    for part in &correction.parts {
                        if !shape.points.is_empty() {
                            shape.breaks.push(shape.points.len());
                        }
                        shape.points.extend(part.iter().map(|p| {
                            let (x, y) = place(p);
                            Point { x, y, at: now }
                        }));
                    }
                    stroke::draw(&canvas.factory, target, &brush, &mut shape, now, 1.0, &mut scratch);
                }
            }
            target.EndDraw(None, None).unwrap();
        }
        canvas.save("shapes.png");
    }
}
