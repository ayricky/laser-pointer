// Thirty cursor candidates in the app's format (see src/cursors.rs): viewBox 0 0 48 48, COLOR is
// the laser colour, Direct2D-safe SVG only (no filters, masks, text or CSS). Hotspots are the
// centre unless given.

const svg = body => `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 48 48">${body}</svg>`;
const C = 24;

/** A radial bloom in the laser colour. */
const bloom = (id, r, inner = 0.6, mid = 0.24, stop = 0.38) =>
  `<defs><radialGradient id="${id}"><stop offset="0" stop-color="COLOR" stop-opacity="${inner}"/>` +
  `<stop offset="${stop}" stop-color="COLOR" stop-opacity="${mid}"/><stop offset="1" stop-color="COLOR" stop-opacity="0"/>` +
  `</radialGradient></defs><circle cx="${C}" cy="${C}" r="${r}" fill="url(#${id})"/>`;

/** A dark translucent underlay so a coloured mark reads on white, then the mark itself. */
const edged = (draw, width, dark = 0.42) =>
  draw(`stroke="#000" stroke-opacity="${dark}" stroke-width="${width + 1.8}"`) + draw(`stroke="COLOR" stroke-width="${width}"`);

const disc = (r, fill, opacity = 1) =>
  `<circle cx="${C}" cy="${C}" r="${r}" fill="${fill}"${opacity < 1 ? ` fill-opacity="${opacity}"` : ''}/>`;

const core = (r, hot = 0.4) => disc(r + 0.9, '#000', 0.38) + disc(r, 'COLOR') + (hot ? disc(r * hot, '#fff', 0.92) : '');

const lines = (d, width = 1.6) =>
  `<g fill="none" stroke-linecap="round" stroke-linejoin="round">` +
  edged(attrs => `<path d="${d}" ${attrs}/>`, width) +
  `</g>`;

const ring = (r, width = 1.6, extra = '') =>
  `<g fill="none" ${extra}>` + edged(attrs => `<circle cx="${C}" cy="${C}" r="${r}" ${attrs}/>`, width) + `</g>`;

/** Four ticks around the centre, from radius `from` to `to`, rotated by `turn` degrees. */
const ticks = (from, to, turn = 0) => {
  const parts = [0, 90, 180, 270].map(a => {
    const t = ((a + turn) * Math.PI) / 180;
    const [c, s] = [Math.cos(t), Math.sin(t)];
    return `M${(C + c * from).toFixed(2)} ${(C + s * from).toFixed(2)}L${(C + c * to).toFixed(2)} ${(C + s * to).toFixed(2)}`;
  });
  return parts.join('');
};

const arrowPath = 'M7 5.5V33.5L13.8 27.1L18.2 37.2L23.1 35.1L18.8 25.3H28Z';

