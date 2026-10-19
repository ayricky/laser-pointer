// The Laser Pointer mark, built from the same idea as the app's strokes: a tapered trail drawn in
// layered passes (the Glow style) that ends in a white-hot dot.

const RED = '#ff2a2a';

function cubic(p0, p1, p2, p3, t) {
  const u = 1 - t;
  return [
    u * u * u * p0[0] + 3 * u * u * t * p1[0] + 3 * u * t * t * p2[0] + t * t * t * p3[0],
    u * u * u * p0[1] + 3 * u * u * t * p1[1] + 3 * u * t * t * p2[1] + t * t * t * p3[1],
  ];
}

/** Outline of a stroke along a cubic whose radius grows from nothing at the tail to `head`. */
export function taper(curve, head, { samples = 96, power = 1.35 } = {}) {
  const centre = [];
  for (let i = 0; i <= samples; i++) centre.push(cubic(...curve, i / samples));
  const left = [];
  const right = [];
  let tangent = [1, 0];
  centre.forEach((c, i) => {
    const a = centre[Math.max(0, i - 1)];
    const b = centre[Math.min(samples, i + 1)];
    const len = Math.hypot(b[0] - a[0], b[1] - a[1]) || 1;
    tangent = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
    const r = head * Math.pow(i / samples, power);
    left.push([c[0] - tangent[1] * r, c[1] + tangent[0] * r]);
    right.push([c[0] + tangent[1] * r, c[1] - tangent[0] * r]);
  });
  // Round the head off: from the left side, through the front, to the right side.
  const end = centre[samples];
  const [tx, ty] = tangent;
  const cap = [];
  for (let k = 1; k < 16; k++) {
    const angle = (Math.PI * k) / 16;
    const [cos, sin] = [Math.cos(angle), Math.sin(angle)];
    cap.push([end[0] + head * (-ty * cos + tx * sin), end[1] + head * (tx * cos + ty * sin)]);
  }
  const points = [...left, ...cap, ...right.reverse()];
  return 'M' + points.map(([x, y]) => `${x.toFixed(2)} ${y.toFixed(2)}`).join('L') + 'Z';
}

// Tail low on the left, a little dip, then up to the dot at the top right.
const CURVE = [[34, 208], [112, 240], [118, 92], [188, 68]];
const HEAD = [188, 68];

/** The full-colour app icon on its dark slate. `plate: false` drops the background. */
export function appIcon({ plate = true, color = RED } = {}) {
  const trail = (scale, opacity, extra = '') =>
    `<path d="${taper(CURVE, 13.5 * scale)}" fill="${color}" fill-opacity="${opacity}" ${extra}/>`;
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256">
  <defs>
    <linearGradient id="slate" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#2a2d38"/><stop offset="1" stop-color="#0c0d11"/>
    </linearGradient>
    <linearGradient id="sheen" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#fff" stop-opacity=".14"/><stop offset=".5" stop-color="#fff" stop-opacity="0"/>
    </linearGradient>
    <radialGradient id="glow" cx="${HEAD[0]}" cy="${HEAD[1]}" r="62" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="${color}" stop-opacity=".75"/>
      <stop offset=".35" stop-color="${color}" stop-opacity=".28"/>
      <stop offset="1" stop-color="${color}" stop-opacity="0"/>
    </radialGradient>
  </defs>
  ${plate ? `<rect x="12" y="12" width="232" height="232" rx="56" fill="url(#slate)"/>
  <rect x="12.75" y="12.75" width="230.5" height="230.5" rx="55.25" fill="none" stroke="url(#sheen)" stroke-width="1.5"/>` : ''}
  ${trail(3.4, 0.12)}
  ${trail(2.1, 0.22)}
  ${trail(1, 1)}
  <circle cx="${HEAD[0]}" cy="${HEAD[1]}" r="62" fill="url(#glow)"/>
  <circle cx="${HEAD[0]}" cy="${HEAD[1]}" r="22" fill="${color}"/>
  <circle cx="${HEAD[0]}" cy="${HEAD[1]}" r="9.5" fill="#fff"/>
</svg>`;
}

/**
 * The tray glyph: same trail and dot, flat, sized to read at 16 px. `ink` is the taskbar's text
 * colour; while the laser is on the dot takes the laser colour.
 */
export function trayIcon({ ink = '#ffffff', dot = null } = {}) {
  const curve = [[14, 232], [124, 250], [120, 112], [182, 74]];
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256">
  <path d="${taper(curve, 34, { power: 1.05 })}" fill="${ink}"/>
  ${dot ? `<circle cx="182" cy="74" r="72" fill="${dot}" fill-opacity=".28"/>` : ''}
  <circle cx="182" cy="74" r="${dot ? 54 : 48}" fill="${dot ?? ink}"/>
  ${dot ? `<circle cx="182" cy="74" r="20" fill="#fff"/>` : ''}
</svg>`;
}

/** Wide tile / social card: the mark beside the name. */
export function wordmark({ width = 1240, height = 600, dark = true } = {}) {
  const text = dark ? '#f4f5f8' : '#14161c';
  const bg = dark ? '#0e1014' : '#ffffff';
  const size = Math.min(height * 0.56, width * 0.28);
  const left = width * 0.08;
  const font = width * 0.07;
  const icon = appIcon().replace('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256">', '').replace('</svg>', '');
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${width} ${height}">
  <rect width="${width}" height="${height}" fill="${bg}"/>
  <g transform="translate(${left} ${(height - size) / 2}) scale(${size / 256})">${icon}</g>
  <text x="${left + size + width * 0.045}" y="${height / 2 - font * 0.05}" font-family="Segoe UI Variable Display, Segoe UI, sans-serif" font-weight="600" font-size="${font}" fill="${text}" letter-spacing="-1">Laser Pointer</text>
  <text x="${left + size + width * 0.048}" y="${height / 2 + font * 0.75}" font-family="Segoe UI Variable Text, Segoe UI, sans-serif" font-size="${font * 0.36}" fill="${dark ? '#98a1b0' : '#5f6672'}">Draw over anything while you present.</text>
</svg>`;
}
