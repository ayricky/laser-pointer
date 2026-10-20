// Thirty tray-icon candidates on the 16 px grid. INK becomes the taskbar's text colour; in the "on"
// state DOT becomes the laser colour. Direct2D-safe SVG, as the app rasterises these at runtime.
// Each glyph is built from a function of `lit`, the colour of its laser mark.

const svg = body => `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16">${body}</svg>`;
const s = (d, w = 1.5, color = 'INK') =>
  `<path d="${d}" fill="none" stroke="${color}" stroke-width="${w}" stroke-linecap="round" stroke-linejoin="round"/>`;
const f = (d, fill = 'INK') => `<path d="${d}" fill="${fill}"/>`;
const dot = (x, y, r, fill) => `<circle cx="${x}" cy="${y}" r="${r}" fill="${fill}"/>`;

const glyphs = [
  ['beam-dot', 'Beam and dot', lit => s('M2 13.5L10 7.5', 1.75) + dot(11.5, 6, 3, lit)],
  ['slide-card', 'Slide card', lit => s('M2.5 3.5H13.5V12.5H2.5Z', 1.25) + s('M5 6.5H8.5', 1.25) + dot(10.5, 9.5, 2, lit)],
  ['ink-loop', 'Ink loop', lit => s('M11 3.6C8.7 2 3.4 2.5 2.6 6.4C1.8 10.4 6.4 12.6 9.6 11.6C12.4 10.7 13.2 8.2 12.4 6.5', 1.6) + dot(12.2, 5.4, 2.1, lit)],
  ['lasso', 'Lasso', lit => s('M2.5 13C5 13.6 7.4 11.6 7.6 9.4C7.9 6.6 4.6 6.4 4.4 8.6C4.2 11 9 11.4 12.2 4.6', 1.6) + dot(12.4, 4, 2, lit)],
  ['focus-rings', 'Focus rings', lit => `<circle cx="8" cy="8" r="6" fill="none" stroke="INK" stroke-width="1.25"/>` + dot(8, 8, 3, lit)],
  ['comet', 'Comet', lit => f('M1.6 14.2C4.8 12.6 7.4 9.6 8.6 7L10.6 8.6C8.6 10.8 5.2 13 1.6 14.2Z') + dot(11.2, 5.4, 3, lit)],
  ['ring-dot', 'Ring and dot', lit => `<circle cx="8" cy="8" r="6.25" fill="none" stroke="INK" stroke-width="1.5"/>` + dot(8, 8, 2.25, lit)],
  ['l-mark', 'L mark', lit => s('M4 2.5V11.5H9.5', 2) + dot(12, 11.5, 2.25, lit)],
  ['pointer', 'Pointer', lit => f('M3 1.5V12.5L5.8 10L7.6 14L9.4 13.2L7.6 9.4H11Z') + dot(13, 13, 2, lit)],
  ['clicker', 'Clicker', lit => `<rect x="5" y="5.5" width="6" height="9.5" rx="3" fill="none" stroke="INK" stroke-width="1.25"/>` + dot(8, 9, 1, 'INK') + s('M8 4V1.5', 1.25) + dot(8, 1.5, 1.5, lit)],
  ['spotlight', 'Spotlight', lit => f('M6 1.5H10L13 13H3Z', 'none') + s('M6.5 2L3.5 12.5M9.5 2L12.5 12.5', 1.25) + s('M3 13.5H13', 1.5) + dot(8, 11, 1.75, lit)],
  ['circled-chart', 'Circled chart', lit => f('M3 9.5H5V14H3ZM7 7.5H9V14H7ZM11 10.5H13V14H11Z') + s('M11.6 2.8C9.8 1.4 6.6 2 6.6 4C6.6 6 11.4 6.4 12 4.2', 1.25, lit)],
  ['underline', 'Underline', lit => s('M3 4H13M3 7.5H10', 1.5) + s('M2.5 12C6 10.8 9.5 11.6 13.5 10.6', 1.75, lit)],
  ['arrow-mark', 'Arrow mark', lit => s('M2.5 13.5C4.5 9 7.5 6.5 11 5', 1.6, lit) + s('M8.5 3.5L11.5 4.8L10.2 8', 1.6, lit) + dot(13, 3, 1.2, 'INK')],
  ['highlighter', 'Highlighter', lit => s('M3 4H13M3 12H10', 1.25) + f('M2 6.5L14 6V9.5L2 10Z', lit)],
  ['orb', 'Orb', lit => (lit === 'INK' ? `<circle cx="8" cy="8" r="5.5" fill="none" stroke="INK" stroke-width="1.5"/>` : dot(8, 8, 6.25, lit)) + dot(6.3, 6.3, 1.6, 'INK')],
  ['ripples', 'Ripples', lit => `<circle cx="8" cy="8" r="6.5" fill="none" stroke="INK" stroke-opacity=".55" stroke-width="1"/><circle cx="8" cy="8" r="4.25" fill="none" stroke="INK" stroke-width="1.25"/>` + dot(8, 8, 2, lit)],
  ['diagonal', 'Diagonal beam', lit => s('M1.5 14.5L11 5', 2) + dot(12, 4, 2.5, lit)],
  ['dotted-trail', 'Dotted trail', lit => dot(2.5, 13.5, 1, 'INK') + dot(5, 11, 1.25, 'INK') + dot(7.8, 8.6, 1.5, 'INK') + dot(11.5, 5, 2.75, lit)],
  ['cursor-spark', 'Cursor spark', lit => f('M2 1.5V12.5L4.8 10L6.6 14L8.4 13.2L6.6 9.4H10Z') + f('M12.5 1L13.3 3.2L15.5 4L13.3 4.8L12.5 7L11.7 4.8L9.5 4L11.7 3.2Z', lit)],
  ['nib', 'Nib', lit => f('M10.5 1.5L14.5 5.5L7 13L2 14L3 9Z', 'none') + s('M10.5 1.5L14.5 5.5L7 13L2 14L3 9Z', 1.25) + dot(4.4, 11.6, 1.5, lit)],
  ['dot-rays', 'Dot and rays', lit => s('M8 1.5V4M8 12V14.5M1.5 8H4M12 8H14.5', 1.5) + dot(8, 8, 2.75, lit)],
  ['tapered', 'Tapered stroke', lit => f('M1.8 14C5.4 13.2 6.6 9.6 8.4 7.6L10.4 9.2C8.4 11.6 6 13.6 1.8 14Z') + dot(11.6, 5.6, 2.75, lit)],
  ['screen', 'Screen', lit => s('M2 2.5H14V10.5H2Z', 1.25) + s('M8 10.5V13.5M5.5 14H10.5', 1.25) + dot(10.5, 6.5, 1.75, lit)],
  ['square-dot', 'Square and dot', lit => `<rect x="2" y="2" width="12" height="12" rx="3.5" fill="none" stroke="INK" stroke-width="1.5"/>` + dot(8, 8, 2.5, lit)],
  ['pixels', 'Pixels', lit => f('M1.5 12.5H3.5V14.5H1.5ZM4.5 10H6.5V12H4.5ZM7.5 7.5H9.5V9.5H7.5Z') + f('M10.5 2.5H14.5V6.5H10.5Z', lit)],
  ['brackets', 'Brackets', lit => s('M2.5 5.5V2.5H5.5M10.5 2.5H13.5V5.5M13.5 10.5V13.5H10.5M5.5 13.5H2.5V10.5', 1.5) + dot(8, 8, 2.25, lit)],
  ['drop', 'Drop', lit => f('M8 1.5L9.4 8H6.6Z') + dot(8, 10.5, 3.5, lit)],
  ['constellation', 'Constellation', lit => s('M3 12.5L7.5 4L13 11', 1.25) + dot(3, 12.5, 1.5, 'INK') + dot(13, 11, 1.5, 'INK') + dot(7.5, 4, 2.5, lit)],
  ['circle-tick', 'Circle and tick', lit => s('M13.2 5.4C12 2.6 7.6 1.6 4.8 3.6C2 5.8 2.4 10.6 5.6 12.6C8.6 14.4 12.8 12.8 13.6 9.6', 1.4, lit) + s('M5.5 8.2L7.4 10L10.8 6', 1.6)],
];

export const tray = glyphs.map(([slug, name, draw]) => ({
  slug,
  name,
  off: svg(draw('INK')),
  on: svg(draw('DOT')),
}));
