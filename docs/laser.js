// The laser, in the browser: strokes, shape snapping and cursor motion ported from the app
// (src/stroke.rs, src/shapes.rs, and the cursor animation in src/main.rs), and a stage that
// behaves like the overlay, with a scripted "ghost" that demonstrates it until a visitor takes
// over. Classic script, so it also works opened straight from disk.
(function () {
  'use strict';

  // ---- Strokes, ported from src/stroke.rs ----------------------------------------------------
  const MIN_STEP = 2, CURVE_STEP = 3, CAP = 8;
  const LAYERS = {
    solid: [[1, 1, 0]],
    glow: [[3.4, 0.10, 0], [2.1, 0.20, 0], [1, 1, 0], [0.38, 0.85, 0.6]],
    neon: [[3.8, 0.12, 0], [2.3, 0.26, 0], [1, 1, 0], [0.45, 1, 0.85]],
  };
  const easeOut4 = k => 1 - Math.pow(1 - k, 4);
  // A snapped shape glows brighter once as it settles, then holds for its fade time and fades
  // out over SHAPE_FADE_MS.
  const FLASH_MS = 400, FLASH = [3, 0.45, 0.35], SHAPE_FADE_MS = 500;
  const lifetime = s => s.look.fade + (s.whole ? SHAPE_FADE_MS : 0);
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
    let radii = new Array(n).fill(base), life = 1;
    if (!s.ink && stroke.whole) {
      const newest = Math.max(...pts.map(p => p.t));
      const k = Math.min(1, Math.max(0, (now - newest - s.fade) / SHAPE_FADE_MS));
      life = 1 - k * k * (3 - 2 * k);
    } else if (!s.ink) {
      for (let i = 0; i < n; i++) radii[i] = base * easeOut4(Math.max(0, 1 - (now - pts[i].t) / s.fade));
      let faded = 0;
      while (faded < n && radii[faded] < 0.1) faded++;
      const drop = faded === n ? n : Math.max(0, faded - 1);
      pts.splice(0, drop); radii = radii.slice(drop);
      if (!pts.length) return;
    }
    const [r, g, b] = rgb(s.color);
    const left = stroke.flash ? 1 - (now - stroke.flash) / FLASH_MS : 0;
    const layers = left > 0 ? [[FLASH[0], FLASH[1] * left * left, FLASH[2]], ...LAYERS[s.style]] : LAYERS[s.style];
    const ends = [...(stroke.breaks || []), pts.length];
    let start = 0;
    for (const end of ends) {
      const part = pts.slice(start, end), widths = radii.slice(start, end);
      start = end;
      if (!part.length) continue;
      const [centre, w] = curveThrough(smooth(part, s.smoothing), widths, CURVE_STEP);
      for (const [k, a, white] of layers) {
        const poly = outline(centre, w.map(x => x * k)), m = v => Math.round(v + (255 - v) * white);
        ctx.fillStyle = `rgba(${m(r)},${m(g)},${m(b)},${a * s.opacity / 100 * fade * life})`;
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
    return wrap(Math.atan2(next[1] - at[1], next[0] - at[0]) - Math.atan2(at[1] - prev[1], at[0] - prev[0]));
  }

  function wrap(angle) {
    while (angle > Math.PI) angle -= TAU;
    while (angle < -Math.PI) angle += TAU;
    return angle;
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

  // Every check loosens as the snap setting rises; see the app's Tolerances for what each one is.
  const tolerances = s => ({
    line: lerp(0.03, 0.085, s), side: lerp(0.048, 0.19, s), ellipse: lerp(0.07, 0.22, s), angle: lerp(3, 12, s) * Math.PI / 180,
    equal: lerp(0.07, 0.15, s), close: lerp(0.14, 0.3, s),
    overrun: lerp(0.12, 0.3, s), beside: lerp(0.08, 0.14, s), outline: lerp(0.03, 0.05, s), doubling: lerp(3.4, 4.6, s) * Math.PI,
    head: lerp(0.45, 0.6, s), hook: lerp(110, 100, s) * Math.PI / 180,
    wing: lerp(0.35, 0.25, s), barbs: lerp(0.18, 0.22, s),
  });

  function nearestOn(p, a, b) {
    const dx = b[0] - a[0], dy = b[1] - a[1], length2 = dx * dx + dy * dy;
    const t = length2 > 0 ? Math.min(1, Math.max(0, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / length2)) : 0;
    return [a[0] + dx * t, a[1] + dy * t];
  }

  function crossing(a, b, c, d) {
    const r = [b[0] - a[0], b[1] - a[1]], s = [d[0] - c[0], d[1] - c[1]], den = r[0] * s[1] - r[1] * s[0];
    if (Math.abs(den) < 1e-9) return null;
    const t = ((c[0] - a[0]) * s[1] - (c[1] - a[1]) * s[0]) / den, u = ((c[0] - a[0]) * r[1] - (c[1] - a[1]) * r[0]) / den;
    return t >= 0 && t <= 1 && u >= 0 && u <= 1 ? [a[0] + r[0] * t, a[1] + r[1] * t] : null;
  }

  // Where the end of a stroke comes back to its beginning: the first place it crosses it, or else
  // where the two come nearest. The beginning is the segments before `lead`, the end those from `from`.
  function seam(path, lead, from) {
    let best = null;
    for (let last = from; last < path.length - 1; last++) {
      const c = path[last], d = path[last + 1];
      for (let first = 0; first < Math.min(lead, last - 2); first++) {
        const a = path[first], b = path[first + 1], x = crossing(a, b, c, d);
        if (x) return { first, last, onFirst: x, onLast: x, gap: 0 };
        for (const [onFirst, onLast] of [[nearestOn(c, a, b), c], [nearestOn(d, a, b), d], [a, nearestOn(a, c, d)], [b, nearestOn(b, c, d)]]) {
          const gap = d2(onFirst, onLast);
          if (!best || gap < best.gap) best = { first, last, onFirst, onLast, gap };
        }
      }
    }
    return best;
  }

  function closedPart(path, tol) {
    const [, , major, minor] = principalAxis(path);
    const size = (Math.sqrt(major) + Math.sqrt(minor)) * 2, total = pathLength(path);
    // The loop may begin a little way in, where the end comes back to: the start can be a short
    // lead-in, as when the end lands on the first side rather than on the very first point.
    let lead = 1, walked = 0;
    while (lead < path.length - 1 && walked + d2(path[lead - 1], path[lead]) <= tol.overrun * total) walked += d2(path[lead - 1], path[lead++]);
    const s = seam(path, lead, Math.floor(path.length * 6 / 10));
    if (!s || s.gap > tol.close * size) return null;
    const before = path.slice(0, s.first + 1).concat([s.onFirst]), after = [s.onLast].concat(path.slice(s.last + 1));
    if (pathLength(after) > tol.overrun * total) return null;
    // Cut at the seam, the loop has no jog where its ends meet, which would read as a corner.
    const loop = [];
    for (const p of [s.onFirst, ...path.slice(s.first + 1, s.last + 1), s.onLast, s.onFirst]) if (!loop.length || d2(loop[loop.length - 1], p) > 1e-3) loop.push(p);
    if (loop.length < 4) return null;
    // Beyond the seam the stroke stays on the loop, as an overshoot does, or stops soon, as where a
    // corner is crossed; a longer tail heads off somewhere else.
    const nearLoop = p => loop.some(q => d2(p, q) < tol.beside * size);
    const stub = part => pathLength(part) <= 0.06 * total || part.every(nearLoop);
    if (!stub(before) || !stub(after)) return null;
    // Once round, counted about the middle so a jog where the ends meet can't flip it; the doubling
    // back of a scribble is counted on the outline, so a hand's tremor along a box's sides doesn't
    // add up to look like it.
    const middle = centroid(loop), bearing = p => Math.atan2(p[1] - middle[1], p[0] - middle[0]);
    let winding = 0;
    for (let i = 1; i < loop.length; i++) winding += wrap(bearing(loop[i]) - bearing(loop[i - 1]));
    const [, absolute] = loopTurning(simplify(loop, tol.outline * size));
    if (Math.round(Math.abs(winding) / TAU) !== 1 || absolute >= tol.doubling) return null;
    return loop;
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

  function arrowheadShaped(head, start, tip, reach, tol) {
    const corners = simplify(head, tol.barbs * reach);
    const side = p => Math.sign((tip[0] - start[0]) * (p[1] - start[1]) - (tip[1] - start[1]) * (p[0] - start[0])) || 0;
    const wing = end => d2(tip, end) > (tol.wing + 0.05) * reach && turnAt(start, tip, end) > tol.hook;
    if (corners.length === 2) return wing(corners[1]);
    if (corners.length === 4) return wing(corners[1]) && wing(corners[3]) && d2(corners[2], tip) < (0.7 - tol.wing) * reach && side(corners[1]) * side(corners[3]) < 0;
    return false;
  }

  function arrow(path, tol, strength, scale) {
    const start = path[0], furthest = Math.max(...path.map(p => d2(p, start)));
    // The first pass at the tip: a double-winged head passes it again, maybe a little further out.
    let tipIndex = path.findIndex(p => d2(p, start) >= furthest - Math.max(1, 0.05 * furthest));
    while (tipIndex < path.length - 1 && d2(path[tipIndex + 1], start) >= d2(path[tipIndex], start)) tipIndex++;
    const tip = path[tipIndex], shaft = d2(start, tip), head = path.slice(tipIndex);
    const reach = Math.max(...head.map(p => d2(p, tip)));
    if (head.length < 3 || reach / shaft < 0.06 || reach / shaft > tol.head) return null;
    let wingPoint = head[0];
    for (const p of head) if (d2(p, tip) > d2(wingPoint, tip)) wingPoint = p;
    if (turnAt(start, tip, wingPoint) < tol.hook) return null;
    const ux = (tip[0] - start[0]) / shaft, uy = (tip[1] - start[1]) / shaft;
    const sideways = Math.max(...head.map(p => Math.abs((p[0] - tip[0]) * uy - (p[1] - tip[1]) * ux)));
    if (sideways < tol.wing * reach || pathLength(head) > (4 + 2 * tol.barbs) * reach || !arrowheadShaped(head, start, tip, reach, tol)) return null;
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
    const count = Math.min(180, Math.max(24, Math.ceil(perimeter / (SPACING * scale)))), s = Math.sin(angle), c = Math.cos(angle);
    const points = [];
    for (let k = 0; k <= count; k++) {
      const t = startAngle + direction * TAU * k / count, x = a * Math.cos(t), y = b * Math.sin(t);
      points.push([centre[0] + x * c - y * s, centre[1] + x * s + y * c]);
    }
    return { shape: 'ellipse', parts: [points] };
  }

  function rectangle(corners, tol, scale) {
    for (let i = 0; i < 4; i++) {
      if (Math.abs(turnAt(corners[(i + 3) % 4], corners[i], corners[(i + 1) % 4]) - Math.PI / 2) >= tol.angle * 3) return null;
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

  // The path without `fraction` of its length at each end.
  function trim(pts, fraction) {
    const total = pathLength(pts), from = fraction * total, to = (1 - fraction) * total, out = [];
    let walked = 0;
    for (let i = 1; i < pts.length; i++) {
      const a = pts[i - 1], b = pts[i], step = d2(a, b);
      const pointAt = d => { const k = step > 0 ? Math.min(1, Math.max(0, (d - walked) / step)) : 0; return [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k]; };
      if (!out.length && walked + step >= from) out.push(pointAt(from));
      if (walked + step >= to) { out.push(pointAt(to)); break; }
      if (out.length) out.push(b);
      walked += step;
    }
    return out;
  }

  // Where the line through a and b meets the line through c and d.
  function meeting(a, b, c, d) {
    const r = [b[0] - a[0], b[1] - a[1]], s = [d[0] - c[0], d[1] - c[1]], den = r[0] * s[1] - r[1] * s[0];
    if (Math.abs(den) < 1e-6 * Math.max(Math.hypot(...r) * Math.hypot(...s), 1e-6)) return null;
    const t = ((c[0] - a[0]) * s[1] - (c[1] - a[1]) * s[0]) / den;
    return [a[0] + r[0] * t, a[1] + r[1] * t];
  }

  // The corners of a closed outline as {first, last, at}. A hand rounds corners off, which the
  // outline keeps as two or three gentler bends close together, so neighbouring bends the same
  // way are taken as one corner where the sides either side of them meet.
  function cornersOf(open) {
    const n = open.length, at = i => open[i % n], deg = Math.PI / 180;
    const turn = i => signedTurn(at(i + n - 1), at(i), at(i + 1)), side = i => d2(at(i), at(i + 1));
    // Start after the longest side: no rounded corner has its ends either side of it.
    let longest = 0;
    for (let i = 1; i < n; i++) if (side(i) >= side(longest)) longest = i;
    const found = [];
    for (let i = longest + 1; i <= longest + n;) {
      const first = i;
      let last = i, total = turn(i);
      // The next bend joins while it turns the same way, neither is sharp enough to be a corner of
      // its own, and the edge between is short beside the side leading in.
      while (last < longest + n) {
        const next = turn(last + 1);
        const gentle = total * next > 0 && Math.abs(turn(last)) < 120 * deg && Math.abs(next) < 120 * deg;
        if (!gentle || side(last) >= 0.4 * side(first + n - 1) || Math.abs(total + next) > 150 * deg) break;
        last++;
        total += next;
      }
      if (Math.abs(total) > 35 * deg) {
        let point = at(first);
        if (first !== last) {
          const b = at(first), c = at(last), met = meeting(at(first + n - 1), b, c, at(last + 1));
          const group = [];
          for (let k = first; k <= last; k++) group.push(at(k));
          point = met && d2(met, b) < 2 * d2(b, c) ? met : centroid(group);
        }
        found.push({ first: first % n, last: last % n, at: point });
      }
      i = last + 1;
    }
    return found;
  }

  function polygon(loop, tol, scale) {
    const [, , major, minor] = principalAxis(loop), size = (Math.sqrt(major) + Math.sqrt(minor)) * 2;
    const open = simplify(loop, 0.06 * size).slice(0, -1), n = open.length;
    if (n < 3) return null;
    const found = cornersOf(open), corners = found.map(c => c.at);
    // Each side must be straight between its corners, leaving out the rounding at each end.
    for (let k = 0; k < found.length; k++) {
      const from = Math.max(0, loop.indexOf(open[found[k].last])), to = Math.max(0, loop.indexOf(open[found[(k + 1) % found.length].first]));
      const side = from <= to ? loop.slice(from, to + 1) : loop.slice(from).concat(loop.slice(0, to + 1));
      if (side.length >= 3 && bend(trim(side, 0.15)) >= tol.side) return null;
    }
    // A corner that all but doubles back is a spike, as where a thin box's outline lost a corner.
    const m = corners.length;
    if (corners.some((c, k) => turnAt(corners[(k + m - 1) % m], c, corners[(k + 1) % m]) > 160 * Math.PI / 180)) return null;
    if (corners.length === 3) return { shape: 'triangle', parts: [beginNear(polyline([...corners, corners[0]], scale), loop[0])] };
    if (corners.length === 4) {
      const r = rectangle(corners, tol, scale);
      return r && { shape: 'rectangle', parts: [beginNear(r.parts[0], loop[0])] };
    }
    return null;
  }

  // How far a stroke lies from a shape: the root mean square of each point's distance to it.
  function misfit(pts, shape) {
    const outline = shape.parts.flat();
    let squares = 0;
    for (const p of pts) {
      let best = Infinity;
      for (let i = 1; i < outline.length; i++) best = Math.min(best, d2(p, nearestOn(p, outline[i - 1], outline[i])));
      squares += best * best;
    }
    return Math.sqrt(squares / pts.length);
  }

  /** `points` as [[x, y]], `strength` 0..1. Returns `{shape, parts}` or null. */
  function correct(points, scale, strength) {
    const walked = pathLength(points);
    if (strength <= 0 || points.length < 4 || !Number.isFinite(walked) || walked < 40 * scale) return null;
    const path = resample(points, SAMPLES);
    // Bends finer than the samples are lost to them; whatever goes that much further isn't a shape.
    if (pathLength(path) < 0.5 * walked) return null;
    const tol = tolerances(strength), loop = closedPart(path, tol);
    if (!loop) return arrow(path, tol, strength, scale) || line(path, tol, strength, scale);
    // A box with rounded corners may pass for an ellipse too, and a lumpy circle for a box:
    // whichever lies closer to the stroke is the one meant.
    const box = polygon(loop, tol, scale), round = ellipse(loop, tol, scale);
    if (box && round) return misfit(loop, box) <= misfit(loop, round) ? box : round;
    return box || round;
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
      const ignite = (now - this.ignitedAt) / (kind === 'spin' ? 200 : 167);
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
    Laser: { color: '#ff2a2a', width: 4, opacity: 100, fade: 1000, smoothing: 2, style: 'glow', ink: false, snap: 35 },
    Highlighter: { color: '#ffd400', width: 18, opacity: 40, fade: 3000, smoothing: 2, style: 'solid', ink: false, snap: 60 },
    Pen: { color: '#1e90ff', width: 4, opacity: 100, fade: 1000, smoothing: 2, style: 'solid', ink: true, snap: 50 },
  };
  const SNAP_MS = 160, CLEAR_MS = 220, RIPPLE_MS = 400, RIPPLE_REACH = 60, TOAST_MS = 1400, TOAST_IN = 250, TOAST_OUT = 167;
  /** How far a snapping stroke has moved towards its shape, `ms` in: a spring that sets off at
   *  once, overshoots by a few percent and lands exactly at SNAP_MS, as in src/main.rs. */
  function spring(ms) {
    if (ms >= SNAP_MS) return 1;
    const t = ms / 1000, decay = 31.6, frequency = 30.3, start = 18.8;
    return 1 - Math.exp(-decay * t) * (Math.cos(frequency * t) + (decay - start) / frequency * Math.sin(frequency * t));
  }

  class Stage {
    /// `manual`: only takeOver() stops the ghost, and the page under it stays usable while it
    /// plays. `keys`: whether this stage answers the keyboard.
    constructor(element, { cursor, onChange, manual = false, keys = true } = {}) {
      this.element = element;
      this.canvas = document.createElement('canvas');
      Object.assign(this.canvas.style, { position: 'absolute', inset: '0', width: '100%', height: '100%', zIndex: '5', touchAction: 'none' });
      element.append(this.canvas);
      this.ctx = this.canvas.getContext('2d');
      this.look = { ...PRESETS.Laser };
      this.cursor = cursor;
      this.onChange = onChange || (() => {});
      this.manual = manual; this.keys = keys;
      this.on = false;
      this.strokes = []; this.undoStack = []; this.redoStack = []; this.clearing = []; this.clearedAt = 0;
      this.ripples = []; this.toast = null; this.nextId = 1; this.drawing = null; this.path = [];
      this.pointer = null; this.pose = new CursorPose(); this.frame = 0;
      new ResizeObserver(() => this.resize()).observe(element);
      this.inView = true;
      new IntersectionObserver(([entry]) => { this.inView = entry.isIntersecting; this.pauseGhost(); }).observe(element);
      document.addEventListener('visibilitychange', () => this.pauseGhost());
      this.resize();
      this.watchPixelRatio();
      this.listen();
      this.applyInput();
      onImageLoad(() => this.wake());
    }

    /// Sized from the canvas's own box (inside any border), at the current pixel ratio, so a
    /// CSS pixel on the canvas is exactly a CSS pixel under the pointer.
    resize() {
      const dpr = window.devicePixelRatio || 1, r = this.canvas.getBoundingClientRect();
      this.width = r.width; this.height = r.height; this.dpr = dpr;
      this.canvas.width = Math.round(r.width * dpr); this.canvas.height = Math.round(r.height * dpr);
      const g = this.ghost;
      if (g && g.path) {
        const began = !!this.drawing;
        this.finish(false);
        if (began && g.spec.after) g.spec.after(this);
        g.path = null; g.travel = null;
        g.index--; g.at = performance.now();
      }
      this.wake();
    }

    /// Moving the window to a monitor with different scaling changes the pixel ratio.
    watchPixelRatio() {
      const query = matchMedia(`(resolution: ${window.devicePixelRatio || 1}dppx)`);
      query.addEventListener('change', () => { this.resize(); this.watchPixelRatio(); }, { once: true });
    }

    applyInput() {
      const live = this.on && !(this.manual && this.ghost);
      this.canvas.style.pointerEvents = live ? 'auto' : 'none';
      this.element.style.cursor = live ? 'none' : '';
    }

    /** The visitor's mouse, unless a manual stage's ghost has the pointer. */
    visitor() { return !(this.manual && this.ghost); }

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
      this.element.addEventListener('pointermove', e => { if (!this.visitor()) return; this.takeOver(); this.pointer = this.local(e); this.wake(); });
      this.element.addEventListener('pointerenter', () => { if (!this.visitor()) return; this.takeOver(); if (this.on) this.pose.ignite(performance.now()); this.inside = true; });
      this.element.addEventListener('pointerleave', () => { if (!this.visitor()) return; this.inside = false; this.wake(); });
      // One pointer owns a gesture; a second finger is ignored until it ends.
      const mine = e => this.drawing && e.pointerId === this.pointerId;
      c.addEventListener('pointerdown', e => {
        if (e.button !== 0 || this.drawing) return;
        c.setPointerCapture(e.pointerId);
        this.pointerId = e.pointerId;
        this.begin(this.local(e), e);
      });
      c.addEventListener('pointermove', e => {
        if (!mine(e)) return;
        for (const sample of e.getCoalescedEvents ? e.getCoalescedEvents() : [e]) this.extend(this.local(sample), e);
      });
      c.addEventListener('pointerup', e => { if (mine(e)) { this.extend(this.local(e), e); this.finish(!e.ctrlKey); } });
      c.addEventListener('pointercancel', e => { if (mine(e)) this.finish(false); });
      c.addEventListener('contextmenu', e => {
        e.preventDefault();
        this.ripples.push({ ...this.local(e), at: performance.now() });
        this.wake();
      });
      if (!this.keys) return;
      window.addEventListener('keydown', e => {
        if (e.target.closest && e.target.closest('input, textarea, select')) return;
        if (!this.visitor()) return;
        const key = e.key.toLowerCase();
        if (key === 'l' || key === 'escape' || e.ctrlKey || e.altKey) this.takeOver();
        if (key === 'l' && !e.ctrlKey) { e.preventDefault(); if (!e.repeat) this.setOn(!this.on); return; }
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
      const stroke = { id: this.nextId++, points: [{ ...point, t: now }], look: { ...this.look }, whole: false, breaks: [], morph: null, ghost: !!this.ghost };
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
      if (stroke.look.snap > 0 && !this.locked) {
        // Long holds thin out rather than grow without end, as in the app.
        if (this.path.length >= 4096) this.path = this.path.filter((_, i) => i % 2 === 0);
        this.path.push([p.x, p.y]);
      }
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
          stroke.flash = now;
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
      if (this.drawing) return;
      while (this.undoStack.length) {
        const edit = this.undoStack.pop();
        if (edit.added) {
          const index = this.strokes.findIndex(s => s.id === edit.added);
          if (index < 0) continue;
          const [stroke] = this.strokes.splice(index, 1);
          this.redoStack.push({ stroke });
          break;
        }
        const now = performance.now();
        const alive = s => s.look.ink || (s.points.length && now - Math.max(...s.points.map(p => p.t)) < lifetime(s));
        const restored = edit.cleared.filter(alive);
        if (!restored.length) continue;
        this.clearing = [];
        this.strokes.push(...restored);
        this.strokes.sort((a, b) => a.id - b.id);
        this.redoStack.push({ clear: restored.map(s => s.id) });
        break;
      }
      this.wake();
    }

    redo() {
      if (this.drawing) return;
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

    // ---- The ghost: a scripted hand that shows the laser off before anyone touches it ----------
    // A script is a list of steps. A step is a gesture, a list of [x, y] points in fractions of the
    // stage, or a function asked just before its turn, which may do something (change the look,
    // the scene) and give a gesture, or `{ points, wait, speed, lock, snap, draw, before, after }`,
    // or nothing. Gestures are drawn at a human pace through the same input path as the mouse, so
    // they snap like real strokes; `lock` holds Alt ('horizontal') or Shift+Alt ('vertical'),
    // `snap: false` lets go with Ctrl, `draw: false` only moves there. `before` and `after` are
    // called as the button goes down and comes up. Between gestures the pointer glides to the next
    // one. None of it enters undo history.

    play(script, { pause = 900, again = 2600, start = 0, delay = 500 } = {}) {
      if (matchMedia('(prefers-reduced-motion: reduce)').matches) return;
      this.finish(false);
      this.ghost = { script, pause, again, index: start, at: performance.now() + delay, path: null, travel: null, pausedAt: null };
      this.forgetHistory();
      this.setOn(true);
      this.applyInput();
      this.toast = null;
      this.pauseGhost();
      this.emit();
      this.wake();
    }

    /** The visitor moved or used a control: the ghost lets go, mid-stroke if need be. */
    takeOver() {
      if (!this.ghost) return;
      this.ghost = null;
      if (this.drawing) this.finish(false);
      // Its marks fade out as a clear does, but nothing of them is left to undo.
      const marks = this.strokes.filter(s => s.ghost);
      if (marks.length) {
        this.strokes = this.strokes.filter(s => !s.ghost);
        this.clearing = marks; this.clearedAt = performance.now();
      }
      this.forgetHistory();
      this.applyInput();
      this.onChange(this);
      this.wake();
    }

    /** Ends the ghost and leaves the stage inert: its marks fade and input passes through. */
    retire() {
      this.takeOver();
      this.finish(false);
      if (this.strokes.length) { this.clearing = this.strokes; this.strokes = []; this.clearedAt = performance.now(); }
      this.forgetHistory();
      this.on = false;
      this.inside = false;
      this.applyInput();
      this.emit();
      this.wake();
    }

    forgetHistory() { this.undoStack = []; this.redoStack = []; }

    /** Holds the ghost while the stage is out of view, and resumes it on the same beat. */
    pauseGhost() {
      const g = this.ghost;
      if (!g) return;
      const paused = !this.inView || document.hidden, now = performance.now();
      if (paused && g.pausedAt === null) g.pausedAt = now;
      else if (!paused && g.pausedAt !== null) {
        const away = now - g.pausedAt;
        g.at += away;
        g.started += away;
        if (g.travel) g.travel.start += away;
        for (const s of new Set([...this.strokes, ...this.clearing])) {
          for (const p of s.points) p.t += away;
          if (s.morph) s.morph.at += away;
          if (s.flash) s.flash += away;
        }
        this.clearedAt += away;
        g.pausedAt = null;
        this.wake();
      }
    }

    advanceGhost(now) {
      const g = this.ghost;
      if (!g || g.pausedAt !== null || now < g.at) return;
      if (!g.path) {
        if (g.index >= g.script.length) {
          // Rest, clear what's left, and start again.
          this.clear(() => true);
          this.forgetHistory();
          g.index = 0;
          g.at = now + g.again;
          return;
        }
        const gesture = g.script[g.index++];
        const step = typeof gesture === 'function' ? gesture(this) : gesture;
        const spec = Array.isArray(step) ? { points: step } : step || {};
        if (!this.ghost || this.ghost !== g) return; // the step handed over or restarted
        if (!spec.points || spec.points.length < 2) { g.at = now + (spec.wait || 0); return; }
        g.spec = spec;
        g.event = spec.lock ? { altKey: true, shiftKey: spec.lock === 'vertical' } : {};
        g.path = this.ghostPath(spec.points, spec.speed || 1);
        const first = g.path.points[0], from = this.pointer, far = from && this.inside ? Math.hypot(first.x - from.x, first.y - from.y) : 0;
        g.travel = far > 12 ? { from: { x: from.x, y: from.y }, start: now, ms: Math.min(650, Math.max(200, far / 1.5)) } : null;
        if (!g.travel) { this.pointer = first; this.inside = true; }
      }
      if (g.travel) {
        const k = Math.min(1, (now - g.travel.start) / g.travel.ms), e = k < 0.5 ? 4 * k * k * k : 1 - Math.pow(2 - 2 * k, 3) / 2;
        const to = g.path.points[0], from = g.travel.from;
        this.pointer = { x: from.x + (to.x - from.x) * e, y: from.y + (to.y - from.y) * e };
        this.inside = true;
        if (k < 1) return;
        g.travel = null;
      }
      if (g.spec.draw === false) {
        g.path = null;
        g.at = now + (g.spec.wait ?? g.pause);
        if (g.spec.after) g.spec.after(this);
        return;
      }
      if (!this.drawing) {
        if (g.spec.before) g.spec.before(this);
        if (this.ghost !== g) return;
        g.started = now;
        this.begin(g.path.points[0], g.event);
        if (!this.drawing) { g.path = null; return; }
      }
      const elapsed = now - g.started, points = g.path.points;
      const target = Math.min(points.length - 1, Math.floor(elapsed / g.path.step));
      for (let i = g.path.done + 1; i <= target; i++) {
        this.pointer = points[i];
        this.extend(points[i], g.event);
      }
      g.path.done = target;
      this.pointer = points[target];
      this.inside = true;
      if (target >= points.length - 1) {
        this.finish(g.spec.snap !== false);
        g.path = null;
        g.at = now + (g.spec.wait ?? g.pause);
        if (g.spec.after) g.spec.after(this);
      }
    }

    /** A gesture as pointer positions about every 4 ms, Catmull-Rom smooth, at about 0.9 px/ms
     *  times `speed`. */
    ghostPath(gesture, speed = 1) {
      const px = gesture.map(([x, y]) => ({ x: x * this.width, y: y * this.height }));
      const points = [];
      for (let i = 0; i < px.length - 1; i++) {
        const a = px[Math.max(0, i - 1)], b = px[i], c = px[i + 1], d = px[Math.min(px.length - 1, i + 2)];
        const steps = Math.max(2, Math.ceil(Math.hypot(c.x - b.x, c.y - b.y) / 3.6));
        for (let k = 0; k < steps; k++) {
          const u = k / steps, u2 = u * u, u3 = u2 * u;
          const at = v => 0.5 * (2 * b[v] + (-a[v] + c[v]) * u + (2 * a[v] - 5 * b[v] + 4 * c[v] - d[v]) * u2 + (-a[v] + 3 * b[v] - 3 * c[v] + d[v]) * u3);
          points.push({ x: at('x'), y: at('y') });
        }
      }
      points.push(px[px.length - 1]);
      return { points, step: 4 / speed, done: 0 };
    }

    render() {
      this.frame = 0;
      if (this.ghost && this.ghost.pausedAt !== null) return;
      const now = performance.now(), ctx = this.ctx;
      this.advanceGhost(now);
      ctx.setTransform(this.dpr, 0, 0, this.dpr, 0, 0);
      ctx.clearRect(0, 0, this.width, this.height);
      let animating = false;
      this.strokes = this.strokes.filter(s => {
        if (s === this.drawing || s.look.ink) return true;
        const newest = s.points.length ? Math.max(...s.points.map(p => p.t)) : 0;
        return now - newest < lifetime(s);
      });
      for (const s of this.strokes) {
        if (s.morph) {
          const elapsed = now - s.morph.at, t = spring(elapsed);
          s.points.forEach((p, i) => { const [fx, fy] = s.morph.from[i], [tx, ty] = s.morph.to[i]; p.x = fx + (tx - fx) * t; p.y = fy + (ty - fy) * t; });
          if (elapsed >= SNAP_MS) s.morph = null; else animating = true;
        }
        if (s.flash) {
          if (now - s.flash >= FLASH_MS) s.flash = null; else animating = true;
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
        ctx.beginPath(); ctx.arc(r.x, r.y, 6 + (RIPPLE_REACH - 6) * (1 - Math.pow(1 - k, 3)), 0, TAU); ctx.stroke(); ctx.globalAlpha = 1;
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
      if (animating || (this.ghost && this.ghost.pausedAt === null)) this.wake();
    }

    drawToast(age) {
      const ctx = this.ctx, alpha = Math.max(0, Math.min(easeOut3(Math.min(1, age / TOAST_IN)), (TOAST_MS - age) / TOAST_OUT));
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
