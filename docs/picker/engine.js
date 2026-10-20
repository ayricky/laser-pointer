// The laser, in the browser: strokes, shape snapping and cursor motion ported from the app
// (src/stroke.rs, src/shapes.rs, and the cursor animation in src/main.rs), and a stage that
// behaves like the overlay. Classic script, so it also works opened straight from disk.
(function () {
  'use strict';

  // ---- Strokes, ported from src/stroke.rs ----------------------------------------------------
  const MIN_STEP = 2, CURVE_STEP = 3, CAP = 8;
  const LAYERS = {
    solid: [[1, 1, 0]],
    glow: [[3.4, 0.10, 0], [2.1, 0.20, 0], [1, 1, 0]],
    neon: [[3.8, 0.12, 0], [2.3, 0.26, 0], [1, 1, 0], [0.45, 1, 0.85]],
  };
  const easeOut4 = k => 1 - Math.pow(1 - k, 4);
  const dist = (a, b) => Math.hypot(b.x - a.x, b.y - a.y);

  function smooth(pts, span) {
    const n = pts.length;
    return pts.map((_, i) => {
      const s = Math.min(span, i, n - 1 - i);
      let x = 0, y = 0, w = 0;
      for (let o = -s; o <= s; o++) { const k = s + 1 - Math.abs(o); x += pts[i + o].x * k; y += pts[i + o].y * k; w += k; }
      return { x: x / w, y: y / w };
    });
  }

  function curveThrough(p, r, step) {
    const n = p.length;
    if (n < 3) return [p.slice(), r.slice()];
    const mid = i => ({ x: (p[i].x + p[i + 1].x) / 2, y: (p[i].y + p[i + 1].y) / 2 }), mr = i => (r[i] + r[i + 1]) / 2;
    const pts = [p[0], mid(0)], ws = [r[0], mr(0)];
    for (let i = 1; i < n - 1; i++) {
      const f = mid(i - 1), c = p[i], t = mid(i), r0 = mr(i - 1), r1 = r[i], r2 = mr(i);
      const steps = Math.min(16, Math.max(1, Math.ceil((dist(f, c) + dist(c, t)) / step)));
      for (let k = 1; k <= steps; k++) {
        const u = k / steps, a = (1 - u) * (1 - u), b = 2 * (1 - u) * u, cc = u * u;
        pts.push({ x: a * f.x + b * c.x + cc * t.x, y: a * f.y + b * c.y + cc * t.y });
        ws.push(a * r0 + b * r1 + cc * r2);
      }
    }
    pts.push(p[n - 1]); ws.push(r[n - 1]);
    return [pts, ws];
  }

  function outline(c, r) {
    const n = c.length, out = [], tan = [];
    let d = { x: 1, y: 0 };
    for (let i = 0; i < n; i++) {
      const a = c[Math.max(0, i - 1)], b = c[Math.min(n - 1, i + 1)], L = dist(a, b);
      if (L > 1e-3) d = { x: (b.x - a.x) / L, y: (b.y - a.y) / L };
      tan.push(d);
    }
    const off = (i, al, ac) => ({ x: c[i].x + (tan[i].x * al - tan[i].y * ac) * r[i], y: c[i].y + (tan[i].y * al + tan[i].x * ac) * r[i] });
    for (let i = 0; i < n; i++) out.push(off(i, 0, 1));
    for (let s = 1; s < CAP; s++) { const a = Math.PI * s / CAP; out.push(off(n - 1, Math.sin(a), Math.cos(a))); }
    for (let i = n - 1; i >= 0; i--) out.push(off(i, 0, -1));
    for (let s = 1; s < CAP; s++) { const a = Math.PI * s / CAP; out.push(off(0, -Math.sin(a), -Math.cos(a))); }
    return out;
  }

  const rgb = hex => { const v = parseInt(hex.slice(1), 16); return [v >> 16 & 255, v >> 8 & 255, v & 255]; };

  /** Draws a stroke `{points, look, whole, breaks}`, dropping faded points; `fade` scales opacity. */
  function drawStroke(ctx, stroke, now, fade = 1) {
    const { points: pts, look: s } = stroke;
    const base = s.width / 2, n = pts.length;
    if (!n) return;
    let radii = new Array(n).fill(base);
    if (!s.ink && stroke.whole) {
      const newest = Math.max(...pts.map(p => p.t));
      radii.fill(base * easeOut4(Math.max(0, 1 - (now - newest) / s.fade)));
    } else if (!s.ink) {
      let behind = 0;
      for (let i = n - 1; i >= 0; i--) {
        if (i + 1 < n) behind += dist(pts[i], pts[i + 1]);
        const age = 1 - (now - pts[i].t) / s.fade, len = 1 - behind / s.trail;
        radii[i] = base * Math.min(easeOut4(Math.max(0, len)), easeOut4(Math.max(0, age)));
      }
      let faded = 0;
      while (faded < n && radii[faded] < 0.1) faded++;
      const drop = faded === n ? n : Math.max(0, faded - 1);
      pts.splice(0, drop); radii = radii.slice(drop);
      if (!pts.length) return;
    }
    const [r, g, b] = rgb(s.color);
    const ends = [...(stroke.breaks || []), pts.length];
    let start = 0;
    for (const end of ends) {
      const part = pts.slice(start, end), widths = radii.slice(start, end);
      start = end;
      if (!part.length) continue;
      const [centre, w] = curveThrough(smooth(part, s.smoothing), widths, CURVE_STEP);
      for (const [k, a, white] of LAYERS[s.style]) {
        const poly = outline(centre, w.map(x => x * k)), m = v => Math.round(v + (255 - v) * white);
        ctx.fillStyle = `rgba(${m(r)},${m(g)},${m(b)},${a * s.opacity / 100 * fade})`;
        ctx.beginPath(); ctx.moveTo(poly[0].x, poly[0].y);
        for (let i = 1; i < poly.length; i++) ctx.lineTo(poly[i].x, poly[i].y);
        ctx.closePath(); ctx.fill('nonzero');
      }
    }
  }

  // ---- Shape snapping, ported from src/shapes.rs ------------------------------------------------
  const TAU = Math.PI * 2, SPACING = 3, SAMPLES = 96;
  const lerp = (a, b, k) => a + (b - a) * Math.min(1, Math.max(0, k));
  const d2 = (a, b) => Math.hypot(b[0] - a[0], b[1] - a[1]);
  const pathLength = pts => { let l = 0; for (let i = 1; i < pts.length; i++) l += d2(pts[i - 1], pts[i]); return l; };

  function resample(pts, count) {
    const total = pathLength(pts);
    if (pts.length < 2 || total <= 0 || count < 2) return Array(Math.max(1, count)).fill(pts[0] || [0, 0]);
    const step = total / (count - 1), out = [pts[0]];
    let seg = 0, walked = 0;
    for (let k = 1; k < count - 1; k++) {
      const target = k * step;
      while (seg < pts.length - 2 && walked + d2(pts[seg], pts[seg + 1]) < target) { walked += d2(pts[seg], pts[seg + 1]); seg++; }
      const a = pts[seg], b = pts[seg + 1], span = Math.max(d2(a, b), 1e-6), t = Math.min(1, Math.max(0, (target - walked) / span));
      out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
    }
    out.push(pts[pts.length - 1]);
    return out;
  }

  function polyline(corners, scale) {
    const out = [corners[0]];
    for (let i = 1; i < corners.length; i++) {
      const a = corners[i - 1], b = corners[i], steps = Math.max(1, Math.ceil(d2(a, b) / (SPACING * scale)));
      for (let k = 1; k <= steps; k++) out.push([a[0] + (b[0] - a[0]) * k / steps, a[1] + (b[1] - a[1]) * k / steps]);
    }
    return out;
  }

  function beginNear(closed, start) {
    const ring = closed.slice(0, -1);
    let first = 0;
    ring.forEach((p, i) => { if (d2(p, start) < d2(ring[first], start)) first = i; });
    const rotated = ring.slice(first).concat(ring.slice(0, first));
    rotated.push(rotated[0]);
    return rotated;
  }

  const centroid = pts => { let x = 0, y = 0; for (const p of pts) { x += p[0]; y += p[1]; } return [x / pts.length, y / pts.length]; };

  function principalAxis(pts) {
    const c = centroid(pts);
    let xx = 0, xy = 0, yy = 0;
    for (const p of pts) { const dx = p[0] - c[0], dy = p[1] - c[1]; xx += dx * dx; xy += dx * dy; yy += dy * dy; }
    const n = pts.length; xx /= n; xy /= n; yy /= n;
    const angle = 0.5 * Math.atan2(2 * xy, xx - yy), mean = (xx + yy) / 2, spread = Math.sqrt((xx - yy) * (xx - yy) / 4 + xy * xy);
    return [c, angle, mean + spread, Math.max(0, mean - spread)];
  }

  function snapAngle(angle, step, tolerance) {
    const nearest = Math.round(angle / step) * step;
    return Math.abs(angle - nearest) <= tolerance ? nearest : angle;
  }

  function rotate(p, about, angle) {
    const s = Math.sin(angle), c = Math.cos(angle), dx = p[0] - about[0], dy = p[1] - about[1];
    return [about[0] + dx * c - dy * s, about[1] + dx * s + dy * c];
  }

  function signedTurn(prev, at, next) {
    if (d2(prev, at) < 1e-3 || d2(at, next) < 1e-3) return 0;
    let turn = Math.atan2(next[1] - at[1], next[0] - at[0]) - Math.atan2(at[1] - prev[1], at[0] - prev[0]);
    while (turn > Math.PI) turn -= TAU;
    while (turn < -Math.PI) turn += TAU;
    return turn;
  }

  function loopTurning(loop) {
    const open = loop.slice(0, -1), n = open.length;
    let signed = 0, absolute = 0;
    for (let i = 0; i < n; i++) { const t = signedTurn(open[(i + n - 1) % n], open[i], open[(i + 1) % n]); signed += t; absolute += Math.abs(t); }
    return [signed, absolute];
  }

  function bend(pts) {
    const a = pts[0], b = pts[pts.length - 1], chord = Math.max(d2(a, b), 1e-6), ux = (b[0] - a[0]) / chord, uy = (b[1] - a[1]) / chord;
    let far = 0;
    for (const p of pts) far = Math.max(far, Math.abs((p[0] - a[0]) * uy - (p[1] - a[1]) * ux));
    return far / chord;
  }

  function simplify(pts, epsilon) {
    const out = [];
    (function recurse(lo, hi) {
      const a = pts[lo], b = pts[hi], chord = d2(a, b);
      let far = 0, index = 0;
      for (let i = lo + 1; i < hi; i++) {
        const p = pts[i];
        const d = chord < 1e-6 ? d2(p, a) : Math.abs((p[0] - a[0]) * (b[1] - a[1]) - (p[1] - a[1]) * (b[0] - a[0])) / chord;
        if (d > far) { far = d; index = i; }
      }
      if (far > epsilon && index > lo) { recurse(lo, index); out.pop(); recurse(index, hi); }
      else { out.push(a); out.push(b); }
    })(0, pts.length - 1);
    return out;
  }

  function turnAt(prev, at, next) {
    let turn = Math.abs(Math.atan2(next[1] - at[1], next[0] - at[0]) - Math.atan2(at[1] - prev[1], at[0] - prev[0]));
    return turn > Math.PI ? TAU - turn : turn;
  }

  const tolerances = s => ({
    line: lerp(0.03, 0.085, s), ellipse: lerp(0.07, 0.16, s), angle: lerp(3, 10, s) * Math.PI / 180,
    equal: lerp(0.07, 0.15, s), close: lerp(0.14, 0.26, s),
  });

  function closedPart(path, tol) {
    const [, , major, minor] = principalAxis(path);
    const size = (Math.sqrt(major) + Math.sqrt(minor)) * 2, start = path[0], from = Math.floor(path.length * 6 / 10);
    let index = from;
    for (let i = from; i < path.length; i++) if (d2(path[i], start) < d2(path[index], start)) index = i;
    if (d2(path[index], start) > tol.close * size) return null;
    if (pathLength(path.slice(index)) > 0.12 * pathLength(path)) return null;
    const loop = path.slice(0, index + 1);
    const nearLoop = p => loop.some(q => d2(p, q) < 0.08 * size);
    if (!path.slice(index).every(nearLoop)) return null;
    if (d2(loop[loop.length - 1], start) < 0.03 * size) loop.pop();
    loop.push(start);
    const [signed, absolute] = loopTurning(loop);
    return Math.round(Math.abs(signed) / TAU) === 1 && absolute < 3.4 * Math.PI ? loop : null;
  }

  function line(path, tol, strength, scale) {
    const [centre, angle] = principalAxis(path), ux = Math.cos(angle), uy = Math.sin(angle);
    const along = p => (p[0] - centre[0]) * ux + (p[1] - centre[1]) * uy;
    const across = p => Math.abs((p[0] - centre[0]) * uy - (p[1] - centre[1]) * ux);
    const first = along(path[0]), last = along(path[path.length - 1]), span = Math.abs(last - first);
    const deviation = Math.max(...path.map(across));
    if (span < 30 * scale || deviation / span > tol.line || pathLength(path) > span * 1.25) return null;
    let ends = [[centre[0] + ux * first, centre[1] + uy * first], [centre[0] + ux * last, centre[1] + uy * last]];
    const mid = [(ends[0][0] + ends[1][0]) / 2, (ends[0][1] + ends[1][1]) / 2];
    const direction = Math.atan2(ends[1][1] - ends[0][1], ends[1][0] - ends[0][0]);
    const turn = snapAngle(direction, strength >= 0.5 ? Math.PI / 4 : Math.PI / 2, tol.angle) - direction;
    ends = ends.map(e => rotate(e, mid, turn));
    return { shape: 'line', parts: [polyline(ends, scale)] };
  }

  function arrowheadShaped(head, start, tip, reach) {
    const corners = simplify(head, 0.18 * reach);
    const side = p => Math.sign((tip[0] - start[0]) * (p[1] - start[1]) - (tip[1] - start[1]) * (p[0] - start[0]));
    const wing = end => d2(tip, end) > 0.4 * reach && turnAt(start, tip, end) > 110 * Math.PI / 180;
    if (corners.length === 2) return wing(corners[1]);
    if (corners.length === 4) return wing(corners[1]) && wing(corners[3]) && d2(corners[2], tip) < 0.35 * reach && side(corners[1]) !== side(corners[3]);
    return false;
  }

  function arrow(path, tol, strength, scale) {
    const start = path[0], furthest = Math.max(...path.map(p => d2(p, start)));
    const tipIndex = path.findIndex(p => d2(p, start) >= furthest - 1);
    const tip = path[tipIndex], shaft = d2(start, tip), head = path.slice(tipIndex);
    const reach = Math.max(...head.map(p => d2(p, tip)));
    if (head.length < 3 || reach / shaft < 0.06 || reach / shaft > 0.45) return null;
    let wingPoint = head[0];
    for (const p of head) if (d2(p, tip) > d2(wingPoint, tip)) wingPoint = p;
    if (turnAt(start, tip, wingPoint) < 110 * Math.PI / 180) return null;
    const ux = (tip[0] - start[0]) / shaft, uy = (tip[1] - start[1]) / shaft;
    const sideways = Math.max(...head.map(p => Math.abs((p[0] - tip[0]) * uy - (p[1] - tip[1]) * ux)));
    if (sideways < 0.35 * reach || pathLength(head) > 4.5 * reach || !arrowheadShaped(head, start, tip, reach)) return null;
    const straight = line(path.slice(0, Math.max(tipIndex, 3) + 1), tol, strength, scale);
    if (!straight) return null;
    const sp = straight.parts[0], from = sp[0], to = sp[sp.length - 1];
    const angle = Math.atan2(to[1] - from[1], to[0] - from[0]), len = Math.min(40 * scale, Math.max(14 * scale, shaft * 0.14)), spread = 28 * Math.PI / 180;
    const wingEnd = side => { const a = angle + Math.PI + side * spread; return [to[0] + len * Math.cos(a), to[1] + len * Math.sin(a)]; };
    return { shape: 'arrow', parts: [sp, polyline([wingEnd(1), to, wingEnd(-1)], scale)] };
  }

  function ellipse(loop, tol, scale) {
    const [centre, axis] = principalAxis(loop), ux = Math.cos(axis), uy = Math.sin(axis);
    const local = p => { const dx = p[0] - centre[0], dy = p[1] - centre[1]; return [dx * ux + dy * uy, -dx * uy + dy * ux]; };
    const projected = loop.map(local);
    const half = k => (Math.max(...projected.map(p => p[k])) - Math.min(...projected.map(p => p[k]))) / 2;
    let a = half(0), b = half(1);
    if (a < 12 * scale || b < 6 * scale || b / a < 0.2) return null;
    const error = Math.sqrt(projected.reduce((sum, p) => { const e = Math.hypot(p[0] / a, p[1] / b) - 1; return sum + e * e; }, 0) / projected.length);
    if (error > tol.ellipse) return null;
    if (Math.abs(a - b) / Math.max(a, b) < tol.equal) a = b = (a + b) / 2;
    const angle = snapAngle(axis, Math.PI / 2, tol.angle);
    const start = local(loop[0]), startAngle = Math.atan2(start[1] / b, start[0] / a), direction = Math.sign(loopTurning(loop)[0]) || 1;
    const perimeter = Math.PI * (3 * (a + b) - Math.sqrt((3 * a + b) * (a + 3 * b)));
    const count = Math.max(24, Math.ceil(perimeter / (SPACING * scale))), s = Math.sin(angle), c = Math.cos(angle);
    const points = [];
    for (let k = 0; k <= count; k++) {
      const t = startAngle + direction * TAU * k / count, x = a * Math.cos(t), y = b * Math.sin(t);
      points.push([centre[0] + x * c - y * s, centre[1] + x * s + y * c]);
    }
    return { shape: 'ellipse', parts: [points] };
  }

  function rectangle(corners, tol, scale) {
    for (let i = 0; i < 4; i++) {
      if (Math.abs(turnAt(corners[(i + 3) % 4], corners[i], corners[(i + 1) % 4]) - Math.PI / 2) >= tol.angle * 2.5) return null;
    }
    let sx = 0, sy = 0;
    for (let i = 0; i < 4; i++) { const a = corners[i], b = corners[(i + 1) % 4], t = 4 * Math.atan2(b[1] - a[1], b[0] - a[0]); sx += Math.cos(t); sy += Math.sin(t); }
    const angle = snapAngle(Math.atan2(sy, sx) / 4, Math.PI / 2, tol.angle), centre = centroid(corners);
    const local = corners.map(c => rotate(c, centre, -angle));
    const xs = local.map(p => p[0]).sort((p, q) => p - q), ys = local.map(p => p[1]).sort((p, q) => p - q);
    let width = (xs[3] - xs[0] + (xs[2] + xs[3]) / 2 - (xs[0] + xs[1]) / 2) / 2;
    let height = (ys[3] - ys[0] + (ys[2] + ys[3]) / 2 - (ys[0] + ys[1]) / 2) / 2;
    if (Math.abs(width - height) / Math.max(width, height) < tol.equal) width = height = (width + height) / 2;
    const lc = [(xs[0] + xs[3]) / 2, (ys[0] + ys[3]) / 2];
    const box = [[-1, -1], [1, -1], [1, 1], [-1, 1]].map(([kx, ky]) => rotate([lc[0] + kx * width / 2, lc[1] + ky * height / 2], centre, angle));
    let area = 0;
    for (let i = 0; i < 4; i++) area += corners[i][0] * corners[(i + 1) % 4][1] - corners[(i + 1) % 4][0] * corners[i][1];
    const ordered = area > 0 ? box : box.slice().reverse();
    ordered.push(ordered[0]);
    return { shape: 'rectangle', parts: [polyline(ordered, scale)] };
  }

  function polygon(loop, tol, scale) {
    const [, , major, minor] = principalAxis(loop), size = (Math.sqrt(major) + Math.sqrt(minor)) * 2;
    const open = simplify(loop, 0.06 * size).slice(0, -1), n = open.length;
    if (n < 3) return null;
    const corners = open.filter((_, i) => turnAt(open[(i + n - 1) % n], open[i], open[(i + 1) % n]) > 35 * Math.PI / 180);
    const indices = corners.map(c => Math.max(0, loop.indexOf(c)));
    for (let k = 0; k < indices.length; k++) {
      const from = indices[k], to = indices[(k + 1) % indices.length];
      const side = from <= to ? loop.slice(from, to + 1) : loop.slice(from).concat(loop.slice(0, to + 1));
      if (side.length >= 3 && bend(side) >= tol.line * 1.6) return null;
    }
    if (corners.length === 3) return { shape: 'triangle', parts: [beginNear(polyline([...corners, corners[0]], scale), loop[0])] };
    if (corners.length === 4) {
      const r = rectangle(corners, tol, scale);
      return r && { shape: 'rectangle', parts: [beginNear(r.parts[0], loop[0])] };
    }
    return null;
  }

  function pull(drawn, ideal, strength) {
    const amount = lerp(0.6, 1, (strength - 0.1) / 0.4);
    if (amount >= 0.999 || ideal.parts.length > 1) return ideal;
    const from = resample(drawn, ideal.parts[0].length);
    return { shape: ideal.shape, parts: [ideal.parts[0].map((to, i) => [lerp(from[i][0], to[0], amount), lerp(from[i][1], to[1], amount)])] };
  }

  /** `points` as [[x, y]], `strength` 0..1. Returns `{shape, parts}` or null. */
  function correct(points, scale, strength) {
    if (strength <= 0 || points.length < 4 || pathLength(points) < 40 * scale) return null;
    const path = resample(points, SAMPLES), tol = tolerances(strength), loop = closedPart(path, tol);
    const ideal = loop ? polygon(loop, tol, scale) || ellipse(loop, tol, scale) : arrow(path, tol, strength, scale) || line(path, tol, strength, scale);
    return ideal && pull(path, ideal, strength);
  }

  function correspondence(drawn, correction) {
    return resample(drawn, Math.max(2, correction.parts.reduce((n, p) => n + p.length, 0)));
  }

  // ---- Cursor motion, as in the app ---------------------------------------------------------------
  const backOut = k => 1 + 2.4 * Math.pow(k - 1, 3) + 1.4 * Math.pow(k - 1, 2);
  const easeOut3 = k => 1 - Math.pow(1 - k, 3);
  /** Scale and rotation (degrees) for `kind`, `ignite` 0..1 (1 once settled), `press` 0..1. */
  function motion(kind, ignite, press) {
    const k = Math.min(1, Math.max(0, ignite)), done = k >= 1;
    let scale = 1, rotate = 0;
    switch (kind) {
      case 'spin':
        if (!done) { scale = 0.5 + 0.5 * backOut(k); rotate = -90 * (1 - easeOut3(k)); }
        scale *= 1 - 0.08 * press; rotate += 45 * press;
        break;
      case 'pulse':
        if (!done) scale = 1.6 - 0.6 * easeOut3(k);
        scale *= 1 - 0.25 * press;
        break;
      case 'tilt':
        if (!done) { scale = 0.6 + 0.4 * backOut(k); rotate = 20 * (1 - easeOut3(k)); }
        scale *= 1 - 0.06 * press; rotate -= 12 * press;
        break;
      default:
        if (!done) scale = 0.35 + 0.65 * backOut(k);
        scale *= 1 - 0.14 * press;
    }
    return { scale, rotate };
  }

  /** Tracks the ignite and press timings and gives the current pose. */
  class CursorPose {
    constructor() { this.ignitedAt = -1e9; this.pressAt = -1e9; this.down = false; }
    ignite(now) { this.ignitedAt = now; }
    press(down, now) { if (down !== this.down) { this.down = down; this.pressAt = now; } }
    pose(kind, now) {
      const ignite = (now - this.ignitedAt) / (kind === 'spin' ? 260 : 200);
      const t = now - this.pressAt;
      const press = this.down ? easeOut3(Math.min(1, t / 90)) : 1 - easeOut3(Math.min(1, t / 140));
      return { ...motion(kind, ignite, press), animating: ignite < 1 || (this.down ? t < 90 : t < 140) };
    }
  }

  const imageCache = new Map();
  /** Called when a cursor image finishes loading, so whatever wanted to draw it can try again. */
  const loadListeners = new Set();
  const onImageLoad = listener => loadListeners.add(listener);
  /** An <img> of `svg` (with COLOR replaced), loaded once. */
  function svgImage(svg, colour) {
    const key = colour + svg;
    let image = imageCache.get(key);
    if (!image) {
      image = new Image();
      image.addEventListener('load', () => loadListeners.forEach(listener => listener()));
      image.src = 'data:image/svg+xml,' + encodeURIComponent(svg.replaceAll('COLOR', colour));
      imageCache.set(key, image);
      if (imageCache.size > 200) imageCache.delete(imageCache.keys().next().value);
    }
    return image;
  }

  function drawCursor(ctx, cursor, colour, x, y, pose) {
    const image = svgImage(cursor.svg, colour);
    if (!image.complete) return;
    const [hx, hy] = cursor.hotspot;
    ctx.save();
    ctx.translate(x, y);
    ctx.rotate(pose.rotate * Math.PI / 180);
    ctx.scale(pose.scale, pose.scale);
    ctx.drawImage(image, -hx, -hy, 48, 48);
    ctx.restore();
  }

  // ---- The stage: an overlay over part of the page ------------------------------------------------
  const PRESETS = {
    Laser: { color: '#ff2a2a', width: 4, opacity: 100, fade: 1000, trail: 500, smoothing: 2, style: 'glow', ink: false, snap: 35 },
    Highlighter: { color: '#ffd400', width: 18, opacity: 40, fade: 3000, trail: 2000, smoothing: 2, style: 'solid', ink: false, snap: 60 },
    Pen: { color: '#1e90ff', width: 4, opacity: 100, fade: 1000, trail: 500, smoothing: 2, style: 'solid', ink: true, snap: 50 },
  };
  const MORPH_MS = 140, CLEAR_MS = 220, RIPPLE_MS = 450, TOAST_MS = 1400;

  class Stage {
    constructor(element, { cursor, onChange } = {}) {
      this.element = element;
      this.canvas = document.createElement('canvas');
      Object.assign(this.canvas.style, { position: 'absolute', inset: '0', width: '100%', height: '100%', zIndex: '5', touchAction: 'none' });
      element.append(this.canvas);
      this.ctx = this.canvas.getContext('2d');
      this.look = { ...PRESETS.Laser };
      this.cursor = cursor;
      this.onChange = onChange || (() => {});
      this.on = false;
      this.strokes = []; this.undoStack = []; this.redoStack = []; this.clearing = []; this.clearedAt = 0;
      this.ripples = []; this.toast = null; this.nextId = 1; this.drawing = null; this.path = [];
      this.pointer = null; this.pose = new CursorPose(); this.frame = 0;
      new ResizeObserver(() => this.resize()).observe(element);
      this.resize();
      this.listen();
      this.applyInput();
      onImageLoad(() => this.wake());
    }

    resize() {
      const dpr = window.devicePixelRatio || 1, r = this.element.getBoundingClientRect();
      this.width = r.width; this.height = r.height; this.dpr = dpr;
      this.canvas.width = Math.round(r.width * dpr); this.canvas.height = Math.round(r.height * dpr);
      this.wake();
    }

    applyInput() {
      this.canvas.style.pointerEvents = this.on ? 'auto' : 'none';
      this.element.style.cursor = this.on ? 'none' : '';
    }

    emit() { this.onChange(this); }

    setOn(on) {
      if (on === this.on) return;
      this.on = on;
      this.finish(false);
      if (on) this.pose.ignite(performance.now());
      this.notify(on ? 'Laser on' : 'Laser off');
      this.applyInput();
      this.emit();
      this.wake();
    }

    setCursor(cursor) { this.cursor = cursor; this.pose.ignite(performance.now()); this.wake(); }
    setLook(partial) { Object.assign(this.look, partial); this.emit(); this.wake(); }
    usePreset(name) { this.look = { ...PRESETS[name], color: name === 'Laser' ? this.look.color : PRESETS[name].color }; this.notify(name); this.emit(); }

    notify(text) { this.toast = { text, at: performance.now() }; this.wake(); }

    local(event) {
      const r = this.canvas.getBoundingClientRect();
      return { x: event.clientX - r.left, y: event.clientY - r.top };
    }

    listen() {
      const c = this.canvas;
      this.element.addEventListener('pointermove', e => { this.pointer = this.local(e); this.wake(); });
      this.element.addEventListener('pointerenter', () => { if (this.on) this.pose.ignite(performance.now()); this.inside = true; });
      this.element.addEventListener('pointerleave', () => { this.inside = false; this.wake(); });
      c.addEventListener('pointerdown', e => {
        if (e.button === 2) return;
        if (e.button !== 0) return;
        c.setPointerCapture(e.pointerId);
        this.begin(this.local(e), e);
      });
      c.addEventListener('pointermove', e => {
        if (!this.drawing) return;
        for (const sample of e.getCoalescedEvents ? e.getCoalescedEvents() : [e]) this.extend(this.local(sample), e);
      });
      c.addEventListener('pointerup', e => { if (this.drawing) { this.extend(this.local(e), e); this.finish(!e.ctrlKey); } });
      c.addEventListener('pointercancel', () => this.finish(true));
      c.addEventListener('contextmenu', e => {
        e.preventDefault();
        this.ripples.push({ ...this.local(e), at: performance.now() });
        this.wake();
      });
      window.addEventListener('keydown', e => {
        if (e.target.closest && e.target.closest('input, textarea, select')) return;
        const key = e.key.toLowerCase();
        if (key === 'l' && !e.ctrlKey) { e.preventDefault(); this.setOn(!this.on); return; }
        if (!this.on) return;
        if (e.key === 'Alt') e.preventDefault();
        if (key === 'escape') this.setOn(false);
        else if (e.ctrlKey && key === 'z') { e.preventDefault(); this.undo(); }
        else if (e.ctrlKey && key === 'y') { e.preventDefault(); this.redo(); }
        else if (e.altKey && key === 'x') { e.preventDefault(); this.clear(() => true); }
      });
      window.addEventListener('keyup', e => { if (e.key === 'Alt' && this.on) e.preventDefault(); });
    }

    begin(point, event) {
      if (!this.on) return;
      const now = performance.now();
      const stroke = { id: this.nextId++, points: [{ ...point, t: now }], look: { ...this.look }, whole: false, breaks: [], morph: null };
      this.strokes.push(stroke);
      this.undoStack.push({ added: stroke.id });
      this.redoStack = [];
      this.drawing = stroke;
      this.path = [[point.x, point.y]];
      this.lock = this.lockFor(event);
      this.locked = this.lock !== 'none';
      this.anchor = point;
      this.pose.press(true, now);
      this.emit();
      this.wake();
    }

    lockFor(event) { return event.altKey ? (event.shiftKey ? 'vertical' : 'horizontal') : 'none'; }

    extend(point, event) {
      const stroke = this.drawing;
      if (!stroke) return;
      const lock = this.lockFor(event);
      if (lock !== this.lock) {
        if (lock === 'none') { this.finish(false); this.begin(point, event); return; }
        const last = stroke.points[stroke.points.length - 1];
        this.anchor = last ? { x: last.x, y: last.y } : point;
        this.locked = true;
        this.lock = lock;
      }
      const p = this.lock === 'horizontal' ? { x: point.x, y: this.anchor.y } : this.lock === 'vertical' ? { x: this.anchor.x, y: point.y } : point;
      const last = stroke.points[stroke.points.length - 1];
      if (last && Math.hypot(p.x - last.x, p.y - last.y) < MIN_STEP) return;
      stroke.points.push({ ...p, t: performance.now() });
      if (stroke.look.snap > 0 && !this.locked) this.path.push([p.x, p.y]);
      this.wake();
    }

    finish(snap) {
      const stroke = this.drawing;
      if (!stroke) return;
      this.drawing = null;
      this.pose.press(false, performance.now());
      if (snap && !this.locked && stroke.look.snap > 0) {
        const correction = correct(this.path, 1, stroke.look.snap / 100);
        if (correction) {
          const now = performance.now();
          stroke.whole = true;
          stroke.look.smoothing = 0;
          stroke.morph = { from: correspondence(this.path, correction), to: correction.parts.flat(), at: now };
          stroke.points = stroke.morph.from.map(([x, y]) => ({ x, y, t: now }));
          stroke.breaks = [];
          let count = 0;
          correction.parts.slice(0, -1).forEach(part => { count += part.length; stroke.breaks.push(count); });
          this.lastShape = correction.shape;
          this.emit();
        }
      }
      this.path = [];
      this.wake();
    }

    undo() {
      while (this.undoStack.length) {
        const edit = this.undoStack.pop();
        if (edit.added) {
          const index = this.strokes.findIndex(s => s.id === edit.added);
          if (index < 0) continue;
          const [stroke] = this.strokes.splice(index, 1);
          this.redoStack.push({ stroke });
          break;
        }
        const restored = edit.cleared.filter(s => s.look.ink || s.whole);
        if (!restored.length) continue;
        this.strokes.push(...restored);
        this.strokes.sort((a, b) => a.id - b.id);
        this.redoStack.push({ clear: restored.map(s => s.id) });
        break;
      }
      this.wake();
    }

    redo() {
      const item = this.redoStack.pop();
      if (!item) return;
      if (item.stroke) {
        const shift = performance.now() - Math.max(...item.stroke.points.map(p => p.t));
        item.stroke.points.forEach(p => (p.t += shift));
        this.strokes.push(item.stroke);
        this.strokes.sort((a, b) => a.id - b.id);
        this.undoStack.push({ added: item.stroke.id });
      } else {
        const keep = this.redoStack;
        this.clear(s => item.clear.includes(s.id));
        this.redoStack = keep;
      }
      this.wake();
    }

    clear(which) {
      this.finish(false);
      const gone = this.strokes.filter(which);
      if (!gone.length) return;
      this.strokes = this.strokes.filter(s => !which(s));
      this.clearing = gone; this.clearedAt = performance.now();
      this.undoStack.push({ cleared: gone });
      this.redoStack = [];
      this.wake();
    }

    wake() { if (!this.frame) this.frame = requestAnimationFrame(t => this.render(t)); }

    render() {
      this.frame = 0;
      const now = performance.now(), ctx = this.ctx;
      ctx.setTransform(this.dpr, 0, 0, this.dpr, 0, 0);
      ctx.clearRect(0, 0, this.width, this.height);
      let animating = false;
      this.strokes = this.strokes.filter(s => {
        if (s === this.drawing || s.look.ink) return true;
        const newest = s.points.length ? Math.max(...s.points.map(p => p.t)) : 0;
        return now - newest < s.look.fade;
      });
      for (const s of this.strokes) {
        if (s.morph) {
          const k = Math.min(1, (now - s.morph.at) / MORPH_MS), t = easeOut3(k);
          s.points.forEach((p, i) => { const [fx, fy] = s.morph.from[i], [tx, ty] = s.morph.to[i]; p.x = fx + (tx - fx) * t; p.y = fy + (ty - fy) * t; });
          if (k >= 1) s.morph = null; else animating = true;
        }
        drawStroke(ctx, s, now);
        if (!s.look.ink && s.points.length) animating = true;
      }
      const clearK = (now - this.clearedAt) / CLEAR_MS;
      if (clearK < 1) {
        for (const s of this.clearing) drawStroke(ctx, { ...s, points: s.points.map(p => ({ ...p })) }, this.clearedAt, (1 - clearK) ** 2);
        animating = true;
      } else this.clearing = [];
      this.ripples = this.ripples.filter(r => now - r.at < RIPPLE_MS);
      for (const r of this.ripples) {
        const k = (now - r.at) / RIPPLE_MS;
        ctx.strokeStyle = this.look.color; ctx.globalAlpha = 0.9 * Math.pow(1 - k, 1.5); ctx.lineWidth = 3 * (1 - 0.5 * k);
        ctx.beginPath(); ctx.arc(r.x, r.y, 6 + 30 * (1 - Math.pow(1 - k, 3)), 0, TAU); ctx.stroke(); ctx.globalAlpha = 1;
        animating = true;
      }
      if (this.toast) {
        const age = now - this.toast.at;
        if (age > TOAST_MS) this.toast = null;
        else { this.drawToast(age); animating = true; }
      }
      if (this.on && this.inside && this.pointer && this.cursor) {
        const pose = this.pose.pose(this.cursor.motion, now);
        drawCursor(ctx, this.cursor, this.look.color, this.pointer.x, this.pointer.y, pose);
        if (pose.animating) animating = true;
      }
      if (animating) this.wake();
    }

    drawToast(age) {
      const ctx = this.ctx, alpha = Math.max(0, Math.min(1, age / 150, (TOAST_MS - age) / 300));
      ctx.font = '600 14px "Segoe UI Variable Text", "Segoe UI", sans-serif';
      const w = ctx.measureText(this.toast.text).width + 56, h = 34, x = (this.width - w) / 2, y = this.height - 56;
      ctx.globalAlpha = alpha;
      ctx.fillStyle = 'rgba(28,28,34,.92)';
      ctx.beginPath(); ctx.roundRect(x, y, w, h, h / 2); ctx.fill();
      ctx.fillStyle = this.look.color; ctx.beginPath(); ctx.arc(x + 23, y + h / 2, 5, 0, TAU); ctx.fill();
      ctx.fillStyle = '#fff'; ctx.textBaseline = 'middle'; ctx.fillText(this.toast.text, x + 38, y + h / 2 + 1);
      ctx.globalAlpha = 1;
    }
  }

  window.Laser = { Stage, CursorPose, drawCursor, svgImage, onImageLoad, motion, correct, PRESETS };
})();
