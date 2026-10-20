// Thirty app-icon candidates, 256 × 256, full colour. Rendered offline into the .ico and Store
// tiles, so any SVG resvg understands is fine here.

const RED = '#ff2a2a';
const svg = (body, defs = '') => `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256"><defs>${defs}</defs>${body}</svg>`;
const tile = (fill, extra = '') => `<rect x="16" y="16" width="224" height="224" rx="56" fill="${fill}" ${extra}/>`;
const grad = (id, a, b, vertical = true) =>
  `<linearGradient id="${id}" x1="0" y1="0" x2="${vertical ? 0 : 1}" y2="${vertical ? 1 : 1}"><stop offset="0" stop-color="${a}"/><stop offset="1" stop-color="${b}"/></linearGradient>`;
const glow = (id, color, opacity = 0.75) =>
  `<radialGradient id="${id}"><stop offset="0" stop-color="${color}" stop-opacity="${opacity}"/><stop offset=".4" stop-color="${color}" stop-opacity="${opacity * 0.35}"/><stop offset="1" stop-color="${color}" stop-opacity="0"/></radialGradient>`;
const dot = (x, y, r, color = RED, core = 0.42) =>
  `<circle cx="${x}" cy="${y}" r="${r * 2.6}" fill="url(#glow)"/><circle cx="${x}" cy="${y}" r="${r}" fill="${color}"/>` +
  (core ? `<circle cx="${x}" cy="${y}" r="${r * core}" fill="#fff"/>` : '');
const sheen = `<rect x="17" y="17" width="222" height="222" rx="55" fill="none" stroke="#fff" stroke-opacity=".12" stroke-width="2"/>`;
const slate = grad('slate', '#2b2f3a', '#0d0f14');

/** A stroke that tapers from nothing at its tail to `head` at its end, along a cubic. */
function taper(curve, head, power = 1.3, samples = 80) {
  const at = t => {
    const u = 1 - t;
    return [0, 1].map(k => u * u * u * curve[0][k] + 3 * u * u * t * curve[1][k] + 3 * u * t * t * curve[2][k] + t * t * t * curve[3][k]);
  };
  const centre = Array.from({ length: samples + 1 }, (_, i) => at(i / samples));
  const left = [], right = [];
  let tangent = [1, 0];
  centre.forEach((c, i) => {
    const a = centre[Math.max(0, i - 1)], b = centre[Math.min(samples, i + 1)];
    const len = Math.hypot(b[0] - a[0], b[1] - a[1]) || 1;
    tangent = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
    const r = head * Math.pow(i / samples, power);
    left.push([c[0] - tangent[1] * r, c[1] + tangent[0] * r]);
    right.push([c[0] + tangent[1] * r, c[1] - tangent[0] * r]);
  });
  const end = centre[samples], [tx, ty] = tangent, cap = [];
  for (let k = 1; k < 12; k++) {
    const a = (Math.PI * k) / 12;
    cap.push([end[0] + head * (-ty * Math.cos(a) + tx * Math.sin(a)), end[1] + head * (tx * Math.cos(a) + ty * Math.sin(a))]);
  }
  return 'M' + [...left, ...cap, ...right.reverse()].map(p => p.map(v => v.toFixed(1)).join(' ')).join('L') + 'Z';
}

const glowRed = glow('glow', RED);

