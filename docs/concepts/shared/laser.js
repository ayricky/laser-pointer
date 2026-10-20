/* Laser — classic script; no dependencies, requests or build step.
 * const demo = Laser.stage(positionedElement, {
 *   look: {color:'#ff2a2a', width:4, opacity:100, fade:1000, trail:500,
 *          smoothing:2, style:'glow', ink:false}, // partial, over Laser preset
 *   on:true, cursor:'dot', keyboard:false, ripples:true,
 *   ghost:[], resume:0, onChange(state) {}
 * });
 * Methods: setOn(bool), toggle(), setLook(partial), usePreset(0|'Laser'),
 * setCursor('dot'|'ring'|'reticle'|'pen'|{size:48,draw(ctx,x,y,s)}),
 * undo(), redo(), clear(), play(script), stop(), destroy(). Mutators chain.
 * state: {on,look,preset,strokes,ghosting}; defensive snapshots; preset is a
 * zero-based index, or -1 for a custom look. strokes counts visible strokes.
 * Laser.presets = [{name,look},...]; Laser.styles = ['solid','glow','neon'];
 * Laser.cursors exposes the four renderers. Cursor s = {color,pressed,on,t,
 * ignite,press,speed,dpr}; size is its nominal CSS-pixel footprint. draw() is
 * isolated with save/restore. It runs on demand, not continuously when idle.
 * Ghost steps (coordinates are fractions of stage width/height):
 * {wait:600}, {move:[x,y]}, {circle:[cx,cy,rx,ry]}, {underline:[x1,x2,y]},
 * {arrow:[x1,y1,x2,y2]}, {path:[[x,y],...]}, {tick:[x,y,size]},
 * {look:{...}}, {preset:'Highlighter'}, {on:true}, {keep:true}, {undo:true},
 * {clear:true}, {do:stage=>{}}, {loop:true}. Gestures include travel and pauses.
 * loop waits for fading strokes, clears kept strokes, then restarts. Real
 * pointer movement/press or a stage key stops playback; resume (ms, default 0)
 * restarts the last script after inactivity, clearing first. stop() cancels
 * resume. Off-screen playback pauses. Reduced motion renders one static,
 * kept annotation pass, ignoring waits/loops and hiding the ghost cursor.
 * Keyboard applies only over/focused within this stage; editable fields are
 * excluded. Off canvases pass input through. clear() fades over 250 ms and
 * is undoable. Resizing preserves existing strokes in CSS-pixel coordinates.
 */