export const cursors = [
  // ---- Laser dots and blooms ----
  { slug: 'laser-dot', name: 'Laser dot', svg: svg(bloom('b', 17) + core(5.4, 0.43)) },
  { slug: 'tight-dot', name: 'Tight dot', svg: svg(bloom('b', 11, 0.7, 0.3, 0.45) + core(4.2, 0.4)) },
  { slug: 'hot-core', name: 'Hot core', svg: svg(bloom('b', 16, 0.5, 0.2) + disc(7, '#000', 0.3) + disc(6.2, 'COLOR') + disc(4.2, '#fff', 0.95)) },
  { slug: 'soft-glow', name: 'Soft glow', svg: svg(bloom('b', 20, 0.75, 0.32, 0.3) + disc(3, 'COLOR') + disc(1.3, '#fff', 0.9)) },
  { slug: 'double-bloom', name: 'Double bloom', svg: svg(bloom('b', 18, 0.45, 0.1, 0.3) + ring(11, 1, 'stroke-opacity=".7"') + core(4.6, 0.4)) },
  // ---- Rings and halos ----
  { slug: 'thin-ring', name: 'Thin ring', svg: svg(ring(10.5, 1.4) + core(2, 0)) },
  { slug: 'bold-ring', name: 'Bold ring', svg: svg(ring(9, 3.2)) },
  { slug: 'white-halo', name: 'White halo', svg: svg(disc(9, '#000', 0.22) + disc(7.6, '#fff') + disc(5.6, 'COLOR') + disc(1.9, '#fff', 0.9)) },
  {
    slug: 'segment-ring',
    name: 'Segment ring',
    svg: svg(lines('M24 13.5A10.5 10.5 0 0 1 32.5 17.8M34.5 24A10.5 10.5 0 0 1 30.2 32.5M24 34.5A10.5 10.5 0 0 1 15.5 30.2M13.5 24A10.5 10.5 0 0 1 17.8 15.5', 2) + core(1.8, 0)),
  },
  { slug: 'glow-ring', name: 'Glow ring', svg: svg(bloom('b', 18, 0, 0.35, 0.55) + ring(10, 2.2) + `<circle cx="24" cy="24" r="10" fill="none" stroke="#fff" stroke-opacity=".8" stroke-width=".8"/>`) },
  // ---- Game-style crosshairs ----
  { slug: 'classic-cross', name: 'Classic cross', svg: svg(lines('M24 19V11M24 29V37M19 24H11M29 24H37', 1.6)) },
  { slug: 'cross-dot', name: 'Cross and dot', svg: svg(lines('M24 18.5V12M24 29.5V36M18.5 24H12M29.5 24H36', 1.4) + core(1.6, 0)) },
  { slug: 'circle-dot', name: 'Circle and dot', svg: svg(ring(7, 1.4) + core(1.6, 0)) },
  { slug: 't-cross', name: 'T cross', svg: svg(lines('M24 29V37M19 24H11M29 24H37', 1.6) + core(1.4, 0)) },
  { slug: 'chevron', name: 'Chevron', svg: svg(lines('M17.5 30.5L24 23.5L30.5 30.5', 2)) },
  { slug: 'brackets', name: 'Brackets', svg: svg(lines('M16 20V16H20M28 16H32V20M32 28V32H28M20 32H16V28', 1.6) + core(1.5, 0)) },
  { slug: 'x-cross', name: 'X cross', svg: svg(lines(ticks(4.5, 12, 45), 1.6)) },
  { slug: 'micro-dot', name: 'Micro dot', svg: svg(disc(3.6, '#fff', 0.85) + disc(2.8, '#000', 0.5) + disc(2.1, 'COLOR')) },
  // ---- Pointers ----
  {
    slug: 'classic-arrow',
    name: 'Classic arrow',
    hotspot: [7, 5.5],
    svg: svg(`<path d="${arrowPath}" fill="#fff" stroke="#111" stroke-width="1.5" stroke-linejoin="round"/>` + `<circle cx="30.5" cy="33.5" r="4.6" fill="#000" fill-opacity=".4"/><circle cx="30.5" cy="33.5" r="3.7" fill="COLOR"/><circle cx="30.5" cy="33.5" r="1.4" fill="#fff" fill-opacity=".9"/>`),
  },
  {
    slug: 'colour-arrow',
    name: 'Colour arrow',
    hotspot: [7, 5.5],
    svg: svg(`<path d="${arrowPath}" fill="COLOR" stroke="#fff" stroke-width="2.4" stroke-linejoin="round"/><path d="${arrowPath}" fill="none" stroke="#000" stroke-opacity=".55" stroke-width=".8" stroke-linejoin="round"/>`),
  },
  {
    slug: 'slim-arrow',
    name: 'Slim arrow',
    hotspot: [8, 6],
    svg: svg(`<path d="M8 6L8.4 31L15 25.2L28.5 25.4Z" fill="#fff" stroke="#000" stroke-opacity=".5" stroke-width="3.6" stroke-linejoin="round"/><path d="M8 6L8.4 31L15 25.2L28.5 25.4Z" fill="#fff" stroke="COLOR" stroke-width="2" stroke-linejoin="round"/>`),
  },
  {
    slug: 'soft-pointer',
    name: 'Soft pointer',
    hotspot: [9, 8],
    svg: svg(`<path d="M9 8L33 18.5Q35.5 19.6 33.2 21L24.2 25.2L20.1 34.2Q18.8 36.6 17.6 34.1Z" fill="COLOR" stroke="#fff" stroke-width="2.2" stroke-linejoin="round"/><path d="M9 8L33 18.5Q35.5 19.6 33.2 21L24.2 25.2L20.1 34.2Q18.8 36.6 17.6 34.1Z" fill="none" stroke="#000" stroke-opacity=".5" stroke-width=".8" stroke-linejoin="round"/>`),
  },
  // ---- Pens ----
  {
    slug: 'stylus',
    name: 'Stylus',
    hotspot: [7, 7],
    svg: svg(`<g transform="translate(7 7) rotate(-45)"><path d="M0 0L-4 9.5V31.5Q0 35.5 4 31.5V9.5Z" fill="#fff" fill-opacity=".9" stroke="#fff" stroke-opacity=".9" stroke-width="2.4" stroke-linejoin="round"/><path d="M-3.8 12.4H3.8V31.3Q0 34.8 -3.8 31.3Z" fill="#171a20"/><path d="M0 .4L-3.8 9.6H3.8Z" fill="COLOR"/><path d="M-3.8 9.6H3.8V12.4H-3.8Z" fill="#3a404b"/><path d="M-1.6 15V29" stroke="#fff" stroke-opacity=".28" stroke-width="1" stroke-linecap="round"/><path d="M0 0L-4 9.5V31.5Q0 35.5 4 31.5V9.5Z" fill="none" stroke="#000" stroke-opacity=".55" stroke-width=".8" stroke-linejoin="round"/></g>`),
  },
  {
    slug: 'marker',
    name: 'Marker',
    hotspot: [8, 8],
    svg: svg(`<g transform="translate(8 8) rotate(-45)"><path d="M-3 0H3L5.5 8V32H-5.5V8Z" fill="#fff" stroke="#fff" stroke-width="2.2" stroke-linejoin="round"/><path d="M-3 .6H3L5 7.6H-5Z" fill="COLOR"/><path d="M-5 8.4H5V31.4H-5Z" fill="#20242c"/><path d="M-5 8.4H5V12H-5Z" fill="COLOR" fill-opacity=".85"/><path d="M-3 0H3L5.5 8V32H-5.5V8Z" fill="none" stroke="#000" stroke-opacity=".5" stroke-width=".8" stroke-linejoin="round"/></g>`),
  },
  {
    slug: 'nib',
    name: 'Fountain nib',
    hotspot: [7, 7],
    svg: svg(`<g transform="translate(7 7) rotate(-45)"><path d="M0 0L-6 13Q-6.5 17 -4.5 19H4.5Q6.5 17 6 13Z" fill="#fff" stroke="#fff" stroke-width="2.2" stroke-linejoin="round"/><path d="M0 .8L-5.2 13Q-5.6 16.4 -4 18.2H4Q5.6 16.4 5.2 13Z" fill="COLOR"/><path d="M0 2V12" stroke="#000" stroke-opacity=".6" stroke-width=".8"/><circle cx="0" cy="12.6" r="1.4" fill="#000" fill-opacity=".6"/><path d="M-4.5 19H4.5V30H-4.5Z" fill="#1d2027" stroke="#fff" stroke-opacity=".8" stroke-width="1"/></g>`),
  },
  // ---- Expressive ----
  {
    slug: 'comet',
    name: 'Comet',
    svg: svg(`<defs><linearGradient id="t" x1="0" y1="1" x2="1" y2="0"><stop offset="0" stop-color="COLOR" stop-opacity="0"/><stop offset="1" stop-color="COLOR" stop-opacity=".9"/></linearGradient></defs>` + bloom('b', 12, 0.55, 0.2) + `<path d="M8 40Q16 36 21.5 27.5L26 25.5L22.5 21.8Q14 30 8 40Z" fill="url(#t)"/>` + core(4.2, 0.42)),
  },
  { slug: 'spark', name: 'Spark', svg: svg(bloom('b', 15, 0.45, 0.15) + `<path d="M24 11L26.4 21.6L37 24L26.4 26.4L24 37L21.6 26.4L11 24L21.6 21.6Z" fill="COLOR" stroke="#000" stroke-opacity=".45" stroke-width=".9" stroke-linejoin="round"/>` + disc(1.8, '#fff', 0.95)) },
  { slug: 'spotlight', name: 'Spotlight', svg: svg(`<circle cx="24" cy="24" r="18" fill="COLOR" fill-opacity=".1"/>` + ring(18, 1.1) + core(2.2, 0)) },
  { slug: 'focus-corners', name: 'Focus corners', svg: svg(lines('M11 17V11H17M31 11H37V17M37 31V37H31M17 37H11V31', 1.8) + core(2.4, 0.4)) },
  { slug: 'magnifier', name: 'Magnifier', svg: svg(ring(9, 2) + lines('M30.6 30.6L38 38', 3) + `<circle cx="24" cy="24" r="7.8" fill="COLOR" fill-opacity=".12"/>` + core(1.4, 0)) },
];

export function cursorHotspot(cursor) {
  return cursor.hotspot ?? [24, 24];
}
