// Gestures that aim at elements on the page, for a Laser.Stage's ghost. Each builder measures
// when it's called, so marks land right at any size, and returns points in fractions of the stage.
(function () {
  'use strict';

  function kit(stage) {
    const frame = () => stage.canvas.getBoundingClientRect();
    /** An element's box in stage pixels. */
    const box = el => {
      const d = frame(), r = (typeof el === 'string' ? document.querySelector(el) : el).getBoundingClientRect();
      return { x: r.left - d.left, y: r.top - d.top, w: r.width, h: r.height };
    };
    const out = pts => { const d = frame(); return pts.map(([x, y]) => [x / d.width, y / d.height]); };
    const wobble = (i, k) => Math.sin(i * 1.7 + k) * 0.5 + Math.sin(i * 0.6 + k * 2) * 0.5;

    /** A loose loop around, a little past where it started, as a hand draws one. */
    function circle(el, { pad = 14, seed = 0 } = {}) {
      const b = box(el);
      return circleAt(b.x + b.w / 2, b.y + b.h / 2, b.w / 2 + pad, b.h / 2 + pad * 1.1, seed);
    }
    function circleAt(cx, cy, rx, ry = rx, seed = 0) {
      return out(Array.from({ length: 28 }, (_, i) => {
        const t = -0.6 + seed + i / 27 * Math.PI * 2.1, w = 1 + 0.035 * wobble(i, seed);
        return [cx + rx * Math.cos(t) * w, cy + ry * Math.sin(t) * w];
      }));
    }

    function underline(el, { gap = 6, wave = 2.5, from = 0, to = 1 } = {}) {
      const b = box(el), y = b.y + b.h + gap;
      return out(Array.from({ length: 12 }, (_, i) => [b.x + b.w * (from + (to - from) * i / 11), y + wave * wobble(i, 1)]));
    }

    /** Across the middle, for the highlighter; with lock: 'horizontal' it comes out ruler-straight. */
    function across(el, { pad = 6, wave = 4 } = {}) {
      const b = box(el), y = b.y + b.h / 2;
      return out(Array.from({ length: 14 }, (_, i) => [b.x - pad + (b.w + pad * 2) * i / 13, y + wave * wobble(i, 2)]));
    }

    /** Down one side, for a vertical lock. */
    function down(el, { side = 'left', pad = 10, wave = 4 } = {}) {
      const b = box(el), x = side === 'left' ? b.x - pad : b.x + b.w + pad;
      return out(Array.from({ length: 12 }, (_, i) => [x + wave * wobble(i, 3), b.y + b.h * i / 11]));
    }

    /** An arrow onto an edge: from 'right', 'left', 'above' or 'below', with one barb. */
    function arrow(el, { from = 'right', length = 120, gap = 10, angle = 30 } = {}) {
      const b = box(el), a = angle * Math.PI / 180;
      let tip, dir;
      if (from === 'right') { tip = [b.x + b.w + gap, b.y + b.h / 2]; dir = [Math.cos(a), Math.sin(a)]; }
      else if (from === 'left') { tip = [b.x - gap, b.y + b.h / 2]; dir = [-Math.cos(a), Math.sin(a)]; }
      else if (from === 'above') { tip = [b.x + b.w / 2, b.y - gap]; dir = [-Math.sin(a), -Math.cos(a)]; }
      else { tip = [b.x + b.w / 2, b.y + b.h + gap]; dir = [Math.sin(a), Math.cos(a)]; }
      return arrowAt([tip[0] + dir[0] * length, tip[1] + dir[1] * length], tip);
    }
    function arrowAt(tail, tip) {
      const length = Math.hypot(tail[0] - tip[0], tail[1] - tip[1]), dir = [(tail[0] - tip[0]) / length, (tail[1] - tip[1]) / length];
      const shaft = Array.from({ length: 9 }, (_, i) => [tail[0] + (tip[0] - tail[0]) * i / 8 + 1.5 * wobble(i, 4), tail[1] + (tip[1] - tail[1]) * i / 8]);
      const turn = 35 * Math.PI / 180, ux = dir[0], uy = dir[1];
      const barb = k => [tip[0] + k * (ux * Math.cos(turn) - uy * Math.sin(turn)), tip[1] + k * (ux * Math.sin(turn) + uy * Math.cos(turn))];
      const reach = Math.min(30, Math.max(16, length * 0.22));
      return out(shaft.concat([barb(reach * 0.7), barb(reach)]));
    }

    /** A rough box, corner by corner. */
    function rect(el, { pad = 10 } = {}) {
      const b = box(el);
      return rectAt(b.x - pad, b.y - pad, b.x + b.w + pad, b.y + b.h + pad);
    }
    function rectAt(l, t, r, btm) {
      const corners = [[l + 2, t + 1], [r - 1, t - 2], [r + 2, btm + 1], [l - 1, btm - 2], [l + 1, t + 6]];
      const pts = [];
      for (let i = 0; i < corners.length - 1; i++) {
        const [a, c] = [corners[i], corners[i + 1]];
        for (let k = 0; k < 7; k++) pts.push([a[0] + (c[0] - a[0]) * k / 7 + wobble(k + i, 5), a[1] + (c[1] - a[1]) * k / 7 + wobble(k + i, 6)]);
      }
      pts.push(corners[corners.length - 1]);
      return out(pts);
    }

    /** A rough triangle around a point, in pixels: begun partway along a side, as a hand does,
     *  and carried a little past where it began. */
    function triangle(cx, cy, size) {
      const c = [0, 1, 2].map(i => { const t = -Math.PI / 2 + i * Math.PI * 2 / 3; return [cx + size * Math.cos(t), cy + size * Math.sin(t)]; });
      const along = (a, b, k) => [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k];
      const ring = [along(c[0], c[1], 0.3), c[1], c[2], c[0], along(c[0], c[1], 0.38)], pts = [];
      for (let i = 0; i < ring.length - 1; i++) for (let k = 0; k < 8; k++) {
        const [x, y] = along(ring[i], ring[i + 1], k / 8);
        pts.push([x + 0.8 * wobble(i * 8 + k, 7), y + 0.8 * wobble(i * 8 + k, 8)]);
      }
      pts.push(ring[ring.length - 1]);
      return out(pts);
    }

    /** A tick just to the right of something, left freehand. */
    function tick(el, { gap = 18, size = 22 } = {}) {
      const b = box(el), x = b.x + b.w + gap, y = b.y + b.h / 2;
      return out([[x, y], [x + size * 0.3, y + size * 0.4], [x + size * 0.45, y + size * 0.55], [x + size * 0.8, y - size * 0.2], [x + size * 1.1, y - size * 0.7]]);
    }

    /** Points given in stage pixels. */
    const at = pts => out(pts);
    /** The middle of an element, in stage pixels. */
    const centre = el => { const b = box(el); return [b.x + b.w / 2, b.y + b.h / 2]; };
    const moveTo = el => { const c = centre(el); return out([c, c]); };

    /** A loose S across a region, in stage pixels. */
    function swoosh(x0, y0, x1, y1, seed = 0) {
      return out(Array.from({ length: 14 }, (_, i) => {
        const k = i / 13;
        return [x0 + (x1 - x0) * k, y0 + (y1 - y0) * k + Math.sin(k * Math.PI * 2 + seed) * Math.abs(x1 - x0) * 0.12];
      }));
    }

    return { box, circle, circleAt, underline, across, down, arrow, arrowAt, rect, rectAt, triangle, tick, swoosh, at, centre, moveTo };
  }

  /** A step that only does something, then waits. */
  const act = (fn, wait = 0) => stage => { fn(stage); return { wait }; };
  /** A step that draws what `points` gives, measured when its turn comes. */
  const draw = (points, options = {}) => stage => ({ points: points(stage), ...options });
  /** A right-click: the pointer goes there, presses, and the ripple shows. */
  const click = (el, kitFor, then, options = {}) => stage => ({
    points: kitFor.moveTo(el), draw: false, wait: 700, ...options,
    after: s => {
      const [x, y] = kitFor.centre(el), now = performance.now();
      s.pose.press(true, now);
      setTimeout(() => { s.pose.press(false, performance.now()); s.wake(); }, 110);
      s.ripples.push({ x, y, at: now });
      s.wake();
      if (then) then(s);
    },
  });

  window.GhostKit = { kit, act, draw, click };
})();