(function () {
  'use strict';
  const LOOK = { color: '#ff2a2a', width: 4, opacity: 100, fade: 1000, trail: 500, smoothing: 2, style: 'glow', ink: false };
  const DEFAULT_PRESETS = [
    { name: 'Laser', look: { ...LOOK } },
    { name: 'Highlighter', look: { color: '#ffd400', width: 18, opacity: 40, fade: 3000, trail: 2000, smoothing: 2, style: 'solid', ink: false } },
    { name: 'Pen', look: { color: '#1e90ff', width: 4, opacity: 100, fade: 1000, trail: 500, smoothing: 2, style: 'solid', ink: true } },
  ];
  const STYLES = ['solid', 'glow', 'neon'];
  const LAYERS = { solid: [[1, 1, 0]], glow: [[3.4, .10, 0], [2.1, .20, 0], [1, 1, 0]], neon: [[3.8, .12, 0], [2.3, .26, 0], [1, 1, 0], [.45, 1, .85]] };
  // Stroke maths copied from docs/index.html; see src/stroke.rs.
  const MIN_STEP = 2, CURVE_STEP = 3, CAP = 8;
  const easeOut = k => 1 - Math.pow(1 - k, 4);
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
  function rgb(hex) { const v = parseInt(hex.slice(1), 16); return [v >> 16 & 255, v >> 8 & 255, v & 255]; }
  function drawStroke(ctx, pts, t, s) {
    const base = s.width / 2, n = pts.length, radii = new Array(n).fill(base);
    if (!s.ink) {
      let behind = 0;
      for (let i = n - 1; i >= 0; i--) {
        if (i + 1 < n) behind += dist(pts[i], pts[i + 1]);
        const age = 1 - (t - pts[i].t) / s.fade, len = 1 - behind / s.trail;
        radii[i] = base * Math.min(easeOut(Math.max(0, len)), easeOut(Math.max(0, age)));
      }
      let faded = 0;
      while (faded < n && radii[faded] < .1) faded++;
      const drop = faded === n ? n : Math.max(0, faded - 1);
      pts.splice(0, drop); radii.splice(0, drop);
    }
    if (!pts.length) return;
    const [centre, widths] = curveThrough(smooth(pts, s.smoothing), radii, CURVE_STEP);
    const [r, g, b] = rgb(s.color);
    for (const [w, a, white] of LAYERS[s.style]) {
      const poly = outline(centre, widths.map(x => x * w)), m = v => Math.round(v + (255 - v) * white);
      ctx.fillStyle = `rgba(${m(r)},${m(g)},${m(b)},${a * s.opacity / 100})`;
      ctx.beginPath(); ctx.moveTo(poly[0].x, poly[0].y);
      for (let i = 1; i < poly.length; i++) ctx.lineTo(poly[i].x, poly[i].y);
      ctx.closePath(); ctx.fill('nonzero');
    }
  }

  const clamp = (v, lo = 0, hi = 1) => Math.max(lo, Math.min(hi, v));
  const now = () => performance.now();
  const ease = u => u * u * (3 - 2 * u);
  const back = u => 1 + 2.70158 * (u - 1) ** 3 + 1.70158 * (u - 1) ** 2;
  function disc(c, x, y, r, fill) {
    c.fillStyle = fill; c.beginPath(); c.arc(x, y, r, 0, Math.PI * 2); c.fill();
  }
  function cursorSpace(c, x, y, s) { c.translate(x, y); c.scale(Math.max(0, s.ignite), Math.max(0, s.ignite)); }
  const cursors = {
    dot: { size: 48, draw(c, x, y, s) {
      cursorSpace(c, x, y, s);
      const r = 16 - s.press * 5, g = c.createRadialGradient(0, 0, 1, 0, 0, r);
      g.addColorStop(0, s.color + 'b0'); g.addColorStop(.3, s.color + '55'); g.addColorStop(1, s.color + '00');
      disc(c, 0, 0, r, g); disc(c, 0, 0, 5.65, '#17202b99');
      disc(c, 0, 0, 5, s.color); disc(c, -.45, -.65, 1.9 + .5 * s.press, '#fff9ed');
    } },
    ring: { size: 48, draw(c, x, y, s) {
      cursorSpace(c, x, y, s); const r = 11 - 4 * s.press;
      c.beginPath(); c.arc(0, 0, r, 0, Math.PI * 2);
      c.strokeStyle = '#18212b99'; c.lineWidth = 3.5; c.stroke();
      c.strokeStyle = '#ffffffdc'; c.lineWidth = 2.75; c.stroke();
      c.strokeStyle = s.color; c.lineWidth = 1.75; c.stroke();
      disc(c, 0, 0, 3.1, '#17202b99'); disc(c, 0, 0, 2.5, s.color); disc(c, -.3, -.3, .8, '#fff');
    } },
    reticle: { size: 48, draw(c, x, y, s) {
      cursorSpace(c, x, y, s); const gap = 7 - 3 * s.press;
      c.lineCap = 'round'; c.beginPath();
      for (let i = 0; i < 4; i++) {
        const a = i * Math.PI / 2, dx = Math.cos(a), dy = Math.sin(a);
        c.moveTo(dx * gap, dy * gap); c.lineTo(dx * (gap + 5), dy * (gap + 5));
      }
      c.strokeStyle = '#18212b99'; c.lineWidth = 4; c.stroke();
      c.strokeStyle = '#ffffffe8'; c.lineWidth = 3; c.stroke();
      c.strokeStyle = s.color; c.lineWidth = 1.5; c.stroke();
      disc(c, 0, 0, 2.7, '#18212b99'); disc(c, 0, 0, 2, s.color); disc(c, 0, 0, .7, '#fff');
    } },
    pen: { size: 48, draw(c, x, y, s) {
      cursorSpace(c, x, y, s); c.rotate(-Math.PI / 4); c.scale(1 - .06 * s.press, 1 - .06 * s.press);
      // The tip is the hotspot; the barrel extends down/right from it.
      c.beginPath(); c.moveTo(0, 0); c.lineTo(-3.8, 9); c.lineTo(-3.8, 28);
      c.quadraticCurveTo(0, 32, 3.8, 28); c.lineTo(3.8, 9); c.closePath();
      c.fillStyle = '#161e29'; c.strokeStyle = '#ffffffdc'; c.lineWidth = 1.2; c.lineJoin = 'round'; c.fill(); c.stroke();
      c.beginPath(); c.moveTo(0, .7); c.lineTo(-3.2, 9); c.lineTo(3.2, 9); c.closePath();
      c.fillStyle = s.color; c.fill(); c.fillRect(-3.2, 10.5, 6.4, 2.4);
      c.strokeStyle = '#ffffff66'; c.lineWidth = 1; c.beginPath(); c.moveTo(-1.9, 15); c.lineTo(-1.9, 27); c.stroke();
      disc(c, 0, 1.5, .8, '#fff');
    } },
  };

  const stages = new Set(), reduced = matchMedia('(prefers-reduced-motion: reduce)');
  let frame = 0, hovered = null;
  function request() { if (!frame && [...stages].some(s => s.visible)) frame = requestAnimationFrame(tick); }
  function tick(t) {
    frame = 0;
    let active = false;
    for (const s of stages) if (s.visible && (s.dirty || s.animating)) {
      s.dirty = false; s.animating = s.render(t); active ||= s.animating;
    }
    if (active) request();
  }
  function cleanLook(partial, base) {
    const s = { ...base };
    for (const k of Object.keys(LOOK)) if (partial[k] !== undefined) s[k] = partial[k];
    if (!/^#[\da-f]{6}$/i.test(s.color)) throw new TypeError('Laser colour must be #rrggbb');
    if (!STYLES.includes(s.style)) throw new TypeError('Unknown Laser style');
    for (const [k, lo, hi] of [['width', .1, 200], ['opacity', 0, 100], ['fade', 1, 120000], ['trail', 1, 100000], ['smoothing', 0, 20]]) {
      if (!Number.isFinite(s[k])) throw new TypeError('Invalid Laser ' + k);
      s[k] = clamp(s[k], lo, hi);
    }
    s.smoothing = Math.round(s.smoothing); s.ink = !!s.ink; return s;
  }
  function resolveCursor(value) {
    const c = typeof value === 'string' ? cursors[value] : value;
    if (!c || typeof c.draw !== 'function') throw new TypeError('Unknown Laser cursor');
    return c;
  }
  function tween(a, target, duration, t, overshoot = false) {
    const value = a.value(t);
    return { end: t + duration, value: time => {
      const u = clamp((time - t) / duration);
      return value + (target - value) * (overshoot ? back(u) : ease(u));
    } };
  }
  const fixed = v => ({ end: 0, value: () => v });

  function stage(element, options = {}) {
    let look = cleanLook(options.look || {}, LOOK), on = options.on !== false;
    let cursor = resolveCursor(options.cursor || 'dot'), destroyed = false;
    const canvas = document.createElement('canvas'), ctx = canvas.getContext('2d');
    canvas.setAttribute('aria-hidden', 'true'); canvas.dataset.laser = '';
    Object.assign(canvas.style, { position: 'absolute', inset: '0', width: '100%', height: '100%', zIndex: '10' });
    element.appendChild(canvas);
    const oldCursor = element.style.cursor, oldTab = element.getAttribute('tabindex');
    if (options.keyboard && oldTab === null) element.tabIndex = 0;
    let W = 0, H = 0, dpr = 1, strokes = [], clearing = [], undoStack = [], redoStack = [];
    let current = null, pointerId = null, point = null, inside = false, pressed = false, speed = 0, lastPointTime = 0;
    let ignite = tween(fixed(0), on ? 1 : 0, 260, now(), true), press = fixed(0);
    let ripples = [], ghost = null, lastScript = options.ghost || null, resumeTimer = 0, resumeEnabled = !!options.ghost, notice = '';
    const listeners = [], rec = { visible: false, dirty: true, animating: false, render };
    const listen = (target, type, fn, opts) => { target.addEventListener(type, fn, opts); listeners.push(() => target.removeEventListener(type, fn, opts)); };
    function wake() { if (!destroyed) { rec.dirty = true; request(); } }
    function notify() {
      const s = api.state, key = JSON.stringify(s);
      if (key !== notice) { notice = key; if (options.onChange) options.onChange(s); }
    }
    function inputStyle() {
      canvas.style.pointerEvents = on ? 'auto' : 'none'; canvas.style.touchAction = on ? 'none' : 'auto';
      canvas.style.cursor = on ? 'none' : 'auto'; element.style.cursor = on ? 'none' : oldCursor;
    }
    function resize() {
      W = element.clientWidth; H = element.clientHeight; dpr = window.devicePixelRatio || 1;
      canvas.width = Math.round(W * dpr); canvas.height = Math.round(H * dpr); wake();
    }
    function release() {
      if (pointerId !== null && canvas.hasPointerCapture(pointerId)) canvas.releasePointerCapture(pointerId);
      pointerId = null;
    }
    function setPressed(value, t = now()) {
      if (pressed === value) return;
      pressed = value; press = tween(press, value ? 1 : 0, value ? 120 : 180, t); wake();
    }
    function finish(t = now()) { current = null; setPressed(false, t); }
    function begin(p, t, kept = false) {
      if (!on) return;
      current = { points: [], look: { ...look, ink: kept || look.ink }, bounds: null };
      strokes.push(current); undoStack.push({ add: current }); undoStack = undoStack.slice(-100); redoStack = [];
      append(p, t, true); setPressed(true, t); notify();
    }
    function append(p, t, force = false) {
      if (!current) return;
      const pts = current.points, last = pts[pts.length - 1];
      if (!force && last && dist(last, p) < MIN_STEP) return;
      pts.push({ x: p.x, y: p.y, t });
      const b = current.bounds;
      current.bounds = b ? { l: Math.min(b.l, p.x), r: Math.max(b.r, p.x), t: Math.min(b.t, p.y), b: Math.max(b.b, p.y) } : { l: p.x, r: p.x, t: p.y, b: p.y };
      wake();
    }
    function position(p, t) {
      const dt = t - lastPointTime;
      speed = point && dt > 0 ? speed * .65 + Math.min(5, dist(point, p) / dt) * .35 : 0;
      point = p; lastPointTime = t; wake();
    }
    function local(e, rect) {
      return { x: (e.clientX - rect.left) * W / rect.width, y: (e.clientY - rect.top) * H / rect.height };
    }
    function cancelGhost(manual = false) {
      if (ghost) { ghost = null; finish(); notify(); }
      clearTimeout(resumeTimer); resumeTimer = 0;
      if (!manual && resumeEnabled && options.resume > 0 && lastScript) resumeTimer = setTimeout(() => {
        if (!destroyed && pointerId === null) { api.clear(); api.play(lastScript); }
      }, options.resume);
    }
    function activity() { cancelGhost(); }
    const api = {
      setOn(value) {
        value = !!value;
        if (on !== value) {
          on = value; ignite = tween(ignite, on ? 1 : 0, 260, now(), on);
          if (!on) { finish(); release(); } inputStyle(); notify(); wake();
        }
        return api;
      },
      toggle() { return api.setOn(!on); },
      setLook(partial) { look = cleanLook(partial, look); notify(); wake(); return api; },
      usePreset(value) {
        const p = typeof value === 'number' ? DEFAULT_PRESETS[value] : DEFAULT_PRESETS.find(p => p.name.toLowerCase() === String(value).toLowerCase());
        if (!p) throw new RangeError('Unknown Laser preset');
        look = { ...p.look }; notify(); wake(); return api;
      },
      setCursor(value) { cursor = resolveCursor(value); wake(); return api; },
      undo() {
        finish(); const op = undoStack.pop();
        if (op) {
          if (op.add) strokes = strokes.filter(s => s !== op.add);
          else { clearing = []; restore(op.clear); }
          redoStack.push(op); notify(); wake();
        }
        return api;
      },
      redo() {
        finish(); const op = redoStack.pop();
        if (op) {
          if (op.add) restore([op.add]); else fadeClear();
          undoStack.push(op); notify(); wake();
        }
        return api;
      },
      clear() {
        finish();
        if (strokes.length) { undoStack.push({ clear: strokes.slice() }); redoStack = []; fadeClear(); notify(); wake(); }
        return api;
      },
      play(script) {
        if (!Array.isArray(script)) throw new TypeError('Laser ghost must be an array');
        cancelGhost(true); lastScript = script; resumeEnabled = true; finish(); release();
        if (reduced.matches) staticScript(script);
        else { ghost = { script, index: 0, queue: [], action: null, time: 0, last: null }; }
        notify(); wake(); return api;
      },
      stop() { resumeEnabled = false; cancelGhost(true); wake(); return api; },
      get state() { return { on, look: { ...look }, preset: DEFAULT_PRESETS.findIndex(p => Object.keys(LOOK).every(k => p.look[k] === look[k])), strokes: strokes.length, ghosting: !!ghost }; },
      destroy() {
        if (destroyed) return;
        cancelGhost(true); release(); destroyed = true; clearTimeout(resumeTimer);
        ro.disconnect(); io.disconnect(); listeners.forEach(off => off()); stages.delete(rec);
        if (hovered === rec) hovered = null;
        element.style.cursor = oldCursor;
        if (options.keyboard && oldTab === null) element.removeAttribute('tabindex');
        canvas.remove(); if (!stages.size && frame) { cancelAnimationFrame(frame); frame = 0; }
      },
    };
    function restore(items) {
      const t = now();
      for (const s of items) {
        const last = s.points[s.points.length - 1];
        if (last && !s.look.ink) { const shift = t - last.t; s.points.forEach(p => p.t += shift); }
        if (last && !strokes.includes(s)) strokes.push(s);
      }
    }
    function fadeClear() {
      const t = now();
      clearing.push(...strokes.map(s => ({ ...s, points: s.points.map(p => ({ ...p })), at: t })));
      strokes = [];
    }
    function inBounds(s) {
      const b = s.bounds, r = s.look.width * LAYERS[s.look.style][0][0] / 2 + 2;
      return b && b.r + r >= 0 && b.b + r >= 0 && b.l - r <= W && b.t - r <= H;
    }

    // A gesture is travel followed by one or more independent strokes.
    const px = p => ({ x: p[0] * W, y: p[1] * H });
    const fraction = p => [p.x / (W || 1), p.y / (H || 1)];
    function paths(step) {
      if (step.circle) {
        const [x, y, rx, ry] = step.circle;
        return [Array.from({ length: 101 }, (_, i) => {
          const a = -Math.PI / 6 + i / 100 * Math.PI * 2.12;
          return [x + rx * Math.cos(a) * (1 + .012 * Math.sin(a * 3)), y + ry * Math.sin(a)];
        })];
      }
      if (step.underline) {
        const [x1, x2, y] = step.underline;
        return [Array.from({ length: 41 }, (_, i) => [x1 + (x2 - x1) * i / 40, y + .002 * Math.sin(i / 40 * Math.PI * 3)])];
      }
      if (step.arrow) {
        const [x1, y1, x2, y2] = step.arrow, a = px([x1, y1]), b = px([x2, y2]);
        const theta = Math.atan2(b.y - a.y, b.x - a.x), len = Math.min(25, dist(a, b) * .22);
        const wing = sign => fraction({ x: b.x - len * Math.cos(theta + sign * .55), y: b.y - len * Math.sin(theta + sign * .55) });
        return [[[x1, y1], [x2, y2]], [wing(1), [x2, y2]], [wing(-1), [x2, y2]]];
      }
      if (step.tick) {
        const [x, y, size] = step.tick;
        return [[[x - size * .4, y], [x - size * .1, y + size * .32 * W / (H || 1)], [x + size * .55, y - size * .45 * W / (H || 1)]]];
      }
      if (step.path && step.path.length) {
        const p = step.path;
        if (p.length < 3) return [p];
        const result = [];
        for (let i = 0; i < p.length - 1; i++) for (let j = 0; j < 16; j++) {
          const u = j / 16, a = p[Math.max(0, i - 1)], b = p[i], c = p[i + 1], d = p[Math.min(p.length - 1, i + 2)];
          result.push([0, 1].map(k => .5 * (2 * b[k] + (-a[k] + c[k]) * u + (2 * a[k] - 5 * b[k] + 4 * c[k] - d[k]) * u * u + (-a[k] + 3 * b[k] - 3 * c[k] + d[k]) * u ** 3)));
        }
        result.push(p[p.length - 1]); return [result];
      }
      return [];
    }
    function action(points, draw) {
      const lengths = [0];
      for (let i = 1; i < points.length; i++) lengths.push(lengths[i - 1] + dist(px(points[i - 1]), px(points[i])));
      const length = lengths[lengths.length - 1];
      return { points, draw, lengths, length, duration: Math.max(draw ? 100 : 140, length / (draw ? .85 : .75)), sampled: -4 };
    }
    function at(a, u) {
      const target = ease(clamp(u)) * a.length;
      let i = 1; while (i < a.lengths.length - 1 && a.lengths[i] < target) i++;
      if (a.points.length === 1) return px(a.points[0]);
      const v = (target - a.lengths[i - 1]) / (a.lengths[i] - a.lengths[i - 1] || 1);
      const p = a.points[i - 1], q = a.points[i];
      const out = px([p[0] + (q[0] - p[0]) * v, p[1] + (q[1] - p[1]) * v]);
      const envelope = Math.sin(Math.PI * clamp(u));
      out.x += envelope * .28 * Math.sin(u * 39); out.y += envelope * (a.draw ? .35 * Math.sin(u * 31) : 5 * Math.sin(u * Math.PI));
      return out;
    }
    function command(step) {
      if (step.look) api.setLook(step.look);
      if (step.preset !== undefined) api.usePreset(step.preset);
      if (step.on !== undefined) api.setOn(step.on);
      if (step.keep !== undefined) api.setLook({ ink: step.keep });
      if (step.undo) api.undo(); if (step.clear) api.clear(); if (step.do) step.do(api);
    }
    function staticScript(script) {
      const t = now();
      for (const step of script) {
        if (step.loop) break;
        command(step);
        for (const points of paths(step)) {
          const a = action(points, true); begin(at(a, 0), t, true);
          for (let k = 4; k < a.duration; k += 4) append(at(a, k / a.duration), t);
          append(at(a, 1), t, true); finish(t);
        }
      }
      point = null; pressed = false; press = fixed(0);
    }
    function advance(t) {
      const g = ghost;
      if (!g) return;
      if (g.last !== null) g.time += t - g.last;
      g.last = t;
      for (let budget = 0; budget < 100 && ghost === g; budget++) {
        if (g.action) {
          const a = g.action, elapsed = Math.min(a.duration, g.time - a.start);
          if (a.points) {
            if (a.sampled < 0 && a.draw) begin(at(a, 0), t - elapsed);
            for (let k = a.sampled + 4; k <= elapsed; k += 4) {
              const p = at(a, k / a.duration), stamp = t - (g.time - a.start - k);
              if (a.draw) append(p, stamp);
              position(p, stamp); a.sampled = k;
            }
            if (elapsed === a.duration) { const p = at(a, 1); if (a.draw) append(p, t, true); position(p, t); }
          }
          if (elapsed < a.duration) break;
          if (a.draw) finish(t);
          g.action = null;
        }
        if (g.queue.length) { g.action = g.queue.shift(); g.action.start = g.time; continue; }
        const step = g.script[g.index++];
        if (!step) { ghost = null; finish(t); point = null; notify(); break; }
        if (step.loop) {
          const delay = Math.max(100, ...strokes.filter(s => !s.look.ink).map(s => s.look.fade - (t - s.points[s.points.length - 1].t)));
          if (strokes.some(s => s.look.ink)) api.clear();
          g.index = 0; g.action = { start: g.time, duration: Math.max(delay, clearing.length ? 260 : 0) }; continue;
        }
        command(step); if (ghost !== g) break;
        if (step.wait !== undefined) g.queue.push({ duration: Math.max(0, step.wait) });
        if (step.move) g.queue.push(action([point ? fraction(point) : [.15, .2], step.move], false));
        for (const p of paths(step)) {
          let from;
          for (let i = g.queue.length - 1; i >= 0; i--) if (g.queue[i].points) { from = g.queue[i].points; break; }
          g.queue.push(action([from ? from[from.length - 1] : point ? fraction(point) : [.15, .2], p[0]], false));
          g.queue.push(action(p, true), { duration: 110 });
        }
      }
    }

    function render(t) {
      advance(t);
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0); ctx.globalCompositeOperation = 'source-over'; ctx.globalAlpha = 1; ctx.clearRect(0, 0, W, H);
      const before = strokes.length;
      strokes = strokes.filter(s => {
        if (!s.look.ink && s.points.length && t - s.points[s.points.length - 1].t >= s.look.fade) s.points.length = 0;
        if (s.points.length && inBounds(s)) drawStroke(ctx, s.points, t, s.look);
        return s.points.length || s === current;
      });
      clearing = clearing.filter(s => {
        if (t - s.at >= 250) return false;
        if (inBounds(s)) { ctx.globalAlpha = 1 - ease((t - s.at) / 250); drawStroke(ctx, s.points, s.at, s.look); }
        return true;
      });
      ctx.globalAlpha = 1;
      ripples = ripples.filter(r => {
        const u = (t - r.t) / 450; if (u >= 1) return false;
        ctx.beginPath(); ctx.arc(r.x, r.y, 5 + 28 * easeOut(u), 0, Math.PI * 2);
        ctx.strokeStyle = r.color; ctx.globalAlpha = (1 - u) * .75; ctx.lineWidth = 2 * (1 - u) + .5; ctx.stroke(); return true;
      });
      ctx.globalAlpha = 1;
      const cursorVisible = point && (inside || ghost) && (on || t < ignite.end);
      if (cursorVisible) {
        ctx.save();
        try { cursor.draw(ctx, point.x, point.y, { color: look.color, pressed, on, t, ignite: ignite.value(t), press: press.value(t), speed, dpr }); }
        finally { ctx.restore(); }
      }
      if (before !== strokes.length) notify();
      return !!ghost || !!clearing.length || !!ripples.length || strokes.some(s => !s.look.ink && s.points.length) || !!(cursorVisible && (t < ignite.end || t < press.end));
    }

    listen(element, 'pointerenter', () => { inside = true; hovered = rec; wake(); });
    listen(element, 'pointerleave', () => { inside = false; if (hovered === rec) hovered = null; wake(); });
    listen(element, 'pointermove', e => {
      activity();
      if (pointerId !== null && e.pointerId !== pointerId) return;
      const rect = canvas.getBoundingClientRect(), events = e.getCoalescedEvents ? e.getCoalescedEvents() : [];
      const t = now();
      for (const ev of events.length ? events : [e]) {
        const p = local(ev, rect), stamp = t - clamp(e.timeStamp - ev.timeStamp, 0, 100);
        position(p, stamp); if (pointerId === e.pointerId) append(p, stamp);
      }
    }, { capture: true });
    listen(element, 'pointerdown', e => {
      activity(); position(local(e, canvas.getBoundingClientRect()), now());
      if (!on || e.button !== 0 || e.isPrimary === false || pointerId !== null) return;
      pointerId = e.pointerId; canvas.setPointerCapture(e.pointerId);
      if (options.keyboard) element.focus({ preventScroll: true });
      begin(point, now());
    }, { capture: true });
    listen(canvas, 'pointerup', e => {
      if (e.pointerId !== pointerId) return;
      append(local(e, canvas.getBoundingClientRect()), now()); finish(); release(); activity(); wake();
    });
    const cancel = e => { if (e.pointerId === pointerId) { finish(); release(); activity(); wake(); } };
    listen(canvas, 'pointercancel', cancel); listen(canvas, 'lostpointercapture', cancel);
    listen(element, 'contextmenu', e => {
      if (options.ripples === false) return;
      e.preventDefault(); activity(); const p = local(e, canvas.getBoundingClientRect());
      ripples.push({ ...p, t: now(), color: look.color }); wake();
    });
    listen(element, 'keydown', activity, { capture: true });
    rec.key = e => {
      activity();
      if (!options.keyboard || e.defaultPrevented || e.metaKey || e.shiftKey || e.target.closest?.('input,textarea,select,[contenteditable]:not([contenteditable="false"])')) return;
      const k = e.key.toLowerCase(); let handle;
      if (e.ctrlKey && !e.altKey) handle = k === 'z' ? api.undo : k === 'y' ? api.redo : null;
      else if (!e.ctrlKey) {
        if (k === 'l') handle = api.toggle;
        else if (k === 'escape') handle = () => api.setOn(false);
        else if (k === 'c' || (k === 'x' && e.altKey)) handle = api.clear;
        else if (k === 'k') handle = () => api.setLook({ ink: !look.ink });
        else if (k === ']' || k === '[') handle = () => api.usePreset((api.state.preset + (k === ']' ? 1 : 2) + 3) % 3);
        else if (!e.altKey && /^[123]$/.test(k)) handle = () => api.usePreset(Number(k) - 1);
      }
      if (handle) { e.preventDefault(); handle(); }
    };
    rec.element = element;
    const ro = new ResizeObserver(resize);
    const io = new IntersectionObserver(entries => {
      rec.visible = entries[0].isIntersecting;
      if (ghost) ghost.last = null;
      if (rec.visible) wake();
    });
    listen(window, 'resize', resize);
    let dpiQuery;
    const dpiChanged = () => { if (dpiQuery) dpiQuery.removeEventListener('change', dpiChanged); resize(); dpiQuery = matchMedia(`(resolution: ${dpr}dppx)`); dpiQuery.addEventListener('change', dpiChanged); };
    dpiChanged(); listeners.push(() => dpiQuery.removeEventListener('change', dpiChanged));
    listen(reduced, 'change', () => { if (reduced.matches && ghost) { const script = ghost.script; api.clear(); api.play(script); } });
    inputStyle(); stages.add(rec); ro.observe(element); io.observe(element); notify();
    if (options.ghost) api.play(options.ghost);
    return api;
  }
  document.addEventListener('keydown', e => {
    const focused = [...stages].find(s => s.element.contains(document.activeElement));
    const target = hovered || focused; if (target) target.key(e);
  });
  window.Laser = Object.freeze({ stage, presets: Object.freeze(DEFAULT_PRESETS.map(p => Object.freeze({ name: p.name, look: Object.freeze({ ...p.look }) }))), styles: Object.freeze(STYLES.slice()), cursors: Object.freeze(cursors) });
})();