export const logos = [
  {
    slug: 'ember-beam',
    name: 'Ember beam',
    svg: svg(tile('url(#slate)') + sheen + `<ellipse cx="128" cy="214" rx="96" ry="22" fill="url(#haze)"/>` +
      `<path d="M44 176L172 104L180 118Z" fill="${RED}" fill-opacity=".25"/><path d="M44 176L174 108L177 114Z" fill="${RED}"/>` + dot(178, 110, 15),
      slate + glowRed + `<radialGradient id="haze"><stop offset="0" stop-color="${RED}" stop-opacity=".22"/><stop offset="1" stop-color="${RED}" stop-opacity="0"/></radialGradient>`),
  },
  {
    slug: 'fluent-card',
    name: 'Fluent slide card',
    svg: svg(`<g transform="rotate(-8 128 128)"><rect x="40" y="58" width="176" height="150" rx="22" fill="#0b2a4a" opacity=".25" transform="translate(6 10)"/><rect x="40" y="52" width="176" height="150" rx="22" fill="url(#card)"/><rect x="40.5" y="52.5" width="175" height="149" rx="21.5" fill="none" stroke="#fff" stroke-opacity=".9"/><rect x="62" y="76" width="70" height="9" rx="4.5" fill="#9fb4cc"/><rect x="62" y="94" width="48" height="9" rx="4.5" fill="#c3d2e2"/></g>` +
      `<path d="M58 170C84 196 152 186 150 152C148 124 102 134 112 160C122 186 170 176 190 108" fill="none" stroke="${RED}" stroke-width="11" stroke-linecap="round"/>` + dot(190, 106, 10),
      grad('card', '#ffffff', '#cfe0f3') + glowRed),
  },
  {
    slug: 'margin-loop',
    name: 'Ink loop',
    svg: svg(tile('#f7f3ea', 'stroke="#e6dfd0" stroke-width="2"') +
      `<path d="M174 92C150 70 82 72 64 112C46 154 98 186 150 176C192 168 206 128 186 104" fill="none" stroke="#1d1b18" stroke-width="15" stroke-linecap="round"/>` +
      `<circle cx="186" cy="102" r="15" fill="${RED}"/>`),
  },
  {
    slug: 'neon-lasso',
    name: 'Neon lasso',
    svg: svg(tile('#08080b') + `<g filter="url(#blur)"><path d="M52 188C80 196 112 176 120 150C130 116 84 104 76 136C68 170 150 178 192 76" fill="none" stroke="url(#neon)" stroke-width="18" stroke-linecap="round" opacity=".7"/></g>` +
      `<path d="M52 188C80 196 112 176 120 150C130 116 84 104 76 136C68 170 150 178 192 76" fill="none" stroke="url(#neon)" stroke-width="10" stroke-linecap="round"/>` +
      `<path d="M52 188C80 196 112 176 120 150C130 116 84 104 76 136C68 170 150 178 192 76" fill="none" stroke="#fff" stroke-opacity=".7" stroke-width="3" stroke-linecap="round"/><circle cx="192" cy="76" r="12" fill="#fff"/>`,
      `<linearGradient id="neon" x1="0" y1="1" x2="1" y2="0"><stop offset="0" stop-color="#d62aff"/><stop offset="1" stop-color="${RED}"/></linearGradient><filter id="blur" x="-30%" y="-30%" width="160%" height="160%"><feGaussianBlur stdDeviation="9"/></filter>`),
  },
  {
    slug: 'focus-rings',
    name: 'Focus rings',
    svg: svg(tile('url(#slate)') + sheen + `<circle cx="128" cy="128" r="74" fill="none" stroke="#fff" stroke-opacity=".18" stroke-width="3"/><circle cx="128" cy="128" r="50" fill="none" stroke="#fff" stroke-opacity=".55" stroke-width="5"/>` + dot(128, 128, 22, RED, 0.38), slate + glowRed),
  },
  {
    slug: 'comet-trail',
    name: 'Comet trail',
    svg: svg(tile('url(#slate)') + sheen +
      `<path d="${taper([[40, 204], [112, 236], [118, 92], [184, 72]], 46, 1.35)}" fill="${RED}" fill-opacity=".1"/>` +
      `<path d="${taper([[40, 204], [112, 236], [118, 92], [184, 72]], 28, 1.35)}" fill="${RED}" fill-opacity=".22"/>` +
      `<path d="${taper([[40, 204], [112, 236], [118, 92], [184, 72]], 13, 1.35)}" fill="${RED}"/>` + dot(184, 72, 20, RED, 0.45), slate + glowRed),
  },
  {
    slug: 'just-the-dot',
    name: 'Just the dot',
    svg: svg(tile('#ffffff', 'stroke="#e7e7ea" stroke-width="2"') + `<circle cx="128" cy="128" r="70" fill="url(#glow)"/><circle cx="128" cy="128" r="34" fill="${RED}"/><circle cx="118" cy="118" r="11" fill="#fff" fill-opacity=".85"/>`, glowRed),
  },
  {
    slug: 'l-monogram',
    name: 'L monogram',
    svg: svg(tile('url(#slate)') + sheen + `<path d="M86 60V176H174" fill="none" stroke="#f3f4f7" stroke-width="26" stroke-linecap="round" stroke-linejoin="round"/>` + dot(176, 176, 17), slate + glowRed),
  },
  {
    slug: 'pointer-ray',
    name: 'Pointer and ray',
    svg: svg(tile('url(#blue)') + `<path d="M58 54V150L84 126L101 164L118 156L101 119H137Z" fill="#fff" stroke="#0b1d3d" stroke-width="5" stroke-linejoin="round"/><path d="M128 150L188 186" stroke="${RED}" stroke-width="7" stroke-linecap="round"/>` + dot(192, 188, 13),
      grad('blue', '#3a7bff', '#0b3aa8') + glowRed),
  },
  {
    slug: 'clicker',
    name: 'Presenter clicker',
    svg: svg(tile('url(#slate)') + sheen + `<g transform="rotate(-30 128 150)"><rect x="96" y="96" width="64" height="124" rx="32" fill="url(#remote)" stroke="#fff" stroke-opacity=".35" stroke-width="3"/><circle cx="128" cy="136" r="15" fill="#f3f4f7"/><path d="M128 128V144M120 136H136" stroke="#2b2f38" stroke-width="4" stroke-linecap="round"/><circle cx="128" cy="180" r="10" fill="#f3f4f7" fill-opacity=".55"/><rect x="118" y="88" width="20" height="12" rx="4" fill="${RED}"/></g>` + `<path d="M100 82L76 42" stroke="${RED}" stroke-width="6" stroke-linecap="round" opacity=".8"/>` + dot(72, 36, 13),
      slate + grad('remote', '#5b616e', '#1d2027') + glowRed),
  },
  {
    slug: 'spotlight',
    name: 'Spotlight',
    svg: svg(tile('#11131a') + `<path d="M128 28L66 196H190Z" fill="url(#cone)"/><ellipse cx="128" cy="196" rx="62" ry="16" fill="#ffd966" fill-opacity=".9"/><rect x="112" y="20" width="32" height="20" rx="6" fill="#3a3f4b"/>` + dot(128, 194, 9),
      `<linearGradient id="cone" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#ffd966" stop-opacity=".1"/><stop offset="1" stop-color="#ffd966" stop-opacity=".55"/></linearGradient>` + glowRed),
  },
  {
    slug: 'circled-chart',
    name: 'Circled chart',
    svg: svg(tile('#ffffff', 'stroke="#e5e7eb" stroke-width="2"') + `<rect x="62" y="132" width="26" height="62" rx="5" fill="#c7cbd3"/><rect x="100" y="110" width="26" height="84" rx="5" fill="#a9afba"/><rect x="138" y="80" width="26" height="114" rx="5" fill="#1f2937"/><rect x="176" y="120" width="26" height="74" rx="5" fill="#c7cbd3"/>` +
      `<path d="M182 60C166 38 118 40 112 66C106 94 168 106 190 78C202 62 186 44 160 44" fill="none" stroke="${RED}" stroke-width="9" stroke-linecap="round"/>`),
  },
  {
    slug: 'underline',
    name: 'Underline swoosh',
    svg: svg(tile('#ffffff', 'stroke="#e5e7eb" stroke-width="2"') + `<rect x="54" y="70" width="148" height="16" rx="8" fill="#1f2937"/><rect x="54" y="104" width="112" height="12" rx="6" fill="#9aa1ad"/><rect x="54" y="132" width="130" height="12" rx="6" fill="#c3c8d0"/>` +
      `<path d="${taper([[52, 184], [100, 172], [160, 176], [206, 166]], 9, 0.8)}" fill="${RED}"/>`),
  },
  {
    slug: 'arrow-note',
    name: 'Arrow note',
    svg: svg(tile('#ffffff', 'stroke="#e5e7eb" stroke-width="2"') + `<circle cx="172" cy="88" r="26" fill="#1f2937"/><path d="M58 196C78 146 112 120 140 108" fill="none" stroke="${RED}" stroke-width="11" stroke-linecap="round"/><path d="M118 98L144 106L132 130" fill="none" stroke="${RED}" stroke-width="11" stroke-linecap="round" stroke-linejoin="round"/>`),
  },
  {
    slug: 'highlighter',
    name: 'Highlighter swipe',
    svg: svg(tile('#ffffff', 'stroke="#e5e7eb" stroke-width="2"') + `<rect x="54" y="70" width="148" height="14" rx="7" fill="#c3c8d0"/><rect x="54" y="120" width="148" height="14" rx="7" fill="#1f2937"/><rect x="54" y="170" width="110" height="14" rx="7" fill="#c3c8d0"/><path d="M44 116L212 108L214 142L46 148Z" fill="#ffd400" fill-opacity=".55"/>`),
  },
  {
    slug: 'orb',
    name: 'Red orb',
    svg: svg(`<circle cx="128" cy="138" r="96" fill="url(#shadow)"/><circle cx="128" cy="124" r="92" fill="url(#orb)"/><ellipse cx="102" cy="86" rx="44" ry="28" fill="#fff" fill-opacity=".35" transform="rotate(-25 102 86)"/>`,
      `<radialGradient id="orb" cx=".38" cy=".32" r=".8"><stop offset="0" stop-color="#ff8a7a"/><stop offset=".45" stop-color="${RED}"/><stop offset="1" stop-color="#8f0018"/></radialGradient><radialGradient id="shadow"><stop offset=".7" stop-color="#000" stop-opacity=".25"/><stop offset="1" stop-color="#000" stop-opacity="0"/></radialGradient>`),
  },
  {
    slug: 'ripples',
    name: 'Ripples',
    svg: svg(tile('url(#navy)') + `<circle cx="128" cy="128" r="84" fill="none" stroke="${RED}" stroke-opacity=".25" stroke-width="5"/><circle cx="128" cy="128" r="58" fill="none" stroke="${RED}" stroke-opacity=".5" stroke-width="6"/>` + dot(128, 128, 22),
      grad('navy', '#1b2a4b', '#0a1020') + glowRed),
  },
  {
    slug: 'diagonal-beam',
    name: 'Diagonal beam',
    svg: svg(tile('#0d0f14') + `<path d="M16 240L196 60" stroke="${RED}" stroke-opacity=".2" stroke-width="30" stroke-linecap="round"/><path d="M16 240L196 60" stroke="${RED}" stroke-width="10" stroke-linecap="round"/><path d="M16 240L196 60" stroke="#fff" stroke-opacity=".7" stroke-width="3" stroke-linecap="round"/>` + dot(196, 60, 20), glowRed),
  },
  {
    slug: 'dotted-trail',
    name: 'Dotted trail',
    svg: svg(tile('url(#slate)') + sheen + [[56, 196, 5], [76, 178, 7], [98, 162, 9], [122, 146, 11], [146, 126, 13]].map(([x, y, r], i) => `<circle cx="${x}" cy="${y}" r="${r}" fill="${RED}" fill-opacity="${0.25 + i * 0.15}"/>`).join('') + dot(176, 96, 22), slate + glowRed),
  },
  {
    slug: 'cursor-spark',
    name: 'Cursor spark',
    svg: svg(tile('url(#slate)') + sheen + `<path d="M72 52V178L104 148L124 196L148 186L128 140H176Z" fill="#fff" stroke="#11131a" stroke-width="5" stroke-linejoin="round"/><path d="M186 58L192 82L216 88L192 94L186 118L180 94L156 88L180 82Z" fill="${RED}"/>`, slate),
  },
  {
    slug: 'stylus',
    name: 'Stylus',
    svg: svg(tile('url(#paper)') + `<path d="M50 190C90 150 130 196 170 150" fill="none" stroke="${RED}" stroke-width="10" stroke-linecap="round"/><g transform="rotate(40 170 150)"><path d="M170 150L160 128H180Z" fill="#2b2f38"/><rect x="160" y="40" width="20" height="88" rx="4" fill="url(#barrel)"/><rect x="160" y="112" width="20" height="16" fill="${RED}"/></g>`,
      grad('paper', '#fbfcfe', '#e4ebf5') + grad('barrel', '#5a606c', '#23262d', false)),
  },
  {
    slug: 'red-target',
    name: 'Red target',
    svg: svg(tile('url(#redtile)') + `<circle cx="128" cy="128" r="62" fill="none" stroke="#fff" stroke-width="12"/><circle cx="128" cy="128" r="24" fill="#fff"/>`, grad('redtile', '#ff5a4f', '#c8001f')),
  },
  {
    slug: 'white-on-red',
    name: 'White on red',
    svg: svg(tile('url(#redtile)') + `<path d="${taper([[52, 200], [116, 226], [112, 100], [178, 78]], 13, 1.3)}" fill="#fff"/><circle cx="178" cy="78" r="22" fill="#fff"/>`, grad('redtile', '#ff5a4f', '#c8001f')),
  },
  {
    slug: 'projector-screen',
    name: 'Projector screen',
    svg: svg(tile('url(#slate)') + sheen + `<rect x="48" y="56" width="160" height="108" rx="8" fill="#f3f4f7"/><rect x="44" y="50" width="168" height="10" rx="5" fill="#9aa1ad"/><path d="M128 164V204M104 204H152" stroke="#9aa1ad" stroke-width="8" stroke-linecap="round"/><rect x="66" y="78" width="70" height="10" rx="5" fill="#c3c8d0"/>` + dot(162, 120, 12), slate + glowRed),
  },
  {
    slug: 'glass-blobs',
    name: 'Glass',
    svg: svg(`<g filter="url(#soft)"><circle cx="92" cy="96" r="58" fill="#ff2a2a"/><circle cx="168" cy="110" r="52" fill="#d62aff"/><circle cx="120" cy="176" r="56" fill="#1e90ff"/></g>` + tile('#fff', 'fill-opacity=".18" stroke="#fff" stroke-opacity=".6" stroke-width="2"') + `<circle cx="128" cy="128" r="22" fill="#fff"/>`,
      `<filter id="soft" x="-30%" y="-30%" width="160%" height="160%"><feGaussianBlur stdDeviation="22"/></filter>`),
  },
  {
    slug: 'pixel',
    name: 'Pixel trail',
    svg: svg(tile('#0d0f14') + [[56, 184, 0.3], [80, 168, 0.45], [104, 152, 0.6], [128, 128, 0.75], [152, 104, 0.9]].map(([x, y, o]) => `<rect x="${x}" y="${y}" width="16" height="16" fill="${RED}" fill-opacity="${o}"/>`).join('') + `<rect x="168" y="64" width="32" height="32" fill="${RED}"/><rect x="176" y="72" width="16" height="16" fill="#fff"/>`),
  },
  {
    slug: 'aim-brackets',
    name: 'Brackets',
    svg: svg(tile('url(#slate)') + sheen + `<path d="M64 96V64H96M160 64H192V96M192 160V192H160M96 192H64V160" fill="none" stroke="#f3f4f7" stroke-width="14" stroke-linecap="round" stroke-linejoin="round"/>` + dot(128, 128, 20), slate + glowRed),
  },
  {
    slug: 'ink-drop',
    name: 'Ink drop',
    svg: svg(tile('url(#slate)') + sheen + `<path d="${taper([[128, 44], [128, 90], [128, 120], [128, 150]], 28, 1.6)}" fill="${RED}" fill-opacity=".9"/>` + dot(128, 160, 30, RED, 0.36), slate + glowRed),
  },
  {
    slug: 'constellation',
    name: 'Constellation',
    svg: svg(tile('#0b0f1c') + `<path d="M70 170L124 86L188 150L70 170" fill="none" stroke="${RED}" stroke-opacity=".75" stroke-width="5" stroke-linejoin="round"/>` + dot(70, 170, 11, RED, 0) + dot(188, 150, 11, RED, 0) + dot(124, 86, 16), glowRed),
  },
  {
    slug: 'circle-check',
    name: 'Circle and tick',
    svg: svg(tile('#ffffff', 'stroke="#e5e7eb" stroke-width="2"') + `<path d="M188 92C176 58 120 46 86 70C52 96 58 160 98 182C140 204 190 180 198 138" fill="none" stroke="${RED}" stroke-width="12" stroke-linecap="round"/><path d="M92 130L118 156L170 98" fill="none" stroke="#1f2937" stroke-width="14" stroke-linecap="round" stroke-linejoin="round"/>`),
  },
];
