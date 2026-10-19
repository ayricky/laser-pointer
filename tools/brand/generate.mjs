// Regenerates every logo asset from logo.mjs:  npm install && node generate.mjs
//   assets/brand/      source SVGs, the tray template the app embeds, and app.ico for the exe
//   packaging/Assets/  MSIX / Microsoft Store logos at every scale and target size
//   docs/img/          website favicon, logo and social card
import { Resvg } from '@resvg/resvg-js';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { appIcon, trayIcon, wordmark } from './logo.mjs';

const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const out = (path, data) => {
  const full = join(root, path);
  mkdirSync(dirname(full), { recursive: true });
  writeFileSync(full, data);
};
const png = (svg, width) =>
  new Resvg(svg, { fitTo: { mode: 'width', value: width }, font: { loadSystemFonts: true } }).render().asPng();

/** Centres the mark on a transparent canvas of the given size, at `fill` of its shorter side. */
function padded(svg, width, height, fill) {
  const size = Math.min(width, height) * fill;
  const inner = svg.replace(/^<svg[^>]*>/, '').replace(/<\/svg>\s*$/, '');
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${width} ${height}">
  <g transform="translate(${(width - size) / 2} ${(height - size) / 2}) scale(${size / 256})">${inner}</g></svg>`;
}

/** An .ico holding PNG images, which Windows Vista onwards reads at every size. */
function ico(images) {
  const header = Buffer.alloc(6 + 16 * images.length);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(images.length, 4);
  let offset = header.length;
  images.forEach(({ size, data }, i) => {
    const entry = 6 + 16 * i;
    header.writeUInt8(size >= 256 ? 0 : size, entry);
    header.writeUInt8(size >= 256 ? 0 : size, entry + 1);
    header.writeUInt16LE(1, entry + 4);
    header.writeUInt16LE(32, entry + 6);
    header.writeUInt32LE(data.length, entry + 8);
    header.writeUInt32LE(offset, entry + 12);
    offset += data.length;
  });
  return Buffer.concat([header, ...images.map(image => image.data)]);
}

const icon = appIcon();
const unplated = appIcon({ plate: false });

// ---- Sources ------------------------------------------------------------------------------------
out('assets/brand/logo.svg', icon);
out('assets/brand/logo-unplated.svg', unplated);
out('assets/brand/wordmark.svg', wordmark());
// The app fills in INK (taskbar text colour) and DOT (laser colour) at runtime.
out('assets/brand/tray.svg', trayIcon({ ink: 'INK' }));
out('assets/brand/tray-on.svg', trayIcon({ ink: 'INK', dot: 'DOT' }));
out('assets/brand/app.ico', ico([16, 20, 24, 32, 40, 48, 64, 256].map(size => ({ size, data: png(icon, size) }))));

// ---- MSIX / Store -------------------------------------------------------------------------------
const scales = (name, base, make) => {
  for (const scale of [100, 125, 150, 200, 400]) {
    const [w, h] = base.map(v => Math.round((v * scale) / 100));
    out(`packaging/Assets/${name}.scale-${scale}.png`, png(make(w, h), w));
  }
};
scales('Square44x44Logo', [44, 44], () => icon);
scales('Square150x150Logo', [150, 150], (w, h) => padded(icon, w, h, 0.62));
scales('Wide310x150Logo', [310, 150], (w, h) => padded(icon, w, h, 0.62));
scales('SmallTile', [71, 71], (w, h) => padded(icon, w, h, 0.7));
scales('LargeTile', [310, 310], (w, h) => padded(icon, w, h, 0.6));
scales('StoreLogo', [50, 50], () => icon);
for (const size of [16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 256]) {
  const data = png(icon, size);
  out(`packaging/Assets/Square44x44Logo.targetsize-${size}.png`, data);
  out(`packaging/Assets/Square44x44Logo.targetsize-${size}_altform-unplated.png`, data);
  out(`packaging/Assets/Square44x44Logo.targetsize-${size}_altform-lightunplated.png`, data);
}

// ---- Website ------------------------------------------------------------------------------------
out('docs/img/logo.svg', icon);
out('docs/img/favicon.png', png(icon, 64));
out('docs/img/apple-touch-icon.png', png(icon, 180));
out('docs/img/social-card.png', png(wordmark({ width: 1200, height: 630 }), 1200));

console.log('Brand assets written.');
